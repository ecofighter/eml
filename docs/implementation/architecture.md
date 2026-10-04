# コンパイラの構成

位置づけ: 手引き。

eml の処理系をどの crate に分け、各段階がどんな規律に従うかを説明する。言語の意味は [spec/](../spec/) の各文書が定める。この文書は、それを実装する側の構造を扱う。

## 全体の方針

- 処理系はバッチ型のパイプラインである。各段階を純粋な関数とし、Arena と ID で表現して、後でクエリ化 (salsa など) できるようにしておく
- 実行系は、型付き Core IR (ANF 形式で、RC とエフェクトを明示する) と CEK 風のインタプリタからなる。将来の LLVM バックエンドも同じ IR から変換する。ヒープと RC は `eml_runtime` に分離する
- 構文木には rowan (Red-Green Tree) を使い、その上にリッチな診断と LSP を整える

## リポジトリ

```
eml/
  Cargo.toml            # [workspace] members = ["crates/*"], resolver = "3"
  crates/
    eml_diagnostics/
    eml_syntax/
    eml_hir/
    eml_types/
    eml_core_ir/
    eml_runtime/
    eml_interp/
    eml_cli/            # lib + バイナリ (バイナリ名 `eml`)
    eml_test_support/   # 開発専用。結合テストのパイプラインと診断の文字列
  tests/ui/             # UI テストのコーパス
    run/
    run-fail/
    check-fail/
  docs/
  flake.nix
```

- `[workspace.package]` で `version` と `edition = "2024"` を共有する。外部 crate と workspace 内の crate のバージョンは `[workspace.dependencies]` でまとめて管理する
- `flake.nix` の `buildRustPackage` は、workspace のルートの `Cargo.lock` を使い、`cargoBuildFlags = ["-p" "eml_cli"]` で `eml` バイナリを作る。devShell には `rust-analyzer`、`clippy`、`rustfmt`、`cargo-insta` が入っている
- `.direnv/` と `.envrc` は Git で追跡しない

## crate の依存関係

依存は上から下への一方向だけにする。

```
eml_cli          check / run コマンド。各段階をつなぐだけ。テストから呼べる lib API を公開する
eml_interp       Core IR を CEK 機械で実行する
eml_runtime      オブジェクトのモデル、ヒープ、参照カウント、debug_heap の検査、OutputSink
eml_core_ir      型付き HIR → Core IR。dup/decref の挿入パス
eml_types        Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査
eml_hir          CST → HIR の変換、名前解決、脱糖
eml_syntax       SyntaxKind、lexer、レイアウト段、イベント方式のパーサ、rowan、型付き AST ラッパ
eml_diagnostics  Diagnostic 型、FileId と SourceFiles、行と列、ariadne による表示
```

- 段階の並びは `eml_cli` → `eml_interp` → `eml_core_ir` → `eml_types` → `eml_hir` → `eml_syntax` → `eml_diagnostics` で、`eml_interp` はさらに `eml_runtime` に依存する
- `eml_diagnostics` は、診断を出す crate のすべてから使う。現時点で `eml_runtime` と `eml_interp` は診断を出さないので、`eml_diagnostics` に依存していない
- `eml_test_support` は開発専用の crate で、パイプラインに入らない。各 crate の結合テストが dev-dependency として使う。段階を feature (`hir` < `types` < `core` < `run`) で選び、各 crate は自分の段階までを有効にする。破壊的な変更の途中で下流の crate がまだ組み立たなくても、変更している段階のテストを流せるようにするため ([テスト戦略](testing.md))

## 各段階の規律

- 各段階は `fn stage(input: &In) -> (Out, Vec<Diagnostic>)` の形の純粋な関数にする。グローバルな可変状態は持たない。例外は `eml_core_ir` で、診断のエラーがないプログラムだけを受け取り、`Program` を返す (下の「エラーが出ても止まらない」)
- HIR 以降は `ExprId` / `PatId` / `DefId` などの ID で参照する (`la-arena`)。型などの解析結果は `ExprId → Type` のような別テーブルに置く
- HIR の各ノードは、元の構文の範囲 (`TextRange`) を持つ。演算子の列を組み直した部分式のように、対応する構文ノードのない式があるため
- 型付き HIR は HIR を複製しない。`TypedModule` は、関数ごとの型スキームと推論結果 (式や局所変数の型、呼び出しごとの具体化) の別テーブルだけを持つ。そのため `eml_core_ir` は HIR と `TypedModule` の両方を受け取る

現在の各段階の入口は次のとおり (実装状況は [status.md](status.md))。

| crate | 関数 |
|---|---|
| `eml_syntax` | `parse(FileId, &str) -> (Parse, Vec<Diagnostic>)` |
| `eml_hir` | `lower(FileId, &ast::SourceFile) -> (Module, Vec<Diagnostic>)` |
| `eml_types` | `check(&Module) -> (TypedModule, Vec<Diagnostic>)` |
| `eml_core_ir` | `lower(&Module, &TypedModule) -> Program` |
| `eml_interp` | `run(Arc<Program>, &RunConfig, &OutputSink) -> Result<(), RuntimeError>` |

## エラーが出ても止まらない

- `eml_cli` の `check` / `compile` は、エラーがあっても途中で止めずにすべての段階を実行し、診断を集める。1回の実行で、独立した複数のエラーを報告するため
- パーサは、壊れた入力に対して `ERROR` ノードを作って処理を続ける。壊れた入力でも必ず `SOURCE_FILE` の木を作り、パニックしない
- 回復の同期点は、レイアウト段が挿入する `SEP` と `CLOSE` である。トップレベルでは、列 0 の `SEP` が最も強い同期点になる ([レイアウト規則](../spec/layout.md))
- 名前解決と型推論は、エラーが起きた場所に `Error` 型を入れる。`Error` が関わる制約や線形性の検査からは、追加の診断を出さない
- Core IR は、診断のエラーがないプログラムだけを受け取る。`compile` は、エラーがあれば Core IR を作らない。型付きで正しい入力を前提にできるので、Core IR への変換は診断を返さない

## 構文を差し替えられるようにする

- パーサは rust-analyzer と同じイベント方式にする。ノード開始、トークン、ノード終了のイベントを出し、別の処理 (`sink.rs`) で rowan の木を組み立てる
- 構文の規則は `eml_syntax/src/grammar/` に閉じ込める。構文の変更は、原則として `grammar/`、`SyntaxKind`、lexer、型付き AST ラッパ、レイアウト段の変更だけで済むようにする
- 暫定構文から本番の構文への切り替えは、この方針のとおり `eml_syntax` の中だけで済んだ (構文の段階 S1)

## `eml_syntax` の内部構成

```
lexer/         字句解析。トークン列をつなげると元のテキストに戻る (lossless)。文字列の字句は lexer/string.rs
literal.rs     リテラルの値の解釈。lexer の検査と AST の値の取り出しが同じエスケープの表を使う
layout.rs      レイアウト段。trivia を除いたトークン列に仮想トークンを挿入する
parser.rs      イベント方式のパーサの仕組み。文法の規則は持たない
grammar/       文法の規則 (items / types / patterns / expressions)。括弧とブロックの深さは grammar/scan.rs の Nesting で数える
sink.rs        イベント列から rowan の木を組み立てる
ast.rs         型付き AST ラッパ。範囲 (range)、キーワードの範囲 (keyword_range)、リテラルの値 (Literal::value) を持つ
syntax_kind.rs SyntaxKind。括弧の種類の判定 (is_opening_bracket / is_closing_bracket) もここに置く
token_set.rs   トークンの集合 (u128 のビット集合)
debug_dump.rs  木のダンプ (debug_tree)。構文のテストとデバッグに使う
```

- lexer は logos で単純なトークンを切り出し、文字列、コメント、演算子の分類などを手書きの層で扱う
  - 最終的には、文字列の中、補間 `\{` の中 (括弧の深さを数える)、コマンドリテラルの中、入れ子のブロックコメントの各モードをスタックで持つ層にし、文字列を `STRING_START` / `STRING_TEXT` / `ESCAPE` / `INTERP_START` / `INTERP_END` / `STRING_END` のトークン列に分ける。コマンドリテラルも同様にする
  - S1 の時点では、補間を含む文字列、複数行の文字列、raw 文字列、コマンドリテラルを読み飛ばして E0004 (未対応) を出す。トークン列への分割は、これらを実装する S2、S3 で行う
- レイアウト段は、幅 0 の仮想トークン `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE` を挿入する。規則は [レイアウト規則](../spec/layout.md) が定める。パーサは仮想トークンを読んでも `Event::Token` を出さないので、仮想トークンは木に入らず、CST は lossless のまま
- `grammar/` は [文法](../spec/grammar.md) に従う。演算子の列は `OP_SEQ` ノードに平たく並べ、木への組み直しは HIR で行う
- トークンの種類 (`EOF` まで) は 128 未満に収める。`TokenSet` が `u128` のビット集合であるため
- 型付き AST ラッパのアクセサは、HIR への変換で必要になったものから足していく
- 字句・構文の診断の番号 (E0xxx) は `eml_syntax::codes` に置く ([診断](../spec/diagnostics.md))
- `parse` は、lexer、レイアウト段、パーサの診断を集め、位置の順に並べて返す
- `eml_hir` は型付き AST の API (`range`、`keyword_range`、`Literal::value`、各アクセサ) と、識別子や演算子の `SyntaxToken` だけを使う。CST の木の構造 (`.syntax()`) には触れず、`rowan` に依存しない
- E0004 (未対応) の番号とラベルは、どの段階でも同じ意味なので `eml_diagnostics` に置く (`Diagnostic::not_yet_supported`)

## `eml_hir` で行う脱糖と検査

HIR への変換では、名前解決に加えて、次の脱糖と検査を行う。それぞれの規則は [宣言](../spec/declarations.md)、[式](../spec/expressions.md)、[直積型とレコード](../spec/records.md)、[モジュールと名前解決](../spec/modules.md) が定める。

- 名前の重複定義と未定義の名前の検査
- シグネチャと等式の対応の検査、複数の等式の `match` への脱糖
- 演算子の列の組み直し (fixity の表を引く)、単項マイナス、セクション
- `use`、パラメータ付き handler、`if` の `else` の補完、`let ... in`
- タプルのレコードへの変換、補間の `++` の連結への変換、コマンドリテラルの `Cmd` の構築
- handler の節の引数の個数、`resume` の引数の個数

## `eml_hir` の内部

- `Module` はトップレベルの関数の `Arena<Function>` を持つ。本体は関数ごとの `Body` (`exprs`、`pats`、`locals` のアリーナ) に置く。後でクエリ化したときに関数単位で再計算できるようにするため (rust-analyzer と同じ分け方)。型の注釈は、シグネチャのものを `Signature::types` に、本体のものを `Body::types` に置く。本体を書き換えてもシグネチャが変わらないようにするため
- 型変数と row 変数の表は `Signature::generics` (`Generics`) に置く。シグネチャで定義し、本体の注釈は引くだけにする。ラムダの引数は、ラムダの本体だけで見えるスコープに入る
- シグネチャと等式は名前で対応づけてから、並び方を検査する。シグネチャか等式のない関数も `Function` として残し (`signature` か `body` が `None`)、呼び出し側で名前の誤りを連鎖させない
- 名前は `Res::{Local, Function, Builtin}` に解決する。組み込みは名前解決の最も外側のスコープで、ユーザーの定義で隠せる。組み込みの名前、見え方 (名前で引く値、演算子、名前で引けない内部用)、引数の数は `eml_hir::builtin::BUILTINS` の表に、シグネチャは eml のソースで書いた Prelude (`crates/eml_hir/src/prelude.em`) に置く。変換のはじめに Prelude を構文解析し、`Module::builtins` に置く。Prelude の範囲には `FileId::PRELUDE` を使い、診断には出さない。S2 で `Prelude` モジュールに移す
- 型とエフェクトは item (`Module::types`、`Module::effects`) で、ID (`TypeDefId`、`EffectId`) で参照する。今は組み込みの `Int`、`String`、`Bool`、`Unit` と `IO` だけを、変換のはじめに登録する。処理系が役割で引く item は `Module::lang` (`LangItems`) にある
- トップレベルの名前は、変換の中の `ItemScope` (`lower/scope.rs`) で解決する。値と型 (型名とエフェクト名) の2つの名前空間を持ち、ユーザーの定義を先に引き、なければ組み込みを引く
- `Body` は走査関数を持つ。`walk_child_exprs` は式の直接の子を辿り、`pat_bindings` はパターンが束縛する変数を、`lambda_captures` はラムダが捕まえる変数を返す。段階3と4で式やパターンの種類を足すときは、これらを直す
- 演算子の列は、標準の演算子の表で precedence climbing により組み直す。`&&` / `||` は `if` に脱糖する。`x |> f` は、`x` を先に評価する印 (`ExprKind::Call::evaluate_first`) を付けた関数適用 `f x` に、`f <| x` は関数適用に脱糖する。型検査は印を見ずに普通の呼び出しとして検査し、Core IR への変換が印の付いた引数を先に評価する。`let` に脱糖すると左辺が推論になり、引数の型を期待した診断が失われるため`else` のない `if` は `else_branch: None` のまま残し、型検査が `Unit` を求める

## `eml_types` の内部

- 型は検査器の中の表に置いて ID で引き、型変数の束縛を辿って単一化する。row は「ラベルの並び + 末尾の row 変数」で、scoped labels の書き換えで単一化する。Kind の変数と `≤` の制約は束の上で最小解を求める
- 各関数の本体は、シグネチャだけを見て検査する。引数を1つ消費するごとにシグネチャの矢印を1つたどり、本体の row は最後にたどった矢印の row になる。呼び出しでは、呼び出し先の閉じた row を新しい row 変数で開いてから本体の row と単一化する
- 推論の対象は [型と Kind](../spec/types.md) の Kind・型・row である。推論の後に、型付き HIR の上で別のパスとして、線形性の検査 ([線形性](../spec/linearity.md)) と網羅性の検査 ([網羅性](../spec/exhaustiveness.md)) を行う
- 型の表現はすでに閉じたレコードを使い、`Unit` は `Record([])` である。段階4のタプルは、この表現を数字ラベルの閉じたレコードとして再利用する ([直積型とレコード](../spec/records.md))
- 結果の `TypedModule` が持つ型は、型変数の束縛を解決した `Type` である (別テーブルの形は上の「各段階の規律」)
- シグネチャはスキーム (`scheme.rs`) で持つ。型変数と row 変数は rigid で、参照するたびに具体化し、戻り値の側の閉じた row を開く
- 呼び出しは、たどった矢印の row を今の row に含める (`Table::include_row`)。呼び出し先の末尾が推論変数なら、その row を今の row とそのまま単一化する。閉じた末尾と rigid な末尾では、末尾を新しい row 変数に替えた row を今の row と単一化し、今の row の残りをその変数で受ける。rigid な末尾では、さらに残りの末尾が同じ rigid 変数であることを確かめる。今の row をまだ推論している途中で残りの末尾が推論変数なら、その推論変数を rigid な変数に束縛する (推論されるラムダの中の呼び出しに必要)
- ラムダの引数の個数が合わないときや期待する型が壊れているときは、末尾が `Error` の row で本体を検査し、エフェクトの誤りを連鎖させない
- 呼び出しグラフの SCC (`scc.rs`) を呼ばれる側から検査し、SCC ごとに使用回数のパス (`usage.rs`) で `Unr` の制約を出してから、スキームの Kind 変数を多相化する。残す制約は、内部の変数を経由した推移も含めて求める (`Lattice::residual`)
- 部分適用のクロージャの線形性は、それまでの引数と捕まえた値の Kind 以上になる (`Table::closure_kinds`)
- `TypedModule::signatures` は、関数の型と、スキームに残った Kind の制約のうち定数を片側に持つもの (`Scheme::constraints`) を持つ。`dump` はこれを `kinds:` の行に出す
- 型の表は `table/` に分ける。`mod.rs` は型と変数の格納、`unify.rs` は型の単一化、`row.rs` は row の単一化と `include_row`、`kinds.rs` は Kind の制約、`copy.rs` はスキームの具体化の写し、`export.rs` は外に出す型への変換である。型の形は `TyShape`、関数の矢印の線形性は `ArrowLin` と呼び、Kind (線形性と多重度) と取り違えないようにする
- row の末尾は `Tail::{Closed, Var, Error}` である。未定義のエフェクトか解決できない row 変数の跡は末尾 `Error` の row になり、型の `Error` と同じく束縛されない。末尾 `Error` は相手の側にしかないエフェクトを受け入れるが、自分の側の既知のエフェクトは受け入れない。綴り誤りの E1002 と無関係なエフェクトの誤りを隠さないためである。外に出す型では `{error}` と表示する
- `Table::display` は診断の文言のための変換で、Kind の束を解かず、矢印の線形性を `Unr` にする。`Table::export` は `solve_kinds` の後にだけ呼び、`TypedModule` を組み立てる
- 検査器は `check/` に分ける。`mod.rs` は SCC の順の検査と `TypedModule` の組み立て、`body.rs` は本体の検査、`report.rs` は診断を作る処理である。`if` とブロックは期待する型の有無 (`Expectation`) で check と infer の処理を共有し、矢印をたどる処理は `next_arrow` に、今の row の保存と復元は `with_ambient` にまとめてある。呼び出しの row を今の row に含める処理は `include_call_row` と呼ぶ
- E2002 の副ラベルは、本体の row が入る矢印の部分の型を指す (`body_arrow_range`)
- 組み込みの型は、Prelude のシグネチャから、ユーザーの関数と同じ経路 (`Rigids`、`lower_signature`、`closure_kinds`、`Scheme`) で作る。本体がないので、作ってすぐ多相化する。`True` と `False` は、段階4で `data Bool` にするまで lang item の `Bool` の型である
- 型構成子は `TyShape::Con(TypeDefId)`、row のラベルは `EffectId` である。外に出す型は `Type::Con { id, name }` と `EffectLabel { id, name }` で、`Module` を渡さずに表示できるよう名前を持つ。型変数は、シグネチャの変数 (`Type::Rigid`) と推論で解けなかった変数 (`Type::Flexible`) を区別する

## `eml_core_ir`、`eml_runtime`、`eml_interp` の内部

- Core IR とインタプリタの設計は [Core IR とインタプリタ](../spec/core-ir.md)、ヒープと RC は [ランタイム](../spec/runtime.md) が定める
- インタプリタの値とフレームに `Rc` と `RefCell` を使わない。Core IR は `Arc<Program>` で読み取り専用で共有する。将来、複数のスレッドがそれぞれの CEK 機械で同じプログラムを実行するため ([マルチコア対応の設計](../future/multicore.md))
- Core IR の関数は、ANF の木をアリーナに置き、`CExprId` で参照する。継続のフレームが再開する位置を ID で持てるようにするため。変換は式の値の渡し先 (`Exit::Return` か `Exit::Jump`) を持って回り、末尾の `if` は各枝が返す `Switch` に、末尾にない `if` は続きを本体にした join point (`CExpr::Join`) にする。`CoreFn::joins` は `JoinId` から `Join` の式を引く索引で、アリーナを作り直す Perceus が作り直す。値を返すだけの呼び出しは `TailCall` にする
- 変数が boxed かどうかは、型から `boxed` の1か所で決める。組み込みの引数の数は `Builtin::arity` (R2 の表) から、引数と結果の型は `TypedModule::builtins` (Prelude のスキーム) から引き、変換の種類 (`Prim`、`Perform`、`Compose`、`Constructor`) は `lowering` の1つの match に置く
- 変換は、`main` を `()` で呼ぶ入口の関数 `entry$main` を足す (`Program::entry`)
- ラムダは、捕まえた変数を先頭の引数に持つ関数に持ち上げる (`外側の名前$lambdaN`)。組み込みを値として使うときは、呼ぶだけの関数 (`builtin$名前`) で包む。関数の表は番号を先に取り、変換の途中で関数を足す
- 呼ばれるものが引数のないトップレベルの値のときは、呼ばれるものを先に評価してから引数を評価する (一般の `Apply` の経路)。左から右の評価順を保つため
- クロージャは `Payload::Closure(Closure)` (フィールドは `function` と `args`) で、関数値の呼び出し (`Call::Apply`) は eval/apply で行う。余った引数は `Payload::ApplyFrame(ApplyFrame)` (フィールドは `args` と `next`) として継続に積む。設計文書ではフレームの種類の enum にする案だったが、`Frame` を変えると既存の heap のテストの書き換えが要るため、ペイロードの別の種類にした。テストを守るために構造を曲げた例で、リファクタリング R3b でフレームの種類の enum に直す ([status.md](status.md) の「リファクタリング」)
- 共有されたクロージャを呼ぶときは、捕まえた値の参照を複製してからクロージャを手放す
- Perceus の挿入 (`perceus.rs`) は、変換の後にプログラム全体にかける独立したパスである。生存解析 (`liveness.rs`) は、`Let` の連鎖と join point の本体の連なりが長くなりうるので、作業の列で後順にたどる。join point の本体を範囲より先に求めるのは、範囲の中の `jump` が、本体で使う変数を要るためである。join point の本体は、本体で使う変数をちょうど1つずつ所有して始まり、`jump` の前で残りを捨てる。関数、プリミティブ、`perform` の引数は、どれも所有権を受け取る
- verifier (`verify.rs`) は、Perceus の後に、変数と join point の範囲、引数の数、所有権の釣り合いを確かめる。デバッグビルドの `lower` が毎回呼ぶ
- ヒープはインデックス方式のアリーナで、スロットごとに世代番号を持つ。値は `Copy` な `Value` である
- 変数のスロットは所有する参照の数を持つ (`eml_runtime::Owned { value, refs }`)。束縛で `refs` を 1 にし、`dup` で 1 足し、ヒープの値を読むたびに 1 引く (読み出しは move)。0 になったスロットは空にする。`decref` も 1 引く。継続のフレームのスロットもこの数を持ち、フレームを解放するときは、保持する参照を `refs` 回ずつ解放する。Perceus の `dup` で 1 つの変数が複数の参照を持つため、読み出しでスロットを空にするだけでは足りない。この規則により、[Core IR とインタプリタ](../spec/core-ir.md) の「変数の読み出しは move、複製は `dup` だけ」が文字どおり成り立つ
- CEK 機械の継続は、ヒープ上のフレームの連結リストである。呼び出しのフレームは環境 (スロットの配列) を退避する。`jump` と末尾呼び出しはフレームを積まない。最下部に `IO` の handler のフレームを置く

## ソースファイルと位置

- `FileId` と `SourceFiles` (`FileId` → パスとテキスト) は `eml_diagnostics` に置く。マイルストーン1 は単一ファイルなので、中身は実質1件である
- `TextRange` は `text-size` crate を直接使う (`rowan` が再公開しているものと同じ型)。`eml_diagnostics` は `rowan` に依存しない
- `SourceFiles::add` は、テキストの先頭の BOM を取り除いてから保存する。以後の位置 (`TextRange`、レイアウトの列、診断の行と列) は、すべて BOM を除いたテキストで数える ([字句](../spec/lexical.md))。lexer、レイアウト段、表示は BOM を扱わない
- 将来クエリ化するときは、`SourceFiles` を salsa の入力に置き換え、必要なら別の crate に切り出す

## CLI と lib API

- 引数の解析は `clap` (derive) を使う
- `eml check <file>` は、診断を stderr に表示する。`eml run <file>` は、診断 (警告を含む) を先に stderr に表示し、エラーがなければ実行する。警告がプログラムの出力の後に出ないようにするため
- `eml run --debug-heap` は、RC のリーク検出と解放済みアクセスの検出を有効にする
- 終了コードは、0 = 成功、1 = 診断のエラーあり、または実行時エラー、2 = 使い方の誤り (引数の誤り、ファイルが読めない) とする

`eml_cli` の lib は次の API を公開する。UI テストはこれをプロセス内で呼ぶ。

- `check(files, file_id) -> Vec<Diagnostic>`
- `compile(files, file_id) -> Compiled`。`Compiled` は、検査で出た診断 (警告を含む) と、エラーがなければ `Program` を持つ (`program: Option<Arc<Program>>`)。`main` がないこと (E2003) は `compile` だけが検査し、`check` は検査しない ([型と Kind](../spec/types.md) の「推論」)
- `execute(Arc<Program>, &RunConfig, stdout: OutputSink) -> RunResult`。`RunResult` は `Completed` / `RuntimeError` である

検査と実行を別の関数に分けるのは、呼び出し側が実行の前に診断を表示できるようにするためである。CLI と UI テストは、`compile` の診断を表示してから `execute` を呼ぶ。

関連する型は次のとおり。

- `OutputSink` (`eml_runtime`) は `Send + Sync` な共有の出力先の型で、`Arc<Mutex<dyn Write + Send>>` を包む。将来、複数のスレッドから `println` するため。テストでは、出力を捕まえる `OutputSink::capture()` を渡す
- `RunConfig` (`eml_interp` で定義し、`eml_cli` が再公開する) は `Default` を実装し、`#[non_exhaustive]` にする。`RunConfig::default()` から作り、フィールドを代入して使う。今のフィールドは `debug_heap` だけで、将来 `threads` と `schedule_seed` を足す

## 外部 crate

| crate | バージョン | 用途 |
|---|---|---|
| `rowan` | 0.16 | CST |
| `logos` | 0.16 | lexer |
| `text-size` | 1.1 | `TextRange` |
| `ariadne` | 0.6 | 診断の表示 |
| `clap` | 4.6 (`derive`) | CLI の引数 |
| `insta` | 1.49 (`glob`) | スナップショットテスト |
| `la-arena` | 0.3 | HIR の ID とアリーナ、型検査の別テーブル (`ArenaMap`) |

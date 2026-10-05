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
- `eml_diagnostics` は、診断を出す crate のすべてから使う。現時点で `eml_core_ir`、`eml_runtime`、`eml_interp` は診断を出さないので、`eml_diagnostics` に依存していない
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
| `eml_core_ir` | `lower(&Module, &TypedModule) -> Program`。途中のパスで止める `lower_until(&Module, &TypedModule, Pass) -> Program` |
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
- 演算子の列の組み直し (fixity の表を引く)、単項マイナス、セクションと演算子の参照のラムダへの脱糖
- `use`、`if` の `else` の補完、`let ... in`
- 補間の `++` の連結への変換、コマンドリテラルの `Cmd` の構築
- handler の節の引数の個数、`resume` の引数の個数

## `eml_hir` の内部

- ブロックは、fix のために最後の文の行の情報 (`LineStart`) を持つ
- `Module` はトップレベルの関数の `Arena<Function>` を持つ。本体は関数ごとの `Body` (`exprs`、`pats`、`locals` のアリーナ) に置く。後でクエリ化したときに関数単位で再計算できるようにするため (rust-analyzer と同じ分け方)。型の注釈は、シグネチャのものを `Signature::types` に、本体のものを `Body::types` に置く。本体を書き換えてもシグネチャが変わらないようにするため
- 型変数と row 変数の表は `Signature::generics` (`Generics`) に置く。シグネチャで定義し、本体の注釈は引くだけにする。ラムダの引数は、ラムダの本体だけで見えるスコープに入る
- シグネチャと等式は名前で対応づけてから、並び方を検査する。シグネチャか等式のない関数も `Function` として残し (`signature` か `body` が `None`)、呼び出し側で名前の誤りを連鎖させない
- 名前は `Res::{Local, Function, Operation, Constructor, Builtin}` に解決する。組み込みは名前解決の最も外側のスコープで、ユーザーの定義で隠せる。組み込みの名前、見え方 (名前で引く値、演算子、名前で引けない内部用)、引数の数は `eml_hir::builtin::BUILTINS` の表に、シグネチャは eml のソースで書いた Prelude (`crates/eml_hir/src/prelude.em`) に置く。変換のはじめに Prelude を構文解析し、`Module::builtins` に置く。Prelude の範囲には `FileId::PRELUDE` を使い、診断には出さない。S2 で `Prelude` モジュールに移す
- 型とエフェクトは item (`Module::types`、`Module::effects`) で、ID (`TypeDefId`、`EffectId`) で参照する。組み込みの `Int`、`String`、`Unit` と `IO` を変換のはじめに登録し、Prelude の `data Bool` を変換してから、続けて `effect` の宣言を変換する (`lower/effect.rs`)。エフェクトの名前をすべて登録してから操作を変換するので、操作の引数の型の row は後ろで宣言したエフェクトも引ける。操作は `Module::operations` (`OperationId`) に置き、シグネチャと引数の個数 (外側の矢印の数) を持つ。処理系が役割で引く item は `Module::lang` (`LangItems`) にある。エフェクトの宣言は型引数を `EffectDef::generics` に持つ。操作の `Generics` は、エフェクトの型引数を先頭に写して始め、その個数を `Operation::effect_params` に持つ。row のエフェクトは `EffectRef` (エフェクトと型引数) で、型引数の個数は `ItemScope::effect_params` で確かめる (E1015)
- `data` の宣言は `lower/data.rs` で変換する。型の名前をすべて登録してから宣言を変換するので、宣言どうしは再帰と相互再帰ができる。`TypeDef` は型引数 (`generics`)、フィールドの型の注釈のアリーナ (`types`)、種類 (`TypeDefKind::{Builtin, Data}`) を持つ。コンストラクタは `Module::constructors` (`ConstructorId`) に置き、属する型、タグ (宣言の順)、フィールドの型を持つ。コンストラクタは値の名前空間に入り、式では `Res::Constructor` に解決する。`Bool` の2つのコンストラクタは lang item (`LangItems::true_ctor`、`false_ctor`) で、`&&` と `||` の脱糖が使う
- パターンは `PatKind::Con` (コンストラクタと引数のパターン) を持つ。1つのパターンと、1つの等式、ラムダ、handler の節の引数の並びで同じ変数名を2回束縛すると E1017 にする。型の明示 `(p : T)` はどの `apat` の位置でも `PatKind::Annot` になる。`match` は `ExprKind::Match` で、枝 (`MatchArm`) のパターンの変数は、その枝の本体だけで見える
- 等式が2つ以上ある関数は、引数の位置ごとの隠れた局所変数 (`$0`、`$1`、…) を `Body::params` に束縛し、`Body::root` をそれらのタプル (引数が1つなら変数そのもの) に対する `match` にする。各等式が1つの枝である。この `match` は `MatchSource::Equations` を持ち、網羅性の検査が等式として報告する
- `Function::equation_ranges` は各等式の関数名の位置を持ち、E4002 の secondary になる。等式が1つの関数と引数のない値は、脱糖せず今の形のままにする
- 同じ名前の等式は、E1018 と E1019 の後もソースの順に1つの関数として扱う。引数の個数の違う等式 (E1020) は、`match` の枝に入れず、本体の `has_errors` を立てる
- セクションと演算子の参照は `lower/section.rs` でラムダに脱糖する。隠れた引数は `$a`、`$b`、`$x` である。被演算子が演算子の列のときは、`looser_operator` が列の中の二項演算子を走査し、セクションの演算子より弱く結合するもの (優先順位が同じなら、空いた側に結合の向きが合わないもの) があれば E1023 にする。先頭の前置 `-` は優先順位 6 で左結合の演算子として扱う。右が空いたセクションでは、`(- 2 *)` を E1023 にし、`(- 2 +)` を許す。左が空いたセクションでは、`climb` が演算子の直後に前置 `-` を許さない場合 (右の被演算子の最小の優先順位が 6 を超えるとき) に E1006 にする。どちらの誤りでも、被演算子は名前の誤りを報告するために変換してから捨て、セクション全体を `Missing` にする
- `use` は、`lower_stmts` が `use` の文に出会ったときに、残りの文を内側のブロックにしてラムダで包み、`use` の式の最後の引数に足す。ブロックの最後の `use` は E1024 で、最後の引数を `Missing` にした呼び出しにする。`let p = e in e2` は、ブロックの `let` と同じ経路で変換する
- トップレベルの名前は、変換の中の `ItemScope` (`lower/scope.rs`) で解決する。値と型 (型名とエフェクト名) の2つの名前空間を持ち、ユーザーの定義を先に引き、なければ組み込みを引く
- `Body` は走査関数を持つ。`walk_child_exprs` は式の直接の子を辿り、`pat_bindings` はパターンが束縛する変数を、`captures` は式の中で束縛していない変数 (ラムダ、handle の本体と節が捕まえる変数) を返す。式やパターンの種類を足す段階は、これらを直す
- handler の変換と節の検査は `lower/handler.rs` にある。節の先頭の名前は `ItemScope::operation` で操作だけから引く。誤った節 (引数の個数の誤り、重複、別のエフェクトの節) は診断を出して `ExprKind::Handle::clauses` に入れず、扱うエフェクトが決まらなければ `effect` を `None` にする。型検査はそれを見て診断を連鎖させない
- 演算子の列は、fixity の表で precedence climbing により組み直す。表は `ItemScope` が持ち、`Fixity` (優先順位と結合) を演算子の文字列で引く。標準の演算子の fixity は Prelude (`prelude.em`) の宣言で、`builtin.rs` には表を置かない。ユーザーの宣言は、item をすべて集めた後に表へ入れるので、宣言の位置を問わない (E1021、E1022)
- fixity は、演算子が解決した先の定義に付く。ユーザーの宣言があればそれを使い、なければ、このモジュールで定義した演算子は `infixl 9`、Prelude の演算子は Prelude の宣言、どちらもなければ `infixl 9` である。ユーザーの定義は同じ名前の Prelude の演算子を隠すので、fixity も Prelude のものを引かない
- 中置のコンストラクタのパターンの組み直し (`climb_pat`) も同じ表を引き、式の `climb` と同じ E1006 を報告する。誤りのパターンは `Missing` にする
- 演算子の定義は、演算子の文字列を名前とする関数である。シグネチャ `(</>) : …` と中置の等式 `a </> b = …` は、普通の関数のシグネチャと引数が2つの等式として扱う
- `binary()` は、演算子の文字列で脱糖を選ぶ前に名前を解決する。ユーザーの定義に解決すれば関数の呼び出しにし、そうでなければ次の脱糖にする。`&&` / `||` は `if` に脱糖する。`x |> f` は、`x` を先に評価する印 (`ExprKind::Call::evaluate_first`) を付けた関数適用 `f x` に、`f <| x` は関数適用に脱糖する。型検査は印を見ずに普通の呼び出しとして検査し、Core IR への変換が印の付いた引数を先に評価する。`let` に脱糖すると左辺が推論になり、引数の型を期待した診断が失われるため`else` のない `if` は `else_branch: None` のまま残し、型検査が `Unit` を求める

## `eml_types` の内部

- 型は検査器の中の表に置いて ID で引き、型変数の束縛を辿って単一化する。row は「ラベルの並び + 末尾の row 変数」で、scoped labels の書き換えで単一化する。Kind の変数と `≤` の制約は束の上で最小解を求める
- 各関数の本体は、シグネチャだけを見て検査する。引数を1つ消費するごとにシグネチャの矢印を1つたどり、本体の row は最後にたどった矢印の row になる。呼び出しでは、呼び出し先の閉じた row を新しい row 変数で開いてから本体の row と単一化する
- 推論の対象は [型と Kind](../spec/types.md) の Kind・型・row である。推論の後に、型付き HIR の上で別のパスとして、線形性の検査 ([線形性](../spec/linearity.md)) と網羅性の検査 ([網羅性](../spec/exhaustiveness.md)) を行う
- 型の表現はすでに閉じたレコードを使い、`Unit` は `Record([])` である。
- タプルの式、型、パターンは、数字ラベルの閉じたレコード (`Record([("0", A), ("1", B)])`) に写す。表示は、ラベルが 0 から連番の閉じたレコードを `(A, B)` と書く ([直積型とレコード](../spec/records.md))
- `==` と `!=` は Prelude で `a -> a -> Bool` で、参照した位置を記録しておき、関数の本体の検査の後に `a` の型から比べ方 (`Equality`) を決める。決まった比べ方は `BodyTypes::equalities` に、呼ばれる側の式の ID で入れる。決まらなければ E2006 にする
- 結果の `TypedModule` が持つ型は、型変数の束縛を解決した `Type` である (別テーブルの形は上の「各段階の規律」)
- シグネチャは閉じた形 `Shape` (`shape.rs`) で持つ。型の表を指さず、rigid な型変数、row 変数、Kind 変数をスキームの中の番号で持つ。参照するたびに多相な具体化をし、戻り値の側の閉じた row を開く。自分の本体の検査では rigid な具体化をして、本体の注釈が同じ変数を指せるようにする
- 呼び出しは、たどった矢印の row を今の row に含める (`Table::include_row`)。呼び出し先の末尾が推論変数なら、その row を今の row とそのまま単一化する。閉じた末尾と rigid な末尾では、末尾を新しい row 変数に替えた row を今の row と単一化し、今の row の残りをその変数で受ける。rigid な末尾では、さらに残りの末尾が同じ rigid 変数であることを確かめる。今の row をまだ推論している途中で残りの末尾が推論変数なら、その推論変数を rigid な変数に束縛する (推論されるラムダの中の呼び出しに必要)
- ラムダの引数の個数が合わないときや期待する型が壊れているときは、末尾が `Error` の row で本体を検査し、エフェクトの誤りを連鎖させない
- 型検査は4つの純粋な関数に分ける。`Context::new` (`context.rs`) はモジュール全体の情報 (データ型の Kind、名前、多重度) を1回だけ作り、関数ごとの型の表はこれを借りる。段0の `check::signatures` は、宣言ごとにシグネチャを閉じた形 `Shape` (`shape.rs`) にする。段1の `check::check_body` は、関数ごとに新しい表を作り、全宣言の `Shape` だけを見て本体を検査し、`BodyTypes` と Kind の問題 `KindProblem` (`kind/problem.rs`) を返す。段2の `kind::solve::solve_scc` は、呼び出しグラフの SCC (`scc.rs`) ごとに Kind の問題をまとめて解き、違反の由来と各関数の `KindScheme` を返す。`check_module` はこれらを順に呼んで結果を集めるだけである
- 段1は、トップレベルの値の参照ごとに `Shape` を具体化し、呼び出し先の制約を複写せずに具体化の記録 (`Instance`) を残す。記録は、具体化したときの制約の数 (`at`) を持ち、段2は展開した制約をその位置に差し込む。同じ範囲の違反の報告の順を保つためである。段2は、同じ SCC の参照を変数どうしの等式にし、前の SCC の参照にはそのスキームを複写する。解き方はワークリストで、残す制約は制約のグラフを強連結成分に縮めた DAG の上で求める (`Graph`)。残す制約を求めるときは、閉包に定数の境界もほかの宣言の変数も持たない成分 (何もない成分) を飛ばし、自分の変数を含む成分だけを引く。環状の相互再帰では、1つに縮んだ成分から各関数の矢印の Kind 変数の成分へ辺が関数の数だけ出るので、それを関数ごとにたどらないためである
- 使用回数のパス (`usage.rs`) は `Unr` の制約を出し、使った位置と使わなかった経路を Kind の制約の由来に入れる。報告が由来から E3001〜E3005 を選ぶ。由来 (`KindOrigin`) は型の表を指さない。持ち越しの由来は報告が指す `multi` の操作を持ち、スキームを通った持ち越しの由来は、呼んだ関数の中の1段分の要約 (`CarriedInner`) を持つ
- 持ち越しのパス (`carry.rs`) は、使用回数のパスの直後に、同じ本体と同じ `usage::reliable` の判定で動く。式を Core IR の評価の順の逆にたどる。順は、呼ばれる式、引数を左から、`|>` は左の被演算子を先とする。たどりながら、後で使う局所変数と、評価済みで消費前の部分式の値を持ち、呼び出し、`resume`、`handle` ごとに、持っている値の Kind と呼び出しの row の多重度を組にした持ち越しの制約 `carry(l, s)` を出す。`return` の節が捕まえる変数も、handle の本体の row 全体に対する持ち越しとして、このパスが扱う
- 型検査器は、呼び出しごとの row を `BodyTyping::calls` (`CallRows`) に記録する。トップレベルの値の呼び出しでは、`open_spine` で開く前の宣言の row を `BodyCheck::declared` から取って記録する。開いた row は今の row と単一化されるので、そのままでは今の row と区別できないためである。操作の直接の呼び出しは、row ではなく操作自身の多重度を使う
- 持ち越しの制約は `Table::carries` に入れ、段2が線形性と多重度の両方の束を解いた後に検査する。スキームには、`solve::carry_residual` が内部の変数を経由した推移を含めて残し、同じ組は位置が最も前の由来の1つにまとめる。具体化の展開では `CarriedThrough` の由来を付けて複写する。違反は `report::linear_misuse` が E3006 にする。同じ値の違反は、`check/mod.rs` の報告で値ごとに最初の1件に絞る
- 部分適用のクロージャの線形性は、それまでの引数と捕まえた値の Kind 以上になる (`Table::closure_kinds`)
- `TypedModule::signatures` は、関数の型と、スキームに残った Kind の制約のうち定数を片側に持つもの、および持ち越しの制約 (`Scheme::constraints`) を持つ。`dump` はこれを `kinds:` の行に出し、持ち越しの制約は `a => <e> <= Once`、`<e> <= Once`、`a => Multi <= Once` の形で表す
- 型の表は `table/` に分ける。`mod.rs` は型と変数の格納、`unify.rs` は型の単一化、`row.rs` は row の単一化と `include_row`、`kinds.rs` は Kind の制約を集める処理 (解くのは段2)、`export.rs` は外に出す型への変換である。型の形は `TyShape`、関数の矢印の線形性は `ArrowLin` と呼び、Kind (線形性と多重度) と取り違えないようにする
- row の末尾は `Tail::{Closed, Var, Error}` である。未定義のエフェクトか解決できない row 変数の跡は末尾 `Error` の row になり、型の `Error` と同じく束縛されない。末尾 `Error` は相手の側にしかないエフェクトを受け入れるが、自分の側の既知のエフェクトは受け入れない。綴り誤りの E1002 と無関係なエフェクトの誤りを隠さないためである。外に出す型では `{error}` と表示する
- `Table::export` は、後の段階と診断の文言の両方に渡す形を作る。書き出す `Type` は矢印の線形性を持たない。後の段階は線形性を読まず、持たせると本体の型を Kind を解くまで確定できなくなるためである
- 検査器は `check/` に分ける。`mod.rs` は段0〜2の組み立てと `TypedModule` の組み立て、`body.rs` は本体の検査、`report.rs` は診断を作る処理である。`if` とブロックは期待する型の有無 (`Expectation`) で check と infer の処理を共有し、矢印をたどる処理は `next_arrow` に、今の row の保存と復元は `with_ambient` にまとめてある。呼び出しの row を今の row に含める処理は `include_call_row` と呼ぶ
- E2002 の副ラベルは、本体の row が入る矢印の部分の型を指す (`body_arrow_range`)
- 組み込み、操作、コンストラクタの型は、ユーザーの関数と同じ経路 (`lower_signature` などで下ろしてから `Shape` に閉じる) で作る。本体がないので、宣言から出る制約 (`closure_kinds` と、操作の引数の `unrestricted`) だけを持つ Kind の問題を、1つの宣言だけの SCC として段2で解く
- 型構成子は `TyShape::Con(TypeDefId, Vec<Ty>)` で、型の適用の引数を持ち、row のラベルは `Label` (エフェクトの ID と型引数) である。外に出す型は `Type::Con { id, name, args }` と `EffectLabel { id, name, args }` で、`Module` を渡さずに表示できるよう名前を持つ。型変数は、シグネチャの変数 (`Type::Rigid`) と推論で解けなかった変数 (`Type::Flexible`) を区別する
- データ型の Kind に効く型引数の位置は `data.rs` で、すべての `data` の宣言について不動点で求める。`Table::kind_bounds` は、`Con` の効く位置の引数の境界を並べる。コンストラクタのスキームは関数と同じ経路で作り、`TypedModule::constructors` に置く
- パターンは期待する型を受けて検査する。`match` は scrutinee の型で各枝のパターンを検査し、枝の本体を `if` の枝と同じく検査する。使用回数のパスは `match` の枝を別の経路として扱う
- 網羅性の検査は `exhaustive.rs` にある。型推論と使用回数のパスの後に、型付き HIR の上で Maranget の usefulness を使って検査し、漏れているパターンの例を作る。コンストラクタの集合はパターンの型の型構成子から引く
- 由来が `MatchSource::Equations` の `match` は、等式ごとの行 (引数の並び) の行列として検査する。漏れは E4002、到達しない等式は E4005 (Warning) で、`Function::equation_ranges` を指す。E1020 で枝に入れなかった等式がある関数は、等式の検査を行わない (枝の数が等式の数より少ないので、漏れの診断が連鎖しない)
- `check_function` は、シグネチャの矢印をたどる途中で壊れた矢印か、引数が矢印より多い場合に当たったら、今の row を末尾が `Error` の row にして本体を検査する。ラムダの検査と同じ扱いで、E2002 を連鎖させない
- row の単一化は、同じエフェクトのラベルを row の中の順で対にし、型引数を単一化する。一致しなければ `UnifyError::EffectArgs` で、`include_call_row` が E2001 にする。型引数の中で型変数や row 変数が自分自身に現れるときは `UnifyError::Occurs` で、`include_call_row` が E2005 にする。row 変数を束縛するときは、ラベルの型引数の中の関数型と継続の型の row までたどって、その変数が現れないことを確かめる (`Table::row_occurs`)
- 継続の型は `TyShape::Cont` (操作の結果の型、継続の線形性、handle の外側の row、handle の結果の型) で、外に出す型は `Type::Cont` である。`once` の操作の `k` の線形性は `Lin`、`multi` の操作の `k` は `Unr` である
- 操作のスキームは、組み込みと同じ経路で作る。シグネチャの外側の最後の矢印に、操作のエフェクトだけの row を付ける (`scheme::lower_operation`)。エフェクトの多重度は操作の多重度の最大である。row のラベルの型引数は、エフェクトの型引数の rigid 変数である。操作の引数の型の Kind 変数を `Unr` に固定する `Table::unrestricted` は、エフェクトの型引数の Kind 変数を外す
- handle の検査は `check/handle.rs` にある。本体は今の row の前に扱うエフェクトを足した row で、節は今の row で検査する。handle ごとにエフェクトの型引数を新しい推論用の変数にし、節ではエフェクトの型引数をその変数に、操作自身の型変数だけを新しい rigid 変数にする (`Rigids::with_effect_args`)。`resume` は、推論用の変数でできた継続の型と単一化してから、関数の呼び出しと同じく row を今の row に含める
- Kind の制約は由来 (`KindOrigin`) を持つ。型の表が「今の由来」を持ち、制約を作るときに記録する。本体の検査は単一化、呼び出しと `resume` の row の包含 (`include_call_row`)、参照の具体化の前後で、使用回数のパスは `Unr` の制約の前後で、今の由来を設定する。`solve_kinds` は破れた制約の由来を返し、`check` が `report::linear_misuse` で E3001〜E3006 にする。報告済みの誤りの跡 (`Missing`) がある本体と、型の誤りを報告済みの本体 (`check/mod.rs` の `well_typed` が偽) では、使用回数のパスも持ち越しのパスも由来を記録しない。HIR の誤りがある本体でも記録しない。由来のない制約は、宣言の型から作る制約 (具体化のたびに由来を付けて複写する) と、この記録しない制約に限り、`solve_kinds` は返さない

## `eml_core_ir`、`eml_runtime`、`eml_interp` の内部

- Core IR とインタプリタの設計は [Core IR とインタプリタ](../spec/core-ir.md)、ヒープと RC は [ランタイム](../spec/runtime.md) が定める
- インタプリタの値とフレームに `Rc` と `RefCell` を使わない。Core IR は `Arc<Program>` で読み取り専用で共有する。将来、複数のスレッドがそれぞれの CEK 機械で同じプログラムを実行するため ([マルチコア対応の設計](../future/multicore.md))
- `RunConfig::file_root` が `open` の基準ディレクトリである。既定の空のパスは、カレントディレクトリを指す。`File` の中身は `eml_runtime` の `file.rs` の `FileHandle` で、実行時エラーは `Fault::{FileOpen, FileRead, FileNotUtf8}` である
- パスの順番は `pipeline.rs` だけが持つ。`lower_until` は、変換 (`translate/`)、`simplify`、Perceus を順にかけ、指定したパスの直後で止める。RC の命令を入れる前のパスの後では、`liveness::analyze` で `captures` を埋め直してから `verify_scopes` をかけ、Perceus の後では `verify` をかける。検査はデバッグビルドだけで、誤りはパスの名前を付けた panic にする
- 変換は `translate/` にある。`mod.rs` は式の値の渡し先と join point の組み立てという制御の骨組み、`expr.rs` は式ごとの変換と呼び出しの場合分け、`program.rs` は関数の表 (`ProgramBuilder`)、組み込みと操作を包む関数、入口の関数、エフェクトの表、`types.rs` は型から決まる変数の性質 (`boxed`) と組み込みの変換の種類 (`lowering`) を持つ。`pattern.rs` は `match` と、`let`・ラムダ・等式の引数のパターンを決定木にコンパイルする。列の頭はコンストラクタ、タプル、リテラルで、リテラルの列は比べるプリミティブと `Bool` の `switch` の連なりにする
- Core IR の関数は、ANF の木をアリーナに置き、`CExprId` で参照する。継続のフレームが再開する位置を ID で持てるようにするため。変換は式の値の渡し先 (`Exit::Return` か `Exit::Jump`) を持って回り、末尾の `if` は各枝が返す `Switch` に、末尾にない `if` は続きを本体にした join point (`CExpr::Join`) にする。末尾にない `if` は、`tail` で条件の計算ごと join point の範囲を組み立てる。`CoreFn::joins` は `JoinId` から `Join` の式を引く索引で、アリーナは Perceus が作り直す。値を返すだけの呼び出しは `TailCall` にする
- 変数が boxed かどうかは、型から `boxed` の1か所で決める。組み込みの引数の数は `Builtin::arity` (R2 の表) から、引数と結果の型は `TypedModule::builtins` (Prelude のスキーム) から引き、変換の種類 (`Prim`、`Io`、`Compose`) は `lowering` の1つの match に置く。`data` の型の変数は、引数を持つコンストラクタが1つでもあれば boxed にする。要素が2つ以上の閉じたレコードの型 (タプル) の変数も boxed にする
- 変換は、`main` を `()` で呼ぶ入口の関数 `entry$main` を足す (`Program::entry`)
- ラムダは、捕まえた変数を先頭の引数に持つ関数に持ち上げる (`外側の名前$lambdaN`)。組み込みを値として使うときは、呼ぶだけの関数 (`builtin$名前`) で包む。コンストラクタを値として使うときは、値を作るだけの関数 (`con$名前`) で包む。関数の表は番号を先に取り、変換の途中で関数を足す
- 呼ばれるものが引数のないトップレベルの値のときは、呼ばれるものを先に評価してから引数を評価する (一般の `Apply` の経路)。左から右の評価順を保つため
- クロージャは `Payload::Closure(Closure)` (フィールドは `function` と `args`) で、関数値の呼び出し (`Call::Apply`) は eval/apply で行う。継続のフレームは `Payload::Frame(Frame)` で、`Frame` は種類の enum である。`Return` は呼び出し元に戻るフレームで、呼び出しの後で使う変数だけを退避する。`Apply` は余った引数を持ち、戻った関数値に適用する。`Io` は継続の最下部の `IO` の handler である。記述子はペイロードの種類から決める
- 共有されたクロージャを呼ぶときは、`Heap::take_or_copy` で中身を写し、写した中身の子の参照を1つずつ増やしてから元の参照を手放す。`Payload` は `Clone` を導出しないので、`ObjRef` を `dup` せずに複製できない
- 引数を持つコンストラクタの値は `Payload::Data` (タグとフィールド) で、`Switch` は `Heap::take_or_copy` で分解する
- `simplify` (`simplify.rs`) は、変換の後、Perceus の前に、アリーナの上で join point をその場で書き換える。木から外れた式はアリーナに残るが、Perceus が捨てる。`jump` の位置と親は、根からたどれる式だけで求める。F、B3、K1、B2、B5、B3、B4、DCE の順に1巡だけ回す。F は、本体の先頭に join point の定義が並ぶ join point (決定木が `if` の join point の本体に置くものなど) から、その定義を外へ出し、最初の B3 と B2 が `Switch` に届くようにする。F は `captures` を読むが、パイプラインが直前に埋めているので正しい。最初の B3 は、`match` の枝の join point を `Switch` の枝に戻す。K1 は、値が分かっているコンストラクタへの `Switch` を枝にする。分かっているコンストラクタの表は関数全体で1つ作る。変数は1回だけ束縛され、使用は束縛の範囲にあるので、根からの走査で範囲を追う必要がないためである。K1 と B2 が枝の中の変数を置き換えるので、引くときにアリーナから今の右辺を読み直す。B2 (分かっているタグとコンストラクタの値の `jump`) は、枝ごとに join point を作る。引数のない枝は引数0個の join point にし、枝の中の join point の引数をそのタグの定数に置き換える。フィールドを束縛する枝は、分かっている値が届くものだけを、フィールドを引数に取る join point にし、枝が値全体も使うときは値も引数で渡す。この引数は、フィールドも値全体も束縛なので、元の枝の束縛とは別の新しい変数にする。同じ変数を2回束縛すると verifier が拒否するためである。DCE は、使われない変数の `let` のうち、実行時に何も起こさない右辺 (変数や定数の参照、文字列定数、`con`、クロージャの生成、失敗しない演算) を消す。呼び出し、IO、`drop`、失敗しうる演算は消さないので、エフェクトの順は変わらない。最後に、残った join point に元の順で番号を振り直す
- Perceus の挿入 (`perceus.rs`) は、変換の後にプログラム全体にかける独立したパスである。Perceus は最初に関数ごとに生存解析 (`liveness.rs` の `analyze`) を行う。解析は、`Let` の連鎖と join point の本体の連なりが長くなりうるので、作業の列で後順にたどる。解析が持つのは、ブロックの入口 (`Switch` の枝と join point の範囲) の生きている変数の集合と、join point の `captures` だけである。`Join` の直前の集合は範囲の入口の集合に join point の `captures` を足したものである。verifier は `captures` が `Join` の位置で範囲にあることを求めるので、`jump` が届かない join point でも、`captures` は `Join` まで生かしておく。Perceus は連鎖を逆順に組み立てるときに、生きている変数の集合を1つ更新して `dup` / `decref` と `saved` を決める。連鎖の途中の所有は、生きている RC の対象と直前に束縛した変数から求める。join point の本体は、`captures` と引数のうち RC の対象をちょうど1つずつ所有して始まり、`jump` の前で残りを捨てる。`Switch` は scrutinee を消費し、枝はフィールドのうち RC の対象を所有して始まる。関数、プリミティブ、`perform` の引数は、どれも所有権を受け取る
- `saved` は、各呼び出し (`Rhs::Call`) の後で使う変数で、Perceus が埋める。RC の対象でない変数と、`jump` の先の join point の `captures` も含む。verifier は、`saved` の RC の対象の変数が所有している変数とちょうど一致すること、呼び出しの後は `saved` の変数と結果の変数だけが範囲にあることを確かめる
- verifier (`verify.rs`) は2つの入口を持つ。`verify` は Perceus の後に、変数と join point の範囲、引数の数、所有権の釣り合いを確かめる。`verify_scopes` は Perceus より前に、範囲と引数の数を確かめ、RC の命令と `saved` がないことを確かめる。`verify_scopes` はどの変数も RC の対象として扱わないので、束縛、使用、join point の入口、`jump` の所有の検査が範囲の検査だけになる。どちらも `captures` を宣言として扱い、生存解析は使わない
- ヒープはインデックス方式のアリーナで、スロットごとに世代番号を持つ。値は `Copy` な `Value` である
- 環境のスロットは値だけを持ち、読み出しはスロットを書き換えない。参照の所有は Core IR の命令 (使用、`dup`、`decref`) が表し、verifier が釣り合いを確かめる。呼び出しのフレームには `saved` の変数だけを退避するので、フレームはちょうど所有している参照だけを持ち、解放するときは退避した値を1回ずつ解放する
- CEK 機械の継続は、ヒープ上のフレームの連結リストである。呼び出しのフレームは、呼び出しの後で使う変数 (`saved`) だけを退避する。`jump` と末尾呼び出しはフレームを積まない。最下部に `IO` の handler のフレーム (`Frame::Io`) を置く
- 実行時エラーは `RuntimeError` (実行中の関数で止まった `Fault` と、`debug_heap` の `Leak`) で、`step` は `Result<Step, Fault>` を返す。`Fault` に関数の名前を付けるのは `run` である。表示の文言は CLI と UI テストが使う
- handle の本体と節は、ラムダと同じ `lift` で持ち上げる (`外側の名前$handleN`、`外側の名前$handleN$操作名`、`外側の名前$handleN$return`)。操作を値として使うときは、`perform` を呼ぶだけの関数 (`op$名前`) で包む。`Program::effects` はエフェクトごとの操作の表 (名前と、再開できるか) で、番号は `EffectId` の添字である
- `Call::{Handle, Perform, Resume}` は呼び出しの一種なので、`saved` と末尾の位置の扱い、Perceus と verifier の規則を、ほかの呼び出しと共有する。`IO` の操作は `Rhs::Io` で、その場で実行する
- handler は `Frame::Handler` (エフェクトの番号、節のクロージャ、`return` の節、次のフレーム) で、継続は `Payload::Continuation` (区間の先頭のフレームと、所有しない handler フレームへの参照) である。`perform` は handler フレームの次を切り離し、`resume` はそこに今の継続をつなぐ。`never` の操作は区間をその場で解放する。共有された継続の `resume` は `Heap::take_or_copy` で区間を写す (`copy_segment`)。フレームはつねに一意で、共有されうるのは継続オブジェクトだけである

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
- `execute(Arc<Program>, &RunConfig, stdout: OutputSink) -> Result<(), RuntimeError>`。`RuntimeError` は `eml_interp` の型を再公開したものである

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

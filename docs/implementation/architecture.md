# コンパイラの構成

位置づけ: 手引き。

eml の処理系をどの crate に分け、各段階がどんな規律に従うかを説明する。言語の意味は [spec/](../spec/) の各文書が定める。この文書は、それを実装する側の構造と、構造を選んだ理由を扱う。型や関数の細部はコードとそのコメントにある。

## 全体の方針

- 処理系はバッチ型のパイプラインである。各段階を純粋な関数とし、Arena と ID で表現して、後でクエリ化 (salsa など) できるようにしておく
- 実行系は、型付き Core IR (ANF 形式で、RC とエフェクトを明示する) と CEK 風のインタプリタからなる。将来の LLVM バックエンドも同じ IR から変換する。ヒープと RC は `eml_runtime` に分離する
- 構文木には rowan (Red-Green Tree) を使い、その上にリッチな診断と LSP を整える

## プログラム全体の構成

- 処理系は、item を単位にしたプログラム全体のパイプラインである。rust-analyzer の item tree と DefMap の分け方を、eml の規模に合わせた。名前解決は、`ItemTree` (ファイルごとの宣言の要約)、`DefMap` (モジュールごとのスコープ表)、item ごとの変換の3段で、item の ID (`ItemId`) はプログラム全体で一意である。型検査は宣言、本体、SCC の粒度で動き、モジュールの境目を使わない
- モジュールごとに検査して、依存先のモジュールのインタフェースを借りる方式は採らない。次の3つの負担が生じるためである
  - 相互再帰がモジュールをまたぐと SCC もまたぐので、import の循環を許せなくなる
  - 型検査が、自分のモジュールの表と依存先の表の2系統を引くことになる
  - モジュールのインタフェースとしての型検査の出力を、別に設計する必要がある
- item を単位にすれば、import の循環を許すかどうかは `DefMap` の作り方だけの問題になる

採らなかった形は次のとおり。

- salsa を今入れること。段階をクエリの形にしてあるので、後で載せ替えられる。LSP を作るまでは手間が大きい
- モジュールごとに Core IR を作ってリンクすること。インタプリタでは得るものがない
- HIR の位置を今 source map に移すこと。利点が出るのはクエリ化した後である ([ロードマップ](../future/roadmap.md))
- プログラム全体の item を1つのアリーナに置くこと。モジュールの変換が共有のアリーナを書き換えるので、段階が純粋な関数でなくなる
- どの item も Core IR の関数にして、`simplify` の規則で命令に戻し、使わない関数を取り除くパスで消すこと。変換が満ちた呼び出しをすでに命令にしているので、飽和の場合分けを `saturate` にまとめれば足りる
- `Bool` のタグをインタプリタまで運ぶこと。タグは Prelude の宣言の順で決まるので、定数 `FALSE` と `TRUE` をテストで照らし合わせれば足りる

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

- `version`、`edition = "2024"`、依存する crate のバージョンは、workspace のルートの `Cargo.toml` でまとめて管理する
- `flake.nix` は、workspace のルートの `Cargo.lock` を使って `eml_cli` だけを組み立て、`eml` バイナリを作る

## crate の依存関係

依存は上から下への一方向だけにする。

```
eml_cli          check / run コマンド。各段階をつなぐだけ。テストから呼べる lib API を公開する
eml_interp       Core IR を CEK 機械で実行する
eml_runtime      オブジェクトのモデル、ヒープ、参照カウント、debug_heap の検査、OutputSink
eml_core_ir      型付き HIR → Core IR。dup/decref の挿入パス
eml_types        Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査
eml_hir          モジュールの読み込み、CST → HIR の変換、名前解決、脱糖
eml_syntax       SyntaxKind、lexer、レイアウト段、イベント方式のパーサ、rowan、型付き AST ラッパ
eml_diagnostics  Diagnostic 型、FileId と SourceFiles、行と列、ariadne による表示
```

- 段階の連なりに `eml_runtime` は入らず、`eml_interp` だけが依存する
- `eml_diagnostics` は、診断を出す crate のすべてから使う。今は `eml_core_ir`、`eml_runtime`、`eml_interp` が診断を出さないので、この3つは依存しない
- `eml_test_support` は開発専用の crate で、パイプラインに入らない。各 crate の結合テストが dev-dependency として使う。段階を feature (`hir` < `types` < `core` < `run`) で選び、各 crate は自分の段階までを有効にする。破壊的な変更の途中で下流の crate がまだ組み立たなくても、変更している段階のテストを流せるようにするため ([テスト戦略](testing.md))

## 各段階の規律

- 各段階は `fn stage(input: &In) -> (Out, Vec<Diagnostic>)` の形の純粋な関数にする。グローバルな可変状態は持たない。例外は `eml_core_ir` で、診断のエラーがないプログラムだけを受け取り、`Program` を返す (下の「エラーが出ても止まらない」)
- HIR 以降は ID で参照する (`la-arena`)。型などの解析結果は、`ExprId → Type` のような別テーブルに置く
- HIR の各ノードは、元の構文の範囲 (`TextRange`) を持つ。演算子の列を組み直した部分式のように、対応する構文ノードのない式があるため
- 型付き HIR は HIR を複製せず、宣言ごとと本体ごとの結果の別テーブル (`TypedProgram`) だけを持つ。そのため `eml_core_ir` は HIR と `TypedProgram` の両方を受け取る

現在の各段階の入口は次のとおり。

| crate | 関数 |
|---|---|
| `eml_syntax` | `parse(FileId, &str) -> (Parse, Vec<Diagnostic>)` |
| `eml_hir` | `load(&str, &str, &dyn ModuleSource) -> (Loaded, Vec<Diagnostic>)`、`def_map(&[LoadedModule]) -> (DefMap, Vec<Diagnostic>)`、`lower(&DefMap, &[LoadedModule]) -> (Program, Vec<Diagnostic>)` の順に呼ぶ。`load` は入口の表示のパスと本文を受け取り、ファイルごとに `item_tree(FileId, &ast::SourceFile) -> (ItemTree, Vec<Diagnostic>)` を呼ぶ。モジュールの並びは、0番目が Prelude、1番目が入口のモジュール、2番目からが import で見つけた順のモジュールである |
| `eml_types` | `check(&Program) -> (TypedProgram, Vec<Diagnostic>)` |
| `eml_core_ir` | `lower(&hir::Program, &TypedProgram, FunctionId) -> Program`。途中のパスで止める `lower_until` もある。入口の関数は呼ぶ側が渡す |
| `eml_interp` | `run(Arc<Program>, &RunConfig, &OutputSink) -> Result<(), RuntimeError>` |

## エラーが出ても止まらない

- `eml_cli` の `check` / `compile` は、エラーがあっても途中で止めずにすべての段階を実行し、診断を集める。1回の実行で、独立した複数のエラーを報告するため
- パーサは壊れた入力でも `ERROR` ノードを作って回復し、必ず `SOURCE_FILE` の木を作ってパニックしない。名前解決と型推論は、誤りの場所に `Error` 型を入れて診断の連鎖を抑える。規則は [レイアウト規則](../spec/layout.md) と [診断](../spec/diagnostics.md) の「連鎖する診断の抑止」にある
- Core IR は、診断のエラーがないプログラムだけを受け取る。`compile` は、エラーがあれば Core IR を作らない。型付きで正しい入力を前提にできるので、Core IR への変換は診断を返さない

## 構文を差し替えられるようにする

- パーサは rust-analyzer と同じイベント方式にする。ノード開始、トークン、ノード終了のイベントを出し、別の処理 (`sink.rs`) で rowan の木を組み立てる
- 構文の規則は `eml_syntax/src/grammar/` に閉じ込める。構文の変更は、原則として `grammar/`、`SyntaxKind`、lexer、型付き AST ラッパ、レイアウト段の変更だけで済むようにする

## `eml_syntax` の内部構成

```
lexer/         字句解析。トークン列をつなげると元のテキストに戻る (lossless)
literal.rs     リテラルの値の解釈。lexer の検査と AST の値の取り出しが同じエスケープの表を使う
layout.rs      レイアウト段。trivia を除いたトークン列に仮想トークンを挿入する
parser.rs      イベント方式のパーサの仕組み。文法の規則は持たない
grammar/       文法の規則 (items / types / patterns / expressions)
sink.rs        イベント列から rowan の木を組み立てる
ast.rs         型付き AST ラッパ
syntax_kind.rs SyntaxKind
token_set.rs   トークンの集合 (u128 のビット集合)
debug_dump.rs  木のダンプ。構文のテストとデバッグに使う
```

- lexer は logos で単純なトークンを切り出し、文字列、コメント、演算子の分類などを手書きの層で扱う。最終的には、文字列、補間、コマンドリテラル、入れ子のブロックコメントの各モードをスタックで持つ層にし、文字列をトークン列 ([文法](../spec/grammar.md) の `string`) に分ける。今は、補間を含む文字列などをそれぞれ1つのトークンにしている。トークン列への分割は、これらを実装する M3 と M9 で行う
- レイアウト段は、幅 0 の仮想トークン `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE` を挿入する。規則は [レイアウト規則](../spec/layout.md) が定める。パーサは仮想トークンを読んでも `Event::Token` を出さないので、仮想トークンは木に入らず、CST は lossless のまま
- `grammar/` は [文法](../spec/grammar.md) に従う。演算子の列は `OP_SEQ` ノードに平たく並べ、木への組み直しは HIR で行う
- トークンの種類 (`EOF` まで) は 128 未満に収める。`TokenSet` が `u128` のビット集合であるため
- `eml_hir` は型付き AST の API と、識別子や演算子の `SyntaxToken` だけを使う。CST の木の構造 (`.syntax()`) には触れず、`rowan` に依存しない
- E0004 (未対応) は、原則としてパーサではなく HIR が出す。どの層が出すかの例外は [文法](../spec/grammar.md) の「実装の段階」が定める。E0004 はどの段階でも同じ意味なので、番号とラベルは `eml_diagnostics` に置く

## `eml_hir` で行う脱糖と検査

HIR への変換では、名前解決に加えて、名前の重複と未定義の名前の検査、シグネチャと等式の対応の検査、spec が HIR に割り当てた脱糖と検査を行う。式の脱糖と検査の一覧は [式](../spec/expressions.md) の「HIR で行う脱糖と検査」に、宣言とレコードに関するものは [宣言](../spec/declarations.md) と [直積型とレコード](../spec/records.md) にある。実装上の判断は下の「`eml_hir` の内部」に書く。

## `eml_hir` の内部

- 読み込みの段 (`load.rs`) は、入口の本文を parse して `ItemTree` を作り、import を宣言の順に幅優先でたどって読む。ファイルの読み方は `ModuleSource` の trait で受け取り、IO は実装 (`eml_cli` の `FsProvider`、`eml_test_support` の `MemorySource`) だけが持つ。同じ読み方を渡せば同じ結果を返すので、段階は純粋な関数のままである。依存先の表示のパスは、入口の表示のパスのディレクトリに根からの相対パスをつないだものにする。E1026 と E1030 はこの段で報告する。`load` は Prelude の本文に `PRELUDE_SOURCE` を使う。`load_with_prelude` は Prelude の本文を引数で受け取る形で、Prelude に定義を足したプログラムを変換するテストが、`eml_test_support::lower_with_prelude` を通して使う
- トップレベルの名前の解決は3つの段階に分ける。`item_tree` はファイルごとに宣言を集め、シグネチャと等式を名前で1つの関数にまとめ、名前を解決しなくても判定できる誤りを出す。`def_map` は読み込んだモジュールの列から、item の ID、モジュールごとの名前の表、import のスコープ (修飾子からモジュールの列への表と、修飾なしにした名前の表)、定義に付く fixity、lang item、表示名の表 `DisplayNames` を作り、import の循環 (E1027) と import の並びを検査する。`lower` は `DefMap` で名前を引き、item を `DefMap` と同じ局所の番号の順にアリーナへ置く
- 名前を変換より先にすべて集めるので、宣言の順によらず、`data` どうしの相互再帰や、後ろで宣言したエフェクトへの参照ができる
- HIR の出力は `Program` で、読み込みの段が読んだモジュールの列と、表示名の表を持つ。モジュールの番号は、Prelude が 0、入口が 1、依存先は見つけた順に 2 以降で、順が決まるので診断の並びが安定する。item の ID はモジュールと局所の番号の組で、プログラム全体で一意である。各モジュールは item のアリーナと関数の本体を分けて持つ。本体を書き換えても item が変わらないようにするため。本体は関数ごとの `Body` で、後でクエリ化したときに関数単位で再計算できるようにする (rust-analyzer と同じ分け方)。型の注釈も、シグネチャのものと本体のものを分けて置き、本体を書き換えてもシグネチャが変わらないようにする
- `DefMap` は、モジュールごとに値と型の2つの名前空間を持ち、名前ごとに定義をソースの順にすべて持つ。重複の扱いは [modules.md](../spec/modules.md) の「名前空間」、引く順は「名前の解決」が定める。重複した `data` のコンストラクタと `effect` の操作は使えない印を持ち、それらへの参照は診断を重ねずに `Missing` にする
- 組み込みは eml のソースで書いた Prelude (`crates/eml_hir/src/prelude.em`) で、`SourceFiles` に登録した普通のファイルとして、入口のファイルとは別のモジュールに変換する。Prelude の等式のないシグネチャは、本体のない intrinsic の関数で、E1005 にしない。処理系が役割で引く item (`negate`、`==`、`&&`、`Bool` のコンストラクタ、`IO` など) は lang item で、`pub` によらず Prelude から名前で引く
- シグネチャか等式のない関数も `Function` として残し、呼び出し側で名前の誤りを連鎖させない。引数の個数の違う等式 (E1020) は `match` の枝に入れず、本体に誤りの印を立てる
- 等式が2つ以上ある関数は、引数の位置ごとの隠れた局所変数に対する `match` に脱糖する。この `match` は由来 (`MatchSource::Equations`) を持ち、網羅性の検査が等式として報告する。等式が1つの関数と引数のない値は、脱糖しない
- fixity は定義に付く ([宣言](../spec/declarations.md) の「fixity」)。`DefMap` が item をすべて集めてから付けるので、宣言の位置は問わない。式とパターンの組み直しは同じ fixity を引く
- `&&` と `||` は、解決した先が lang item のときだけ `if` に脱糖する。`|>` と `<|` は脱糖せず、Prelude の関数の普通の呼び出しにする。`let` に脱糖すると左辺が推論になり、関数の引数の型を期待した診断 (E2001 が `x |> f` の `f` を指す) が失われるため
- 呼び出しの評価の順は `eval.rs` の `call_steps` が決める ([関数適用](../spec/expressions.md))。持ち越しのパス (`eml_types`) はこの手順を逆に、Core IR の変換 (`eml_core_ir`) は順にたどる。順の組み立てを1か所に置くのは、2か所で組むとずれたときに持ち越し規則が実行と食い違うためである。値の判定 (`is_value`) と引数のまとめ方 (`known_arity`) も、両方の段階がここから引く
- `Body` の走査関数 (`walk_child_exprs`、`pat_bindings`、`captures`) は、式やパターンの種類を足す段階が直す。誤った handler の節は診断を出して節に入れず、扱うエフェクトが決まらなければ `effect` を `None` にして、型検査に診断を連鎖させない。誤りのあるセクションも、被演算子の名前の誤りを報告してから全体を `Missing` にする

## `eml_types` の内部

- 型は検査器の中の表に置いて ID で引き、型変数の束縛を辿って単一化する。row は「ラベルの並び + 末尾の row 変数」で、scoped labels の書き換えで単一化する。Kind の変数と `≤` の制約は束の上で最小解を求める。推論の対象は [型と Kind](../spec/types.md) が定める
- 推論の後に、型付き HIR の上で別のパスとして、線形性の検査 ([線形性](../spec/linearity.md)) と網羅性の検査 ([網羅性](../spec/exhaustiveness.md)、Maranget の usefulness) を行う
- 型検査は4つの純粋な関数に分ける。`Context::new` はプログラム全体の情報 (データ型の Kind、名前、多重度) を1回だけ作り、関数ごとの型の表はこれを借りる。表を作る費用を関数の大きさに比例させるため。段0はシグネチャを閉じた形 `Shape` にする。段1は関数ごとに新しい表を作り、全宣言の `Shape` だけを見て本体を検査し、Kind の制約は集めるだけにする。段2は呼び出しグラフの SCC ごとに Kind の問題を解く
- `Shape` は型の表を指さず、変数をスキームの中の番号で持つ。参照するたびに具体化し、段1は呼び出し先の制約を複写せずに具体化の記録を残して、段2が展開する。intrinsic の関数、操作、コンストラクタの型も、ユーザーの関数と同じ経路で `Shape` に閉じる
- 宣言の型は `TypedProgram::decls` の `DeclType` にある。結果を宣言ごとに持つのは、クエリ化したときに宣言ごとのクエリの結果として使い、REPL で前の入力の宣言を検査し直さずに使い回せるようにするためである
- `Table::export` は、後の段階と診断の文言の両方に渡す形を作る。書き出す `Type` は矢印の線形性を持たない。後の段階は線形性を読まず、持たせると本体の型を Kind を解くまで確定できなくなるためである。同じ理由で、`DeclType` の `Shape` と `KindScheme` は crate の外から読めない
- 持ち越しのパスは、使用回数のパスの直後に、同じ本体を Core IR の評価の順の逆にたどる。型検査器が記録した呼び出しごとの row を読み、持っている値と row の多重度を組にした制約を出す。この制約は、段2が線形性と多重度の両方の束を解いた後に検査する
- Kind の制約は由来 (`Provenance`) を持ち、型の表の「今の由来」から記録する。段2は破れた制約の由来を返し、`Suppressed` の制約は捨てる。そのため、報告したい制約には必ず由来を付ける。既定の由来 `Unattributed` は付け忘れを見つけるためのもので、それが破れたときと、宣言の型だけで作った制約が破れたときは処理系の誤りとして扱う
- 報告済みの誤りのある本体 (誤りの跡がある、型の誤りを報告済み、HIR の誤りがある) では、使用回数のパスも持ち越しのパスも由来を記録しない ([診断](../spec/diagnostics.md))
- 由来の位置は、ファイルと範囲の組 (`Span`) で持つ。具体化を通った由来は、呼んだ関数のある別のモジュール (Prelude など) の中を指しうるためである。報告は由来のファイルを使い、ファイル、範囲の順に並べる
- row の末尾には `Error` がある。未定義のエフェクトか解決できない row 変数の跡で、相手の側にしかないエフェクトを受け入れるが、自分の側の既知のエフェクトは受け入れない。綴り誤りの E1002 と無関係なエフェクトの誤りを隠さないためである。ラムダやシグネチャの矢印が壊れているときも、末尾が `Error` の row で本体を検査し、エフェクトの誤りを連鎖させない
- 型の走査は `Type`、`TyShape`、`ShapeTy` の `for_each_child` だけがたどり、そこでは `..` を使わず欄をすべて名前で受ける。欄を足したときに、occurs の検査などから漏れないようにするためである。内部の型の形は `TyShape`、矢印の線形性は `ArrowLin` と呼び、Kind と取り違えないようにする
- `Type::Con` と `EffectLabel` は ID だけを持ち、名前を持たない。表示は `ty.display(&names)` で、`eml_hir` の `DisplayNames` を引く。同じ名前の型やエフェクトを、名前を定義するモジュールが2つ以上あるときだけモジュール名で修飾して表示するためである。表がモジュールの文脈によらないので、HIR の診断、型検査の診断、`dump`、`pretty` が同じ表を引ける

## `eml_core_ir`、`eml_runtime`、`eml_interp` の内部

- Core IR、パス、インタプリタの設計は [Core IR とインタプリタ](../spec/core-ir.md)、ヒープと RC は [ランタイム](../spec/runtime.md) が定める。`Rc` を使わず `Arc<Program>` で共有する規約は、core-ir.md の「実行時の規約」にある
- パスの順番は `pipeline.rs` だけが持つ。join point の `captures` はパスの間でつねに正しく保つ。RC の命令を入れる前のパスの後では生存解析で埋め直してから範囲を検査し、Perceus の後では所有権まで検査する。verifier はデバッグビルドだけでかける。`compact` が見つけた木の誤りは、共有された式が入れ子になるとたどる時間が指数的に増えるので、どのビルドでも報告する
- 変換 (`translate/`) は入口の関数から届く関数だけを変換する。`mod.rs` は値の渡し先と join point の骨組み、`expr.rs` は式ごとの変換、`program.rs` は関数の表と包む関数、`types.rs` は型から決まる性質と intrinsic の表、`pattern.rs` は決定木を持つ
- 既知の呼ばれる式への呼び出しは、種類 (`Callee`) ごとに、引数の数、足りないときの包む関数、ちょうどのときの命令だけを決める。足りない・ちょうど・余るの場合分けは `saturate` の1か所で行う
- 変数が boxed かどうかは、型から `boxed` の1か所で決める。intrinsic の名前と実装の対応を置くのは `INTRINSICS` だけで、`eml_core_ir` の単体テストが、Prelude の intrinsic のすべてが表に行を持ち、表の行がすべて Prelude にあることを確かめる。本体のある Prelude の関数は表に持たず、普通の関数として変換する
- 変換は、渡された入口の関数を `()` で呼ぶ関数 `entry$<名前>` を足す。入口の関数を引数で受け取るのは、REPL で `main` の代わりにその回の式から作った関数を渡せるようにするためである
- 持ち上げた関数の名前は、ラムダが `外側の名前$lambdaN`、handle の本体と節が `外側の名前$handleN` (と `$操作名`、`$return`)、値として使う intrinsic、コンストラクタ、操作を包む関数が `builtin$`、`con$`、`op$` である。入口以外のモジュールの関数、`con$` と `op$` の後ろの名前、エフェクトの表の名前には、`モジュール名.` を付ける (`Report.Csv.parse`、`con$Report.Csv.Row`、`op$Prelude.open`)。ラムダと handle の関数は外側の名前を前に付けるので、同じく修飾される。`builtin$` は Prelude の intrinsic にしか作らないので付けない ([Core IR とインタプリタ](../spec/core-ir.md))。Core IR のエフェクトの番号は `eml_hir::Program::effects` の順である
- Core IR の関数は ANF の木をアリーナに置き、`CExprId` で参照する。継続のフレームが再開する位置を ID で持てるようにするため。アリーナはパスのたびに `compact` が作り直す。式、変数、join point の番号の払い出しは `builder.rs` の1か所に置く
- 子の式をたどる処理 (生存解析、Perceus、verifier、`simplify`、`pretty`、インタプリタ) は、visitor (`for_each_child` など) を通すか、`..` を使わずに欄をすべて名前で受ける `match` で式を分解する。子を持つ欄を IR に足したときに、たどる処理のすべてがコンパイルエラーになるようにするため
- 生存解析は、`Let` の連鎖と join point の本体の連なりが長くなりうるので、再帰ではなく作業の列でたどる。`jump` が届かない join point でも `captures` を `Join` まで生かしておく。verifier が `captures` を `Join` の位置で範囲にあることを求めるためである
- インタプリタの環境のスロットは値だけを持ち、読み出しはスロットを書き換えない。参照の所有は Core IR の命令が表し、verifier が釣り合いを確かめる。`Payload` は `Clone` を導出しないので、`ObjRef` を `dup` せずに複製できない
- `eml_interp` は関心ごとにファイル (`machine.rs`、`effects.rs`、`prim.rs`、`io.rs`、`error.rs`) を分け、`Machine` の処理を `impl` ごとに書く。フレームに積む戻り位置は `ReturnPoint` と呼び、継続の `resume` と名前が重ならないようにする

## ソースファイルと位置

- `FileId` と `SourceFiles` (`FileId` → パスとテキスト) は `eml_diagnostics` に置く。`SourceFiles` には読み込みの段が読んだすべてのファイルが入る。`FileId` はモジュールの番号と同じ順である
- `TextRange` は `text-size` crate を直接使う (`rowan` が再公開しているものと同じ型)。`eml_diagnostics` は `rowan` に依存しない
- `SourceFiles::add` は、テキストの先頭の BOM を取り除いてから保存する ([字句](../spec/lexical.md))。lexer、レイアウト段、表示は BOM を扱わない
- 将来クエリ化するときは、`SourceFiles` を salsa の入力に置き換え、必要なら別の crate に切り出す

## CLI と lib API

- 引数の解析は `clap` (derive) を使う
- `eml check <file>` は、診断を stderr に表示する。`eml run <file>` は、診断 (警告を含む) を先に stderr に表示し、エラーがなければ実行する。警告がプログラムの出力の後に出ないようにするため
- `eml run --debug-heap` は、RC のリーク検出と解放済みアクセスの検出を有効にする
- 終了コードは、0 = 成功、1 = 診断のエラーあり、または実行時エラー、2 = 使い方の誤り (引数の誤り、入口のファイルが読めない) とする。2 は、clap が引数の誤りで返す値に合わせた。import したモジュールが見つからないことは診断 (E1026) なので 1 である

`eml_cli` の lib は次の API を公開する。UI テストはこれをプロセス内で呼ぶ。

- `Session::load(entry_path, entry_text, &dyn ModuleSource) -> Session` は、入口の表示のパスと本文を受け取り、読み込みの段で Prelude と import したモジュールを読む。`Session` は1回の検査や実行で読むソースの集まりである。入口のファイルは `main.rs` が読み、読めなければ終了コード 2 にする。ファイルシステムから読む `ModuleSource` は `FsProvider` で、パスの各段の名前がディレクトリの一覧と大文字小文字まで一致することを確かめる。macOS のように大文字小文字を区別しないファイルシステムで、`import Report.Csv` が `report/Csv.em` に当たらないようにするためである。実行を始める `main` は入口のモジュールからだけ探し (`hir::Program::main`)、Prelude には置かない
- `Session::check() -> Vec<Diagnostic>`。`check` と `compile` は、診断を `sort_diagnostics` で並べて返す。各段階は診断の順を約束しない
- `Session::compile() -> Compiled`。`Compiled` は、検査で出た診断 (警告を含む) と、エラーがなければ `Program` を持つ。`main` がないこと (E2003) は `compile` だけが検査する ([型と Kind](../spec/types.md) の「推論」)
- `execute(Arc<Program>, &RunConfig, stdout: OutputSink) -> Result<(), RuntimeError>`

検査と実行を別の関数に分けるのは、呼び出し側が実行の前に診断を表示できるようにするためである。CLI と UI テストは、`compile` の診断を表示してから `execute` を呼ぶ。

`RunConfig` は `eml_interp` で定義し、`eml_cli` が再公開する。`#[non_exhaustive]` にしてあり、`RunConfig::default()` から作ってフィールドを代入する。`OutputSink` と `RunConfig` に将来足すものは [ランタイム](../spec/runtime.md) の「実行の API」にある。

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

# コンパイラの構成

位置づけ: 手引き。

eml の処理系をどの crate に分け、各段階がどんな規律に従うかを説明する。言語の意味は [spec/](../spec/) の各文書が定める。この文書は、それを実装する側の構造と、構造を選んだ理由を扱う。型や関数の細部はコードとそのコメントにある。

## 全体の方針

- 処理系はバッチ型のパイプラインである。各段階を純粋な関数とし、Arena と ID で表現して、後でクエリ化 (salsa など) できるようにしておく
- 実行系は、型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、中間値に名前を付け、RC とエフェクトを明示する) と CEK 風のインタプリタからなる。ネイティブ化では、Perceus の後の Core IR をコード生成のバックエンドに渡す。バックエンドは未定である。ヒープと RC は `eml_runtime` に分離する
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
- どの item も Core IR の関数にして、書き換えのパスで命令に戻し、使わない関数を取り除くパスで消すこと。変換が満ちた呼び出しをすでに命令にしているので、飽和の場合分けを `saturate` にまとめれば足りる
- `Bool` のタグをインタプリタまで運ぶこと。タグは Prelude の宣言の順で決まるので、定数 `FALSE` と `TRUE` をテストで照らし合わせれば足りる

## リポジトリ

```
eml/
  Cargo.toml            # [workspace] members = ["crates/*"], resolver = "3"
  crates/
    eml_diagnostics/
    eml_extern/         # 依存を持たない extern の表
    eml_syntax/
    eml_hir/
    eml_types/
    eml_core_ir/
    eml_runtime/
    eml_interp/
    eml_cli/            # lib + バイナリ (バイナリ名 `eml`)
    eml_test_support/   # 開発専用。結合テストのパイプラインと診断の文字列
  std/                  # 標準ライブラリ (Prelude.em、Fs.em)。eml_hir がバイナリに埋め込む
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
eml_cli          check / run コマンド。パイプラインを組む唯一の場所 (Session)。テストから呼べる lib API を公開する
eml_interp       Core IR を CEK 機械で実行する
eml_runtime      オブジェクトのモデル、ヒープ、参照カウント、debug_heap の検査、OutputSink
eml_core_ir      型付き HIR → Core IR (基本ブロックの列)。縮約と dup/decref の挿入のパス
eml_types        Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査
eml_hir          モジュールの読み込み、CST → HIR の変換、名前解決、脱糖
eml_syntax       SyntaxKind、lexer、レイアウト段、イベント方式のパーサ、rowan、型付き AST ラッパ
eml_diagnostics  Diagnostic 型、FileId と SourceFiles、行と列、ariadne による表示
eml_extern       extern の表 (型、エフェクト、関数)。依存を持たず、HIR、型検査、Core IR、インタプリタが引く
```

- 段階の連なりに `eml_runtime` は入らず、`eml_interp` だけが依存する
- `eml_diagnostics` は、診断を出す crate のすべてから使う。今は `eml_core_ir`、`eml_runtime`、`eml_interp` が診断を出さず、`eml_extern` は何にも依存しない。`eml_core_ir` だけは、extern の呼び出しの位置を行と列にするために `SourceFiles` を使うので依存する。残りの3つは依存しない
- `eml_extern` は表だけを持つ。`eml_hir`、`eml_types`、`eml_core_ir`、`eml_interp` が引く。`eml_extern` を一番下に置くのは、どの段階も実装を表の行で引くようにして、名前の文字列で実装を探す処理をなくすためである
- `eml_cli` は `eml_diagnostics` と `eml_hir` に常に依存し、下流の段階を feature で足す。`types` は `eml_types`、`core` は `eml_core_ir`、`run` は `eml_interp` と `eml_runtime` を足す。既定は `run` で、バイナリ `eml` は `run` がなければ作らない (`required-features`)。ワークスペースの `eml_cli` の依存は既定の feature を外してあり、依存する側が段階を選ぶ
- `eml_test_support` は開発専用の crate で、パイプラインに入らない。各 crate の結合テストが dev-dependency として使う。パイプラインは `eml_cli::Session` で組み、段階を feature (`hir` < `types` < `core` < `run`) で選ぶ。各 feature は、`eml_cli` の同じ段階までの feature を有効にする。各 crate は自分の段階までを有効にする。破壊的な変更の途中で下流の crate がまだ組み立たなくても、変更している段階のテストを流せるようにするため ([テスト戦略](testing.md))

## 各段階の規律

- 各段階は `fn stage(input: &In) -> (Out, Vec<Diagnostic>)` の形の純粋な関数にする。グローバルな可変状態は持たない。例外は `eml_core_ir` で、診断のエラーがないプログラムだけを受け取り、`Program` を返す (下の「エラーが出ても止まらない」)
- HIR 以降は ID で参照する (`la-arena`)。型などの解析結果は、`ExprId → Type` のような別テーブルに置く
- HIR の各ノードは、元の構文の範囲 (`TextRange`) を持つ。演算子の列を組み直した部分式のように、対応する構文ノードのない式があるため
- HIR が持つ位置は、ソースに書かれた名前やノードの位置に限る。行頭や字下げのような、そこから導ける見た目の情報は持たず、要る段階がソースのテキストから求める。E3003 の fix の字下げがその例である (下の「`eml_types` の内部」)
- 型付き HIR は HIR を複製せず、宣言ごとと本体ごとの結果の別テーブル (`TypedProgram`) だけを持つ。本体ごとの結果には、式、局所変数、パターンの型と、参照ごとの具体化の表 (`instantiations`) がある。そのため `eml_core_ir` は HIR と `TypedProgram` の両方を受け取る

現在の各段階の入口は次のとおり。

| crate | 関数 |
|---|---|
| `eml_syntax` | `parse(FileId, &str) -> (Parse, Vec<Diagnostic>)` |
| `eml_hir` | `load(&str, &str, &dyn ModuleSource) -> (Loaded, Vec<Diagnostic>)`、`def_map(&[LoadedModule]) -> (DefMap, Vec<Diagnostic>)`、`lower(&DefMap, &[LoadedModule]) -> (Program, Vec<Diagnostic>)` の順に呼ぶ。`load` は入口の表示のパスと本文を受け取り、ファイルごとに `item_tree(FileId, &Parse) -> (ItemTree, Vec<Diagnostic>)` を呼ぶ。モジュールの並びは、0番目が Prelude、1番目が入口のモジュールで、その後に Prelude を除く標準ライブラリのモジュール、import で見つけた順のモジュールが続く |
| `eml_types` | `check(&Program, &SourceFiles) -> (TypedProgram, Vec<Diagnostic>)`。`SourceFiles` は E3003 の fix の字下げを求めるのに使う |
| `eml_core_ir` | `lower(&hir::Program, &TypedProgram, FunctionId, &SourceFiles) -> Program`。途中のパスで止める `lower_until` もある。入口の関数は呼ぶ側が渡す。`SourceFiles` は extern の呼び出しの位置を行と列にするのに使う |
| `eml_interp` | `run(&Program, &RunConfig, &OutputSink) -> Result<RunStats, RuntimeError>` |

## エラーが出ても止まらない

- `eml_cli::Session` の段階のメソッド (`def_map`、`lower`、`check`、`compile`) は、エラーがあっても途中で止めずに、その段階までのすべての段階を実行し、診断を集める。1回の実行で、独立した複数のエラーを報告するため
- パーサは壊れた入力でも `ERROR` ノードを作って回復し、必ず `SOURCE_FILE` の木を作ってパニックしない。名前解決と型推論は、誤りの場所に `Error` 型を入れて診断の連鎖を抑える。規則は下の「構文解析の回復」と「名前解決の回復」、[診断](../spec/diagnostics.md) の「連鎖する診断の抑止」にある
- Core IR は、診断のエラーがないプログラムだけを受け取る。`compile` は、エラーがあれば Core IR を作らない。型付きで正しい入力を前提にできるので、Core IR への変換は診断を返さない

### 構文解析の回復

- 列 0 の `SEP` (トップレベルの項目の区切り) を、最も強い回復の同期点にする
- ブロックの中では、`SEP` と `CLOSE` を同期点にする
- parser の診断が仮想トークンかファイルの終わりの位置で出る場合は、直前の実トークンの直後の空の範囲を指す。仮想トークンは次の行の先頭にあり、そこを指すと誤りのない行を指してしまうためである。E0009 で回復のために作る空のブロックも、同じ位置に置く
- `match` / `handle` の `with` の後に E0009 を出し、次の行からが `|` で始まる行の並びなら、parser はそれらを枝として読み飛ばし、追加の診断を出さない。E0009 には、枝を `match` / `handle` より深く字下げするよう促す help を付ける
- 行末の `->` で E0009 を出した次の行もまた `->` で終わり、今のブロックが `->` で開いたものなら、その `->` の E0009 は出さない。揃えた複数行のシグネチャ (`f : A ->` の次の行から `B ->`、`C ->` と同じ列に並べたもの) の誤りを1件にするためである。ブロックを開いたトークンを見るのは、トップレベルや `where` の中で、続けて `->` で終わる別々の行の誤りの診断を抑えないためである
- [レイアウト規則](../spec/layout.md) の規則 5 の `;` のエラーは1件だけ出し、parser は対応する閉じ括弧まで読み飛ばす

### 名前解決の回復

- 見つからないモジュール (E1026) と予約したモジュール (E1030) の import も、修飾子と並びの名前をスコープに登録し、壊れた import の印を付ける。壊れた import は、どの名前にも「不明」として答える
- 名前を引いたとき、壊れていないモジュールの定義がちょうど1つ見つかれば、それを使う。壊れた import は E1028 の候補に数えない。定義が1つも見つからず、合流した修飾子か競合する import のどれかが壊れていれば、診断を出さずに誤りの式にする。E1031、E1001、E1028 が連鎖しないようにするためである
- `import Missing (show_int)` は、[モジュールと名前解決](../spec/modules.md) の「名前の解決」の手順3の名前として Prelude の `show_int` を隠す。その `show_int` を使った位置は、診断を出さずに誤りの式になる
- 部品の分からない `T(..)` か `E(..)` (壊れた import のものか、モジュールにない型やエフェクトのもの) を並びに持つモジュールでは、修飾しない値の名前がどこにも見つからなければ、診断を出さずに誤りにする。どの名前も、分からない部品の1つでありうるためである
- パスの後ろ (別名と並びを含む) に構文の誤りがある import (`import Report.csv`、`import Report as`、閉じていない並び) は、構文の誤りだけを報告し、ファイルを読まずに壊れた import として扱う。読めたところまでで import を解釈すると、書いたつもりと違うモジュールや取り込み方を黙って選ぶためである。並びの `(:+)` (E0011) はその名前だけを落とす誤りなので、import は壊れない
- import の並びで報告した名前 (定義がない名前と `pub` でない名前) を本体で使った位置は、診断を出さずに誤りにする
- 宣言の後の import (E0011) も、ほかの import と同じく読み込み、スコープに登録する。報告するのは E0011 だけである
- handler の節の先頭の名前が曖昧 (E1028) なときや、壊れた import から来たときは、その handler の節のない操作 (E1013) を報告しない。その節がどの操作を指すか分からず、書いてある節を「ない」と報告しうるためである
- 曖昧な演算子 (E1028) と、壊れた import から来た演算子を含む演算子の列は、fixity で組み直さず、診断を出さずに誤りの式にする。既定の `infixl 9` で組み直すと、E1006 が連鎖しうるためである
- 循環 (E1027) を報告した後も、名前解決と型検査を続ける。item 単位の検査なので、循環があっても下流の段階は困らない

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

- lexer は logos で単純なトークンを切り出し、文字列、コメント、演算子の分類などを手書きの層で扱う。最終的には、文字列、補間、コマンドリテラル、入れ子のブロックコメントの各モードをスタックで持つ層にし、文字列をトークン列 ([文法](../spec/grammar.md) の `string`) に分ける。今は、補間を含む文字列などをそれぞれ1つのトークンにしている。トークン列への分割は、これらを実装する S4 とコマンドリテラルの段 で行う
- レイアウト段は、幅 0 の仮想トークン `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE` を挿入する。規則は [レイアウト規則](../spec/layout.md) が定める。パーサは仮想トークンを読んでも `Event::Token` を出さないので、仮想トークンは木に入らず、CST は lossless のまま
- `grammar/` は [文法](../spec/grammar.md) に従う。演算子の列は `OP_SEQ` ノードに平たく並べ、木への組み直しは HIR で行う
- トークンの種類 (`EOF` まで) は 128 未満に収める。`TokenSet` が `u128` のビット集合であるため
- `eml_hir` は、型付き AST の API、識別子や演算子の `SyntaxToken`、`eml_syntax` が再公開する `AstPtr` だけを使う。CST の木の構造 (`.syntax()`) には、`AstPtr` を解決する根を作るときのほかは触れず、`rowan` に依存しない
- E0004 (未対応) は、原則としてパーサではなく HIR が出す。どの層が出すかの例外は [実装の現在地](status.md) の「未対応の構文と E0004」にある。E0004 はどの段階でも同じ意味なので、番号とラベルは `eml_diagnostics` に置く

## `eml_hir` で行う脱糖と検査

HIR への変換では、名前解決に加えて、名前の重複と未定義の名前の検査、シグネチャと等式の対応の検査、spec が HIR に割り当てた脱糖と検査を行う。式の脱糖と検査の一覧は [式](../spec/expressions.md) の「HIR で行う脱糖と検査」に、宣言とレコードに関するものは [宣言](../spec/declarations.md) と [直積型とレコード](../spec/records.md) にある。実装上の判断は下の「`eml_hir` の内部」に書く。

## `eml_hir` の内部

- 読み込みの段 (`load.rs`) は、入口の本文を parse して `ItemTree` を作り、import を宣言の順に幅優先でたどって読む。各モジュール (`LoadedModule`) は、構文木 (`parse: eml_syntax::Parse`) と `ItemTree` を持つ。`Parse` の中身は green node なので、読み込みの結果と `eml_cli::Session` は `Send + Sync` である。ファイルの読み方は `ModuleSource` の trait で受け取り、IO は実装 (`eml_cli` の `FsProvider`、`eml_test_support` の `MemorySource`) だけが持つ。同じ読み方を渡せば同じ結果を返すので、段階は純粋な関数のままである。依存先の表示のパスは、入口の表示のパスのディレクトリに根からの相対パスをつないだものにする。標準ライブラリのモジュールの表示のパスは `<std>/Fs.em` のようにする。E1026 と E1030 はこの段で報告する。パスの後ろに構文の誤りがある import は、ファイルを読まずに壊れた import にする (上の「名前解決の回復」)。標準ライブラリは、リポジトリの `std/` のファイルを `eml_hir::STD` (ファイル名と本文の組の並び。最初が Prelude) として `include_str!` で埋め込む。`load` は `STD` を使い、`load_with_std` は標準ライブラリの並びを引数で受け取る。標準ライブラリを差し替えるテストが、`eml_cli::Session::load_with_std` を通して使う (`eml_test_support::lower_with_std` と `check_with_std`)。並びは `Prelude.em` と本物の `Fs.em` (または同じ extern の宣言を持つもの) を含まなければならない。extern の索引が両方を引き、足りなければ panic するためである。モジュールは出どころ (`ModuleOrigin::User` か標準ライブラリ) を持つ。`import Fs` は、ユーザーの根にファイルが見つからないときだけ `std/` を探し、`import Std.X` はいつも `std/` を探す ([モジュールと名前解決](../spec/modules.md) の「標準ライブラリ」)。標準ライブラリのモジュールの import は `std/` だけを探す
- トップレベルの名前の解決は3つの段階に分ける。`item_tree` はファイルごとに宣言を集め、シグネチャと等式を名前で1つの関数にまとめ、名前を解決しなくても判定できる誤り (型引数の重複 E1003 を含む) を出す。`def_map` は読み込んだモジュールの列から、item の ID、モジュールごとの名前の表、import のスコープ (修飾子からモジュールの列への表と、修飾なしにした名前の表)、定義に付く fixity、lang item、extern の索引、表示名の表 `DisplayNames` を作り、import の循環 (E1027) と import の並びを検査する。`lower` は `DefMap` で名前を引き、item を `DefMap` と同じ局所の番号の順にアリーナへ置く
- `ItemTree` は rowan の red node を持たない。名前解決の前に決まる宣言の情報 (名前とその位置、`extern` のキーワード、重複を除いた型引数、コンストラクタがあるか) は、木を作るときに取り出す。型の変換のように resolver の要るもの (シグネチャ、等式、コンストラクタ、操作) だけを `AstPtr` で指す。`lower` は各モジュールの根を1回だけ作り、ポインタを解決して item と本体の変換に使う。`item_tree` が `&ast::SourceFile` ではなく `&Parse` を受け取るのは、ポインタと、それを解決する木の組を取り違えないためである
- 名前を変換より先にすべて集めるので、宣言の順によらず、`data` どうしの相互再帰や、後ろで宣言したエフェクトへの参照ができる
- HIR の出力は `Program` で、読み込みの段が読んだモジュールの列と、表示名の表を持つ。モジュールの番号は、Prelude が 0、入口が 1、Prelude を除く標準ライブラリのモジュールが埋め込んだ並びの順に 2 から、ユーザーの import を見つけた順の依存先がその後で、順が決まるので診断の並びが安定する。item の ID はモジュールと局所の番号の組で、プログラム全体で一意である。各モジュールは item のアリーナと関数の本体を分けて持つ。本体を書き換えても item が変わらないようにするため。本体は関数ごとの `Body` で、後でクエリ化したときに関数単位で再計算できるようにする (rust-analyzer と同じ分け方)。型の注釈も、シグネチャのものと本体のものを分けて置き、本体を書き換えてもシグネチャが変わらないようにする
- `DefMap` は、モジュールごとに値と型の2つの名前空間を持ち、名前ごとに定義をソースの順にすべて持つ。重複の扱いは [modules.md](../spec/modules.md) の「名前空間」、引く順は「名前の解決」が定める。重複した `data` のコンストラクタと `effect` の操作は使えない印を持ち、それらへの参照は診断を重ねずに `Missing` にする。名前を引いた結果は `Resolved` である。診断を出さずに誤りにする名前は `Silent(Silence)` になり、`Silence::Unusable` は重複した宣言の部品、`Silence::Broken` は壊れた import から来た名前、import の並びで報告済みの名前、不明な `T(..)` の部分である。診断を出さない点はどちらも同じで、違いは fixity の求め方にだけ現れる (下の fixity の項)
- 値の item は `ValueItem` (関数、操作、コンストラクタ) の1つの形で表す。式の参照 (`Res::Item`)、型検査の宣言ごとの表 (`TypedProgram::decls`) も同じキーを使う
- item の変換は、モジュールごとの文脈 `ItemLowering` (ファイル、モジュール、`DefMap`、resolver、構文木の根、診断の出し先) のメソッドで行う。`ItemTree` の `AstPtr` は、この構文木の根で解決する。ファイルと診断の出し先の組だけを束ねる型は作らない。受け渡しの多い組の半分しかまとまらず、複数のファイルを扱う箇所 (`check_cycles`、`Loader`) に合わないためである
- 組み込みは、`extern` と書いた宣言と `eml_extern` の表の行の組である。宣言は標準ライブラリ (`std/Prelude.em`、`std/Fs.em`) に eml のソースで書き、`SourceFiles` に登録した普通のファイルとして、入口のファイルとは別のモジュールに変換する。`extern` のシグネチャは `FunctionKind::Extern(Option<Extern>)`、`extern data` は `TypeDefKind::Extern(Option<ExternType>)`、`extern effect` は `EffectKind::Extern(Option<ExternEffect>)` になる。`Some` の行は、宣言をモジュールの正式な名前で修飾した名前 (`Prelude.println`、`Std.Fs.open`) を表で引いて決める。ユーザーのモジュールの `extern` は E1033 にして `None` を持たせ、`None` を持つ宣言のあるプログラムは Core IR に届かない。E1005 と E1025 は、`extern` でない宣言にはどのモジュールでも出す
- `def_map` は、処理系が役割で引く item を2つの形で持つ。extern でないもの (`Bool` とそのコンストラクタ、`&&`、`||`) は `LangItems`、extern の型、`IO`、`negate` は `ExternIndex` で、標準ライブラリのモジュールから正式な名前で引いて作る。`def_map` は lowering の前に `Unit` を使い、lowering は `negate` を使うので、索引は `def_map` で作る。足りなければ名前を添えて panic する。これは `std/` が壊れているときだけ起こり、表と `std/` を照らし合わせる結合テストが防ぐ。`==` と `!=` は関数の種類 (`Eq` / `Ne` の行) で見分ける
- handler の節の先頭の名前は、まず操作として引く。見つからないときだけ、同じ段を extern の関数だけに絞って引き直し、extern のエフェクトを起こす関数なら E1009、そうでなければ E1001 にする。ユーザーが定義した同名の関数が、Prelude の extern の関数を隠さないようにするためである
- シグネチャか等式のない関数も `Function` として残し、呼び出し側で名前の誤りを連鎖させない。引数の個数の違う等式 (E1020) は `match` の枝に入れず、本体に誤りの印を立てる
- 等式が2つ以上ある関数は、引数の位置ごとの隠れた局所変数に対する `match` に脱糖する。この `match` は由来 (`MatchSource::Equations`) を持ち、網羅性の検査が等式として報告する。等式が1つの関数と引数のない値は、脱糖しない
- fixity は定義に付く ([宣言](../spec/declarations.md) の「fixity」)。`DefMap` が item をすべて集めてから付けるので、宣言の位置は問わない。fixity は解決の結果から `Resolver::fixity_of` で求める。見つかった item はその fixity を使い、宣言がないときと、別のモジュールの `pub` でない宣言しかないときは既定の fixity を使う。`Silent(Unusable)`、見つからない名前、`pub` でない名前、不明な修飾子は既定の fixity で組み直す。`Silent(Broken)` と曖昧な名前を含む列は組み直さない (上の「名前解決の回復」)。演算子の列とセクションは、各演算子を1回だけ値として解決し、その結果を fixity と変換の両方に使う。中置のパターンも、fixity は各演算子を1回だけ値として解決して求める。組み直しが決まらない列では、ほかの演算子の E1001 も出さない。セクションの先読み (`looser_operator`) は被演算子を変換する前に被演算子の中の演算子の fixity が要るので、名前から引く `Resolver::fixity` を使う
- 式とパターンの組み直しは同じ fixity を引く。パターンの演算子は `constructor()` で引き直すが、`:` で始まる演算子は文法上コンストラクタにしかならないので、式として引いても同じ item と fixity になる
- `&&` と `||` は、解決した先が lang item のときだけ `if` に脱糖する。`|>` と `<|` は脱糖せず、Prelude の関数の普通の呼び出しにする。`let` に脱糖すると左辺が推論になり、関数の引数の型を期待した診断 (E2001 が `x |> f` の `f` を指す) が失われるため
- 呼び出しの評価の順は `eval.rs` の `call_steps` が決める ([関数適用](../spec/expressions.md))。持ち越しのパス (`eml_types`) はこの手順を逆に、Core IR の変換 (`eml_core_ir`) は順にたどる。順の組み立てを1か所に置くのは、2か所で組むとずれたときに持ち越し規則が実行と食い違うためである。値の判定 (`is_value`) と引数のまとめ方 (`known_arity`) も、両方の段階がここから引く
- `Body` の走査関数 (`walk_child_exprs`、`pat_bindings`、`captures`) は、式やパターンの種類を足す段階が直す。誤った handler の節は診断を出して節に入れず、扱うエフェクトが決まらなければ `effect` を `None` にして、型検査に診断を連鎖させない。誤りのあるセクションも、被演算子の名前の誤りを報告してから全体を `Missing` にする

## `eml_types` の内部

- 型は検査器の中の表に置いて ID で引き、型変数の束縛を辿って単一化する。row は「ラベルの並び + 末尾の row 変数」で、scoped labels の書き換えで単一化する。Kind の変数と `≤` の制約は束の上で最小解を求める。推論の対象は [型と Kind](../spec/types.md) が定める
- 推論の後に、型付き HIR の上で別のパスとして、線形性の検査 ([線形性](../spec/linearity.md)) と網羅性の検査 ([網羅性](../spec/exhaustiveness.md)、Maranget の usefulness) を行う
- 型検査は4つの純粋な関数に分ける。`Context::new` はプログラム全体の情報 (データ型の Kind、名前、多重度) を1回だけ作り、関数ごとの型の表はこれを借りる。表を作る費用を関数の大きさに比例させるため。段0はシグネチャを閉じた形 `Shape` にする。段1は関数ごとに新しい表を作り、全宣言の `Shape` だけを見て本体を検査し、Kind の制約は集めるだけにする。段2は呼び出しグラフの SCC ごとに Kind の問題を解く
- `Shape` は型の表を指さず、変数をスキームの中の番号で持つ。参照するたびに具体化し、段1は呼び出し先の制約を複写せずに Kind の具体化の記録 (`Instance`) を残して、段2が展開する。extern の関数、操作、コンストラクタの型も、ユーザーの関数と同じ経路で `Shape` に閉じる
- 段1は、Kind の具体化の記録とは別に、参照ごとの具体化の表 (`BodyTypes::instantiations`) を作る。式の中のトップレベルの item への参照 (`ExprKind::Path` の式) ごとに、宣言と型引数を持つ。S4 の組み込みのクラスの証拠と、後の型クラスと `Num` の解決は、この型引数から決める。型引数は `Shape::rigids` の順に並ぶ。関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の頭の型引数の順である。row 変数と Kind 変数は持たない。S4 の組み込みのクラスと、後の型クラスと `Num` の解決は型引数だけで決まるためである
- 表は本体の検査の間に型の表の変数のまま記録し、持ち越しのパスの後で `exprs` と同じく書き出す。後の文の単一化で決まる型 (`let` で束縛したラムダの引数など) を取り込むためである。最後まで決まらない型引数は `Flexible` になる。局所変数の参照は具体化しないので記録しない。パターンのコンストラクタと handler の節の操作も記録しない。S4 と型クラスの段でも、コンストラクタと操作は制約を持てず、解決が要らないためである。型ごとの解決が要る参照は、HIR で `ExprKind::Path` に脱糖してこの表に届ける。前置の `-` の `negate`、`&&` と `||` の `True` と `False`、セクションのラムダの中の演算子がそうである。S4 の補間の穴のように処理系が暗黙に持ち込む参照も同じ形に脱糖し、別の形を選ぶときは表のキーと記録の場所を広げる
- 段1は、呼び出しの `mask` の表 (`BodyTypes::masks`) も作る。キーは呼び出しの式と矢印の番号の組で、`k v` も普通の呼び出しとして同じ形で記録する。値は飛ばすエフェクトの並びで、ラベルの順のまま持つ。空の `mask` は記録しない。`mask` は row の包含 (`Table::include_row`) が返し、呼び出しの row を含める1か所 (`include_call_row`) で記録する。Core IR への変換は、この表から呼び出しごとの `mask` を作る ([Core IR とインタプリタ](../spec/core-ir.md))
- extern の型の Kind は `eml_extern` の表の行から決め、`Context` が extern のエフェクトの印 (`EffectKind::Extern`) を持つ。extern のエフェクトのラベルの重なりは、row を展開する `resolve_row` の1か所で落とし、`mask` には入れない。多重度は、操作がなくても `Once` にする。重なりを `resolve_row` の1か所で落とすのは、単一化、包含、表示がどれも展開した row を使うためである ([型と Kind](../spec/types.md) の「推論」)
- `==` と `!=` の比べ方は `equality` の1か所で決め、型検査と Core IR が同じ関数を使う。型検査は本体の検査の直後に、表の `==` と `!=` の記録の最初の型引数から比べ方を判定し、決まらなければ E2006 にする。この判定は使用回数のパスの `usage::reliable` より前に置く。E2006 を本体の誤りに数え、線形性の診断を連鎖させないためである。`equality` は `Int` と `String` を extern の索引から引く。Core IR は同じ型引数から、比べ方ごとの extern (`int_eq` など) を選ぶ
- 表の型引数は、rigid な型変数を名前だけで書き出す。そのため、関数の型変数と、同じ名前の handler の節の型変数は、表の上で区別できない。S4 で特殊化のときに型引数へ代入する前に、節の型変数を区別する表し方を決める。また Core IR は、extern を値として使う場所ごとに包む関数を作るが、包む関数は参照の型引数から extern の行を選ばない。型ごとの解決が要る extern は今はどれも `==` と `!=` で、HIR が演算子の参照とセクションをラムダに脱糖するので、いつも引数がそろって呼ばれる。extern に変換するメソッドを S4 で値として使えるようにするときに見直す
- 宣言の型は `TypedProgram::decls` の `DeclType` にある。結果を宣言ごとに持つのは、クエリ化したときに宣言ごとのクエリの結果として使い、REPL で前の入力の宣言を検査し直さずに使い回せるようにするためである
- `Table::export` は、後の段階と診断の文言の両方に渡す形を作る。書き出す `Type` は矢印の線形性を持たない。後の段階は線形性を読まず、持たせると本体の型を Kind を解くまで確定できなくなるためである。同じ理由で、`DeclType` の `Shape` と `KindScheme` は crate の外から読めない
- 持ち越しのパスは、使用回数のパスの直後に、同じ本体を Core IR の評価の順の逆にたどる。型検査器が記録した呼び出しごとの row を読み、持っている値と row の多重度を組にした制約を出す。この制約は、段2が線形性と多重度の両方の束を解いた後に検査する
- Kind の制約は由来 (`Provenance`) を持ち、型の表の「今の由来」から記録する。段2は破れた制約の由来を返し、`Suppressed` の制約は捨てる。そのため、報告したい制約には必ず由来を付ける。既定の由来 `Unattributed` は付け忘れを見つけるためのもので、それが破れたときと、宣言の型だけで作った制約が破れたときは処理系の誤りとして扱う
- 報告済みの誤りのある本体 (誤りの跡がある、型の誤りを報告済み、HIR の誤りがある) では、使用回数のパスも持ち越しのパスも由来を記録しない ([診断](../spec/diagnostics.md))
- `check` は `Program` と `SourceFiles` を受け取る。HIR のブロックは最後の文の開始位置 (`last_start`) だけを持ち、E3003 の fix は `check/report.rs` が作る。開始位置の行の先頭からそこまでのテキストが空白とタブだけなら、それをそのまま写した字下げで `drop x` の行を入れる。ほかの文字があれば (最後の文がその行の最初のトークンでなければ) fix を出さない。字下げを HIR に持たせないのは、HIR が見た目の情報を持たないためである (上の「各段階の規律」)
- 由来の位置は、ファイルと範囲の組 (`Span`) で持つ。具体化を通った由来は、呼んだ関数のある別のモジュール (Prelude など) の中を指しうるためである。報告は由来のファイルを使い、ファイル、範囲の順に並べる
- row の末尾には `Error` がある。未定義のエフェクトか解決できない row 変数の跡で、相手の側にしかないエフェクトを受け入れるが、自分の側の既知のエフェクトは受け入れない。綴り誤りの E1002 と無関係なエフェクトの誤りを隠さないためである。ラムダやシグネチャの矢印が壊れているときも、末尾が `Error` の row で本体を検査し、エフェクトの誤りを連鎖させない
- 型の走査は `Type`、`TyShape`、`ShapeTy` の `for_each_child` だけがたどり、そこでは `..` を使わず欄をすべて名前で受ける。欄を足したときに、occurs の検査などから漏れないようにするためである。内部の型の形は `TyShape`、矢印の線形性は `ArrowLin` と呼び、Kind と取り違えないようにする
- `Type::Con` と `EffectLabel` は ID だけを持ち、名前を持たない。表示は `ty.display(&names)` で、`eml_hir` の `DisplayNames` を引く。同じ名前の型やエフェクトを、名前を定義するモジュールが2つ以上あるときだけモジュール名で修飾して表示するためである。表がモジュールの文脈によらないので、HIR の診断、型検査の診断、`dump`、`pretty` が同じ表を引ける

## `eml_core_ir`、`eml_runtime`、`eml_interp` の内部

- Core IR の構成、評価と所有権の意味、パスの境界の不変条件は [Core IR とインタプリタ](../spec/core-ir.md) が、ヒープと RC は [ランタイム](../spec/runtime.md) が定める。translate の組み立てと継続のフレームは、下の「translate の組み立て」と「継続のフレーム」に書く。値とフレームに `Rc` と `RefCell` を使わない規約は、core-ir.md の「実行時の規約」にある
- Core IR の関数は、前向きの辺だけを持つ基本ブロックの列 (`CoreFn.blocks`) である。命令の位置はブロックの番号 (`BlockId`) と文の番号で表せるので、式のアリーナ、式ごとの ID、join point の索引は持たない。パスの間で古くなる情報 (生存集合など) も IR に書かず、要るパスがその場で求める
- パスの順番は `pipeline.rs` だけが持つ。verifier はデバッグビルドだけでかけ、translate と縮約の後は範囲の段 (`verify_scopes`)、Perceus の後は所有の段 (`verify`) を通す
- 変換 (`translate/`) は入口の関数から届く関数だけを変換する。`mod.rs` は式の値の渡し先 (出口) と条件の分かれ方、`builder.rs` はブロックの組み立て、`expr.rs` は式ごとの変換、`pattern.rs` は決定木と case-of-case、`program.rs` は関数の表と包む関数、`types.rs` は型から決まる Repr と、`==` と `!=` から比べ方ごとの extern の行を選ぶ関数を持つ。handler の節の `k` は、節ごとに HIR で使い方を調べ、2つの形のどちらかに変換する。使用がすべて引数をそろえた直接の呼び出しか `drop k` なら、クロージャを作らず、呼び出しを生の継続への `Call::Resume` にする (直接の形)。`k` を関数の値として使うなら、節の入口で生の継続を `cont$` か `cont$state` のクロージャに包んで `k` とし、呼び出しを `Call::Apply` にする (包む形)。`cont$` と `cont$state` は、使うときだけ1つずつ作る ([Core IR とインタプリタ](../spec/core-ir.md))
- 既知の呼ばれる式への呼び出しは、種類 (`Callee`) ごとに、引数の数、足りないときの包む関数、ちょうどのときの命令だけを決める。足りない・ちょうど・余るの場合分けは `saturate` の1か所で行う
- 変数の Repr は、型から `types.rs` の `repr` の1か所で決める。RC の対象 (`Repr::is_rc`) は `obj` と `tobj` である。extern の名前と実装の対応を置くのは `eml_extern` の表だけである。表の行がすべて `std/` にちょうど1回宣言されていることは、`eml_hir` の結合テストが確かめる。本体のある標準ライブラリの関数は表に持たず、普通の関数として変換する。extern の呼び出しは `Rhs::Extern` になり、引数の数は表から取る
- 変換は、渡された入口の関数を `()` で呼ぶ関数 `entry$<名前>` を足す。入口の関数を引数で受け取るのは、REPL で `main` の代わりにその回の式から作った関数を渡せるようにするためである
- 持ち上げた関数の名前は、ラムダが `外側の名前$lambdaN`、handle の本体と節が `外側の名前$handleN` (と `$操作名`、`$return`)、値として使う extern を包む関数が `外側の名前$externN`、コンストラクタと操作を包む関数が `con$` と `op$`、継続を包む関数が `cont$` (状態のない handler 用) と `cont$state` (状態のある handler 用) である。入口以外のモジュールの関数、`con$` と `op$` の後ろの名前、エフェクトの表の名前には、`モジュール名.` を付ける (`Report.Csv.parse`、`con$Report.Csv.Row`)。ラムダ、handle、extern を包む関数は外側の名前を前に付けるので、同じく修飾される ([Core IR とインタプリタ](../spec/core-ir.md))。N は、本体を変換する前に HIR を1回たどって、式の ID の順に振る (`numbering`)。変換の順で番号が変わらないようにするためである。Core IR のエフェクトの番号は `eml_hir::Program::effects` の順から、extern のエフェクトを飛ばして数える。この数え方は `effect_index` と `effect_table` が同じ補助関数を使う。`IO` は Prelude の最初のエフェクトなので、表からだけ外すと、ユーザーのエフェクトの番号がすべて1つずれるためである
- extern を値として使うときは、参照する場所ごとに包む関数を作り、その `extern` 命令に参照した場所の位置を持たせる。extern ごとに1つの共有の包みにすると、実行時エラーが包む関数の中を指し、どこで使った extern かが分からなくなるためである
- `lower` は `SourceFiles` を受け取り、extern の呼び出しの位置 (`Loc`) を `SourceFiles::line_col` で行と列にする。`Program.files` には、位置に使った表示用のパスを使った順に入れる。`eml_core_ir` が `eml_diagnostics` に依存するのはこのためで、診断は出さない。`line_col` は、`SourceFiles::add` が作った行の先頭の表を二分探索し、その行の中だけ文字を数える
- 文と終端の値と行き先をたどる処理 (生存解析、Perceus、verifier、縮約、`pretty`、インタプリタ) は、`Stmt::for_each_atom`、`Term::successors` などの visitor を通すか、`..` を使わずに欄をすべて名前で受ける `match` で分解する。欄を IR に足したときに、たどる処理のすべてがコンパイルエラーになるようにするため
- パス、verifier、`pretty`、`parse`、インタプリタは、ブロックと文の並びをループでたどり、IR の大きさに比例して再帰しない。生存解析は、ブロックを後ろからたどる1回のループで、ブロックごとの入口の生存集合を側の表 (`liveness::live_in`) に返す。辺がすべて前向きなので、不動点の計算は要らない
- 縮約 (`contract.rs`) は使われない純粋な `let` を消すだけのパスである。S3b-1 までの `simplify` の書き換え (合流の畳み込み、値が分かっているコンストラクタへの `switch`、case-of-case、末尾呼び出し) は、translate が組み立てるときに行う。`simplify` は書き換えのたびに枝の部分木を置き換えたので、長い `else if` の連鎖で2乗の時間がかかり、続きを `switch` の枝の中へ移して入れ子を深くした
- インタプリタの環境 (`Env`) のスロットは値だけを持ち、読み出しはスロットを書き換えない。参照の所有は Core IR の命令が表し、verifier が釣り合いを確かめる。`Payload` は `Clone` を導出しないので、`ObjRef` を `dup` せずに複製できない
- `eml_interp` は関心ごとにファイル (`machine.rs`、`runtime.rs`、`effects.rs`、`externs.rs`、`error.rs`) を分ける。`machine.rs` は IR の形に依る部分 (制御 (関数、ブロック、文の番号)、`jump` と `switch` と `unpack`、関数への入り方、再開の番地の作り方と読み方) だけを持つ。IR の形に依らない部分 (ヒープ、継続、handler の連鎖、不死のリテラル、関数値の適用、戻り、extern、エフェクト) は `runtime.rs` の `Runtime` が持ち、呼び出しと戻りの行き先を `Transfer` で機械に返す。将来のバイトコード VM が `Runtime` を使い回せるようにするためである。`externs.rs` の `call_extern` が `Extern` のすべての行を、既定の腕のない `match` で実行する (行を足して実装を忘れるとコンパイルが通らない)
- 戻りのフレーム (`Frame::Return`) は、関数の番号、再開の番地 (`resume: u64`)、退避した値 (`saved`)、次のフレームを持つ。再開の番地と `saved` の鍵 (スロットの番号) の意味は実行する側が決め、`eml_runtime` は中身を解釈しない。インタプリタは、呼び出しの文のブロックの番号を上位の32ビットに、文の番号を下位の32ビットに入れる。戻ったら、その文が `Rhs::Call` の `let` であることを確かめ、その変数に結果を入れて次の文から続ける。番地を `u64` にしたのは、将来のバイトコード VM が自分の `pc` をそのまま入れられるようにするためである
- 機械は、位置を持つ `Rhs::Extern` の `call_extern` が返した誤りにだけ、`Program.files` のパスと行と列 (`SourceLocation`) を付ける ([Core IR とインタプリタ](../spec/core-ir.md) の「実行時エラー」)
- `RunStats` の `handler_visits` は `find_handler` が数え、`string_bytes_copied` はヒープが数える (`Heap::string_bytes_written`)。文字列の物体の中身を書くのは、ヒープの確保、写し、`append_str` だけで、ヒープはそこで書いた長さを足す。`alloc_immortal` が作る不死のリテラルは数えない

### translate の組み立て

- 関数のブロックの列は、`builder.rs` の `FnBuilder` が前から組み立てる。`FnBuilder` は、文を足す (`emit`)、終端を置いてブロックを閉じる (`terminate`)、ラベルを作る (`new_label`)、開いたままのブロックに戻る (`reopen`) を持ち、終端のないブロックを複数開いたまま持てる。ブロックは作った順に番号が付く。`switch` の行き先は `switch` を置くときに作り、ラベルのブロックはそこへ向かう辺がすべて出そろってから作るので、作った順は前向きの辺だけになる。`finish` は、ブロックをたたんだ後に番号を詰める
- ラベルは、そこへ向かう開いたままのブロックの一覧を持つ。ラベルを持つ式を変換し終えたら、一覧の数で扱いを決める。0本なら本体を変換しない。1本ならそのブロックに戻り、本体をそこで変換する (引数は変数の対応で渡す)。2本以上なら、各ブロックを `jump` で閉じ、ラベルのブロックを置いて本体を変換する。前もってラベルの使用を数えないので、HIR を1回たどるだけで済む。`switch` の行き先が2本以上から入るラベルへ向かうときは、`jump` だけを持つ辺のブロックを挟む (R3)
- 式の値の渡し先 (出口) は、`Return`、`Jump(ラベル)`、`Scrutinize(文脈)` の3つである。値の `if` と `match` は、続きをラベルにして `Jump` で変換する。`Scrutinize` は、値がそのまま `match` に入る出口である。文脈 (`Ctx`) は、行 (パターン)、枝ごとのラベル、決まらない出口の一覧、枝の本体の出口を持つ。条件は `Bool` の文脈 (`True` と `False` のラベル) の `Scrutinize` で変換するので、`&&`、`||`、入れ子の `if`、条件の位置の `match` は `Bool` の値を作らない
- 値は出現 (`Occ`) で表す。出現は、値のアトムか、タグと出現のフィールドを持つ既知のコンストラクタである。タプルはタグ 0 のコンストラクタである。`let x = con ..` で束縛した変数は、`FnLowering.cons` が変数から引数への対応を覚える。変数は1回だけ定義され、定義は使う位置を支配するので、この対応はどこから引いても正しい。決定木で頭が既知の出現に当たれば、その場で case を選ぶ。値全体を束縛する枝では、その葉でだけ `con` を作る。使われない `con` は縮約が消す
- `match`、分解する `let`、分解する引数は、`decide` が決定木 (`Switch`、`Unpack`、`Leaf`) を作り、それを出してから、枝を枝の順に変換する。`Unpack` は値が `obj` の変数のときだけ使う。R8 がほかの Repr の `unpack` を拒むためである
- case-of-case: 出口の値で、決定木が未知の値を調べずに1つの枝に行き着けば、その枝のラベルへフィールドのアトムを引数にして `jump` し、コンストラクタを作らない。行き着かない出口は集めておき、scrutinee を変換し終えてから決める。0 個なら `switch` を出さない。1 個ならそのブロックで決定木を出す (タプルのリテラルも作らない)。2 個以上なら、各出口で値を作って1つの合流のブロックへ `jump` し、そこで決定木を出す。枝の本体は文脈の出口で変換するので、入れ子の case-of-case もそのまま組み合わさる。呼び出しの結果と translate の後に現れる case-of-case は扱わない ([ロードマップ](../future/roadmap.md) の「処理系」の jump threading)
- 末尾呼び出しは、`finish` の最後にブロックの形から作り、構文の規則には頼らない。文がなく、`return p` だけを持ち、引数が `p` だけのブロックがあれば、そこへの `jump b(a)` をすべて `return a` にしてそのブロックを消す。番号の大きいブロックから処理するので、連鎖も1回でたたまれる。そのうえで、`let x = <呼び出し>` と `return x` を `tail` にする。この規則 (`tail_call`) は縮約と共有する

### 継続のフレーム

- 継続は、ヒープ上のイミュータブルなフレームの連結リストで表す。handler フレームは、その中の目印になる。
- `handle` は、節のクロージャか関数の値を持つ handler フレームを積み、本体のクロージャか関数の値に `()` を適用する。本体の値が handler フレームに届いたら、フレームを外す。`return` の節への受け渡しは、下の状態の項で定める。
- handler フレームと `Mask` フレームは、連結リストのほかに、外側へ向かう連鎖でもつながる ([ランタイム](../spec/runtime.md) の「handler の連鎖」)。機械は `handlers` を持ち、`cont` から `next` でたどって最初に会う連鎖のフレーム (handler、`Mask`、`Root` のどれか) を指す。連鎖のフレームを積むときは、そのフレームの `outer` (handler フレームでは `Attached` の `outer`) を `handlers` にし、`handlers` をそのフレームにする。値が届いてフレームを外すときは、`handlers` をそのフレームの `outer` に戻す。`perform` が handler を探すときは連鎖だけをたどるので、費用は継続の深さによらず、間にある handler と `Mask` のフレームの数に比例する。`Mask` フレームの数は handler フレームの数を超えない ([エフェクトと handler](../spec/effects.md) の「健全性」)。
- `perform` は、`handlers` から `outer` で連鎖を遡って、同じエフェクトの一番内側の handler フレーム h を探す。`Mask` フレームが飛ばす handler は数えない (下の `find_handler`)。先頭から h までの区間が継続になる。h の `Attached { next, state, outer }` を読み、機械の継続を `next`、`handlers` を `outer` にして、節を呼ぶ。h は `Detached { inner }` にし、`inner` には元の `handlers` を入れる。元の `handlers` は、区間の中でいちばん内側の連鎖のフレームで、区間に連鎖のフレームがなければ h 自身である。`once` と `multi` の操作は区間を継続オブジェクトにして `k` として渡し、`never` の操作は区間をその場で解放する。
- `resume` は、継続オブジェクトの h の `Detached { inner }` を読み、h を `Attached { next: 今の継続, state, outer: handlers }` にしてから、`handlers` を `inner` にし、区間の先頭に `v` を返す。区間のフレームはつねに一意なので、つなぎ直しは書き換え1回で済む。一意なオブジェクトの書き換えは観測できないので、フレームをイミュータブルとして扱う前提と両立する (Perceus の reuse と同じ理屈)。継続オブジェクトが共有されていれば (`multi` の `k` をもう一度使う場合)、区間のフレームを写してから、写した h を今の継続につなぐ。写すときは、区間の中の連鎖の参照も写した側に付け替える。元の区間はそのまま残るので、継続を何度でも再開できる。最後の1回の再開では継続が一意なので、写さずに書き換える。
- handler フレームは、つながっている間つねに状態を持つ。`handle` は初期値を状態にしてフレームを積む。`perform` は区間を切り出すときに `Attached` から状態を取り出し、節の最後の引数として渡す。区間に残る h は `Detached` で、状態を持たない。`resume k v s` は、h を `Attached` に戻すときに状態を `s` にする。区間を写す場合は、写した h に入れる。本体の値が handler フレームに届いたら、状態を取り出し、値と一緒に `return` の節に渡す。`return` の節はつねにある。省略した節は、HIR が `| return x -> x` として合成する。状態のある handler では、状態を `_` で捨てる `| return x _ -> x` として合成する ([式](../spec/expressions.md) の「handler」と「パラメータ付き handler」)。
- `drop k` と `never` の操作による中断は、継続の区間を解放する。フレームは退避した値だけを所有するので、子をたどる解放が、捕まっていた値を1回ずつ解放する。`Lin` の値の破棄処理はオブジェクトの解放そのものなので、捕まっていた `File` は、この解放で読み出し口が捨てられて閉じる ([ランタイム](../spec/runtime.md))。
- `mask` 付きの呼び出しは、`Mask` フレーム (`Frame::Mask { effects, next, outer }`) を1つ積んでから呼ぶ。`effects` は `mask` の多重集合で、フレームは値を持たない。末尾でない呼び出しでは戻りのフレームの上に積むので、呼び出し先が値を返すと先に外れる。末尾呼び出しでは、戻りのフレームの代わりに積む。`resume` では、`resume` が今の継続を読む前に積むので、再開した区間の h の `next` と `outer` は、どちらもその `Mask` フレームになる。値が `Mask` フレームに届いたら、フレームを外して値をそのまま次へ返す。
- `find_handler` は、探すエフェクトについて飛ばす数を数えながら連鎖を遡る。`Mask` フレームでは、その数に `effects` の中の同じエフェクトの数を足す。同じエフェクトの handler フレームでは、数が正なら1減らして飛ばし、0 ならその handler を選ぶ。調べた連鎖のフレームの数を、`RunStats` の `handler_visits` に足す。
- `Mask` フレームは、ほかのフレームと同じく継続の区間に入り、`multi` の再開では写され、中断では解放される。子のたどり方 (`children`) と `debug_heap` のリークの数え方では、引数のない `Apply` フレームと同じに扱う。`outer` は所有しないので `children` はたどらず、区間の複写 (`copy_segment`) が写した側のフレームに付け替える。
- 連鎖の不変条件は2つある。`handlers` は、`cont` から `next` でたどって最初に会う連鎖のフレームである。捕まえた区間の中の連鎖の参照は区間の外を指さず、区間のいちばん外側の連鎖のフレームの `outer` は h を指す。
- インタプリタは、次の O(1) の検査を `debug_heap` によらず常に行い、破れていれば `Fault::Internal` にする。`ret` が `Mask` か handler のフレームを外すとき、そのフレームが `handlers` と同じであること。`perform` のたどりが `Attached` の handler で終わること。`resume` で、h が `Detached` であり、`inner` が handler か `Mask` のフレームであること。`copy_segment` は区間をすべてたどるので、そのついでに、区間の中の連鎖が `inner` から `outer` で h までつながっていることを確かめる。
- `perform` ごとに継続の全体をたどる検査は入れない。`debug_heap` は UI テストとテストの補助で常に付くので、全体をたどるとどのテストも2乗の時間になるためである。
- 末尾の `mask` 付き呼び出しで、継続の先頭がすでに `Mask` フレームなら、2つを1つのフレームに併合しても意味は変わらない。間に handler フレームがないので、飛ばす数を足す順が結果に影響しないためである。この併合は許される最適化だが、今は行わない。`Mask` フレームの数は handler フレームの数以下に収まり ([エフェクトと handler](../spec/effects.md) の「健全性」)、併合で減るのは定数倍にとどまるためである。
- 連結リストの最下部には、プログラムの終わりを表す `Frame::Root` がある。連鎖のいちばん外側も `Root` で、`handlers` は始めに `Root` を指す。値がここに戻れば実行が終わる。操作の handler を探してここに届いたら、内部の誤りである。
- `extern` 命令は、連結リストを遡らず、フレームも積まずにその場で実行して値を返す。extern は eml のコードを呼び返さないので、継続を切り出す必要がないためである。

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

`eml_cli` の lib は次の API を公開する。`Session` はパイプラインを組む唯一の場所で、CLI、UI テスト、`eml_test_support` がこれを通す。UI テストはこれをプロセス内で呼ぶ。どのメソッドも読み込みの結果から計算し直し、途中の結果を持たない。途中の結果を使い回すのは、salsa でクエリ化するときに考える。`Session` は `Send + Sync` で、`crates/eml_cli/tests/api.rs` が確かめる。salsa への載せ替えと、フロントエンドの並列のコンパイルに備えるためで、インタプリタとは関係ない。読み込みの結果が持つのは緑の木 (green tree) と `AstPtr` だけで、ノードを持たないので `Send + Sync` にできる。

- `Session::load(entry_path, entry_text, &dyn ModuleSource) -> Session` は、入口の表示のパスと本文を受け取り、読み込みの段で標準ライブラリ全体と import したモジュールを読む。`Session::user_module_names` は出どころがユーザーのモジュールの名前を返し、UI テストの harness が1ファイルのテストの import を確かめるのに使う。`Session` は1回の検査や実行で読むソースの集まりである。入口のファイルは `main.rs` が読み、読めなければ終了コード 2 にする。ファイルシステムから読む `ModuleSource` は `FsProvider` で、パスの各段の名前がディレクトリの一覧と大文字小文字まで一致することを確かめる。macOS のように大文字小文字を区別しないファイルシステムで、`import Report.Csv` が `report/Csv.em` に当たらないようにするためである。大文字小文字だけが違う名前があれば、実際の名前を読めない理由として返す (E1026)。`main.rs` は、入口のファイル名をディレクトリの一覧にある綴りに直してから `Session::load` に渡す。`t/server.em` で `t/Server.em` を開けたときも、依存先の `import Server` が入口を指すと分かるようにするためである。実行を始める `main` は入口のモジュールからだけ探し (`hir::Program::main`)、Prelude には置かない
- `Session::def_map() -> DefMapped`、`Session::lower() -> Lowered`、`Session::check() -> Checked` (feature `types`)。結果は段階の出力 (`DefMap`、HIR の `Program`、`TypedProgram`) と診断を持つ。診断は読み込みの段からその段階までのすべてで、`sort_diagnostics` で並べて返す。各段階は診断の順を約束しない。`eml check` は `check().diagnostics` を表示する
- `Session::compile() -> Compiled` (feature `core`)。`Compiled` は、検査で出た診断 (警告を含む) と、エラーがなければ Core IR の `Program` (`Option<Program>`) を持つ。`main` がないこと (E2003) は `compile` だけが検査する ([型と Kind](../spec/types.md) の「推論」)
- テストのための口が2つある。`Session::load_with_std(std, entry_path, entry_text, source)` は標準ライブラリを `(ファイル名, 本文)` の並びに差し替えて読み、`Session::compile_until(last: Pass) -> Compiled` は Core IR を `last` のパスの直後で止める。`eml_hir` の `load` / `load_with_std` と、`eml_core_ir` の `lower` / `lower_until` の組に合わせて置く
- `execute(&Program, &RunConfig, stdout: OutputSink) -> Result<RunStats, RuntimeError>` (feature `run`)。`RunStats` は実行の仕事の回数で、CLI は使わない。テストが2乗の時間にならないことを確かめるのに使う

検査と実行を別の関数に分けるのは、呼び出し側が実行の前に診断を表示できるようにするためである。CLI と UI テストは、`compile` の診断を表示してから `execute` を呼ぶ。

`RunConfig` と `RunStats` は `eml_interp` で定義し、`eml_cli` が再公開する。`RunConfig` は `#[non_exhaustive]` にしてあり、`RunConfig::default()` から作ってフィールドを代入する。`OutputSink` と `RunStats` の定めは [ランタイム](../spec/runtime.md) の「実行の API」にある。

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

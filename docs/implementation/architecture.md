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
  tests/ui/             # UI テストのコーパス
    run/
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
eml_diagnostics  Diagnostic 型、FileId と SourceFiles、ariadne による表示
```

- 段階の並びは `eml_cli` → `eml_interp` → `eml_core_ir` → `eml_types` → `eml_hir` → `eml_syntax` → `eml_diagnostics` で、`eml_interp` はさらに `eml_runtime` に依存する
- `eml_diagnostics` は、診断を出す crate のすべてから使う。現時点で `eml_runtime` と `eml_interp` は診断を出さないので、`eml_diagnostics` に依存していない

## 各段階の規律

- 各段階は `fn stage(input: &In) -> (Out, Vec<Diagnostic>)` の形の純粋な関数にする。グローバルな可変状態は持たない
- HIR 以降は `ExprId` / `PatId` / `DefId` などの ID で参照する (`la-arena`)。型などの解析結果は `ExprId → Type` のような別テーブルに置く
- HIR の各ノードは、元の構文ノードへのポインタ (`SyntaxNodePtr`) を持つ

現在の各段階の入口は次のとおり。`eml_syntax` 以外はまだ仮実装で、空の結果を返す (実装状況は [status.md](status.md))。

| crate | 関数 |
|---|---|
| `eml_syntax` | `parse(FileId, &str) -> (Parse, Vec<Diagnostic>)` |
| `eml_hir` | `lower(FileId, &ast::SourceFile) -> (Module, Vec<Diagnostic>)` |
| `eml_types` | `check(&Module) -> (TypedModule, Vec<Diagnostic>)` |
| `eml_core_ir` | `lower(&TypedModule) -> (Program, Vec<Diagnostic>)` |
| `eml_interp` | `run(Arc<Program>, &RunConfig, &OutputSink) -> Result<(), RuntimeError>` |

## エラーが出ても止まらない

- `eml_cli` の `analyze` は、エラーがあっても途中で止めずにすべての段階を実行し、診断を集める。1回の実行で、独立した複数のエラーを報告するため
- パーサは、壊れた入力に対して `ERROR` ノードを作って処理を続ける。壊れた入力でも必ず `SOURCE_FILE` の木を作り、パニックしない
- 回復の同期点は、レイアウト段が挿入する `SEP` と `CLOSE` である。トップレベルでは、列 0 の `SEP` が最も強い同期点になる ([レイアウト規則](../spec/layout.md))
- 名前解決と型推論は、エラーが起きた場所に `Error` 型を入れる。`Error` が関わる制約や線形性の検査からは、追加の診断を出さない

## 構文を差し替えられるようにする

- パーサは rust-analyzer と同じイベント方式にする。ノード開始、トークン、ノード終了のイベントを出し、別の処理 (`sink.rs`) で rowan の木を組み立てる
- 構文の規則は `eml_syntax/src/grammar/` に閉じ込める。構文の変更は、原則として `grammar/`、`SyntaxKind`、lexer、型付き AST ラッパ、レイアウト段の変更だけで済むようにする
- 暫定構文から本番の構文への切り替えは、この方針のとおり `eml_syntax` の中だけで済んだ (構文の段階 S1)

## `eml_syntax` の内部構成

```
lexer.rs       字句解析。トークン列をつなげると元のテキストに戻る (lossless)
layout.rs      レイアウト段。trivia を除いたトークン列に仮想トークンを挿入する
parser.rs      イベント方式のパーサの仕組み。文法の規則は持たない
grammar/       文法の規則 (items / types / patterns / expressions)
sink.rs        イベント列から rowan の木を組み立てる
ast.rs         型付き AST ラッパ
syntax_kind.rs SyntaxKind
token_set.rs   トークンの集合 (u128 のビット集合)
debug_dump.rs  テスト用の木のダンプ (debug_tree)
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

## `eml_hir` で行う脱糖と検査

HIR への変換では、名前解決に加えて、次の脱糖と検査を行う。それぞれの規則は [宣言](../spec/declarations.md)、[式](../spec/expressions.md)、[直積型とレコード](../spec/records.md)、[モジュールと名前解決](../spec/modules.md) が定める。

- 名前の重複定義と未定義の名前の検査
- シグネチャと等式の対応の検査、複数の等式の `match` への脱糖
- 演算子の列の組み直し (fixity の表を引く)、単項マイナス、セクション
- `use`、パラメータ付き handler、`if` の `else` の補完、`let ... in`
- タプルのレコードへの変換、補間の `++` の連結への変換、コマンドリテラルの `Cmd` の構築
- handler の節の引数の個数、`resume` の引数の個数

## `eml_types`

- Kind・型・row の推論 ([型と Kind](../spec/types.md))
- 推論の後に、型付き HIR の上で別のパスとして、線形性の検査 ([線形性](../spec/linearity.md)) と網羅性の検査 ([網羅性](../spec/exhaustiveness.md)) を行う
- 型検査器を実装するときは、S2 の名前付きレコードを待たずに、M1 のタプルを最初から「数字ラベルの閉じたレコード」として表現することを勧める。後の作り直しを避けるため

## `eml_core_ir`、`eml_runtime`、`eml_interp`

- Core IR とインタプリタの設計は [Core IR とインタプリタ](../spec/core-ir.md)、ヒープと RC は [ランタイム](../spec/runtime.md) が定める
- インタプリタの値とフレームに `Rc` と `RefCell` を使わない。Core IR は `Arc<Program>` で読み取り専用で共有する。将来、複数のスレッドがそれぞれの CEK 機械で同じプログラムを実行するため ([マルチコア対応の設計](../future/multicore.md))

## ソースファイルと位置

- `FileId` と `SourceFiles` (`FileId` → パスとテキスト) は `eml_diagnostics` に置く。マイルストーン1 は単一ファイルなので、中身は実質1件である
- `TextRange` は `text-size` crate を直接使う (`rowan` が再公開しているものと同じ型)。`eml_diagnostics` は `rowan` に依存しない
- 将来クエリ化するときは、`SourceFiles` を salsa の入力に置き換え、必要なら別の crate に切り出す

## CLI と lib API

- 引数の解析は `clap` (derive) を使う
- `eml check <file>` は、診断を stderr に表示する。`eml run <file>` は、検査を通ったら実行する
- `eml run --debug-heap` は、RC のリーク検出と解放済みアクセスの検出を有効にする
- 終了コードは、0 = 成功、1 = 診断のエラーあり、または実行時エラー、2 = 使い方の誤り (引数の誤り、ファイルが読めない) とする

`eml_cli` の lib は次の API を公開する。UI テストはこれをプロセス内で呼ぶ。

- `check(files, file_id) -> Vec<Diagnostic>`
- `run(files, file_id, &RunConfig, stdout: OutputSink) -> RunOutcome`。`RunOutcome` は、検査で出た診断 (警告を含む) と、結果 `RunResult` (`NotRun` / `Completed` / `RuntimeError`) を持つ

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
| `la-arena` | 未導入 | HIR の ID。HIR で ID を使い始める段階で追加する |

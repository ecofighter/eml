# テスト戦略

位置づけ: 手引き。

テストの書き方、テストの変更の運用、テストの置き場所、層ごとの方法、UI テストの仕組みを定める。

## 方針

- TDD で進める。テストを先に書き、実装をテストに合わせる
- 既存のテストは合意済みの仕様である。テストが失敗したら実装を直す。テストを変えるときは、下の「テストの変更の運用」に従う
- スナップショットは `insta` を使う。多くはインラインのスナップショット (`@"..."`) にする
- スナップショットの更新は `cargo insta review` で行う

## テストの変更の運用

テストの変更を3種類に分け、種類ごとに合意の取り方を決める。このプロジェクトで合意した運用で、「既存のテストを変えない」という原則の例外にあたる。

| 種類 | 何が変わるか | 合意と記録 |
|---|---|---|
| 成否の変更 | `run` / `run-fail` / `check-fail` の間でのテストの移動と、UI テストの削除 | 作業の spec に1件ずつ挙げる。spec の承認を合意とみなす |
| 期待値の変更 | UI テストの出力、診断の番号と文言、各段階のダンプのスナップショット、成否を変えない UI テストの移動 | 作業の spec に、変わるテストを範囲で書く (「E2007 を使う UI テストはすべて書き換える」「Core IR のダンプはすべて取り直す」など)。spec の承認を、その範囲の変更すべてへの合意とみなす |
| 機械的な追随 | テストの組み立てだけが変わり、スナップショットの文字列と `assert` の値は1文字も変わらない | 合意は要らない。記録はコミットメッセージで足りる |

- テストを変えないことを理由に設計を曲げない。テストの変更が要ると分かったら、上のどの種類に当たるかを示して、作業の spec に書く
- 成否の変更と期待値の変更では、変える理由を作業の spec とコミットメッセージに書く
- 作業の計画の全体制約は「成否と期待値は、spec に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す」と書く
- UI テストの最上位のディレクトリが成否を決める

## テストの置き場所

新しいテストの置き場所は、期待するものを上から順に見て、最初に当てはまる行で決める。

| # | 期待するもの | 置き場所 |
|---|---|---|
| 1 | 機能ごとの代表的なプログラムを `eml check` / `eml run` にかけたときにユーザーが見るもの。stdout、実行時エラー、プログラム全体が通るか落ちるか、代表的な診断の表示 | UI テスト (`tests/ui/`) |
| 2 | 1つの段階の出力 (CST、HIR、推論したシグネチャ、Core IR) と、その段階の診断の細部 (位置、回復、端のケース) | その段階の crate の結合テスト (`crates/<段階>/tests/`)。ソースから `eml_test_support` で組み立てる |
| 3 | フロントエンドからは作れない状態 (壊れた IR、`decref` の抜けた IR など) と、テストの中で生成した大きなソース | `eml_core_ir/tests/verify.rs` か `eml_interp/tests/`。Core IR は、アリーナを組まずに IR のテキストで書き、`eml_core_ir::parse` で読む。アリーナの形そのものを確かめるテストだけは、`eml_core_ir` の単体テストで組む |
| 4 | 公開の API から届かないか、内部の状態を直接組まないと確かめにくい部品の振る舞い。レイアウト段、パーサのマーカー、単一化の表、Kind の制約の解消、ヒープなど | `src/` の単体テスト |
| 5 | lib API の流れ、CLI の終了コード、テスト補助そのもの | `eml_cli/tests/api.rs`、`eml_cli/tests/cli.rs`、`eml_test_support/tests/` |

### 重複させない

1つの事実は、上の表で選んだ1か所だけで確かめる。UI テストは機能ごとの代表的な筋書きを確かめ、段階の端のケースを繰り返さない。例えば診断なら、端のケースは段階の crate のテストに置き、代表的な表示を `check-fail/` に1つ置く。

### crate の中の置き方

- 結合テストは、話題ごとに1ファイルにする。ファイル名は spec の節か言語の機能から付ける (`effects.rs`、`operators.rs`)。1つのファイルが複数の話題にまたがったら、話題で分ける
- 結合テストは、crate ごとに1つのバイナリ (`integration`) にまとめる。`Cargo.toml` に `autotests = false` と `[[test]]` を書き、`tests/main.rs` で各ファイルを `mod` で宣言する。テストのバイナリが増えると、リンクと、macOS が新しい実行ファイルを最初に起動するときの検査に時間がかかるためである。`tests/main.rs` で宣言しないファイルはコンパイルされず、テストが流れない
- lib には `doctest = false` を付ける。doc コメントは説明だけで、実行する例を書かない。単体テストのない lib (`eml_hir`、`eml_cli`、`eml_test_support`) と `eml_cli` の bin には `test = false` も付け、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
- crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置き、`tests/main.rs` で1回だけ宣言して、各ファイルから `crate::common` で使う。複数の crate で使う部品は `eml_test_support` に置く
- Core IR の結合テストは、確かめるパスごとのファイルに置き、`eml_test_support::core_until` でそのパスの直後の IR を見る。後のパスの書き換えや RC の命令を、確かめたいことと一緒に期待値に入れないためである
- 単体テストは、ファイルの末尾の `#[cfg(test)] mod tests` に置く。テストが300行を超え、ファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける
- `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`lower`、`check`、`core`、`core_until`、`run`、`execute`) と、診断のないことを確かめて組む関数 (`parse_clean`、`lower_clean`)、診断を文字列にする関数 (`short`、`short_text`、`full`) と fix を文字列にする関数 (`fixes`)、段階の表示に診断を足す関数 (`with_diagnostics`)を持つ。`lower` は、メモリ上の `(パス, 本文)` の並びを読む `MemorySource` を読み込みの段 (`eml_hir::load`) に渡してモジュールを読み、`def_map`、`lower` の順に変換する。結果の診断には読み込みの段の診断も入る。`def_map` と `def_map_files` は、`DefMap` と、入口のファイルの `ItemTree` と `DefMap` の診断を返し、構文解析と読み込みの段の診断は含めない。複数のファイルのテストには `*_files` の関数 (`lower_files`、`def_map_files`、`check_files`、`core_files`、`core_until_files`、`run_files`) を使う。入口の本文と、根からの相対パス (`Report/Csv.em`) と本文の組の並びを受け取る。1つのテキストの関数は、並びが空の `*_files` と同じ経路を通る。入口の表示のパスは `ENTRY_PATH` (`test.em`) である。Prelude に定義を足したプログラムは、`lower_with_prelude` で変換する。Prelude の本文を `eml_hir::load_with_prelude` に渡し、Prelude の中の item の扱いを確かめるテスト (`eml_hir` の `structure.rs`、`eml_types` の `modules.rs`) が使う。`Lowered` と `Checked` は HIR の `Program` を持つ。開発専用の crate で、各 crate の `tests/` からだけ使う。段階は feature (`hir` < `types` < `core` < `run`) で選び、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる

## 層ごとの方法

| 層 | 方法 |
|---|---|
| 字句 | トークン列のスナップショット |
| パーサ | `insta` で CST をダンプしたスナップショット。壊れた入力から回復できるかのケースを多めに用意する |
| 名前解決と脱糖 | HIR の pretty printer で変換結果 (演算子の組み直しと脱糖を含む) をダンプしたスナップショット |
| 型推論 | 推論したシグネチャ (Kind と row を含む) のスナップショット |
| Core IR | Core IR の pretty printer で、`dup` / `decref` の位置を含めてダンプしたスナップショット |
| ランタイム | ヒープの単体テスト (確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放) |
| 診断 | 表示した診断テキストのスナップショット |
| 線形性 | 線形な値 (`once` の操作の `k` など) を1回でなく使う不正なプログラム (E3001) と、正しく通るべきプログラムの対 |
| 全体 (UI テスト) | 下の「UI テスト」 |
| CLI | `eml run` / `eml check` の終了コードと引数の誤りを数件確認する |
| RC | すべての実行テストで `debug_heap` を有効にする。違反があればテストを失敗させる |

## UI テスト

- `tests/ui/run/**/*.em` は、診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする
- `tests/ui/check-fail/**/*.em` は、診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- `tests/ui/run-fail/**/*.em` は、診断のエラーなしでコンパイルでき、実行が実行時エラーで終わることを確認する。エラーまでの出力と実行時エラーのメッセージをスナップショットにする
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、`debug_heap` を有効にした `RunConfig` で実行する
- テストは1つのファイル (`<最上位>/<分類>/<名前>.em`) か、1つのディレクトリ (`<最上位>/<分類>/<名前>/main.em`) である。ディレクトリのテストでは `main.em` が入口で、そのディレクトリが根になる。ディレクトリの中のほかの `.em` は `main.em` から import するモジュールで、単独のテストにしない。1つのファイルのテストは import を書かない。根が分類のディレクトリなので、import すると隣のテストのファイルをモジュールとして読んでしまう。依存先のモジュールを読み込んだ1つのファイルのテストは、ハーネスが失敗にする
- 成功すべきか失敗すべきかは最上位のディレクトリで決める。そのため、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの中のパスは `tests/ui` からの相対パスにして、実行する環境に依存しないようにする。依存先のファイルのパスも同じく `tests/ui` からの相対パスになる (`check-fail/names/import_cycle/B.em`)

### 分類

`run/`、`check-fail/`、`run-fail/` の下に、分類のサブディレクトリを切る。成功すべきか失敗すべきかは、最上位のディレクトリで決まる。`ui.rs` は、最上位のディレクトリの直下の .em、分類の直下の `main.em`、深さ3以上にあって `<分類>/<名前>/main.em` に属さない .em を拒み、テストを失敗させる。`main.em` を書き忘れたディレクトリのモジュールが、単独のテストとして黙って通るのを防ぐためである。

- `run/` と `run-fail/` は言語の機能で分け、両方で同じ名前を使う
  - `basics/`: 値、演算子、`let`、`if`、短絡評価
  - `functions/`: クロージャ、高階関数、部分適用
  - `effects/`: エフェクト、`multi`、継続
  - `data/`: `data`、コンストラクタ、`match`、パターン
  - `files/`: `File` と `open` / `read_all` / `close`、中断のときの `File` の解放、ファイルの実行時エラー
  - `modules/`: import、修飾した名前、`pub`、Prelude の修飾。ディレクトリのテストを置く
  - `runtime/`: 実装の性質を確かめるテスト。メモリの解放、スタックの深さ、join point、末尾呼び出し
- 実行テストは入口の `.em` のあるディレクトリを `open` の基準ディレクトリにする。入力のファイルはテストの隣に置く。拡張子が `.em` でないファイルは、テストにもモジュールにも数えない
- `check-fail/` は、主なエラーの番号の範囲 ([診断](../spec/diagnostics.md) の「番号の範囲」) で分ける。機能で分けると、エフェクトの誤りのように E1xxx と E2xxx にまたがるものの置き場所が決まらないためである
  - `syntax/` (E0xxx)、`names/` (E1xxx)、`types/` (E2xxx)、`linearity/` (E3xxx)。`exhaustiveness/` (E4xxx)
  - E3001 は今は `eml_types` が出すが、出す crate ではなく番号の範囲に従って `linearity/` に置く
  - E0004 (まだ対応していない構文) は、どの段階が出しても `not-yet-supported/` に置く
- サブディレクトリの名前は、親の `check-fail` と同じくケバブケースにする
- スナップショットの名前は、最上位のディレクトリからの相対パスで固定する (`run/basics/hello.em` は `integration__ui__run@basics__hello.em.snap`。頭の `integration__ui__` は、insta が付けるテストのバイナリとモジュールの名前である)。insta の既定では、分類が1つしかないディレクトリの名前に分類が入らず、分類が増えたときに名前が変わるためである。ディレクトリのテストの名前はディレクトリのパスで、`check-fail/names/import_cycle/` は `integration__ui__check_fail@names__import_cycle.snap` になる。テストのパスが名前になるので、UI テストを移動するとスナップショットの名前が変わる。成否を変えない移動は期待値の変更で、最上位のディレクトリをまたぐ移動は成否の変更である (「テストの変更の運用」)

## 文書の引用の検査

`crates/eml_cli/tests/citations.rs` は、コメントと文書の引用を2つ確かめる。

- `docs/…/ファイル.md の「見出し」` の形の引用と、Markdown のリンクの直後に `の「見出し」` を続けた引用は、行き先のファイルにその見出しがある。照合では空白を無視するので、引用を折り返してもよい
- 文書の Markdown のリンクの行き先がある

見出しのない引用 (`(docs/spec/core-ir.md)` など) は検査できない。節を移すときは、そのファイルを引くコメントを grep し、移した内容に頼っているものを直す。`docs/superpowers` は作業中の設計と計画なので、検査から外す。

## CLI のテスト

`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。

## 性能のテスト

`crates/eml_types/tests/scaling.rs` は、合成プログラムを4倍の大きさにしたときの型検査の時間の比を確かめる。形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰、1つの本体で同じ名前の `let` が続く連鎖の6つである。HIR まで作ってから `eml_types::check` の時間だけを測り、各大きさで3回測った最小を使う。大きさは関数の数で、2000 と 8000 にする。最後の形だけは、関数の数ではなく `let` の数を大きさにする。比が6以下なら通る。時間を測るので `#[ignore]` を付け、release ビルドで流す。型検査の構造を変えたときに流す。

`crates/eml_hir/tests/scaling.rs` は、名前の違う `data` と `effect` の宣言の数を 4000 から 16000 にしたときの、構文解析から `DefMap` までの時間の比を確かめる。比が6以下なら通る。`DefMap` の重複の判定を変えたときに流す。

## よく使うコマンド

```sh
cargo test                                           # すべてのテスト
cargo test -p eml_syntax --test integration parser::empty_file    # 1つのテスト
cargo test -p eml_cli --test integration ui::            # UI テスト
cargo insta review                                   # スナップショットの承認
cargo clippy --all-targets && cargo fmt
cargo test --release -p eml_types --test integration scaling:: -- --ignored   # 型検査の時間の伸び (性能のテスト)
cargo test --release -p eml_hir --test integration scaling:: -- --ignored     # 名前の表を作る時間の伸び (性能のテスト)
```

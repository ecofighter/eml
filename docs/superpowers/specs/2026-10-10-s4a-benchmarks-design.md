# S4a 計測の基準

位置づけ: 作業の設計。ロードマップの S4 (単相化と計測の基準) を S4a と S4b に分け、その前半の S4a を定める。S4a を終えたら、決まったことを `docs/implementation/benchmarks.md` に移し、この文書を削除する。

## 目的

S4b の単相化と、その後の S7 (evidence passing) と S10 (VM) で、前後の費用を比べるための基準を作る。基準は2つの数で持つ。

- 実行の仕事の回数 (`RunStats`): 決定的なので、テストで固定する。回数が変わればスナップショットの差分として出るので、前後の回数が git の履歴に残る
- 実行した命令の数: 手元のマシンで測り、測定の環境と一緒に文書に記録する

IR、型検査、実行の意味は変えない。

## 段の分け方

ロードマップの S4 を次の2段に分ける。

| 段 | 中身 | 前提 | 完了の条件 |
|---|---|---|---|
| S4a 計測の基準 | 基準のプログラム、`RunStats` の `boxes` と `unboxes`、回数のテスト、命令数の測定と記録 | なし | 下の「完了の条件」 |
| S4b 単相化 | translate の (関数, 型引数) の instance の表 | S4a | UI テストの出力が変わらない。instance の数とコンパイル時間の scaling テストが上限を守る。単相化の前後の回数を `benchmarks.md` とスナップショットの差分で記録する |

S5 と S7 の前提は S4b にする。

## 命令数の測り方

callgrind は使わない。開発のマシンが arm64 の macOS で、valgrind はこの環境に対応していないためである。代わりに、macOS の `/usr/bin/time -l` が出す `instructions retired` を使う。

S4a の設計のときに、release ビルドの `fib 30` を3回測った結果は次のとおりである。

| 回 | instructions retired | 実時間 |
|---|---|---|
| 1 | 10,475,804,283 | 0.44 s |
| 2 | 10,466,538,246 | 0.43 s |
| 3 | 10,467,735,173 | 0.43 s |

揺れは約0.1%なので、前後の比較に使える。arm64 の命令数なので、調査レポート (`docs/reports/eml/`) の callgrind (x86_64) の値とは直接比べない。

## 基準のプログラム

リポジトリの根に `bench/` を置き、次の6本を入れる。

| ファイル | 中身 | 主に見るもの |
|---|---|---|
| `empty.em` | 何もしない `main` | 起動と標準ライブラリのコンパイルの分 |
| `fib.em` | `fib 25` | 呼び出しと `Int` の算術 |
| `loop.em` | 末尾再帰のループ | 末尾呼び出し |
| `state.em` | `get` と `put` を持つ `Counter` の handler の下で数える再帰 (調査レポートの `state.em` と同じ形) | `perform` と `k` の呼び出し |
| `list.em` | プログラムの中で `data List a` を定義し、`range`、`map`、`foldl` で合計する | 型変数のフィールドの `Int`、高階関数 |
| `tree.em` | 二分探索木に、線形合同法で作った `Int` を入れ、総称な `size` と `fold` で畳む | 再帰的なデータ、`switch` と `unpack` |

- S6 まではリストのリテラルも標準ライブラリの `List` もないので、`list.em` と `tree.em` はデータ型を自分で定義する
- `list.em` と `tree.em` には、多相な関数 (`map`、`foldl`、`size`、`fold`) を入れる。S4b の単相化で `box` と `unbox` の回数が減るかを見るためである
- 大きさは、debug ビルドで `debug_heap` を付けたときに1本1秒以内で終わるように、実装のときに測って決める。設計のときに測った `fib 25` は 0.41 秒で、release では 981,355,387 命令だった。何もしないプログラム (`println` 1回) は release で 22,451,439 命令で、`fib 25` の約2%に当たる。この起動の分は `empty.em` で分けて見る
- 各プログラムは、結果を1行出力する。`empty.em` は何も出力しない

## `RunStats` の `boxes` と `unboxes`

`eml_interp::RunStats` に次の2項目を足す。

- `boxes`: 実行した `box` の文の数
- `unboxes`: 実行した `unbox` の文の数

CEK では `box` と `unbox` は値をそのまま渡すだけで、今の実行の費用はほとんどない。それでも数えるのは、S9 の語の値の表現 (`tobj` の `Int` を即値にする) 以降で費用になる変換の数を、今のうちから追うためである。

`scaling.rs` に、数え方を確かめる小さなテストを足す。型変数のフィールドに `Int` を入れて取り出すプログラムで、`boxes` と `unboxes` がそれぞれ1以上になることと、`box` と `unbox` を含まないプログラムで0になることを確かめる。

## 回数のテスト

- `crates/eml_interp/tests/bench.rs` を足し、`tests/main.rs` に登録する
- プログラムごとに1つのテスト関数を置く。テストを並列に走らせるためである。各テストは `bench/<name>.em` を読み、`debug_heap` 付きで走らせ、出力を確かめ、`RunStats` の全項目を insta のインラインスナップショットで固定する
- `bench/` の `.em` のファイルの一覧と、テストが知っているプログラムの一覧が一致することを確かめるテストを1つ置く。プログラムを足してテストを書き忘れることを防ぐためである
- ファイルは `env!("CARGO_MANIFEST_DIR")` からの相対パスで読む

## 命令数の測定 (`bench/run.sh`)

- bash で書く。macOS 以外では、理由を出して終了する
- `cargo build --release -p eml_cli` の後、各プログラムを `/usr/bin/time -l target/release/eml run` で3回走らせ、`instructions retired` の中央値と実時間の中央値を Markdown の表で出す
- 表の前に、測定の環境 (機種と CPU、macOS の版、`rustc -V`、コミット) を出す

## 文書

- `docs/implementation/benchmarks.md` (計測の基準) を新しく作る。目的、プログラムの表、回数のテストの見方、`bench/run.sh` の使い方、記録の表を書く。記録の表は段ごとに足していく形にし、最初の記録を S4a の基準にする。`docs/README.md` と `docs/implementation/testing.md` からリンクする
- ロードマップ
  - 段の列の S4 を、上の「段の分け方」のとおり S4a と S4b の2行に分け、S5 と S7 の前提を S4b にする
  - 「S4 単相化と計測の基準」の節を S4b の節にし、計測の項目 (callgrind の行を含む) を除く。S4b の完了の条件は、`benchmarks.md` の記録とスナップショットの差分を指すようにする
  - S4a の決めたことは、ロードマップに節を作らず、段の終わりに `benchmarks.md` へ入れる
  - ほかの節にある S4 への参照は、内容に合わせて S4a か S4b に直す
- ロードマップの外にある S4 への参照も直す
  - `docs/overview.md` の段の一覧の「S4 単相化と計測の基準」を、「S4a 計測の基準、S4b 単相化」にする
  - `docs/implementation/architecture.md` の「S4 の単相化」と「S4 で単相化のときに」を S4b にする
  - 同じ文書の「S4 の組み込みのクラス」、「S4 の補間の穴」、「extern に変換するメソッドを S4 で値として使えるようにする」は、引き直す前のロードマップの S4 を指したまま残っている。今の段に合わせて、それぞれ S5 の型クラス、S6 の補間、S5 に直す
- `RunStats` の項目を足すので、その定めと説明を直す
  - `docs/spec/runtime.md` の「実行の API」の回数の列 (今は5つで、はじめの4つが仕事の回数) に、`boxes` と `unboxes` を足す
  - `docs/spec/core-ir.md` の「`box` と `unbox` は `RunStats` の回数を変えない」を、参照の数の回数は変えず、`boxes` と `unboxes` だけを数える形にする
  - `docs/implementation/architecture.md` の「`handler_visits` は `find_handler` が数え、ほかの4つはヒープが数える」に、`boxes` と `unboxes` を数える場所を足す
  - `docs/implementation/testing.md` の「性能のテスト」に `bench.rs` を、「よく使うコマンド」に `bench/run.sh` と回数のテストを足す
- 段を終えたら、`docs/README.md` の表のロードマップの「再設計の段 (S4〜S13)」を S4b〜S13 にする。ロードマップの段の列からも S4a の行を消し、S4b の前提を「なし」にする。終えた段を段の列から消す、という進め方の決まりのためである
- CLAUDE.md の `RunStats` の説明 (four work counts and `peak_objects`) を、`boxes` と `unboxes` を足した形に直し、`bench/` の説明を1行足す
- `docs/implementation/status.md` と、`docs/implementation/architecture.md` の上に挙げた箇所以外は、書き換える箇所があるときだけ直す

## テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

- 成否の変更: なし
- 期待値の変更: なし。既存のテストの出力は変わらない
- 機械的な追随: `RunStats` に2項目を足すことによる追随だけである。今の `RunStats` のテストは項目を名前で読むので、直す箇所はない見込みである
- 新しいテスト: `bench.rs` のプログラムごとのテストと一覧の対応のテスト、`scaling.rs` の `boxes` と `unboxes` の数え方のテスト

## 完了の条件

- `cargo test` がすべて通り、UI テストの出力が変わらない
- `bench/run.sh` がこのマシンで表を出し、その表と測定の環境を `benchmarks.md` に S4a の基準として記録してある
- `cargo clippy --all-targets`、`cargo fmt`、`nix build` が通る

## 進め方

worktree のブランチで実装し、main へ fast-forward で入れる。入れたら、この spec と S4a の計画を削除する。

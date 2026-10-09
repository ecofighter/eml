# 計測の基準

位置づけ: 手引き。

段の前後で実行の費用を比べるための、基準のプログラムと測り方をまとめる。S4b の単相化、S7 の evidence passing、S10 の VM は、ここの記録と比べる ([ロードマップ](../future/roadmap.md))。

費用は2つの数で見る。

- 実行の仕事の回数 (`RunStats`): 決定的なので、テストのスナップショットで固定する。回数が変わると、スナップショットの差分として出る
- 実行した命令の数: macOS の `/usr/bin/time -l` が出す `instructions retired` を、`bench/run.sh` で測る。測定の環境と一緒に、下の「記録」に書く

## 基準のプログラム

リポジトリの `bench/` に置く。各プログラムは結果を1行出力する。`empty.em` だけは何も出力しない。大きさは、debug ビルドで `debug_heap` を付けたときに1本1秒以内で終わるように決めた。

| ファイル | 中身 | 主に見るもの |
|---|---|---|
| `empty.em` | 何もしない `main` | 起動と標準ライブラリのコンパイルの分 |
| `fib.em` | `fib 25` | 呼び出しと `Int` の算術 |
| `loop.em` | 末尾再帰のループ | 末尾呼び出し |
| `state.em` | `get` と `put` を持つ `Counter` の handler の下で数える再帰 | `perform` と `k` の呼び出し |
| `list.em` | プログラムの中で定義した `List a` を `range`、`map`、`foldl` で合計する | 型変数のフィールドの `Int`、高階関数 |
| `tree.em` | 線形合同法で作った `Int` を二分探索木に入れ、総称な `size` と `fold` で畳む | 再帰的なデータ、`switch` と `unpack` |

S6 まではリストのリテラルも標準ライブラリの `List` もないので、`list.em` と `tree.em` はデータ型を自分で定義する。`list.em` と `tree.em` の多相な関数 (`map`、`foldl`、`size`、`fold`) では、型変数の位置の `Int` が `box` と `unbox` を通る。S4b の単相化で減るかを見るために入れてある。

## 回数のテスト

`crates/eml_interp/tests/bench.rs` は、プログラムごとに1つのテストで、`debug_heap` を付けて走らせ、出力を確かめ、`RunStats` の全項目をインラインスナップショットで固定する。`bench/` のファイルの一覧と、テストのプログラムの一覧が一致することも確かめる。回数は機械の速さに左右されないので、ふだんの `cargo test` で流す。

段の前後の回数は、このスナップショットの差分で残る。回数を変える変更は、その段の spec に期待値の変更として書く ([テスト戦略](testing.md) の「テストの変更の運用」)。

## 命令の数の測り方

```sh
bench/run.sh
```

`bench/run.sh` は、release ビルドの `eml run` で各プログラムを3回走らせ、`instructions retired` と実時間の中央値を Markdown の表で出す。表の前に、測定の環境 (機種と CPU、macOS の版、`rustc -V`、コミット) を出す。コミットしていない変更があれば、コミットの後に `-dirty` が付く。プログラムが実行時エラーで止まったときと、`time -l` の出力から命令の数か実時間を読めなかったときは、その出力を見せて止まる。macOS でだけ動く。

valgrind が arm64 の macOS で動かないので、callgrind は使わない。`instructions retired` は callgrind ほど決定的ではないが、S4a の設計のときに release ビルドの `fib 30` を3回測った揺れは約0.1%だった。arm64 の命令の数なので、調査レポート (`docs/reports/eml/`) の callgrind (x86_64) の値とは直接比べない。

`empty.em` の命令の数は、起動と標準ライブラリのコンパイルの分である。ほかのプログラムの値からこれを引くと、実行の分の目安になる。実時間は 0.01 秒単位なので、命令の数を主に見る。

段の終わりに `bench/run.sh` を流し、出力の環境と表を、下の「記録」に段の見出しを付けて足す。

## 記録

### S4a (基準)

- 機種: Mac16,1 (Apple M4)
- OS: macOS 26.6.2 (25G83)
- rustc: rustc 1.95.0 (59807616e 2026-04-14) (built from a source tarball)
- コミット: aa525f7

| プログラム | instructions retired | 実時間 (s) |
|---|---:|---:|
| `empty` | 22434850 | 0.00 |
| `fib` | 974553802 | 0.04 |
| `list` | 1761261161 | 0.07 |
| `loop` | 1505815428 | 0.05 |
| `state` | 1041348334 | 0.04 |
| `tree` | 1558524168 | 0.06 |

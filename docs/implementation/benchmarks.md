# 計測の基準

位置づけ: 手引き。

段の前後で実行の費用を比べるための、基準のプログラムと測り方をまとめる。S7 の evidence passing と S10 の VM は、ここの記録と比べる ([ロードマップ](../future/roadmap.md))。S4b の単相化の前後と、S5 の型クラスの前後は、下の「記録」にある。

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

S6 まではリストのリテラルも標準ライブラリの `List` もないので、`list.em` と `tree.em` はデータ型を自分で定義する。`list.em` と `tree.em` の多相な関数 (`map`、`foldl`、`size`、`fold`) では、型変数の位置の `Int` が `box` と `unbox` を通る。単相化の前後でこの回数がどう変わったかは、下の「記録」の「S4b (単相化)」にある。

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
- コミット: b056989

| プログラム | instructions retired | 実時間 (s) |
|---|---:|---:|
| `empty` | 22471689 | 0.00 |
| `fib` | 974018003 | 0.04 |
| `list` | 1767496581 | 0.07 |
| `loop` | 1505639585 | 0.05 |
| `state` | 1046168606 | 0.04 |
| `tree` | 1563454184 | 0.07 |

### S4b (単相化)

- 機種: Mac16,1 (Apple M4)
- OS: macOS 26.6.2 (25G83)
- rustc: rustc 1.95.0 (59807616e 2026-04-14) (built from a source tarball)
- コミット: 9fed413

| プログラム | instructions retired | 実時間 (s) |
|---|---:|---:|
| `empty` | 22521353 | 0.00 |
| `fib` | 973960651 | 0.04 |
| `list` | 1791431775 | 0.07 |
| `loop` | 1505792016 | 0.05 |
| `state` | 1040445939 | 0.04 |
| `tree` | 1555696977 | 0.06 |

S4b で `list` と `tree` の `box` と `unbox` の回数は増えた。`list` は `boxes` と `unboxes` が 300001 から 400000 に、`tree` は `boxes` が 29797 から 44694 に、`unboxes` が 266462 から 281359 に増えた。`empty`、`fib`、`loop`、`state` は変わらない。単相化の後は `foldl@[Int, Int]` と `fold@[Int, Int]` のアキュムレータが `int` になり、一様なコールバックを `apply` するたびに、呼ぶ前に `box` し、返った後に `unbox` するためである。要素のフィールドは、データの配置が一様なので `tobj` のままである。この回数は、コミット c30ab6c での `bench.rs` のスナップショットの差分である。

命令の数は、S4a と比べて `list` で約1.4%増え、`tree` で約0.5%減った。ほかは0.6%以内の差である。

### S5 (型クラス)

- 機種: Mac16,1 (Apple M4)
- OS: macOS 26.6.2 (25G83)
- rustc: rustc 1.95.0 (59807616e 2026-04-14) (built from a source tarball)
- コミット: dd33ee2

| プログラム | instructions retired | 実時間 (s) |
|---|---:|---:|
| `empty` | 25930097 | 0.00 |
| `fib` | 975640407 | 0.04 |
| `list` | 1789277949 | 0.07 |
| `loop` | 1509485107 | 0.05 |
| `state` | 1044144814 | 0.04 |
| `tree` | 1557668106 | 0.07 |

S5 で `RunStats` は変わらない。`crates/eml_interp/tests/bench.rs` のスナップショットは S4b のままで、`bench/` の `show_int` を `show` に書き換えたときも差分は出なかった。`Int` の `==`、`<` と `show` は、instance の `extern` の行として、今までと同じく `extern` 命令を直接出すためである ([Core IR とインタプリタ](../spec/core-ir.md) の「メソッドの解決」)。

命令の数は、`empty` が S4b より約341万 (約15%) 増えた。増えたのは起動と標準ライブラリのコンパイルの分である。同じビルドで `eml check bench/empty.em` を測ると約2556万で、`eml run` の約2575万とほとんど同じなので、この分はほぼすべてフロントエンド (読み込みと型検査) にある。Prelude は 69 行から 124 行になり、クラス `Eq`、`Ord`、`Show` と既定のメソッド、instance、導出した instance (`Bool` と `Ordering`) を毎回検査するようになった。`loop` と `state` の増分 (約369万と約370万) は `empty` の増分とほぼ同じである。`empty` の値を引いた実行の分は、`loop` で約0.019%、`state` で約0.028% 増え、`tree` で約0.09%、`fib` で約0.18%、`list` で約0.31% 減った。仕事の回数は同じである。雑音を超える差の原因は調べていない。

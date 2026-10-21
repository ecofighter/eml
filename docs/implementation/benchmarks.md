# 計測の基準

位置づけ: 手引き。

実行の費用を測るための、基準のプログラムと測り方と、今の数値をまとめる。費用は2つの数で見る。

- 実行の仕事の回数 (`RunStats`): 決定的なので、テストのスナップショットで固定する。回数が変わると、スナップショットの差分として出る
- 実行した命令の数: macOS の `/usr/bin/time -l` が出す `instructions retired` を、`bench/run.sh` と `bench/rt.sh` で測る。スクリプトと数値計算は、Python と実時間で比べる (`bench/scripts.sh`)

下の「今の数値」の表は、段を閉じるときに、3つとも同じ機種で測り直して入れ替える ([ロードマップ](../future/roadmap.md) の「進め方」)。測り直すのは、前の表のコミットから `crates/eml_vm`、`crates/eml_rt`、`crates/eml_lir`、`crates/eml_core_ir`、`std/`、`bench/` のどれかが変わった段である。測るのはコードの最後のコミットの作業ツリーで、未コミットの変更がある木の値は載せない。段の途中では入れ替えない。段ごとの前後は、この文書の git の履歴 (`git log -p docs/implementation/benchmarks.md`) と、閉じるコミットのメッセージで読む。

今も判断の根拠になっている計測は、下の「所見」に題名の見出しで残し、コードと文書はその見出しを引く。各所見は、主張、支える数値と測った機種とコミット、根拠にしている所 (この所見を引く規則、コメント、項目)、受け入れた後退ならその理由を書く。残すのは、コードか `docs/` の規則が今も引くもの、[ロードマップ](../future/roadmap.md) の項目の根拠であるもの、受け入れた後退とその理由である。所見の数値は表と独立で、表の入れ替えでは更新しない。段が変えた規則を根拠にしている所見は、その段を閉じるときに測り直して更新するか、根拠でなくなったなら消す。

## 基準のプログラム

リポジトリの `bench/` に置く。各プログラムは結果を1行出力する。`empty.em` だけは何も出力しない。大きさは、debug ビルドで `debug_heap` を付けたときに1本1秒以内で終わるように決めた。

| ファイル | 中身 | 主に見るもの |
|---|---|---|
| `empty.em` | 何もしない `main` | 起動と標準ライブラリのコンパイルの分 |
| `fib.em` | `fib 25` | 呼び出しと `Int` の算術 |
| `loop.em` | 末尾再帰のループ | 末尾呼び出し |
| `state.em` | `get` と `put` を持つ `Counter` の handler の下で数える再帰 | すぐに再開する操作 |
| `state_loop.em` | 末尾でない `put` を `Std.State` の handler の下で回すループ | 状態を渡す形の特殊化 |
| `state_loop_by_hand.em` | `state_loop.em` のループの状態を、引数で手で受け渡した版 | `state_loop.em` の下限 |
| `state_nested.em` | 状態つきの handler を2つ重ねたループ | 入れ子の handler の特殊化 |
| `apply_int.em` | 実行時に選んだ `Int -> Int` の関数の値を、ループの中で10万回 `apply` する | 引数と結果を一様な語で渡す `apply` |
| `apply_float.em` | `apply_int.em` の `Float -> Float` の版 | `Float` の引数と結果が `apply` を通る費用 |
| `ep_float.em` | `Float` の結果と `Int` の状態を持つ操作を、特殊化しない handler の下で10万回起こす | EP の道で、操作の結果と状態を運ぶ費用 |
| `fold_state.em` | `State.run` の中で `Yield.fold` に純粋なラムダを渡し、10万要素を合計する | 純粋なコールバックを節がその場で呼ぶこと (`yields` が0) |
| `list.em` | プログラムの中で定義した `List a` を `range`、`map`、`foldl` で合計する | 型変数のフィールドの `Int`、高階関数 |
| `tree.em` | 線形合同法で作った `Int` を二分探索木に入れ、総称な `size` と `fold` で畳む | 再帰的なデータ、`switch` と `unpack` |
| `orders.em` | 10万件の `Order` のレコードを、ラムダを渡した `List.filter`、`List.map`、`List.fold` で集計する | リストの高階関数に渡すラムダ |
| `orders_by_hand.em` | `orders.em` のラムダを各ループに書き込んだ版 | `orders.em` の特殊化が目指す下限 |
| `push_fold.em` | `Yield.fold (fn acc x -> acc + x) 0 (nums 100000)` | `Yield` の畳み込みに渡すラムダ |
| `push_fold_by_hand.em` | 専用の `sum` handler に `acc + x` を書き込んだ版 | `push_fold.em` の下限 |
| `iter_pipeline.em` | `Iter.generate (nums 100000)` を `Iter.filter`、`Iter.map`、`Iter.fold` に通す | `Iter` の段ごとのラムダ |
| `iter_pipeline_by_hand.em` | 専用の `evens`、`squares`、`sum` にした版 | `iter_pipeline.em` の下限 |
| `iter_generate.em` | `Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (nums 100000))` | `Iter.generate` が生産者を包む費用 |
| `sum_loop.em` | 1 から 100000 までの合計を直接の末尾再帰で求める | `iter_generate.em` と `push_fold.em` を比べる基準 |
| `tree_by_hand.em` | `tree.em` の `fold (fn acc x -> acc + x)` を専用の `fold_sum` にした版 | `tree.em` の下限 |
| `upsert.em` | `logstat` の AA 木の `upsert key increment` で 20000 個の文字列のキーを数え、`sort_by (fn (_, a) (_, b) -> a > b)` で並べる | 高階の更新関数と比較関数 |
| `upsert_by_hand.em` | `increment` と比較を `upsert` と `sort_by` に書き込んだ版 | `upsert.em` の下限 |
| `records.em` | 4つのフィールドのレコードを、`{ st \| … }` で10万回更新する | 構造体の値の更新 (確保も reuse もしない) |
| `records_wide.em` | `records.em` を7つの `Int` のフィールドのレコードにした版 | 構造体にしない値の reuse |
| `rbtree.em` | 赤黒木に1万個の `Int` を入れる。`balance` は組み直す節を受ける | 同じ関数の中で節を作り直す reuse |
| `tree_map.em` | 8191 節の完全二分木の全節を、`incr` で20回作り直す | 書かないフィールドとその場の書き換え |
| `std_list.em` | 2万要素のリストに、std の `List.map` と `List.filter` を5回通す | std のリストの関数の reuse |
| `sieve_var.em` | 引数 (既定は 20000) より小さい素数の数を、`var` で書いたふるいで数える | 入れ子の `var` の、状態を渡す形の特殊化 |
| `sieve_by_hand.em` | `MutArray` を末尾再帰のループに線形に受け渡した版 | `sieve_var.em` の下限 |
| `fannkuch_var.em` | fannkuch-redux (既定は n = 7) を、3本の配列の変数と10本の `Unr` の変数で書く | 何重にも入れ子になった `var` と、配列の添字 |
| `fannkuch_by_hand.em` | 状態をすべて末尾再帰の局所関数に受け渡した版 | `fannkuch_var.em` の下限 |
| `nbody.em` | Benchmarks Game の5つの天体を、`Vec3` の値で引数の回数 (既定は 1000) だけ進める | 配列に書く構造体の箱と `Float` の算術 |
| `spectral_norm.em` | 引数の大きさ (既定は 100) のスペクトルノルムを、`Array Float` と `var` で求める | 配列の `Float` の要素の箱 |
| `mandelbrot.em` | 引数の大きさ (既定は 128) の正方形の格子で、集合に入る点を `Complex` の値で数える | 構造体の値の算術 |

`nbody.em`、`spectral_norm.em`、`mandelbrot.em`、`sieve_var.em`、`fannkuch_var.em` は、大きさを省略できる引数で受ける。引数がなければ表の既定の大きさで動き、回数のテストと `bench/run.sh` はこの大きさで測る。`bench/scripts.sh` は大きな大きさを渡して Python の版と比べる (下の「スクリプトの計測」)。大きさを読むために `Std.Env` と `String.to_int` のコードがプログラムに入るので、この5つの実行の分には、それをコンパイルして動かす約 2922 万命令が加わる (「今の数値」の環境で、大きさを読むだけのプログラムを測った値)。

`_by_hand` の版は、対応する自動の版と同じ出力を出す。特殊化の効果は、自動の版の `RunStats` が手書きの版に近づくことで測る。

`list.em` と `tree.em` はデータ型を自分で定義する。2つの多相な関数 (`map`、`foldl`、`size`、`fold`) では、型変数の位置の `Int` が `box` と `unbox` を通る。

## 回数のテスト

`crates/eml_vm/tests/bench.rs` は、プログラムごとに1つのテストで、VM の検査モードで走らせ、出力を確かめ、`RunStats` の全項目 ([VM](../spec/vm.md) の「`RunStats`」) をインラインスナップショットで固定する。`every_program_in_bench_has_a_test` は、`bench/` のファイルの一覧とテストのプログラムの一覧が一致することを確かめる。回数は機械の速さに左右されないので、ふだんの `cargo test` で流す。

プログラムをまたいで比べるテストは次のとおりである。

- `specialization_matches_hand_written_work` と `specialization_matches_hand_written_rc`: 自動の版と `_by_hand` の版の6つの組で、`allocations`、`boxes`、`unboxes` と RC の回数が同じである
- `handler_specialization_removes_yields_and_evidence`: `iter_generate` と `iter_pipeline` の `yields` と、`push_fold`、`fold_state`、`state_loop`、`state_nested` の `evidence_steps` が0である
- `iter_allocations_do_not_grow_with_the_input` と `iter_generate_allocates_as_much_as_the_direct_loop`: `Iter.fold` で引く形の確保の数が入力の大きさによらず、直接のループ `sum_loop` と同じである。`Iter.filter` を重ねた形は、残した要素ごとに4つ確保する
- `rebuilding_programs_reuse_boxes`: 関数の中で手放して作り直す8つのプログラムで `reuses` が0より大きい。`allocations_are_counted_in_checked_mode` は、確保の数が検査モードの数え方から来ることを確かめる
- `var_matches_hand_written_work`: `var` で書いた版と手書きの版の2つの組で、`var` の版の `evidence_steps` と `yields` が0で、確保の数の差が表 `RESIDUAL` の値に等しい
- `nbody_allocates_25_per_step`、`mandelbrot_does_not_allocate_complex`、`spectral_norm_allocates_14_fewer`: 構造体の表現で消える確保と残る確保の数 (下の「配列に書く構造体の箱」と「配列の `Float` の要素の箱」)
- `unwind_points`: `bench/` と `tests/ui/run/scripts/` のプログラムの巻き戻しの表の行の数を固定する
- `trade_offs`: 呼び出しの規約と構造体の表現の引き換えの形の回数を固定する (下の「呼び出しの規約の引き換え」と「構造体の表現の引き換え」)

回数を変える変更は、期待値の変更として段の設計 (`docs/plans/`) に書く ([テスト戦略](testing.md) の「テストの変更の運用」)。

## 測り方

命令の数は `bench/run.sh` と `bench/rt.sh` で、Python との実時間の比は `bench/scripts.sh` で測る。どれも macOS の `/usr/bin/time -l` を使うので、macOS でだけ動く。

### 命令の数の測り方

```sh
bench/run.sh
```

`bench/run.sh` は、release ビルドの `eml run` (VM の速いモード) で各プログラムを3回ずつ走らせ、`instructions retired` と実時間の中央値と、実行の分を Markdown の表で出す。実行の分は、各プログラムの命令の数から `empty` の命令の数を引いた値である。`empty` はフロントエンド、コード生成、起動の費用なので、プログラムどうしと、測り直した表どうしは実行の分で比べる。実行の分が 0 以下になったときは、理由を出して止まる。比べられる値でないものを記録しないためである。表の前に、測定の環境 (機種と CPU、macOS の版、`rustc -V`、コミット) を出す。コミットしていない変更があれば、コミットの行に未コミットの変更を示す印が付く。プログラムが実行時エラーで止まったときと、`time -l` の出力から命令の数か実時間を読めなかったときは、その出力を見せて止まる。macOS でだけ動く。

valgrind が arm64 の macOS で動かないので、callgrind は使わない。`instructions retired` は callgrind ほど決定的ではない。同じ木で `bench/run.sh` を2回流すと、実行の分の差は最大で 0.27% (`push_fold_by_hand`、実行の分 47441191 命令) だった。arm64 の命令の数なので、x86_64 の callgrind で測った値とは直接比べない。

`empty.em` の命令の数は、フロントエンド、コード生成、起動の分である。ほかのプログラムの値からこれを引くと、実行の分の目安になる。実時間は 0.01 秒単位なので、命令の数を主に見る。

`bench/rt.sh` は、`eml_rt` の速いモードを測る ([オブジェクトモデル](../spec/object-model.md))。`crates/eml_rt/examples/rt_bench.rs` を release ビルドにし、次の14の計測を `bench/run.sh` と同じ方法で3回ずつ走らせて、命令の数と実時間の表を出す。`bench/` のプログラムでなく、ヒープの操作を直接呼ぶ。

| 計測 | 中身 |
|---|---|
| `empty` | 何もしない。起動の分 |
| `list` | 1,000,000 個の cons セルのリストを作って捨てることを、10回繰り返す |
| `box-list` | 同じことを、`std::alloc` を使う `Box` のリストで行う。`list` と比べる基準である |
| `dup` | 局所の物体の `dup` と `decref` の組を、10,000,000 回繰り返す |
| `free` | cons セルを1つ確保して2つの語を書き、`decref` で空けることを、10,000,000 回繰り返す。`list` の解放は解放の連鎖の中の `release_ref` を通るので、`decref` が一意な物体を空ける道はこの計測で測る |
| `map` | 1,000,000 個のリストを作り、`map (+1)` に当たる作り直しを10回行う。古いセルは `release` で空け、新しいセルは `alloc` で取る |
| `map-reuse` | `map` と同じ作り直しを、`release_reuse` と `con_at` で行う。`eml_rt` だけを測るので、列は手で組む。VM の reuse の命令の速い道と同じ基本操作を、`eml_rt` だけで測る |
| `append` | 空の文字列から始め、一意な文字列に `string_push` で 1 バイトずつ足すことを、10,000,000 回繰り返す。その場の追記と、倍の容量での作り直しの費用である |
| `perform` | evidence が1つの `evv` で、`ev_lookup` と `ev_clause` を呼んで結果を手放すことを、10,000,000 回繰り返す。handler の中で `perform` してその場で戻る道である |
| `perform-deep` | 同じことを、8つの evidence の `evv` の最も奥の evidence に対して行う |
| `handle` | handler を `evv_get`、`ev_new`、`ev_set_clause`、`evv_push`、`evv_set` で設け、`ev_evv` と `evv_set` で外すことを、1,000,000 回繰り返す |
| `yield` | yield を始め、κ を取り出して入れ直し、yield を終える1往復を、10,000,000 回繰り返す |
| `mask` | 4つの evidence の `evv` で、2つ目の evidence を飛ばす `evv_mask` (項目は n = 0、m = 1 の1つ) を呼び、結果を手放すことを、10,000,000 回繰り返す |
| `poll` | 安全点の確かめ `Ctx::poll` を、100,000,000 回繰り返す |

`bench/rt.sh` は続けて、extern の計測のプログラム `crates/eml_rt/examples/rt_extern_bench.rs` の次の12の計測を、同じ方法で測って2つ目の表に出す ([オブジェクトモデル](../spec/object-model.md) の「extern」)。`rt_bench` と別のプログラムにするのは、上の14の計測のコード生成単位の分け方を変えないためである (下の「`eml_rt` のインライン化」)。どの計測も速いモードの `Ctx` で動かす。渡す文字列の参照は、ヒープの文字列なら毎回 `dup` で、不死の文字列 (`concat-copy` の右と `read-all` のパス) なら `acquire_immortal` で作る。`show-int` は整数だけを渡すので、参照を作らない。

| 計測 | 中身 |
|---|---|
| `string-eq` | 等しい 64 KiB の2つの文字列の `==` を、10,000 回繰り返す |
| `string-eq-early` | 先頭のバイトで違う 64 KiB の2つの文字列の `==` を、10,000,000 回繰り返す |
| `compare` | 末尾のバイトだけが違う 64 KiB の2つの文字列の `compare` を、10,000 回繰り返す |
| `concat-copy` | 共有した 1 KiB の左と、16 バイトの不死の右の `++` を、1,000,000 回繰り返す。左が共有なので、毎回両辺を写して作り直す |
| `show-int` | `Int.show` で `7` を文字列にすることを、10,000,000 回繰り返す |
| `show-int-long` | 同じことを `i64::MIN` (20 バイト) で行う |
| `show-string` | エスケープのない 1 KiB の文字列の `String.show` を、100,000 回繰り返す |
| `show-string-dense` | `"a\n"` を 512 回並べた 1 KiB の文字列の `String.show` を、100,000 回繰り返す |
| `println-block` | 23 バイトの行を、`StdHost` の `Block` で `/dev/null` に 1,000,000 回書く |
| `println-line` | 同じことを `Line` (行ごとに flush) で行う |
| `read-all` | メモリの Host の 1 MiB のファイルを、開いて全部読んで閉じることを、100 回繰り返す。Host は長さの見込みを返す |
| `read-all-unsized` | 同じことを、長さの見込みを返さない Host で行う |

### スクリプトの計測

`tests/ui/run/scripts/` のスクリプトのうち入力の大きさが効く4本と、数値計算の5つのプログラムを、Python と比べる。目標は、ロードマップの速さの目標 (Python 3.14 の 1.5 倍以下の実時間) である。`bench/` の基準のプログラムと違い、命令の数でなく実時間と最大の常駐メモリを見る。比べる相手の Python の実行が、命令の数で比べられる形でないためである。

```sh
bench/scripts.sh
```

- 表は2つある。スクリプトの表は `wc` (ログを標準入力から)、`grep -n` (約 0.5% の行が一致するパターン)、`logstat` (1行ずつ読む)、`logstat --whole` (全体を読んでから分ける)、`csv2jsonl` (3つの列を選ぶ) の5行である。数値計算の表は nbody、spectral-norm、mandelbrot、fannkuch-redux、ふるいの5行である
- スクリプトの入力は、`bench/scripts/gen.py` が種類 (アクセスログか CSV) と大きさごとに、種を固定して1つずつ作る。`target/bench-scripts/` に大きさを名前に入れて置き、すでにあれば作り直さない
- 比べる Python の版は、スクリプトが `bench/scripts/<名前>.py`、数値計算が `bench/numeric/<名前>.py` で、Python の標準ライブラリで普通に書いたものである。速さのために手で凝ったものにはしない。数値計算の版は、浮動小数の演算を eml の版と同じ順で行い、ビットまで同じ結果を出す
- 各行の前に `bench/scripts/compare.sh` で、eml と Python の標準出力と終了コード (と書き出したファイル) が一致することを確かめる。一致しなければ止まる
- release ビルドの `eml run` (VM の速いモード) と `python3 -I` を3回ずつ走らせ、`/usr/bin/time -l` の実時間と最大の常駐メモリの中央値を出す。`bench/run.sh` と同じく macOS でだけ動く

#### 大きさ

行ごとの大きさ (入力の KiB か、プログラムに渡す大きさ) は `bench/scripts.sh` が持ち、次の規則で決める。

- 記録する機種で、eml の実時間 (3回の中央値) がおよそ 300 ms になる大きさにする。Python の実時間はその大きさでの値をそのまま記録する
- 段を閉じる計測で、eml の実時間が 150 ms より短いか 600 ms より長い行は、`bench/scripts.sh` の大きさを変えて、閉じるコミットの前の別のコミットにし、その後で3つの表を測り直す。表に載せるのは新しい大きさの値だけで、変えた行と前後の値は閉じるコミットのメッセージに書く
- 表の「大きさ」の欄に、どの大きさで測ったかを残す
- 実時間は 0.01 秒の単位で出るので、300 ms では約 3% の刻みになる。小さな差は `instructions retired` で確かめる。Python の実時間が数十 ms の行 (`wc`) では、比が刻みの分だけ大きく揺れる
- fannkuch-redux の大きさは順列の長さ n で、n が1つ増えると実時間が約9倍になる。300 ms に近い n がないので、300 ms との比が最も小さい n にし、実時間が 150 ms から 600 ms の範囲を外れても決め直さない

`bench/scripts/compare.sh` は3つの形で使う。`--paths <eml の版> <Python の版> [引数...]` は、パスで渡した2つの版を比べる。`<名前> [引数...]` は、`tests/ui/run/scripts/<名前>/main.em` と `bench/scripts/<名前>.py` を比べる。`--fixture <名前>` は、UI テストの付属のファイルで出力を確かめる ([テスト戦略](testing.md) の「分類」)。どちらかのファイルがなければ、比べずに止まる。

## 今の数値

段を閉じるときに入れ替える3つの表である。

### `bench/run.sh`

- 機種: Mac17,2 (Apple M5)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- コミット: 80d960f

| プログラム | instructions retired | 実行の分 | 実時間 (s) |
|---|---:|---:|---:|
| `empty` | 44465300 | | 0.00 |
| `apply_float` | 177670766 | 133205466 | 0.00 |
| `apply_int` | 146702769 | 102237469 | 0.00 |
| `ep_float` | 229564122 | 185098822 | 0.01 |
| `fannkuch_by_hand` | 189388848 | 144923548 | 0.00 |
| `fannkuch_var` | 285655257 | 241189957 | 0.01 |
| `fib` | 130555936 | 86090636 | 0.00 |
| `fold_state` | 95272926 | 50807626 | 0.00 |
| `iter_generate` | 106307184 | 61841884 | 0.00 |
| `iter_pipeline` | 214420679 | 169955379 | 0.00 |
| `iter_pipeline_by_hand` | 215703426 | 171238126 | 0.00 |
| `list` | 203979998 | 159514698 | 0.00 |
| `loop` | 211362104 | 166896804 | 0.00 |
| `mandelbrot` | 469466600 | 425001300 | 0.02 |
| `nbody` | 190534441 | 146069141 | 0.01 |
| `orders` | 294671799 | 250206499 | 0.01 |
| `orders_by_hand` | 295968620 | 251503320 | 0.01 |
| `push_fold` | 91857433 | 47392133 | 0.00 |
| `push_fold_by_hand` | 92163765 | 47698465 | 0.00 |
| `rbtree` | 262317954 | 217852654 | 0.01 |
| `records` | 135430429 | 90965129 | 0.00 |
| `records_wide` | 208186893 | 163721593 | 0.01 |
| `sieve_by_hand` | 104845803 | 60380503 | 0.00 |
| `sieve_var` | 136448422 | 91983122 | 0.00 |
| `spectral_norm` | 522565735 | 478100435 | 0.02 |
| `state` | 82393410 | 37928110 | 0.00 |
| `state_loop` | 84405656 | 39940356 | 0.00 |
| `state_loop_by_hand` | 82082127 | 37616827 | 0.00 |
| `state_nested` | 96354782 | 51889482 | 0.00 |
| `std_list` | 199371679 | 154906379 | 0.00 |
| `sum_loop` | 80017330 | 35552030 | 0.00 |
| `tree` | 237867998 | 193402698 | 0.01 |
| `tree_by_hand` | 237180911 | 192715611 | 0.01 |
| `tree_map` | 185164006 | 140698706 | 0.00 |
| `upsert` | 515639376 | 471174076 | 0.02 |
| `upsert_by_hand` | 513657702 | 469192402 | 0.02 |

### `bench/rt.sh`

- 機種: Mac17,2 (Apple M5)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- コミット: 80d960f

`rt_bench` の計測

| 計測 | instructions retired | 実時間 (s) |
|---|---:|---:|
| `empty` | 16772736 | 0.00 |
| `list` | 1250117850 | 0.04 |
| `box-list` | 3075414148 | 0.09 |
| `dup` | 276773068 | 0.01 |
| `free` | 1826927771 | 0.05 |
| `map` | 1809055045 | 0.06 |
| `map-reuse` | 599038679 | 0.02 |
| `append` | 277773443 | 0.01 |
| `perform` | 516852307 | 0.01 |
| `perform-deep` | 866865626 | 0.03 |
| `handle` | 613862038 | 0.01 |
| `yield` | 347027932 | 0.09 |
| `mask` | 3826981158 | 0.10 |
| `poll` | 1116855171 | 0.03 |

extern の計測 (`rt_extern_bench`)

| 計測 | instructions retired | 実時間 (s) |
|---|---:|---:|
| `string-eq` | 345331550 | 0.01 |
| `string-eq-early` | 737076591 | 0.01 |
| `compare` | 345449541 | 0.01 |
| `concat-copy` | 2634012465 | 0.09 |
| `show-int` | 2346938123 | 0.06 |
| `show-int-long` | 5497239759 | 0.16 |
| `show-string` | 3702649605 | 0.15 |
| `show-string-dense` | 8643002984 | 0.28 |
| `println-block` | 254017287 | 0.00 |
| `println-line` | 11148325287 | 0.61 |
| `read-all` | 125151875 | 0.00 |
| `read-all-unsized` | 136540533 | 0.00 |

### `bench/scripts.sh`

- 機種: Mac17,2 (Apple M5)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- Python: Python 3.14.8
- コミット: 80d960f
- 入力: スクリプトの入力は bench/scripts/gen.py が作る。大きさは表の「大きさ」の欄にある

スクリプト

| 計測 | 大きさ | eml の実時間 (s) | Python の実時間 (s) | 比 (eml / Python) | eml の最大 RSS (MB) | Python の最大 RSS (MB) |
|---|---:|---:|---:|---:|---:|---:|
| `wc` | 9000 KiB | 0.30 | 0.03 | 10.0 | 29 | 78 |
| `grep -n` | 70000 KiB | 0.29 | 0.27 | 1.1 | 9 | 17 |
| `logstat` | 6700 KiB | 0.31 | 0.11 | 2.8 | 10 | 19 |
| `logstat --whole` | 6700 KiB | 0.30 | 0.10 | 3.0 | 35 | 53 |
| `csv2jsonl` | 2900 KiB | 0.29 | 0.18 | 1.6 | 21 | 51 |

数値計算

| 計測 | 大きさ | eml の実時間 (s) | Python の実時間 (s) | 比 (eml / Python) | eml の最大 RSS (MB) | Python の最大 RSS (MB) |
|---|---:|---:|---:|---:|---:|---:|
| nbody | 140000 | 0.29 | 0.64 | 0.5 | 9 | 14 |
| spectral-norm | 510 | 0.31 | 0.86 | 0.4 | 9 | 14 |
| mandelbrot | 480 | 0.20 | 0.30 | 0.7 | 8 | 14 |
| fannkuch-redux | 9 | 0.52 | 0.24 | 2.2 | 10 | 14 |
| sieve | 3500000 | 0.30 | 0.17 | 1.8 | 35 | 41 |

## 所見

機種の M4 は Mac16,1 (Apple M4)、M5 は Mac17,2 (Apple M5) である。OS はどれも macOS 27.0.1 (26A434)、rustc は 1.98.1 で、「`decref` が一意な物体を空ける道」だけ 1.95.0 である。

### `decref` が一意な物体を空ける道

- 主張: 速いモードの `decref` の速い道は、rc が 1 なら `#[inline(never)]` で `#[cold]` でない解放の連鎖を直に呼ぶ。遅い道に入れる形と、連鎖を包む関数を呼ぶ形は命令が多い
- 数値: `free` の計測で、遅い道に進む形 1,905,243,516、遅い道から `#[cold]` を外した形 1,896,524,999、直に呼ぶ形 1,785,276,496 (約6.3%、1回あたり約12命令の減り)。包む関数を呼ぶ形は 1,825,319,011。ほかの計測はどの形でも 0.4% 以内。連鎖の根を作業リストに積まない2つの形は `list` が約10.4% と約33.3% 増えた。M4、56211a5 に未コミットの変更を足した木。コミットからは再現できない
- 根拠にしている所: `crates/eml_rt/src/heap/mod.rs` の `decref` のコメント、[オブジェクトモデル](../spec/object-model.md) の「核の操作」

### 破棄処理の判定

- 主張: 箱を空けるたびに読む破棄処理の判定は、`Kind` の `match` を通さず、`Meta::has_finalizer` が種類のビットを `Foreign` の番号と直に比べる。`Kind::from_bits` の腕が1つ増えるだけで、解放の多い計測が数%増える
- 数値: 腕を1つ足すと `list` +5.56%、`free` +3.25%、`map` +3.70%。腕を除くと 0.25% 以内に戻った。M4、8b98876 に未コミットの変更を足した木。コミットからは再現できない
- 根拠にしている所: `crates/eml_rt/src/meta.rs` の `has_finalizer` のコメント、[オブジェクトモデル](../spec/object-model.md) の「種類」

### `eml_rt` のインライン化

- 主張: `eml_rt` の速い道は、呼ぶ側に展開されていることを `-C remark=inline` の「definition is unavailable」で確かめる。展開されないと、`map-reuse` と `append` の命令の数が倍を超える。原因はどれも、総称な関数の実体が呼ぶ側と別のコード生成単位に入ることか、総称でない関数が crate をまたいで展開されないことだった。次の置き方はそのためである
  - 総称な関数 (`set_field`、`field`、`string_push`、`push_unchecked`、`release_unkept`、`meta`、`scanned`) と、繰り返しの中の総称でない関数 (`release_children`、`Control::stop_bits`、`into_token`、`drop_token`) に `#[inline]` を付ける
  - `Arc` を持つ欄 (`Heap` の付けた塊、`Pages` の `Control`、`HostRef` の Host) は `ManuallyDrop` にし、`Drop` が `#[inline(never)]` の関数に渡して手放す。欄の後始末が展開されると、単位の分け方が変わる
  - 検査モードだけの確かめ (`checked_unshared`) は、呼ぶ側の `M::CHECKED` の枝の中で呼ぶ
  - `rt_extern_bench` を `rt_bench` と別のプログラムにし、`rt_bench` の単位の分け方を変えない
  - この戻し方は脆い。`release_children` は展開の閾値の近くにある
- 数値:
  - `release_children` に `#[inline]` がないと `map-reuse` が 600,306,311 から 1,281,810,080 命令になり、付けると `append` は1回あたり約73命令から約28命令になった。M4、8b98876 に未コミットの変更を足した木。コミットからは再現できない
  - 付けた塊の欄を `Option<Arc<Frozen<M>>>` のまま持つと `map-reuse` +46%、`append` +152%。`checked_unshared` を速いモードでも呼ぶと `map-reuse` +49%、`append` +152%。M4、fd26d0d に未コミットの変更を足した木。コミットからは再現できない
  - `Ctx` を計測のプログラムで使い始めたとき `map-reuse` +85%、`append` +152%。`field` を除く4つに `#[inline]` を付けた形では、同じ設定のビルドを繰り返すと `map-reuse` が約566M と約1058M 命令の間で変わり、`field` を足すと3回のビルドで約568M に留まった。`stop_bits` に付けると `poll` は1回あたり約14命令から約11命令になった。M4、28eb18d
  - `into_token` と `drop_token` に付ける前後で、26の計測のどれも 0.5% より大きくは増えなかった (最大は `println-line` の +0.227%)。M4、e5f86d8 の前の未コミットの木。コミットからは再現できない
  - VM の繰り返しから「definition is unavailable」で展開されなかった `eml_rt` の20の関数は、どれも繰り返しの外か冷えた道にあった。M4、73e31d1
- 根拠にしている所: `crates/eml_rt/src/heap/release.rs` の `release_children`、`heap/mod.rs` の `region` の欄と `checked_unshared` と `set_field`、`alloc.rs` の `Pages` の `control` の欄、`control.rs` の `unbounded`、`host.rs` の `release_host`、`examples/rt_extern_bench.rs` の先頭のコメント。[オブジェクトモデル](../spec/object-model.md) の「核の操作」、「extern」の `Ctx` を取らない算術の項、「`unsafe` の核」の T4 の行。[VM](../spec/vm.md) の「置き場所」の `eml_rt` の項

### 出力のバッファと Host の呼び出し

- 主張: 標準出力と標準エラーは `Mutex` の中の 64 KiB の `BufWriter` にまとめて書く。行ごとに flush すると約65〜160倍遅い。Host は `dyn Host` で呼び、総称の型引数にしない。動的な呼び出しの分は、書き込みの費用と比べて小さい
- 数値: 23 バイトの行を100万行書くと、行ごとの flush はファイルへ 1.30 µs/行、パイプへ 324 ns/行、`BufWriter` は 8 ns/行と 5 ns/行 (M4。設計のときの使い捨ての計測で、コミットの記録はない)。`rt.sh` の `println-block` (`dyn Host` の呼び出しを含む) は1行あたり約238命令、`println-line` は約12,131命令 (M4、42aee31)
- 根拠にしている所: [オブジェクトモデル](../spec/object-model.md) の「extern」の「出力のバッファ」と Host の項

### `Fs.read_all` の読み方

- 主張: `Fs.read_all` は `Vec` に読んでからヒープへ写す。ヒープに直接読む形は、空き領域を0で埋める費用が省いた写しとほぼ相殺するので 6〜10% しか速くならず、信頼する関数が4つ要る。mmap はファイルが縮むと SIGBUS で落ち、`Vec` をそのまま文字列にすると文字列の全操作が2つの形に分かれる。読み手を `StopOnInterrupt` で包むので、`read_to_end` は空き領域を0で埋めてから読む
- 数値: 採った形は 1 MiB で 60.2 µs、64 MiB で 7.0 ms (M4、ページキャッシュにあるファイル。設計のときの計測で、コミットの記録はない)。0で埋めるようにすると `read-all` +12.1%、`read-all-unsized` +10.0% (M4、1f36698 と 223d52f)
- 根拠にしている所: [オブジェクトモデル](../spec/object-model.md) の「`File`」
- 受け入れた理由: `read_buf` が stable の Rust にないため、中断で止められる読みを優先した

### `codegen-units`

- 主張: release のプロファイルの `codegen-units` は既定の値のままにする。1 にすると、VM の実行の分はプログラムによって減ったり増えたりする
- 数値: `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` の `bench/run.sh` を既定のビルドと比べると、実行の分は35のプログラムのうち22で減り、13で増えた。減りの最大は `fannkuch_by_hand` -6.89%、`fannkuch_var` -5.08%、`sieve_by_hand` -4.29%、増えの最大は `std_list` +3.89%、`list` +3.51%、`tree_by_hand` +3.45%、`tree` +3.32%。`state` -0.70%、`loop` -0.86%、`fib` -0.05% (M5、64c16c8)
- 根拠にしている所: [VM](../spec/vm.md) の「置き場所」の `codegen-units` の項

### `eml_vm` のインライン化

- 主張: `eml_vm` の関数には、`vm::run` に「definition is unavailable」で展開されていないことと、付けると `bench/run.sh` で命令の数が減ることの2つが分かったときだけ `#[inline]` を付ける。`data` と `apply` の9つの関数は両方に当たった。`apply::apply` は付けても変わらなかったので付けない
- 数値: 9つに付けると実行の分が `tree` 約5.4%、`list` 約3.8%、`state` 約2.8%、`fib` と `loop` 約1.2〜1.3% 減った。`apply::apply` は 0.2% 以内で、`fib` は約0.2% 増えた。基本操作の命令を `vm::step` の `match` に移すと、`state` が約17%、ほかが1〜4% 減った。M4、33cd286 から 4b5ef6c2 の間の未コミットの木。コミットからは再現できず、今のコードでは測り直していない
- 根拠にしている所: [VM](../spec/vm.md) の「置き場所」の `eml_vm` の項

### reuse の命令の関数の置き方

- 主張: `release_reuse` と `con_reuse` は `#[inline]` にして token を使う道だけを `vm::run` に展開し、値が共有されていた道 (`release_shared`) と token が空の道 (`con_fresh`) は `#[cold]` と `#[inline(never)]` の関数に出す。3つの置き方のうち、reuse するプログラムで最も減り、reuse しないプログラムの増え方も 0.5% 以内に収まった
- 数値: 基準 9ed9a349 からの実行の分の増減。`#[inline(never)]` では `tree` -4.93% (試作は -9.03%)。全部を `#[inline]` にすると reuse しない `iter_pipeline` +1.49%、`iter_pipeline_by_hand` +1.59%。採った形は `tree` -11.12%、`std_list` -14.28%、reuse しないプログラムは最大 +0.33% (`fib`)。M4、e5f86d8 の前の未コミットの木。コミットからは再現できない
- 根拠にしている所: [VM](../spec/vm.md) の「置き場所」の reuse の命令の項

### `INLINE_LIMIT` と `GROWTH_FACTOR`

- 主張: `INLINE_LIMIT` の 40 は、測った値 (10、20、40、80) のうち、どのプログラムも下げ止まる最小の値である。書き換えで生まれた呼び出しの展開の予算 `GROWTH_FACTOR` は8倍で、`bench/` と `tests/ui/run`、`tests/ui/run-fail` のどの関数も予算に届かない。予算がないと、本体が自分を k 回呼ぶ `Fix` のラムダで関数が回ごとに k 倍に育つ
- 数値: 実行の分で、`iter_pipeline` は 20 で 295,711,502、40 で 197,919,428 (20 では49% 遅い)。`upsert` は 10 で 535,233,564、20 以上で約490M (10 では9% 遅い)。80 は 40 と ±0.2% 以内。予算なしの `Fix` のラムダは、本体の呼び出し3で Core IR 157,491 行・0.31 秒・206 MB、4で 1,747,659 行・4.2 秒・2.5 GB。8倍の予算では約400行。育ち方の最大は `tests/ui/run/std/std_iter_lin.em` の `main` の 48 から 179 (3.7倍) で、4倍では余裕が13文しかない。M4、b9b2ff0 の前の未コミットの木 (定数を変えて測った)。コミットからは再現できない
- 根拠にしている所: `crates/eml_core_ir/src/optimize/fuse/mod.rs` の `GROWTH_FACTOR` と `fuse/inline.rs` の `INLINE_LIMIT`、[Core IR](../spec/core-ir.md) の「インライン化」と「融合が止まること」

### インライン化で増えるスタック

- 主張: 末尾でない再帰の中に小さな関数を展開すると、1つのフレームのスロットが増え、深い再帰でスタックの上限に早く当たる
- 数値: `upsert` の `peak_stack` は 352 語で、展開しなければ 206 語 (6f77feb)。`RunStats` の値なので機種によらない。352 は `crates/eml_vm/tests/bench.rs` のスナップショットと同じ
- 根拠にしている所: [実装の現在地](status.md) の「最適化が扱わない形」
- 受け入れた理由: 展開は、自動の版と手書きの版の組の RC の回数をそろえるために要る

### reuse の token で増えるスタック

- 主張: 末尾でない再帰の呼び出しをまたいで reuse の token を持つと、token のスロットでフレームごとに1語増え、深い再帰でスタックの上限に早く当たる
- 数値: `list` の `peak_stack` は 700,015 語で、token を持たなければ 600,014 語 (9ed9a349)。`tree_map` は 126 語と 112 語。`RunStats` の値なので機種によらず、`crates/eml_vm/tests/bench.rs` のスナップショットと同じ
- 根拠にしている所: [実装の現在地](status.md) の「実行の費用」
- 受け入れた理由: token をフレームの中に限る形 (frame-limited reuse) は、token を呼び出しの引数と戻り値で渡さない簡単な形で、その費用として受け入れた

### 1バイトずつ回すループ

- 主張: eml で1バイトずつ回すループは、`bench/loop.em` の1反復に約330命令かかる。入力を1バイトずつ数える `wc` は Python の版の約14倍の実時間で、Python の版は C で書かれた `bytes.count` と `bytes.split` に任せている。1行の読みと検索を extern に任せる `grep -n` は Python とほぼ同じになる。素のループでは VM は CPython より速い
- 数値: `loop` の実行の分 165,337,806 命令 / 500,000 反復 (M5、fc990f9)。30 MB の入力で `wc` は eml 1.01 秒、Python 0.07 秒 (14.4倍)、`grep -n` は 1.1倍 (M5、5bb2254、Python 3.14.7)。3,000万回のループは VM 0.26 秒、Python 0.66 秒で、`wc` を Python でも1バイトずつのループで書くと VM 1.56 秒、Python 1.45 秒 (M4、Python 3.14.7。リポジトリにない使い捨ての試作で、コミットの記録はない)。「今の数値」の `wc` の比は大きさが 9000 KiB で Python の実時間が小さく、この30 MB の値とは違う数になる
- 根拠にしている所: [実装の現在地](status.md) の「実行の費用」、[ロードマップ](../future/roadmap.md) のネイティブ化の項目の完了の条件

### ネイティブ化の見込み

- 主張: `wc` の内側のループを LIR のまま Rust に写すと Python の約1.2倍、借用とインライン化を足すと約0.45倍になる。ネイティブ化で Python の1.5倍以内に届く見込みはある。ただし Cranelift のコードは LLVM より遅いことがあり、確保の多い `logstat` は測っていない
- 数値: LIR をそのまま写した Rust (`opt-level=1`。1バイトごとの `dup`、`String.byte_at` の呼び出しと `decref`、`is_space` の呼び出しを残す) は読み込みを含めて約0.10秒、借用とインライン化を足すと約0.035秒。M4、Python 3.14.7。リポジトリにない使い捨ての試作で、コミットの記録はない
- 根拠にしている所: [ロードマップ](../future/roadmap.md) のネイティブ化の項目の完了の条件

### `Yield` を変える handler の yield

- 主張: `Yield a` を `Yield b` に変える handler の `fn yield` の節は、節が外側の `yield` を起こすので、外側の handler が特殊化される `Yield.fold` でも、`State.run` の中でも、1要素ごとに yield する
- 数値: 1,000 要素の `map_lists` (`tests/ui/run/effects/pure_callback_under_state.em` と同じ形) を `Yield.fold` で畳むと、`yields` 1000、`evidence_steps` 4002。`State.run` の中でも同じ値。0fff343a の検査モード (`eml_test_support::run_stats`)。`RunStats` の値なので機種によらない
- 根拠にしている所: [ロードマップ](../future/roadmap.md) の「処理系」の、yield しうる外のエフェクトを呼ぶ節をその場で実行する項目、[実装の現在地](status.md) の「実行の費用」

### 状態を渡す形にしない handler

- 主張: どの節もすぐに再開する handler でも、`perform` の結果と節の末尾の `resume k(v, s)` の Repr か印が合わないものは、状態を渡す形の条件 (`resumes_fit`) に当たって EP の道に残る。経路がすべて末尾の呼び出しでも、末尾の経路の形には切り替えない
- 数値: `bench/` には当たるプログラムがなく、`RunStats` は変わらなかった (M5、e19347bc と c2e34728)。`crates/eml_core_ir/tests/optimize.rs` の `a_resume_value_of_another_repr_keeps_the_handle` が、この handle が残ることを固定する
- 根拠にしている所: [Core IR](../spec/core-ir.md) の「handler への特殊化」
- 受け入れた理由: 1つの handler に2つの形を持たないため

### 配列の `Float` の要素の箱

- 主張: 配列の要素は一様な位置なので、`Array Float` と `MutArray Float` の要素は書くたびに `Float` の箱に入る。spectral-norm の確保のほとんどはこの箱である
- 数値: spectral-norm (既定の大きさ) は `allocations` 4043、`boxes` 4001。10回のくり返しで行列とベクトルの積を40回計算し、そのたびに100個の要素を作る (40 × 100 = 4000)。状態を渡す形の複製が返す組を構造体にしない場合は 4057 だった (e6665e64)。`RunStats` の値なので機種によらず、0fff343a の `bench.rs` のスナップショットと同じ
- 根拠にしている所: [ロードマップ](../future/roadmap.md) のスカラーの要素を生で持つ配列の項目、`bench/spectral_norm.em` の先頭のコメント、`crates/eml_vm/tests/bench.rs` の `spectral_norm_allocates_14_fewer`

### 配列に書く構造体の箱

- 主張: 配列の要素と、配置で構造体の型を宣言したフィールドは一様な位置なので、そこに書く構造体の値は箱に入る。nbody の確保はこの箱だけである
- 数値: nbody は1回進めるごとに 25 (速度に20、位置に5) で、既定の1000回で `allocations` 25051。計算の途中の `Vec3` と、状態を渡す形の複製が返す組は確保しない。`RunStats` の値なので機種によらず、0fff343a の `bench.rs` のスナップショットと同じ
- 根拠にしている所: [ロードマップ](../future/roadmap.md) の構造体のフィールドを親の物体に並べることの項目、[実装の現在地](status.md) の「実行の費用」、`bench/nbody.em` の先頭のコメント、`crates/eml_vm/tests/bench.rs` の `nbody_allocates_25_per_step`

### 呼び出しの規約の引き換え

- 主張: 関数の値の、捕まえた値より後ろの引数と結果をいつも一様にする規約 ([LIR](../spec/lir.md) の「関数の ABI」) には、次の費用がある
  - 特殊化されない関数の値に `Float` や構造体を渡すと、呼ぶたびに引数と結果が箱に入る。`Int` の箱はタグを付けるだけで、確保しない
  - 値として参照する型どおりの関数には、版 `<f>$uniform` ができる。末尾の位置で `apply` か `clause` に届く関数の版は本体を写した写しの形になり、コードが増える
  - 値として生まれて値としても参照される関数は、直接呼ぶ所でも引数と結果を一様にして渡す
  - 直接だけ呼ばれる持ち上げた関数と、値の参照が残らない handler への特殊化の継続の関数は、型どおりのままなので、この費用を持たない ([Core IR](../spec/core-ir.md) の「直接だけ呼ばれる持ち上げた関数」)。節の写しは `perform` の地点ごとに継続の関数について複製するので、継続を閉包にせず、再開の値と状態も箱に入らない ([Core IR](../spec/core-ir.md) の「節の写しの、継続についての複製」)。その代わり、節の本体のコードは地点の数だけ写る
- 数値:
  - `apply_float` (10万回の `apply`) は `boxes` 200000、`allocations` 200002 で、呼ぶたびに引数と結果の箱を1つずつ確保する。`apply_int` も `boxes` 200000 だが、`allocations` は 2 である
  - `ep_float` の `allocations` は10万回の操作で 9 である
  - `unspecialized_map_passes_floats.em` は `boxes` 20、`allocations` 22。`boxes` はリストの10個の要素の箱と、ラムダの版 `main$lambda0$uniform` が要素ごとに結果を入れる箱である
  - `bench/` と `tests/ui/run*` のプログラムの reuse の後の LIR は、関数が 3685、文が 33432 で、版が 54、そのうち写しの形が 17 である ([テスト戦略](testing.md) の「開発用の数え方」)
  - d650bdec の値。`RunStats` の値なので機種によらず、`bench.rs` のスナップショットと同じ
- 根拠にしている所: `crates/eml_vm/tests/bench.rs` の `trade_offs`、`bench/apply_int.em`、`bench/apply_float.em`、`bench/ep_float.em` と `tests/ui/run/functions/unspecialized_map_passes_floats.em` の先頭のコメント
- 受け入れた理由: 実行時に Repr を変える仕組みが要らず、`apply` は引数の数だけを比べればよい ([VM](../spec/vm.md) の「呼び出しと戻り」)。末尾の位置の呼び出しの鎖ごとに残るフレームは多くとも1つで ([LIR](../spec/lir.md) の「末尾の位置の呼び出し」)、ネイティブ化では Cranelift の `return_call` をそのまま使える。ABI は名前で決まるので、REPL の追記とネイティブのコードの使い回しで食い違わない

### 構造体の表現の引き換え

- 主張: 小さな単一コンストラクタの型とタプルを構造体にしたことで、増えた費用が2つある
  - 特殊化されない `map` に組を入れ替える関数の値を渡すと、ラムダが要素の組を箱から取り出して入れ替え、新しい箱に入れて返す。共有した組を展開しない関数に渡すと、RC の葉ごとに `dup` する
  - `Option` の中の組を構造体の `unbox` と `release unboxed` で読むので、`iter_pipeline` と `orders` の実行の分が増えた
- 数値:
  - `swap_map_unspecialized.em` は `allocations` 42、`reuses` 50。`swap_map_specialized.em` は構造体にしない場合と同じ 32 と 50 (`SWAP_MAP_BEFORE_STRUCTS`)。`three_rc_leaves_through_a_call.em` は `rc_increments` 3。今の値は d650bdec の `trade_offs` のスナップショットと同じ
  - `iter_pipeline` +3.4%、`iter_pipeline_by_hand` +3.5%、`orders_by_hand` +2.0%、`orders` +1.3%。M5、1a91f47 に未コミットの変更を足した木。コミットからは再現できない
- 根拠にしている所: `crates/eml_vm/tests/bench.rs` の `trade_offs` と `SWAP_MAP_BEFORE_STRUCTS`、`tests/ui/run/records/swap_map_unspecialized.em` と `three_rc_leaves_through_a_call.em` の先頭のコメント
- 受け入れた理由:
  - 組の箱と葉の `dup`: データに通した関数の値は引数と結果が一様なので、組は箱で渡る。構造体の表現で、`records` -26.9%、`mandelbrot` -24.7% と、確保の多いプログラムの実行の分が大きく減った
  - `Option` の組の2命令: 増えたのは組を読む2つの命令の分と見ている。VM の中までは調べていない

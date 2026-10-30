# 計測の基準

位置づけ: 手引き。

実行の費用を測るための、基準のプログラムと測り方と、今の数値をまとめる。費用は2つの数で見る。

- 実行の仕事の回数 (`RunStats`): 決定的なので、テストのスナップショットで固定する。回数が変わると、スナップショットの差分として出る
- 実行した命令の数: macOS の `/usr/bin/time -l` が出す `instructions retired` を、`bench/run.sh` と `bench/rt.sh` で測る。スクリプトと数値計算は、Python と実時間で比べ、スクリプトは大きい入力で Rust の実装とも比べる (`bench/scripts.sh`)

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
| `state_loop_by_hand.em` | `state_loop.em` のループの状態を、引数で手で受け渡したもの | `state_loop.em` の下限 |
| `state_nested.em` | 状態つきの handler を2つ重ねたループ | 入れ子の handler の特殊化 |
| `apply_int.em` | 実行時に選んだ `Int -> Int` の関数の値を、ループの中で10万回 `apply` する | 引数と結果を一様な語で渡す `apply` |
| `apply_float.em` | `apply_int.em` を `Float -> Float` にしたもの | `Float` の引数と結果が `apply` を通る費用 |
| `ep_float.em` | `Float` の結果と `Int` の状態を持つ操作を、特殊化しない handler の下で10万回起こす | EP の道で、操作の結果と状態を運ぶ費用 |
| `fold_state.em` | `State.run` の中で `Yield.fold` に純粋なラムダを渡し、10万要素を合計する | 純粋なコールバックを節がその場で呼ぶこと (`yields` が0) |
| `list.em` | プログラムの中で定義した `List a` を `range`、`map`、`foldl` で合計する | 型変数のフィールドの `Int`、高階関数 |
| `tree.em` | 線形合同法で作った `Int` を二分探索木に入れ、総称な `size` と `fold` で畳む | 再帰的なデータ、`switch` と `unpack` |
| `orders.em` | 10万件の `Order` のレコードを、ラムダを渡した `List.filter`、`List.map`、`List.fold` で集計する | リストの高階関数に渡すラムダ |
| `orders_by_hand.em` | `orders.em` のラムダを各ループに書き込んだもの | `orders.em` の特殊化が目指す下限 |
| `push_fold.em` | `Yield.fold (fn acc x -> acc + x) 0 (nums 100000)` | `Yield` の畳み込みに渡すラムダ |
| `push_fold_by_hand.em` | 専用の `sum` handler に `acc + x` を書き込んだもの | `push_fold.em` の下限 |
| `iter_pipeline.em` | `Iter.generate (nums 100000)` を `Iter.filter`、`Iter.map`、`Iter.fold` に通す | `Iter` の段ごとのラムダ |
| `iter_pipeline_by_hand.em` | 専用の `evens`、`squares`、`sum` にしたもの | `iter_pipeline.em` の下限 |
| `iter_generate.em` | `Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (nums 100000))` | `Iter.generate` が生産者を包む費用 |
| `sum_loop.em` | 1 から 100000 までの合計を直接の末尾再帰で求める | `iter_generate.em` と `push_fold.em` を比べる基準 |
| `byte_loop.em` | 10 万バイトの文字列を作り、`String.byte_at` で先頭から足し上げる | 1バイトずつ読むループの RC (借用の extern は `rc_increments` が0) |
| `char_loop.em` | `byte_loop.em` と同じ文字列を、`String.char_at` で1文字ずつ読んで足し上げる | 汎用の extern の命令で文字を読むループ (`byte_at` の専用の命令との差) |
| `bytes_loop.em` | 10 万バイトのバイト列を `Bytes.push` で作り、`Bytes.get` で先頭から足し上げる | バイト列の `push` が容量を倍にして伸ばすこと (確保が log2 に比例する) と、1バイトずつ読むループの RC (`rc_increments` が0) |
| `tree_by_hand.em` | `tree.em` の `fold (fn acc x -> acc + x)` を専用の `fold_sum` にしたもの | `tree.em` の下限 |
| `upsert.em` | AA 木の `upsert key increment` で 20000 個の文字列のキーを数え、`sort_by (fn (_, a) (_, b) -> a > b)` で並べる | 高階の更新関数と比較関数 |
| `upsert_by_hand.em` | `increment` と比較を `upsert` と `sort_by` に書き込んだもの | `upsert.em` の下限 |
| `records.em` | 4つのフィールドのレコードを、`{ st \| … }` で10万回更新する | 構造体の値の更新 (確保も reuse もしない) |
| `records_wide.em` | `records.em` を7つの `Int` のフィールドのレコードにしたもの | 構造体にしない値の reuse |
| `rbtree.em` | 赤黒木に1万個の `Int` を入れる。`balance` は組み直す節を受ける | 同じ関数の中で節を作り直す reuse |
| `tree_map.em` | 8191 節の完全二分木の全節を、`incr` で20回作り直す | 書かないフィールドとその場の書き換え |
| `par_tree.em` | 4095 節の完全二分木の全節に、上の深さ 4 までを `par` で分けて 1 を足し (15 回の fork)、逐次にもう一度足す | fork と join の仕事。右の子は共有の印の付いた木から新しく作り、左の子と join の後の親はその場で作り直す |
| `std_list.em` | 2万要素のリストに、std の `List.map` と `List.filter` を5回通す | std のリストの関数の reuse |
| `std_map.em` | std の `Map.upsert` で 20000 個の文字列のキーを数え、`List.sort_by` で件数の降順に並べる (`upsert.em` と同じ入力) | 釣り合いを直す関数が受けた節の箱を使い回すことと、並べ替えが手放すセルを使い回すこと |
| `sieve_var.em` | 引数 (既定は 20000) より小さい素数の数を、`var` で書いたふるいで数える | 入れ子の `var` の、状態を渡す形の特殊化 |
| `sieve_by_hand.em` | `MutArray` を末尾再帰のループに線形に受け渡したもの | `sieve_var.em` の下限 |
| `fannkuch_var.em` | fannkuch-redux (既定は n = 7) を、3本の配列の変数と10本の `Unr` の変数で書く | 何重にも入れ子になった `var` と、配列の添字 |
| `fannkuch_by_hand.em` | 状態をすべて末尾再帰の局所関数に受け渡したもの | `fannkuch_var.em` の下限 |
| `nbody.em` | Benchmarks Game の5つの天体を、`Vec3` の値で引数の回数 (既定は 1000) だけ進める | 配列に書く構造体の箱と `Float` の算術 |
| `spectral_norm.em` | 引数の大きさ (既定は 100) のスペクトルノルムを、`Array Float` と `var` で求める | 配列の `Float` の要素の箱 |
| `mandelbrot.em` | 引数の大きさ (既定は 128) の正方形の格子で、集合に入る点を `Complex` の値で数える | 構造体の値の算術 |

`nbody.em`、`spectral_norm.em`、`mandelbrot.em`、`sieve_var.em`、`fannkuch_var.em` は、大きさを省略できる引数で受ける。引数がなければ表の既定の大きさで動き、回数のテストと `bench/run.sh` はこの大きさで測る。`bench/scripts.sh` は大きな大きさを渡して Python の実装と比べる (下の「スクリプトの計測」)。大きさを読むために `Std.Env` と `String.to_int` のコードがプログラムに入るので、この5つの実行の分には、それをコンパイルして動かす約 6608 万命令が加わる (「今の数値」の環境で、大きさを読むだけのプログラムを測った値)。

`_by_hand` のプログラムは、対応する自動に任せたプログラムと同じ出力を出す。特殊化の効果は、自動に任せたプログラムの `RunStats` が手書きのプログラムに近づくことで測る。

`list.em` と `tree.em` はデータ型を自分で定義する。2つの多相な関数 (`map`、`foldl`、`size`、`fold`) では、型変数の位置の `Int` が `box` と `unbox` を通る。

`bench/` のプログラムは std の `List` を `import Std.List` と書く。`bench/run.sh` と `bench/scripts.sh` は `bench/` を根にして読むので、`import List` は大文字小文字だけが違う `bench/list.em` を見つけて E1026 になり、標準ライブラリに進まない ([モジュールと名前解決](../spec/modules.md) の「標準ライブラリ」)。回数のテストはエントリのファイルだけをメモリの上の木 (`MemorySource`) から読むので、`import List` と書いても通り、この誤りに気づかない。

## 回数のテスト

`crates/eml_vm/tests/bench.rs` は、プログラムごとに1つのテストで、VM の検査モードで走らせ、出力を確かめ、`RunStats` の全項目 ([VM](../spec/vm.md) の「`RunStats`」) をインラインスナップショットで固定する。`every_program_in_bench_has_a_test` は、`bench/` のファイルの一覧とテストのプログラムの一覧が一致することを確かめる。回数は機械の速さに左右されないので、ふだんの実行で流す。

`par_tree` のテストは、逐次の方針で回数を固定し、seed 1 と 2026 の方針でもスケジュールによらない欄が同じであることを確かめる ([テスト戦略](testing.md) の「seed で流す」)。

プログラムをまたいで比べるテストは次のとおりである。

- `specialization_matches_hand_written_work` と `specialization_matches_hand_written_rc`: 自動に任せたプログラムと `_by_hand` のプログラムの6つの組で、`allocations`、`boxes`、`unboxes` と RC の回数が同じである
- `handler_specialization_removes_yields_and_evidence`: `iter_generate` と `iter_pipeline` の `yields` と、`push_fold`、`fold_state`、`state_loop`、`state_nested` の `evidence_steps` が0である
- `iter_allocations_do_not_grow_with_the_input` と `iter_generate_allocates_as_much_as_the_direct_loop`: `Iter.fold` で引く形の確保の数が入力の大きさによらず、直接のループ `sum_loop` と同じである。`Iter.filter` を重ねた形は、残した要素ごとに4つ確保する
- `rebuilding_programs_reuse_boxes`: 関数の中で手放して作り直す8つのプログラムで `reuses` が0より大きい。`allocations_are_counted_in_checked_mode` は、確保の数が検査モードの数え方から来ることを確かめる
- `var_matches_hand_written_work`: `var` で書いたプログラムと手書きのプログラムの2つの組で、`var` のプログラムの `evidence_steps` と `yields` が0で、確保の数の差が表 `RESIDUAL` の値に等しい
- `nbody_allocates_25_per_step`、`mandelbrot_does_not_allocate_complex`、`spectral_norm_allocates_14_fewer`: 構造体の表現で消える確保と残る確保の数 (下の「配列に書く構造体の箱」と「配列の `Float` の要素の箱」)
- `unwind_points`: `bench/` と `tests/ui/run/scripts/` のプログラムの巻き戻しの表の行の数を固定する
- `trade_offs`: 呼び出しの規約と構造体の表現の引き換えの形の回数を固定する (下の「呼び出しの規約の引き換え」と「構造体の表現の引き換え」)

回数を変える変更は、期待値の変更として段の設計 (`docs/plans/`) に書く ([テスト戦略](testing.md) の「テストの変更の運用」)。

## 測り方

命令の数は `bench/run.sh` と `bench/rt.sh` で、Python と Rust の実装との実時間の比は `bench/scripts.sh` で測る。どれも macOS の `/usr/bin/time -l` を使うので、macOS でだけ動く。

### 命令の数の測り方

```sh
bench/run.sh
```

`bench/run.sh` は、release ビルドの `eml run` (VM の速いモード) で各プログラムを3回ずつ走らせ、`instructions retired` と実時間の中央値と、実行の分を Markdown の表で出す。実行の分は、各プログラムの命令の数から `empty` の命令の数を引いた値である。`empty` はフロントエンド、コード生成、起動の費用なので、プログラムどうしと、測り直した表どうしは実行の分で比べる。実行の分が 0 以下になったときは、理由を出して止まる。比べられる値でないものを記録しないためである。表の前に、測定の環境 (機種と CPU、macOS のバージョン、`rustc -V`、コミット) を出す。コミットしていない変更があれば、コミットの行に未コミットの変更を示す印が付く。プログラムが実行時エラーで止まったときと、`time -l` の出力から命令の数か実時間を読めなかったときは、その出力を見せて止まる。macOS でだけ動く。

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
| `handle` | handler を `evv_get`、`ev_new`、`ev_set_clause`、`evv_push`、`evv_set` で設け、`prompt$` と同じく `ev_take_evv` と `evv_set` で外すことを、1,000,000 回繰り返す |
| `yield` | yield を始め、κ を取り出して入れ直し、yield を終える1往復を、10,000,000 回繰り返す |
| `mask` | 4つの evidence の `evv` で、2つ目の evidence を飛ばす `evv_mask` (項目は n = 0、m = 1 の1つ) を呼び、結果を手放すことを、10,000,000 回繰り返す |
| `poll` | 安全点の確かめ `Ctx::poll` を、100,000,000 回繰り返す |

`bench/rt.sh` は続けて、extern の計測のプログラム `crates/eml_rt/examples/rt_extern_bench.rs` の次の27の計測を、同じ方法で測って2つ目の表に出す ([オブジェクトモデル](../spec/object-model.md) の「extern」)。`rt_bench` と別のプログラムにするのは、上の14の計測のコード生成単位の分け方を変えないためである (下の「`eml_rt` のインライン化」)。どの計測も速いモードの `Ctx` で動かす。引数を消費する extern に渡す文字列の参照は、ヒープの文字列なら毎回 `dup` で、不死の文字列 (`concat-copy` の右と `read-all` のパス) なら `acquire_immortal` で作る。借りる extern (`string-eq`、`compare`、`show-string`、`find-byte-1mib`、`find-bytes-1mib`) には、呼ぶ側が持つ参照をそのまま渡す。`-position` と `-windows` の行は、同じ探索を Rust の標準ライブラリで行う比べる基準で、`box-list` と同じく `eml_rt` を呼ばない。`show-int` は整数だけを渡すので、参照を作らない。

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
| `find-byte-1mib` | 52 バイトの行を並べた 1 MiB のバイト列の末尾の1バイトを `Bytes.find_byte_from` で探すことを、1,000 回繰り返す |
| `find-byte-1mib-position` | 同じ探索を `<[u8]>::iter().position` で行う |
| `find-bytes-1mib` | 同じ並びの末尾の 16 バイトを `Bytes.find_from` で探すことを、100 回繰り返す。探す列の先頭のバイトは行ごとに1回合い、そのたびに 15 バイト目まで比べて外れる |
| `find-bytes-1mib-windows` | 同じ探索を `<[u8]>::windows().position` で行う |
| `find-byte` | 52 バイトの行で次の `\n` を `Bytes.find_byte_from` で探すことを、10,000,000 回繰り返す |
| `find-byte-swar` | 同じ探索を、Rust だけで書いた 8 バイトずつの SWAR のループで行う |
| `find-sub` | 16 バイトの needle を 52 バイトの行で `Bytes.find_from` で探すことを、10,000,000 回繰り返す |
| `find-sub-contains` | 同じ探索を `str::contains` で行う |
| `slice-short` | 52 バイトの `Bytes` の先頭 50 バイトを `Bytes.slice` で切り出して手放すことを、10,000,000 回繰り返す |
| `slice-short-box` | 同じことを `Box<[u8]>::from(&s[..50])` と drop で行う |
| `read-all-bytes` | メモリの Host の 64 MiB のファイルを、開いて全部読んで閉じることを、8 回繰り返す。読みは `handle_read` の `Form::Bytes` で、UTF-8 を検査しない |
| `read-all-bytes-std` | 同じ 64 MiB を `&[u8]` から `read_to_end` で読むことを、8 回繰り返す |
| `read-line-bytes-64mib` | 56 バイトの行を並べたメモリの Host の 64 MiB のファイルを、`handle_read` の `Form::Bytes` の行の読みで1行ずつ読み、行を手放す |
| `read-line-bytes-64mib-std` | 同じ 64 MiB を、64 KiB の `BufReader` の `read_until` で行ごとに新しい `Vec` に読んで落とす |
| `stdout-write-line-10m` | 23 バイトの行を、`Stdout` のハンドルの `write_line` (`handle_write` の `Write::Line`) で、`io::sink` へまとめて書く `StdHost` に 10,000,000 回書く |

`find-byte-1mib` と `find-bytes-1mib` は 1 MiB を走査する行で、比の目標は約2倍である。`find-byte` から `read-all-bytes` までの4組は、行ごとの処理の予算の行で、`bench/rt.sh` が eml の行と参照の行の命令の数の比と実時間の比を、組ごとに表にして出す。予算は次のとおりである。`find-byte` が 1.5 以下、`find-sub` が 1.6 以下、`slice-short` が 1.0 以下、`read-all-bytes` が 1.3 以下である。`find-byte` の予算は `grep -n` と `logstat` の行ごとの探索から、`find-sub` の予算は `grep -n` の行ごとの検査と測った SWAR の差から、`slice-short` の予算は `grep -n`、`logstat`、`csv2jsonl` の行ごとの確保の見積もりから、`read-all-bytes` の予算は読みの見込みから決めた。最後の `read-line-bytes-64mib` の組は、行の読みの費用を記録するだけで、予算を持たない。`stdout-write-line-10m` も、ハンドルの1回の書き込みの費用 (extern の呼び出し、`dyn Host` の呼び出し、競合しない lock、buffer への写し) を記録するだけの行で、参照の行と予算を持たない。

### スクリプトの計測

`tests/ui/run/scripts/` のスクリプトのうち入力の大きさが効く4本と、手元の TLS の server から `Std.Http` で本文を取る `https-get` と、数値計算の5つのプログラムを、Python と比べる。目標は、ロードマップの速さの目標 (Python 3.14 の 1.5 倍以下の実時間) である。`bench/` の基準のプログラムと違い、命令の数でなく実時間と最大の常駐メモリを見る。比べる相手の Python の実行が、命令の数で比べられる形でないためである。

```sh
bench/scripts.sh
```

- 表は3つある。スクリプトの表は `wc` (ログを標準入力から)、`grep -n` (約 0.5% の行が一致するパターン)、`logstat` (1行ずつ読む)、`logstat --whole` (全体を読んでから分ける)、`csv2jsonl` (3つの列を選ぶ)、`https-get` (600 MiB の本文を https で取る) の6行である。数値計算の表は nbody、spectral-norm、mandelbrot、fannkuch-redux、ふるいの5行である。3つ目の表は、大きい入力で Rust の実装と比べる (下の「C/Rust との比べ方」)
- スクリプトの入力は、`bench/scripts/gen.py` が種類 (アクセスログか CSV) と大きさごとに、種を固定して1つずつ作る。`target/bench-scripts/` に大きさを名前に入れて置き、すでにあれば作り直さない
- 比べる Python の実装は、スクリプトが `bench/scripts/<名前>.py`、数値計算が `bench/numeric/<名前>.py` で、Python の標準ライブラリで普通に書いたものである。速さのために手で凝ったものにはしない。数値計算の実装は、浮動小数の演算を eml の実装と同じ順で行い、ビットまで同じ結果を出す
- `https-get` の相手は `crates/eml_tls/examples/https_server.rs` で、`127.0.0.1` で待ち受け、`eml_tls` の fixtures の証明書と TLS 1.3 の `TLS13_AES_256_GCM_SHA384` だけで応える。両方のクライアントが同じ暗号を使うためである。本文は番号を振った 16 バイトの行の列で、`Content-Length` を付け、close_notify の後に書く側を閉じる。chunked にしないのは、chunk の解析でなく `Net` と `Tls` の読みの道を測るためである。名前の解決を測らないよう、URL は `https://127.0.0.1:PORT/` にする。`https_server MIB` で呼ぶと MIB MiB の本文を返す server になり、ポートを標準出力の1行目に出し、`bench/scripts.sh` が signal で止めるまで動く。`bench/scripts.sh` は server を組んで裏で起こし、比べと6回の計測で使い、最後に止める。server の CPU 時間を標準エラーに出す。両方の実装は状態、本文の長さ、最後の行を出す。eml の実装は `bench/scripts/https_get.em` にある。UI テストはテストの driver で動き、本物のソケットに届かないためである。`bench/scripts/` には、std のモジュールと大文字小文字を除いて同じ名前の `.em` を置かない。置くと `import Http` などがそのファイルを見つけて E1026 になる (上の `bench/list.em` と同じ)。目標はほかの行と同じく Python の 1.5 倍以下で、CI に上限は置かない
- `https-get` を測ったら、server が eml の速さを抑えていないことを確かめる。標準エラーの server の CPU 時間の増分を接続の数 (8) で割り、eml の実時間の中央値の半分より短ければよい。半分以上なら、server の書きの塊を 1 MiB から大きくして測り直す。macOS の Python が `TLS13_AES_256_GCM_SHA384` で handshake できなければ、`compare.sh` が計測の前に止まる。そのときは server の暗号を絞るのをやめ、上の箇条から暗号の文を消す
- 各行の前に `bench/scripts/compare.sh` で、eml と Python の標準出力と終了コード (と書き出したファイル) が一致することを確かめる。一致しなければ止まる
- release ビルドの `eml run` (VM の速いモード) と `python3 -I` を3回ずつ走らせ、`/usr/bin/time -l` の実時間と最大の常駐メモリの中央値を出す。`bench/run.sh` と同じく macOS でだけ動く

#### 大きさ

行ごとの大きさ (入力の KiB か、プログラムに渡す大きさ) は `bench/scripts.sh` が持ち、次の規則で決める。

- 記録する機種で、eml の実時間 (3回の中央値) がおよそ 300 ms になる大きさにする。Python の実時間はその大きさでの値をそのまま記録する
- 段を閉じる計測で、eml の実時間が 150 ms より短いか 600 ms より長い行は、`bench/scripts.sh` の大きさを変えて、閉じるコミットの前の別のコミットにし、その後で3つの表を測り直す。表に載せるのは新しい大きさの値だけで、変えた行と前後の値は閉じるコミットのメッセージに書く
- 表の「大きさ」の欄に、どの大きさで測ったかを残す
- 実時間は 0.01 秒の単位で出るので、300 ms では約 3% の刻みになる。小さな差は `instructions retired` で確かめる。Python の実時間が数十 ms の行 (`wc`) では、比が刻みの分だけ大きく揺れる
- fannkuch-redux の大きさは順列の長さ n で、n が1つ増えると実時間が約9倍になる。300 ms に近い n がないので、300 ms との比が最も小さい n にし、実時間が 150 ms から 600 ms の範囲を外れても決め直さない

`bench/scripts/compare.sh` は3つの形で使う。`--paths <eml の実装> <Python の実装> [引数...]` は、パスで渡した2つの実装を比べる。`<名前> [引数...]` は、`tests/ui/run/scripts/<名前>/main.em` と `bench/scripts/<名前>.py` を比べる。`--fixture <名前>` は、UI テストの付属のファイルで出力を確かめる ([テスト戦略](testing.md) の「分類」)。どちらかのファイルがなければ、比べずに止まる。先頭に `--rust` を置くと、相手を Python の実装から `bench/scripts/rust/<名前>.rs` に替える。`rustc --edition 2024 -C opt-level=3` で `target/bench-scripts/rust/<名前>` に build してから動かす。

#### C/Rust との比べ方

ネイティブ化の目標は、`wc`、`grep -n`、`logstat`、`csv2jsonl` で、同じ出力を返す Rust の実装の約2倍以内の実時間にすることである ([ネイティブ化の設計](../future/native.md) の「目的と範囲」)。Python の1.5倍以下という完了の条件は変えず、この目標は門にしない。核だけのベンチは、原因を見るためだけに使う。

- 対象: `bench/scripts.sh` のスクリプトの表の、`https-get` のほかの5行 (`wc`、`grep -n`、`logstat`、`logstat --whole`、`csv2jsonl`)
- 基準: スクリプトごとに1つの Rust のプログラム `bench/scripts/rust/<名前>.rs` を置く。安全な std だけで書き、crate、`unsafe`、明示した SIMD を使わない。`rustc --edition 2024 -C opt-level=3` (既定の target CPU) で build する。eml の実装の手順をなぞる
  - 入力を走査する回数が同じである
  - 意味の仕事が同じである (eml が `String` を読む所では UTF-8 を確かめる)
  - eml が呼ぶ std の操作 (`split`、`lines`、`contains`、整数の解析) には、std の対応物を使う
  - 同じ計算量の一番近い std のデータ構造を使う (順序つきの木は `BTreeMap`)
  - 確保と借用のやり方は自由である。eml の実装が手順を変えたら、Rust の実装も書き直す
- 等しさ: 標準出力、終了コード、書いたファイルが eml と同じことを `bench/scripts/compare.sh --rust` で確かめる。`bench/scripts.sh` は各行を測る前にこれを流し、違えば止まる
- 入力: `bench/scripts/gen.py` が作る log (約 102 MB) と CSV (約 51 MB) を `target/bench-scripts/access-100000k.log` と `target/bench-scripts/data-50000k.csv` に置く。Rust の実装は bench の大きさの入力では 5 から 40 ms で、揺れが大きいためである
- 測り方: 実時間の5回の中央値を、ほかの表と同じ機種で測る。(最大 - 最小) / 中央値 が 10% を超えたら、1回だけ測り直してその値を使う。比 = (t_eml - t_eml_empty) / (t_rust - t_rust_empty) で、`_empty` はそれぞれの空のプログラムの実時間である。起動の時間は比から引くので、ネイティブの eml が AOT か JIT かに比が左右されない。今の空のプログラムは `time -l` の分解能 (10 ms) より短く、表では 0.00 になる
- 2倍を超えた行は、主な原因を所見に書く。原因の候補は、1バイトずつの呼び出し、RC、確保、写し、境界と桁あふれの確かめ、ベクトル化の有無、読み込みである

比べる相手は、同じ1回の走査を書いた Rust のループである。2回の走査にした慣用の Rust は、書き方で結果が変わり、eml の側にベクトル化が要るので基準にしない。SIMD の crate や GNU と BSD の道具は、SIMD があるかを測ることになるので基準にしない。C の実装は、道具が1つ増え、std の順序つきの写像がないので作らない。


## 今の数値

段を閉じるときに入れ替える3つの表である。

### `bench/run.sh`

- 機種: Mac17,2 (Apple M5)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.99.1 (58cf8f9c3 2026-10-13)
- コミット: d4a6e71

| プログラム | instructions retired | 実行の分 | 実時間 (s) |
|---|---:|---:|---:|
| `empty` | 58160546 | | 0.00 |
| `apply_float` | 191981586 | 133821040 | 0.00 |
| `apply_int` | 160936832 | 102776286 | 0.00 |
| `byte_loop` | 218892212 | 160731666 | 0.01 |
| `bytes_loop` | 159819309 | 101658763 | 0.00 |
| `char_loop` | 259470817 | 201310271 | 0.01 |
| `ep_float` | 242335624 | 184175078 | 0.01 |
| `fannkuch_by_hand` | 198600786 | 140440240 | 0.01 |
| `fannkuch_var` | 319436661 | 261276115 | 0.01 |
| `fib` | 144182444 | 86021898 | 0.00 |
| `fold_state` | 119918271 | 61757725 | 0.00 |
| `iter_generate` | 130882107 | 72721561 | 0.00 |
| `iter_pipeline` | 237669876 | 179509330 | 0.01 |
| `iter_pipeline_by_hand` | 238775520 | 180614974 | 0.01 |
| `list` | 216426082 | 158265536 | 0.00 |
| `loop` | 224286694 | 166126148 | 0.00 |
| `mandelbrot` | 523269457 | 465108911 | 0.02 |
| `nbody` | 233886052 | 175725506 | 0.01 |
| `orders` | 317189231 | 259028685 | 0.01 |
| `orders_by_hand` | 318645427 | 260484881 | 0.01 |
| `par_tree` | 81540906 | 23380360 | 0.00 |
| `push_fold` | 116486963 | 58326417 | 0.00 |
| `push_fold_by_hand` | 116912232 | 58751686 | 0.00 |
| `rbtree` | 272745440 | 214584894 | 0.01 |
| `records` | 148386162 | 90225616 | 0.00 |
| `records_wide` | 220747684 | 162587138 | 0.01 |
| `sieve_by_hand` | 130080477 | 71919931 | 0.00 |
| `sieve_var` | 185232827 | 127072281 | 0.01 |
| `spectral_norm` | 568802196 | 510641650 | 0.02 |
| `state` | 96180504 | 38019958 | 0.00 |
| `state_loop` | 98066119 | 39905573 | 0.00 |
| `state_loop_by_hand` | 95752347 | 37591801 | 0.00 |
| `state_nested` | 109378390 | 51217844 | 0.00 |
| `std_list` | 223099649 | 164939103 | 0.00 |
| `std_map` | 371998841 | 313838295 | 0.01 |
| `sum_loop` | 93612583 | 35452037 | 0.00 |
| `tree` | 250205909 | 192045363 | 0.01 |
| `tree_by_hand` | 249575408 | 191414862 | 0.01 |
| `tree_map` | 197855875 | 139695329 | 0.00 |
| `upsert` | 513381343 | 455220797 | 0.02 |
| `upsert_by_hand` | 511914543 | 453753997 | 0.02 |

### `bench/rt.sh`

- 機種: Mac17,2 (Apple M5)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.99.1 (58cf8f9c3 2026-10-13)
- コミット: d4a6e71

`rt_bench` の計測

| 計測 | instructions retired | 実時間 (s) |
|---|---:|---:|
| `empty` | 16892473 | 0.00 |
| `list` | 1224255277 | 0.04 |
| `box-list` | 3056712417 | 0.09 |
| `dup` | 276978719 | 0.01 |
| `free` | 1826955980 | 0.05 |
| `map` | 1810061290 | 0.06 |
| `map-reuse` | 610115659 | 0.02 |
| `append` | 278540987 | 0.01 |
| `perform` | 516977812 | 0.01 |
| `perform-deep` | 866964845 | 0.03 |
| `handle` | 607922550 | 0.01 |
| `yield` | 367035605 | 0.09 |
| `mask` | 3807113492 | 0.10 |
| `poll` | 1117011384 | 0.04 |

extern の計測 (`rt_extern_bench`)

| 計測 | instructions retired | 実時間 (s) |
|---|---:|---:|
| `string-eq` | 345124159 | 0.01 |
| `string-eq-early` | 437186745 | 0.01 |
| `compare` | 345297326 | 0.01 |
| `concat-copy` | 2615116876 | 0.09 |
| `show-int` | 2107044194 | 0.06 |
| `show-int-long` | 5257193911 | 0.15 |
| `show-string` | 3392231261 | 0.14 |
| `show-string-dense` | 8435026129 | 0.27 |
| `println-block` | 254185591 | 0.00 |
| `println-line` | 11143369201 | 0.62 |
| `read-all` | 125656941 | 0.00 |
| `read-all-unsized` | 137104938 | 0.00 |
| `find-byte-1mib` | 1211450931 | 0.03 |
| `find-byte-1mib-position` | 6323519180 | 0.23 |
| `find-bytes-1mib` | 422936938 | 0.01 |
| `find-bytes-1mib-windows` | 3282450074 | 0.12 |
| `find-byte` | 1307065419 | 0.04 |
| `find-byte-swar` | 1047043487 | 0.03 |
| `find-sub` | 2077045279 | 0.06 |
| `find-sub-contains` | 1537060217 | 0.05 |
| `slice-short` | 2797086741 | 0.08 |
| `slice-short-box` | 2600734930 | 0.08 |
| `read-all-bytes` | 423647058 | 0.04 |
| `read-all-bytes-std` | 212984453 | 0.02 |
| `read-line-bytes-64mib` | 657603650 | 0.02 |
| `read-line-bytes-64mib-std` | 780467419 | 0.03 |
| `stdout-write-line-10m` | 3027650123 | 0.10 |

eml / 参照の比 (1 MiB の2組は目標が2倍、続く4組は予算の行、最後の組は記録だけの行)

| eml の計測 | 参照の計測 | 命令の数の比 | 実時間の比 |
|---|---|---:|---:|
| `find-byte-1mib` | `find-byte-1mib-position` | 0.19 | 0.13 |
| `find-bytes-1mib` | `find-bytes-1mib-windows` | 0.13 | 0.08 |
| `find-byte` | `find-byte-swar` | 1.25 | 1.33 |
| `find-sub` | `find-sub-contains` | 1.35 | 1.20 |
| `slice-short` | `slice-short-box` | 1.08 | 1.00 |
| `read-all-bytes` | `read-all-bytes-std` | 1.99 | 2.00 |
| `read-line-bytes-64mib` | `read-line-bytes-64mib-std` | 0.84 | 0.67 |

#### 予算を超えた行の既知の差

命令の数の比は、`find-byte` が 1.25 (予算 1.5)、`find-sub` が 1.35 (予算 1.6) で、予算の中にある。超えた行は次の初めの2つである。

- `slice-short` は命令の数の比が 1.08 で (実時間は 0.01 秒きざみで粗く、この大きさの比には使わない)、予算の 1.0 を超える。1回の切り出しと手放しが `Box<[u8]>::from` と drop より約20命令多く (2,807,219,643 対 2,609,685,808 命令を 10,000,000 回で割った差)、`Ctx` の呼び出しとヒープの確保の道の費用である。行ごとの確保は `grep -n` で 3.0、`logstat` で 25.8、`csv2jsonl` で 35.8 なので (下の「VM と Rust の実装の比の主な原因」)、確保がすべてこの切り出しなら差は行あたり約60、約510、約700 命令になる。上限の見積もりで、約2倍の目標に足りるかは測っていない。`Box` の確保と同じ費用まで縮めるには、確保の道のインライン化 (「`eml_rt` のインライン化」) が要る。
- `read-all-bytes` は命令の数の比が 2.01 で (404,354,702 対 201,333,790 命令)、行の予算の 1.3 を超える。どちらの側も、入力の 64 MiB を組む費用を含むので、比は 1 の側に寄る。測っているのは `handle_read` の `Form::Bytes` で、UTF-8 の検査はない。差は、`Vec` に読んでからヒープへ写す2回目の写しと、`read_to_end` が空き領域を 0 で埋める費用である (「`FileIn.read_all` の読み方」)。参照の `read_to_end` は `&[u8]` から `Vec` へ1回写すだけである
  - 2倍の行への費用: 100 MB の入力で、eml の `read_all` (読み、UTF-8 の検査、写し) は 23 ms、Rust の `read_to_end` は 9〜10 ms で、差は約 13 ms である (設計のときの計測)。`wc` の Rust の実装は 0.07 s なので、2倍の目標 ([ネイティブ化の設計](../future/native.md) の「目的と範囲」の、スクリプト全体の目標で、この行の予算とは別) の余裕約 70 ms のうち、約 13 ms を読みが使う
  - `read_into` (Host の読みを一意な `Bytes` の空きに直接書く) は入れていない。入れる引き金は、ネイティブのコードで、この行が `read_to_end` の 1.3 倍を超えることで、VM の測定では判定しない ([ロードマップ](../future/roadmap.md) の「処理系」の `read_into`)。`read_into` は `Vec` からの写しを省くが、stable の Rust は `read_buf` を持たず、読む前に空きを 0 で埋めるので、ヒープへ直接読んでも得は 6〜10% に留まる。1.3 の予算に入れるには、写しを省くことに加えて、0 で埋めずに読む手段が要る
- `read-line-bytes-64mib` (56 バイトの行の 64 MiB を `Form::Bytes` の行の読みで読む) は記録だけの行で、予算を持たない。64 KiB の `BufReader` の `read_until` で行ごとに `Vec` に読む参照に対し、命令の数の比は 0.84 である (657,603,650 対 780,467,419 命令。どちらも入力の 64 MiB を組む費用を含む)。行の読みは読み手の buffer から行を1回だけヒープへ写し、行の終わりを T13 の `find_byte` で 8 バイトずつ探す。行の終わりを1バイトずつ探すループで測ったときの比は 1.28 で、差の主な原因はそのループだった。
- `stdout-write-line-10m` (23 バイトの行を `Stdout` のハンドルで 10,000,000 回書く) は記録だけの行で、1回の書き込みは約303命令、約 10 ns である (3,027,650,123 命令と 0.10 秒)。`println-block` の1行あたり約254命令に比べ、ハンドルの型の確かめと、ハンドルを返す分が多い。

### `bench/scripts.sh`

- 機種: Mac17,2 (Apple M5)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.99.1 (58cf8f9c3 2026-10-13)
- Python: Python 3.14.8
- コミット: d4a6e71
- 入力: スクリプトの入力は bench/scripts/gen.py が作る。大きさは表の「大きさ」の欄にある

スクリプト

| 計測 | 大きさ | eml の実時間 (s) | Python の実時間 (s) | 比 (eml / Python) | eml の最大 RSS (MB) | Python の最大 RSS (MB) |
|---|---:|---:|---:|---:|---:|---:|
| `wc` | 9000 KiB | 0.28 | 0.03 | 9.3 | 31 | 78 |
| `grep -n` | 70000 KiB | 0.24 | 0.26 | 0.9 | 11 | 16 |
| `logstat` | 6700 KiB | 0.26 | 0.11 | 2.4 | 13 | 19 |
| `logstat --whole` | 6700 KiB | 0.24 | 0.10 | 2.4 | 38 | 53 |
| `csv2jsonl` | 2900 KiB | 0.26 | 0.18 | 1.4 | 23 | 51 |

数値計算

| 計測 | 大きさ | eml の実時間 (s) | Python の実時間 (s) | 比 (eml / Python) | eml の最大 RSS (MB) | Python の最大 RSS (MB) |
|---|---:|---:|---:|---:|---:|---:|
| nbody | 140000 | 0.29 | 0.64 | 0.5 | 12 | 14 |
| spectral-norm | 510 | 0.34 | 0.86 | 0.4 | 11 | 14 |
| mandelbrot | 480 | 0.21 | 0.30 | 0.7 | 10 | 14 |
| fannkuch-redux | 9 | 0.52 | 0.24 | 2.2 | 12 | 14 |
| sieve | 3500000 | 0.30 | 0.17 | 1.8 | 37 | 41 |

C/Rust との比

| 計測 | 大きさ | eml の実時間 (s) | Rust の実時間 (s) | eml の空 (s) | Rust の空 (s) | 比 (eml / Rust) |
|---|---:|---:|---:|---:|---:|---:|
| `wc` | 100000 KiB | 3.09 | 0.07 | 0.00 | 0.00 | 44.1 |
| `grep -n` | 100000 KiB | 0.34 | 0.08 | 0.00 | 0.00 | 4.2 |
| `logstat` | 100000 KiB | 3.66 | 0.24 | 0.00 | 0.00 | 15.3 |
| `logstat --whole` | 100000 KiB | 3.49 | 0.23 | 0.00 | 0.00 | 15.2 |
| `csv2jsonl` | 50000 KiB | 4.31 | 0.15 | 0.00 | 0.00 | 28.7 |

## ネットワークの計測

`bench/net.sh` は、`Std.Net` を本物の driver の上で動かす物差しである。release ビルドの eml で `bench/net/cases.tsv` の件を流し、1件1行の JSONL を標準出力に出す。`python3 -I bench/net/report.py` がそれを Markdown の表にし、strace の run があるラベルには syscall の目標の表も出す。テストではなく、CI では流さない。命令の数でなく CPU の時間と syscall の数を測るので、同じ機械で前と後を並べたときだけ意味がある。

`bench.py` は `LABEL=EML` を1つ以上受け取る。2つ以上渡すと、時間の run を件ごとに交互に流す。`LABEL=EML@DIR` と書いたラベルは、件のプログラムを `DIR` から読む。`std/` の型が変わり、前のコミットの eml が今のプログラムを型検査できないときに使う。`DIR` には、そのコミットの `bench/net` を `git archive <コミット> bench/net | tar -x -C <置き場所>` で取り出したものを渡す。件の行と引数と `peer.py` は今の `bench/net` のものを使い、`@DIR` のないラベルは今のプログラムを使う。`--only REGEX` で件を絞り、`--stats` で `eml run --stats` の `RunStats` を1回取る。`--stats` の run は検査モードなので、時間には使わない。

### 件

eml の側は `Std.Net` で書く。相手は同じ run の中の eml のタスク (`-self`) か、ブロックする socket の Python (`peer.py`、`-py`) である。`-py` の件では eml のプロセスだけを測る。名前が `py:` で始まる件は Python 同士の床で、時間だけを測る。

| 件 | 何をするか | 単位 |
|---|---|---|
| `base` | `Net` と `Async` を読み込み、listen して閉じるだけ。コンパイルと起動の床 | run |
| `compute-fib` | I/O のない単一タスクの run (`bench/fib.em`)。監視のスレッドを run の始めに起こす費用を見る | run |
| `echo-{self,py}-{fresh,reuse}` | 1本の接続で 16 バイトの往復を繰り返す。`fresh` は書くたびに `Bytes.make`、読むたびに `Bytes.empty ()`。`reuse` は1つの buffer を `Bytes.truncate 0` で空にして読みに渡し直す | 往復 |
| `bulk-{self,shared,reuse,1k}` | 1本の接続で大きく転送する。`shared` は同じ `Bytes` を毎回書き、`reuse` は読み手が buffer を使い回し、`1k` は 1 KiB ずつ書く | MiB |
| `bulk-{send,recv}-py` | eml が送るだけ、または読むだけ | MiB |
| `bulk-accum` | 1つの buffer に足し続け、写しの伸び方を見る | MiB |
| `many-self-{250,1000,2000}`、`many-py-1000` | C 本を1本ずつ順に張り、張り終えてから全部で 16 バイトを往復して閉じる。張る間は読みが張った本数だけ保留になる | 接続 |
| `conn-self-{1,32}`、`conn-py` | 接続を張って EOF を読んで閉じることを繰り返す。`32` は32個のタスクが同時に行う | 接続 |
| `busy-self` | 同じ run の中の echo を往復する2つのタスクと、待ちの操作をしない計算のタスク1つを動かす。集める地点が OS を見る頻度が往復の時間に出るかを見る | 往復 |
| `py:{echo,bulk,conn}` | Python 同士の床 | 往復、MiB、接続 |

各件の引数と単位の数は `cases.tsv` にある。件の足し引きは行の足し引きで済む。

### 主指標

主指標は、eml のプロセスの CPU (user+sys) から `base` の中央値を引き、単位で割った値である。時間の run は件ごとに 11 回流し、中央値を使う。実時間、sys、最大 RSS、`minflt`、自発と非自発の切り替えは副の指標として並べる。`-py` の件の実時間は Python の相手が律速するので、eml の違いは CPU にしか出ない。

syscall の数は `strace -f` で数える (Linux のみ)。main のスレッドとほかのスレッドに分け、名前ごと、失敗の errno ごとに数え、`base` を引いて単位で割る。3回流して範囲を出す。`MSG_PEEK` 付きの `recvfrom` は `recvfrom(peek)` として別に数え、失敗は `recvfrom(peek)!EAGAIN` に入る。データのある空きなしの読みが覗きと読みの2回で済むかは、この分けで見える。`RunStats` は `eml run --stats` の1回から取り、床の行にある欄だけを表に出す。

`report.py` の目標の表は、件ごとに床を引いた単位あたりの式を strace の3回それぞれで計算し、値の最小-最大と、全回が基準を満たすかの判定を出す。式は mem = `brk` + `mmap` + `munmap`、E = `recvfrom!EAGAIN` + `recvfrom(peek)!EAGAIN`、R = `recvfrom` − `recvfrom!EAGAIN`、P = `recvfrom(peek)` − `recvfrom(peek)!EAGAIN` で組む。本数によらない費用の行だけは、many-self-250 と many-self-2000 の1接続あたりの CPU (床を引いた中央値) の比を時間の run から出す。件が流れていない行は「件なし」と出る。

### 比べる規則

- 後 ÷ 前の比が 0.90 以下か 1.10 以上で、かつ生の CPU の四分位の範囲 (25% から 75%) が前と後で重ならないときだけ「差」とする。そうでなければ「差なし (この機械では見分けられない)」と書く
- 最小値は採らない。同じ実行の道の2つの eml を比べた A/A の run では、この規則で「差」になった件がなく、最小値の比は中央値の比より広く開いた
- CPU は固定しない。`taskset` で固定しても揺れは狭まらなかった
- `minflt` の単位あたりの値が前と後で 1 以上違う件は「確保の揺れ」と印を付け、時間の判定から外す。1往復ごとに確保して手放す buffer が glibc のヒープの頂きを縮めて伸ばすかはバイナリの配置で変わり、物差しでは決まらない。その件の確保の違いは `RunStats` の `allocations` と syscall の `brk` で見る
- syscall は main のスレッドで比べ、3回のどれもが目標以下であることを見る
- 前と後は同じ機械で、ほかに負荷のない状態で測る

### driver の syscall の目標

本物の driver の syscall と CPU の目標を、`report.py` が strace と時間の run から出す目標の表で確かめる。式の記号は次のとおりで、どれも main のスレッドの、床を引いた単位あたりの値である。

- main: 失敗の鍵 (`名前!ERRNO`) を除いた呼び出しの計。`run.py` は失敗した呼び出しを `名前` と `名前!ERRNO` の両方で数えるためである
- mem: `brk`、`mmap`、`munmap` の計
- E: EAGAIN で終わった読みと覗き (`recvfrom!EAGAIN` と `recvfrom(peek)!EAGAIN`) の計
- R: データを読んだ `recvfrom`。P: データのあった覗き (`recvfrom(peek)`)

値は Intel Xeon Processor @ 2.10GHz、4 vCPU、メモリ 16477352 kB、kernel 6.18.44-fc-v185 の Linux の container で、`python3 -I bench/net/bench.py --stats` の全件の run から出した。strace の3回の範囲で、合格は3回すべてで満たすことである。

| 項目 | 件 | 式 | 目標 | 今 |
|---|---|---|---|---|
| 書き 1 | bulk-send-py | (main − mem) ÷ 書き | ≤ 1.05 | 1.000 |
| データのある読み (空きあり) 1 | echo-py-reuse | (main − `sendto` − mem − 2E) ÷ 往復 | ≤ 1.05 | 1.001〜1.002 |
| 1往復 (eml が片側) | echo-py-reuse | main ÷ 往復 | ≤ 4 | 2.010〜2.036 |
| 空きなしの読みは覗きと読みの 2 まで | bulk-recv-py | (R + P) ÷ R | ≤ 2.02 | 1.966〜1.968 |
| 空きなしの読みは覗きと読みの 2 まで | echo-py-fresh | (R + P) ÷ R | ≤ 2.02 | 1.794〜1.952 |
| 待つ読み 3 | echo-self-fresh | (main − `sendto` − mem) ÷ E | ≤ 3.05 (E ≥ 1.5 /往復) | 3.004 (E 2.000) |
| 操作ごとの `epoll_ctl` 0 | echo-self-fresh、echo-py-reuse | `epoll_ctl` ÷ 往復 | ≤ 0.005 | 0.001、0.000 |
| `epoll_ctl` の総数 | echo-py-reuse | main の `epoll_ctl` (床を引かない) | ≤ 2 (eventfd 1 と接続のソケット 1) | 2 |
| 受けと登録 | conn-self-32 | `accept4` ÷ 接続、`epoll_ctl` ÷ 接続 | ≤ 1.05、≤ 2.005 | 1.031、2.000 |
| 待ち1回の費用 | echo-self-fresh | (`timerfd_settime` + `read(eventfd)` + `epoll_ctl`) ÷ 待ち | ≤ 0.05 | 0.000 |
| 本数によらない費用 | many-self-250、-2000 | 1接続あたり CPU の 2000 本 ÷ 250 本 | ≤ 1.5 | 0.886 |

- 待つ読みの3回は、EAGAIN の覗き、`epoll_pwait`、読みである。辺の直後の `Readable` は、記憶が新しければ覗かずに答える ([VM](../spec/vm.md) の「時計と driver」)
- 空きなしの読みは、短い読みの後に覗き1回と読み1回になる。P ÷ R は bulk-recv-py で 0.97、echo-py-fresh で 0.79〜0.95 である
- 1接続あたりの CPU は many-self-250、-1000、-2000 で 96.80、84.08、85.79 µs で、本数とともに伸びない
- 目標を外したときは不具合として扱い、`strace -f -o` で余分な呼び出しの出どころを特定する

### 34cad91 と e942eeb

driver の約束を変える前 (34cad91) と後 (e942eeb) を、同じ機械で比べた。機械は Intel Xeon Processor @ 2.10GHz、4 vCPU、メモリ 16477352 kB (`/proc/meminfo` の MemTotal) の Linux の container で、kernel は 6.18.44-fc-v185 である。どちらも release の eml で、34cad91 の eml は今のプログラムを型検査できないので (`write_bytes` の型が違う)、34cad91 の `bench/net` と `bench/fib.em` を `git archive` で取り出して `before=<34cad91 の eml>@<取り出した bench/net>` で流した。`busy-self` は 34cad91 にないので `--only '^(?!busy-self$)'` で外した。

CPU と実時間は床 (`base`) を引いた単位あたりの µs で、11 回の中央値である。syscall は strace の小さい run の main のスレッドの計を床を引いて単位で割った値で、3回の範囲である。`allocations` は `--stats` の1回から床を引いて単位で割った値である。判定は上の「比べる規則」による。

| 件 | 単位 | CPU µs 34cad91 | CPU µs e942eeb | 比 | 判定 | 実時間 µs 34cad91 | 実時間 µs e942eeb | syscall 34cad91 | syscall e942eeb | 最大 RSS MiB 34cad91 | 最大 RSS MiB e942eeb | allocations 34cad91 | allocations e942eeb |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| echo-self-fresh | 往復 | 27.34 | 21.15 | 0.77 | 差 | 27.65 | 21.67 | 32.02 | 24.02 | 12.8 | 13.1 | 76.06 | 43.05 |
| echo-self-reuse | 往復 | 27.90 | 19.93 | 0.71 | 差 | 28.60 | 20.09 | 32.02 | 24.02 | 12.8 | 13.0 | 74.06 | 38.05 |
| echo-py-fresh | 往復 | 33.83 | 29.71 | 0.88 | 差 | 35.12 | 34.36 | 10.02-10.06 | 3.08-3.50 | 12.8 | 13.1 | 42.02 | 23.92 |
| echo-py-reuse | 往復 | 32.68 | 28.99 | 0.89 | 差 | 33.99 | 35.21 | 10.02-10.03 | 2.01-2.19 | 12.8 | 13.1 | 40.02 | 20.96 |
| bulk-self | MiB | 428.25 | 416.24 | 0.97 | 差なし | 428.40 | 420.87 | 160.44 | 192.45 | 12.9 | 13.2 | 609.34 | 609.15 |
| bulk-shared | MiB | 387.37 | 330.05 | 0.85 | 差なし | 386.79 | 333.88 | 160.44 | 192.45 | 13.0 | 13.1 | 609.35 | 593.16 |
| bulk-reuse | MiB | 413.46 | 402.11 | 0.97 | 差なし | 413.78 | 407.58 | 160.44 | 192.45 | 13.0 | 13.2 | 577.34 | 577.15 |
| bulk-1k | MiB | 13721.39 | 1765.23 | 0.13 | 差 | 13702.75 | 1765.57 | 10242.62 | 1336.25-1337.06 | 12.9 | 13.2 | 38920.38 | 1959.62 |
| bulk-send-py | MiB | 521.72 | 465.05 | 0.89 | 差 | 524.00 | 484.79 | 16.25 | 16.26 | 12.9 | 13.1 | 304.60 | 16.75 |
| bulk-recv-py | MiB | 488.27 | 387.86 | 0.79 | 差 | 686.38 | 708.08 | 166.48-196.63 | 44.32-44.77 | 12.9 | 13.0 | 441.19 | 34.15 |
| bulk-accum | MiB | 63879.75 | 2528.37 | 0.04 | 確保の揺れ | 63978.18 | 2568.94 | 194.62 | 195.81 | 44.8 | 45.1 | 600.38 | 583.75 |
| many-self-250 | 接続 | 226.28 | 158.72 | 0.70 | 確保の揺れ | 388.79 | 348.98 | 71.07-71.12 | 38.38-38.41 | 19.6 | 16.4 | 165.36 | 125.58 |
| many-self-1000 | 接続 | 271.33 | 191.24 | 0.70 | 差 | 331.03 | 261.56 | 71.05-71.05 | 38.30-38.35 | 53.7 | 50.2 | 165.09 | 124.74 |
| many-self-2000 | 接続 | 449.36 | 242.67 | 0.54 | 確保の揺れ | 479.74 | 293.07 | 71.05-74.90 | 38.31-38.34 | 221.7 | 155.3 | 165.04 | 124.94 |
| many-py-1000 | 接続 | 71.73 | 82.27 | 1.15 | 差なし | 130.37 | 128.12 | 25.51 | 21.90 | 25.7 | 25.6 | 72.02 | 55.04 |
| conn-self-1 | 接続 | 44.86 | 41.61 | 0.93 | 差なし | 46.36 | 42.82 | 41.99 | 26.77-26.98 | 12.9 | 13.2 | 58.05 | 47.48 |
| conn-self-32 | 接続 | 47.55 | 31.24 | 0.66 | 差 | 48.21 | 32.05 | 32.18 | 14.51-14.62 | 12.9 | 13.4 | 56.57 | 26.74 |
| conn-py | 接続 | 52.77 | 51.49 | 0.98 | 差なし | 53.63 | 57.76 | 24.00-24.04 | 17.00-17.02 | 12.9 | 13.2 | 40.01 | 27.91 |

- 終わった操作を待たない道で、両端が同じ run にある件の CPU が縮んだ。echo-self は1往復 27〜28 µs から 20〜21 µs に、conn-self-32 は1接続 47.6 µs から 31.2 µs になった。echo-py-reuse の1往復の main の syscall は 10 回から 2 回 (`recvfrom` と `sendto`) になった。相手の Python の返事がもう届いているので、空きのある buffer への読みは受け付けで終わる。echo-py-fresh も 10 回から 3.1〜3.5 回になった。`Readable` が受け付けで1バイトを覗き、データがあればその場で終わるので、poll の登録と待ちがなくなる
- 足し続ける読みの写しが線形になった。bulk-accum の 16 MiB は実時間 1.052 s から 0.068 s に、bulk-1k は1 MiB あたりの CPU が 0.13 倍になった。bulk-1k の syscall は 1 MiB あたり 10 243 回から 1 336 回になった。書きの多くが受け付けで終わり、待ちの poll がなくなったためである
- 同じ run の bulk (bulk-self、bulk-shared、bulk-reuse) は、1 MiB あたりの CPU が 34cad91 と同じかそれより少ない (比 0.97、0.85、0.97、判定は差なし)。操作の予算に書きの重みを入れる前のコード (d96346f) では、同じ機械の別の run で 669.30、582.08、545.34 µs と、34cad91 の 1.29〜1.63 倍だった。原因は、書き手が読み手より先に走り続けることだった。書きはどれも受け付けで終わるので、書き手は `sendto` が EAGAIN を返すまで平均で約 27 回 (約 1.7 MiB) 書き続け、読み手は OS を見るまで起きなかった。送りの列にたまったバイトを読むときの写しが重くなり、送りの処理が softirq に回って、1 MiB あたりの sys が倍になった。今は書きの待ちが長さに応じて予算を使い (64 KiB の書きで予算を使い切る)、予算を使い切った終わった操作の待ちが OS を1回見てから譲る ([標準ライブラリ](../spec/stdlib.md) の「操作の予算」)。書き手は 64 KiB ごとに読み手を起こし、1回の書きと1回の読みが交互に進む。1 MiB あたりの main の syscall が 160 回から 192 回なのは、読み手が書きの届く前に読みを試すので、`recvfrom` の半分が EAGAIN になるためである。`allocations` が bulk-reuse で d96346f の 29.03 から 577.15 に戻ったのは、読みが受け付けで終わらずに待つ回数が増えたためと推定する
- 同じ run の bulk の後退を調べた数字と、試して採らなかった形は次のとおりである。どれも同じ機械の試しのビルドで、bulk の数は 1 GiB あたりの CPU の秒、ほかの件は単位 (1往復、1接続) あたりの CPU の µs である
  - perf の cpu-clock の標本数 (`bulk.em 1024 64 shared fresh`) では、後退したビルドで `_copy_to_iter` が 148 から 270 に、`handle_softirqs` がほぼ 0 から 173 に増えた
  - 予算を 1 に、`POLL_EVERY` を 1 にする (1回の書きごとに OS を見て譲る): bulk-self、bulk-shared、bulk-reuse は 0.444、0.349、0.386 s で、34cad91 (0.441、0.444、0.446 s) 以下に戻る。しかし echo-self-reuse が 18.5 から 27.2、many-self-1000 が 156 から 259、conn-self-32 が 39 から 49 になり、終わった操作を待たない道で縮んだ分がなくなる
  - 予算を 1 か 4 にするだけ: bulk-shared が 0.735 s と 0.623 s で、直らない。譲っても OS を見なければ、読み手は切り替えが `POLL_EVERY` 回続くまで起きない
  - 書きの重みの単位: OS を見る間に 128 KiB 書くと、bulk-self は 0.53 s だった。1回の書きごとに OS を見ると 0.38〜0.45 s である
  - 受け付けで終わった書きの待ちで譲る: bulk-reuse は 0.420 s になったが、bulk-self は 1.078 s に悪くなり、echo-self-reuse が 23% 遅くなった。受け付けで終わったかはスケジュールで変わるので、譲る位置と仕事の回数もスケジュールに依る
  - 予算を使い切った終わった操作の待ちが、走れるタスクがなくても譲る: bulk-send-py の CPU が 1.16 倍 (9 回で流し直して 1.10 倍) になった。走れるタスクがなければ続ける今の形では 0.99 倍である。syscall の数は同じで、差は `reschedule` の費用である
  - 送りの列を `TCP_NOTSENT_LOWAT`、`SO_SNDBUF`、`SO_RCVBUF` で抑える: どの値でも run が良い状態と悪い状態に分かれた。`TCP_NOTSENT_LOWAT` を 64 KiB にした bulk-self は、9 回のうち4回が 0.36〜0.39 s、5回が 0.72〜0.78 s だった。OS ごとの設定で、Windows には `TCP_NOTSENT_LOWAT` に当たるものがない
  - `Readable` を受け付けで終えるだけの変更では、bulk-self が 0.73 から 0.62〜0.63 s、bulk-shared が 0.62 から 0.51〜0.53 s になったが、読みがもう受け付けで終わる bulk-reuse は変わらなかった (0.59〜0.61 s)
- bulk-self は buffer の確保の揺れが出やすい。チャンクごとに新しい 64 KiB の buffer を2つ作って手放すので、glibc がヒープの頂きを縮めるかで run が2つに分かれることがある。原因を調べた run では、同じ eml の bulk-self が、`minflt` 約 1 600 で 1 GiB あたり 0.40 s の run と、約 149 000 で 0.73 s の run に分かれた。この表の 11 回では分かれず、`minflt` は 1 354〜1 362、CPU は 0.441〜0.544 s だった。d96346f の列では `minflt` が1 MiB あたり 6.43 で、確保の揺れとして判定から外していた。bulk-shared の 11 回の CPU は 0.330〜0.468 s と、ほかの bulk より幅が広かった
- 相手が Python の bulk-send-py は 0.89 倍、bulk-recv-py は 0.79 倍である。bulk-recv-py の1 MiB あたりの main の syscall は 166〜197 回から 44 回になった。相手の書いたバイトがもう届いているので、覗きで `Readable` が受け付けで終わり、poll の登録と待ちがない
- bulk-recv-py の `recvfrom` は 1 MiB あたり 44.06 回である。読み1回に覗きと読みが1回ずつなら、読みは 22 回ほどで、1回の平均は約 46 KiB になる。1回の読みに貸す 65 536 バイトを使い切らない
- many-self の1接続あたりの CPU の伸び (2000 本 ÷ 250 本) は 1.99 から 1.53 になった。残りは本物の driver が1回の待ちで保留の表を歩く分である。2000 本の最大 RSS は 221.7 MiB から 155.3 MiB に減ったが、本数とともに段になることは変わらない。段になる理由は確かめていない
- `completions` は、buffer を使い回す件で往復あたり減った。echo-self-reuse は 6.00 から 4.00 に、echo-py-reuse は 3.00 から 2.00 になった。空きのある buffer への読みは `Readable` を出さずに `Read` 1つで済むので、読み1回の完了が2つから1つになる。`parks` は2列で同じ (4.00 と 2.00) で、待ちの数はプログラムで決まる。`allocations` は、使い回す件で往復あたり 2 以上減った (echo-py-reuse は 40.02 から 20.96、echo-self-reuse は 74.06 から 38.05)。床の `allocations` も 34 から 17 になった
- Rust の側の確保は、d96346f と同じコードの速いモードの release の eml で、echo.em の reuse を 200 往復と 2200 往復で callgrind に通し、差を 2000 で割って数えた。1往復の `malloc` は 10 回、`realloc` は 0 回である。`collect` の列とエントリの `Task` の scratch を使い回す前は、`malloc` が 12 回、`realloc` が 2 回だった。残る 10 回は、compio の `Proactor::push` の鍵の箱 (送りと受けで 2 回ずつ)、poll ごとの `polling::epoll::Events::with_capacity` (2 回)、`eml_io` の `real.rs` の操作の表 `HashMap<OpKey, Entry>` の伸び直し (2 回)、compio の `VecDeque<ErasedKey>` の伸び (2 回) である。表が伸び直すのは、`real.rs` の `reap` が `wait` のたびに `std::mem::take` で表を空の `HashMap` に替えるためである
- 書きの重みと OS の確かめは、bulk 以外では見分けられない。d96346f と同じコードの eml と e942eeb の eml を、`--only '^(base|many-.*|echo-.*|conn-self-1)$' --runs 11 --no-strace` で同じ run に交互に流すと、9件とも差なしで、比は 0.96〜1.08 だった。上の表と d96346f の別の run の数を比べると、many-self-250、many-self-1000、many-py-1000 の中央値が 1.14〜1.25 倍に見えるが、四分位は重なり、同じ run の比べでは 1.08、1.04、0.98 である
- 監視のスレッドを run の始めに起こす費用は、この機械では見分けられない。`base` の CPU の中央値は 27.7 ms (四分位 27〜34) から 26.0 ms (26〜29)、`compute-fib` は 15 ms (14〜16) から 14 ms (14〜15) で、四分位は重なる

### `POLL_EVERY` の値

集める地点が OS を見る頻度 `POLL_EVERY` ([VM](../spec/vm.md) の「時計と driver」) は 16 である。d96346f と同じコードの release の eml で、`POLL_EVERY` を 1 にしたもの (`one`) と 16 のもの (`sixteen`) を、上と同じ機械で比べた。

- `busy-self` (`python3 -I bench/net/bench.py --only busy-self --runs 11 --strace-runs 3`) は `POLL_EVERY` を見分けない。この run は `base` を流しておらず、時間の run は 2000 往復の run だけなので、1往復の値は中央値を 2000 で割っただけで、起動の床 (約 27 ms、1往復あたり約 13 µs) を含む

| ラベル | 実時間 s | CPU s (四分位) | 1往復の実時間 µs | 1往復の CPU µs | 1往復の sys µs | 最大 RSS MiB |
|---|---|---|---|---|---|---|
| one | 80.165 (80.095-80.267) | 79.570 (79.535-79.655) | 40082.26 | 39784.87 | 29.17 | 13.0 |
| sixteen | 80.205 (80.094-80.519) | 79.623 (79.532-79.685) | 40102.63 | 39811.65 | 15.52 | 13.0 |

- CPU の比は 1.00 で、四分位は重なる。1往復の実時間の四分位の幅は `one` で 56.4 µs、`sixteen` で 84.1 µs で、16 のほうが 20.37 µs 大きいという差はこの揺れの中にある。200 往復の strace の run では、1往復の `epoll_pwait` が `one` で 4.02、`sixteen` で 2.01、main の計が 37.49〜37.58 と 25.42〜25.43 だった。`one` は OS を倍の回数見ても、1往復の時間が変わらない
- 1往復の約 40 ms は横取り2回分である。`--stats` では 20 往復で `preemptions` が 43、40 往復で 83 だった。横取りは1つのタスクが刻みの全体を使ったときだけ起こるので ([VM](../spec/vm.md) の「タスクとスケジュール」)、刻みの直後に切り替わった計算のタスクは2つ目の刻みまで走り、横取り1回は約 20 ms になる。往復の時間は、計算のタスクが刻みを使い切る間に、起こされた I/O のタスクが待つことで決まる
- 走れる I/O のタスクが多い many-self でも比べた (`--only '^(base|many-self-.*)$'`)。`one` から `sixteen` の順に、1接続あたりの CPU は 250 本で 222.54 から 138.55 µs、1000 本で 454.96 から 155.61 µs、2000 本で 847.05 から 237.19 µs だったが、`minflt` の1接続あたりの値が 1 以上違うので、3件とも確保の揺れとして時間の判定から外す。syscall は、1接続あたりの `epoll_pwait` が 5.66〜6.39 から 1.19 に、main の計が 59.28〜63.67 から 32.29〜32.53 になり、16 のほうが OS を見る回数が少ない
- 16 を保った根拠は、どちらの件の時間でもなく、many-self の syscall の数である

### 書きの重みの単位

操作の予算で書きが長さに応じて使う重みの単位 `WRITE_UNIT` ([標準ライブラリ](../spec/stdlib.md) の「操作の予算」) は 512 バイトである。自前の driver の上で、単位だけを 256 と 1024 に替えた release の eml を 512 と比べた。機械は Intel Xeon Processor @ 2.10GHz、4 vCPU、メモリ 16477352 kB の Linux の container で、kernel は 6.18.44-fc-v185 である。

```sh
W='^(base|bulk-(self|shared|reuse|1k|send-py|recv-py)|echo-self-(fresh|reuse)|many-self-1000)$'
python3 -I bench/net/bench.py --only "$W" --no-strace --stats u512=<512 の eml> u256=<256 の eml>
python3 -I bench/net/bench.py --only "$W" --no-strace --stats u512=<512 の eml> u1024=<1024 の eml>
```

候補が 512 を置き換えるのは、次の3つが成り立つときだけとした。

- bulk-self、bulk-shared、bulk-reuse、bulk-1k のどれかで、候補の側に「差」がある
- 測ったどの件にも、CPU で候補に不利な「差」がない
- bulk-self、bulk-shared、bulk-reuse の単位あたりの sys が 1.10 倍を越えず、echo-self-fresh と echo-self-reuse の実時間の比が 1.10 以下である。単位の役目は同じ worker の読み手を起こすことで、読み手が遅れると送りの列がたまって sys が増えるためである (上の「34cad91 と e942eeb」)

CPU は床を引いた単位あたりの中央値 (µs)、比は候補 ÷ 512 である。

| 件 | 512 | 256 | 比 | 判定 | 512 | 1024 | 比 | 判定 |
|---|---|---|---|---|---|---|---|---|
| echo-self-fresh | 10.71 | 11.41 | 1.07 | 差なし | 11.51 | 11.77 | 1.02 | 差なし |
| echo-self-reuse | 12.74 | 10.43 | 0.82 | 差なし | 11.19 | 11.14 | 1.00 | 差なし |
| bulk-self | 276.45 | 266.77 | 0.96 | 差なし | 277.79 | 812.95 | 2.93 | 差 |
| bulk-shared | 218.87 | 220.87 | 1.01 | 差なし | 212.85 | 649.47 | 3.05 | 差 |
| bulk-reuse | 279.55 | 263.41 | 0.94 | 差なし | 266.48 | 698.35 | 2.62 | 差 |
| bulk-1k | 1450.50 | 1629.17 | 1.12 | 差なし | 1310.17 | 1228.83 | 0.94 | 差なし |
| bulk-send-py | 509.23 | 505.05 | 0.99 | 差なし | 498.43 | 508.30 | 1.02 | 差なし |
| bulk-recv-py | 372.12 | 373.82 | 1.00 | 差なし | 360.78 | 376.54 | 1.04 | 差なし |
| many-self-1000 | 97.25 | 89.46 | 0.92 | 差なし | 92.70 | 86.96 | 0.94 | 差なし |

- 256 は、どの件にも「差」がなく、1つ目の条件を満たさない
- 1024 は、同じ run の bulk で CPU が約3倍になり、2つ目の条件を破る。64 KiB の書きが予算を使い切らなくなり、書き手が読み手を起こさずに走り続ける
- どちらの候補も Linux で外れるので、512 を保つ

macOS でも同じ件を比べた。機械は Apple M5、10 コア、メモリ 32 GiB の macOS 27.0.1 である。macOS は loopback の接続で TIME_WAIT の port を使い直さないので、many-self-1000 を2つのラベルで11回ずつ続けて流すと、ephemeral port が尽きて `connect` が `EADDRNOTAVAIL` で落ちる。この件だけは時間の run を 4 回、4 回、3 回に分け、間で TIME_WAIT が減るのを待って流し、結果をつないだ。

| 件 | 512 | 256 | 比 | 判定 | 512 | 1024 | 比 | 判定 |
|---|---|---|---|---|---|---|---|---|
| echo-self-fresh | 13.91 | 14.04 | 1.01 | 差なし | 13.85 | 13.90 | 1.00 | 差なし |
| echo-self-reuse | 15.02 | 14.98 | 1.00 | 差なし | 14.97 | 14.94 | 1.00 | 差なし |
| bulk-self | 203.06 | 203.34 | 1.00 | 差なし | 200.56 | 204.66 | 1.02 | 差なし |
| bulk-shared | 197.52 | 195.79 | 0.99 | 差なし | 196.03 | 174.99 | 0.89 | 差 |
| bulk-reuse | 202.56 | 201.47 | 0.99 | 差なし | 201.91 | 202.56 | 1.00 | 差なし |
| bulk-1k | 1214.89 | 1357.45 | 1.12 | 差 | 1226.45 | 1220.84 | 1.00 | 差なし |
| bulk-send-py | 129.54 | 125.53 | 0.97 | 差なし | 127.39 | 126.83 | 1.00 | 差なし |
| bulk-recv-py | 98.23 | 98.62 | 1.00 | 差なし | 97.45 | 97.76 | 1.00 | 差なし |
| many-self-1000 | 39.06 | 39.06 | 1.00 | 差なし | 38.27 | 37.57 | 0.98 | 差なし |

- 256 は、bulk-1k で候補に不利な「差」があり、1つ目と2つ目の条件を満たさない
- 1024 は、bulk-shared で候補の側に「差」があり、不利な「差」はない。単位あたりの sys の比は bulk-self 1.06、bulk-shared 0.91、bulk-reuse 1.04 で、echo-self-fresh と echo-self-reuse の実時間の比はどちらも 1.00 なので、3つの条件を満たす。それでも 512 を保つのは、Linux で2つ目の条件を破るためである
- macOS の bulk-shared は、非自発の切り替えの中央値が 512 で 6330 回 (256 との run では 5849 回)、1024 で 114 回だった。原因は調べていない。kqueue の `EVFILT_WRITE` が ACK ごとに待ちを起こすことと関係するかも確かめていない

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
- 数値: 23 バイトの行を100万行書くと、行ごとの flush はファイルへ 1.30 µs/行、パイプへ 324 ns/行、`BufWriter` は 8 ns/行と 5 ns/行 (M4。設計のときの使い捨ての計測で、コミットの記録はない)。`rt.sh` の `println-block` (`dyn Host` の呼び出しを含む) は1行あたり約254命令、`println-line` は約11,144命令で、`Stdout` のハンドルの1回の書き込み (`stdout-write-line-10m`) は約304命令である (「今の数値」の表)
- 根拠にしている所: [オブジェクトモデル](../spec/object-model.md) の「extern」の「Host」と「出力のバッファ」

### `FileIn.read_all` の読み方

- 主張: `FileIn.read_all` と `FileIn.read_all_bytes` は、読み手の buffer に残ったバイトを `Vec` に移し、残りを `Vec` に読んでからヒープへ1回写す。ヒープに直接読む形は、空き領域を0で埋める費用が省いた写しとほぼ相殺するので 6〜10% しか速くならず、信頼する関数が4つ要る。mmap はファイルが縮むと SIGBUS で落ち、`Vec` をそのまま文字列にすると文字列の全操作が2つの形に分かれる。読み手を `StopOnInterrupt` で包むので、`read_to_end` は空き領域を0で埋めてから読む
- 数値: 採った形は 1 MiB で 60.2 µs、64 MiB で 7.0 ms (M4、ページキャッシュにあるファイル。設計のときの計測で、コミットの記録はない)。0で埋めるようにすると `read-all` +12.1%、`read-all-unsized` +10.0% (M4、1f36698 と 223d52f)。`read-all-bytes` は `read_to_end` の参照の 2.01 倍の命令である (「今の数値」の「予算を超えた行の既知の差」)
- 根拠にしている所: [オブジェクトモデル](../spec/object-model.md) の「読み手」
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

### モードと方針を束ねた型

- 主張: VM の繰り返しを方針ごとに写すとき、`step` から呼ぶ補助の関数をモードだけで総称にすると、補助の関数の呼び手が写しの数だけでき、`vm::run` に展開されなくなる。モードと方針を束ねた1つの型で総称にすると、写しごとに呼び手が1つに戻り、逐次の命令の数は写しのない繰り返しと同じになる
- 数値: 同じ機種で測った c8ec310f の `bench/run.sh` を基準にした、逐次の実行の分の増減。モードだけで総称にした形は `records_wide` +9.81%、`std_list` +6.84%、`tree` +5.84%、`list` +5.55%、`fib` と `loop` は ±0.6% 以内で、`-C remark=inline` では `con`、`release`、`unpack`、`con_reuse`、`release_reuse`、`switch_int`、`switch_string`、`apply_rest`、`make_closure`、`extern_call`、`finish` が `vm::run` に展開されなくなった。seed の方針の写しを逐次の写しに置き換えると、基準と同じ数に戻った。束ねた型の形では、増えたプログラムはどれも 0.43% 以内である (最大は `state`)。M5、c8ec310f の上の未コミットの木。束ねた型の形は、この所見を入れたコミットのコードである
- 根拠にしている所: [VM](../spec/vm.md) の「置き場所」の VM の繰り返しの写しの項

### `INLINE_LIMIT` と `GROWTH_FACTOR`

- 主張: `INLINE_LIMIT` の 40 は、測った値 (10、20、40、80) のうち、どのプログラムも下げ止まる最小の値である。書き換えで生まれた呼び出しの展開の予算 `GROWTH_FACTOR` は8倍で、`bench/` と `tests/ui/run`、`tests/ui/run-fail` のどの関数も予算に届かない。予算がないと、本体が自分を k 回呼ぶ `Fix` のラムダで関数が回ごとに k 倍に育つ
- 数値: 実行の分で、`iter_pipeline` は 20 で 295,711,502、40 で 197,919,428 (20 では49% 遅い)。`upsert` は 10 で 535,233,564、20 以上で約490M (10 では9% 遅い)。80 は 40 と ±0.2% 以内。予算なしの `Fix` のラムダは、本体の呼び出し3で Core IR 157,491 行・0.31 秒・206 MB、4で 1,747,659 行・4.2 秒・2.5 GB。8倍の予算では約400行。育ち方の最大は `tests/ui/run/std/std_iter_lin.em` の `main` の 48 から 179 (3.7倍) で、4倍では余裕が13文しかない。M4、b9b2ff0 の前の未コミットの木 (定数を変えて測った)。コミットからは再現できない
- 根拠にしている所: `crates/eml_core_ir/src/optimize/fuse/mod.rs` の `GROWTH_FACTOR` と `fuse/inline.rs` の `INLINE_LIMIT`、[Core IR](../spec/core-ir.md) の「インライン化」と「融合が止まること」

### インライン化で増えるスタック

- 主張: 末尾でない再帰の中に小さな関数を展開すると、1つのフレームのスロットが増え、深い再帰でスタックの上限に早く当たる
- 数値: `upsert` の `peak_stack` は 352 語で、展開しなければ 206 語 (6f77feb)。`RunStats` の値なので機種によらない。352 は `crates/eml_vm/tests/bench.rs` のスナップショットと同じ
- 根拠にしている所: [実装の現在地](status.md) の「最適化が扱わない形」
- 受け入れた理由: 展開は、自動に任せたプログラムと手書きのプログラムの組の RC の回数をそろえるために要る

### reuse の token で増えるスタック

- 主張: 末尾でない再帰の呼び出しをまたいで reuse の token を持つと、token のスロットでフレームごとに1語増え、深い再帰でスタックの上限に早く当たる
- 数値: `list` の `peak_stack` は 700,015 語で、token を持たなければ 600,014 語 (9ed9a349)。`tree_map` は 126 語と 112 語。`RunStats` の値なので機種によらず、`crates/eml_vm/tests/bench.rs` のスナップショットと同じ
- 根拠にしている所: [実装の現在地](status.md) の「実行の費用」
- 受け入れた理由: token をフレームの中に限る形 (frame-limited reuse) は、token を呼び出しの引数と戻り値で渡さない簡単な形で、その費用として受け入れた

### 1バイトずつ回すループ

- 主張: eml で1バイトずつ回すループは、`bench/loop.em` の1反復に約336命令かかる。入力を1バイトずつ数える `wc` は Python の実装の約13倍の実時間で、Python の実装は C で書かれた `bytes.count` と `bytes.split` に任せている。1行の読みと検索を extern に任せる `grep -n` は Python より少し速い。素のループでは VM は CPython より速い
- 数値: `loop` の実行の分 167,771,674 命令 / 500,000 反復 (「今の数値」の表)。`gen.py log 30000` の 30 MB の入力で、`wc` は eml 0.88 秒、Python 0.07 秒 (12.6倍)、`grep -n` は eml 0.10 秒、Python 0.12 秒 (0.8倍)。3回の中央値 (M5、6120a11、Python 3.14.8)。3,000万回のループは VM 0.26 秒、Python 0.66 秒で、`wc` を Python でも1バイトずつのループで書くと VM 1.56 秒、Python 1.45 秒 (M4、Python 3.14.7。リポジトリにない使い捨ての試作で、コミットの記録はない)。「今の数値」の `wc` の比は大きさが 9000 KiB で Python の実時間が小さく、この30 MB の値とは違う数になる
- 根拠にしている所: [実装の現在地](status.md) の「実行の費用」、[ネイティブ化の設計](../future/native.md) の「目的と範囲」の完了の条件

### 文字を読むループと `char_at` の命令

- 主張: `String.char_at` には専用の VM の命令を置かず、汎用の `Op::Extern` で動かす。専用の命令 `StrCharAt` を足しても、文字を読むループの命令の数は 10% も減らない。一方、区切りがすべて ASCII の走査を `char_at` で書くと、専用の命令を持つ `byte_at` で書くより遅いので、`wc` はバイトで走査する
- 数値: `bench/char_loop.em` は、汎用の `Op::Extern` で 207,838,220 命令、`StrCharAt` を足すと 200,857,118 命令で、実行の分で 4.4% の減りだった。文字列を伸ばすだけのプログラム (131,419,002 命令) を引いた読みのループの分では、約 9.1% の減りである (M5、4364489a の木と、それに `StrCharAt` を足した未コミットの木)。`bench/scripts.sh` の `wc` の行 (9000 KiB) は、`char_at` と `String.next` で読む形で eml 0.40 秒、`byte_at` で読む形で 0.28 秒だった (M5、64e6248 と be27bf7)
- 根拠にしている所: [VM](../spec/vm.md) の「基本操作と extern」の `byte_at` と `byte_length` の項、[標準ライブラリ](../spec/stdlib.md) の「`String`」の走査の方針、`tests/ui/run/scripts/wc/main.em` の `is_space`、[ロードマップ](../future/roadmap.md) の「処理系」の、`Char`、`Byte`、`Bits`、`String` の行に VM の命令を足す項目

### `String` を import するプログラムの前段の分

- 主張: `Std.String` を import するプログラムは、`String` が import する `List`、`Char`、`Iter`、`Yield` も読んでコンパイルする。この前段の命令は `empty` に入らないので、そのまま「実行の分」に加わる。大きさの引数を読む5つのプログラムも、`String.to_int` を通してこの分を持つ。std のコンパイルの結果を使い回せるようになったら (ロードマップの「処理系」のインクリメンタル化)、実行の分の引き方を見直す
- 数値: 34c7489 の表から be27bf7 の表で、`String` を import するプログラムの実行の分は約3500万〜3900万命令増えた。`byte_loop` +31%、`sieve_var` +39%、`nbody` +24%、`push_fold` +24%、`fold_state` +22%、`fannkuch_var` +15% である。`RunStats` は変わっていないので、増えたのは前段の分である。`List` だけを import するプログラムは、`List.em` が大きくなった分の約1100万命令増えた (M5)
- 根拠にしている所: 上の「基準のプログラム」の大きさの引数の段落、[ロードマップ](../future/roadmap.md) の「処理系」のインクリメンタル化の項目

### `Map` と `Set` の釣り合いの形

- 主張: `Std.Map` と `Std.Set` は、左右の高さの差をコンストラクタで表す AVL 木にする。高さか大きさを `match` か `if` の値として読む釣り合いの関数は合流のブロックを作り、token は合流へ流れない ([LIR](../spec/lir.md) の「token の流れ」) ので、節の箱を使い回せない。成分を受ける `bal l k v r` の形も、使い回しが 0 である。赤黒木は確保が約2倍で、更新が 25〜45% 遅い。`Set` を `Map a Unit` にしないのは、確保の数が同じで、節が1語小さく、API に `Unit` が出ないためである
- 数値: release、5回の中央値。文字列のキー 2000 種に30万回の `upsert` で、採った形 0.21 秒、赤黒木 0.26 秒、高さの欄を持つ AVL 0.49 秒、OCaml の AVL 0.37 秒、Adams の木 0.36 秒、AA 木 0.32 秒。昇順の Int のキー30万では 0.37 秒、0.53 秒、0.69 秒、0.64 秒、0.62 秒、0.55 秒。5万個に100万回の `lookup` はどれも 0.43〜0.45 秒。昇順の2万個を入れる確保は、検査モードで採った形 40012、赤黒木 99955、高さの欄を持つ AVL 327230、OCaml の AVL 347213、Adams の木 416480。2319493b に試作を足した木で測った。リポジトリにない試作で、コミットからは再現できない
- 根拠にしている所: [標準ライブラリ](../spec/stdlib.md) の「`Std.Map`」と「`Std.Set`」の釣り合いを直す関数の項、`bench/std_map.em`

### VM と Rust の実装の比の主な原因

- 主張: 今の VM の比は、どの行も約2倍の目標を大きく超える。主な原因は行ごとに違う。`wc` は1バイトずつ回すループの命令の数、`grep -n` は行ごとの写しと探索、`logstat` と `csv2jsonl` は行ごとの確保と RC である
- 数値: 「今の数値」の「C/Rust との比」の表 (M5)。`wc` 43.0、`grep -n` 4.0、`logstat` 13.1、`logstat --whole` 13.0、`csv2jsonl` 35.9。1 MB の入力 (`gen.py` の `log 1000` と `csv 1000`) の検査モードの `RunStats` は、`grep -n` が行ごとに確保 3.0、`rc_increments` 0.0、写しは入力の 2.0 倍。`logstat` が行ごとに確保 25.8、`rc_increments` 25.5、写しは 2.9 倍。`csv2jsonl` が行ごとに確保 35.8、`rc_increments` 26.7、写しは 5.8 倍である (be27bf7 の木に使い捨てのテストを足して測った。テストはリポジトリにない)
- 根拠にしている所: [ネイティブ化の設計](../future/native.md) の「目的と範囲」の目標、「1バイトずつ回すループ」

### Cranelift のコードと Rust の1回の走査

- 主張: `wc` の内側のループを Cranelift で書くと、同じ1回の走査の Rust (LLVM) のループに対して、バイトの読みを展開した形は約1.5〜1.7倍、借用した extern を呼ぶ形は約2.2〜2.6倍、所有の引数と1バイトごとの `dup` を持つ LIR の形は約3.7〜4.0倍になる。ネイティブ化で約2倍以内の目標に届くには、`String` と `Bytes` を1バイトずつ読む行を借りる extern ([LIR](../spec/lir.md) の「extern の借用」) を、さらに展開することが要る
- 数値: `wc` の内側のループを Cranelift 0.137 で書いた試作の ns/バイト。同じ形の Rust (LLVM) は 0.42〜0.44、検査なしの Cranelift は 0.48〜0.51、範囲と桁あふれの検査つきでバイトの読みを展開すると 0.65〜0.72、借りた extern を呼ぶと 0.94〜1.10、所有の引数の LIR の形は 1.59〜1.73 だった。同じコードでの Cranelift と LLVM の差は 0.9〜1.3 倍である。試作は、baea5793 の `wc` の LIR (`count$let$go`) と同じ形の CLIF を手で書いて JIT で動かし、`gen.py log 100000` の 102 MB の入力で、5回の最良を3〜4回くり返して測った (Apple M5、macOS 27、rustc 1.98.1)。リポジトリにない試作で、コマンドは残っていない
- 根拠にしている所: [ネイティブ化の設計](../future/native.md) の「目的と範囲」の目標

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
- 根拠にしている所: [ロードマップ](../future/roadmap.md) の「段の列」の生の要素の配列の段、`bench/spectral_norm.em` の先頭のコメント、`crates/eml_vm/tests/bench.rs` の `spectral_norm_allocates_14_fewer`

### 配列に書く構造体の箱

- 主張: 配列の要素と、配置で構造体の型を宣言したフィールドは一様な位置なので、そこに書く構造体の値は箱に入る。nbody の確保はこの箱だけである
- 数値: nbody は1回進めるごとに 25 (速度に20、位置に5) で、既定の1000回で `allocations` 25051。計算の途中の `Vec3` と、状態を渡す形の複製が返す組は確保しない。`RunStats` の値なので機種によらず、0fff343a の `bench.rs` のスナップショットと同じ
- 根拠にしている所: [ロードマップ](../future/roadmap.md) の構造体のフィールドを親の物体に並べることの項目、[実装の現在地](status.md) の「実行の費用」、`bench/nbody.em` の先頭のコメント、`crates/eml_vm/tests/bench.rs` の `nbody_allocates_25_per_step`

### 呼び出しの規約の引き換え

- 主張: 関数の値の、捕まえた値より後ろの引数と結果をいつも一様にする規約 ([LIR](../spec/lir.md) の「関数の ABI」) には、次の費用がある
  - 特殊化されない関数の値に `Float` や構造体を渡すと、呼ぶたびに引数と結果が箱に入る。`Int` の箱はタグを付けるだけで、確保しない
  - 値として参照する型どおりの関数には、変種 `<f>$uniform` ができる。末尾の位置で `apply` か `clause` か、結果が `ret` と互換でない直接の呼び出しに届く関数の変種は本体を写した写しの形になり、コードが増える
  - 値として生まれて値としても参照される関数は、直接呼ぶ所でも引数と結果を一様にして渡す
  - 直接だけ呼ばれる持ち上げた関数と、値の参照が残らない handler への特殊化の継続の関数は、型どおりのままなので、この費用を持たない ([Core IR](../spec/core-ir.md) の「直接だけ呼ばれる持ち上げた関数」)。節の写しは `perform` の地点ごとに継続の関数について複製するので、継続を閉包にせず、再開の値と状態も箱に入らない ([Core IR](../spec/core-ir.md) の「節の写しの、継続についての複製」)。その代わり、節の本体のコードは地点の数だけ写る
- 数値:
  - `apply_float` (10万回の `apply`) は `boxes` 200000、`allocations` 200002 で、呼ぶたびに引数と結果の箱を1つずつ確保する。`apply_int` も `boxes` 200000 だが、`allocations` は 2 である
  - `ep_float` の `allocations` は10万回の操作で 9 である
  - `unspecialized_map_passes_floats.em` は `boxes` 20、`allocations` 22。`boxes` はリストの10個の要素の箱と、ラムダの変種 `main$lambda0$uniform` が要素ごとに結果を入れる箱である
  - `bench/` と `tests/ui/run*` のプログラムの reuse の後の LIR は、関数が 3685、文が 33432 で、変種が 54、そのうち写しの形が 17 である ([テスト戦略](testing.md) の「開発用の数え方」)
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

### コンパイラの段階のハッシュ関数

- 主張: コンパイラの段階のハッシュ表のハッシュ関数は、FxHash (`rustc-hash`) にする。aHash 0.8、foldhash 0.2 (`fast`)、rapidhash 4.5 (`fast`) と比べると、x86_64 と aarch64 のどちらでも FxHash の命令の数が最も少なく、`eml check` の CPU 時間を 2% 以上縮める候補はなかった
- 数値:
  - M5 で release の `eml` を4つ組み、`bench/` の10本を51回ずつ交互に走らせた中央値。命令の数は、`eml check` と `eml run` のどの組でも FxHash が最も少なく、ほかの候補は 0.07〜0.71% 多い。`eml check` の CPU 時間の FxHash との差は -0.2〜+1.0% である (FxHash は 4.73〜9.34 ms)。`eml run` の差はおおむね ±1.5% の中にあり、2.8〜3.7% 長い4行は命令の数の差が 0.4% 以下なので、揺れとみなした。858c97c の木の比べるスクリプトで測った
  - x86_64 の Linux では、命令の数 (callgrind、`bench/` の6本) はほかの候補が 0.1〜1.3% 多く、`-C target-feature=+aes` で組んだ aHash も 0.1〜1.0% 多かった。CPU 時間 (41回の中央値) の差は、どの候補も ±2% の揺れの中に収まった。390f2cf の木で測った
  - 4つの `eml` の出力は、どのプログラムでも同じだった
- 根拠にしている所: [コンパイラの構成](architecture.md) の「外部 crate」の `rustc-hash` の行

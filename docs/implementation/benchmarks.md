# 計測の基準

位置づけ: 手引き。

実行の費用を測るための、基準のプログラムと測り方と、今の数値をまとめる。費用は2つの数で見る。

- 実行の仕事の回数 (`RunStats`): 決定的なので、テストのスナップショットで固定する。回数が変わると、スナップショットの差分として出る
- 実行した命令の数: macOS の `/usr/bin/time -l` が出す `instructions retired` を、`bench/run.sh` と `bench/rt.sh` で測る。スクリプトと数値計算は、Python と実時間で比べる (`bench/scripts.sh`)

下の「今の数値」の表は、段を閉じるときに、3つとも同じ機種で測り直して入れ替える ([ロードマップ](../future/roadmap.md) の「進め方」)。測り直すのは、前の表のコミットから `crates/eml_vm`、`crates/eml_rt`、`crates/eml_lir`、`crates/eml_core_ir`、`std/`、`bench/` のどれかが変わった段である。測るのはコードの最後のコミットの作業ツリーで、`-dirty` の値は載せない。段の途中では入れ替えない。段ごとの前後は、この文書の git の履歴 (`git log -p docs/implementation/benchmarks.md`) と、閉じるコミットのメッセージで読む。

今も判断の根拠になっている計測は、下の「所見」に題名の見出しで残し、コードと文書はその見出しを引く。各所見は、主張、支える数値と測った機種とコミット、根拠にしている所 (この所見を引く規則、コメント、項目)、受け入れた後退ならその理由を書く。残すのは、コードか `docs/` の規則が今も引くもの、[ロードマップ](../future/roadmap.md) の項目の根拠であるもの、受け入れた後退とその理由である。所見の数値は表と独立で、表の入れ替えでは更新しない。段が変えた規則を根拠にしている所見は、その段を閉じるときに測り直して更新するか、根拠でなくなったなら消す。

## 基準のプログラム

リポジトリの `bench/` に置く。各プログラムは結果を1行出力する。`empty.em` だけは何も出力しない。大きさは、debug ビルドで `debug_heap` を付けたときに1本1秒以内で終わるように決めた。

| ファイル | 中身 | 主に見るもの |
|---|---|---|
| `empty.em` | 何もしない `main` | 起動と標準ライブラリのコンパイルの分 |
| `fib.em` | `fib 25` | 呼び出しと `Int` の算術 |
| `loop.em` | 末尾再帰のループ | 末尾呼び出し |
| `state.em` | `get` と `put` を持つ `Counter` の handler の下で数える再帰 | すぐに再開する操作 |
| `fold_state.em` | `State.run` の中で `Yield.fold` に純粋なラムダを渡し、10万要素を合計する (S13d で足した) | 純粋なコールバックを節がその場で呼ぶこと (`yields` が0) |
| `list.em` | プログラムの中で定義した `List a` を `range`、`map`、`foldl` で合計する | 型変数のフィールドの `Int`、高階関数 |
| `tree.em` | 線形合同法で作った `Int` を二分探索木に入れ、総称な `size` と `fold` で畳む | 再帰的なデータ、`switch` と `unpack` |
| `orders.em` | 10万件の `Order` のレコードを、ラムダを渡した `List.filter`、`List.map`、`List.fold` で集計する (S13e で足した) | リストの高階関数に渡すラムダ |
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
| `records.em` | 4つのフィールドのレコードを、`{ st \| … }` で10万回更新する (S13e-4 で足した) | `decref` を元にする reuse |
| `rbtree.em` | 赤黒木に1万個の `Int` を入れる。`balance` は組み直す節を受ける (S13e-4 で足した) | 同じ関数の中で節を作り直す reuse |
| `tree_map.em` | 8191 節の完全二分木の全節を、`incr` で20回作り直す (S13e-4 で足した) | 書かないフィールドとその場の書き換え |
| `std_list.em` | 2万要素のリストに、std の `List.map` と `List.filter` を5回通す (S13e-4 で足した) | std のリストの関数の reuse |
| `sieve_var.em` | 引数 (既定は 20000) より小さい素数の数を、`var` で書いたふるいで数える (S14c-3 で足し、S14d で引数を足した) | 入れ子の `var` の、状態を渡す形の特殊化 |
| `sieve_by_hand.em` | `MutArray` を末尾再帰のループに線形に受け渡した版 | `sieve_var.em` の下限 |
| `fannkuch_var.em` | fannkuch-redux (既定は n = 7) を、3本の配列の変数と10本の `Unr` の変数で書く (S14c-3 で足し、S14d で n を引数にした) | 何重にも入れ子になった `var` と、配列の添字 |
| `fannkuch_by_hand.em` | 状態をすべて末尾再帰の局所関数に受け渡した版 | `fannkuch_var.em` の下限 |
| `nbody.em` | Benchmarks Game の5つの天体を、`Vec3` の値で引数の回数 (既定は 1000) だけ進める (S14d で足した) | ベクトルの値の確保と `Float` の算術 |
| `spectral_norm.em` | 引数の大きさ (既定は 100) のスペクトルノルムを、`Array Float` と `var` で求める (S14d で足した) | 配列の `Float` の要素の箱 |
| `mandelbrot.em` | 引数の大きさ (既定は 128) の正方形の格子で、集合に入る点を `Complex` の値で数える (S14d で足した) | 複素数の値の reuse |

S14d で足した3つと、`sieve_var.em`、`fannkuch_var.em` は、大きさを省略できる引数で受ける。引数がなければ表の既定の大きさで動き、回数のテストと `bench/run.sh` はこの大きさで測る。`bench/scripts.sh` は大きな大きさを渡して Python の版と比べる (下の「スクリプトの計測」)。大きさを読むために `Std.Env` と `String.to_int` のコードがプログラムに入るので、この5つの実行の分には、それをコンパイルして動かす約 2900 万命令が加わる (下の「S14d」)。

`_by_hand` の版は、対応する自動の版と同じ出力を出す。特殊化の効果は、自動の版の `RunStats` が手書きの版に近づくことで測る。

`list.em` と `tree.em` はデータ型を自分で定義する。S6a で Prelude に `List` とリストのリテラルが入った後も、前の段の記録と比べられるように、自分のデータ型を使い続ける。`list.em` と `tree.em` の多相な関数 (`map`、`foldl`、`size`、`fold`) では、型変数の位置の `Int` が `box` と `unbox` を通る。単相化の前後でこの回数がどう変わったかは、下の「記録」の「S4b (単相化)」にある。

## 回数のテスト

`crates/eml_vm/tests/bench.rs` は、プログラムごとに1つのテストで、VM の検査モードで走らせ、出力を確かめ、`RunStats` の全項目 (S13e で足した `allocations` と、S13e-4 で足した `reuses` を含む) をインラインスナップショットで固定する。`bench/` のファイルの一覧と、テストのプログラムの一覧が一致することも確かめる。`specialization_matches_hand_written_work` は、自動の版と `_by_hand` の版の6つの組で、`allocations`、`boxes`、`unboxes` が同じことを確かめる (S13e-1 の完了の条件。下の「S13e-1」。6つ目の `state_loop` の組は S14c-2 の完了の条件で、下の「S14c-2」)。`handler_specialization_removes_yields_and_evidence` は、`iter_generate` と `iter_pipeline` の `yields` と、`push_fold` と `fold_state` の `evidence_steps` が0であることを確かめる (S13e-2 の完了の条件。下の「S13e-2」)。`state_loop` と `state_nested` の `evidence_steps` が0であることも確かめる (S14c-2 の完了の条件)。`specialization_matches_hand_written_rc` は6つの組で RC の回数が同じことを、`iter_allocations_do_not_grow_with_the_input` と `iter_generate_allocates_as_much_as_the_direct_loop` は `Iter` で引く形の確保の数を確かめる (S13e-3 の完了の条件。下の「S13e-3」)。`rebuilding_programs_reuse_boxes` は、関数の中で手放して作り直す8つのプログラムで `reuses` が 0 より大きいことを確かめる (S13e-4 の完了の条件。下の「S13e-4」)。`var_matches_hand_written_work` は、`var` で書いた版と手書きの版の2つの組で、`var` の版の `evidence_steps` と `yields` が0であることと、確保の数の差が表 `RESIDUAL` の値に等しいことを確かめる (S14c-3 の完了の条件。下の「S14c-3」)。`nbody_allocates_25_per_step`、`mandelbrot_does_not_allocate_complex`、`spectral_norm_allocates_14_fewer` は、構造体の表現で消える確保を確かめる (S14e-2 の完了の条件。下の「S14e-2」)。`trade_offs` は、型から決める呼び出しの規約と構造体の表現の引き換えの形の回数を固定する (下の「S14e-1」と「S14e-2」)。回数は機械の速さに左右されないので、ふだんの `cargo test` で流す。

段の前後の回数は、このスナップショットの差分で残る。回数を変える変更は、その段の spec に期待値の変更として書く ([テスト戦略](testing.md) の「テストの変更の運用」)。S10a までの記録の回数は S10b で消したインタプリタのもので、`rc_increments`、`rc_decrements`、`peak_objects` は VM と数え方が違う (下の「S10a (VM)」)。`peak_stack` は S10b で足した欄で、それより前の記録にはない。

## 命令の数の測り方

```sh
bench/run.sh
```

`bench/run.sh` は、release ビルドの `eml run` (VM の速いモード) で各プログラムを3回ずつ走らせ、`instructions retired` と実時間の中央値と、実行の分を Markdown の表で出す。実行の分は、各プログラムの命令の数から `empty` の命令の数を引いた値である。`empty` はフロントエンド、コード生成、起動の費用なので、段の前後は実行の分で比べる。実行の分が 0 以下になったときは、理由を出して止まる。比べられる値でないものを記録しないためである。表の前に、測定の環境 (機種と CPU、macOS の版、`rustc -V`、コミット) を出す。コミットしていない変更があれば、コミットの後に `-dirty` が付く。プログラムが実行時エラーで止まったときと、`time -l` の出力から命令の数か実時間を読めなかったときは、その出力を見せて止まる。macOS でだけ動く。

valgrind が arm64 の macOS で動かないので、callgrind は使わない。`instructions retired` は callgrind ほど決定的ではないが、S4a の設計のときに release ビルドの `fib 30` を3回測った揺れは約0.1%だった。arm64 の命令の数なので、過去の調査で x86_64 の callgrind を使って測った値とは直接比べない。

`empty.em` の命令の数は、フロントエンド、コード生成、起動の分である。ほかのプログラムの値からこれを引くと、実行の分の目安になる。実時間は 0.01 秒単位なので、命令の数を主に見る。

段の終わりに `bench/run.sh` を流し、出力の環境と表を、下の「記録」に段の見出しを付けて足す。

`bench/rt.sh` は、`eml_rt` の速いモードを測る ([オブジェクトモデル](../spec/object-model.md))。`crates/eml_rt/examples/rt_bench.rs` を release ビルドにし、次の14の計測を `bench/run.sh` と同じ方法で3回ずつ走らせて、同じ形の表を出す。`bench/` のプログラムでなく、ヒープの操作を直接呼ぶ。

| 計測 | 中身 |
|---|---|
| `empty` | 何もしない。起動の分 |
| `list` | 1,000,000 個の cons セルのリストを作って捨てることを、10回繰り返す |
| `box-list` | 同じことを、`std::alloc` を使う `Box` のリストで行う。`list` と比べる基準である |
| `dup` | 局所の物体の `dup` と `decref` の組を、10,000,000 回繰り返す |
| `free` | cons セルを1つ確保して2つの語を書き、`decref` で空けることを、10,000,000 回繰り返す。`list` の解放は解放の連鎖の中の `release_ref` を通るので、`decref` が一意な物体を空ける道はこの計測で測る |
| `map` | 1,000,000 個のリストを作り、`map (+1)` に当たる作り直しを10回行う。古いセルは `release` で空け、新しいセルは `alloc` で取る |
| `map-reuse` | `map` と同じ作り直しを、`release_reuse` と `con_at` で行う。`eml_rt` だけを測るので、列は手で組む。reuse のパスを作る前に、reuse の効果の見込みを数字で持つために足した |
| `append` | 空の文字列から始め、一意な文字列に `string_push` で 1 バイトずつ足すことを、10,000,000 回繰り返す。その場の追記と、倍の容量での作り直しの費用である |
| `perform` | evidence が1つの `evv` で、`ev_lookup` と `ev_clause` を呼んで結果を手放すことを、10,000,000 回繰り返す。handler の中で `perform` してその場で戻る道である |
| `perform-deep` | 同じことを、8つの evidence の `evv` の最も奥の evidence に対して行う |
| `handle` | handler を `evv_get`、`ev_new`、`ev_set_clause`、`evv_push`、`evv_set` で設け、`ev_evv` と `evv_set` で外すことを、1,000,000 回繰り返す |
| `yield` | yield を始め、κ を取り出して入れ直し、yield を終える1往復を、10,000,000 回繰り返す |
| `mask` | 4つの evidence の `evv` で、2つ目の evidence を飛ばす `evv_mask` (項目は n = 0、m = 1 の1つ) を呼び、結果を手放すことを、10,000,000 回繰り返す |
| `poll` | 安全点の確かめ `Ctx::poll` を、100,000,000 回繰り返す |

`bench/rt.sh` は続けて、extern の計測のプログラム `crates/eml_rt/examples/rt_extern_bench.rs` の次の12の計測を、同じ方法で測って2つ目の表に出す ([オブジェクトモデル](../spec/object-model.md) の「extern」)。`rt_bench` と別のプログラムにするのは、上の14の計測のコード生成単位の分け方を変えないためである (下の「S9b-2b (実行文脈)」)。どの計測も速いモードの `Ctx` で動かす。渡す文字列の参照は、ヒープの文字列なら毎回 `dup` で、不死の文字列 (`concat-copy` の右と `read-all` のパス) なら `acquire_immortal` で作る。`show-int` は整数だけを渡すので、参照を作らない。

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

## スクリプトの計測

S12d の5本のスクリプト (`tests/ui/run/scripts/`) のうち入力の大きさが効く4本と、数値計算の5つのプログラムを、Python と比べる。目標は、ロードマップの速さの目標 (Python 3.14 の 1.5 倍以下の実時間) である。`bench/` の基準のプログラムと違い、命令の数でなく実時間と最大の常駐メモリを見る。比べる相手の Python の実行が、命令の数で比べられる形でないためである。

```sh
bench/scripts.sh
```

- 表は2つある。S12d のスクリプトの表は `wc` (ログを標準入力から)、`grep -n` (約 0.5% の行が一致するパターン)、`logstat` (1行ずつ読む)、`logstat --whole` (全体を読んでから分ける)、`csv2jsonl` (3つの列を選ぶ) の5行である。数値計算の表は nbody、spectral-norm、mandelbrot、fannkuch-redux、ふるいの5行である
- S12d のスクリプトの入力は、`bench/scripts/gen.py` が種類 (アクセスログか CSV) と大きさごとに、種を固定して1つずつ作る。`target/bench-scripts/` に大きさを名前に入れて置き、すでにあれば作り直さない
- 比べる Python の版は、スクリプトが `bench/scripts/<名前>.py`、数値計算が `bench/numeric/<名前>.py` で、Python の標準ライブラリで普通に書いたものである。速さのために手で凝ったものにはしない。数値計算の版は、浮動小数の演算を eml の版と同じ順で行い、ビットまで同じ結果を出す
- 各行の前に `bench/scripts/compare.sh` で、eml と Python の標準出力と終了コード (と書き出したファイル) が一致することを確かめる。一致しなければ止まる
- release ビルドの `eml run` (VM の速いモード) と `python3 -I` を3回ずつ走らせ、`/usr/bin/time -l` の実時間と最大の常駐メモリの中央値を出す。`bench/run.sh` と同じく macOS でだけ動く

### 大きさ

行ごとの大きさ (入力の KiB か、プログラムに渡す大きさ) は `bench/scripts.sh` が持ち、次の規則で決める。

- 記録する機種で、eml の実時間 (3回の中央値) がおよそ 300 ms になる大きさにする。Python の実時間はその大きさでの値をそのまま記録する
- 段を閉じる計測で、eml の実時間が 150 ms より短いか 600 ms より長い行は、`bench/scripts.sh` の大きさを変えて、閉じるコミットの前の別のコミットにし、その後で3つの表を測り直す。表に載せるのは新しい大きさの値だけで、変えた行と前後の値は閉じるコミットのメッセージに書く
- 表の「大きさ」の欄に、どの大きさで測ったかを残す
- 実時間は 0.01 秒の単位で出るので、300 ms では約 3% の刻みになる。小さな差は `instructions retired` で確かめる。Python の実時間が数十 ms の行 (`wc`) では、比が刻みの分だけ大きく揺れる
- fannkuch-redux の大きさは順列の長さ n で、n が1つ増えると実時間が約9倍になる。300 ms に近い n がないので、300 ms との比が最も小さい n にする

`bench/scripts/compare.sh` は、パスで渡す形 (`compare.sh --paths <eml の版> <Python の版> [引数...]`) のほか、UI テストの付属のファイルで出力を確かめるとき (`compare.sh --fixture <名前>`) にも使う ([テスト戦略](testing.md) の「分類」)。どちらかのファイルがなければ、比べずに止まる。

## 今の数値

段を閉じるときに入れ替える3つの表である。

### `bench/run.sh`

- 機種: Mac17,2 (Apple M5)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- コミット: 1a91f47-dirty

| プログラム | instructions retired | 実行の分 | 実時間 (s) |
|---|---:|---:|---:|
| `empty` | 44270332 | | 0.00 |
| `apply_float` | 124407386 | 80137054 | 0.00 |
| `apply_int` | 117404379 | 73134047 | 0.00 |
| `ep_float` | 228679308 | 184408976 | 0.01 |
| `fannkuch_by_hand` | 189871375 | 145601043 | 0.00 |
| `fannkuch_var` | 284475382 | 240205050 | 0.01 |
| `fib` | 130835331 | 86564999 | 0.00 |
| `fold_state` | 94689237 | 50418905 | 0.00 |
| `iter_generate` | 105322292 | 61051960 | 0.00 |
| `iter_pipeline` | 215626545 | 171356213 | 0.00 |
| `iter_pipeline_by_hand` | 216854473 | 172584141 | 0.00 |
| `list` | 203704476 | 159434144 | 0.00 |
| `loop` | 211613416 | 167343084 | 0.00 |
| `mandelbrot` | 467899681 | 423629349 | 0.02 |
| `nbody` | 189147731 | 144877399 | 0.01 |
| `orders` | 293895192 | 249624860 | 0.01 |
| `orders_by_hand` | 295224001 | 250953669 | 0.01 |
| `push_fold` | 91380114 | 47109782 | 0.00 |
| `push_fold_by_hand` | 91609147 | 47338815 | 0.00 |
| `rbtree` | 262667791 | 218397459 | 0.01 |
| `records` | 135841805 | 91571473 | 0.00 |
| `records_wide` | 208056957 | 163786625 | 0.00 |
| `sieve_by_hand` | 104062283 | 59791951 | 0.00 |
| `sieve_var` | 135102202 | 90831870 | 0.00 |
| `spectral_norm` | 520347076 | 476076744 | 0.02 |
| `state` | 82345729 | 38075397 | 0.00 |
| `state_loop` | 84380479 | 40110147 | 0.00 |
| `state_loop_by_hand` | 82126714 | 37856382 | 0.00 |
| `state_nested` | 96217286 | 51946954 | 0.00 |
| `std_list` | 198605894 | 154335562 | 0.00 |
| `sum_loop` | 79988438 | 35718106 | 0.00 |
| `tree` | 237866753 | 193596421 | 0.01 |
| `tree_by_hand` | 237228219 | 192957887 | 0.01 |
| `tree_map` | 185464389 | 141194057 | 0.00 |
| `upsert` | 514381178 | 470110846 | 0.02 |
| `upsert_by_hand` | 512520955 | 468250623 | 0.02 |

### `bench/rt.sh`

- 機種: Mac16,1 (Apple M4)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)

`#[inline]` を付ける前と付けた後の値である。

| 計測 | 付ける前 | 付けた後 | 増減 |
|---|---:|---:|---:|
| `empty` | 16916377 | 16871603 | -0.265% |
| `list` | 1250936658 | 1251747828 | +0.065% |
| `box-list` | 3075597568 | 3077582307 | +0.065% |
| `dup` | 276957929 | 277534043 | +0.208% |
| `free` | 1826974397 | 1827477618 | +0.028% |
| `map` | 1810030753 | 1810077397 | +0.003% |
| `map-reuse` | 599526043 | 600060135 | +0.089% |
| `append` | 277778672 | 278151001 | +0.134% |
| `perform` | 516949484 | 516913384 | -0.007% |
| `perform-deep` | 867007445 | 866979921 | -0.003% |
| `handle` | 613976003 | 613960335 | -0.003% |
| `yield` | 346975812 | 347096610 | +0.035% |
| `mask` | 3827016782 | 3827222755 | +0.005% |
| `poll` | 1116955000 | 1117010205 | +0.005% |
| `string-eq` | 345367596 | 345410807 | +0.013% |
| `string-eq-early` | 737165272 | 737167062 | +0.000% |
| `compare` | 345598258 | 345637867 | +0.011% |
| `concat-copy` | 2634022767 | 2634170400 | +0.006% |
| `show-int` | 2347008611 | 2347241326 | +0.010% |
| `show-int-long` | 5497107972 | 5497306183 | +0.004% |
| `show-string` | 3702661293 | 3703558636 | +0.024% |
| `show-string-dense` | 8642957599 | 8643718073 | +0.009% |
| `println-block` | 254480731 | 254474001 | -0.003% |
| `println-line` | 12147750999 | 12175342621 | +0.227% |
| `read-all` | 125349728 | 125400879 | +0.041% |
| `read-all-unsized` | 136933280 | 136846355 | -0.063% |

### `bench/scripts.sh`

- 機種: Mac17,2 (Apple M5)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- Python: Python 3.14.8

数値計算

| 計測 | 大きさ | eml の実時間 (s) | Python の実時間 (s) | 比 (eml / Python) | eml の最大 RSS (MB) | Python の最大 RSS (MB) |
|---|---:|---:|---:|---:|---:|---:|
| nbody | 140000 | 0.29 | 0.64 | 0.5 | 10 | 15 |
| spectral-norm | 510 | 0.31 | 0.86 | 0.4 | 9 | 14 |
| mandelbrot | 480 | 0.20 | 0.30 | 0.7 | 9 | 14 |
| fannkuch-redux | 9 | 0.51 | 0.24 | 2.1 | 10 | 14 |
| sieve | 3500000 | 0.27 | 0.16 | 1.7 | 35 | 41 |

スクリプト

| 計測 | 大きさ | eml の実時間 (s) | Python の実時間 (s) | 比 (eml / Python) | eml の最大 RSS (MB) | Python の最大 RSS (MB) |
|---|---:|---:|---:|---:|---:|---:|
| `wc` | 9000 KiB | 0.32 | 0.02 | 16.0 | 29 | 78 |
| `grep -n` | 70000 KiB | 0.29 | 0.26 | 1.1 | 9 | 17 |
| `logstat` | 6700 KiB | 0.30 | 0.11 | 2.7 | 10 | 19 |
| `logstat --whole` | 6700 KiB | 0.29 | 0.10 | 2.9 | 35 | 53 |
| `csv2jsonl` | 2900 KiB | 0.29 | 0.18 | 1.6 | 21 | 51 |

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
- 数値: `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` で、`state` -14.2%、`tree` -6.1%、`list` -4.6%、`loop` -1.1%、`fib` +2.5% (M4、73e31d1)
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

- 主張: 関数の Repr を型から決める規約で、増えた費用が2つある
  - 特殊化されない `List.map` が、受けた `Float` をそのまま返す関数の値を型どおりに呼ぶので、要素ごとに `Float` の箱が1つ増える
  - `apply` を使わないプログラムで、VM の命令のループが1回まわるごとに約3命令増えた。原因は分かっていない
- 数値:
  - `unspecialized_map_passes_floats.em` は `boxes` 20、`allocations` 22。型どおりに呼ばない規約では `boxes` 11、`allocations` 14 (ab9768c9)。今の値は 0fff343a の `trade_offs` のスナップショットと同じ
  - `apply` を使わないプログラムの実行の分は +0.2% (`nbody`) から +1.7% (`fannkuch_by_hand`)。M5、061eb96 と、f6aee0e に未コミットの変更を足した木。コミットからは再現できない
- 根拠にしている所: `crates/eml_vm/tests/bench.rs` の `trade_offs`、`tests/ui/run/functions/unspecialized_map_passes_floats.em` の先頭のコメント
- 受け入れた理由:
  - `Float` の箱: 特殊化されない `apply` を型どおりに呼ぶ規約の帰結で、`apply` の多いプログラムの実行の分が大きく減った (`apply_float` -43.3%、`apply_int` -33.5%、`ep_float` -17.8%)
  - ループの約3命令: `apply` の関数を展開する形、しない形、`FnEntry` の欄を減らす形のどれでも消えず、`apply` の多いプログラムで最もよい形を残した

### 構造体の表現の引き換え

- 主張: 小さな単一コンストラクタの型とタプルを構造体にしたことで、増えた費用が2つある
  - 特殊化されない `map` に組を入れ替える関数の値を渡すと、要素の組を箱から取り出して葉の並びで渡し、戻った組を新しい箱に入れる。共有した組を展開しない関数に渡すと、RC の葉ごとに `dup` する
  - `Option` の中の組を構造体の `unbox` と `release unboxed` で読むので、`iter_pipeline` と `orders` の実行の分が増えた
- 数値:
  - `swap_map_unspecialized.em` は `allocations` 52、`reuses` 40。構造体にしない場合は 34 と 50 (e6665e64)。`swap_map_specialized.em` は構造体にしない場合と同じ 32 と 50 (`SWAP_MAP_BEFORE_STRUCTS`)。`three_rc_leaves_through_a_call.em` は `rc_increments` 3。今の値は 0fff343a の `trade_offs` のスナップショットと同じ
  - `iter_pipeline` +3.4%、`iter_pipeline_by_hand` +3.5%、`orders_by_hand` +2.0%、`orders` +1.3%。M5、1a91f47 に未コミットの変更を足した木。コミットからは再現できない
- 根拠にしている所: `crates/eml_vm/tests/bench.rs` の `trade_offs` と `SWAP_MAP_BEFORE_STRUCTS`、`tests/ui/run/records/swap_map_unspecialized.em` と `three_rc_leaves_through_a_call.em` の先頭のコメント
- 受け入れた理由:
  - 組の箱と葉の `dup`: データに通した関数の値は、葉の並びの規約で呼ぶほかない。構造体の表現で、`records` -26.9%、`mandelbrot` -24.7% と、確保の多いプログラムの実行の分が大きく減った
  - `Option` の組の2命令: 増えたのは組を読む2つの命令の分と見ている。VM の中までは調べていない

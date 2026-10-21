# 付録 C: benchmarks.md の書き直し (S3)

計画書 plan.md が正である。この付録と plan.md が食い違うときは plan.md に従う。

## 1. 結論

### 1.1 新しい章立て

S1 の6節に合わせ、次の見出しにする。外から引かれる「基準のプログラム」「回数のテスト」「命令の数の測り方」「スクリプトの計測」は名前を変えない。`citations.rs` は見出しの深さを見ないので [V] (`headings()` は `#` を除いた行全体を比べる。g2 の5章)、2つを `## 測り方` の下に下げても引用は壊れない。

```
# 計測の基準                     (冒頭は単位1の文案のまま)
## 基準のプログラム              (36 行の表)
## 回数のテスト
## 測り方                        (新設。導入の1文だけ)
### 命令の数の測り方            (bench/run.sh と bench/rt.sh)
### スクリプトの計測            (bench/scripts.sh と compare.sh)
#### 大きさ                     (単位1の文案に、fannkuch-redux の例外を1文足す)
## 今の数値
### `bench/run.sh`
### `bench/rt.sh`               (表2つ: rt_bench と rt_extern_bench)
### `bench/scripts.sh`          (表2つ: スクリプトと数値計算)
## 所見                         (1.3 の20の見出し)
## 記録                         (C2 で消すまで、所見の後ろに残る)
```

所見は「今の数値」の後ろ、「記録」の前に置く。ファイルに書く所見の形は次の4つの箇条だけである。

```
### <題名>

- 主張: …
- 数値: … (機種、コミット)
- 根拠にしている所: …
- 受け入れた理由: …          (受け入れた後退のときだけ)
```

ファイルに書かないものは3つある。1.3 の「元の記録」の行と「(S2 が引く)」のような作業の注は、この計画の中だけのもので、ファイルに写さない。「根拠にしている所」は、ファイルと見出しか関数や定数の名前で書き、行番号は書かない (例: `crates/eml_rt/src/heap/mod.rs` の `decref` のコメント、[オブジェクトモデル](../spec/object-model.md) の「核の操作」)。

「数値」の機種とコミットは次の3つの書き方のどれかにする。

- コミットで測った値: 「M5、fc990f9」
- コミットに未コミットの変更を足して測った値: 「M4、8b98876 に未コミットの変更を足した木。コミットからは再現できない」
- コミットの記録がない値: 「M4、コミットの記録はない」

「M4」は Mac16,1 (Apple M4)、「M5」は Mac17,2 (Apple M5) と、所見の節の頭に1度だけ書く。OS はどれも macOS 27.0.1 (26A434)、rustc は 1.98.1 で、所見1 だけ 1.95.0 である。

`## 測り方` の導入の文案。

```
命令の数は `bench/run.sh` と `bench/rt.sh` で、Python との実時間の比は `bench/scripts.sh` で測る。どれも macOS の `/usr/bin/time -l` を使うので、macOS でだけ動く。
```

「大きさ」に足す1文 (5章の M の手順2と同じ決まり)。

```
- fannkuch-redux は、実時間が 150 ms から 600 ms の範囲を外れても、300 ms との比が最も小さい n のままにし、大きさを決め直さない
```

### 1.2 記録より前の節の食い違い19件の直し

番号は g2 の 1.1 の表のものである。作業の名前は5章のもの。

| # | 箇所 | 直し | 作業 |
|---|---|---|---|
| 1 | 冒頭の「S4b の単相化…下の「記録」にある」 | 単位1 が冒頭を置き換えている。C1 で消えたことだけ確かめる | 単位1 |
| 2 | 冒頭の「S10 の VM は、ここの記録と比べる」 | 同上 | 単位1 |
| 3 | 表が29行で、7本がない | 1.2.1 の7行を足す。由来の括弧 (「(S13d で足した)」など11か所) を消す。`records`、`nbody`、`mandelbrot` の行を今の表現に直す | C1 |
| 4 | `RunStats` の項目の列挙に `conversions` と `unwound_frames` がない | 項目を列挙せず「`RunStats` の全項目 ([VM](../spec/vm.md) の「`RunStats`」)」と書く | C1 |
| 5 | テストの列挙に3本がない | 「回数のテスト」を 1.2.2 の文案に置き換える | C1 |
| 6 | `rebuilding_programs_reuse_boxes` の8つ | 今と合う [V] (`bench.rs` の配列)。そのまま | C1 |
| 7 | 組の数、`RESIDUAL` など | 今と合う。文案は値を書かず「表 `RESIDUAL` の値」と書く | C1 |
| 8 | 「約 2900 万命令 (下の「S14d」)」 | M の手順6の値で置き換え、出典の括弧を「(「今の数値」の環境で、大きさを読むだけのプログラムを測った値)」にする | C1 |
| 9 | `bench/run.sh` の説明 | 「段の前後は実行の分で比べる」を「プログラムどうしと、測り直した表どうしは実行の分で比べる」にする | C1 |
| 10 | 「S4a の設計のときに … `fib 30` を3回測った揺れは約0.1%」 | M の手順3の値で「同じ木で2回流すと、実行の分の差は最大で X% (`<名前>`、実行の分 N 命令) だった」と書き、`fib 30` の文は消す | C1 |
| 11 | `rt.sh` の計測の表 | 今と合う [V] (g2 の #11) | なし |
| 12 | `map-reuse` の「reuse のパスを作る前に…足した」 | 「VM の reuse の命令の速い道と同じ基本操作を、`eml_rt` だけで測る」にする。同じ古いコメントの `rt_bench.rs` は B1 で直す | C1、B1 |
| 13 | 「S12d の5本のスクリプト」 | 「`tests/ui/run/scripts/` のスクリプトのうち入力の大きさが効く4本」にし、表の名前を「スクリプト」にする。`bench/scripts.sh` の出力の「S12d のスクリプト」は B1 で直す | C1、B1 |
| 14 | 「大きさ」の節 | 単位1 が置き換えている。1.1 の1文を足す | C1 |
| 15 | `gen.py` と `target/bench-scripts/` | 今と合う | なし |
| 16 | `compare.sh` の形が2つしか書いてない | 3つの形を並べる: `--paths <eml の版> <Python の版> [引数...]`、`<名前> [引数...]` (`tests/ui/run/scripts/<名前>/main.em` と `bench/scripts/<名前>.py` を比べる)、`--fixture <名前>`。`compare.sh` の使い方のコメントと同じ [V] | C1 |
| 17 | `[テスト戦略](testing.md) の「分類」` | 見出しがある。そのまま | なし |
| 18 | `bench.rs` と本文の相互参照 | そのまま | なし |
| 19 | 本文から記録の節への「下の「…」」 | 本文の書き直しで全部なくなる。理由が要る所は所見を引く (`_by_hand` と `list.em` の段落は引用を外す。`peak_stack` と CEK の文は消す) | C1 |

#### 1.2.1 基準のプログラムの表の行

足す7行 (中身は各 `.em` の先頭のコメントから取った [V])。位置は表の中の関係の近い行の後にする。

```
| `state_loop.em` | 末尾でない `put` を `Std.State` の handler の下で回すループ | 状態を渡す形の特殊化 |
| `state_loop_by_hand.em` | `state_loop.em` のループの状態を、引数で手で受け渡した版 | `state_loop.em` の下限 |
| `state_nested.em` | 状態つきの handler を2つ重ねたループ | 入れ子の handler の特殊化 |
| `apply_int.em` | 実行時に選んだ `Int -> Int` の関数の値を、ループの中で10万回 `apply` する | 型どおりの Repr で呼ぶ `apply` |
| `apply_float.em` | `apply_int.em` の `Float -> Float` の版 | `Float` の引数と結果が `apply` を通る費用 |
| `ep_float.em` | `Float` の結果と `Int` の状態を持つ操作を、特殊化しない handler の下で10万回起こす | EP の道で、操作の結果と状態を型どおりの Repr で運ぶ費用 |
| `records_wide.em` | `records.em` を7つの `Int` のフィールドのレコードにした版 | 構造体にしない値の reuse |
```

直す3行。

```
| `records.em` | 4つのフィールドのレコードを、`{ st \| … }` で10万回更新する | 構造体の値の更新 (確保も reuse もしない) |
| `nbody.em` | Benchmarks Game の5つの天体を、`Vec3` の値で引数の回数 (既定は 1000) だけ進める | 配列に書く構造体の箱と `Float` の算術 |
| `mandelbrot.em` | 引数の大きさ (既定は 128) の正方形の格子で、集合に入る点を `Complex` の値で数える | 構造体の値の算術 |
```

`records` は `reuses` 0、`allocations` 7、`mandelbrot` は `allocations` 2、`reuses` 0、`nbody` は `allocations` 25051 である [V] (`bench.rs` のスナップショットを Python で読んだ)。表の後の段落は次の形にする。

```
`nbody.em`、`spectral_norm.em`、`mandelbrot.em`、`sieve_var.em`、`fannkuch_var.em` は、大きさを省略できる引数で受ける。…(以下は今の文のまま。最後の文の数と出典を #8 のとおり置き換える)

`list.em` と `tree.em` はデータ型を自分で定義する。2つの多相な関数 (`map`、`foldl`、`size`、`fold`) では、型変数の位置の `Int` が `box` と `unbox` を通る。
```

「S6a で Prelude に…使い続ける」と「単相化の前後で…「S4b (単相化)」にある」は消す。比べる記録がなくなるためである。

#### 1.2.2 「回数のテスト」の文案

```
## 回数のテスト

`crates/eml_vm/tests/bench.rs` は、プログラムごとに1つのテストで、VM の検査モードで走らせ、出力を確かめ、`RunStats` の全項目 ([VM](../spec/vm.md) の「`RunStats`」) をインラインスナップショットで固定する。`every_program_in_bench_has_a_test` は、`bench/` のファイルの一覧とテストのプログラムの一覧が一致することを確かめる。回数は機械の速さに左右されないので、ふだんの `cargo test` で流す。

プログラムをまたいで比べるテストは次のとおりである。

- `specialization_matches_hand_written_work` と `specialization_matches_hand_written_rc`: 自動の版と `_by_hand` の版の6つの組で、`allocations`、`boxes`、`unboxes` と RC の回数が同じである
- `handler_specialization_removes_yields_and_evidence`: `iter_generate` と `iter_pipeline` の `yields` と、`push_fold`、`fold_state`、`state_loop`、`state_nested` の `evidence_steps` が0である
- `iter_allocations_do_not_grow_with_the_input` と `iter_generate_allocates_as_much_as_the_direct_loop`: `Iter.fold` で引く形の確保の数が入力の大きさによらず、直接のループ `sum_loop` と同じである。`Iter.filter` を重ねた形は、残した要素ごとに4つ確保する
- `rebuilding_programs_reuse_boxes`: 関数の中で手放して作り直す8つのプログラムで `reuses` が0より大きい。`allocations_are_counted_in_checked_mode` は、確保の数が検査モードの数え方から来ることを確かめる
- `var_matches_hand_written_work`: `var` で書いた版と手書きの版の2つの組で、`var` の版の `evidence_steps` と `yields` が0で、確保の数の差が表 `RESIDUAL` の値に等しい
- `nbody_allocates_25_per_step`、`mandelbrot_does_not_allocate_complex`、`spectral_norm_allocates_14_fewer`: 構造体の表現で消える確保と残る確保の数 (下の「配列に書く構造体の箱」と「配列の `Float` の要素の箱」)
- `typed_benches_do_not_convert`: どのプログラムでも `conversions` が0である
- `unwind_points`: `bench/` と `tests/ui/run/scripts/` のプログラムの巻き戻しの表の行の数を固定する
- `trade_offs`: 呼び出しの規約と構造体の表現の引き換えの形の回数を固定する (下の「呼び出しの規約の引き換え」と「構造体の表現の引き換え」)

回数を変える変更は、期待値の変更として段の作業文書に書く ([テスト戦略](testing.md) の「テストの変更の運用」)。
```

`bench.rs` の `#[test]` は51本で、プログラムごとの36本のほかは上の15本である [V] (`grep -c '#\[test\]'` と関数の名前の一覧)。`unwind_points` が `tests/ui/run/scripts/` も数えることは `bench.rs` の本体から読んだ [V]。S4 の W-C2 が `typed_benches_do_not_convert` を書き換えても、この文は変わらない。

### 1.3 「所見」に残すもの (20)

並べる順は、`eml_rt`、VM、Core IR、言語の性能の順である。「元の記録」と「この所見を引く S2 の行」はファイルに書かない (1.1)。

#### 1. 「`decref` が一意な物体を空ける道」

- 主張: 速いモードの `decref` の速い道は、rc が 1 なら `#[inline(never)]` で `#[cold]` でない解放の連鎖を直に呼ぶ。遅い道に入れる形と、連鎖を包む関数を呼ぶ形は命令が多い
- 数値: `free` の計測で、遅い道に進む形 1,905,243,516、遅い道から `#[cold]` を外した形 1,896,524,999、直に呼ぶ形 1,785,276,496 (約6.3%、1回あたり約12命令の減り)。包む関数を呼ぶ形は 1,825,319,011。ほかの計測はどの形でも 0.4% 以内。連鎖の根を作業リストに積まない2つの形は `list` が約10.4% と約33.3% 増えた。M4、rustc 1.95.0、56211a5 に未コミットの変更を足した木 (コミットからは再現できない)
- 根拠: `crates/eml_rt/src/heap/mod.rs` の `decref` のコメント、[オブジェクトモデル](../spec/object-model.md) の「核の操作」
- 元の記録: S9a

#### 2. 「破棄処理の判定」

- 主張: 箱を空けるたびに読む破棄処理の判定は、`Kind` の `match` を通さず、`Meta::has_finalizer` が種類のビットを `Foreign` の番号と直に比べる。`Kind::from_bits` の腕が1つ増えるだけで、解放の多い計測が数%増える
- 数値: 腕を1つ足すと `list` +5.56%、`free` +3.25%、`map` +3.70%。腕を除くと 0.25% 以内に戻った。M4、8b98876 に未コミットの変更を足した木
- 根拠: `crates/eml_rt/src/meta.rs` の `has_finalizer` のコメント、[オブジェクトモデル](../spec/object-model.md) の「種類」
- 元の記録: S9b-1 の1つ目の原因

#### 3. 「`eml_rt` のインライン化」

- 主張: `eml_rt` の速い道は、呼ぶ側に展開されていることを `-C remark=inline` の「definition is unavailable」で確かめる。展開されないと、`map-reuse` と `append` の命令の数が倍を超える。原因はどれも、総称な関数の実体が呼ぶ側と別のコード生成単位に入ることか、総称でない関数が crate をまたいで展開されないことだった。次の置き方はそのためである
  - 総称な関数 (`set_field`、`field`、`string_push`、`push_unchecked`、`release_unkept`、`meta`、`scanned`) と、繰り返しの中の総称でない関数 (`release_children`、`Control::stop_bits`、`into_token`、`drop_token`) に `#[inline]` を付ける
  - `Arc` を持つ欄 (`Heap` の付けた塊、`Pages` の `Control`、`HostRef` の Host) は `ManuallyDrop` にし、`Drop` が `#[inline(never)]` の関数に渡して手放す。欄の後始末が展開されると、単位の分け方が変わる
  - 検査モードだけの確かめ (`checked_unshared`) は、呼ぶ側の `M::CHECKED` の枝の中で呼ぶ
  - `rt_extern_bench` を `rt_bench` と別のプログラムにし、`rt_bench` の単位の分け方を変えない
  - この戻し方は脆い。`release_children` は展開の閾値の近くにある
- 数値:
  - `release_children` に `#[inline]` がないと `map-reuse` が 600,306,311 から 1,281,810,080 命令になり、付けると `append` は1回あたり約73命令から約28命令になった。M4、8b98876 に未コミットの変更を足した木
  - 付けた塊の欄を `Option<Arc<Frozen<M>>>` のまま持つと `map-reuse` +46%、`append` +152%。`checked_unshared` を速いモードでも呼ぶと `map-reuse` +49%、`append` +152%。M4、fd26d0d に未コミットの変更を足した木
  - `Ctx` を計測のプログラムで使い始めたとき `map-reuse` +85%、`append` +152%。`field` を除く4つに `#[inline]` を付けた形では、同じ設定のビルドを繰り返すと `map-reuse` が約566M と約1058M 命令の間で変わり、`field` を足すと3回のビルドで約568M に留まった。`stop_bits` に付けると `poll` は1回あたり約14命令から約11命令になった。M4、28eb18d
  - `into_token` と `drop_token` に付ける前後で、26の計測のどれも 0.5% より大きくは増えなかった (最大は `println-line` の +0.227%)。M4、e5f86d8 の前の未コミットの木
  - VM の繰り返しから「definition is unavailable」で展開されなかった `eml_rt` の20の関数は、どれも繰り返しの外か冷えた道にあった。M4、73e31d1
- 根拠: `crates/eml_rt/src/heap/release.rs` の `release_children`、`heap/mod.rs` の `region` の欄と `checked_unshared` と `set_field`、`alloc.rs` の `Pages` の `control` の欄、`control.rs` の `unbounded`、`host.rs` の `release_host`、`examples/rt_extern_bench.rs` の先頭のコメント。[オブジェクトモデル](../spec/object-model.md) の「核の操作」、「extern」の `Ctx` を取らない算術の項、「`unsafe` の核」の T4 の行。[VM](../spec/vm.md) の「置き場所」の `eml_rt` の項
- 元の記録: S9b-1 の2つ目の原因、S9b-2a、S9b-2b、S13e-4 の「`eml_rt` の `#[inline]`」、S10a の `#[inline]` の判断

#### 4. 「出力のバッファと Host の呼び出し」

- 主張: 標準出力と標準エラーは `Mutex` の中の 64 KiB の `BufWriter` にまとめて書く。行ごとに flush すると約65〜160倍遅い。Host は `dyn Host` で呼び、総称の型引数にしない。動的な呼び出しの分は、書き込みの費用と比べて小さい
- 数値: 23 バイトの行を100万行書くと、行ごとの flush はファイルへ 1.30 µs/行、パイプへ 324 ns/行、`BufWriter` は 8 ns/行と 5 ns/行 (M4、設計のときの使い捨ての計測で、コミットの記録はない)。`rt.sh` の `println-block` (`dyn Host` の呼び出しを含む) は1行あたり約238命令、`println-line` は約12,131命令 (M4、42aee31)
- 根拠: [オブジェクトモデル](../spec/object-model.md) の「extern」の「出力のバッファ」と Host の項
- 元の記録: S9b-3

object-model.md の Host の項の「数ナノ秒」は記録に直接の値がない [V] (S9b-3 の節を読んだ)。B2 で、この文は「動的な呼び出しの分は、書き込みの費用と比べて小さい」にし、数を書かない。

#### 5. 「`Fs.read_all` の読み方」

- 主張: `Fs.read_all` は `Vec` に読んでからヒープへ写す。ヒープに直接読む形は、空き領域を0で埋める費用が省いた写しとほぼ相殺するので 6〜10% しか速くならず、信頼する関数が4つ要る。mmap はファイルが縮むと SIGBUS で落ち、`Vec` をそのまま文字列にすると文字列の全操作が2つの形に分かれる。読み手を `StopOnInterrupt` で包むので、`read_to_end` は空き領域を0で埋めてから読む
- 数値: 採った形は 1 MiB で 60.2 µs、64 MiB で 7.0 ms (M4、ページキャッシュにあるファイル、設計のときの計測で、コミットの記録はない)。0で埋めるようにすると `read-all` +12.1%、`read-all-unsized` +10.0% (M4、1f36698 と 223d52f)
- 根拠: [オブジェクトモデル](../spec/object-model.md) の「`File`」
- 受け入れた理由: `read_buf` が stable の Rust にないため、中断で止められる読みを優先した
- 元の記録: S9b-3、S11c-1

#### 6. 「`codegen-units`」

- 主張: release のプロファイルの `codegen-units` は既定の値のままにする。1 にすると、VM の実行の分はプログラムによって減ったり増えたりする
- 数値: C1 で M の手順7の値に置き換える。それまで (A の時点) は記録の値を書く: `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` で、`state` -14.2%、`tree` -6.1%、`list` -4.6%、`loop` -1.1%、`fib` +2.5% (M4、73e31d1)
- 根拠: [VM](../spec/vm.md) の「置き場所」の `codegen-units` の項
- 元の記録: S10a

M の手順7で、どのプログラムも減るか、どれも増える結果になったら、主張が成り立たない。そのときは C1 に進まず、ユーザーに聞く (`vm.md` の規則の見直しはこの作業の範囲の外)。

#### 7. 「`eml_vm` のインライン化」

- 主張: `eml_vm` の関数には、`vm::run` に「definition is unavailable」で展開されていないことと、付けると `bench/run.sh` で命令の数が減ることの2つが分かったときだけ `#[inline]` を付ける。`data` と `apply` の9つの関数は両方に当たった。`apply::apply` は付けても変わらなかったので付けない
- 数値: 9つに付けると実行の分が `tree` 約5.4%、`list` 約3.8%、`state` 約2.8%、`fib` と `loop` 約1.2〜1.3% 減った。`apply::apply` は 0.2% 以内で、`fib` は約0.2% 増えた。基本操作の命令を `vm::step` の `match` に移すと、`state` が約17%、ほかが1〜4% 減った。M4、33cd286 から 4b5ef6c2 の間の未コミットの木。今のコードでは測り直していない
- 根拠: [VM](../spec/vm.md) の「置き場所」の `eml_vm` の項
- 元の記録: S10b の「`eml_vm` の関数のインライン化」

`4b5ef6c2` が9つの関数に `#[inline]` を付けたコミットであることは `git log -S'#[inline]' -- crates/eml_vm/src/data.rs` で確かめた [V]。

#### 8. 「reuse の命令の関数の置き方」

- 主張: `release_reuse` と `con_reuse` は `#[inline]` にして token を使う道だけを `vm::run` に展開し、値が共有されていた道 (`release_shared`) と token が空の道 (`con_fresh`) は `#[cold]` と `#[inline(never)]` の関数に出す。3つの置き方のうち、reuse するプログラムで最も減り、reuse しないプログラムの増え方も 0.5% 以内に収まった
- 数値: 基準 9ed9a349 からの実行の分の増減。`#[inline(never)]` では `tree` -4.93% (試作は -9.03%)。全部を `#[inline]` にすると reuse しない `iter_pipeline` +1.49%、`iter_pipeline_by_hand` +1.59%。採った形は `tree` -11.12%、`std_list` -14.28%、reuse しないプログラムは最大 +0.33% (`fib`)。M4、e5f86d8 の前の未コミットの木
- 根拠: [VM](../spec/vm.md) の「置き場所」の reuse の命令の項
- 元の記録: S13e-4 の「reuse の命令の関数の置き方」

#### 9. 「ブロック引数の合併と比較の融合」 (6章の Q1 の既定)

- 主張: VM のコード生成は、ブロック引数と実引数に同じ色を付けやすくする合併と、比較と分岐を1つにした命令を持たない。どちらもなしで、VM の実行の分は前の CEK のインタプリタの約0.10〜0.17倍に収まったので、簡潔さを先にした。CEK のインタプリタはもうないので、この比べは測り直せない
- 数値: 実行の分の VM / CEK は `fib` 0.102、`list` 0.171、`loop` 0.130、`state` 0.151、`tree` 0.169。M4、73e31d1
- 根拠: [VM](../spec/vm.md) の「スロットの割り当て」と「switch」の判断の項
- 元の記録: S10a

#### 10. 「`INLINE_LIMIT` と `GROWTH_FACTOR`」

- 主張: `INLINE_LIMIT` の 40 は、測った値 (10、20、40、80) のうち、どのプログラムも下げ止まる最小の値である。書き換えで生まれた呼び出しの展開の予算 `GROWTH_FACTOR` は8倍で、`bench/` と `tests/ui/run`、`tests/ui/run-fail` のどの関数も予算に届かない。予算がないと、本体が自分を k 回呼ぶ `Fix` のラムダで関数が回ごとに k 倍に育つ
- 数値: 実行の分で、`iter_pipeline` は 20 で 295,711,502、40 で 197,919,428 (20 では49% 遅い)。`upsert` は 10 で 535,233,564、20 以上で約490M (10 では9% 遅い)。80 は 40 と ±0.2% 以内。予算なしの `Fix` のラムダは、本体の呼び出し3で Core IR 157,491 行・0.31 秒・206 MB、4で 1,747,659 行・4.2 秒・2.5 GB。8倍の予算では約400行。育ち方の最大は `tests/ui/run/std/std_iter_lin.em` の `main` の 48 から 179 (3.7倍) で、4倍では余裕が13文しかない。M4、b9b2ff0 の前の未コミットの木 (定数を変えて測った。記録はしきい値の表のコミットを書かない)
- 根拠: `crates/eml_core_ir/src/optimize/fuse/mod.rs` の `GROWTH_FACTOR` と `fuse/inline.rs` の `INLINE_LIMIT`、[Core IR](../spec/core-ir.md) の「インライン化」と「融合が止まること」
- 元の記録: S13e-3 の「インライン化のしきい値」と「書き換えで生まれた呼び出しの展開の予算」

#### 11. 「インライン化で増えるスタック」

- 主張: 末尾でない再帰の中に小さな関数を展開すると、1つのフレームのスロットが増え、深い再帰でスタックの上限に早く当たる
- 数値: `upsert` の `peak_stack` は 352 語で、展開しなければ 206 語 (6f77feb)。`RunStats` の値なので機種によらない。352 は 0fff343a の `bench.rs` のスナップショットと同じ [V]
- 根拠: [実装の現在地](status.md) の最適化が扱わない形の項
- 受け入れた理由: 展開は、自動の版と手書きの版の組の RC の回数をそろえるために要る
- 元の記録: S13e-3

#### 12. 「reuse の token で増えるスタック」

- 主張: 末尾でない再帰の呼び出しをまたいで reuse の token を持つと、token のスロットでフレームごとに1語増え、深い再帰でスタックの上限に早く当たる
- 数値: `list` の `peak_stack` は 700,015 語で、token を持たなければ 600,014 語 (9ed9a349)。`tree_map` は 126 語と 112 語。0fff343a の `bench.rs` のスナップショットと同じ [V]
- 根拠: [実装の現在地](status.md) の実行の費用の項
- 受け入れた理由: token をフレームの中に限る形 (frame-limited reuse) は、token を呼び出しの引数と戻り値で渡さない簡単な形で、その費用として受け入れた
- 元の記録: S13e-4 の「実行の仕事の回数」

status.md の見出しは S2 が決める。「根拠」の見出しの名前は、A を書く時点の S2 の見出しに合わせる。

#### 13. 「1バイトずつ回すループ」

- 主張: eml で1バイトずつ回すループは、`bench/loop.em` の1反復に約330命令かかる。入力を1バイトずつ数える `wc` は Python の版の約14倍の実時間で、Python の版は C で書かれた `bytes.count` と `bytes.split` に任せている。1行の読みと検索を extern に任せる `grep -n` は Python とほぼ同じになる。素のループでは VM は CPython より速い
- 数値: `loop` の実行の分 165,337,806 命令 / 500,000 反復 (M5、fc990f9)。30 MB の入力で `wc` は eml 1.01 秒、Python 0.07 秒 (14.4倍)、`grep -n` は 1.1倍 (M5、5bb2254、Python 3.14.7)。3,000万回のループは VM 0.26 秒、Python 0.66 秒で、`wc` を Python でも1バイトずつのループで書くと VM 1.56 秒、Python 1.45 秒 (M4、Python 3.14.7、リポジトリにない使い捨ての試作で、コミットの記録はない)
- 根拠: [実装の現在地](status.md) の実行の費用の項、[ロードマップ](../future/roadmap.md) の「ネイティブ化」の完了の条件
- 元の記録: S12d (スクリプト)、S13c、S13 の前の試作、S14d

`wc` の比は、「今の数値」の表 (大きさ 9000 KiB) では Python の実時間が 20〜30 ms と粗く、10〜16倍の間で振れる (S14d と S14e-2 の記録)。所見は30 MB の入力の値を持つので、表の比とは違う数になる。本文に「30 MB の入力で」と書いて区別する。

#### 14. 「ネイティブ化の見込み」

- 主張: `wc` の内側のループを LIR のまま Rust に写すと Python の約1.2倍、借用とインライン化を足すと約0.45倍になる。ネイティブ化で Python の1.5倍以内に届く見込みはある。ただし Cranelift のコードは LLVM より遅いことがあり、確保の多い `logstat` は測っていない
- 数値: LIR をそのまま写した Rust (`opt-level=1`。1バイトごとの `dup`、`String.byte_at` の呼び出しと `decref`、`is_space` の呼び出しを残す) は読み込みを含めて約0.10秒、借用とインライン化を足すと約0.035秒。M4、Python 3.14.7、リポジトリにない使い捨ての試作で、コミットの記録はない
- 根拠: [ロードマップ](../future/roadmap.md) の「ネイティブ化」の完了の条件
- 元の記録: S13 の前の試作

#### 15. 「`Yield` を変える handler の yield」

- 主張: `Yield a` を `Yield b` に変える handler の `fn yield` の節は、節が外側の `yield` を起こすので、外側の handler が特殊化される `Yield.fold` でも、`State.run` の中でも、1要素ごとに yield する
- 数値: 1,000 要素の `map_lists` (`tests/ui/run/effects/pure_callback_under_state.em` と同じ形) を `Yield.fold` で畳むと、`yields` 1000、`evidence_steps` 4002。`State.run` の中でも同じ値。0fff343a の検査モード (`eml_test_support::run_stats`) [V]
- 根拠: [ロードマップ](../future/roadmap.md) の「処理系」の、yield しうる外のエフェクトを呼ぶ節をその場で実行する項目、[実装の現在地](status.md) の実行の費用の項
- 元の記録: S13 の前の試作、S13d

#### 16. 「状態を渡す形にしない handler」

- 主張: どの節もすぐに再開する handler でも、`perform` の結果と節の末尾の `resume k(v, s)` の Repr か印が合わないものは、状態を渡す形の条件 (`resumes_fit`) に当たって EP の道に残る。経路がすべて末尾の呼び出しでも、末尾の経路の形には切り替えない
- 数値: `bench/` には当たるプログラムがなく、`RunStats` は変わらなかった (M5、e19347bc と c2e34728)。`crates/eml_core_ir/tests/optimize.rs` の `a_resume_value_of_another_repr_keeps_the_handle` が、この handle が残ることを固定する
- 根拠: [Core IR](../spec/core-ir.md) の「handler への特殊化」
- 受け入れた理由: 1つの handler に2つの形を持たないため
- 元の記録: S14c-2 の「受け入れた後退」

#### 17. 「配列の `Float` の要素の箱」

- 主張: 配列の要素は一様な位置なので、`Array Float` と `MutArray Float` の要素は書くたびに `Float` の箱に入る。spectral-norm の確保のほとんどはこの箱である
- 数値: spectral-norm (既定の大きさ) は `allocations` 4043、`boxes` 4001。10回のくり返しで行列とベクトルの積を40回計算し、そのたびに100個の要素を作る (40 × 100 = 4000)。状態を渡す形の複製が返す組を構造体にしない場合は 4057 だった (e6665e64)。`RunStats` の値なので機種によらず、0fff343a の `bench.rs` のスナップショットと同じ [V]
- 根拠: [ロードマップ](../future/roadmap.md) の「スカラーの要素を生で持つ配列」、`bench/spectral_norm.em` の先頭のコメント、`crates/eml_vm/tests/bench.rs` の `spectral_norm_allocates_14_fewer`
- 元の記録: S14d の「S14e が消す確保と消さない確保」、S14e-2 の「完了の条件」

#### 18. 「配列に書く構造体の箱」

- 主張: 配列の要素と、配置で構造体の型を宣言したフィールドは一様な位置なので、そこに書く構造体の値は箱に入る。nbody の確保はこの箱だけである
- 数値: nbody は1回進めるごとに 25 (速度に20、位置に5) で、既定の1000回で `allocations` 25051。計算の途中の `Vec3` と、状態を渡す形の複製が返す組は確保しない。0fff343a の `bench.rs` のスナップショットと同じ [V]
- 根拠: [ロードマップ](../future/roadmap.md) の「構造体のフィールドを親の物体に並べること」、[実装の現在地](status.md) の構造体にしない型の項、`bench/nbody.em` の先頭のコメント、`crates/eml_vm/tests/bench.rs` の `nbody_allocates_25_per_step`
- 元の記録: S14d の「S14e が消す確保と消さない確保」、S14e-2 の「完了の条件」

#### 19. 「呼び出しの規約の引き換え」

- 主張: 関数の Repr を型から決める規約で、増えた費用が2つある
  - 特殊化されない `List.map` が、受けた `Float` をそのまま返す関数の値を型どおりに呼ぶので、要素ごとに `Float` の箱が1つ増える
  - `apply` を使わないプログラムで、VM の命令のループが1回まわるごとに約3命令増えた。原因は分かっていない
- 数値:
  - `unspecialized_map_passes_floats.em` は `boxes` 20、`allocations` 22。型どおりに呼ばない規約では `boxes` 11、`allocations` 14 (ab9768c9)。今の値は 0fff343a の `trade_offs` のスナップショットと同じ [V]
  - `apply` を使わないプログラムの実行の分は +0.2% (`nbody`) から +1.7% (`fannkuch_by_hand`)。M5、061eb96 と、f6aee0e に未コミットの変更を足した木
- 根拠: `crates/eml_vm/tests/bench.rs` の `trade_offs`、`tests/ui/run/functions/unspecialized_map_passes_floats.em` の先頭のコメント
- 受け入れた理由:
  - `Float` の箱: 特殊化されない `apply` を型どおりに呼ぶ規約の帰結で、`apply` の多いプログラムの実行の分が大きく減った (`apply_float` -43.3%、`apply_int` -33.5%、`ep_float` -17.8%)
  - ループの約3命令: `apply` の関数を展開する形、しない形、`FnEntry` の欄を減らす形のどれでも消えず、`apply` の多いプログラムで最もよい形を残した
- 元の記録: S14e-1 の「実行の仕事の回数」と「基準のプログラムの命令の数」

#### 20. 「構造体の表現の引き換え」

- 主張: 小さな単一コンストラクタの型とタプルを構造体にしたことで、増えた費用が2つある
  - 特殊化されない `map` に組を入れ替える関数の値を渡すと、要素の組を箱から取り出して葉の並びで渡し、戻った組を新しい箱に入れる。共有した組を展開しない関数に渡すと、RC の葉ごとに `dup` する
  - `Option` の中の組を構造体の `unbox` と `release unboxed` で読むので、`iter_pipeline` と `orders` の実行の分が増えた
- 数値:
  - `swap_map_unspecialized.em` は `allocations` 52、`reuses` 40。構造体にしない場合は 34 と 50 (e6665e64)。`swap_map_specialized.em` は構造体にしない場合と同じ 32 と 50 (`SWAP_MAP_BEFORE_STRUCTS`)。`three_rc_leaves_through_a_call.em` は `rc_increments` 3。今の値は 0fff343a の `trade_offs` のスナップショットと同じ [V]
  - `iter_pipeline` +3.4%、`iter_pipeline_by_hand` +3.5%、`orders_by_hand` +2.0%、`orders` +1.3%。M5、1a91f47 に未コミットの変更を足した木
- 根拠: `crates/eml_vm/tests/bench.rs` の `trade_offs` と `SWAP_MAP_BEFORE_STRUCTS`、`tests/ui/run/records/swap_map_unspecialized.em` と `three_rc_leaves_through_a_call.em` の先頭のコメント
- 受け入れた理由:
  - 組の箱と葉の `dup`: データに通した関数の値は、葉の並びの規約で呼ぶほかない。構造体の表現で、`records` -26.9%、`mandelbrot` -24.7% と、確保の多いプログラムの実行の分が大きく減った
  - `Option` の組の2命令: 増えたのは組を読む2つの命令の分と見ている (VM の中までは調べていない)。`Iter` は再帰する型で構造体にならず、組だけが構造体になる形の帰結である [I]
- 元の記録: S14e-2 の「実行の仕事の回数」と「基準のプログラムの命令の数」

所見20 の「`Iter` は再帰する型で構造体にならず…帰結である」は記録にない推測である。A を書く前に、記録の文 (「増えはこの2つの命令の分と見ている」) だけで足りるなら、その1文に留める。

#### 1.3.1 S2 の仮の名前との対応

S2 の初版 (1.6) の仮の名前と、この20の見出しの対応である。

| S2 の仮の名前 | この見出し | 違い |
|---|---|---|
| 1バイトずつ回すループの費用 | 「1バイトずつ回すループ」 | 「の費用」を付けない |
| 型を変える handler の `yield` | 「`Yield` を変える handler の yield」 | 変えるのは `Yield` の型引数なので、`Yield` を名指す |
| インライン化で増えるスタック | 「インライン化で増えるスタック」 | 同じ |
| reuse の token で増えるスタック | 「reuse の token で増えるスタック」 | 同じ |
| 一様な位置の境目の変換 | 所見にしない | `conversions` は回数の定義で、計測の所見がない。status.md:109 は [VM](../spec/vm.md) の「`RunStats`」と [LIR](../spec/lir.md) の「関数の ABI」を引く |
| 配列に書く構造体の箱 | 「配列に書く構造体の箱」 | 同じ |
| spectral-norm の確保は `Float` の箱 | 「配列の `Float` の要素の箱」 | 規則 (配列の要素の箱) の名前にし、spectral-norm は数値の中で書く |
| ネイティブ化の見込み | 「ネイティブ化の見込み」 | 同じ |

S2 の roadmap.md:186 の「CPS のラムダの位置の変換 (試作で 200,125 回)」は benchmarks.md に記録がない [V] (`grep -c 200,125 docs/implementation/benchmarks.md` が 0)。所見にしないので、S2 の既定 (消す) のままでよい。

r1 から変えた見出しは次のとおりである。

| r1 | r2 |
|---|---|
| 「末尾でない再帰のフレームの大きさ」 | 「インライン化で増えるスタック」と「reuse の token で増えるスタック」に分けた |
| 「配列の要素の箱」 | 「配列の `Float` の要素の箱」と「配列に書く構造体の箱」に分けた |
| 「呼び出しの規約と構造体の表現の引き換え」 | 「呼び出しの規約の引き換え」と「構造体の表現の引き換え」に分けた |
| (なし) | 「ブロック引数の合併と比較の融合」を足した (6章の Q1 の既定) |

#### 1.3.2 残さない記録

残す基準に当たらないので消す: S4a、S4b、S5、S6a、S6b、S6c、S8d、S9v、S11c-3、S12a、S12b、S12c、S13b-1、S13b (スクリプト)、S14a、S14b、S14c-1。引かれていても所見にしないものは次のとおりで、付け替えで引用を外す。

- S7、S8b (status.md:91): S1 が「段ごとの増減は消す」と決めている
- S10a の `RunStats` の CEK と VM の差 (vm.md:411): 6章の Q1
- S13 の前の試作のうち書き心地の記述 (status.md:5、roadmap.md:5): 項目と順を決めた材料という経緯で、今の判断の根拠でない
- 各段の「完了の条件」(bench.rs のコメント): 条件はテストそのものが持つ。「回数のテスト」がテストの確かめることを書く
- S14d の「書いてみて分かったこと」: E2002 と E3008 は spec が書き、配列の箱は所見17 と18 が持つ
- S14c-3 の「添字1つに3つの extern」: 今も当てはまるかを確かめていない。6章の Q4

### 1.4 引用の付け替えの対応表

#### benchmarks.md の外から benchmarks.md を引く103か所

「そのまま」は見出しが残るので変えない。行の後の括弧は今引いている見出しである。付け替える文の段の名前は、同じ作業で消す。

bench/ (26)

| 引用元 | 新しい引用 | 作業 |
|---|---|---|
| `bench/run.sh:4` (命令の数の測り方) | そのまま | なし |
| `bench/rt.sh:4` (見出しなし) | そのまま | なし |
| `bench/scripts.sh:3` (スクリプトの計測) | そのまま。2行目と96行目のコメント、131行目と133行目の出力の「S12d のスクリプト」を「スクリプト」にする | B1 |
| `bench/scripts/compare.sh:3`、`gen.py:1`、`wc.py:2`、`grep.py:2`、`logstat.py:2`、`csv2jsonl.py:2` (スクリプトの計測) | そのまま | なし |
| `bench/numeric/{nbody,spectral_norm,mandelbrot,fannkuch,sieve}.py:2` (スクリプトの計測) | そのまま | なし |
| `bench/{nbody:108,spectral_norm:43,mandelbrot:42,sieve_var:24,fannkuch_var:58}.em` (スクリプトの計測) | そのまま | なし |
| `bench/nbody.em:3` (S14e-2) | 「配列に書く構造体の箱」。1-2行目の「(S14d の計測)」「S14e の構造体の表現で」を消し、「計算の途中の `Vec3` は構造体なので確保しない」にする | B1 |
| `bench/spectral_norm.em:3` (S14d) | 「配列の `Float` の要素の箱」。1行目の「(S14d の計測)」を消す | B1 |
| `bench/mandelbrot.em:4` (S14d) | `docs/spec/core-ir.md の「構造体にする型」`。2-4行目の古い記述を「`Complex` は構造体なので、まわすたびに確保しない」にし、1行目の「(S14d の計測)」を消す | B1 |
| `bench/apply_int.em:3`、`apply_float.em:3`、`ep_float.em:4`、`records_wide.em:3` (S14e-1) | `docs/implementation/benchmarks.md の「基準のプログラム」`。「(S14e-1 の基準)」を消し、「S14e の規約は」「S14e の構造体の表現は」を「型から決める呼び出しの規約は」「構造体の表現は」にする | B1 |

crates/eml_vm (17。どれも `crates/eml_vm/tests/bench.rs`)

| 行 | 新しい引用 | 作業 |
|---|---|---|
| 2 (回数のテスト) | そのまま | なし |
| 50、456、473、482、494、964、1000、1030、1046、1135、1237 (各段の完了の条件) | 「回数のテスト」。「S… の完了の条件」を消し、テストが確かめることだけを書く。456 の「S13e-3 の前の 900008 の約4分の1に減った」は「残した要素ごとの4つの確保の上限 (200,100) を固定する」に、473 の「S13e-2 の完了の条件は…だった。S13e-3 の融合の後は」は「融合の後は」にする | B1 |
| 977 (S14e-2) | `docs/spec/core-ir.md の「構造体にする型」`。「S14e-2 で構造体になり」を「構造体になり」にする | B1 |
| 1132 (S14e-1) | `docs/spec/vm.md の「`RunStats`」`。S4 の W-C2 がこの定数ごと消す | B1 |
| 1155 (S14e-1) | 「呼び出しの規約の引き換え」と「構造体の表現の引き換え」 | B1 |
| 1268 (S14d と S14e-1) | 「配列の `Float` の要素の箱」。コメントを「組を構造体にしない場合の spectral-norm の確保の数」にし、定数を `SPECTRAL_NORM_BOXED_TUPLE_ALLOCATIONS` に改名する (2か所) | B1 |
| 1285 (S14e-2) | 「構造体の表現の引き換え」。「(コミット e6665e64)」を消す (ハッシュは所見20 が持つ) | B1 |

`bench.rs` で引用のない段の名前 (1031 の「S13e-3 で揃うので」、1061 の「S14a が」、1257 と 1271 の「S14e-2 の完了の条件。」) も B1 で直す。

crates/eml_rt (12)

| 引用元 | 新しい引用 | 作業 |
|---|---|---|
| `examples/rt_bench.rs:1` (見出しなし) | そのまま。2行目の「reuse analysis はまだないので、…パスを作る前に数字で持つためである」を「VM の reuse の命令の速い道と同じ基本操作を、`eml_rt` だけで測るためである」にする | B1 |
| `examples/rt_extern_bench.rs:1` (見出しなし) | そのまま | なし |
| `examples/rt_extern_bench.rs:3` (S9b-2b、`docs/` なしの形) | `docs/implementation/benchmarks.md の「`eml_rt` のインライン化」`。`docs/` から書き、`citations.rs` が検査する形にする | B1 |
| `src/heap/mod.rs:2256` (S9a) | 「`decref` が一意な物体を空ける道」 | B1 |
| `src/meta.rs:179` (S9b-1) | 「破棄処理の判定」 | B1 |
| `src/heap/release.rs:377` (S9b-1)、`src/alloc.rs:175`、`src/control.rs:51`、`src/heap/mod.rs:151`、`:3235` (S9b-2a)、`src/heap/mod.rs:3381`、`src/host.rs:341` (S9b-2b) | 「`eml_rt` のインライン化」 | B1 |

crates/eml_core_ir (1)

| 引用元 | 新しい引用 | 作業 |
|---|---|---|
| `src/optimize/fuse/mod.rs:44` (S13e-3) | 「`INLINE_LIMIT` と `GROWTH_FACTOR`」 | B1 |

docs/spec (21)。S3 が文ごと書き直し、S4 はこの21の文に触れない。

| 引用元 | 新しい引用と文の直し | 作業 |
|---|---|---|
| object-model.md:17 (見出しなし) | そのまま | なし |
| object-model.md:78 (S9b-1) | 「破棄処理の判定」 | B2 |
| object-model.md:302 (S9b-2b と S9a) | 総称な関数の理由は「`eml_rt` のインライン化」。最後の文「命令の数は … 「S9a (`eml_rt` の核)」にある」は「`decref` の解放の道を選んだ計測は … の「`decref` が一意な物体を空ける道」に、今の命令の数は「今の数値」にある」にする | B2 |
| object-model.md:805 (S9b-2b) | 「`eml_rt` のインライン化」 | B2 |
| object-model.md:914 (S9b-3) | 「出力のバッファと Host の呼び出し」。「動的な呼び出しの費用は数ナノ秒で」を「動的な呼び出しの分は」にする | B2 |
| object-model.md:927 (S9b-3) | 「出力のバッファと Host の呼び出し」。「約 65〜160 倍速い」は所見4 に任せ、「行ごとに flush する書き方より速い」にする | B2 |
| object-model.md:953 (S11c-1) | 「`Fs.read_all` の読み方」 | B2 |
| object-model.md:960 (S9b-3) | 「`Fs.read_all` の読み方」 | B2 |
| object-model.md:992 (S9b-2a) | 「`eml_rt` のインライン化」 | B2 |
| vm.md:5 (見出しなし) | そのまま | なし |
| vm.md:17 (見出しなし) | 文案 A (下) | B2 |
| vm.md:18 (S10b) | 「`eml_vm` のインライン化」 | B2 |
| vm.md:19 (S13e-4) | 「reuse の命令の関数の置き方」。「試作の半分ほど」「約1.5% 増えた」の数は所見8 に任せ、「`bench/tree.em` の減り方が小さく」「`bench/iter_pipeline.em` の命令の数が増えた」にする | B2 |
| vm.md:20 (S10a) | 「`codegen-units`」。括弧の中の数を消し、引用だけにする | B2 |
| vm.md:130 (S10a) | 「ブロック引数の合併と比較の融合」。文案 B (下)。Q1 の既定 | B2 |
| vm.md:250 (S10a) | 同上。文案 C (下) | B2 |
| vm.md:411 (S10a) | 文を消す。Q1 の既定 | B2 |
| core-ir.md:588 (S14c-2) | 「状態を渡す形にしない handler」。「S13e-2 の末尾の経路の形はこうした handler も特殊化していたが、…S14c-2 からは特殊化しない。受け入れた後退である」を「末尾の経路の形にすればこうした handler も特殊化できるが、1つの handler に2つの形を持たないために特殊化しない」にする | B2 |
| core-ir.md:686 (S13e-3、リンクの文字が「基準の計測」) | 「`INLINE_LIMIT` と `GROWTH_FACTOR`」。リンクの文字を「計測の基準」にする | B2 |
| core-ir.md:725 (S13e-3、同上) | 同上。「(本体が自分を4回呼ぶ `Fix` のラムダで、Core IR が175万行になった)」は所見10 に任せて消す | B2 |

文案 A (vm.md:17 の箇条を置き換える)。

```
- `eml_rt` の関数に、VM のために `#[inline]` を足すのは、繰り返しの中で呼ぶ関数にインライン化の注釈 (`-C remark=inline`) が「definition is unavailable」を示したときだけにする。足す前と後に `bench/rt.sh` を流し、どの計測も 0.5% より大きく増えないことを確かめる。`#[inline]` の置き方によって、`map-reuse` の命令の数がビルドごとに約2倍振れたためである ([計測の基準](../implementation/benchmarks.md) の「`eml_rt` のインライン化」)
```

文案 A の「足す前と後に…確かめる」は、今の文の「前後に `bench/rt.sh` を流して…記録する」の言い換えで、「0.5%」は記録の完了の条件 (S13e-4) の値である。記録する先がなくなるので、確かめる中身を書いた。

文案 B (vm.md:130)。新しい条件を足さず、判断の根拠を所見に置く。

```
- ブロック引数と実引数に同じ色を付けやすくする合併は行わない。移動の命令を減らす工夫だが、合併なしで VM の実行の分の命令の数が前のインタプリタの約 0.10〜0.17 倍に収まったので、簡潔さを先にした ([計測の基準](../implementation/benchmarks.md) の「ブロック引数の合併と比較の融合」)
```

文案 C (vm.md:250)。最後の文を次にする。

```
… テストが増える。融合なしで VM の実行の分の命令の数が前のインタプリタの約 0.10〜0.17 倍に収まったので、簡潔さを先にした ([計測の基準](../implementation/benchmarks.md) の「ブロック引数の合併と比較の融合」)
```

文案 B と C は今の文から「S10a で比べた」だけを消した形で、数を spec に残す。S1 は spec に数を書かないと決めているので、数を消して「前のインタプリタより十分に少なかったので」とする形もある。どちらにするかは Q1 で一緒に聞く。

docs/implementation (13)

| 引用元 | 新しい引用 | 作業 |
|---|---|---|
| status.md:5 (S13 の前の試作) | S2 が付け替える。引用を外す (所見なし) | S2 |
| status.md:90 (S13d) | S2 が付け替える。「`Yield` を変える handler の yield」 | S2 |
| status.md:91 (S7、S8b) | S2 が付け替える。引用を外す | S2 |
| status.md:96 (S12d と S13c) | S2 が付け替える。「1バイトずつ回すループ」。S2 の初版のとおり約20倍、約330命令、約1.7秒を消す (所見13 の値 14.4倍、30 MB の入力と食い違うため) | S2 |
| status.md:97 (S13e-3) | S2 が付け替える。benchmarks.md の引用を外す (spec の引用は残る) | S2 |
| status.md:101 (S13e-3) | S2 が付け替える。RC の回数は「回数のテスト」、`peak_stack` は「インライン化で増えるスタック」。「±0.4% 以内」は消す | S2 |
| status.md:103 (S13e-4) | S2 が付け替える。「reuse の token で増えるスタック」 | S2 |
| status.md:109 (S14e-1) | S2 が付け替える。[VM](../spec/vm.md) の「`RunStats`」と [LIR](../spec/lir.md) の「関数の ABI」 | S2 |
| status.md:110 (S14e-2) | S2 が付け替える。「配列に書く構造体の箱」 | S2 |
| testing.md:80 (命令の数の測り方) | そのまま | なし |
| testing.md:268 (スクリプトの計測) | そのまま | なし |
| testing.md:356 (見出しなし) | そのまま | なし |

docs/future (6)

| 引用元 | 新しい引用 | 作業 |
|---|---|---|
| roadmap.md:5 (S13 の前の試作) | S2 が付け替える。引用を外す。同じ行の `docs/superpowers` のパスも S2 が消す | S2 |
| roadmap.md:174 (S13 の前の試作) | S2 が付け替える。「ネイティブ化の見込み」 | S2 |
| roadmap.md:184 (S14d) | S2 が付け替える。「配列の `Float` の要素の箱」 | S2 |
| roadmap.md:185 (S14e-2) | S2 が付け替える。「配列に書く構造体の箱」 | S2 |
| roadmap.md:190 (S13 の前の試作と S13d) | S2 が付け替える。2つとも「`Yield` を変える handler の yield」にし、1つにまとめる | S2 |
| roadmap.md:28 (同じファイルの下の「…」の `wc` は20倍) | S2 が「1バイトずつ回すループ」を引く形にする (S2 の初版の 28 の行) | S2 |

roadmap.md:28 は benchmarks.md を引かないので103に入らないが、所見13 の数と食い違う `wc` の「20倍」を持つので、S2 への申し送りとして載せた。

docs/README.md (2) と CLAUDE.md (2)

| 引用元 | 新しい引用 | 作業 |
|---|---|---|
| `docs/README.md:34` (リンクの文とパス) | 単位1 が地図の行を置き換えている | 単位1 |
| `CLAUDE.md:106` (記録と見出しなし) | 単位1 が「今の数値」に置き換えている | 単位1 |

tests/ui (3)

| 引用元 | 新しい引用 | 作業 |
|---|---|---|
| `tests/ui/run/functions/unspecialized_map_passes_floats.em:1-2` (S14e-1) | 「呼び出しの規約の引き換え」 | B1 |
| `tests/ui/run/records/swap_map_unspecialized.em:1` (S14e-2) | 「構造体の表現の引き換え」 | B1 |
| `tests/ui/run/records/three_rc_leaves_through_a_call.em:4` (S14e-2) | 「構造体の表現の引き換え」 | B1 |

#### 103に入らない当たり

依頼の grep (`benchmarks.md\|計測の基準\|基準の計測\|docs/superpowers`) に当たるが、benchmarks.md への引用でないもの。

| 箇所 | 扱い |
|---|---|
| `docs/overview.md:96` の「S4a 計測の基準」 | 段の一覧の表の語で、リンクもパスもない。benchmarks.md への引用でないので S3 は持たない。overview.md の段の名前は S1 の単位4 (S4 の側) の担当 |
| `docs/implementation/status.md:5` の「S4a 計測の基準」 | 同上。status.md は S2 が書き直す |

#### docs/superpowers を引く10行 (superpowers の外)

| 引用元 | 新しい引用 | 作業 |
|---|---|---|
| `crates/eml_cli/tests/citations.rs:15`、`:25` | 単位1 が `docs/plans` に付け替えている | 単位1 |
| `docs/implementation/testing.md:308` | 単位1 が文案に置き換えている | 単位1 |
| `docs/future/roadmap.md:5` | S2 がパスを消す。バッククォートの中のパスだけで見出しを引かないので、D の後に残っても `citations::` は落ちない [V] (`citations.rs` は `の「` の前のパスを見る。g3 の4節) | S2 |
| `docs/implementation/benchmarks.md:1549`、`:2290`、`:2429`、`:2515` | 記録の節ごと消える。2429 と 2515 の見出し付きの引用はバッククォートの中で、検査されない [V] (g3 の4節) | C2 |
| `tests/ui/run/effects/specialized_handler_under_state.em:3` (「意味が変わらない理由」) | `docs/spec/core-ir.md の「handler の特殊化で意味が変わらない理由」` | D |
| `tests/ui/run/effects/specialized_clause_performs_outer.em:2` (同上) | 同上 | D |
| `tests/ui/run/effects/specialized_constant_operation_argument.em:3` (「書き換え」) | `docs/spec/core-ir.md の「handler の複製」` (定数の操作の引数の規則がこの節の中にある) | D |

core-ir.md に「handler の特殊化で意味が変わらない理由」と「handler の複製」の見出しがある [V] (`grep -n '^#' docs/spec/core-ir.md`)。

### 1.5 コードとテストの段の名前の除去 (範囲の追加)

対象は `git ls-files crates std tests bench` のうち、`crates/eml_syntax/tests/corpus.rs` と `corpus/` を除くファイルで、正規表現 `\bS[0-9]+[a-z]?(-[0-9a-z]+)?\b` に当たる行である。0fff343a で40ファイル96行ある [V] (`grep -nE` の行数とファイル数)。`std/` には当たりがない。corpus の2つのファイルは `S1` (コーパスの名前) のほかに当たりがない [V]。

| ファイル | 行 | 担当 |
|---|---|---|
| `bench/` の16ファイル | 1.4 の行と、`fannkuch_var.em:1`、`fold_state.em:1`、`orders.em:2`、`sieve_var.em:1`、`state_loop.em:1`、`state_nested.em:1` | B1 |
| `crates/eml_vm/tests/bench.rs` | 1.4 の行と 1031、1061、1257、1271 | B1 |
| `crates/eml_rt/examples/rt_extern_bench.rs`、`src/{alloc,control,host,meta}.rs`、`src/heap/{mod,release}.rs` | 1.4 の行 | B1 |
| `crates/eml_rt/src/control.rs:62`、`externs.rs:63`、`host.rs:404` | 引用のない段の名前 | B1 (同じ eml_rt の検査で済ませる) |
| `crates/eml_core_ir/src/optimize/fuse/mod.rs:44` | 1.4 の行 | B1 |
| `tests/ui` の3本 (1.4) | 1.4 の行 | B1 |
| `crates/eml_core_ir/src/optimize/fuse/copies.rs:38,139`、`fuse/known_con.rs:765`、`handlers/mod.rs:115,196,794`、`specialize.rs:28,193`、`translate/instances.rs:704,735` | 引用のない段の名前 | B3 |
| `crates/eml_core_ir/tests/optimize.rs:203,366,678,726,966,967,1154` | 同上 | B3 |
| `tests/ui/run/effects/state_passing_nested.em:1`、`tests/ui/run/float/float_bits_through_structs.em:1`、`tests/ui/run/scripts/{csv2jsonl,deploy,grep,logstat,wc}/main.em:1` | 同上 | B3 |
| `crates/eml_core_ir/src/optimize/fuse/known_con.rs:4` | 同上 | S2 の W3 (同じ行の引用を S2 が直す。S2 の初版の 1.7) |

直し方の決まり。

- 段の名前で機能を指す所は、機能の名前か spec の節に替える。「S13e-1 の規則」は「既知の閉包の規則 (docs/spec/core-ir.md の「既知の閉包」)」、「S13e-2 の handler への特殊化」は「handler への特殊化」、「融合 (S13e-3)」は「融合」、「S8d の V2」は「V2 (docs/spec/lir.md の「Kind の印の検査」)」、「S12 の `try_io`」は「`try_io`」、「S10 の VM」は「VM」、「(S12d のスクリプト)」は消す、「(S14c-2)」は消す、「S14e の新しい道」は「型から決める呼び出しの規約の道」にする
- 時期だけを言う所 (「S12 で行ごとに読む extern を足すときに」) は、今の状態の文にする。`host.rs:404` は今は `read_line` があるので「行ごとに読む extern (`read_line`) も、そのまま使えるよう `BufRead` で渡す」にする [I] (`Stdin.read_line` があることは testing.md の std の分類から読んだ。実装者は `host.rs` の呼び出しで確かめる)
- 文字列の `specialize.rs:193` の `unreachable!("S13e-1 keys hold only closures")` は `unreachable!("known-closure keys hold only closures")` にする。到達しない分岐の文言で、テストの期待値に出ない [I] (`grep -rn 'keys hold only' crates` で、テストが文言を比べていないことを実装者が確かめる)
- 名前の中の段の名前は `SPECTRAL_NORM_S14E1_ALLOCATIONS` だけである [V] (`grep -noE` で `S1[0-9][A-Ea-e]` を含む識別子を探した。ほかは tests/ui の `.em` のパスの `s13e` で、D が消す)。B1 で `SPECTRAL_NORM_BOXED_TUPLE_ALLOCATIONS` に改名する。正規表現は大文字の `S14E1` に当たらないが、段の名前なので改名する
- コメントの行の数は変えない (UI テストの出力と `RunStats` に影響しないが、念のため)

### 1.6 測り直し

5章の M に手順を書く。「今の数値」の3つの表、本文の2つの数 (#8 の大きさを読む分、#10 の揺れ)、所見6 (`codegen-units`) を測り直す。所見6 は環境変数で測れるので、コードを変えない。ほかの所見の数値は測り直さない。コードを変えて測った値 (所見3、7、8、10 など) は、変えた木がコミットにないので「再現できない」と書き、そのまま残す。

## 2. 根拠

- 所見の粒度は「1つの規則か1つのしきい値につき1つの見出し」(S1 の6章の既定) に従った。例外は所見3 で、`eml_rt` の8か所のコメントと spec の4か所が引く。どれも「速い道が展開されないと命令の数が倍になる。`-C remark=inline` で確かめる」という1つの規則の場合分けなので、1つにした
- r1 の「末尾でない再帰のフレームの大きさ」「配列の要素の箱」「呼び出しの規約と構造体の表現の引き換え」は、それぞれ原因の違う2つを束ねていた。S2 の status.md と roadmap.md は別々の項目からそれぞれを引くので、分けた。受け入れた理由も原因ごとに書ける
- 所見11、12、16、19、20 は受け入れた後退で、残す基準の3つ目に当たる。所見19 の「ループの約3命令」はどこからも引かれていないが、原因の分からない後退として残っているので、S1 の基準どおり残す
- 所見9 を足したのは、vm.md の2つの規則が今も引くので、S1 の残す基準の1つ目に当たるからである。比べた CEK はもうないが、判断の根拠として記録の値を残し、測り直せないことを所見に書く。引用を外して数を消す形は、規則の理由を言い換えることになるので、ユーザーに聞く (Q1)
- 所見13 の数を S14e-2 の表 (1a91f47 に変更を足した木) でなく S14d の値 (fc990f9 と 5bb2254) から取ったのは、どちらもコミットで測った値で、再現できるためである。`wc` の比は、大きさを決め直した後の表では Python が 20〜30 ms と粗く、比が 10.0 と 15.0 に振れた (S14d の記録)
- 所見15 は記録の値 (d3e5dde) が今も当てはまるかを、HEAD で走らせて確かめた (7章)
- 所見6 だけを測り直すのは、環境変数でコードを変えずに測れ、`-dirty` にならないからである。所見7、8、10 の比べは `#[inline]` や定数を変えた木でしか測れない
- 作業の順は、見出しを引く文はその見出しのあるコミットに入れ、見出しを消すコミットは引く側を同じコミットで直すという制約から決めた。所見の見出しを先に足し (A)、引く側を記録から所見へ移し (B1、B2、S2)、記録を消すのは引く側がなくなってから (C2) にする
- D を最初に置くのは、単位1 の後は `citations.rs` が docs/superpowers を走査するかもしれないためである [I]。superpowers の中の引用は、記録の見出しのほか、S2 と S4 が直す status.md や spec の見出しも引く。superpowers を先に消せば、S2、S4、A、B のどのコミットも superpowers の引用に巻き込まれない。D は tests/ui の3本の `.em` の引用を同じコミットで移すだけで、ほかに依存がない
- M と C1 を計画の最後に置くのは、表の「コミット」を S1 の「コードの最後のコミット」にするためである。S2、S4、B1、B3 はどれも `crates/`、`bench/` のコメントかコードを変える
- spec の21の文は S3 が文ごと直し、S4 は触れない。記録の見出しを消す前に引く側を直す必要があり、その仕事は S3 にある。S4 の r2 の項目はこの21の文と重ならない [I] (r1 で読み比べた。r2 は W-C1、W-C2、W-D8 のコードの作業を読んだ)
- コードの段の名前のうち、benchmarks.md を引く行と eml_rt の行は B1 に入れた。eml_rt の検査 (Verus、verusfmt) を1回で済ませるためである。残りの eml_core_ir と tests/ui は B3 にまとめた。`known_con.rs:4` は S2 が同じ行を直すので S2 に任せる

## 3. 検討して退けた案

- 所見3 を「欄の後始末」「総称な関数」「`release_children`」に分ける: 同じ原因を3か所で説明することになる
- 所見の数を HEAD で測り直す (`INLINE_LIMIT` の比べ、`#[inline]` の有無など): コードを変えた木で測ることになり、表と同じ再現の決まりを満たせない。S1 は所見の測り直しを、規則を変えた段に限っている
- vm.md:130、:250 の引用を外し、「計測で目立ったときに入れる」と書く (r1 の文案): spec に新しい条件を足すことになる
- `## 測り方` を作らず、2つの節を `##` のまま置く: S1 の6節と README の地図の「測り方」に合わない
- M を C1 の直前 (B の直後) に置く (r1): その後の S2 と S4 のコミットがコードを変え、表のコミットがコードの最後のコミットでなくなる
- D を B1 の後に置く (r1): 2章のとおり
- コードの段の名前を S1 の単位の担当に回す: 担当が決まらず、S1 の完了の条件を満たす作業がなくなる。B1 が同じファイルを開くので、S3 が持つほうが少ない手間で済む
- 記録の削除と本文の書き直しを1つのコミットにする: C2 は S2 を待つが、本文の書き直しは M を待つ。待つものが違うので分けた

## 4. 置いた仮定

- 単位1 は S1 の文案どおりに済んでいる。とくに `citations.rs` が `bench/` を走査し、docs/superpowers の除外を外している [I]。除外が残っていても、D を最初に置く順はそのままでよい
- 測る機械は Mac17,2 (Apple M5) である [I]。単位1 が写した rt.sh の表は S13e-4 の M4 の値なので、C1 の表とは機種が違う。C1 のコミットのメッセージに「rt.sh の表は機種が変わったので前の表と比べられない」と書く
- S2 は 1.3.1 の見出しを引く。S2 が見出しの名前を変えたいときは、A と同じコミットか、A の後の別のコミットで benchmarks.md の見出しと引く側を一緒に直す
- S4 の W-C2 は B1 の後に来る。B1 が直した `bench.rs` の `EP_FLOAT_CONVERSIONS` のコメントを、W-C2 が定数ごと消す
- `tests/ui` と `bench/` の `.em` のコメントを直しても、UI テストの出力と `RunStats` は変わらない [I]。`run` の UI テストは標準出力だけを比べ、`RunStats` は行番号を数えない

## 5. 実装への含意 (コミットの列)

共通の確かめ。`$S` はリポジトリの外の作業ディレクトリ (例: scratchpad の下) にする。リポジトリの中に置くと、M の前提の `git status --porcelain` が空にならない。

```sh
cargo test -p eml_cli --test integration citations::   # 通る
python3 -I $S/record_cites.py /Users/arakaki/Projects/eml 2>&1 >/dev/null   # 記録の見出しを引く数
git ls-files crates std tests bench | grep -v '^crates/eml_syntax/tests/corpus' | xargs grep -nE '\bS[0-9]+[a-z]?(-[0-9a-z]+)?\b' | wc -l   # コードとテストの段の名前の数
```

`record_cites.py` は7章の内容で置く (この回答の作業ディレクトリの `S3-scratch/record_cites.py` を写してよい)。数は、benchmarks.md の「## 記録」の下にだけある見出しを、ほかのファイルが引く回数である。0fff343a では 74 [V]。単位1 の後は 73 になる見込みである (`CLAUDE.md` の「記録」が消える) [I]。段の名前の数は 0fff343a で 96 [V]。

順序: 単位1 → D → A → B1、B2、B3 (互いに入れ替えてよい) → [S2、S4 のコードと文書の作業] → C2 → M → C1。S2 は A の後に始められる。S4 は B2 の後なら spec をいつ直してもよく、W-C2 は B1 の後に置く。M の後には `crates/`、`std/`、`bench/` を変えるコミットを入れない。

### D: docs/superpowers を消す (`Remove docs/superpowers`)

- 対象: `docs/superpowers/` の全部と、`tests/ui/run/effects/` の3本の `.em` の引用 (1.4)
- 確かめ方:
  - `ls docs/superpowers` が「No such file or directory」になる
  - `grep -rln 'docs/superpowers' tests` が3ファイルから0になる
  - `git ls-files tests/ui/run/effects | xargs grep -c 's13e'` の合計が 3 から 0 になる
  - `grep -rn 'docs/superpowers' crates std tests bench docs CLAUDE.md README.md` が、`docs/future/roadmap.md` の5行目 (S2 が消す)、benchmarks.md の記録の4行 (C2 が消す)、単位1 の決まりの文だけになる
  - `cargo test -p eml_cli --test integration citations::` と `ui::` が通る
- テストの変更: 機械的な追随 (コメントだけ)
- 依存: 単位1

### A: 所見の見出しを足す (`Add the benchmark findings`)

- 対象: `docs/implementation/benchmarks.md` の「## 所見」の下だけ
- 手順: 1.3 の20の所見を 1.1 の形で書く。「元の記録」と S2 の注は書かない。根拠は行番号で書かない。所見12 の status.md の見出しは、A を書く時点の status.md にある見出しで書く (S2 の前なら「深さと性能」。S2 が見出しを変えるコミットで、引く側として直す)
- 確かめ方:
  - `citations::` が通る
  - `awk '/^## 所見/{f=1;next} /^## /{f=0} f && /^### /' docs/implementation/benchmarks.md | wc -l` が 0 から 20 になる
  - 同じ範囲で `grep -cE '\bS[0-9]+[a-z]?(-[0-9a-z]+)?\b'` と `grep -c dirty` が 0 (所見に段の名前と `-dirty` を書かない)
  - 同じ範囲で `grep -cE '\.(rs|md):[0-9]+'` が 0 (行番号を書かない)
- テストの変更: なし
- 依存: 単位1

### B1: benchmarks を引くコードとテストと bench/ のコメント、eml_rt の段の名前 (`Repoint benchmark citations in code, tests and bench`)

- 対象: 1.5 の表の担当が B1 の行 (`bench/`、`crates/eml_vm/tests/bench.rs`、`crates/eml_rt`、`crates/eml_core_ir/src/optimize/fuse/mod.rs`、`tests/ui` の3本)。`bench/records.em` の古い reuse の記述も「構造体の値の更新なので確保しない」に直す
- 手順: 1.4 と 1.5 のとおり直す。`eml_rt` のコメントは `verus!` の中にあるものがあるので、`verusfmt` の形を崩さない。`SPECTRAL_NORM_S14E1_ALLOCATIONS` を改名する
- 確かめ方:
  - `git ls-files bench crates/eml_vm/tests/bench.rs crates/eml_rt tests/ui/run/functions/unspecialized_map_passes_floats.em tests/ui/run/records/swap_map_unspecialized.em tests/ui/run/records/three_rc_leaves_through_a_call.em | xargs grep -nE '\bS[0-9]+[a-z]?(-[0-9a-z]+)?\b' | wc -l` が 0 になる (0fff343a で、eml_rt の src の9行を足して 67 [I]。r1 の 58 は eml_rt の src を含まなかった)
  - `grep -c SPECTRAL_NORM_S14E1 crates/eml_vm/tests/bench.rs` が 2 から 0
  - `grep -n 'benchmarks.md の「S' crates/eml_rt/examples/rt_extern_bench.rs` が 1 から 0
  - `record_cites.py` の数が 38 減る (B2 の前なら 73 から 35、B2 の後なら 55 から 17) [I]
  - `cargo test` が通り、`cargo insta pending-snapshots` に何も出ない
  - `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`cargo verus verify -p eml_rt`、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` が通る (`crates/eml_rt/CLAUDE.md` の決まり)
- テストの変更: 機械的な追随 (コメントと定数の名前だけ。期待値は変わらない)。コミットのメッセージに書く
- 依存: A

### B2: spec の21の文を付け替える (`Repoint spec citations of the benchmark records`)

- 対象: `docs/spec/object-model.md`、`docs/spec/vm.md`、`docs/spec/core-ir.md` の 1.4 の行
- 手順: 1.4 の表と文案 A〜C のとおり直す (Q1 の答えで vm.md:130、:250、:411 の形が変わる)
- 確かめ方:
  - `record_cites.py` の数が 18 減る (B1 の前なら 73 から 55、B1 の後なら 35 から 17) [I]
  - `grep -nE 'benchmarks\.md\) の「' docs/spec/*.md | grep -cE '\bS[0-9]'` が 0 になる
  - `grep -c '基準の計測' docs/spec/core-ir.md` が 2 から 0 になる
  - `citations::` が通る
- テストの変更: なし
- 依存: A。S4 の spec の作業はこの21の文を除いて進める

### B3: コードとテストの残りの段の名前 (`Remove stage names from code comments`)

- 対象: 1.5 の表の担当が B3 の行 (`crates/eml_core_ir` の src と tests、`tests/ui` の7本)
- 手順: 1.5 の直し方の決まりのとおり直す。`specialize.rs` の `unreachable!` の文言も直す
- 確かめ方:
  - `git ls-files crates/eml_core_ir tests | grep -v corpus | xargs grep -nE '\bS[0-9]+[a-z]?(-[0-9a-z]+)?\b' | grep -v 'known_con.rs:4:' | wc -l` が 0 になる (0fff343a で 24 [I]。`known_con.rs:4` は S2 が直す)
  - `cargo test -p eml_core_ir` と `cargo test -p eml_cli --test integration ui::` が通る。`cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` が通る
- テストの変更: 機械的な追随 (コメントと到達しない分岐の文言)
- 依存: なし (A の前でもよい)

B1、B3、S2 の W3 の後で、共通の確かめの段の名前の数が 96 から 0 になる。

### C2: 記録を消す (`Remove the benchmark records`)

- 対象: `docs/implementation/benchmarks.md` の「## 記録」から終わりまで
- 前提: S2 が status.md と roadmap.md の記録への引用を付け替え終えている。`record_cites.py` の数が 0
- 手順: 消す前に、上の「103に入らない当たり」のとおり grep の当たりを表と突き合わせる: `grep -rln 'benchmarks\.md\|計測の基準\|基準の計測' crates docs tests std bench CLAUDE.md README.md | grep -v '^docs/implementation/benchmarks.md$' | sort` のファイルの一覧が、1.4 の表と「103に入らない当たり」のファイルの一覧と同じであることを見る (表にないファイルが出たら、そこを付け替えてから消す)
- 確かめ方:
  - `grep -c '^### S' docs/implementation/benchmarks.md` が 40 から 0 になる [V] (0fff343a で 40。「S13 の前の試作」を含む)
  - `grep -c '^## 記録' docs/implementation/benchmarks.md` が 1 から 0 になる
  - `wc -l docs/implementation/benchmarks.md` が約2,700から数百になる
  - `citations::` が通る
- テストの変更: なし
- 依存: A、B1、B2、S2。S2 が遅れるときの代わり: C2 の中で status.md と roadmap.md の17の引用を 1.4 の表の見出しに機械的に付け替える (文はそのまま)。その場合は S2 にそう伝える

### M: 測り直す (コミットなし。計画の全体の最後)

- 前提: S2、S4、B1、B3 の、`crates/`、`std/`、`bench/` を変えるコミットがすべて main にある。`git status --porcelain` が空
- 手順:
  1. `sysctl -n hw.model` を見る。Mac17,2 でなければ、C1 のメッセージに機種が変わったことを書く
  2. `bench/scripts.sh > $S/scripts.md`。両方の表の eml の実時間が 0.15〜0.60 秒の中にあるかを見る。fannkuch-redux は範囲を外れても、300 ms との比が最も小さい n のままにする (1.1 の「大きさ」の1文)。ほかに外れた行があれば、外れた行をまとめて1つのコミット (`Resize bench rows`) で直し、`bench/scripts.sh` を1回だけ流し直す。まだ外れた行が残っても、そのまま進み、行と値を C1 のメッセージに書く (流し直しを繰り返さない)。変えた行と前後の値も C1 のメッセージに書く
  3. `bench/run.sh > $S/run1.md` と `bench/run.sh > $S/run2.md`。2回の実行の分の差の最大と、その行の名前と実行の分を求める:
     ```sh
     awk -F'|' 'FNR<=2{next} NR==FNR{a[$2]=$4+0; next} ($2 in a) && $4+0>0 {d=($4-a[$2])/a[$2]*100; if(d<0)d=-d; if(d>m){m=d;n=$2;v=a[$2]}} END{printf "%s 実行の分 %d の差 %.2f%%\n", n, v, m}' $S/run1.md $S/run2.md
     ```
     表には `run1.md` を使う
  4. `bench/rt.sh > $S/rt.md`
  5. 3つの出力の「コミット」の行が同じで、`-dirty` がなく、`git rev-parse --short=7 HEAD` と同じことを見る
  6. 大きさを読む分: `$S/args_only.em` を7章の内容で置き、2つの中央値の差をとる
     ```sh
     for f in bench/empty.em $S/args_only.em; do for i in 1 2 3; do /usr/bin/time -l target/release/eml run $f > /dev/null; done 2>&1 | awk '/instructions retired/{print $1}' | sort -n | sed -n 2p; done
     ```
  7. `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 bench/run.sh > $S/cgu1.md` を最後に流し、手順3の awk で `run1.md` と比べる代わりに、プログラムごとに `(cgu1 - run1) / run1` の符号と大きさを読む。減る行と増える行が両方あれば、所見6 の主張のとおりで、C1 で値を置き換える。どの行も同じ向きなら C1 に進まず、ユーザーに聞く。流した後は `cargo build --release -p eml_cli` で既定のビルドに戻す
- 期待する結果: 3つの出力に `-dirty` がない。scripts の各行が範囲の中にあるか、範囲を外れた理由が書ける

r1 の手順3の awk は欄を取り違えていた。上の awk は、2つの表を行の名前で突き合わせる。手元で2つの小さな表を作って流すと、`fib 実行の分 1000 の差 1.00%` を出した [V]。

### C1: 今の数値と本文を書き直す (`Measure and describe the benchmarks`)

- 対象: `docs/implementation/benchmarks.md`
- 手順:
  1. 1.1 の章立てにする (`## 測り方` を足し、2つの節を `###` と `####` に下げ、「大きさ」に1文を足す)
  2. 「今の数値」の3つの表を、M の出力の環境の行と表で置き換える。scripts の表の名前は「スクリプト」と「数値計算」
  3. 1.2 の19件を直す (#8 は M の手順6、#10 は M の手順3の値)
  4. 所見6 の数値を M の手順7の値にし、機種とコミットを M の環境にする
  5. 本文から、段の名前、記録への「下の「…」」、日付を消す
- 確かめ方:
  - `git diff --stat <表のコミット> HEAD -- crates std bench` が空 (表のコミットは M の出力の「コミット」の行の値)
  - `grep -c dirty docs/implementation/benchmarks.md` が 1 以上から 0 になる (単位1 が写した表は `1a91f47-dirty` を持つ)
  - `grep -cE '\bS[0-9]+[a-z]?(-[0-9a-z]+)?\b' docs/implementation/benchmarks.md` が 0 になる
  - 「基準のプログラム」の表の行の数が 29 から 36 になり、`ls bench/*.em | wc -l` と同じ: `awk '/^## 基準のプログラム/{f=1;next} /^## /{f=0} f && /^\| `/' docs/implementation/benchmarks.md | wc -l`
  - 同じファイルの中の「上の「…」」「下の「…」」がどれも見出しを指す: `python3 -I -c "import re;t=open('docs/implementation/benchmarks.md').read();h={re.sub(r'\s','',l.lstrip('#')) for l in t.splitlines() if l.startswith('#')};print([m for m in re.findall(r'[上下]の「([^」]*)」',t) if re.sub(r'\s','',m) not in h])"` が `[]`
  - `citations::` と `cargo test -p eml_vm --test integration bench::every_program_in_bench_has_a_test` が通る
- コミットのメッセージ: 表の機種とコミット、rt.sh の表は機種が変わったので前の表と比べられないこと、scripts の大きさを変えたならその行と前後の値、所見6 を測り直したこと
- テストの変更: なし
- 依存: M、C2

## 6. 未決の事項

- Q1 (ユーザーに聞く) vm.md の CEK との比べの扱い。選べる形は3つある
  - (a) 所見9 を残し、vm.md:130、:250 は「S10a で比べた」だけを消して数と引用を残す。vm.md:411 は消す (今のどの値も前のインタプリタの数え方でないので、説明する対象がない)
  - (b) (a) と同じだが、spec の数を消し「前のインタプリタより十分に少なかったので」とし、数は所見9 だけが持つ
  - (c) 所見9 を作らず、vm.md:130、:250 は「簡潔さを先にした」で止め、引用を外す。vm.md:411 は「前のインタプリタの数え方の値」と書いて引用だけを外す
  - 既定: (b)。S1 の残す基準 (規則が今も引く) と「数値は benchmarks.md だけが持つ」の両方に合う。文案 B と C は (a) の形なので、(b) なら括弧の前の「約 0.10〜0.17 倍に収まった」を「十分に少なかった」に替える
- Q2 `SPECTRAL_NORM_S14E1_ALLOCATIONS` の新しい名前。既定: `SPECTRAL_NORM_BOXED_TUPLE_ALLOCATIONS` (組を構造体にしない場合の確保の数)。`SWAP_MAP_BEFORE_STRUCTS` は段の名前を持たないので変えない
- Q3 所見6 の測り直しで主張が成り立たなかったとき (どの行も同じ向き) の扱い。既定: C1 に進まずユーザーに聞く。vm.md の規則を変えるかは、この作業の範囲の外
- Q4 「`var` の添字の費用」(fannkuch とふるいが目標に届かない理由) を所見にするか。既定: しない。今も添字1つに3つの extern を呼ぶかを確かめていない。S2 が要るなら、最適化した Core IR (`Pass::Optimize`) で `fannkuch_var.em` の添字を数えて確かめてから足す

## 7. 追加の確認

- [V] `Yield` を変える handler の yield を HEAD で確かめた: `git archive HEAD` をこの回答の作業ディレクトリ (`S3-scratch/repo`) に写し、`crates/eml_vm/tests/` に一時のテストを足して `eml_test_support::run_stats` で流した。`pure_callback_under_state.em` の `map_lists` の形で、`State.run` なしでもありでも `yields: 1000`、`evidence_steps: 4002`、出力 `3000`。リポジトリは書き換えていない
- [V] 大きさを読むだけのプログラム `args_only.em` (下) は、同じ写しの debug ビルドで、引数なしと引数 `5` のどちらでも何も書かずに終了コード0で終わった。r1 の版は結果を `println` で書いたので、`empty` との差に `show` と `println` の分が入っていた。この版は結果を書かない

```
-- 大きさを引数で受ける基準のプログラムの、大きさを読む部分だけ。結果は書かない
main : Unit -> <IO, Panic> Unit
main () =
  let n = size 1000
  if n > 0 then () else ()

size : Int -> <IO, Panic> Int
size default =
  match Env.args () with
    | Nil -> default
    | [arg] ->
      match String.to_int arg with
        | Some n -> if n >= 1 then n else usage ()
        | None -> usage ()
    | _ -> usage ()

usage : Unit -> <Panic> Int
usage () = panic "usage: args_only [SIZE]"
```

- [V] `bench.rs` のスナップショットの今の値: `spectral_norm` は `allocations` 4043、`boxes` 4001。`nbody` は 25051、`peak_stack` 76。`list` は `peak_stack` 700015。`upsert` は 352。`tree_map` は 126。`mandelbrot` は `allocations` 2、`reuses` 0。`records` は `reuses` 0。`trade_offs` は 22/20、52/40、32/50、3 (Python でスナップショットを読んだ)
- [V] `bench.rs` の `#[test]` は51本 (`grep -c`)
- [V] コードとテストの段の名前: `git ls-files crates std tests bench | grep -v '^crates/eml_syntax/tests/corpus' | xargs grep -nE '\bS[0-9]+…\b'` で40ファイル96行。識別子の中の段の名前は `grep -noE '[A-Za-z_]*(S1[0-9][A-Ea-e][0-9]?|s1[0-9][a-e][0-9]?)[A-Za-z_0-9]*'` で探し、`SPECTRAL_NORM_S14E1_ALLOCATIONS` と、tests/ui の3本の superpowers のパスの `s13e` だけだった。ファイル名とディレクトリ名は `corpus/s1.em` だけ (`git ls-files | grep -iE …`)
- [V] `record_cites.py` の数は 0fff343a で 74 (`CLAUDE.md` 1、`bench/` 7、`crates/eml_rt` 10、`crates/eml_core_ir` 1、`bench.rs` 17、`roadmap.md` 6、`status.md` 11、spec 18、`tests/ui` 3)。「スクリプトの計測」は S14e-2 の記録の中にも同じ名前の `####` があるので、記録より前にもある見出しは数えないようにした。中身は次のとおりである

```python
# benchmarks.md の「## 記録」の下の見出しを引く箇所を数える。citations.rs と同じく、行頭のコメント記号を除き空白を詰めて読む
import os, re, sys
root = sys.argv[1]
bench = open(os.path.join(root, 'docs/implementation/benchmarks.md'), encoding='utf-8').read()
records = set()
others = set()
inside = False
for line in bench.splitlines():
    if line.startswith('## '):
        inside = line.strip() == '## 記録'
    if line.startswith('#'):
        (records if inside else others).add(re.sub(r'\s', '', line.lstrip('#')))
records -= others
hits = []
for d, ds, fs in os.walk(root):
    rel_d = os.path.relpath(d, root)
    ds[:] = [x for x in ds if x not in ('target', '.git', '.yomiyasu') and not os.path.join(rel_d, x).startswith(('docs/superpowers', 'docs/plans', 'docs/reports'))]
    for f in fs:
        rel = os.path.normpath(os.path.join(rel_d, f))
        if rel == 'docs/implementation/benchmarks.md' or not f.endswith(('.rs', '.em', '.md', '.py', '.sh')):
            continue
        text = open(os.path.join(root, rel), encoding='utf-8').read()
        flat = ''.join(re.sub(r'^\s*(//[/!]?|--|#)', '', l) for l in text.splitlines())
        flat = re.sub(r'\s', '', flat)
        for m in re.finditer(r'(?:benchmarks\.md\)?|benchmarks\.md`?)(?:の「([^」]*)」)((?:と「[^」]*」)*)', flat):
            heads = [m.group(1)] + re.findall(r'と「([^」]*)」', m.group(2) or '')
            for h in heads:
                if h in records:
                    hits.append((rel, h))
for rel, h in sorted(hits):
    print(f'{rel}\t{h}')
print(len(hits), file=sys.stderr)
```

- [V] 103か所の内訳は g2 の `g2_cites.py` を流し直して確かめた (`S3-scratch/cites.tsv`)。status.md:96 は2つの見出しを引くが1か所と数えている
- [V] vm.md の「スロットの割り当て」、「`RunStats`」、lir.md の「関数の ABI」の見出しがある (`grep -n '^#'`)
- [V] 0fff343a のベンチマークの数 (命令の数と実時間) は測っていない。M で測る
- 単位1 が写した「今の数値」の rt.sh の表は S13e-4 の「付ける前と付けた後」の表で、M4 の値である (g2 の4.2)。C1 で M5 の値に置き換わる

---


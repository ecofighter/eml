# raw-arrays の段の設計と計画

- 作成: 2026-11-05
- 基点: `main` @ `986b65a0` (「背景」と、ファイルと行の参照はこの状態を指す)
- 状態: 計画は確定。残る論点は 10 に既定つきで置いた
- 付録: `docs/plans/2026-11-05-raw-arrays/c1c2-probe.diff`。T-2 と T-3 の出発点にする試作の差分で、基点に `git apply` できる

## 1. 目的と完了の条件

- 目的: `Float` と `Int` の要素を生の 8 バイトの語で並べる配列 `UnboxedArray a` と `MutUnboxedArray a` を、新しい物体の種類 `Raw` で入れる。あわせて、`Array` と `MutArray` の物体が長さによらず要素の数を数の語に持つようにする。spectral-norm の要素ごとの `Float` の箱 (`RunStats` の `boxes` 4001) を消し、`ffi-c`、`native-inline`、`std-par` の前提をそろえる。
- 完了の条件:
  - `bench/spectral_norm.em` が `UnboxedArray Float` で書かれ、`boxes` が 0 か 1、`allocations` が 50 以下で、出力が `1.274219991` である (T-7)
  - 9 の「全体の確かめ」のコマンドがすべて通る
  - 5.1 の適合の確かめ (G-*) がすべて通る

## 2. 範囲

- 範囲に入るもの:
  - `eml_rt`: 配列の数の語、`Kind::Raw`、`unboxed_*` の操作 (Verus の証明つき)
  - `eml_extern` の行、`eml_vm` の分岐と専用の命令3つ、std の2つのモジュール
  - UI テスト、REPL のトランスクリプト、bench の書き換え
  - `docs/spec/` の各節と `docs/future/` の名前の更新
- 範囲の外 (この段では作らない):
  - ネイティブのコードでの展開 (`native-inline`)
  - 添字の範囲のプリミティブと `slice`/`append` (`std-par`)
  - `extern "C"` (`ffi-c`)
  - 新しい型への `e[i]`、`x[i] <- v`、`var` の配列の変数
  - `Byte` や 32 ビットの要素
  - `closed` のクラス

  どれも T-8 でロードマップの「その後の項目」に移す。

## 3. 背景

- 物体のヘッダは `rc: i32` と `meta: u32 = tag(16) | scan(8) | info(8)` である。`info` の下位4ビットが種類で、最上位ビットが `lin` ビットになる。`scan` が 255 なら、ペイロードの語 0 が数で、走査する語はその後に続く (docs/spec/object-model.md:53-59, crates/eml_rt/src/meta.rs:132-138, crates/eml_rt/src/heap/inv.rs:95-110)
- 今の種類は `Data` 0、`BigInt` 1、`Foreign` 2、`String` 3、`Float` 4、`Bytes` 5、`Free` 15 である (docs/spec/object-model.md の「種類」)
- `Array`/`MutArray` は tag 0 の `Data` である。要素が 255 未満なら `scan = n` で、要素は語 0 から並ぶ。255 以上なら `scan = 255` で、語 0 が数、要素は語 1 からになる。`MutArray` は `lin` ビットを立てた `Array` で、`freeze`/`thaw` は `relin` を通る (object-model.md の「配列」、crates/eml_rt/src/heap/ctx/arrays.rs:2331)
- arrays.rs で 255 の規則に関わる箇所は次の 20 である
  - `n/k >= 255` の 11 か所: 273, 384, 456, 488, 538, 611, 720, 760, 1049, 1795, 2410
  - `scan_of(..) == 255` の 7 か所: 335, 1269, 1298, 1328, 1390, 1447, 1721
  - `alloc_data(0, ..)` の 4 か所: 1063, 1810, 2418, 2594
- VM の凍結した領域の空の `MutArray` (crates/eml_vm/src/compile/constants.rs:128-136) は `alloc_immortal(0, Meta::new(0, 0, Kind::Data, true))` で作っている (docs/spec/vm.md:232)
- 要素の位置は一様な位置 (`tobj`) なので、`Array Float` の要素は書くたびに `Float` の箱になる (docs/spec/lir.md:100-102)。spectral-norm の `RunStats` は `allocations` 4043、`boxes` 4001、`unboxes` 400400 である (crates/eml_vm/tests/bench.rs:1265)
- extern の行には型引数ごとの欄がない。型ごとに振る舞いを変える手段は、メソッドを extern の行に結んだクラスの instance だけである (例: `Prelude.Num Int.+` と `Prelude.Num Float.+`)。instance を extern に結べるのは std だけである (E1033)。クラスは単相化で解決し、実行時の辞書はない (docs/spec/declarations.md の「extern」、docs/spec/core-ir.md の「メソッドの解決」)
- クラスの制約 `C a` は `a ≤ Unr` を含むので、`Lin` の型は instance を持てない (E2010)。ただし、頭が `Float`/`Int` (`Unr`) のクラスのメソッドは、`Lin` の配列を受けて返してよい。これは試作で確かめた
- `Bytes` は `Unr` で、一意ならその場で更新する伸びるバッファである。対象は最後の引数に置く。`String` と同じ配置を持つ。名前は約 2600 行に現れ、そのうち 1317 行は生成される `crates/eml_rt/src/heap/tests/expected.txt` にある
- `eml_rt` の変更は crates/eml_rt/CLAUDE.md の関門を通す。基点で `cargo verus verify -p eml_rt` は `1152 verified, 0 errors`、`cargo nextest run -p eml_rt` は 814 passed、ワークスペース全体の `cargo nextest run` は 4463 passed / 21 skipped である

## 4. 前提

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| devShell (`nix develop` か direnv) | すべて | `cargo --version` → `1.99.1` | 止めて利用者に聞く |
| Verus と verusfmt | T-2〜T-4、T-8 | `cargo verus verify -p eml_rt` → `1152 verified, 0 errors` | 止めて利用者に聞く |
| macOS (計測) | T-8 の計測 | `bench/run.sh` が動く | 表の入れ替えだけを直後の別のコミット `Measure raw-arrays` にする (ロードマップの「進め方」) |
| Linux と `.#miri` の shell | T-4 の Miri | `nix develop .#miri -c cargo miri --version` | CI の Miri の job に任せ、そのことをコミットのメッセージに書く |
| `python3` | T-7 の期待値 | `python3 bench/numeric/spectral_norm.py 20` が値を出す | 止めて利用者に聞く |

## 5. 決めたこと

### 5.1 長い期間の方針 (変えるには見直しが要る)

実装中にこれらとぶつかったら、止めて報告する。

#### D-1: 生の要素の配列は多相な2つの型とする

- 決めたこと:
  - Prelude に `pub extern data UnboxedArray a` と `pub extern data MutUnboxedArray a` を置く。前者の Kind は要素に従い、後者はいつも `Lin` である
  - モジュールは `Std.UnboxedArray` と `Std.MutUnboxedArray` で、それぞれ `Std.Array` と `Std.MutArray` と同じ形にする
  - 値の位置に `a` が現れる関数は、クラスのメソッドにする。`Std.MutUnboxedArray` の `pub class MutUnboxed a` は `make`、`read`、`write`、`fill` を持つ。`Std.UnboxedArray` の `pub class MutUnboxed a => Unboxed a` は `make`、`get`、`set` を持つ
  - instance は `Float` と `Int` の2つだけで、メソッドはすべて `extern` で行に結ぶ。`a` が幽霊の型にすぎない関数 (`length`、`empty`、`freeze`、`thaw`) は、ふつうの多相な extern にする
  - `Std.FloatArray`、`MutFloatArray`、`IntArray`、`MutIntArray` の名前は使わない
- 理由:
  - 型ごとの行は、既存の instance の仕組み (`Num Int.+` と同じ形) でそのまま作れる。Float の行の Repr は `float`、Int の行の Repr は `int` になり、VM ではどちらも 64 ビットの生の語なので、実行時の操作は1つで足りる
  - 単相な4つの型にすると、eml の関数と `std-par` の関数がすべて2倍になる。Float と Int を通した汎用のコードも書けなくなり、要素の型が増えるたびに型が4つずつ増える
  - `Std.MutArray` に extern に結んだ instance を足す試作を作った。`borrowed` と `with_arg` を付けた行が、コンパイラを変えずに動いた。superclass を別のモジュールから import する形と、2つのクラスがどちらも `make` を持つ形も、どちらも解決した
  - 型を2つに分けるのは、各モジュールが型ごとのメソッドを要るからである。`get` を `thaw` から作ると共有した配列を写すことになり、`write` を `freeze`/`set`/`thaw` で作ると、いちばん多く通る道で extern を3回呼ぶことになる
  - クラスを Prelude に置くと、`get`/`set`/`make`/`write` が修飾なしで全プログラムに見えるので、各モジュールに置く
- 退けた案:
  - 単相な4つの型: 上の理由
  - Prelude の1つのクラスに接頭辞を付けたメソッドを集める案: 操作ごとに公開の名前が2つでき、Prelude の名前が増える
  - 新しい Repr「生の語」を作って多相な行を1つにする案: クラスの仕組みで足りることに、コンパイラの新しい仕組みが要る
  - 名前を `RawArray` にする案: 「raw」は安全でない操作を連想させる
- 影響の範囲: std、`eml_extern`、`eml_vm`、`docs/spec/stdlib.md`、`docs/future/` の ffi.md、multicore.md、native.md、roadmap.md
- 見直す条件: 8 バイト未満の要素の型を足すとき (D-3)
- 適合の確かめ:
  - G-1 (MUST) 新しい型は `UnboxedArray a` と `MutUnboxedArray a` の2つだけで、どちらも Prelude の `pub extern data` である。(MUST NOT) `FloatArray`、`IntArray`、`MutFloatArray`、`MutIntArray` をどこにも作らない。確かめ: `git grep -n "FloatArray\|IntArray" -- docs std crates tests bench` → 空 (T-8 の後。T-5 の後は roadmap.md の段の項目だけが残る)。`git grep -n "pub extern data UnboxedArray a\|pub extern data MutUnboxedArray a" std/Prelude.em` → 2 行
  - G-2 (MUST) 値の位置に `a` を持つ関数は `MutUnboxed` (`make`、`read`、`write`、`fill`) か `Unboxed` (`make`、`get`、`set`) のメソッドで、instance は `Float` と `Int` だけ、メソッドはすべて `extern` である。`Unboxed` の superclass は `MutUnboxed` である。確かめ:
    - `git grep -n "^pub class" std/UnboxedArray.em std/MutUnboxedArray.em` → `pub class MutUnboxed a => Unboxed a` と `pub class MutUnboxed a`
    - `git grep -c "^instance" std/UnboxedArray.em std/MutUnboxedArray.em` → 2 と 2
    - `git grep -c '"Std.UnboxedArray.Unboxed \|"Std.MutUnboxedArray.MutUnboxed ' crates/eml_extern/src/lib.rs` → 14
  - G-3 (MUST) 写した関数の名前、引数の順 (配列が先)、借用、`with_arg` を `Std.Array`/`Std.MutArray` と同じにする。足す関数は `of_array`、`to_array`、`fill` だけで、`slice`/`append` は足さない。確かめ: stdlib.md の2つの表が D-12 と一致する。`tests/ui/run/unboxed/basics.em` (`tests/ui/run/arrays/basics.em` の写し) が通る
  - G-5 (MUST NOT) 新しい型に `Eq`/`Ord`/`Show` の instance を作らない。`e[i]` と `var` の配列の変数を広げない。`closed` のクラスの仕組みを作らない。(MUST) stdlib.md に「instance は `Float` と `Int` だけで、ほかの型の instance は意味を持たない」と書き、`closed` のクラスをロードマップの項目にする。確かめ: `git grep -n "instance .*UnboxedArray" std` → 空。check-fail の `unboxed_index.em` が E2001 を出す。`git grep -n "closed" docs/future/roadmap.md` が項目を示す
- リポジトリに残す所: `docs/spec/stdlib.md` の「`Std.UnboxedArray`」「`Std.MutUnboxedArray`」(T-5)、`docs/spec/types.md` の「Kind」(T-5)

#### D-2: `Bytes` は名前を変えず、族にも入れない

- 決めたこと: `Bytes` の名前を `ByteArray` などに変えない。`UnboxedArray Byte` にもせず、`MutBytes` も作らない。この段では `Unboxed Byte`/`MutUnboxed Byte` の instance を作らない。実行時でも、`Bytes` の種類 5 を `Raw` にまとめない
- 理由:
  - `Bytes` は、容量が長さを超えて伸びるバッファ (`push`、`append`、`with_capacity`) である。対象を最後に置く順で、`String` と配置を共有し、その場で変換する。I/O 全体のバッファでもある (std の 14 ファイル、UI テストの 63 ファイル)
  - `ByteArray` と呼ぶと、引数の順も可変性の扱いも API も違う族の一員に見える。型の族が多相になったので、名前をそろえる相手の `FloatArray` もない。それでいて名前を変える作業は、生成でない行だけで約 1300 行になる
  - 実行時でまとめると、要素を T10 (信頼したバイトの読み書き) に通すことになるか、`Bytes` の約 4600 行を語の単位に書き直すことになる。要素の幅を変数にした `body_wf` は、object-model.md が `String`/`Bytes` について退けた「種類で条件付きの不変条件」になる
- 退けた案:
  - `ByteArray` への改名: 上の理由
  - `Bytes` を今族に入れる案: API の形が合わない。1 バイトの幅には、実行時が幅を知る仕組みが要る (D-3)
  - `MutBytes`: `Bytes` はすでに一意ならその場で更新するので、`Lin` のバイトのバッファを求める所がない
- 将来の費用: C に `uint8_t*` を書かせる API が要るときは、`instance MutUnboxed Byte` を足して `MutUnboxedArray Byte` を使う。そのときバイトの列の型が `Bytes` と `UnboxedArray Byte` の2つになり、変換の関数2つでつなぐ
- 見直す条件: C の API が可変のバイトのバッファを求めたとき (`ffi-c` の段)
- 適合の確かめ: G-4 (MUST NOT) `Bytes` の名前を変えない、`MutBytes` を作らない、`Unboxed Byte`/`MutUnboxed Byte` を作らない。確かめ: `git grep -nw ByteArray -- docs std crates tests` → 空。`git grep -n "instance Unboxed Byte\|instance MutUnboxed Byte" std` → 空。`git grep -c "pub extern data Bytes" std/Prelude.em` → 1
- リポジトリに残す所: roadmap.md の「その後の項目」の要素の幅の項目 (T-8)

#### D-3: 要素の幅はこの段では 8 バイトに決め、`tag` を幅のために空けておく

- 決めたこと: `Raw` の物体の要素はいつも 8 バイトとする。`tag` は 0 でなければならないとし、後で幅の符号に使えるよう空けておく。幅を知る必要のある操作 (`length`、`set` と `thaw` の写す道、回復) は、今は幅を 8 と決めて書く
- 理由: 狭い幅の要素を後で足すとき、表面の型を変えずに済む。そのとき実行時の物体の `tag` に幅を書けば (道 R1)、ふつうの行はふつうの行のままでよい
- 見直す条件: `Byte` か 32 ビットの instance を足すとき。そのとき道 R1 (物体の `tag` に幅を書く) か道 R2 (幅に依る関数をクラスに移す) を選ぶ
- リポジトリに残す所: roadmap.md の「その後の項目」の FFI の要素の幅の項目 (T-8)、object-model.md の `Raw` の行 (T-3)

#### D-4: 配列はいつも数の語を持ち、`Raw` の物体は走査しない語だけを持つ

- 決めたこと:
  - `Array`/`MutArray` の物体はどれも `scan = 255` で、ペイロードの語 0 に要素の数を持つ。長さ 0 の配列と、VM の凍結した領域の空の `MutArray` も同じである
  - 配列でない `Data` は `n >= 255` の規則のままにする
  - `Raw` は種類 6 で、tag 0、`scan = 0`。語 0 が要素の数 n で、語 1..=n が要素である。容量までのすべての語を初期化する (`raw_wf`: `n + 1 <= cap`)
  - `Raw` は `raw`/`set_raw` から不透明で、token にならず、破棄処理を持たない。`Building` にならず、`alloc`/`alloc_immortal`/`con_at` の `meta` にもならない
  - `set_lin` は `Data` と `Raw` を受ける
  - `share`、`transfer_back`、解放の鎖、`foreign_children`、`push_children` には、`Raw` のための分岐を入れない
  - 信頼の一覧に行を足さない
- 理由:
  - ネイティブのコードは数を `base + 8` で、i 番目の要素を `base + 8 + 8*i` で読む (docs/future/native.md:121)
  - 走査を決めるのは `scan` だけなので、`scan = 0` にすれば子がなく、`share` は O(1) になり、解放にもコードが要らない
  - 要素を語のスロットの `PointsTo` で扱えば、すべて検証できる (object-model.md:525)
  - 配列でない `Data` に数の語を足すと、VM の `con`、LIR の `Shape`、reuse が変わるうえ、すべてのコンストラクタとクロージャが 8 バイト増える。それを要る所はない
- 退けた案:
  - 配列を今の形のままにして、ネイティブのコードを `scan` で分岐させる案: native.md:121 と合わない
  - `Raw` を `has_body` の仕組み (ghost のバイト列) で表す案: 要素が T10 を通ることになる
  - Float と Int の区別を tag に書く案: VM のスロットには型がないので、確かめる者がいない
- 見直す条件: 狭い幅の要素を足すとき (D-3)
- 適合の確かめ: 下の G-R1〜G-R15 (出典は試作の設計。確かめのコマンドはこの計画で直した)
  - G-R1 (MUST) `Array`/`MutArray` の物体はどれも `scan == 255` で、語 0 が数である。確かめ: 単体テスト `arrays_always_carry_the_count_word`。`checked_array` が `scan == 0` の tag 0 の `Data` を断る checked.rs のテスト。Verus の `array_ok ==> scan_of == 255`
  - G-R2 (MUST) 配列でない `Data` は `n >= 255` の規則のままで、`alloc_data` の引数と ensures を変えない。確かめ: T-2 の差分で eml_rt の外に変わるのは `crates/eml_vm/src/compile/constants.rs` だけである。ワークスペース全体のテストが通る
  - G-R3 (MUST) 回復は `Data` の物体を元と同じ形で写す。確かめ: `grep -n "f.first == 1" crates/eml_rt/src/heap/recover.rs` が空でなく、`grep -n "f.count >= 255" crates/eml_rt/src/heap/recover.rs` が空。recover.rs の単体テスト
  - G-R4 (MUST) `Raw` の物体は `scan == 0`、語 0 が n、`n + 1 <= cap`、容量まで初期化 (`raw_wf`)、この段では `tag == 0` である。確かめ: Verus。`meta(w).scan() == 0 && tag() == 0` を読む単体テスト
  - G-R5 (MUST) `Raw` は `raw`/`set_raw` から不透明 (`opaque_spec`) で、`Building` にならず、`alloc`/`alloc_immortal`/`con_at` の `meta` にならず (`self_made`)、token にならない。確かめ: checked.rs のテスト (`raw`/`set_raw` → `WrongKind`、`into_token` → `NotTokenable`)
  - G-R6 (MUST) `Raw` の物体は `Heap::alloc_unboxed` だけが作り、`unboxed_put`/`unboxed_copy` だけが書く。eml_vm は `Raw` の `meta` を作らない。確かめ: `git grep -n "Kind::Raw" crates/eml_vm/src` → 空
  - G-R7 (MUST) `set_lin` が受けるのは `Data` と `Raw` だけである (検査モードでは、ほかの種類に `WrongKind`、rc != 1 に `NotUnique`)。確かめ: mod.rs の `matches!(m.kind(), Kind::Data | Kind::Raw)` と単体テスト
  - G-R8 (MUST) 生の配列のコードは信頼の一覧に行を足さない。`heap/unboxed.rs` と `heap/ctx/unboxed.rs` はすべて `verus!` の中にあり、`text.rs` を通らない。`unboxed_copy` は `dst != src` を requires に持つ。確かめ: `cargo nextest run -p eml_rt trust::` が通る。`grep -n "external_body\|assume" crates/eml_rt/src/heap/unboxed.rs crates/eml_rt/src/heap/ctx/unboxed.rs` → 空。`grep -n "dst.bits_spec() != src.bits_spec()" crates/eml_rt/src/heap/unboxed.rs` → 空でない
  - G-R9 (MUST NOT) share.rs、transfer.rs、release.rs、graph.rs、remote.rs に `Raw` のための分岐を入れない。許すのは、証明の揺れが出たときの `#[verifier::rlimit(N)]` の属性とそのコメントだけである。確かめ: T-3 の `git diff --stat` にこれらのファイルがない (あっても rlimit の行だけ)
  - G-R10 (MUST) 検査モードでは、`Raw` の物体が配列、`String`、`Bytes`、`unbox`、`foreign` の操作に来たとき、また `Raw` でない物体が `unboxed_*` に来たときに `Heap(WrongKind)` を返す。速いモードでは requires だけにする。`Data` の側の確かめは `kind == Data && tag == 0 && scan == 255` で、255 以上の配列について今より強くはならない。確かめ: 組み合わせごとの checked.rs のテスト
  - G-R11 (MUST) 長さと予算の確かめは確保の前に両方のモードで行う。予算は `array_make` と同じ `(n + 4) * 8` にする。誤りのときは消費した引数を手放し、`Fault::Index` か `Stopped(MemoryLimit)` を返す。借りた引数には触れない。`IndexFault` に新しい variant を足さない。確かめ: expected.txt の境界の行。誤りの後に `mortal == 0` を確かめる単体テスト。`git diff 986b65a0 -- crates/eml_rt/src/externs.rs` → 空
  - G-R12 (MUST) 種類ごとの決めは `Kind` の `match` と spec fn に置く。object-model.md の3つの表 (「種類」、回復、「核の操作」) は、コードと同じコミットで変える。確かめ: `name()` に `Raw` の腕がある。T-3 の object-model.md の差分が3つの表に及ぶ
  - G-R13 (MUST) `known_kind` は `bits <= 6 || bits == 15` と書く。どのコミットも 9 の関門を通す。確かめ: `grep -n "bits <= 6 || bits == 15" crates/eml_rt/src/meta.rs` が空でない。`cargo verus verify -p eml_rt` → `0 errors`
  - G-R14 (MUST) expected.txt は T-2 と T-3 の後で基点と同じバイト列で、T-4 は行を足すだけにする。確かめ: T-2、T-3 の後に `git diff --stat 986b65a0 -- crates/eml_rt/src/heap/tests/expected.txt` → 空。T-4 の後に `git diff --no-ext-diff HEAD~1 -- crates/eml_rt/src/heap/tests/expected.txt | grep '^-[^-]'` → 空
  - G-R15 (MUST) eml_rt の操作の名前は、`Ctx` の `unboxed_{make,empty,length,get,set,fill,freeze,thaw}` (8 つ) と、`Heap` の `alloc_unboxed`、`unboxed_{len,word,put,copy}` にする。`get` は `get`/`read` の行を、`set` は `set`/`write` の行を、`length` は2つの `length` の行を受け持つ。確かめ: `git grep -c "pub fn unboxed_\|pub unsafe fn unboxed_" crates/eml_rt/src/heap/ctx/unboxed.rs` → 8。`git grep -c "pub fn alloc_unboxed\|pub unsafe fn unboxed_" crates/eml_rt/src/heap/unboxed.rs` → 5
  - G-6 (MUST) Float と Int の行が共有する実行時の操作は、1つの操作につき1つで、要素は 64 ビットの語とする。確かめは G-R15 と同じ。試作の設計にあった `git grep -n "fn unboxed_"` は spec fn にも当たるので使わない
- リポジトリに残す所: `docs/spec/object-model.md` の「種類」「配列」「回復」「核の操作」「凍結した領域」(T-2〜T-4)、`docs/spec/vm.md:232` (T-2)

#### D-5: 段の中の文書は、実装した規則を同じコミットで書く

- 決めたこと:
  - G-7 (MUST) 型の名前を書いた将来の文書を、この段の中で直す: roadmap.md:31-33 と :492 と :528-530 と「型システム」、native.md:115-123、ffi.md:41-66 (型変数の規則を含む)、multicore.md:396 と :562。確かめ: `git grep -n "tabulate_floats\|for_mut_floats\|tabulate_ints\|for_mut_ints" docs` → 空 (T-5 の後)。`git grep -l "UnboxedArray" docs/future/ffi.md docs/future/multicore.md docs/future/native.md docs/future/roadmap.md` → 4 ファイル (T-8 の後)
  - native.md の「生の要素の配列」は、実装したコミットごとに削る。数の語の行 (:123) は T-2 で、`Raw` の物体の行 (:118) は T-3 で、`eml_rt` の操作と `Lin`/`lin` の行 (:119-120) は T-4 で消す。残りを新しい型の名前で書き直し、VM の文を消すのは T-5 で行う
- 理由: リポジトリの規則では、`docs/` は今の状態を書き、`spec/` は規則を実装したコミットで書く。native.md を T-5 でまとめて直す案もあったが、それでは T-2〜T-4 の間、実装した事柄を将来のこととして書くことになる
- リポジトリに残す所: 各タスクの文書の変更そのもの

#### D-6: 汎用の関数は、閉包を特殊化したときに要素を箱に入れない

- 決めたこと:
  - G-8 (MUST) `bench/spectral_norm.em` を `UnboxedArray Float` と `UnboxedArray.get` で書き直し、`var u`/`var v` は残す。`RunStats` の snapshot は `boxes <= 1`、`allocations <= 50`、出力は `1.274219991` とする。満たさないときは G-8 を緩めない。`eml_test_support::lir` で `box` を探し、原因を直す
  - G-9 (MUST) Float の `UnboxedArray.tabulate 1000` の `RunStats` のテストを置き、`boxes = 0` を確かめる
- 理由: 高階関数を既知の閉包で特殊化すると、閉包の結果は箱に入らない。2つのモジュールにまたがる形でも `boxes=0` だった (試作)
- 確かめ: T-5 と T-7 の完了の条件
- リポジトリに残す所: `docs/implementation/benchmarks.md` の「配列の `Float` の要素の箱」(T-7)

### 5.2 短中期の設計

理由があれば実装中に直してよい。直したら、コミットのメッセージに書く。

#### D-10: `eml_rt` の配列の数の語 (T-2)

- `Heap::alloc_data` の本体を `alloc_data_ext(tag, scanned, raw, lin, force: bool)` に移し、`let extended = force || scanned >= Meta::EXTENDED_SCAN as usize;` とする。ensures の `scanned >= 255` はすべて `force || scanned >= 255` にする。`lemma_data_perm` の `extended == (scanned >= 255)` は `!extended ==> scanned < 255` にする。`pub fn alloc_data(..)` は `alloc_data_ext(.., false)` で、引数も ensures も変えない
- 本体を `alloc_data_rc(tag, scanned, raw, lin, rc: i32, force)` (`requires rc == 0 || rc == 1`、`ensures rc == 1 ==> made(..)`、`rc == 0 ==> made_immortal(..)`) にくくり出す。`string.rs:619-640` の `make_text` と同じ形にする。`alloc_data_ext` は rc 1 で、`pub fn alloc_array_immortal(&mut self, lin: bool) -> Word` は `alloc_data_rc(0, 0, 0, lin, 0, true)` とする
- arrays.rs:
  - 11 か所の `scan_of(t) == if n >= 255 {255u8} else {n as u8}` を `scan_of(t) == 255u8` に、`first == if n >= 255 {1int} else {0int}` を `first == 1int` に、`let first = if k >= 255 {1} else {0}` を `let first = 1` に直す
  - 4 か所の `alloc_data(0, k, 0, lin)` を `alloc_data_ext(0, k, 0, lin, true)` にする
  - `array_ok` に `&&& h.scan_of(a) == 255` を足し、`checked_array` は `m.scan() != Meta::EXTENDED_SCAN` のときも `WrongKind` を返す
  - 定数になった `first` の引数 (`fill` と4つの補題) を消し、`if scan_of == 255 {1} else {0}` の 7 か所を `1` にする。これは整理で、試作の最小の形でも検証は通る
- recover.rs:1355-1361 は `let nw = new.alloc_data_ext(f.tag, f.count, raw, false, f.first == 1); let nfirst: usize = f.first;` とする。`frame_ok`/`build_ok`/`lemma_build_start` は変えない (試作で確かめた)
- `crates/eml_vm/src/compile/constants.rs` の `empty_mut_array` は `self.heap().alloc_array_immortal(true)` にする。`alloc_immortal` は `scan == 255` を禁じているからである (mod.rs:1157-1165)
- 変えない所 (試作で確かめた): `Core::scan_get`、`push_children`、`foreign_children`、share.rs、transfer.rs、release.rs、walk.rs、`src/frozen.rs`、`ctx/proof.rs:1551-1589`、`field.rs:173`、eml_vm の `data.rs:13`、`data.rs:53`、`forks.rs:82`、`apply.rs:298`
- 255 未満の配列は 8 バイト増える。それでもワークスペース全体のテスト 4463 件と bench の `RunStats` は変わらない (試作で確かめた。`RunStats` にバイトの数はない)

#### D-11: `Raw` の種類と `eml_rt` の操作 (T-3、T-4)

meta.rs:

- `Kind::Raw = 6` を足し、`kind_of`、`from_bits`、`name` (`"Raw"`) に腕を足す。`known_kind(bits) = bits <= 6 || bits == 15` とする。選言を 8 つ並べる形にすると `share.rs:1368` の `lemma_expand` が rlimit を超えることを、3 回の実行で確かめてある
- `pub open spec fn opaque_spec(self) = Foreign || has_body() || Raw` を足し、`opaque()` が `r == self.opaque_spec()` を保証するようにする。`pub open spec fn self_made(self) = has_body() || Raw` も足す。`tokenable` は変えない

inv.rs:

- `pub(crate) open spec fn raw_wf(p, cap) = p_count(p) == 0 && !p_extended(p) && cap >= 1 && p_filled(p, cap) && p_word(p, 0).bits_spec() + 1 <= cap` を足す
- `perm_wf` に2つ足す
  - `st == Live || st == Dying ==> (p_kind(p) == Raw ==> raw_wf(p, cap))`
  - `st == Building ==> !p_kind(p).self_made()`
- 同じ形を写す所: `safe.rs` の `safe_perm_wf` (2 か所)、`safe.rs:546` と `:614` (`self_made`)、`src/frozen.rs` の物体の述語 (`p_kind(p) == Raw ==> raw_wf(p, cap)`)

新しい述語に変える所 (付録の差分にある):

- mod.rs: `alloc`/`alloc_immortal`/`alloc_building` の requires と `debug_assert!`、`raw_at`、`raw_check` (ensures の 2 か所)、`set_lin` (requires、`r is Ok` の節、ghost の `c`、検査の `matches!(m.kind(), Kind::Data | Kind::Raw)`)
- `reuse.rs:488` の `con_at`、`field.rs:279` の `lemma_raw_set`、`states.rs:466` と `:662`

変えない所 (理由つきで確かめた):

- `walk.rs:97`/`:162`、`safe.rs:1303`: 凍結で除くのは `Foreign` だけ
- `release.rs:689` の `free_block_ok`: `Foreign` だけを除き、`Dying` では `raw_wf` が保たれる
- check.rs:475/:493 と release.rs の `WrongLayout`: `Data` を求める
- `ctx/io.rs`、`ctx/strings.rs`、`ctx/bytes.rs`、`ctx/proof.rs:860`/`:912`、`Meta::has_finalizer`

`crates/eml_rt/src/heap/unboxed.rs` は新しいファイルで、すべて `verus!` の中に書く。heap/mod.rs に `mod unboxed;` を足し、`trust.rs` の `FILES` に `("heap/unboxed.rs", include_str!("heap/unboxed.rs"))` を足す。

```rust
impl<M: Mode> Heap<M> {
    pub closed spec fn is_unboxed(&self, w: Word) -> bool;        // ptr、読める、種類 Raw、tag 0
    pub closed spec fn unboxed_len_spec(&self, w: Word) -> int;   // word_at(w, 0).bits_spec()
    pub closed spec fn unboxed_at(&self, w: Word, i: int) -> u64; // word_at(w, 1 + i).bits_spec() as u64
    pub fn alloc_unboxed(&mut self, n: usize, x: u64, lin: bool) -> (w: Word)
        requires old(self).wf(), old(self).safe_settleable(), !M::CHECKED ==> old(self).settleable(), n <= MAX_LEN
        ensures final(self).made(*old(self), w), final(self).is_unboxed(w), final(self).unboxed_len_spec(w) == n,
                final(self).lin_of(w) == lin, final(self).owned(w),
                forall|i: int| 0 <= i < n ==> #[trigger] final(self).unboxed_at(w, i) == x,
                final(self).grew(*old(self), w), M::CHECKED ==> final(self).words_kept(*old(self));
    pub unsafe fn unboxed_len(&self, w: Word) -> (r: Result<usize, HeapError>);             // 借りる。検査: WrongKind
    pub unsafe fn unboxed_word(&self, w: Word, i: usize) -> (r: Result<u64, HeapError>);   // 借りる。requires i < len。検査: WrongKind、OutOfBounds
    pub unsafe fn unboxed_put(&mut self, w: Word, i: usize, v: u64) -> (r: Result<(), HeapError>); // set_raw と同じく一意で書けること。検査: NotWritable、OutOfBounds
    pub unsafe fn unboxed_copy(&mut self, dst: Word, src: Word, n: usize) -> (r: Result<(), HeapError>)
        requires dst.bits_spec() != src.bits_spec(), ...; // dst は一意で書ける。n は両方の長さ以下
}
```

- `alloc_unboxed` は次の順で作る
  1. `alloc_header(n + 1, 1, Meta::new(0, 0, Kind::Raw, lin))` を呼ぶ
  2. 語 0 に `Word::from_bits(n)` を書く
  3. 語 1..=n を `alloc_data` の単位の詰め物のループ (mod.rs:1374-1420) と同じ形の `ptr_mut_write` で埋める。値は `Word::from_bits(x as usize)` にする
  4. `fill_zero(h, perm, n + 1, cap_words, ..)` で残りを 0 にする
  5. `lemma_data_perm` の兄弟の `lemma_raw_perm` で `raw_wf` から `perm_wf(.., Live, ..)` を導く
- `unboxed_put` は `slot_put` に、新しい補題 `lemma_unboxed_set` (requires は `kind == Raw && 1 <= i < 1 + len`) を添える。`p_scanned` のスロットは変わらないので、辺についての論法は `lemma_raw_set` と同じになる
- 要素に任意のビット列を置いてよい。`word_ok`/`slot_word` の義務は `p_scanned` のスロットにしか掛からず (inv.rs:456-470)、`Word::from_bits` はどの `usize` も表すからである (word.rs:166-172)
- 回復は recover.rs の `Bytes` の腕の後に `Kind::Raw` の腕を足す。検査モードの持ち主の確かめは `Bytes` と同じにし、`n` を語 0 から読む。`alloc_unboxed(n, 0, false)` と `unboxed_copy` で写し、`lemma_target_made` と `lemma_source_insert` を `Bytes` の腕 (recover.rs:764-787) と同じに使う

`crates/eml_rt/src/heap/ctx/unboxed.rs` も新しいファイルである。heap/ctx.rs に `mod unboxed;` を足し、`trust.rs` の `FILES` にも足す。spec の書き方は arrays.rs と同じで、`Ctx` の `pub closed spec fn` として `is_unboxed`、`unboxed_array_len`、`unboxed_array_at`、`unboxed_array_lin` を置く。内部の関数 `checked_unboxed` (ptr、`meta`、`kind == Raw`、`tag == 0`。外れたら `WrongKind`) と、`length_of` に当たる `unboxed_length_of` も置く。`MAX_LEN` (`0xFFFF_0000`) は arrays.rs から heap/ctx.rs に移して共有する。

| 操作 | 所有 | 両方のモードでの振る舞い | 誤り |
|---|---|---|---|
| `pub fn unboxed_make(&mut self, n: i64, x: u64, lin: bool) -> Result<Word, Fault>` | | `n < 0` なら `Index(IndexFault::Length { len: n })`。`n as usize > MAX_LEN \|\| over_budget(&self.control, (n + 4) * 8)` なら `Stopped(MemoryLimit)`。それ以外は `alloc_unboxed(n, x, lin)` | `array_make` と同じ |
| `pub fn unboxed_empty(&mut self, lin: bool) -> Result<Word, Fault>` | | `alloc_unboxed(0, 0, lin)` | いつも `Ok` |
| `pub unsafe fn unboxed_length(&mut self, a: Word) -> Result<i64, Fault>` | 借りる | `unboxed_len` | 検査: `Heap(WrongKind)` |
| `pub unsafe fn unboxed_get(&mut self, a: Word, i: i64) -> Result<u64, Fault>` | 借りる | 範囲を確かめて `unboxed_word` | `Index(IndexFault::Array { index, len })`。検査: `Heap(WrongKind)` |
| `pub unsafe fn unboxed_set(&mut self, a: Word, i: i64, x: u64) -> Result<Word, Fault>` | `a` を消費 | 範囲外なら `decref(a)` して `Index(Array{..})`。`is_unique(a)` ならその場で `unboxed_put` して `a` を返す。それ以外は `alloc_unboxed(n, 0, lin_of(a))`、`unboxed_copy`、`unboxed_put`、`decref(a)` | `array_set` と同じ |
| `pub unsafe fn unboxed_fill(&mut self, a: Word, x: u64) -> Result<Word, Fault>` | `a` を消費 | 一意ならその場ですべてに書き、`a` を返す。共有なら、検査モードは `Heap(NotUnique)`、速いモードは `alloc_unboxed(n, x, lin_of(a))` して `decref(a)` | `relin` の共有した `freeze` の規則と同じ |
| `pub unsafe fn unboxed_freeze(&mut self, m: Word)` / `unboxed_thaw(&mut self, a: Word) -> Result<Word, Fault>` | 消費 | `relin_unboxed(a, lin)`: 一意なら `set_lin`。共有で `M::CHECKED && !lin` なら `Heap(NotUnique)`。それ以外は `alloc_unboxed(n, 0, lin)`、`unboxed_copy`、`decref(a)` | `relin` と同じ |

- 各操作の postcondition: `wf`、`evv_spec`/`yield_spec` が変わらないこと、誤りの集合を書く。`held_ok && is_unboxed(a) && holds(a)` のもとで、さらに次を書く
  - 結果の `holds`/`is_unboxed`、`unboxed_array_len` の関係、`unboxed_array_lin`
  - `unboxed_array_at(w, j)` は、書いた添字を除いて元の要素に等しい。`fill` ではすべて `x` に等しい
  - `evolved_by` (relin では `evolved_lin`) と `held_ok`
- `dupable` の義務がないので、どの証明も arrays.rs の対応するものより小さい
- `Fault` と `IndexFault` は変えない

#### D-12: extern の行と std のモジュール (T-5)

`crates/eml_extern/src/lib.rs` の `ExternType` では、`MutArray` の後に次の2行を足す。

```rust
UnboxedArray => generic_type_row("Prelude.UnboxedArray", Unr),
MutUnboxedArray => generic_type_row("Prelude.MutUnboxedArray", Lin),
```

`Extern` の行は `MutArrayFreeze` の後、`CheckIndex` のコメントの前に置く。

- 群の上に、docs/spec/stdlib.md の「`Std.UnboxedArray`」を引く日本語のコメントを1つ置く。中身は、要素が生の `int`/`float` であること、instance ごとに行が1つあること、Float と Int の行が1つの実行時の操作を共有すること、とする
- コメントに、引用符つきの行の名前を書かない。G-2 は `"Std.UnboxedArray.Unboxed ` などを含む行を数えるからである

下の表で、`T` は `Float` か `Int`、`R` はそれに合う `Repr` である。

| variant | 行 |
|---|---|
| `UnboxedArrayLength` | `borrowing(&[true], kinded("Std.UnboxedArray.length", &[Obj], &[Unr], Int, Unr, Purity::Pure))` |
| `UnboxedArrayThaw` | `kinded("Std.UnboxedArray.thaw", &[Obj], &[Unr], Obj, Lin, Purity::Pure)` |
| `MutUnboxedArrayEmpty` | `kinded("Std.MutUnboxedArray.empty", &[Unit], &[Unr], Obj, Lin, Purity::Pure)` |
| `MutUnboxedArrayLength` | `borrowing(&[true], FunctionRow { with_arg: Some(0), ..kinded("Std.MutUnboxedArray.length", &[Obj], &[Lin], Int, Unr, Purity::Pure) })` |
| `MutUnboxedArrayFreeze` | `kinded("Std.MutUnboxedArray.freeze", &[Obj], &[Lin], Obj, Unr, Purity::Pure)` |
| `UnboxedArrayMake{T}` | `row("Std.UnboxedArray.Unboxed T.make", &[Int, R], Obj, Purity::Panics)` |
| `UnboxedArrayGet{T}` | `borrowing(&[true, false], kinded("Std.UnboxedArray.Unboxed T.get", &[Obj, Int], &[Unr, Unr], R, Unr, Purity::Panics))` |
| `UnboxedArraySet{T}` | `row("Std.UnboxedArray.Unboxed T.set", &[Obj, Int, R], Obj, Purity::Panics)` |
| `MutUnboxedArrayMake{T}` | `kinded("Std.MutUnboxedArray.MutUnboxed T.make", &[Int, R], &[Unr, Unr], Obj, Lin, Purity::Panics)` |
| `MutUnboxedArrayRead{T}` | `borrowing(&[true, false], FunctionRow { with_arg: Some(0), ..kinded("Std.MutUnboxedArray.MutUnboxed T.read", &[Obj, Int], &[Lin, Unr], R, Unr, Purity::Panics) })` |
| `MutUnboxedArrayWrite{T}` | `kinded("Std.MutUnboxedArray.MutUnboxed T.write", &[Obj, Int, R], &[Lin, Unr, Unr], Obj, Lin, Purity::Panics)` |
| `MutUnboxedArrayFill{T}` | `kinded("Std.MutUnboxedArray.MutUnboxed T.fill", &[Obj, R], &[Lin, Unr], Obj, Lin, Purity::Pure)` |

型の行が2つ、関数の行が 19 (ふつうの行 5、instance の行 14) になる。コンパイラのコード、`FunctionRow` の欄、Repr は変えない。

`std/Prelude.em` の `pub extern data MutArray a` の後に次を足す。上のコメントは4つの型を名指す形に直す。中身は、`Array` と `UnboxedArray` は要素の Kind に従い、`MutArray` と `MutUnboxedArray` はいつも `Lin`、とする。

```haskell
pub extern data UnboxedArray a
pub extern data MutUnboxedArray a
```

`crates/eml_hir/src/lib.rs` の `STD` には、`MutArray.em` の後に `("UnboxedArray.em", include_str!("../../../std/UnboxedArray.em"))` と `("MutUnboxedArray.em", include_str!("../../../std/MutUnboxedArray.em"))` を足す。

`std/MutUnboxedArray.em` は次の本文に、「`Std.MutUnboxedArray`」を引く日本語の頭のコメントを添える。この形のまま、コンパイルと実行を試作で確かめてある。

```haskell
import Array

pub class MutUnboxed a where
  make : Int -> a -> <Panic> MutUnboxedArray a
  read : MutUnboxedArray a -> Int -> <Panic> (a, MutUnboxedArray a)
  write : MutUnboxedArray a -> Int -> a -> <Panic> MutUnboxedArray a
  fill : MutUnboxedArray a -> a -> MutUnboxedArray a

instance MutUnboxed Float where
  extern make
  extern read
  extern write
  extern fill

instance MutUnboxed Int where
  extern make
  extern read
  extern write
  extern fill

pub extern empty : Unit -> MutUnboxedArray a
pub extern length : MutUnboxedArray a -> (Int, MutUnboxedArray a)
pub extern freeze : MutUnboxedArray a -> UnboxedArray a

pub tabulate : MutUnboxed a => Int -> (Int -> <e> a) -> <Panic | e> MutUnboxedArray a
tabulate n f =
  let go i m =
    if i == n then m else go (i + 1) (write m i (f i))
  if n < 0 then
    -- (理由のコメントを日本語で) make に渡す要素がない。Array.make が同じ Panic を起こし、Unr の結果は捨てられる
    let _ = Array.make n ()
    empty ()
  else if n == 0 then
    empty ()
  else
    let first = f 0
    go 1 (make n first)
```

`std/UnboxedArray.em` には「`Std.UnboxedArray`」を引く頭のコメントを添え、`try_panic` のコメントは `std/Array.em` に合わせる。

```haskell
import Array
import List
import MutUnboxedArray (MutUnboxed)

pub class MutUnboxed a => Unboxed a where
  make : Int -> a -> <Panic> UnboxedArray a
  get : UnboxedArray a -> Int -> <Panic> a
  set : UnboxedArray a -> Int -> a -> <Panic> UnboxedArray a

instance Unboxed Float where
  extern make
  extern get
  extern set

instance Unboxed Int where
  extern make
  extern get
  extern set

pub extern length : UnboxedArray a -> Int
pub extern thaw : UnboxedArray a -> MutUnboxedArray a

pub tabulate : Unboxed a => Int -> (Int -> <e> a) -> <Panic | e> UnboxedArray a
tabulate n f = MutUnboxedArray.freeze (MutUnboxedArray.tabulate n f)

pub from_list : Unboxed a => List a -> UnboxedArray a
from_list xs =
  let go i ys m =
    match ys with
      | Nil -> m
      | y :: rest -> go (i + 1) rest (MutUnboxedArray.write m i y)
  let build () =
    match xs with
      | Nil -> MutUnboxedArray.freeze (MutUnboxedArray.empty ())
      | x :: rest -> MutUnboxedArray.freeze (go 1 rest (MutUnboxedArray.make (List.length xs) x))
  match try_panic build with
    | Ok a -> a
    | Err _ -> MutUnboxedArray.freeze (MutUnboxedArray.empty ())

pub to_list : Unboxed a => UnboxedArray a -> List a
to_list a =
  let collect i acc =
    if i < 0 then acc else collect (i - 1) (get a i :: acc)
  match try_panic (fn () -> collect (length a - 1) Nil) with
    | Ok xs -> xs
    | Err _ -> Nil

pub of_array : Unboxed a => Array a -> UnboxedArray a
of_array a =
  match try_panic (fn () -> tabulate (Array.length a) (fn i -> Array.get a i)) with
    | Ok u -> u
    | Err _ -> from_list Nil

pub to_array : Unboxed a => UnboxedArray a -> Array a
to_array u =
  match try_panic (fn () -> Array.tabulate (length u) (fn i -> get u i)) with
    | Ok a -> a
    | Err _ -> Array.from_list Nil
```

#### D-13: VM は、まず汎用の extern で動かし、次に専用の命令を3つ足す (T-5、T-6)

- T-5 では、19 の行をすべて `extern_call` の汎用の `Op::Extern` で動かす
  - 実行時の操作1つにつき腕を1つ置き、Float と Int の行で共有する
  - 要素は `task.raw(slot) as u64` で渡し、`task.set_raw(dst, x as i64)` で受ける
  - `extern_call` の `match` はすでに外側の `unsafe` の中にあるので、腕に `unsafe` を書かない。書くと `unused_unsafe` になり、clippy の `-D warnings` で落ちる
  - 腕の書き方:
    - `make` の4行: `let lin = matches!(ext, Extern::MutUnboxedArrayMakeFloat | Extern::MutUnboxedArrayMakeInt); let w = ctx.unboxed_make(task.raw(arg(0)), task.raw(arg(1)) as u64, lin)?; task.set(dst, w);`
    - `get`/`read` の4行: `let x = ctx.unboxed_get(task.get(arg(0)), task.raw(arg(1)))?; task.set_raw(dst, x as i64);`
    - `set`/`write` の4行: `let w = ctx.unboxed_set(task.get(arg(0)), task.raw(arg(1)), task.raw(arg(2)) as u64)?; task.set(dst, w);`
    - `length` の2行: `let n = ctx.unboxed_length(task.get(arg(0)))?; task.set_raw(dst, n);`
    - `fill` の2行: `ctx.unboxed_fill(task.get(arg(0)), task.raw(arg(1)) as u64)?`
    - `MutUnboxedArrayEmpty`: `ctx.unboxed_empty(true)?` (引数は読まない)
    - `UnboxedArrayThaw`/`MutUnboxedArrayFreeze`: `ctx.unboxed_thaw`/`ctx.unboxed_freeze`
- T-6 では `UnboxedGet { dst, a, b }`、`UnboxedLength { dst, a }`、`UnboxedSet { dst, at }` を足す。`Array` が専用の命令に写す行は、ここでも専用の命令に写す
- 理由: 行の族ごとに専用の命令から汎用の extern に移して測った。1 回あたりの命令の数は、`get` で +67.1、`length` で +62.8、`set` で +59.0 増えた (release、100 万回のループ、3 回の中央値)。spectral_norm では、`get` だけで +26.9M (+4.7%) 増えた。`set` は `tabulate`、`from_list`、`of_array` で要素ごとに1回通る。native.md:119 は「新しい命令は作らない」と書いていたが、この測定でそれを改め、vm.md に書く。ネイティブのコードは物体の配置から展開するので、VM の命令を写す必要はない
- 退けた案:
  - すべてを汎用の extern で動かす案: 操作ごとに 1 回約 60 命令遅い
  - `Bytes` にならって `get`/`length` だけを専用にする案: `set` も 1 回あたりの差が同じで、組み立てで要素ごとに通る
  - `Op::ArrayGet` を種類で分岐させる案: 呼ぶ `Ctx` の操作と結果のスロット (語か生の値か) が違い、どちらの道にも分岐が入る
- `RunStats`: VM は新しく何も数えない。`allocations` が動くのは `Heap` を通したときだけである
- 名前: 新しい命令の名前は `Unboxed*` とする。構造体の箱を開ける既存の命令 `ReleaseUnboxed`/`UnboxStruct` と並ぶが、型の名前に合わせてこのままにする

## 6. 実装の制約と約束

- コードのコメントと `docs/` は日本語で書き、natural-japanese のスキルに従う。コメントは理由だけを書き、規則は `docs/` の節を引く。コードと `docs/` から `docs/plans/` とこの計画の ID (D-x、T-x、G-x) を引かない (リポジトリの規則)
- `spec/` の節は、その規則を実装したコミットの中で書く (D-5)
- テストの変更の種類は、docs/implementation/testing.md の「テストの変更の運用」に従う。この段で合意した種類1と種類2の変更は、下の表のものだけである。ほかの既存のテストの期待値が変わったら、止めて計画を直す

| テスト | タスク | 種類 | 理由 |
|---|---|---|---|
| `crates/eml_vm/tests/bench.rs::spectral_norm` の inline snapshot | T-7 | 2 | プログラムを書き直した。`boxes` が 4001 から 0 か 1 になる |
| `bench.rs::unwind_points` の snapshot の `spectral_norm: 73` の行 | T-7 | 2 | プログラムを書き直した |
| `bench.rs::spectral_norm_allocates_14_fewer` と定数 `SPECTRAL_NORM_BOXED_TUPLE_ALLOCATIONS` | T-7 | 1 (消す) | 基準の 4057 は、消える要素の箱を数えていた。`spectral_norm_does_not_box_elements` に置き換える |

- 足すだけのもの (既存の期待値は変わらないので、合意は要らない):
  - `crates/eml_core_ir/tests/externs.rs` の `PROBES`、`probe()` の腕、2つの許可の一覧の腕
  - eml_rt の単体テストと expected.txt の行
  - meta.rs の種類の一覧のテストに `Kind::Raw` を足すこと
  - 新しい UI テストと REPL のトランスクリプト
  - scaling.rs と bench.rs の新しいテスト
- `cargo nextest run` のどのテストも、`-j 1` で 1 秒以内に終わること (リポジトリの規則)
- 新しい UI テストの `run`/`run-fail` は、検査モードと速いモードの両方で流れる。`par/` と `async/` のものは、2 つの seed と 2/8 の worker でも流れる

## 7. 実装のタスク

各タスクは1つのコミットで、それだけで確かめられる。計画そのものを入れる最初のコミット (`Add the raw-arrays plan`) を T-1 とする。

| ID | タスク | 根拠 | 先に要るもの | 並べられるか |
|---|---|---|---|---|
| T-1 | 計画を入れる | - | - | - |
| T-2 | 配列がいつも数の語を持つ | D-4、D-10 | T-1 | T-3 と並べられる |
| T-3 | `Raw` の種類を `Heap` に入れる | D-3、D-4、D-11 | T-1 (T-2 と同じファイルに触れるので、T-2 の後が既定) | 条件つき |
| T-4 | `Ctx` の `unboxed_*` の操作と期待値の表 | D-4、D-11 | T-3 | いいえ |
| T-5 | 型、行、std、汎用の VM の腕、spec、テスト | D-1、D-2、D-5、D-6、D-12、D-13 | T-4 | いいえ |
| T-6 | 専用の命令を3つ足す | D-13 | T-5 | いいえ |
| T-7 | spectral_norm を書き直す | D-6 | T-6 | いいえ |
| T-8 | 段を閉じる | D-1〜D-6 | T-7 (と、要れば R) | いいえ |

関門 (T-2 以降のすべてのコミットで、devShell の中、リポジトリの根から流す):

```sh
cargo build
cargo nextest run                                              # すべて通る
cargo clippy --all-targets -- -D warnings && cargo fmt --check # exit 0
cargo nextest run -p eml_cli --test integration -E 'test(citations)'   # 通る
! git status --porcelain | grep -q '\.snap\.new$'              # exit 0
```

eml_rt の関門 (T-2〜T-4 で上に加える):

```sh
cargo verus verify -p eml_rt     # "verification results:: N verified, 0 errors"。警告と trigger の注意がない
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')   # exit 0
cargo nextest run -p eml_rt      # すべて通る (基点は 814)
```

### T-1: 計画を入れる

- 目的: ロードマップの「進め方」の、段の最初のコミット
- 対象: この文書と `docs/plans/2026-11-05-raw-arrays/c1c2-probe.diff`
- 作業: 承認された計画をコミットする (`Add the raw-arrays plan`)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件: `git show --stat HEAD` → 2 ファイル

### T-2: 配列がいつも数の語を持つ

- 目的: ネイティブのコードが配列の数を `base + 8` で読めるようにする (D-4)
- 対象: `crates/eml_rt/src/heap/mod.rs`、`heap/ctx/arrays.rs`、`heap/recover.rs`、`crates/eml_vm/src/compile/constants.rs`、テスト、文書
- 作業:
  1. `git apply docs/plans/2026-11-05-raw-arrays/c1c2-probe.diff` を流し、mod.rs (`alloc_data_ext` の部分)、arrays.rs、recover.rs の hunk だけを残す。meta.rs、inv.rs、safe.rs、frozen.rs、field.rs、states.rs、reuse.rs の hunk と、mod.rs の述語と `set_lin` の hunk は T-3 に回す (`git stash` か別のブランチに取っておく)
  2. D-10 の残り、つまり `alloc_data_rc`、`alloc_array_immortal`、`array_ok`/`checked_array`、`first` の整理、`constants.rs` を書く
  3. 単体テストを足す
     - `heap/tests/arrays.rs`: `arrays_always_carry_the_count_word`。n が 0、1、254、255、300 のときと `array_empty` のときに `meta(a).scan() == Meta::EXTENDED_SCAN` であることを確かめる
     - `heap/tests/recover.rs`: 回復した 3 要素の配列が scan 255 を保つ
     - `heap/tests/frozen.rs`: `alloc_array_immortal(true)` を凍結し、つないだヒープから長さ 0 を読む
     - `heap/tests/checked.rs`: `checked_array` が scan 0 の tag 0 の `Data` を断る
  4. 文書を直す
     - object-model.md の「配列」: 「要素が 255 個以上なら … 数の語を先に置く」を「長さによらず」に直す。理由として、ネイティブのコードが数を `base + 8` で読むことを書く
     - object-model.md の「ヘッダ」の `scan` の箇条: 配列はいつも 255
     - object-model.md の「核の操作」の表: `alloc_data_ext` と `alloc_array_immortal` の行
     - object-model.md の「凍結した領域」: 空の `MutArray` の定数が数の語を持つ
     - object-model.md の回復の `Data` の行: 数の語の有無も同じにする
     - vm.md:232: `empty_mut_array` の作り方を `alloc_array_immortal(true)` に直し、数の語 (0) を持つことを書く
     - native.md:123: この行を消す (D-5)
  5. コードのコメントは docs/spec/object-model.md の「配列」を引く
- 実装者に任せること: `first` の引数を消す整理の範囲。消す (既定) か、試作の最小の形のままにするか。消したときに証明が通らなければ、最小の形に戻す
- 条件: いつも
- 完了の条件:
  - eml_rt の関門と関門 → すべて通る。Verus は `0 errors`
  - `git diff --stat 986b65a0 -- crates/eml_rt/src/heap/tests/expected.txt` → 空
  - `cargo nextest run -p eml_vm --test integration -E 'test(bench::)'` → 通る (snapshot は変わらない)
  - `grep -n "f.first == 1" crates/eml_rt/src/heap/recover.rs` → 1 行。`grep -n "f.count >= 255" crates/eml_rt/src/heap/recover.rs` → 空
- 気をつける所: `alloc_immortal` は `scan == 255` を禁じているので、VM の定数を手で作らない。eml_rt の外で変わるのは `constants.rs` だけである (G-R2)

### T-3: `Raw` の種類を `Heap` に入れる

- 目的: 生の要素の物体を、検証つきで作り、読み、書けるようにする (D-4、D-11)
- 対象: `crates/eml_rt/src/meta.rs`、`heap/inv.rs`、`heap/safe.rs`、`src/frozen.rs`、`heap/mod.rs`、`heap/field.rs`、`heap/states.rs`、`heap/reuse.rs`、`heap/recover.rs`、新しい `heap/unboxed.rs`、`trust.rs`、テスト、文書
- 作業:
  1. T-2 で取っておいた試作の hunk を当てる
  2. meta.rs に日本語のコメントを足す。不透明にする理由は数の語が不変条件だから、token にしない理由は token の箱がいつも `Data` だから、と書く
  3. D-11 の `heap/unboxed.rs` を書き、回復の `Raw` の腕を足す
  4. テストを足す
     - 新しい `heap/tests/unboxed.rs` (`both_modes!`。`heap/tests/mod.rs` に `mod unboxed;`): 作る、長さ、読む、書くの往復。`alloc_unboxed(0, ..)`。範囲外の `unboxed_word` が検査モードで `OutOfBounds`
     - checked.rs: `Raw` に `raw`/`set_raw`/`field`/`set_field`/`unbox_float`/`unbox_int`/`string`/`bytes`/`foreign`/`into_token`/`release` を当てると、`WrongKind`/`NotTokenable`/`WrongLayout`/`OutOfBounds` になる
     - `set_lin`: 一意な `Raw` は成功し、共有なら `NotUnique`、`Bytes` は `WrongKind`
     - `share`: rc が -1 になり、`unboxed_word` でまだ読める
     - `decref` で `lin` の `Raw` が空き、`mortal == 0` になる
     - recover.rs: 局所と共有の `Raw` を回復し、`lin` の `Raw` は `Unrecoverable` になる
     - リークの報告が `"Raw"` を数える
     - meta.rs の種類の一覧のテストに `Kind::Raw` を足し、`opaque() && !has_body() && !tokenable()` を確かめる
  5. 文書を直す
     - object-model.md の「種類」の表に `| Raw | 6 | 生の1語目に要素の数、その後に生の 8 バイトの要素 | 0 (scan は 0) | できない | なし | 不透明 |` を足し、表の下の箇条も直す
     - 回復の表に `| Raw | 要素を語で写す (alloc_unboxed と unboxed_copy) |` を足す
     - 「核の操作」: `alloc`/`alloc_immortal`/`con_at` の種類の一覧、`raw`/`set_raw` の `WrongKind` の一覧、`set_lin` の行 (`Data` か `Raw`)、`Heap` の5つの関数の行
     - 「報告する誤り」: `WrongKind` と `OutOfBounds` の箇条
     - 「`unsafe` の核」: 配列について書いた文を生の配列にも広げる
     - native.md:118: `Raw` の物体の行を消す (D-5)
- 実装者に任せること: `lemma_raw_perm` と `lemma_unboxed_set` の証明の組み立て方。rlimit を超えたら、その補題に `#[verifier::rlimit(N)]` と理由のコメントを付けてよい (先例は ctx.rs:3432)。ただし share.rs などの走査の側に置くのは rlimit の行だけにする (G-R9)
- 条件: いつも
- 完了の条件:
  - eml_rt の関門と関門 → すべて通る
  - `cargo nextest run -p eml_rt trust::` → 通る (`FILES` に足し、T の一覧は変わらない)
  - `grep -n "bits <= 6 || bits == 15" crates/eml_rt/src/meta.rs` → 1 行
  - `grep -n "external_body\|assume" crates/eml_rt/src/heap/unboxed.rs` → 空
  - `git diff --stat 986b65a0 -- crates/eml_rt/src/heap/tests/expected.txt` → 空
  - `git diff --stat HEAD~1 -- crates/eml_rt/src/heap/share.rs crates/eml_rt/src/heap/transfer.rs crates/eml_rt/src/heap/release.rs crates/eml_rt/src/heap/graph.rs crates/eml_rt/src/heap/remote.rs` → 空 (rlimit の行を足したときはその行だけ)
- 気をつける所: `known_kind` を選言で書くと、関係のない share.rs の補題が落ちる。`unboxed_copy` の requires に `dst != src` を忘れない

### T-4: `Ctx` の `unboxed_*` の操作と期待値の表

- 目的: VM が呼ぶ 8 つの操作を、検証つきで入れる (D-11)
- 対象: 新しい `crates/eml_rt/src/heap/ctx/unboxed.rs`、`heap/ctx.rs`、`heap/ctx/arrays.rs` (`MAX_LEN` を移す)、`trust.rs`、`heap/tests/table.rs`、`heap/tests/externs.rs`、`heap/tests/expected.txt`、`heap/tests/trusted.rs`、文書
- 作業:
  1. D-11 の表どおりに 8 つの操作を書く
  2. `heap/tests/table.rs` に生の配列の入力の形 (例: `U:[1,2]` は一意な `Unr`、`UD:` は共有、`UM:` は `lin`、Float には `UF:`) を足す。行は D-12 の行の名前で書く
     - 長さ: -1、0、1、3
     - 添字: 両端、1 つ外、`i64` の両端
     - `set`/`thaw`/`fill`/`freeze` は共有と一意の両方
  3. `heap/tests/externs.rs` の `call` (2032 行あたり) に新しい行の名前の腕を足す。`"Std.Array.get" | "Std.MutArray.read"` の腕 (2406 行) と同じ形にする
  4. `cargo nextest run -p eml_rt --run-ignored ignored-only the_table_can_be_written` で expected.txt を書き直し、差分を人が確かめる
  5. `heap/tests/trusted.rs::every_core_operation_once` に、`alloc_unboxed`、`unboxed_len`、`unboxed_word`、`unboxed_put`、`unboxed_copy` (別々の2つの物体)、`unboxed_make`、`unboxed_set` の呼び出しを1つずつ足す
  6. 文書: object-model.md の「配列」に生の配列の段落を足す。中身は次のとおり
     - `Raw` の物体の形
     - `make` は長さと予算 `(n + 4) * 8` を確保の前に確かめる
     - `get`
     - `set` は一意ならその場で、そうでなければ写す
     - `fill` はその場で書く。共有なら、検査モードは `NotUnique`、速いモードは写す
     - `freeze`/`thaw` は `MutArray` と同じ relin で、共有の `freeze` は検査モードで `NotUnique`
     - 誤りは `Array` と同じ

     native.md:119-120 (`eml_rt` の操作と `Lin`/`lin` の行) を消し、`Lin`/`lin` の意味はこの段落に移す (D-5)
- 実装者に任せること: 表の入力の文字。既存の `A:`/`D:`/`M:` と衝突しなければよい
- 条件: いつも
- 完了の条件:
  - eml_rt の関門と関門 → すべて通る
  - `git diff --no-ext-diff HEAD~1 -- crates/eml_rt/src/heap/tests/expected.txt | grep '^-[^-]'` → 空 (足すだけ)
  - `git grep -c "pub fn unboxed_\|pub unsafe fn unboxed_" crates/eml_rt/src/heap/ctx/unboxed.rs` → 8
  - `git grep -c "pub fn alloc_unboxed\|pub unsafe fn unboxed_" crates/eml_rt/src/heap/unboxed.rs` → 5
  - `git diff 986b65a0 -- crates/eml_rt/src/externs.rs` → 空
  - Linux で次の2つが、CPU の予算 (280 秒と 365 秒) の中で通る。Linux がなければ CI に任せ、そのことをメッセージに書く
    - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::`
    - 同じコマンドに `-Zmiri-tree-borrows` を足したもの
- 気をつける所: VM は、この時点ではまだ `unboxed_*` を呼ばない。eml_vm と eml_cli のファイルは変えない

### T-5: 型、行、std、汎用の VM の腕、spec、テスト

- 目的: 利用者が `UnboxedArray`/`MutUnboxedArray` を使えるようにする (D-1、D-12、D-13 の前半)
- 対象: `std/Prelude.em`、新しい `std/UnboxedArray.em`、`std/MutUnboxedArray.em`、`crates/eml_hir/src/lib.rs`、`crates/eml_extern/src/lib.rs`、`crates/eml_vm/src/vm.rs`、`crates/eml_core_ir/tests/externs.rs`、テスト、文書
- 作業:
  1. D-12 の行、型、std、`STD` を入れる
  2. D-13 の汎用の腕を入れる
  3. `crates/eml_core_ir/tests/externs.rs` を直す (この形で試作がコンパイルし、30 のテストが通った)
     - `PROBES` の `probe_cell` の前に次を足す

       ```haskell
       probe_unboxed_array : UnboxedArray Float -> UnboxedArray Float
       probe_unboxed_array a = a

       probe_mut_unboxed_array : MutUnboxedArray Float -> MutUnboxedArray Float
       probe_mut_unboxed_array a = a
       ```

     - `probe()` に `ExternType::UnboxedArray => "probe_unboxed_array"` と `ExternType::MutUnboxedArray => "probe_mut_unboxed_array"` を足す。足さないと、網羅していない `match` でコンパイルできない
     - `only_the_listed_rows_borrow_or_return_a_pair` の `"Std.MutArray.read"` の後に、次の4つの腕を足す。doc コメントの対の一覧に「`Std.MutUnboxedArray` の3つの行」を足す
       - `"Std.UnboxedArray.length" => (&[true], None)`
       - `"Std.UnboxedArray.Unboxed Float.get" | "Std.UnboxedArray.Unboxed Int.get" => (&[true, false], None)`
       - `"Std.MutUnboxedArray.length" => (&[true], Some(0))`
       - `"Std.MutUnboxedArray.MutUnboxed Float.read" | "Std.MutUnboxedArray.MutUnboxed Int.read" => (&[true, false], Some(0))`
     - `only_the_listed_rows_carry_a_lin_or_poly_mark` の `"Std.MutArray.freeze"` の後に、次の腕を足す。doc コメントに「`MutUnboxedArray` を扱う12」を足す
       - `"Std.UnboxedArray.thaw" => (&[Unr], Lin)`
       - `make` の2行 `=> (&[Unr, Unr], Lin)`
       - `"Std.MutUnboxedArray.empty" => (&[Unr], Lin)`
       - `"Std.MutUnboxedArray.length" => (&[Lin], Unr)`
       - `read` の2行 `=> (&[Lin, Unr], Unr)`
       - `write` の2行 `=> (&[Lin, Unr, Unr], Lin)`
       - `fill` の2行 `=> (&[Lin, Unr], Lin)`
       - `"Std.MutUnboxedArray.freeze" => (&[Lin], Unr)`
     - eml_hir のテストは `STD` を総なめにするので、変えない
  4. UI テストを足す (それぞれ 1 秒未満)
     - `tests/ui/run/unboxed/basics.em`: `tests/ui/run/arrays/basics.em` を写し、`Array` を `UnboxedArray` に、`MutArray` を `MutUnboxedArray` に置き換える。要素はすべて `Float` にする (`0` → `0.0`、文字列の要素は Float に、`noisy` は `Float.of_int (i * 10)` を返す)。`first_two` は `UnboxedArray.Unboxed a =>`、`read_later` は `MutUnboxedArray.MutUnboxed a =>` を取り、`first_two` を `Int` と `Float` で使う。snapshot の行の数は `run@arrays__basics.em.snap` と同じで、`Err` の行も同じになる
     - `tests/ui/run/unboxed/int_elements.em`: 最小の値は `-9223372036854775807 - 1` と書く。`from_list [9223372036854775807, lo, -1]` を `to_list` すると `[9223372036854775807, -9223372036854775808, -1]` と出る。`set`、`thaw`/`write`/`read`、`freeze` の往復もする
     - `tests/ui/run/unboxed/generic.em`: `Unboxed a => (a -> Float) -> UnboxedArray a -> <Panic> Float` の和と、`read`/`write` でその場で並びを逆にする `MutUnboxed a => MutUnboxedArray a -> <Panic> MutUnboxedArray a` を、`Float` と `Int` の両方で使う。部分適用した `MutUnboxedArray.read` も使う
     - `tests/ui/run/unboxed/conversions.em`: 空の場合を含めた `of_array`/`to_array`、`from_list`/`to_list`。`MutUnboxedArray.empty` と `length`。`-0.0` と無限大の往復
     - `tests/ui/run/unboxed/shared_fill_and_thaw.em`: 共有した配列を `thaw` して `fill` すると、元は変わらない。試作の出力は `(7.0, [7.0, 7.0, 7.0], [0.0, 5.0, 0.0])` だった。一意な配列の `thaw` と `fill` も試す
     - `tests/ui/run/par/unboxed_shared.em`: `UnboxedArray.tabulate 1000 Float.of_int` を作り、`Par.par` の左の子で `[0, 500)` を足す。右の子で `[500, 1000)` を足し、`freeze (fill (thaw a) 1.0)` の和も加える (a は共有なので写す)。`(l, r, UnboxedArray.get a 999)` を出す。期待は `(124750.0, 375750.0, 999.0)` である
     - `tests/ui/run/async/chan_mut_unboxed_array.em`: `tests/ui/run/async/chan_mutarray.em` を写し、`MutArray` を `MutUnboxedArray` に、`MutArray Int` を `MutUnboxedArray Int` にする。期待は `7 8`
     - `crates/eml_cli/tests/repl/unboxed/recover.repl`: `crates/eml_cli/tests/repl/arrays/recover.repl` を写し、`import UnboxedArray` と `UnboxedArray.make 3 0.0` にする。期待は arrays の snapshot と同じ形で、値は `[0.0, 0.0, 0.0]`、型は `UnboxedArray Float` (`<extern> : UnboxedArray Float`、`(<extern>, 1)`、`Some <extern>`) になり、`no leaks` で終わる
     - `tests/ui/run-fail/unboxed/get_out_of_range.em`: `main () = println (show (UnboxedArray.get (UnboxedArray.make 3 0.0) 3))`。期待は `array index 3 is out of range for length 3` で、位置は 26 桁目
     - `tests/ui/run-fail/unboxed/tabulate_negative_length.em`: `MutUnboxedArray.tabulate (-1) (fn i -> Float.of_int i)` を `MutUnboxedArray.freeze` で使う。期待は `array length -1 is negative` で、位置は `<std>/MutUnboxedArray.em:<Array.make の行>:13`。速いモードでも同じ
     - `tests/ui/run-fail/unboxed/make_negative_length.em`: `UnboxedArray.make (-1) 0.0` を直接呼ぶ。`array length -1 is negative` になる。`unboxed_make` の道を UI でも通すために置く
     - `tests/ui/check-fail/types/unboxed_no_instance.em`: `UnboxedArray.make 3 "x"` → E2006 (`Unboxed` の `String` の instance がない)
     - `tests/ui/check-fail/types/unboxed_index.em`: `(UnboxedArray.make 3 1.0)[0]` → E2001 (`Array _` を期待して `UnboxedArray Float`)
     - `tests/ui/check-fail/types/unboxed_var_index.em`: `var u = UnboxedArray.make 3 1.0` の後に `u[0] <- 2.0` → E2001 (`MutArray _` を期待)
     - `tests/ui/check-fail/linearity/mut_unboxed_array_unused.em`: 使わない `MutUnboxedArray.make 3 0.0` → E3003
  5. `crates/eml_vm/tests/scaling.rs` に `unboxed_float_elements_are_not_boxed_and_use_the_dedicated_instructions` を足す (G-9)。補助の `unboxed_sum(n)` は次のプログラムを返す。`stats(&unboxed_sum(1000), "499500.0\n")` と `stats(&unboxed_sum(2000), "1999000.0\n")` を流し、どちらも `boxes == 0` で、`allocations` と `unboxes` が2つで等しいことを確かめる

     ```haskell
     import Float
     import UnboxedArray

     sum_from : UnboxedArray Float -> Int -> Float -> <Panic> Float
     sum_from a i acc = if i == UnboxedArray.length a then acc else sum_from a (i + 1) (acc + UnboxedArray.get a i)

     main : Unit -> <IO, Panic> Unit
     main () =
       let a = UnboxedArray.tabulate {n} (fn i -> Float.of_int i)
       println (show (sum_from a 0 0.0))
     ```

  6. 文書 (このコミットで書く):

     | ファイル | 節 | 中身 |
     |---|---|---|
     | docs/spec/stdlib.md | 「`Std.MutArray`」の後に新しく「`Std.UnboxedArray`」「`Std.MutUnboxedArray`」 | 下の箇条 |
     | docs/spec/stdlib.md | 「実行時エラー」 | 配列の2つの行に `UnboxedArray` と `MutUnboxedArray` も挙げる (文は同じ) |
     | docs/spec/types.md | :19 の共変、:21 の「Kind」 | `UnboxedArray a` は引数に従い、`MutUnboxedArray a` はいつも `Lin`。どちらも共変 |
     | docs/spec/modules.md | :64 の Prelude の 5 番、:69 の std の一覧 | 2つの型と2つのファイルを足す |
     | docs/spec/core-ir.md | 「表の印の列」 | `Std.UnboxedArray.thaw` と、`Std.MutUnboxedArray` の `make`、`empty`、`length`、`read`、`write`、`fill`、`freeze` の行 |
     | docs/spec/lir.md | 「extern の借用」:266、:269 | 借用の一覧が配列の 10 行になる。`with_arg` の一覧に `Std.MutUnboxedArray.length` と2つの `read` の行を足す |
     | docs/spec/vm.md | 「基本操作と extern」:237 | 新しい行は `Op::Extern` で動き、Float と Int の行が `Ctx` の操作を共有する (T-6 でこの文を書き直す) |
     | docs/spec/object-model.md | :993 の extern から `Ctx` への表、:995 の単位の引数の一覧 | 19 の行を 8 つの `unboxed_*` に写す行。:995 に `Std.MutUnboxedArray.empty` を足す |
     | docs/implementation/testing.md | 「分類」 | `unboxed/` の箇条: `UnboxedArray` と `MutUnboxedArray` の組み立て、読み書き、変換、範囲外の添字 |
     | docs/implementation/testing.md | `scaling.rs` の一覧 (:598 あたり) | G-9 のテストの箇条 |
     | docs/future/native.md | 「生の要素の配列」と :29 の表の行 | 残すのは将来のことだけにする。`get`/`read`/`write` を箱なしで `base + 8 + 8*i` に展開することと、添字の範囲で並列に書くことを、新しい型の名前で書く。VM の文を消す |
     | docs/future/ffi.md | :41-66、:105、:171 | 例と対応を `UnboxedArray Float` → `const double*`、`UnboxedArray Int` → `const int64_t*`、`MutUnboxedArray Float` → `double*`、`MutUnboxedArray Int` → `int64_t*` に直す。`extern "C"` の型に型変数があれば誤りにする規則を書く (E コードは `ffi-c` で決める)。:171 はロードマップの幅の項目と合わせる |
     | docs/future/multicore.md | :396、:562 | `tabulate_unboxed : Unboxed a => ...` と `for_mut_unboxed : MutUnboxed a => ...` にし、「クラスが `Lin` の型を受けない」の文を消す。:562 の `Par.tabulate_floats` を `Par.tabulate_unboxed` にする |
     | docs/future/roadmap.md | :492 | `Par.tabulate_floats` を `Par.tabulate_unboxed` にする (段の項目 :31-33 は T-8 で消す) |

     stdlib.md の新しい節に書くこと:
     - D-1 の関数の一覧 (名前、型、実装、借用、対を返すもの)
     - 「instance は `Float` と `Int` だけで、ほかの型の instance は意味を持たない」の文
     - 要素は生の 8 バイトの語で、箱に入らない
     - `make`、`tabulate`、`set`、`thaw`、`freeze`、`length`、`read`、`write` は `Array` の文と同じに振る舞う
     - `fill` はその場で書く
     - 変換は `Panic` を起こさない
     - `tabulate` の E3006 の文
     - `e[i]`、`var` の配列の変数、`Eq`/`Ord`/`Show` はない
- 実装者に任せること: UI テストの中の具体的な値。上に期待を書いたものは、その値に合わせる
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_core_ir -p eml_hir --test integration externs` → 30 passed
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` → 新しい 14 の UI の snapshot と 1 つのトランスクリプトだけを受け入れて通る
  - `cargo nextest run -j 1 -p eml_vm --test integration -E 'test(scaling::unboxed)'` → 通る。1 秒未満
  - 関門 → すべて通り、既存の snapshot は変わらない
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` → exit 0 (`core`、`run` でも)
  - G-1 の2つ目、G-2、G-4、G-5 の1つ目のコマンド → 5.1 の期待どおり
  - `git grep -n "Kind::Raw" crates/eml_vm/src` → 空
  - `git grep -n "tabulate_floats\|for_mut_floats\|tabulate_ints\|for_mut_ints" docs` → 空
  - `git grep -n "FloatArray\|IntArray" -- docs std crates tests bench` → `docs/future/roadmap.md` の段の項目だけ
- 気をつける所:
  - `extern_call` の腕に `unsafe` を書かない
  - `import MutUnboxedArray (MutUnboxed)` の後でも、`MutUnboxedArray.*` は修飾して呼べる (試作で確かめた)
  - 期待値の位置 (`:26`、`:13`) は試作の値なので、実際の行に合わせて snapshot を確かめる

### T-6: 専用の命令を3つ足す

- 目的: よく通る読み書きで、行の番号の `match` を飛ばす (D-13)
- 対象: `crates/eml_vm/src/op.rs`、`compile/emit.rs`、`vm.rs`、`disasm.rs`、`crates/eml_vm/tests/scaling.rs`、docs/spec/vm.md
- 作業:
  1. `op.rs` の `Array*` の命令の隣に、同じ欄の型で `UnboxedGet { dst, a, b }`、`UnboxedLength { dst, a }`、`UnboxedSet { dst, at }` を足す
  2. `compile/emit.rs` の `array_op` で行を命令に写す。`length` の2行は `UnboxedLength` に、`get`/`read` の4行は `UnboxedGet` に、`set`/`write` の4行は `UnboxedSet` (operands は `[array, index, value]`) にする
  3. `vm.rs` の `step` に腕を足す
     - `UnboxedGet`: `let x = unsafe { ctx.unboxed_get(task.get(a), task.raw(b)) }?; task.set_raw(dst, x as i64);`
     - `UnboxedLength`: `let n = unsafe { ctx.unboxed_length(task.get(a)) }?; task.set_raw(dst, n);`
     - `UnboxedSet`: operands を `o` として `let w = unsafe { ctx.unboxed_set(task.get(o[0] as Slot), task.raw(o[1] as Slot), task.raw(o[2] as Slot) as u64) }?; task.set(dst, w);`。値のスロットは生の値なので、`ArraySet` と違う
  4. `step_failed` では `UnboxedGet`/`UnboxedSet` を `Purity::Panics`、`UnboxedLength` を `Purity::Pure` とする
  5. `extern_call` から 10 行の汎用の腕を外し、`VmFault::Internal("a dedicated-instruction extern reached the generic extern instruction")` の一覧に足す
  6. `disasm.rs` に `unboxed_get` (`bin` を通す)、`unboxed_length s{dst}, s{a}`、`unboxed_set s{dst} = {slots}` を足す
  7. vm.md:237 を書き直す
     - unboxed の行は3つの命令に写り、借用、巻き戻し、`Panic` の規則は `Array` と同じ
     - Float と Int の行は命令を共有し、要素はスロットの生のビットで渡す
     - `make`、`fill`、`empty`、`freeze`、`thaw` は `Op::Extern` のまま
     - 理由: よく通る読み書きが行の番号の `match` を飛ばす
  8. T-5 の scaling.rs のテストに、`let code = disassembly(&unboxed_sum(3));` の確かめを足す。`code` が `unboxed_get s`、`unboxed_length s`、`unboxed_set s` を含み、`= Std.UnboxedArray.Unboxed Float.get(`、`= Std.UnboxedArray.length(`、`= Std.MutUnboxedArray.MutUnboxed Float.write(` を含まないことを確かめる。汎用の行は `extern s{dst} = {行の名前}({引数})` と出る (disasm.rs:380)。`make` の行は残ってよいので、前方一致で否定しない
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm --test integration -E 'test(scaling::unboxed) | test(codegen::)'` → 通る
  - 関門 → すべて通る。UI の snapshot は T-5 から変わらない
- 気をつける所: `UnboxedSet` の値は `task.raw` で読む。`task.get` で読むと Float のビットを語として扱ってしまう

### T-7: spectral_norm を書き直す

- 目的: 段の動機の計測を固定する (D-6、G-8)
- 対象: `bench/spectral_norm.em`、`crates/eml_vm/tests/bench.rs`、`docs/implementation/benchmarks.md`
- 作業:
  1. `bench/spectral_norm.em` を書き直す
     - `import Array` → `import UnboxedArray`、`Array Float` → `UnboxedArray Float`、`Array.length`/`tabulate`/`make` → `UnboxedArray.*`
     - `v[j]`、`u[i]`、`v[i]` は `UnboxedArray.get` にし、オペランドの順は変えない: `s + a i j * UnboxedArray.get v j`、`vbv + UnboxedArray.get u i * UnboxedArray.get v i`、`vv + UnboxedArray.get v i * UnboxedArray.get v i`
     - `var u` と `var v` は残す
     - 頭のコメントに、ベクトルが生の要素の `UnboxedArray Float` であることを書き、benchmarks.md の「配列の `Float` の要素の箱」を引く
  2. `bench.rs` の `spectral_norm` と `unwind_points` を受け入れ直す (種類2)。`spectral_norm_allocates_14_fewer` と定数を消す (種類1)。代わりに `spectral_norm_does_not_box_elements` を足す
     - `stats_with_args("spectral_norm", &["20"], E20)` と `(&["40"], E40)` を流す。E20 と E40 は `python3 bench/numeric/spectral_norm.py 20` と `40` の出力に `\n` を付けたもの
     - `boxes <= 1` で、2つの `boxes` と `allocations` が等しいことを確かめる
  3. benchmarks.md を直す
     - :59 の表の行を `UnboxedArray Float` と `var` にする
     - :83 の消したテストの名前を `spectral_norm_does_not_box_elements` にする
     - 「配列の `Float` の要素の箱」(:854-858) を書き直す。主張は、`Array`/`MutArray` の Float の要素は書くたびに箱になり、`UnboxedArray` の Float の要素は生なので spectral-norm は要素の箱を作らない、とする。数は新しい snapshot から取る。根拠は spectral_norm.em の頭のコメントと `spectral_norm_does_not_box_elements` にする
- 実装者に任せること: なし。snapshot が G-8 を満たさないときは、G-8 を緩めずに原因を直す (D-6)
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm --test integration -E 'test(spectral_norm)'` → 通る
  - `timeout 20 cargo run -p eml_cli -- run --stats bench/spectral_norm.em` → `1.274219991`。`boxes` は 0 か 1、`allocations` は 50 以下
  - `cargo nextest run -j 1 -p eml_vm --test integration -E 'test(bench::spectral_norm)'` → どれも 1 秒未満
  - `git grep -n "allocates_14_fewer\|SPECTRAL_NORM_BOXED" -- crates docs` → 空
  - 関門 → すべて通る

### R: bench の行の大きさを変える (条件つき)

- 条件: T-8 の手順 2 で `bench/scripts.sh` を流し、spectral-norm の行の実時間が範囲を外れたときだけ行う。外れなければ飛ばす
- 作業: `bench/scripts.sh:251` の大きさを変え、`Resize bench rows` のコミットにする
- 完了の条件: `bench/scripts.sh` の spectral-norm の行が範囲に入る

### T-8: 段を閉じる (`Close raw-arrays`)

ロードマップの「進め方」の順に行う。

1. spec を確かめる: `git grep -n "raw-arrays" docs/spec` → 空。生の配列について「今は〜、後で〜」が残っていない
2. 計測する (macOS)
   1. `bench/scripts.sh` を先に流し、範囲を外れたら R を先に行う
   2. コードの最後のコミットのきれいな作業ツリーで、`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を流す
   3. benchmarks.md の「今の数値」の3つの表を入れ替え、配列を引く「所見」を確かめ直す
   4. macOS がなければ、表の入れ替えだけを直後の `Measure raw-arrays` にする
3. status.md: :28 の std の一覧に `UnboxedArray.em` と `MutUnboxedArray.em` を、:34 に2つの型 (`Float`/`Int`、生の要素) を足す
4. architecture.md: `heap/unboxed.rs` と `heap/ctx/unboxed.rs` の載せ方を確かめる。eml_rt のファイルの一覧があれば足す
5. roadmap.md
   - `raw-arrays` の項目 (:31-33) を消す
   - ほかの段の「前提」を直す: `ffi-c` は「なし」、`native-inline` は `native-aot`、`std-par` は `par-steal` にする
   - 「その後の項目」に次を足し、それぞれに「後に回した理由」と「見直す条件」を添える
     - 型システム: `closed` のクラス。`Unboxed` と `MutUnboxed` に要る。理由は、利用者の instance は使い道がないだけで、壊れることはないから。見直すのは、利用者が instance を書いて混乱したとき
     - 言語機能と構文: 新しい型への `e[i]`、`x[i] <- v`、`var` の配列の変数
     - マルチコア対応: `UnboxedArray` の `slice`/`append`。`std-par` は添字の範囲のプリミティブで足りるので、要るかはそこで決める
     - FFI と拡張 (:528-530 を書き直す): `Byte` か 32 ビットの要素は新しい `Unboxed` の instance になる。幅に依る関数は、物体の `tag` に幅を書く (R1) か、幅に依る関数をクラスに移す (R2)。使い道がまだないので後に回す
6. overview.md:49 の配列の行に2つの型を書く。README.md の「文書の地図」の「内容」の欄で native.md の行 (:39) を確かめ、T-5 で節の見出しを消したときだけ直す
7. CLAUDE.md: 規則か道しるべが変わったときだけ直す (変わらない見込み)
8. 計画を消す前に、5 の決め (D-1〜D-13) がそれぞれ `docs/spec` と `docs/future` に入っていることを確かめる。そのうえで `docs/plans/2026-11-05-raw-arrays.md` と `docs/plans/2026-11-05-raw-arrays/` を消す
9. コミットのメッセージに次を書く
   - 何をどこへ移したか
   - 6 のテストの変更の表と理由
   - 計測の目立つ差
   - 大きさを変えた行
   - 機種が変わったか
   - 後に回した項目の決めた日 (計画を承認した日)

- 完了の条件:
  - `git grep -n "FloatArray\|IntArray" -- docs std crates tests bench` → 空
  - `git grep -l "UnboxedArray" docs/future/ffi.md docs/future/multicore.md docs/future/native.md docs/future/roadmap.md` → 4 ファイル
  - `git grep -n "closed" docs/future/roadmap.md` → `closed` のクラスの項目
  - `git grep -n "raw-arrays" docs ':!docs/reports'` → 空
  - `ls docs/plans` → この計画がない
  - `cargo nextest run && cargo verus verify -p eml_rt` → すべて通り、`0 errors`

## 8. 実行の順

```
T-1 → T-2 → T-3 → T-4 → T-5 → T-6 → T-7 → (R) → T-8
```

T-2 と T-3 はどちらも mod.rs に触れるが、試作の差分では同じ hunk に入らない。並べるときは別々の worktree で行い、T-3 を T-2 の上に rebase する。既定は順に行うことである。T-4 以降は前のタスクの結果に依るので、並べられない。

## 9. 確かめの計画

- 基点 (最初の変更の前):
  - `cargo verus verify -p eml_rt` → `1152 verified, 0 errors`
  - `cargo nextest run -p eml_rt` → 814 passed
  - `cargo nextest run` → 4463 passed、21 skipped
  - `git apply --check docs/plans/2026-11-05-raw-arrays/c1c2-probe.diff` → exit 0
- 全体の確かめ (T-8 の後):
  - `cargo nextest run` → すべて通る
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → exit 0
  - `cargo verus verify -p eml_rt` → `0 errors`
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → exit 0
  - `cargo nextest run --profile timing -j 1` (x86_64 Linux があれば) → `SLOW` がない
  - 5.1 の G-1〜G-9 と G-R1〜G-R15 の確かめ → どれも期待どおり
- 利用者が手で確かめること: T-8 の計測の表 (macOS)。spectral-norm の行の実時間と、`bench/run.sh` の命令の数が大きく変わった行を見る。完了の条件の1つ目に当たる

## 10. 危険と残る論点

| ID | 論点 | 影響 | どう決めるか | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | `alloc_unboxed`、アクセサ、回復の腕、`Ctx` の 8 つの操作は試作していない。証明が重くなるおそれがある | T-3、T-4 | 実装者が証明を書くとき | 補題に rlimit を付けてよい。それでも通らなければ止めて報告する |
| Q-2 | T-5 の std は、`MutArray` の物体で行を代わりに動かす腕で確かめた。本物の `Raw` の物体が VM を通るのは T-5 が初めてである | T-5 | T-5 の `par/`、`async/`、REPL のテストで確かめる | テストが落ちたら eml_rt の側を疑い、T-3/T-4 に戻って直す |
| Q-3 | `known_kind` を範囲で書いても、後の変更で別の補題が揺れることがある | T-3〜T-8 | 揺れが出たとき | その補題に rlimit を付け、理由をコメントに書く |
| Q-4 | `unboxed_fill` が速いモードで共有した配列を受けたとき、写すか、requires にとどめるか | T-4 | 実装者 | 写す (`relin` と同じ)。検査モードはどちらでも `NotUnique` |
| Q-5 | UI テストの位置 (`:26`、`:13`) は試作の値である | T-5 | snapshot を受け入れるとき | 実際の行に合わせる。メッセージの文が違えば止めて報告する |

## 付録: レビューで変わった主なこと

蒸し返さないために要る分だけを書く。

- 名前と型の形
  - 単相な4つの型 (`FloatArray` など) から、多相な `UnboxedArray a`/`MutUnboxedArray a` に変えた (D-1)
  - `Bytes` の改名と族への編入は、検討して退けた (D-2)
- この段で入れる関数
  - `slice`/`append` (`copy_slice`/`concat`) と範囲つきの `fill` は、使う所がないので外した。`std-par` は添字の範囲のプリミティブで足りる
  - `IndexFault::ArraySlice` と、凍結した領域の生の配列も外した
- 試作で確かめたこと
  - extern に結んだ instance を Prelude でない std のモジュールに置く形と、`borrowed`/`with_arg` は、コンパイラを変えずに動いた
  - 2つのクラスがどちらも `make` を持つ形も解決した
  - T-2 の部分と T-3 の述語の部分は、Verus (1153 verified、0 errors) とワークスペース全体のテストで確かめた
  - `known_kind` を選言で書くと share.rs の補題が落ちることも確かめた
- VM の専用の命令
  - native.md:119 の「命令を作らない」は、操作ごとの測定 (1 回約 60 命令) で改めた
  - 差分を測れるよう、T-5 と T-6 に分けた
- native.md の直し方
  - 1 回にまとめる案から、実装したコミットごとに削る形に変えた。docs が今の状態を書くという規則に合わせるためである
- 照合で見つかった食い違いの直し
  - multicore.md:562 と roadmap.md:492 の `tabulate_floats` を T-5 の対象に足した
  - vm.md:232 の空の `MutArray` の文を T-2 の対象に足した
  - G-6 と G-R15 の確かめのコマンドは、spec fn に当たらない形に直した
  - T-4 に `heap/tests/externs.rs` の `call` の腕を足した
  - 直接の `make` の負の長さの UI テストを足した

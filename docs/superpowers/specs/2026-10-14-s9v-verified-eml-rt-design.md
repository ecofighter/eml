# S9v `eml_rt` を Verus で検証した1つの crate にする

位置づけ: 作業の設計。S9a と S9b の間に入れる作業を定める。S9v を終えたら、決まったことを `docs/spec/object-model.md`、`docs/implementation/testing.md`、`docs/future/roadmap.md` に移し、この文書を削除する。

## 目的

S9a の `eml_rt` は、純粋な関数と `share` のたどりだけを、`crates/eml_rt/verus/` に置いた写しで Verus に証明させている。写しと本体が同じコードであることは、`verus_copies.rs` の文字列の照合で確かめる。`unsafe` の核 (確保器、ヘッダとフィールドの読み書き、RC、解放の連鎖、reuse と token、検査モード) は証明の外にあり、Miri の全体のテスト (約 18 分と約 23 分) で確かめている。

S9v では、次の3つを行う。

- vstd への依存を受け入れ、本体のコードを `verus!` の中に書く。写しと照合をやめ、`cargo verus verify -p eml_rt` が crate 全体を検証する
- 証明の範囲を `unsafe` の核まで広げる。生のポインタの読み書きは、どれも Verus の権限 (`PointsTo`) の上で行う
- 証明できない部分を、名前を挙げた信頼の一覧にまとめる。Miri はその部分だけを確かめる。今後の FFI の境界も、この一覧に入る。今の `Foreign` の物体 (Rust の値と破棄処理を持つ物体) が FFI の前身なので、Miri のテストはそこを重点にする

基盤は、この設計の前にコミット 6c6aafa で入れた。nixpkgs は nixos-unstable を追い、Rust の toolchain は stable も nightly も fenix から取る。stable は Verus が求める 1.98.1 に揃える。Verus は release の zip を flake の input にし、`nix/verus.nix` が devShell に入れる。

`eml_rt` はまだパイプラインから使わない。UI テストの出力と `bench/` の仕事の回数は変わらない。

完了の条件は次の6つである。

- `cargo verus verify -p eml_rt` が 0 errors で通る
- 信頼の一覧のテスト (下の「信頼の一覧」) が通る。`verus!` の外の `unsafe` と、一覧にない信頼の項目がない
- `eml_rt` の単体テストが、速いモードと検査モードの両方で通る。既存のテストと UI テストが、変えずに通る
- Miri が、信頼の部分のテスト (`cargo miri test -p eml_rt trusted::`) を、`-Zmiri-strict-provenance` の設定と、それに `-Zmiri-tree-borrows` を足した設定の両方で通す。かかった時間を記録する
- `bench/rt.sh` の速いモードの命令の数が、S9a の記録 ([計測の基準](../../implementation/benchmarks.md) の「S9a (`eml_rt` の核)」) より増えない。増えたら原因を調べ、直せないものは理由を spec に足して相談する
- `cargo clippy --all-targets` と `cargo fmt --check` が通る

## 段の位置

S9v は S9a の後、S9b の前に置く。S9b の extern の語の ABI は、FFI の最初の境界になる。境界を信頼の一覧と Miri のテストに足す決まりを、S9b より前に決めておくためである。段の列の S9 の1行は変えず、S9 の作業の表に S9v の行を足す。

## 証明すること

どれも、値の全域と任意の大きさのヒープについて成り立つ。公開の API の前提 (下の「API の約束」) の下で示す。

1. メモリの安全: 検証したコードの生のポインタの読み書きは、どれもヒープが持つ権限で行う。権限は vstd の `allocate` が作り、ページを返すときに `deallocate` に戻す。解放の後の使用、二重の解放、ブロックの外の読み書き、書いていない語の読み込みは起きない
2. ヒープの形の不変条件: ページの一覧、級ごとの空きリスト、切り出しの位置、各ブロックの権限が食い違わない。ブロックの番地の下位 16 ビットを落とすと、そのブロックを含むページの頭になる。この不変条件は `Heap` の型の不変条件にし、公開のどの操作も保つ
3. RC の正しさ: 各物体の rc は、抽象のモデル (下の「抽象のモデル」) の参照の数と対応する。物体を空けるのは、張り付いていない物体の参照がなくなったときだけで、そのときは必ず空ける。解放の連鎖は、子の参照を1つずつ手放す。この性質から、生きている物体のフィールドが指す物体はどれも生きている (1 のメモリの安全に要る)
4. `share`: 今の `share_walk` の証明を、ヒープの上の `Graph` の実装と結ぶ。`share` は、`share_walk` の結論をヒープの抽象のモデルの言葉で満たす
5. `release` と reuse: `release` と `release_reuse` は、抽象のモデルの上で、子を手放す一般の版と同じ結果になる。部品を並べた展開 (`is_last` と `free_block`) も同じ結果になる。token の箱は、`con_at` か `drop_token` で1回だけ使う
6. 検査モード: 速いモードより弱い前提 (下の「検査モードの約束」) の下でメモリが安全である。確かめられる前提がすべて成り立つときにちょうど `Ok` を返し、そのときの結果は速いモードと同じである。`Err` は、その誤りに対応する違反があったときだけ返す
7. S9a の5つの性質 (ヘッダの詰め方、即値、RC の遷移、`share_walk`、`release` と reuse の判定) を引き継ぐ。ヘッダの権限が値を持つので、S9a で信頼していた「メモリから読んだ `Meta` が正しい形であること」は、証明する側に移る

## 抽象のモデル

ヒープの見方 (`Heap` の `view`) は、ブロックの番地からブロックの抽象の値への写像である。ブロックの値は次のものを持つ。

- 状態: 生きている、token の箱、空いた (検査モードだけが空いたブロックを残す)
- rc、`meta`、ペイロードの走査する語の列、生の語の列 (書いたかどうかを含む)
- 検査モードの前置き (所有者 ID か不死の物体の数、状態、世代の番号)

参照の数は、ヒープの中の辺の数と、外の参照の数の和である。ヒープの中の辺は、生きている物体の走査する語のうち、その物体を指すものである。外の参照の数は、ヒープの外 (VM のレジスタや Rust の変数) が持つ、数えた参照の数で、ghost の値として `Heap` が持つ。外の呼び出し側は検証しないので、この数は API の約束の言葉としてだけ意味を持つ。

- `alloc` は外の参照を1つ作る。`dup` は1つ増やし、`decref` は1つ減らす
- `set_field` は、書く語の外の参照を1つ辺に移し、上書きされる語の辺を外の参照に移す。所有の受け渡しを呼ぶ側が決める、という今の約束をこの形で書く
- `release` で残すフィールドは、辺から外の参照に移る
- 借りた参照 (Perceus の借りたフィールドと、基本操作の借用の引数) は数えない。読むだけの操作の前提は、外の参照を持つか、生きている物体のフィールドから読んだ語であることにする

張り付いた数 (局所の `MAX` と、`DROP_LIMIT` より小さい共有の数) と不死の物体 (rc 0) の扱いは、今の `rc.rs` の証明の理想のモデル (rc、本当の参照の数、張り付きの旗) をそのまま使う。

共有の物体も、今はこの `Heap` の見方に入れる。ほかの worker が並行に数えることは、まだ検証の対象にしない (下の「信頼の一覧」の T2)。今の検証は、すべての参照がこの `Heap` の操作を通るとして行う。VM の並列化のときに、Verus の並行の証明に置き換える。

## API の約束

公開の操作の形 (`unsafe fn`、引数、`Result`) は変えない。各操作の `# Safety` の文は、同じ条件を `requires` にも書き、文と `requires` が同じ条件を指すようにする。[オブジェクトモデル](../../spec/object-model.md) の「核の操作」の表が、そのまま `requires` の一覧になる。

S9a では確かめていなかった前提も、`requires` に書く。例えば `raw` は、読む生の語を先に書いてあることを前提にする。今は文書にだけある条件で、検査モードも確かめない。

`Heap` の内部の関数は、権限を引数で受け渡すか、`Heap` の ghost の欄から取り出して戻す。単体テストは公開の API と内部の関数を今と同じように呼ぶ。ghost の引数を取る内部の関数は、テストでは `Tracked::assume_new()` を渡すか、公開の API を通して呼ぶ。

## 検査モードの約束

検査モードは、空けたブロックを使い回さず、権限を `Heap` に持ったままにする。そのため、検査モードの前提は「語は即値か、このヒープが確保したことのあるブロックを指す」まで弱くできる。解放の後の使用と token の二重の使用は、この前提の下で前置きを読んで見つける。

各操作の `requires` は、モードで分ける (`M::CHECKED ==> 弱い前提`、`!M::CHECKED ==> 速いモードの前提`)。検査モードでも確かめない前提 (書いていない生の語を読まないことなど) は、どちらのモードの前提にも残す。

## 信頼の一覧

証明しない部分を、名前を挙げて次の表にまとめる。`external_body`、`assume_specification`、`verifier::external` の項目は、どれもこの表のどれかに属する。

| # | 中身 | 理由 | Miri で確かめること |
|---|---|---|---|
| T1 | vstd の `allocate` と `deallocate` (中身は `std::alloc`) | OS と確保器の境界 | ページの確保と返却、64 KiB の揃え、大きい物体のページ、`Heap` を落とすときの返却 |
| T2 | rc と前置きの atomic の読み書き。欄は `i32` と `u32` で、`AtomicI32::from_ptr` で Relaxed と AcqRel の操作をする。約束は、権限を1つのスレッドが持つときの逐次の値である | vstd の atomic は SeqCst だけを扱う。核では SeqCst を使わない | 局所と共有の `dup` と `decref`、`is_last` の2回の読み込み、検査モードの前置きの書き換え |
| T3 | `Foreign` の物体の中身。破棄処理の表 (関数のポインタ)、`TypeId` の照らし合わせ、`drop_in_place` | Verus は関数のポインタと `TypeId` を扱わない | 型の違う照らし合わせ、`Drop` を持つ値 (`Box`、`String`、`Vec`)、大きさ 0 の型、破棄処理が panic する場合、解放と `Heap` の破棄での破棄処理の呼び出しが1回だけであること |
| T4 | `verus!` の外の `Drop` の実装 (`Heap`、`Pages`) | Verus は利用者の `Drop` を扱わない。中身は検証した関数を呼ぶだけにする | T1 と T3 のテストが通る道 |
| T5 | 証明に使う std の約束のうち vstd にないもの。今は `<[T]>::contains` と、大きさの桁あふれで止める panic の関数 | vstd の std の約束が足りない | なし (値の約束で、メモリに触れない) |
| T6 | 64 ビットの環境 (`global size_of usize == 8`)、ヘッダの配置 (`global layout`) | Verus は `global` の宣言をコンパイル時に照らす。本体の `const` の assert もほかの環境のビルドを止める | なし |
| T7 | Verus、Z3、rustc、vstd の std の約束 | 道具そのもの | なし |

FFI の境界 (S9b の extern の語の ABI と、その後の外の関数の呼び出し) を足すときは、この表に行を足し、Miri のテストを `trusted` の module に足す。

信頼の一覧は単体テストで固定する。テストは `eml_rt` の各ファイルから `external_body`、`assume_specification`、`verifier::external`、`assume(`、`admit(` を探し、見つけた項目が、テストの中の一覧と同じであることを確かめる。`verus!` の外のコードに `unsafe` がないことも確かめる (`unsafe impl Send` と T4 の `Drop` を除く)。S9a の `the_copies_trust_nothing_but_the_contains_spec` の役を、このテストが引き継ぐ。

`verus!` の外に置くのは、`fmt` の実装、T4 の `Drop`、`unsafe impl Send`、`Sync` でないことの型の assert、テストだけにする。どれも検証した関数を呼ぶか、`unsafe` を使わない。

## Verus に合わせるための書き換え

動作を変えずに、次のように書き換える。下書きの段階 (spike) で、どれも Verus が受け付けることを確かめた。

- 内部では `NonNull<Header>` の代わりに `*mut Header` を使う。vstd は `NonNull` に約束を持たないためである。番地の計算は `addr` と `with_addr` で書き、strict provenance を保つ
- rc と前置きの欄は `AtomicI32` と `AtomicU32` でなく、`i32` と `u32` にする。atomic の操作は T2 の関数が `from_ptr` を通して行う。配置は変わらない
- 空の enum (`Fast`、`Checked`) は Verus が受け付けないので、中身のない struct にする。`Heap` を `Sync` にしない印は、`PhantomData<Cell<()>>` から `PhantomData<*mut ()>` に変える。`Send` は今と同じく `unsafe impl` で足す
- 大きさの桁あふれで止める `expect(TOO_LARGE)` は、T5 の panic の関数を呼ぶ形にする
- `mem::take` は `mem::swap` にする (vstd が約束を持つ)
- 閉包を受ける `for_each_block` は、使う所ごとのループに分けるか、検証できる形に直す
- ページの頭の計算 (`addr & !(PAGE_BYTES - 1)`) は、bit_vector の補題で、ブロックを含むページの頭に等しいことを示す

公開の API で変わるのは、`Fast` と `Checked` が値を持てる struct になることだけである。

## Miri

Miri は、信頼の部分を通るテストだけを流す。テストは `crates/eml_rt/src/heap/tests/trusted.rs` (module `trusted`) に置き、上の表の「Miri で確かめること」を1つずつ確かめる。

```sh
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_rt trusted::
```

全体の単体テストを Miri で流すことは、決まった手順から外す。単体テストは `cargo test` で流し、メモリの安全は Verus の証明が受け持つ。

## 証明の進め方

下から順に、段ごとにコミットする。どの段の終わりでも、`cargo test`、`cargo verus verify -p eml_rt`、clippy、fmt が通る。まだ証明していない部分は、一時的に `external_body` にし、信頼の一覧のテストの「未証明」の欄に載せる。S9v の終わりには、この欄を空にする。

1. crate の準備: vstd の依存、`[package.metadata.verus]`、写しの証明 (`meta`、`word`、`rc`、`share_walk`、`check` の判定) を本体に移す。`crates/eml_rt/verus/` と `verus_copies.rs` を消す。信頼の一覧のテストを置く
2. 語、ヘッダ、ペイロードの語の権限、T2 の atomic の関数、ページの確保器
3. `Heap` の型の不変条件と抽象のモデル、確保、`dup`、`decref`、解放の連鎖、フィールドと生の語の読み書き、`is_unique`、`is_last`、`free_block`、`unbox_int`
4. `release`、`release_reuse`、token と `con_at`、`reuse_in_place`、`drop_token`
5. ヒープの上の `share`
6. 検査モード: 前置き、隔離、token の表、各検査、生存の列挙、不変条件の検査、リーク
7. `Foreign` (T3)、`Drop` (T4)、Miri のテスト、計測、文書

段の中の細かな順序は、計画で決める。

## テストの変更

- 成否の変更 (テストの削除): `crates/eml_rt/src/verus_copies.rs` の7つのテストを消す。写しがなくなり、照合するものがなくなるためである
  - `the_verus_copies_match_the_code`
  - `verus_only_syntax_is_removed`
  - `trait_specs_and_spec_fns_are_removed`
  - `items_in_an_impl_are_found_by_type_and_name`
  - `a_changed_body_is_reported`
  - `calls_are_found_with_or_without_a_space`
  - `the_copies_trust_nothing_but_the_contains_spec` (役は信頼の一覧のテストが引き継ぐ)
- 期待値の変更: なし
- 機械的な追随: 内部の関数の形が変わる単体テスト (`alloc.rs` の確保器のテストなど) と `examples/rt_bench.rs` は、呼び方だけを直す。`assert` の値は変えない
- 足すテスト: 信頼の一覧のテストと、`trusted` の module の Miri のテスト

## 文書

S9v の終わりに、次のように移す。

- `docs/spec/object-model.md` の「`unsafe` の核」: 核を Verus で検証すること、信頼の一覧、FFI の境界を足すときの決まり
- `docs/implementation/testing.md` の「`eml_rt` の検証」: 写しの節を、本体の検証、信頼の一覧のテスト、Miri の `trusted::` に書き換える
- `docs/future/roadmap.md`: S9 の作業の表に S9v の行を足す
- `docs/implementation/benchmarks.md`: `bench/rt.sh` の新しい記録
- `CLAUDE.md`: `cargo verus verify -p eml_rt` と Miri の手順

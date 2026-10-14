# S9b-2b 実行文脈 `Ctx` と基本操作 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_rt` に実行文脈 `Ctx<M>` と、セッションに1つの実行の制御 `Control` を足し、evidence passing の19個の基本操作を `Ctx` のメソッドとして作って、動作まで Verus で証明する。

**Architecture:** 6つのタスクで積む。
- Task 1: 核の操作の約束を強める。証明だけで、実行するコードは変えない
- Task 2: `Control` と、確保器のページの数え方
- Task 3: `Ctx` の骨組み。yield の状態、yield の8つの操作、`poll`、`check_finished`
- Task 4: `evv` と evidence の spec の関数と、evidence の11の操作
- Task 5: `evv_mask`、EP の形の筋書き、数え方のテスト、Miri、計測の追加
- Task 6: 計測の記録と文書

**Tech Stack:** Rust (edition 2024、fenix の 1.98.1)、Verus 0.2026.10.04、vstd 0.0.0-2026-10-04-0306、verusfmt、Miri (fenix の nightly)、macOS の `/usr/bin/time -l`。

**Spec:** `docs/superpowers/specs/2026-10-14-s9b2b-ctx-primitives-design.md`
- 実装する人は、各タスクの前に spec の該当する節を読む
- spec が規範である。計画とずれたら spec に従い、ずれを報告する
- spec を書く前の敵対的なレビューの記録は、`/private/tmp/claude-501/-Users-arakaki-Projects-eml/48290583-2b6b-4503-89ab-8fa5db74ab2f/scratchpad/s9b2b/` の `review-safety.md` と `review-fitness.md` にある。spec の各決定の根拠と、コードの行番号の手がかりがある (行番号は今のコードで確かめる)。`probe/` には、燃料の付いた再帰のリストの spec が Verus で通ることを確かめた試作がある

## Global Constraints

- 各タスクの終わりに次が通る。途中のタスクでもワークスペースを壊したままコミットしない
  - `cargo test` (ワークスペース全体)
  - `cargo verus verify -p eml_rt` が 0 errors。`target/**/.fingerprint/eml_rt-*` を消してから流し (zsh では、合わない glob で命令全体が止まるので `find target -path '*/.fingerprint/eml_rt-*' -prune -exec rm -rf {} +` を使う)、最後の行の `verification results:: N verified, 0 errors` をコミットメッセージに書く
  - `cargo clippy --all-targets` が警告を出さない
  - `cargo fmt --check` と `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`。新しいファイルは `git add` してから流す。verusfmt は `unsafe { .. }?` を読めない。verusfmt は、コメントと文の間の空行を戻すことがあるので、コメントは文の直前に置くか、`if` の本体の中に置く
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、それに `-Zmiri-tree-borrows` を足したもの
  - `bench/rt.sh` の今ある8つの計測のどれも、`docs/implementation/benchmarks.md` の「S9b-2a (凍結した領域)」の値から +0.5% を超えて増えない (Task 2 と Task 5 の終わりで測る。ほかのタスクは速い道に触れないことを差分で確かめる)
- 検証が通っても `cargo build` が通るとは限らない。どのステップでも両方を流す
- `eml_rt` の外の crate は変えない (UI テストと `bench/` のスナップショットは変わらない)。`bench/rt.sh` と `crates/eml_rt/examples/rt_bench.rs` は変えてよい
- `unsafe` は、信頼の一覧の項目の中と、`verus!` の外の `unsafe impl Send` / `Sync` と T4 の `Drop` にだけ書く。信頼の一覧の新しい項目は、T1 か T7 の行に入れて `trust.rs` の `TRUSTED` に足す。新しい T の番号は作らない
- `assume`、`admit`、新しい `external_body` を証明のために足さない。`external_body` を足してよいのは、Task 2 のページの数の加減 (T1) だけである
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則は `docs/` のパスと見出しで引く。この計画と spec は Task 6 で消すので引かない。`eml_cli` の `citations::every_cited_heading_exists` は引いた見出しが在ることを確かめるので、新しい節「実行文脈 `Ctx`」を書く Task 6 より前は、今ある見出し (`docs/spec/lir.md` の「基本操作」「実行の文脈」「evidence」、`docs/spec/object-model.md` の「evidence の3つの物体」「核の操作」など) を引く。コメントに計画のタスクの番号を書かない
- コミットメッセージは英語で、既存の形 (命令形の1行目、本文に理由) に合わせる。各コミットの末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014bwniBqkZgCLERCaKyDB5S
  ```

- `git diff` は `--no-ext-diff` で見る。sed は GNU (`sed -i` に `''` を付けない)
- コマンドはすべて devShell の中で流す (direnv の入ったシェルか `nix develop -c ...`)

## 名前 (各タスクで共通に使う)

- 実行の制御: `crates/eml_rt/src/control.rs` の `pub struct Control`、`pub struct Limits { pub max_bytes: usize, pub max_depth: usize, pub max_evv: usize }` (`Default` はどれも `usize::MAX`)、`pub enum Stop { Interrupted, DepthLimit, MemoryLimit }`。止める理由のビットは `Control::INTERRUPT: u32 = 1` と `Control::MEMORY: u32 = 2`
- `Control` のメソッド: `pub fn new(limits: Limits) -> Control`、`pub fn interrupt(&self)`、`pub fn clear(&self)`、`pub fn limits(&self) -> Limits`、`pub fn bytes(&self) -> usize`、`pub fn stop_bits(&self) -> u32`
- `Heap` の側: `Heap::with_control(control: Arc<Control>) -> Heap<M>`、`Heap::control(&self) -> &Arc<Control>`。`Heap::new()` は `Heap::with_control(Arc::new(Control::new(Limits::default())))` と同じである
- 実行文脈: `crates/eml_rt/src/heap/ctx.rs` の `pub struct Ctx<M: Mode>`、`pub struct CtxStats { pub evidence_steps: u64, pub yields: u64, pub yield_frames: u64 }`、`pub(crate) struct YieldState` (`#[repr(C)]`、欄 `target`、`clause`、`cont: Word`、`abort: bool`)
- `Ctx` の安全なメソッド: `new(control: Arc<Control>) -> Ctx<M>`、`heap(&self) -> &Heap<M>`、`poll(&self, depth: usize) -> Result<(), Stop>`、`check_finished(&self) -> bool`、`stats(&self) -> CtxStats`、`yielding(&self) -> i64`、`yield_status(&self, ev: Word) -> i64`、`yield_cont_take(&mut self) -> Result<Word, HeapError>` (ヒープに触れないので安全にする。yield 中でなければ、検査モードは `YieldMisuse` を返し、速いモードは欄の値 (`()`) を返す)
- `Ctx` の `unsafe` なメソッド (結果はどれも `Result<_, HeapError>`): `heap_mut(&mut self) -> &mut Heap<M>` (これだけは `Result` でない)、`evv_get() -> Word`、`evv_set(l: Word) -> ()`、`evv_push(l: Word, e: Word) -> Word`、`evv_mask(effects: &[u16], w: Word) -> Word`、`ev_new(effect: u16, n_ops: usize, state: Word, outer: Word) -> Word`、`ev_set_clause(ev: Word, op: usize, c: Word) -> ()`、`ev_renew(ev: Word, state: Word) -> Word`、`ev_lookup(effect: u16) -> Word`、`ev_clause(ev: Word, op: usize) -> Word`、`ev_evv(ev: Word) -> Word`、`ev_take_state(ev: Word) -> Word`、`ev_set_state(ev: Word, s: Word) -> ()`、`yield_to(ev: Word, clause: Word, k: Word) -> ()`、`yield_abort(ev: Word, clause: Word, k: Word) -> ()`、`yield_cont_set(k: Word) -> ()`、`yield_finish() -> Word`
- 関数の値 `&f` は値が関数の番号の即値なので、関数 0 の値と `()` は同じ語である ([オブジェクトモデル](../../spec/object-model.md) の「語」)。そのため、入口の欄や κ の欄が「空 (`()`)」かを確かめる検査モードの確かめは、関数 0 の値を空と見て見逃しうる。誤って断ることはない (空でない欄を空と見るだけで、空の欄を空でないと見ることはない)。これは受け入れ、Task 6 で文書に書く。テストでは、空でない κ や入口に、物体か `Word::imm(1)` 以上の即値を使う
- spec の関数 (`heap/ctx.rs`、`pub closed`): `ev_shape(e)`、`evv_path(w, fuel)`、`evv_seq(w)`、`evv_wf(w)`、`mask_spec(effects, seq)`、`held_ok()`。`Ctx::wf()` は `self.heap.wf() && self.held_ok()` と yield の状態の不変条件である
- 新しい誤り: `HeapError::BadEvidence`、`HeapError::NoHandler`、`HeapError::YieldMisuse`
- テスト: `crates/eml_rt/src/heap/tests/ctx.rs` (新しいファイル、`both_modes!`)、`crates/eml_rt/src/heap/tests/control.rs` (新しいファイル)、`heap/tests/trusted.rs` の `// 実行文脈` の節

## Review Focus

- 操作が 255 個以上あるエフェクトの入口の表では、先頭に数の語がある。`ev_set_clause` と `ev_clause` は、欄の位置を1つずらして正しい欄を読み書きする (Task 4 のテスト `entry_tables_with_a_count_word`)
- `ev_set_state` で元の状態を手放すとき、元の状態から ev 自身に届く輪がある (`lir.md` の「evidence」の例)。ev を呼ぶ側が持っている限り、ev は生き残る (Task 4 のテスト `replacing_a_state_that_reaches_its_evidence`)
- 別のスレッドが `Control::interrupt()` を呼んだら、次の `poll` は `Stop::Interrupted` を返す。`clear` の後は `Ok` に戻る (Task 2 と Task 3 のテスト、Task 5 の Miri)
- 確保量の上限を超えるページを取ると `MEMORY` が立ち、`poll` は `Stop::MemoryLimit` を返す。ページを返して下回っても、`clear` するまでビットは立ったままである (Task 2 のテスト `memory_limit_sets_the_stop_bit`)
- yield 中や `evv` が空でないまま `Ctx` を落としても、どちらのモードでも panic も漏れもない (Task 3 のテスト `dropping_a_ctx_mid_yield`)

---

### Task 1: 核の操作の約束を強める

基本操作の証明は、ヒープの核の操作の組み合わせである。その証明に要る約束を、核の操作の ensures に足す。中で証明済みの事実を外に出すだけで、実行するコードは変えない (spec の「ヒープの側に足すこと」)。

**Files:**
- Modify: `crates/eml_rt/src/heap/mod.rs` (`decref`、`alloc_data`、`alloc`、`alloc_immortal`、`alloc_int`、`dup`、`set_field`、`set_raw`、`acquire_immortal`、`attach`、`set_worker` の ensures、新しい公開の補題)
- Modify: `crates/eml_rt/src/heap/release.rs` (`release`、`free_block` の ensures)、`crates/eml_rt/src/heap/reuse.rs` (`release_reuse` の ensures)、`crates/eml_rt/src/heap/string.rs` (`alloc_string`、`string_push`、`string_concat` の ensures)
- Modify: 必要なら `heap/proof.rs`、`heap/safe.rs` (補題を `pub(crate)` から外に出す、補題を足す)

**Interfaces:**
- Produces (どれも `Heap<M>` の上の約束)
  - `pub closed spec fn kept_live(&self, old: Heap<M>) -> bool`: 呼ぶ前と後の両方で生きている (`settled_state` が `Live`) ブロックは、`meta`、走査する語、生の語、文字列のバイトを変えない。spec の「`decref`、`release`、`release_reuse` の ensures に足す」約束で、モードによらない
  - `pub closed spec fn words_kept(&self, old: Heap<M>) -> bool`: `forall|w| old.word_in(w) ==> self.word_in(w)`。検査モードの `&mut self` の操作の ensures に `M::CHECKED ==> final(self).words_kept(*old(self))` で足す
  - `pub proof fn lemma_readable_scanned(&self, w: Word, i: int)`: `consistent`、`settleable`、`readable(w)`、`0 <= i < count_of(w)` から `readable(scanned_word(w, i))` を出す (`field` の ensures にすでにある事実の証明だけの形)
  - `alloc_data` の ensures に、`tag_of(w) == tag`、`lin_of(w) == lin`、`forall|i| 0 <= i < scanned ==> scanned_word(w, i) == Word::imm(0)` を足す (`tag_of` がなければ `meta_of(w).tag_spec()` で書く)

- [ ] **Step 1: 何を足すかを確かめる**

次を読んで、足す約束の形を決める。
- `heap/proof.rs` の `Core::evolved`: 不死か、外の参照を持ち続ける物体の中身だけを保つ。外の参照を持たないリストの後ろの節点について何も言わないのが、spec の言う欠けである
- `decref` の ensures: `heap/mod.rs` の `pub unsafe fn decref`
- 解放の連鎖が生き残る物体の欄に書かないことを示している所: `release.rs`、`chain.rs`
- 検査モードの `word_in` の単調性がすでに証明されている所: `safe.rs` の `lemma_*`、`proof.rs`

`kept_live` は、`decref` の連鎖の後で生きているブロックが連鎖の前から生きていて中身を変えていないことである。連鎖は生き残る物体のスロットに書かず、何も確保しない。

- [ ] **Step 2: 約束を足して証明を通す**

- `decref`、`release`、`release_reuse`、`free_block` の ensures に `final(self).kept_live(*old(self))` を足す。モードによらず、前提 (`decref_ok` など) が成り立たないときも足す。検査モードの連鎖が途中で `Err` を返す場合も、生き残るブロックは書き換えていないので成り立つ。成り立たない道が見つかったら、その道を報告し、ensures を `r is Ok ==> …` に弱める
- 検査モードの `&mut self` の操作 (`alloc`、`alloc_immortal`、`alloc_data`、`alloc_int`、`alloc_string`、`alloc_string_immortal`、`alloc_foreign`、`dup`、`decref`、`release`、`release_reuse`、`free_block`、`set_field`、`set_raw`、`acquire_immortal`、`string_push`、`string_concat`、`attach`、`set_worker`、`into_token`、`con_at`、`reuse_in_place`、`drop_token`) の ensures に `M::CHECKED ==> final(self).words_kept(*old(self))` を足す。検査モードはブロックを使い回さない (隔離する) ので成り立つ
- `lemma_readable_scanned` を足す
- `alloc_data` の ensures を足す

実行するコードを変えないことを、`git diff --no-ext-diff` で `proof`、`ensures`、spec の関数、補題の外に変更がないことで確かめる。

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt`
Expected: 0 errors

- [ ] **Step 3: 約束を使う小さい証明を足す**

足した約束が実際に使える形かを、`heap/proof.rs` の末尾に置く証明だけの関数で確かめる。次の2つを書く。
1. 2つの節点のリスト `a -> b -> ()` で、`a` だけが外の参照を持つとき、ほかの物体 `x` への `decref` の後も `b` の中身が変わらない (`kept_live` から)
2. 検査モードで、`alloc_data` の後も前の語が `word_in` である (`words_kept` から)

関数は、`requires` に状況を書き、`decref` と `alloc_data` を呼ぶ exec の関数として書いてよい (テストから呼ばない。`#[allow(dead_code)]`)。Task 4 の証明がこの形を使う。

Run: `cargo verus verify -p eml_rt`
Expected: 0 errors

- [ ] **Step 4: 全体を確かめてコミットする**

Global Constraints の全体の確かめを流す (計測は差分で速い道に触れないことを確かめる)。

```bash
git add crates/eml_rt
git commit -m "Promise that releases keep surviving objects and checked words stay valid"
```

---

### Task 2: `Control` と確保器のページの数え方

**Files:**
- Create: `crates/eml_rt/src/control.rs`
- Modify: `crates/eml_rt/src/lib.rs` (`mod control;`、`pub use control::{Control, Limits, Stop};`、型の assert `Control: Send + Sync`)
- Modify: `crates/eml_rt/src/alloc.rs` (`Pages` に `control: Arc<Control>` の欄、ページを取る所と返す所と `Drop for Pages` での数の加減)
- Modify: `crates/eml_rt/src/heap/mod.rs` (`Heap::new`、`Heap::with_control`、`Heap::control`)、`crates/eml_rt/src/frozen.rs` (ページを移すときに数を変えないこと)
- Modify: `crates/eml_rt/src/trust.rs` (`FILES` に `control.rs`、`TRUSTED` の T1 に数の加減の `external_body`)
- Create: `crates/eml_rt/src/heap/tests/control.rs`。Modify: `heap/tests/mod.rs` (`mod control;`)、`heap/tests/trusted.rs`

**Interfaces:**
- Produces: 「名前」の節の `Control`、`Limits`、`Stop`、`Heap::with_control`、`Heap::control`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_rt/src/heap/tests/control.rs` を作る。

```rust
//! 実行の制御 (docs/spec/object-model.md の「確保と解放」)。

use std::sync::Arc;

use crate::{Control, Heap, Limits, Mode};

const PAGE_BYTES: usize = 0x1_0000;

fn heaps_of_one_session_add_up_their_pages<M: Mode>() {
    let control = Arc::new(Control::new(Limits::default()));
    let before = control.bytes();
    let a = Heap::<M>::with_control(control.clone());
    let b = Heap::<M>::with_control(control.clone());
    assert_eq!(control.bytes(), before + a.bytes_in_use_pages() + b.bytes_in_use_pages());
    drop(a);
    drop(b);
    assert_eq!(control.bytes(), before);
}

fn memory_limit_sets_the_stop_bit<M: Mode>() {
    let control = Arc::new(Control::new(Limits {
        max_bytes: 3 * PAGE_BYTES,
        ..Limits::default()
    }));
    let mut heap = Heap::<M>::with_control(control.clone());
    assert_eq!(control.stop_bits(), 0);
    // 大きい物体を1つずつ取り、上限を超えるまでページを足す
    let mut held = Vec::new();
    while control.stop_bits() & Control::MEMORY == 0 {
        held.push(heap.alloc_data(0, 0, PAGE_BYTES / 8, false));
    }
    assert!(control.bytes() > 3 * PAGE_BYTES);
    for w in held {
        unsafe { heap.decref(w) }.unwrap();
    }
    drop(heap);
    // 下回っても、clear するまでビットは立ったままである
    assert_ne!(control.stop_bits() & Control::MEMORY, 0);
    control.clear();
    assert_eq!(control.stop_bits(), 0);
}

fn interrupt_and_clear<M: Mode>() {
    let control = Control::new(Limits::default());
    control.interrupt();
    assert_eq!(control.stop_bits(), Control::INTERRUPT);
    control.clear();
    assert_eq!(control.stop_bits(), 0);
}

fn frozen_chunks_stay_counted_until_dropped<M: Mode>() {
    let control = Arc::new(Control::new(Limits::default()));
    let mut b = Heap::<M>::with_control(control.clone());
    let _ = b.alloc_string_immortal("constant");
    let region = b.freeze().unwrap();
    // 作り手のヒープは消えたが、塊のページは塊が生きている間は数に入る
    assert!(control.bytes() > 0);
    drop(region);
    assert_eq!(control.bytes(), 0);
}

both_modes!(
    heaps_of_one_session_add_up_their_pages,
    memory_limit_sets_the_stop_bit,
    interrupt_and_clear,
    frozen_chunks_stay_counted_until_dropped,
);
```

`bytes_in_use_pages` は、テストのための補助で、`Heap` が OS から取ったページのバイト数 (`self.core.pages.bytes()`) を返す `pub(crate) fn` にする。既にある `bytes_in_use` は検査モードで隔離したブロックを引くので、比べる値には使えない。

`heap/tests/mod.rs` に `mod control;` を足す。

Run: `cargo test -p eml_rt control::`
Expected: コンパイルの誤り (`Control` がない)

- [ ] **Step 2: `Control` を作る**

`crates/eml_rt/src/control.rs` を作る。
- `Control` は、`stop: AtomicU32`、`bytes: AtomicUsize`、`limits: Limits` を持つ。`verus!` の中で atomic の欄を持つ構造体を Verus が受け付けなければ、構造体とメソッドを `verus!` の外に置く (`Control` のメソッドは `Pages` のページの数の加減のほか、検証したコードから呼ばない)
- `interrupt` は `stop.fetch_or(INTERRUPT, Relaxed)`、`clear` は `stop.store(0, Relaxed)`、`stop_bits` は `stop.load(Relaxed)`。Relaxed でよいのは、止める語が伝えるのは「止めよ」の1ビットだけで、ほかのメモリの公開を伴わないためである (コメントに書く)
- 数の加減は `pub(crate) fn charge(&self, bytes: usize)` と `pub(crate) fn refund(&self, bytes: usize)`。`charge` は `fetch_add` の後の値が `limits.max_bytes` を超えたら `stop.fetch_or(MEMORY)` する。`refund` は `fetch_sub` だけで、ビットを下ろさない (下ろすのは `clear` である。spec の「実行の制御」)
- `Limits` は `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`、`Default` はどれも `usize::MAX`。`Stop` は `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`

- [ ] **Step 3: 確保器で数える**

- `Pages` に `control: Arc<Control>` の欄を足し、`Pages` を作る関数に `Arc<Control>` を渡す
- ページを取る所 (`alloc.rs` の `alloc_page(bytes)` を呼ぶ所) の後で `charge(bytes)`、返す所 (`dealloc_page` を呼ぶ所) の後で `refund(bytes)`、`Drop for Pages` で返すページごとに `refund`
- 検証したコードから呼ぶ数の加減は、`verus!` の中に `#[verifier::external_body] fn charge_pages(control: &Control, bytes: usize)` と `refund_pages` を置き、中で `Control` のメソッドを呼ぶ。何も約束しない (`ensures` を書かない)。`trust.rs` の `TRUSTED` に `("alloc.rs: external_body charge_pages", "T1")` と `refund_pages` を足す
- `Heap::new()` は上限のない新しい `Control` で、`Heap::with_control(control)` は渡された `Control` でページを持つ。`Heap::control(&self) -> &Arc<Control>` を足す
- `Frozen` は `Pages` を作り手から移すので、`Arc<Control>` も一緒に移る。数は変えない。塊を落とすとき、`Drop for Pages` が数を減らす

`Heap` の欄は増やさない (`Arc<Control>` は `Pages` の中に置く)。S9b-2a で `Heap` に `Arc` の欄を足したとき、計測のプログラムで速い道の関数がインライン化されなくなった (`docs/implementation/benchmarks.md` の「S9b-2a (凍結した領域)」)。`Pages` の中でも同じことが起きたら、`ManuallyDrop` と `#[inline(never)]` の手放しで同じように直す。

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt control::`
Expected: 通る

- [ ] **Step 4: Miri のテストを足す**

`heap/tests/trusted.rs` に `// 実行の制御` の節を足す。

```rust
// T1: 2つのスレッドのヒープが、1つの Control にページの数を足し引きする
fn two_threads_share_one_control<M: Mode>() {
    let control = Arc::new(crate::Control::new(crate::Limits::default()));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let control = control.clone();
            std::thread::spawn(move || {
                let mut heap = Heap::<M>::with_control(control);
                let big = heap.alloc_data(0, 0, PAGE_BYTES / 8, false);
                unsafe { heap.decref(big) }.unwrap();
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(control.bytes(), 0);
}

// 別のスレッドが立てた止める語を読む
fn an_interrupt_from_another_thread_is_seen() {
    let control = Arc::new(crate::Control::new(crate::Limits::default()));
    let setter = {
        let control = control.clone();
        std::thread::spawn(move || control.interrupt())
    };
    setter.join().unwrap();
    assert_eq!(control.stop_bits(), crate::Control::INTERRUPT);
}
```

`both_modes!` の一覧 (このファイルの書き方に合わせる。`every_core_operation_once_fast` などと同じく、モードごとの `#[test]` 関数を書く形かもしれない) に足す。

Run: `cargo test -p eml_rt trusted::` と Miri の2つの設定
Expected: 通る

- [ ] **Step 5: 計測して、全体を確かめてコミットする**

Run: `bench/rt.sh` を2回。どの計測も「S9b-2a (凍結した領域)」の値から +0.5% を超えて増えない。超えたら、インライン化されなくなった関数を `nm` の差で探し、Step 3 の直し方を使う。Global Constraints の全体の確かめを流す。

```bash
git add crates/eml_rt
git commit -m "Count each session's pages in a shared Control with a stop word"
```

---

### Task 3: `Ctx` の骨組みと yield の操作

**Files:**
- Create: `crates/eml_rt/src/heap/ctx.rs`
- Modify: `crates/eml_rt/src/heap/mod.rs` (`mod ctx;`、`pub use ctx::{Ctx, CtxStats};`)、`crates/eml_rt/src/lib.rs` (`pub use heap::{Ctx, CtxStats, …}`、型の assert)
- Modify: `crates/eml_rt/src/error.rs` (`YieldMisuse` と、その `Display`)
- Modify: `crates/eml_rt/src/trust.rs` (`FILES` に `heap/ctx.rs`)
- Create: `crates/eml_rt/src/heap/tests/ctx.rs`。Modify: `heap/tests/mod.rs` (`mod ctx;`)

**Interfaces:**
- Consumes: Task 1 の `kept_live`、`words_kept`、Task 2 の `Control`、`Limits`、`Stop`、`Heap::with_control`
- Produces: 「名前」の節の `Ctx`、`CtxStats`、`YieldState`、`Ctx::new`、`heap`、`heap_mut`、`poll`、`check_finished`、`stats`、`yielding`、`yield_status`、`yield_to`、`yield_abort`、`yield_cont_take`、`yield_cont_set`、`yield_finish`、`evv_get`、`evv_set`。spec の関数のうち `held_ok` と、`evv_wf` の仮の形 (この Task では「`evv` は `()`」か「Task 4 で定める述語」を受ける。下の Step 3)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_rt/src/heap/tests/ctx.rs` を作る。

```rust
//! 実行文脈と基本操作 (docs/spec/lir.md の「基本操作」)。

use std::sync::Arc;

use crate::{Control, Ctx, HeapError, Limits, Mode, Stop, Word};

pub(super) const UNIT: Word = Word::imm(0);

pub(super) fn ctx<M: Mode>() -> Ctx<M> {
    Ctx::new(Arc::new(Control::new(Limits::default())))
}

fn a_new_ctx_is_finished<M: Mode>() {
    let ctx = ctx::<M>();
    assert!(ctx.check_finished());
    assert_eq!(ctx.yielding(), 0);
    assert_eq!(ctx.poll(0), Ok(()));
}

fn poll_reports_each_stop<M: Mode>() {
    let control = Arc::new(Control::new(Limits {
        max_depth: 100,
        ..Limits::default()
    }));
    let ctx = Ctx::<M>::new(control.clone());
    assert_eq!(ctx.poll(100), Ok(()));
    assert_eq!(ctx.poll(101), Err(Stop::DepthLimit));
    control.interrupt();
    assert_eq!(ctx.poll(0), Err(Stop::Interrupted));
    control.clear();
    assert_eq!(ctx.poll(0), Ok(()));
}

/// yield して、継続を2回組み立て直し、終える。`eml_interp` の
/// `yields_and_continuation_frames_are_counted` と同じ列で、同じ数になる。最初の κ は関数の値 (`Word::imm(1)`) で、
/// 関数 0 の値は `()` と同じ語になるので使わない。
fn yields_and_continuation_frames_are_counted<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    unsafe {
        ctx.yield_to(ev, UNIT, Word::imm(1)).unwrap();
        assert_eq!(ctx.yielding(), 1);
        assert_eq!(ctx.yield_status(ev), 1);
        for _ in 0..2 {
            let k = ctx.yield_cont_take().unwrap();
            ctx.yield_cont_set(k).unwrap();
        }
        let k = ctx.yield_cont_take().unwrap();
        ctx.heap_mut().decref(k).unwrap();
        let clause = ctx.yield_finish().unwrap();
        assert_eq!(clause, UNIT);
    }
    assert!(ctx.check_finished());
    if M::CHECKED {
        let stats = ctx.stats();
        assert_eq!((stats.yields, stats.yield_frames), (1, 2));
    }
}

fn yield_status_tells_targets_apart<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    let other = ctx_evidence(&mut ctx);
    assert_eq!(ctx.yield_status(ev), 0);
    unsafe { ctx.yield_abort(ev, UNIT, UNIT) }.unwrap();
    assert_eq!(ctx.yield_status(ev), 2);
    assert_eq!(ctx.yield_status(other), 3);
    unsafe {
        ctx.yield_finish().unwrap();
        ctx.heap_mut().decref(other).unwrap();
    }
    assert!(ctx.check_finished());
}

fn misused_yields_are_reported_in_checked_mode<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        assert_eq!(ctx.yield_finish().unwrap_err(), HeapError::YieldMisuse);
        assert_eq!(ctx.yield_cont_take().unwrap_err(), HeapError::YieldMisuse);
        let ev = ctx_evidence(&mut ctx);
        ctx.yield_to(ev, UNIT, Word::imm(1)).unwrap();
        // κ を持ったまま、もう1つの κ は入れられない
        assert_eq!(ctx.yield_cont_set(Word::imm(2)).unwrap_err(), HeapError::YieldMisuse);
        // yield 中にもう1つの yield は始められない
        let ev2 = ctx_evidence(&mut ctx);
        assert_eq!(ctx.yield_to(ev2, UNIT, UNIT).unwrap_err(), HeapError::YieldMisuse);
        // κ を持ったまま終えられない
        assert_eq!(ctx.yield_finish().unwrap_err(), HeapError::YieldMisuse);
    }
}

fn evv_get_and_set_swap_the_list<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let empty = ctx.evv_get().unwrap();
        assert_eq!(empty, UNIT);
        ctx.evv_set(UNIT).unwrap();
    }
    assert!(ctx.check_finished());
}

fn dropping_a_ctx_mid_yield<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    unsafe {
        let k = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.yield_to(ev, UNIT, k).unwrap();
    }
    assert!(!ctx.check_finished());
    drop(ctx);
}

both_modes!(
    a_new_ctx_is_finished,
    poll_reports_each_stop,
    yields_and_continuation_frames_are_counted,
    yield_status_tells_targets_apart,
    misused_yields_are_reported_in_checked_mode,
    evv_get_and_set_swap_the_list,
    dropping_a_ctx_mid_yield,
);
```

この Task には `ev_new` がまだないので、テストの evidence はヒープに直接作る補助 `ctx_evidence` で作る。evidence の形 (spec の `ev_shape`): tag 1 (エフェクト 0 の番号 + 1) の `Data` で、走査する語が3つ [入口の表, 状態, 外側の `evv`]。入口の表は tag 0 の `Data` で、走査する語が1つ (`()`)。

```rust
/// エフェクト 0、操作1つの evidence を、基本操作を使わずにヒープに作る。
pub(super) fn ctx_evidence<M: Mode>(ctx: &mut Ctx<M>) -> Word {
    let heap = unsafe { ctx.heap_mut() };
    let table = heap.alloc_data(0, 1, 0, false);
    let ev = heap.alloc_data(1, 3, 0, false);
    unsafe { heap.set_field(ev, 0, table) }.unwrap();
    ev
}
```

`heap/tests/mod.rs` に `mod ctx;` を足す。

Run: `cargo test -p eml_rt ctx::`
Expected: コンパイルの誤り (`Ctx` がない)

- [ ] **Step 2: `Ctx` の構造体と安全なメソッドを作る**

`crates/eml_rt/src/heap/ctx.rs` を作る。`heap` の子のモジュールにするのは、ヒープの spec の関数 (`pub closed`) の中を開けるためである (spec の「`Ctx` の形」)。

- 構造体は spec の「`Ctx` の形」のとおり。`max_depth` と `max_evv` は `Control::limits()` から写す
- `YieldState` は `#[repr(C)]` で、`target` が `()` なら「なし」。不変条件 `yield_wf`: `target` が `()` なら `clause` と `cont` も `()` で `abort` は `false`
- `Ctx::new(control)` は `Heap::with_control(control.clone())` でヒープを作り、`evv` を `()`、yield の状態をなしにする
- `heap(&self) -> &Heap<M>` は安全、`heap_mut(&mut self) -> &mut Heap<M>` は `unsafe fn`。`# Safety` に「ヒープを入れ替えない。`Ctx` が持つ語 (`evv` と yield の状態) を手放したり書き換えたりしない」と書く
- `poll(&self, depth)` は `verus!` の外に書く (spec の「実行の制御」)。速い道は `depth > self.max_depth || self.control.stop_bits() != 0` (`Ctx` が自分の `Arc<Control>` の欄を持ち、ヒープの中の `Pages` をたどらない) で、真なら遅い道で `Stop` を決める (`INTERRUPT` が立っていれば `Interrupted`、`MEMORY` なら `MemoryLimit`、どちらでもなければ `DepthLimit`)。遅い道は `#[cold]` の関数に分ける
- `check_finished(&self) -> bool` は `evv` が `()` で yield の状態がなしであること
- `yielding(&self) -> i64`、`yield_status(&self, ev: Word) -> i64`、`yield_cont_take(&mut self)` は安全 (spec の表)
- `stats(&self) -> CtxStats`。数は検査モードだけで増やす (`if M::CHECKED { … }`)

- [ ] **Step 3: `held_ok` と yield と `evv_get` / `evv_set` を作る**

- `held_ok()`: spec の「spec の関数」のとおり。この Task では、`evv_wf` を「`evv` は `()`」で仮に置いてよい。Task 4 で本当の定義に替える (替えるのは spec の関数の本体だけで、操作の契約の形は変えない)。仮に置くときは、Task 4 の報告で替えたことが分かるように、関数の中にその旨を書かない (コメントに計画のことを書かない) で、Task 3 の報告に書く
- `Ctx::wf()` = `self.heap.wf() && self.held_ok() && self.yielding.yield_wf()`。どの `unsafe` な操作も `requires old(self).wf()`、`ensures final(self).wf()` を持つ。検査モードの `Err` のときも `wf` を保つ
- yield の操作は spec の表のとおり。宛先、節の本体、κ の参照を `Ctx` が持つ。前提が崩れていれば、検査モードは `YieldMisuse` を状態を変える前に返す。速いモードの前提は `requires !M::CHECKED ==> …` で書く
- `yield_finish` は宛先を `decref` する (この Task で `decref` を使う唯一の所)。`kept_live` と `words_kept` で `held_ok` を保つ
- `yield_cont_set` で `yield_frames` を、`yield_to` / `yield_abort` で `yields` を、検査モードだけで数える
- `evv_get` は `self.evv` を `dup` して返し、`evv_set(l)` は `evv` を l にして元を `decref` する。前提は spec の表のとおり (`evv_wf(l)`、`holds(l)`)

`error.rs` に `YieldMisuse` を足し、`Display` は `"an evidence-passing primitive breaks the yield state"` にする。

- [ ] **Step 4: 型の assert を足す**

`lib.rs` の tests に `send::<Ctx<Fast>>()`、`send::<Ctx<Checked>>()`、`send_and_sync::<Control>()` を足す。`Ctx` が `Sync` でないことは、`Heap` の `AmbiguousIfSync` と同じ形の assert を足す。

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt ctx::`
Expected: 通る

- [ ] **Step 5: 全体を確かめてコミットする**

Global Constraints の全体の確かめを流す (計測は差分で速い道に触れないことを確かめる。`Ctx` は新しい型で、核の操作のコードは変えない)。

```bash
git add crates/eml_rt
git commit -m "Add the execution context with its yield state and safe point"
```

---

### Task 4: `evv` と evidence の spec の関数と、evidence の操作

**Files:**
- Modify: `crates/eml_rt/src/heap/ctx.rs`
- Modify: `crates/eml_rt/src/error.rs` (`BadEvidence`、`NoHandler`)
- Modify: `crates/eml_rt/src/heap/tests/ctx.rs`

**Interfaces:**
- Consumes: Task 1 の約束と補題、Task 3 の `Ctx`
- Produces: `ev_shape`、`evv_path`、`evv_seq`、`evv_wf` (本当の定義)、`evv_push`、`ev_new`、`ev_set_clause`、`ev_renew`、`ev_lookup`、`ev_clause`、`ev_evv`、`ev_take_state`、`ev_set_state`

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests/ctx.rs` に、次のテストの補助を足す。

```rust
/// 今の evv の先頭に、エフェクト `effect` の新しい evidence を足し、その evidence を返す (呼ぶ側が1つ参照を持つ)。
pub(super) fn push<M: Mode>(ctx: &mut Ctx<M>, effect: u16, n_ops: usize) -> Word {
    unsafe {
        let outer = ctx.evv_get().unwrap();
        let ev = ctx.ev_new(effect, n_ops, UNIT, outer).unwrap();
        ctx.heap_mut().dup(ev).unwrap();
        let list = ctx.evv_get().unwrap();
        let list = ctx.evv_push(list, ev).unwrap();
        ctx.evv_set(list).unwrap();
        ev
    }
}

/// 今の evv を空にする。
pub(super) fn clear<M: Mode>(ctx: &mut Ctx<M>) {
    unsafe { ctx.evv_set(UNIT) }.unwrap();
}
```

続けて、次のテストを足す。

```rust
fn lookup_finds_the_innermost_evidence<M: Mode>() {
    let mut ctx = ctx::<M>();
    let e0 = push(&mut ctx, 0, 1);
    let e1 = push(&mut ctx, 1, 1);
    let e0_inner = push(&mut ctx, 0, 1);
    unsafe {
        let found = ctx.ev_lookup(0).unwrap();
        assert_eq!(found, e0_inner);
        ctx.heap_mut().decref(found).unwrap();
        let found = ctx.ev_lookup(1).unwrap();
        assert_eq!(found, e1);
        ctx.heap_mut().decref(found).unwrap();
        for e in [e0, e1, e0_inner] {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
}

fn clauses_states_and_outer_lists<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 2);
    unsafe {
        let clause = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 1, clause).unwrap();
        let got = ctx.ev_clause(ev, 1).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        assert_eq!(ctx.ev_clause(ev, 0).unwrap(), UNIT);
        // 状態の出し入れ
        let s = ctx.heap_mut().alloc_string("state");
        ctx.ev_set_state(ev, s).unwrap();
        let taken = ctx.ev_take_state(ev).unwrap();
        assert_eq!(taken, s);
        assert_eq!(ctx.ev_take_state(ev).unwrap(), UNIT);
        ctx.heap_mut().decref(taken).unwrap();
        // 外側の evv は、作ったときの evv (空) である
        assert_eq!(ctx.ev_evv(ev).unwrap(), UNIT);
        // renew は同じ入口の表を指し、外側を今の evv にする
        let renewed = ctx.ev_renew(ev, UNIT).unwrap();
        let got = ctx.ev_clause(renewed, 1).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        let outer = ctx.ev_evv(renewed).unwrap();
        let now = ctx.evv_get().unwrap();
        assert_eq!(outer, now);
        for w in [outer, now, renewed, ev] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 操作が 255 個以上のエフェクトの入口の表は、先頭に数の語を持つ (docs/spec/object-model.md の「物体の形」)。
fn entry_tables_with_a_count_word<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 300);
    unsafe {
        let clause = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 299, clause).unwrap();
        assert_eq!(ctx.ev_clause(ev, 0).unwrap(), UNIT);
        let got = ctx.ev_clause(ev, 299).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        ctx.heap_mut().decref(ev).unwrap();
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 状態の欄を通る輪 (docs/spec/lir.md の「evidence」): 状態が ev を指す物体を持っていても、ev を呼ぶ側が
/// 持っている限り、元の状態を手放しても ev は生き残る。
fn replacing_a_state_that_reaches_its_evidence<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 1);
    unsafe {
        let holder = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.heap_mut().dup(ev).unwrap();
        ctx.heap_mut().set_field(holder, 0, ev).unwrap();
        ctx.ev_set_state(ev, holder).unwrap();
        ctx.ev_set_state(ev, UNIT).unwrap();
        // ev はまだ読める
        assert_eq!(ctx.ev_take_state(ev).unwrap(), UNIT);
        ctx.heap_mut().decref(ev).unwrap();
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

fn bad_evidence_is_reported_in_checked_mode<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        // evidence でない物体 (tag 0 のクロージャ)
        let closure = ctx.heap_mut().alloc_data(0, 3, 0, false);
        assert_eq!(ctx.ev_clause(closure, 0).unwrap_err(), HeapError::BadEvidence);
        assert_eq!(ctx.ev_take_state(closure).unwrap_err(), HeapError::BadEvidence);
        // 範囲の外の操作
        let ev = push(&mut ctx, 0, 1);
        assert_eq!(ctx.ev_clause(ev, 1).unwrap_err(), HeapError::BadEvidence);
        // 入口の欄に2回入れる
        let c = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 0, c).unwrap();
        let c2 = ctx.heap_mut().alloc_data(0, 1, 0, false);
        assert_eq!(ctx.ev_set_clause(ev, 0, c2).unwrap_err(), HeapError::BadEvidence);
        // エフェクトの番号の上限 (tag は番号 + 1 なので 65535 は作れない)
        assert_eq!(ctx.ev_new(u16::MAX, 1, UNIT, UNIT).unwrap_err(), HeapError::BadEvidence);
        // 合う evidence がない
        assert_eq!(ctx.ev_lookup(7).unwrap_err(), HeapError::NoHandler);
    }
}

both_modes!(
    lookup_finds_the_innermost_evidence,
    clauses_states_and_outer_lists,
    entry_tables_with_a_count_word,
    replacing_a_state_that_reaches_its_evidence,
    bad_evidence_is_reported_in_checked_mode,
);
```

Task 3 の補助 `ctx_evidence` は、ここで `ev_new` に置き換えてよい (Task 3 のテストの期待値は変わらない。機械的な追従)。

Run: `cargo test -p eml_rt ctx::`
Expected: コンパイルの誤り (`ev_new` などがない)

- [ ] **Step 2: spec の関数を定める**

spec の「spec の関数」のとおりに `heap/ctx.rs` に書く。

- `ev_shape(e)` は浅い形で、再帰しない。読める (`readable`)、`Data`、走査する語が3つ、tag が 0 でない、`lin` ビットがない、`owned`。入口の表は読める tag 0 の `Data` で、`lin` ビットがなく、どの欄も `()` か `lin` ビットのない `owned` の語
- `evv_path(w, fuel) -> Option<Seq<(u16, Word)>>` は `decreases fuel`。`w` が `()` なら `Some(empty)`、ほかの即値なら `None`、`fuel == 0` なら `None`。節点は読める `Data` で、走査する語が2つ、`lin` ビットがなく、`owned`、tag は evidence の tag と同じで、evidence は `ev_shape` を満たす
- `evv_seq(w)` は `choose` で `evv_path` が `Some` になる `fuel` を選んだ結果。`evv_wf(w)` は、そうなる `fuel` があり、列の長さが `self.max_evv` 以下であること。長さが同じ列になる (どの `fuel` でも、`Some` なら同じ列) ことの補題を、燃料について単調な補題として足す (`probe/` の `lemma_mono` の形)
- 枠の補題: `kept_live` で、`evv_path(w, fuel)` が `Some(s)` なら後も `Some(s)` (`probe/` の `lemma_frame` の形)。どの操作の後でも `held_ok` を保つのに使う
- Task 3 で仮に置いた `evv_wf` をこの定義に替える

Run: `cargo verus verify -p eml_rt`
Expected: 0 errors (Task 3 の操作の証明は、`kept_live` と枠の補題で通る)

- [ ] **Step 3: evidence の操作を作る**

spec の「契約」の表と、その下の項のとおりに作る。要点は次のとおりである。

- evidence と節点の tag は、エフェクトの番号 + 1 にする。`ev_new` は `effect == u16::MAX` を、検査モードで `BadEvidence`、速いモードで前提にする
- `ev_new(effect, n_ops, state, outer)`: 入口の表を `alloc_data(0, n_ops, 0, false)` で、evidence を `alloc_data(effect + 1, 3, 0, false)` で作り、`set_field` で [表, state, outer] を入れる。Task 1 の `alloc_data` の ensures (欄はどれも `()`) で、入口の欄が `()` であることを出す
- `evv_push(l, e)`: 節点を `alloc_data(tag(e), 2, 0, false)` で作り、[e, l] を入れる。検査モードは、確保の前に e の形と、長さが `max_evv` 未満であることを確かめる (長さの確かめは、検査モードでは l をたどって数える。速いモードは前提である)
- `ev_lookup(effect)`: `evv` を先頭からたどる。速いモードは、節点の `meta` を1回読み、作っておいた tag E + 1 の `Meta` と比べ、次の節点を `field(node, 1)` で読む。検査モードは、節点ごとに形を確かめ、`max_evv` 歩を超えたら `BadEvidence` にし、`evidence_steps` を数える。見つけた evidence を `dup` して返す。最後まで見つからなければ `NoHandler`
- `ev_set_clause`、`ev_clause`: 入口の表の欄は `scanned` で読み、書くときは表の `scanned_len` が 255 以上なら位置を1つずらして `set_field` する (`scanned` の中の数え方と同じ。`object-model.md` の「クロージャ」の 255 の規則)。`ev_set_clause` は欄が `()` でなければ検査モードで `BadEvidence`
- `ev_set_state(ev, s)`: 前提に、ev をこのヒープが持っていること (外の参照が1つ以上) を書く。spec の「契約」の下の項のとおり、元の状態を手放す連鎖が ev 自身を空けないためである
- `ev_take_state`、`ev_set_state`、`ev_set_clause` は `set_field` で書き、前提に `writable` を書く。検査モードの `NotWritable` は `set_field` がそのまま返す
- 検査モードの確かめは、状態を変える前にすべて行う。手放す連鎖の途中の誤り (`chain_err`) を返しうるのは `evv_set`、`ev_set_state`、`yield_finish` だけである
- evidence (3語) と節点 (2語) は `field` と `set_field` で読み書きする
- どの操作も `wf()` を保ち、RC のモデルでの増減を ensures に書く (`evolved` か、`handover.rs` の `evolved_by` を使う)

`error.rs` に `BadEvidence` (`"an evidence-passing primitive reads something that is not an evidence, an evv node or an entry table"`) と `NoHandler` (`"no evidence of the effect is in the evv"`) を足す。

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt ctx::`
Expected: 通る

- [ ] **Step 4: 全体を確かめてコミットする**

Global Constraints の全体の確かめを流す (計測は差分で速い道に触れないことを確かめる)。

```bash
git add crates/eml_rt
git commit -m "Add the evidence primitives, proven against the evv as a sequence"
```

---

### Task 5: `evv_mask`、EP の形の筋書き、Miri、計測の追加

**Files:**
- Modify: `crates/eml_rt/src/heap/ctx.rs` (`mask_spec`、`evv_mask`)
- Modify: `crates/eml_rt/src/heap/tests/ctx.rs`、`crates/eml_rt/src/heap/tests/trusted.rs`
- Modify: `crates/eml_rt/examples/rt_bench.rs`、`bench/rt.sh`

**Interfaces:**
- Consumes: Task 3 と Task 4 の操作
- Produces: `evv_mask`、計測 `perform`、`perform-deep`、`handle`、`yield`、`mask`、`poll`

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests/ctx.rs` に足す。`eml_interp/src/prims.rs` の tests の3つのテストを、同じ列と同じ期待値で写す (spec の「性能」の項)。

```rust
/// `evv_seq` の evidence を先頭から並べる (テストの補助)。節点は `field(node, 0)` が evidence、`field(node, 1)` が次である。
fn nodes<M: Mode>(ctx: &Ctx<M>, mut list: Word) -> Vec<(Word, Word)> {
    let mut nodes = Vec::new();
    while list != UNIT {
        let ev = unsafe { ctx.heap().field(list, 0) }.unwrap();
        nodes.push((list, ev));
        list = unsafe { ctx.heap().field(list, 1) }.unwrap();
    }
    nodes
}

fn lookups_and_masks_count_the_list_nodes_they_visit<M: Mode>() {
    let mut ctx = ctx::<M>();
    let evs: Vec<Word> = [0, 1, 1].into_iter().map(|e| push(&mut ctx, e, 1)).collect();
    unsafe {
        let found = ctx.ev_lookup(0).unwrap();
        ctx.heap_mut().decref(found).unwrap();
        if M::CHECKED {
            assert_eq!(ctx.stats().evidence_steps, 3);
        }
        let list = ctx.evv_get().unwrap();
        let masked = ctx.evv_mask(&[1], list).unwrap();
        if M::CHECKED {
            assert_eq!(ctx.stats().evidence_steps, 4);
        }
        ctx.heap_mut().decref(masked).unwrap();
        ctx.heap_mut().decref(list).unwrap();
        for e in evs {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// evv は先頭から 0、1、1、0 のエフェクトの evidence である。多重集合の各エフェクトについて、先頭から合う
/// evidence をその数だけ飛ばし、飛ばした最後の節点の前で残した節点だけを写す。
fn a_mask_skips_the_first_matches_of_each_effect_in_its_multiset<M: Mode>() {
    let mut ctx = ctx::<M>();
    let evs: Vec<Word> = [0, 1, 1, 0].into_iter().map(|e| push(&mut ctx, e, 1)).collect();
    unsafe {
        let list = ctx.evv_get().unwrap();
        let original = nodes(&ctx, list);
        let distinct = ctx.evv_mask(&[0, 1], list).unwrap();
        assert_eq!(distinct, original[2].0);
        let twice = ctx.evv_mask(&[1, 1], list).unwrap();
        let masked = nodes(&ctx, twice);
        assert_eq!(
            masked.iter().map(|&(_, ev)| ev).collect::<Vec<_>>(),
            [original[0].1, original[3].1]
        );
        assert_ne!(masked[0].0, original[0].0);
        assert_eq!(masked[1].0, original[3].0);
        if M::CHECKED {
            assert_eq!(ctx.stats().evidence_steps, 2 + 3);
        }
        for w in [distinct, twice, list] {
            ctx.heap_mut().decref(w).unwrap();
        }
        for e in evs {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

fn a_mask_that_skips_more_evidence_than_the_list_holds<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    let _e0 = push(&mut ctx, 0, 1);
    let _e1 = push(&mut ctx, 1, 1);
    unsafe {
        let list = ctx.evv_get().unwrap();
        assert_eq!(ctx.evv_mask(&[1, 1], list).unwrap_err(), HeapError::NoHandler);
    }
}

/// 輪になった evv は、小さい上限で BadEvidence になって止まる。
fn a_cyclic_evv_stops_at_the_limit<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let control = Arc::new(Control::new(Limits {
        max_evv: 8,
        ..Limits::default()
    }));
    let mut ctx = Ctx::<M>::new(control);
    let _e0 = push(&mut ctx, 0, 1);
    unsafe {
        let list = ctx.evv_get().unwrap();
        // 節点の next を自分自身に向けて輪にする (前提を崩すテストなので raw で書く)
        super::raw::set_slot(ctx.heap_mut(), list, 1, list);
        ctx.heap_mut().decref(list).unwrap();
        assert_eq!(ctx.ev_lookup(5).unwrap_err(), HeapError::BadEvidence);
    }
}
```

EP が出す形を並べた筋書き (spec の「検証とテスト」) も足す。

```rust
/// handle の中で perform して、その場で戻る (in-place の節)。
fn perform_in_place<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 3, 1);
    unsafe {
        let clause = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 0, clause).unwrap();
        // perform: ev_lookup + ev_clause + 節の呼び出し (ここでは節を読むだけ)
        let found = ctx.ev_lookup(3).unwrap();
        let c = ctx.ev_clause(found, 0).unwrap();
        assert_eq!(c, clause);
        ctx.heap_mut().decref(c).unwrap();
        ctx.heap_mut().decref(found).unwrap();
        // handle の終わり: evv を外側に戻す
        let outer = ctx.ev_evv(ev).unwrap();
        ctx.evv_set(outer).unwrap();
        ctx.heap_mut().decref(ev).unwrap();
    }
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// yield して継続を組み立て、再開する1往復。
fn yield_and_resume_once<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 1);
    unsafe {
        let body = ctx.heap_mut().alloc_data(0, 1, 0, false);
        let k0 = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.heap_mut().dup(ev).unwrap();
        ctx.yield_to(ev, body, k0).unwrap();
        // 呼び出しのフレームごとに継続を包む
        let k = ctx.yield_cont_take().unwrap();
        let wrapped = ctx.heap_mut().alloc_data(0, 2, 0, false);
        ctx.heap_mut().set_field(wrapped, 1, k).unwrap();
        ctx.yield_cont_set(wrapped).unwrap();
        // prompt$ が受ける
        assert_eq!(ctx.yield_status(ev), 1);
        let k = ctx.yield_cont_take().unwrap();
        let body = ctx.yield_finish().unwrap();
        // 再開: 新しい evidence で節の本体を動かす (ここでは作るだけ)
        let renewed = ctx.ev_renew(ev, UNIT).unwrap();
        for w in [renewed, body, k, ev] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

both_modes!(
    lookups_and_masks_count_the_list_nodes_they_visit,
    a_mask_skips_the_first_matches_of_each_effect_in_its_multiset,
    a_mask_that_skips_more_evidence_than_the_list_holds,
    a_cyclic_evv_stops_at_the_limit,
    perform_in_place,
    yield_and_resume_once,
);
```

`super::raw::set_slot` は、S9b-2a の終わりに足した、前提を崩すテストのための補助である (`heap/tests/raw.rs`)。引数の形が違えば合わせる。`push` が返す evidence の参照を手放す所 (`_e0` など) は、テストの終わりにヒープごと落とすので省いてよい。

Run: `cargo test -p eml_rt ctx::`
Expected: コンパイルの誤り (`evv_mask` がない)

- [ ] **Step 2: `evv_mask` を作る**

spec の「契約」の下の項のとおりに作る。

- `mask_spec(effects, seq)` は、(tag、evidence) の組の列の上の再帰の spec の関数である。多重集合 `effects` の各エフェクト e について、先頭から tag が e + 1 の組をその数だけ除く
- `evv_mask(effects: &[u16], w)`: w を借りる (手放さない)。先頭からたどり、除く組を数え、残す節点を写す。写す節点は先頭から順に作り、1つ前の写しの `next` を `set_field` で書く。最後に除いた節点の次から後ろは元のリストを共有し、その先頭を `dup` する。除くものがもうなくなったら、たどりを止める。Rust の `Vec` は使わない。多重集合は、節点ごとに `effects` を見直して、まだ除く数が残っているかを数える (除いた数を数える小さい配列も `Vec` も使わず、`effects` の中で e が何回現れるかと、今までに除いた e の数を比べる。m は小さいので O(n·m²) でもよい)
- 結果は `evv_seq(結果) == mask_spec(effects, evv_seq(w))` を ensures に書く
- 検査モードは、節点ごとに形を確かめ、`max_evv` 歩を超えたら `BadEvidence`、除く組が足りなければ `NoHandler` を返す。誤りのときは、それまでに写した節点を手放してから返す (ヒープを元に戻す)。手放しの連鎖は写した節点と、その `dup` した evidence の参照だけに届く
- 検査モードで、たどった節点の数を `evidence_steps` に足す (インタプリタと同じく、写した節点も飛ばした節点も1歩と数え、止めた後は数えない)

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt ctx::`
Expected: 通る

- [ ] **Step 3: Miri のテストを足す**

`heap/tests/trusted.rs` の、核の操作を1回ずつ通す T8 のテスト (`every_core_operation_once`) に、基本操作を1回ずつ足す。`Ctx` を作り、`push` の補助 (`super::ctx::push`) で evidence を2つ足し、`ev_lookup`、`ev_clause`、`ev_set_clause`、`ev_evv`、`ev_take_state`、`ev_set_state`、`ev_renew`、`evv_mask`、yield の1往復、`poll` を通す。最後に `check_finished` を確かめる。

Task 2 の `an_interrupt_from_another_thread_is_seen` を、`Ctx::poll` が `Stop::Interrupted` を返す形に広げる (別のスレッドが `Control::interrupt()` を呼んだ後、この `Ctx` の `poll(0)`)。

Run: `cargo test -p eml_rt trusted::` と Miri の2つの設定
Expected: 通る

- [ ] **Step 4: 計測を足す**

`crates/eml_rt/examples/rt_bench.rs` に6つの計測を足し、`bench/rt.sh` の `for case in` の一覧と、`main` の `expect` の文言に足す。どの計測も、今ある計測と同じく1回あたり数ミリ秒から数十ミリ秒になる回数で回す (`black_box` で値を隠す)。

- `perform`: evidence を1つ足した `evv` で、`ev_lookup(0)` と `ev_clause(ev, 0)` と2つの `decref` を 10,000,000 回
- `perform-deep`: 8つのエフェクト (1〜8) の evidence を足した `evv` で、一番奥のエフェクト 1 への `perform` を 10,000,000 回
- `handle`: `ev_new`、`ev_set_clause`、`evv_push`、`evv_set` で handler を設け、`ev_evv` と `evv_set` で外す1回を 1,000,000 回
- `yield`: `yield_to`、`yield_cont_take`、`yield_cont_set`、`yield_cont_take`、`yield_finish` の1往復を 10,000,000 回
- `mask`: 4つの evidence の `evv` で `evv_mask(&[2], list)` と結果の `decref` を 10,000,000 回
- `poll`: `poll(black_box(depth))` を 100,000,000 回

Run: `bench/rt.sh` を2回
Expected: 今ある8つの計測が「S9b-2a (凍結した領域)」の値から +0.5% を超えて増えない。新しい6つの値は Task 6 で記録する

- [ ] **Step 5: 全体を確かめてコミットする**

Global Constraints の全体の確かめを流す。

```bash
git add crates/eml_rt bench/rt.sh
git commit -m "Add evv_mask, the effect scenarios and the primitive measurements"
```

---

### Task 6: 計測の記録と文書

**Files:**
- Modify: `docs/spec/object-model.md`、`docs/spec/lir.md`、`docs/future/roadmap.md`、`docs/future/multicore.md`、`docs/implementation/testing.md`、`docs/implementation/benchmarks.md`、`docs/README.md`、`CLAUDE.md` (`eml_rt` の段落)
- Modify: `crates/eml_rt/src` のコメントのうち、新しい節「実行文脈 `Ctx`」を指すべき引用
- Delete: `docs/superpowers/specs/2026-10-14-s9b2b-ctx-primitives-design.md`、`docs/superpowers/plans/2026-10-14-s9b2b-ctx-primitives.md`

**Interfaces:**
- Consumes: Task 1〜5 の公開の操作

- [ ] **Step 1: 計測を記録する**

Run: `bench/rt.sh` を2回。`benchmarks.md` の「S9b-2a (凍結した領域)」の後に「S9b-2b (実行文脈)」の節を足す。出力の表 (14の計測) と、今ある8つの計測の S9b-2a との差を書く。新しい6つは S9b-2b の基準である。`eml_rt` は `eml run` から使わないので `bench/run.sh` の表は測らないことも、前の節と同じ文で書く。

- [ ] **Step 2: 文書を直す**

spec の「文書」の節のとおりに直す。

- `object-model.md`
  - 新しい節「実行文脈 `Ctx`」: 欄、yield の状態の形 (平らな `#[repr(C)]` の記録とその理由)、タスクの切り替え、`Control` と `poll` (止める語、ページで数える確保量、深さ)、上限、`check_finished`、`heap_mut` が `unsafe` である理由、基本操作の契約の考え方 (spec の関数でヒープの上に表すことと、その理由)、速いモードと検査モードの分け方
  - 「evidence の3つの物体」: tag をエフェクトの番号 + 1 にすること、エフェクトの数の上限 65535。関数 0 の値と `()` が同じ語なので、入口の欄と κ の欄の「空」の確かめは関数 0 の値を見逃しうること (誤って断ることはない)
  - 「論理的な worker ID」の「まだ決めていないこと」を閉じる
  - 「報告する誤り」に `BadEvidence`、`NoHandler`、`YieldMisuse`。「検査モードの約束」に、状態を変えた後に `Err` を返しうる基本操作 (`evv_set`、`ev_set_state`、`yield_finish`)
  - 「核の操作」に、`decref` と `release` の生き残るブロックの約束 (`kept_live`)、検査モードの `word_in` の約束、`alloc_data` の約束
  - 「確保と解放」に、ページを `Control` で数えること
  - 信頼の一覧の T1 (数の加減の `charge_pages`、`refund_pages`、何も約束しない) と T7 (`Ctx` の `Drop` は持つ語に触れない)
- `lir.md`: 「実行の文脈」と「基本操作」に、`eml_rt` の `Ctx` が実装すること、yield の状態の形、タスクの切り替え、`ev_new` が入口の数を引数で受けること
- `roadmap.md`
  - S9b-2b を終えた状態にし、論点「SIGINT の受け方」を閉じる (spec の「SIGINT」の決定を書く)
  - S9 の決めたことの `Ctx` の欄の一覧を直す (worker ID は `Heap`、出力先は S9b-3、セッションの根は S11)
  - S10 の決めたことに、ハンドラ (ctrlc、`eml run` では2回目で終了コード 130)、上限の既定の値と `RunConfig`、`RunStats` の仕事の回数は `eml_rt` の検査モードの定義を引き継ぐこと
  - S11 に、入力の始めの `Control::clear` と、2回目の Ctrl-C で終わらないこと
- `multicore.md`: `Control` はセッションに1つで、worker が共有する。タスクは yield の状態がなしの安全点でだけ切り替え、`par` の子は `evv = ()` で始める
- `testing.md`: `eml_rt` の検証の節に、`Ctx` のテスト、Miri の対象 (実行の制御の2つのスレッドのテスト、T8 の基本操作)
- `docs/README.md`: object-model の行の見出しの並びに「実行文脈 `Ctx`」を足す
- `CLAUDE.md` (英語): `eml_rt` の段落に、`Ctx` (the execution context: evv, a flat yield record, the 19 evidence primitives proven against the evv as a sequence) と、セッションの `Control` (stop word, page-counted memory budget) を1〜2文で足す

コードのコメントで、この節を指すべき引用 (Task 1〜5 で「基本操作」「実行の文脈」などを仮に引いた所) を「実行文脈 `Ctx`」に替える。`cargo test -p eml_cli citations` が通ることを確かめる。

- [ ] **Step 3: spec と計画を消して、全体を確かめてコミットする**

```bash
git rm docs/superpowers/specs/2026-10-14-s9b2b-ctx-primitives-design.md docs/superpowers/plans/2026-10-14-s9b2b-ctx-primitives.md
```

Run: `rg -n 's9b2b|S9b-2b の設計' docs crates CLAUDE.md` で、消した文書への参照が残っていないことを確かめる。Global Constraints の全体の確かめを流す。

```bash
git add -A docs crates/eml_rt CLAUDE.md
git commit -m "Document the execution context and record its measurement"
```

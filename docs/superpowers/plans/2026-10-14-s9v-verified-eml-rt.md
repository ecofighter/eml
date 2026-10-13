# S9v `eml_rt` を Verus で検証した1つの crate にする 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `crates/eml_rt` を `verus!` の中に書き直し、vstd に依存させ、`cargo verus verify -p eml_rt` で `unsafe` の核まで検証する。証明しない部分は信頼の一覧 (T0-T9) に閉じ、Miri は `trusted` の module のテストだけを流す。

**Architecture:** 下から積む。道具と写しの移設 (Task 1)、語とヘッダとページの権限 (Task 2)、`Heap` のモデルと RC と解放の連鎖 (Task 3)、`release` と reuse (Task 4)、ヒープの上の `share` (Task 5)、検査モード (Task 6)、`Foreign` と `Drop` と Miri と文書 (Task 7)。どの段でも、まだ証明していない関数は `external_body` に、まだ移していないファイルは `verus!` の外に置き、信頼の一覧のテストの一時の欄に載せる。

**Tech Stack:** Rust (edition 2024、fenix の 1.98.1)、Verus 0.2026.10.04.426d8b0、vstd 0.0.0-2026-10-04-0306、verusfmt 0.7.4、Miri (fenix の nightly)、macOS の `/usr/bin/time -l`。

**Spec:** `docs/superpowers/specs/2026-10-14-s9v-verified-eml-rt-design.md`。実装する人は、各タスクの前に spec の該当する節を読む。spec が規範で、計画とずれたら spec に従い、ずれを報告する。

| 見出し | 中身 |
|---|---|
| Task 0 | 計測の基準の取り直し (済み。値は下の「計測の基準」) |
| Task 1 | vstd と道具、写しの証明の移設、写しと照合の削除、信頼の一覧のテスト |
| Task 2 | 語、ヘッダとペイロードの権限、T1 と T2 の関数、ページの確保器 |
| Task 3 | `Heap` の `wf` と抽象のモデル、確保と構築中の物体、RC、解放の連鎖、フィールド |
| Task 4 | `release`、`release_reuse`、token、展開の補題 |
| Task 5 | ヒープの上の `share` |
| Task 6 | 検査モード |
| Task 7 | `Foreign`、`Drop`、`trusted` の Miri のテスト、計測、文書、spec と計画の削除 |

## Global Constraints

- 各タスクの終わりに次が通る。途中のタスクでもワークスペースを壊したままコミットしない
  - `cargo test` (ワークスペース全体)
  - `cargo verus verify -p eml_rt` が 0 errors。最後の行の `verification results:: N verified, 0 errors` と時間をコミットメッセージに書く
  - `cargo clippy --all-targets` が警告を出さない。`eml_rt` で許す警告は、Task 1 で `lib.rs` の頭に理由付きで置いた `allow` の一覧だけにする
  - `cargo fmt --check`、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`
  - Task 7 で `trusted` のテストを置いてからは、spec の「Miri」の2つの設定
- 成否と期待値は、spec の「テストの変更」に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す
- 公開の API は、spec の「API の約束」に挙げた4つのほかは変えない
- 検証が通っても `cargo build` が通るとは限らない (Verus は `const fn` の制約を見ない)。どのステップでも両方を流す
- 権限の形は spec の「権限の形」のとおりにする。`&Header` を作らない。整数からポインタを作るのは `ptr::without_provenance_mut` だけで、番地を変えるのは `with_addr` だけにする
- `unsafe` は、信頼の一覧の項目の中と、`verus!` の外の `unsafe impl Send` / `Sync` と T4 の `Drop` にだけ書く
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書く。規則を指すときは、`docs/` のパスと見出しで引く (`docs/spec/object-model.md の「核の操作」`)。この計画と spec の見出しは引かない。Task 7 で消すためである
- コメントに計画のタスクの番号を書かない
- コミットメッセージは英語で、既存の形 (命令形の1行目、本文に理由) に合わせる。各コミットの末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01T5GWBBHbwZa87PXuQK6wei
  ```

- `git diff` は `--no-ext-diff` で見る
- コマンドはすべて devShell の中で流す (`nix develop -c ...`、または direnv の入ったシェル)

## 名前 (各タスクで共通に使う)

- 検証の道具の置き場所: `crates/eml_rt/src/trust.rs` (信頼の一覧のテストと、探し方の小さいテスト。`#[cfg(test)]`)
- T1: `crate::alloc::os::{alloc_page, dealloc_page}`。`alloc_page(size: usize) -> (*mut u8, Tracked<PointsToRaw>, Tracked<Dealloc>)`、`dealloc_page(p: *mut u8, size: usize, Tracked<PointsToRaw>, Tracked<Dealloc>)`。揃えはどちらも `PAGE_BYTES`
- T2: `crate::meta::cell::{rc_load, rc_store, rc_fetch_sub_relaxed, rc_fetch_add_acq_rel, rc_load_acquire}` (`*mut i32` と `Tracked<&PointsTo<i32>>` か `Tracked<&mut PointsTo<i32>>` を受ける)、検査モードの前置きの `u32_load`、`u32_store`
- T3: `crate::foreign::{ForeignPerm, write, value, finalizer, Finalizer::run}`。`Finalizer` と `VTable` は `#[verifier::external_body]` の struct
- T5: `crate::word::too_large() -> !` などの panic の関数と、`ptr::without_provenance_mut` の `assume_specification`
- 権限: `crate::heap::perm::BlockPerm` (tracked struct。`rc: PointsTo<i32>`、`meta: PointsTo<u32>`、`slots: Map<int, PointsTo<Word>>`、`prefix: Option<(PointsTo<u32>, PointsTo<u32>)>`、`foreign: Option<ForeignPerm>`)、ページの `crate::alloc::PagePerm` (tracked struct。頭の `PointsTo<PageHeader>`、切り出していない `PointsToRaw`、`Dealloc`)
- モデル: `crate::heap::model::{HeapView, BlockView, BlockState, Slot}`。`BlockState { Building, Live, Token, Freed }`、`Slot { Unwritten, Written(Word), Kept(Word) }`、`HeapView { blocks: Map<usize, BlockView>, ext: Map<usize, nat>, building: Option<usize>, worker: u32, consistent: bool }`
- `Heap` の述語: `pub closed spec fn view(&self) -> HeapView`、`pub closed spec fn safe_wf(&self) -> bool`、`pub closed spec fn model_wf(&self) -> bool`、`pub open spec fn wf(&self) -> bool { self.safe_wf() && (self.view().consistent ==> self.model_wf()) }`
- API の前提の述語 (`heap/model.rs`): `holds(w)` (数えた参照を持つ)、`readable(w)` (モデルで生きている物体を指す)、`settled()` (構築中の物体が書き終えている)、`allocated(w)` (このヒープが確保したブロックを指す)

## 計測の基準

Task 0 で取り直した。S9v の終わりに同じ手順 (`bench/rt.sh`) で測り、どの場合も 0.5% を超えて増えないことを確かめる。

- 機種: Mac16,1 (Apple M4)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- コミット: 3614fe1

| 計測 | instructions retired | 実時間 (s) |
|---|---:|---:|
| `empty` | 15439590 | 0.00 |
| `list` | 1137337070 | 0.03 |
| `box-list` | 3063987273 | 0.10 |
| `dup` | 275427466 | 0.01 |
| `free` | 1815547900 | 0.05 |
| `map` | 1515992677 | 0.06 |
| `map-reuse` | 765398685 | 0.03 |

## Review Focus

- 検査モードで、空けた物体の語を `field` / `decref` / `share` に渡すこと: `Err(UseAfterFree)` を返し、ヒープを落としても安全である。Task 6 のテスト `a_freed_word_is_reported_by_every_operation`
- 同じ子を2つのフィールドに書いた親を空けること (検査モード): 2回目は `UseAfterFree` で、破棄処理は1回だけ呼ばれる。Task 6 のテスト `a_child_reached_twice_is_freed_once`
- 走査する語に生の値を書くこと、`Foreign` の生の語に書くこと (検査モード): `OutOfBounds` か `WrongKind` を返す。Task 6 のテスト `raw_and_scanned_words_are_not_mixed_up`
- 解放の連鎖の途中で `Foreign` の破棄処理が panic すること: panic は呼ぶ側に届き、その後 `Heap` を落としても二重の破棄も未定義の動作もない。Task 7 の `trusted` のテスト `a_panicking_finalizer_in_a_chain_leaks_the_rest`
- 2つのスレッドが共有の rc の欄を並行に増減すること: 最後の参照を見るのはちょうど1つのスレッドである。Task 7 の `trusted` のテスト `shared_counts_race_on_two_threads`
- token の箱を使い回し、残したフィールドを書かずに `con_at` の後の操作に進むこと: 残した語が辺に戻り、RC が釣り合う。Task 4 のテスト `kept_fields_return_to_edges_after_con_at`

---

### Task 1: vstd と道具、写しの証明の移設、信頼の一覧のテスト

**Files:**
- Modify: `crates/eml_rt/Cargo.toml`、`Cargo.lock`、`Cargo.toml` (workspace の lints)
- Modify: `crates/eml_rt/src/lib.rs`、`meta.rs`、`word.rs`、`rc.rs`、`heap/share.rs`、`heap/check.rs`
- Create: `crates/eml_rt/src/trust.rs`
- Delete: `crates/eml_rt/verus/` (全体)、`crates/eml_rt/src/verus_copies.rs`
- Modify: `CLAUDE.md` (Commands の `check.sh` の行)、`docs/implementation/testing.md` (「よく使うコマンド」の `check.sh` の行だけ。Verus の節の全体は Task 7)

**Interfaces:**
- Produces: `meta.rs` の `Kind`、`Meta` とその整数の関数が `verus!` の中にあり、S9a の写しと同じ `ensures` を持つ。`word.rs` の `fits_imm`、`imm_bits`、`is_imm_bits`、`is_ptr_bits`、`imm_value_bits`。`rc.rs` の `dup_step`、`decref_step`、`mark_step` と理想のモデルの補題。`heap/share.rs` の `Graph` と `share_walk` (`visited_lin` の探索は番地で比べる自前のループ `contains_node`)。`heap/check.rs` の `layout_check`、`reuse_in_place_check`
- Produces: `trust.rs` の `TRUSTED: &[(&str, &str, &str)]` (語、項目の名前、T の番号)、`UNPROVED: &[&str]` (未証明の関数)、`OUTSIDE: &[&str]` (まだ `verus!` に入れていないファイル)

- [ ] **Step 1: 依存と設定を足す**

`crates/eml_rt/Cargo.toml`:

```toml
[package]
name = "eml_rt"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
doctest = false

[dependencies]
vstd = "=0.0.0-2026-10-04-0306"

[package.metadata.verus]
verify = true

[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(verus_keep_ghost)', 'cfg(verus_only)'] }
```

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt`
Expected: vstd の検証が通り、`eml_rt` は `verus!` がまだないので 0 verified で終わる。

- [ ] **Step 2: 信頼の一覧のテストを先に書く (失敗する)**

`crates/eml_rt/src/trust.rs` を作り、`lib.rs` に `#[cfg(test)] mod trust;` を足す。探し方は次のとおりである。

- `src/` の `.rs` を `include_str!` で読む (ファイルの一覧はテストの中に書き、`src/` のファイルの数と一致することも `std::fs::read_dir` で確かめる。新しいファイルの載せ忘れを防ぐ)
- 行のコメント (`//`)、文書のコメント、文字列のリテラルを除く
- `#[cfg(test)]` の後の `mod 名前 { … }` を、括弧の対応で除く
- `PATTERNS` の各語を探し、見つけた所を含む一番内側の `fn`、`struct`、`enum`、`impl`、`assume_specification`、`global` の名前と組にする
- `verus! {` の外にある `unsafe` を探す。`unsafe impl Send for`、`unsafe impl Sync for`、`unsafe impl<M: Mode> Send for` と、`impl Drop for Heap` / `impl Drop for Pages` の中は除く

```rust
const PATTERNS: &[&str] = &[
    "external_body",
    "external_fn_specification",
    "external_type_specification",
    "external_trait_specification",
    "verifier::external",
    "assume_specification",
    "exec_allows_no_decreases_clause",
    "uninterp",
    "axiom",
    "global",
    "assume(",
    "admit(",
    "Tracked::assume_new",
    "Ghost::assume_new",
];
```

テストは4つ置く: `the_trust_list_is_exactly_the_trusted_items` (見つけた組の集合が `TRUSTED` と `UNPROVED` の和に等しい)、`no_unsafe_outside_verus`、`every_source_file_is_listed`、探し方の規則の小さいテスト `comments_strings_and_test_modules_are_skipped`。

Run: `cargo test -p eml_rt trust::`
Expected: `the_trust_list_is_exactly_the_trusted_items` は通る (まだ `verus!` がない)。`no_unsafe_outside_verus` は、`OUTSIDE` に全ファイルを載せる前は失敗する。全ファイルを `OUTSIDE` に載せて通す。

- [ ] **Step 3: 写しの証明を本体に移す**

写し (`crates/eml_rt/verus/*.rs`) の各項目を、本体の同じ名前の項目に置き換える。仕様と証明と spec fn はそのまま持ってくる。本体の項目を `verus! { }` で囲み、`use vstd::prelude::*;` を足す。

- `meta.rs`: `Kind`、`Kind::from_bits`、`Meta`、`Meta` の定数とメソッド。`Meta` に型の不変条件 `#[verifier::type_invariant] spec fn wf(self)` (種類のビットが既知の値で、予約のビットが 0) を付け、`Meta::kind` は `proof { use_type_invariant(self); }` で `from_bits` の `unreachable!` に届かないことを示す (spec の「権限の形」)。`Header` と生のポインタの関数 (`init`、`rc`、`meta`、`set_meta`、`slot`) は、このタスクでは `verus!` の外に残す (Task 2 で移す)。`fmt::Debug` の実装は外に残す
- `word.rs`: 5つの整数の関数と `Word::MIN_INT` / `MAX_INT`、`global size_of usize == 8`、補題。`Word` の型とメソッドは外に残す
- `rc.rs`: 全体。`dup_shared`、`decref_shared`、`is_last` は Task 2 で T2 に移すまで外に残す
- `heap/share.rs`: `Graph`、`ShareError`、`share_walk` と補題。`<[T]>::contains` の代わりに `contains_node` を書く:

```rust
fn contains_node<N: Copy + PartialEq>(s: &Vec<N>, x: N) -> (r: bool)
    ensures
        r == s@.contains(x),
```

`N` の `==` が値の等しさであることは、`Graph::Node` に spec の約束 (`Graph::node_eq`) を置かずに示せないので、`share_walk` は Task 1 では `Node = *mut Header` に限らず総称のまま、`contains_node` の比較を `Graph::same(a, b) -> (r: bool) ensures r == (a == b)` に任せる。`Objects` の `same` は Task 5 で番地の比較として証明する。`<[T]>::contains` の `assume_specification` と `obeys_concrete_eq` の前提を消す

- `heap/check.rs`: `layout_check`、`reuse_in_place_check`

Run: `cargo verus verify -p eml_rt`
Expected: S9a の写しの 67 個に近い数が verified で、0 errors。

- [ ] **Step 4: 写しと照合を消す**

`git rm -r crates/eml_rt/verus crates/eml_rt/src/verus_copies.rs`。`lib.rs` の `mod verus_copies;` を消す。

- [ ] **Step 5: clippy の `allow` を決める**

Run: `cargo clippy -p eml_rt --all-targets`
消した ghost のコードから出る警告だけを、`lib.rs` の頭に理由のコメント付きで `#![allow(...)]` する。本体のコードの警告は直す。

- [ ] **Step 6: verusfmt と全体の確かめ**

Run: `verusfmt crates/eml_rt/src/*.rs crates/eml_rt/src/heap/*.rs crates/eml_rt/src/heap/tests/*.rs && cargo fmt && cargo test && cargo verus verify -p eml_rt && cargo clippy --all-targets && cargo fmt --check && verusfmt --check $(git ls-files 'crates/eml_rt/src/*.rs')`
Expected: すべて通る。

- [ ] **Step 7: `check.sh` を指す記述を直す**

`CLAUDE.md` の Commands の `crates/eml_rt/verus/check.sh` の行を `cargo verus verify -p eml_rt         # Verus proofs of eml_rt (see testing.md)` にし、`verusfmt --check` の行を足す。`docs/implementation/testing.md` の「よく使うコマンド」の同じ行も直す。Verus の節の本文は Task 7 で書き換える。

- [ ] **Step 8: Commit**

```bash
git add -A crates/eml_rt Cargo.lock CLAUDE.md docs/implementation/testing.md
git commit  # "Verify eml_rt in place with vstd instead of Verus copies"
```

### Task 2: 語、ヘッダとペイロードの権限、T1 と T2、ページの確保器

**Files:**
- Modify: `crates/eml_rt/src/word.rs`、`meta.rs`、`rc.rs`、`alloc.rs`、`mode.rs`、`trust.rs`
- Create: `crates/eml_rt/src/meta/cell.rs` (T2)、`crates/eml_rt/src/alloc/os.rs` (T1)。`meta.rs` と `alloc.rs` は module のディレクトリにする (`meta/mod.rs`、`alloc/mod.rs`)

**Interfaces:**
- Consumes: Task 1 の `Meta`、整数の関数
- Produces: `Word` が `verus!` の中にあり、`pub closed spec fn bits_spec(self) -> usize` と `ptr_spec(self) -> *mut ()` を持つ。`Word::UNIT` / `EMPTY` は `pub exec const`。内部の `Word::from_header(h: *mut Header) -> Word`、`Word::header(self) -> *mut Header`
- Produces: `Header` は `#[repr(C)] struct Header { rc: i32, meta: u32 }`。`meta::{rc_ptr, meta_ptr, slot_ptr}` は番地の計算だけをする安全な関数 (`with_addr`)
- Produces: `Pages` が `verus!` の中にあり、`Pages::alloc(&mut self, words: usize) -> (h: *mut Header, Tracked<BlockRaw>)`、`Pages::free(&mut self, h, Tracked<BlockRaw>)`、`Pages::blocks(&self) -> Vec<*mut Header>`、`Pages::block_words`、`block_bytes`、`fits`、`bytes`。`BlockRaw` はブロックの `PointsToRaw` とそのブロックが属するページの番地を持つ tracked struct
- Produces: `Mode`、`Fast`、`Checked` が `verus!` の中にある (中身のない struct)

- [ ] **Step 1: `Mode` と `Word`**

`mode.rs` の `Fast` と `Checked` を `pub struct Fast;` / `pub struct Checked;` にし、全体を `verus!` に入れる。sealed の supertrait が Verus で通らなければ、`Sealed` を `verus!` の外に置き、`Mode: sealed::Sealed` を `#[verifier::external_trait_specification]` なしで書けるかを試し、だめなら sealed をやめて理由を spec に足す。

`word.rs` の `Word` を `verus!` に入れる。`Word::imm` は `requires fits_imm(n)` を付けず、`if !fits_imm(n) { not_immediate() }` (T5 の panic の関数、`const fn`) で止める。`imm_value` の `debug_assert` を外し、`requires self.is_imm_spec()` を付ける。`header()` は `NonNull` でなく `*mut Header` を返す。

- [ ] **Step 2: T2 の関数を書き、`rc.rs` の atomic の関数を移す**

`meta/cell.rs`:

```rust
verus! {

/// T0 の下で、権限を持つスレッドだけがこの欄を読み書きするときの値を約束する (docs/spec/object-model.md の「`unsafe` の核」)。
#[verifier::external_body]
#[inline]
pub(crate) fn rc_load(p: *mut i32, Tracked(perm): Tracked<&PointsTo<i32>>) -> (r: i32)
    requires
        perm.ptr() == p,
        perm.is_init(),
    ensures
        r == perm.value(),
{
    unsafe { AtomicI32::from_ptr(p) }.load(Ordering::Relaxed)
}

} // verus!
```

`rc_store`、`rc_fetch_sub_relaxed` (共有の `dup`)、`rc_fetch_add_acq_rel` (共有の `decref`、前の値を返す)、`rc_is_last` (今の `rc::is_last` の2回の読み込み)、`u32_load`、`u32_store` を同じ形で書く。`rc.rs` の `dup_shared`、`decref_shared`、`is_last` はこれに置き換えて消す。`rc.rs` の単体テスト `shared_counts_move_atomically_and_find_the_last_reference` と `is_last_is_true_only_at_one_and_minus_one` は、`AtomicI32` の代わりに `Box<i32>` の欄を作り、`Tracked::assume_new()` を渡して同じ値を確かめる (機械的な追随)。

`trust.rs` の `TRUSTED` に T2 の各関数を足す。

- [ ] **Step 3: ヘッダの権限**

`Header` の欄を `rc: i32` と `meta: u32` にする。`global layout Header is size == 8, align == 4;` を置く。`meta::{init, rc, meta, set_meta, slot}` を、権限を受ける形に置き換える:

```rust
pub(crate) fn rc_ptr(h: *mut Header) -> (p: *mut i32)
    ensures p@.addr == h@.addr, p@.provenance == h@.provenance,
pub(crate) fn meta_ptr(h: *mut Header) -> (p: *mut u32)
    ensures p@.addr == h@.addr + 4, p@.provenance == h@.provenance,
pub(crate) fn slot_ptr(h: *mut Header, i: usize) -> (p: *mut Word)
    requires h@.addr + 8 * (HEADER_WORDS + i) <= usize::MAX,
    ensures p@.addr == h@.addr + 8 * (HEADER_WORDS + i), p@.provenance == h@.provenance,
```

`meta` の読み書きは `ptr_ref(meta_ptr(h), Tracked(&perm.meta))` と `ptr_mut_write`。

- [ ] **Step 4: T1 と確保器**

`alloc/os.rs` に `alloc_page` と `dealloc_page` を書く。中身は今の `new_page` と `release_large` の `alloc` / `handle_alloc_error` / `dealloc`。約束は vstd の `allocate` / `deallocate` と同じ形で、揃えを `PAGE_BYTES` に固定する。

`new_page` の `u32::try_from(block_words)` の検査は、ページを確保する前に移す (T7: panic は状態を変える前)。空きリストの次のブロックは、provenance を持つ `Word` としてペイロードの先頭の語に書き、ポインタとして読み戻す。

`Pages` を `verus!` に入れる。各級の空きリストとページの一覧の権限を、`Pages` の `Tracked` の欄 (`pages: Map<usize, PagePerm>`、`free: Map<usize, BlockRaw>`) に持つ。`Pages::wf()` を閉じた述語にし、次を書く。

- ページの一覧の各ページは `PAGE_BYTES` に揃い、頭の `PageHeader` の `index` が一覧の添字に等しい
- 級の状態 (`Class`) の `cursor` と `end` は今のページの中にあり、`cursor` から `end` までの `PointsToRaw` をページの権限が持つ
- 空きリストの各ブロックはその級のページの中にあり、ペイロードの先頭の語に次のブロックを持つ
- `page_of(h)` の補題: ブロックの番地の下位 16 ビットを落とすと、ブロックを含むページの頭になる

`CLASS_OF` は今の `const` のまま置き、`class_of` に `ensures` (`r is Some ==> CLASS_WORDS[r] >= block_words`、級は最小) を付ける。Verus が `const` の中のループを扱えなければ、spec のとおり値で書き、`by (compute)` で中身を確かめる。

`for_each_block` を `blocks(&self) -> Vec<*mut Header>` に置き換える。`alloc.rs` のテストの `blocks()` の数え方は `pages.blocks().len()` にする (機械的な追随)。

`Drop for Pages` は `verus!` の外に置き、検証した `Pages::release_all(&mut self)` を呼ぶだけにする。

- [ ] **Step 5: 確かめとコミット**

Run: Global Constraints の全部。
Expected: 通る。`Heap` はまだ `verus!` の外で、`Pages` の新しい形を呼ぶ (`Tracked::assume_new()` は `verus!` の外の非テストのコードに書かない。`Heap` の側は、`Pages` が返す `Tracked` をそのまま捨てずに `Heap` の `Tracked` の欄に入れる。`Heap` を移すまでは `Tracked<Map<usize, BlockRaw>>` の欄を一時に置く)

```bash
git commit  # "Verify the eml_rt words, headers and page allocator"
```

### Task 3: `Heap` のモデル、確保と構築中の物体、RC、解放の連鎖、フィールド

**Files:**
- Create: `crates/eml_rt/src/heap/model.rs`、`crates/eml_rt/src/heap/perm.rs`
- Modify: `crates/eml_rt/src/heap/mod.rs`、`heap/tests/*.rs` (呼び方だけ)、`trust.rs`

**Interfaces:**
- Consumes: Task 2 の `Pages`、T2、ヘッダの権限
- Produces: `HeapView`、`BlockView`、`BlockState`、`Slot`、`Heap::view`、`safe_wf`、`model_wf`、`wf`、述語 `holds`、`readable`、`settled`、`allocated`。公開の各操作の `requires` / `ensures`

- [ ] **Step 1: モデルを書く**

`heap/model.rs` に spec の「抽象のモデル」をそのまま型にする。参照の数は `pub open spec fn refs(v: HeapView, a: usize) -> nat` (辺の数と `ext[a]` の和)、辺の数は `edges(v, a)` (生きている物体の `Written(w)` で `w` が `a` を指す語の数。`Kept` と token の箱と空いたブロックの語は数えない) とする。`model_wf` は次を含む。

- 生きている物体ごとに、張り付いていなければ rc の符号を除いた値が `refs` に等しく、張り付いていれば `rc.rs` の理想のモデルの張り付きの範囲にある
- 生きている物体の `Written(w)` の `w` は、即値か、生きている物体を指す
- 構築中の物体は `building` の1つだけ

- [ ] **Step 2: 失敗するテストを足す**

`heap/tests/reuse.rs` に `kept_fields_return_to_edges_after_con_at` (Review Focus) を足す。`release_reuse` で残した2つのフィールドを書かずに `con_at` し、次の `decref` までの間に RC が釣り合い、最後に `mortal(&heap) == 0` になることを、両方のモード (`both_modes!`) で確かめる。今の実装でも通るはずのテストで、Task 4 の証明がこの形を壊さないことを固定する。

- [ ] **Step 3: `Heap` を `verus!` に入れる**

`Heap` の欄に `perms: Tracked<Map<usize, BlockPerm>>` と `model: Ghost<HeapView>` を足す。`alloc_header`、`alloc`、`alloc_immortal`、`alloc_data`、`alloc_int`、`dup`、`dup_slow`、`decref`、`decref_slow`、`release_ref`、`free_chain`、`drain`、`free_box`、`is_unique`、`is_last`、`free_block`、`meta`、`field`、`set_field`、`raw`、`set_raw`、`scanned_len`、`scanned`、`unbox_int` に、spec の「検査モードの約束」の表のとおりに `requires` を書く。速いモードの前提は `!M::CHECKED ==> …`、安全の前提はモードによらず書く。`dup` と `decref` の速い道の形 (`#[inline]` の速い道、`#[cold]` の遅い道、rc が 1 なら `free_chain` を直に呼ぶ) は変えない。検査モードの検査の関数 (`check_live`、`check_owner`、`check_bounds`、`check_dup`、`check_decref`) は、このタスクでは `external_body` にして `UNPROVED` に載せる (Task 6 で証明する)。`Foreign` の関数は `external_body` にして `UNPROVED` に載せる (Task 7)

解放の連鎖の作業リストは `mem::swap` で取り出して戻す。`drain` の `while let` は `loop` と `match` にし、`decreases` に「まだ空けていない生きている物体の数」を書く。

- [ ] **Step 4: 確かめとコミット**

Run: Global Constraints の全部。
Expected: 通る。

```bash
git commit  # "Verify eml_rt allocation, RC and the free chain against a heap model"
```

### Task 4: `release`、`release_reuse`、token

**Files:**
- Modify: `crates/eml_rt/src/heap/release.rs`、`heap/reuse.rs`、`heap/model.rs`、`trust.rs`

**Interfaces:**
- Consumes: Task 3 のモデルと述語
- Produces: `release`、`release_children`、`release_unkept`、`release_reuse`、`into_token`、`con_at`、`reuse_in_place`、`drop_token` の `requires` / `ensures`。補題 `lemma_release_unrolled` (`is_last` と `free_block` を並べた遷移が `release` の遷移に等しい)

- [ ] **Step 1: token の箱の状態と `Kept` の語をモデルに足し、`release_reuse` の `ensures` を書く**
- [ ] **Step 2: `con_at` を構築中の物体として書き、`settled` の遷移 (`Kept` の語が `ext` から辺に移る) を書く**
- [ ] **Step 3: `lemma_release_unrolled` を書く**
- [ ] **Step 4: 確かめとコミット** (`"Verify eml_rt release and reuse tokens"`)

### Task 5: ヒープの上の `share`

**Files:**
- Modify: `crates/eml_rt/src/heap/share.rs`、`trust.rs`

**Interfaces:**
- Consumes: Task 1 の `share_walk`、Task 3 のモデル
- Produces: `Objects<M>` が権限の写像を持ち、`Graph` を実装する。`Graph::same` は番地の比較で、同じ番地のポインタは同じ provenance を持つ (`safe_wf` に、ブロックを指す語の provenance がページの provenance に等しいことを入れる)。`Heap::share` の `ensures` は `share_walk` の結論をモデルの言葉で書いたもの

- [ ] **Step 1: `Objects` を書き、`gv()` をモデルから作る**
- [ ] **Step 2: 検査モードの `Dead` と `LinReachedTwice` の結論を `share_walk` に足す**
- [ ] **Step 3: 確かめとコミット** (`"Verify share on the eml_rt heap"`)

### Task 6: 検査モード

**Files:**
- Modify: `crates/eml_rt/src/heap/check.rs`、`heap/walk.rs`、`heap/reuse.rs`、`heap/tests/checked.rs`、`trust.rs`

**Interfaces:**
- Consumes: Task 3 から Task 5 の全部
- Produces: `check_*` の証明、`Prefix` の権限、`safe_wf` と `consistent`、強めた範囲の検査、`live_objects`、`check_invariants`、`leak_counts` (種類ごとの数を返す検証した関数) と、`verus!` の外の `leaks` (`BTreeMap` を組む)

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests/checked.rs` に足す (Review Focus):

```rust
#[test]
fn raw_and_scanned_words_are_not_mixed_up() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 1, false);
    let f = heap.alloc_foreign(5u64, false);
    unsafe {
        assert_eq!(heap.set_raw(x, 0, 7), Err(HeapError::OutOfBounds));
        assert_eq!(heap.raw(x, 0), Err(HeapError::OutOfBounds));
        assert_eq!(heap.set_field(x, 1, Word::UNIT), Err(HeapError::OutOfBounds));
        assert_eq!(heap.set_raw(f, 0, 7), Err(HeapError::WrongKind));
        assert_eq!(heap.set_field(f, 0, Word::UNIT), Err(HeapError::OutOfBounds));
        heap.decref(x).unwrap();
        heap.decref(f).unwrap();
    }
}

#[test]
fn a_freed_word_is_reported_by_every_operation() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.decref(x).unwrap();
        assert_eq!(heap.field(x, 0), Err(HeapError::UseAfterFree));
        assert_eq!(heap.dup(x), Err(HeapError::UseAfterFree));
        assert_eq!(heap.decref(x), Err(HeapError::UseAfterFree));
        assert_eq!(heap.share(x, 1), Err(HeapError::UseAfterFree));
    }
    drop(heap);
}
```

Run: `cargo test -p eml_rt checked::raw_and_scanned`
Expected: FAIL (今の検査はブロックの容量だけを見るので `Ok` が返る)。

```rust
#[test]
fn a_child_reached_twice_is_freed_once() {
    // 呼ぶ側が dup せずに同じ語を2つのフィールドに書いた親。2回目に届いた子は UseAfterFree になる
    struct Probe(Arc<AtomicU32>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let finalized = Arc::new(AtomicU32::new(0));
    let mut heap = H::new();
    let f = heap.alloc_foreign(Probe(finalized.clone()), false);
    let p = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(p, 0, f).unwrap();
        heap.set_field(p, 1, f).unwrap();
        assert_eq!(heap.decref(p), Err(HeapError::UseAfterFree));
    }
    drop(heap);
    assert_eq!(finalized.load(Ordering::Relaxed), 1);
}
```

Run: `cargo test -p eml_rt checked::a_child_reached_twice`
Expected: FAIL (今は `decref` が `Ok` を返し、破棄処理が2回呼ばれる)。

- [ ] **Step 2: 空ける途中の状態と、範囲の検査を強める**

前置きの状態に「空ける途中」(`State::Dying`) を足す。`release_ref` が最後の参照と決めたとき、`release` の最後の参照の枝、`release_reuse` の子の処理の前で、検査モードなら前置きを `Dying` にする。`check_live` は `Alive` だけを生きているとみなす。生存の列挙、リーク、`Heap` を落とすときの `Foreign` の破棄処理は、`Dying` を `Alive` と同じに扱う。状態の欄の下位2ビットに4つの状態が収まるように、値を `Dying = 0` にする (`init_prefix` は `Alive` を書くので、0 が書き忘れと紛れることはない)。



`check_bounds` を `check_scanned(h, i)` (i は走査する語) と `check_raw(h, i)` (種類が `Data` で、i は走査する語の後でブロックの中) の2つにし、`field` / `set_field` と `raw` / `set_raw` で使い分ける。

Run: `cargo test -p eml_rt checked::`
Expected: PASS。ほかのテストも通る。

- [ ] **Step 3: `Prefix` の権限と `safe_wf`、各検査の証明**

Task 3 で `external_body` にした検査の関数を証明し、`UNPROVED` から外す。検査モードの各操作の `ensures` に spec の3つの性質 (`safe_wf` を保つ、前提が成り立てば `Ok` で同じ遷移、`Err` は違反があったときだけ) を書く。

- [ ] **Step 4: 生存の列挙、不変条件の検査、リーク**
- [ ] **Step 5: 確かめとコミット** (`"Verify the eml_rt check mode and tighten its index checks"`)

### Task 7: `Foreign`、`Drop`、`trusted` の Miri のテスト、計測、文書

**Files:**
- Modify: `crates/eml_rt/src/foreign.rs`、`heap/walk.rs`、`heap/mod.rs`、`trust.rs`
- Create: `crates/eml_rt/src/heap/tests/trusted.rs`
- Modify: 文書 (spec の「文書」の一覧の全部)、`docs/implementation/benchmarks.md`
- Delete: `docs/superpowers/specs/2026-10-14-s9v-verified-eml-rt-design.md`、この計画

**Interfaces:**
- Produces: T3 の `ForeignPerm`、`write`、`value`、`finalizer`、`Finalizer::run`。`UNPROVED` と `OUTSIDE` が空

- [ ] **Step 1: T3 を書き、`alloc_foreign`、`foreign`、`free_box` の破棄処理の道を証明する**
- [ ] **Step 2: `Drop for Heap` を `verus!` の外に置き、検証した `Heap::finalize_all(&mut self)` を呼ぶだけにする**
- [ ] **Step 3: `trusted` のテストを書く**

spec の信頼の一覧の「Miri で確かめること」を1つずつテストにする。名前は次のとおりにする。

- T1: `pages_are_aligned_and_returned`、`large_objects_get_their_own_page`、`dropping_a_heap_returns_every_page`
- T2: `local_and_shared_counts_move`、`is_last_reads_twice_only_at_minus_one`、`check_mode_prefixes_change`、`shared_counts_race_on_two_threads` (2つのスレッドが、同じ共有の rc の欄に T2 の `rc_fetch_sub_relaxed` と `rc_fetch_add_acq_rel` を並行に呼び、最後の参照を見るのがちょうど1つのスレッドであることを確かめる。別の worker のヒープで箱を空けることはオブジェクトモデルがまだ決めていないので、`Heap` は使わず欄だけを使う)
- T3: `a_foreign_value_of_another_type_is_wrong_kind`、`foreign_values_with_drop_are_finalized_once`、`a_zero_sized_foreign_value`、`a_panicking_finalizer_is_not_run_twice`
- T7: `a_panicking_finalizer_in_a_chain_leaks_the_rest`
- T8: `every_core_operation_once_fast`、`every_core_operation_once_checked`

`heap/tests/mod.rs` に `mod trusted;` を足す。全体を Miri で流したテストの `cfg!(miri)` の分岐は、`trusted` が使わなければ外す。

Run: spec の「Miri」の2つのコマンド
Expected: 通る。かかった時間を記録する。

- [ ] **Step 4: 計測**

Run: `bench/rt.sh`
Expected: 上の「計測の基準」から、どの場合も 0.5% を超えて増えない。超えたら原因を調べる。

- [ ] **Step 5: 文書**

spec の「文書」の一覧の所をすべて書き換える。`docs/future/roadmap.md` の S9 の作業の表に S9v の行を「終えた」で足す。`CLAUDE.md` の Architecture と Testing の `eml_rt` の記述を直す。

- [ ] **Step 6: spec と計画を消し、確かめてコミット**

Run: Global Constraints の全部と Miri。`rg -n "s9v-verified|2026-10-14-s9v" docs crates CLAUDE.md` が何も出さない。

```bash
git commit  # "Document the verified eml_rt and delete the S9v design and plan"
```

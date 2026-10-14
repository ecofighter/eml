# S9b-2a 凍結した領域 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_rt` に、不死の定数を置く凍結した領域 (`Frozen<M>` の鎖) を足す。ヒープは付けた塊とその祖先のブロックを指す語を受け付け、どの操作も領域に書き込まない。Verus は読むだけの共有の権限でこれを証明し、速いモードの速い道は変えない。

**Architecture:** 4つのタスクで積む。Task 1 は `string_push` が容量を一意な左辺のときだけ読むようにする (独立した小さい変更)。Task 2 は `Frozen<M>` と、塊を付けていない作り手の `freeze`、`Drop`、`Send + Sync`。Task 3 は `attach` と、核のモデルを `has(h)` (自分のブロックか、付けた塊とその祖先のブロック) に広げる作業と、親の塊を持つ `freeze`。Task 4 はスレッドのテスト、計測、文書。

**Tech Stack:** Rust (edition 2024、fenix の 1.98.1)、Verus 0.2026.10.04、vstd 0.0.0-2026-10-04-0306 (`vstd::shared::Shared`、`vstd::raw_ptr::PointsTo::is_disjoint`)、verusfmt、Miri (fenix の nightly)、macOS の `/usr/bin/time -l`。

**Spec:** `docs/superpowers/specs/2026-10-14-s9b2a-frozen-region-design.md`。実装する人は、各タスクの前に spec の該当する節を読む。spec が規範で、計画とずれたら spec に従い、ずれを報告する。調べた結果の地図は、spec を書くときに作った `crates/eml_rt` のコードの地図である (各タスクの指示に要点を写す。古い行番号は今のコードで確かめる)。

| 見出し | 中身 |
|---|---|
| Task 1 | `string_push` は一意な左辺のときだけ容量を読む |
| Task 2 | `Frozen<M>`、親のない `freeze`、`Drop`、`Send + Sync`、`Arc::clone` の約束 |
| Task 3 | `attach`、`has(h)` へのモデルの拡張、親の塊を持つ `freeze` |
| Task 4 | 2つのスレッドのテスト、計測、文書、spec と計画の削除 |

## Global Constraints

- 各タスクの終わりに次が通る。途中のタスクでもワークスペースを壊したままコミットしない
  - `cargo test` (ワークスペース全体)
  - `cargo verus verify -p eml_rt` が 0 errors。`target/**/.fingerprint/eml_rt-*` を消してから流し、最後の行の `verification results:: N verified, 0 errors` をコミットメッセージに書く
  - `cargo clippy --all-targets` が警告を出さない
  - `cargo fmt --check` と `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`。新しいファイルは `git add` してから流す。verusfmt は `unsafe { .. }?` を読めない
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、それに `-Zmiri-tree-borrows` を足したもの
  - `bench/rt.sh` のどの計測も、`docs/implementation/benchmarks.md` の「S9b-1 (文字列)」の値から +0.5% を超えて増えない (Task 1 と Task 3 の終わりで測る。ほかのタスクは速い道に触れないことを差分で確かめる)
- 検証が通っても `cargo build` が通るとは限らない。どのステップでも両方を流す。`vstd::shared::Shared` は Verus のビルドでだけ現れるので、その型の欄と使う所に `#[cfg(verus_keep_ghost)]` を付ける
- `eml_rt` の外の crate は変えない (Task 4 の `bench/` を除く)。UI テストと `bench/` のスナップショットは変わらない
- `unsafe` は、信頼の一覧の項目の中と、`verus!` の外の `unsafe impl Send` / `Sync` と T4 の `Drop` にだけ書く。信頼の一覧の新しい項目は、T0、T4、T5 のどれかの行に入れ、`trust.rs` の `TRUSTED` に足す。新しい T の番号は作らない
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則は `docs/` のパスと見出しで引く。この計画と spec は Task 4 で消すので引かない。`eml_cli` の `citations::every_cited_heading_exists` は引いた見出しが在ることを確かめるので、新しい節「凍結した領域」を書く Task 4 より前は、今ある見出し (「核の操作」「不死の物体の数え方」など) を引く。コメントに計画のタスクの番号を書かない
- コミットメッセージは英語で、既存の形 (命令形の1行目、本文に理由) に合わせる。各コミットの末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014bwniBqkZgCLERCaKyDB5S
  ```

- `git diff` は `--no-ext-diff` で見る
- コマンドはすべて devShell の中で流す (direnv の入ったシェルか `nix develop -c ...`)

## 名前 (各タスクで共通に使う)

- 塊: `pub struct Frozen<M: Mode>` (`crates/eml_rt/src/frozen.rs`、新しいファイル)。欄は、作り手から移した `pages: Pages`、親 `parent: Option<Arc<Frozen<M>>>`、ghost の読むだけの権限 `perms: Tracked<Shared<RegionPerms>>` (`#[cfg(verus_keep_ghost)]`)、`_mode: PhantomData<M>`
- 塊の権限: `pub(crate) tracked struct RegionPerms`。この塊の生きているブロックの権限 `blocks: Map<usize, BlockPerm>`、ページの頭の読む権限 `headers: Map<usize, PointsTo<PageHeader>>`、親の塊の権限 `parent: Option<Shared<RegionPerms>>`。祖先の権限は、親の `Shared` の写しで持つ
- 塊の仕様の関数: `pub closed spec fn has(&self, h: usize) -> bool` (この塊か祖先のブロック)、`pub closed spec fn depth(&self) -> nat` (鎖の長さ。再帰の `decreases` に使う)
- 塊の不変条件: Verus の型の不変条件 `#[verifier::type_invariant] spec fn inv(self)`。どのブロックも生存で rc 0、ブロックの形が正しい (`block_shape` と同じ)、検査モードなら前置きの状態が生存で数が `UNCOUNTED`、走査する語は即値か `has` のブロック、`blocks` のドメインが有限、親の鎖が `inv` を満たす、この塊のブロックと祖先のブロックが重ならない
- ヒープの側: `Heap<M>` の欄 `region: Option<Arc<Frozen<M>>>` と、`Core` の ghost の欄 `region: Tracked<Option<Shared<RegionPerms>>>` (`Arc` が指す塊の `perms` の写し)。`Core::has(h) = self.dom().contains(h) || (region に h がある)`
- 公開の操作: `Heap<M>::freeze(self) -> Result<Frozen<M>, HeapError>`、`Heap<M>::attach(&mut self, region: Arc<Frozen<M>>)`
- テスト: `crates/eml_rt/src/heap/tests/frozen.rs` (新しいファイル、`both_modes!`)、`heap/tests/trusted.rs` の `// 凍結した領域` の節

## Review Focus

- 領域の文字列を左辺にした `string_push` と `string_concat`: 写す道に進み、領域の中身も前置きも変わらず、速いモードはページの頭を読まない。Task 1 の `push_onto_a_literal_copies_without_reading_capacity` と Task 3 の `region_words_go_through_every_operation`
- ヒープの物体のフィールドに領域の語を入れて、その物体を解放の連鎖で手放すこと: 領域の物体は解放されず、`check_invariants` も通る。Task 3 の `own_objects_hold_region_words`
- 子孫でない塊の `attach` (兄弟の塊、無関係の塊): panic し、ヒープは前の塊を付けたまま使える。Task 3 の `attaching_a_non_descendant_panics`
- `Arc` を落とす順 (ヒープが先、塊が先、子の塊が先) と、長い鎖 (10 000 個の塊) の `Drop`: 漏れず、スタックを溢れさせない。Task 2 の `long_chains_drop_without_recursion` と Task 3 の `drop_order_does_not_matter`
- 検査モードで、凍結の後の領域の物体への `acquire_immortal`、`dup`、`decref` の繰り返し: どれも `Ok` で、前置きの数は `UNCOUNTED` のまま。Task 3 の `region_objects_are_not_counted`

---

### Task 1: `string_push` は一意な左辺のときだけ容量を読む

**Files:**
- Modify: `crates/eml_rt/src/heap/string.rs` (`push_unchecked`)
- Modify: `crates/eml_rt/src/heap/tests/string.rs`

**Interfaces:**
- Produces: 速いモードの `string_push` と `string_concat` は、左辺が一意でなければページの頭を読まない。公開の API は変えない

- [ ] **Step 1: 失敗するテストを書く**

左辺が一意でないときにページの頭を読まないことは、外から直接は見えない。そこで、左辺の容量が足りない共有の文字列で、写す道の結果が正しいことと、Task 3 で領域の語に同じ道を使うことの前提を確かめるテストを足す (この Step のテストは今のコードでも通る。目的は、次の Step の並べ替えで道を壊さないことを固定することである)。

```rust
fn push_onto_a_literal_copies_without_reading_capacity<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // 不死の左辺 (後の領域のリテラルと同じく rc 0)
    let lit = heap.alloc_string_immortal("literal");
    unsafe { heap.acquire_immortal(lit) }.unwrap();
    let r = unsafe { heap.string_push(lit, " + tail") }.unwrap();
    assert_ne!(r, lit);
    assert_eq!(text(&heap, r), "literal + tail");
    assert_eq!(text(&heap, lit), "literal");
    unsafe { heap.decref(r) }.unwrap();
}
```

`both_modes!` の一覧に名前を足す。

Run: `cargo test -p eml_rt string::`
Expected: PASS (今のコードでも通る)

- [ ] **Step 2: 容量の読み込みを一意な枝に移す**

`push_unchecked` は今、一意かどうかを見る前に `cap_get(h)` を読んでいる (spec の地図では `heap/string.rs` の `push_unchecked` の中、`cap_get(h)` の行)。一意 (`rc == 1`) と分かった後の枝でだけ読むように並べ替える。共有と不死の左辺は `need` だけで確保するので、容量は使わない。証明で容量の値が要る所は ghost の `self.core.cap(h)` を使う。

関数の頭に、なぜここで読まないかを1行で書く (速いモードは rc 0 の物体のページの頭を読まない。docs/spec/object-model.md の「文字列」)。

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt string::`
Expected: 通る

- [ ] **Step 3: 計測して、全体を確かめてコミットする**

Run: `bench/rt.sh` を2回。`append` を含むどの計測も「S9b-1 (文字列)」の値から +0.5% を超えて増えない (`append` は一意な左辺の道なので変わらないはずである)。Global Constraints の全体の確かめを流す。

```bash
git add crates/eml_rt
git commit -m "Read a string's capacity only when the left side is unique"
```

---

### Task 2: `Frozen<M>` と親のない `freeze`

**Files:**
- Create: `crates/eml_rt/src/frozen.rs`
- Modify: `crates/eml_rt/src/lib.rs` (`mod frozen;`、`pub use frozen::Frozen;`、`Send + Sync` の型の assert)
- Modify: `crates/eml_rt/src/heap/mod.rs` (`freeze`)、必要なら `heap/walk.rs` (ページを歩いて確かめる部分を共有する)
- Modify: `crates/eml_rt/src/alloc.rs` (ページの頭の読む権限を取り出す、各級の切り出しの位置をページの頭に書き戻す)
- Modify: `crates/eml_rt/src/trust.rs` (`FILES` に `frozen.rs`、`TRUSTED` に T4 の `Drop for Frozen`、T5 の `Arc::clone` の `assume_specification`)
- Create: `crates/eml_rt/src/heap/tests/frozen.rs`。Modify: `heap/tests/mod.rs` (`mod frozen;`)、`heap/tests/trusted.rs`

**Interfaces:**
- Produces: `Frozen<M>`、`RegionPerms`、`Frozen::has`、`Frozen::depth`、`Frozen` の型の不変条件 `inv`、`Heap<M>::freeze` (この Task では、塊を付けていない作り手だけを受ける。付けている作り手は Task 3 で受ける)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_rt/src/heap/tests/frozen.rs` を作り、`heap/tests/mod.rs` に `mod frozen;` を足す。

```rust
//! 凍結した領域 (docs/spec/object-model.md の「不死の物体の数え方」)。

use std::sync::Arc;

use super::string::text;
use crate::{Frozen, Heap, HeapError, Kind, Meta, Mode, Word};

/// 文字列の定数と、即値に収まらない Int の定数を持つ作り手。
pub(super) fn builder<M: Mode>() -> (Heap<M>, Word, Word) {
    let mut b = Heap::<M>::new();
    let s = b.alloc_string_immortal("constant");
    let big = unsafe { b.alloc_immortal(1, Meta::new(0, 0, Kind::BigInt, false)) };
    unsafe { b.set_raw(big, 0, i64::MAX as u64) }.unwrap();
    (b, s, big)
}

fn freezing_immortal_constants_succeeds<M: Mode>() {
    let (b, s, big) = builder::<M>();
    let region: Frozen<M> = b.freeze().unwrap();
    let region = Arc::new(region);
    // 付ける前でも、塊は自分の物体を落とさずに持つ
    drop(region);
    let _ = (s, big);
}

fn freezing_a_mortal_object_is_refused<M: Mode>() {
    let (mut b, _, _) = builder::<M>();
    let _cell = b.alloc_data(0, 1, 0, false);
    assert_eq!(b.freeze().unwrap_err(), HeapError::NotImmortal);
}

fn freezing_a_token_is_refused<M: Mode>() {
    let (mut b, _, _) = builder::<M>();
    let cell = b.alloc_data(0, 0, 1, false);
    let token = unsafe { b.into_token(cell) }.unwrap();
    let _ = token;
    assert_eq!(b.freeze().unwrap_err(), HeapError::NotImmortal);
}

fn freezing_a_foreign_object_is_refused<M: Mode>() {
    let (mut b, _, _) = builder::<M>();
    // Foreign の物体は rc 0 にできないので、rc の確かめで断られる
    let _f = b.alloc_foreign(7u64, false);
    assert_eq!(b.freeze().unwrap_err(), HeapError::NotImmortal);
}

fn freezing_after_freeing_keeps_only_live_blocks<M: Mode>() {
    let (mut b, s, _) = builder::<M>();
    // 空けたブロック (検査モードでは隔離) があっても凍結できる
    let tmp = b.alloc_string("temporary");
    unsafe { b.decref(tmp) }.unwrap();
    let region = b.freeze().unwrap();
    drop(region);
    let _ = s;
}

fn long_chains_drop_without_recursion<M: Mode>() {
    // Task 3 で鎖を作れるようになるまでは、1つの塊を落とす
    let (b, _, _) = builder::<M>();
    drop(Arc::new(b.freeze().unwrap()));
}

both_modes!(
    freezing_immortal_constants_succeeds,
    freezing_a_mortal_object_is_refused,
    freezing_a_token_is_refused,
    freezing_a_foreign_object_is_refused,
    freezing_after_freeing_keeps_only_live_blocks,
    long_chains_drop_without_recursion,
);
```

`lib.rs` の `tests` に、`Frozen<Fast>` と `Frozen<Checked>` が `Send + Sync` であることの型の assert を足す (`send_and_sync::<Frozen<Fast>>()` の形)。

Run: `cargo test -p eml_rt frozen::`
Expected: コンパイルが失敗する (`Frozen` と `freeze` がない)

- [ ] **Step 2: `Frozen<M>` を書く**

`crates/eml_rt/src/frozen.rs`:

```rust
//! 凍結した領域 (docs/spec/object-model.md の「不死の物体の数え方」)。不死の定数を、どのヒープにも属さない読み出し
//! 専用の塊に置く。塊は親の塊への `Arc` を持ち、鎖になる。凍結した後はどの操作も書き込まないので、権限は vstd の
//! `Shared` で読むだけの形にして、付けたヒープに写す。

use std::marker::PhantomData;
use std::sync::Arc;

use vstd::prelude::*;

use crate::alloc::Pages;
use crate::mode::Mode;

verus! {

pub struct Frozen<M: Mode> {
    // `Drop` が返すページの一覧。凍結の後は `Drop` だけが読む
    pages: Pages,
    parent: Option<Arc<Frozen<M>>>,
    #[cfg(verus_keep_ghost)]
    perms: Tracked<Shared<RegionPerms>>,
    _mode: PhantomData<M>,
}

} // verus!
```

- `RegionPerms` (上の「名前」) は `meta.rs` か `frozen.rs` に置く。`BlockPerm` の欄を読むのは `meta.rs` の中だけ (`text` の扱いと同じ) なので、ブロックの権限を `Map` に入れるだけなら `frozen.rs` に置ける
- 型の不変条件 `inv` (上の「名前」) を書く。`has` と `depth` は親の鎖をたどる再帰の spec で、`decreases self.depth()` を使う
- `Drop for Frozen<M>` は `verus!` の外に置く (T4)。親への `Arc` を、強い参照が1つのあいだ `Arc::into_inner` で取り出して手放すループにし、再帰しない。`pages` は、今の `Pages` の `Drop` (T4) が返す
- `Frozen` は `Send + Sync` を `unsafe impl` で持つ (`verus!` の外。`trust.rs` の `UNSAFE_OUTSIDE_ALLOWED` が許す形)。理由のコメントに、凍結の後は書き込まないこと (T0 の改め) を書く

- [ ] **Step 3: `freeze` を書く**

`heap/mod.rs` の `impl<M: Mode> Heap<M>`:

```rust
/// 作り手のヒープを凍結した塊にする。生きているブロックがどれも rc 0 の不死の物体で、token の箱と `Foreign` の
/// 物体がないことを、実行時に確かめる (docs/spec/object-model.md の「不死の物体の数え方」)。
pub fn freeze(self) -> (r: Result<Frozen<M>, HeapError>)
    requires
        self.wf(),
        self.safe_settleable(),
        !M::CHECKED ==> self.settleable(),
    ensures
        r matches Ok(f) ==> f.inv(),
```

本体は次の順に書く。

1. `self.settle()` (`freeze` は `self` を値で受けるので、`let mut b = self;` にしてから)
2. ページを歩く (`live_objects` と同じ歩き方。速いモードは種類が `Free` でないブロック、検査モードは前置きの状態が解放でないブロック)。各ブロックについて、検査モードでは前置きの状態が生存 (`ALIVE_STATE`。空ける途中は断る) で、どちらのモードでも rc が 0 であることを確かめ、そうでなければ `NotImmortal` を返す。token の箱と `Foreign` の物体は rc 1 なので、ここで断られる
3. 検査モードでは、生きているブロックの前置きの数の欄を `UNCOUNTED` にする (今の `prefix_first_put`)。ダミーはすでに `UNCOUNTED` である
4. 各級の今のページの切り出しの位置をページの頭に書き戻す (`Pages` に関数を足す。今のページを替えるときの書き戻しと同じ処理)
5. `Pages` と、生きているブロックの権限を取り出す。`Heap` は `Drop` を持つので、`b` の `core` (と `dummy`) を `Heap::new()` の新しいものと `mem::replace` で入れ替え、入れ替えた後の `b` を普通に落とす (`Drop` の `finalize_all` が求める `wf` を保つため)。ページの頭の読む権限を `Pages` の `pages` から取り出して `RegionPerms::headers` に入れ、ブロックの権限を `RegionPerms::blocks` に入れ、`Shared::new` で包む
6. `Frozen { pages, parent: None, perms, _mode }` を返す。この Task では、作り手が塊を付けていないことを `requires` にしてよい (Task 3 で外す)。`inv` の「重ならない」は、親がないので、この塊の中のブロックどうしだけになる (`Core` の不変条件からすでに出る)

`Arc::clone` は Task 3 で使う。`assume_specification` (`ensures` は「同じ値を指す `Arc` を返す」) は、使う Task で足す。

`trust.rs`: `FILES` に `frozen.rs`、`TRUSTED` に `Drop for Frozen` (T4) を足す。

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt frozen:: trust::`
Expected: 通る

- [ ] **Step 4: Miri のテストを足す**

`heap/tests/trusted.rs` に `// 凍結した領域` の節を足し、`both_modes!` に名前を足す。

```rust
fn frozen_pages_are_returned<M: Mode>() {
    let (b, _, _) = super::frozen::builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    // Miri は返していないページを漏れとして誤りにする
    drop(region);
}
```

Run: `cargo test -p eml_rt trusted::` と Miri の2つの設定
Expected: 通る

- [ ] **Step 5: 全体を確かめてコミットする**

Run: Global Constraints の全体の確かめ (計測は、速い道に触れていないことを `git diff --no-ext-diff` で確かめて省いてよい)

```bash
git add crates/eml_rt
git commit -m "Add frozen regions of immortal constants to eml_rt"
```

本文に、`Frozen` の形、`freeze` の確かめ、`Drop` (T4)、`Send + Sync` (T0 の改め) を書き、verified の数を付ける。

---

### Task 3: `attach`、`has(h)` へのモデルの拡張、親の塊を持つ `freeze`

**Files:**
- Modify: `crates/eml_rt/src/heap/inv.rs`、`heap/safe.rs`、`heap/core.rs`、`heap/check.rs`、`heap/field.rs`、`heap/proof.rs`、`heap/handover.rs`、`heap/release.rs`、`heap/reuse.rs`、`heap/share.rs`、`heap/walk.rs`、`heap/string.rs`、`heap/mod.rs`、`alloc.rs`
- Modify: `crates/eml_rt/src/frozen.rs` (`parent` の鎖、`has` の再帰)
- Modify: `crates/eml_rt/src/trust.rs` (T5 に `Arc::clone` の `assume_specification`)
- Modify: `crates/eml_rt/src/heap/tests/frozen.rs`

**Interfaces:**
- Consumes: Task 2 の `Frozen`、`RegionPerms`、`has`、`depth`、`inv`、`freeze`
- Produces: `Heap<M>::attach`、`Core::has`、`Heap` の `region` の欄、親を持つ `freeze`

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests/frozen.rs` に足し、`both_modes!` の一覧に名前を足す。

```rust
/// 定数の塊を作って付けたヒープと、その定数。
fn attached<M: Mode>() -> (Heap<M>, Arc<Frozen<M>>, Word, Word) {
    let (b, s, big) = builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    let mut heap = Heap::<M>::new();
    heap.attach(region.clone());
    (heap, region, s, big)
}

fn region_words_go_through_every_operation<M: Mode>() {
    let (mut heap, _region, s, big) = attached::<M>();
    unsafe {
        heap.acquire_immortal(s).unwrap();
        heap.dup(s).unwrap();
        heap.decref(s).unwrap();
        assert_eq!(heap.meta(s).unwrap().kind(), Kind::String);
        assert!(!heap.is_unique(s).unwrap());
        assert!(!heap.is_last(s).unwrap());
        assert_eq!(heap.unbox_int(big).unwrap(), i64::MAX);
        assert_eq!(heap.scanned_len(s).unwrap(), 0);
        // 領域の左辺は写す道に進む。string_push は手元の参照を1つ消費する
        let r = heap.string_push(s, "!").unwrap();
        assert_ne!(r, s);
        assert_eq!(text(&heap, r), "constant!");
        // string_concat(s, s) は参照を2つ消費する
        heap.acquire_immortal(s).unwrap();
        heap.acquire_immortal(s).unwrap();
        let c = heap.string_concat(s, s).unwrap();
        assert_eq!(text(&heap, c), "constantconstant");
        heap.decref(r).unwrap();
        heap.decref(c).unwrap();
        heap.acquire_immortal(s).unwrap();
        heap.share(s, 1).unwrap();
        heap.decref(s).unwrap();
    }
    assert_eq!(text(&heap, s), "constant");
}

fn own_objects_hold_region_words<M: Mode>() {
    let (mut heap, _region, s, big) = attached::<M>();
    let pair = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.acquire_immortal(s).unwrap();
        heap.acquire_immortal(big).unwrap();
        heap.set_field(pair, 0, s).unwrap();
        heap.set_field(pair, 1, big).unwrap();
        // 残すフィールドを持つ release と、解放の連鎖の両方を通す
        heap.dup(pair).unwrap();
        heap.release(pair, 0, &[true, false]).unwrap();
        heap.decref(s).unwrap();
        heap.decref(pair).unwrap();
    }
    if let Some(check) = (&heap as &dyn std::any::Any).downcast_ref::<Heap<crate::Checked>>() {
        check.check_invariants().unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
    assert_eq!(text(&heap, s), "constant");
}

fn region_objects_are_not_counted<M: Mode>() {
    let (mut heap, _region, s, _) = attached::<M>();
    for _ in 0..3 {
        unsafe { heap.acquire_immortal(s) }.unwrap();
    }
    for _ in 0..5 {
        // 数えないので、手放しすぎも誤りにしない (docs/spec/object-model.md の「不死の物体の数え方」)
        unsafe { heap.decref(s) }.unwrap();
    }
    if M::CHECKED {
        assert_eq!(unsafe { super::raw::prefix_first(s) }, u32::MAX);
    }
}

fn chains_hold_constants_that_point_into_ancestors<M: Mode>() {
    let (b1, s1, _) = builder::<M>();
    let r1 = Arc::new(b1.freeze().unwrap());
    let mut b2 = Heap::<M>::new();
    b2.attach(r1.clone());
    // 祖先の定数を指す定数 (Some s1 の形)
    let some = unsafe { b2.alloc_immortal(1, Meta::new(1, 1, Kind::Data, false)) };
    // set_field は呼ぶ側が数えた参照を1つ渡す
    unsafe { b2.acquire_immortal(s1) }.unwrap();
    unsafe { b2.set_field(some, 0, s1) }.unwrap();
    let r2 = Arc::new(b2.freeze().unwrap());
    drop(r1);
    let mut heap = Heap::<M>::new();
    heap.attach(r2.clone());
    let inner = unsafe { heap.field(some, 0) }.unwrap();
    assert_eq!(text(&heap, inner), "constant");
}

fn attach_moves_to_a_descendant<M: Mode>() {
    let (b1, _, _) = builder::<M>();
    let r1 = Arc::new(b1.freeze().unwrap());
    let mut b2 = Heap::<M>::new();
    b2.attach(r1.clone());
    let s2 = b2.alloc_string_immortal("second");
    let r2 = Arc::new(b2.freeze().unwrap());
    let mut heap = Heap::<M>::new();
    heap.attach(r1.clone());
    heap.attach(r2.clone());
    assert_eq!(text(&heap, s2), "second");
}

#[should_panic]
fn attaching_a_non_descendant_panics<M: Mode>() {
    let (b1, _, _) = builder::<M>();
    let (b2, _, _) = builder::<M>();
    let r1 = Arc::new(b1.freeze().unwrap());
    let r2 = Arc::new(b2.freeze().unwrap());
    let mut heap = Heap::<M>::new();
    heap.attach(r1);
    heap.attach(r2);
}

fn drop_order_does_not_matter<M: Mode>() {
    for heap_first in [true, false] {
        let (heap, region, _, _) = attached::<M>();
        if heap_first {
            drop(heap);
            drop(region);
        } else {
            drop(region);
            drop(heap);
        }
    }
}
```

`both_modes!` は `#[should_panic]` を付けた関数を受けないので、`attaching_a_non_descendant_panics` は `both_modes!` の外で、モードごとに `#[test] #[should_panic]` の2つの関数を書く。

`long_chains_drop_without_recursion` (Task 2) を、10 000 個の塊の鎖を作って落とす形に書き直す (作り手が前の塊を付けて `freeze` するのを繰り返し、最後の `Arc` だけを持って落とす)。

Run: `cargo test -p eml_rt frozen::`
Expected: コンパイルが失敗する (`attach` がない)

- [ ] **Step 2: ヒープに塊の欄を足し、`has` を定める**

- `Heap<M>` に `region: Option<Arc<Frozen<M>>>`、`Core` に ghost の `region: Tracked<Option<Shared<RegionPerms>>>` (`#[cfg(verus_keep_ghost)]`) を足す。`Heap::new` は両方を `None` にする
- `Core::has(h)` を `inv.rs` に足す (自分のドメインか、`region` の `Shared` の `has`)。`perm`、`block`、`cap`、`state` を、領域のブロックでは塊の権限から読む形に広げる (状態は生存)。`Core` の欄を並べている枠の spec (`recounted`、`rc_changed`、`prefix_moved`、`slot_changed`、`text_changed`、`safe_step`、`bumped`、`dupped`、`taken`、`frame`、`evolved`、`evolved_by`) に `region == old.region` を足す
- `word_ok`、`edge_wf` の行き先、`safe_slot` を `has` にする
- 不変条件に「自分のブロックと、`region` のブロックは重ならない」を足す (`wf` の速いモードの `full` と、検査モードの `safe_wf` の両方)

Run: `cargo verus verify -p eml_rt`
Expected: `has` を使う所の証明が通らない。次の Step で直す

- [ ] **Step 3: 読む補助の関数と RC の遷移を広げる**

- `core.rs` の読む補助の関数 (`rc_get`、`meta_get`、`slot_get`、`scan_get`、`rc_last`、`cap_get`、`prefix_state_get`、`prefix_first_get`) の `requires` を `dom().contains(h)` から `has(h)` に替え、借りる先を ghost の条件で選ぶ (`carved_meta` と同じ形。自分のドメインなら `self.perms`、そうでなければ `region` の `Shared` の `borrow`)。`cap_get` は、領域のブロックでは `RegionPerms::headers` のページの頭を読む
- `check.rs` の `checked_live`、`checked_owner`、`immortal_counted`、`checked_dup`、`checked_decref`、`checked_decref_twice` と、`header_of`、`scanned_check`、`raw_check`、`release_check`、`checked_string` を同じく広げる
- `count_ghost`、`count_model` に、rc 0 の行き先の場合を足す。状態と数を変えず、辺 (`edge` が `Some` なら) だけを外す。`release_ref` はこれに頼る
- 書き込む操作 (`rc_put`、`slot_put`、`prefix_*_put` など) は自分のドメインだけを受けるままにする。rc が 0 でない物体と、`unshared` の物体は自分のブロックであることを補題で示す (領域のブロックはいつも rc 0 で生存なので)
- 領域の物体の数は `UNCOUNTED` なので、`immortal_counted` は偽を返し、`count_immortal` は呼ばれない。これを `has` の領域の側の事実 (`Frozen::inv`) から示す

Run: `cargo verus verify -p eml_rt`
Expected: 残りは操作ごとの証明の場合分けである

- [ ] **Step 4: 操作ごとの証明と `share` と歩く処理を広げる**

- `release_shared`: 領域の物体からの辺はないので、残すフィールドの `dup` は領域の子に当たってよい
- `push_unchecked`、`make_text`、`write_tail`: 領域の左辺と右辺は、`Shared` から借りた権限で読む
- `string`、`foreign`、`unbox_int`、`field`、`scanned`: 借りる先を ghost の条件で選ぶ
- `lemma_readable`、`lemma_live_slot`、`lemma_safe_live_slot` に領域の場合を足す
- `share.rs`: `Graph` の `set_rc` と `set_owner` に `rc != 0` の `requires` を足す (たどりはその呼び出しの所で rc が 0 でないことを知っている)。`node_ok`、`nodes_of`、`graph_of`、`same_frame` を `has` に広げ、`Objects` に `region` の写しを持たせる
- `walk.rs`: `alive` と `child_ok` を `has` に広げる。`live_objects`、`leak_entries`、`finalize_all` は自分のページだけを歩くまま変えない
- 確保の道 (`alloc_header` と、`publish` の前): 新しいブロックが `region` のブロックと重ならないことを、自分のページの権限 (`&mut`) と塊のページの頭の権限 (`&`) で `is_disjoint` を使って示す。ページの単位で示すと、級の切り出しと大きい物体の両方を1つの補題で覆える

Run: `cargo verus verify -p eml_rt`
Expected: 0 errors

- [ ] **Step 5: `attach` と、親を持つ `freeze` を書く**

`heap/mod.rs`:

```rust
/// 付けている塊を、その子孫の塊に替える。何も付けていなければ、そのまま付ける。子孫でなければ panic する
/// (docs/spec/object-model.md の「不死の物体の数え方」)。付けた後、このヒープは塊とその祖先のブロックを指す語を
/// 受け付ける。
pub fn attach(&mut self, region: Arc<Frozen<M>>)
    requires
        old(self).wf(),
    ensures
        final(self).wf(),
        old(self).consistent() ==> final(self).consistent(),
```

本体は次の順に書く。

1. 子孫かどうかを、`region` から親をたどって確かめる (`Arc::ptr_eq` で今の塊と比べる)。今の塊が `None` ならいつも通る。子孫でなければ panic する (T5 の panic の関数を足すか、今の `too_large` と同じ形の関数を足す)
2. ghost: 自分のブロックの権限と、新しく見えるブロック (新しい塊と、今の塊より新しい祖先) の権限が重ならないことを、ページの単位で `is_disjoint` を使って示す。自分のページの権限の有限の写像の上の再帰の補題にする
3. `self.region = Some(region)`、`self.core.region` に `Shared` の写しを入れる。権限は `Arc` が指す塊の `perms` から写す (spec の「付ける」。`Arc` なしに権限だけを持つことはない)
4. `Arc::clone` の `assume_specification` を `trust.rs` の T5 に足す (このステップで `clone` を使うなら)

`freeze`: Task 2 の「作り手は塊を付けていない」の `requires` を外す。作り手が付けていた `Arc` を新しい塊の `parent` に移し、`RegionPerms::parent` に作り手の `Shared` の写しを入れる。`inv` の「この塊のブロックと祖先のブロックが重ならない」は、作り手の不変条件 (Step 2) から出る。走査する語で閉じていることは、作り手の `edge_wf` と、どのブロックも rc 0 であることから出る

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt frozen::`
Expected: すべて通る

- [ ] **Step 6: 計測して、全体を確かめてコミットする**

Run: `bench/rt.sh` を2回。どの計測も「S9b-1 (文字列)」の値から +0.5% を超えて増えない。増えたら、速い道に足した実行時の処理 (あってはならない) を探し、原因と数を報告する。Global Constraints の全体の確かめを流す。

```bash
git add crates/eml_rt
git commit -m "Let every heap accept words into an attached frozen region"
```

本文に、`has` へのモデルの拡張、読むだけの権限の共有、重ならないことの証明、`attach` の子孫の確かめ、親を持つ `freeze` を書き、verified の数を付ける。

---

### Task 4: 2つのスレッドのテスト、計測、文書

**Files:**
- Modify: `crates/eml_rt/src/heap/tests/trusted.rs`
- Modify: `docs/spec/object-model.md`、`docs/future/roadmap.md`、`docs/future/multicore.md`、`docs/implementation/testing.md`、`docs/implementation/benchmarks.md`、`docs/README.md` (object-model の行の見出し)、`CLAUDE.md` (`eml_rt` の段落)
- Modify: `crates/eml_rt/src` のコメントのうち、新しい節「凍結した領域」を指すべき引用
- Delete: `docs/superpowers/specs/2026-10-14-s9b2a-frozen-region-design.md`、`docs/superpowers/plans/2026-10-14-s9b2a-frozen-region.md`

**Interfaces:**
- Consumes: Task 2 と Task 3 の公開の操作

- [ ] **Step 1: 2つのスレッドのテストを書く**

`heap/tests/trusted.rs` の `// 凍結した領域` の節に足す。T0 の改め (凍結した塊は公開の後に書き込まれず、読むだけである) を、2つのスレッドが同じ塊を付けたそれぞれのヒープで同じ定数を読み、`dup` と `decref` を繰り返す形で確かめる。

```rust
fn two_threads_read_one_region<M: Mode>() {
    let (b, s, _) = super::frozen::builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let region = region.clone();
            std::thread::spawn(move || {
                let mut heap = Heap::<M>::new();
                heap.attach(region);
                for _ in 0..100 {
                    unsafe { heap.dup(s) }.unwrap();
                    unsafe { heap.decref(s) }.unwrap();
                }
                text(&heap, s)
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), "constant");
    }
}
```

`M` は `Send + 'static` を求めるので、必要なら `Mode` の境界に合わせて `both_modes!` の2つの関数を書く。

Run: `cargo test -p eml_rt trusted::` と Miri の2つの設定
Expected: 通る

- [ ] **Step 2: 計測を記録する**

Run: `bench/rt.sh` を2回。`benchmarks.md` の「S9b-1 (文字列)」の後に「S9b-2a (凍結した領域)」の節を足し、出力の表と、各計測の S9b-1 との差を書く。`eml_rt` は `eml run` から使わないので `bench/run.sh` の表は測らないことも、前の節と同じ文で書く。

- [ ] **Step 3: 文書を直す**

spec の「文書」の節のとおりに直す。

- `object-model.md`: 新しい節「凍結した領域」(鎖、作る、凍結する、付ける、寿命、`Sync`、検査モードでは数えないことと、その理由)。核の操作の表の「どの操作も」の行の安全の前提を「語は即値か、このヒープのブロックか、このヒープに付けた塊とその祖先のブロックである」にする。「不死の物体の数え方」に領域の物体を `UNCOUNTED` にすることを足す。「報告する誤り」に凍結の断り (`NotImmortal`) を足す。「文字列」に、速いモードは rc 0 の物体のページの頭を読まない約束 (`string_push` は一意な左辺のときだけ容量を読む) を足す。信頼の一覧の T0 (凍結した塊は公開の後に書き込まれない)、T4 (`Frozen` の `Drop`)、T5 (`Arc::clone`) を改める。「まだ決めていないこと」のうち `CodeBase` の不死の定数を閉じる
- `roadmap.md`: S9 の作業の表の S9b-2 を S9b-2a (終えた) と S9b-2b (これから。`Ctx`、基本操作、SIGINT) に分ける。「論点」の `CodeBase` の定数を「凍結した領域」で閉じる。spec の「範囲の外」の項目 (定数の表と LIR からの組み立て、別の worker の物体、ネイティブの静的なデータ、塊の詰め直し、`RunStats` の不死の物体の数え方) を、それぞれの段 (S10、`par`、ネイティブ化、S11) の論点に足す
- `multicore.md`: 「VM の並列化の備え」の制約2に、`CodeBase` の `Sync` が `Frozen` に頼ることを書く
- `testing.md`: Miri の対象に、凍結した領域のテスト (T0、T4) を足す
- `docs/README.md`: object-model の行の見出しの並びに「凍結した領域」を足す
- `CLAUDE.md` (英語): `eml_rt` の段落に、凍結した領域 (constants in a chain of frozen chunks that every heap accepts) を1文で足す

コードのコメントで、この節を指すべき引用 (Task 2 と Task 3 で「不死の物体の数え方」などを仮に引いた所) を「凍結した領域」に替える。`cargo test -p eml_cli citations` が通ることを確かめる。

- [ ] **Step 4: spec と計画を消して、全体を確かめてコミットする**

```bash
git rm docs/superpowers/specs/2026-10-14-s9b2a-frozen-region-design.md docs/superpowers/plans/2026-10-14-s9b2a-frozen-region.md
```

Run: `rg -n 's9b2a|S9b-2a の設計' docs crates CLAUDE.md` で、消した文書への参照が残っていないことを確かめる。Global Constraints の全体の確かめを流す。

```bash
git add -A docs crates/eml_rt CLAUDE.md
git commit -m "Document the frozen region and record its measurement"
```

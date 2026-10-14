# S9b-1 物体の中身 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_rt` に種類 `String` (物体の中に UTF-8 のバイト列を持つ文字列) とその操作を足し、Verus で UTF-8 と長さの不変条件まで証明する。クロージャと evidence の物体の形、データの物理的な形の規則を文書に定める。

**Architecture:** 3つのタスクで積む。Task 1 は種類 `String`、不透明なペイロードの欄、ghost のバイト列と不変条件、信頼の一覧の T10、文字列を作る操作と読む操作。Task 2 は追記 (`string_push`) と連結 (`string_concat`)。Task 3 は計測と文書。クロージャ、evidence の物体、物理的な形の規則は、`eml_rt` のコードを変えずに文書だけで定める (使うのは S9b-2 と S10)。

**Tech Stack:** Rust (edition 2024、fenix の 1.98.1)、Verus 0.2026.10.04、vstd 0.0.0-2026-10-04-0306 (`vstd::utf8::valid_utf8`、`valid_utf8_concat`、`encode_utf8_valid_utf8`、`str::spec_bytes`)、verusfmt、Miri (fenix の nightly)、macOS の `/usr/bin/time -l`。

**Spec:** `docs/superpowers/specs/2026-10-14-s9b1-object-contents-design.md`。実装する人は、各タスクの前に spec の該当する節を読む。spec が規範で、計画とずれたら spec に従い、ずれを報告する。

| 見出し | 中身 |
|---|---|
| Task 1 | 種類 `String`、不透明なペイロード、ghost のバイト列と不変条件、T10、`alloc_string`、`alloc_string_immortal`、`string` |
| Task 2 | `string_push`、`string_concat`、検査モードの事前の検査、T10 の Miri のテストの残り |
| Task 3 | `rt_bench` の追記の計測、文書、spec と計画の削除 |

## Global Constraints

- 各タスクの終わりに次が通る。途中のタスクでもワークスペースを壊したままコミットしない
  - `cargo test` (ワークスペース全体)
  - `cargo verus verify -p eml_rt` が 0 errors。最後の行の `verification results:: N verified, 0 errors` をコミットメッセージに書く。`cargo verus` は結果を保存するので、確かめ直すときは `target/**/.fingerprint/eml_rt-*` を消してから流す
  - `cargo clippy --all-targets` が警告を出さない
  - `cargo fmt --check` と `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`。新しいファイルは `git add` してから流す (`git ls-files` が拾うため)。verusfmt は `unsafe { .. }?` を読めないので、`let x = unsafe { .. }; x?` と書く
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、それに `-Zmiri-tree-borrows` を足したもの
- 検証が通っても `cargo build` が通るとは限らない。どのステップでも両方を流す
- `eml_rt` の外の crate は変えない (Task 3 の `examples/rt_bench.rs` と `bench/rt.sh` を除く)。UI テストと `bench/` のスナップショットは変わらない
- `unsafe` は、信頼の一覧の項目の中と、`verus!` の外の `unsafe impl Send` / `Sync` と T4 の `Drop` にだけ書く。T10 の関数は生のポインタだけでバイトを写し、同じブロックに `&mut [u8]` と `&[u8]` を同時に作らない
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則は `docs/` のパスと見出しで引く (`docs/spec/object-model.md の「核の操作」`)。この計画と spec は Task 3 で消すので引かない。コメントに計画のタスクの番号を書かない
- コミットメッセージは英語で、既存の形 (命令形の1行目、本文に理由) に合わせる。各コミットの末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014bwniBqkZgCLERCaKyDB5S
  ```

- `git diff` は `--no-ext-diff` で見る
- コマンドはすべて devShell の中で流す (direnv の入ったシェルか `nix develop -c ...`)

## 名前 (各タスクで共通に使う)

- 種類: `Kind::String = 3` (`crates/eml_rt/src/meta.rs`)。`Kind::opaque(self) -> bool` (`Foreign` か `String`)。名前は `"String"`
- ghost のバイト列: `BlockPerm` の欄 `pub(crate) ghost text: Seq<u8>`。`String` でないブロックでは意味を持たない (`from_raw` は空にする)。仕様の関数 `p_text(p: BlockPerm) -> Seq<u8>` を `heap/inv.rs` の `p_word` などの並びに足す
- 不変条件の述語: `pub(crate) open spec fn string_wf(p: BlockPerm, cap: nat) -> bool` (`heap/inv.rs`)

  ```rust
  &&& p_count(p) == 0
  &&& !p_extended(p)
  &&& cap >= 1
  &&& p_init(p, 0)
  &&& p_word(p, 0).bits_spec() == p.text.len()
  &&& p.text.len() <= 8 * (cap - 1)
  &&& valid_utf8(p.text)
  ```

- T10 の関数 (`crates/eml_rt/src/text.rs`、新しいファイル): `write_text`、`text`、`text_ptr` (下の Task 1 の Step 3)
- `Heap` の公開の仕様の関数 (`crates/eml_rt/src/heap/string.rs`、新しいファイル): `pub closed spec fn is_string(&self, w: Word) -> bool`、`pub closed spec fn text_of(&self, w: Word) -> Seq<u8>`、`pub closed spec fn made_immortal(&self, old: Heap<M>, w: Word) -> bool`、`pub closed spec fn push_ok(&self, left: Word) -> bool`、`pub closed spec fn pushed(&self, old: Heap<M>, left: Word, w: Word) -> bool`、`pub closed spec fn concat_ok(&self, left: Word, right: Word) -> bool`、`pub closed spec fn concatenated(&self, old: Heap<M>, left: Word, right: Word, w: Word) -> bool`
- 公開の操作: `alloc_string(&mut self, s: &str) -> Word`、`alloc_string_immortal(&mut self, s: &str) -> Word`、`unsafe fn string(&self, w: Word) -> Result<*const str, HeapError>`、`unsafe fn string_push(&mut self, left: Word, s: &str) -> Result<Word, HeapError>`、`unsafe fn string_concat(&mut self, left: Word, right: Word) -> Result<Word, HeapError>`
- テスト: `crates/eml_rt/src/heap/tests/string.rs` (新しいファイル、`both_modes!`)、`heap/tests/trusted.rs` の `// T10` の節

## Review Focus

- 一意な左辺に、その左辺自身の中身を足すこと (`string_push(w, &*string(w))`): その場の追記でも作り直しでも `"abcabc"` になり、Miri の2つの借用の規則で UB にならない。Task 2 の `push_of_its_own_bytes` と trusted の `own_bytes_are_appended_under_both_borrow_models`
- 左辺と右辺が同じ物体で参照が1つしかない `string_concat(x, x)` (呼ぶ側の誤り) を検査モードに渡すこと: 確保の前に `Err` を返し、ヒープは変わらない (生きている箱の数も文字列の中身も同じ)。Task 2 の `concat_of_one_reference_twice_is_refused`
- 文字列の物体に `raw` と `set_raw` を使うこと (検査モード): `WrongKind` を返し、長さの語も中身も変わらない。Task 1 の `raw_words_of_a_string_are_refused`
- 1 ページ (64 KiB) を超える文字列を作り、さらに足すこと: 大きい物体のページから大きい物体のページへ作り直し、中身が正しく、速いモードでは古いページを返す。Task 2 の `large_strings_grow_across_pages` と trusted の `large_strings_cross_pages`
- 不死の文字列を左辺にした追記で、外に出ている参照の数を使い切っていること (検査モード): 確保の前に `ImmortalOverRelease` を返し、ヒープは変わらない。Task 2 の `over_released_immortal_left_is_refused_before_allocating`

---

### Task 1: 種類 `String`、不変条件、T10、文字列を作る操作と読む操作

**Files:**
- Modify: `crates/eml_rt/src/meta.rs` (`Kind`、`kind_of`、`known_kind`、`Kind::from_bits`、`Kind::name`、`Kind::opaque`、`BlockPerm` の `text`、`BlockPerm::from_raw` と分解する所、`meta.rs` のテストの種類の一覧)
- Modify: `crates/eml_rt/src/heap/inv.rs`、`heap/safe.rs`、`heap/field.rs` (`BigInt` の条件を書いている各所に `String` の条件を並べる。下の Step 4)
- Modify: `crates/eml_rt/src/heap/mod.rs` (`alloc`、`alloc_immortal`、`alloc_building` の `requires`、`raw_at`、`raw_check`、`mod string;`)
- Modify: `crates/eml_rt/src/heap/reuse.rs` (`con_at` の `requires`)
- Create: `crates/eml_rt/src/text.rs` (T10)、`crates/eml_rt/src/heap/string.rs`
- Modify: `crates/eml_rt/src/lib.rs` (`mod text;`)
- Modify: `crates/eml_rt/src/trust.rs` (`FILES`、`TRUSTED`、番号の一覧に `"T10"`)
- Create: `crates/eml_rt/src/heap/tests/string.rs`。Modify: `heap/tests/mod.rs` (`mod string;`)、`heap/tests/trusted.rs` (`// T10`)

**Interfaces:**
- Produces: 上の「名前」の `Kind::String`、`Kind::opaque`、`BlockPerm::text`、`p_text`、`string_wf`、T10 の3つの関数、`is_string`、`text_of`、`made_immortal`、`alloc_string`、`alloc_string_immortal`、`string`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_rt/src/heap/tests/string.rs` を作り、`heap/tests/mod.rs` に `mod string;` を足す。

```rust
//! 文字列の物体 (docs/spec/object-model.md の「文字列」)。

use super::{mortal, raw};
use crate::{Heap, HeapError, Kind, Mode, Word};

/// 文字列の中身を写して返す。
pub(super) fn text<M: Mode>(heap: &Heap<M>, w: Word) -> String {
    let p = unsafe { heap.string(w) }.unwrap();
    unsafe { &*p }.to_string()
}

/// 空、1語に収まる、語の境目、多バイトの文字、128 語を超える (大きい物体の道) 長さ。
fn samples() -> Vec<String> {
    vec![
        String::new(),
        "a".to_string(),
        "x".repeat(8),
        "日本語の文字列".to_string(),
        "y".repeat(2000),
    ]
}

fn strings_hold_their_text<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for s in samples() {
        let w = heap.alloc_string(&s);
        assert_eq!(unsafe { heap.meta(w) }.unwrap().kind(), Kind::String);
        assert_eq!(text(&heap, w), s);
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn immortal_strings_are_never_freed<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string_immortal("literal");
    unsafe { heap.acquire_immortal(w) }.unwrap();
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(text(&heap, w), "literal");
    assert!(!unsafe { heap.is_unique(w) }.unwrap());
    // 不死の物体は数が 0 なら漏れと数えない (docs/spec/object-model.md の「不死の物体の数え方」)
    assert!(heap.leaks().is_empty());
}

fn leaks_are_named_string<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("left behind");
    if M::CHECKED {
        assert_eq!(heap.leaks().get("String"), Some(&1));
    }
    unsafe { heap.decref(w) }.unwrap();
}

fn reading_a_data_object_as_a_string_is_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let d = heap.alloc_data(0, 0, 1, false);
    assert_eq!(unsafe { heap.string(d) }.unwrap_err(), HeapError::WrongKind);
    unsafe { heap.decref(d) }.unwrap();
}

fn raw_words_of_a_string_are_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("abc");
    assert_eq!(unsafe { heap.raw(w, 0) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(unsafe { heap.raw(w, 1) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(unsafe { heap.set_raw(w, 0, 1000) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(unsafe { heap.set_raw(w, 1, 0) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(text(&heap, w), "abc");
    assert_eq!(unsafe { raw::rc(w) }, 1);
    unsafe { heap.decref(w) }.unwrap();
}

both_modes!(
    strings_hold_their_text,
    immortal_strings_are_never_freed,
    leaks_are_named_string,
    reading_a_data_object_as_a_string_is_refused,
    raw_words_of_a_string_are_refused,
);
```

`raw` と `set_raw` のテストで `Foreign` を断る今のテストがあれば、そのまま通ること (置き換えた欄でも `Foreign` を断る) を Step 6 で確かめる。

Run: `cargo test -p eml_rt string::`
Expected: コンパイルが失敗する (`alloc_string` などがない)。

- [ ] **Step 2: 種類と不透明なペイロードの欄を足す**

`meta.rs`:

- `Kind` に `String = 3` を足し、`kind_of` と `known_kind` に `bits == 3` を、`from_bits` に `3 => Kind::String` を、`name` に `"String"` を足す。`meta.rs` のテストの種類の一覧 (`for kind in [Kind::Data, Kind::BigInt, Kind::Foreign, Kind::Free]`) にも足す
- 不透明なペイロードの欄を足す。`raw` と `set_raw` が断る種類の判定を、`has_finalizer` からこれに置き換えるためである

  ```rust
  /// ペイロードが核の操作から見て不透明か。`raw` と `set_raw` はこの種類を断る。`Foreign` は Rust の値を、`String` は
  /// 途中までしか書いていない語を持つので、語として読み書きすると未初期化の読み込みか不変条件の破れになる
  /// (docs/spec/object-model.md の「種類」)。
  #[inline]
  pub(crate) fn opaque(self) -> (r: bool)
      ensures
          r == (self == Kind::Foreign || self == Kind::String),
  {
      matches!(self, Kind::Foreign | Kind::String)
  }
  ```

- `tokenable` と `has_finalizer` は変えない (`String` はどちらも偽)
- `BlockPerm` に `pub(crate) ghost text: Seq<u8>` を足す。`from_raw` は `text: Seq::empty()` で作り、`BlockPerm { .. }` を分解している所 (`meta.rs:631` の付近) も欄を受ける

`heap/mod.rs`:

- `raw_at` の `self.core.kind(h) != Kind::Foreign` を `!(self.core.kind(h) == Kind::Foreign || self.core.kind(h) == Kind::String)` に替える
- `raw_check` の `has_finalizer()` を `opaque()` に、`ensures` の `!= Kind::Foreign` を同じ2つの比較に替える
- `alloc`、`alloc_immortal`、`alloc_building` の `requires` に `meta.kind_spec() != Kind::String` を足し、`# Safety` の文を「`meta` の種類は `Foreign`、`String`、`Free` のどれでもなく」に直す

`heap/reuse.rs`: `con_at` の `requires` と `# Safety` に同じものを足す。`reuse_in_place` は token の箱 (`Data`) と同じ種類しか受けないので足さない。

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt`
Expected: 通る (`String` の物体はまだ作れないので、不変条件の証明は変わらない)。通らない所は、`Kind` の `match` の網羅と `kind_of` を使う補題である。

- [ ] **Step 3: T10 の関数を書く**

`crates/eml_rt/src/text.rs` を作り、`lib.rs` に `mod text;` を足す。形は `foreign.rs` (T3) に合わせる。

```rust
//! 文字列のバイト列 (docs/spec/object-model.md の「文字列」)。Verus の権限は語の単位で、語の中のバイトの位置を
//! 扱わないので、ここの関数は信頼の一覧の T10 である。どれも `String` のブロックの権限を受け、長さの語 (ペイロードの
//! 0 語目) の後ろの容量の中だけを読み書きする。メモリのバイト列は、いつも権限の ghost のバイト列 `text` に等しい。

use crate::meta::{BlockPerm, Header, slot_ptr};
use vstd::prelude::*;
use vstd::utf8::valid_utf8;

verus! {

/// `h` のペイロードの `at` バイト目 (長さの語の後ろから数える) から `s` のバイトを写す。`at` は今の ghost のバイト列の
/// 長さで、写した後の ghost のバイト列は連結になる。長さの語と、ほかの欄は書かない。
#[verifier::external_body]
#[inline]
pub(crate) fn write_text(
    h: *mut Header,
    cap: usize,
    at: usize,
    s: &str,
    Tracked(perm): Tracked<&mut BlockPerm>,
)
    requires
        forall|i: int| #[trigger] old(perm).slots.contains_key(i) <==> 0 <= i < cap,
        cap >= 1,
        at == old(perm).text.len(),
        at + s.spec_bytes().len() <= 8 * (cap - 1),
    ensures
        final(perm).text == old(perm).text + s.spec_bytes(),
        final(perm).rc == old(perm).rc,
        final(perm).meta == old(perm).meta,
        final(perm).prefix == old(perm).prefix,
        final(perm).slots.dom() == old(perm).slots.dom(),
        final(perm).slots[0] == old(perm).slots[0],
        forall|i: int|
            0 <= i < cap ==> (#[trigger] final(perm).slots[i]).ptr() == old(perm).slots[i].ptr(),
{
    // 生のポインタだけで写す。s がこのブロック自身の中身を指していても、写す先 (at から後ろ) とは重ならない
    unsafe {
        std::ptr::copy_nonoverlapping(
            s.as_ptr(),
            slot_ptr(h, 1).cast::<u8>().add(at),
            s.len(),
        );
    }
}

/// `h` の中身を `str` として借りる。借りている間、権限 (と同じブロック) は書き換えられない。
#[verifier::external_body]
#[inline]
pub(crate) fn text<'a>(h: *mut Header, len: usize, Tracked(perm): Tracked<&'a BlockPerm>) -> (r:
    &'a str)
    requires
        len == perm.text.len(),
        valid_utf8(perm.text),
    ensures
        r.spec_bytes() == perm.text,
{
    unsafe {
        std::str::from_utf8_unchecked(
            std::slice::from_raw_parts(slot_ptr(h, 1).cast::<u8>().cast_const(), len),
        )
    }
}

/// `h` の中身の位置。公開の `string` が返す。指す先を読めるのは、物体が生きていて、`string_push` で書き換えていない間
/// だけである (docs/spec/object-model.md の「文字列」)。
#[verifier::external_body]
#[inline]
pub(crate) fn text_ptr(h: *mut Header, len: usize, Tracked(perm): Tracked<&BlockPerm>) -> *const str
    requires
        len == perm.text.len(),
        valid_utf8(perm.text),
{
    std::ptr::slice_from_raw_parts(slot_ptr(h, 1).cast::<u8>().cast_const(), len) as *const str
}

} // verus!
```

`text_ptr` の本体の `as *const str` は、`external_body` の中なので Verus は見ない。公開の `string` は `*const str` を返し、`*const [u8]` を返す形や `verus!` の外の補助の関数は作らない。

`trust.rs`:

- `FILES` に `("text.rs", include_str!("text.rs"))` と `("heap/string.rs", include_str!("heap/string.rs"))` を足す
- `TRUSTED` に `("text.rs: external_body write_text", "T10")`、`("text.rs: external_body text", "T10")`、`("text.rs: external_body text_ptr", "T10")` を足す
- 番号の一覧 `["T0", ..., "T9"]` に `"T10"` を足す

Run: `cargo test -p eml_rt trust::`
Expected: `heap/string.rs` がまだないので、コンパイルが失敗する。Step 5 の後で通る。

- [ ] **Step 4: 不変条件を足す**

`heap/inv.rs` に `p_text` と `string_wf` (上の「名前」) を足す。`use vstd::utf8::valid_utf8;` も足す。

`BigInt` の値の条件 (`p_kind(p) == Kind::BigInt ==> self.cap(h) >= 1 && p_init(p, 0)`) を書いているすべての所に、`String` の条件を並べる。今の所は次のとおりである (`rg -n 'Kind::BigInt' crates/eml_rt/src/heap` で確かめる)。

- `heap/inv.rs` の `perm_wf` (速いモードの `full` の中) と `settleable` の付近
- `heap/safe.rs` の `safe_perm_wf`、`lemma_safe_live_block`、`safe_settleable`、ブロックの形の条件 (`block_shape` と、そこから導く補題)
- `heap/field.rs` の `BigInt` の行

足す条件は次のとおりである。

```rust
&&& st == BlockState::Live || st == BlockState::Dying ==> (p_kind(p) == Kind::String
    ==> string_wf(p, self.cap(h)))
```

`String` の物体は構築中 (`Building`) を通らずに生きている物体として作る (`alloc_int` と同じ) ので、`Building` と `safe_settleable` に条件は要らない。`alloc`、`alloc_immortal`、`con_at` が `String` を断るので、構築中の `String` は現れない。`p_count(p) == 0` は `set_field` (走査する語だけに書く) が保つ。`set_raw` は `raw_at` と `raw_check` が `String` を断るので、長さの語を書き換えない。

Run: `cargo verus verify -p eml_rt`
Expected: 通る。通らない補題は、ブロックの権限が変わる操作 (`slot_put`、`publish`、`free_box`、`share` の rc の書き換え、`con_at`) の後で、変わっていないブロックの条件を写す所である。`text` は rc と `meta` の書き換えでは変わらないので、各補題の「ほかの欄は同じ」に `text` も入れる。

- [ ] **Step 5: 文字列を作る操作と読む操作を書く**

`crates/eml_rt/src/heap/string.rs` を作り、`heap/mod.rs` に `mod string;` を足す。

```rust
//! 文字列の物体 (docs/spec/object-model.md の「文字列」)。種類 `String` の物体は、ここの操作だけが作り、書く。
//! 長さの語は生の 0 語目で、バイト列はその後ろにあり、ghost のバイト列 `text` に等しい。

use vstd::prelude::*;
use vstd::utf8::valid_utf8;

use super::*;
use crate::text::{text_ptr, write_text};

verus! {

impl<M: Mode> Heap<M> {
    pub closed spec fn is_string(&self, w: Word) -> bool {
        self.core.kind(w.bits_spec()) == Kind::String
    }

    /// 文字列のバイト列。
    pub closed spec fn text_of(&self, w: Word) -> Seq<u8> {
        self.core.perm(w.bits_spec()).text
    }

    /// `alloc_string_immortal` の後の状態。`w` は rc 0 の生きている物体で、検査モードの外に出ている参照の数は 0 である。
    pub closed spec fn made_immortal(&self, old: Heap<M>, w: Word) -> bool {
        let h = w.bits_spec();
        &&& self.wf()
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.model@.building is None
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& !old.core.dom().contains(h)
        &&& self.core.dom() == old.core.dom().insert(h)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.rc_of(h) == 0
        &&& old.consistent() && old.settleable() ==> {
            &&& self.consistent()
            &&& self.core.frame(old.core)
        }
    }

    /// `s` の UTF-8 のバイト列を持つ文字列を作る。
    pub fn alloc_string(&mut self, s: &str) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == s.spec_bytes(),
    {
        self.make_string(s, 1)
    }

    /// `alloc_string` と同じで、rc 0 の不死の物体にする。`CodeBase` の文字列の定数が使う。
    pub fn alloc_string_immortal(&mut self, s: &str) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made_immortal(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == s.spec_bytes(),
    {
        self.make_string(s, 0)
    }

    /// # Safety
    /// `w` はこのヒープの生きている文字列の物体を指す。返す位置を読めるのは、物体が生きていて、`string_push` でその
    /// 物体を書き換えていない間だけである。
    pub unsafe fn string(&self, w: Word) -> (r: Result<*const str, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && self.is_string(w),
        ensures
            self.consistent() && self.settleable() && self.readable(w) && self.is_string(w)
                ==> r is Ok,
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        if M::CHECKED && self.core.meta_get(h).kind() != Kind::String {
            return Err(HeapError::WrongKind);
        }
        let len = self.core.slot_get(h, 0).bits();
        let tracked p = self.core.perms.borrow().tracked_borrow(h@.addr);
        Ok(text_ptr(h, len, Tracked(p)))
    }
}

} // verus!
```

`make_string(&mut self, s: &str, rc: i32) -> Word` は `alloc_int` の `None` の枝と同じ形で書く。

1. `let bytes = s.len();` 語の数は `1 + bytes.div_ceil(8)` だが、ペイロードは少なくとも1語であり、空いたブロックのためにも足りる。大きさの計算は `checked_add` で行い、桁あふれは `too_large()` にする
2. `self.settle();` の後、`self.alloc_header(payload_words, rc, Meta::new(0, 0, Kind::String, false))` で確保する。`alloc_header` は種類 `String` を断らない内部の関数であることを確かめる (断るのは公開の `alloc` 系だけ)
3. 0 語目に `Word::from_bits(0)` を書き、`write_text(h, cap, 0, s, Tracked(&mut perm))` で写し、0 語目を `Word::from_bits(bytes)` に書き直す。`cap` は `self.core.cap_get(h)` (S9a の `cap_of` を `Core` から呼ぶ形。名前は `heap/core.rs` で確かめる)
4. `valid_utf8(s.spec_bytes())` は `vstd::utf8::encode_utf8_valid_utf8(s@)` から出る (`spec_bytes` は `encode_utf8(s@)`)
5. `self.core.publish(Ghost(h), Tracked(perm), Ghost(BlockState::Live), Ghost(self.worker))` で生きている物体にし、`alloc_int` の証明の列 (`lemma_dummy_kept`、`lemma_frame_chain`、`lemma_published_safe`) を写す。rc 0 の枝は `Heap::new` のダミーの作り方 (`heap/mod.rs:436` の付近) と、`alloc_immortal` の検査モードの前置き (外に出ている参照の数 0) に合わせる

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt string:: trust::`
Expected: すべて通る。

- [ ] **Step 6: T10 の Miri のテストのうち、作る操作と読む操作の分を書く**

`heap/tests/trusted.rs` の最後の節の後に足し、ファイルの `both_modes!` の一覧に名前を足す。`super::string::text` を使うため、`use super::string::text;` を足す。

```rust
// T10
fn strings_of_every_length_are_read_back<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // 語に揃わない長さと、1ページを超える長さ
    for n in [0, 1, 7, 8, 9, 15, 16, 17, 1023, 1025, PAGE_BYTES + 3] {
        let s: String = (0..n).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let w = heap.alloc_string(&s);
        assert_eq!(text(&heap, w), s);
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn multibyte_strings_are_read_back<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let s = "é ü 日本 🦀".repeat(100);
    let w = heap.alloc_string(&s);
    assert_eq!(text(&heap, w), s);
    unsafe { heap.decref(w) }.unwrap();
}
```

Run: `cargo test -p eml_rt trusted::` と、Global Constraints の Miri の2つの設定
Expected: 通る。

- [ ] **Step 7: 全体を確かめてコミットする**

Run: `cargo fmt && verusfmt --verus-only $(git ls-files 'crates/eml_rt/src/*.rs') && cargo test && cargo clippy --all-targets && cargo verus verify -p eml_rt` と Miri の2つの設定
Expected: すべて通る。

```bash
git add crates/eml_rt
git commit -m "Add String objects to eml_rt with verified UTF-8 contents"
```

本文に、`String` の種類、不透明なペイロードの欄、ghost のバイト列で UTF-8 と長さを証明したこと、T10 を書き、末尾に verified の数と Global Constraints の2行を付ける。

---

### Task 2: `string_push` と `string_concat`

**Files:**
- Modify: `crates/eml_rt/src/heap/string.rs`
- Modify: `crates/eml_rt/src/heap/check.rs` (同じ物体の2回の `decref` の検査)
- Modify: `crates/eml_rt/src/heap/tests/string.rs`、`heap/tests/trusted.rs`

**Interfaces:**
- Consumes: Task 1 の `is_string`、`text_of`、`string`、`write_text`、`text`、`string_wf`
- Produces: `push_ok`、`pushed`、`concat_ok`、`concatenated`、`string_push`、`string_concat`

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests/string.rs` に足し、`both_modes!` の一覧に名前を足す。

```rust
fn push(heap: &mut Heap<impl Mode>, w: Word, s: &str) -> Word {
    unsafe { heap.string_push(w, s) }.unwrap()
}

fn push_in_place_keeps_the_box<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("a");
    // 1 バイトの文字列のブロックは、どちらのモードでも少なくとも 8 バイトの容量を持つ
    let w2 = push(&mut heap, w, "b");
    assert_eq!(w2, w);
    assert_eq!(text(&heap, w2), "ab");
    unsafe { heap.decref(w2) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn push_grows_a_full_string<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut w = heap.alloc_string("a");
    let first = w;
    let mut expected = "a".to_string();
    while w == first {
        w = push(&mut heap, w, "b");
        expected.push('b');
    }
    assert_eq!(text(&heap, w), expected);
    // 古い箱は空けた
    assert_eq!(mortal(&heap), 1);
    unsafe { heap.decref(w) }.unwrap();
}

fn regrowth_is_logarithmic<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let n = 100_000;
    let mut w = heap.alloc_string("");
    let mut moves = 0;
    for _ in 0..n {
        let next = push(&mut heap, w, "x");
        if next != w {
            moves += 1;
        }
        w = next;
    }
    assert_eq!(text(&heap, w).len(), n);
    // 容量を倍にするので、作り直しは log2(n) 回 (約17回) と小さい級の刻みの分で済む
    assert!(moves <= 40, "{moves} regrowths for {n} pushes");
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn shared_left_is_copied<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("ab");
    unsafe { heap.dup(w) }.unwrap();
    let w2 = push(&mut heap, w, "c");
    assert_ne!(w2, w);
    assert_eq!(text(&heap, w), "ab");
    assert_eq!(text(&heap, w2), "abc");
    unsafe { heap.decref(w) }.unwrap();
    unsafe { heap.decref(w2) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn immortal_left_is_copied<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string_immortal("lit");
    unsafe { heap.acquire_immortal(w) }.unwrap();
    let w2 = push(&mut heap, w, "!");
    assert_ne!(w2, w);
    assert_eq!(text(&heap, w), "lit");
    assert_eq!(text(&heap, w2), "lit!");
    unsafe { heap.decref(w2) }.unwrap();
    assert!(heap.leaks().is_empty());
}

fn push_of_its_own_bytes<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // その場の追記 ("abc" は 8 バイトの容量に収まる) と、作り直し (8 バイトの文字列はもう収まらない) の両方
    for s in ["abc", "abcdefgh"] {
        let w = heap.alloc_string(s);
        let own = unsafe { &*heap.string(w).unwrap() };
        let w2 = push(&mut heap, w, own);
        assert_eq!(text(&heap, w2), format!("{s}{s}"));
        unsafe { heap.decref(w2) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn large_strings_grow_across_pages<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let chunk = "z".repeat(50_000);
    let mut w = heap.alloc_string(&chunk);
    for _ in 0..4 {
        w = push(&mut heap, w, &chunk);
    }
    assert_eq!(text(&heap, w), chunk.repeat(5));
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn concat_consumes_both_sides<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = heap.alloc_string("foo");
    let b = heap.alloc_string("bar");
    let r = unsafe { heap.string_concat(a, b) }.unwrap();
    assert_eq!(text(&heap, r), "foobar");
    assert_eq!(mortal(&heap), 1);
    unsafe { heap.decref(r) }.unwrap();
}

fn concat_of_a_string_with_itself<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = heap.alloc_string("ab");
    unsafe { heap.dup(x) }.unwrap();
    let r = unsafe { heap.string_concat(x, x) }.unwrap();
    assert_eq!(text(&heap, r), "abab");
    unsafe { heap.decref(r) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn errors_leave_the_heap_unchanged<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let s = heap.alloc_string("abc");
    let d = heap.alloc_data(0, 0, 1, false);
    let live = heap.live_objects().len();
    assert_eq!(unsafe { heap.string_push(d, "x") }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(unsafe { heap.string_concat(s, d) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(unsafe { heap.string_concat(d, s) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(heap.live_objects().len(), live);
    assert_eq!(text(&heap, s), "abc");
    assert_eq!(unsafe { raw::rc(s) }, 1);
    unsafe { heap.decref(s) }.unwrap();
    assert_eq!(unsafe { heap.string_push(s, "x") }.unwrap_err(), HeapError::UseAfterFree);
    unsafe { heap.decref(d) }.unwrap();
}

fn concat_of_one_reference_twice_is_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let x = heap.alloc_string("ab");
    let live = heap.live_objects().len();
    assert_eq!(unsafe { heap.string_concat(x, x) }.unwrap_err(), HeapError::UseAfterFree);
    assert_eq!(heap.live_objects().len(), live);
    assert_eq!(text(&heap, x), "ab");
    assert_eq!(unsafe { raw::rc(x) }, 1);
    unsafe { heap.decref(x) }.unwrap();
}

fn over_released_immortal_left_is_refused_before_allocating<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    // 外に出ている参照を作っていない不死の文字列
    let w = heap.alloc_string_immortal("lit");
    let live = heap.live_objects().len();
    assert_eq!(
        unsafe { heap.string_push(w, "!") }.unwrap_err(),
        HeapError::ImmortalOverRelease
    );
    assert_eq!(heap.live_objects().len(), live);
}
```

Run: `cargo test -p eml_rt string::`
Expected: コンパイルが失敗する (`string_push` と `string_concat` がない)。

- [ ] **Step 2: 前提の述語と同じ物体の2回の検査を書く**

`heap/string.rs` に述語を足す。

- `push_ok(left)`: `self.settleable() && self.decref_ok(left) && self.readable(left) && self.is_string(left)`。`decref_ok` (呼ぶ側が参照を1つ持ち、不死なら外に出ている参照、共有なら `lin` でない、rc が正なら持ち主が今の worker) をそのまま使う
- `pushed(old, left, w)`: `final` は、`old` から left の参照を1つ手放し (`evolved(old, left, -1)` と同じ数の変化)、`w` の参照を1つ作った状態である。`w == left` (その場の追記) なら、left の参照の数も辺も変わらず、`text_of(left)` だけが変わる。`w != left` なら、`w` は新しい生きている物体 (`made` と同じ形) で、left は `decref` の後と同じである。どちらでも `text_of(w) == old.text_of(left) + s.spec_bytes()` は `string_push` の `ensures` に別に書く
- `concat_ok(left, right)`: `push_ok(left) && self.readable(right) && self.is_string(right) && self.decref_ok(right)`、かつ `left == right` なら呼ぶ側がその参照を2つ持つ。2つ持つことを書く述語が `heap/inv.rs` になければ、`holds` の数 (`refs - indeg`) が 2 以上であることで書く
- `concatenated(old, left, right, w)`: `pushed` の後に right を `decref` した状態

`heap/check.rs` に、同じ物体を2回手放す前の検査を足す。`checked_decref` と同じ形で、物体の rc と前置きを読んで決める。

```rust
/// `string_concat(x, x)` の2回の `decref` の検査。両方が通るなら `Ok` を返す。局所の数と共有の数は 2 以上、不死の
/// 物体の外に出ている参照の数も 2 以上でなければならない。足りなければ、2回目の `decref` が返す誤りを返す。
pub(super) fn checked_decref_twice(&self, h: *mut Header) -> (r: Result<(), HeapError>)
```

rc が 1 か -1 なら `UseAfterFree` (1回目で空くので、2回目は空けた物体の使用になる)、rc が 0 で数が 1 なら `ImmortalOverRelease` を返す。

Run: `cargo verus verify -p eml_rt`
Expected: 通る。

- [ ] **Step 3: `string_push` を書く**

```rust
/// left の後ろに s を足した文字列を返す。left の参照を1つ受け取り、結果の参照を1つ返す。どの誤りも最初の確保より
/// 前に返すので、`Err` のときヒープは変わらない (docs/spec/object-model.md の「文字列」)。
///
/// # Safety
/// `left` はこのヒープの生きている文字列の物体を指し、呼ぶ側がその参照を1つ持つ (`decref` と同じ前提)。`s` は left の
/// 中身を指してもよいが、ほかの文字列の中身を指すときは、その物体はこの呼び出しの間生きている。
pub unsafe fn string_push(&mut self, left: Word, s: &str) -> (r: Result<Word, HeapError>)
    requires
        old(self).wf(),
        old(self).safe_settleable(),
        left.is_ptr_spec(),
        old(self).word_in(left),
        !M::CHECKED ==> old(self).push_ok(left),
    ensures
        final(self).wf(),
        final(self).same_tokens(*old(self)),
        r is Err ==> *final(self) == *old(self),
        !M::CHECKED ==> r is Ok,
        r matches Ok(w) ==> final(self).is_string(w) && final(self).text_of(w) == old(
            self,
        ).text_of(left) + s.spec_bytes(),
        old(self).push_ok(left) ==> r is Ok,
        old(self).push_ok(left) && r matches Ok(w) ==> final(self).consistent() && final(
            self,
        ).pushed(*old(self), left, w),
```

本体は次の順に書く。

1. `self.settle();` `let h = self.header_of(left);`
2. 検査モードの検査 (状態を変える前): `checked_live`、種類が `String` でなければ `WrongKind`、`checked_decref(h)` (不死の数、共有の `lin`、持ち主)。速いモードでは何も確かめない
3. 長さ `len` (0 語目)、`need = len.checked_add(s.len())` (桁あふれは `too_large()`)、容量 `room = 8 * (cap - 1)`
4. `is_unique` (rc が 1) で `need <= room` なら、`write_text(h, cap, len, s, ..)` で写し、0 語目を `need` に書き直して `left` を返す
5. そうでなければ、新しい容量 `want = if is_unique { max(need, 2 * room) } else { need }` (`checked_mul` と `checked_add`、桁あふれは `too_large()`) で `make_string` と同じ確保をし、`text(h, len, ..)` で借りた left の中身と `s` を順に `write_text` で写す。その後で、一意なら left の箱を空ける (`free_box` か `free_block` の内部の道。left は子を持たないので、子の処理はない)。一意でなければ `decref` と同じ道で left の参照を1つ手放す (共有の最後の参照なら空ける)
6. `valid_utf8` の連結は `vstd::utf8::valid_utf8_concat(old_text, s.spec_bytes())` と `encode_utf8_valid_utf8(s@)` で示す

`s` を写すのは left を手放す前である。速いモードでは、大きい物体のページを箱の解放がすぐに返すためである。

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt string::`
Expected: `string_concat` のテストのほかは通る。

- [ ] **Step 4: `string_concat` を書く**

```rust
/// `++`。両辺の参照を受け取り、結果の参照を返す。`string_push(left, string(right))` の後に right を手放す。どの誤りも
/// 最初の確保より前に返す (docs/spec/object-model.md の「文字列」)。
///
/// # Safety
/// `left` と `right` はこのヒープの生きている文字列の物体を指し、呼ぶ側がそれぞれの参照を1つずつ持つ (同じ物体なら
/// 2つ持つ)。
pub unsafe fn string_concat(&mut self, left: Word, right: Word) -> (r: Result<Word, HeapError>)
    requires
        old(self).wf(),
        old(self).safe_settleable(),
        left.is_ptr_spec(),
        right.is_ptr_spec(),
        old(self).word_in(left),
        old(self).word_in(right),
        !M::CHECKED ==> old(self).concat_ok(left, right),
    ensures
        final(self).wf(),
        final(self).same_tokens(*old(self)),
        r is Err ==> *final(self) == *old(self),
        !M::CHECKED ==> r is Ok,
        r matches Ok(w) ==> final(self).is_string(w) && final(self).text_of(w) == old(
            self,
        ).text_of(left) + old(self).text_of(right),
        old(self).concat_ok(left, right) ==> r is Ok,
        old(self).concat_ok(left, right) && r matches Ok(w) ==> final(self).consistent() && final(
            self,
        ).concatenated(*old(self), left, right, w),
```

本体は次の順に書く。

1. 検査モードの検査を、状態を変える前にすべて行う。left と right のそれぞれに `checked_live`、種類、`checked_decref`。`left == right` なら `checked_decref_twice`
2. right の中身を `text` で借り、`string_push` の本体と同じ処理を呼ぶ。借用と `&mut self` がぶつかるので、`string_push` の本体を、検査を済ませた後の内部の関数 `push_unchecked(&mut self, h: *mut Header, s: &str, ..)` に分け、`string_push` と `string_concat` の両方から呼ぶ。right の中身は、`text_ptr` で得た位置を `string_push` と同じく `&str` として渡す (Verus の中では `text` で借りた `&str` の ghost の値を、`push_unchecked` の `requires` で `s.spec_bytes() == old.text_of(right)` として渡す)。right の権限を借りたまま left のブロックを書けない形になるときは、`push_unchecked` が受けるのを `right` のヘッダにし、中で right の権限を借りて読む
3. right を `decref` する。right は子を持たないので、この `decref` は誤りを返さない (検査は 1. で済んでいる)。`left == right` で一意でない場合は、`push_unchecked` の写す道が left の参照を1つ手放した後でも、right の参照がまだ1つ残っている

Run: `cargo build -p eml_rt && cargo verus verify -p eml_rt && cargo test -p eml_rt string::`
Expected: すべて通る。

- [ ] **Step 5: T10 の Miri のテストの残りを書く**

`heap/tests/trusted.rs` の `// T10` の節に足し、`both_modes!` の一覧に名前を足す。

```rust
fn unaligned_appends_are_written<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut w = heap.alloc_string("");
    let mut expected = String::new();
    // 1, 2, 3, ... バイトずつ足して、語の境目をまたぐ書き込みを作る
    for n in 1..40 {
        let piece = "q".repeat(n);
        w = unsafe { heap.string_push(w, &piece) }.unwrap();
        expected.push_str(&piece);
    }
    assert_eq!(text(&heap, w), expected);
    unsafe { heap.decref(w) }.unwrap();
}

fn large_strings_cross_pages<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let big = "L".repeat(PAGE_BYTES + 10);
    let w = heap.alloc_string(&big);
    // 大きい左辺からの作り直し
    let w = unsafe { heap.string_push(w, &big) }.unwrap();
    assert_eq!(text(&heap, w).len(), 2 * big.len());
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn empty_strings_are_appended<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = heap.alloc_string("");
    let b = heap.alloc_string("");
    let r = unsafe { heap.string_concat(a, b) }.unwrap();
    assert_eq!(text(&heap, r), "");
    let r = unsafe { heap.string_push(r, "") }.unwrap();
    assert_eq!(text(&heap, r), "");
    unsafe { heap.decref(r) }.unwrap();
}

fn own_bytes_are_appended_under_both_borrow_models<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for s in ["abc", "abcdefgh", &"m".repeat(PAGE_BYTES)] {
        let w = heap.alloc_string(s);
        let own = unsafe { &*heap.string(w).unwrap() };
        let w = unsafe { heap.string_push(w, own) }.unwrap();
        assert_eq!(text(&heap, w).len(), 2 * s.len());
        unsafe { heap.decref(w) }.unwrap();
    }
}

fn reading_then_appending<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("first");
    // 読んだ位置の使用を終えてから追記する
    let seen = unsafe { &*heap.string(w).unwrap() }.to_string();
    let w = unsafe { heap.string_push(w, "+second") }.unwrap();
    assert_eq!(seen, "first");
    assert_eq!(text(&heap, w), "first+second");
    unsafe { heap.decref(w) }.unwrap();
}
```

Run: `cargo test -p eml_rt trusted::` と Miri の2つの設定
Expected: 通る。

- [ ] **Step 6: 全体を確かめてコミットする**

Run: Task 1 の Step 7 と同じ。
Expected: すべて通る。

```bash
git add crates/eml_rt
git commit -m "Add string_push and string_concat to eml_rt"
```

本文に、その場の追記と倍の容量での作り直し、誤りを確保より前に返すこと、`string_concat` を `string_push` の上に組んだことを書き、verified の数を付ける。

---

### Task 3: 計測と文書

**Files:**
- Modify: `crates/eml_rt/examples/rt_bench.rs`、`bench/rt.sh`
- Modify: `docs/implementation/benchmarks.md`、`docs/spec/object-model.md`、`docs/future/roadmap.md`、`docs/implementation/testing.md`
- Delete: `docs/superpowers/specs/2026-10-14-s9b1-object-contents-design.md`、`docs/superpowers/plans/2026-10-14-s9b1-object-contents.md`

**Interfaces:**
- Consumes: Task 1 と Task 2 の公開の操作

- [ ] **Step 1: 追記の計測を足す**

`rt_bench.rs` に場合 `append` を足す。`main` の `expect` の文言と `match` の腕、`bench/rt.sh` の `for case in ...` の一覧にも `append` を足す。

```rust
const APPENDS: usize = 10_000_000;

/// 一意な文字列に 1 バイトずつ足していく。その場の追記と、倍の容量での作り直しの費用である。
fn append() {
    let mut heap = Heap::<Fast>::new();
    let mut s = heap.alloc_string("");
    for _ in 0..APPENDS {
        // SAFETY: s は直前の追記が返した生きた文字列で、参照を1つ持っている。
        s = unsafe { heap.string_push(black_box(s), "x") }.unwrap();
    }
    // SAFETY: 同上。
    unsafe { heap.decref(s) }.unwrap();
}
```

Run: `cargo run --release -p eml_rt --example rt_bench append && bench/rt.sh`
Expected: 表が出る。ほかの場合の値が S9v の終わり (`benchmarks.md` の「S9v (`eml_rt` の検証)」の表) から 0.5% を超えて増えていないことを確かめる。増えていたら原因 (`raw_check` の欄の置き換えなど) を調べて報告する。

- [ ] **Step 2: `benchmarks.md` に記録する**

「S9v (`eml_rt` の検証)」の後に「S9b-1 (文字列)」の節を足す。`bench/rt.sh` の出力 (機種、OS、rustc、コミット、表) をそのまま貼り、`append` の1回あたりの命令の数 (`empty` を引いて `APPENDS` で割る) と、ほかの場合の S9v の終わりとの差を書く。`eml_rt` は `eml run` から使わないので `bench/run.sh` の表は測らないことも、S9v の節と同じ文で書く。

- [ ] **Step 3: `object-model.md` を直す**

spec の「文書」の節のとおりに直す。

- 「種類」: 表に `String` (番号 3、走査の始まり 0、token にできない、破棄処理なし) と「ペイロードが不透明か」の欄 (`Foreign` と `String` が「不透明」) を足す。`String` を token にしない理由 (token の箱は `Data`)、後の項目の種類の見通しを足す。「まだ決めていないこと」の段落を、残る項目 (`File` の物体の中身の型は S9b-3) だけにする
- 「ペイロード」: 物理的な形の規則、規則の関数の置き場所 (`eml_lir`、S10 で足す) を書き、「まだ決めていないこと」を消す
- 新しい節「物体の形」: spec の「物体の形」の表、クロージャの読み方 (`scanned(w, 0)`)、`copy` を作らない理由と部品の並べ方、部分適用で作るクロージャの `lin` ビット、evidence の3つの物体と tag、状態の欄の書き換えと `par` の申し送り
- 新しい節「文字列」: 不変条件、ghost のバイト列、5つの操作の表、`string_push` の動作の図、誤りを確保より前に返すこと
- 「核の操作」: 前提の表の「物体を受ける操作」の行、`alloc` / `alloc_immortal` / `con_at` の行 (種類 `String` を断る)、`raw` / `set_raw` の行 (不透明なペイロードを `WrongKind`)、文字列の操作の行を足す。冒頭の「生きている物体」の定義に「`String` なら長さの語」を足す。解放の作業リストの段落を cons セルの形と、再帰するフィールドが最後の走査する語であるという条件で書き直し、「まだ決めていないこと」から閉じた項目 (`copy`、cons セルの形) を消す
- 「語」の「まだ決めていないこと」: クロージャの形を消し、`Float` の語の表現はロードマップの「`Float`、`Char`、`Num`」に移したと書く
- 「報告する誤り」: `WrongKind` の項目に `string`、`string_push`、`string_concat`、`raw` と `set_raw` の不透明なペイロードを足す
- 「`unsafe` の核」: 信頼の一覧に T10 の行を足す (spec の「信頼の一覧の T10」の表)

- [ ] **Step 4: `roadmap.md` と `testing.md` を直す**

`roadmap.md` の「S9 共通ランタイム `eml_rt`」:

- 冒頭の文を「S9a、S9v、S9b-1、S9b-2、S9b-3 の5つの作業」にし、作業の表の S9b の行を3行に分ける。S9b-1 の状態は「終えた。決めたことは [オブジェクトモデル](../spec/object-model.md) の「種類」、「物体の形」、「文字列」、「`unsafe` の核」に移した。UI テストの出力と `bench/` の仕事の回数は変わらない ([計測の基準](../implementation/benchmarks.md) の「S9b-1 (文字列)」)」、S9b-2 と S9b-3 は「これから」で、中身は spec の「目的と範囲」の表のとおりである
- 「論点」から、形の規則の置き場所、文字列の表現、`Float` を入れたときの語の表現を消す。`Float` の論点は「`Float`、`Char`、`Num`」の「論点」の「多相な位置での `Float` の表現」の項目に、「`eml_rt` の語の表現 (`tobj` では `BigInt` と同じく箱、`float` の位置では生の 64 ビットを方向とする) も、ここで決める」と足す
- 「決めたこと」の「リスト、文字列、レコードの物理的な表現を決める」は、S9b-1 で決めたことを書く (オブジェクトモデルへのリンク)

`testing.md` の「`eml_rt` の検証」: Miri の対象の説明に T10 を足す (信頼の一覧の番号を並べている所があれば、そこに足す)。

- [ ] **Step 5: spec と計画を消して、全体を確かめてコミットする**

```bash
git rm docs/superpowers/specs/2026-10-14-s9b1-object-contents-design.md docs/superpowers/plans/2026-10-14-s9b1-object-contents.md
```

Run: `rg -n 's9b1|S9b-1 の設計' docs crates` で、消した文書への参照が残っていないことを確かめる。Task 1 の Step 7 と同じ全体の確かめを流す。

```bash
git add -A docs crates/eml_rt bench
git commit -m "Document the eml_rt object contents and record the string measurement"
```

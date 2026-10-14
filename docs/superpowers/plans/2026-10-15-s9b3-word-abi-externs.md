# S9b-3 語の ABI の extern Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_extern` の表の関数の行25個を `eml_rt` に語の ABI で1回だけ書き、OS とのやりとりをセッションの `Host` にまとめ、今の実装と同じ結果と誤りの文言であることを期待値の表で確かめる。

**Architecture:** `Ctx` を取らない算術と比較は `src/externs.rs` の `#[inline]` の関数にし、ヒープを使う文字列と I/O の extern は `Ctx` のメソッドとして `heap/ctx/` の子の module に置く (`Ctx` と `Heap` の閉じた spec を開けるようにするため)。OS とのやりとりは `src/host.rs` の trait `Host` にまとめ、`Ctx` は不透明な `HostRef` で持つ。期待値の表を1つ置き、`eml_rt` のテストと `eml_interp` のテストが同じ表で確かめる。

**Tech Stack:** Rust (edition 2024)、Verus (vstd `=0.0.0-2026-10-04-0306`)、Miri (nightly)、insta は使わない (表はテキストのファイル)。

**Spec:** `docs/superpowers/specs/2026-10-15-s9b3-word-abi-externs-design.md`

## Global Constraints

- `eml_rt` の `[dependencies]` は vstd だけのままにする。`eml_interp` の `[dev-dependencies]` に `eml_rt` を足すのはよい
- `eml_rt` の新しいコードは `verus!` の中に書く。`verus!` の外に置いてよいのは、spec の「信頼の一覧」に挙げたもの (`Host`、`Opened`、`Buffering`、`StdHost`、`HostRef::new` と `HostRef` の `Drop`、`Fault` と `ArithFault` と `Stop` の `Display`、`io::Error` を写す関数) と、今の一覧にあるものだけである
- 新しいソースのファイルは `crates/eml_rt/src/trust.rs` の `FILES` に足す。新しい信頼の項目は `TRUSTED` に足し、行の名前は `T0`〜`T11` のどれかにする (`T11` は Task 4 で足す)
- 誤りの文言は、今の `eml_interp::Fault` と1文字も違わない: `division by zero`、`integer overflow`、``cannot write the output: {e}``、``cannot open `{path}`: {reason}``、``cannot read `{path}`: {reason}``、``` `{path}` is not valid UTF-8 ```、理由は `not found`、`permission denied`、`I/O error`
- タグの値: `FALSE=0`、`TRUE=1`、`LT=0`、`EQ=1`、`GT=2` (`i64`)、`TUPLE=0` (`u16`)
- `read_all` は 64 KiB ずつ読む。`StdHost` の `Block` は 64 KiB の `BufWriter`
- 計測: 今ある `rt_bench` の14の計測は、`benchmarks.md` の「S9b-2b (実行文脈)」の値から +0.5% を超えて増えない。extern の計測は別のプログラム `rt_extern_bench` に置く
- Miri の `trusted::` のテストはメモリの Host だけを使い、`StdHost` と実際のファイルは使わない。テストを `cfg!(miri)` で小さくしない
- コードのコメントと `docs/` は日本語で、なぜを書く。日本語は `yomiyasu:yomiyasu` のスキルに従い、`python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py <file>` で確かめる
- 確かめる手順 (どのタスクの終わりにも全部通す)
  - `cargo test`
  - `find target -path '*/.fingerprint/eml_rt-*' -prune -exec rm -rf {} +` の後に `cargo verus verify -p eml_rt` (0 errors)
  - `cargo clippy --all-targets`
  - `cargo fmt --check`
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、`-Zmiri-tree-borrows` を足したもの
  - 計測を変えるタスクでは `bench/rt.sh` (macOS)
- `git diff` は difftastic を使うので、スクリプトでは `--no-ext-diff` を付ける
- コミットの終わりに次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014bwniBqkZgCLERCaKyDB5S
  ```

## Review Focus

- 同じ文字列を両方の引数に渡す (`s == s`、`compare s s`、`s ++ s`)。呼ぶ側は2つの参照を持って渡すので、2回 `decref` しても正しく動き、物体が残らないこと (Task 3 のテスト `the_same_string_on_both_sides`)
- `i64::MIN` の `Int.show`、`negate`、`MIN / -1`、`MIN % -1` (Task 2 の `arithmetic_matches_checked_rust`、Task 3 の `ints_show_in_decimal`)
- 読み手が途中で `Interrupted` を返す (シグナルで中断された読み)。誤りにせず読み直すこと (Task 5 の `an_interrupted_read_is_retried`)
- `Block` の `StdHost` を明示の `flush` なしで落とす。書いていない出力が落とすときに書かれること (Task 4 の `a_dropped_block_host_writes_what_it_holds`)
- 空の文字列の `println` と、空のファイルの `read_all` (Task 4 の `println_writes_a_line_and_releases_its_text`、Task 5 の `an_empty_file_reads_as_an_empty_string`)

## ファイルの構成

| ファイル | 役割 |
|---|---|
| `crates/eml_rt/src/externs.rs` (新) | `ArithFault`、`Fault`、`IoOp`、`IoReason`、タグの定数、`Ctx` を取らない算術と比較の extern。`Display` は `verus!` の外 |
| `crates/eml_rt/src/host.rs` (新) | `Host`、`Stream`、`Opened`、`Buffering`、`StdHost` (`verus!` の外)。`HostRef`、`File`、Host を呼ぶ `external_body` の関数 (`verus!` の中) |
| `crates/eml_rt/src/heap/ctx/strings.rs` (新) | 文字列の extern (`string_eq`、`string_ne`、`string_compare`、`string_append`、`int_show`、`string_show`) と spec の関数 `lex`、`decimal`、`escape` |
| `crates/eml_rt/src/heap/ctx/io.rs` (新) | I/O の extern (`println`、`flush`、`fs_open`、`fs_read_all`、`fs_close`) |
| `crates/eml_rt/src/heap/string.rs` | `alloc_string_room`、`string_push_part` |
| `crates/eml_rt/src/heap/mod.rs` | `foreign_unique` |
| `crates/eml_rt/src/heap/ctx.rs` | `Ctx` の欄 `host`、`Ctx::new(control, host)`、`CtxStats::string_bytes_copied`、子の module の宣言 |
| `crates/eml_rt/src/control.rs` | `Stop` の `Display` |
| `crates/eml_rt/src/lib.rs` | `pub mod externs;`、`pub use host::…` |
| `crates/eml_rt/src/trust.rs` | `FILES`、`TRUSTED`、行の名前の集合に `T11` |
| `crates/eml_rt/src/heap/tests/externs.rs` (新) | extern の単体テスト (`both_modes!`) |
| `crates/eml_rt/src/heap/tests/host.rs` (新) | テストの Host (`MemoryHost`、`MemoryFile`) と `StdHost` のテスト |
| `crates/eml_rt/src/heap/tests/table.rs` (新) | 期待値の表の形 (読み書き、場合の生成、ファイルの用意)。`eml_interp` のテストも `#[path]` で読む |
| `crates/eml_rt/src/heap/tests/expected.txt` (新) | 期待値の表 |
| `crates/eml_rt/src/heap/tests/trusted.rs` | Miri のテスト (T3、T10、T11、T8) |
| `crates/eml_interp/tests/rt_externs.rs` (新) | 今の実装が表と同じ結果になること |
| `crates/eml_rt/examples/rt_extern_bench.rs` (新)、`bench/rt.sh` | extern の計測 |

---

### Task 1: ヒープの部品 (`alloc_string_room`、`string_push_part`、`foreign_unique`)

**Files:**
- Modify: `crates/eml_rt/src/heap/string.rs`
- Modify: `crates/eml_rt/src/heap/mod.rs`
- Test: `crates/eml_rt/src/heap/tests/string.rs`、`crates/eml_rt/src/heap/tests/trusted.rs`

**Interfaces:**
- Consumes: 今の `make_text`、`Tail`、`write_tail`、`text` (T10)、`foreign` (T3)
- Produces:
  - `pub fn alloc_string_room(&mut self, room: usize) -> (w: Word)`: 空の文字列で、容量が `room` バイト以上
  - `pub unsafe fn string_push_part(&mut self, left: Word, src: Word, from: usize, to: usize) -> (r: Result<Word, HeapError>)`: `left` の後ろに `src` のバイトの範囲 `[from, to)` を足す。`left` は消費し、`src` は借りる
  - `pub unsafe fn foreign_unique<T: 'static>(&self, w: Word) -> (r: Result<*mut T, HeapError>)`: 書き換えるための `foreign`。局所で rc が 1 で、今の worker が持つ物体だけを通す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_rt/src/heap/tests/string.rs` に足し、ファイルの終わりの `both_modes!` に名前を足す。

```rust
/// 容量を決めて作った空の文字列は、容量の中の追記をその場で受ける (`String.show` が1回だけ確保するため)。
fn a_room_string_takes_pushes_in_place<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let s = heap.alloc_string_room(10);
    assert_eq!(text(&heap, s), "");
    unsafe {
        let t = heap.string_push(s, "abc").unwrap();
        assert_eq!(t, s);
        let u = heap.string_push(t, "defghij").unwrap();
        assert_eq!(u, s);
        assert_eq!(text(&heap, u), "abcdefghij");
        heap.decref(u).unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
}

fn a_part_of_another_string_is_pushed<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // a (1 バイト)、é (2 バイト)、日 (3 バイト)、b (1 バイト)
    let src = heap.alloc_string("aé日b");
    let left = heap.alloc_string_room(8);
    unsafe {
        let w = heap.string_push_part(left, src, 1, 6).unwrap();
        assert_eq!(text(&heap, w), "é日");
        let w = heap.string_push_part(w, src, 0, 0).unwrap();
        assert_eq!(text(&heap, w), "é日");
        let w = heap.string_push_part(w, src, 6, 7).unwrap();
        assert_eq!(text(&heap, w), "é日b");
        assert_eq!(text(&heap, src), "aé日b");
        heap.decref(w).unwrap();
        heap.decref(src).unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
}

/// 文字の境目でない範囲と、長さを超える範囲は、検査モードで断る。断ったときは何も変えない。
fn a_part_off_a_char_boundary_is_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let src = heap.alloc_string("é");
    let left = heap.alloc_string("x");
    unsafe {
        assert_eq!(heap.string_push_part(left, src, 0, 1), Err(HeapError::OutOfBounds));
        assert_eq!(heap.string_push_part(left, src, 1, 2), Err(HeapError::OutOfBounds));
        assert_eq!(heap.string_push_part(left, src, 0, 3), Err(HeapError::OutOfBounds));
        assert_eq!(text(&heap, left), "x");
        heap.decref(left).unwrap();
        heap.decref(src).unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
}

fn a_unique_foreign_value_can_be_written<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(vec![1u8], true);
    unsafe {
        let p = heap.foreign_unique::<Vec<u8>>(f).unwrap();
        (*p).push(2);
        assert_eq!(*heap.foreign::<Vec<u8>>(f).unwrap(), vec![1, 2]);
        assert_eq!(heap.foreign_unique::<String>(f), Err(HeapError::WrongKind));
        heap.decref(f).unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
}

/// 参照が2つある物体と、ほかの worker の物体は、書き換えのために借りられない。
fn a_shared_or_foreign_workers_value_is_not_written<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(0u64, false);
    unsafe {
        heap.dup(f).unwrap();
        assert_eq!(heap.foreign_unique::<u64>(f), Err(HeapError::NotUnique));
        heap.decref(f).unwrap();
        heap.set_worker(1);
        assert!(matches!(heap.foreign_unique::<u64>(f), Err(HeapError::WrongOwner { .. })));
        heap.set_worker(0);
        heap.decref(f).unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
}
```

`string.rs` のテストの先頭の `use` に `HeapError` がなければ足す。`set_worker` は今ある `Heap::set_worker(&mut self, worker: u32)` である。

Run: `cargo test -p eml_rt string::`
Expected: コンパイルの誤り (`alloc_string_room`、`string_push_part`、`foreign_unique` がない)

- [ ] **Step 2: `alloc_string_room` を作る**

`heap/string.rs` の `alloc_string` の隣に置く。本体は `make_text(room, Tail::Str(""), None, 1)` である。

```rust
/// 容量が `room` バイト以上の空の文字列を作る。結果の長さを先に数えられる extern (`String.show`) が、追記の
/// たびの作り直しを避けるために使う
pub fn alloc_string_room(&mut self, room: usize) -> (w: Word)
    requires
        old(self).wf(),
        old(self).safe_settleable(),
        !M::CHECKED ==> old(self).settleable(),
    ensures
        final(self).made(*old(self), w),
        final(self).is_string(w),
        final(self).text_of(w) == Seq::<u8>::empty(),
        M::CHECKED ==> final(self).words_kept(*old(self)),
```

`room` が大きすぎて容量の語の数が溢れるときは、今の `make_text` の確保と同じ panic の道を通る (T5)。

- [ ] **Step 3: `string_push_part` を作る**

spec は T10 の写す関数を範囲を受ける形に広げるとしていたが、vstd の `str::split_at` の約束で範囲を切り出せるので、T10 の関数は変えない (信頼の一覧が増えない方を採る)。`Tail::Text` に範囲を持たせる: `Text(*mut Header, usize)` を `Text(*mut Header, usize, usize)` (`from`、`to`) にし、`string_concat` は `Text(h, 0, len)` を渡す。`write_tail` の `Text` の道は、`text(h, len, perm)` で借りた `&str` を vstd の `str::split_at` で2回切り (`is_char_boundary` を前提にする vstd の約束がある)、切り出した `&str` を今の `write_text` に渡す。T10 の関数は変えない。

```rust
/// `left` の後ろに `src` のバイトの範囲 `[from, to)` を足す。`left` を消費し、`src` は借りる。
///
/// # Safety
///
/// 速いモードでは、`left` について `string_push` と同じ前提 (`push_ok`) を、`src` について読める文字列で
/// あることと、`from <= to <= len` で両端が文字の境目であることを前提にする。`left` と `src` は違う物体で
/// なければならない (`left` を作り直すと `src` の中身を指す位置が古くなるため)。
pub unsafe fn string_push_part(&mut self, left: Word, src: Word, from: usize, to: usize) -> (r: Result<Word, HeapError>)
    requires
        old(self).wf(),
        old(self).safe_settleable(),
        left.is_ptr_spec(),
        src.is_ptr_spec(),
        left != src,
        old(self).word_in(left),
        old(self).word_in(src),
        !M::CHECKED ==> old(self).part_ok(left, src, from, to),
    ensures
        final(self).wf(),
        final(self).same_tokens(*old(self)),
        r is Err ==> *final(self) == old(self).settled_heap(),
        !M::CHECKED ==> r is Ok,
        r matches Ok(w) ==> final(self).is_string(w)
            && final(self).text_of(w) == old(self).text_of(left) + old(self).text_of(src).subrange(from as int, to as int),
        old(self).part_ok(left, src, from, to) ==> r is Ok || chain_err(r),
        r matches Ok(w) ==> (old(self).part_ok(left, src, from, to) ==> final(self).consistent()
            && final(self).pushed(*old(self), left, w)),
        M::CHECKED ==> final(self).words_kept(*old(self)),
```

- `part_ok(left, src, from, to)` は新しい `closed spec fn`: `push_ok(left) && readable(src) && is_string(src) && from <= to <= text_of(src).len() && is_char_boundary(text_of(src), from) && is_char_boundary(text_of(src), to)` (vstd の `is_char_boundary`)
- 検査モードは、`src` が文字列でなければ `WrongKind`、範囲が長さを超えるか両端が文字の境目でなければ `OutOfBounds` を返す。境目は、バイトが `0x80..=0xBF` (続きのバイト) でないことで確かめる。どの確かめも、`settle` のほかは状態を変える前に行う
- 本体は `string_push` と同じ形にし、足す側を `Tail::Text(src の h, from, to)` にする。`pushed` の約束 (その場で足すか作り直すか) も `string_push` と同じである

- [ ] **Step 4: `foreign_unique` を作る**

`heap/mod.rs` の `foreign` の隣に置く。

```rust
/// 書き換えるために `Foreign` の値を借りる。`Fs.read_all` が `File` の読み手を進めるために使う。
///
/// # Safety
///
/// 速いモードでは、`w` が読める物体で、局所で rc が 1 で、今の worker が持つことを前提にする。返したポインタは、
/// 次にこの物体の参照を手放すか、ほかの参照を作るまでの間だけ使える。
pub unsafe fn foreign_unique<T: 'static>(&self, w: Word) -> (r: Result<*mut T, HeapError>)
    requires
        self.wf(),
        w.is_ptr_spec(),
        self.word_in(w),
        !M::CHECKED ==> self.settleable() && self.readable(w) && self.owned(w) && self.rc_value(w) == 1,
    ensures
        r is Err ==> M::CHECKED || r == Err::<*mut T, HeapError>(HeapError::WrongKind),
```

- 検査モードは、読めなければ `UseAfterFree`、rc が 1 でなければ (2以上も、共有の負の値も) `NotUnique`、持ち主が今の worker でなければ `WrongOwner { owner, worker }` を返す。そのあと `foreign::<T>` と同じ型の照らし合わせをする (どちらのモードでも型が違えば `WrongKind`)
- 中身は `is_unique` と `owned` の確かめと、今の `foreign` の本体である。`foreign` を呼んでよい

- [ ] **Step 5: Miri のテストを足す**

`heap/tests/trusted.rs` の `// T10` の節に、範囲の写しのテストを足し、ファイルの終わりの `both_modes!` に名前を足す。

```rust
/// `string_push_part` は、写す元の文字列の範囲を写す (T10 の写しを、先頭、途中、末尾、多バイトの文字の前後で通す)。
fn parts_of_a_string_are_copied<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let src = heap.alloc_string("ab日本cd");
    unsafe {
        let mut w = heap.alloc_string_room(0);
        for (from, to) in [(0, 1), (2, 5), (5, 8), (8, 10), (10, 10)] {
            w = heap.string_push_part(w, src, from, to).unwrap();
        }
        assert_eq!(text(&heap, w), "a日本cd");
        heap.decref(w).unwrap();
        heap.decref(src).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}
```

`// T3` の節に、書き換えるための借用のテストを足す。

```rust
/// `foreign_unique` で借りて書き換えた値は、解放のときに1回だけ後始末される。
fn a_foreign_value_written_in_place_is_finalized_once<M: Mode>() {
    let (p, drops) = probe();
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(Some(p), true);
    unsafe {
        let v = heap.foreign_unique::<Option<Probe>>(f).unwrap();
        let (q, more) = probe();
        let old = (*v).replace(q);
        drop(old);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        heap.decref(f).unwrap();
        assert_eq!(more.load(Ordering::Relaxed), 1);
    }
}
```

`Probe` と `probe` は `heap/tests/mod.rs` の補助である。`trusted.rs` の `use` に `Probe` を足す。

- [ ] **Step 6: 全体を確かめてコミットする**

Global Constraints の確かめる手順を全部流す (計測は不要)。

```bash
git add crates/eml_rt
git commit -m "Add a sized empty string, a ranged string push and a writable foreign borrow"
```

---

### Task 2: 誤りの型、整数と `Bool` の extern、タグの定数

**Files:**
- Create: `crates/eml_rt/src/externs.rs`
- Modify: `crates/eml_rt/src/lib.rs`、`crates/eml_rt/src/control.rs`、`crates/eml_rt/src/trust.rs`
- Test: `crates/eml_rt/src/heap/tests/externs.rs` (新)、`crates/eml_rt/src/heap/tests/mod.rs`

**Interfaces:**
- Consumes: `HeapError`、`Stop`
- Produces (`eml_rt::externs` から公開し、`ArithFault`、`Fault`、`IoOp`、`IoReason` は `eml_rt` の直下からも `pub use` する):
  - `pub enum ArithFault { DivisionByZero, IntegerOverflow }` (`Copy`)
  - `pub enum Fault { Heap(HeapError), Arith(ArithFault), Output(Box<str>), Io { op: IoOp, path: Box<str>, reason: IoReason }, NotUtf8 { path: Box<str> }, Stopped(Stop) }`、`impl From<ArithFault> for Fault`、`impl From<HeapError> for Fault`
  - `pub enum IoOp { Open, Read }`、`pub enum IoReason { NotFound, PermissionDenied, Other }` (どちらも `Copy`)
  - `pub const FALSE: i64 = 0; pub const TRUE: i64 = 1; pub const LT: i64 = 0; pub const EQ: i64 = 1; pub const GT: i64 = 2; pub const TUPLE: u16 = 0;`
  - `#[inline] pub fn int_add(a: i64, b: i64) -> Result<i64, ArithFault>`、同じ形の `int_sub`、`int_mul`、`int_div`、`int_rem`
  - `#[inline] pub fn int_negate(a: i64) -> Result<i64, ArithFault>`
  - `#[inline] pub fn int_eq(a: i64, b: i64) -> i64`、同じ形の `int_ne`、`int_lt`、`int_le`、`int_gt`、`int_ge`、`int_compare`
  - `#[inline] pub fn bool_eq(a: i64, b: i64) -> i64`、`bool_ne`

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests/mod.rs` の module の一覧に `mod externs;` を足し、`heap/tests/externs.rs` を作る。

```rust
//! extern の単体テスト。結果は spec のとおりで、今の実装と同じ文言で誤りを返すことを確かめる。

use std::cmp::Ordering as Order;

use crate::externs::*;
use crate::{HeapError, Stop};

const EDGES: [i64; 13] = [
    0, 1, -1, 2, -2, i64::MIN, i64::MIN + 1, i64::MAX, i64::MAX - 1, 1 << 31, -(1 << 31), 1 << 62, -(1 << 62),
];

fn ordering(o: Order) -> i64 {
    match o {
        Order::Less => LT,
        Order::Equal => EQ,
        Order::Greater => GT,
    }
}

#[test]
fn arithmetic_matches_checked_rust() {
    for a in EDGES {
        assert_eq!(int_negate(a), a.checked_neg().ok_or(ArithFault::IntegerOverflow));
        for b in EDGES {
            let over = ArithFault::IntegerOverflow;
            assert_eq!(int_add(a, b), a.checked_add(b).ok_or(over));
            assert_eq!(int_sub(a, b), a.checked_sub(b).ok_or(over));
            assert_eq!(int_mul(a, b), a.checked_mul(b).ok_or(over));
            let zero = Err(ArithFault::DivisionByZero);
            assert_eq!(int_div(a, b), if b == 0 { zero } else { a.checked_div(b).ok_or(over) });
            assert_eq!(int_rem(a, b), if b == 0 { zero } else { a.checked_rem(b).ok_or(over) });
            assert_eq!(int_eq(a, b), (a == b) as i64);
            assert_eq!(int_ne(a, b), (a != b) as i64);
            assert_eq!(int_lt(a, b), (a < b) as i64);
            assert_eq!(int_le(a, b), (a <= b) as i64);
            assert_eq!(int_gt(a, b), (a > b) as i64);
            assert_eq!(int_ge(a, b), (a >= b) as i64);
            assert_eq!(int_compare(a, b), ordering(a.cmp(&b)));
        }
    }
}

#[test]
fn bools_compare_by_tag() {
    for a in [FALSE, TRUE] {
        for b in [FALSE, TRUE] {
            assert_eq!(bool_eq(a, b), (a == b) as i64);
            assert_eq!(bool_ne(a, b), (a != b) as i64);
        }
    }
}

/// 文言は今の `eml_interp::Fault` と同じである (S10 で切り替えても UI テストの出力を変えないため)。
#[test]
fn faults_print_as_the_interpreter_does() {
    let cases: [(Fault, &str); 9] = [
        (ArithFault::DivisionByZero.into(), "division by zero"),
        (ArithFault::IntegerOverflow.into(), "integer overflow"),
        (Fault::Output("disk full".into()), "cannot write the output: disk full"),
        (Fault::Io { op: IoOp::Open, path: "a.txt".into(), reason: IoReason::NotFound }, "cannot open `a.txt`: not found"),
        (
            Fault::Io { op: IoOp::Open, path: "p".into(), reason: IoReason::PermissionDenied },
            "cannot open `p`: permission denied",
        ),
        (Fault::Io { op: IoOp::Read, path: "d".into(), reason: IoReason::Other }, "cannot read `d`: I/O error"),
        (Fault::NotUtf8 { path: "b.bin".into() }, "`b.bin` is not valid UTF-8"),
        (HeapError::WrongKind.into(), "an object is not of the expected kind"),
        (Fault::Stopped(Stop::Interrupted), "interrupted"),
    ];
    for (fault, text) in cases {
        assert_eq!(fault.to_string(), text);
    }
    assert_eq!(Stop::MemoryLimit.to_string(), "the memory limit is exceeded");
    assert_eq!(Stop::DepthLimit.to_string(), "the call depth limit is exceeded");
}

/// 算術の誤りは2つのレジスタで返り、`Fault` は 24 バイトに収まる。
#[test]
fn faults_stay_small() {
    assert_eq!(std::mem::size_of::<Result<i64, ArithFault>>(), 16);
    assert_eq!(std::mem::size_of::<Fault>(), 24);
}
```

`Stop` の文言はこのタスクで決める (S10 の VM も安全点で止めたときにこの `Display` を使う)。

Run: `cargo test -p eml_rt externs::`
Expected: コンパイルの誤り (`crate::externs` がない)

- [ ] **Step 2: `externs.rs` を作る**

- ファイルの先頭に `use vstd::prelude::*;` と `verus! { … }` を置き、型と関数を `verus!` の中に書く。`Display` は `verus!` の外に書く
- `Fault` の `Display`: `Heap(e)` は `{e}`、`Arith` は2つの文言、`Output(e)` は ``cannot write the output: {e}``、`Io` は `op` で ``cannot open `{path}`: {reason}`` か ``cannot read `{path}`: {reason}``、`NotUtf8` は ``` `{path}` is not valid UTF-8 ```、`Stopped(s)` は `{s}`。`IoReason` の `Display` は `not found`、`permission denied`、`I/O error`
- 整数の関数の約束は vstd の spec で書く。例

```rust
#[inline]
pub fn int_add(a: i64, b: i64) -> (r: Result<i64, ArithFault>)
    ensures
        r is Ok <==> i64::MIN <= a + b <= i64::MAX,
        r matches Ok(v) ==> v == a + b,
        r is Err ==> r == Err::<i64, ArithFault>(ArithFault::IntegerOverflow),
{
    match a.checked_add(b) {
        Some(v) => Ok(v),
        None => Err(ArithFault::IntegerOverflow),
    }
}
```

- `int_negate` は `0i64.checked_sub(a)` で作る (vstd に `checked_neg` の spec がないため)
- `int_div` と `int_rem` は、`b == 0` なら `DivisionByZero`、そうでなければ `checked_div` と `checked_rem` (vstd の `rust_div` と `rust_rem`) の結果で、`None` なら `IntegerOverflow`。約束は `b != 0 && !(a == i64::MIN && b == -1) ==> r == Ok(rust_div(a, b))` の形で書く
- 比較は `if a < b { TRUE } else { FALSE }` の形で書き、`ensures r == if a < b { TRUE } else { FALSE }` とする。`int_compare` は `LT`、`EQ`、`GT`
- `bool_eq` は `if a == b { TRUE } else { FALSE }`、`bool_ne` はその逆。引数に前提を置かない
- `control.rs` の `verus!` の外に `impl fmt::Display for Stop` を置く: `Interrupted` は `interrupted`、`MemoryLimit` は `the memory limit is exceeded`、`DepthLimit` は `the call depth limit is exceeded`
- `lib.rs` に `pub mod externs;` と `pub use externs::{ArithFault, Fault, IoOp, IoReason};` を足す
- `trust.rs` の `FILES` に `("externs.rs", include_str!("externs.rs"))` を足す

- [ ] **Step 3: 全体を確かめてコミットする**

Global Constraints の確かめる手順を全部流す (計測は不要)。

```bash
git add crates/eml_rt
git commit -m "Add the extern faults and the integer and Bool externs"
```

---

### Task 3: 文字列の extern

**Files:**
- Create: `crates/eml_rt/src/heap/ctx/strings.rs`
- Modify: `crates/eml_rt/src/heap/ctx.rs`、`crates/eml_rt/src/trust.rs`
- Test: `crates/eml_rt/src/heap/tests/externs.rs`

**Interfaces:**
- Consumes: Task 1 の `alloc_string_room`、`string_push_part`。Task 2 の `Fault`、`TRUE`、`FALSE`、`LT`、`EQ`、`GT`。今の `string_concat`、`alloc_string`、`text_of`、`is_string`、`Ctx` の `held_ok`、`holds_args`、`holds`、`evolved_by`
- Produces (`Ctx<M>` のメソッド):
  - `pub unsafe fn string_eq(&mut self, a: Word, b: Word) -> Result<i64, Fault>`、同じ形の `string_ne`、`string_compare`
  - `pub unsafe fn string_append(&mut self, a: Word, b: Word) -> Result<Word, Fault>` (`++`)
  - `pub fn int_show(&mut self, i: i64) -> Word`
  - `pub unsafe fn string_show(&mut self, s: Word) -> Result<Word, Fault>`
  - `CtxStats` の新しい欄 `pub string_bytes_copied: u64`
  - spec の関数 `pub open spec fn lex(a: Seq<u8>, b: Seq<u8>) -> i64`、`pub open spec fn decimal(i: int) -> Seq<u8>`、`pub open spec fn escape(s: Seq<u8>) -> Seq<u8>`

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests/externs.rs` に足す。`both_modes!` を使うので、ファイルの先頭の `use` に `crate::{Mode, Word}`、`super::ctx::ctx`、`super::string::text`、`super::mortal` を足す。

```rust
fn s<M: Mode>(ctx: &mut crate::Ctx<M>, x: &str) -> Word {
    unsafe { ctx.heap_mut() }.alloc_string(x)
}

const TEXTS: [&str; 10] = ["", "a", "ab", "b", "é", "日本", "a\u{0}", "\u{7f}", "\u{80}", "ab\u{0}"];

fn strings_compare_by_bytes<M: Mode>() {
    let mut ctx = ctx::<M>();
    for x in TEXTS {
        for y in TEXTS {
            unsafe {
                let (a, b) = (s(&mut ctx, x), s(&mut ctx, y));
                assert_eq!(ctx.string_eq(a, b), Ok((x == y) as i64));
                let (a, b) = (s(&mut ctx, x), s(&mut ctx, y));
                assert_eq!(ctx.string_ne(a, b), Ok((x != y) as i64));
                let (a, b) = (s(&mut ctx, x), s(&mut ctx, y));
                assert_eq!(ctx.string_compare(a, b), Ok(ordering(x.as_bytes().cmp(y.as_bytes()))));
            }
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 呼ぶ側は同じ文字列の参照を2つ持って渡す。両方を手放しても正しく動く。
fn the_same_string_on_both_sides<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = s(&mut ctx, "abc");
        ctx.heap_mut().dup(a).unwrap();
        assert_eq!(ctx.string_eq(a, a), Ok(TRUE));
        let a = s(&mut ctx, "abc");
        ctx.heap_mut().dup(a).unwrap();
        assert_eq!(ctx.string_compare(a, a), Ok(EQ));
        let a = s(&mut ctx, "abc");
        ctx.heap_mut().dup(a).unwrap();
        let w = ctx.string_append(a, a).unwrap();
        assert_eq!(text(ctx.heap(), w), "abcabc");
        ctx.heap_mut().decref(w).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 写すバイトを数える。その場で足せば右の長さ、作り直せば両方の長さ (今のインタプリタと同じ)。
fn appending_counts_the_bytes_it_copies<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = s(&mut ctx, "ab");
        let b = s(&mut ctx, "cde");
        let before = ctx.stats().string_bytes_copied;
        let w = ctx.string_append(a, b).unwrap();
        let in_place = w == a;
        assert_eq!(text(ctx.heap(), w), "abcde");
        if M::CHECKED {
            assert_eq!(ctx.stats().string_bytes_copied - before, if in_place { 3 } else { 5 });
        }
        // 共有した左は作り直す
        ctx.heap_mut().dup(w).unwrap();
        let c = s(&mut ctx, "fg");
        let before = ctx.stats().string_bytes_copied;
        let x = ctx.string_append(w, c).unwrap();
        assert_ne!(x, w);
        assert_eq!(text(ctx.heap(), x), "abcdefg");
        if M::CHECKED {
            assert_eq!(ctx.stats().string_bytes_copied - before, 7);
        }
        // 不死の左も作り直す
        let k = ctx.heap_mut().alloc_string_immortal("k");
        let d = s(&mut ctx, "z");
        let y = ctx.string_append(k, d).unwrap();
        assert_eq!(text(ctx.heap(), y), "kz");
        for v in [w, x, y] {
            ctx.heap_mut().decref(v).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn ints_show_in_decimal<M: Mode>() {
    let mut ctx = ctx::<M>();
    for v in [0, 7, -7, 10, -10, 1_234_567_890, i64::MAX, i64::MIN, i64::MIN + 1] {
        let w = ctx.int_show(v);
        assert_eq!(text(ctx.heap(), w), v.to_string());
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// エスケープの規則は今のインタプリタの `show_string` と同じである。C1 の制御文字 (U+0080〜U+009F) はそのまま写す。
fn strings_show_with_the_interpreters_escapes<M: Mode>() {
    let cases = [
        ("", r#""""#),
        ("abc", r#""abc""#),
        ("a\"b", r#""a\"b""#),
        ("a\\b", r#""a\\b""#),
        ("\n\t\r\0", r#""\n\t\r\0""#),
        ("\u{1}\u{1f}\u{7f}", r#""\u{1}\u{1f}\u{7f}""#),
        ("\u{80}\u{85}\u{9f}é日\u{2028}", "\"\u{80}\u{85}\u{9f}é日\u{2028}\""),
        ("x\ny", r#""x\ny""#),
    ];
    let mut ctx = ctx::<M>();
    for (input, shown) in cases {
        unsafe {
            let a = s(&mut ctx, input);
            let w = ctx.string_show(a).unwrap();
            assert_eq!(text(ctx.heap(), w), shown);
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 文字列でない引数は検査モードで断る。断ったときはヒープを変えない。
fn a_string_extern_refuses_a_non_string<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        let d = ctx.heap_mut().alloc_data(0, 0, 1, false);
        let a = s(&mut ctx, "a");
        let live = mortal(ctx.heap());
        assert_eq!(ctx.string_eq(a, d), Err(Fault::Heap(HeapError::WrongKind)));
        assert_eq!(ctx.string_show(d), Err(Fault::Heap(HeapError::WrongKind)));
        assert_eq!(mortal(ctx.heap()), live);
        ctx.heap_mut().decref(a).unwrap();
        ctx.heap_mut().decref(d).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

both_modes!(
    strings_compare_by_bytes,
    the_same_string_on_both_sides,
    appending_counts_the_bytes_it_copies,
    ints_show_in_decimal,
    strings_show_with_the_interpreters_escapes,
    a_string_extern_refuses_a_non_string,
);
```

`super::ctx::ctx` は今ある補助で、Task 4 でテストの Host を渡す形に直す。

Run: `cargo test -p eml_rt externs::`
Expected: コンパイルの誤り (`string_eq` などがない)

- [ ] **Step 2: spec の関数と `CtxStats` の欄を足す**

- `heap/ctx.rs` に `mod strings;` を置く (`#[cfg(verus_only)] mod proof;` と同じ並びに。`strings` は実行するコードなので `cfg` を付けない)。`trust.rs` の `FILES` に `("heap/ctx/strings.rs", include_str!("heap/ctx/strings.rs"))` を足す
- `CtxStats` に `pub string_bytes_copied: u64` を足し、`Ctx::new` で 0 にする。欄の doc コメントに「検査モードだけで数える。数え方は今のインタプリタの `RunStats::string_bytes_copied` と同じで、S10 の VM が引き継ぐ」と書く
- `strings.rs` に spec の関数を置く
  - `lex(a, b)`: 先頭から比べて最初に違うバイトの大小、どちらかが尽きたら短い方が小さい。`LT`、`EQ`、`GT` を返す
  - `decimal(i)`: `i < 0` なら `seq![b'-'] + digits(|i|)`、そうでなければ `digits(i)`。`digits(n)` は `n < 10` なら1桁、そうでなければ `digits(n / 10) + seq![b'0' + n % 10]`
  - `escape(s)`: `seq![b'"'] + escape_body(s) + seq![b'"']`。`escape_body` はバイトごとに、`"` は `\"`、`\` は `\\`、`\n` は `\n`、`\t` は `\t`、`\r` は `\r`、0 は `\0`、0x01〜0x1F と 0x7F は `\u{` + 小文字の16進 (桁埋めなし) + `}`、それ以外はそのバイト

- [ ] **Step 3: 比較を作る**

- 文字列のバイト列は、`Heap` の T10 の `text(h, len, perm)` で `&str` として借り、`as_bytes()` で読む (`heap` の子の module なので使える)
- `string_eq`: 長さが違えば `FALSE`。同じなら slice の `==` (vstd に spec がある) で比べる。`string_ne` はその逆
- `string_compare`: `<[u8] as Ord>::cmp` で比べる。結果が `lex` に一致することは、`strings.rs` に置く `assume_specification` で約束する。これは T5 の新しい項目で、`trust.rs` の `TRUSTED` に `("heap/ctx/strings.rs: assume_specification<>[<[u8] as Ord>::cmp]", "T5")` の形で足す (項目の書き方は `trust.rs` のテストが出す名前に合わせる)
- どれも、検査モードでは両方が文字列であることと、両方を手放せること (同じ物体なら2回手放せること) を、状態を変える前に確かめる。読んだ後で `a`、`b` の順に `decref` する
- 約束は S9b-2b の基本操作の形にする。`requires old(self).wf()`、`word_in`、`!M::CHECKED ==>` 両方が文字列で `holds_args(seq![a, b])`。`ensures` は、`Heap(_)` の `Err` ならヒープが変わらないこと、`held_ok` と前提が成り立つとき `Ok(lex(..))` などの結果、`final.held_ok()`、`evolved_by(old, |b| -points(a, b) - points(b, b), Set::empty())`

- [ ] **Step 4: `++`、`Int.show`、`String.show` を作る**

- `string_append(a, b)`: 検査モードの確かめは `string_concat` に任せる (同じ物体の2つの参照も扱う)。長さを先に読み、結果が `a` と同じ語ならその場で足したので `b` の長さを、違えば両方の長さを `string_bytes_copied` に足す (検査モードだけ)。結果は `holds(w)` で、`text_of(w) == text_of(a) + text_of(b)`
- `int_show(i)`: 20 バイトの配列 `[u8; 20]` の後ろから、`i` の絶対値を `u64` で求めて (`i64::MIN` の絶対値は `i64` に収まらないため) 数字を書き、負なら `-` を足す。ASCII なら UTF-8 であることを証明して `str::from_utf8_unchecked` (vstd の約束) で `&str` にし、`alloc_string` で作る。`ensures text_of(w) == decimal(i as int)`。長さを `string_bytes_copied` に足す
- `string_show(s)`
  1. 検査モードで文字列であることと手放せることを確かめる
  2. 1回目の走査で、`escape(text)` の長さを数える (エスケープのないバイトは 1、名前付きは 2、`\u{…}` は 4 + 16進の桁数)
  3. `alloc_string_room(n)` で確保する
  4. 2回目の走査で、エスケープのない区間は `string_push_part(w, s, from, to)` でまとめて写し、エスケープは `string_push(w, "\\n")` のような短い定数で足す。区間の両端は ASCII のバイトの直前か直後か、文字列の端なので、文字の境目である。この補題 (正しい UTF-8 の列で、ASCII のバイトの直前と直後は文字の境目) を `strings.rs` に置く
  5. `s` を `decref` し、結果の長さを `string_bytes_copied` に足す
  - `ensures` は `text_of(w) == escape(old text_of(s))`。UTF-8 であることは `string_push` と `string_push_part` の約束から出る

- [ ] **Step 5: テストを通す**

Run: `cargo test -p eml_rt externs::` と `cargo verus verify -p eml_rt`
Expected: 通る

- [ ] **Step 6: 全体を確かめてコミットする**

Global Constraints の確かめる手順を全部流す (計測は不要)。

```bash
git add crates/eml_rt
git commit -m "Add the string externs, proven against bytes, decimal and escape specs"
```

---

### Task 4: Host、`println`、`flush`

**Files:**
- Create: `crates/eml_rt/src/host.rs`、`crates/eml_rt/src/heap/ctx/io.rs`、`crates/eml_rt/src/heap/tests/host.rs`
- Modify: `crates/eml_rt/src/heap/ctx.rs`、`crates/eml_rt/src/lib.rs`、`crates/eml_rt/src/trust.rs`、`crates/eml_rt/src/heap/tests/mod.rs`、`crates/eml_rt/src/heap/tests/ctx.rs`、`crates/eml_rt/src/heap/tests/trusted.rs`、`crates/eml_rt/src/heap/tests/control.rs` (`Ctx::new` を呼ぶ所)、`crates/eml_rt/examples/rt_bench.rs`
- Test: `crates/eml_rt/src/heap/tests/host.rs`、`crates/eml_rt/src/heap/tests/externs.rs`

**Interfaces:**
- Consumes: Task 2 の `Fault`
- Produces:
  - `verus!` の外 (`host.rs`):

    ```rust
    pub enum Stream { Out, Err }                       // verus! の中に置き、Verus にも見せる
    pub trait Host: Send + Sync {
        fn write_line(&self, stream: Stream, line: &str) -> io::Result<()>;
        fn flush(&self) -> io::Result<()>;
        fn open(&self, path: &str) -> io::Result<Opened>;
    }
    pub struct Opened { pub reader: Box<dyn BufRead + Send>, pub size: Option<u64> }
    pub enum Buffering { Line, Block }
    pub struct StdHost { /* out、err の Mutex<BufWriter<Box<dyn Write + Send>>>、mode、file_root */ }
    impl StdHost {
        pub fn stdio(file_root: PathBuf) -> StdHost;
        pub fn new(out: Box<dyn Write + Send>, err: Box<dyn Write + Send>, file_root: PathBuf, mode: Buffering) -> StdHost;
    }
    impl HostRef { pub fn new(host: Arc<dyn Host>) -> HostRef; }
    ```

  - `verus!` の中: `#[verifier::external_body] pub struct HostRef` (`Arc<dyn Host>` を `ManuallyDrop` の欄に持つ)、`Clone`
  - `Ctx::new(control: Arc<Control>, host: HostRef) -> Ctx<M>`
  - `pub unsafe fn println(&mut self, s: Word) -> Result<(), Fault>`
  - `pub fn flush(&mut self) -> Result<(), Fault>`
  - `lib.rs`: `pub use host::{Buffering, Host, HostRef, Opened, StdHost, Stream};`
  - テストの補助 (`heap/tests/host.rs`): `MemoryHost`、`MemoryFile`、`host()`

- [ ] **Step 1: テストの Host を書く**

`heap/tests/mod.rs` の module の一覧に `mod host;` を足し、`heap/tests/host.rs` を作る。Task 5 も使うので、ファイルの読み手の振る舞いもここに置く。

```rust
//! テストの Host。出力をメモリに受け、ファイルをメモリから読ませる。Miri でも動くように、OS に触れない。

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::{Host, HostRef, Opened, Stream};

/// メモリの上のファイル。読み手の振る舞い (1回に返すバイト数、途中の誤り、中断) を決められる。
#[derive(Clone, Default)]
pub(super) struct MemoryFile {
    pub(super) bytes: Vec<u8>,
    /// 読む前に分かる長さとして Host が返すか
    pub(super) sized: bool,
    /// 1回の読みで返す最大のバイト数。0 なら制限しない
    pub(super) chunk: usize,
    /// このバイト数を読んだ後の読みで誤りを返す
    pub(super) fail_after: Option<usize>,
    /// 最初の読みで `Interrupted` を1回返す
    pub(super) interrupt_once: bool,
    /// 読み手が落ちた回数
    pub(super) drops: Option<Arc<AtomicUsize>>,
}

impl MemoryFile {
    pub(super) fn text(s: &str) -> MemoryFile {
        MemoryFile { bytes: s.as_bytes().to_vec(), sized: true, ..MemoryFile::default() }
    }
}

struct Reader {
    file: MemoryFile,
    at: usize,
    interrupted: bool,
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.file.interrupt_once && !self.interrupted {
            self.interrupted = true;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if self.file.fail_after.is_some_and(|n| self.at >= n) {
            return Err(io::Error::other("the disk went away"));
        }
        let rest = &self.file.bytes[self.at..];
        let mut n = rest.len().min(buf.len());
        if self.file.chunk > 0 {
            n = n.min(self.file.chunk);
        }
        buf[..n].copy_from_slice(&rest[..n]);
        self.at += n;
        Ok(n)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        if let Some(drops) = &self.file.drops {
            drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
pub(super) struct MemoryHost {
    out: Mutex<Vec<u8>>,
    err: Mutex<Vec<u8>>,
    files: Mutex<HashMap<String, MemoryFile>>,
    /// 書き込みで返す誤りの文言
    fail_write: Option<&'static str>,
}

impl MemoryHost {
    pub(super) fn failing(message: &'static str) -> MemoryHost {
        MemoryHost { fail_write: Some(message), ..MemoryHost::default() }
    }
    pub(super) fn add(&self, path: &str, file: MemoryFile) {
        self.files.lock().unwrap().insert(path.to_string(), file);
    }
    pub(super) fn out(&self) -> String {
        String::from_utf8(self.out.lock().unwrap().clone()).unwrap()
    }
    pub(super) fn err(&self) -> String {
        String::from_utf8(self.err.lock().unwrap().clone()).unwrap()
    }
}

impl Host for MemoryHost {
    fn write_line(&self, stream: Stream, line: &str) -> io::Result<()> {
        if let Some(message) = self.fail_write {
            return Err(io::Error::other(message));
        }
        let mut target = match stream {
            Stream::Out => self.out.lock().unwrap(),
            Stream::Err => self.err.lock().unwrap(),
        };
        target.extend_from_slice(line.as_bytes());
        target.push(b'\n');
        Ok(())
    }
    fn flush(&self) -> io::Result<()> {
        Ok(())
    }
    fn open(&self, path: &str) -> io::Result<Opened> {
        let file = self.files.lock().unwrap().get(path).cloned().ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let size = file.sized.then_some(file.bytes.len() as u64);
        let reader: Box<dyn BufRead + Send> = Box::new(BufReader::new(Reader { file, at: 0, interrupted: false }));
        Ok(Opened { reader, size })
    }
}

/// テストの `Ctx` に渡す Host
pub(super) fn host(memory: &Arc<MemoryHost>) -> HostRef {
    HostRef::new(memory.clone())
}
```

`heap/tests/ctx.rs` の `ctx()` を、メモリの Host を渡す形に直す (期待値は変わらない。spec の「テストの変更」の機械的な直し)。

```rust
pub(super) fn ctx<M: Mode>() -> Ctx<M> {
    ctx_with(&Arc::new(MemoryHost::default()))
}

pub(super) fn ctx_with<M: Mode>(memory: &Arc<MemoryHost>) -> Ctx<M> {
    Ctx::new(Arc::new(Control::new(Limits::default())), host(memory))
}
```

ほかに `Ctx::new(…)` を呼んでいるテスト (`trusted.rs` の `every_primitive_once`、`heap/tests/control.rs` など) と `examples/rt_bench.rs` の `ctx()` も、Host を渡す形に直す。計測は `StdHost::new(Box::new(io::sink()), Box::new(io::sink()), PathBuf::new(), Buffering::Block)` を渡す。

- [ ] **Step 2: 失敗するテストを書く**

`heap/tests/externs.rs` に足し、`both_modes!` の一覧に名前を足す。

```rust
fn println_writes_a_line_and_releases_its_text<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let a = s(&mut ctx, "héllo");
        ctx.println(a).unwrap();
        let e = s(&mut ctx, "");
        ctx.println(e).unwrap();
    }
    ctx.flush().unwrap();
    assert_eq!(memory.out(), "héllo\n\n");
    assert_eq!(memory.err(), "");
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 書けなくても文字列は手放す (今の実装と同じ)。
fn a_failed_write_still_releases_the_text<M: Mode>() {
    let memory = Arc::new(MemoryHost::failing("disk full"));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let a = s(&mut ctx, "x");
        assert_eq!(ctx.println(a), Err(Fault::Output("disk full".into())));
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn println_refuses_a_non_string<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let d = ctx.heap_mut().alloc_data(0, 0, 1, false);
        assert_eq!(ctx.println(d), Err(Fault::Heap(HeapError::WrongKind)));
        assert_eq!(memory.out(), "");
        ctx.heap_mut().decref(d).unwrap();
    }
}
```

`heap/tests/host.rs` の終わりに、`StdHost` のテストを足す (ふつうの `#[test]`。`trusted::` の外なので Miri では流れない)。

```rust
mod std_host {
    use std::io::{self, Write};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use crate::{Buffering, Host, StdHost, Stream};

    /// 書いた中身を外から見られる書き込み先
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>, Arc<Mutex<bool>>);

    impl Shared {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut panic_next = self.1.lock().unwrap();
            if *panic_next {
                *panic_next = false;
                panic!("a writer that panics once");
            }
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn std_host(mode: Buffering) -> (StdHost, Shared, Shared) {
        let (out, err) = (Shared::default(), Shared::default());
        let host = StdHost::new(Box::new(out.clone()), Box::new(err.clone()), PathBuf::new(), mode);
        (host, out, err)
    }

    #[test]
    fn a_line_host_writes_each_line_at_once() {
        let (host, out, err) = std_host(Buffering::Line);
        host.write_line(Stream::Out, "a").unwrap();
        assert_eq!(out.text(), "a\n");
        host.write_line(Stream::Err, "e").unwrap();
        assert_eq!(err.text(), "e\n");
    }

    #[test]
    fn a_block_host_writes_on_flush() {
        let (host, out, _) = std_host(Buffering::Block);
        host.write_line(Stream::Out, "a").unwrap();
        assert_eq!(out.text(), "");
        host.flush().unwrap();
        assert_eq!(out.text(), "a\n");
    }

    /// 明示の flush なしで落としても、まだ書いていない出力は書かれる (unwind のときも同じ)。
    #[test]
    fn a_dropped_block_host_writes_what_it_holds() {
        let (host, out, _) = std_host(Buffering::Block);
        host.write_line(Stream::Out, "kept").unwrap();
        drop(host);
        assert_eq!(out.text(), "kept\n");
    }

    /// 標準エラーは、`Block` でも行ごとに書く。
    #[test]
    fn the_error_stream_is_written_by_line() {
        let (host, _, err) = std_host(Buffering::Block);
        host.write_line(Stream::Err, "e").unwrap();
        assert_eq!(err.text(), "e\n");
    }

    /// 書き込み先が panic してロックが毒されても、次の行は書ける。
    #[test]
    fn a_poisoned_lock_is_recovered() {
        let (host, out, _) = std_host(Buffering::Line);
        *out.1.lock().unwrap() = true;
        let panicked = catch_unwind(AssertUnwindSafe(|| host.write_line(Stream::Out, "lost")));
        assert!(panicked.is_err());
        host.write_line(Stream::Out, "b").unwrap();
        assert!(out.text().ends_with("b\n"));
    }

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eml_rt-host-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn files_open_under_the_root_or_by_absolute_path() {
        let root = fixture("open");
        std::fs::write(root.join("a.txt"), "abc").unwrap();
        std::fs::create_dir_all(root.join("dir")).unwrap();
        let host = StdHost::new(Box::new(io::sink()), Box::new(io::sink()), root.clone(), Buffering::Line);
        let mut opened = host.open("a.txt").unwrap();
        assert_eq!(opened.size, Some(3));
        let mut text = String::new();
        opened.reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "abc");
        let absolute = root.join("a.txt");
        assert!(host.open(absolute.to_str().unwrap()).is_ok());
        assert_eq!(host.open("missing.txt").err().unwrap().kind(), io::ErrorKind::NotFound);
        // ディレクトリも開ける (読むときに誤りになる。今の実装と同じ)。長さの見込みは返さない
        assert_eq!(host.open("dir").unwrap().size, None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
```

`files_open_under_the_root_or_by_absolute_path` は `read_to_string` に `std::io::Read` を使うので、`use std::io::Read;` を足す。ディレクトリを開けるのは Unix の振る舞いなので、そのテストの行を `#[cfg(unix)]` の別のテストに分けてよい。

Run: `cargo test -p eml_rt externs:: host::`
Expected: コンパイルの誤り (`Host`、`HostRef`、`println` などがない)

- [ ] **Step 3: `host.rs` を作る**

- `verus!` の中に置くもの
  - `pub enum Stream { Out, Err }`
  - `#[verifier::external_body] pub struct HostRef { host: ManuallyDrop<Arc<dyn Host>> }` と、`external_body` の `Clone`
  - Host を呼ぶ関数。どれも `#[verifier::external_body]` で何も約束しない (T11)
    - `pub(crate) fn host_write_line(host: &HostRef, stream: Stream, line: &str) -> Result<(), Box<str>>` (誤りは `io::Error` の文言)
    - `pub(crate) fn host_flush(host: &HostRef) -> Result<(), Box<str>>`
- `verus!` の外に置くもの
  - `Host`、`Opened`、`Buffering`、`StdHost` と、`HostRef::new`
  - `HostRef` の `Drop`: `#[inline(never)]` の関数に `ManuallyDrop::take` した `Arc` を渡して手放す。`Ctx` を使うコードのコード生成単位の分け方を変えないための予防で、`Control` と凍結した領域の鎖と同じ形である (コメントに [計測の基準](../../implementation/benchmarks.md) の「S9b-2b (実行文脈)」を引く)。T4 の項目になる
  - `StdHost`
    - 欄: `out: Mutex<BufWriter<Box<dyn Write + Send>>>`、`err: Mutex<BufWriter<Box<dyn Write + Send>>>`、`line: bool`、`file_root: PathBuf`。`BufWriter` の容量は 64 KiB
    - `stdio(file_root)`: `std::io::stdout().is_terminal()` なら `Line`、そうでなければ `Block`。標準エラーは `std::io::stderr()`
    - `write_line`: ストリームのロックを `lock().unwrap_or_else(PoisonError::into_inner)` で取り、本文と `\n` を書く。`Line` のとき、または `Stream::Err` のときは、続けて `flush` する
    - `flush`: 両方のストリームを flush する
    - 落とすときの flush は `BufWriter` の `Drop` が行う (誤りは捨てる)。`StdHost` に手で `Drop` を書かない
    - `open(path)`: `file_root.join(path)` を開く (`path` が絶対パスなら `join` はそのパスになる)。`metadata` がふつうのファイルなら長さを `size` に、そうでなければ (`metadata` の誤りも) `None` にする。読み手は `BufReader::new(file)`
- `lib.rs` に `mod host;` と `pub use host::{Buffering, Host, HostRef, Opened, StdHost, Stream};` を足す。`trust.rs` の `FILES` に `host.rs` と `heap/ctx/io.rs` を足す
- `trust.rs`: 行の名前の集合に `T11` を足し、`TRUSTED` に `HostRef`、`HostRef` の `Clone`、`host_write_line`、`host_flush` を `T11` として、`HostRef` の `Drop` を `T4` として足す (項目の書き方は `trust.rs` のテストが出す名前に合わせる)

- [ ] **Step 4: `Ctx` に Host を持たせ、`println` と `flush` を作る**

- `Ctx` に欄 `host: HostRef` を足し、`Ctx::new(control: Arc<Control>, host: HostRef)` にする。`Ctx` の doc コメントに「Host はセッションに1つで、`par` の子の `Ctx` は同じ Host を写して持つ」と書く
- `heap/ctx.rs` に `mod io;` を置く
- `io.rs`
  - `println(s)`: 検査モードでは、文字列であることと手放せることを確かめる (`Heap(_)` の誤りなら何も書かずに返す)。T10 の `text` で `&str` を借りて `host_write_line(&self.host, Stream::Out, text)` を呼び、借用を終えてから `s` を `decref` する。書けなければ `Fault::Output(文言)` を返す (このときも `s` は手放している)
  - `flush()`: `host_flush(&self.host)` を呼び、誤りは `Fault::Output(文言)` にする
  - 約束: `println` は成功のときも `Output` のときも `evolved_by(old, |b| -points(s, b), Set::empty())`、`Heap(_)` のときはヒープが変わらない。`flush` はヒープに触れない

- [ ] **Step 5: Miri のテストを足す**

`heap/tests/trusted.rs` に `// T11` の節を作り、ファイルの終わりの `both_modes!` に名前を足す。

```rust
// T11

/// Host を通る書き込み。メモリの Host で、`println` と `flush` を通す。
fn println_goes_through_the_host<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let a = ctx.heap_mut().alloc_string("日本");
        ctx.println(a).unwrap();
    }
    ctx.flush().unwrap();
    assert_eq!(memory.out(), "日本\n");
    assert_eq!(mortal(ctx.heap()), 0);
}
```

`trusted.rs` の `use` に `super::host::MemoryHost` を足す。

- [ ] **Step 6: 全体を確かめてコミットする**

Global Constraints の確かめる手順を全部流す。`rt_bench` の `ctx()` を変えたので、`bench/rt.sh` を2回流し、今ある14の計測が「S9b-2b (実行文脈)」の値から +0.5% を超えて増えないことを確かめる。

```bash
git add crates/eml_rt
git commit -m "Hold the session Host in Ctx and add println with stdio-style buffering"
```

---

### Task 5: `File` と `Fs.open`、`Fs.read_all`、`Fs.close`

**Files:**
- Modify: `crates/eml_rt/src/host.rs`、`crates/eml_rt/src/heap/ctx/io.rs`、`crates/eml_rt/src/trust.rs`
- Test: `crates/eml_rt/src/heap/tests/externs.rs`、`crates/eml_rt/src/heap/tests/trusted.rs`

**Interfaces:**
- Consumes: Task 1 の `foreign_unique`。Task 2 の `Fault`、`IoOp`、`IoReason`、`TUPLE`。Task 4 の `HostRef`、`Opened`、テストの `MemoryHost`、`MemoryFile`
- Produces:
  - `#[verifier::external_body] pub(crate) struct File { path: Box<str>, reader: Box<dyn BufRead + Send>, size: Option<u64> }`
  - `pub unsafe fn fs_open(&mut self, path: Word) -> Result<Word, Fault>`
  - `pub unsafe fn fs_read_all(&mut self, file: Word) -> Result<Word, Fault>` (結果は組 `(File, String)`)
  - `pub unsafe fn fs_close(&mut self, file: Word) -> Result<(), Fault>`

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests/externs.rs` に足し、`both_modes!` の一覧に名前を足す。

```rust
/// 開いて、全部読み、組から取り出す。組は `lin` ビットを持ち、File の参照を持つ。
unsafe fn open_and_read<M: Mode>(ctx: &mut crate::Ctx<M>, path: &str) -> Result<(Word, String), Fault> {
    unsafe {
        let p = s(ctx, path);
        let f = ctx.fs_open(p)?;
        let t = ctx.fs_read_all(f)?;
        assert!(ctx.heap().meta(t).unwrap().lin());
        let file = ctx.heap().field(t, 0).unwrap();
        let text_word = ctx.heap().field(t, 1).unwrap();
        let read = text(ctx.heap(), text_word);
        ctx.heap_mut().release(t, crate::externs::TUPLE, &[true, true]).unwrap();
        ctx.heap_mut().decref(text_word).unwrap();
        Ok((file, read))
    }
}

fn a_file_is_read_whole_and_closed<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let drops = Arc::new(AtomicUsize::new(0));
    memory.add("a.txt", MemoryFile { drops: Some(drops.clone()), ..MemoryFile::text("line 1\nline 2\n") });
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let (file, read) = open_and_read(&mut ctx, "a.txt").unwrap();
        assert_eq!(read, "line 1\nline 2\n");
        // 2回目は終わりから読むので空である
        let again = ctx.fs_read_all(file).unwrap();
        let second = ctx.heap().field(again, 1).unwrap();
        assert_eq!(text(ctx.heap(), second), "");
        let file = ctx.heap().field(again, 0).unwrap();
        ctx.heap_mut().release(again, crate::externs::TUPLE, &[true, true]).unwrap();
        ctx.heap_mut().decref(second).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        ctx.fs_close(file).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn an_empty_file_reads_as_an_empty_string<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("empty.txt", MemoryFile::text(""));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let (file, read) = open_and_read(&mut ctx, "empty.txt").unwrap();
        assert_eq!(read, "");
        ctx.fs_close(file).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 長さの見込みがなく、短い読みしか返さない読み手でも、64 KiB の区切りをまたいで全部読む。
fn a_long_unsized_file_is_read_in_chunks<M: Mode>() {
    let body: String = "abcdefg\n".repeat(20_000);
    let memory = Arc::new(MemoryHost::default());
    memory.add("big.txt", MemoryFile { sized: false, chunk: 1000, ..MemoryFile::text(&body) });
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let (file, read) = open_and_read(&mut ctx, "big.txt").unwrap();
        assert_eq!(read, body);
        ctx.fs_close(file).unwrap();
    }
    if M::CHECKED {
        assert_eq!(ctx.stats().string_bytes_copied, body.len() as u64);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// シグナルで中断された読みは、誤りにせず読み直す。
fn an_interrupted_read_is_retried<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("a.txt", MemoryFile { interrupt_once: true, ..MemoryFile::text("abc") });
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let (file, read) = open_and_read(&mut ctx, "a.txt").unwrap();
        assert_eq!(read, "abc");
        ctx.fs_close(file).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn a_missing_file_is_a_fault_naming_its_path<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let p = s(&mut ctx, "missing.txt");
        assert_eq!(
            ctx.fs_open(p),
            Err(Fault::Io { op: IoOp::Open, path: "missing.txt".into(), reason: IoReason::NotFound })
        );
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 読みの誤りと UTF-8 でない中身は、File を手放して閉じる。作りかけの文字列も残らない。
fn a_failed_read_closes_the_file<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let drops = Arc::new(AtomicUsize::new(0));
    memory.add("broken.txt", MemoryFile { fail_after: Some(3), drops: Some(drops.clone()), ..MemoryFile::text("abcdef") });
    memory.add(
        "bad.bin",
        MemoryFile { bytes: vec![b'a', 0xff, 0xfe], sized: true, drops: Some(drops.clone()), ..MemoryFile::default() },
    );
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let p = s(&mut ctx, "broken.txt");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(
            ctx.fs_read_all(f),
            Err(Fault::Io { op: IoOp::Read, path: "broken.txt".into(), reason: IoReason::Other })
        );
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        let p = s(&mut ctx, "bad.bin");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(ctx.fs_read_all(f), Err(Fault::NotUtf8 { path: "bad.bin".into() }));
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 読む間に止める語を見る。割り込まれていれば止め、File を手放す。
fn a_read_stops_on_an_interrupt<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("a.txt", MemoryFile::text("abc"));
    let control = Arc::new(Control::new(Limits::default()));
    let mut ctx = crate::Ctx::<M>::new(control.clone(), host(&memory));
    unsafe {
        let p = s(&mut ctx, "a.txt");
        let f = ctx.fs_open(p).unwrap();
        control.interrupt();
        assert_eq!(ctx.fs_read_all(f), Err(Fault::Stopped(Stop::Interrupted)));
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// ヒープの外の読みの領域も、確保量の上限に数える。
fn a_read_past_the_memory_limit_stops<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("big.txt", MemoryFile::text(&"x".repeat(200_000)));
    let control = Arc::new(Control::new(Limits { max_bytes: 150_000, ..Limits::default() }));
    let mut ctx = crate::Ctx::<M>::new(control, host(&memory));
    unsafe {
        let p = s(&mut ctx, "big.txt");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(ctx.fs_read_all(f), Err(Fault::Stopped(Stop::MemoryLimit)));
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// File でない物体と、参照が2つある File は読めない。断ったときはヒープを変えない。
fn read_all_refuses_a_non_file_or_a_shared_file<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("a.txt", MemoryFile::text("abc"));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let other = ctx.heap_mut().alloc_foreign(0u64, false);
        assert_eq!(ctx.fs_read_all(other), Err(Fault::Heap(HeapError::WrongKind)));
        assert_eq!(ctx.fs_close(other), Err(Fault::Heap(HeapError::WrongKind)));
        ctx.heap_mut().decref(other).unwrap();
        if M::CHECKED {
            let p = s(&mut ctx, "a.txt");
            let f = ctx.fs_open(p).unwrap();
            super::raw::set_rc(ctx.heap_mut(), f, 2);
            assert_eq!(ctx.fs_read_all(f), Err(Fault::Heap(HeapError::NotUnique)));
            super::raw::set_rc(ctx.heap_mut(), f, 1);
            ctx.fs_close(f).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}
```

- `File` は `lin` の物体なので `dup` できない。参照が2つある File は `raw` の補助で rc を書いて作る (前提を崩すテストなので raw で書く)。`heap/tests/raw.rs` に rc を書く補助がなければ、今ある `raw::rc` の隣に `pub(super) unsafe fn set_rc<M: Mode>(heap: &mut Heap<M>, w: Word, rc: i32)` を足す
- `a_long_unsized_file_is_read_in_chunks` の `string_bytes_copied` は、`read_all` が数えるファイルの長さだけを見る (パスの文字列は `alloc_string` で作るので数えない)
- `heap/tests/externs.rs` の先頭の `use` に `std::sync::atomic::{AtomicUsize, Ordering}`、`std::sync::Arc`、`crate::{Control, Limits, Stop}`、`super::host::{MemoryFile, MemoryHost, host}`、`super::ctx::ctx_with` を足す

Run: `cargo test -p eml_rt externs::`
Expected: コンパイルの誤り (`fs_open` などがない)

- [ ] **Step 2: `File` と読む関数を作る**

`host.rs` の `verus!` の中に置く。どれも `#[verifier::external_body]` で、何も約束しない (T11)。

- `pub(crate) struct File { path: Box<str>, reader: Box<dyn BufRead + Send>, size: Option<u64> }`
- `pub(crate) fn host_open(host: &HostRef, path: &str) -> Result<File, (Box<str>, IoReason)>`: パスを1回だけ写して `File` に持たせる。誤りなら写したパスと理由を返す
- `pub(crate) unsafe fn read_text(file: *mut File, control: &Control) -> Result<String, ReadError>`
  - `pub(crate) enum ReadError { Io(Box<str>, IoReason), NotUtf8(Box<str>), Stopped(Stop) }` (`Box<str>` は File のパス)
  - `size` を `Option::take` で取り出し、あれば `Vec::try_reserve` する。失敗は `Io(path, Other)` (今の `read_to_end` と同じ)
  - ループ: 止める語 (`control.stop_bits()`) が立っていれば `Stopped` (`INTERRUPT` なら `Interrupted`、`MEMORY` なら `MemoryLimit`)。`control.bytes()` と `Vec` の容量の和が `control.limits().max_bytes` を超えれば `Stopped(MemoryLimit)`。`(&mut reader).take(64 KiB).read_to_end(&mut vec)` で読み、0 バイトなら終わる
  - 読みの誤りは `Io(path, 理由)`。理由は `io::Error` の種類を写す関数 (`NotFound`、`PermissionDenied`、それ以外は `Other`) で決める。`read_to_end` は `Interrupted` を読み直すので、`Interrupted` はここに届かない
  - 読み終えたら `String::from_utf8(vec)` で確かめる。誤りなら `NotUtf8(path)`
  - ポインタが生きた `File` を指し、ほかに参照がないことは、呼ぶ側が `foreign_unique` で確かめる (T3)
- `trust.rs` の `TRUSTED` に、`File`、`host_open`、`read_text`、`ReadError` (`external_body` にした場合) を `T11` で足す

- [ ] **Step 3: `fs_open`、`fs_read_all`、`fs_close` を作る**

`io.rs` に置く。

- `fs_open(path)`
  1. 検査モードでは、文字列であることと手放せることを確かめる
  2. T10 の `text` で `&str` を借りて `host_open(&self.host, text)` を呼び、借用を終えてから `path` を `decref` する
  3. 成功なら `self.heap.alloc_foreign(file, true)` を返す (`lin` ビットを立てる)。誤りなら `Fault::Io { op: Open, path, reason }`
- `fs_read_all(file)`
  1. `self.heap.foreign_unique::<File>(file)` で借りる。誤りは `Fault::Heap(e)` で、ヒープは変わらない
  2. `read_text(ptr, &self.control)` を呼ぶ
  3. 誤りなら `file` を `decref` し (破棄処理が閉じる)、`ReadError` を `Fault` に写して返す
  4. 成功なら `alloc_string(&text)` で文字列を作り、長さを `string_bytes_copied` に足す (検査モードだけ)。`alloc_data(TUPLE, 2, 0, true)` で組を作り、`set_field` で 0 番に `file`、1 番に文字列を入れる。`file` の参照は組に移るので `dup` しない
  - 約束: 成功なら結果を持ち (`holds(t)`)、`file` の参照は組に移る。`Heap(_)` ならヒープは変わらない。I/O の誤りと `Stopped` なら `file` を手放したほかは変わらない
- `fs_close(file)`
  1. `self.heap.foreign::<File>(file)` で型を確かめる (`WrongKind` はどちらのモードでも返る)。検査モードでは手放せることも確かめる
  2. `decref` する

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_rt externs::` と `cargo verus verify -p eml_rt`
Expected: 通る

- [ ] **Step 5: Miri のテストを足す**

`heap/tests/trusted.rs` の `// T11` の節に足し、`both_modes!` に名前を足す。`// T8` の `every_primitive_once` の終わりに、extern を1回ずつ通す呼び出しを足す。

```rust
/// Host を通る読み。区切りをまたぐ長さ、長さの見込みのない読み手、短い読み、途中の誤り、UTF-8 でない中身、
/// 止める語、閉じることを通す。
fn files_go_through_the_host<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let body = "0123456789".repeat(7000);
    memory.add("big.txt", MemoryFile { sized: false, chunk: 4096, ..MemoryFile::text(&body) });
    memory.add("broken.txt", MemoryFile { fail_after: Some(1), ..MemoryFile::text("ab") });
    memory.add("bad.bin", MemoryFile { bytes: vec![0xc3], sized: true, ..MemoryFile::default() });
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let p = ctx.heap_mut().alloc_string("big.txt");
        let f = ctx.fs_open(p).unwrap();
        let t = ctx.fs_read_all(f).unwrap();
        let w = ctx.heap().field(t, 1).unwrap();
        assert_eq!(text(ctx.heap(), w), body);
        ctx.heap_mut().decref(t).unwrap();
        for name in ["broken.txt", "bad.bin"] {
            let p = ctx.heap_mut().alloc_string(name);
            let f = ctx.fs_open(p).unwrap();
            assert!(ctx.fs_read_all(f).is_err());
        }
        let p = ctx.heap_mut().alloc_string("big.txt");
        let f = ctx.fs_open(p).unwrap();
        ctx.fs_close(f).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}
```

`every_primitive_once` には次を足す (Host はテストの `MemoryHost` にし、`ctx_tests::ctx_with` で作る形に変える)。

```rust
    // extern を1回ずつ
    let a = ctx.heap_mut().alloc_string("a");
    let b = ctx.heap_mut().alloc_string("b");
    assert_eq!(ctx.string_compare(a, b), Ok(crate::externs::LT));
    let a = ctx.heap_mut().alloc_string("a\n");
    let shown = ctx.string_show(a).unwrap();
    let n = ctx.int_show(-12);
    let joined = ctx.string_append(shown, n).unwrap();
    ctx.println(joined).unwrap();
    ctx.flush().unwrap();
    assert_eq!(memory.out(), "\"a\\n\"-12\n");
```

- [ ] **Step 6: 全体を確かめてコミットする**

Global Constraints の確かめる手順を全部流す (計測は不要)。

```bash
git add crates/eml_rt
git commit -m "Add File and the Fs externs, reading in chunks that watch the Control"
```

---

### Task 6: 期待値の表と extern の計測

**Files:**
- Create: `crates/eml_rt/src/heap/tests/table.rs`、`crates/eml_rt/src/heap/tests/expected.txt`、`crates/eml_interp/tests/rt_externs.rs`、`crates/eml_rt/examples/rt_extern_bench.rs`
- Modify: `crates/eml_rt/src/heap/tests/mod.rs`、`crates/eml_rt/src/heap/tests/externs.rs`、`crates/eml_interp/Cargo.toml`、`crates/eml_interp/tests/main.rs`、`bench/rt.sh`

**Interfaces:**
- Consumes: Task 2〜5 のすべての extern、テストの `MemoryHost`。`eml_interp::run`、`RunConfig::with_file_root`、`OutputSink::capture`、`eml_lir::parse`
- Produces: 表の形 (`table.rs`)。`eml_interp` のテストも `#[path = "../../eml_rt/src/heap/tests/table.rs"] mod table;` で同じファイルを読む。`table.rs` は `std` だけを使う

**表の形**

1行が1つの場合で、欄はタブで区切る。`#` で始まる行と空の行は読み飛ばす。

```
<extern の行の名前>\t<引数>…\t=> <結果>
```

- 引数: `i:<10進>` (`int`)、`b:True` か `b:False` (`Bool`)、`s:<Rust の {:?} の文字列>` (定数の文字列。今の実装では `const`、`eml_rt` では `alloc_string_immortal`)、`u:<文字列>` (一意な文字列。今の実装では定数と `""` の `++`、`eml_rt` では `alloc_string`)、`d:<文字列>` (共有した文字列。一意な文字列を `dup` して渡し、残った参照は後で手放す)、`f:<文字列>` (`file_root` からの相対パス)、`a:<文字列>` (同じ名前を `file_root` につないだ絶対パス)
- 結果: `i:<10進>`、`b:True`/`b:False`、`o:LT`/`o:EQ`/`o:GT`、`s:<文字列>`、`out:<文字列>` (`println` が書いた出力)、`err:<文言>` (位置を除いた誤りの文言)
- `Std.Fs.read_all` の場合は、開いて (`Std.Fs.open`)、読み、閉じる (`Std.Fs.close`) までを1つにし、結果は読んだ文字列である。`Std.Fs.read_all2` の場合は2回読み、2回目の結果を書く
- ファイルの用意 (`table.rs` の `fixtures(root)` が作る。両方のテストが同じものを使う)
  - `plain.txt`: `"line 1\nline 2\n"`、`empty.txt`: `""`、`bad.bin`: `[b'a', 0xff, 0xfe]`、`big.txt`: `"abcdefg\n"` を 131072 回 (1 MiB)、`dir/`: ディレクトリ、`locked.txt`: `"secret"` を mode 000 で (Unix で root でないときだけ。root のときは、この名前の場合を飛ばす)
  - `missing.txt` と NUL を含むパス `"a\u{0}b"` は作らない

- [ ] **Step 1: `table.rs` を書く**

`table.rs` には次を置く。

```rust
//! 期待値の表の形。`eml_rt` のテストと `eml_interp` のテスト (`#[path]` で読む) が同じ表を使う。`std` だけを使う。

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg { Int(i64), Bool(bool), Const(String), Unique(String), Shared(String), Rel(String), Abs(String) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome { Int(i64), Bool(bool), Order(&'static str), Str(String), Out(String), Err(String) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case { pub name: String, pub args: Vec<Arg>, pub outcome: Outcome }

pub const TABLE: &str = include_str!("expected.txt");

/// 表を読む。形の誤りは、どの行かを添えて panic する
pub fn cases() -> Vec<Case> { … }

/// 1つの場合を表の1行に書く (表を作るときに使う)
pub fn line(case: &Case) -> String { … }

/// Rust の `{:?}` で書いた文字列を戻す。`\"`、`\\`、`\'`、`\n`、`\t`、`\r`、`\0`、`\u{…}` を読む
pub fn unquote(text: &str) -> String { … }

/// 表を作るための入力。結果の欄は空で、`eml_interp` の側が埋める
pub fn inputs() -> Vec<(String, Vec<Arg>)> { … }

/// ファイルを用意する。`locked.txt` を作れなかった (root で動いている) ら false を返す
pub fn fixtures(root: &Path) -> bool { … }
```

`inputs()` が並べる場合は次のとおりである (順は固定する)。

- 整数の2引数の行 (`Prelude.+`、`Prelude.-`、`Prelude.*`、`Prelude./`、`Prelude.%`、`Prelude.Eq Int.==`、`Prelude.Eq Int.!=`、`Prelude.Ord Int.compare`、`Prelude.Ord Int.<`、`Prelude.Ord Int.<=`、`Prelude.Ord Int.>`、`Prelude.Ord Int.>=`): `EDGES` (Task 2 と同じ 13 個) のすべての組と、種を固定した xorshift64 (種 `0x9E37_79B9_7F4A_7C15`) の 50 組
- `Prelude.negate`、`Prelude.Show Int.show`: `EDGES` と同じ乱数の 50 個
- `Prelude.Eq Bool.==`、`Prelude.Eq Bool.!=`: 4 つの組
- 文字列の2引数の行 (`Prelude.Eq String.==`、`Prelude.Eq String.!=`、`Prelude.Ord String.compare`、`Prelude.++`): 次の `STRINGS` のすべての組 (定数の引数)
  - `""`、`"a"`、`"ab"`、`"b"`、`"é"`、`"日本"`、`"😀"`、`"a\u{0}"`、`"\u{7f}"`、`"\u{80}"`、`"\u{85}"`、`"\u{9f}"`、`"\u{2028}"`、`"\"q\""`、`"back\\slash"`、`"tab\tnl\n"`、`"\u{1}\u{1f}"`
- `Prelude.++` の左の種類: `u:"ab"` と `s:"cd"`、`d:"ab"` と `s:"cd"`、`s:"ab"` と `u:"cd"`
- `Prelude.Show String.show`、`Prelude.println`: `STRINGS` と、種を固定した乱数の文字列 30 個 (`STRINGS` の文字を 1〜8 個つないだもの)
- ファイル: `Std.Fs.read_all` で `f:"plain.txt"`、`f:"empty.txt"`、`f:"bad.bin"`、`f:"big.txt"`、`f:"dir"`、`f:"locked.txt"`、`f:"missing.txt"`、`f:"a\u{0}b"`、`a:"plain.txt"`。`Std.Fs.read_all2` で `f:"plain.txt"`

`EDGES` と `STRINGS` は `table.rs` の `pub const` に置き、Task 2 と Task 3 のテストもそれを使うように直してよい。

- [ ] **Step 2: `eml_interp` の側のテストを書き、表を作る**

`crates/eml_interp/Cargo.toml` の `[dev-dependencies]` に `eml_rt.workspace = true` を足す (ワークスペースの `Cargo.toml` の `[workspace.dependencies]` に `eml_rt` がなければ足す)。`tests/main.rs` に `mod rt_externs;` を足す。

`rt_externs.rs` は、場合ごとに LIR のプログラムを作って `eml_interp::run` で動かし、結果を表の結果の形に戻す。

```rust
//! 今の実装 (`eml_interp` と `eml_runtime`) が、`eml_rt` の extern の期待値の表と同じ結果になることを確かめる。
//! S10 で CEK を消すとき、このテストも消す (docs/implementation/testing.md の「期待値の表」)。

#[path = "../../eml_rt/src/heap/tests/table.rs"]
mod table;

use std::path::{Path, PathBuf};

use eml_interp::{RunConfig, RuntimeError, run};
use eml_runtime::OutputSink;
use table::{Arg, Case, Outcome};

/// 変数の番号を振りながら、プログラムの本体の行を積む
struct Body { lines: Vec<String>, next: u32 }

impl Body {
    fn var(&mut self, name: &str) -> String {
        self.next += 1;
        format!("{name}.{}", self.next)
    }
    fn push(&mut self, line: String) {
        self.lines.push(format!("  {line}"));
    }
    /// 引数を1つ用意し、渡すアトムを返す。負の整数と `i64::MIN` は引き算で作る (テキストの形に依らないため)
    fn arg(&mut self, arg: &Arg, root: &Path, release: &mut Vec<String>) -> String {
        match arg {
            Arg::Int(v) if *v >= 0 => v.to_string(),
            Arg::Int(v) => {
                let x = self.var("n");
                let magnitude = v.unsigned_abs();
                if magnitude > i64::MAX as u64 {
                    let y = self.var("n");
                    self.push(format!("let {y}: int = extern Prelude.-(0, {})", i64::MAX));
                    self.push(format!("let {x}: int = extern Prelude.-({y}, 1)"));
                } else {
                    self.push(format!("let {x}: int = extern Prelude.-(0, {magnitude})"));
                }
                x
            }
            Arg::Bool(b) => if *b { "#1".into() } else { "#0".into() },
            Arg::Const(s) => {
                let x = self.var("c");
                self.push(format!("let {x}: obj = const {s:?}"));
                x
            }
            Arg::Unique(s) => {
                let (c, e, x) = (self.var("c"), self.var("e"), self.var("u"));
                self.push(format!("let {c}: obj = const {s:?}"));
                self.push(format!("let {e}: obj = const \"\""));
                self.push(format!("let {x}: obj = extern Prelude.++({c}, {e})"));
                x
            }
            Arg::Shared(s) => {
                let x = self.arg(&Arg::Unique(s.clone()), root, release);
                self.push(format!("dup {x}"));
                release.push(x.clone());
                x
            }
            Arg::Rel(p) => self.arg(&Arg::Const(p.clone()), root, release),
            Arg::Abs(p) => self.arg(&Arg::Const(root.join(p).to_str().unwrap().to_string()), root, release),
        }
    }
}
```

結果の出し方 (どれも `println` で出し、`run` の出力を読み戻す)。

- `int` の結果: `let s.k: obj = extern "Prelude.Show Int.show"(r)` を `println` する。出力の数を `Outcome::Int` に読む
- `Bool` の結果: `switch r Prelude.Bool { #0 -> b1, #1 -> b2 }` で、`b1` は `const "False"`、`b2` は `const "True"` を `println` する。`Ordering` は `switch r Prelude.Ordering { #0 -> …, #1 -> …, #2 -> … }` で `LT`、`EQ`、`GT`
- 文字列の結果: そのまま `println` する。出力の終わりの `\n` を除いたものが結果である
- `Prelude.println`: 出力そのものを `Outcome::Out` にする
- `Std.Fs.read_all`: `let h: obj lin = extern Std.Fs.open(p)`、`let t: obj lin = extern Std.Fs.read_all(h)`、`unpack t (,) #0(f: obj lin, s: obj)`、`release t (,) #0(f, s)`、`extern Std.Fs.close(f)`、`extern Prelude.println(s)`。`read_all2` は `f` をもう一度読む。プログラムの先頭に `layout (,) { (,)(tobj, tobj) }` を置く
- `Shared` で残した参照は、結果を出した後で `decref` する
- 関数は `fn main() -> unit {` … `return ()` で、最後に `}` を置く。ほかの関数は作らない
- `run(&program, &RunConfig::default().with_file_root(root.to_path_buf()), &sink)` で動かす。`Err(RuntimeError::Fault { fault, .. })` なら `Outcome::Err(fault.to_string())` (位置を含まない文言)

extern の名前は、いつも `{name:?}` で引用して書く (`Prelude.+` のような記号を含む名前もあるため)。LIR のテキストの形 (`unpack` と `release` の書き方、`lin` の印の書き方、`switch` の配置の名前) は、`crates/eml_interp/tests/data.rs` の `UNPACK_TWICE` と `crates/eml_interp/tests/run.rs` の `Std.Fs.open` の例に合わせる。verifier が拒む形にしない (`run` の前に `eml_lir::verify` を通す)。

テストは2つ置く。

```rust
/// 表を作る。表を変えるときだけ、`EML_WRITE_TABLE=1 cargo test -p eml_interp rt_externs` で流す
#[test]
fn the_table_can_be_written_from_the_interpreter() {
    if std::env::var_os("EML_WRITE_TABLE").is_none() {
        return;
    }
    let root = scratch("write");
    table::fixtures(&root);
    let mut text = String::from("# eml_rt の extern の期待値の表。今の実装 (eml_interp) の出力から作った\n");
    for (name, args) in table::inputs() {
        let outcome = outcome(&name, &args, &root);
        text.push_str(&table::line(&Case { name, args, outcome }));
        text.push('\n');
    }
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join("../eml_rt/src/heap/tests/expected.txt"), text).unwrap();
}

/// 今の実装が、表のどの場合も同じ結果になる
#[test]
fn the_interpreter_matches_the_table() {
    let root = scratch("check");
    let locked = table::fixtures(&root);
    for case in table::cases() {
        if !locked && case.args.iter().any(|a| matches!(a, Arg::Rel(p) if p == "locked.txt")) {
            continue;
        }
        assert_eq!(outcome(&case.name, &case.args, &root), case.outcome, "{}", table::line(&case));
    }
}

/// `eml_lir` のタグと組の定数が、`eml_rt` の定数と同じである (S10 では VM の crate に移す)
#[test]
fn tags_agree_with_eml_lir() {
    assert_eq!(eml_lir::TRUE as i64, eml_rt::externs::TRUE);
    assert_eq!(eml_lir::FALSE as i64, eml_rt::externs::FALSE);
    assert_eq!(eml_lir::LT as i64, eml_rt::externs::LT);
    assert_eq!(eml_lir::EQ as i64, eml_rt::externs::EQ);
    assert_eq!(eml_lir::GT as i64, eml_rt::externs::GT);
    assert_eq!(eml_lir::TUPLE as u16, eml_rt::externs::TUPLE);
}
```

`scratch(name)` は `std::env::temp_dir().join(format!("eml_interp-rt-externs-{}-{name}", std::process::id()))` を作り直して返す関数である。`eml_lir` の定数の名前と型は `crates/eml_lir/src/lib.rs` の 644〜654 行に合わせる。

Run: `EML_WRITE_TABLE=1 cargo test -p eml_interp rt_externs` で表を作り、`cargo test -p eml_interp rt_externs`
Expected: 表が書かれ、`the_interpreter_matches_the_table` が通る。表の中身を目で見て、誤りの文言 (`integer overflow`、``cannot open `missing.txt`: not found``、``cannot read `dir`: I/O error``、``` `bad.bin` is not valid UTF-8 ``` など) が並んでいることを確かめる

- [ ] **Step 3: `eml_rt` の側のテストを書く**

`heap/tests/mod.rs` に `mod table;` を足す (`eml_rt` の側でも `pub` の項目が使われないと警告になるので、`#[allow(dead_code)]` を `mod table;` に付けてよい)。`heap/tests/externs.rs` に、表のどの場合も `eml_rt` の extern で同じ結果になることを確かめるテストを足す。

```rust
/// `eml_rt` の extern が、期待値の表のどの場合も同じ結果を返し、物体を残さない
fn the_externs_match_the_table<M: Mode>() {
    let root = std::env::temp_dir().join(format!("eml_rt-table-{}-{}", std::process::id(), M::CHECKED));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let locked = table::fixtures(&root);
    for case in table::cases() {
        if !locked && case.args.iter().any(|a| matches!(a, Arg::Rel(p) if p == "locked.txt")) {
            continue;
        }
        let out = Shared::default();
        let host = StdHost::new(Box::new(out.clone()), Box::new(io::sink()), root.clone(), Buffering::Line);
        let mut ctx = crate::Ctx::<M>::new(Arc::new(Control::new(Limits::default())), HostRef::new(Arc::new(host)));
        let got = unsafe { call(&mut ctx, &case, &root, &out) };
        assert_eq!(got, case.outcome, "{}", table::line(&case));
        assert_eq!(mortal(ctx.heap()), 0, "{}", table::line(&case));
    }
    std::fs::remove_dir_all(&root).ok();
}
```

- `call` は、行の名前で `match` して extern を呼び、結果を `Outcome` に戻す関数である。引数は `Arg::Int` を `i64`、`Arg::Bool` を `TRUE`/`FALSE`、`Arg::Const` を `alloc_string_immortal`、`Arg::Unique` を `alloc_string`、`Arg::Shared` を `alloc_string` と `dup` (残った参照は結果を読んだ後に `decref`)、`Arg::Rel` を定数の文字列、`Arg::Abs` を `root` につないだ絶対パスの定数の文字列にする。文字列の結果は読んでから `decref` し、`Fs` の場合は Task 5 の `open_and_read` と同じく組から取り出して閉じる
- `Shared` は Task 4 の `std_host` のテストの書き込み先と同じものを、`heap/tests/host.rs` の `pub(super)` に移して使う
- このテストは `StdHost` と実際のファイルを使うので、`trusted::` には置かない (Miri では流れない)

Run: `cargo test -p eml_rt externs::`
Expected: 通る

- [ ] **Step 4: 書き込みの誤りの文言をそろえる**

表に入れない書き込みの誤りを、両方の側で同じ文言にする。`rt_externs.rs` に足す。

```rust
/// 書けない出力先への `println` は、`eml_rt` の `Fault::Output` と同じ文言で止まる
#[test]
fn a_failed_write_reads_the_same_on_both_sides() {
    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let program = eml_lir::parse(
        "fn main() -> unit {\n  let s.1: obj = const \"x\"\n  let o.2: unit = extern Prelude.println(s.1)\n  return o.2\n}\n",
    )
    .unwrap();
    let Err(RuntimeError::Fault { fault, .. }) = run(&program, &RunConfig::default(), &OutputSink::new(Failing)) else {
        panic!("println to a failing writer did not fail");
    };
    assert_eq!(fault.to_string(), eml_rt::Fault::Output("disk full".into()).to_string());
}
```

- [ ] **Step 5: extern の計測を足す**

`crates/eml_rt/examples/rt_extern_bench.rs` を作る。形は `rt_bench.rs` と同じ (`main` で計測の名前を受け、`expect` の文言に名前を並べる)。どの計測も、1回あたり数ミリ秒から数十ミリ秒になる回数で回し、`black_box` で値を隠す。

| 計測 | 中身 | 回数 |
|---|---|---|
| `string-eq` | 64 KiB の等しい2つの文字列の `string_eq` (引数は毎回 `dup` して渡す) | 10,000 |
| `string-eq-early` | 先頭で違う 64 KiB の2つの文字列の `string_eq` | 10,000,000 |
| `compare` | 64 KiB で末尾だけ違う2つの文字列の `string_compare` | 10,000 |
| `concat-copy` | 共有した 1 KiB の左と 16 バイトの右の `string_append` と結果の `decref` | 1,000,000 |
| `show-int` | `int_show(7)` と結果の `decref` | 10,000,000 |
| `show-int-long` | `int_show(i64::MIN)` と結果の `decref` | 10,000,000 |
| `show-string` | エスケープのない 1 KiB の文字列の `string_show` と結果の `decref` | 100,000 |
| `show-string-dense` | 半分が `\n` の 1 KiB の文字列の `string_show` と結果の `decref` | 100,000 |
| `println-block` | `StdHost` (`Block`) で `/dev/null` (`std::fs::File::create("/dev/null")`) に 23 バイトの行を `println` | 1,000,000 |
| `println-line` | 同じことを `Line` で | 1,000,000 |
| `read-all` | メモリの Host で 1 MiB のファイルを開いて読んで閉じる (長さの見込みあり) | 100 |
| `read-all-unsized` | 同じことを長さの見込みなしで | 100 |

- メモリの Host は計測のプログラムの中に小さく書く (テストの `MemoryHost` は `pub(super)` で使えないため)
- `bench/rt.sh` に、`rt_extern_bench` をビルドして同じ表の形で流す2つ目のループを足す。2つ目の表の前に `echo` で見出しの行 (`extern の計測 (rt_extern_bench)`) を出す

Run: `bench/rt.sh` を2回
Expected: 今ある14の計測が「S9b-2b (実行文脈)」の値から +0.5% を超えて増えない。新しい計測の値は Task 7 で記録する

- [ ] **Step 6: 全体を確かめてコミットする**

Global Constraints の確かめる手順を全部流す。

```bash
git add crates/eml_rt crates/eml_interp bench/rt.sh Cargo.toml
git commit -m "Pin the externs to one expected-value table checked on both implementations"
```

---

### Task 7: 計測の記録と文書

**Files:**
- Modify: `docs/spec/object-model.md`、`docs/spec/lir.md`、`docs/future/roadmap.md`、`docs/future/multicore.md`、`docs/implementation/testing.md`、`docs/implementation/benchmarks.md`、`docs/README.md`、`CLAUDE.md` (`eml_rt` の段落)
- Modify: `crates/eml_rt/src` と `crates/eml_interp/tests/rt_externs.rs` のコメントのうち、新しい節を指すべき引用
- Delete: `docs/superpowers/specs/2026-10-15-s9b3-word-abi-externs-design.md`、`docs/superpowers/plans/2026-10-15-s9b3-word-abi-externs.md`

**Interfaces:**
- Consumes: Task 1〜6 の公開の操作と計測の値

- [ ] **Step 1: 計測を記録する**

Run: `bench/rt.sh` を2回。`benchmarks.md` の「S9b-2b (実行文脈)」の後に「S9b-3 (extern)」の節を足す。2つの表 (今ある14の計測と、extern の12の計測) と、今ある14の計測の S9b-2b との差を書く。extern の計測は S9b-3 の基準である。spec の計測の値 (行ごとに flush すると 1.30 µs/行と 324 ns/行、`Block` で 8 ns/行と 5 ns/行。`read_all` の4つの作り方の表) も、この節に記録として写す。`eml_rt` は `eml run` から使わないので `bench/run.sh` の表は測らないことも、前の節と同じ文で書く。

- [ ] **Step 2: 文書を直す**

spec の「文書」の節のとおりに直す。spec と違う形で作ったところは、コードに合わせて書く。

- `object-model.md`
  - 新しい節「extern」: 語の ABI (行ごとに引数の数が決まった関数、Repr ごとの型、`Ctx` を取らない算術と比較、`unsafe fn`)、`ArithFault` と `Fault` と文言、誤りのときの約束 (2段の表)、Host と `StdHost` と出力のバッファ (flush する所、書き込みの誤りの意味)、`File` の中身、`read_all` (区切りと `Control`)、仕事の回数 (`string_bytes_copied`)
  - 種類の表の `File` の行、「実行文脈 `Ctx`」の欄の表に `host`、「核の操作」に `alloc_string_room`、`string_push_part`、`foreign_unique` (書き込む操作)
  - 「まだ決めていないこと」の3か所 (`File` の中身、`lin` ビットを立てる実装の `File` の側、出力先) を閉じる
  - 「文字列の操作」の「`show`、比較、`println` は S9b-3 で作る」の文を、作ったものに合わせて直す
  - 信頼の一覧に T11 を足し、T3 (`foreign_unique`)、T4 (`HostRef` の `Drop`)、T5 (`<[u8] as Ord>::cmp`、作った場合は `String::from_utf8` の包み)、T7 (新しい unwind の場所と、その後に残る状態)、「`verus!` の外に置くもの」を広げる
- `lir.md`: 「実行時エラー」の節に、extern の実装は `eml_rt` にあることと、出力をまとめて書くときの書き込みの誤りの位置
- `roadmap.md`
  - S9b-3 と S9 全体を終えた状態にし、決めたことの移った先を書く
  - S10 の決めたことに、行の番号の `match` で `eml_rt` の extern を呼ぶこと、`Ctx::flush` を呼ぶ所、`Stopped` の扱い (安全点の `Stop` と同じ)、`eml_interp` の側の表のテストを CEK と一緒に消すこと、`Heap(_)` と `Internal` の文言の変更を UI テストの期待値の変更として扱うこと、タグの定数のテストを VM の crate に移すこと
  - S12 の論点に、閉包を呼び戻す extern の形
  - ネイティブの段に、C ABI の入口と `catch_unwind`、`Fault` を `Ctx` の欄に置く形と `Ctx` の配置
- `multicore.md`: Host はセッションに1つで、worker が共有する。行の途中で混ざらない
- `testing.md`: 「`eml_rt` の検証」に期待値の表の節 (表の形、2つのテスト、表を作り直す手順、S10 で消す側) と、Miri の対象 (T11 のメモリの Host)
- `docs/README.md`: object-model の行の見出しの並びに「extern」を足す
- `CLAUDE.md` (英語): `eml_rt` の段落に、the 25 externs in the word ABI (Ctx-free arithmetic, string and I/O externs as `Ctx` methods, results proven for the pure ones), the session `Host` (stdio-style buffering in `StdHost`), and `File` as a `Foreign` with a `BufRead` を1〜2文で足す

コードのコメントで、この節を指すべき引用を「extern」に替える。`cargo test -p eml_cli citations` が通ることを確かめる。

- [ ] **Step 3: spec と計画を消して、全体を確かめてコミットする**

```bash
git rm docs/superpowers/specs/2026-10-15-s9b3-word-abi-externs-design.md docs/superpowers/plans/2026-10-15-s9b3-word-abi-externs.md
```

Run: `rg -n 's9b3|S9b-3 の設計' docs crates CLAUDE.md` で、消した文書への参照が残っていないことを確かめる。Global Constraints の確かめる手順を全部流す。

```bash
git add -A docs crates CLAUDE.md
git commit -m "Document the word-ABI externs and record their measurement"
```

//! 信頼の一覧の「Miri で確かめること」(docs/implementation/testing.md の「Miri」)。Verus が確かめない部分と、そこへ
//! 届く検証したコードを通す小さいテストで、Miri はこの module だけを流す。

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vstd::prelude::Tracked;

use super::ctx as ctx_tests;
use super::host::{MemoryFile, MemoryHost, host};
use super::string::text;
use super::{CONS, Probe, cons, list, mortal, probe, raw};
use crate::alloc::MAX_SMALL_WORDS;
use crate::cell::{rc_decref_shared, rc_dup_shared};
use crate::externs::{Fault, Form, Handle, IoOp, IoReason, Open, ReadAmount, Write};
use crate::heap::ctx::positions::find_byte;
use crate::host::{BUFFER_BYTES, Failure, FileIn, Reader, WriteMode, consume, fill, host_open};
use crate::{Checked, Control, Ctx, Fast, Heap, HeapError, Kind, Limits, Meta, Mode, Word};

const PAGE_BYTES: usize = 0x1_0000;

/// 両方のモードで大きい物体になる、語に揃った中身の長さ。大きい物体のページは物体の大きさちょうどに
/// 確保するので、この長さの中身はページの末尾で終わる。64 KiB を超える長さにしても Miri に見えるものは増えない
const LARGE_BODY: usize = MAX_SMALL_WORDS * 8;

/// 破棄処理が panic する `Foreign` の値。呼ばれた回数を数える。
struct Panicker(Arc<AtomicUsize>);

impl Drop for Panicker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("the finalizer panics");
    }
}

// T1
fn pages_are_aligned_and_returned<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let before = heap.bytes_in_use();
    // 一番大きい級 (検査モードの前置きを含めて `MAX_SMALL_WORDS` 語) の物体を1ページに収まらない数だけ確保して、
    // 2ページ目を取らせる
    let cells: Vec<Word> = (0..PAGE_BYTES / (MAX_SMALL_WORDS * 8) + 1)
        .map(|_| heap.alloc_data(0, 0, MAX_SMALL_WORDS - 2, false))
        .collect();
    assert!(heap.bytes_in_use() >= before + 2 * PAGE_BYTES);
    assert_eq!(heap.bytes_in_use() % PAGE_BYTES, 0);
    for &c in &cells {
        assert_eq!(c.bits() % 8, 0);
        unsafe { heap.decref(c) }.unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

fn large_objects_get_their_own_page<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let before = heap.bytes_in_use();
    let big = heap.alloc_data(0, 300, 0, false);
    assert!(heap.bytes_in_use() >= before + 300 * 8);
    unsafe { heap.decref(big) }.unwrap();
    // 速いモードは大きい物体のページを空けたときに返す。検査モードは隔離して、`Heap` を落とすときに返す
    if !M::CHECKED {
        assert_eq!(heap.bytes_in_use(), before);
    }
}

fn dropping_a_heap_returns_every_page<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, 100);
    let big = heap.alloc_data(0, 300, 0, false);
    // 走査する語が 255 個以上の物体は、先頭の語に数を持つ
    unsafe { heap.set_field(big, 1, l) }.unwrap();
    // 物体を生きたまま `Heap` を落とす。Miri はページを返し忘れるとリークとして報告する
    drop(heap);
}

// T2
fn local_and_shared_counts_move<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = heap.alloc_data(0, 0, 1, false);
    unsafe {
        heap.dup(x).unwrap();
        assert_eq!(raw::rc(x), 2);
        heap.decref(x).unwrap();
        assert_eq!(raw::rc(x), 1);
        heap.share(x, 1).unwrap();
        assert_eq!(raw::rc(x), -1);
        heap.dup(x).unwrap();
        assert_eq!(raw::rc(x), -2);
        heap.decref(x).unwrap();
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

fn is_last_reads_twice_only_at_minus_one<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = heap.alloc_data(0, 0, 1, false);
    unsafe {
        assert_eq!(heap.is_last(x), Ok(true));
        heap.share(x, 1).unwrap();
        assert_eq!(heap.is_last(x), Ok(true));
        heap.dup(x).unwrap();
        assert_eq!(heap.is_last(x), Ok(false));
        heap.decref(x).unwrap();
        heap.decref(x).unwrap();
    }
}

/// `share` の3つのスタックと訪問済みの集まりを通す。`lin` の物体と rc 1 の子は所有を移し、rc 2 の子には印を付け、
/// 送り直しでは検査モードの所有者だけのたどりが共有の親の下へ進む
fn share_moves_and_passes_owners<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let unique = list(&mut heap, 1);
    let kept = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 2, 0, true);
    let pair = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.dup(kept).unwrap();
        heap.set_field(lin, 0, unique).unwrap();
        heap.set_field(lin, 1, kept).unwrap();
        heap.set_field(pair, 0, lin).unwrap();
        let rcs = || (raw::rc(pair), raw::rc(lin), raw::rc(unique), raw::rc(kept));
        heap.share(pair, 1).unwrap();
        assert_eq!(rcs(), (-1, 1, 1, -2));
        heap.share(pair, 2).unwrap();
        assert_eq!(rcs(), (-1, 1, 1, -2));
        if M::CHECKED {
            assert_eq!((raw::prefix_first(lin), raw::prefix_first(unique)), (2, 2));
        }
        heap.decref(kept).unwrap();
        heap.set_worker(2);
        heap.decref(pair).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

/// `transfer_back` の2つのスタックと訪問済みの集まりを通す。共有のリストを裏返し、子が確保したセルの所有者を書き換え、
/// その下の共有の組を検査モードの所有者だけのたどりで通る
fn transfer_back_flips_and_rewrites<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    let shared = list(&mut heap, 2);
    let pair = cons(&mut heap, Word::UNIT, Word::UNIT);
    unsafe {
        heap.share(shared, child).unwrap();
        heap.share(pair, child).unwrap();
        heap.set_worker(child);
        let inner = list(&mut heap, 1);
        let top = cons(&mut heap, inner, pair);
        heap.set_worker(0);
        heap.transfer_back(shared, child).unwrap();
        heap.transfer_back(top, child).unwrap();
        assert_eq!(
            (raw::rc(shared), raw::rc(top), raw::rc(inner), raw::rc(pair)),
            (1, 1, 1, -1)
        );
        if M::CHECKED {
            assert_eq!((raw::prefix_first(top), raw::prefix_first(inner)), (0, 0));
        }
        assert_eq!(heap.retire_worker(child), Ok(()));
        heap.decref(shared).unwrap();
        heap.decref(top).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

#[test]
fn check_mode_prefixes_change() {
    let mut heap = Heap::<Checked>::new();
    let lin = heap.alloc_data(0, 0, 1, true);
    let parent = heap.alloc_data(0, 1, 0, false);
    let immortal = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) };
    unsafe {
        heap.set_raw(immortal, 0, 0).unwrap();
        heap.acquire_immortal(immortal).unwrap();
        assert_eq!(raw::prefix_first(immortal), 1);
        heap.set_field(parent, 0, lin).unwrap();
        heap.share(parent, 7).unwrap();
        assert_eq!(raw::prefix_first(lin), 7);
        heap.decref(immortal).unwrap();
        assert_eq!(raw::prefix_first(immortal), 0);
        heap.set_worker(7);
        heap.decref(parent).unwrap();
        assert_eq!(heap.decref(lin), Err(HeapError::UseAfterFree));
    }
}

/// 2つのスレッドに渡す共有の rc の欄の位置。
struct Field(*mut i32);

unsafe impl Send for Field {}

#[test]
fn shared_counts_race_on_two_threads() {
    const REFS: i32 = 64;
    let field = Box::into_raw(Box::new(-REFS));
    let lasts = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let f = Field(field);
            let lasts = lasts.clone();
            std::thread::spawn(move || {
                let f = f;
                // 各スレッドは数えた参照を REFS / 2 個持つ。1つ増やしてから2つ手放すことを繰り返し、持つ参照を
                // 使い切る。最後の参照を見るのは1つのスレッドだけである
                for _ in 0..REFS / 2 {
                    rc_dup_shared(f.0, Tracked::assume_new());
                    for _ in 0..2 {
                        if rc_decref_shared(f.0, Tracked::assume_new()) {
                            lasts.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(lasts.load(Ordering::Relaxed), 1);
    drop(unsafe { Box::from_raw(field) });
}

// T3
fn a_foreign_value_of_another_type_is_wrong_kind<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(String::from("value"), false);
    unsafe {
        assert_eq!(heap.foreign::<Vec<u8>>(f), Err(HeapError::WrongKind));
        let s = heap.foreign::<String>(f).unwrap();
        assert_eq!(*s, "value");
        heap.decref(f).unwrap();
    }
}

fn foreign_values_with_drop_are_finalized_once<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let (value, drops) = probe();
    let boxed = heap.alloc_foreign(Box::new(value), false);
    let string = heap.alloc_foreign(String::from("text"), false);
    let vec = heap.alloc_foreign(vec![1u64, 2, 3], false);
    unsafe {
        heap.decref(boxed).unwrap();
        heap.decref(string).unwrap();
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    // 生きたまま残した `vec` は、`Heap` を落とすときに1回だけ破棄する
    let _ = vec;
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

fn a_zero_sized_foreign_value<M: Mode>() {
    struct Nothing;
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(Nothing, false);
    unsafe {
        assert!(heap.foreign::<Nothing>(f).is_ok());
        heap.decref(f).unwrap();
    }
}

fn a_panicking_finalizer_is_not_run_twice<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let drops = Arc::new(AtomicUsize::new(0));
    let f = heap.alloc_foreign(Panicker(drops.clone()), false);
    let result = catch_unwind(AssertUnwindSafe(|| unsafe { heap.decref(f) }));
    assert!(result.is_err());
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

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

// T7
fn a_panicking_finalizer_in_a_chain_leaks_the_rest<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let panics = Arc::new(AtomicUsize::new(0));
    let (value, drops) = probe();
    let first = heap.alloc_foreign(Panicker(panics.clone()), false);
    let second = heap.alloc_foreign(value, false);
    let parent = cons(&mut heap, first, second);
    let result = catch_unwind(AssertUnwindSafe(|| unsafe { heap.decref(parent) }));
    assert!(result.is_err());
    // 連鎖は止まり、残った箱は漏れる。`Heap` を落とすと、まだ破棄していない値を1回ずつ破棄する
    drop(heap);
    assert_eq!(panics.load(Ordering::Relaxed), 1);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

// T8
fn every_core_operation_once<M: Mode>() {
    const META: Meta = Meta::new(CONS, 2, Kind::Data, false);
    let mut heap = Heap::<M>::new();
    let a = heap.alloc_data(0, 1, 1, false);
    let n = heap.alloc_int(i64::MAX);
    let imm = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::BigInt, false)) };
    // 不死の物体に書けるのは構築中の間だけ
    unsafe { heap.set_raw(imm, 0, 9) }.unwrap();
    let f = heap.alloc_foreign(5u64, false);
    unsafe {
        heap.acquire_immortal(imm).unwrap();
        let raw_cell = heap.alloc(2, META);
        heap.set_field(raw_cell, 0, Word::imm(1)).unwrap();
        heap.set_field(raw_cell, 1, Word::imm(2)).unwrap();
        heap.set_field(a, 0, n).unwrap();
        heap.set_raw(a, 1, 7).unwrap();
        assert_eq!(heap.raw(a, 1), Ok(7));
        assert_eq!(heap.field(a, 0), Ok(n));
        assert_eq!(heap.scanned_len(a), Ok(1));
        assert_eq!(heap.scanned(a, 0), Ok(n));
        assert_eq!(heap.unbox_int(n), Ok(i64::MAX));
        assert_eq!(heap.unbox_int(imm), Ok(9));
        assert_eq!(heap.meta(f).map(|m| m.kind()), Ok(Kind::Foreign));
        assert_eq!(*heap.foreign::<u64>(f).unwrap(), 5);
        heap.dup(a).unwrap();
        assert_eq!(heap.is_unique(a), Ok(false));
        heap.decref(a).unwrap();
        assert_eq!(heap.is_unique(a), Ok(true));
        let token = heap.release_reuse(raw_cell, CONS, &[false, false]).unwrap();
        let (cell, reused) = heap.con_at(token, 2, META).unwrap();
        assert!(reused);
        heap.set_field(cell, 0, Word::imm(3)).unwrap();
        heap.set_field(cell, 1, Word::imm(4)).unwrap();
        let token = heap.into_token(cell).unwrap();
        let cell = heap.reuse_in_place(token, META).unwrap();
        let token = heap.release_reuse(cell, CONS, &[true, true]).unwrap();
        heap.drop_token(token).unwrap();
        let shared = cons(&mut heap, Word::imm(1), Word::imm(2));
        heap.share(shared, 1).unwrap();
        heap.release(shared, CONS, &[false, false]).unwrap();
        let last = heap.alloc_data(0, 0, 0, false);
        assert_eq!(heap.is_last(last), Ok(true));
        heap.free_block(last).unwrap();
        heap.decref(a).unwrap();
        heap.decref(imm).unwrap();
        heap.decref(f).unwrap();
        let d = heap.dummy();
        heap.dup(d).unwrap();
        heap.decref(d).unwrap();
        let x = heap.alloc_float((-0.0f64).to_bits());
        assert_eq!(heap.unbox_float(x), Ok((-0.0f64).to_bits()));
        assert_eq!(heap.unbox_float(d), Ok(0));
        heap.decref(x).unwrap();
    }
    // 生きているのは、ダミーと不死の物体だけである
    assert_eq!(heap.live_objects().len(), 2);
    every_primitive_once::<M>();
}

/// 実行文脈の基本操作を1回ずつ通す。
fn every_primitive_once<M: Mode>() {
    const UNIT: Word = Word::imm(0);
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    let e0 = ctx_tests::push(&mut ctx, 0, 1);
    let e1 = ctx_tests::push(&mut ctx, 1, 1);
    unsafe {
        let c = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(e0, 0, c).unwrap();
        let found = ctx.ev_lookup(0).unwrap();
        assert_eq!(found, e0);
        let got = ctx.ev_clause(found, 0).unwrap();
        assert_eq!(got, c);
        ctx.heap_mut().decref(got).unwrap();
        let outer = ctx.ev_evv(found).unwrap();
        ctx.heap_mut().decref(outer).unwrap();
        ctx.ev_set_state(found, Word::imm(1)).unwrap();
        assert_eq!(ctx.ev_take_state(found).unwrap(), Word::imm(1));
        let renewed = ctx.ev_renew(found, UNIT).unwrap();
        ctx.heap_mut().decref(found).unwrap();
        let list = ctx.evv_get().unwrap();
        // e0 を除き、e1 の節点を写す
        let masked = ctx.evv_mask(&[ctx_tests::item(0, 0, 1)], list).unwrap();
        assert_ne!(masked, list);
        assert_eq!(ctx.heap().field(masked, 0), Ok(e1));
        // 切り替えの1往復
        let current = ctx.switch_task(masked, 0, 0);
        assert_eq!(ctx.switch_task(current, 0, 0), masked);
        for w in [masked, list] {
            ctx.heap_mut().decref(w).unwrap();
        }
        // yield の1往復
        ctx.yield_to(renewed, UNIT, Word::imm(1)).unwrap();
        assert_eq!(ctx.yielding(), 1);
        assert_eq!(ctx.yield_status(renewed), 1);
        let k = ctx.yield_cont_take().unwrap();
        ctx.yield_cont_set(k).unwrap();
        assert_eq!(ctx.yield_cont_take(), Ok(Word::imm(1)));
        assert_eq!(ctx.yield_finish(), Ok(UNIT));
        // 中断の1往復
        ctx.heap_mut().dup(e1).unwrap();
        ctx.yield_abort(e1, UNIT).unwrap();
        assert_eq!(ctx.yield_status(e1), 2);
        assert_eq!(ctx.yield_finish(), Ok(UNIT));
        assert_eq!(ctx.poll(0), Ok(()));
        for e in [e0, e1] {
            ctx.heap_mut().decref(e).unwrap();
        }
        // extern を1回ずつ
        let a = ctx.heap_mut().alloc_string("a");
        let b = ctx.heap_mut().alloc_string("b");
        assert_eq!(ctx.string_compare(a, b), Ok(crate::externs::LT));
        ctx.heap_mut().decref(a).unwrap();
        ctx.heap_mut().decref(b).unwrap();
        let a = ctx.heap_mut().alloc_string("a\n");
        let shown = ctx.string_show(a).unwrap();
        ctx.heap_mut().decref(a).unwrap();
        let n = ctx.int_show(-12);
        let joined = ctx.string_append(shown, n).unwrap();
        ctx.println(crate::Stream::Out, joined).unwrap();
        ctx.flush().unwrap();
        assert_eq!(memory.out(), "\"a\\n\"-12\n");
    }
    ctx_tests::clear(&mut ctx);
    assert!(ctx.check_finished());
    assert_eq!(mortal(&mut ctx), 0);
}

#[test]
fn every_core_operation_once_fast() {
    every_core_operation_once::<Fast>();
}

#[test]
fn every_core_operation_once_checked() {
    every_core_operation_once::<Checked>();
    let mut heap = Heap::<Checked>::new();
    let l = list(&mut heap, 3);
    assert_eq!(super::check_all(&mut heap), Ok(()));
    assert_eq!(heap.leaks().get("Data"), Some(&3));
    unsafe { heap.decref(l) }.unwrap();
    assert!(heap.leaks().is_empty());
}

// T10
fn strings_of_every_length_are_read_back<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // 語に揃わない長さと、大きい物体のページの末尾まで続く長さ
    for n in [
        0,
        1,
        7,
        8,
        9,
        15,
        16,
        17,
        1023,
        1025,
        LARGE_BODY,
        LARGE_BODY + 3,
    ] {
        // 1文字ずつ push すると Miri の下で遅いので、繰り返しを切り詰めて作る
        let mut s = "abcdefghijklmnopqrstuvwxyz".repeat(n / 26 + 1);
        s.truncate(n);
        let w = heap.alloc_string(&s);
        assert_eq!(text(&heap, w), s);
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

fn multibyte_strings_are_read_back<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let s = "é ü 日本 🦀".repeat(100);
    let w = heap.alloc_string(&s);
    assert_eq!(text(&heap, w), s);
    unsafe { heap.decref(w) }.unwrap();
}

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

fn large_strings_are_rebuilt<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let big = "L".repeat(LARGE_BODY + 10);
    let w = heap.alloc_string(&big);
    // 大きい左辺からの作り直し
    let w = unsafe { heap.string_push(w, &big) }.unwrap();
    assert_eq!(text(&heap, w).len(), 2 * big.len());
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(mortal(&mut heap), 0);
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

fn self_concat_under_both_borrow_models<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // 右辺の中身を読んで写した後で x を空ける。大きい物体では、そのときページを返す
    for s in ["abc", "abcdefgh", &"m".repeat(LARGE_BODY)] {
        let x = heap.alloc_string(s);
        unsafe { heap.dup(x) }.unwrap();
        let r = unsafe { heap.string_concat(x, x) }.unwrap();
        assert_eq!(text(&heap, r).len(), 2 * s.len());
        unsafe { heap.decref(r) }.unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

fn appending_the_text_of_another_string<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let big = "R".repeat(LARGE_BODY + 5);
    // その場の追記、語を埋めた左辺の作り直し、大きい右辺、大きい左辺の作り直し
    for (left, right) in [
        ("ab", "c"),
        ("abcdefgh", "ijk"),
        ("ab", big.as_str()),
        (big.as_str(), "xyz"),
    ] {
        let a = heap.alloc_string(left);
        let b = heap.alloc_string(right);
        let s = unsafe { &*heap.string(b).unwrap() };
        let a = unsafe { heap.string_push(a, s) }.unwrap();
        assert_eq!(text(&heap, a), format!("{left}{right}"));
        assert_eq!(text(&heap, b), right);
        unsafe { heap.decref(a) }.unwrap();
        unsafe { heap.decref(b) }.unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

/// `write_byte` は1バイトだけを書き換え、ほかのバイトと長さを変えない。先頭、語の境目の前後、末尾と、大きい物体の
/// ページの末尾の近くを書き、`bytes` で読み戻す。
fn single_bytes_are_overwritten<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for n in [1, 9, 17, LARGE_BODY, LARGE_BODY + 3] {
        let mut expected = "a".repeat(n).into_bytes();
        let w = heap.alloc_string(std::str::from_utf8(&expected).unwrap());
        let h = w.as_ptr();
        let cap = n.div_ceil(8) + 1;
        let mut at = vec![0, n - 1, n / 2];
        at.extend([7, 8].into_iter().filter(|&i| i < n));
        for (k, &i) in at.iter().enumerate() {
            let b = b'A' + k as u8;
            crate::text::write_byte(h, cap, i, b, Tracked::assume_new());
            expected[i] = b;
        }
        assert_eq!(
            crate::text::bytes(h, n, Tracked::assume_new()),
            &expected[..]
        );
        assert_eq!(text(&heap, w).as_bytes(), &expected[..]);
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
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

/// `String.show` は、エスケープしない区間を写す元の文字列の範囲として写す (T10 の写しを、区間が先頭、途中、末尾に
/// あり、多バイトの文字と接する場合で通す)。
fn parts_of_a_string_are_copied<M: Mode>() {
    let mut ctx = ctx_tests::ctx::<M>();
    let cases = [
        ("日\n本", "\"日\\n本\""),
        ("\nab日", "\"\\nab日\""),
        ("ab\"", "\"ab\\\"\""),
        ("\u{1}é\u{7f}", "\"\\u{1}é\\u{7f}\""),
    ];
    for (input, shown) in cases {
        unsafe {
            let s = ctx.heap_mut().alloc_string(input);
            let w = ctx.string_show(s).unwrap();
            assert_eq!(text(ctx.heap(), w), shown);
            ctx.heap_mut().decref(w).unwrap();
            ctx.heap_mut().decref(s).unwrap();
        }
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// T5 の `utf8_valid` は `core::str::from_utf8` で確かめる。`String.decode` が渡すのと同じく物体の中身の `&[u8]` を、
/// 1語に収まる長さ、語の境目をまたぐ長さ、大きい物体のページの後ろまで続く長さで渡す。妥当でなければ、確かめが
/// 止まった位置 (先頭、途中、末尾) を返す。
fn utf8_is_checked_by_core<M: Mode>() {
    let mut ctx = ctx_tests::ctx::<M>();
    // 大きい物体のページの末尾で終わる長さ (8 文字の 24 バイトずつで語に揃う) と、語に揃わない長さ
    let long = "日".repeat(LARGE_BODY.div_ceil(24) * 8).into_bytes();
    let long_unaligned = "日".repeat(LARGE_BODY / 3 + 1).into_bytes();
    let mut broken_end = long.clone();
    broken_end.push(0xff);
    let mut broken_middle = long.clone();
    let middle = long.len() / 2 + 2;
    broken_middle[middle] = b'a';
    let cases = [
        (b"".to_vec(), None),
        ("a日".as_bytes().to_vec(), None),
        (vec![b'a'; 9], None),
        (vec![0xe6, 0x97], Some(0)),
        (b"ab\xffcdefghij".to_vec(), Some(2)),
        (long.clone(), None),
        (long_unaligned, None),
        (broken_end, Some(long.len())),
        // 3 バイトの文字の途中を ASCII に替えると、その文字の頭で止まる
        (broken_middle, Some(middle - middle % 3)),
    ];
    for (b, at) in cases {
        unsafe {
            let w = ctx.heap_mut().alloc_bytes(&b);
            let p = ctx.heap().bytes(w).unwrap();
            assert_eq!(crate::heap::ctx::bytes::utf8_valid(&*p), at);
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// バイト列を書き換える道 (T10 の `write_byte` と、同じ物体の範囲の写し)。`src` と `dst` が同じ物体の
/// `append_part` は、写した先へ元から読む。
fn bytes_are_written_through_the_core<M: Mode>() {
    let mut ctx = ctx_tests::ctx::<M>();
    unsafe {
        let w = ctx.bytes_make(17, b'a' as i64).unwrap();
        let w = ctx.bytes_set(8, b'b' as i64, w).unwrap();
        let w = ctx.bytes_push(b'c' as i64, w).unwrap();
        ctx.heap_mut().dup(w).unwrap();
        let ww = ctx.bytes_append_part(7, 10, w, w).unwrap();
        let ww = ctx.bytes_truncate(20, ww).unwrap();
        let p = ctx.heap().bytes(ww).unwrap();
        assert_eq!(&*p, b"aaaaaaaabaaaaaaaacab");
        ctx.heap_mut().decref(ww).unwrap();
        ctx.heap_mut().decref(w).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 別のバイト列の範囲をその場で足す `append_part` は、`src` の中身を借りたまま `dst` に `write_bytes` で写す。
/// 写す元と写す先は別のブロックである。大きい物体の `src` からも写す。
fn a_part_of_other_bytes_is_appended_in_place<M: Mode>() {
    let mut ctx = ctx_tests::ctx::<M>();
    let large: Vec<u8> = (0..LARGE_BODY + 3).map(|i| i as u8).collect();
    unsafe {
        let dst = ctx.heap_mut().alloc_bytes_room(LARGE_BODY + 32);
        let dst = ctx.bytes_push(b'-' as i64, dst).unwrap();
        let src = ctx.heap_mut().alloc_bytes(&large);
        let w = ctx.bytes_append_part(5, 14, src, dst).unwrap();
        assert_eq!(w, dst);
        let w = ctx
            .bytes_append_part(9, LARGE_BODY as i64 + 3, src, w)
            .unwrap();
        assert_eq!(w, dst);
        let mut expected = vec![b'-'];
        expected.extend_from_slice(&large[5..14]);
        expected.extend_from_slice(&large[9..]);
        assert_eq!(&*ctx.heap().bytes(w).unwrap(), &expected[..]);
        assert_eq!(&*ctx.heap().bytes(src).unwrap(), &large[..]);
        ctx.heap_mut().decref(src).unwrap();
        ctx.heap_mut().decref(w).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 共有されたバイト列の `truncate` と `set` は、新しいブロックに写してから書く (T10 の `write_bytes` で前の部分を
/// 写し、`set` はその後 `write_byte` で書き換える)。元の中身は変わらない。
fn shared_bytes_are_copied_before_writing<M: Mode>() {
    let mut ctx = ctx_tests::ctx::<M>();
    let large: Vec<u8> = (0..LARGE_BODY + 3).map(|i| (i % 251) as u8).collect();
    for b in [b"abcdefghijklmnopq".to_vec(), large] {
        unsafe {
            let a = ctx.heap_mut().alloc_bytes(&b);
            ctx.heap_mut().dup(a).unwrap();
            let t = ctx.bytes_truncate(9, a).unwrap();
            assert_ne!(t, a);
            assert_eq!(&*ctx.heap().bytes(t).unwrap(), &b[..9]);
            ctx.heap_mut().dup(a).unwrap();
            let last = b.len() as i64 - 1;
            let s = ctx.bytes_set(last, 0xee, a).unwrap();
            assert_ne!(s, a);
            let mut expected = b.clone();
            expected[last as usize] = 0xee;
            assert_eq!(&*ctx.heap().bytes(s).unwrap(), &expected[..]);
            assert_eq!(&*ctx.heap().bytes(a).unwrap(), &b[..]);
            for w in [a, t, s] {
                ctx.heap_mut().decref(w).unwrap();
            }
        }
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// `bytes_lend` の位置に外から書き、`bytes_commit` で確定する。語に揃わない長さの後ろへの書き込みと、大きい物体の
/// ページの末尾まで埋める書き込みを通し、`bytes` で読み戻す。
fn lent_room_is_written_and_committed<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for room in [16, LARGE_BODY + 3] {
        unsafe {
            let w = heap.alloc_bytes_room(room);
            let mut expected = Vec::new();
            for step in [3, 0, usize::MAX] {
                let b = heap.bytes_lend(w).unwrap();
                assert!(b.cap >= room - expected.len());
                let n = step.min(b.cap);
                let more: Vec<u8> = (expected.len()..expected.len() + n)
                    .map(|i| i as u8)
                    .collect();
                for (i, &v) in more.iter().enumerate() {
                    b.ptr.add(i).write(v);
                }
                heap.bytes_commit(w, n).unwrap();
                expected.extend(more);
                assert_eq!(&*heap.bytes(w).unwrap(), &expected[..]);
            }
            // 埋めた後は空きがない
            assert_eq!(heap.bytes_lend(w).unwrap().cap, 0);
            heap.decref(w).unwrap();
        }
    }
    assert_eq!(mortal(&mut heap), 0);
}

/// `bytes_lend` で貸した後、外から書く前に、ほかの物体を確保して解放する (VM が完了を待つ間にほかのタスクを
/// 流す形)。貸した位置に書いて確定し、貸した物体とほかの物体をどちらも読み戻す。
fn lent_room_outlives_other_allocations<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for room in [16, LARGE_BODY + 3] {
        let other: Vec<u8> = (0..room).map(|i| (i % 251) as u8 ^ 0x5a).collect();
        unsafe {
            let w = heap.alloc_bytes_room(room);
            let b = heap.bytes_lend(w).unwrap();
            let x = heap.alloc_bytes(&other);
            let y = heap.alloc_bytes(b"xyz");
            heap.decref(x).unwrap();
            // 空いた `x` の場所を使い直す確保
            let z = heap.alloc_bytes(&other);
            let n = b.cap.min(room);
            let expected: Vec<u8> = (0..n).map(|i| i as u8).collect();
            for (i, &v) in expected.iter().enumerate() {
                b.ptr.add(i).write(v);
            }
            heap.bytes_commit(w, n).unwrap();
            assert_eq!(&*heap.bytes(w).unwrap(), &expected[..]);
            assert_eq!(&*heap.bytes(y).unwrap(), b"xyz");
            assert_eq!(&*heap.bytes(z).unwrap(), &other[..]);
            for v in [w, y, z] {
                heap.decref(v).unwrap();
            }
        }
    }
    assert_eq!(mortal(&mut heap), 0);
}

// 凍結した領域
fn frozen_pages_are_returned<M: Mode>() {
    let (b, _, _) = super::frozen::builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    // Miri は返していないページを漏れとして誤りにする
    drop(region);
}

fn two_threads_read_one_region<M: Mode>() {
    let (b, s, _) = super::frozen::builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let region = region.clone();
            std::thread::spawn(move || {
                let mut heap = Heap::<M>::new();
                heap.attach(region);
                // 領域の物体には、どちらのモードの dup と decref も書き込まない
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

fn chains_drop_in_any_order<M: Mode>() {
    // 最初に落とすのは、子の塊、親の塊、付けたヒープのどれか
    for first in ["child", "parent", "heap"] {
        let (b1, s1, _) = super::frozen::builder::<M>();
        let r1 = Arc::new(b1.freeze().unwrap());
        let mut b2 = Heap::<M>::new();
        b2.attach(r1.clone());
        unsafe { b2.acquire_immortal(s1) }.unwrap();
        let some = unsafe { b2.alloc_immortal(1, Meta::new(1, 1, Kind::Data, false)) };
        unsafe { b2.set_field(some, 0, s1) }.unwrap();
        let r2 = Arc::new(b2.freeze().unwrap());
        let mut heap = Heap::<M>::new();
        heap.attach(r2.clone());
        match first {
            "child" => {
                drop(r2);
                drop(r1);
            }
            "parent" => {
                drop(r1);
                drop(r2);
            }
            _ => {
                let inner = unsafe { heap.field(some, 0) }.unwrap();
                assert_eq!(text(&heap, inner), "constant");
                drop(heap);
                // 子の塊の親への `Arc` が、親の塊を生かす
                drop(r1);
                drop(r2);
                continue;
            }
        }
        // 付けたヒープの `Arc` だけが鎖を生かす
        let inner = unsafe { heap.field(some, 0) }.unwrap();
        assert_eq!(text(&heap, inner), "constant");
        drop(heap);
    }
}

fn own_objects_read_region_words<M: Mode>() {
    let (b, s, big) = super::frozen::builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    let mut heap = Heap::<M>::new();
    heap.attach(region);
    let pair = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.acquire_immortal(s).unwrap();
        heap.acquire_immortal(big).unwrap();
        heap.set_field(pair, 0, s).unwrap();
        heap.set_field(pair, 1, big).unwrap();
        let n = heap.field(pair, 1).unwrap();
        assert_eq!(heap.unbox_int(n).unwrap(), i64::MAX);
        // 領域の左辺から写して、自分の文字列にする
        heap.dup(s).unwrap();
        heap.dup(s).unwrap();
        let r = heap.string_push(s, "!").unwrap();
        let c = heap.string_concat(r, s).unwrap();
        assert_eq!(text(&heap, c), "constant!constant");
        heap.decref(c).unwrap();
        // share は領域の子でたどりを止め、共有の親を手放す連鎖は領域の子に書き込まない
        heap.share(pair, 1).unwrap();
        heap.decref(pair).unwrap();
    }
    assert_eq!(text(&heap, s), "constant");
    assert_eq!(mortal(&mut heap), 0);
}

// T14: 回復は、根から届く語のブロックを、ページの頭の `sink` でこのヒープのブロックと塊のブロックに分ける。凍結した
// 塊のページの `sink` は null で、このヒープのページの `sink` はこのヒープの `RemoteSink` を指す
fn recovery_tells_own_blocks_from_region_blocks<M: Mode>() {
    let (b, s, _) = super::frozen::builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    let mut ctx = ctx_tests::ctx::<M>();
    unsafe {
        ctx.heap_mut().attach(region);
        let own = ctx.heap_mut().alloc_string("own");
        ctx.root_set(0, own).unwrap();
        ctx.root_set(1, s).unwrap();
        ctx.recover().unwrap();
        let own = ctx.root_get(0).unwrap();
        assert_eq!(text(ctx.heap(), own), "own");
        ctx.heap_mut().decref(own).unwrap();
        // 塊の語は写さずにそのまま根に残る
        assert_eq!(ctx.root_get(1), Ok(s));
    }
    for w in ctx.take_roots().into_iter().flatten() {
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

// T14: 別のスレッドからの解放 (docs/spec/object-model.md の「別のスレッドからの解放」)。どのテストも、受け手の
// ヒープを作ったスレッドから別のスレッドへ動かして使い、`RemoteSink` の番地がヒープを動かしても変わらないことを通す

/// 共有の Control を持つヒープを2つ作る。
fn two_heaps<M: Mode>(limits: Limits) -> (Heap<M>, Heap<M>) {
    let control = Arc::new(Control::new(limits));
    (
        Heap::with_control(control.clone()),
        Heap::with_control(control),
    )
}

/// 長さ `n` の共有のリスト。
fn shared_list<M: Mode>(heap: &mut Heap<M>, n: usize) -> Word {
    let xs = list(heap, n);
    unsafe { heap.share(xs, 0) }.unwrap();
    xs
}

/// リストのセルの番地。
fn cells<M: Mode>(heap: &Heap<M>, xs: Word) -> Vec<usize> {
    let mut out = Vec::new();
    let mut w = xs;
    while w.is_ptr() {
        out.push(w.bits());
        w = unsafe { heap.field(w, 1) }.unwrap();
    }
    out
}

// 別のスレッドが空けた小さいブロックは、持ち主の取り込みで空きリストに戻り、速いモードでは次の確保に使われる。
// 取り込まずに落としたヒープは、`Drop` が取り込む
fn a_remote_small_free_is_reclaimed_and_reused<M: Mode>() {
    const N: usize = 8;
    let (mut a, b) = two_heaps::<M>(Limits::default());
    let xs = shared_list(&mut a, N);
    let freed = cells(&a, xs);
    let b = std::thread::spawn(move || {
        let mut b = b;
        unsafe { b.decref(xs) }.unwrap();
        b
    })
    .join()
    .unwrap();
    if M::CHECKED {
        assert_eq!(b.stats().remote_frees, N as u64);
    }
    // 受けた数が少ないうちは取り込まない
    assert!(!a.reclaim_if_due());
    a.reclaim();
    if M::CHECKED {
        assert_eq!(a.stats().reclaimed, N as u64);
    }
    assert_eq!(mortal(&mut a), 0);
    let y = cons(&mut a, Word::UNIT, Word::UNIT);
    if !M::CHECKED {
        assert!(freed.contains(&y.bits()));
    }
    unsafe { a.decref(y) }.unwrap();
    // 積んだまま落とす
    let ys = shared_list(&mut a, N);
    let mut b = b;
    unsafe { b.decref(ys) }.unwrap();
    drop(a);
    drop(b);
}

// 別のスレッドが空けた大きい物体のページは、空けたスレッドが Control のバイト数から引く。上限がちょうど大きい
// 物体1つ分のとき、持ち主が取り込む前に2つ目を取っても上限に届かない。持ち主はページを返すときにもう一度は引かない
fn a_remote_large_free_refunds_at_once<M: Mode>() {
    let (base, large) = {
        let (mut a, b) = two_heaps::<M>(Limits::default());
        let control = a.control().clone();
        let base = control.bytes();
        let big = a.alloc_data(0, 0, LARGE_BODY / 8, false);
        let large = control.bytes() - base;
        unsafe { a.decref(big) }.unwrap();
        drop(b);
        (base, large)
    };
    let (mut a, b) = two_heaps::<M>(Limits {
        max_bytes: base + large,
        ..Limits::default()
    });
    let control = a.control().clone();
    let big = a.alloc_data(0, 0, LARGE_BODY / 8, false);
    unsafe { a.share(big, 0) }.unwrap();
    assert_eq!(control.bytes(), base + large);
    let b = std::thread::spawn(move || {
        let mut b = b;
        unsafe { b.decref(big) }.unwrap();
        b
    })
    .join()
    .unwrap();
    if M::CHECKED {
        // 検査モードの大きい物体のページは隔離して返さない
        assert_eq!(control.bytes(), base + large);
        assert!(a.reclaim_if_due());
        assert_eq!(control.bytes(), base + large);
    } else {
        assert_eq!(control.bytes(), base);
        let second = a.alloc_data(0, 0, LARGE_BODY / 8, false);
        assert_eq!(control.stop_bits(), 0);
        assert!(a.reclaim_if_due());
        assert_eq!(control.bytes(), base + large);
        unsafe { a.decref(second) }.unwrap();
    }
    assert_eq!(mortal(&mut a), 0);
    drop(b);
}

// 2つのスレッドが1つのページに積む。その間、持ち主のヒープも別のスレッドへ動かす
fn two_threads_free_into_one_page<M: Mode>() {
    const N: usize = 16;
    let control = Arc::new(Control::new(Limits::default()));
    let mut a = Heap::<M>::with_control(control.clone());
    let xs = shared_list(&mut a, N);
    let ys = shared_list(&mut a, N);
    let freers: Vec<_> = [xs, ys]
        .into_iter()
        .map(|w| {
            let b = Heap::<M>::with_control(control.clone());
            std::thread::spawn(move || {
                let mut b = b;
                unsafe { b.decref(w) }.unwrap();
                b
            })
        })
        .collect();
    let owner = std::thread::spawn(move || {
        let mut a = a;
        let z = list(&mut a, N);
        unsafe { a.decref(z) }.unwrap();
        a
    });
    let mut a = owner.join().unwrap();
    for t in freers {
        drop(t.join().unwrap());
    }
    a.reclaim();
    if M::CHECKED {
        assert_eq!(a.stats().reclaimed, 2 * N as u64);
    }
    assert_eq!(mortal(&mut a), 0);
}

// 持ち主がページを替えながら切り出す間に、別のスレッドが共有の文字列の長さと中身を読み、小さいブロックと大きい
// 物体を空ける。持ち主の次の `reclaim_if_due` は、返ってきた大きい物体のページを見て取り込み、速いモードではその後の
// 確保はページを増やさない。数が 256 に届いて取り込むことは、スレッドを使わない単体テストで確かめる
fn the_owner_carves_while_another_thread_reads_and_frees<M: Mode>() {
    const N: usize = 16;
    let (mut a, b) = two_heaps::<M>(Limits::default());
    let control = a.control().clone();
    let s = a.alloc_string("shared text");
    let xs = shared_list(&mut a, N);
    let big = a.alloc_data(0, 0, LARGE_BODY / 8, false);
    unsafe {
        a.share(s, 0).unwrap();
        a.share(big, 0).unwrap();
    }
    let freer = std::thread::spawn(move || {
        let mut b = b;
        assert_eq!(text(&b, s), "shared text");
        unsafe {
            b.decref(xs).unwrap();
            b.decref(s).unwrap();
            b.decref(big).unwrap();
        }
        b
    });
    // 1 KiB の箱でページを替えるまで切り出す (64 KiB のページに 64 個も入らない)
    let carved: Vec<Word> = (0..70)
        .map(|_| a.alloc_string_room(MAX_SMALL_WORDS * 8 - 64))
        .collect();
    let b = freer.join().unwrap();
    for w in carved {
        unsafe { a.decref(w) }.unwrap();
    }
    assert!(a.reclaim_if_due());
    let before = control.bytes();
    let w = list(&mut a, N);
    if !M::CHECKED {
        assert_eq!(control.bytes(), before);
    }
    unsafe { a.decref(w) }.unwrap();
    assert_eq!(mortal(&mut a), 0);
    drop(b);
}

// 取り込みは ID ごとの数も仕事の回数も変えない。数は空けたスレッドが減らしてある
#[test]
fn reclaim_changes_no_owner_count_and_no_work_counter() {
    const N: usize = 8;
    let (mut a, b) = two_heaps::<Checked>(Limits::default());
    let ids: Vec<u32> = (0..3).map(|_| a.fork_worker()).collect();
    assert_eq!(ids, [1, 2, 3]);
    a.set_worker(3);
    let xs = list(&mut a, N);
    a.set_worker(0);
    let b = std::thread::spawn(move || {
        let mut b = b;
        b.set_worker(3);
        unsafe { b.decref(xs) }.unwrap();
        b.set_worker(0);
        b
    })
    .join()
    .unwrap();
    let counts = (a.owned_by(3), a.owned_by(0));
    let before = a.stats();
    a.reclaim();
    let after = a.stats();
    assert_eq!((a.owned_by(3), a.owned_by(0)), counts);
    assert_eq!(counts.0, 0);
    assert_eq!(
        (
            after.rc_increments,
            after.rc_decrements,
            after.allocations,
            after.peak_live,
            after.shared,
            after.remote_frees,
        ),
        (
            before.rc_increments,
            before.rc_decrements,
            before.allocations,
            before.peak_live,
            before.shared,
            before.remote_frees,
        )
    );
    assert_eq!(after.live, before.live - N as u64);
    assert_eq!(after.reclaimed, before.reclaimed + N as u64);
    for id in ids {
        assert_eq!(a.retire_worker(id), Ok(()));
    }
    assert_eq!(super::check_all(&mut a), Ok(()));
    drop(b);
}

// 実行の制御
// T1: 2つのスレッドのヒープが、1つの Control にページの数を足し引きする
fn two_threads_share_one_control<M: Mode>() {
    let control = Arc::new(Control::new(Limits::default()));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let control = control.clone();
            std::thread::spawn(move || {
                let mut heap = Heap::<M>::with_control(control);
                let big = heap.alloc_data(0, 0, LARGE_BODY / 8, false);
                unsafe { heap.decref(big) }.unwrap();
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(control.bytes(), 0);
}

// 別のスレッドが立てている間の止める語を読む。join の後だけ読むと、並んだ読み書きを通らない
#[test]
fn an_interrupt_from_another_thread_is_seen() {
    let control = Arc::new(Control::new(Limits::default()));
    let ctx = Ctx::<Fast>::new(control.clone(), host(&Arc::default()));
    let setter = {
        let control = control.clone();
        std::thread::spawn(move || control.interrupt())
    };
    while ctx.poll(0).is_ok() {
        std::thread::yield_now();
    }
    assert_eq!(ctx.poll(0), Err(crate::Stop::Interrupted));
    setter.join().unwrap();
    assert_eq!(control.stop_bits(), Control::INTERRUPT);
}

// `PREEMPT` は worker の語だけに立つ。別のスレッドが立てたのを `poll` が読む
#[test]
fn a_preempt_from_another_thread_is_seen() {
    let control = Arc::new(Control::new(Limits::default()));
    let ctx = Ctx::<Fast>::new(control.clone(), host(&Arc::default()));
    let setter = {
        let control = control.clone();
        std::thread::spawn(move || control.preempt())
    };
    while ctx.poll(0).is_ok() {
        std::thread::yield_now();
    }
    assert_eq!(ctx.poll(0), Err(crate::Stop::Preempt));
    setter.join().unwrap();
    assert_eq!(control.stop_bits(), 0);
    control.take_preempt();
    assert_eq!(ctx.poll(0), Ok(()));
}

// T11

/// Host を通る書き込み。メモリの Host で、`println` と `flush` を通す。
fn println_goes_through_the_host<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let a = ctx.heap_mut().alloc_string("日本");
        ctx.println(crate::Stream::Out, a).unwrap();
    }
    ctx.flush().unwrap();
    assert_eq!(memory.out(), "日本\n");
    assert_eq!(mortal(&mut ctx), 0);
}

/// `handle_read` でハンドルを借りて読み、読んだ中身を写して手放す。
unsafe fn read_borrowed<M: Mode>(
    ctx: &mut Ctx<M>,
    f: Word,
    read: ReadAmount,
) -> Result<String, crate::Fault> {
    unsafe {
        let w = ctx.handle_read(f, Handle::FileIn, read, Form::Text)?;
        let read = text(ctx.heap(), w);
        ctx.heap_mut().decref(w).unwrap();
        Ok(read)
    }
}

/// 長い行のファイル。長さの見込みのない読み手で、行も全体も 64 KiB の区切りをまたぐ。
fn long_lines() -> (String, String, MemoryFile) {
    let long_line = "x".repeat(65_600);
    let all = format!("{long_line}\na\r\n\nlast");
    let file = MemoryFile {
        sized: false,
        chunk: 4096,
        ..MemoryFile::text(&all)
    };
    (long_line, all, file)
}

/// Host を通る行の読み。64 KiB の区切りをまたぐ行を、ハンドルを借りる `handle_read` で通し、続く行を同じハンドルで
/// 読んで、区切りをまたいだ後に読み手に残った分を確かめる。
fn borrowed_lines_cross_the_buffer<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let (long_line, _, file) = long_lines();
    memory.add("lines.txt", file);
    let mut ctx = Ctx::<M>::new(Arc::new(Control::new(Limits::default())), host(&memory));
    unsafe {
        let p = ctx.heap_mut().alloc_string("lines.txt");
        let f = ctx.fs_open(p, Open::Read).unwrap();
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::Line).unwrap(),
            format!("{long_line}\n")
        );
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::Line).unwrap(),
            "a\r\n"
        );
        ctx.handle_close(f, Handle::FileIn).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// Host を通る全部の読みと行の読み。区切りをまたぐ長さと長さの見込みのない読み手、`\r\n` を残した行、空の行、行の
/// 終わりのない最後の行、終わりの空の文字列を通し、短い読み、途中の誤り、UTF-8 でない
/// 中身、止める語、閉じることを、全部の読みと行の読みの両方で通す。誤りと止めたときも、ハンドルは呼ぶ側に残り、
/// `close` で閉じる。
fn borrowed_reads_cross_the_buffer_and_fail<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let control = Arc::new(Control::new(Limits::default()));
    memory.add(
        "broken.txt",
        MemoryFile {
            fail_after: Some(1),
            ..MemoryFile::text("ab")
        },
    );
    memory.add(
        "bad.bin",
        MemoryFile {
            bytes: vec![0xc3],
            sized: true,
            ..MemoryFile::default()
        },
    );
    memory.add(
        "fifo",
        MemoryFile {
            interrupt_with: Some(control.clone()),
            ..MemoryFile::text("abc")
        },
    );
    let (_, all, file) = long_lines();
    memory.add("lines.txt", file);
    memory.add("short.txt", MemoryFile::text("a\r\n\nlast"));
    let mut ctx = Ctx::<M>::new(control.clone(), host(&memory));
    let open = |ctx: &mut Ctx<M>, name: &str| unsafe {
        let p = ctx.heap_mut().alloc_string(name);
        ctx.fs_open(p, Open::Read).unwrap()
    };
    unsafe {
        let f = open(&mut ctx, "short.txt");
        for expected in ["a\r\n", "\n", "last", ""] {
            assert_eq!(
                read_borrowed(&mut ctx, f, ReadAmount::Line).unwrap(),
                expected
            );
        }
        ctx.handle_close(f, Handle::FileIn).unwrap();
        for read in [ReadAmount::Line, ReadAmount::All] {
            for name in ["broken.txt", "bad.bin"] {
                let f = open(&mut ctx, name);
                assert!(read_borrowed(&mut ctx, f, read).is_err());
                assert_eq!(raw::rc(f), 1);
                ctx.handle_close(f, Handle::FileIn).unwrap();
                assert_eq!(mortal(&mut ctx), 0);
            }
            // 止める語が立った後の `Interrupted` で止まる (T11)
            let f = open(&mut ctx, "fifo");
            assert_eq!(
                read_borrowed(&mut ctx, f, read),
                Err(crate::Fault::Stopped(crate::Stop::Interrupted))
            );
            ctx.handle_close(f, Handle::FileIn).unwrap();
            control.clear();
            let f = open(&mut ctx, "lines.txt");
            control.interrupt();
            assert_eq!(
                read_borrowed(&mut ctx, f, read),
                Err(crate::Fault::Stopped(crate::Stop::Interrupted))
            );
            control.clear();
            ctx.handle_close(f, Handle::FileIn).unwrap();
            assert_eq!(mortal(&mut ctx), 0);
        }
        let f = open(&mut ctx, "lines.txt");
        assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(), all);
        ctx.handle_close(f, Handle::FileIn).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 信頼の一覧の総称の `fill` と `consume` を、`FileIn` の生のポインタで直接通す。buffer に残ったバイト、短い読みの後の
/// 補充、終わり、補充の前の書き出し、読みの誤りにパスを付けること、止める語を確かめる。
#[test]
fn fill_and_consume_read_a_file_through_a_pointer() {
    let memory = Arc::new(MemoryHost::default());
    memory.add(
        "a.txt",
        MemoryFile {
            chunk: 3,
            ..MemoryFile::text("abcde")
        },
    );
    memory.add(
        "broken.txt",
        MemoryFile {
            fail_after: Some(1),
            ..MemoryFile::text("ab")
        },
    );
    let control = Control::new(Limits::default());
    let host = host(&memory);
    let opened = |path: &str| match host_open(&host, path) {
        Ok(file) => file,
        Err(_) => panic!("{path} did not open"),
    };
    let mut file = opened("a.txt");
    let p: *mut FileIn = &mut file;
    unsafe {
        assert_eq!(fill(p, &control, None).unwrap(), b"abc");
        consume(p, 2);
        assert_eq!(fill(p, &control, None).unwrap(), b"c");
        consume(p, 1);
        assert_eq!(memory.flushes(), 0);
        assert_eq!(fill(p, &control, Some(&host)).unwrap(), b"de");
        assert_eq!(memory.flushes(), 1);
        // buffer から読むときは書き出さない
        assert_eq!(fill(p, &control, Some(&host)).unwrap(), b"de");
        assert_eq!(memory.flushes(), 1);
        consume(p, 2);
        assert_eq!(fill(p, &control, None).unwrap(), b"");
    }
    let mut file = opened("broken.txt");
    let p: *mut FileIn = &mut file;
    unsafe {
        assert_eq!(fill(p, &control, None).unwrap(), b"ab");
        consume(p, 2);
        assert_eq!(
            fill(p, &control, None),
            Err(crate::Fault::Io {
                op: crate::IoOp::Read,
                name: "broken.txt".into(),
                reason: crate::IoReason::Other
            })
        );
    }
    let mut file = opened("a.txt");
    let p: *mut FileIn = &mut file;
    control.interrupt();
    assert_eq!(
        unsafe { fill(p, &control, None) },
        Err(crate::Fault::Stopped(crate::Stop::Interrupted))
    );
}

/// 渡した buffer より大きい長さを1回だけ返す読み手。`Read` の約束を破る安全なコードである。
struct OverlongOnce {
    lied: bool,
}

impl io::Read for OverlongOnce {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.lied {
            self.lied = true;
            return Ok(buf.len() + 1);
        }
        buf[0] = b'x';
        Ok(1)
    }
}

/// buffer より大きい長さを返す読み手は I/O の誤りにし、読み手を壊さない。次の読みは続けて読める。
#[test]
fn an_overlong_read_is_refused_and_the_reader_survives() {
    let control = Control::new(Limits::default());
    let mut reader = Reader::new(Box::new(OverlongOnce { lied: false }));
    assert!(matches!(
        reader.fill_buf(&control, None),
        Err(Failure::Io(crate::IoReason::Other))
    ));
    assert!(matches!(reader.fill_buf(&control, None), Ok(b"x")));
}

/// `handle_read` のバイト列の読みは、検証したコードから `fill` と `consume` を呼び、借りた読み手の buffer の slice を
/// ヒープのバイト列にそのまま写す。1つの buffer の中の行、2回の補充をまたぐ行、行の途中の読みの誤り (途中まで読んだ
/// 行を手放す)、塊の読み (その場で足すことと、共有のバイト列を写すこと)、全部の読みを通す。
fn borrowed_bytes_go_through_the_host<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("lines.txt", MemoryFile::text("ab\ncd\n"));
    memory.add(
        "slow.txt",
        MemoryFile {
            chunk: 3,
            ..MemoryFile::text("abcdefg\nhi")
        },
    );
    memory.add(
        "broken.txt",
        MemoryFile {
            chunk: 2,
            fail_after: Some(3),
            ..MemoryFile::text("abcdef")
        },
    );
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    let open = |ctx: &mut Ctx<M>, name: &str| unsafe {
        let p = ctx.heap_mut().alloc_string(name);
        ctx.fs_open(p, Open::Read).unwrap()
    };
    let read = |ctx: &mut Ctx<M>, f: Word, amount: ReadAmount| unsafe {
        let w = ctx.handle_read(f, Handle::FileIn, amount, Form::Bytes)?;
        let bytes = (*ctx.heap().bytes(w).unwrap()).to_vec();
        ctx.heap_mut().decref(w).unwrap();
        Ok::<_, crate::Fault>(bytes)
    };
    unsafe {
        let f = open(&mut ctx, "lines.txt");
        assert_eq!(read(&mut ctx, f, ReadAmount::Line).unwrap(), b"ab\n");
        assert_eq!(read(&mut ctx, f, ReadAmount::Line).unwrap(), b"cd\n");
        assert_eq!(read(&mut ctx, f, ReadAmount::Line).unwrap(), b"");
        ctx.handle_close(f, Handle::FileIn).unwrap();
        let f = open(&mut ctx, "slow.txt");
        assert_eq!(read(&mut ctx, f, ReadAmount::Line).unwrap(), b"abcdefg\n");
        let buf = ctx.heap_mut().alloc_bytes(b">");
        ctx.heap_mut().dup(buf).unwrap();
        let mut w = ctx
            .handle_read(f, Handle::FileIn, ReadAmount::Chunk(buf), Form::Bytes)
            .unwrap();
        assert_ne!(w, buf);
        // buffer に残った1バイトだけを足し、次の塊は補充して読む
        assert_eq!(&*ctx.heap().bytes(w).unwrap(), b">h");
        for _ in 0..2 {
            w = ctx
                .handle_read(f, Handle::FileIn, ReadAmount::Chunk(w), Form::Bytes)
                .unwrap();
            assert_eq!(&*ctx.heap().bytes(w).unwrap(), b">hi");
        }
        ctx.heap_mut().decref(w).unwrap();
        ctx.heap_mut().decref(buf).unwrap();
        ctx.handle_close(f, Handle::FileIn).unwrap();
        let f = open(&mut ctx, "broken.txt");
        assert!(read(&mut ctx, f, ReadAmount::Line).is_err());
        assert_eq!(raw::rc(f), 1);
        ctx.handle_close(f, Handle::FileIn).unwrap();
        let f = open(&mut ctx, "lines.txt");
        assert_eq!(read(&mut ctx, f, ReadAmount::All).unwrap(), b"ab\ncd\n");
        ctx.handle_close(f, Handle::FileIn).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 開くファイルの読み手が、渡した buffer より大きい長さを1回返す Host。
struct OverlongHost;

impl crate::Host for OverlongHost {
    fn write_line(&self, _: crate::Stream, _: &str) -> io::Result<()> {
        Ok(())
    }

    fn write(&self, _: crate::Stream, _: &[u8]) -> io::Result<()> {
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }

    fn open(&self, _: &str) -> io::Result<crate::Opened> {
        Ok(crate::Opened {
            reader: Box::new(OverlongOnce { lied: false }),
            size: None,
        })
    }
}

/// 全部の読み (文字列とバイト列) も、buffer より大きい長さを返す読み手を std の中で panic させずに I/O の誤りにする。
#[test]
fn an_overlong_read_is_refused_by_a_whole_read() {
    let control = Arc::new(Control::new(Limits::default()));
    let mut ctx = Ctx::<Fast>::new(control, crate::HostRef::new(Arc::new(OverlongHost)));
    for form in [Form::Text, Form::Bytes] {
        unsafe {
            let p = ctx.heap_mut().alloc_string("x");
            let f = ctx.fs_open(p, Open::Read).unwrap();
            assert_eq!(
                ctx.handle_read(f, Handle::FileIn, ReadAmount::All, form),
                Err(crate::Fault::Io {
                    op: crate::IoOp::Read,
                    name: "x".into(),
                    reason: crate::IoReason::Other
                })
            );
            assert_eq!(raw::rc(f), 1);
            ctx.handle_close(f, Handle::FileIn).unwrap();
        }
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 標準入力の1行を読んで写し、手放す。
fn stdin_line<M: Mode>(ctx: &mut Ctx<M>) -> String {
    let w = ctx_tests::stdin_read(ctx, true).unwrap();
    let line = text(ctx.heap(), w).to_string();
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
    line
}

/// 1回の補充で buffer に入った2行は、補充の前の1回だけ書き出して読む。2行目は buffer から読み、書き出さない。
fn two_buffered_lines_flush_once<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile::text("a\nb\n")));
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    assert_eq!(stdin_line(&mut ctx), "a\n");
    assert_eq!(stdin_line(&mut ctx), "b\n");
    assert_eq!(memory.flushes(), 1);
    assert_eq!(mortal(&mut ctx), 0);
}

/// 2回の補充をまたぐ行は、補充の前ごとに書き出す。
fn a_line_across_two_refills_flushes_before_each<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        chunk: 3,
        ..MemoryFile::text("abcd\n")
    }));
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    assert_eq!(stdin_line(&mut ctx), "abcd\n");
    assert_eq!(memory.flushes(), 2);
    assert_eq!(mortal(&mut ctx), 0);
}

/// 標準入力の読み手は Host の slot に戻るので、同じ Host の上の2つ目の `Ctx` は、1つ目が buffer に残した行から読む。
fn the_stdin_reader_survives_a_second_ctx<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile::text("a\nb\n")));
    let mut first = ctx_tests::ctx_with::<M>(&memory);
    assert_eq!(stdin_line(&mut first), "a\n");
    assert_eq!(mortal(&mut first), 0);
    drop(first);
    let mut second = ctx_tests::ctx_with::<M>(&memory);
    assert_eq!(stdin_line(&mut second), "b\n");
    assert_eq!(stdin_line(&mut second), "");
    assert_eq!(mortal(&mut second), 0);
}

/// 標準入力のハンドルは、開くときに Host の slot から読み手を取り、閉じるときに buffer に残ったバイトごと返す。開いて
/// いる間の2つ目の `open` は `InUse` で、閉じた後は開ける。標準入力のない Host では読み手のないハンドルが開き、読みは
/// `not available` になる。`Heap` を落とすときの破棄処理も読み手を返す。
fn stdin_handles_take_and_return_the_reader<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile::text("a\nb\nc\n")));
    let read = |ctx: &mut Ctx<M>, h: Word| unsafe {
        let w = ctx.handle_read(h, Handle::Stdin, ReadAmount::Line, Form::Text)?;
        let line = text(ctx.heap(), w).to_string();
        ctx.heap_mut().decref(w).unwrap();
        Ok::<_, crate::Fault>(line)
    };
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let h = ctx.handle_open(Handle::Stdin).unwrap();
        assert_eq!(
            ctx.handle_open(Handle::Stdin),
            Err(crate::Fault::InUse(crate::StdStream::Stdin))
        );
        assert_eq!(read(&mut ctx, h).unwrap(), "a\n");
        ctx.handle_close(h, Handle::Stdin).unwrap();
        let h = ctx.handle_open(Handle::Stdin).unwrap();
        assert_eq!(read(&mut ctx, h).unwrap(), "b\n");
        // 閉じずに `Heap` ごと落とす
        assert_eq!(raw::rc(h), 1);
    }
    drop(ctx);
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let h = ctx.handle_open(Handle::Stdin).unwrap();
        assert_eq!(read(&mut ctx, h).unwrap(), "c\n");
        assert_eq!(read(&mut ctx, h).unwrap(), "");
        ctx.handle_close(h, Handle::Stdin).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
    let mut ctx = ctx_tests::ctx_with::<M>(&Arc::new(MemoryHost::default()));
    unsafe {
        let h = ctx.handle_open(Handle::Stdin).unwrap();
        assert_eq!(
            read(&mut ctx, h),
            Err(crate::Fault::Stdin(crate::IoReason::NotAvailable))
        );
        let buf = ctx.heap_mut().alloc_bytes(b"");
        assert_eq!(
            ctx.handle_read(h, Handle::Stdin, ReadAmount::Chunk(buf), Form::Bytes),
            Err(crate::Fault::Stdin(crate::IoReason::NotAvailable))
        );
        ctx.handle_close(h, Handle::Stdin).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 標準入力のバイト列の読みは、補充の前ごとに書き出す。2回の補充をまたぐ行は2回書き出し、buffer に残ったバイトを
/// 読む塊の読みは書き出さず、buffer が空の塊の読みは1回書き出す。
fn stdin_bytes_flush_before_each_refill<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        chunk: 3,
        ..MemoryFile::text("abcd\nefg")
    }));
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let h = ctx.handle_open(Handle::Stdin).unwrap();
        let w = ctx
            .handle_read(h, Handle::Stdin, ReadAmount::Line, Form::Bytes)
            .unwrap();
        assert_eq!(&*ctx.heap().bytes(w).unwrap(), b"abcd\n");
        assert_eq!(memory.flushes(), 2);
        ctx.heap_mut().decref(w).unwrap();
        // 2回目の補充で読んだ "e" は buffer に残っている
        let buf = ctx.heap_mut().alloc_bytes(b"");
        let w = ctx
            .handle_read(h, Handle::Stdin, ReadAmount::Chunk(buf), Form::Bytes)
            .unwrap();
        assert_eq!(&*ctx.heap().bytes(w).unwrap(), b"e");
        assert_eq!(memory.flushes(), 2);
        let w = ctx
            .handle_read(h, Handle::Stdin, ReadAmount::Chunk(w), Form::Bytes)
            .unwrap();
        assert_eq!(&*ctx.heap().bytes(w).unwrap(), b"efg");
        assert_eq!(memory.flushes(), 3);
        ctx.heap_mut().decref(w).unwrap();
        let w = ctx
            .handle_read(h, Handle::Stdin, ReadAmount::All, Form::Bytes)
            .unwrap();
        assert_eq!(&*ctx.heap().bytes(w).unwrap(), b"");
        ctx.heap_mut().decref(w).unwrap();
        ctx.handle_close(h, Handle::Stdin).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 書き込みのハンドルは Host の buffer に書き (`host_write`)、開いた印を `HostRef` に立てる。閉じずに `Heap` ごと
/// 落としても、破棄処理 (`Drop`) が印を下ろすので、同じ `HostRef` を写した次の `Ctx` でまた開ける。標準入力のない
/// Host でも、標準入力のハンドルの印は同じく働く。
fn stream_handles_lower_their_flags_when_dropped<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let shared = host(&memory);
    let handles = [Handle::Stdin, Handle::Stdout, Handle::Stderr];
    let mut ctx = Ctx::<M>::new(Arc::new(Control::new(Limits::default())), shared.clone());
    unsafe {
        let opened: Vec<Word> = handles
            .iter()
            .map(|&handle| ctx.handle_open(handle).unwrap())
            .collect();
        for (&handle, stream) in handles.iter().zip([
            crate::StdStream::Stdin,
            crate::StdStream::Stdout,
            crate::StdStream::Stderr,
        ]) {
            assert_eq!(ctx.handle_open(handle), Err(crate::Fault::InUse(stream)));
        }
        let b = ctx.heap_mut().alloc_bytes(b"o");
        let out = ctx
            .handle_write(opened[1], Handle::Stdout, b, crate::Write::Bytes)
            .unwrap();
        let err = ctx
            .handle_write(opened[2], Handle::Stderr, b, crate::Write::Bytes)
            .unwrap();
        ctx.heap_mut().decref(b).unwrap();
        assert_eq!((raw::rc(out), raw::rc(err)), (1, 1));
    }
    drop(ctx);
    assert_eq!(
        (memory.out(), memory.err()),
        ("o".to_string(), "o".to_string())
    );
    let mut ctx = Ctx::<M>::new(Arc::new(Control::new(Limits::default())), shared);
    for handle in handles {
        unsafe {
            let h = ctx.handle_open(handle).unwrap();
            ctx.handle_close(h, handle).unwrap();
        }
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// `FileOut` を `path` に `mode` で開く。
unsafe fn open_out<M: Mode>(ctx: &mut Ctx<M>, path: &str, mode: WriteMode) -> Word {
    unsafe {
        let p = ctx.heap_mut().alloc_string(path);
        ctx.fs_open(p, Open::Write(mode)).unwrap()
    }
}

/// `FileOut` は自分の buffer に書き、`flush` で Host の書き手に書き出す。文字列、行、バイト列の書き込みと、buffer の
/// 大きさ以上の書き込みが buffer を通らずに届くこと、`Append` が前の中身を残すことを確かめる。開けないパスは、パスを
/// 手放して `Io { op: Open, .. }` を返す。
fn a_file_out_writes_through_its_buffer<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    let large = vec![b'z'; BUFFER_BYTES + 3];
    unsafe {
        let t = ctx.heap_mut().alloc_string("日本");
        let b = ctx.heap_mut().alloc_bytes(b"\xff");
        let w = ctx.heap_mut().alloc_bytes(&large);
        let f = open_out(&mut ctx, "out.txt", WriteMode::Truncate);
        let f = ctx
            .handle_write(f, Handle::FileOut, t, Write::Text)
            .unwrap();
        let f = ctx
            .handle_write(f, Handle::FileOut, t, Write::Line)
            .unwrap();
        let f = ctx
            .handle_write(f, Handle::FileOut, b, Write::Bytes)
            .unwrap();
        assert_eq!(memory.written_bytes("out.txt"), Some(Vec::new()));
        let f = ctx.handle_flush(f, Handle::FileOut).unwrap();
        let head = "日本日本\n".len() + 1;
        assert_eq!(
            memory.written_bytes("out.txt").unwrap(),
            ["日本日本\n".as_bytes(), b"\xff"].concat()
        );
        let f = ctx
            .handle_write(f, Handle::FileOut, w, Write::Bytes)
            .unwrap();
        assert_eq!(
            memory.written_bytes("out.txt").unwrap().len(),
            head + large.len()
        );
        assert_eq!((raw::rc(f), raw::rc(t), raw::rc(w)), (1, 1, 1));
        ctx.handle_close(f, Handle::FileOut).unwrap();
        let f = open_out(&mut ctx, "out.txt", WriteMode::Append);
        let f = ctx
            .handle_write(f, Handle::FileOut, t, Write::Line)
            .unwrap();
        let f = ctx.handle_flush(f, Handle::FileOut).unwrap();
        ctx.handle_close(f, Handle::FileOut).unwrap();
        let written = memory.written_bytes("out.txt").unwrap();
        assert_eq!(written.len(), head + large.len() + "日本\n".len());
        assert!(written.ends_with("z日本\n".as_bytes()));
        let p = ctx.heap_mut().alloc_string("");
        assert_eq!(
            ctx.fs_open(p, Open::Write(WriteMode::Truncate)),
            Err(Fault::Io {
                op: IoOp::Open,
                name: "".into(),
                reason: IoReason::PermissionDenied
            })
        );
        for x in [t, b, w] {
            ctx.heap_mut().decref(x).unwrap();
        }
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 閉じない `FileOut` は、`Ctx` を落とすときの破棄処理 (`BufWriter` の drop glue) が buffer を書く。書けない書き手
/// では誤りを捨て、panic しない。
fn a_dropped_file_out_writes_its_buffer<M: Mode>() {
    for (memory, expected) in [
        (MemoryHost::default(), "kept\n"),
        (MemoryHost::failing("disk full"), ""),
    ] {
        let memory = Arc::new(memory);
        let mut ctx = ctx_tests::ctx_with::<M>(&memory);
        unsafe {
            let t = ctx.heap_mut().alloc_string("kept");
            let f = open_out(&mut ctx, "kept.txt", WriteMode::Truncate);
            ctx.handle_write(f, Handle::FileOut, t, Write::Line)
                .unwrap();
            ctx.heap_mut().decref(t).unwrap();
        }
        assert_eq!(memory.written("kept.txt").as_deref(), Some(""));
        drop(ctx);
        assert_eq!(memory.written("kept.txt").as_deref(), Some(expected));
        assert_eq!(memory.received(), "kept\n".len());
    }
}

/// buffer があふれた書き込みと `flush` が書けなければ、`Io { op: Write, .. }` を返してハンドルを手放し、書いていない
/// buffer を捨てる。その後は、`Ctx` を落としても書き手は何も受けない。読むハンドルと書くハンドルの取り違えは
/// `WrongKind` で、ヒープは変わらない。
fn a_failed_file_out_write_discards_the_rest<M: Mode>() {
    let memory = Arc::new(MemoryHost::failing("disk full"));
    memory.add("in.txt", MemoryFile::text("x"));
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    let failed = |path: &str| {
        Err(Fault::Io {
            op: IoOp::Write,
            name: path.into(),
            reason: IoReason::Other,
        })
    };
    unsafe {
        let t = ctx.heap_mut().alloc_string("abc");
        let w = ctx.heap_mut().alloc_bytes(&vec![b'z'; BUFFER_BYTES]);
        let f = open_out(&mut ctx, "a.txt", WriteMode::Truncate);
        let f = ctx
            .handle_write(f, Handle::FileOut, t, Write::Text)
            .unwrap();
        assert_eq!(memory.received(), 0);
        assert_eq!(
            ctx.handle_write(f, Handle::FileOut, w, Write::Bytes),
            failed("a.txt")
        );
        assert_eq!(mortal(&mut ctx), 2);
        let f = open_out(&mut ctx, "b.txt", WriteMode::Truncate);
        let f = ctx
            .handle_write(f, Handle::FileOut, t, Write::Line)
            .unwrap();
        assert_eq!(ctx.handle_flush(f, Handle::FileOut), failed("b.txt"));
        assert_eq!(mortal(&mut ctx), 2);
        let out = open_out(&mut ctx, "c.txt", WriteMode::Truncate);
        let p = ctx.heap_mut().alloc_string("in.txt");
        let file = ctx.fs_open(p, Open::Read).unwrap();
        let live = mortal(&mut ctx);
        for handle in [Handle::FileOut, Handle::FileIn] {
            assert_eq!(
                ctx.handle_read(out, handle, ReadAmount::All, Form::Text),
                Err(Fault::Heap(HeapError::WrongKind))
            );
            assert_eq!(
                ctx.handle_write(file, handle, t, Write::Text),
                Err(Fault::Heap(HeapError::WrongKind))
            );
        }
        assert_eq!(mortal(&mut ctx), live);
        assert_eq!((raw::rc(out), raw::rc(file), raw::rc(t)), (1, 1, 1));
        ctx.handle_close(out, Handle::FileOut).unwrap();
        ctx.handle_close(file, Handle::FileIn).unwrap();
        ctx.heap_mut().decref(t).unwrap();
        ctx.heap_mut().decref(w).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
    drop(ctx);
    // 書き手が受けたのは、あふれたときの "abc" と、`flush` の "abc\n" だけである
    assert_eq!(memory.received(), "abc".len() + "abc\n".len());
}

/// スクリプトの extern の橋 (引数、環境変数、標準入力) を、メモリの Host で通す。
/// Host から写した文字列がヒープの文字列になり、引数の文字列が手放されることを確かめる。
fn script_externs_go_through_the_host<M: Mode>() {
    let memory = Arc::new(
        MemoryHost::default()
            .with_args(vec!["日本".into()])
            .with_env(vec![("K", "v".into())])
            .with_stdin(MemoryFile::text("a\nbc")),
    );
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        assert_eq!(ctx.env_arg_count(), 1);
        let w = ctx.env_arg(0).unwrap();
        assert_eq!(text(ctx.heap(), w), "日本");
        ctx.heap_mut().decref(w).unwrap();
        assert!(ctx.env_arg(1).is_err());
        let k = ctx.heap_mut().alloc_string("K");
        assert_eq!(ctx.env_has(k), Ok(crate::externs::TRUE));
        let k = ctx.heap_mut().alloc_string("K");
        let w = ctx.env_var(k).unwrap();
        assert_eq!(text(ctx.heap(), w), "v");
        ctx.heap_mut().decref(w).unwrap();
        let w = ctx_tests::stdin_read(&mut ctx, true).unwrap();
        assert_eq!(text(ctx.heap(), w), "a\n");
        ctx.heap_mut().decref(w).unwrap();
        let w = ctx_tests::stdin_read(&mut ctx, false).unwrap();
        assert_eq!(text(ctx.heap(), w), "bc");
        ctx.heap_mut().decref(w).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 子の起動、標準入力への書き込み、`close_stdin`、標準出力の全部の読み、`wait` の流れを、メモリの Host で通す。どの
/// 引数の文字列も手放され、待った子の破棄処理は kill しない。
fn a_child_goes_through_the_host<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_child_output("b\na\n日本".as_bytes()));
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let t = ctx.heap_mut().alloc_string("a\n");
        let b = ctx.heap_mut().alloc_bytes(b"\xff");
        let c = ctx_tests::spawn(&mut ctx, "sort", &["-r"], &["K", "v"], "sub", (2, 2, 0)).unwrap();
        let c = ctx.handle_write(c, Handle::Child, t, Write::Text).unwrap();
        let c = ctx.handle_write(c, Handle::Child, t, Write::Line).unwrap();
        let c = ctx.handle_write(c, Handle::Child, b, Write::Bytes).unwrap();
        let c = ctx.proc_close_stdin(c).unwrap();
        let w = ctx
            .handle_read(c, Handle::Child, ReadAmount::All, Form::Text)
            .unwrap();
        assert_eq!(text(ctx.heap(), w), "b\na\n日本");
        assert_eq!((raw::rc(c), raw::rc(t), raw::rc(b)), (1, 1, 1));
        assert_eq!(ctx.proc_wait(c), Ok(1));
        for x in [w, t, b] {
            ctx.heap_mut().decref(x).unwrap();
        }
    }
    assert_eq!(memory.child_input(), b"a\na\n\n\xff");
    assert_eq!(memory.waits_and_kills(), (1, 0));
    assert_eq!(mortal(&mut ctx), 0);
}

/// 待たずに `Heap` を落とすと、`Child` の破棄処理が子を kill して1回だけ待つ。
fn a_dropped_child_is_killed_and_waited<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    ctx_tests::spawn(&mut ctx, "sort", &[], &[], "", (2, 2, 0)).unwrap();
    assert_eq!(memory.waits_and_kills(), (0, 0));
    drop(ctx);
    assert_eq!(memory.waits_and_kills(), (1, 1));
}

/// 待った子の `Child` を手放しても、kill せず、もう待たない。
fn a_waited_child_is_not_killed<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    let c = ctx_tests::spawn(&mut ctx, "sort", &[], &[], "", (2, 2, 0)).unwrap();
    assert_eq!(unsafe { ctx.proc_wait(c) }, Ok(0));
    assert_eq!(mortal(&mut ctx), 0);
    drop(ctx);
    assert_eq!(memory.waits_and_kills(), (1, 0));
}

/// 子の標準入力に書けなければ、プログラムの名前を付けた `Io { op: Write, .. }` を返してハンドルを手放す。手放した
/// `Child` の破棄処理が子を kill して待つ。借りたデータは手放さない。
fn a_failed_child_write_releases_the_handle<M: Mode>() {
    let memory = Arc::new(MemoryHost::failing("broken pipe"));
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let t = ctx.heap_mut().alloc_string("abc");
        let c = ctx_tests::spawn(&mut ctx, "sort", &[], &[], "", (2, 2, 0)).unwrap();
        assert_eq!(
            ctx.handle_write(c, Handle::Child, t, Write::Line),
            Err(Fault::Io {
                op: IoOp::Write,
                name: "sort".into(),
                reason: IoReason::Other,
            })
        );
        assert_eq!(mortal(&mut ctx), 1);
        assert_eq!(memory.waits_and_kills(), (1, 1));
        ctx.heap_mut().decref(t).unwrap();
    }
    assert!(memory.child_input().is_empty());
    assert_eq!(mortal(&mut ctx), 0);
}

/// 子の標準入力への書き込みが `Interrupted` で戻ったとき、止める語が立っていなければ書き直し、立っていれば `Stopped`
/// を返してハンドルを手放す。
fn an_interrupted_child_write_stops_only_when_asked<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_child_interrupted_once());
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let t = ctx.heap_mut().alloc_string("abc");
        let c = ctx_tests::spawn(&mut ctx, "sort", &[], &[], "", (2, 2, 0)).unwrap();
        let c = ctx.handle_write(c, Handle::Child, t, Write::Line).unwrap();
        assert_eq!(ctx.proc_wait(c), Ok(0));
        ctx.heap_mut().decref(t).unwrap();
    }
    assert_eq!(memory.child_input(), b"abc\n");
    let control = Arc::new(Control::new(Limits::default()));
    let memory = Arc::new(
        MemoryHost::default()
            .interrupting(control.clone())
            .with_child_interrupted_once(),
    );
    let mut ctx = Ctx::<M>::new(control, host(&memory));
    unsafe {
        let t = ctx.heap_mut().alloc_string("abc");
        let c = ctx_tests::spawn(&mut ctx, "sort", &[], &[], "", (2, 2, 0)).unwrap();
        assert_eq!(
            ctx.handle_write(c, Handle::Child, t, Write::Line),
            Err(Fault::Stopped(crate::Stop::Interrupted))
        );
        assert_eq!(mortal(&mut ctx), 1);
        ctx.heap_mut().decref(t).unwrap();
    }
    assert!(memory.child_input().is_empty());
    assert_eq!(memory.waits_and_kills(), (1, 1));
    assert_eq!(mortal(&mut ctx), 0);
}

// T13

/// `find_byte` は物体の中身を借りた `&[u8]` を読む。1語に収まる長さ、語の境目の前後、大きい物体のページの末尾で
/// 終わる長さと語に揃わない残りで終わる長さで、`Bytes.find_byte_from` と `Bytes.find_from` を通す。
fn find_byte_reads_the_body_of_an_object<M: Mode>() {
    let mut ctx = ctx_tests::ctx::<M>();
    // 期待する位置は中身と一緒に書く。素朴なループで求めると、大きい中身を1バイトずつ読む走査が Tree Borrows の下で
    // 遅いためである。大きい中身は `b` で埋めて末尾にだけ "ax" を置くので、`x` と "ax" はそれぞれ高々1つしかない
    let ending_in = |len: usize, tail: &[u8]| {
        let mut body = vec![b'b'; len - tail.len()];
        body.extend_from_slice(tail);
        body
    };
    let cases = [
        (b"".to_vec(), None, None),
        (b"abcdefg".to_vec(), None, None),
        (b"abcdefgx".to_vec(), Some(7), None),
        (b"abcdefghx".to_vec(), Some(8), None),
        // `x` がページの末尾で終わる語の中にある場合と、語に揃わない残りの中にある場合
        (
            ending_in(LARGE_BODY, b"ax"),
            Some(LARGE_BODY - 1),
            Some(LARGE_BODY - 2),
        ),
        (
            ending_in(LARGE_BODY + 3, b"ax"),
            Some(LARGE_BODY + 2),
            Some(LARGE_BODY + 1),
        ),
        (ending_in(LARGE_BODY + 3, b""), None, None),
    ];
    unsafe {
        let needle = ctx.heap_mut().alloc_bytes(b"ax");
        for (body, x_at, ax_at) in cases {
            let w = ctx.heap_mut().alloc_bytes(&body);
            let len = body.len();
            let froms: &[usize] = if len >= LARGE_BODY {
                &[0, len]
            } else {
                &[0, 1, 8, len / 2, len]
            };
            for &from in froms {
                if from > len {
                    continue;
                }
                let expected = x_at.filter(|&i| i >= from);
                let p = ctx.heap().bytes(w).unwrap();
                assert_eq!(find_byte(&*p, from, b'x'), expected);
                let r = expected.map_or(-1, |i| i as i64);
                assert_eq!(ctx.bytes_find_byte(b'x' as i64, from as i64, w), Ok(r));
                let r = ax_at.filter(|&i| i >= from).map_or(-1, |i| i as i64);
                assert_eq!(ctx.bytes_find_from(needle, from as i64, w), Ok(r));
            }
            ctx.heap_mut().decref(w).unwrap();
        }
        ctx.heap_mut().decref(needle).unwrap();
    }
    assert_eq!(mortal(&mut ctx), 0);
}

both_modes!(
    pages_are_aligned_and_returned,
    large_objects_get_their_own_page,
    dropping_a_heap_returns_every_page,
    local_and_shared_counts_move,
    is_last_reads_twice_only_at_minus_one,
    share_moves_and_passes_owners,
    transfer_back_flips_and_rewrites,
    a_foreign_value_of_another_type_is_wrong_kind,
    foreign_values_with_drop_are_finalized_once,
    a_zero_sized_foreign_value,
    a_panicking_finalizer_is_not_run_twice,
    a_foreign_value_written_in_place_is_finalized_once,
    a_panicking_finalizer_in_a_chain_leaks_the_rest,
    strings_of_every_length_are_read_back,
    multibyte_strings_are_read_back,
    unaligned_appends_are_written,
    large_strings_are_rebuilt,
    empty_strings_are_appended,
    self_concat_under_both_borrow_models,
    appending_the_text_of_another_string,
    single_bytes_are_overwritten,
    reading_then_appending,
    parts_of_a_string_are_copied,
    utf8_is_checked_by_core,
    bytes_are_written_through_the_core,
    a_part_of_other_bytes_is_appended_in_place,
    shared_bytes_are_copied_before_writing,
    lent_room_is_written_and_committed,
    lent_room_outlives_other_allocations,
    frozen_pages_are_returned,
    two_threads_read_one_region,
    chains_drop_in_any_order,
    own_objects_read_region_words,
    recovery_tells_own_blocks_from_region_blocks,
    two_threads_share_one_control,
    a_remote_small_free_is_reclaimed_and_reused,
    a_remote_large_free_refunds_at_once,
    two_threads_free_into_one_page,
    the_owner_carves_while_another_thread_reads_and_frees,
    println_goes_through_the_host,
    borrowed_lines_cross_the_buffer,
    borrowed_reads_cross_the_buffer_and_fail,
    borrowed_bytes_go_through_the_host,
    two_buffered_lines_flush_once,
    a_line_across_two_refills_flushes_before_each,
    the_stdin_reader_survives_a_second_ctx,
    stdin_handles_take_and_return_the_reader,
    stdin_bytes_flush_before_each_refill,
    stream_handles_lower_their_flags_when_dropped,
    a_file_out_writes_through_its_buffer,
    a_dropped_file_out_writes_its_buffer,
    a_failed_file_out_write_discards_the_rest,
    script_externs_go_through_the_host,
    a_child_goes_through_the_host,
    a_dropped_child_is_killed_and_waited,
    a_waited_child_is_not_killed,
    a_failed_child_write_releases_the_handle,
    an_interrupted_child_write_stops_only_when_asked,
    find_byte_reads_the_body_of_an_object,
);

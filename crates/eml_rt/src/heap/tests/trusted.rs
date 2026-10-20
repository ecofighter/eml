//! 信頼の一覧の「Miri で確かめること」(docs/implementation/testing.md の「Miri」)。Verus が確かめない部分と、そこへ
//! 届く検証したコードを通す小さいテストで、Miri はこの module だけを流す。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vstd::prelude::Tracked;

use super::ctx as ctx_tests;
use super::host::{MemoryFile, MemoryHost, host};
use super::string::text;
use super::{CONS, Probe, cons, list, mortal, probe, raw};
use crate::cell::{rc_decref_shared, rc_dup_shared};
use crate::{Checked, Control, Ctx, Fast, Heap, HeapError, Kind, Limits, Meta, Mode, Word};

const PAGE_BYTES: usize = 0x1_0000;

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
    // 2 語の物体を1ページに収まらない数だけ確保して、2ページ目を取らせる
    let cells: Vec<Word> = (0..PAGE_BYTES / 16 + 1)
        .map(|_| heap.alloc_data(0, 1, 0, false))
        .collect();
    assert!(heap.bytes_in_use() > before);
    assert_eq!(heap.bytes_in_use() % PAGE_BYTES, 0);
    for &c in &cells {
        assert_eq!(c.bits() % 8, 0);
        unsafe { heap.decref(c) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
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
    assert_eq!(mortal(&heap), 0);
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
        let a = ctx.heap_mut().alloc_string("a\n");
        let shown = ctx.string_show(a).unwrap();
        let n = ctx.int_show(-12);
        let joined = ctx.string_append(shown, n).unwrap();
        ctx.write_string(crate::Stream::Out, joined).unwrap();
        ctx.flush().unwrap();
        assert_eq!(memory.out(), "\"a\\n\"-12\n");
    }
    ctx_tests::clear(&mut ctx);
    assert!(ctx.check_finished());
    assert_eq!(mortal(ctx.heap()), 0);
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
    assert_eq!(heap.check_invariants(), Ok(()));
    assert_eq!(heap.leaks().get("Data"), Some(&3));
    unsafe { heap.decref(l) }.unwrap();
    assert!(heap.leaks().is_empty());
}

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

fn self_concat_under_both_borrow_models<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // 右辺の中身を読んで写した後で x を空ける。1ページの長さでは、そのときページを返す
    for s in ["abc", "abcdefgh", &"m".repeat(PAGE_BYTES)] {
        let x = heap.alloc_string(s);
        unsafe { heap.dup(x) }.unwrap();
        let r = unsafe { heap.string_concat(x, x) }.unwrap();
        assert_eq!(text(&heap, r).len(), 2 * s.len());
        unsafe { heap.decref(r) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn appending_the_text_of_another_string<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let big = "R".repeat(PAGE_BYTES + 5);
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
    assert_eq!(mortal(&heap), 0);
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
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
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
    assert_eq!(mortal(&heap), 0);
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

// T11

/// Host を通る書き込み。メモリの Host で、`println` と `flush` を通す。
fn println_goes_through_the_host<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_tests::ctx_with::<M>(&memory);
    unsafe {
        let a = ctx.heap_mut().alloc_string("日本");
        ctx.write_string(crate::Stream::Out, a).unwrap();
    }
    ctx.flush().unwrap();
    assert_eq!(memory.out(), "日本\n");
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 1行を読み、組から File と行を取り出す。
unsafe fn next_line<M: Mode>(ctx: &mut Ctx<M>, f: Word) -> (Word, String) {
    unsafe {
        let t = ctx.fs_read(f, true).unwrap();
        let file = ctx.heap().field(t, 0).unwrap();
        let w = ctx.heap().field(t, 1).unwrap();
        let line = text(ctx.heap(), w);
        ctx.heap_mut()
            .release(t, crate::externs::TUPLE, &[true, true])
            .unwrap();
        ctx.heap_mut().decref(w).unwrap();
        (file, line)
    }
}

/// Host を通る読み。区切りをまたぐ長さ、長さの見込みのない読み手、短い読み、途中の誤り、UTF-8 でない中身、
/// 止める語、閉じることを、全部の読みと行の読みの両方で通す。
fn files_go_through_the_host<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let control = Arc::new(Control::new(Limits::default()));
    let body = "0123456789".repeat(7000);
    memory.add(
        "big.txt",
        MemoryFile {
            sized: false,
            chunk: 4096,
            ..MemoryFile::text(&body)
        },
    );
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
    let long_line = "x".repeat(70_000);
    memory.add(
        "lines.txt",
        MemoryFile {
            chunk: 4096,
            ..MemoryFile::text(&format!("{long_line}\na\r\n\nlast"))
        },
    );
    let mut ctx = Ctx::<M>::new(control.clone(), host(&memory));
    unsafe {
        // 64 KiB の区切りをまたぐ行、`\r\n` を残した行、空の行、行の終わりのない最後の行、終わりの空の文字列
        let p = ctx.heap_mut().alloc_string("lines.txt");
        let mut f = ctx.fs_open(p).unwrap();
        for expected in [
            format!("{long_line}\n"),
            "a\r\n".into(),
            "\n".into(),
            "last".into(),
            String::new(),
        ] {
            let line;
            (f, line) = next_line(&mut ctx, f);
            assert_eq!(line, expected);
        }
        ctx.fs_close(f).unwrap();
        assert_eq!(mortal(ctx.heap()), 0);
        for name in ["broken.txt", "bad.bin"] {
            let p = ctx.heap_mut().alloc_string(name);
            let f = ctx.fs_open(p).unwrap();
            assert!(ctx.fs_read(f, true).is_err());
            assert_eq!(mortal(ctx.heap()), 0);
        }
        let p = ctx.heap_mut().alloc_string("fifo");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(
            ctx.fs_read(f, true),
            Err(crate::Fault::Stopped(crate::Stop::Interrupted))
        );
        control.clear();
        let p = ctx.heap_mut().alloc_string("lines.txt");
        let f = ctx.fs_open(p).unwrap();
        control.interrupt();
        assert_eq!(
            ctx.fs_read(f, true),
            Err(crate::Fault::Stopped(crate::Stop::Interrupted))
        );
        control.clear();
        assert_eq!(mortal(ctx.heap()), 0);
        let p = ctx.heap_mut().alloc_string("big.txt");
        let f = ctx.fs_open(p).unwrap();
        let t = ctx.fs_read(f, false).unwrap();
        let w = ctx.heap().field(t, 1).unwrap();
        assert_eq!(text(ctx.heap(), w), body);
        ctx.heap_mut().decref(t).unwrap();
        for name in ["broken.txt", "bad.bin"] {
            let p = ctx.heap_mut().alloc_string(name);
            let f = ctx.fs_open(p).unwrap();
            assert!(ctx.fs_read(f, false).is_err());
        }
        let p = ctx.heap_mut().alloc_string("big.txt");
        let f = ctx.fs_open(p).unwrap();
        ctx.fs_close(f).unwrap();
        // 止める語が立った後の `Interrupted` で止まる (T11)
        let p = ctx.heap_mut().alloc_string("fifo");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(
            ctx.fs_read(f, false),
            Err(crate::Fault::Stopped(crate::Stop::Interrupted))
        );
        control.clear();
        let before = mortal(ctx.heap());
        let p = ctx.heap_mut().alloc_string("big.txt");
        let f = ctx.fs_open(p).unwrap();
        control.interrupt();
        assert_eq!(
            ctx.fs_read(f, false),
            Err(crate::Fault::Stopped(crate::Stop::Interrupted))
        );
        assert_eq!(mortal(ctx.heap()), before);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// スクリプトの extern の橋 (引数、環境変数、標準入力、ファイルの書き込み、子のプロセス) を、メモリの Host で通す。
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
        let w = ctx.stdin_read(true).unwrap();
        assert_eq!(text(ctx.heap(), w), "a\n");
        ctx.heap_mut().decref(w).unwrap();
        let w = ctx.stdin_read(false).unwrap();
        assert_eq!(text(ctx.heap(), w), "bc");
        ctx.heap_mut().decref(w).unwrap();
        let p = ctx.heap_mut().alloc_string("f");
        let t = ctx.heap_mut().alloc_string("x");
        ctx.fs_write_text(p, t).unwrap();
        let p = ctx.heap_mut().alloc_string("prog");
        let a = ctx.heap_mut().alloc_string("2\0a\0b\0");
        assert_eq!(ctx.proc_status(p, a), Ok(2));
    }
    assert_eq!(memory.written("f").as_deref(), Some("x"));
    assert_eq!(mortal(ctx.heap()), 0);
}

// T12

/// 浮動小数の演算と `compare`。特別な値 (±0、±無限大、NaN、非正規化数) を含む組で、Rust の演算と同じビット列を返す。
/// NaN の結果は、Rust の結果の符号と中身によらず、正の quiet NaN (`f64::NAN`) のビット列でなければならない
#[test]
fn float_operations_match_rust() {
    use crate::externs::*;
    let values = [
        0.0,
        -0.0,
        1.5,
        -2.5,
        f64::MIN_POSITIVE,
        5e-324,
        f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -f64::NAN,
    ];
    let same = |got: f64, want: f64| {
        let want = if want.is_nan() { f64::NAN } else { want };
        assert_eq!(got.to_bits(), want.to_bits(), "{got:?} != {want:?}")
    };
    for a in values {
        same(float_negate(a), -a);
        for b in values {
            same(float_add(a, b), a + b);
            same(float_sub(a, b), a - b);
            same(float_mul(a, b), a * b);
            same(float_div(a, b), a / b);
            same(float_rem(a, b), a % b);
            let order = match a.total_cmp(&b) {
                std::cmp::Ordering::Less => LT,
                std::cmp::Ordering::Equal => EQ,
                std::cmp::Ordering::Greater => GT,
            };
            assert_eq!(float_compare(a, b), order, "{a:?} {b:?}");
        }
    }
}

/// 浮動小数の変換、数学の関数、書式、読み。各関数が Rust の `f64` の関数と同じ値 (NaN は `f64::NAN` のビット列) を
/// 返し、書式が Python の `repr` と `f"{x:.{n}f}"` の形で、読みが書式の結果を同じビット列に読み戻すこと
#[test]
fn float_conversions_and_formats_match_rust() {
    use crate::externs::*;
    let values = [
        0.0,
        -0.0,
        0.1,
        2.5,
        -3.5,
        1e16,
        1e-5,
        f64::MIN_POSITIVE,
        5e-324,
        f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -f64::NAN,
    ];
    let same = |got: f64, want: f64| {
        let want = if want.is_nan() { f64::NAN } else { want };
        assert_eq!(got.to_bits(), want.to_bits(), "{got:?} != {want:?}")
    };
    // Miri は libm の関数 (`sin` など) の結果に、決まらない小さな誤差を足す。非正規化数か 0 の結果では、符号も 0 か
    // どうかも変わりうる。そのため、それらの有限の結果は、どちらも非正規化数か 0 の大きさか、相対の誤差が 1e-12 以内
    // かだけを比べる
    let near = |got: f64, want: f64| {
        let tiny = got.abs() < f64::MIN_POSITIVE && want.abs() < f64::MIN_POSITIVE;
        if want.is_nan() || want.is_infinite() {
            same(got, want)
        } else {
            assert!(
                tiny || (got - want).abs() <= want.abs() * 1e-12,
                "{got:?} !~ {want:?}"
            )
        }
    };
    let int = |t: f64, x: f64| {
        if (-9223372036854775808.0..9223372036854775808.0).contains(&t) {
            Ok(t as i64)
        } else {
            Err(ArithFault::FloatToInt(x.to_bits()))
        }
    };
    for x in values {
        same(float_sqrt(x), x.sqrt());
        same(float_abs(x), x.abs());
        near(float_sin(x), x.sin());
        near(float_cos(x), x.cos());
        near(float_exp(x), x.exp());
        near(float_log(x), x.ln());
        for y in values {
            near(float_pow(x, y), x.powf(y));
        }
        assert_eq!(float_truncate(x), int(x.trunc(), x));
        assert_eq!(float_floor(x), int(x.floor(), x));
        assert_eq!(float_ceil(x), int(x.ceil(), x));
        assert_eq!(float_round(x), int(x.round_ties_even(), x));
        let minus = x.is_sign_negative() && !x.is_nan();
        assert_eq!(float_shows_minus(x), if minus { TRUE } else { FALSE });
        let shown = float_repr(x);
        assert_eq!(shown.starts_with('-'), minus, "{shown}");
        let back = float_parse(&shown).unwrap();
        same(back, x);
        assert_eq!(
            float_fixed(3, x),
            if x.is_nan() {
                "nan".to_string()
            } else {
                format!("{x:.3}")
            }
        );
    }
    for i in [0, 1, -1, i64::MAX, i64::MIN, (1 << 53) + 1] {
        same(float_of_int(i), i as f64);
    }
    same(float_infinity(), f64::INFINITY);
    same(float_nan(), f64::NAN);
    // 2 の冪では往復できる区間が下側だけ狭い。正しく丸めた最短の桁が読み戻せない 2 の冪でも、表示を読み戻すと同じ
    // ビット列になる。すべての ±2^k は Miri では遅いので、`externs.rs` の単体テストが流す。`powi` は Miri が誤差を
    // 足すので、2 の冪はビット列から作る
    let pow2 = |k: i32| f64::from_bits(((k + 1023) as u64) << 52);
    for k in super::table::POW2_EXPONENTS {
        for x in [pow2(k), -pow2(k)] {
            let shown = float_repr(x);
            assert_eq!(
                float_parse(&shown).map(f64::to_bits),
                Some(x.to_bits()),
                "{shown}"
            );
        }
    }
    assert_eq!(float_repr(pow2(-1017)), "7.120236347223045e-307");
    assert_eq!(float_repr(pow2(89)), "6.189700196426902e+26");
    assert_eq!(float_repr(1e16), "1e+16");
    assert_eq!(float_repr(1e-5), "1e-05");
    assert_eq!(float_repr(123456.0), "123456.0");
    assert_eq!(float_parse(" 1_0 "), None);
    same(float_parse("-nan").unwrap(), f64::NAN);
}

both_modes!(
    pages_are_aligned_and_returned,
    large_objects_get_their_own_page,
    dropping_a_heap_returns_every_page,
    local_and_shared_counts_move,
    is_last_reads_twice_only_at_minus_one,
    a_foreign_value_of_another_type_is_wrong_kind,
    foreign_values_with_drop_are_finalized_once,
    a_zero_sized_foreign_value,
    a_panicking_finalizer_is_not_run_twice,
    a_foreign_value_written_in_place_is_finalized_once,
    a_panicking_finalizer_in_a_chain_leaks_the_rest,
    strings_of_every_length_are_read_back,
    multibyte_strings_are_read_back,
    unaligned_appends_are_written,
    large_strings_cross_pages,
    empty_strings_are_appended,
    self_concat_under_both_borrow_models,
    appending_the_text_of_another_string,
    reading_then_appending,
    parts_of_a_string_are_copied,
    frozen_pages_are_returned,
    two_threads_read_one_region,
    chains_drop_in_any_order,
    own_objects_read_region_words,
    two_threads_share_one_control,
    println_goes_through_the_host,
    files_go_through_the_host,
    script_externs_go_through_the_host,
);

//! 実行時エラーの後の回復 (docs/spec/object-model.md の「回復」)。根から届く物体を
//! 新しいヒープに写し、古いヒープを丸ごと捨てる。

use std::sync::Arc;

use super::ctx::{UNIT, ctx, ctx_evidence, push};
use super::raw::rc;
use super::{cons, list, mortal, probe};
use crate::{Checked, Ctx, Heap, HeapError, Kind, Meta, Mode, Word};

/// 根の語を読んで、読んだ参照を手放す。
fn root<M: Mode>(ctx: &mut Ctx<M>, i: u32) -> Word {
    unsafe {
        let w = ctx.root_get(i).unwrap();
        ctx.heap_mut().decref(w).unwrap();
        w
    }
}

/// 根をすべて手放す。
fn release_roots<M: Mode>(ctx: &mut Ctx<M>) {
    for w in ctx.take_roots().into_iter().flatten() {
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
}

/// 回復は根から届く物体だけを、同じ中身と共有の形で写す。届かない物体 (token の箱と `Foreign` の物体を含む) は
/// 古いヒープと一緒に消え、`Foreign` の破棄処理はちょうど1回呼ばれ、ページは `Control` に戻る。
fn recovery_keeps_only_what_the_roots_reach<M: Mode>() {
    let mut builder = Heap::<M>::new();
    let constant = builder.alloc_string_immortal("constant");
    let region = Arc::new(builder.freeze().unwrap());
    let mut ctx = ctx::<M>();
    let (value, drops) = probe();
    unsafe {
        ctx.heap_mut().attach(region);
        let heap = ctx.heap_mut();
        // 根から届く物体: 共有した文字列、共有したセル、生の語、走査する語が 255 個以上の物体、BigInt、余りの容量を
        // 持つ文字列、凍結した塊の語、ダミー
        let shared = heap.alloc_string("shared");
        heap.dup(shared).unwrap();
        let a = cons(heap, shared, Word::imm(1));
        heap.dup(a).unwrap();
        let b = cons(heap, shared, a);
        let raw = heap.alloc_data(5, 1, 2, false);
        heap.set_field(raw, 0, Word::imm(9)).unwrap();
        heap.set_raw(raw, 1, 0xdead).unwrap();
        heap.set_raw(raw, 2, 0xbeef).unwrap();
        let wide = heap.alloc_data(0, 300, 0, false);
        for i in 0..300 {
            heap.set_field(wide, 1 + i, Word::imm(i as i64)).unwrap();
        }
        let big = heap.alloc_int(i64::MAX);
        let room = heap.alloc_string_room(64);
        let room = heap.string_push(room, "room").unwrap();
        let dummy = heap.dummy();
        // 届かない物体: 長いリスト、token の箱、`Foreign` の物体
        let garbage = list(heap, 20_000);
        let boxed = cons(heap, Word::imm(0), Word::imm(0));
        let _token = heap.into_token(boxed).unwrap();
        let _foreign = heap.alloc_foreign(value, false);
        let _ = garbage;
        for (i, w) in [b, a, raw, wide, big, room, constant, dummy]
            .into_iter()
            .enumerate()
        {
            ctx.root_set(i as u32, w).unwrap();
        }
    }
    let bytes = ctx.heap().bytes_in_use();
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 0);
    unsafe { ctx.recover() }.unwrap();
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert!(ctx.check_finished());
    assert!(
        ctx.heap().bytes_in_use() < bytes,
        "{} >= {bytes}",
        ctx.heap().bytes_in_use()
    );
    // shared、a、b、raw、wide、big、room の7つだけが残る
    assert_eq!(mortal(ctx.heap()), 7);
    let b = root(&mut ctx, 0);
    let a = root(&mut ctx, 1);
    unsafe {
        let heap = ctx.heap_mut();
        let shared = heap.field(a, 0).unwrap();
        assert_eq!(heap.field(b, 0), Ok(shared));
        assert_eq!(heap.field(b, 1), Ok(a));
        assert_eq!(heap.field(a, 1), Ok(Word::imm(1)));
        assert_eq!(heap.string_text(shared), "shared");
        // shared は a と b のフィールドから、a は根と b のフィールドから持たれる
        assert_eq!(rc(shared), 2);
        assert_eq!(rc(a), 2);
        assert_eq!(rc(b), 1);
    }
    let raw = root(&mut ctx, 2);
    let wide = root(&mut ctx, 3);
    let big = root(&mut ctx, 4);
    let room = root(&mut ctx, 5);
    unsafe {
        let heap = ctx.heap_mut();
        assert_eq!(heap.field(raw, 0), Ok(Word::imm(9)));
        assert_eq!(heap.raw(raw, 1), Ok(0xdead));
        assert_eq!(heap.raw(raw, 2), Ok(0xbeef));
        assert_eq!(heap.scanned_len(wide), Ok(300));
        assert_eq!(heap.field(wide, 300), Ok(Word::imm(299)));
        assert_eq!(heap.unbox_int(big), Ok(i64::MAX));
        assert_eq!(heap.string_text(room), "room");
    }
    // 凍結した塊の語とダミーは、そのまま根に残る
    assert_eq!(root(&mut ctx, 6), constant);
    assert_eq!(root(&mut ctx, 7), ctx.heap().dummy());
    release_roots(&mut ctx);
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 回復は `Float` の箱を、同じビット列の新しい箱に写す。
fn recovery_copies_floats<M: Mode>() {
    let mut ctx = ctx::<M>();
    let bits = [1.5f64.to_bits(), (-0.0f64).to_bits(), 0x7ff8_0000_0000_0001];
    unsafe {
        let heap = ctx.heap_mut();
        let ws: Vec<Word> = bits.iter().map(|&b| heap.alloc_float(b)).collect();
        for (i, w) in ws.into_iter().enumerate() {
            ctx.root_set(i as u32, w).unwrap();
        }
        ctx.recover().unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 3);
    for (i, &b) in bits.iter().enumerate() {
        let w = root(&mut ctx, i as u32);
        unsafe {
            assert_eq!(ctx.heap().meta(w).unwrap().kind(), Kind::Float);
            assert_eq!(ctx.heap().unbox_float(w), Ok(b));
        }
    }
    release_roots(&mut ctx);
    assert_eq!(mortal(ctx.heap()), 0);
}

/// evv と yield の状態が残っていても、回復は空に戻し、それらの物体も消す。
fn recovery_empties_the_evv_and_the_yield_state<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        ctx.root_set(0, Word::imm(1)).unwrap();
        let ev = push(&mut ctx, 0, 1);
        let target = ctx_evidence(&mut ctx);
        ctx.yield_to(target, UNIT, Word::imm(1)).unwrap();
        ctx.heap_mut().decref(ev).unwrap();
    }
    assert!(!ctx.check_finished());
    assert!(mortal(ctx.heap()) > 0);
    unsafe { ctx.recover() }.unwrap();
    assert!(ctx.check_finished());
    assert_eq!(ctx.yielding(), 0);
    assert_eq!(mortal(ctx.heap()), 0);
    unsafe {
        assert_eq!(ctx.root_get(0), Ok(Word::imm(1)));
        push(&mut ctx, 0, 1);
        ctx.evv_set(UNIT).unwrap();
    }
    assert!(ctx.check_finished());
}

/// 写せない物体に根が届けば、回復は `Unrecoverable` を返し、根とヒープの物体を変えない。
fn recovery_refuses_what_it_cannot_copy<M: Mode>() {
    let cases: [fn(&mut Heap<M>) -> Word; 3] = [
        |heap| heap.alloc_foreign(5u64, false),
        |heap| heap.alloc_data(0, 0, 1, true),
        |heap| unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) },
    ];
    for make in cases {
        let mut ctx = ctx::<M>();
        let w = unsafe {
            let heap = ctx.heap_mut();
            let w = make(heap);
            let holder = cons(heap, w, Word::imm(0));
            ctx.root_set(0, holder).unwrap();
            holder
        };
        let before = mortal(ctx.heap());
        assert_eq!(unsafe { ctx.recover() }, Err(HeapError::Unrecoverable));
        assert_eq!(mortal(ctx.heap()), before);
        assert_eq!(root(&mut ctx, 0), w);
    }
}

/// 共有の物体 (rc が負) も、回復は局所の物体として写す。すべてのタスクを捨てるので、写しを持つのは根と写した親だけで
/// あり、2つ目からの使い手は `dup` する (docs/spec/object-model.md の「回復」)。
fn recovery_copies_shared_objects_as_local<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let heap = ctx.heap_mut();
        let x = cons(heap, Word::imm(7), Word::imm(8));
        heap.share(x, 1).unwrap();
        heap.dup(x).unwrap();
        assert_eq!(rc(x), -2);
        let both = cons(heap, x, x);
        let y = list(heap, 2);
        heap.share(y, 1).unwrap();
        let holder = cons(heap, both, y);
        ctx.root_set(0, holder).unwrap();
        ctx.recover().unwrap();
    }
    // holder、both、x の写し、y の2つのセルだけが残る
    assert_eq!(mortal(ctx.heap()), 5);
    let holder = root(&mut ctx, 0);
    unsafe {
        let heap = ctx.heap_mut();
        let both = heap.field(holder, 0).unwrap();
        let y = heap.field(holder, 1).unwrap();
        let x = heap.field(both, 0).unwrap();
        assert_eq!(heap.field(both, 1), Ok(x));
        assert_eq!(rc(x), 2);
        assert_eq!(heap.is_unique(x), Ok(false));
        assert_eq!(heap.field(x, 0), Ok(Word::imm(7)));
        assert_eq!(heap.field(x, 1), Ok(Word::imm(8)));
        let tail = heap.field(y, 1).unwrap();
        for (cell, head) in [(y, 1), (tail, 0)] {
            assert!(rc(cell) > 0);
            assert_eq!(heap.is_unique(cell), Ok(true));
            assert_eq!(heap.field(cell, 0), Ok(Word::imm(head)));
        }
        assert_eq!(heap.field(tail, 1), Ok(Word::imm(0)));
        for w in [holder, both] {
            assert_eq!(heap.is_unique(w), Ok(true));
        }
    }
    unsafe { ctx.heap_mut().decref(holder) }.unwrap();
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 共有の印がある `lin` の物体は、rc が負でも写せない。
fn recovery_still_refuses_a_lin_object_at_a_negative_rc<M: Mode>() {
    let mut ctx = ctx::<M>();
    let w = unsafe {
        let heap = ctx.heap_mut();
        let w = heap.alloc_data(0, 0, 1, true);
        super::raw::set_rc(heap, w, -1);
        let holder = cons(heap, w, Word::imm(0));
        ctx.root_set(0, holder).unwrap();
        holder
    };
    let before = mortal(ctx.heap());
    assert_eq!(unsafe { ctx.recover() }, Err(HeapError::Unrecoverable));
    assert_eq!(mortal(ctx.heap()), before);
    assert_eq!(root(&mut ctx, 0), w);
}

/// 回復が一時に使うメモリは、根から届く物体の数で決まり、古いヒープの大きさによらない。回復は確保量の上限で止まった
/// 直後にも走るので、古いヒープに比例するメモリを足すと、上限が機械を使い切る前に止める意味がなくなる
/// (docs/spec/object-model.md の「回復」)。根がない `eml run` も同じである。
fn recovery_needs_no_memory_in_proportion_to_the_old_heap<M: Mode>() {
    for rooted in [true, false] {
        let mut ctx = ctx::<M>();
        unsafe {
            let heap = ctx.heap_mut();
            let _garbage = list(heap, 1_000_000);
            if rooted {
                let kept = list(heap, 10);
                ctx.root_set(0, kept).unwrap();
            }
        }
        let peak = counting::peak_during(|| unsafe { ctx.recover() }.unwrap());
        assert!(peak < 1 << 20, "{peak} bytes (rooted = {rooted})");
        release_roots(&mut ctx);
    }
}

/// このスレッドが確保しているバイト数を数える確保器。並んで走るテストの確保を混ぜないよう、スレッドごとに数える。
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct Counting;

    thread_local! {
        static CURRENT: Cell<isize> = const { Cell::new(0) };
        static PEAK: Cell<isize> = const { Cell::new(0) };
    }

    fn add(n: isize) {
        let _ = CURRENT.try_with(|current| {
            let now = current.get() + n;
            current.set(now);
            let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
        });
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            add(layout.size() as isize);
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            add(-(layout.size() as isize));
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    /// `f` の間に、始めの量を超えて確保した量の最大。
    pub(super) fn peak_during(f: impl FnOnce()) -> usize {
        let start = CURRENT.with(Cell::get);
        PEAK.with(|peak| peak.set(start));
        f();
        (PEAK.with(Cell::get) - start) as usize
    }
}

both_modes!(
    recovery_keeps_only_what_the_roots_reach,
    recovery_empties_the_evv_and_the_yield_state,
    recovery_copies_floats,
    recovery_refuses_what_it_cannot_copy,
    recovery_copies_shared_objects_as_local,
    recovery_still_refuses_a_lin_object_at_a_negative_rc,
    recovery_needs_no_memory_in_proportion_to_the_old_heap,
);

/// 検査モードでは、空けた物体と token の箱に届く根も断る。速いモードでは、根が生きていない物体を指すこと自体が
/// 前提に反する。
#[test]
fn checked_recovery_refuses_freed_objects_and_token_boxes() {
    for token in [false, true] {
        let mut ctx = ctx::<Checked>();
        unsafe {
            let heap = ctx.heap_mut();
            let x = cons(heap, Word::imm(0), Word::imm(0));
            if token {
                let _ = heap.into_token(x).unwrap();
            } else {
                heap.decref(x).unwrap();
            }
            ctx.root_set(0, x).unwrap();
        }
        assert_eq!(unsafe { ctx.recover() }, Err(HeapError::Unrecoverable));
        ctx.take_roots();
    }
}

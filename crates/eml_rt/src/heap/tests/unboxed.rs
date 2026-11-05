//! 生の要素の配列の物体 `Raw` (docs/spec/object-model.md の「種類」) と、それを受ける `Ctx` の操作
//! (docs/spec/object-model.md の「配列」)。

use std::sync::Arc;

use super::ctx::ctx;
use super::host::host;
use super::mortal;
use super::raw::rc;
use crate::externs::{Fault, IndexFault};
use crate::{Checked, Control, Ctx, Heap, HeapError, Kind, Limits, Mode, Stop, Word};

/// 作った物体の語 0 は要素の数で、`scan` と tag は 0 である。要素はどのビット列でも書いて読める。
fn unboxed_words_round_trip<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_unboxed(3, 7, false);
    let bits = [7, u64::MAX, (-0.0f64).to_bits()];
    unsafe {
        let m = heap.meta(w).unwrap();
        assert_eq!(
            (m.kind(), m.scan(), m.tag(), m.lin()),
            (Kind::Raw, 0, 0, false)
        );
        assert_eq!(heap.unboxed_len(w), Ok(3));
        for i in 0..3 {
            assert_eq!(heap.unboxed_word(w, i), Ok(7));
        }
        heap.unboxed_put(w, 1, bits[1]).unwrap();
        heap.unboxed_put(w, 2, bits[2]).unwrap();
        assert_eq!(heap.unboxed_len(w), Ok(3));
        for (i, &b) in bits.iter().enumerate() {
            assert_eq!(heap.unboxed_word(w, i), Ok(b));
        }
        heap.decref(w).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

fn an_empty_unboxed_array_has_length_zero<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_unboxed(0, 1, true);
    unsafe {
        assert_eq!(heap.unboxed_len(w), Ok(0));
        assert!(heap.meta(w).unwrap().lin());
        heap.decref(w).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

/// `unboxed_copy` は写す元の先頭の n 要素を写し、写す先の残りの要素と写す元を変えない。
fn unboxed_copy_copies_the_first_elements<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let src = heap.alloc_unboxed(3, 0, false);
    let dst = heap.alloc_unboxed(4, 9, false);
    unsafe {
        for i in 0..3 {
            heap.unboxed_put(src, i, i as u64 + 1).unwrap();
        }
        heap.unboxed_copy(dst, src, 2).unwrap();
        let read = |heap: &Heap<M>, w, n| -> Vec<u64> {
            (0..n).map(|i| heap.unboxed_word(w, i).unwrap()).collect()
        };
        assert_eq!(read(&heap, dst, 4), [1, 2, 9, 9]);
        assert_eq!(read(&heap, src, 3), [1, 2, 3]);
        heap.decref(src).unwrap();
        heap.decref(dst).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

/// `Raw` は走査する語を持たないので、`share` は rc を負にするだけで、要素はそのまま読める。
fn a_shared_unboxed_array_can_still_be_read<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_unboxed(2, 5, false);
    unsafe {
        heap.share(w, 1).unwrap();
        assert_eq!(rc(w), -1);
        assert_eq!(heap.unboxed_word(w, 1), Ok(5));
        heap.decref(w).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

/// `Raw` は破棄処理を持たないので、`lin` ビットがあっても最後の `decref` で空く。
fn a_lin_unboxed_array_is_freed_by_its_last_decref<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_unboxed(4, 0, true);
    assert_eq!(mortal(&mut heap), 1);
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(mortal(&mut heap), 0);
}

/// 一意な `Raw` の `lin` ビットは、写さずに変えられる。
fn set_lin_relinks_a_unique_unboxed_array<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_unboxed(1, 3, false);
    unsafe {
        heap.set_lin(w, true).unwrap();
        assert!(heap.meta(w).unwrap().lin());
        heap.set_lin(w, false).unwrap();
        let m = heap.meta(w).unwrap();
        assert_eq!((m.kind(), m.lin()), (Kind::Raw, false));
        assert_eq!(heap.unboxed_word(w, 0), Ok(3));
        heap.decref(w).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

fn lin_of<M: Mode>(ctx: &Ctx<M>, w: Word) -> bool {
    unsafe { ctx.heap().meta(w) }.unwrap().lin()
}

fn elements<M: Mode>(ctx: &mut Ctx<M>, w: Word) -> Vec<u64> {
    unsafe {
        let n = ctx.unboxed_length(w).unwrap();
        (0..n).map(|i| ctx.unboxed_get(w, i).unwrap()).collect()
    }
}

fn release<M: Mode>(ctx: &mut Ctx<M>, w: Word) {
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
}

/// 一意な配列の `set` と `fill` は、その場で書いて同じ物体を返す。
fn unboxed_make_get_set_round_trip<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.unboxed_make(3, 7, false).unwrap();
        let b = ctx.unboxed_set(a, 1, u64::MAX).unwrap();
        assert_eq!(b, a);
        assert_eq!(elements(&mut ctx, b), [7, u64::MAX, 7]);
        let m = ctx.unboxed_make(2, 0, true).unwrap();
        let f = ctx.unboxed_fill(m, 5).unwrap();
        assert_eq!(f, m);
        assert!(lin_of(&ctx, f));
        assert_eq!(elements(&mut ctx, f), [5, 5]);
        release(&mut ctx, b);
        release(&mut ctx, f);
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 共有の配列の `set` と `thaw` は、`lin` ビットを決めて写し、元の配列を変えない。
fn set_and_thaw_copy_a_shared_unboxed_array<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.unboxed_make(2, 1, false).unwrap();
        ctx.heap_mut().dup(a).unwrap();
        let b = ctx.unboxed_set(a, 0, 9).unwrap();
        assert_ne!(b, a);
        assert!(!lin_of(&ctx, b));
        assert_eq!(elements(&mut ctx, b), [9, 1]);
        assert_eq!(elements(&mut ctx, a), [1, 1]);
        ctx.heap_mut().dup(a).unwrap();
        let m = ctx.unboxed_thaw(a).unwrap();
        assert_ne!(m, a);
        assert!(lin_of(&ctx, m) && !lin_of(&ctx, a));
        assert_eq!(elements(&mut ctx, m), [1, 1]);
        for w in [a, b, m] {
            release(&mut ctx, w);
        }
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 一意な配列の `freeze` と `thaw` は、写さずに `lin` ビットだけを変える。
fn freeze_and_thaw_flip_the_lin_bit_of_an_unboxed_array<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let m = ctx.unboxed_empty(true).unwrap();
        let a = ctx.unboxed_freeze(m).unwrap();
        assert_eq!(a, m);
        assert!(!lin_of(&ctx, a));
        let m = ctx.unboxed_thaw(a).unwrap();
        assert_eq!(m, a);
        assert!(lin_of(&ctx, m));
        assert_eq!(ctx.unboxed_length(m), Ok(0));
        release(&mut ctx, m);
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// `MutUnboxedArray` はいつも一意なので、共有の物体の `fill` と `freeze` は検査モードでは誤りである。速いモードは
/// 写す。`fill` の写しは元の `lin` ビットを引き継ぐ。検査モードは `lin` の物体の `dup` を断るので、検査モードの共有の
/// 物体は `lin` を持たない。
fn filling_and_freezing_a_shared_unboxed_array<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.unboxed_make(2, 3, false).unwrap();
        for _ in 0..2 {
            ctx.heap_mut().dup(a).unwrap();
        }
        let not_unique = Err(Fault::Heap(HeapError::NotUnique));
        if M::CHECKED {
            assert_eq!(ctx.unboxed_fill(a, 4), not_unique);
            assert_eq!(ctx.unboxed_freeze(a), not_unique);
            for _ in 0..2 {
                release(&mut ctx, a);
            }
        } else {
            let f = ctx.unboxed_fill(a, 4).unwrap();
            assert_ne!(f, a);
            assert!(!lin_of(&ctx, f));
            assert_eq!(elements(&mut ctx, f), [4, 4]);
            let b = ctx.unboxed_freeze(a).unwrap();
            assert_ne!(b, a);
            assert!(!lin_of(&ctx, b));
            assert_eq!(elements(&mut ctx, b), [3, 3]);
            release(&mut ctx, f);
            release(&mut ctx, b);
            // 速いモードは `lin` の物体も `dup` できる。`fill` の写しは `lin` ビットを引き継ぎ、`freeze` の写しは下ろす
            let m = ctx.unboxed_make(2, 5, true).unwrap();
            for _ in 0..2 {
                ctx.heap_mut().dup(m).unwrap();
            }
            let f = ctx.unboxed_fill(m, 6).unwrap();
            assert_ne!(f, m);
            assert!(lin_of(&ctx, f));
            assert_eq!(elements(&mut ctx, f), [6, 6]);
            let b = ctx.unboxed_freeze(m).unwrap();
            assert_ne!(b, m);
            assert!(!lin_of(&ctx, b));
            assert_eq!(elements(&mut ctx, b), [5, 5]);
            assert!(lin_of(&ctx, m));
            assert_eq!(elements(&mut ctx, m), [5, 5]);
            for w in [f, b, m] {
                release(&mut ctx, w);
            }
        }
        assert_eq!(elements(&mut ctx, a), [3, 3]);
        release(&mut ctx, a);
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 範囲の外の `set` は配列を手放し、負の長さの `make` は何も作らない。
fn unboxed_index_faults_release_the_array<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.unboxed_make(2, 0, false).unwrap();
        let fault = |index| Fault::Index(Box::new(IndexFault::Array { index, len: 2 }));
        assert_eq!(ctx.unboxed_get(a, 2), Err(fault(2)));
        assert_eq!(ctx.unboxed_set(a, -1, 1), Err(fault(-1)));
        assert_eq!(mortal(&mut ctx), 0);
        assert_eq!(
            ctx.unboxed_make(-3, 0, true),
            Err(Fault::Index(Box::new(IndexFault::Length { len: -3 })))
        );
    }
    assert_eq!(mortal(&mut ctx), 0);
}

/// 確保器が作れない長さと、確保量の上限を超える長さの `make` は、何も確保せずに止まる。
fn oversized_unboxed_make_stops_before_allocating<M: Mode>() {
    let stopped = Err(Fault::Stopped(Stop::MemoryLimit));
    let mut ctx = ctx::<M>();
    assert_eq!(ctx.unboxed_make(i64::MAX, 0, false), stopped);
    assert_eq!(mortal(&mut ctx), 0);
    let limits = Limits {
        max_bytes: 1 << 20,
        ..Limits::default()
    };
    let mut ctx = Ctx::<M>::new(Arc::new(Control::new(limits)), host(&Arc::default()));
    assert_eq!(ctx.unboxed_make((1 << 20) / 8 + 1, 0, true), stopped);
    assert_eq!(mortal(&mut ctx), 0);
    let a = ctx.unboxed_make(10, 1, false).unwrap();
    release(&mut ctx, a);
    assert_eq!(mortal(&mut ctx), 0);
}

both_modes!(
    unboxed_words_round_trip,
    an_empty_unboxed_array_has_length_zero,
    unboxed_copy_copies_the_first_elements,
    a_shared_unboxed_array_can_still_be_read,
    a_lin_unboxed_array_is_freed_by_its_last_decref,
    set_lin_relinks_a_unique_unboxed_array,
    unboxed_make_get_set_round_trip,
    set_and_thaw_copy_a_shared_unboxed_array,
    freeze_and_thaw_flip_the_lin_bit_of_an_unboxed_array,
    filling_and_freezing_a_shared_unboxed_array,
    unboxed_index_faults_release_the_array,
    oversized_unboxed_make_stops_before_allocating,
);

fn wrong_kind<T>() -> Result<T, Fault> {
    Err(Fault::Heap(HeapError::WrongKind))
}

/// 検査モードの `unboxed_*` は、`Raw` でない物体と即値を `WrongKind` で断り、何も変えない。
#[test]
fn checked_unboxed_externs_refuse_other_kinds() {
    let mut ctx = ctx::<Checked>();
    unsafe {
        let arr = ctx.array_make(1, Word::imm(0), false).unwrap();
        for w in [arr, Word::imm(3)] {
            assert_eq!(ctx.unboxed_length(w), wrong_kind());
            assert_eq!(ctx.unboxed_get(w, 0), wrong_kind());
            assert_eq!(ctx.unboxed_set(w, 0, 1), wrong_kind());
            assert_eq!(ctx.unboxed_fill(w, 1), wrong_kind());
            assert_eq!(ctx.unboxed_thaw(w), wrong_kind());
            assert_eq!(ctx.unboxed_freeze(w), wrong_kind());
        }
        assert_eq!(ctx.array_length(arr), Ok(1));
        release(&mut ctx, arr);
    }
    assert_eq!(mortal(&mut ctx), 0);
}

#[test]
fn checked_unboxed_operations_refuse_an_index_past_the_length() {
    let mut heap = Heap::<Checked>::new();
    let w = heap.alloc_unboxed(2, 0, false);
    let src = heap.alloc_unboxed(1, 0, false);
    unsafe {
        assert_eq!(heap.unboxed_word(w, 2), Err(HeapError::OutOfBounds));
        assert_eq!(heap.unboxed_put(w, 2, 1), Err(HeapError::OutOfBounds));
        assert_eq!(heap.unboxed_copy(w, src, 2), Err(HeapError::OutOfBounds));
        assert_eq!(heap.unboxed_copy(src, w, 2), Err(HeapError::OutOfBounds));
        assert_eq!(heap.unboxed_word(w, 1), Ok(0));
        heap.decref(w).unwrap();
        heap.decref(src).unwrap();
    }
}

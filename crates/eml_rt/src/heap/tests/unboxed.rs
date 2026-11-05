//! 生の要素の配列の物体 `Raw` (docs/spec/object-model.md の「種類」)。

use super::mortal;
use super::raw::rc;
use crate::{Checked, Heap, HeapError, Kind, Mode};

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

both_modes!(
    unboxed_words_round_trip,
    an_empty_unboxed_array_has_length_zero,
    unboxed_copy_copies_the_first_elements,
    a_shared_unboxed_array_can_still_be_read,
    a_lin_unboxed_array_is_freed_by_its_last_decref,
    set_lin_relinks_a_unique_unboxed_array,
);

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

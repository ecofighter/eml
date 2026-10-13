use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{LONG, cons, list, mortal, probe};
use crate::{Fast, Heap, HeapError, Kind, Meta, Mode, Word};

fn a_cons_list_is_freed_by_one_decref<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, 3);
    assert_eq!(mortal(&heap), 3);
    unsafe { heap.decref(l) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn a_dupped_object_survives_one_decref<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = list(&mut heap, 1);
    unsafe {
        heap.dup(x).unwrap();
        assert!(!heap.is_unique(x).unwrap());
        heap.decref(x).unwrap();
        assert!(heap.is_unique(x).unwrap());
        assert_eq!(mortal(&heap), 1);
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_long_list_with_boxed_elements_frees_with_a_short_worklist<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut l = Word::imm(0);
    for i in 0..LONG {
        let pair = heap.alloc_data(0, 2, 0, false);
        unsafe {
            heap.set_field(pair, 0, Word::imm(i as i64)).unwrap();
            heap.set_field(pair, 1, Word::imm(i as i64)).unwrap();
        }
        l = cons(&mut heap, pair, l);
    }
    unsafe { heap.decref(l) }.unwrap();
    assert_eq!(mortal(&heap), 0);
    // 要素を残りのリストより先に空けるので、作業リストは要素1つと残りの1つで足りる
    assert!(heap.work.capacity() <= 4, "{}", heap.work.capacity());
}

fn a_deep_chain_does_not_overflow_the_stack<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut x = Word::imm(0);
    for _ in 0..LONG {
        let cell = heap.alloc_data(0, 1, 0, false);
        unsafe { heap.set_field(cell, 0, x) }.unwrap();
        x = cell;
    }
    unsafe { heap.decref(x) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn a_big_int_is_boxed_only_outside_63_bits<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let small = heap.alloc_int(5);
    let big = heap.alloc_int(i64::MAX);
    assert!(small.is_imm());
    assert!(big.is_ptr());
    unsafe {
        assert_eq!(heap.unbox_int(small), Ok(5));
        assert_eq!(heap.unbox_int(big), Ok(i64::MAX));
        assert_eq!(heap.meta(big).unwrap().kind(), Kind::BigInt);
        heap.decref(big).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn the_dummy_ignores_dup_and_decref_and_unboxes_to_zero<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let dummy = heap.dummy();
    unsafe {
        for _ in 0..3 {
            heap.dup(dummy).unwrap();
        }
        for _ in 0..5 {
            heap.decref(dummy).unwrap();
        }
        assert_eq!(heap.unbox_int(dummy), Ok(0));
    }
    assert_eq!(heap.live_objects(), vec![dummy]);
}

fn an_extended_scan_object_frees_all_its_children<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let n = 300;
    let x = heap.alloc_data(0, n, 1, false);
    unsafe {
        assert_eq!(heap.meta(x).unwrap().scan(), Meta::EXTENDED_SCAN);
        assert_eq!(heap.scanned_len(x), Ok(n));
        for i in 0..n {
            // 物理的な添字は、数の語の次から始まる
            let child = list(&mut heap, 1);
            heap.set_field(x, 1 + i, child).unwrap();
            assert_eq!(heap.scanned(x, i), Ok(child));
        }
        heap.set_raw(x, 1 + n, 7).unwrap();
        assert_eq!(heap.raw(x, 1 + n), Ok(7));
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_foreign_value_is_finalized_exactly_once<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let (value, drops) = probe();
    let f = heap.alloc_foreign(value, true);
    let holder = heap.alloc_data(0, 1, 0, false);
    unsafe {
        assert!(heap.meta(f).unwrap().lin());
        heap.set_field(holder, 0, f).unwrap();
        heap.decref(holder).unwrap();
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

fn dropping_the_heap_finalizes_live_foreign_values<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let (value, drops) = probe();
    heap.alloc_foreign(value, false);
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

fn a_panicking_finalizer_runs_once<M: Mode>() {
    struct Panicker(Arc<AtomicUsize>);
    impl Drop for Panicker {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("the finalizer panics");
        }
    }
    let mut heap = Heap::<M>::new();
    let drops = Arc::new(AtomicUsize::new(0));
    let f = heap.alloc_foreign(Panicker(drops.clone()), false);
    let result = catch_unwind(AssertUnwindSafe(|| unsafe { heap.decref(f) }));
    assert!(result.is_err());
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

fn a_zero_sized_foreign_value_fits<M: Mode>() {
    struct Nothing;
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(Nothing, false);
    unsafe {
        assert!(heap.foreign::<Nothing>(f).is_ok());
        assert_eq!(heap.foreign::<u8>(f), Err(HeapError::WrongKind));
        heap.decref(f).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn is_last_and_free_block_free_a_known_shape<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = heap.alloc_data(0, 0, 1, false);
    unsafe {
        assert!(heap.is_last(x).unwrap());
        heap.dup(x).unwrap();
        assert!(!heap.is_last(x).unwrap());
        heap.decref(x).unwrap();
        heap.free_block(x).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn bytes_in_use_come_back_after_a_large_object_dies<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let before = heap.bytes_in_use();
    let x = heap.alloc_data(0, 0, 500, false);
    assert!(heap.bytes_in_use() > before);
    unsafe { heap.decref(x) }.unwrap();
    assert_eq!(heap.bytes_in_use(), before);
}

fn immortal_objects_are_never_freed<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) };
    unsafe {
        heap.acquire_immortal(x).unwrap();
        heap.dup(x).unwrap();
        heap.decref(x).unwrap();
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&heap), 1);
}

fn the_boundary_between_small_and_large_works_in_both_modes<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // 127 語のペイロードとヘッダで 128 語。検査モードでは前置きを足して大きい物体の道に移る
    for raw in [126, 127, 128] {
        let x = heap.alloc_data(0, 0, raw, false);
        unsafe {
            heap.set_raw(x, raw - 1, 9).unwrap();
            assert_eq!(heap.raw(x, raw - 1), Ok(9));
        }
        assert_eq!(mortal(&heap), 1);
        unsafe { heap.decref(x) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn mixed_sizes_keep_the_free_lists_apart<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut live: Vec<(Word, u64)> = Vec::new();
    let mut seed: u64 = 1;
    let rounds = if cfg!(miri) { 200 } else { 20_000 };
    for id in 0..rounds {
        // 線形合同法で大きさと、確保か解放かを決める
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        if seed >> 62 == 0 && !live.is_empty() {
            let (w, _) = live.swap_remove((seed >> 8) as usize % live.len());
            unsafe { heap.decref(w) }.unwrap();
        } else {
            let raw = 1 + (seed >> 40) as usize % 40;
            let w = heap.alloc_data(0, 0, raw, false);
            unsafe { heap.set_raw(w, 0, id) }.unwrap();
            live.push((w, id));
        }
    }
    assert_eq!(mortal(&heap), live.len());
    for (w, id) in live {
        assert_eq!(unsafe { heap.raw(w, 0) }, Ok(id));
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn walking_counts_objects_across_pages<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let objects: Vec<Word> = (0..2000)
        .map(|_| heap.alloc_data(0, 0, 15, false))
        .collect();
    assert_eq!(mortal(&heap), 2000);
    for w in objects {
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

both_modes!(
    a_cons_list_is_freed_by_one_decref,
    a_dupped_object_survives_one_decref,
    a_long_list_with_boxed_elements_frees_with_a_short_worklist,
    a_deep_chain_does_not_overflow_the_stack,
    a_big_int_is_boxed_only_outside_63_bits,
    the_dummy_ignores_dup_and_decref_and_unboxes_to_zero,
    an_extended_scan_object_frees_all_its_children,
    a_foreign_value_is_finalized_exactly_once,
    dropping_the_heap_finalizes_live_foreign_values,
    a_panicking_finalizer_runs_once,
    a_zero_sized_foreign_value_fits,
    is_last_and_free_block_free_a_known_shape,
    bytes_in_use_come_back_after_a_large_object_dies,
    immortal_objects_are_never_freed,
    the_boundary_between_small_and_large_works_in_both_modes,
    mixed_sizes_keep_the_free_lists_apart,
    walking_counts_objects_across_pages,
);

#[test]
fn the_fast_heap_reuses_a_freed_block() {
    let mut heap = Heap::<Fast>::new();
    let a = heap.alloc_data(0, 0, 1, false);
    unsafe { heap.decref(a) }.unwrap();
    assert_eq!(heap.alloc_data(0, 0, 1, false), a);
}

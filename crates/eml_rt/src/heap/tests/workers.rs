//! 論理的な worker ID の割り当てと、ID ごとの数 (docs/spec/object-model.md の「論理的な worker ID」)。

use super::{CONS, cons, list, mortal, raw};
use crate::{Checked, Heap, HeapError, Kind, Meta, Mode, Word};

type H = Heap<Checked>;

const CONS_META: Meta = Meta::new(CONS, 2, Kind::Data, false);

/// 数が `expected` のとおりで、数え直しも通る。
fn assert_owned(heap: &H, expected: &[(u32, u64)]) {
    for &(id, n) in expected {
        assert_eq!(heap.owned_by(id), n, "worker {id}");
    }
    assert_eq!(heap.check_invariants(), Ok(()));
}

#[test]
fn the_size_of_heap_error_stays_at_twelve_bytes() {
    assert_eq!(size_of::<HeapError>(), 12);
}

#[test]
fn counts_follow_allocation_and_release() {
    let mut heap = H::new();
    assert_owned(&heap, &[(0, 0)]);
    let l = list(&mut heap, 3);
    assert_owned(&heap, &[(0, 3)]);
    let s = heap.alloc_string("a");
    assert_owned(&heap, &[(0, 4)]);
    unsafe {
        heap.decref(l).unwrap();
        assert_owned(&heap, &[(0, 1)]);
        heap.decref(s).unwrap();
    }
    assert_owned(&heap, &[(0, 0)]);
}

#[test]
fn immortal_objects_are_not_counted() {
    let mut heap = H::new();
    let w = heap.alloc_string_immortal("literal");
    let _ = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) };
    assert_owned(&heap, &[(0, 0)]);
    let _ = w;
}

#[test]
fn a_string_that_grows_by_copying_keeps_its_count() {
    let mut heap = H::new();
    let mut w = heap.alloc_string("a");
    for _ in 0..8 {
        w = unsafe { heap.string_push(w, "0123456789abcdef") }.unwrap();
        assert_owned(&heap, &[(0, 1)]);
    }
    unsafe { heap.decref(w) }.unwrap();
    assert_owned(&heap, &[(0, 0)]);
}

#[test]
fn objects_count_for_the_worker_that_allocated_them() {
    let mut heap = H::new();
    let a = list(&mut heap, 1);
    heap.set_worker(3);
    let b = list(&mut heap, 2);
    assert_owned(&heap, &[(0, 1), (3, 2)]);
    unsafe {
        heap.decref(b).unwrap();
        heap.set_worker(0);
        heap.decref(a).unwrap();
    }
    assert_owned(&heap, &[(0, 0), (3, 0)]);
}

/// token の間は持ち主のまま数え、使い回すと今の worker の数に移る。
#[test]
fn a_token_round_trip_moves_the_count_to_the_rebuilding_worker() {
    let mut heap = H::new();
    let cell = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        let token = heap.release_reuse(cell, CONS, &[false, false]).unwrap();
        assert_owned(&heap, &[(0, 1)]);
        heap.set_worker(2);
        let (q, reused) = heap.con_at(token, 2, CONS_META).unwrap();
        assert!(reused);
        heap.set_field(q, 0, Word::UNIT).unwrap();
        heap.set_field(q, 1, Word::UNIT).unwrap();
        assert_owned(&heap, &[(0, 0), (2, 1)]);
        let token = heap.release_reuse(q, CONS, &[false, false]).unwrap();
        heap.set_worker(0);
        let q = heap.reuse_in_place(token, CONS_META).unwrap();
        assert_owned(&heap, &[(0, 1), (2, 0)]);
        let token = heap.into_token(q).unwrap();
        assert_owned(&heap, &[(0, 1)]);
        heap.drop_token(token).unwrap();
    }
    assert_owned(&heap, &[(0, 0), (2, 0)]);
}

/// 印を付けた物体は数から外れ、`lin` の物体は受け手の数に移る。
#[test]
fn share_takes_marked_objects_out_and_moves_lin_objects() {
    let mut heap = H::new();
    let child = list(&mut heap, 2);
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        heap.set_field(lin, 0, child).unwrap();
        assert_owned(&heap, &[(0, 3)]);
        heap.share(lin, 7).unwrap();
        assert_owned(&heap, &[(0, 0), (7, 1)]);
        heap.set_worker(7);
        heap.decref(lin).unwrap();
    }
    assert_owned(&heap, &[(0, 0), (7, 0)]);
    assert_eq!(mortal(&heap), 0);
}

#[test]
fn a_count_that_disagrees_with_the_heap_is_reported() {
    let mut heap = H::new();
    let _ = list(&mut heap, 1);
    heap.set_worker(4);
    let w = list(&mut heap, 1);
    unsafe {
        // 数を通さずに共有の印を付ける
        raw::set_rc(&mut heap, w, -1);
        assert_eq!(
            heap.check_invariants(),
            Err(HeapError::OwnedMiscount { worker: 4 })
        );
        raw::set_rc(&mut heap, w, 1);
    }
    assert_eq!(heap.check_invariants(), Ok(()));
}

#[test]
fn a_miscount_past_the_table_is_reported() {
    let mut heap = H::new();
    let w = list(&mut heap, 1);
    unsafe {
        heap.share(w, 0).unwrap();
        // 表にまだない ID の物体を、数を通さずに局所に戻す
        raw::set_rc(&mut heap, w, 1);
        raw::set_prefix_first(&mut heap, w, 9);
    }
    assert_eq!(
        heap.check_invariants(),
        Err(HeapError::OwnedMiscount { worker: 9 })
    );
}

fn a_retired_worker_without_objects_is_ok<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    heap.set_worker(child);
    let l = list(&mut heap, 3);
    unsafe { heap.decref(l) }.unwrap();
    heap.set_worker(0);
    assert_eq!(heap.retire_worker(child), Ok(()));
}

fn a_retired_worker_with_a_live_object_leaks_in_checked_mode<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    heap.set_worker(child);
    let kept = list(&mut heap, 1);
    let gone = list(&mut heap, 2);
    unsafe { heap.decref(gone) }.unwrap();
    heap.set_worker(0);
    let r = heap.retire_worker(child);
    if M::CHECKED {
        assert_eq!(
            r,
            Err(HeapError::TransferLeak {
                worker: child,
                live: 1
            })
        );
        assert_eq!(
            r.unwrap_err().to_string(),
            format!("1 objects of worker {child} are left after its join")
        );
    } else {
        assert_eq!(r, Ok(()));
    }
    let _ = kept;
}

fn fast_mode_hands_out_the_base_id<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let ids = [heap.fork_worker(), heap.fork_worker()];
    if M::CHECKED {
        assert_eq!(ids, [1, 2]);
    } else {
        assert_eq!(ids, [0, 0]);
    }
}

both_modes!(
    a_retired_worker_without_objects_is_ok,
    a_retired_worker_with_a_live_object_leaks_in_checked_mode,
    fast_mode_hands_out_the_base_id,
);

#[test]
fn the_most_recently_retired_id_is_used_again() {
    let mut heap = H::new();
    let ids = [heap.fork_worker(), heap.fork_worker(), heap.fork_worker()];
    assert_eq!(ids, [1, 2, 3]);
    heap.retire_worker(1).unwrap();
    heap.retire_worker(3).unwrap();
    assert_eq!(heap.fork_worker(), 3);
    assert_eq!(heap.fork_worker(), 1);
    assert_eq!(heap.fork_worker(), 4);
}

#[test]
fn a_leaked_id_is_not_used_again() {
    let mut heap = H::new();
    let child = heap.fork_worker();
    heap.set_worker(child);
    let _ = list(&mut heap, 1);
    heap.set_worker(0);
    assert!(heap.retire_worker(child).is_err());
    assert_eq!(heap.fork_worker(), child + 1);
}

#[test]
fn fork_skips_ids_that_set_worker_used() {
    let mut heap = H::new();
    heap.set_worker(5);
    heap.set_worker(0);
    assert_eq!(heap.fork_worker(), 6);
    heap.set_worker(2);
    assert_eq!(heap.fork_worker(), 7);
}

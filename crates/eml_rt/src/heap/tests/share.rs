use std::sync::atomic::Ordering;

use super::{LONG, list, mortal, probe};
use crate::meta;
use crate::{Checked, Heap, HeapError, Kind, Meta, Mode, Word};
use std::sync::atomic::Ordering as AtomicOrdering;

fn rc<M: Mode>(_heap: &Heap<M>, w: Word) -> i32 {
    unsafe { meta::rc(w.header()) }.load(AtomicOrdering::Relaxed)
}

fn sharing_a_list_marks_every_cell<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, 3);
    unsafe {
        heap.share(l, 0).unwrap();
        let mut cell = l;
        while cell.is_ptr() {
            assert_eq!(rc(&heap, cell), -1);
            cell = heap.field(cell, 1).unwrap();
        }
        assert!(!heap.is_unique(l).unwrap());
        assert!(heap.is_last(l).unwrap());
        heap.decref(l).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn shared_counts_go_through_the_atomic_path<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = list(&mut heap, 1);
    unsafe {
        heap.share(x, 0).unwrap();
        heap.dup(x).unwrap();
        assert_eq!(rc(&heap, x), -2);
        assert!(!heap.is_last(x).unwrap());
        heap.decref(x).unwrap();
        assert!(heap.is_last(x).unwrap());
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn share_stops_at_immortal_objects<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let immortal = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) };
    let parent = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.acquire_immortal(immortal).unwrap();
        heap.set_field(parent, 0, immortal).unwrap();
        heap.share(parent, 0).unwrap();
        assert_eq!(rc(&heap, immortal), 0);
        assert_eq!(rc(&heap, parent), -1);
        heap.decref(parent).unwrap();
    }
    assert_eq!(mortal(&heap), 1);
}

fn share_walks_a_cycle_once<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = heap.alloc_data(0, 1, 0, false);
    let b = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.set_field(a, 0, b).unwrap();
        // b が a の参照を1つ持つ
        heap.dup(a).unwrap();
        heap.set_field(b, 0, a).unwrap();
        heap.share(a, 0).unwrap();
        assert_eq!((rc(&heap, a), rc(&heap, b)), (-2, -1));
        // 輪を切ってから手放す
        heap.set_field(b, 0, Word::UNIT).unwrap();
        heap.decref(a).unwrap();
        heap.decref(a).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_lin_object_moves_to_the_receiver_and_its_children_are_shared<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        heap.set_field(lin, 0, child).unwrap();
        heap.share(lin, 7).unwrap();
        assert_eq!(rc(&heap, lin), 1);
        assert_eq!(rc(&heap, child), -1);
        if M::CHECKED {
            let owner = crate::heap::check::prefix(lin.header())
                .first
                .load(AtomicOrdering::Relaxed);
            assert_eq!(owner, 7);
        }
        heap.set_worker(7);
        heap.decref(lin).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn releasing_a_shared_parent_takes_its_lin_child_without_dup<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let (value, drops) = probe();
    let file = heap.alloc_foreign(value, true);
    let rest = list(&mut heap, 1);
    let parent = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(parent, 0, file).unwrap();
        heap.set_field(parent, 1, rest).unwrap();
        heap.share(parent, 0).unwrap();
        // 検査モードでは、file を dup すると LinDup になる
        heap.release(parent, 0, &[true, false]).unwrap();
        assert_eq!(mortal(&heap), 1);
        heap.decref(file).unwrap();
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(mortal(&heap), 0);
}

fn a_deep_list_is_shared_without_recursion<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, LONG);
    unsafe {
        heap.share(l, 0).unwrap();
        heap.decref(l).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn releasing_a_unique_object_hands_over_the_kept_fields<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = list(&mut heap, 1);
    let b = list(&mut heap, 1);
    let p = heap.alloc_data(3, 2, 0, false);
    unsafe {
        heap.set_field(p, 0, a).unwrap();
        heap.set_field(p, 1, b).unwrap();
        heap.release(p, 3, &[true, false]).unwrap();
        assert_eq!(mortal(&heap), 1);
        assert!(heap.is_unique(a).unwrap());
        heap.decref(a).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn releasing_a_shared_object_dups_the_kept_fields<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = list(&mut heap, 1);
    let b = list(&mut heap, 1);
    let p = heap.alloc_data(3, 2, 0, false);
    unsafe {
        heap.set_field(p, 0, a).unwrap();
        heap.set_field(p, 1, b).unwrap();
        heap.dup(p).unwrap();
        heap.release(p, 3, &[true, false]).unwrap();
        assert_eq!(rc(&heap, a), 2);
        assert_eq!(mortal(&heap), 3);
        heap.decref(p).unwrap();
        heap.decref(a).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn the_expanded_release_matches_the_general_one<M: Mode>() {
    // 一意な場合と rc が 2 の場合のそれぞれで、一般の `release` と部品を並べた展開
    // (docs/spec/object-model.md の「核の操作」) を流し、手放した後に生きている箱の数を比べる
    for shared in [false, true] {
        let outcome = |expanded: bool| {
            let mut heap = Heap::<M>::new();
            let a = list(&mut heap, 1);
            let b = list(&mut heap, 1);
            let p = heap.alloc_data(3, 2, 0, false);
            unsafe {
                heap.set_field(p, 0, a).unwrap();
                heap.set_field(p, 1, b).unwrap();
                if shared {
                    heap.dup(p).unwrap();
                }
                if !expanded {
                    heap.release(p, 3, &[true, false]).unwrap();
                } else if heap.is_last(p).unwrap() {
                    heap.decref(b).unwrap();
                    heap.free_block(p).unwrap();
                } else {
                    heap.dup(a).unwrap();
                    heap.decref(p).unwrap();
                }
                let live = mortal(&heap);
                if shared {
                    heap.decref(p).unwrap();
                }
                heap.decref(a).unwrap();
                assert_eq!(mortal(&heap), 0);
                live
            }
        };
        assert_eq!(outcome(false), outcome(true), "shared: {shared}");
    }
}

both_modes!(
    sharing_a_list_marks_every_cell,
    shared_counts_go_through_the_atomic_path,
    share_stops_at_immortal_objects,
    share_walks_a_cycle_once,
    a_lin_object_moves_to_the_receiver_and_its_children_are_shared,
    releasing_a_shared_parent_takes_its_lin_child_without_dup,
    a_deep_list_is_shared_without_recursion,
    releasing_a_unique_object_hands_over_the_kept_fields,
    releasing_a_shared_object_dups_the_kept_fields,
    the_expanded_release_matches_the_general_one,
);

type H = Heap<Checked>;

#[test]
fn the_same_lin_object_reached_twice_is_an_error() {
    let mut heap = H::new();
    let lin = heap.alloc_data(0, 0, 1, true);
    let inner = heap.alloc_data(0, 1, 0, false);
    let outer = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(inner, 0, lin).unwrap();
        heap.set_field(outer, 0, inner).unwrap();
        heap.set_field(outer, 1, lin).unwrap();
        assert_eq!(heap.share(outer, 0), Err(HeapError::LinReachedTwice));
    }
}

#[test]
fn a_shared_object_reaching_a_local_one_breaks_the_invariant() {
    let mut heap = H::new();
    let p = heap.alloc_data(0, 1, 0, false);
    let c = list(&mut heap, 1);
    unsafe {
        heap.share(p, 0).unwrap();
        heap.set_field(p, 0, c).unwrap();
        assert_eq!(heap.check_invariants(), Err(HeapError::SharedReachesLocal));
        heap.set_field(p, 0, Word::UNIT).unwrap();
        heap.decref(c).unwrap();
        heap.decref(p).unwrap();
    }
    assert_eq!(heap.check_invariants(), Ok(()));
}

#[test]
fn an_immortal_object_reaching_a_mortal_one_breaks_the_invariant() {
    let mut heap = H::new();
    let p = unsafe { heap.alloc_immortal(1, Meta::new(0, 1, Kind::Data, false)) };
    let c = list(&mut heap, 1);
    unsafe {
        heap.set_field(p, 0, c).unwrap();
        assert_eq!(
            heap.check_invariants(),
            Err(HeapError::ImmortalReachesMortal)
        );
        heap.set_field(p, 0, Word::UNIT).unwrap();
        heap.decref(c).unwrap();
    }
}

#[test]
fn a_shared_parent_may_reach_a_local_lin_child() {
    let mut heap = H::new();
    let lin = heap.alloc_data(0, 0, 1, true);
    let p = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.set_field(p, 0, lin).unwrap();
        heap.share(p, 0).unwrap();
        assert_eq!(heap.check_invariants(), Ok(()));
        heap.decref(p).unwrap();
    }
}

#[test]
fn a_lin_object_on_the_shared_side_is_an_error() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 0, 1, false);
    unsafe {
        heap.share(x, 0).unwrap();
        // share は lin の物体に印を付けないので、この形は印付けの誤りでしか起きない
        let m = meta::meta(x.header());
        meta::set_meta(x.header(), m.with_lin(true));
        assert_eq!(heap.decref(x), Err(HeapError::LinShared));
        meta::set_meta(x.header(), m);
        heap.decref(x).unwrap();
    }
}

#[test]
fn using_a_freed_shared_object_is_an_error() {
    let mut heap = H::new();
    let x = list(&mut heap, 1);
    unsafe {
        heap.share(x, 0).unwrap();
        heap.decref(x).unwrap();
        // 最後の共有の decref で rc は 0 になっているが、前置きで解放を見分ける
        assert_eq!(heap.dup(x), Err(HeapError::UseAfterFree));
    }
}

#[test]
fn release_checks_the_layout() {
    let mut heap = H::new();
    let p = heap.alloc_data(3, 2, 0, false);
    unsafe {
        assert_eq!(
            heap.release(p, 4, &[false, false]),
            Err(HeapError::WrongLayout)
        );
        assert_eq!(heap.release(p, 3, &[false]), Err(HeapError::WrongLayout));
        heap.release(p, 3, &[false, false]).unwrap();
    }
}

#[test]
fn sharing_reaches_no_freed_object() {
    let mut heap = H::new();
    let c = list(&mut heap, 1);
    let p = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.decref(c).unwrap();
        // 解放した c を古い語のまま置く。rc は検査モードでは 1 のままで、前置きだけが解放を示す
        heap.set_field(p, 0, c).unwrap();
        assert_eq!(heap.share(p, 0), Err(HeapError::UseAfterFree));
        heap.set_field(p, 0, Word::UNIT).unwrap();
        heap.decref(p).unwrap();
    }
}

#[test]
fn share_leaves_an_immortal_lin_object_alone() {
    let mut heap = H::new();
    let immortal = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, true)) };
    let parent = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.acquire_immortal(immortal).unwrap();
        heap.set_field(parent, 0, immortal).unwrap();
        heap.share(parent, 7).unwrap();
        assert_eq!(rc(&heap, immortal), 0);
        let count = crate::heap::check::prefix(immortal.header())
            .first
            .load(AtomicOrdering::Relaxed);
        assert_eq!(count, 1);
        heap.set_field(parent, 0, Word::UNIT).unwrap();
        heap.decref(parent).unwrap();
        heap.decref(immortal).unwrap();
    }
}

#[test]
fn a_shared_lin_object_reached_by_share_is_an_error() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 0, 1, false);
    let parent = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.share(x, 0).unwrap();
        // share は lin の物体に印を付けないので、この形は印付けの誤りでしか起きない
        let m = meta::meta(x.header());
        meta::set_meta(x.header(), m.with_lin(true));
        heap.set_field(parent, 0, x).unwrap();
        assert_eq!(heap.share(parent, 0), Err(HeapError::LinShared));
        heap.set_field(parent, 0, Word::UNIT).unwrap();
        meta::set_meta(x.header(), m);
        heap.decref(x).unwrap();
        heap.decref(parent).unwrap();
    }
}

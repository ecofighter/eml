use std::sync::atomic::Ordering;

use super::raw;
use super::{LONG, cons, list, mortal, probe};
use crate::{Checked, Heap, HeapError, Kind, Meta, Mode, Word};

fn rc<M: Mode>(_heap: &Heap<M>, w: Word) -> i32 {
    unsafe { raw::rc(w) }
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
        raw::set_slot(b, 0, Word::UNIT);
        heap.decref(a).unwrap();
        heap.decref(a).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_lin_object_moves_to_the_receiver_and_its_unique_children_move_with_it<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        heap.set_field(lin, 0, child).unwrap();
        heap.share(lin, 7).unwrap();
        assert_eq!(rc(&heap, lin), 1);
        assert_eq!(rc(&heap, child), 1);
        if M::CHECKED {
            let owner = raw::prefix_first(lin);
            assert_eq!(owner, 7);
            assert_eq!(raw::prefix_first(child), 7);
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

/// 検査モードの持ち主。速いモードには持ち主がないので確かめない。
fn assert_owner<M: Mode>(w: Word, owner: u32) {
    if M::CHECKED {
        assert_eq!(unsafe { raw::prefix_first(w) }, owner);
    }
}

fn unique_descendants_of_a_lin_root_move<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, 3);
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        heap.set_field(lin, 0, l).unwrap();
        heap.share(lin, 7).unwrap();
        assert_eq!(rc(&heap, lin), 1);
        assert_owner::<M>(lin, 7);
        heap.set_worker(7);
        let mut cell = l;
        while cell.is_ptr() {
            assert_eq!(rc(&heap, cell), 1);
            assert_owner::<M>(cell, 7);
            cell = heap.field(cell, 1).unwrap();
        }
        assert!(heap.is_unique(l).unwrap());
        heap.decref(lin).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_shared_node_under_a_lin_root_is_marked_with_its_unique_child<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let below = list(&mut heap, 1);
    let kept = cons(&mut heap, Word::imm(0), below);
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        // kept はほかの所からも参照されている
        heap.dup(kept).unwrap();
        heap.set_field(lin, 0, kept).unwrap();
        heap.share(lin, 7).unwrap();
        assert_eq!(
            (rc(&heap, lin), rc(&heap, kept), rc(&heap, below)),
            (1, -2, -1)
        );
        assert_owner::<M>(lin, 7);
        heap.decref(kept).unwrap();
        heap.set_worker(7);
        heap.decref(lin).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_lin_object_under_a_marked_parent_moves_its_unique_descendants<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, 2);
    let lin = heap.alloc_data(0, 1, 0, true);
    let parent = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.set_field(lin, 0, l).unwrap();
        heap.set_field(parent, 0, lin).unwrap();
        heap.share(parent, 7).unwrap();
        assert_eq!(
            (rc(&heap, parent), rc(&heap, lin), rc(&heap, l)),
            (-1, 1, 1)
        );
        assert_owner::<M>(lin, 7);
        assert_owner::<M>(l, 7);
        heap.set_worker(7);
        heap.decref(parent).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

/// `poly` の組 (`(a, FileIn)` の形) を、共有にした後でもう一度送る。`lin` の部分とその下の rc 1 の物体は、検査モードで
/// 2つ目の受け手の持ち物になる。rc はどちらのモードでも同じである
fn resending_a_shared_pair_passes_its_lin_part_to_the_second_receiver<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = list(&mut heap, 1);
    let l = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 1, 0, true);
    let pair = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(lin, 0, l).unwrap();
        heap.set_field(pair, 0, a).unwrap();
        heap.set_field(pair, 1, lin).unwrap();
        heap.share(pair, 1).unwrap();
        assert_owner::<M>(lin, 1);
        assert_owner::<M>(l, 1);
        heap.set_worker(1);
        heap.share(pair, 2).unwrap();
        assert_eq!(
            (rc(&heap, pair), rc(&heap, a), rc(&heap, lin), rc(&heap, l)),
            (-1, -1, 1, 1)
        );
        assert_owner::<M>(lin, 2);
        assert_owner::<M>(l, 2);
        heap.set_worker(2);
        heap.decref(pair).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

/// 送る前から `lin` の物体の下にある rc > 1 の物体は、初めの `share` で印が付く。送り直しは印の付いた物体で止まるので
/// 誤りにならない
fn resending_after_marking_an_rc2_node_under_the_lin_part_is_ok<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = list(&mut heap, 1);
    let kept = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 1, 0, true);
    let pair = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.dup(kept).unwrap();
        heap.set_field(lin, 0, kept).unwrap();
        heap.set_field(pair, 0, a).unwrap();
        heap.set_field(pair, 1, lin).unwrap();
        heap.share(pair, 1).unwrap();
        assert_eq!(rc(&heap, kept), -2);
        heap.set_worker(1);
        heap.share(pair, 2).unwrap();
        assert_eq!(
            (rc(&heap, pair), rc(&heap, lin), rc(&heap, kept)),
            (-1, 1, -2)
        );
        assert_owner::<M>(lin, 2);
        heap.set_worker(2);
        heap.decref(pair).unwrap();
        heap.decref(kept).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_long_unique_list_under_a_lin_root_moves_without_recursion<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, LONG);
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        heap.set_field(lin, 0, l).unwrap();
        heap.share(lin, 7).unwrap();
        heap.set_worker(7);
        assert!(heap.is_unique(l).unwrap());
        heap.decref(lin).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

/// 受け手が今の worker なら、`Unr` の値には印を付けるだけで、`lin` の物体は局所のまま持ち主も変わらない。
fn sharing_with_the_own_worker_marks_only_unr_values<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, 2);
    let below = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        heap.set_field(lin, 0, below).unwrap();
        heap.share(l, 0).unwrap();
        heap.share(lin, 0).unwrap();
        assert_eq!(rc(&heap, l), -1);
        assert_eq!((rc(&heap, lin), rc(&heap, below)), (1, 1));
        assert_owner::<M>(lin, 0);
        assert_owner::<M>(below, 0);
        assert!(heap.is_unique(below).unwrap());
        heap.decref(l).unwrap();
        heap.decref(lin).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

both_modes!(
    sharing_a_list_marks_every_cell,
    shared_counts_go_through_the_atomic_path,
    share_stops_at_immortal_objects,
    share_walks_a_cycle_once,
    a_lin_object_moves_to_the_receiver_and_its_unique_children_move_with_it,
    releasing_a_shared_parent_takes_its_lin_child_without_dup,
    a_deep_list_is_shared_without_recursion,
    releasing_a_unique_object_hands_over_the_kept_fields,
    releasing_a_shared_object_dups_the_kept_fields,
    the_expanded_release_matches_the_general_one,
    unique_descendants_of_a_lin_root_move,
    a_shared_node_under_a_lin_root_is_marked_with_its_unique_child,
    a_lin_object_under_a_marked_parent_moves_its_unique_descendants,
    resending_a_shared_pair_passes_its_lin_part_to_the_second_receiver,
    resending_after_marking_an_rc2_node_under_the_lin_part_is_ok,
    a_long_unique_list_under_a_lin_root_moves_without_recursion,
    sharing_with_the_own_worker_marks_only_unr_values,
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
        raw::set_slot(p, 0, c);
        assert_eq!(heap.check_invariants(), Err(HeapError::SharedReachesLocal));
        raw::set_slot(p, 0, Word::UNIT);
        heap.decref(c).unwrap();
        heap.decref(p).unwrap();
    }
    assert_eq!(heap.check_invariants(), Ok(()));
}

#[test]
fn an_immortal_object_reaching_a_mortal_one_breaks_the_invariant() {
    let mut heap = H::new();
    let c = list(&mut heap, 1);
    // 書けるのは構築中の間だけなので、子を先に作る
    let p = unsafe { heap.alloc_immortal(1, Meta::new(0, 1, Kind::Data, false)) };
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
        let m = raw::meta(x);
        raw::set_meta(x, m.with_lin(true));
        assert_eq!(heap.decref(x), Err(HeapError::LinShared));
        raw::set_meta(x, m);
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
    }
    // share が状態を変えた後の Err なので、ヒープは落とすだけにする (docs/spec/object-model.md の「検査モードの約束」)
    drop(heap);
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
        let count = raw::prefix_first(immortal);
        assert_eq!(count, 1);
        // 共有の parent を空けると、子の不死の物体の参照も手放す
        heap.decref(parent).unwrap();
        assert_eq!(raw::prefix_first(immortal), 0);
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
        let m = raw::meta(x);
        raw::set_meta(x, m.with_lin(true));
        heap.set_field(parent, 0, x).unwrap();
        assert_eq!(heap.share(parent, 0), Err(HeapError::LinShared));
        raw::set_meta(x, m);
    }
    // share が状態を変えた後の Err なので、ヒープは落とすだけにする (docs/spec/object-model.md の「検査モードの約束」)
    drop(heap);
}

/// 共有の物体から局所の物体だけを通って届く rc > 1 の物体は、`lin` の部分の不変条件を破る。送り直しの所有者だけの
/// たどりがそこに着くと `SharedReachesLocal` になる (docs/spec/object-model.md の「`share`」)
#[test]
fn an_rc2_object_under_a_resent_lin_object_is_an_error() {
    let mut heap = H::new();
    let a = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 1, 0, true);
    let pair = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(pair, 0, a).unwrap();
        heap.set_field(pair, 1, lin).unwrap();
        heap.share(pair, 1).unwrap();
        heap.set_worker(1);
        // 1つ目の受け手が、受け取った lin の物体に、参照が2つある自分の物体を入れる
        let kept = list(&mut heap, 1);
        heap.dup(kept).unwrap();
        heap.set_field(lin, 0, kept).unwrap();
        assert_eq!(heap.share(pair, 2), Err(HeapError::SharedReachesLocal));
    }
    // share が状態を変えた後の Err なので、ヒープは落とすだけにする (docs/spec/object-model.md の「検査モードの約束」)
    drop(heap);
}

/// 印を付けた物体は持ち主の数から外れ、所有を移した物体は受け手の数に移る。
#[test]
fn counts_move_with_the_walk() {
    let mut heap = H::new();
    let l = list(&mut heap, 2);
    let kept = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 2, 0, true);
    unsafe {
        heap.dup(kept).unwrap();
        heap.set_field(lin, 0, l).unwrap();
        heap.set_field(lin, 1, kept).unwrap();
        assert_eq!(heap.owned_by(0), 4);
        heap.share(lin, 7).unwrap();
        assert_eq!((heap.owned_by(0), heap.owned_by(7)), (0, 3));
        assert_eq!(heap.check_invariants(), Ok(()));
        heap.decref(kept).unwrap();
        heap.set_worker(7);
        heap.decref(lin).unwrap();
    }
    assert_eq!((heap.owned_by(0), heap.owned_by(7)), (0, 0));
    assert_eq!(heap.check_invariants(), Ok(()));
}

/// `share` が rc を負にした物体を1つずつ数える。すでに共有の物体は数えない。
#[test]
fn share_counts_the_objects_it_turns_negative() {
    let mut heap = H::new();
    let l = list(&mut heap, 3);
    assert_eq!(heap.stats().shared, 0);
    unsafe {
        heap.share(l, 0).unwrap();
        assert_eq!(heap.stats().shared, 3);
        heap.share(l, 0).unwrap();
        assert_eq!(heap.stats().shared, 3);
        heap.decref(l).unwrap();
    }
}

/// fork で印を付けた閉包から届く物体には、子が触れてよい。印を付けなければ、子の所有者の検査が必ず落ちる
/// (docs/future/multicore.md の「論理的な worker ID」)
#[test]
fn fork_marking_lets_the_child_touch_what_the_closure_reaches() {
    // クロージャの形の物体 (`Data`、tag 0、走査する語に [`&f`, 捕獲]) が3つのセルのリストを捕まえ、参照が2つある
    let closure = |heap: &mut H| {
        let l = list(heap, 3);
        let c = heap.alloc_data(0, 2, 0, false);
        unsafe {
            heap.set_field(c, 0, Word::imm(1 << 32)).unwrap();
            heap.set_field(c, 1, l).unwrap();
            heap.dup(c).unwrap();
        }
        (c, l)
    };

    let mut heap = H::new();
    let (c, _) = closure(&mut heap);
    let child = heap.fork_worker();
    heap.set_worker(child);
    assert_eq!(
        unsafe { heap.dup(c) },
        Err(HeapError::WrongOwner {
            owner: 0,
            worker: child
        })
    );
    drop(heap);

    let mut heap = H::new();
    let (c, l) = closure(&mut heap);
    let child = heap.fork_worker();
    unsafe {
        heap.share(c, child).unwrap();
        heap.set_worker(child);
        heap.dup(c).unwrap();
        let mut cell = l;
        while cell.is_ptr() {
            heap.dup(cell).unwrap();
            heap.decref(cell).unwrap();
            cell = heap.field(cell, 1).unwrap();
        }
        heap.decref(c).unwrap();
        heap.set_worker(0);
        heap.retire_worker(child).unwrap();
        heap.decref(c).unwrap();
        heap.decref(c).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

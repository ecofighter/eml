//! `transfer_back` (docs/spec/object-model.md の「`transfer_back`」)。

use std::any::Any;
use std::sync::atomic::Ordering;

use super::raw;
use super::{LONG, cons, list, mortal, probe};
use crate::{Checked, Heap, HeapError, Kind, Meta, Mode, Word};

type H = Heap<Checked>;

/// リストのセルの rc を先頭から並べる。
fn rcs(heap: &Heap<impl Mode>, l: Word) -> Vec<i32> {
    let mut out = Vec::new();
    let mut cell = l;
    while cell.is_ptr() {
        out.push(unsafe { raw::rc(cell) });
        cell = unsafe { heap.field(cell, 1) }.unwrap();
    }
    out
}

/// リストのセルの前置きの所有者を先頭から並べる。検査モードだけで使う。
fn owners(heap: &H, l: Word) -> Vec<u32> {
    let mut out = Vec::new();
    let mut cell = l;
    while cell.is_ptr() {
        out.push(unsafe { raw::prefix_first(cell) });
        cell = unsafe { heap.field(cell, 1) }.unwrap();
    }
    out
}

fn a_shared_list_becomes_local_again<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    let l = list(&mut heap, 3);
    unsafe {
        heap.share(l, child).unwrap();
        assert_eq!(rcs(&heap, l), [-1, -1, -1]);
        heap.transfer_back(l, child).unwrap();
        assert_eq!(rcs(&heap, l), [1, 1, 1]);
        assert!(heap.is_unique(l).unwrap());
        heap.decref(l).unwrap();
    }
    assert_eq!(heap.retire_worker(child), Ok(()));
    assert_eq!(mortal(&mut heap), 0);
}

fn shared_counts_above_one_and_immortal_objects_stop<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    let x = list(&mut heap, 1);
    let immortal = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) };
    let p = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.acquire_immortal(immortal).unwrap();
        heap.set_field(p, 0, x).unwrap();
        heap.set_field(p, 1, immortal).unwrap();
        heap.share(p, child).unwrap();
        heap.dup(x).unwrap();
        heap.transfer_back(p, child).unwrap();
        assert_eq!((raw::rc(p), raw::rc(x), raw::rc(immortal)), (1, -2, 0));
        heap.decref(x).unwrap();
        heap.decref(p).unwrap();
    }
    assert_eq!(mortal(&mut heap), 1);
}

/// 子が確保した値は、速いモードでは何もせずに局所の値で、検査モードでは所有者を今の worker に書き換える。
fn a_list_of_the_child_comes_back_to_the_parent<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    heap.set_worker(child);
    let l = list(&mut heap, 3);
    heap.set_worker(0);
    unsafe {
        heap.transfer_back(l, child).unwrap();
        assert_eq!(rcs(&heap, l), [1, 1, 1]);
    }
    assert_eq!(heap.retire_worker(child), Ok(()));
    unsafe { heap.decref(l) }.unwrap();
    assert_eq!(mortal(&mut heap), 0);
}

#[test]
fn the_owners_of_a_list_of_the_child_are_rewritten() {
    let mut heap = H::new();
    let child = heap.fork_worker();
    heap.set_worker(child);
    let l = list(&mut heap, 3);
    heap.set_worker(0);
    assert_eq!(owners(&heap, l), [child; 3]);
    unsafe { heap.transfer_back(l, child) }.unwrap();
    assert_eq!(owners(&heap, l), [0; 3]);
    assert_eq!((heap.owned_by(0), heap.owned_by(child)), (3, 0));
    assert_eq!(super::check_all(&mut heap), Ok(()));
    unsafe { heap.decref(l) }.unwrap();
}

/// 子が確保したセルの下の共有の物体は、子に渡した親の値なので、共有のまま残る。
fn a_shared_object_under_a_cell_of_the_child_stays_shared<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    let g = list(&mut heap, 1);
    unsafe {
        heap.share(g, child).unwrap();
        heap.set_worker(child);
        let c = cons(&mut heap, Word::imm(1), g);
        heap.set_worker(0);
        heap.transfer_back(c, child).unwrap();
        assert_eq!((raw::rc(c), raw::rc(g)), (1, -1));
        assert_eq!(heap.retire_worker(child), Ok(()));
        heap.decref(c).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

/// `lin` でない共有の組の中の `lin` の物体は、`share` で子に移っている。検査モードでは、子が返したセルから組を通って
/// その物体に届き、所有者を親に戻すので、子の ID を退かせられ、親が手放せる。
fn a_poly_pair_with_a_file_comes_back_to_the_parent<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    let (value, drops) = probe();
    let file = heap.alloc_foreign(value, true);
    let a = list(&mut heap, 1);
    let pair = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(pair, 0, a).unwrap();
        heap.set_field(pair, 1, file).unwrap();
        heap.share(pair, child).unwrap();
        assert_eq!((raw::rc(pair), raw::rc(a), raw::rc(file)), (-1, -1, 1));
        heap.set_worker(child);
        let c = cons(&mut heap, Word::imm(1), pair);
        heap.set_worker(0);
        heap.transfer_back(c, child).unwrap();
        assert_eq!(
            (raw::rc(c), raw::rc(pair), raw::rc(a), raw::rc(file)),
            (1, -1, -1, 1)
        );
        if M::CHECKED {
            assert_eq!(raw::prefix_first(file), 0);
        }
        assert_eq!(heap.retire_worker(child), Ok(()));
        heap.decref(c).unwrap();
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(mortal(&mut heap), 0);
}

/// 子が受け取った共有の組をそのまま返す形。組とその下の rc -1 の物体を裏返し、組の中の子の持ち物の `lin` の物体は、
/// 検査モードでは裏返しのスタックの上で所有者を親に戻す。
fn a_shared_poly_pair_returned_as_is_flips_and_its_file_comes_back<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    let (value, drops) = probe();
    let file = heap.alloc_foreign(value, true);
    let a = list(&mut heap, 1);
    let pair = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(pair, 0, a).unwrap();
        heap.set_field(pair, 1, file).unwrap();
        heap.share(pair, child).unwrap();
        heap.transfer_back(pair, child).unwrap();
        assert_eq!((raw::rc(pair), raw::rc(a), raw::rc(file)), (1, 1, 1));
        if M::CHECKED {
            assert_eq!(raw::prefix_first(file), 0);
        }
        assert_eq!(heap.retire_worker(child), Ok(()));
        heap.decref(pair).unwrap();
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(mortal(&mut heap), 0);
}

/// `lin` ビットの立った rc -1 の物体も裏返す。
fn a_shared_lin_object_is_flipped<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        heap.set_field(lin, 0, Word::UNIT).unwrap();
        // `share` は `lin` の物体に印を付けないので、数を通さずに rc -1 にする
        raw::set_rc(&mut heap, lin, -1);
        heap.transfer_back(lin, child).unwrap();
        assert_eq!(raw::rc(lin), 1);
        assert!(heap.is_unique(lin).unwrap());
        heap.decref(lin).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

fn a_deep_shared_list_comes_back_without_recursion<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    let l = list(&mut heap, LONG);
    unsafe {
        heap.share(l, child).unwrap();
        heap.transfer_back(l, child).unwrap();
        assert!(heap.is_unique(l).unwrap());
        heap.decref(l).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
}

fn a_deep_list_of_the_child_comes_back_without_recursion<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    heap.set_worker(child);
    let l = list(&mut heap, LONG);
    heap.set_worker(0);
    unsafe { heap.transfer_back(l, child) }.unwrap();
    assert_eq!(heap.retire_worker(child), Ok(()));
    unsafe { heap.decref(l) }.unwrap();
    assert_eq!(mortal(&mut heap), 0);
}

/// 子が確保した値を移し戻さずに ID を退かせると、検査モードでは `TransferLeak` になる。
fn skipping_transfer_back_leaks_the_list_of_the_child<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = heap.fork_worker();
    heap.set_worker(child);
    let l = list(&mut heap, 3);
    heap.set_worker(0);
    let r = heap.retire_worker(child);
    if M::CHECKED {
        assert_eq!(
            r,
            Err(HeapError::TransferLeak {
                worker: child,
                live: 3
            })
        );
    } else {
        assert_eq!(r, Ok(()));
    }
    let _ = l;
}

/// 受け手が今の worker である `share` の後、別の ID が `transfer_back` で受け取れる。VM がタスクの結果を預かる ID に
/// 預けて受け取る道で、持ち主の数は検査モードだけで動く。
fn a_value_shared_to_the_current_id_is_received_by_another_id<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = heap.fork_worker();
    heap.set_worker(a);
    let x = list(&mut heap, 1);
    // 持ち主の数は検査モードのヒープにだけある
    let owned = |heap: &Heap<M>, id| {
        (heap as &dyn Any)
            .downcast_ref::<H>()
            .map(|heap| heap.owned_by(id))
    };
    let before = owned(&heap, a);
    unsafe {
        heap.share(x, a).unwrap();
        assert_eq!(raw::rc(x), -1);
        assert_eq!(owned(&heap, a), before.map(|n| n - 1));
        let b = heap.fork_worker();
        heap.set_worker(b);
        let before = owned(&heap, b);
        assert_eq!(heap.transfer_back(x, a), Ok(()));
        assert_eq!(raw::rc(x), 1);
        assert_eq!(owned(&heap, b), before.map(|n| n + 1));
        if let Some(heap) = (&mut heap as &mut dyn Any).downcast_mut::<H>() {
            assert_eq!(super::check_all(heap), Ok(()));
        }
        assert_eq!(heap.retire_worker(a), Ok(()));
        heap.decref(x).unwrap();
        assert_eq!(heap.retire_worker(b), Ok(()));
    }
}

both_modes!(
    a_shared_list_becomes_local_again,
    shared_counts_above_one_and_immortal_objects_stop,
    a_list_of_the_child_comes_back_to_the_parent,
    a_shared_object_under_a_cell_of_the_child_stays_shared,
    a_poly_pair_with_a_file_comes_back_to_the_parent,
    a_shared_poly_pair_returned_as_is_flips_and_its_file_comes_back,
    a_shared_lin_object_is_flipped,
    a_deep_shared_list_comes_back_without_recursion,
    a_deep_list_of_the_child_comes_back_without_recursion,
    skipping_transfer_back_leaks_the_list_of_the_child,
    a_value_shared_to_the_current_id_is_received_by_another_id,
);

#[test]
fn an_object_of_a_third_worker_is_wrong_owner() {
    let mut heap = H::new();
    let child = heap.fork_worker();
    let other = heap.fork_worker();
    heap.set_worker(other);
    let x = list(&mut heap, 1);
    heap.set_worker(child);
    let c = cons(&mut heap, Word::imm(1), x);
    heap.set_worker(0);
    assert_eq!(
        unsafe { heap.transfer_back(c, child) },
        Err(HeapError::WrongOwner {
            owner: other,
            worker: 0
        })
    );
}

#[test]
fn transfer_back_from_the_current_worker_changes_nothing() {
    let mut heap = H::new();
    let child = heap.fork_worker();
    let l = list(&mut heap, 2);
    unsafe {
        heap.share(l, child).unwrap();
        assert_eq!(
            heap.transfer_back(l, 0),
            Err(HeapError::WrongOwner {
                owner: 0,
                worker: 0
            })
        );
        assert_eq!(rcs(&heap, l), [-1, -1]);
        assert_eq!(super::check_all(&mut heap), Ok(()));
        heap.decref(l).unwrap();
    }
}

#[test]
fn a_freed_object_reached_by_transfer_back_is_use_after_free() {
    let mut heap = H::new();
    let child = heap.fork_worker();
    heap.set_worker(child);
    let c = list(&mut heap, 1);
    let p = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.decref(c).unwrap();
        // 解放した c を古い語のまま置く。前置きだけが解放を示す
        heap.set_field(p, 0, c).unwrap();
        heap.set_worker(0);
        assert_eq!(heap.transfer_back(p, child), Err(HeapError::UseAfterFree));
    }
}

#[test]
fn transfer_back_moves_the_counts_to_the_current_worker() {
    let mut heap = H::new();
    let child = heap.fork_worker();
    let shared = list(&mut heap, 2);
    unsafe {
        heap.share(shared, child).unwrap();
        assert_eq!(heap.owned_by(0), 0);
        heap.set_worker(child);
        let c = cons(&mut heap, Word::imm(1), shared);
        heap.set_worker(0);
        assert_eq!((heap.owned_by(0), heap.owned_by(child)), (0, 1));
        // c の所有者を書き換え、共有のリストは c の下で rc -1 のまま残す
        heap.transfer_back(c, child).unwrap();
        assert_eq!((heap.owned_by(0), heap.owned_by(child)), (1, 0));
        assert_eq!(super::check_all(&mut heap), Ok(()));
        heap.decref(c).unwrap();
        // 裏返した物体は今の worker の数に入る
        let l = list(&mut heap, 2);
        heap.share(l, child).unwrap();
        heap.transfer_back(l, child).unwrap();
        assert_eq!(heap.owned_by(0), 2);
        assert_eq!(super::check_all(&mut heap), Ok(()));
        heap.decref(l).unwrap();
    }
    assert_eq!(heap.owned_by(0), 0);
}

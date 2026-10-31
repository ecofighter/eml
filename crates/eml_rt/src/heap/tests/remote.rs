//! 別のスレッドからの解放の取り込み (docs/spec/object-model.md の「別のスレッドからの解放」)。2つのヒープを1つの
//! スレッドで使い、もう一方のヒープのページの物体を空けて積む。スレッドをまたぐ形は `trusted` のテストにある。

use std::sync::Arc;

use super::{list, mortal};
use crate::{Checked, Control, Heap, HeapError, Limits, Mode};

/// 共有の Control を持つヒープ `a` と `b`。`b` が `a` の共有のリストを空けて、`a` に積む。
fn pushed<M: Mode>(n: usize) -> (Heap<M>, Heap<M>) {
    let control = Arc::new(Control::new(Limits::default()));
    let mut a = Heap::<M>::with_control(control.clone());
    let mut b = Heap::<M>::with_control(control);
    let xs = list(&mut a, n);
    unsafe {
        a.share(xs, 0).unwrap();
        b.decref(xs).unwrap();
    }
    (a, b)
}

// 取り込みが変えるのは、生きている箱の数、隔離のバイト数、取り込んだ数だけである
fn reclaim_changes_no_work_counter<M: Mode>() {
    const N: usize = 5;
    let (mut a, b) = pushed::<M>(N);
    let before = a.stats();
    a.reclaim();
    let after = a.stats();
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
    if M::CHECKED {
        assert_eq!(after.live, before.live - N as u64);
        assert_eq!(after.reclaimed, before.reclaimed + N as u64);
        assert_eq!(b.stats().remote_frees, N as u64);
    }
    assert_eq!(mortal(&mut a), 0);
}

// 受けた数が `REMOTE_RECLAIM` に届くまでは、`reclaim_if_due` は取り込まない
fn reclaim_if_due_waits_for_enough_frees<M: Mode>() {
    let (mut a, mut b) = pushed::<M>(255);
    assert!(!a.reclaim_if_due());
    let x = list(&mut a, 1);
    unsafe {
        a.share(x, 0).unwrap();
        b.decref(x).unwrap();
    }
    assert!(a.reclaim_if_due());
    assert!(!a.reclaim_if_due());
    assert_eq!(mortal(&mut a), 0);
}

both_modes!(
    reclaim_changes_no_work_counter,
    reclaim_if_due_waits_for_enough_frees,
);

// 取り込めない `check_owned` は、受けて取り込んでいない解放が残るヒープを断る
#[test]
fn check_owned_refuses_a_heap_with_pending_frees() {
    let (mut a, b) = pushed::<Checked>(3);
    assert_eq!(
        a.control().check_owned(&[&a, &b]),
        Err(HeapError::NotQuiescent)
    );
    a.reclaim();
    assert_eq!(a.control().check_owned(&[&a, &b]), Ok(()));
}

// 生存の列挙とリークの数えは、先に取り込む
#[test]
fn walks_reclaim_first() {
    let (mut a, _b) = pushed::<Checked>(3);
    assert!(a.leaks().is_empty());
    assert_eq!(a.check_invariants(), Ok(()));
    assert_eq!(a.stats().reclaimed, 3);
}

//! `dup`と`decref`の監査と本体 (docs/spec/object-model.mdの「核の操作」)。

use super::{list, mortal, raw};
use crate::{Heap, HeapError, Kind, Meta, Mode, Word};

type Maker<M> = fn(&mut Heap<M>) -> Word;

/// 局所の物体、`share`した物体、即値、ダミー。
fn words<M: Mode>() -> [Maker<M>; 4] {
    [
        |heap| heap.alloc_data(0, 0, 1, false),
        |heap| {
            let x = heap.alloc_data(0, 0, 1, false);
            unsafe { heap.share(x, 1) }.unwrap();
            x
        },
        |_| Word::imm(7),
        |heap| heap.dummy(),
    ]
}

fn rc_of(w: Word) -> Option<i32> {
    w.is_ptr().then(|| unsafe { raw::rc(w) })
}

/// `dup`の監査と本体を続けると`dup`になる。局所の物体、共有の物体、即値、ダミーのどれでも、数も数え上げも
/// 結果も同じである。
fn an_audit_and_a_body_are_a_dup<M: Mode>() {
    for make in words::<M>() {
        let mut whole = Heap::<M>::new();
        let mut halves = Heap::<M>::new();
        let (a, b) = (make(&mut whole), make(&mut halves));
        let expected = unsafe { whole.dup(a) };
        let got = unsafe { halves.audit_dup(b) }.and_then(|()| unsafe { halves.dup_body(b) });
        assert_eq!(got, expected);
        assert_eq!(halves.stats(), whole.stats());
        assert_eq!(rc_of(b), rc_of(a));
    }
}

/// `decref`も同じである。リストの最後の参照では、全部のノードをふつうの検査つきの`decref`で解放する。
fn an_audit_and_a_body_are_a_decref<M: Mode>() {
    for make in words::<M>() {
        let mut whole = Heap::<M>::new();
        let mut halves = Heap::<M>::new();
        let (a, b) = (make(&mut whole), make(&mut halves));
        let expected = unsafe { whole.decref(a) };
        let got = unsafe { halves.audit_decref(b) }.and_then(|()| unsafe { halves.decref_body(b) });
        assert_eq!(got, expected);
        assert_eq!(halves.stats(), whole.stats());
        assert_eq!(mortal(&mut halves), mortal(&mut whole));
    }

    let mut heap = Heap::<M>::new();
    let xs = list(&mut heap, 3);
    unsafe {
        heap.audit_decref(xs).unwrap();
        heap.decref_body(xs).unwrap();
    }
    assert_eq!(mortal(&mut heap), 0);
    if M::CHECKED {
        assert_eq!(heap.stats().rc_decrements, 3);
    }
}

/// 不死の物体では、監査が操作の全部を済ませ、本体は物体に触れない。外に出ている参照の数を動かすのも監査である。
fn an_audit_finishes_an_immortal_object<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) };
    unsafe {
        if M::CHECKED {
            heap.audit_dup(x).unwrap();
            assert_eq!(raw::prefix_first(x), 1);
            heap.dup_body(x).unwrap();
            assert_eq!(raw::prefix_first(x), 1);
            assert_eq!(heap.stats().rc_increments, 1);

            heap.audit_decref(x).unwrap();
            assert_eq!(raw::prefix_first(x), 0);
            heap.decref_body(x).unwrap();
            assert_eq!(raw::prefix_first(x), 0);
            assert_eq!(heap.stats().rc_decrements, 1);

            assert_eq!(heap.audit_decref(x), Err(HeapError::ImmortalOverRelease));
        } else {
            heap.audit_dup(x).unwrap();
            heap.dup_body(x).unwrap();
            heap.audit_decref(x).unwrap();
            heap.decref_body(x).unwrap();
        }
    }
    assert_eq!(unsafe { raw::rc(x) }, 0);
}

/// 失敗した監査は何も変えず、何も数えない。
fn a_failing_audit_changes_nothing<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let freed = heap.alloc_data(0, 0, 1, false);
    let lin = heap.alloc_data(0, 0, 1, true);
    let live = heap.alloc_data(0, 0, 1, false);
    unsafe { heap.decref(freed) }.unwrap();
    let before = heap.stats();
    unsafe {
        if M::CHECKED {
            assert_eq!(heap.audit_dup(freed), Err(HeapError::UseAfterFree));
            assert_eq!(heap.audit_decref(freed), Err(HeapError::UseAfterFree));
            assert_eq!(heap.audit_dup(lin), Err(HeapError::LinDup));
        } else {
            heap.audit_dup(live).unwrap();
            heap.audit_decref(live).unwrap();
        }
    }
    assert_eq!(heap.stats(), before);
    assert_eq!(unsafe { raw::rc(live) }, 1);
    assert_eq!(unsafe { raw::rc(lin) }, 1);
}

both_modes!(
    an_audit_and_a_body_are_a_dup,
    an_audit_and_a_body_are_a_decref,
    an_audit_finishes_an_immortal_object,
    a_failing_audit_changes_nothing,
);

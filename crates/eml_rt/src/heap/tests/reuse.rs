use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use super::{CONS, cons, list, mortal, probe};
use crate::{Checked, Heap, HeapError, Kind, Meta, Mode, Token, Word};

const CONS_META: Meta = Meta::new(CONS, 2, Kind::Data, false);
const PAIR_META: Meta = Meta::new(0, 2, Kind::Data, false);

fn a_unique_cell_is_reused_in_place_without_writing_fields<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = list(&mut heap, 1);
    let b = list(&mut heap, 1);
    let p = cons(&mut heap, a, b);
    unsafe {
        let token = heap.release_reuse(p, CONS, &[true, true]).unwrap();
        assert!(!token.is_empty());
        let q = heap.reuse_in_place(token, CONS_META).unwrap();
        assert_eq!(q, p);
        assert_eq!((heap.field(q, 0), heap.field(q, 1)), (Ok(a), Ok(b)));
        assert_eq!(mortal(&heap), 3);
        heap.decref(q).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_box_moves_to_another_type_of_the_same_size<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let cell = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        let token = heap.release_reuse(cell, CONS, &[false, false]).unwrap();
        let (pair, reused) = heap.con_at(token, 2, PAIR_META).unwrap();
        assert!(reused);
        assert_eq!(pair, cell);
        assert_eq!(heap.meta(pair), Ok(PAIR_META));
        heap.set_field(pair, 0, Word::imm(2)).unwrap();
        heap.set_field(pair, 1, Word::imm(3)).unwrap();
        heap.decref(pair).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_shared_object_is_never_a_token<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        heap.dup(p).unwrap();
        let token = heap.release_reuse(p, CONS, &[false, false]).unwrap();
        assert!(token.is_empty());
        let (q, reused) = heap.con_at(token, 2, CONS_META).unwrap();
        assert!(!reused);
        assert_ne!(q, p);
        heap.set_field(q, 0, Word::UNIT).unwrap();
        heap.set_field(q, 1, Word::UNIT).unwrap();
        heap.decref(p).unwrap();
        heap.decref(q).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_shared_marked_object_is_never_a_token<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        heap.share(p, 0).unwrap();
        // rc −1 は最後の参照だが共有の印を持つので、箱を使い回さずに空ける
        assert!(
            heap.release_reuse(p, CONS, &[false, false])
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(mortal(&heap), 0);
}

fn drop_token_frees_the_box<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        let token = heap.into_token(p).unwrap();
        heap.drop_token(token).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_heap_dropped_with_an_outstanding_token_and_large_objects_is_clean<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    let _token = unsafe { heap.into_token(p) }.unwrap();
    heap.alloc_data(0, 0, 500, false);
    unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) };
    let (value, drops) = probe();
    heap.alloc_foreign(value, true);
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

both_modes!(
    a_unique_cell_is_reused_in_place_without_writing_fields,
    a_box_moves_to_another_type_of_the_same_size,
    a_shared_object_is_never_a_token,
    a_shared_marked_object_is_never_a_token,
    drop_token_frees_the_box,
    a_heap_dropped_with_an_outstanding_token_and_large_objects_is_clean,
);

type H = Heap<Checked>;

#[test]
fn a_token_cannot_be_used_twice_even_after_its_box_is_a_token_again() {
    let mut heap = H::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        let first = heap.into_token(p).unwrap().into_word();
        let (q, _) = heap.con_at(Token::from_word(first), 2, CONS_META).unwrap();
        // 同じ箱がまた token になっても、古い token は断る (docs/spec/object-model.md の「検査モードの token」)
        let second = heap.into_token(q).unwrap();
        assert_eq!(
            heap.con_at(Token::from_word(first), 2, CONS_META),
            Err(HeapError::StaleToken)
        );
        heap.drop_token(second).unwrap();
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn only_unique_data_becomes_a_token() {
    let mut heap = H::new();
    let f = heap.alloc_foreign(0u64, false);
    let p = list(&mut heap, 1);
    unsafe {
        assert_eq!(heap.into_token(f), Err(HeapError::NotTokenable));
        heap.dup(p).unwrap();
        assert_eq!(heap.into_token(p), Err(HeapError::NotUnique));
        heap.decref(p).unwrap();
        heap.decref(p).unwrap();
        heap.decref(f).unwrap();
    }
}

#[test]
fn a_token_of_another_class_is_an_error() {
    let mut heap = H::new();
    let p = cons(&mut heap, Word::UNIT, Word::UNIT);
    unsafe {
        let token = heap.into_token(p).unwrap();
        assert_eq!(
            heap.con_at(token, 10, CONS_META),
            Err(HeapError::WrongClass)
        );
    }
}

#[test]
fn reuse_in_place_rejects_a_kept_lin_bit_and_another_shape() {
    let mut heap = H::new();
    let lin = heap.alloc_data(CONS, 2, 0, true);
    let plain = cons(&mut heap, Word::UNIT, Word::UNIT);
    let other = cons(&mut heap, Word::UNIT, Word::UNIT);
    unsafe {
        let token = heap.into_token(lin).unwrap();
        assert_eq!(
            heap.reuse_in_place(token, CONS_META),
            Err(HeapError::LinBitKept)
        );
        let token = heap.into_token(plain).unwrap();
        assert_eq!(
            heap.reuse_in_place(token, PAIR_META),
            Err(HeapError::WrongMeta)
        );
        // ビットが欠けるのは安全の側なので通す
        let token = heap.into_token(other).unwrap();
        let reused = heap
            .reuse_in_place(token, CONS_META.with_lin(true))
            .unwrap();
        heap.decref(reused).unwrap();
    }
}

#[test]
fn an_unused_token_is_a_leak() {
    let mut heap = H::new();
    let p = cons(&mut heap, Word::UNIT, Word::UNIT);
    unsafe {
        let token = heap.into_token(p).unwrap();
        assert_eq!(heap.leaks(), BTreeMap::from([("Token", 1)]));
        heap.drop_token(token).unwrap();
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn another_worker_cannot_take_a_token() {
    let mut heap = H::new();
    let p = cons(&mut heap, Word::UNIT, Word::UNIT);
    heap.set_worker(1);
    assert_eq!(
        unsafe { heap.into_token(p) },
        Err(HeapError::WrongOwner {
            owner: 0,
            worker: 1
        })
    );
}

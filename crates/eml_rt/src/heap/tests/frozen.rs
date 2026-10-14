//! 凍結した領域 (docs/spec/object-model.md の「不死の物体の数え方」)。

use std::sync::Arc;

use super::string::text;
use crate::{Frozen, Heap, HeapError, Kind, Meta, Mode, Word};

/// 文字列の定数と、即値に収まらない Int の定数を持つ作り手。
pub(super) fn builder<M: Mode>() -> (Heap<M>, Word, Word) {
    let mut b = Heap::<M>::new();
    let s = b.alloc_string_immortal("constant");
    let big = unsafe { b.alloc_immortal(1, Meta::new(0, 0, Kind::BigInt, false)) };
    unsafe { b.set_raw(big, 0, i64::MAX as u64) }.unwrap();
    (b, s, big)
}

fn freezing_immortal_constants_succeeds<M: Mode>() {
    let (b, s, big) = builder::<M>();
    let region: Frozen<M> = b.freeze().unwrap();
    let region = Arc::new(region);
    // 付ける前でも、塊は自分の物体を落とさずに持つ
    drop(region);
    let _ = (s, big);
}

fn freezing_a_mortal_object_is_refused<M: Mode>() {
    let (mut b, _, _) = builder::<M>();
    let _cell = b.alloc_data(0, 1, 0, false);
    assert_eq!(b.freeze().unwrap_err(), HeapError::NotImmortal);
}

fn freezing_a_token_is_refused<M: Mode>() {
    let (mut b, _, _) = builder::<M>();
    let cell = b.alloc_data(0, 0, 1, false);
    let token = unsafe { b.into_token(cell) }.unwrap();
    let _ = token;
    assert_eq!(b.freeze().unwrap_err(), HeapError::NotImmortal);
}

fn freezing_a_foreign_object_is_refused<M: Mode>() {
    let (mut b, _, _) = builder::<M>();
    // Foreign の物体は rc 0 にできないので、rc の確かめで断られる
    let _f = b.alloc_foreign(7u64, false);
    assert_eq!(b.freeze().unwrap_err(), HeapError::NotImmortal);
}

fn freezing_after_freeing_keeps_only_live_blocks<M: Mode>() {
    let (mut b, s, _) = builder::<M>();
    // 空けたブロック (検査モードでは隔離) があっても凍結できる
    let tmp = b.alloc_string("temporary");
    unsafe { b.decref(tmp) }.unwrap();
    let region = b.freeze().unwrap();
    drop(region);
    let _ = s;
}

fn long_chains_drop_without_recursion<M: Mode>() {
    // 作り手が前の塊を付けて凍結するのを繰り返し、最後の `Arc` だけを持つ。落とすと、親への `Arc` を順に手放す
    let mut region: Option<Arc<Frozen<M>>> = None;
    for _ in 0..10_000 {
        let mut b = Heap::<M>::new();
        if let Some(parent) = region.take() {
            b.attach(parent);
        }
        region = Some(Arc::new(b.freeze().unwrap()));
    }
    drop(region);
}

/// 定数の塊を作って付けたヒープと、その定数。
fn attached<M: Mode>() -> (Heap<M>, Arc<Frozen<M>>, Word, Word) {
    let (b, s, big) = builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    let mut heap = Heap::<M>::new();
    heap.attach(region.clone());
    (heap, region, s, big)
}

fn region_words_go_through_every_operation<M: Mode>() {
    let (mut heap, _region, s, big) = attached::<M>();
    unsafe {
        heap.acquire_immortal(s).unwrap();
        heap.dup(s).unwrap();
        heap.decref(s).unwrap();
        assert_eq!(heap.meta(s).unwrap().kind(), Kind::String);
        assert!(!heap.is_unique(s).unwrap());
        assert!(!heap.is_last(s).unwrap());
        assert_eq!(heap.unbox_int(big).unwrap(), i64::MAX);
        assert_eq!(heap.scanned_len(s).unwrap(), 0);
        // 領域の左辺は写す道に進む。string_push は手元の参照を1つ消費する
        let r = heap.string_push(s, "!").unwrap();
        assert_ne!(r, s);
        assert_eq!(text(&heap, r), "constant!");
        // string_concat(s, s) は参照を2つ消費する
        heap.acquire_immortal(s).unwrap();
        heap.acquire_immortal(s).unwrap();
        let c = heap.string_concat(s, s).unwrap();
        assert_eq!(text(&heap, c), "constantconstant");
        heap.decref(r).unwrap();
        heap.decref(c).unwrap();
        heap.acquire_immortal(s).unwrap();
        heap.share(s, 1).unwrap();
        heap.decref(s).unwrap();
    }
    assert_eq!(text(&heap, s), "constant");
}

fn own_objects_hold_region_words<M: Mode>() {
    let (mut heap, _region, s, big) = attached::<M>();
    let pair = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.acquire_immortal(s).unwrap();
        heap.acquire_immortal(big).unwrap();
        heap.set_field(pair, 0, s).unwrap();
        heap.set_field(pair, 1, big).unwrap();
        // 残すフィールドを持つ release と、解放の連鎖の両方を通す
        heap.dup(pair).unwrap();
        heap.release(pair, 0, &[true, false]).unwrap();
        heap.decref(s).unwrap();
        heap.decref(pair).unwrap();
    }
    if let Some(check) = (&heap as &dyn std::any::Any).downcast_ref::<Heap<crate::Checked>>() {
        check.check_invariants().unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
    assert_eq!(text(&heap, s), "constant");
}

fn region_objects_are_not_counted<M: Mode>() {
    let (mut heap, _region, s, _) = attached::<M>();
    for _ in 0..3 {
        unsafe { heap.acquire_immortal(s) }.unwrap();
    }
    for _ in 0..5 {
        // 数えないので、手放しすぎも誤りにしない (docs/spec/object-model.md の「不死の物体の数え方」)
        unsafe { heap.decref(s) }.unwrap();
    }
    if M::CHECKED {
        assert_eq!(unsafe { super::raw::prefix_first(s) }, u32::MAX);
    }
}

fn chains_hold_constants_that_point_into_ancestors<M: Mode>() {
    let (b1, s1, _) = builder::<M>();
    let r1 = Arc::new(b1.freeze().unwrap());
    let mut b2 = Heap::<M>::new();
    b2.attach(r1.clone());
    // 祖先の定数を指す定数 (Some s1 の形)
    let some = unsafe { b2.alloc_immortal(1, Meta::new(1, 1, Kind::Data, false)) };
    // set_field は呼ぶ側が数えた参照を1つ渡す
    unsafe { b2.acquire_immortal(s1) }.unwrap();
    unsafe { b2.set_field(some, 0, s1) }.unwrap();
    let r2 = Arc::new(b2.freeze().unwrap());
    drop(r1);
    let mut heap = Heap::<M>::new();
    heap.attach(r2.clone());
    let inner = unsafe { heap.field(some, 0) }.unwrap();
    assert_eq!(text(&heap, inner), "constant");
}

fn attach_moves_to_a_descendant<M: Mode>() {
    let (b1, _, _) = builder::<M>();
    let r1 = Arc::new(b1.freeze().unwrap());
    let mut b2 = Heap::<M>::new();
    b2.attach(r1.clone());
    let s2 = b2.alloc_string_immortal("second");
    let r2 = Arc::new(b2.freeze().unwrap());
    let mut heap = Heap::<M>::new();
    heap.attach(r1.clone());
    heap.attach(r2.clone());
    assert_eq!(text(&heap, s2), "second");
}

fn attaching_a_non_descendant_panics<M: Mode>() {
    let (b1, _, _) = builder::<M>();
    let (b2, _, _) = builder::<M>();
    let r1 = Arc::new(b1.freeze().unwrap());
    let r2 = Arc::new(b2.freeze().unwrap());
    let mut heap = Heap::<M>::new();
    heap.attach(r1);
    heap.attach(r2);
}

// `both_modes!` は `#[should_panic]` を付けた関数を受けないので、モードごとに書く
#[test]
#[should_panic]
fn attaching_a_non_descendant_panics_fast() {
    attaching_a_non_descendant_panics::<crate::Fast>();
}

#[test]
#[should_panic]
fn attaching_a_non_descendant_panics_checked() {
    attaching_a_non_descendant_panics::<crate::Checked>();
}

fn drop_order_does_not_matter<M: Mode>() {
    for heap_first in [true, false] {
        let (heap, region, _, _) = attached::<M>();
        if heap_first {
            drop(heap);
            drop(region);
        } else {
            drop(region);
            drop(heap);
        }
    }
}

both_modes!(
    freezing_immortal_constants_succeeds,
    freezing_a_mortal_object_is_refused,
    freezing_a_token_is_refused,
    freezing_a_foreign_object_is_refused,
    freezing_after_freeing_keeps_only_live_blocks,
    long_chains_drop_without_recursion,
    region_words_go_through_every_operation,
    own_objects_hold_region_words,
    region_objects_are_not_counted,
    chains_hold_constants_that_point_into_ancestors,
    attach_moves_to_a_descendant,
    drop_order_does_not_matter,
);

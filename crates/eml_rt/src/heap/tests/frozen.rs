//! 凍結した領域 (docs/spec/object-model.md の「不死の物体の数え方」)。

use std::sync::Arc;

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
    // 鎖を作れるようになるまでは、1つの塊を落とす
    let (b, _, _) = builder::<M>();
    drop(Arc::new(b.freeze().unwrap()));
}

both_modes!(
    freezing_immortal_constants_succeeds,
    freezing_a_mortal_object_is_refused,
    freezing_a_token_is_refused,
    freezing_a_foreign_object_is_refused,
    freezing_after_freeing_keeps_only_live_blocks,
    long_chains_drop_without_recursion,
);

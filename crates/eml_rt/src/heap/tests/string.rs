//! 文字列の物体 (docs/spec/object-model.md の「種類」)。

use std::any::Any;
use std::collections::BTreeMap;

use super::{mortal, raw};
use crate::{Checked, Heap, HeapError, Kind, Mode, Word};

/// 文字列の中身を写して返す。
pub(super) fn text<M: Mode>(heap: &Heap<M>, w: Word) -> String {
    let p = unsafe { heap.string(w) }.unwrap();
    unsafe { &*p }.to_string()
}

/// 検査モードのリークの報告。速いモードはリークを数えないので `None` を返す。
fn leaks<M: Mode>(heap: &Heap<M>) -> Option<BTreeMap<&'static str, u64>> {
    (heap as &dyn Any)
        .downcast_ref::<Heap<Checked>>()
        .map(Heap::leaks)
}

/// 空、1語に収まる、語の境目、多バイトの文字、128 語を超える (大きい物体の道) 長さ。
fn samples() -> Vec<String> {
    vec![
        String::new(),
        "a".to_string(),
        "x".repeat(8),
        "日本語の文字列".to_string(),
        "y".repeat(2000),
    ]
}

fn strings_hold_their_text<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for s in samples() {
        let w = heap.alloc_string(&s);
        assert_eq!(unsafe { heap.meta(w) }.unwrap().kind(), Kind::String);
        assert_eq!(text(&heap, w), s);
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn immortal_strings_are_never_freed<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string_immortal("literal");
    unsafe { heap.acquire_immortal(w) }.unwrap();
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(text(&heap, w), "literal");
    assert!(!unsafe { heap.is_unique(w) }.unwrap());
    // 不死の物体は数が 0 なら漏れと数えない (docs/spec/object-model.md の「不死の物体の数え方」)
    assert!(leaks(&heap).is_none_or(|l| l.is_empty()));
}

fn leaks_are_named_string<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("left behind");
    if let Some(l) = leaks(&heap) {
        assert_eq!(l.get("String"), Some(&1));
    }
    unsafe { heap.decref(w) }.unwrap();
}

fn reading_a_data_object_as_a_string_is_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let d = heap.alloc_data(0, 0, 1, false);
    assert_eq!(unsafe { heap.string(d) }.unwrap_err(), HeapError::WrongKind);
    unsafe { heap.decref(d) }.unwrap();
}

fn raw_words_of_a_string_are_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("abc");
    assert_eq!(unsafe { heap.raw(w, 0) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(unsafe { heap.raw(w, 1) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(
        unsafe { heap.set_raw(w, 0, 1000) }.unwrap_err(),
        HeapError::WrongKind
    );
    assert_eq!(
        unsafe { heap.set_raw(w, 1, 0) }.unwrap_err(),
        HeapError::WrongKind
    );
    assert_eq!(text(&heap, w), "abc");
    assert_eq!(unsafe { raw::rc(w) }, 1);
    unsafe { heap.decref(w) }.unwrap();
}

both_modes!(
    strings_hold_their_text,
    immortal_strings_are_never_freed,
    leaks_are_named_string,
    reading_a_data_object_as_a_string_is_refused,
    raw_words_of_a_string_are_refused,
);

//! 文字列の物体 (docs/spec/object-model.md の「バイト列と文字列」)。

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

fn push(heap: &mut Heap<impl Mode>, w: Word, s: &str) -> Word {
    unsafe { heap.string_push(w, s) }.unwrap()
}

fn push_in_place_keeps_the_box<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("a");
    // 1 バイトの文字列のブロックは、どちらのモードでも少なくとも 8 バイトの容量を持つ
    let w2 = push(&mut heap, w, "b");
    assert_eq!(w2, w);
    assert_eq!(text(&heap, w2), "ab");
    unsafe { heap.decref(w2) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn push_grows_a_full_string<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut w = heap.alloc_string("a");
    let first = w;
    let mut expected = "a".to_string();
    while w == first {
        w = push(&mut heap, w, "b");
        expected.push('b');
    }
    assert_eq!(text(&heap, w), expected);
    // 古い箱は空けた
    assert_eq!(mortal(&heap), 1);
    unsafe { heap.decref(w) }.unwrap();
}

fn regrowth_is_logarithmic<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let n = 100_000;
    let mut w = heap.alloc_string("");
    let mut moves = 0;
    for _ in 0..n {
        let next = push(&mut heap, w, "x");
        if next != w {
            moves += 1;
        }
        w = next;
    }
    assert_eq!(text(&heap, w).len(), n);
    // 容量を倍にするので、作り直しは log2(n) 回 (約17回) と小さい級の刻みの分で済む
    assert!(moves <= 40, "{moves} regrowths for {n} pushes");
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn shared_left_is_copied<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("ab");
    unsafe { heap.dup(w) }.unwrap();
    let w2 = push(&mut heap, w, "c");
    assert_ne!(w2, w);
    assert_eq!(text(&heap, w), "ab");
    assert_eq!(text(&heap, w2), "abc");
    unsafe { heap.decref(w) }.unwrap();
    unsafe { heap.decref(w2) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn immortal_left_is_copied<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string_immortal("lit");
    unsafe { heap.acquire_immortal(w) }.unwrap();
    let w2 = push(&mut heap, w, "!");
    assert_ne!(w2, w);
    assert_eq!(text(&heap, w), "lit");
    assert_eq!(text(&heap, w2), "lit!");
    unsafe { heap.decref(w2) }.unwrap();
    if let Some(l) = leaks(&heap) {
        assert!(l.is_empty());
    }
}

fn push_onto_a_literal_copies_without_reading_capacity<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let lit = heap.alloc_string_immortal("literal");
    unsafe { heap.acquire_immortal(lit) }.unwrap();
    let r = unsafe { heap.string_push(lit, " + tail") }.unwrap();
    assert_ne!(r, lit);
    assert_eq!(text(&heap, r), "literal + tail");
    assert_eq!(text(&heap, lit), "literal");
    unsafe { heap.decref(r) }.unwrap();
}

fn large_strings_grow_across_pages<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let chunk = "z".repeat(50_000);
    let mut w = heap.alloc_string(&chunk);
    for _ in 0..4 {
        w = push(&mut heap, w, &chunk);
    }
    assert_eq!(text(&heap, w), chunk.repeat(5));
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn concat_consumes_both_sides<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = heap.alloc_string("foo");
    let b = heap.alloc_string("bar");
    let r = unsafe { heap.string_concat(a, b) }.unwrap();
    assert_eq!(text(&heap, r), "foobar");
    assert_eq!(mortal(&heap), 1);
    unsafe { heap.decref(r) }.unwrap();
}

fn concat_of_a_string_with_itself<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // `x ++ x` は `dup` で rc 2 になって届くので、容量に収まる長さでも、1語を埋める長さでも写す
    for s in ["ab", "abcdefgh"] {
        let x = heap.alloc_string(s);
        unsafe { heap.dup(x) }.unwrap();
        let r = unsafe { heap.string_concat(x, x) }.unwrap();
        assert_ne!(r, x);
        assert_eq!(text(&heap, r), format!("{s}{s}"));
        unsafe { heap.decref(r) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

/// 共有の側の left は一意でないので写す。rc -1 なら最後の参照なので写した後で空け、rc -2 なら参照を1つ手放して残す。
fn copy_of_a_left_on_the_shared_side<M: Mode>(concat: bool) {
    let mut heap = Heap::<M>::new();
    for extra in [false, true] {
        let x = heap.alloc_string("ab");
        unsafe { heap.share(x, 0) }.unwrap();
        if extra {
            unsafe { heap.dup(x) }.unwrap();
        }
        let r = if concat {
            let y = heap.alloc_string("c");
            unsafe { heap.string_concat(x, y) }.unwrap()
        } else {
            push(&mut heap, x, "c")
        };
        assert_ne!(r, x);
        assert_eq!(text(&heap, r), "abc");
        if extra {
            assert_eq!(unsafe { raw::rc(x) }, -1);
            assert_eq!(text(&heap, x), "ab");
            unsafe { heap.decref(x) }.unwrap();
        }
        assert_eq!(mortal(&heap), 1);
        unsafe { heap.decref(r) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn push_copies_a_left_on_the_shared_side<M: Mode>() {
    copy_of_a_left_on_the_shared_side::<M>(false);
}

fn concat_copies_a_left_on_the_shared_side<M: Mode>() {
    copy_of_a_left_on_the_shared_side::<M>(true);
}

fn errors_leave_the_heap_unchanged<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let s = heap.alloc_string("abc");
    let d = heap.alloc_data(0, 0, 1, false);
    let live = heap.live_objects().len();
    assert_eq!(
        unsafe { heap.string_push(d, "x") }.unwrap_err(),
        HeapError::WrongKind
    );
    assert_eq!(
        unsafe { heap.string_concat(s, d) }.unwrap_err(),
        HeapError::WrongKind
    );
    assert_eq!(
        unsafe { heap.string_concat(d, s) }.unwrap_err(),
        HeapError::WrongKind
    );
    assert_eq!(heap.live_objects().len(), live);
    assert_eq!(text(&heap, s), "abc");
    assert_eq!(unsafe { raw::rc(s) }, 1);
    unsafe { heap.decref(s) }.unwrap();
    assert_eq!(
        unsafe { heap.string_push(s, "x") }.unwrap_err(),
        HeapError::UseAfterFree
    );
    unsafe { heap.decref(d) }.unwrap();
}

fn concat_of_one_reference_twice_is_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let x = heap.alloc_string("ab");
    let live = heap.live_objects().len();
    assert_eq!(
        unsafe { heap.string_concat(x, x) }.unwrap_err(),
        HeapError::UseAfterFree
    );
    assert_eq!(heap.live_objects().len(), live);
    assert_eq!(text(&heap, x), "ab");
    assert_eq!(unsafe { raw::rc(x) }, 1);
    unsafe { heap.decref(x) }.unwrap();
}

fn over_released_immortal_left_is_refused_before_allocating<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    // 外に出ている参照を作っていない不死の文字列
    let w = heap.alloc_string_immortal("lit");
    let live = heap.live_objects().len();
    assert_eq!(
        unsafe { heap.string_push(w, "!") }.unwrap_err(),
        HeapError::ImmortalOverRelease
    );
    assert_eq!(heap.live_objects().len(), live);
}

fn concat_of_a_shared_last_reference_twice_is_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let y = heap.alloc_string("ab");
    unsafe { heap.share(y, 0) }.unwrap();
    let live = heap.live_objects().len();
    assert_eq!(
        unsafe { heap.string_concat(y, y) }.unwrap_err(),
        HeapError::UseAfterFree
    );
    assert_eq!(heap.live_objects().len(), live);
    assert_eq!(text(&heap, y), "ab");
    assert_eq!(unsafe { raw::rc(y) }, -1);
    unsafe { heap.decref(y) }.unwrap();
}

fn concat_of_an_immortal_with_itself_needs_two_references<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string_immortal("lit");
    unsafe { heap.acquire_immortal(w) }.unwrap();
    let live = heap.live_objects().len();
    assert_eq!(
        unsafe { heap.string_concat(w, w) }.unwrap_err(),
        HeapError::ImmortalOverRelease
    );
    assert_eq!(heap.live_objects().len(), live);
    assert_eq!(unsafe { raw::prefix_first(w) }, 1);
    unsafe { heap.acquire_immortal(w) }.unwrap();
    let r = unsafe { heap.string_concat(w, w) }.unwrap();
    assert_eq!(text(&heap, r), "litlit");
    assert_eq!(unsafe { raw::prefix_first(w) }, 0);
    unsafe { heap.decref(r) }.unwrap();
    assert!(leaks(&heap).is_some_and(|l| l.is_empty()));
}

fn another_worker_cannot_push_to_a_local_left<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_string("ab");
    heap.set_worker(1);
    let live = heap.live_objects().len();
    assert_eq!(
        unsafe { heap.string_push(w, "c") }.unwrap_err(),
        HeapError::WrongOwner {
            owner: 0,
            worker: 1
        }
    );
    assert_eq!(heap.live_objects().len(), live);
    heap.set_worker(0);
    assert_eq!(text(&heap, w), "ab");
    assert_eq!(unsafe { raw::rc(w) }, 1);
    unsafe { heap.decref(w) }.unwrap();
}

/// ほかの worker の一意なバイト列は、検査モードで貸す前に断る。`bytes_commit` と同じ誤りで、外はまだ何も書いていない。
fn another_worker_cannot_lend_a_local_bytes<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_bytes_room(8);
    heap.set_worker(1);
    let wrong = HeapError::WrongOwner {
        owner: 0,
        worker: 1,
    };
    assert_eq!(unsafe { heap.bytes_lend(w) }.err(), Some(wrong));
    assert_eq!(unsafe { heap.bytes_commit(w, 0) }.unwrap_err(), wrong);
    heap.set_worker(0);
    assert_eq!(unsafe { raw::rc(w) }, 1);
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(super::mortal(&heap), 0);
}

/// 容量を決めて作った空の文字列は、容量の中の追記をその場で受ける (`String.show` が1回だけ確保するため)。
fn a_room_string_takes_pushes_in_place<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let s = heap.alloc_string_room(10);
    assert_eq!(text(&heap, s), "");
    unsafe {
        let t = heap.string_push(s, "abc").unwrap();
        assert_eq!(t, s);
        let u = heap.string_push(t, "defghij").unwrap();
        assert_eq!(u, s);
        assert_eq!(text(&heap, u), "abcdefghij");
        heap.decref(u).unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
}

fn a_unique_foreign_value_can_be_written<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(vec![1u8], true);
    unsafe {
        let p = heap.foreign_unique::<Vec<u8>>(f).unwrap();
        (*p).push(2);
        assert_eq!(*heap.foreign::<Vec<u8>>(f).unwrap(), vec![1, 2]);
        assert_eq!(heap.foreign_unique::<String>(f), Err(HeapError::WrongKind));
        heap.decref(f).unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
}

/// 参照が2つある物体、共有した物体、ほかの worker の物体、手放した物体は、書き換えのために借りられない。
fn a_shared_or_foreign_workers_value_is_not_written<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(0u64, false);
    let g = heap.alloc_foreign(0u64, false);
    unsafe {
        heap.dup(f).unwrap();
        assert_eq!(heap.foreign_unique::<u64>(f), Err(HeapError::NotUnique));
        heap.decref(f).unwrap();
        heap.set_worker(1);
        assert!(matches!(
            heap.foreign_unique::<u64>(f),
            Err(HeapError::WrongOwner { .. })
        ));
        heap.set_worker(0);
        heap.decref(f).unwrap();
        assert_eq!(heap.foreign_unique::<u64>(f), Err(HeapError::UseAfterFree));
        // 共有した物体はほかの worker からも見えるので、参照が1つでも借りられない。rc が負なので `NotUnique` になる
        heap.share(g, 0).unwrap();
        assert_eq!(heap.foreign_unique::<u64>(g), Err(HeapError::NotUnique));
        heap.decref(g).unwrap();
    }
    assert_eq!(super::mortal(&heap), 0);
}

both_modes!(
    strings_hold_their_text,
    immortal_strings_are_never_freed,
    leaks_are_named_string,
    reading_a_data_object_as_a_string_is_refused,
    raw_words_of_a_string_are_refused,
    push_in_place_keeps_the_box,
    push_grows_a_full_string,
    regrowth_is_logarithmic,
    shared_left_is_copied,
    immortal_left_is_copied,
    push_onto_a_literal_copies_without_reading_capacity,
    large_strings_grow_across_pages,
    concat_consumes_both_sides,
    concat_of_a_string_with_itself,
    errors_leave_the_heap_unchanged,
    concat_of_one_reference_twice_is_refused,
    over_released_immortal_left_is_refused_before_allocating,
    push_copies_a_left_on_the_shared_side,
    concat_copies_a_left_on_the_shared_side,
    concat_of_a_shared_last_reference_twice_is_refused,
    concat_of_an_immortal_with_itself_needs_two_references,
    another_worker_cannot_push_to_a_local_left,
    another_worker_cannot_lend_a_local_bytes,
    a_room_string_takes_pushes_in_place,
    a_unique_foreign_value_can_be_written,
    a_shared_or_foreign_workers_value_is_not_written,
);

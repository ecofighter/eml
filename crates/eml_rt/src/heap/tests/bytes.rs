//! バイト列の物体 (docs/spec/object-model.md の「バイト列と文字列」)。

use std::any::Any;
use std::sync::Arc;

use super::ctx::ctx;
use super::mortal;
use super::string::text;
use crate::{Checked, Heap, HeapError, Kind, Mode, Word};

/// バイト列の中身を写して返す。
fn body<M: Mode>(heap: &Heap<M>, w: Word) -> Vec<u8> {
    let p = unsafe { heap.bytes(w) }.unwrap();
    unsafe { &*p }.to_vec()
}

/// 空、UTF-8 でない、1語に収まる、語の境目、128 語を超える (大きい物体の道) 長さ。
fn samples() -> Vec<Vec<u8>> {
    vec![
        Vec::new(),
        vec![0xff, 0x00, 0x80],
        vec![b'x'; 8],
        "日本語".as_bytes().to_vec(),
        (0..2000).map(|i| i as u8).collect(),
    ]
}

fn bytes_hold_their_body<M: Mode>() {
    let mut heap = Heap::<M>::new();
    for b in samples() {
        let w = heap.alloc_bytes(&b);
        assert_eq!(unsafe { heap.meta(w) }.unwrap().kind(), Kind::Bytes);
        assert_eq!(body(&heap, w), b);
        assert_eq!(unsafe { heap.scanned_len(w) }, Ok(0));
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

/// 容量のバイト数。ブロックのペイロードの語の数から長さの1語を引く (docs/spec/object-model.md の「中身の不変条件」)。
fn capacity<M: Mode>(heap: &Heap<M>, w: Word) -> usize {
    8 * (heap.core.cap_get(w.as_ptr()) - 1)
}

fn a_room_bytes_object_starts_empty<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_bytes_room(100);
    assert_eq!(unsafe { heap.meta(w) }.unwrap().kind(), Kind::Bytes);
    assert_eq!(body(&heap, w), b"");
    assert!(capacity(&heap, w) >= 100, "{}", capacity(&heap, w));
    unsafe { heap.decref(w) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn leaks_are_named_bytes<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_bytes(b"left behind");
    if let Some(l) = (&heap as &dyn Any)
        .downcast_ref::<Heap<Checked>>()
        .map(Heap::leaks)
    {
        assert_eq!(l.get("Bytes"), Some(&1));
    }
    unsafe { heap.decref(w) }.unwrap();
}

/// `String` と `Bytes` は同じ形の別の種類で、検査モードはもう一方の読みを断る。
fn strings_and_bytes_are_not_read_as_each_other<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let s = heap.alloc_string("abc");
    let b = heap.alloc_bytes(b"abc");
    assert_eq!(unsafe { heap.bytes(s) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(unsafe { heap.string(b) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(
        unsafe { heap.string_push(b, "d") }.unwrap_err(),
        HeapError::WrongKind
    );
    unsafe {
        heap.decref(s).unwrap();
        heap.decref(b).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn raw_words_of_bytes_are_refused<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut heap = Heap::<M>::new();
    let w = heap.alloc_bytes(b"abc");
    assert_eq!(unsafe { heap.raw(w, 0) }.unwrap_err(), HeapError::WrongKind);
    assert_eq!(
        unsafe { heap.set_raw(w, 1, 0) }.unwrap_err(),
        HeapError::WrongKind
    );
    assert_eq!(body(&heap, w), b"abc");
    unsafe { heap.decref(w) }.unwrap();
}

/// 不死のバイト列は凍結でき、付けた塊から読める。
fn immortal_bytes_are_frozen<M: Mode>() {
    let mut b = Heap::<M>::new();
    let w = b.alloc_bytes_immortal(&[0xfe, 0xff]);
    let region = Arc::new(b.freeze().unwrap());
    let mut heap = Heap::<M>::new();
    heap.attach(region);
    unsafe {
        heap.acquire_immortal(w).unwrap();
        assert_eq!(heap.meta(w).unwrap().kind(), Kind::Bytes);
        assert_eq!(body(&heap, w), [0xfe, 0xff]);
        assert!(!heap.is_unique(w).unwrap());
        heap.decref(w).unwrap();
    }
}

/// 回復はバイト列を、同じ中身の新しいバイト列に写す。`alloc_bytes_room` が作った余りの容量は写さない。
fn recovery_copies_bytes<M: Mode>() {
    let mut ctx = ctx::<M>();
    let n = samples().len();
    unsafe {
        let heap = ctx.heap_mut();
        let mut ws: Vec<Word> = samples().iter().map(|b| heap.alloc_bytes(b)).collect();
        ws.push(heap.alloc_string("text"));
        let room = heap.alloc_bytes_room(256);
        assert!(capacity(heap, room) >= 256);
        ws.push(room);
        for (i, w) in ws.into_iter().enumerate() {
            ctx.root_set(i as u32, w).unwrap();
        }
        ctx.recover().unwrap();
    }
    assert_eq!(mortal(ctx.heap()), n + 2);
    for (i, b) in samples().iter().enumerate() {
        unsafe {
            let w = ctx.root_get(i as u32).unwrap();
            assert_eq!(ctx.heap().meta(w).unwrap().kind(), Kind::Bytes);
            assert_eq!(&body(ctx.heap(), w), b);
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    let s = unsafe { ctx.root_get(n as u32) }.unwrap();
    assert_eq!(text(ctx.heap(), s), "text");
    unsafe { ctx.heap_mut().decref(s) }.unwrap();
    let room = unsafe { ctx.root_get(n as u32 + 1) }.unwrap();
    assert_eq!(body(ctx.heap(), room), b"");
    assert!(
        capacity(ctx.heap(), room) < 256,
        "{}",
        capacity(ctx.heap(), room)
    );
    unsafe { ctx.heap_mut().decref(room) }.unwrap();
    for w in ctx.take_roots().into_iter().flatten() {
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

both_modes!(
    bytes_hold_their_body,
    a_room_bytes_object_starts_empty,
    leaks_are_named_bytes,
    strings_and_bytes_are_not_read_as_each_other,
    raw_words_of_bytes_are_refused,
    immortal_bytes_are_frozen,
    recovery_copies_bytes,
);

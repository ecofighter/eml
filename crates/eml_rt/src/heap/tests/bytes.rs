//! バイト列の物体 (docs/spec/object-model.md の「バイト列と文字列」)。

use std::any::Any;
use std::sync::Arc;

use super::ctx::ctx;
use super::host::host;
use super::mortal;
use super::string::text;
use crate::externs::{EQ, FALSE, Fault, GT, IndexFault, LT, TRUE};
use crate::{Checked, Control, Ctx, Heap, HeapError, Kind, Limits, Mode, Stop, Word};

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

fn index(index: i64, len: i64) -> Fault {
    Fault::Index(Box::new(IndexFault::BytesIndex { index, len }))
}

fn slice(start: i64, end: i64, len: i64) -> Fault {
    Fault::Index(Box::new(IndexFault::BytesSlice { start, end, len }))
}

/// 読むだけの操作は、引数を借りて参照の数を変えない (docs/spec/object-model.md の「extern」)。
fn bytes_are_read_by_position<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let w = ctx.heap_mut().alloc_bytes(&[0x00, 0xff, b'a']);
        assert_eq!(ctx.bytes_length(w), Ok(3));
        assert_eq!(ctx.bytes_get(1, w), Ok(0xff));
        assert_eq!(ctx.bytes_get(3, w), Err(index(3, 3)));
        assert_eq!(ctx.bytes_get(-1, w), Err(index(-1, 3)));
        let part = ctx.bytes_slice(1, 3, w).unwrap();
        assert_eq!(body(ctx.heap(), part), [0xff, b'a']);
        ctx.heap_mut().decref(part).unwrap();
        assert_eq!(ctx.bytes_slice(2, 1, w), Err(slice(2, 1, 3)));
        assert_eq!(ctx.bytes_slice(0, 4, w), Err(slice(0, 4, 3)));
        let empty = ctx.bytes_slice(3, 3, w).unwrap();
        assert_eq!(body(ctx.heap(), empty), b"");
        ctx.heap_mut().decref(empty).unwrap();
        assert!(ctx.heap().is_unique(w).unwrap());
        ctx.heap_mut().decref(w).unwrap();
        let large: Vec<u8> = (0..2000).map(|i| i as u8).collect();
        let w = ctx.heap_mut().alloc_bytes(&large);
        assert_eq!(ctx.bytes_length(w), Ok(2000));
        assert_eq!(ctx.bytes_get(1999, w), Ok(1999 % 256));
        let part = ctx.bytes_slice(7, 1500, w).unwrap();
        assert_eq!(body(ctx.heap(), part), &large[7..1500]);
        ctx.heap_mut().decref(part).unwrap();
        ctx.heap_mut().decref(w).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn utf8_is_validated<M: Mode>() {
    let mut ctx = ctx::<M>();
    for (b, valid) in [
        (&b""[..], TRUE),
        ("日本語".as_bytes(), TRUE),
        (&[0xe6, 0x97][..], FALSE),
        (&[0xff][..], FALSE),
        (&[0xed, 0xa0, 0x80][..], FALSE),
    ] {
        unsafe {
            let w = ctx.heap_mut().alloc_bytes(b);
            assert_eq!(ctx.bytes_valid_utf8(w), Ok(valid), "{b:?}");
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 比べるのはバイトの辞書順で、短いほうが長いほうの前の部分なら短いほうが小さい。
fn bytes_are_compared_in_byte_order<M: Mode>() {
    let mut ctx = ctx::<M>();
    for (a, b, order) in [
        (&b""[..], &b""[..], EQ),
        (&b"ab"[..], &b"abc"[..], LT),
        (&[0xff][..], &[0x00, 0x00][..], GT),
        (&b"abd"[..], &b"abc"[..], GT),
    ] {
        unsafe {
            let x = ctx.heap_mut().alloc_bytes(a);
            let y = ctx.heap_mut().alloc_bytes(b);
            assert_eq!(ctx.bytes_compare(x, y), Ok(order), "{a:?} {b:?}");
            let eq = if order == EQ { TRUE } else { FALSE };
            assert_eq!(ctx.bytes_eq(x, y), Ok(eq));
            assert_eq!(ctx.bytes_ne(x, y), Ok(TRUE - eq));
            assert!(ctx.heap().is_unique(x).unwrap());
            ctx.heap_mut().decref(x).unwrap();
            ctx.heap_mut().decref(y).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn bytes_are_made_of_one_byte<M: Mode>() {
    let mut ctx = ctx::<M>();
    for n in [0, 5, 64, 65, 2000] {
        unsafe {
            let w = ctx.bytes_make(n, 0xab).unwrap();
            assert_eq!(body(ctx.heap(), w), vec![0xab; n as usize]);
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    unsafe {
        assert_eq!(
            ctx.bytes_make(-1, 0),
            Err(Fault::Index(Box::new(IndexFault::BytesLength { len: -1 })))
        );
        assert_eq!(
            ctx.bytes_make(i64::MAX, 0),
            Err(Fault::Stopped(Stop::MemoryLimit))
        );
    }
    assert_eq!(mortal(ctx.heap()), 0);
    let limits = Limits {
        max_bytes: 1 << 20,
        ..Limits::default()
    };
    let mut ctx = Ctx::<M>::new(Arc::new(Control::new(limits)), host(&Arc::default()));
    assert_eq!(
        unsafe { ctx.bytes_make(1 << 20, 0) },
        Err(Fault::Stopped(Stop::MemoryLimit))
    );
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 一意のバイト列はその場で書き換え、語も容量も変えない。
fn unique_bytes_are_changed_in_place<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let w = ctx.heap_mut().alloc_bytes_room(16);
        let w1 = ctx.bytes_push(1, w).unwrap();
        assert_eq!(w1, w);
        let w2 = ctx.bytes_push(2, w1).unwrap();
        assert_eq!(w2, w);
        let w3 = ctx.bytes_set(0, 9, w2).unwrap();
        assert_eq!(w3, w);
        let cap = capacity(ctx.heap(), w3);
        let w4 = ctx.bytes_truncate(1, w3).unwrap();
        assert_eq!(w4, w);
        assert_eq!(capacity(ctx.heap(), w4), cap);
        assert_eq!(body(ctx.heap(), w4), [9]);
        ctx.heap_mut().decref(w4).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 共有されたバイト列は写してから書き、元の中身は変えない。
fn shared_bytes_are_copied<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.heap_mut().alloc_bytes(b"abc");
        ctx.heap_mut().dup(a).unwrap();
        let b = ctx.bytes_push(b'd' as i64, a).unwrap();
        assert_ne!(b, a);
        assert_eq!(body(ctx.heap(), b), b"abcd");
        ctx.heap_mut().dup(a).unwrap();
        let c = ctx.bytes_set(1, b'x' as i64, a).unwrap();
        assert_ne!(c, a);
        assert_eq!(body(ctx.heap(), c), b"axc");
        ctx.heap_mut().dup(a).unwrap();
        let d = ctx.bytes_truncate(2, a).unwrap();
        assert_ne!(d, a);
        assert_eq!(body(ctx.heap(), d), b"ab");
        assert_eq!(body(ctx.heap(), a), b"abc");
        assert!(ctx.heap().is_unique(a).unwrap());
        for w in [a, b, c, d] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 範囲の外の位置は、受け取った参照を手放して誤りを返す。共有されていれば、ほかの参照の分は残る。
fn out_of_range_writes_release_their_argument<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.heap_mut().alloc_bytes(b"abc");
        ctx.heap_mut().dup(a).unwrap();
        assert_eq!(ctx.bytes_set(3, 0, a), Err(index(3, 3)));
        assert!(ctx.heap().is_unique(a).unwrap());
        ctx.heap_mut().dup(a).unwrap();
        assert_eq!(ctx.bytes_truncate(4, a), Err(slice(0, 4, 3)));
        ctx.heap_mut().dup(a).unwrap();
        assert_eq!(ctx.bytes_truncate(-1, a), Err(slice(0, -1, 3)));
        let src = ctx.heap_mut().alloc_bytes(b"xy");
        ctx.heap_mut().dup(a).unwrap();
        assert_eq!(ctx.bytes_append_part(1, 3, src, a), Err(slice(1, 3, 2)));
        assert_eq!(body(ctx.heap(), a), b"abc");
        assert!(ctx.heap().is_unique(a).unwrap());
        assert!(ctx.heap().is_unique(src).unwrap());
        ctx.heap_mut().decref(src).unwrap();
        assert_eq!(ctx.bytes_set(0, 0, a), Ok(a));
        assert_eq!(ctx.bytes_truncate(9, a), Err(slice(0, 9, 3)));
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// `append` は両方の引数の参照を受け取る (`String.append` と同じ)。
fn bytes_are_appended<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.heap_mut().alloc_bytes(b"ab");
        let b = ctx.heap_mut().alloc_bytes(&[0xff]);
        let w = ctx.bytes_append(a, b).unwrap();
        assert_eq!(body(ctx.heap(), w), [b'a', b'b', 0xff]);
        // 同じ物体を2つの引数に渡すときは参照が2つあり、写す道を通る
        ctx.heap_mut().dup(w).unwrap();
        let ww = ctx.bytes_append(w, w).unwrap();
        assert_eq!(body(ctx.heap(), ww), [b'a', b'b', 0xff, b'a', b'b', 0xff]);
        ctx.heap_mut().decref(ww).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// `append_part` は `src` の `from..to` を `dst` の後ろに足す。`src` は借りるだけで参照の数を変えない。
fn parts_of_bytes_are_appended<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let dst = ctx.heap_mut().alloc_bytes_room(32);
        let dst = ctx.bytes_push(b'-' as i64, dst).unwrap();
        let src = ctx.heap_mut().alloc_bytes(b"0123456789");
        let w = ctx.bytes_append_part(2, 5, src, dst).unwrap();
        assert_eq!(w, dst);
        assert_eq!(body(ctx.heap(), w), b"-234");
        let w = ctx.bytes_append_part(10, 10, src, w).unwrap();
        assert_eq!(body(ctx.heap(), w), b"-234");
        assert!(ctx.heap().is_unique(src).unwrap());
        ctx.heap_mut().decref(src).unwrap();
        // `src` と `dst` が同じ物体なら参照は2つあり、その場では書かずに写す
        ctx.heap_mut().dup(w).unwrap();
        let ww = ctx.bytes_append_part(1, 3, w, w).unwrap();
        assert_ne!(ww, w);
        assert_eq!(body(ctx.heap(), ww), b"-23423");
        assert_eq!(body(ctx.heap(), w), b"-234");
        assert!(ctx.heap().is_unique(w).unwrap());
        ctx.heap_mut().decref(w).unwrap();
        ctx.heap_mut().decref(ww).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 検査モードは、同じ物体を `src` と `dst` に渡しながら参照が1つしかない呼び出しを `NotUnique` で断り、ヒープを
/// 変えない (docs/spec/lir.md の「extern の借用」)。
fn appending_a_part_of_itself_needs_two_references<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        let w = ctx.heap_mut().alloc_bytes(b"abc");
        assert_eq!(
            ctx.bytes_append_part(0, 1, w, w),
            Err(Fault::Heap(HeapError::NotUnique))
        );
        assert_eq!(body(ctx.heap(), w), b"abc");
        assert!(ctx.heap().is_unique(w).unwrap());
        ctx.heap_mut().decref(w).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 検査モードは、バイト列でない引数を断り、ヒープを変えない。
fn bytes_externs_refuse_a_string<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    fn wrong<T>() -> Result<T, Fault> {
        Err(Fault::Heap(HeapError::WrongKind))
    }
    unsafe {
        let s = ctx.heap_mut().alloc_string("abc");
        let b = ctx.heap_mut().alloc_bytes(b"abc");
        assert_eq!(ctx.bytes_length(s), wrong());
        assert_eq!(ctx.bytes_get(0, s), wrong());
        assert_eq!(ctx.bytes_valid_utf8(s), wrong());
        assert_eq!(ctx.bytes_eq(b, s), wrong());
        assert_eq!(ctx.bytes_ne(s, b), wrong());
        assert_eq!(ctx.bytes_compare(b, s), wrong());
        assert_eq!(ctx.bytes_slice(0, 1, s), wrong());
        assert_eq!(ctx.bytes_push(0, s), wrong());
        assert_eq!(ctx.bytes_append(b, s), wrong());
        assert_eq!(ctx.bytes_append(s, b), wrong());
        assert_eq!(ctx.bytes_append_part(0, 1, s, b), wrong());
        assert_eq!(ctx.bytes_append_part(0, 1, b, s), wrong());
        assert_eq!(ctx.bytes_truncate(0, s), wrong());
        assert_eq!(ctx.bytes_set(0, 0, s), wrong());
        assert_eq!(text(ctx.heap(), s), "abc");
        assert_eq!(body(ctx.heap(), b), b"abc");
        assert!(ctx.heap().is_unique(s).unwrap());
        assert!(ctx.heap().is_unique(b).unwrap());
        ctx.heap_mut().decref(s).unwrap();
        ctx.heap_mut().decref(b).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 検査モードは、バイト列に写したバイトも `string_bytes_copied` に数える。一意な物体に足すときは足した分だけを、
/// 共有された物体から作り直すときは結果に写した分を数え、その場で書き換えるだけなら数えない
/// (docs/spec/object-model.md の「仕事の回数」)。
fn bytes_copies_are_counted<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    let mut last = 0;
    let mut copied = |ctx: &Ctx<M>| {
        let now = ctx.stats().string_bytes_copied;
        let d = now - last;
        last = now;
        d
    };
    unsafe {
        let m = ctx.bytes_make(5, b'a' as i64).unwrap();
        assert_eq!(copied(&ctx), 5);
        let part = ctx.bytes_slice(1, 4, m).unwrap();
        assert_eq!(copied(&ctx), 3);
        ctx.heap_mut().decref(m).unwrap();
        let a = ctx.heap_mut().alloc_bytes_room(32);
        let a = ctx.bytes_push(1, a).unwrap();
        assert_eq!(copied(&ctx), 1);
        ctx.heap_mut().dup(a).unwrap();
        let b = ctx.bytes_push(2, a).unwrap();
        assert_eq!(copied(&ctx), 2);
        let a = ctx.bytes_append_part(0, 2, b, a).unwrap();
        assert_eq!(copied(&ctx), 2);
        ctx.heap_mut().dup(a).unwrap();
        let c = ctx.bytes_append_part(1, 3, part, a).unwrap();
        assert_eq!(copied(&ctx), 3 + 2);
        let a = ctx.bytes_set(0, 7, a).unwrap();
        assert_eq!(copied(&ctx), 0);
        ctx.heap_mut().dup(a).unwrap();
        let d = ctx.bytes_set(0, 8, a).unwrap();
        assert_eq!(copied(&ctx), 3);
        let a = ctx.bytes_truncate(2, a).unwrap();
        assert_eq!(copied(&ctx), 0);
        ctx.heap_mut().dup(a).unwrap();
        let e = ctx.bytes_truncate(1, a).unwrap();
        assert_eq!(copied(&ctx), 1);
        let a = ctx.bytes_append(a, part).unwrap();
        assert_eq!(copied(&ctx), 3);
        ctx.heap_mut().dup(a).unwrap();
        let f = ctx.bytes_append(a, b).unwrap();
        assert_eq!(copied(&ctx), 5 + 2);
        for w in [a, c, d, e, f] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 文字列はバイト列に写される。元の文字列の参照は手放し、共有やリテラルでも同じ結果になる。
fn strings_become_bytes<M: Mode>() {
    let mut ctx = ctx::<M>();
    let mut literals = 0;
    let large = "ab日本語".repeat(300);
    for s in ["", "abc", "日本語", large.as_str()] {
        unsafe {
            let w = ctx.heap_mut().alloc_string(s);
            let b = ctx.bytes_of_string(w).unwrap();
            assert_eq!(body(ctx.heap(), b), s.as_bytes());
            ctx.heap_mut().decref(b).unwrap();

            let w = ctx.heap_mut().alloc_string(s);
            ctx.heap_mut().dup(w).unwrap();
            let b = ctx.bytes_of_string(w).unwrap();
            assert_eq!(body(ctx.heap(), b), s.as_bytes());
            assert_eq!(text(ctx.heap(), w), s);
            assert!(ctx.heap().is_unique(w).unwrap());
            ctx.heap_mut().decref(b).unwrap();
            ctx.heap_mut().decref(w).unwrap();

            let lit = ctx.heap_mut().alloc_string_immortal(s);
            literals += 1;
            ctx.heap_mut().dup(lit).unwrap();
            let b = ctx.bytes_of_string(lit).unwrap();
            assert_eq!(body(ctx.heap(), b), s.as_bytes());
            assert_eq!(text(ctx.heap(), lit), s);
            ctx.heap_mut().decref(b).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), literals);
}

/// 妥当な UTF-8 のバイト列は、同じバイトの文字列に写される。
fn bytes_decode_to_strings<M: Mode>() {
    let mut ctx = ctx::<M>();
    let mut literals = 0;
    let large = "ab日本語".repeat(300);
    for s in ["", "abc", "日本語", large.as_str()] {
        unsafe {
            let b = ctx.heap_mut().alloc_bytes(s.as_bytes());
            let w = ctx.string_decode(b).unwrap();
            assert_eq!(text(ctx.heap(), w), s);
            ctx.heap_mut().decref(w).unwrap();

            let b = ctx.heap_mut().alloc_bytes(s.as_bytes());
            ctx.heap_mut().dup(b).unwrap();
            let w = ctx.string_decode(b).unwrap();
            assert_eq!(text(ctx.heap(), w), s);
            assert_eq!(body(ctx.heap(), b), s.as_bytes());
            assert!(ctx.heap().is_unique(b).unwrap());
            ctx.heap_mut().decref(w).unwrap();
            ctx.heap_mut().decref(b).unwrap();

            let lit = ctx.heap_mut().alloc_bytes_immortal(s.as_bytes());
            literals += 1;
            ctx.heap_mut().dup(lit).unwrap();
            let w = ctx.string_decode(lit).unwrap();
            assert_eq!(text(ctx.heap(), w), s);
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), literals);
}

/// 妥当でないバイト列は、最初の妥当でない位置を持つ誤りになる。引数の参照は手放し、共有されていればほかの参照は残る。
fn invalid_bytes_do_not_decode<M: Mode>() {
    let mut ctx = ctx::<M>();
    let mut large = "日本語".repeat(500).into_bytes();
    large.push(0xff);
    for (b, at) in [
        (b"\xff".to_vec(), 0i64),
        (b"abc\xff".to_vec(), 3),
        (b"ab\xe6\x97".to_vec(), 2),
        ("日本語".as_bytes()[..8].to_vec(), 6),
        (vec![0xed, 0xa0, 0x80], 0),
        (large.clone(), large.len() as i64 - 1),
    ] {
        unsafe {
            let w = ctx.heap_mut().alloc_bytes(&b);
            assert_eq!(
                ctx.string_decode(w),
                Err(Fault::Utf8 { at }),
                "{:?}",
                &b[..b.len().min(8)]
            );
            let w = ctx.heap_mut().alloc_bytes(&b);
            ctx.heap_mut().dup(w).unwrap();
            assert_eq!(ctx.string_decode(w), Err(Fault::Utf8 { at }));
            assert_eq!(body(ctx.heap(), w), b);
            assert!(ctx.heap().is_unique(w).unwrap());
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 検査モードは、種類の違う引数を断り、ヒープを変えない。
fn conversions_refuse_the_wrong_kind<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    fn wrong<T>() -> Result<T, Fault> {
        Err(Fault::Heap(HeapError::WrongKind))
    }
    unsafe {
        let s = ctx.heap_mut().alloc_string("abc");
        let b = ctx.heap_mut().alloc_bytes(b"abc");
        assert_eq!(ctx.bytes_of_string(b), wrong());
        assert_eq!(ctx.string_decode(s), wrong());
        assert_eq!(text(ctx.heap(), s), "abc");
        assert_eq!(body(ctx.heap(), b), b"abc");
        assert!(ctx.heap().is_unique(s).unwrap());
        assert!(ctx.heap().is_unique(b).unwrap());
        ctx.heap_mut().decref(s).unwrap();
        ctx.heap_mut().decref(b).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 検査モードは、どちらの変換も写したバイトを `string_bytes_copied` に数える。妥当でないバイト列は何も写さない
/// (docs/spec/object-model.md の「仕事の回数」)。
fn conversions_are_counted<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    let mut last = 0;
    let mut copied = |ctx: &Ctx<M>| {
        let now = ctx.stats().string_bytes_copied;
        let d = now - last;
        last = now;
        d
    };
    unsafe {
        let s = ctx.heap_mut().alloc_string("日本語");
        copied(&ctx);
        let b = ctx.bytes_of_string(s).unwrap();
        assert_eq!(copied(&ctx), 9);
        ctx.heap_mut().dup(b).unwrap();
        let t = ctx.string_decode(b).unwrap();
        assert_eq!(copied(&ctx), 9);
        let lit = ctx.heap_mut().alloc_string_immortal("ab");
        ctx.heap_mut().dup(lit).unwrap();
        let c = ctx.bytes_of_string(lit).unwrap();
        assert_eq!(copied(&ctx), 2);
        let bad = ctx.heap_mut().alloc_bytes(&[b'a', 0xff]);
        copied(&ctx);
        assert_eq!(ctx.string_decode(bad), Err(Fault::Utf8 { at: 1 }));
        assert_eq!(copied(&ctx), 0);
        for w in [b, c, t] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 1);
}

/// `Bytes.empty` は長さ 0 の物体を作り、`Bytes.find_from` は素朴な探索で最初の位置を返す。検査モードは文字列を断る。
fn empty_bytes_and_a_search<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let e = ctx.bytes_empty().unwrap();
        assert_eq!(ctx.heap().meta(e).unwrap().kind(), Kind::Bytes);
        assert_eq!(body(ctx.heap(), e), b"");
        let hay = ctx.heap_mut().alloc_bytes(b"abcabc");
        let needle = ctx.heap_mut().alloc_bytes(b"ca");
        assert_eq!(ctx.bytes_find_from(needle, 0, hay), Ok(2));
        assert_eq!(ctx.bytes_find_from(needle, 3, hay), Ok(-1));
        assert_eq!(ctx.bytes_find_from(e, 6, hay), Ok(6));
        assert_eq!(
            ctx.bytes_find_from(needle, 7, hay),
            Err(Fault::Index(Box::new(IndexFault::BytesIndex {
                index: 7,
                len: 6
            })))
        );
        if M::CHECKED {
            let s = ctx.heap_mut().alloc_string("abc");
            assert_eq!(
                ctx.bytes_find_from(needle, 0, s),
                Err(Fault::Heap(HeapError::WrongKind))
            );
            assert_eq!(
                ctx.bytes_find_from(s, 0, hay),
                Err(Fault::Heap(HeapError::WrongKind))
            );
            ctx.heap_mut().decref(s).unwrap();
        }
        for w in [e, hay, needle] {
            ctx.heap_mut().decref(w).unwrap();
        }
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
    bytes_are_read_by_position,
    utf8_is_validated,
    bytes_are_compared_in_byte_order,
    bytes_are_made_of_one_byte,
    unique_bytes_are_changed_in_place,
    shared_bytes_are_copied,
    out_of_range_writes_release_their_argument,
    bytes_are_appended,
    parts_of_bytes_are_appended,
    appending_a_part_of_itself_needs_two_references,
    bytes_externs_refuse_a_string,
    empty_bytes_and_a_search,
    bytes_copies_are_counted,
    strings_become_bytes,
    bytes_decode_to_strings,
    invalid_bytes_do_not_decode,
    conversions_refuse_the_wrong_kind,
    conversions_are_counted,
);

use std::cell::Cell;
use std::io::Read;
use std::rc::Rc;

use super::*;
use crate::FileHandle;

fn string(heap: &mut Heap, text: &str) -> ObjRef {
    heap.alloc(Payload::Str(text.to_string()))
}

/// 継続の最下部のフレーム。
fn bottom(heap: &mut Heap) -> ObjRef {
    heap.alloc(Payload::Frame(Frame::Root))
}

fn frame(heap: &mut Heap, saved: Vec<(u32, Value)>, next: ObjRef) -> ObjRef {
    heap.alloc(Payload::Frame(Frame::Return {
        function: 0,
        resume: 0,
        saved,
        next,
    }))
}

fn handler(heap: &mut Heap, clauses: Vec<Value>, link: Attachment) -> ObjRef {
    heap.alloc(Payload::Frame(Frame::Handler {
        effect: 1,
        clauses,
        ret: Value::Unit,
        link,
    }))
}

fn attached(next: ObjRef, outer: ObjRef) -> Attachment {
    Attachment::Attached {
        next,
        state: Value::Unit,
        outer,
    }
}

/// 切り離された handler フレーム。`inner` は自身を指す (区間に連鎖のフレームがない形)。自身を指すために、先に
/// フレームを確保してから中身を入れる。
fn detached_handler(heap: &mut Heap, clauses: Vec<Value>) -> ObjRef {
    let frame = bottom(heap);
    *heap.get_mut(frame).unwrap() = Payload::Frame(Frame::Handler {
        effect: 1,
        clauses,
        ret: Value::Unit,
        link: Attachment::Detached { inner: frame },
    });
    frame
}

/// 切り離された handler フレームの `inner` を、区間の中でいちばん内側の連鎖のフレームにする。
fn set_inner(heap: &mut Heap, detached: ObjRef, inner: ObjRef) {
    let Payload::Frame(Frame::Handler { link, .. }) = heap.get_mut(detached).unwrap() else {
        panic!("not a handler frame");
    };
    *link = Attachment::Detached { inner };
}

#[test]
fn a_handler_frame_releases_its_clauses_and_the_rest_of_the_continuation() {
    let mut heap = Heap::new();
    let clause = heap.alloc(Payload::Closure(Closure {
        function: 0,
        args: vec![],
    }));
    let ret = heap.alloc(Payload::Closure(Closure {
        function: 1,
        args: vec![],
    }));
    let end = bottom(&mut heap);
    let frame = heap.alloc(Payload::Frame(Frame::Handler {
        effect: 1,
        clauses: vec![Value::Obj(clause)],
        ret: Value::Obj(ret),
        link: attached(end, end),
    }));
    heap.decref(frame).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn releasing_a_continuation_stops_at_its_detached_handler() {
    let mut heap = Heap::new();
    // handler の外側は機械の継続が持っている
    let outside = bottom(&mut heap);
    let s = string(&mut heap, "saved");
    let detached = detached_handler(&mut heap, vec![]);
    let top = frame(&mut heap, vec![(0, Value::Obj(s))], detached);
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    assert_eq!(
        heap.live_objects(),
        vec![
            ("Continuation".to_string(), 1),
            ("Frame".to_string(), 3),
            ("String".to_string(), 1),
        ]
    );
    heap.decref(k).unwrap();
    assert_eq!(heap.live_objects(), vec![("Frame".to_string(), 1)]);
    heap.decref(outside).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_closure_releases_its_arguments() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let closure = heap.alloc(Payload::Closure(Closure {
        function: 0,
        args: vec![Value::Obj(s), Value::Int(1)],
    }));
    heap.decref(closure).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn an_apply_frame_releases_its_arguments_and_the_rest_of_the_continuation() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let end = bottom(&mut heap);
    let next = frame(&mut heap, vec![], end);
    let apply = heap.alloc(Payload::Frame(Frame::Apply {
        args: vec![Value::Obj(s)],
        next,
    }));
    heap.decref(apply).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_mask_frame_releases_the_rest_of_the_continuation() {
    let mut heap = Heap::new();
    let end = bottom(&mut heap);
    let next = frame(&mut heap, vec![], end);
    let mask = heap.alloc(Payload::Frame(Frame::Mask {
        effects: vec![0],
        next,
        outer: end,
    }));
    heap.decref(mask).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn decref_to_zero_frees_the_object() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
    assert_eq!(heap.get(s), Err(HeapError::UseAfterFree));
}

#[test]
fn dup_keeps_the_object_alive() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    heap.dup(s).unwrap();
    heap.decref(s).unwrap();
    assert_eq!(heap.get(s).unwrap(), &Payload::Str("a".to_string()));
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_reused_slot_does_not_revive_old_references() {
    let mut heap = Heap::new();
    let old = string(&mut heap, "a");
    heap.decref(old).unwrap();
    let new = string(&mut heap, "b");
    assert_ne!(old, new);
    assert_eq!(heap.get(old), Err(HeapError::UseAfterFree));
    assert_eq!(heap.decref(old), Err(HeapError::UseAfterFree));
    assert_eq!(heap.get(new).unwrap(), &Payload::Str("b".to_string()));
}

#[test]
fn decref_releases_children() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let inner = bottom(&mut heap);
    let outer = frame(
        &mut heap,
        vec![(0, Value::Obj(s)), (1, Value::Int(1))],
        inner,
    );
    heap.decref(outer).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn releasing_a_long_chain_does_not_overflow_the_stack() {
    let mut heap = Heap::new();
    let mut next = bottom(&mut heap);
    for _ in 0..200_000 {
        next = frame(&mut heap, vec![], next);
    }
    heap.decref(next).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn live_objects_are_counted_by_kind() {
    let mut heap = Heap::new();
    string(&mut heap, "a");
    string(&mut heap, "b");
    bottom(&mut heap);
    assert_eq!(
        heap.live_objects(),
        [("Frame".to_string(), 1), ("String".to_string(), 2)]
    );
}

#[test]
fn reference_count_writes_are_counted_but_frees_are_not() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let end = bottom(&mut heap);
    let outer = frame(&mut heap, vec![(0, Value::Obj(s))], end);
    let lit = literal(&mut heap, "lit");
    heap.dup(s).unwrap();
    heap.acquire_immortal(lit).unwrap();
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (2, 0));
    // 連鎖する解放は、フレーム、退避した文字列、下のフレームの数を1つずつ減らす
    heap.decref(outer).unwrap();
    assert_eq!(heap.rc_decrements(), 3);
    heap.decref(lit).unwrap();
    // `take` は数を書かずに箱を空けるので、数えない
    assert_eq!(heap.take(s), Ok(Payload::Str("a".to_string())));
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (2, 4));
    assert!(heap.live_objects().is_empty());
}

#[test]
fn string_bytes_written_counts_the_contents_of_string_objects() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "abc");
    let end = bottom(&mut heap);
    assert_eq!(heap.string_bytes_written(), 3);
    // 一意な文字列は、取り出しても写さない
    let t = string(&mut heap, "de");
    assert_eq!(heap.take_or_copy(t), Ok(Payload::Str("de".to_string())));
    assert_eq!(heap.string_bytes_written(), 5);
    // 共有された文字列は、取り出すときに中身を写す
    heap.dup(s).unwrap();
    assert_eq!(heap.take_or_copy(s), Ok(Payload::Str("abc".to_string())));
    assert_eq!(heap.string_bytes_written(), 8);
    heap.decref(s).unwrap();
    heap.decref(end).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_requires_a_unique_object() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    heap.dup(s).unwrap();
    assert_eq!(heap.take(s), Err(HeapError::Shared));
    heap.decref(s).unwrap();
    assert_eq!(heap.take(s), Ok(Payload::Str("a".to_string())));
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_or_copy_takes_a_unique_object() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    assert_eq!(heap.take_or_copy(s), Ok(Payload::Str("a".to_string())));
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_or_copy_copies_a_shared_object_and_its_children() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let closure = heap.alloc(Payload::Closure(Closure {
        function: 0,
        args: vec![Value::Obj(s), Value::Int(1)],
    }));
    heap.dup(closure).unwrap();
    let copy = heap.take_or_copy(closure).unwrap();
    assert_eq!(
        copy,
        Payload::Closure(Closure {
            function: 0,
            args: vec![Value::Obj(s), Value::Int(1)],
        })
    );
    // 元のクロージャと写した中身が、捕まえた文字列の参照を1つずつ持つ
    heap.decref(closure).unwrap();
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

/// 区間の先頭から、切り離された handler フレームまでのフレーム。
fn segment(heap: &Heap, top: ObjRef) -> Vec<ObjRef> {
    let mut frames = vec![top];
    loop {
        let next = match heap.get(*frames.last().unwrap()).unwrap() {
            Payload::Frame(
                Frame::Return { next, .. } | Frame::Apply { next, .. } | Frame::Mask { next, .. },
            ) => *next,
            Payload::Frame(Frame::Handler {
                link: Attachment::Attached { next, .. },
                ..
            }) => *next,
            Payload::Frame(Frame::Handler {
                link: Attachment::Detached { .. },
                ..
            }) => return frames,
            other => panic!("not a frame: {other:?}"),
        };
        frames.push(next);
    }
}

#[test]
fn take_or_copy_takes_a_unique_continuation_without_copying() {
    let mut heap = Heap::new();
    let detached = detached_handler(&mut heap, vec![]);
    let top = frame(&mut heap, vec![], detached);
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    assert_eq!(
        heap.take_or_copy(k),
        Ok(Payload::Continuation {
            top,
            handler: detached
        })
    );
    heap.decref(top).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_or_copy_copies_the_segment_of_a_shared_continuation() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "saved");
    let clause = heap.alloc(Payload::Closure(Closure {
        function: 0,
        args: vec![],
    }));
    let detached = detached_handler(&mut heap, vec![Value::Obj(clause)]);
    let below_attached = frame(&mut heap, vec![], detached);
    // 本体の中の別の handle は、外側につながったまま区間に入る
    let attached = handler(&mut heap, vec![], attached(below_attached, detached));
    set_inner(&mut heap, detached, attached);
    let top = frame(&mut heap, vec![(0, Value::Obj(s))], attached);
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    } = heap.take_or_copy(k).unwrap()
    else {
        panic!("not a continuation");
    };
    let original = segment(&heap, top);
    let copied = segment(&heap, copied_top);
    assert_eq!(original, [top, attached, below_attached, detached]);
    assert_eq!(copied.len(), 4);
    assert_eq!(copied[3], copied_handler);
    assert!(copied.iter().all(|frame| !original.contains(frame)));
    // どちらの区間のフレームも一意である
    for &frame in original.iter().chain(&copied) {
        assert!(heap.is_unique(frame).unwrap());
    }
    // 退避した文字列と節のクロージャは、両方の区間から参照される
    assert!(!heap.is_unique(s).unwrap());
    assert!(!heap.is_unique(clause).unwrap());
    let copy = heap.alloc(Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    });
    heap.decref(copy).unwrap();
    heap.decref(k).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn copying_a_segment_keeps_the_resume_address_of_a_return_frame() {
    // 再開の番地は実行する側が意味を決める `u64` で、ランタイムは解釈せずにそのまま写す。
    // 上位の32ビットも落とさないことを確かめる
    let mut heap = Heap::new();
    let resume = (7 << 32) | 3;
    let detached = detached_handler(&mut heap, vec![]);
    let top = heap.alloc(Payload::Frame(Frame::Return {
        function: 2,
        resume,
        saved: vec![(5, Value::Int(1))],
        next: detached,
    }));
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    } = heap.take_or_copy(k).unwrap()
    else {
        panic!("not a continuation");
    };
    assert_ne!(copied_top, top);
    assert_eq!(
        heap.get(copied_top),
        Ok(&Payload::Frame(Frame::Return {
            function: 2,
            resume,
            saved: vec![(5, Value::Int(1))],
            next: copied_handler,
        }))
    );
    let copy = heap.alloc(Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    });
    heap.decref(copy).unwrap();
    heap.decref(k).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn copying_a_segment_copies_its_mask_frames() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "saved");
    let detached = detached_handler(&mut heap, vec![]);
    let below_mask = frame(&mut heap, vec![(0, Value::Obj(s))], detached);
    // `mask` 付きの呼び出しの中で操作したので、区間の途中に `Mask` フレームが入る
    let mask = heap.alloc(Payload::Frame(Frame::Mask {
        effects: vec![0],
        next: below_mask,
        outer: detached,
    }));
    set_inner(&mut heap, detached, mask);
    let top = frame(&mut heap, vec![], mask);
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    } = heap.take_or_copy(k).unwrap()
    else {
        panic!("not a continuation");
    };
    let original = segment(&heap, top);
    let copied = segment(&heap, copied_top);
    assert_eq!(original, [top, mask, below_mask, detached]);
    assert_eq!(copied.len(), 4);
    assert_eq!(copied[3], copied_handler);
    assert!(copied.iter().all(|frame| !original.contains(frame)));
    assert_eq!(
        heap.get(copied[1]).unwrap(),
        &Payload::Frame(Frame::Mask {
            effects: vec![0],
            next: copied[2],
            outer: copied[3],
        })
    );
    for &frame in original.iter().chain(&copied) {
        assert!(heap.is_unique(frame).unwrap());
    }
    assert!(!heap.is_unique(s).unwrap());
    let copy = heap.alloc(Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    });
    heap.decref(copy).unwrap();
    heap.decref(k).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn copying_a_long_segment_does_not_overflow_the_stack() {
    let mut heap = Heap::new();
    let detached = detached_handler(&mut heap, vec![]);
    let mut top = detached;
    for _ in 0..200_000 {
        top = frame(&mut heap, vec![], top);
    }
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let copy = heap.take_or_copy(k).unwrap();
    let copy = heap.alloc(copy);
    heap.decref(copy).unwrap();
    heap.decref(k).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_data_object_releases_its_fields() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s), Value::Int(3), Value::Tag(0)],
    });
    assert_eq!(
        heap.live_objects(),
        [("Data".to_string(), 1), ("String".to_string(), 1)]
    );
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_or_copy_takes_a_unique_data_object_with_its_fields() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s)],
    });
    assert_eq!(
        heap.take_or_copy(data),
        Ok(Payload::Data {
            tag: 1,
            fields: vec![Value::Obj(s)],
        })
    );
    // 箱は解放され、フィールドの参照は取り出した側に移る
    assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_or_copy_copies_a_shared_data_object_and_dups_its_fields() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s), Value::Int(2)],
    });
    heap.dup(data).unwrap();
    let copy = heap.take_or_copy(data).unwrap();
    assert_eq!(
        copy,
        Payload::Data {
            tag: 1,
            fields: vec![Value::Obj(s), Value::Int(2)],
        }
    );
    // 元の箱と取り出したフィールドが、文字列の参照を1つずつ持つ
    heap.decref(data).unwrap();
    assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

/// 捨てられたことを旗で知らせる読み出し口。
struct Flagged(Rc<Cell<bool>>);

impl Read for Flagged {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl Drop for Flagged {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

fn file(heap: &mut Heap, dropped: &Rc<Cell<bool>>) -> ObjRef {
    heap.alloc(Payload::File(FileHandle::new(
        "a.txt".to_string(),
        Box::new(Flagged(dropped.clone())),
    )))
}

#[test]
fn releasing_a_file_drops_its_reader() {
    let mut heap = Heap::new();
    let dropped = Rc::new(Cell::new(false));
    let f = file(&mut heap, &dropped);
    assert_eq!(heap.live_objects(), [("File".to_string(), 1)]);
    heap.decref(f).unwrap();
    assert!(dropped.get());
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_file_inside_data_is_released_with_it() {
    let mut heap = Heap::new();
    let dropped = Rc::new(Cell::new(false));
    let f = file(&mut heap, &dropped);
    let pair = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(f), Value::Int(1)],
    });
    heap.decref(pair).unwrap();
    assert!(dropped.get());
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_shared_file_is_not_copied() {
    let mut heap = Heap::new();
    let dropped = Rc::new(Cell::new(false));
    let f = file(&mut heap, &dropped);
    heap.dup(f).unwrap();
    assert!(matches!(heap.take_or_copy(f), Err(HeapError::NotCopyable)));
    assert!(!dropped.get());
    // take_or_copy did not decref when it returned error, so refcount is still 2.
    // Release both references to verify no leak or double-free.
    heap.decref(f).unwrap();
    heap.decref(f).unwrap();
    assert!(dropped.get());
    assert!(heap.live_objects().is_empty());
}

#[test]
fn an_attached_handler_releases_its_state() {
    let mut heap = Heap::new();
    let end = bottom(&mut heap);
    let state = string(&mut heap, "state");
    let frame = handler(
        &mut heap,
        vec![],
        Attachment::Attached {
            next: end,
            state: Value::Obj(state),
            outer: end,
        },
    );
    heap.decref(frame).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn copying_a_segment_shares_the_state_of_an_attached_handler() {
    let mut heap = Heap::new();
    let state = string(&mut heap, "state");
    let detached = detached_handler(&mut heap, vec![]);
    let below_attached = frame(&mut heap, vec![], detached);
    // 本体の中の別の handle は、状態を持ったまま区間に入る
    let inner = handler(
        &mut heap,
        vec![],
        Attachment::Attached {
            next: below_attached,
            state: Value::Obj(state),
            outer: detached,
        },
    );
    set_inner(&mut heap, detached, inner);
    let top = frame(&mut heap, vec![], inner);
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    } = heap.take_or_copy(k).unwrap()
    else {
        panic!("not a continuation");
    };
    let copied = segment(&heap, copied_top);
    assert_eq!(copied.len(), 4);
    // 写した内側の handler フレームは、写した次のフレームにつながり、同じ状態を指す
    let Payload::Frame(Frame::Handler {
        link:
            Attachment::Attached {
                next,
                state: copied_state,
                ..
            },
        ..
    }) = heap.get(copied[1]).unwrap()
    else {
        panic!("the copied inner handler is not attached");
    };
    assert_eq!(*next, copied[2]);
    assert_eq!(*copied_state, Value::Obj(state));
    assert!(!heap.is_unique(state).unwrap());
    let copy = heap.alloc(Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    });
    heap.decref(copy).unwrap();
    heap.decref(k).unwrap();
    assert!(heap.live_objects().is_empty());
}

/// 文字列のリテラルと同じ形の不死の物体。数は 0 で始まる。
fn literal(heap: &mut Heap, text: &str) -> ObjRef {
    heap.alloc_immortal(Payload::Str(text.to_string()))
}

#[test]
fn an_immortal_object_counts_the_references_it_hands_out() {
    let mut heap = Heap::new();
    let s = literal(&mut heap, "lit");
    // リテラルの表から作るのは実行の仕事ではないので、写したバイトに数えない
    assert_eq!(heap.string_bytes_written(), 0);
    assert_eq!(heap.get(s), Err(HeapError::UseAfterFree));
    heap.acquire_immortal(s).unwrap();
    heap.dup(s).unwrap();
    assert_eq!(heap.live_objects(), [("String".to_string(), 2)]);
    heap.decref(s).unwrap();
    assert_eq!(heap.get(s).unwrap(), &Payload::Str("lit".to_string()));
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn an_immortal_object_is_not_freed_at_zero() {
    let mut heap = Heap::new();
    let s = literal(&mut heap, "lit");
    heap.acquire_immortal(s).unwrap();
    heap.decref(s).unwrap();
    // スロットは空かないので、次の確保は別のスロットに入り、同じ参照でまた数を増やせる
    let other = string(&mut heap, "other");
    assert_ne!(other.index, s.index);
    heap.acquire_immortal(s).unwrap();
    assert_eq!(heap.get(s).unwrap(), &Payload::Str("lit".to_string()));
    heap.decref(s).unwrap();
    heap.decref(other).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn every_operation_on_an_immortal_object_at_zero_is_use_after_free() {
    let mut heap = Heap::new();
    let s = literal(&mut heap, "lit");
    heap.acquire_immortal(s).unwrap();
    heap.decref(s).unwrap();
    assert_eq!(heap.get(s), Err(HeapError::UseAfterFree));
    assert_eq!(heap.get_mut(s), Err(HeapError::UseAfterFree));
    assert_eq!(heap.dup(s), Err(HeapError::UseAfterFree));
    assert_eq!(heap.decref(s), Err(HeapError::UseAfterFree));
    assert_eq!(heap.is_unique(s), Err(HeapError::UseAfterFree));
    assert_eq!(heap.take(s), Err(HeapError::UseAfterFree));
    assert_eq!(heap.take_or_copy(s), Err(HeapError::UseAfterFree));
}

#[test]
fn an_immortal_object_is_never_unique() {
    let mut heap = Heap::new();
    let s = literal(&mut heap, "lit");
    heap.acquire_immortal(s).unwrap();
    assert_eq!(heap.is_unique(s), Ok(false));
    assert_eq!(heap.take(s), Err(HeapError::Shared));
    // 断った `take` は参照を手放さない
    assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_or_copy_copies_an_immortal_string() {
    let mut heap = Heap::new();
    let s = literal(&mut heap, "lit");
    heap.acquire_immortal(s).unwrap();
    assert_eq!(heap.take_or_copy(s), Ok(Payload::Str("lit".to_string())));
    assert_eq!(heap.string_bytes_written(), 3);
    // 写した後に元の参照を手放すので、数は 0 に戻る
    assert_eq!(heap.get(s), Err(HeapError::UseAfterFree));
    assert!(heap.live_objects().is_empty());
}

#[test]
fn leaked_references_to_an_immortal_object_are_each_counted() {
    let mut heap = Heap::new();
    let s = literal(&mut heap, "lit");
    // 数が 0 の不死の物体は報告しない
    literal(&mut heap, "unused");
    // 1つのリテラルを2回評価して、どちらも手放し忘れた
    heap.acquire_immortal(s).unwrap();
    heap.acquire_immortal(s).unwrap();
    string(&mut heap, "ordinary");
    assert_eq!(heap.live_objects(), [("String".to_string(), 3)]);
}

#[test]
fn releasing_a_parent_releases_its_immortal_child_without_freeing_it() {
    let mut heap = Heap::new();
    let s = literal(&mut heap, "lit");
    heap.acquire_immortal(s).unwrap();
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(s)],
    });
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
    heap.acquire_immortal(s).unwrap();
    assert_eq!(heap.get(s).unwrap(), &Payload::Str("lit".to_string()));
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn append_str_extends_a_unique_string_in_place() {
    let mut heap = Heap::new();
    let left = string(&mut heap, "ab");
    let right = string(&mut heap, "cde");
    let before = heap.string_bytes_written();
    assert_eq!(heap.append_str(left, right), Ok(3));
    // 足したのは右辺の分だけで、左辺は写さない
    assert_eq!(heap.string_bytes_written() - before, 3);
    assert_eq!(heap.get(left).unwrap(), &Payload::Str("abcde".to_string()));
    assert_eq!(heap.get(right).unwrap(), &Payload::Str("cde".to_string()));
    heap.decref(left).unwrap();
    heap.decref(right).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn append_str_refuses_a_shared_left_side() {
    // `x ++ x` の形。同じ物体を両辺に渡すと RC が 2 なので、その場で足さない
    let mut heap = Heap::new();
    let left = string(&mut heap, "ab");
    heap.dup(left).unwrap();
    assert_eq!(heap.append_str(left, left), Err(HeapError::Shared));
    assert_eq!(heap.get(left).unwrap(), &Payload::Str("ab".to_string()));
    heap.decref(left).unwrap();
    heap.decref(left).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn append_str_refuses_one_unique_object_on_both_sides() {
    // RC が 1 のまま両辺に同じ物体を渡すのは参照の数え誤りなので、黙って空を足さずに断る
    let mut heap = Heap::new();
    let s = string(&mut heap, "ab");
    let before = heap.string_bytes_written();
    assert_eq!(heap.append_str(s, s), Err(HeapError::Shared));
    assert_eq!(heap.get(s).unwrap(), &Payload::Str("ab".to_string()));
    assert_eq!(heap.string_bytes_written(), before);
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn append_str_refuses_an_immortal_left_side() {
    // 不死のリテラルは外に出ている参照が1つでも一意にならない。その場で足すと、次に評価したリテラルの中身が変わる
    let mut heap = Heap::new();
    let lit = literal(&mut heap, "lit");
    heap.acquire_immortal(lit).unwrap();
    let right = string(&mut heap, "x");
    assert_eq!(heap.append_str(lit, right), Err(HeapError::Shared));
    assert_eq!(heap.get(lit).unwrap(), &Payload::Str("lit".to_string()));
    heap.decref(lit).unwrap();
    heap.decref(right).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn append_str_keeps_the_left_side_when_the_right_side_is_not_a_string() {
    let mut heap = Heap::new();
    let left = string(&mut heap, "ab");
    let end = bottom(&mut heap);
    let before = heap.string_bytes_written();
    assert_eq!(heap.append_str(left, end), Err(HeapError::NotAString));
    assert_eq!(heap.append_str(end, left), Err(HeapError::NotAString));
    // 取り出した左辺の中身は、誤りの経路でも戻っている
    assert_eq!(heap.get(left).unwrap(), &Payload::Str("ab".to_string()));
    assert_eq!(heap.string_bytes_written(), before);
}

/// 連鎖のフレームが指す次の連鎖のフレーム。`Mask` とつながった handler フレームは `outer`、切り離された handler
/// フレームは `inner` である。
fn chain_link(heap: &Heap, frame: ObjRef) -> ObjRef {
    match heap.get(frame).unwrap() {
        Payload::Frame(
            Frame::Mask { outer, .. }
            | Frame::Handler {
                link: Attachment::Attached { outer, .. },
                ..
            },
        ) => *outer,
        Payload::Frame(Frame::Handler {
            link: Attachment::Detached { inner },
            ..
        }) => *inner,
        other => panic!("not a frame of the handler chain: {other:?}"),
    }
}

#[test]
fn copying_a_segment_remaps_its_handler_chain() {
    let mut heap = Heap::new();
    let detached = detached_handler(&mut heap, vec![]);
    let below = frame(&mut heap, vec![], detached);
    let inner = handler(&mut heap, vec![], attached(below, detached));
    // `mask` 付きの呼び出しの中の handle の本体で操作したので、区間の連鎖は mask、inner、detached の順につながる
    let mask = heap.alloc(Payload::Frame(Frame::Mask {
        effects: vec![1],
        next: inner,
        outer: inner,
    }));
    let top = frame(&mut heap, vec![], mask);
    set_inner(&mut heap, detached, mask);
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    } = heap.take_or_copy(k).unwrap()
    else {
        panic!("not a continuation");
    };
    assert_eq!(segment(&heap, top), [top, mask, inner, below, detached]);
    let copied = segment(&heap, copied_top);
    assert_eq!(copied.len(), 5);
    assert_eq!(copied[4], copied_handler);
    // 写した区間の連鎖は写した区間の中だけを指し、元の区間の連鎖は変わらない
    assert_eq!(chain_link(&heap, copied_handler), copied[1]);
    assert_eq!(chain_link(&heap, copied[1]), copied[2]);
    assert_eq!(chain_link(&heap, copied[2]), copied_handler);
    assert_eq!(chain_link(&heap, detached), mask);
    assert_eq!(chain_link(&heap, mask), inner);
    assert_eq!(chain_link(&heap, inner), detached);
    let copy = heap.alloc(Payload::Continuation {
        top: copied_top,
        handler: copied_handler,
    });
    heap.decref(copy).unwrap();
    heap.decref(k).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_chain_link_out_of_the_segment_breaks_the_segment() {
    let mut heap = Heap::new();
    // 区間の外の連鎖のフレーム
    let outside = bottom(&mut heap);
    let detached = detached_handler(&mut heap, vec![]);
    let mask = heap.alloc(Payload::Frame(Frame::Mask {
        effects: vec![1],
        next: detached,
        outer: outside,
    }));
    set_inner(&mut heap, detached, mask);
    let k = heap.alloc(Payload::Continuation {
        top: mask,
        handler: detached,
    });
    heap.dup(k).unwrap();
    assert_eq!(heap.take_or_copy(k), Err(HeapError::BrokenSegment));
    // 写す前に誤りになるので、何も確保せず、`k` の参照も手放さない
    assert_eq!(
        heap.live_objects(),
        [("Continuation".to_string(), 1), ("Frame".to_string(), 3)]
    );
    heap.decref(k).unwrap();
    heap.decref(k).unwrap();
    heap.decref(outside).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn an_inner_link_out_of_the_segment_breaks_the_segment() {
    let mut heap = Heap::new();
    let outside = bottom(&mut heap);
    let detached = detached_handler(&mut heap, vec![]);
    let top = frame(&mut heap, vec![], detached);
    // 区間に連鎖のフレームがないので、`inner` は自身を指さなければならない
    set_inner(&mut heap, detached, outside);
    let k = heap.alloc(Payload::Continuation {
        top,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let live = heap.live_objects();
    assert_eq!(heap.take_or_copy(k), Err(HeapError::BrokenSegment));
    assert_eq!(heap.live_objects(), live);
    heap.decref(k).unwrap();
    heap.decref(k).unwrap();
    heap.decref(outside).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn an_inner_link_to_a_chain_frame_that_is_not_the_innermost_breaks_the_segment() {
    let mut heap = Heap::new();
    let detached = detached_handler(&mut heap, vec![]);
    let outer_mask = heap.alloc(Payload::Frame(Frame::Mask {
        effects: vec![1],
        next: detached,
        outer: detached,
    }));
    let inner_mask = heap.alloc(Payload::Frame(Frame::Mask {
        effects: vec![1],
        next: outer_mask,
        outer: outer_mask,
    }));
    // いちばん内側は `inner_mask` なのに、`inner` が外側の mask を指す
    set_inner(&mut heap, detached, outer_mask);
    let k = heap.alloc(Payload::Continuation {
        top: inner_mask,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let live = heap.live_objects();
    assert_eq!(heap.take_or_copy(k), Err(HeapError::BrokenSegment));
    assert_eq!(heap.live_objects(), live);
    heap.decref(k).unwrap();
    heap.decref(k).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_mask_whose_outer_is_not_a_chain_frame_breaks_the_segment() {
    let mut heap = Heap::new();
    let detached = detached_handler(&mut heap, vec![]);
    let plain = frame(&mut heap, vec![], detached);
    // `outer` は連鎖のフレームを指さなければならないが、`Return` フレームを指す
    let mask = heap.alloc(Payload::Frame(Frame::Mask {
        effects: vec![1],
        next: plain,
        outer: plain,
    }));
    set_inner(&mut heap, detached, mask);
    let k = heap.alloc(Payload::Continuation {
        top: mask,
        handler: detached,
    });
    heap.dup(k).unwrap();
    let live = heap.live_objects();
    assert_eq!(heap.take_or_copy(k), Err(HeapError::BrokenSegment));
    assert_eq!(heap.live_objects(), live);
    heap.decref(k).unwrap();
    heap.decref(k).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn acquiring_a_stale_immortal_reference_is_use_after_free() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "gone");
    heap.decref(s).unwrap();
    assert_eq!(heap.acquire_immortal(s), Err(HeapError::UseAfterFree));
}

/// 生きている物体の参照の数。
fn rc(heap: &Heap, obj: ObjRef) -> u32 {
    heap.object(obj).unwrap().header.rc
}

#[test]
fn release_fields_frees_a_unique_box_and_drops_the_fields_it_does_not_keep() {
    let mut heap = Heap::new();
    let (a, b) = (string(&mut heap, "a"), string(&mut heap, "b"));
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(a), Value::Int(3), Value::Obj(b)],
    });
    heap.release_fields(data, 1, &[true, false, false]).unwrap();
    assert_eq!(heap.get(data), Err(HeapError::UseAfterFree));
    assert_eq!(heap.get(b), Err(HeapError::UseAfterFree));
    // 残した `a` は箱の参照を受け継ぐので、数は変わらない
    assert_eq!(rc(&heap, a), 1);
    // 箱の参照と `b` の参照を1回ずつ減らしたと数える。一意な箱は数を書かずに空けるが、参照を1つ手放している
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (0, 2));
    heap.decref(a).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_on_a_shared_box_dups_the_kept_fields_and_gives_up_the_box() {
    let mut heap = Heap::new();
    let (a, b) = (string(&mut heap, "a"), string(&mut heap, "b"));
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(a), Value::Obj(b)],
    });
    heap.dup(data).unwrap();
    heap.release_fields(data, 0, &[false, true]).unwrap();
    // 箱はもう1つの参照で生きていて、フィールドの参照を持ったままである
    assert_eq!((rc(&heap, data), rc(&heap, a), rc(&heap, b)), (1, 1, 2));
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (2, 1));
    heap.decref(b).unwrap();
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_treats_one_value_in_two_fields_as_two_references() {
    let mut heap = Heap::new();
    // 一意な箱では、残す位置の参照を受け継ぎ、残さない位置の参照を手放す
    let a = string(&mut heap, "a");
    heap.dup(a).unwrap();
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(a), Value::Obj(a)],
    });
    heap.release_fields(data, 0, &[true, false]).unwrap();
    assert_eq!(rc(&heap, a), 1);
    heap.decref(a).unwrap();
    assert!(heap.live_objects().is_empty());
    // 共有された箱では、残す位置ごとに1回 `dup` する
    let a = string(&mut heap, "a");
    heap.dup(a).unwrap();
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(a), Value::Obj(a)],
    });
    heap.dup(data).unwrap();
    heap.release_fields(data, 0, &[true, true]).unwrap();
    assert_eq!((rc(&heap, data), rc(&heap, a)), (1, 4));
    heap.decref(a).unwrap();
    heap.decref(a).unwrap();
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_closes_a_file_it_does_not_keep_and_hands_over_one_it_keeps() {
    let mut heap = Heap::new();
    let dropped = Rc::new(Cell::new(false));
    let f = file(&mut heap, &dropped);
    let s = string(&mut heap, "text");
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(f), Value::Obj(s)],
    });
    heap.release_fields(data, 0, &[false, true]).unwrap();
    assert!(dropped.get());
    heap.decref(s).unwrap();
    let kept = Rc::new(Cell::new(false));
    let f = file(&mut heap, &kept);
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(f), Value::Int(1)],
    });
    heap.release_fields(data, 0, &[true, false]).unwrap();
    assert!(!kept.get());
    assert_eq!(rc(&heap, f), 1);
    heap.decref(f).unwrap();
    assert!(kept.get());
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_on_an_immortal_value_takes_the_shared_path() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc_immortal(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(s)],
    });
    heap.acquire_immortal(data).unwrap();
    heap.release_fields(data, 0, &[true]).unwrap();
    // 不死の箱は一意にならないので、残すフィールドを `dup` する。箱は数が 0 になっても解放しない
    assert_eq!(rc(&heap, s), 2);
    heap.decref(s).unwrap();
    heap.acquire_immortal(data).unwrap();
    assert_eq!(
        heap.get(data).unwrap(),
        &Payload::Data {
            tag: 0,
            fields: vec![Value::Obj(s)],
        }
    );
    heap.decref(data).unwrap();
    // 文字列に残った参照は、不死の箱が持っている
    assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
}

#[test]
fn release_fields_refuses_a_value_of_another_layout_and_changes_nothing() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "s");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s)],
    });
    assert_eq!(
        heap.release_fields(s, 0, &[true]),
        Err(HeapError::WrongLayout)
    );
    assert_eq!(
        heap.release_fields(data, 0, &[true]),
        Err(HeapError::WrongLayout)
    );
    assert_eq!(
        heap.release_fields(data, 1, &[true, false]),
        Err(HeapError::WrongLayout)
    );
    assert_eq!((rc(&heap, data), rc(&heap, s)), (1, 1));
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (0, 0));
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn release_fields_on_a_freed_value_is_use_after_free() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "s");
    let data = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(s)],
    });
    heap.release_fields(data, 0, &[true]).unwrap();
    assert_eq!(
        heap.release_fields(data, 0, &[true]),
        Err(HeapError::UseAfterFree)
    );
    heap.decref(s).unwrap();
    // 数が 0 の不死の物体も同じである
    let lit = literal(&mut heap, "lit");
    assert_eq!(
        heap.release_fields(lit, 0, &[]),
        Err(HeapError::UseAfterFree)
    );
    assert!(heap.live_objects().is_empty());
}

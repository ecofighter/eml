use super::*;

fn string(heap: &mut Heap, text: &str) -> ObjRef {
    heap.alloc(Payload::Str(text.to_string()))
}

/// 継続の最下部のフレーム。
fn bottom(heap: &mut Heap) -> ObjRef {
    heap.alloc(Payload::Frame(Frame::Io))
}

fn frame(heap: &mut Heap, saved: Vec<(u32, Value)>, next: ObjRef) -> ObjRef {
    heap.alloc(Payload::Frame(Frame::Return {
        function: 0,
        resume: 0,
        bind: 0,
        saved,
        next,
    }))
}

fn handler(
    heap: &mut Heap,
    clauses: Vec<Value>,
    ret: Option<Value>,
    next: Option<ObjRef>,
) -> ObjRef {
    heap.alloc(Payload::Frame(Frame::Handler {
        effect: 1,
        clauses,
        ret,
        next,
    }))
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
    let frame = handler(
        &mut heap,
        vec![Value::Obj(clause)],
        Some(Value::Obj(ret)),
        Some(end),
    );
    heap.decref(frame).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn releasing_a_continuation_stops_at_its_detached_handler() {
    let mut heap = Heap::new();
    // handler の外側は機械の継続が持っている
    let outside = bottom(&mut heap);
    let s = string(&mut heap, "saved");
    let detached = handler(&mut heap, vec![], None, None);
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
fn live_objects_are_counted_by_descriptor() {
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
            Payload::Frame(Frame::Return { next, .. } | Frame::Apply { next, .. }) => *next,
            Payload::Frame(Frame::Handler {
                next: Some(next), ..
            }) => *next,
            Payload::Frame(Frame::Handler { next: None, .. }) => return frames,
            other => panic!("not a frame: {other:?}"),
        };
        frames.push(next);
    }
}

#[test]
fn take_or_copy_takes_a_unique_continuation_without_copying() {
    let mut heap = Heap::new();
    let detached = handler(&mut heap, vec![], None, None);
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
    let detached = handler(&mut heap, vec![Value::Obj(clause)], None, None);
    let below_attached = frame(&mut heap, vec![], detached);
    // 本体の中の別の handle は、外側につながったまま区間に入る
    let attached = handler(&mut heap, vec![], None, Some(below_attached));
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
fn copying_a_long_segment_does_not_overflow_the_stack() {
    let mut heap = Heap::new();
    let detached = handler(&mut heap, vec![], None, None);
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
fn mark_shared_is_reserved_for_multicore() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    assert_eq!(
        heap.mark_shared(s),
        Err(HeapError::NotImplemented("mark_shared"))
    );
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

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::FileHandle;

/// 捨てられたことを旗で知らせる読み出し口。
struct Flagged(Arc<AtomicBool>);

impl Read for Flagged {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl Drop for Flagged {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn file(heap: &mut Heap, dropped: &Arc<AtomicBool>) -> ObjRef {
    heap.alloc(Payload::File(FileHandle::new(
        "a.txt".to_string(),
        Box::new(Flagged(dropped.clone())),
    )))
}

#[test]
fn releasing_a_file_drops_its_reader() {
    let mut heap = Heap::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let f = file(&mut heap, &dropped);
    assert_eq!(heap.live_objects(), [("File".to_string(), 1)]);
    heap.decref(f).unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_file_inside_data_is_released_with_it() {
    let mut heap = Heap::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let f = file(&mut heap, &dropped);
    let pair = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(f), Value::Int(1)],
    });
    heap.decref(pair).unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_shared_file_is_not_copied() {
    let mut heap = Heap::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let f = file(&mut heap, &dropped);
    heap.dup(f).unwrap();
    assert!(matches!(heap.take_or_copy(f), Err(HeapError::NotCopyable)));
    assert!(!dropped.load(Ordering::SeqCst));
}

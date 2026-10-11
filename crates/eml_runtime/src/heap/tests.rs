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
    // 連鎖する解放は、フレーム、退避した文字列、下のフレームの数を1つずつ減らす。文字列は上で `dup` したので、数が
    // 1 に戻るだけで解放されない
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
    let t = string(&mut heap, "de");
    assert_eq!(heap.string_bytes_written(), 5);
    // 連結は、右辺の中身を左辺の後ろに書く
    assert_eq!(heap.append_str(s, t), Ok(2));
    assert_eq!(heap.string_bytes_written(), 7);
    heap.decref(s).unwrap();
    heap.decref(t).unwrap();
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
    let closure = heap.alloc(Payload::Closure(Closure {
        function: 0,
        args: vec![Value::Int(1)],
    }));
    assert_eq!(
        heap.take_or_copy(closure),
        Ok(Payload::Closure(Closure {
            function: 0,
            args: vec![Value::Int(1)],
        }))
    );
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

#[test]
fn take_or_copy_refuses_data_strings_and_files() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s)],
    });
    let dropped = Rc::new(Cell::new(false));
    let f = file(&mut heap, &dropped);
    // 物体の種類だけで断り、一意かどうかは見ない
    for obj in [s, data, f] {
        assert_eq!(heap.take_or_copy(obj), Err(HeapError::NotCopyable));
    }
    heap.dup(data).unwrap();
    assert_eq!(heap.take_or_copy(data), Err(HeapError::NotCopyable));
    // 断った `take_or_copy` は参照を手放さない
    assert_eq!((rc(&heap, data), rc(&heap, s), rc(&heap, f)), (2, 1, 1));
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (1, 0));
    assert!(!dropped.get());
    heap.decref(data).unwrap();
    heap.decref(data).unwrap();
    heap.decref(f).unwrap();
    assert!(dropped.get());
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
    // `File` は参照の数によらず写さない。断った `take_or_copy` は参照を手放さないので、数は 2 のままである。
    // 2つの参照を手放して、漏れも二重の解放もないことを確かめる
    heap.decref(f).unwrap();
    heap.decref(f).unwrap();
    assert!(dropped.get());
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
    // 共有された箱でも、残すフィールドの `dup` と箱の `decref` より先に形を確かめる
    heap.dup(data).unwrap();
    assert_eq!(
        heap.release_fields(data, 0, &[true]),
        Err(HeapError::WrongLayout)
    );
    assert_eq!(
        heap.release_fields(data, 1, &[true, false]),
        Err(HeapError::WrongLayout)
    );
    assert_eq!((rc(&heap, data), rc(&heap, s)), (2, 1));
    assert_eq!((heap.rc_increments(), heap.rc_decrements()), (1, 0));
    heap.decref(data).unwrap();
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

#[test]
fn an_evidence_releases_its_table_state_and_outer_list() {
    let mut heap = Heap::new();
    let clause = string(&mut heap, "clause");
    let table = heap.alloc(Payload::EvTable {
        effect: 0,
        entries: vec![Value::Obj(clause)],
    });
    let state = string(&mut heap, "state");
    // 再開で作り直した evidence は、元の evidence と入口の表を共有する
    heap.dup(table).unwrap();
    let outer_evidence = heap.alloc(Payload::Evidence {
        table,
        state: Value::Unit,
        outer: Value::Unit,
    });
    let outer = heap.alloc(Payload::Evv {
        evidence: outer_evidence,
        next: Value::Unit,
    });
    let evidence = heap.alloc(Payload::Evidence {
        table,
        state: Value::Obj(state),
        outer: Value::Obj(outer),
    });
    heap.decref(evidence).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn evidence_objects_are_counted_by_their_kind() {
    let mut heap = Heap::new();
    let table = heap.alloc(Payload::EvTable {
        effect: 0,
        entries: vec![Value::Unit],
    });
    let evidence = heap.alloc(Payload::Evidence {
        table,
        state: Value::Unit,
        outer: Value::Unit,
    });
    let list = heap.alloc(Payload::Evv {
        evidence,
        next: Value::Unit,
    });
    assert_eq!(
        heap.live_objects(),
        vec![
            ("EvTable".to_string(), 1),
            ("Evidence".to_string(), 1),
            ("Evv".to_string(), 1)
        ]
    );
    heap.decref(list).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn an_evidence_is_not_copied_like_a_closure() {
    let mut heap = Heap::new();
    let table = heap.alloc(Payload::EvTable {
        effect: 0,
        entries: Vec::new(),
    });
    let evidence = heap.alloc(Payload::Evidence {
        table,
        state: Value::Unit,
        outer: Value::Unit,
    });
    assert_eq!(heap.take_or_copy(evidence), Err(HeapError::NotCopyable));
    heap.decref(evidence).unwrap();
    assert!(heap.live_objects().is_empty());
}

use std::collections::BTreeMap;

use super::{list, probe};
use crate::{Checked, Heap, HeapError, Kind, Meta, Word};

type H = Heap<Checked>;

#[test]
fn using_a_freed_object_is_an_error() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.decref(x).unwrap();
        assert_eq!(heap.dup(x), Err(HeapError::UseAfterFree));
        assert_eq!(heap.field(x, 0), Err(HeapError::UseAfterFree));
        assert_eq!(heap.decref(x), Err(HeapError::UseAfterFree));
        assert_eq!(heap.is_unique(x), Err(HeapError::UseAfterFree));
    }
}

#[test]
fn a_freed_word_is_reported_by_every_operation() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.decref(x).unwrap();
        assert_eq!(heap.field(x, 0), Err(HeapError::UseAfterFree));
        assert_eq!(heap.dup(x), Err(HeapError::UseAfterFree));
        assert_eq!(heap.decref(x), Err(HeapError::UseAfterFree));
        assert_eq!(heap.share(x, 1), Err(HeapError::UseAfterFree));
    }
    drop(heap);
}

#[test]
fn a_child_reached_twice_is_freed_once() {
    // 呼ぶ側が dup せずに同じ語を2つのフィールドに書いた親。2回目に届いた子は UseAfterFree になる
    let (value, drops) = probe();
    let mut heap = H::new();
    let f = heap.alloc_foreign(value, false);
    let p = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(p, 0, f).unwrap();
        heap.set_field(p, 1, f).unwrap();
        assert_eq!(heap.decref(p), Err(HeapError::UseAfterFree));
    }
    drop(heap);
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn the_checked_heap_never_reuses_a_freed_block() {
    let mut heap = H::new();
    let a = heap.alloc_data(0, 0, 1, false);
    unsafe { heap.decref(a) }.unwrap();
    assert_ne!(heap.alloc_data(0, 0, 1, false), a);
}

#[test]
fn dupping_a_lin_object_is_an_error() {
    let mut heap = H::new();
    let f = heap.alloc_data(0, 0, 1, true);
    unsafe {
        assert_eq!(heap.dup(f), Err(HeapError::LinDup));
        heap.decref(f).unwrap();
    }
}

#[test]
fn another_worker_cannot_touch_a_local_object() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 0, false);
    heap.set_worker(1);
    let wrong = Err(HeapError::WrongOwner {
        owner: 0,
        worker: 1,
    });
    unsafe {
        assert_eq!(heap.dup(x), wrong);
        assert_eq!(heap.is_unique(x).map(|_| ()), wrong);
        assert_eq!(heap.set_field(x, 0, Word::UNIT), wrong);
        heap.set_worker(0);
        heap.decref(x).unwrap();
    }
}

#[test]
fn an_immortal_object_counts_its_outside_references() {
    let mut heap = H::new();
    let x = unsafe { heap.alloc_immortal(1, Meta::new(0, 1, Kind::Data, false)) };
    unsafe {
        // 作るときのフィールドの書き込みは数を見ない
        heap.set_field(x, 0, Word::UNIT).unwrap();
        assert_eq!(heap.decref(x), Err(HeapError::ImmortalOverRelease));
        heap.acquire_immortal(x).unwrap();
        assert_eq!(heap.leaks(), BTreeMap::from([("Data", 1)]));
        heap.decref(x).unwrap();
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn a_borrowed_child_of_an_immortal_object_can_be_dupped() {
    let mut heap = H::new();
    let child = unsafe { heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false)) };
    let parent = unsafe { heap.alloc_immortal(1, Meta::new(0, 1, Kind::Data, false)) };
    unsafe {
        heap.set_field(parent, 0, child).unwrap();
        let borrowed = heap.field(parent, 0).unwrap();
        heap.dup(borrowed).unwrap();
        heap.decref(borrowed).unwrap();
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn acquire_on_a_mortal_object_is_an_error() {
    let mut heap = H::new();
    let x = list(&mut heap, 1);
    unsafe {
        assert_eq!(heap.acquire_immortal(x), Err(HeapError::NotImmortal));
        heap.decref(x).unwrap();
    }
}

#[test]
fn leaks_are_counted_by_kind() {
    let mut heap = H::new();
    let a = list(&mut heap, 1);
    let b = heap.alloc_int(i64::MAX);
    let (value, _) = probe();
    let f = heap.alloc_foreign(value, false);
    assert_eq!(
        heap.leaks(),
        BTreeMap::from([("BigInt", 1), ("Data", 1), ("Foreign", 1)])
    );
    unsafe {
        for w in [a, b, f] {
            heap.decref(w).unwrap();
        }
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn the_dummy_is_never_a_leak() {
    let mut heap = H::new();
    unsafe { heap.dup(heap.dummy()) }.unwrap();
    assert!(heap.leaks().is_empty());
}

#[test]
fn stats_count_rc_operations_and_live_boxes() {
    let mut heap = H::new();
    let before = heap.stats();
    let x = list(&mut heap, 1);
    unsafe {
        heap.dup(x).unwrap();
        heap.decref(x).unwrap();
        heap.decref(x).unwrap();
    }
    let after = heap.stats();
    assert_eq!(after.rc_increments - before.rc_increments, 1);
    assert_eq!(after.rc_decrements - before.rc_decrements, 2);
    assert_eq!(after.allocations - before.allocations, 1);
    assert_eq!(after.live, before.live);
    assert_eq!(after.peak_live, before.live + 1);
}

#[test]
fn a_field_index_past_the_block_is_an_error() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 0, false);
    unsafe {
        assert_eq!(heap.field(x, 1), Err(HeapError::OutOfBounds));
        heap.decref(x).unwrap();
    }
}

#[test]
fn raw_and_scanned_words_are_not_mixed_up() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 1, false);
    let f = heap.alloc_foreign(5u64, false);
    unsafe {
        assert_eq!(heap.set_raw(x, 0, 7), Err(HeapError::OutOfBounds));
        assert_eq!(heap.raw(x, 0), Err(HeapError::OutOfBounds));
        assert_eq!(
            heap.set_field(x, 1, Word::UNIT),
            Err(HeapError::OutOfBounds)
        );
        assert_eq!(heap.set_raw(f, 0, 7), Err(HeapError::WrongKind));
        assert_eq!(
            heap.set_field(f, 0, Word::UNIT),
            Err(HeapError::OutOfBounds)
        );
        heap.decref(x).unwrap();
        heap.decref(f).unwrap();
    }
}

#[test]
fn free_block_refuses_a_finalizer_and_a_shared_count() {
    let mut heap = H::new();
    let (value, _) = probe();
    let f = heap.alloc_foreign(value, false);
    let x = heap.alloc_data(0, 0, 1, false);
    unsafe {
        assert_eq!(heap.free_block(f), Err(HeapError::FinalizerSkipped));
        heap.dup(x).unwrap();
        assert_eq!(heap.free_block(x), Err(HeapError::NotLast));
        heap.decref(x).unwrap();
        heap.decref(x).unwrap();
        heap.decref(f).unwrap();
    }
}

/// 走査する語を1つと生の語を1つ持つ不死の物体を書き終えて返す。
fn immortal_cell(heap: &mut H) -> Word {
    let c = unsafe { heap.alloc_immortal(2, Meta::new(0, 1, Kind::Data, false)) };
    unsafe {
        heap.set_field(c, 0, Word::imm(1)).unwrap();
        heap.set_raw(c, 1, 2).unwrap();
    }
    c
}

#[test]
fn writing_a_region_constant_is_an_error() {
    let mut b = H::new();
    let c = immortal_cell(&mut b);
    let region = std::sync::Arc::new(b.freeze().unwrap());
    let mut heap = H::new();
    heap.attach(region);
    unsafe {
        assert_eq!(
            heap.set_field(c, 0, Word::imm(3)),
            Err(HeapError::NotWritable)
        );
        assert_eq!(heap.set_raw(c, 1, 4), Err(HeapError::NotWritable));
        assert_eq!(heap.field(c, 0), Ok(Word::imm(1)));
        assert_eq!(heap.raw(c, 1), Ok(2));
    }
}

#[test]
fn writing_an_own_immortal_after_it_is_built_is_an_error() {
    let mut heap = H::new();
    let c = immortal_cell(&mut heap);
    // 次の物体を作ると、c は構築中でなくなる。作っている物体には書ける
    let d = immortal_cell(&mut heap);
    unsafe {
        assert_eq!(
            heap.set_field(c, 0, Word::imm(3)),
            Err(HeapError::NotWritable)
        );
        assert_eq!(heap.set_raw(c, 1, 4), Err(HeapError::NotWritable));
        heap.set_field(d, 0, Word::imm(5)).unwrap();
        assert_eq!(heap.field(c, 0), Ok(Word::imm(1)));
        assert_eq!(heap.raw(c, 1), Ok(2));
    }
}

#[test]
fn writing_a_shared_object_is_an_error() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 1, false);
    unsafe {
        heap.set_raw(x, 1, 2).unwrap();
        heap.share(x, 1).unwrap();
        assert_eq!(
            heap.set_field(x, 0, Word::imm(3)),
            Err(HeapError::NotWritable)
        );
        assert_eq!(heap.set_raw(x, 1, 4), Err(HeapError::NotWritable));
        heap.decref(x).unwrap();
    }
}

#[test]
fn freezing_a_constant_that_holds_a_freed_object_is_an_error() {
    let mut b = H::new();
    let s = b.alloc_string("mortal");
    let cell = unsafe { b.alloc_immortal(1, Meta::new(0, 1, Kind::Data, false)) };
    unsafe {
        b.set_field(cell, 0, s).unwrap();
        // cell に移した参照を、もう一度手放す。s は空けて隔離したブロックになる
        b.decref(s).unwrap();
    }
    assert_eq!(b.freeze().unwrap_err(), HeapError::UseAfterFree);
}

/// 数の語を持たない tag 0 の `Data` は配列でない。配列は長さによらず数の語を持つ
/// (docs/spec/object-model.md の「配列」)。
#[test]
fn array_operations_refuse_data_without_the_count_word() {
    let mut ctx = super::ctx::ctx::<Checked>();
    let wrong_kind = Err(crate::externs::Fault::Heap(HeapError::WrongKind));
    unsafe {
        for scanned in [0, 2] {
            let w = ctx.heap_mut().alloc_data(0, scanned, 0, false);
            assert_eq!(ctx.heap().meta(w).unwrap().scan(), scanned as u8);
            assert_eq!(ctx.array_length(w), wrong_kind);
            assert_eq!(ctx.array_get(w, 0).map(|_| 0), wrong_kind);
            ctx.heap_mut().decref(w).unwrap();
        }
    }
}

/// `Raw` の語 0 は要素の数なので、語として読み書きする操作と、ほかの種類を求める操作は `Raw` を断る。`unboxed_*` は
/// `Raw` でない物体を断る (docs/spec/object-model.md の「種類」)。
#[test]
fn raw_objects_and_other_kinds_are_not_mixed_up() {
    let mut heap = H::new();
    let u = heap.alloc_unboxed(2, 0, false);
    let x = heap.alloc_data(0, 1, 1, false);
    let b = heap.alloc_bytes(&[1, 2]);
    unsafe {
        assert_eq!(heap.raw(u, 1), Err(HeapError::WrongKind));
        assert_eq!(heap.set_raw(u, 1, 7), Err(HeapError::WrongKind));
        assert_eq!(heap.field(u, 0), Err(HeapError::OutOfBounds));
        assert_eq!(
            heap.set_field(u, 0, Word::UNIT),
            Err(HeapError::OutOfBounds)
        );
        assert_eq!(heap.unbox_float(u), Err(HeapError::WrongKind));
        assert_eq!(heap.unbox_int(u), Err(HeapError::WrongKind));
        assert_eq!(heap.string(u).map(|_| ()), Err(HeapError::WrongKind));
        assert_eq!(heap.bytes(u).map(|_| ()), Err(HeapError::WrongKind));
        assert_eq!(
            heap.foreign::<u64>(u).map(|_| ()),
            Err(HeapError::WrongKind)
        );
        assert_eq!(heap.release(u, 0, &[]), Err(HeapError::WrongLayout));
        assert_eq!(heap.into_token(u).map(|_| ()), Err(HeapError::NotTokenable));
        for w in [x, b] {
            assert_eq!(heap.unboxed_len(w), Err(HeapError::WrongKind));
            assert_eq!(heap.unboxed_word(w, 0), Err(HeapError::WrongKind));
            assert_eq!(heap.unboxed_put(w, 0, 7), Err(HeapError::WrongKind));
            assert_eq!(heap.unboxed_copy(u, w, 0), Err(HeapError::WrongKind));
            assert_eq!(heap.unboxed_copy(w, u, 0), Err(HeapError::WrongKind));
        }
        assert_eq!(heap.unboxed_word(u, 1), Ok(0));
        for w in [u, x, b] {
            heap.decref(w).unwrap();
        }
    }
}

/// `set_lin` が受けるのは一意な `Data` と `Raw` だけである。
#[test]
fn set_lin_refuses_a_shared_raw_object_and_other_kinds() {
    let mut heap = H::new();
    let u = heap.alloc_unboxed(1, 0, false);
    let b = heap.alloc_bytes(&[1]);
    unsafe {
        heap.dup(u).unwrap();
        assert_eq!(heap.set_lin(u, true), Err(HeapError::NotUnique));
        heap.decref(u).unwrap();
        heap.set_lin(u, true).unwrap();
        assert_eq!(heap.set_lin(b, true), Err(HeapError::WrongKind));
        heap.decref(u).unwrap();
        heap.decref(b).unwrap();
    }
}

#[test]
fn raw_leaks_are_counted_by_kind() {
    let mut heap = H::new();
    let u = heap.alloc_unboxed(3, 0, false);
    assert_eq!(heap.leaks(), BTreeMap::from([("Raw", 1)]));
    unsafe { heap.decref(u) }.unwrap();
    assert!(heap.leaks().is_empty());
}

/// `Raw` の物体は配列でないので、配列の操作は `WrongKind` で断る (docs/spec/object-model.md の「種類」)。
#[test]
fn array_operations_refuse_a_raw_object() {
    let mut ctx = super::ctx::ctx::<Checked>();
    let wrong_kind = Err(crate::externs::Fault::Heap(HeapError::WrongKind));
    unsafe {
        let u = ctx.heap_mut().alloc_unboxed(2, 0, false);
        assert_eq!(ctx.array_length(u), wrong_kind);
        assert_eq!(ctx.array_get(u, 0).map(|_| 0), wrong_kind);
        for consume in [
            |ctx: &mut crate::Ctx<Checked>, w| ctx.array_set(w, 0, Word::imm(1)),
            |ctx: &mut crate::Ctx<Checked>, w| ctx.array_freeze(w),
            |ctx: &mut crate::Ctx<Checked>, w| ctx.array_thaw(w),
        ] {
            assert_eq!(consume(&mut ctx, u).map(|_| 0), wrong_kind);
            assert_eq!(ctx.heap().unboxed_word(u, 1), Ok(0));
        }
        ctx.heap_mut().decref(u).unwrap();
    }
}

/// 共有した `Raw` の要素は、ほかのスレッドが読めるので書けない。断った後も要素は変わらない。
#[test]
fn writing_a_shared_raw_object_is_an_error() {
    let mut heap = H::new();
    let u = heap.alloc_unboxed(2, 5, false);
    let src = heap.alloc_unboxed(2, 9, false);
    unsafe {
        heap.share(u, 1).unwrap();
        assert_eq!(heap.unboxed_put(u, 0, 7), Err(HeapError::NotWritable));
        assert_eq!(heap.unboxed_copy(u, src, 2), Err(HeapError::NotWritable));
        for i in 0..2 {
            assert_eq!(heap.unboxed_word(u, i), Ok(5));
        }
        heap.decref(u).unwrap();
        heap.decref(src).unwrap();
    }
}

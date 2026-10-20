//! 配列の物体 (docs/spec/object-model.md の「配列」)。`Array` と `MutArray` は同じ形の `Data` の物体で、`MutArray`
//! は `lin` ビットを持つ。

use super::ctx::ctx;
use super::mortal;
use crate::externs::{Fault, IndexFault};
use crate::{Ctx, HeapError, Mode, Word};

fn int(n: i64) -> Word {
    Word::imm(n)
}

fn lin_of<M: Mode>(ctx: &Ctx<M>, w: Word) -> bool {
    unsafe { ctx.heap().meta(w) }.unwrap().lin()
}

fn release<M: Mode>(ctx: &mut Ctx<M>, w: Word) {
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
}

fn get<M: Mode>(ctx: &mut Ctx<M>, a: Word, i: i64) -> Word {
    unsafe { ctx.array_get(a, i) }.unwrap()
}

fn array_fault(index: i64, len: i64) -> Fault {
    Fault::Index(Box::new(IndexFault::Array { index, len }))
}

fn make_get_set_round_trip<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.array_make(3, int(7), false).unwrap();
        let b = ctx.array_set(a, 1, int(9)).unwrap();
        // 一意な配列はその場で書き換える
        assert_eq!(b, a);
        assert_eq!(get(&mut ctx, b, 0), int(7));
        assert_eq!(get(&mut ctx, b, 1), int(9));
        assert_eq!(get(&mut ctx, b, 2), int(7));
        assert_eq!(ctx.array_length(b), Ok(3));
        release(&mut ctx, b);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn elements_are_counted_references<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let s = ctx.heap_mut().alloc_string("x");
        let a = ctx.array_make(3, s, false).unwrap();
        let e = get(&mut ctx, a, 2);
        assert_eq!(e, s);
        release(&mut ctx, e);
        let t = ctx.heap_mut().alloc_string("y");
        // 書き換えた要素の参照は手放す
        let a = ctx.array_set(a, 0, t).unwrap();
        let u = ctx.heap_mut().alloc_string("z");
        let a = ctx.array_set(a, 0, u).unwrap();
        release(&mut ctx, a);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn set_copies_a_shared_array<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let s = ctx.heap_mut().alloc_string("kept");
        let a = ctx.array_make(2, s, false).unwrap();
        ctx.heap_mut().dup(a).unwrap();
        let b = ctx.array_set(a, 1, int(5)).unwrap();
        assert_ne!(b, a);
        assert_eq!(get(&mut ctx, b, 1), int(5));
        let e = get(&mut ctx, a, 1);
        assert_eq!(e, s);
        release(&mut ctx, e);
        let e = get(&mut ctx, b, 0);
        assert_eq!(e, s);
        release(&mut ctx, e);
        assert_eq!(ctx.array_length(b), Ok(2));
        release(&mut ctx, a);
        release(&mut ctx, b);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn extended_arrays_index_past_the_count_word<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let mut a = ctx.array_make(300, int(1), false).unwrap();
        for i in [0, 254, 255, 299] {
            a = ctx.array_set(a, i, int(1000 + i)).unwrap();
        }
        assert_eq!(ctx.array_length(a), Ok(300));
        for i in [0, 254, 255, 299] {
            assert_eq!(get(&mut ctx, a, i), int(1000 + i));
        }
        assert_eq!(get(&mut ctx, a, 1), int(1));
        assert_eq!(get(&mut ctx, a, 298), int(1));
        // 共有の配列の写しも、数の語の後ろに要素を並べる
        ctx.heap_mut().dup(a).unwrap();
        let b = ctx.array_set(a, 299, int(-1)).unwrap();
        assert_ne!(b, a);
        assert_eq!(ctx.array_length(b), Ok(300));
        assert_eq!(get(&mut ctx, b, 255), int(1255));
        assert_eq!(get(&mut ctx, b, 299), int(-1));
        assert_eq!(get(&mut ctx, a, 299), int(1299));
        release(&mut ctx, a);
        release(&mut ctx, b);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn out_of_range_and_negative_length_fault<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.array_make(3, int(0), false).unwrap();
        assert_eq!(ctx.array_get(a, 3), Err(array_fault(3, 3)));
        assert_eq!(ctx.array_get(a, -1), Err(array_fault(-1, 3)));
        // 借りた配列は呼ぶ側が持ち続ける
        assert_eq!(ctx.array_length(a), Ok(3));
        let s = ctx.heap_mut().alloc_string("x");
        assert_eq!(
            ctx.array_make(-2, s, false),
            Err(Fault::Index(Box::new(IndexFault::Length { len: -2 })))
        );
        // 範囲の外の `set` は配列と値を手放す
        let t = ctx.heap_mut().alloc_string("y");
        assert_eq!(ctx.array_set(a, 3, t), Err(array_fault(3, 3)));
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn empty_and_zero_length_arrays<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let m = ctx.array_empty(true).unwrap();
        assert!(lin_of(&ctx, m));
        assert_eq!(ctx.array_length(m), Ok(0));
        assert_eq!(ctx.array_get(m, 0), Err(array_fault(0, 0)));
        release(&mut ctx, m);
        // 長さ 0 の `make` は値を手放す
        let s = ctx.heap_mut().alloc_string("x");
        let a = ctx.array_make(0, s, false).unwrap();
        assert!(!lin_of(&ctx, a));
        assert_eq!(ctx.array_length(a), Ok(0));
        release(&mut ctx, a);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn freeze_and_thaw_flip_the_lin_bit<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let m = ctx.array_make(2, int(4), true).unwrap();
        assert!(lin_of(&ctx, m));
        let a = ctx.array_freeze(m).unwrap();
        assert_eq!(a, m);
        assert!(!lin_of(&ctx, a));
        let m = ctx.array_thaw(a).unwrap();
        assert_eq!(m, a);
        assert!(lin_of(&ctx, m));
        let a = ctx.array_freeze(m).unwrap();
        // 共有の配列は写して `lin` ビットを立てる
        ctx.heap_mut().dup(a).unwrap();
        let m = ctx.array_thaw(a).unwrap();
        assert_ne!(m, a);
        assert!(lin_of(&ctx, m));
        assert!(!lin_of(&ctx, a));
        assert_eq!(get(&mut ctx, m, 1), int(4));
        let m = ctx.array_set(m, 0, int(8)).unwrap();
        assert!(lin_of(&ctx, m));
        assert_eq!(get(&mut ctx, a, 0), int(4));
        release(&mut ctx, a);
        release(&mut ctx, m);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// `MutArray` はいつも一意なので、共有の物体の `freeze` は検査モードでは誤りである。速いモードは写す。
fn freezing_a_shared_array<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.array_make(2, int(3), false).unwrap();
        ctx.heap_mut().dup(a).unwrap();
        if M::CHECKED {
            assert_eq!(ctx.array_freeze(a), Err(Fault::Heap(HeapError::NotUnique)));
            release(&mut ctx, a);
        } else {
            let b = ctx.array_freeze(a).unwrap();
            assert_ne!(b, a);
            assert!(!lin_of(&ctx, b));
            assert_eq!(get(&mut ctx, b, 1), int(3));
            release(&mut ctx, b);
        }
        release(&mut ctx, a);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn checked_mode_rejects_a_lin_dup_of_a_mut_array<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        let m = ctx.array_make(2, int(0), true).unwrap();
        assert_eq!(ctx.heap_mut().dup(m), Err(HeapError::LinDup));
        release(&mut ctx, m);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

both_modes!(
    make_get_set_round_trip,
    elements_are_counted_references,
    set_copies_a_shared_array,
    extended_arrays_index_past_the_count_word,
    out_of_range_and_negative_length_fault,
    empty_and_zero_length_arrays,
    freeze_and_thaw_flip_the_lin_bit,
    freezing_a_shared_array,
    checked_mode_rejects_a_lin_dup_of_a_mut_array,
);

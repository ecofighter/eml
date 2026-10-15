//! セッションの根と、実行時エラーの後に evv と yield の状態を空に戻す `abandon`
//! (docs/spec/object-model.md の「実行文脈 `Ctx`」)。

use super::ctx::{UNIT, ctx, ctx_evidence, push};
use crate::{HeapError, Mode, Word};

/// 根に置いた値は、読むたびに `dup` され、`take_roots` で呼ぶ側に戻る。
fn a_root_keeps_its_value_until_taken<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let s = ctx.heap_mut().alloc_string("kept");
        ctx.root_set(0, s).unwrap();
        for _ in 0..2 {
            let got = ctx.root_get(0).unwrap();
            assert_eq!(got, s);
            ctx.heap_mut().decref(got).unwrap();
        }
        assert_eq!(super::mortal(ctx.heap()), 1);
        let roots = ctx.take_roots();
        assert_eq!(roots, vec![Some(s)]);
        ctx.heap_mut().decref(s).unwrap();
    }
    assert_eq!(super::mortal(ctx.heap()), 0);
    assert!(ctx.take_roots().is_empty());
}

/// 表は番号まで伸び、間の根は空のまま残る。即値も根に置ける。
fn the_table_grows_to_the_index<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        ctx.root_set(3, Word::imm(7)).unwrap();
        assert_eq!(ctx.root_get(3), Ok(Word::imm(7)));
        assert_eq!(ctx.root_get(1), Err(HeapError::RootMisuse));
        ctx.root_set(1, UNIT).unwrap();
        assert_eq!(ctx.root_get(1), Ok(UNIT));
    }
    assert_eq!(
        ctx.take_roots(),
        vec![None, Some(UNIT), None, Some(Word::imm(7))]
    );
}

/// 空の根と表の外の根は読めない。どちらのモードでも誤りを返す。
fn an_empty_root_is_misuse<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        assert_eq!(ctx.root_get(0), Err(HeapError::RootMisuse));
        ctx.root_set(0, UNIT).unwrap();
        assert_eq!(ctx.root_get(u32::MAX), Err(HeapError::RootMisuse));
    }
}

/// 根には1回だけ置ける。断った値は呼ぶ側に残り、根の値は変わらない。
fn a_root_is_set_once<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = ctx.heap_mut().alloc_string("a");
        let b = ctx.heap_mut().alloc_string("b");
        ctx.root_set(0, a).unwrap();
        assert_eq!(ctx.root_set(0, b), Err(HeapError::RootMisuse));
        let got = ctx.root_get(0).unwrap();
        assert_eq!(got, a);
        for w in [got, b] {
            ctx.heap_mut().decref(w).unwrap();
        }
        for w in ctx.take_roots().into_iter().flatten() {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// `abandon` は evv と yield の状態を空にするが、それらの語は手放さないので、物体はヒープに残る。根は残し、その後の
/// 基本操作もそのまま使える。
fn abandon_forgets_the_evv_and_the_yield_state<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        ctx.root_set(0, Word::imm(1)).unwrap();
        let ev = push(&mut ctx, 0, 1);
        let target = ctx_evidence(&mut ctx);
        ctx.yield_to(target, UNIT, Word::imm(1)).unwrap();
        ctx.heap_mut().decref(ev).unwrap();
    }
    assert!(!ctx.check_finished());
    let before = super::mortal(ctx.heap());
    ctx.abandon();
    assert!(ctx.check_finished());
    assert_eq!(ctx.yielding(), 0);
    assert_eq!(super::mortal(ctx.heap()), before);
    assert!(before > 0);
    unsafe {
        assert_eq!(ctx.root_get(0), Ok(Word::imm(1)));
        push(&mut ctx, 0, 1);
        ctx.evv_set(UNIT).unwrap();
    }
    assert!(ctx.check_finished());
}

both_modes!(
    a_root_keeps_its_value_until_taken,
    the_table_grows_to_the_index,
    an_empty_root_is_misuse,
    a_root_is_set_once,
    abandon_forgets_the_evv_and_the_yield_state,
);

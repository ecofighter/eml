//! 実行文脈と基本操作 (docs/spec/lir.md の「基本操作」)。

use std::sync::Arc;

use crate::{Control, Ctx, HeapError, Limits, Mode, Stop, Word};

pub(super) const UNIT: Word = Word::imm(0);

pub(super) fn ctx<M: Mode>() -> Ctx<M> {
    Ctx::new(Arc::new(Control::new(Limits::default())))
}

/// エフェクト 0、操作1つの evidence を作る。外側の evv は空である。
pub(super) fn ctx_evidence<M: Mode>(ctx: &mut Ctx<M>) -> Word {
    unsafe { ctx.ev_new(0, 1, UNIT, UNIT) }.unwrap()
}

/// 今の evv の先頭に、エフェクト `effect` の新しい evidence を足し、その evidence を返す (呼ぶ側が1つ参照を持つ)。
pub(super) fn push<M: Mode>(ctx: &mut Ctx<M>, effect: u16, n_ops: usize) -> Word {
    unsafe {
        let outer = ctx.evv_get().unwrap();
        let ev = ctx.ev_new(effect, n_ops, UNIT, outer).unwrap();
        ctx.heap_mut().dup(ev).unwrap();
        let list = ctx.evv_get().unwrap();
        let list = ctx.evv_push(list, ev).unwrap();
        ctx.evv_set(list).unwrap();
        ev
    }
}

/// 今の evv を空にする。
pub(super) fn clear<M: Mode>(ctx: &mut Ctx<M>) {
    unsafe { ctx.evv_set(UNIT) }.unwrap();
}

fn a_new_ctx_is_finished<M: Mode>() {
    let ctx = ctx::<M>();
    assert!(ctx.check_finished());
    assert_eq!(ctx.yielding(), 0);
    assert_eq!(ctx.poll(0), Ok(()));
}

fn poll_reports_each_stop<M: Mode>() {
    let control = Arc::new(Control::new(Limits {
        max_depth: 100,
        ..Limits::default()
    }));
    let ctx = Ctx::<M>::new(control.clone());
    assert_eq!(ctx.poll(100), Ok(()));
    assert_eq!(ctx.poll(101), Err(Stop::DepthLimit));
    control.interrupt();
    assert_eq!(ctx.poll(0), Err(Stop::Interrupted));
    control.clear();
    assert_eq!(ctx.poll(0), Ok(()));
}

/// yield して、継続を2回組み立て直し、終える。`eml_interp` の
/// `yields_and_continuation_frames_are_counted` と同じ列で、同じ数になる。最初の κ は関数の値 (`Word::imm(1)`) で、
/// 関数 0 の値は `()` と同じ語になるので使わない。
fn yields_and_continuation_frames_are_counted<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    unsafe {
        ctx.yield_to(ev, UNIT, Word::imm(1)).unwrap();
        assert_eq!(ctx.yielding(), 1);
        assert_eq!(ctx.yield_status(ev), 1);
        for _ in 0..2 {
            let k = ctx.yield_cont_take().unwrap();
            ctx.yield_cont_set(k).unwrap();
        }
        let k = ctx.yield_cont_take().unwrap();
        ctx.heap_mut().decref(k).unwrap();
        let clause = ctx.yield_finish().unwrap();
        assert_eq!(clause, UNIT);
    }
    assert!(ctx.check_finished());
    if M::CHECKED {
        let stats = ctx.stats();
        assert_eq!((stats.yields, stats.yield_frames), (1, 2));
    }
}

fn yield_status_tells_targets_apart<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    let other = ctx_evidence(&mut ctx);
    assert_eq!(ctx.yield_status(ev), 0);
    unsafe { ctx.yield_abort(ev, UNIT, UNIT) }.unwrap();
    assert_eq!(ctx.yield_status(ev), 2);
    assert_eq!(ctx.yield_status(other), 3);
    unsafe {
        ctx.yield_finish().unwrap();
        ctx.heap_mut().decref(other).unwrap();
    }
    assert!(ctx.check_finished());
}

fn misused_yields_are_reported_in_checked_mode<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        assert_eq!(ctx.yield_finish().unwrap_err(), HeapError::YieldMisuse);
        assert_eq!(ctx.yield_cont_take().unwrap_err(), HeapError::YieldMisuse);
        let ev = ctx_evidence(&mut ctx);
        ctx.yield_to(ev, UNIT, Word::imm(1)).unwrap();
        // κ を持ったまま、もう1つの κ は入れられない
        assert_eq!(
            ctx.yield_cont_set(Word::imm(2)).unwrap_err(),
            HeapError::YieldMisuse
        );
        // yield 中にもう1つの yield は始められない
        let ev2 = ctx_evidence(&mut ctx);
        assert_eq!(
            ctx.yield_to(ev2, UNIT, UNIT).unwrap_err(),
            HeapError::YieldMisuse
        );
        // κ を持ったまま終えられない
        assert_eq!(ctx.yield_finish().unwrap_err(), HeapError::YieldMisuse);
    }
}

fn evv_get_and_set_swap_the_list<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let empty = ctx.evv_get().unwrap();
        assert_eq!(empty, UNIT);
        ctx.evv_set(UNIT).unwrap();
    }
    assert!(ctx.check_finished());
}

fn dropping_a_ctx_mid_yield<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = ctx_evidence(&mut ctx);
    unsafe {
        let k = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.yield_to(ev, UNIT, k).unwrap();
    }
    assert!(!ctx.check_finished());
    drop(ctx);
}

fn lookup_finds_the_innermost_evidence<M: Mode>() {
    let mut ctx = ctx::<M>();
    let e0 = push(&mut ctx, 0, 1);
    let e1 = push(&mut ctx, 1, 1);
    let e0_inner = push(&mut ctx, 0, 1);
    unsafe {
        let found = ctx.ev_lookup(0).unwrap();
        assert_eq!(found, e0_inner);
        ctx.heap_mut().decref(found).unwrap();
        let found = ctx.ev_lookup(1).unwrap();
        assert_eq!(found, e1);
        ctx.heap_mut().decref(found).unwrap();
        for e in [e0, e1, e0_inner] {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
}

fn clauses_states_and_outer_lists<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 2);
    unsafe {
        let clause = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 1, clause).unwrap();
        let got = ctx.ev_clause(ev, 1).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        assert_eq!(ctx.ev_clause(ev, 0).unwrap(), UNIT);
        // 状態の出し入れ
        let s = ctx.heap_mut().alloc_string("state");
        ctx.ev_set_state(ev, s).unwrap();
        let taken = ctx.ev_take_state(ev).unwrap();
        assert_eq!(taken, s);
        assert_eq!(ctx.ev_take_state(ev).unwrap(), UNIT);
        ctx.heap_mut().decref(taken).unwrap();
        // 外側の evv は、作ったときの evv (空) である
        assert_eq!(ctx.ev_evv(ev).unwrap(), UNIT);
        // renew は同じ入口の表を指し、外側を今の evv にする
        let renewed = ctx.ev_renew(ev, UNIT).unwrap();
        let got = ctx.ev_clause(renewed, 1).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        let outer = ctx.ev_evv(renewed).unwrap();
        let now = ctx.evv_get().unwrap();
        assert_eq!(outer, now);
        for w in [outer, now, renewed, ev] {
            ctx.heap_mut().decref(w).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 操作が 255 個以上のエフェクトの入口の表は、先頭に数の語を持つ (docs/spec/object-model.md の「物体の形」)。
fn entry_tables_with_a_count_word<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 300);
    unsafe {
        let clause = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 299, clause).unwrap();
        assert_eq!(ctx.ev_clause(ev, 0).unwrap(), UNIT);
        let got = ctx.ev_clause(ev, 299).unwrap();
        assert_eq!(got, clause);
        ctx.heap_mut().decref(got).unwrap();
        ctx.heap_mut().decref(ev).unwrap();
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

/// 状態の欄を通る輪 (docs/spec/lir.md の「evidence」): 状態が ev を指す物体を持っていても、ev を呼ぶ側が
/// 持っている限り、元の状態を手放しても ev は生き残る。
fn replacing_a_state_that_reaches_its_evidence<M: Mode>() {
    let mut ctx = ctx::<M>();
    let ev = push(&mut ctx, 0, 1);
    unsafe {
        let holder = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.heap_mut().dup(ev).unwrap();
        ctx.heap_mut().set_field(holder, 0, ev).unwrap();
        ctx.ev_set_state(ev, holder).unwrap();
        ctx.ev_set_state(ev, UNIT).unwrap();
        // ev はまだ読める
        assert_eq!(ctx.ev_take_state(ev).unwrap(), UNIT);
        ctx.heap_mut().decref(ev).unwrap();
    }
    clear(&mut ctx);
    assert_eq!(super::mortal(ctx.heap()), 0);
}

fn bad_evidence_is_reported_in_checked_mode<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        // evidence でない物体 (tag 0 のクロージャ)
        let closure = ctx.heap_mut().alloc_data(0, 3, 0, false);
        assert_eq!(
            ctx.ev_clause(closure, 0).unwrap_err(),
            HeapError::BadEvidence
        );
        assert_eq!(
            ctx.ev_take_state(closure).unwrap_err(),
            HeapError::BadEvidence
        );
        // 範囲の外の操作
        let ev = push(&mut ctx, 0, 1);
        assert_eq!(ctx.ev_clause(ev, 1).unwrap_err(), HeapError::BadEvidence);
        // 入口の欄に2回入れる
        let c = ctx.heap_mut().alloc_data(0, 1, 0, false);
        ctx.ev_set_clause(ev, 0, c).unwrap();
        let c2 = ctx.heap_mut().alloc_data(0, 1, 0, false);
        assert_eq!(
            ctx.ev_set_clause(ev, 0, c2).unwrap_err(),
            HeapError::BadEvidence
        );
        // エフェクトの番号の上限 (tag は番号 + 1 なので 65535 は作れない)
        assert_eq!(
            ctx.ev_new(u16::MAX, 1, UNIT, UNIT).unwrap_err(),
            HeapError::BadEvidence
        );
        // 合う evidence がない
        assert_eq!(ctx.ev_lookup(7).unwrap_err(), HeapError::NoHandler);
    }
}

/// `evv` の長さの上限が 0 なら、空のリストにも節点を足せない (docs/spec/lir.md の「実行の文脈」)。
fn a_push_past_the_evv_limit_is_bad_evidence<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let control = Arc::new(Control::new(Limits {
        max_evv: 0,
        ..Limits::default()
    }));
    let mut ctx = Ctx::<M>::new(control);
    unsafe {
        let ev = ctx.ev_new(0, 1, UNIT, UNIT).unwrap();
        assert_eq!(ctx.evv_push(UNIT, ev).unwrap_err(), HeapError::BadEvidence);
        // 確かめは確保の前なので、ev は呼ぶ側が持ったままである
        ctx.heap_mut().decref(ev).unwrap();
    }
    assert!(ctx.check_finished());
    assert_eq!(super::mortal(ctx.heap()), 0);
}

both_modes!(
    a_new_ctx_is_finished,
    poll_reports_each_stop,
    yields_and_continuation_frames_are_counted,
    yield_status_tells_targets_apart,
    misused_yields_are_reported_in_checked_mode,
    evv_get_and_set_swap_the_list,
    dropping_a_ctx_mid_yield,
    lookup_finds_the_innermost_evidence,
    clauses_states_and_outer_lists,
    entry_tables_with_a_count_word,
    replacing_a_state_that_reaches_its_evidence,
    bad_evidence_is_reported_in_checked_mode,
    a_push_past_the_evv_limit_is_bad_evidence,
);

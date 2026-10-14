//! 実行文脈と基本操作 (docs/spec/lir.md の「基本操作」)。

use std::sync::Arc;

use crate::{Control, Ctx, HeapError, Limits, Mode, Stop, Word};

pub(super) const UNIT: Word = Word::imm(0);

pub(super) fn ctx<M: Mode>() -> Ctx<M> {
    Ctx::new(Arc::new(Control::new(Limits::default())))
}

/// エフェクト 0、操作1つの evidence を、基本操作を使わずにヒープに作る。
pub(super) fn ctx_evidence<M: Mode>(ctx: &mut Ctx<M>) -> Word {
    let heap = unsafe { ctx.heap_mut() };
    let table = heap.alloc_data(0, 1, 0, false);
    let ev = heap.alloc_data(1, 3, 0, false);
    unsafe { heap.set_field(ev, 0, table) }.unwrap();
    ev
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

both_modes!(
    a_new_ctx_is_finished,
    poll_reports_each_stop,
    yields_and_continuation_frames_are_counted,
    yield_status_tells_targets_apart,
    misused_yields_are_reported_in_checked_mode,
    evv_get_and_set_swap_the_list,
    dropping_a_ctx_mid_yield,
);

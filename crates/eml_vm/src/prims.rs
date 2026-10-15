//! 基本操作 (docs/spec/lir.md の「基本操作」)。どれも `Ctx` のメソッドを呼ぶ。借用の引数は `Ctx` が手放さないので、
//! VM は語を渡すだけでよい。結果が `unit` の操作は、結果のスロットに `()` を書く。

use eml_rt::{Ctx, Mode, Word};

use crate::code::CodeBase;
use crate::error::VmFault;
use crate::op::{Op, Slot};
use crate::vm::Task;

/// 基本操作の命令を実行する。基本操作でない命令なら `false` を返す。
///
/// 結果のスロットは引数のスロットと同じ色でありうるので (docs/spec/vm.md の「スロットの割り当て」)、どの腕も引数を
/// 読み終えてから結果を書く。
pub(crate) fn step<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    op: Op,
) -> Result<bool, VmFault> {
    let ops = |at: u32| &code.operands[at as usize..];
    // 引数の語は、LIR の所有の規則により VM が持つ参照か借りた語で、基本操作の契約 (docs/spec/object-model.md の
    // 「基本操作の契約」) のとおりに渡す。VM は基本操作の合間に `Ctx` の持つ語に触れない
    unsafe {
        match op {
            Op::EvvGet { dst } => {
                let w = ctx.evv_get()?;
                task.set(dst, w);
            }
            Op::EvvSet { dst, l } => {
                ctx.evv_set(task.get(l))?;
                task.set(dst, Word::UNIT);
            }
            Op::EvvPush { dst, l, e } => {
                let w = ctx.evv_push(task.get(l), task.get(e))?;
                task.set(dst, w);
            }
            Op::EvvMask { dst, at } => {
                let o = ops(at);
                let list = &code.effect_lists[o[0] as usize..(o[0] + o[1]) as usize];
                let w = ctx.evv_mask(list, task.get(o[2] as Slot))?;
                task.set(dst, w);
            }
            Op::EvNew { dst, at } => {
                let o = ops(at);
                let w = ctx.ev_new(
                    o[0] as u16,
                    o[1] as usize,
                    task.get(o[2] as Slot),
                    task.get(o[3] as Slot),
                )?;
                task.set(dst, w);
            }
            Op::EvSetClause { dst, at } => {
                let o = ops(at);
                ctx.ev_set_clause(
                    task.get(o[0] as Slot),
                    o[1] as usize,
                    task.get(o[2] as Slot),
                )?;
                task.set(dst, Word::UNIT);
            }
            Op::EvRenew { dst, ev, state } => {
                let w = ctx.ev_renew(task.get(ev), task.get(state))?;
                task.set(dst, w);
            }
            Op::EvLookup { dst, effect } => {
                let w = ctx.ev_lookup(effect)?;
                task.set(dst, w);
            }
            Op::EvClause { dst, ev, op } => {
                let w = ctx.ev_clause(task.get(ev), op.into())?;
                task.set(dst, w);
            }
            Op::EvEvv { dst, ev } => {
                let w = ctx.ev_evv(task.get(ev))?;
                task.set(dst, w);
            }
            Op::EvTakeState { dst, ev } => {
                let w = ctx.ev_take_state(task.get(ev))?;
                task.set(dst, w);
            }
            Op::EvSetState { dst, ev, s } => {
                ctx.ev_set_state(task.get(ev), task.get(s))?;
                task.set(dst, Word::UNIT);
            }
            Op::Yielding { dst } => {
                let r = ctx.yielding();
                task.set_raw(dst, r);
            }
            Op::YieldStatus { dst, ev } => {
                let r = ctx.yield_status(task.get(ev));
                task.set_raw(dst, r);
            }
            Op::YieldTo { dst, at } | Op::YieldAbort { dst, at } => {
                let o = ops(at);
                let (ev, clause, k) = (
                    task.get(o[0] as Slot),
                    task.get(o[1] as Slot),
                    task.get(o[2] as Slot),
                );
                if matches!(op, Op::YieldTo { .. }) {
                    ctx.yield_to(ev, clause, k)?;
                } else {
                    ctx.yield_abort(ev, clause, k)?;
                }
                task.set(dst, Word::UNIT);
            }
            Op::YieldContTake { dst } => {
                let w = ctx.yield_cont_take()?;
                task.set(dst, w);
            }
            Op::YieldContSet { dst, k } => {
                ctx.yield_cont_set(task.get(k))?;
                task.set(dst, Word::UNIT);
            }
            Op::YieldFinish { dst } => {
                let w = ctx.yield_finish()?;
                task.set(dst, w);
            }
            Op::Dummy { dst } => {
                let w = ctx.heap().dummy();
                task.set(dst, w);
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

//! 実行の駆動がモードごとに変えること。runの終わりの検査と`RunStats`の数である (docs/spec/native.md の「実行の始めと終わり」)。
//! `Fast`と`Checked`を名指すのは、このcrateでこのmoduleだけである。
pub use eml_rt::{Checked, Fast};
use eml_rt::{Ctx, Mode};
use eml_sched::config::RunStats;
use eml_sched::error::{RunFault, RuntimeError};

use crate::census::census;
use crate::image::Census;
use crate::worker::{
    COUNT_BOXES, COUNT_REUSES, COUNT_UNBOXES, COUNT_UNWOUND_FRAMES, NATIVE_CODE, Worker,
};

/// ネイティブのコードが走れるモード。
pub trait Engine: Mode + Sized {
    /// 成功したrunの後の検査。
    fn end(ctx: &mut Ctx<Self>, census: Census) -> Result<(), RuntimeError>;
    /// runの数。速いモードは何も数えない。
    fn stats(ctx: &Ctx<Self>, worker: &Worker) -> RunStats;
}

impl Engine for Fast {
    fn end(ctx: &mut Ctx<Fast>, option: Census) -> Result<(), RuntimeError> {
        if option == Census::Check {
            let left = census(ctx);
            if !left.is_empty() {
                return Err(RuntimeError::Leak(left));
            }
        }
        Ok(())
    }

    fn stats(_ctx: &Ctx<Fast>, _worker: &Worker) -> RunStats {
        RunStats::default()
    }
}

impl Engine for Checked {
    // インラインにして、検査モードを使うcrateにだけこのコードが現れるようにする
    #[inline]
    fn end(ctx: &mut Ctx<Checked>, _census: Census) -> Result<(), RuntimeError> {
        let fault = |error: eml_rt::HeapError| RuntimeError::Fault {
            fault: RunFault::from(error),
            function: NATIVE_CODE.to_string(),
            at: None,
        };
        ctx.check_invariants().map_err(fault)?;
        ctx.heap()
            .control()
            .check_owned(&[ctx.heap()])
            .map_err(fault)?;
        let leaks = ctx.leaks();
        if leaks.is_empty() {
            Ok(())
        } else {
            Err(RuntimeError::Leak(
                leaks
                    .into_iter()
                    .map(|(name, n)| (name.to_string(), n))
                    .collect(),
            ))
        }
    }

    #[inline]
    fn stats(ctx: &Ctx<Checked>, worker: &Worker) -> RunStats {
        let c = ctx.stats();
        let h = ctx.heap().stats();
        RunStats {
            evidence_steps: c.evidence_steps,
            yields: c.yields,
            yield_frames: c.yield_frames,
            unwound_frames: worker.counts[COUNT_UNWOUND_FRAMES as usize],
            string_bytes_copied: c.string_bytes_copied,
            rc_increments: h.rc_increments,
            rc_decrements: h.rc_decrements,
            boxes: worker.counts[COUNT_BOXES as usize],
            unboxes: worker.counts[COUNT_UNBOXES as usize],
            allocations: h.allocations,
            reuses: worker.counts[COUNT_REUSES as usize],
            peak_objects: h.peak_live,
            shared_objects: h.shared,
            ..RunStats::default()
        }
    }
}

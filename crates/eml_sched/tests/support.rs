//! スケジューラのテストが分け合う道具。どのテストもrunを`sequence::run`で動かす (docs/spec/runtime.mdの「runの手
//! 順」)。

// SAFETY: このmoduleの`unsafe`の呼び出しは、`sequence::run`と、タスクの行の代わりに項が呼ぶ`cells`の関数と
// `WorkerCore::here`である。渡す`Workers`は`Scripts`か`Scripts`に任せるテストの型である。`Start.owners`は、そのrunの
// ために作った表か、貸す`Ctx`のヒープの表である。項は、自分が借りた核と`Ctx`の上で、`Here`を`cells`の関数にだけ渡し、
// `WorkerCore`を通して置く語は即値か、項がその`Ctx`に確保して`spawn_with`に渡す物体である。

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use eml_rt::{Buffering, Checked, Control, Ctx, HostRef, Limits, Mode, Owners, StdHost, Word};
use eml_sched::cells::{self, CellRef, Outcome as Finish};
use eml_sched::config::RunConfig;
use eml_sched::core::WorkerCore;
use eml_sched::exec::{Halt, Workers};
use eml_sched::job::Item;
use eml_sched::policy::{InOrder, Policy};
use eml_sched::script::Scripts;
use eml_sched::sequence::{self, Outcome, Start};

/// 出力を捨てるhost。
pub fn host() -> HostRef {
    HostRef::new(Arc::new(StdHost::new(
        Box::new(io::sink()),
        Box::new(io::sink()),
        PathBuf::new(),
        Buffering::Block,
    )))
}

/// workerが`workers`個のrunの`Control`。
pub fn control(workers: usize) -> Arc<Control> {
    Arc::new(Control::new(Limits {
        workers,
        ..Limits::default()
    }))
}

/// 合わせた席のrunを、新しい所有者の表で動かす。
pub fn run<W: Workers>(workers: &W, control: &Arc<Control>) -> Outcome<W::Retired> {
    run_with(workers, control, &Owners::new())
}

/// 合わせた席のrunを、所有者の表`owners`で動かす。
pub fn run_with<W: Workers>(
    workers: &W,
    control: &Arc<Control>,
    owners: &Owners,
) -> Outcome<W::Retired> {
    let config = RunConfig::default().with_control(control.clone());
    // SAFETY: このファイルの頭の約束による。
    unsafe {
        sequence::run(
            workers,
            Start {
                owners,
                host: &host(),
                config: &config,
            },
        )
    }
}

/// worker 0が閉包`enter`で項を動かす、workerが1つの`Scripts`。
pub fn one<'a>(
    enter: impl Fn(&mut WorkerCore<InOrder>, &mut Ctx<Checked>, Item) -> Result<(), Halt>
    + Send
    + Sync
    + 'a,
) -> Scripts<'a, Checked, InOrder> {
    Scripts::new(|_| InOrder, vec![Box::new(enter)])
}

/// 根のスコープを開き、runの根にする。`main`の項が呼ぶ。
pub fn open_root<P: Policy>(core: &mut WorkerCore<P>) -> CellRef {
    let scope = core.shared().lock().open_root().unwrap();
    core.set_root(scope);
    scope
}

/// `scope`にこのworkerが持つタスクを置き、その始まりを準備のできた列に積む。閉包は即値である。
pub fn spawn<M: Mode, P: Policy>(
    core: &mut WorkerCore<P>,
    ctx: &mut Ctx<M>,
    scope: CellRef,
) -> CellRef {
    spawn_with(core, ctx, scope, Word::UNIT)
}

/// `spawn`と同じだが、タスクの閉包は`closure`である。`closure`の参照は始まりの項に移る。
pub fn spawn_with<M: Mode, P: Policy>(
    core: &mut WorkerCore<P>,
    ctx: &mut Ctx<M>,
    scope: CellRef,
    closure: Word,
) -> CellRef {
    ctx.reclaim_if_due();
    let spawner = ctx.heap().worker();
    let (cell, start) = {
        // SAFETY: このファイルの頭の約束による。
        let here = unsafe { core.here() };
        // SAFETY: このファイルの頭の約束による。
        unsafe {
            core.shared().lock().start(
                ctx,
                &here,
                scope,
                spawner,
                core.index(),
                closure,
                &[],
                false,
            )
        }
        .unwrap()
        .expect("the scope is open")
    };
    core.schedule(start.expect("the task is this worker's"));
    cell
}

/// 始める項のタスクを動かし始めた印を付ける。
pub fn started<P: Policy>(core: &mut WorkerCore<P>, cell: CellRef) {
    core.shared().lock().started(cell).unwrap();
}

/// タスク`cell`を`()`で終え、それが起こすタスクを準備のできた列に積む。
pub fn finish<M: Mode, P: Policy>(core: &mut WorkerCore<P>, ctx: &mut Ctx<M>, cell: CellRef) {
    let mut out = Vec::new();
    {
        // SAFETY: このファイルの頭の約束による。
        let here = unsafe { core.here() };
        // SAFETY: このファイルの頭の約束による。
        unsafe {
            cells::finish(
                core.shared(),
                &here,
                ctx,
                cell,
                Finish::Value(Word::UNIT),
                0,
                &mut out,
            )
        }
        .unwrap();
    }
    for run in out {
        core.schedule(run);
    }
}

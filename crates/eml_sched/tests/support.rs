//! スケジューラのテストが分け合う道具。どのテストもrunを`sequence::run`で動かす (docs/spec/runtime.mdの「runの手
//! 順」)。

// SAFETY: このmoduleの`unsafe`の呼び出しは`sequence::run`だけである。渡す`Workers`は`Scripts`か`Scripts`に任せるテス
// トの型で、どの閉包も`WorkerCore`を通して語を置かない。`Start.owners`はそのrunのためだけに作った表である。

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use eml_rt::{Buffering, Checked, Control, Ctx, HostRef, Limits, Owners, StdHost};
use eml_sched::config::RunConfig;
use eml_sched::core::WorkerCore;
use eml_sched::exec::{Halt, Workers};
use eml_sched::job::Item;
use eml_sched::policy::InOrder;
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
    let owners = Owners::new();
    let config = RunConfig::default().with_control(control.clone());
    // SAFETY: このファイルの頭の約束による。
    unsafe {
        sequence::run(
            workers,
            Start {
                owners: &owners,
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

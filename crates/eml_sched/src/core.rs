//! workerが自分で持つスケジューラの部分 (docs/spec/runtime.mdの「もの」)。持つのはworkerのスレッドだけである。`Env`が
//! factoryに渡し、`Exec::with`が項の間に貸す。

use std::marker::PhantomData;
use std::sync::Arc;

use eml_rt::Control;

use crate::policy::{Point, Policy};
use crate::rank;
use crate::run::Run;

/// worker `k`の核。`Send`ではなく、workerのスレッドにとどまり、そこで落とす。
pub struct WorkerCore<P> {
    index: u32,
    policy: P,
    control: Arc<Control>,
    run: Arc<Run>,
    on_its_thread: PhantomData<*const ()>,
}

impl<P: Policy> WorkerCore<P> {
    pub(crate) fn new(
        index: u32,
        policy: P,
        control: Arc<Control>,
        run: Arc<Run>,
    ) -> WorkerCore<P> {
        WorkerCore {
            index,
            policy,
            control,
            run,
            on_its_thread: PhantomData,
        }
    }

    /// workerの番号`k`。その欄と止める語を指す。論理的なworker IDではない。
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn control(&self) -> &Arc<Control> {
        &self.control
    }

    pub(crate) fn run(&self) -> &Arc<Run> {
        &self.run
    }

    /// `point`を方針に知らせる。lockは持たない (docs/spec/runtime.mdの「フック」)。
    pub(crate) fn at(&self, point: Point) {
        rank::unlocked();
        self.policy.at(self.index, point);
    }
}

//! workerが自分で持つスケジューラの部分 (docs/spec/runtime.mdの「もの」)。持つのはworkerのスレッドだけである。`Env`が
//! factoryに渡し、`Exec::with`が項の間に貸す。

use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::Arc;

use eml_rt::{Control, Ctx, Mode};

use crate::cells::{self, CellRef, Here, Runnable, Shared};
use crate::error::RunFault;
use crate::job::{Entry, EntryKind};
use crate::policy::{Point, Policy};
use crate::rank;
use crate::run::{Cause, Mailer, Run, Signs};

/// worker `k`の核。`Send`ではなく、workerのスレッドにとどまり、そこで落とす。
pub struct WorkerCore<P> {
    index: u32,
    policy: P,
    control: Arc<Control>,
    run: Arc<Run>,
    signs: Arc<Signs>,
    mailer: Mailer,
    /// 準備のできた項。持ち主のworkerだけが触れる。
    ready: VecDeque<Entry>,
    /// `main`の項が置いた根のスコープ。
    root: Option<CellRef>,
    on_its_thread: PhantomData<*const ()>,
}

impl<P: Policy> WorkerCore<P> {
    pub(crate) fn new(
        index: u32,
        policy: P,
        control: Arc<Control>,
        run: Arc<Run>,
        signs: Arc<Signs>,
    ) -> WorkerCore<P> {
        WorkerCore {
            index,
            policy,
            control,
            mailer: Mailer::new(run.clone(), index),
            run,
            signs,
            ready: VecDeque::new(),
            root: None,
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

    /// runの登録表とそのmail。エンジンの行がこれで登録表をlockするのは、スケジューラのlockを1つも持たないときだけで
    /// ある (docs/spec/runtime.mdの「lockの順位」)。
    pub fn shared(&self) -> &Arc<Shared> {
        self.run.shared()
    }

    /// このworkerのフックの値。runの外でも読める。
    pub fn signs(&self) -> &Arc<Signs> {
        &self.signs
    }

    /// `cells`の関数に渡す、このworkerの`Here`。
    ///
    /// # Safety
    /// 返す値は`cells`の関数にだけ渡す (docs/spec/runtime.mdの「信頼の一覧」のSM-7)。
    pub unsafe fn here(&self) -> Here<'_> {
        Here {
            me: self.index,
            host: self.index,
            mail: &self.run.shared().mail,
            post: &self.mailer,
        }
    }

    /// `cells`が返した仕事を、準備のできた列の後ろに積む。積んだ項は語の参照を持つ。
    pub fn schedule(&mut self, run: Runnable) {
        let (cell, worker, kind, word) = match run {
            Runnable::Start {
                cell,
                worker,
                closure,
            } => (cell, worker, EntryKind::Start, closure),
            Runnable::Resume {
                cell,
                worker,
                k,
                wake,
            } => (cell, worker, EntryKind::Resume { wake }, k),
            Runnable::ResumeIo {
                cell,
                worker,
                k,
                wake,
                op,
            } => (cell, worker, EntryKind::ResumeIo { wake, op }, k),
            // tokenで待つのはVMのスタックを持ったエントリだけで、このスケジューラのエンジンはtokenで待たない
            Runnable::Token(_) => {
                self.run.record(
                    self.index,
                    Cause::Fault(RunFault::Internal(
                        "a token wait reached the native scheduler",
                    )),
                );
                return;
            }
        };
        self.ready.push_back(Entry {
            cell,
            worker,
            kind,
            word,
        });
    }

    /// 根のスコープを`scope`にする。`main`の項が呼ぶ。ループはそれを`main`が戻ったときに1回だけ読むので、後の呼び出しは
    /// 何も変えない (docs/spec/runtime.mdの「根」)。
    pub fn set_root(&mut self, scope: CellRef) {
        self.root = Some(scope);
    }

    pub(crate) fn root(&self) -> Option<CellRef> {
        self.root
    }

    pub(crate) fn run(&self) -> &Arc<Run> {
        &self.run
    }

    pub(crate) fn ready_len(&self) -> usize {
        self.ready.len()
    }

    pub(crate) fn take_ready(&mut self) -> Option<Entry> {
        self.ready.pop_front()
    }

    /// 受け箱の通り。`NOTICE`を下ろし、このworkerが取ってよいmailの項を処理し、それが再開するタスクを準備のできた列に
    /// 積む。
    ///
    /// # Safety
    /// `ctx`はこのworkerのスレッドの`Ctx`で、`Exec::with`が貸したものである (docs/spec/runtime.mdの「信頼の一覧」の
    /// SM-5)。
    pub(crate) unsafe fn take_mail<M: Mode>(&mut self, ctx: &mut Ctx<M>) -> Result<(), RunFault> {
        self.control.take_notice(self.index as usize);
        let mut out = Vec::new();
        // SAFETY: C1はこの関数の`# Safety`による。C3は`sequence::run`の`# Safety`による。driverは渡さないので、C4は貸す
        // 記憶を持たない。`Here`は`cells`の関数にだけ渡す (SM-7)。
        rank::cells(|| unsafe {
            cells::drain(self.shared(), &self.here(), ctx, None, 0, &mut out)
        })?;
        for run in out {
            self.schedule(run);
        }
        Ok(())
    }

    /// `point`を方針に知らせる。lockは持たない (docs/spec/runtime.mdの「フック」)。
    pub(crate) fn at(&self, point: Point) {
        rank::unlocked();
        self.policy.at(self.index, point);
    }
}

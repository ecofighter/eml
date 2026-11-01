//! run ごとに worker の間で共有する表 `Run` と、worker の空きの待ち (docs/spec/vm.md の「タスクとスケジュール」
//! 「時計と driver」)。`Run` が持つのは atomic の数と lock の下の小さな状態だけで、ヒープの語も `Ctx` も持たない。
//! ヒープに触れるのはその持ち主の worker のスレッドだけにするためである (docs/future/multicore.md の「スレッドと worker」)。

use std::any::Any;
use std::mem::ManuallyDrop;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{self, Scope, ScopedJoinHandle};

use eml_rt::{Control, Ctx, HostRef, IoWaker, Mode, Stop};

use crate::code::CodeBase;
use crate::doorbell::Doorbell;
use crate::driver::DriverKind;
use crate::sched::{JoinId, JoinNode, Outcome, Policy, Scheduler, driver_mut};
use crate::vm::{ExecWith, Task};
use crate::{RunCounts, RuntimeError, VmFault, WORKER_STACK};

// 4つの型はどれも、スレッドをまたいで渡すか共有する (docs/spec/vm.md の「タスクとスケジュール」)
const _: () = {
    const fn sync<T: Sync>() {}
    const fn send<T: Send>() {}
    sync::<Run>();
    send::<JoinNode>();
    send::<Outcome>();
    send::<Notice>();
};

/// ほかの worker から受け箱に届く知らせ (docs/spec/vm.md の「`par` の fork と join」)。
pub(crate) enum Notice {
    /// fork がこの worker に置いた右の子。受けた worker が自分のタスクの番号を振り、走れるタスクの列の後ろに入れる。
    Child(Task),
    /// この worker が置いた右の子の終わり。`join` はこの worker の join の番号で、`outcome` の語は子が残した参照である。
    Joined { join: JoinId, outcome: Outcome },
}

/// run の worker 1..N-1 の状態。worker 0 の `Scheduler` は run の始めに決め、worker 1..N-1 の `Scheduler` は
/// いつも `Peer` を持つ (docs/spec/vm.md の「タスクとスケジュール」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Workers {
    /// 起動の条件を満たしている。最初の `par` の fork で作る。worker 0 だけが持つ。
    Wanted,
    /// 作った。数は worker 0 を除く。
    Started(usize),
    /// 作らない。条件を満たさないか、1つも作れなかった。
    Denied,
    /// worker 1..N-1 の `Scheduler`。worker を作る判断はせず、右の子を置くことだけをする。作り直しの印 (`Wanted`) を
    /// 持たないので、worker を作るために戻る先のない実行の繰り返しが、fork から抜けることはない。
    Peer,
}

impl Workers {
    /// 右の子をほかの worker に置いてよいか。
    pub(crate) fn places(self) -> bool {
        matches!(self, Workers::Started(_) | Workers::Peer)
    }
}

/// worker のスレッドの結果。panic したら、その中身を worker 0 に運ぶ。`Ctx` は漏らしてある。
pub(crate) type WorkerExit<M> = Result<Ctx<M>, Box<dyn Any + Send>>;

/// worker の受け箱の lock の下にある状態。
struct SlotState {
    notices: Vec<Notice>,
    /// 起こされて、まだ空きの待ちが見ていない。
    woken: bool,
    /// 眠っている。始めた worker では、偽の間だけ `Run::running` に数える。
    parked: bool,
}

/// worker 1つ分の欄。
pub(crate) struct Slot {
    state: Mutex<SlotState>,
    /// 受け箱に知らせがある。集める地点が lock を取らずに1回の読みで確かめるためにある。
    pub(crate) has_notices: AtomicBool,
    cv: Condvar,
    /// driver を持つ worker の driver の waker。起こすときに driver の待ちも終わらせる。
    pub(crate) waker: OnceLock<Arc<dyn IoWaker>>,
    /// worker が動くタスクを替えた回数。監視のスレッドが横取りの判定に読む。
    pub(crate) switches: AtomicU64,
}

/// 1回の run で worker が共有する表。run ごとに作り、run の終わりに捨てる。
pub(crate) struct Run {
    /// worker ごとの欄。添字が worker の番号で、数は `Limits::workers` である。
    pub(crate) slots: Box<[Slot]>,
    /// 始めた worker のうち眠っていないものの数。`parked` を替えるときに、その欄の lock の下でだけ替える。
    pub(crate) running: AtomicUsize,
    /// 送った子の知らせのうち、終わりの知らせをまだ受け取っていないものの数。親を失った節点の分も数える。
    pub(crate) in_flight: AtomicUsize,
    /// ほかの worker に置いた右の子の数。どの worker の fork も足す。
    pub(crate) placed: AtomicU64,
    /// 受け箱が空で待ちに入った worker の番号。重ならない。
    idle: Mutex<Vec<usize>>,
    /// 監視のスレッドの自己パイプ。眠っていた worker が動き始めて `running` が 0 から 1 になったら鳴らし、刻まずに
    /// 眠っていた監視のスレッドを起こす。
    pub(crate) bell: Option<Arc<Doorbell>>,
    /// worker 1..N-1 の致命的な誤り。最初に書いたものだけを残し、worker 0 の結果より優先する。
    fatal: Mutex<Option<RuntimeError>>,
}

/// worker `k` の語に止めるビットか `END` が立っている。眠る直前に読み、立っていれば眠らずに戻る。監視のスレッドは
/// 動く worker のない間は眠りうるので、自分で立てたビット (確保量の上限など) に眠った後で気づく道がないためである。
fn stopping(control: &Control, k: usize) -> bool {
    control.worker_bits(k)
        & (Control::INTERRUPT | Control::TERMINATE | Control::MEMORY | Control::END)
        != 0
}

/// lock を取る。中の状態は1回の代入でしか替えないので、ほかのスレッドの panic で毒が付いても続けてよい。
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Run {
    /// worker が `workers` 個の run の表。始めた worker は worker 0 だけで、動いている。まだ始めていない worker の欄は
    /// 眠っていない形にしておく。起こしても `running` に数えないためである。
    pub(crate) fn new(workers: usize, bell: Option<Arc<Doorbell>>) -> Run {
        let slots = (0..workers)
            .map(|_| Slot {
                state: Mutex::new(SlotState {
                    notices: Vec::new(),
                    woken: false,
                    parked: false,
                }),
                has_notices: AtomicBool::new(false),
                cv: Condvar::new(),
                waker: OnceLock::new(),
                switches: AtomicU64::new(0),
            })
            .collect();
        Run {
            slots,
            running: AtomicUsize::new(1),
            in_flight: AtomicUsize::new(0),
            placed: AtomicU64::new(0),
            idle: Mutex::new(Vec::new()),
            bell,
            fatal: Mutex::new(None),
        }
    }

    /// 欄 `k` の lock の下で、眠っていれば起きた形にして `running` に数える。0 から 1 になったら監視のスレッドを起こす。
    fn unpark_locked(&self, state: &mut SlotState) {
        if state.parked {
            state.parked = false;
            if self.running.fetch_add(1, Ordering::AcqRel) == 0
                && let Some(bell) = &self.bell
            {
                bell.ring();
            }
        }
    }

    /// worker `k` を起きた形にする。driver の待ちから戻った worker が自分に対して呼ぶ。
    fn unpark(&self, k: usize) {
        self.unpark_locked(&mut lock(&self.slots[k].state));
    }

    /// worker `k` を起こす。眠っていれば条件変数か driver の waker で待ちを終わらせ、動いていれば次の空きの待ちを
    /// 1回だけ素通りさせる。
    pub(crate) fn wake(&self, k: usize) {
        let slot = &self.slots[k];
        {
            let mut state = lock(&slot.state);
            state.woken = true;
            self.unpark_locked(&mut state);
        }
        slot.cv.notify_one();
        if let Some(waker) = slot.waker.get() {
            waker.wake();
        }
    }

    /// worker `k` の受け箱に知らせを積む。欄の lock の下で積んで `k` を動いている形にし、`NOTICE` を立ててから、
    /// 条件変数と driver の waker で起こす。lock が知らせの中身の Release と Acquire になる
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn send(&self, control: &Control, k: usize, notice: Notice) {
        let slot = &self.slots[k];
        {
            let mut state = lock(&slot.state);
            state.notices.push(notice);
            slot.has_notices.store(true, Ordering::Release);
            self.unpark_locked(&mut state);
        }
        control.notice(k);
        slot.cv.notify_one();
        if let Some(waker) = slot.waker.get() {
            waker.wake();
        }
    }

    /// 空いている worker を1つ取り出す。取り出した worker には、置く側が `Child` を1つ送る。
    pub(crate) fn pop_idle(&self) -> Option<usize> {
        lock(&self.idle).pop()
    }

    /// すべての worker を起こす。止めるビットを眠っている worker に読ませるときに呼ぶ。
    pub(crate) fn wake_all(&self) {
        for k in 0..self.slots.len() {
            self.wake(k);
        }
    }

    /// 受け箱に知らせがあるか起こされていれば、起こされた印を下ろして真を返す。知らせは集める地点が取る。
    fn take_wake(&self, k: usize) -> bool {
        let mut state = lock(&self.slots[k].state);
        let woke = state.woken || !state.notices.is_empty();
        state.woken = false;
        woke
    }

    /// worker `k` の受け箱の知らせをすべて取り出す。
    pub(crate) fn take_notices(&self, k: usize) -> Vec<Notice> {
        let slot = &self.slots[k];
        let mut state = lock(&slot.state);
        slot.has_notices.store(false, Ordering::Relaxed);
        std::mem::take(&mut state.notices)
    }

    fn join_idle(&self, k: usize) {
        let mut idle = lock(&self.idle);
        if !idle.contains(&k) {
            idle.push(k);
        }
    }

    fn leave_idle(&self, k: usize) {
        lock(&self.idle).retain(|&i| i != k);
    }

    /// worker `k` を始める前の形にする。眠っていて空いている形で、`running` には数えない。作る前に置くので、作る途中の
    /// 起こしも取りこぼさない。始めていない欄には `wake_all` の起こした印が、語には監視のスレッドの `PREEMPT` が
    /// 溜まりうるので、どちらも下ろす。
    fn prepare(&self, control: &Control, k: usize) {
        {
            let mut state = lock(&self.slots[k].state);
            state.parked = true;
            state.woken = false;
            control.take_preempt(k);
        }
        self.join_idle(k);
    }

    /// 作れなかった worker `k` の欄を、始めていない形に戻す。準備の後に起こされていれば `running` から引く。
    fn unprepare(&self, k: usize) {
        {
            let mut state = lock(&self.slots[k].state);
            if !state.parked {
                self.running.fetch_sub(1, Ordering::AcqRel);
            }
            state.parked = false;
            state.woken = false;
        }
        self.leave_idle(k);
    }

    /// worker `k` (`k` ≥ 1) が眠った形で起こされるのを待つ。届いた知らせは受け箱に残し、続く集める地点が取る。
    fn park_wait(&self, k: usize) {
        let slot = &self.slots[k];
        let mut state = lock(&slot.state);
        while state.parked {
            state = slot.cv.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.woken = false;
        drop(state);
        self.leave_idle(k);
    }

    /// worker `k` (`k` ≥ 1) の空きの待ち。受け箱が空なら空きの集まりに入って眠り、起こされたら戻る。最後に眠った
    /// worker は worker 0 を起こす。worker 0 が、ほかの worker が動いている間は仮想の時計を進めずに待つためである
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    fn idle_wait(&self, control: &Control, k: usize) {
        let slot = &self.slots[k];
        {
            let mut state = lock(&slot.state);
            if state.woken || !state.notices.is_empty() {
                state.woken = false;
                return;
            }
        }
        self.join_idle(k);
        let previous = {
            let mut state = lock(&slot.state);
            if state.woken || !state.notices.is_empty() || stopping(control, k) {
                state.woken = false;
                drop(state);
                self.leave_idle(k);
                return;
            }
            state.parked = true;
            self.running.fetch_sub(1, Ordering::AcqRel)
        };
        if previous == 1 {
            self.wake(0);
        }
        self.park_wait(k)
    }

    /// worker `k` が抜ける。動いていれば眠った形にして `running` から引き、空きの集まりから外す。
    fn exit_worker(&self, k: usize) {
        {
            let mut state = lock(&self.slots[k].state);
            if !state.parked {
                state.parked = true;
                self.running.fetch_sub(1, Ordering::AcqRel);
            }
        }
        self.leave_idle(k);
    }

    /// worker 1..N-1 の致命的な誤りを書く。先に書かれていれば捨てる。
    fn set_fatal(&self, error: RuntimeError) {
        lock(&self.fatal).get_or_insert(error);
    }

    /// 書かれた致命的な誤りを取り出す。worker を join した後に呼ぶ。
    pub(crate) fn take_fatal(&self) -> Option<RuntimeError> {
        lock(&self.fatal).take()
    }
}

/// worker 1..N-1 に run の終わりを知らせる見張り。作った worker があれば、落とすときに `END` をすべての語に立てて
/// すべての worker を起こす。worker 0 が panic で `thread::scope` を抜けるときも、scope は worker の終わりを待つので、
/// 眠った worker を残さないように `Drop` に置く (docs/spec/vm.md の「タスクとスケジュール」)。
pub(crate) struct Shutdown {
    pub(crate) control: Arc<Control>,
    pub(crate) run: Arc<Run>,
    /// worker を作った。
    pub(crate) armed: bool,
}

impl Drop for Shutdown {
    fn drop(&mut self) {
        if self.armed {
            self.control.end_run();
            self.run.wake_all();
        }
    }
}

/// run の結果を決める。worker 1..N-1 の致命的な誤りがあれば、worker 0 の結果 (`exit` の `Stopped(Ended)` も含む)
/// より優先する。どのヒープも信用できないためである (docs/spec/vm.md の「タスクとスケジュール」)。
pub(crate) fn settle(
    result: Result<(), RuntimeError>,
    fatal: Option<RuntimeError>,
) -> Result<(), RuntimeError> {
    match fatal {
        Some(error) => Err(error),
        None => result,
    }
}

/// worker 1..N-1 を作る。作るのは worker 0 で、最初の `par` の fork のときに1回だけである。どの worker も眠って
/// 空いている形で始める。worker `k` を作れなければ、それより後は作らず、作れた worker だけを使う
/// (docs/spec/vm.md の「タスクとスケジュール」)。
pub(crate) fn spawn_workers<'scope, M: Mode, P: Policy + 'scope>(
    scope: &'scope Scope<'scope, '_>,
    code: &'scope CodeBase<M>,
    policy: &P,
    run: &Arc<Run>,
    control: &Arc<Control>,
    host: &HostRef,
    max_depth: usize,
) -> Vec<ScopedJoinHandle<'scope, WorkerExit<M>>> {
    let mut handles = Vec::new();
    for k in 1..run.slots.len() {
        run.prepare(control, k);
        let builder = thread::Builder::new()
            .name(format!("eml-worker-{k}"))
            .stack_size(WORKER_STACK);
        let (run_k, control_k, host_k) = (Arc::clone(run), Arc::clone(control), host.clone());
        // worker 0 は run の方針をそのまま使い、worker 1..N-1 は worker ごとの方針を使う
        let policy_k = policy.for_worker(k);
        // worker 0 のほかの worker は SIGINT と SIGTERM を塞ぐ (docs/spec/vm.md の「上限と割り込み」)
        match eml_io::spawn_scoped_masked(scope, builder, move || {
            run_worker::<M, P>(code, policy_k, run_k, control_k, host_k, k, max_depth)
        }) {
            Ok(handle) => handles.push(handle),
            Err(_) => {
                run.unprepare(k);
                break;
            }
        }
    }
    handles
}

/// worker `k` (`k` ≥ 1) のスレッドの本体。自分のスレッドで `Ctx` を作り、run の終わりに worker 0 へ返す。`Ctx` を
/// 作るところから `catch_unwind` の中で動き、panic は `fatal` に書いてすべての worker に終わりを知らせる。そのときの
/// `Ctx` は、途中の状態のヒープを落とさないように漏らす。
fn run_worker<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    policy: P,
    run: Arc<Run>,
    control: Arc<Control>,
    host: HostRef,
    k: usize,
    max_depth: usize,
) -> WorkerExit<M> {
    let mut slot: Option<ManuallyDrop<Ctx<M>>> = None;
    let body = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let ctx = slot.insert(ManuallyDrop::new(Ctx::<M>::for_worker(
            Arc::clone(&control),
            host,
            k,
        )));
        work::<M, P>(code, ctx, policy, &run, &control, k, max_depth);
    }));
    match body {
        Ok(()) => {
            run.exit_worker(k);
            let ctx = slot.expect("a worker that returns has built its Ctx");
            Ok(ManuallyDrop::into_inner(ctx))
        }
        Err(payload) => {
            run.set_fatal(RuntimeError::Fault {
                fault: VmFault::Internal("a worker thread panicked"),
                function: format!("<worker {k}>"),
                at: None,
            });
            control.end_run();
            run.wake_all();
            run.exit_worker(k);
            Err(payload)
        }
    }
}

/// worker `k` の実行。眠った形で始め、置かれた右の子を自分の `Scheduler` で動かす。抜けるのは止めるビットか `END` を
/// 見たときと、致命的な誤りのときである。止めるビットと `END` では何も書かずに、ほかの worker を起こしてから抜ける。
/// 眠っている worker に止める語を読ませ、自分の確保で立てた `MEMORY` も worker 0 に届けるためである。致命的な誤りは
/// run の `fatal` に書き、`END` をすべての語に立ててから同じく起こす (docs/spec/vm.md の「子の誤り」)。
fn work<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    policy: P,
    run: &Arc<Run>,
    control: &Arc<Control>,
    k: usize,
    max_depth: usize,
) {
    run.park_wait(k);
    let mut sched = Scheduler::worker(
        policy,
        Arc::clone(run),
        Arc::clone(control),
        k,
        ctx.heap().worker(),
        max_depth,
    );
    let mut counts = RunCounts::default();
    let error = crate::vm::work::<ExecWith<M, P>>(code, ctx, &mut sched, &mut counts);
    if !quiet(&error) {
        run.set_fatal(error);
        control.end_run();
    }
    run.wake_all();
}

/// worker 1..N-1 が何も書かずに抜ける誤り。止めるビットと `END` で、どれも worker 0 が自分の語で読む。
fn quiet(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Fault {
            fault: VmFault::Stopped(
                Stop::Interrupted | Stop::Terminated | Stop::MemoryLimit | Stop::Ended
            ) | VmFault::Extern(eml_rt::Fault::Stopped(
                Stop::Interrupted | Stop::Terminated | Stop::MemoryLimit | Stop::Ended
            )),
            ..
        }
    )
}

impl<P: Policy> Scheduler<P> {
    /// worker 0 の空きの待ち。走れるタスクがないときに呼び、起こされるか driver の待ちが戻ったら `Ok` で戻る。呼ぶ側は
    /// 戻るたびに止める語を読んで集め直す。デッドロックを決めるのは、受け箱が空で、期限も保留の操作も送った子もない
    /// ときだけである (docs/spec/vm.md の「タスクとスケジュール」「時計と driver」)。
    pub(crate) fn idle_wait(&mut self) -> Result<(), VmFault> {
        let run = Arc::clone(&self.run);
        let k = self.index;
        // worker 1..N-1 は driver も時計も持たないので、起こされるまで眠るだけである。デッドロックは worker 0 が決める
        if k != 0 {
            run.idle_wait(&self.watcher.control, k);
            return Ok(());
        }
        if run.take_wake(k) {
            return Ok(());
        }
        let mut deadline = self.cells.next_deadline();
        // OS のシグナルを待つタスクがいれば、ほかに進むものがなくてもシグナルが来るまで眠る。届いたときは監視の
        // スレッドが起こす
        if self.listening() {
            deadline = deadline.or(Some(u64::MAX));
        }
        let pending = driver_mut(&mut self.driver)?.pending();
        run.join_idle(k);
        let slot = &run.slots[k];
        let mut state = lock(&slot.state);
        if state.woken || !state.notices.is_empty() {
            state.woken = false;
            drop(state);
            run.leave_idle(k);
            return Ok(());
        }
        if deadline.is_none() && pending == 0 && run.in_flight.load(Ordering::Acquire) == 0 {
            drop(state);
            run.leave_idle(k);
            return Err(VmFault::Deadlock);
        }
        if stopping(&self.watcher.control, k) {
            drop(state);
            run.leave_idle(k);
            return Ok(());
        }
        state.parked = true;
        let others = run.running.fetch_sub(1, Ordering::AcqRel) - 1;
        let virtual_ = self.driver_kind == DriverKind::Virtual;
        // 仮想の時計を進めてよいのは、どの worker も動いておらず、どの受け箱にも知らせがないときだけである
        let waited = if virtual_ && others == 0 && (deadline.is_some() || pending > 0) {
            drop(state);
            Some(driver_mut(&mut self.driver)?.wait(deadline))
        } else if (virtual_ && others > 0) || (deadline.is_none() && pending == 0) {
            while state.parked {
                state = slot.cv.wait(state).unwrap_or_else(PoisonError::into_inner);
            }
            drop(state);
            None
        } else {
            drop(state);
            let deadline = self.wait_deadline(deadline)?;
            Some(driver_mut(&mut self.driver)?.wait(deadline))
        };
        let result = match waited {
            None => Ok(()),
            Some(waited) => {
                run.unpark(k);
                match waited {
                    Ok(true) => Ok(()),
                    // 送った子の終わりか知らせが残っていれば、driver が何も進まないと答えてもまだ進みうる
                    Ok(false)
                        if run.in_flight.load(Ordering::Acquire) == 0
                            && !slot.has_notices.load(Ordering::Acquire) =>
                    {
                        Err(VmFault::Deadlock)
                    }
                    Ok(false) => Ok(()),
                    Err(e) => Err(VmFault::Driver(e.to_string())),
                }
            }
        };
        run.leave_idle(k);
        result
    }
}

#[cfg(test)]
impl Run {
    /// worker `k` を眠った形にする。空きの待ちが眠るときと同じく、欄の lock の下で `running` から引く。
    pub(crate) fn park(&self, k: usize) {
        let mut state = lock(&self.slots[k].state);
        state.parked = true;
        self.running.fetch_sub(1, Ordering::AcqRel);
    }

    /// worker `k` が起こされるまで条件変数で待つ。
    pub(crate) fn wait_parked(&self, k: usize) {
        let slot = &self.slots[k];
        let mut state = lock(&slot.state);
        while state.parked {
            state = slot.cv.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use eml_rt::{Checked, Control, HeapError, Limits, Stop};

    use super::{Run, settle};
    use crate::{Machine, RuntimeError, VmFault};

    fn fault(fault: VmFault) -> RuntimeError {
        RuntimeError::Fault {
            fault,
            function: "main".to_string(),
            at: None,
        }
    }

    /// ほかの worker の致命的な誤りは、worker 0 のどの結果にも勝つ。`exit` の `Stopped(Ended)` にも勝つので、run は
    /// 捨てたことにならず誤りで終わる。誤りがなければ worker 0 の結果のままである。
    #[test]
    fn a_fatal_wins_over_any_result_of_worker_0() {
        let heap = || {
            fault(VmFault::Extern(eml_rt::Fault::Heap(
                HeapError::WrongOwner {
                    owner: 1,
                    worker: 2,
                },
            )))
        };
        // `RuntimeError` は写せないので、同じ結果を作り直す
        let result = |i: usize| match i {
            0 => Ok(()),
            1 => Err(fault(VmFault::Stopped(Stop::Ended))),
            2 => Err(fault(VmFault::Stopped(Stop::Interrupted))),
            _ => Err(RuntimeError::Deadlock),
        };
        for i in 0..4 {
            assert_eq!(settle(result(i), Some(heap())), Err(heap()));
            assert_eq!(settle(result(i), None), result(i));
        }
    }

    /// 眠っている worker を起こすと `running` に数え、もう一度起こしても数は増えない。
    #[test]
    fn waking_a_parked_worker_counts_it_once() {
        let run = Run::new(2, None);
        run.park(0);
        assert_eq!(run.running.load(Ordering::Acquire), 0);
        run.wake(0);
        run.wake(0);
        assert_eq!(run.running.load(Ordering::Acquire), 1);
        assert!(run.take_wake(0));
        assert!(!run.take_wake(0));
    }

    /// 右の子がリストを作り、親が join の後に手放す。worker が2つの run では子をほかの worker に置くので、親は
    /// worker 1 のヒープの物体を別のスレッドから解放し、worker 0 のヒープの統計の `remote_frees` に数える
    /// (docs/spec/object-model.md の「別のスレッドからの解放」)。統計を読むために `Machine` の `ctx` を見る。
    #[test]
    fn the_parent_frees_a_child_result_remotely() {
        let text = "\
layout (,) { (,)(tobj, tobj) }
layout Cell { Cell(tobj, tobj) }
effect Prelude.Panic { never panicked(obj) -> never }
fn main() -> unit {
  let t.0: obj = par (,) #0(&one, &list)
  decref t.0
  return ()
}
fn list(p.0: unit) -> tobj {
  let a.1: obj = con Cell #0(#1, #0)
  let b.2: obj = con Cell #0(#2, a.1)
  let c.3: obj = con Cell #0(#3, b.2)
  return c.3
}
fn one(p.0: unit) -> tobj {
  let n.1: tobj = box 1
  return n.1
}
clause Prelude.Panic.panicked fn fork$Prelude.Panic.panicked(ev.0: obj, a.1: obj) -> obj poly {
  let c.2: tobj = closure reperform$Prelude.Panic.panicked(a.1)
  let u.3: unit = prim child_abort(ev.0, c.2)
  decref ev.0
  let d.4: obj poly = prim dummy()
  return d.4
}
uniform from 1 fn reperform$Prelude.Panic.panicked(a.0: obj, u.1: unit) -> tobj poly {
  let r.2: tobj poly = extern Prelude.panic(a.0)
  return r.2
}
";
        let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            ..Limits::default()
        }));
        let (host, _) = crate::capture_host(Default::default());
        let mut machine = Machine::<Checked>::new(control, host);
        let entry = machine.load(&program).unwrap().entry;
        let finished = machine.run(entry).unwrap();
        assert_eq!(finished.placed, 1);
        assert!(
            machine.ctx.heap().stats().remote_frees >= 1,
            "{:?}",
            machine.ctx.heap().stats()
        );
        machine.close().unwrap();
    }

    /// まだ始めていない worker の欄を起こしても、`running` には数えない。
    #[test]
    fn waking_an_unstarted_worker_counts_nothing() {
        let run = Run::new(3, None);
        run.wake_all();
        assert_eq!(run.running.load(Ordering::Acquire), 1);
    }
}

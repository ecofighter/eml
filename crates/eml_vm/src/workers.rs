//! run ごとに worker の間で共有する表 `Run` と、worker の空きの待ち (docs/spec/vm.md の「タスクとスケジュール」
//! 「時計と driver」)。`Run` が持つのは atomic の数と lock の下の小さな状態だけで、`Ctx` もヒープも持たない。ヒープの語を
//! 持つのは欄の lock の下の受け箱と実行待ちの列だけで、`Run` はその語に触れずに運ぶ。ヒープに触れるのはその持ち主の
//! worker のスレッドだけにするためである (docs/future/multicore.md の「スレッドと worker」)。

use std::any::Any;
use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{self, Scope, ScopedJoinHandle};
use std::time::Duration;

use eml_rt::{Control, Ctx, HostRef, IoDriver, IoWaker, Mode, Stop, Word};

use crate::cells::{CellRef, HostBin, Post, Shared, Woken};
use crate::code::CodeBase;
use crate::counters::Counters;
use crate::doorbell::Doorbell;
use crate::driver::{DriverKind, JOB_QUIESCE_LIMIT, RunClock, ceil_ms, floor_ms};
use crate::park::Park;
use crate::sched::{JoinId, JoinNode, Outcome, Policy, Scheduler, TaskId, driver_mut};
use crate::vm::{ExecWith, Task};
use crate::{HeapWork, RunCounts, RuntimeError, VmFault, WORKER_STACK};

// 6つの型はどれも、スレッドをまたいで渡すか共有する (docs/spec/vm.md の「タスクとスケジュール」)
const _: () = {
    const fn sync<T: Sync>() {}
    const fn send<T: Send>() {}
    sync::<Run>();
    send::<JoinNode>();
    send::<Outcome>();
    send::<Notice>();
    send::<Ready>();
    send::<Entry>();
};

/// worker の実行待ちの列の項 (docs/spec/vm.md の「タスクとスケジュール」)。
pub enum Ready {
    /// このworkerの表にあるVMの`Task`。`par`の右の子、`Seeded`が切り替えたタスク、joinで待つ親、`Async.run`で待つ`Task`
    /// で、フレームを持つ。
    Task(TaskId),
    /// フレームを持たない `Async` の仕事。持ち主が `pick` で取ったときにエントリの `Task` にする。
    Entry(Entry),
}

/// フレームを持たない `Async` の仕事。セル、タスクの論理的な ID、種類と語を1つずつ持ち、ほかの状態は登録表にある。
pub struct Entry {
    pub(crate) cell: CellRef,
    pub(crate) worker: u32,
    pub(crate) kind: EntryKind,
    /// 始める仕事では閉包、再開する仕事では継続で、項が参照を1つ持つ。
    pub(crate) word: Word,
}

/// `Entry` が入るエントリの関数と、語のほかに渡す即値。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Start,
    Resume { wake: i64 },
    ResumeIo { wake: i64, op: i64 },
}

/// ほかの worker から受け箱に届く知らせ (docs/spec/vm.md の「`par` の fork と join」「タスクとスケジュール」)。
/// 語を運ぶのは `Child`、`Joined`、`Place` だけで、ほかの `Async` のタスクの知らせはセルの番号しか持たない。driver の
/// 操作は記録が持ち、知らせは運ばない (docs/spec/vm.md の「操作の記録」)。
pub(crate) enum Notice {
    /// fork がこの worker に置いた右の子か、`fork_job` が置いた `compute` の仕事。受けた worker が自分のタスクの番号を
    /// 振り、走れるタスクの列の後ろに入れる。
    Child(Task),
    /// この worker が置いた右の子の終わり。`join` はこの worker の join の番号で、`outcome` の語は子が残した参照である。
    Joined { join: JoinId, outcome: Outcome },
    /// `start` がこの worker を持ち主にしたタスク。`closure` はタスクの論理的な ID `worker` に印を付けた閉包で、
    /// 知らせが参照を1つ持つ。受けた worker が始める仕事にする。
    Place {
        cell: CellRef,
        worker: u32,
        closure: Word,
    },
    /// この worker が持ち主のタスクか待つ `Task` の起こし。継続はセルにあり、受けた worker が取り出す。
    Wake(Woken),
    /// この worker が持ち主のタスクの取り消し。受けた worker が待ちを外す。
    Cancel(CellRef),
    /// この worker が host の記録に、持ち主が操作を置いた。受けた worker が取り出して自分の driver に出す。
    Submit(CellRef),
    /// この worker が host の記録の、driver に出した操作の取り消し。
    CancelIo(CellRef),
    /// この worker が持ち主の記録に、host が結果を置いた。受けた worker が当てて段階を進める。
    Completion(CellRef),
    /// このworkerが持ち主の、動いているタスク`cell`への上限の旗。受けたworkerが登録表のlockの下で旗がまだ立っているかを
    /// 確かめ、立っていればそのVMの`Task`に旗を付ける。語を持たず、`in_flight`に数えない。runの終わりでは仕事に数えず、
    /// 捨てる (docs/spec/vm.mdの「タスクとスケジュール」「タスクの上限」)。
    Limit(CellRef),
}

/// 登録表の lock の下から、ほかの worker の受け箱に `Async` のタスクの知らせを積む道 (docs/spec/vm.md の
/// 「タスクとスケジュール」)。欄の lock は登録表の lock の内側なので、持ったまま積める。
pub(crate) struct Mailer<'a> {
    run: &'a Run,
    control: &'a Control,
}

impl<'a> Mailer<'a> {
    pub(crate) fn new(run: &'a Run, control: &'a Control) -> Mailer<'a> {
        Mailer { run, control }
    }
}

impl Post for Mailer<'_> {
    fn wake(&self, k: u32, w: Woken) {
        self.run.send(self.control, k as usize, Notice::Wake(w));
    }

    fn cancel(&self, k: u32, cell: CellRef) {
        self.run
            .send(self.control, k as usize, Notice::Cancel(cell));
    }

    fn place(&self, k: u32, cell: CellRef, worker: u32, closure: Word) {
        let notice = Notice::Place {
            cell,
            worker,
            closure,
        };
        self.run.send(self.control, k as usize, notice);
    }

    fn submit(&self, k: u32, op: CellRef) {
        self.run.send(self.control, k as usize, Notice::Submit(op));
    }

    fn cancel_io(&self, k: u32, op: CellRef) {
        self.run
            .send(self.control, k as usize, Notice::CancelIo(op));
    }

    fn completion(&self, k: u32, op: CellRef) {
        self.run
            .send(self.control, k as usize, Notice::Completion(op));
    }

    fn limit(&self, k: u32, cell: CellRef) {
        self.run.send(self.control, k as usize, Notice::Limit(cell));
    }
}

/// run の worker 1..N-1 の状態。worker 0 の `Scheduler` は run の始めに決め、worker 1..N-1 の `Scheduler` は
/// いつも `Peer` を持つ (docs/spec/vm.md の「タスクとスケジュール」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Workers {
    /// 作った。数は worker 0 を除く。
    Started(usize),
    /// 作らない。条件を満たさないか、1つも作れなかった。
    Denied,
    /// worker 1..N-1 の `Scheduler`。worker を作る判断はせず、右の子とタスクと仕事を置くことだけをする。
    Peer,
}

impl Workers {
    /// 右の子と `Async` のタスクと `compute` の仕事をほかの worker に置いてよいか。
    pub(crate) fn places(self) -> bool {
        matches!(self, Workers::Started(_) | Workers::Peer)
    }
}

/// worker のスレッドの結果。panic したら、その中身を worker 0 に運ぶ。`Ctx` は漏らしてある。
pub(crate) type WorkerExit<M> = Result<Retired<M>, Box<dyn Any + Send>>;

/// 抜けた worker が worker 0 に返すもの。`Ctx` のほかに、`RunStats` に足す worker の仕事を運ぶ
/// (docs/spec/vm.md の「`RunStats`」)。
pub(crate) struct Retired<M: Mode> {
    pub(crate) ctx: Ctx<M>,
    /// VM が数える回数。
    pub(crate) counts: RunCounts,
    pub(crate) preemptions: u64,
    /// worker のタスクのスタックで同時に使った語の数の最大。
    pub(crate) peak_stack: u64,
    /// `Ctx` を作った直後のヒープの仕事。ヒープごとのダミーの物体の確保は実行の仕事ではないので、`RunStats` から引く
    /// (docs/spec/vm.md の「`RunStats`」)。
    pub(crate) base: HeapWork,
    /// 最後の集める地点で、この worker が走らせるはずの知らせ (`Child`、`Place`、`Wake`) が受け箱に残っていた。成功した
    /// run でも、タスクを残したまま終わったことになる (docs/spec/vm.md の「誤り、止めること、終わり」)。
    pub(crate) left: bool,
    /// この worker の driver が、静止させた後にも保留の操作を残した。run の `Machine` を poisoned にする
    /// (docs/spec/vm.md の「時計と driver」)。
    pub(crate) kept_pending: bool,
}

/// worker の受け箱の lock の下にある状態。
struct SlotState {
    notices: Vec<Notice>,
    /// 起こされて、まだ空きの待ちが見ていない。
    woken: bool,
    /// 眠っている。始めた worker では、偽の間だけ `Run::running` に数える。
    parked: bool,
}

/// `Machine`を持つスレッドと、`main`を動かすworker 0の関係。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// `Machine`を持つスレッドがworker 0を兼ねる。`eml repl`と`Machine`の既定である。
    Merged,
    /// `Machine`を持つスレッドがメインのworkerになり、worker 0は別のスレッドで動く。
    Separate,
}

/// worker 1つ分の欄。
pub(crate) struct Slot {
    state: Mutex<SlotState>,
    /// 受け箱に知らせがある。集める地点が lock を取らずに1回の読みで確かめるためにある。
    pub(crate) has_notices: AtomicBool,
    cv: Condvar,
    /// driver を持つ worker の driver の waker。起こすときに driver の待ちも終わらせる。
    pub(crate) waker: OnceLock<Arc<dyn IoWaker>>,
    /// 分かれたrunのメインのworkerの`Park`のwaker。runごとに、メインのworkerが繰り返しを始める前に1回だけ置く
    /// (docs/spec/vm.mdの「メインの worker の待ち」)。
    pub(crate) park_waker: OnceLock<Arc<dyn IoWaker>>,
    /// driver を持つ worker の driver の bin。ほかの worker は集める地点で印を見て、host を起こす
    /// (docs/spec/vm.md の「時計と driver」)。
    pub(crate) bin: OnceLock<Arc<HostBin>>,
    /// 走れるタスクの列。受け箱とは別の lock で、持つ間はほかの lock を取らない。触れるのは `Run` の列の操作だけである。
    ready: Mutex<VecDeque<Ready>>,
    /// `current`、`bytes`、`mark` の版の数。持ち主が3つを書く前に奇数に、書いた後に偶数に進める。読み手は前後で同じ
    /// 偶数を読めたときだけ3つを組として使う。同じセルを数え直して公開し直すことがあるので、`current` を2回比べる
    /// 形では古い組を受け入れてしまう (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) seq: AtomicU64,
    /// 動いている `Async` のタスクのセルの符号 (`cell_word`)。なければ `NO_CELL`。持ち主の worker が切り替えの地点で
    /// 書き、監視のスレッドが lock なしで読む。
    pub(crate) current: AtomicU64,
    /// `current` のタスクの今の区間より前の数と、区間の始めの `Control::taken`。動いているタスクの数は
    /// `bytes + taken(k) - mark` である。
    pub(crate) bytes: AtomicUsize,
    pub(crate) mark: AtomicUsize,
    /// このworkerの実行待ちのうち、数のいちばん大きいタスクのセルの符号とその数。なければ`NO_CELL`。持ち主だけが書く。
    pub(crate) top: AtomicU64,
    pub(crate) top_bytes: AtomicUsize,
}

impl Slot {
    /// 眠っているworkerの待ちを終わらせる。条件変数と、driverのwakerと、`Park`のwakerを鳴らす。
    pub(crate) fn ring(&self) {
        self.cv.notify_one();
        if let Some(waker) = self.waker.get() {
            waker.wake();
        }
        if let Some(waker) = self.park_waker.get() {
            waker.wake();
        }
    }
}

/// `Slot::current` と `Slot::top` の、タスクのないことを表す値。
pub(crate) const NO_CELL: u64 = u64::MAX;

/// `Slot::current` と `Slot::top` に置くセルの符号。
pub(crate) fn cell_word(cell: CellRef) -> u64 {
    cell.encode() as u64
}

/// 1回の run で worker が共有する表。run ごとに作り、run の終わりに捨てる。
pub(crate) struct Run {
    /// workerごとの欄。添字がworkerの番号で、数は`Limits::workers`に、`Separate`ではメインのworkerの分を1つ足したものである。
    pub(crate) slots: Box<[Slot]>,
    /// `main`とタスクを動かすworkerの数。メインのworkerを含まない。
    workers: usize,
    seat: Seat,
    /// 始めた worker のうち眠っていないものの数。`parked` を替えるときに、その欄の lock の下でだけ替える。
    pub(crate) running: AtomicUsize,
    /// メインのworkerの`Park`がrunの外の待ちを抱えている。偽で始まり、`Pending::set_holds`が書く。worker 0は2つの
    /// デッドロックの判定で`running`の後にAcquireで読み、真の間は判定せずに待つ (docs/spec/vm.mdの
    /// 「メインの worker の待ち」)。
    holds: AtomicBool,
    /// 送った子の知らせのうち、終わりの知らせをまだ受け取っていないものの数。親を失った節点の分も数える。
    pub(crate) in_flight: AtomicUsize,
    /// ほかの worker に置いた右の子の数。どの worker の fork も足す。
    pub(crate) placed: AtomicU64,
    /// ほかの worker に置いた `compute` の仕事の数。どの worker の `fork_job` も足す。
    pub(crate) placed_jobs: AtomicU64,
    /// 次に置く`Async`のタスクの持ち主を決める数。0から始め、`start`のたびに1つ進めて、worker 0を除く作ったworkerの
    /// 数で割った余りに1を足したものを持ち主にする。`main`を動かすworker 0には`start`でタスクを置かない
    /// (docs/spec/vm.mdの「`Async`のタスク」)。
    next_place: AtomicUsize,
    /// 作ったworkerの数で、worker 0を含む。worker 0がworker 1..N-1を作った後、runの最初の命令の前に1回だけ書く。
    /// 作れなかったworkerの欄には置かないためである。
    live: AtomicUsize,
    /// 受け箱が空で待ちに入った worker の番号。重ならない。
    idle: Mutex<Vec<usize>>,
    /// run の時間の基準。VM が時刻を読む道はこれだけである (docs/spec/vm.md の「時計と driver」)。
    clock: RunClock,
    /// 監視のスレッドの自己パイプ。眠っていた worker が動き始めて `running` が 0 から 1 になったら鳴らし、刻まずに
    /// 眠っていた監視のスレッドを起こす。
    pub(crate) bell: Option<Arc<Doorbell>>,
    /// worker 1..N-1 の致命的な誤り。最初に書いたものだけを残し、worker 0 の結果より優先する。
    fatal: Mutex<Option<RuntimeError>>,
    /// `Std.Atomic.Counter` の表。`Machine` が持ち、run が終わっても残る。lock を取らずに読み書きする。
    pub(crate) counters: Arc<Counters>,
    /// run のすべての静止が分け合う、`Op::Job` を待てる長さ (docs/spec/vm.md の「時計と driver」)。
    pub(crate) job_budget: Duration,
    /// 仕事を待つ期限。run の時間の基準の時刻で、最初の静止が決める。worker ごとに持つと N worker で N 倍待つので、
    /// run に1つだけ置く。
    job_deadline: OnceLock<Duration>,
}

/// worker `k` の語に止めるビットか `END` が立っている。眠る直前に読み、立っていれば眠らずに戻る。監視のスレッドは
/// 動く worker のない間は眠りうるので、自分で立てたビット (確保量の上限など) に眠った後で気づく道がないためである。
pub(crate) fn stopping(control: &Control, k: usize) -> bool {
    control.worker_bits(k)
        & (Control::INTERRUPT | Control::TERMINATE | Control::MEMORY | Control::END)
        != 0
}

/// lock を取る。中の状態は1回の代入でしか替えないので、ほかのスレッドの panic で毒が付いても続けてよい。
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Run {
    /// workerが`workers`個のrunの表。始めたworkerはworker 0だけで、動いている。まだ始めていないworkerの欄は
    /// 眠っていない形にしておく。起こしても`running`に数えないためである。`Separate`ではメインのworkerの欄を
    /// 最後に足し、始めてあるが眠っている形にする。
    pub(crate) fn new(
        workers: usize,
        seat: Seat,
        bell: Option<Arc<Doorbell>>,
        clock: RunClock,
        counters: Arc<Counters>,
    ) -> Run {
        let separate = seat == Seat::Separate;
        let slots = (0..workers + usize::from(separate))
            .map(|k| Slot {
                state: Mutex::new(SlotState {
                    notices: Vec::new(),
                    woken: false,
                    parked: separate && k == workers,
                }),
                has_notices: AtomicBool::new(false),
                cv: Condvar::new(),
                waker: OnceLock::new(),
                park_waker: OnceLock::new(),
                bin: OnceLock::new(),
                ready: Mutex::new(VecDeque::new()),
                seq: AtomicU64::new(0),
                current: AtomicU64::new(NO_CELL),
                bytes: AtomicUsize::new(0),
                mark: AtomicUsize::new(0),
                top: AtomicU64::new(NO_CELL),
                top_bytes: AtomicUsize::new(0),
            })
            .collect();
        Run {
            slots,
            workers,
            seat,
            running: AtomicUsize::new(1),
            holds: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            placed: AtomicU64::new(0),
            placed_jobs: AtomicU64::new(0),
            next_place: AtomicUsize::new(usize::from(separate)),
            live: AtomicUsize::new(1),
            idle: Mutex::new(Vec::new()),
            clock,
            bell,
            fatal: Mutex::new(None),
            counters,
            job_budget: JOB_QUIESCE_LIMIT,
            job_deadline: OnceLock::new(),
        }
    }

    /// `main`とタスクを動かすworkerの数。メインのworkerを含まない。
    pub(crate) fn workers(&self) -> usize {
        self.workers
    }

    /// メインのworkerの番号。`Separate`では最後の欄で、`Merged`ではworker 0がメインのworkerを兼ねる。
    pub(crate) fn main(&self) -> usize {
        if self.separate() { self.workers } else { 0 }
    }

    pub(crate) fn separate(&self) -> bool {
        self.seat == Seat::Separate
    }

    /// worker 0が最初に作るdriver。`Separate`ではworker 0のスレッドで作る。`Box<dyn IoDriver>`は`Send`でないので、作ったスレッドで
    /// 捨てるためである。
    pub(crate) fn entry_driver(&self, seed: Option<u64>) -> std::io::Result<Box<dyn IoDriver>> {
        self.clock.driver(seed)
    }

    /// 静止で仕事を待てる残り。run の最初の静止が期限を決め、ほかの worker はそれを分け合う。
    pub(crate) fn job_wait(&self, now: Duration) -> Duration {
        self.job_deadline
            .get_or_init(|| now.saturating_add(self.job_budget))
            .saturating_sub(now)
    }

    /// worker 1..N-1 を作った後に、作れた数 (worker 0 を含む) を書く。
    pub(crate) fn set_live(&self, n: usize) {
        self.live.store(n, Ordering::Release);
    }

    /// 作った worker の数で、worker 0 を含む。worker 0 が書いた後に動き始めた worker と、その知らせを受けた worker が読む。
    pub(crate) fn live(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }

    /// 次に始める`Async`のタスクの持ち主。worker 0を除く作ったworkerをworker 1から順に回り、ほかに作ったworkerが
    /// なければworker 0である。数えるだけの数なのでRelaxedで足し、作った数はworker 0が書いた後の知らせを受けてから
    /// 読むので、Acquireで足りる (docs/spec/vm.mdの「`Async`のタスク」)。
    pub(crate) fn next_owner(&self) -> u32 {
        let live = self.live();
        if live <= 1 {
            return 0;
        }
        (1 + self.next_place.fetch_add(1, Ordering::Relaxed) % (live - 1)) as u32
    }

    /// `start`が新しいタスクを置くworker。固定したタスクはメインのworkerに置く。ほかのタスクは、workerを作ったrunでは
    /// `next_owner`の輪に置き、作らないrunでは呼んだworker `me`に置く。タスクは置いたworkerから動かない
    /// (docs/spec/vm.mdの「`Async`のタスク」)。
    pub(crate) fn place_owner(&self, places: bool, me: u32, pinned: bool) -> u32 {
        if pinned {
            self.main() as u32
        } else if places {
            self.next_owner()
        } else {
            me
        }
    }

    /// runのdriverの出どころ。
    pub(crate) fn driver_kind(&self) -> DriverKind {
        self.clock.kind()
    }

    /// worker `k` のタスクが新しいハンドルを作る操作を受け付ける worker (host)。本物の時計の run では各 worker が自分の
    /// driver を持つので `k` である。テストの driver の仮想のネットワークは driver ごとにあるので、ほかの run では driver を
    /// worker 0 の1つに集め、どの worker の操作も worker 0 が driver に出す (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn io_worker(&self, k: usize) -> usize {
        match self.clock {
            RunClock::Real { .. } => k,
            RunClock::Virtual(_) | RunClock::Injected => 0,
        }
    }

    /// worker が2つ以上あり、各 worker が自分の driver の host になる run か。
    pub(crate) fn hosts_apart(&self) -> bool {
        self.slots.len() > 1 && matches!(self.clock, RunClock::Real { .. })
    }

    /// 本物の時計の run で、worker `k` (`k` ≥ 1) が最初に要るときに作る driver。run の時間の基準から作るので、どの
    /// worker の driver も同じ時刻を読む。ほかの run では作らず `None` である。渡された driver の run (`Injected`) は
    /// worker を作らないので、基準から driver を作る道を通らない (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn new_driver(&self) -> Option<std::io::Result<Box<dyn IoDriver>>> {
        match self.clock {
            RunClock::Real { .. } => Some(self.clock.driver(None)),
            RunClock::Virtual(_) | RunClock::Injected => None,
        }
    }

    /// 満期の判定で読む今の時刻 (ミリ秒、切り捨て)。`driver` は基準が渡された driver の `Injected` のときだけ読む。
    pub(crate) fn now_ms_floor(&self, driver: Option<&dyn IoDriver>) -> Result<u64, VmFault> {
        self.clock.now(driver).map(floor_ms)
    }

    /// 登録で読む今の時刻 (ミリ秒、切り上げ)。
    pub(crate) fn now_ms_ceil(&self, driver: Option<&dyn IoDriver>) -> Result<u64, VmFault> {
        self.clock.now(driver).map(ceil_ms)
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
    pub(crate) fn unpark(&self, k: usize) {
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
        slot.ring();
    }

    /// worker `k` の受け箱に知らせを積む。欄の lock の下で積んで `k` を動いている形にし、`NOTICE` を立ててから、
    /// 眠っていた相手だけを条件変数と driver の waker で起こす。lock が知らせの中身の Release と Acquire になる
    /// (docs/spec/vm.md の「タスクとスケジュール」)。眠っていない相手は、眠る前に欄の lock の下で受け箱を見るので、
    /// 起こさなくても知らせを取りこぼさない。登録表の lock の下から送る知らせが、たいていは syscall を伴わないためである。
    pub(crate) fn send(&self, control: &Control, k: usize, notice: Notice) {
        let slot = &self.slots[k];
        let parked = {
            let mut state = lock(&slot.state);
            state.notices.push(notice);
            slot.has_notices.store(true, Ordering::Release);
            let parked = state.parked;
            self.unpark_locked(&mut state);
            parked
        };
        control.notice(k);
        if parked {
            slot.ring();
        }
    }

    /// worker `k` が、自分のほかの host の bin に積んだハンドルの印を見て、まだ誰も起こしていなければ host を起こす。
    /// host は自分の待ちと集める地点でしか bin を空にしないためである。集める地点の終わりで、どの lock も持たずに
    /// 呼ぶ (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn ring_hosts(&self, k: usize) {
        for (h, slot) in self.slots.iter().enumerate() {
            if h != k
                && let Some(bin) = slot.bin.get()
                && bin.take_ring()
            {
                self.wake(h);
            }
        }
    }

    /// worker `k` の実行待ちの列の後ろに積む。
    pub(crate) fn push_ready(&self, k: usize, item: Ready) {
        lock(&self.slots[k].ready).push_back(item);
    }

    /// worker `k` の実行待ちの列から、方針 `policy` が選ぶ項を取り出す。
    pub(crate) fn pick_ready<P: Policy>(&self, k: usize, policy: &mut P) -> Option<Ready> {
        policy.pick(&mut lock(&self.slots[k].ready))
    }

    /// 起こしたタスク `id` を、方針 `policy` が決める位置で worker `k` の実行待ちの列に入れる。
    pub(crate) fn wake_ready<P: Policy>(&self, k: usize, policy: &mut P, id: TaskId) {
        policy.wake(&mut lock(&self.slots[k].ready), id);
    }

    /// worker `k` の実行待ちの列のうち、`keep` が真を返す項だけを順に残す。`keep` は列の lock の下で呼ぶので、項を
    /// 見るほかに何もしない。
    pub(crate) fn retain_ready(&self, k: usize, keep: impl FnMut(&Ready) -> bool) {
        lock(&self.slots[k].ready).retain(keep);
    }

    /// worker `k`の実行待ちのタスク`cell` (数`count`) を、今の`top`より大きいか同じなら`top`にする。
    /// 同じセルなら数を書き直す。`top`と`top_bytes`は組として読めないので、読み手は古いセルと新しい数の組を見うる。
    /// 旗はそれでも生きたセルへ行き、数は選び方を変えるだけなので害はない (docs/spec/vm.mdの「タスクとスケジュール」)。
    pub(crate) fn offer_top(&self, k: usize, cell: CellRef, count: usize) {
        let slot = &self.slots[k];
        let word = cell_word(cell);
        let top = slot.top.load(Ordering::Relaxed);
        if top == NO_CELL || top == word || count >= slot.top_bytes.load(Ordering::Relaxed) {
            slot.top_bytes.store(count, Ordering::Relaxed);
            slot.top.store(word, Ordering::Release);
        }
    }

    /// worker `k` の `top` が `cell` なら消す。ほかのセルに替わっていれば残す。
    pub(crate) fn clear_top(&self, k: usize, cell: CellRef) {
        let _ = self.slots[k].top.compare_exchange(
            cell_word(cell),
            NO_CELL,
            Ordering::Release,
            Ordering::Relaxed,
        );
    }

    /// worker `k` の `top` を消す。実行待ちが空になったときに持ち主が呼ぶ。
    pub(crate) fn drop_top(&self, k: usize) {
        self.slots[k].top.store(NO_CELL, Ordering::Release);
    }

    pub(crate) fn ready_is_empty(&self, k: usize) -> bool {
        lock(&self.slots[k].ready).is_empty()
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

    /// worker `k`を始める前の形にする。眠っていて空いている形で、`running`には数えない。作る前に置くので、作る途中の
    /// 起こしも取りこぼさない。始めていない欄には`wake_all`の起こした印が
    /// 溜まりうるので下ろす。
    fn prepare(&self, k: usize) {
        {
            let mut state = lock(&self.slots[k].state);
            state.parked = true;
            state.woken = false;
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

    /// worker `k` が眠った形で起こされるのを待つ。届いた知らせは受け箱に残し、続く集める地点が取る。
    fn sleep(&self, k: usize) {
        let slot = &self.slots[k];
        let mut state = lock(&slot.state);
        while state.parked {
            state = slot.cv.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.woken = false;
    }

    /// worker `k` (`k` ≥ 1) が起こされるまで待ち、空きの集まりから外れる。
    fn park_wait(&self, k: usize) {
        self.sleep(k);
        self.leave_idle(k);
    }

    /// worker `k` が driver の待ちから戻った。起こされていなければ自分で起きた形に戻す。
    fn woke(&self, k: usize) {
        let mut state = lock(&self.slots[k].state);
        self.unpark_locked(&mut state);
        state.woken = false;
    }

    /// worker `k` (`k` ≥ 1) が driver の待ちから戻った。起こされていなければ自分で起きた形に戻し、空きの集まりから
    /// 外れる。
    fn driver_waited(&self, k: usize) {
        self.woke(k);
        self.leave_idle(k);
    }

    /// メインの worker の空きの待ちの前半。`park_idle` と同じだが、空きの集まりには入らないので、`pop_idle` が選ぶこと
    /// はない。メインの worker は眠った形で始まるので、その形では真を返すだけである。最後に眠った worker なら worker 0 を
    /// 起こす (docs/spec/vm.md の「メインの worker」)。
    pub(crate) fn park_main(&self, control: &Control, k: usize) -> bool {
        let slot = &self.slots[k];
        let previous = {
            let mut state = lock(&slot.state);
            if state.woken || !state.notices.is_empty() || stopping(control, k) {
                state.woken = false;
                return false;
            }
            if state.parked {
                return true;
            }
            state.parked = true;
            self.running.fetch_sub(1, Ordering::AcqRel)
        };
        if previous == 1 {
            self.wake(0);
        }
        true
    }

    /// メインのworkerの`Park`がrunの外の待ちを抱えているかを書き、前の値を返す。
    ///
    /// ここで書いた真を、メインのworkerの次の眠りの後のどの判定も読むのは、次のためである。worker 0が判定するのは、
    /// 自分の引き算かその後の読みで`running`が0だと読んだ後だけである。`running`の変更はどれも読んで書き換える操作
    /// なので、メインのworkerの次の眠りの引き算か、そのrelease sequenceの後の書き込みから読む読みは、この書き込みより
    /// 後に起きる。この書き込みはプログラムの順でその引き算より前にあるためである。読みがメインのworkerのそれより前の
    /// 眠りから来る判定は、見逃しうる。`Park`はタスクを動かさせる前に立てるので、そのタスクが始めた待ちで眠った
    /// メインのworkerの判定は、立てた真を読む。
    pub(crate) fn swap_holds(&self, holds: bool) -> bool {
        self.holds.swap(holds, Ordering::AcqRel)
    }

    fn holds(&self) -> bool {
        self.holds.load(Ordering::Acquire)
    }

    /// メインのworker `k`が`Park`から戻った。起こされたか、知らせがあるか、もう眠った形でないか、止める語を読めば、
    /// 起きた形にして真を返す。どれもなければ眠った形のままにする。何も変わらずに戻る`Park`は、どの数も替えない。
    pub(crate) fn main_woken(&self, control: &Control, k: usize) -> bool {
        let mut state = lock(&self.slots[k].state);
        let woke =
            state.woken || !state.notices.is_empty() || !state.parked || stopping(control, k);
        if woke {
            state.woken = false;
            self.unpark_locked(&mut state);
        }
        woke
    }

    /// メインのworkerの`Park`がrunの外の待ちを抱えている間に、worker 0 (`k`) が進むものを見つけられなかった。
    /// デッドロックと決める代わりに、起こされるまで条件変数で眠る。
    fn hold_sleep(&self, control: &Control, k: usize) {
        let slot = &self.slots[k];
        let mut state = lock(&slot.state);
        if state.woken || !state.notices.is_empty() || stopping(control, k) {
            state.woken = false;
            return;
        }
        state.parked = true;
        self.running.fetch_sub(1, Ordering::AcqRel);
        while state.parked {
            state = slot.cv.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.woken = false;
    }

    /// worker `k` (`k` ≥ 1) の空きの待ちの前半。眠ってよければ眠った形にして真を返し、知らせか起こしか止める語が
    /// あれば眠らずに偽を返す。
    fn park_idle(&self, control: &Control, k: usize) -> bool {
        let slot = &self.slots[k];
        {
            let mut state = lock(&slot.state);
            if state.woken || !state.notices.is_empty() {
                state.woken = false;
                return false;
            }
        }
        self.join_idle(k);
        let previous = {
            let mut state = lock(&slot.state);
            if state.woken || !state.notices.is_empty() || stopping(control, k) {
                state.woken = false;
                drop(state);
                self.leave_idle(k);
                return false;
            }
            state.parked = true;
            self.running.fetch_sub(1, Ordering::AcqRel)
        };
        if previous == 1 {
            self.wake(0);
        }
        true
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

/// worker 1..N-1を作る。作るのはworker 0で、runの始め、最初の命令の前に1回だけである。どのworkerも眠って空いている
/// 形で始める。worker `k`を作れなければ、それより後は作らず、作れたworkerだけを使う (docs/spec/vm.mdの
/// 「タスクとスケジュール」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_workers<'scope, M: Mode, P: Policy + 'scope>(
    scope: &'scope Scope<'scope, '_>,
    code: &'scope CodeBase<M>,
    policy: &P,
    run: &Arc<Run>,
    control: &Arc<Control>,
    cells: &Arc<Shared>,
    host: &HostRef,
    max_depth: usize,
) -> Vec<ScopedJoinHandle<'scope, WorkerExit<M>>> {
    let mut handles = Vec::new();
    for k in 1..run.workers() {
        run.prepare(k);
        let builder = thread::Builder::new()
            .name(format!("eml-worker-{k}"))
            .stack_size(WORKER_STACK);
        let (run_k, control_k, host_k) = (Arc::clone(run), Arc::clone(control), host.clone());
        let cells_k = Arc::clone(cells);
        // worker 0 は run の方針をそのまま使い、worker 1..N-1 は worker ごとの方針を使う
        let policy_k = policy.for_worker(k);
        // worker 0 のほかの worker は SIGINT と SIGTERM を塞ぐ (docs/spec/vm.md の「上限と割り込み」)
        match eml_io::spawn_scoped_masked(scope, builder, move || {
            run_worker::<M, P>(
                code, policy_k, run_k, control_k, cells_k, host_k, k, max_depth,
            )
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
#[allow(clippy::too_many_arguments)]
fn run_worker<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    policy: P,
    run: Arc<Run>,
    control: Arc<Control>,
    cells: Arc<Shared>,
    host: HostRef,
    k: usize,
    max_depth: usize,
) -> WorkerExit<M> {
    on_own_ctx(&run, &control, host, k, format!("<worker {k}>"), |ctx| {
        work::<M, P>(code, ctx, policy, &run, &control, cells, k, max_depth)
    })
}

/// 分かれた run のメインの worker の本体。`Machine` を持つスレッドで、worker 0 を作った後に動き、自分の `Ctx` を
/// `run_scheduled` の畳み込みに返す (docs/spec/vm.md の「メインの worker」)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_main<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    policy: P,
    run: &Arc<Run>,
    control: &Arc<Control>,
    cells: Arc<Shared>,
    host: HostRef,
    max_depth: usize,
    mut park: Box<dyn Park>,
) -> WorkerExit<M> {
    let k = run.main();
    on_own_ctx(run, control, host, k, "<main worker>".into(), move |ctx| {
        let done = main_work::<M, P>(
            code, ctx, policy, run, control, cells, max_depth, &mut *park,
        );
        // `Park`の`Drop`も利用者のコードなので`catch_unwind`の中で落とし、そこでのpanicはメインのworkerのpanicと
        // 同じに扱う
        drop(park);
        done
    })
}

/// 呼んだスレッドで worker `k` の `Ctx` を作り、`catch_unwind` の中で `body` を動かして、`Ctx` を返す。`Ctx` を作る
/// ところから `catch_unwind` の中で動き、panic は関数の名前 `function` で `fatal` に書いてすべての worker に終わりを
/// 知らせる。そのときの `Ctx` は、途中の状態のヒープを落とさないように漏らす。
fn on_own_ctx<M: Mode>(
    run: &Run,
    control: &Arc<Control>,
    host: HostRef,
    k: usize,
    function: String,
    body: impl FnOnce(&mut Ctx<M>) -> Done,
) -> WorkerExit<M> {
    let mut slot: Option<ManuallyDrop<Ctx<M>>> = None;
    let mut done: Option<Done> = None;
    let mut base = HeapWork::default();
    let body = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let ctx = slot.insert(ManuallyDrop::new(Ctx::<M>::for_worker(
            Arc::clone(control),
            host,
            k,
        )));
        base = HeapWork::of(ctx.heap().stats());
        done = Some(body(ctx));
    }));
    match body {
        Ok(()) => {
            run.exit_worker(k);
            let ctx = slot.expect("a worker that returns has built its Ctx");
            let done = done.expect("a worker that returns has finished its loop");
            Ok(Retired {
                ctx: ManuallyDrop::into_inner(ctx),
                counts: done.counts,
                preemptions: done.preemptions,
                peak_stack: done.peak_stack,
                base,
                left: done.left,
                kept_pending: done.kept_pending,
            })
        }
        Err(payload) => {
            run.set_fatal(RuntimeError::Fault {
                fault: VmFault::Internal("a worker thread panicked"),
                function,
                at: None,
            });
            control.end_run();
            run.wake_all();
            run.exit_worker(k);
            Err(payload)
        }
    }
}

/// `work` が返す、worker の `Scheduler` と実行の繰り返しが数えた仕事。
struct Done {
    counts: RunCounts,
    preemptions: u64,
    peak_stack: u64,
    left: bool,
    kept_pending: bool,
}

/// worker `k` の実行。眠った形で始め、置かれた右の子と仕事を自分の `Scheduler` で動かす。抜けるのは止めるビットか `END` を
/// 見たときと、致命的な誤りのときである。止めるビットと `END` では何も書かずに、ほかの worker を起こしてから抜ける。
/// 眠っている worker に止める語を読ませ、自分の確保で立てた `MEMORY` も worker 0 に届けるためである。致命的な誤りは
/// run の `fatal` に書き、`END` をすべての語に立ててから同じく起こす (docs/spec/vm.md の「子の誤り」)。
#[allow(clippy::too_many_arguments)]
fn work<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    policy: P,
    run: &Arc<Run>,
    control: &Arc<Control>,
    cells: Arc<Shared>,
    k: usize,
    max_depth: usize,
) -> Done {
    run.park_wait(k);
    let mut sched = Scheduler::worker(
        policy,
        Arc::clone(run),
        Arc::clone(control),
        k,
        ctx.heap().worker(),
        max_depth,
        cells,
    );
    let mut counts = RunCounts::default();
    let (error, peak_stack) = crate::vm::work::<ExecWith<M, P>>(code, ctx, &mut sched, &mut counts);
    // `Finish`の後にはいつも集める地点が続き、その最初の区間が`added`を空にする (docs/spec/vm.mdの
    // 「タスクとスケジュール」)
    debug_assert!(
        sched.added.is_empty(),
        "an entry is left unsettled when a worker exits"
    );
    // 止めるビットと `END` は安全点か空きの待ちでだけ見るので、ここに来た worker は集める地点の途中で抜けておらず、
    // lock の外で行う破棄も終えている。その後に受け箱に届いた知らせを最後に1回だけ見て、走らせるはずの仕事が残って
    // いれば数える (docs/spec/vm.md の「誤り、止めること、終わり」)
    let left = quiet(&error) && sched.left_notices();
    // 予約と確定の間には安全点も切り替えも失敗しうる命令もないので、worker が抜けるときの hold は空である。例外は、
    // 予約を書いた `reserve_recv` の行が、続く起こしの処理 (`route` と `schedule`) の内部の誤りで抜けたときだけで、
    // そのときは品物の語を持ったまま `Scheduler` ごと捨て、回復がヒープを片付ける (docs/spec/vm.md の「channel」)
    debug_assert!(
        sched.hold.is_none()
            || matches!(
                error,
                RuntimeError::Fault {
                    fault: VmFault::Internal(_),
                    ..
                }
            ),
        "a channel reservation is held when a worker exits"
    );
    let quiet = quiet(&error);
    if !quiet {
        run.set_fatal(error);
        control.end_run();
    }
    run.wake_all();
    // driver の静止は取り消しの終わりを待ちうるので、誤りを書いてほかの worker を止めてから行う。`Ctx` を返す前で
    // あればよい (docs/spec/vm.md の「時計と driver」)
    let kept_pending = sched.retire_driver(quiet);
    Done {
        counts,
        preemptions: sched.preemptions(),
        peak_stack,
        left,
        kept_pending,
    }
}

/// メインのworkerの仕事。固定したタスクを動かし、走れるものがなければ待つ。worker 1..N-1と同じく`END`か止める
/// ビットで抜け、致命的な誤りはrunの`fatal`に書く。返す前に自分のdriverを自分のスレッドで静止させる
/// (docs/spec/vm.mdの「メインのworker」)。
#[allow(clippy::too_many_arguments)]
fn main_work<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    policy: P,
    run: &Arc<Run>,
    control: &Arc<Control>,
    cells: Arc<Shared>,
    max_depth: usize,
    park: &mut dyn Park,
) -> Done {
    let mut sched = Scheduler::worker(
        policy,
        Arc::clone(run),
        Arc::clone(control),
        run.main(),
        ctx.heap().worker(),
        max_depth,
        cells,
    );
    sched.main_loop = true;
    let mut counts = RunCounts::default();
    let (error, peak_stack) =
        crate::vm::main_work::<ExecWith<M, P>>(code, ctx, &mut sched, &mut counts, park);
    let quiet = error.as_ref().is_none_or(quiet);
    // 止めるビットと`END`は安全点か待ちの後でだけ読むので、ここに来たメインのworkerは集める地点の途中で抜けて
    // いない。その後に受け箱に届いた知らせを最後に1回だけ見る (docs/spec/vm.mdの「誤り、止めること、終わり」)
    let left = quiet && sched.left_notices();
    if let Some(error) = error.filter(|_| !quiet) {
        run.set_fatal(error);
        control.end_run();
    }
    run.wake_all();
    let kept_pending = sched.retire_driver(quiet);
    Done {
        counts,
        preemptions: sched.preemptions(),
        peak_stack,
        left,
        kept_pending,
    }
}

/// 成功した run の、worker を join した後の確かめ。driver が静止させた後にも保留の操作を残していれば driver の誤り、
/// どれかの worker の受け箱に走らせるはずの知らせが残っていれば内部の誤りである (docs/spec/vm.md の
/// 「誤り、止めること、終わり」)。
pub(crate) fn end_check(kept_pending: bool, left: bool) -> Result<(), VmFault> {
    if kept_pending {
        return Err(VmFault::Driver(crate::error::KEPT_PENDING.to_string()));
    }
    if left {
        return Err(VmFault::Internal("a task is left at the end"));
    }
    Ok(())
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
    /// 戻るたびに止める語を読んで集め直す (docs/spec/vm.md の「タスクとスケジュール」「時計と driver」)。
    ///
    /// 手順は4つである。(a) 欄の lock の下で眠った形にして `running` から引く。(b) ほかの worker が動いていれば、
    /// 起こされるまで待つ。最後に眠った worker が worker 0 を起こす。(c) 登録表の1回の区間で、すべての区画の
    /// いちばん早い期限、driver に出した操作の数の和、OS のシグナルを待つタスクがいるかを読む。(d) ほかの worker が
    /// どれも眠っていれば、欄の lock の下で、受け箱、起こされた印、`running`、mail、`NOTICE`、止める語を読み直す。
    /// どれかがあれば起きた形に戻して集め直す。どれもなく、期限も保留の操作も送った子もなければデッドロックである。
    /// 先に眠った形にするので、(c) の後に眠ったほかの worker は worker 0 を起こすか (d) で `running` に見え、
    /// その前に登録表に足したタイマーは (c) が読む。
    pub(crate) fn idle_wait(&mut self) -> Result<(), VmFault> {
        let run = Arc::clone(&self.run);
        let k = self.index;
        if k != 0 {
            return self.peer_wait();
        }
        if run.take_wake(k) {
            return Ok(());
        }
        run.join_idle(k);
        let slot = &run.slots[k];
        let control = Arc::clone(&self.watcher.control);
        // (a)
        let others = {
            let mut state = lock(&slot.state);
            if state.woken || !state.notices.is_empty() || stopping(&control, k) {
                state.woken = false;
                drop(state);
                run.leave_idle(k);
                return Ok(());
            }
            state.parked = true;
            run.running.fetch_sub(1, Ordering::AcqRel) - 1
        };
        // (c)
        let (d_all, d_own, elsewhere, pending, own_pending, listening) = {
            let cells = self.cells.lock();
            let mut d_all: Option<u64> = None;
            let mut elsewhere = false;
            for (owner, deadline) in cells.deadlines() {
                d_all = Some(d_all.map_or(deadline, |d| d.min(deadline)));
                elsewhere |= owner != k;
            }
            (
                d_all,
                cells.next_deadline(k),
                elsewhere,
                cells.in_flight_all(),
                cells.in_flight(k),
                cells.relay && cells.signal_bits() != 0,
            )
        };
        #[cfg(test)]
        self.cells.mail.run_hook();
        // OS のシグナルを待つタスクがいれば、ほかに進むものがなくてもシグナルが来るまで眠る。届いたときは監視の
        // スレッドが起こす
        let listen = |d: Option<u64>| if listening { d.or(Some(u64::MAX)) } else { d };
        let deadline = listen(d_all);
        let mut state = lock(&slot.state);
        // (d) ほかの worker は mail を処理して知らせを送ってから眠り、監視のスレッドは mail に積んで `NOTICE` を立ててから
        // 起こすので、その途中の run をデッドロックにしない
        if others == 0 {
            if state.woken
                || !state.notices.is_empty()
                || run.running.load(Ordering::Acquire) != 0
                || self.cells.mail.pending()
                || control.worker_bits(k) & Control::NOTICE != 0
                || stopping(&control, k)
            {
                state.woken = false;
                run.unpark_locked(&mut state);
                drop(state);
                run.leave_idle(k);
                return Ok(());
            }
            if deadline.is_none()
                && pending == 0
                && run.in_flight.load(Ordering::Acquire) == 0
                && !run.holds()
            {
                run.unpark_locked(&mut state);
                drop(state);
                run.leave_idle(k);
                return Err(VmFault::Deadlock);
            }
        }
        let virtual_ = self.driver_kind == DriverKind::Virtual;
        // 本物の時計では、ほかの worker は自分の区画の期限と自分の driver の完了で自分で起きるので、worker 0 は自分の
        // 区画の期限と自分の driver の操作だけを待つ。期限がなくても条件変数でなく driver で眠る。自分で落とした
        // ハンドルは driver が待ちの始めに閉じるので、ほかの worker の相手が EOF を待つ間に閉じずに眠らないためである。
        // 仮想の時計では、driver を持つのは worker 0 だけなので、どの操作も自分の driver にある
        let deadline = match self.driver_kind {
            DriverKind::Virtual => deadline,
            DriverKind::Real => d_own.or(Some(u64::MAX)),
            DriverKind::Injected => listen(d_own),
        };
        // 仮想の時計を進めてよいのは、どの worker も動いておらず、どの受け箱にも知らせがないときだけである。進める先は
        // すべての区画のいちばん早い期限である
        let waited = if virtual_ && others == 0 && (deadline.is_some() || own_pending > 0) {
            drop(state);
            Some(driver_mut(&mut self.driver)?.wait(deadline))
        } else if (virtual_ && others > 0) || (deadline.is_none() && own_pending == 0) {
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
                // ほかの worker の区画の満期は、その持ち主が自分の集める地点で外す。仮想の時計を進めるのは worker 0
                // だけで、ほかの worker は driver を持たずに眠っているので、期限が来た区画の持ち主を起こす
                if virtual_ && elsewhere {
                    self.wake_due_owners()?;
                }
                match waited {
                    Ok(true) => Ok(()),
                    // 送った子の終わりか知らせが残っていれば、driver が何も進まないと答えてもまだ進みうる
                    Ok(false)
                        if run.in_flight.load(Ordering::Acquire) == 0
                            && !slot.has_notices.load(Ordering::Acquire)
                            && !self.cells.mail.pending() =>
                    {
                        if run.holds() {
                            run.hold_sleep(&control, k);
                            Ok(())
                        } else {
                            Err(VmFault::Deadlock)
                        }
                    }
                    Ok(false) => Ok(()),
                    Err(e) => Err(VmFault::Driver(e.to_string())),
                }
            }
        };
        run.leave_idle(k);
        result
    }

    /// worker `k` (`k` ≥ 1) の空きの待ち。受け箱が空なら空きの集まりに入って眠り、起こされたら戻る。最後に眠った
    /// worker は worker 0 を起こす。worker 0 が、ほかの worker が動いている間は仮想の時計を進めずに待つためである。
    /// デッドロックと仮想の時計は worker 0 が受け持つ。本物の時計の run では、自分の区画に期限があれば driver を作り、
    /// driver を持てば driver で待つ。期限は最後の集める地点が読んだもので、ここでは登録表の lock を取らない。
    /// 待つ間にほかの worker の取り消しが項を外しても、早く起きて集め直すだけである (docs/spec/vm.md の
    /// 「タスクとスケジュール」「時計と driver」)。
    fn peer_wait(&mut self) -> Result<(), VmFault> {
        let k = self.index;
        let deadline = self.own_deadline.filter(|_| self.run.io_worker(k) == k);
        if deadline.is_some() {
            // wakerを置いてから眠った形にするので、作った直後の起こしもdriverの待ちを終わらせる
            self.own_driver()?;
        }
        if !self.run.park_idle(&self.watcher.control, k) {
            return Ok(());
        }
        // driver を持てば、期限がなくても driver で眠る。自分で落としたハンドルは driver が待ちの始めに閉じるので、
        // ほかの worker の相手が EOF を待つ間に閉じずに眠らないためである
        let Some(driver) = self.driver.as_deref_mut() else {
            self.run.park_wait(k);
            return Ok(());
        };
        // 眠った形にした後に起こされても、waker は鳴った印を残すので、driver の待ちはすぐ戻る
        let waited = driver.wait(deadline.or(Some(u64::MAX)));
        self.run.driver_waited(k);
        match waited {
            Ok(_) => Ok(()),
            Err(e) => Err(VmFault::Driver(e.to_string())),
        }
    }

    /// メインのworkerの待ちの前半。空きの集まりに入らずにworker 1..N-1の手順で眠った形にし、眠ったかを返す。本物の
    /// 時計で自分の区画に期限があれば、先にdriverを作る (docs/spec/vm.mdの「メインの worker」)。
    pub(crate) fn main_park(&mut self) -> Result<bool, VmFault> {
        let k = self.index;
        if self.own_deadline.is_some() && self.run.io_worker(k) == k {
            // wakerを置いてから眠った形にするので、作った直後の起こしもdriverの待ちを終わらせる
            self.own_driver()?;
        }
        Ok(self.run.park_main(&self.watcher.control, k))
    }

    /// メインのworkerの`Park`が待ってよい長さ。本物の時計でdriverを持てば自分の区画のいちばん早い期限まで、ほかでは
    /// 起こされるまで (`None`) である。仮想の時計では、期限の来たタイマーのためにworker 0がメインのworkerを起こす。
    pub(crate) fn main_timeout(&self) -> Option<Duration> {
        let k = self.index;
        let deadline = self.own_deadline.filter(|_| self.run.io_worker(k) == k)?;
        let driver = self.driver.as_deref()?;
        Some(Duration::from_millis(deadline).saturating_sub(driver.now()))
    }

    /// メインのworkerの既定の待ち (`Pending::default_wait`)。driverを持てば期限までdriverで待ち、戻ったら起きた形に
    /// なる。完了と期限の来たタイマーは次の集める地点が拾うためである。driverを持たなければ、起こされるまで欄の
    /// 条件変数で眠る (docs/spec/vm.mdの「メインの worker の待ち」)。
    pub(crate) fn main_default_wait(&mut self) -> std::io::Result<()> {
        let k = self.index;
        let deadline = self.own_deadline.filter(|_| self.run.io_worker(k) == k);
        let Some(driver) = self.driver.as_deref_mut() else {
            self.run.sleep(k);
            return Ok(());
        };
        let waited = driver.wait(deadline.or(Some(u64::MAX)));
        self.run.woke(k);
        waited.map(|_| ())
    }

    /// worker `k` (`k` ≥ 1) が抜ける前に、自分の driver を静止させ、`record` なら driver に出したままの記録に、driver が
    /// 返した完了を置く。記録に置くだけで、ヒープには触れない。driver は自分のスレッドで落とし、静止させた後にも保留の
    /// 操作が残っていたかを返す。`Ctx` を返す前に行うので、カーネルとプールのスレッドがヒープに書く操作は残らない
    /// (docs/spec/vm.md の「誤り、止めること、終わり」「時計と driver」)。
    fn retire_driver(&mut self, record: bool) -> bool {
        let Some(mut driver) = self.driver.take() else {
            return false;
        };
        driver.quiesce(self.run.job_wait(driver.now()));
        let k = self.index;
        if record && self.cells.lock().in_flight(k) > 0 {
            let mut completed = Vec::new();
            driver.completions(&mut completed);
            self.cells.lock().quiesced(k, &mut completed);
        }
        driver.pending() > 0
    }

    /// worker `k` (`k` ≥ 1) が抜ける前に、受け箱に残った知らせを取り出し、このworkerが走らせるはずの仕事があるかを
    /// 返す。実行待ちの列に残った`Entry`、右の子と仕事 (`Child`)、`Place`、
    /// 継続を取り出せる`Wake`を数え、`Joined`、`Cancel`、`Submit`、`CancelIo`、`Completion`、`Limit`と、
    /// もう待っていないセルへの起こしは数えない。登録表は書き換えず、列の項と知らせの語は手放さない。
    /// 数えたらrunは誤りで終わり、回復がヒープを捨てるからである (docs/spec/vm.mdの「誤り、止めること、終わり」)。
    pub(crate) fn left_notices(&mut self) -> bool {
        let mut left = false;
        self.run.retain_ready(self.index, |item| {
            left |= matches!(item, Ready::Entry(_));
            true
        });
        let notices = self.run.take_notices(self.index);
        if notices.is_empty() {
            return left;
        }
        let me = self.me();
        let mut cells = self.cells.lock();
        for notice in &notices {
            left |= match notice {
                Notice::Child(_) | Notice::Place { .. } => true,
                Notice::Wake(w) => cells.would_run(w, me),
                Notice::Joined { .. }
                | Notice::Cancel(_)
                | Notice::Submit(_)
                | Notice::CancelIo(_)
                | Notice::Completion(_)
                | Notice::Limit(_) => false,
            };
        }
        left
    }

    /// 期限が今までに来た区画の持ち主 (worker 0 を除く) を起こす。
    fn wake_due_owners(&self) -> Result<(), VmFault> {
        let now = self.run.now_ms_floor(self.driver.as_deref())?;
        let due: Vec<usize> = self
            .cells
            .lock()
            .deadlines()
            .filter(|&(owner, deadline)| owner != self.index && deadline <= now)
            .map(|(owner, _)| owner)
            .collect();
        for owner in due {
            self.run.wake(owner);
        }
        Ok(())
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
    use std::time::Duration;

    use eml_rt::{Checked, Control, HeapError, Limits, Stop};

    use super::{Notice, Run, Seat, settle};
    use crate::cells::{CellRef, MailItem, Registry, Shared};
    use crate::driver::RunClock;
    use crate::sched::{Scheduler, Sequential};
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
        let run = Run::new(
            2,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
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
        let entry = machine
            .load(&program, &crate::Linked::default())
            .unwrap()
            .entry;
        let finished = machine.run(entry).unwrap();
        assert_eq!(finished.placed, 1);
        assert!(
            machine.ctx.heap().stats().remote_frees >= 1,
            "{:?}",
            machine.ctx.heap().stats()
        );
        machine.close().unwrap();
    }

    /// `END` の後、worker 1 が最後に受け箱を見る前に届いた `Place` は、走らせるはずの仕事として数える。数えた run は
    /// 成功しても `a task is left at the end` で終わる。結果を当てる知らせ (`Completion`) は数えない
    /// (docs/spec/vm.md の「誤り、止めること、終わり」)。スレッドは作らず、worker 1 の抜ける手順を手で動かす。
    #[test]
    fn a_notice_after_the_last_collect_is_counted_as_left() {
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            ..Limits::default()
        }));
        let run = Arc::new(Run::new(
            2,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        ));
        let shared = Arc::new(Shared::new(Registry::new(Vec::new(), 2)));
        let mut sched = Scheduler::worker(
            Sequential,
            Arc::clone(&run),
            Arc::clone(&control),
            1,
            0,
            usize::MAX,
            Arc::clone(&shared),
        );
        let placed = crate::cells::placed_task(&shared, 1);
        control.end_run();
        // 結果を当てる知らせだけなら、残った仕事はない
        run.send(&control, 1, Notice::Completion(placed));
        assert!(!sched.left_notices());
        run.send(&control, 1, Notice::Completion(placed));
        run.send(
            &control,
            1,
            Notice::Place {
                cell: placed,
                worker: 0,
                closure: eml_rt::Word::UNIT,
            },
        );
        assert!(sched.left_notices());
        assert!(run.take_notices(1).is_empty());
        assert_eq!(
            super::end_check(false, true),
            Err(VmFault::Internal("a task is left at the end"))
        );
        assert_eq!(super::end_check(false, false), Ok(()));
    }

    /// workerが3つなら、`start`はworker 1と2に順に置き、`main`を動かすworker 0には置かない。worker 0だけなら持ち主は
    /// worker 0である (docs/spec/vm.mdの「`Async`のタスク」)。
    #[test]
    fn the_ring_never_places_on_worker_0() {
        let run = Run::new(
            3,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
        run.set_live(3);
        let owners: Vec<u32> = (0..4).map(|_| run.next_owner()).collect();
        assert_eq!(owners, [1, 2, 1, 2]);
        let run = Run::new(
            3,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
        run.set_live(1);
        assert_eq!(run.next_owner(), 0);
    }

    /// 仕事を待つ期限は run の最初の静止が決め、後の静止はその残りだけを受け取る。過ぎた後は 0 である
    /// (docs/spec/vm.md の「時計と driver」)。
    #[test]
    fn job_wait_shares_one_deadline() {
        let mut run = Run::new(
            2,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
        run.job_budget = Duration::from_secs(1);
        let ms = Duration::from_millis;
        assert_eq!(run.job_wait(ms(10)), ms(1000));
        assert_eq!(run.job_wait(ms(400)), ms(610));
        assert_eq!(run.job_wait(ms(2000)), Duration::ZERO);
    }

    /// 新しいハンドルを作る操作の host は、本物の時計の run では受け付けた worker、仮想の時計の run では worker 0 である。
    /// 本物の時計の run だけが worker の driver を基準から作る (docs/spec/vm.md の「時計と driver」)。
    #[test]
    fn only_the_real_clock_gives_each_worker_its_own_driver() {
        let real = Run::new(
            3,
            Seat::Merged,
            None,
            RunClock::Real {
                epoch: std::time::Instant::now(),
            },
            Arc::default(),
        );
        assert_eq!(
            (0..3).map(|k| real.io_worker(k)).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert!(matches!(real.new_driver(), Some(Ok(_))));
        let virtual_ = Run::new(
            3,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
        assert_eq!(
            (0..3).map(|k| virtual_.io_worker(k)).collect::<Vec<_>>(),
            [0, 0, 0]
        );
        assert!(virtual_.new_driver().is_none());
    }

    fn separate(workers: usize, clock: RunClock) -> Run {
        Run::new(workers, Seat::Separate, None, clock, Arc::default())
    }

    /// 分かれた run は、`main` とタスクを動かす worker の後にメインの worker の欄を1つ足す。その欄は眠った形で始まるので
    /// `running` に数えず、起こすと数える (docs/spec/vm.md の「メインの worker」)。
    #[test]
    fn a_separate_run_adds_a_parked_main_slot() {
        let run = separate(3, RunClock::Virtual(Arc::default()));
        assert_eq!(
            (run.slots.len(), run.workers(), run.main(), run.separate()),
            (4, 3, 3, true)
        );
        assert_eq!(run.running.load(Ordering::Acquire), 1);
        run.wake(3);
        assert_eq!(run.running.load(Ordering::Acquire), 2);
        let merged = Run::new(
            3,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
        assert_eq!(
            (merged.slots.len(), merged.main(), merged.separate()),
            (3, 0, false)
        );
    }

    /// `start` の持ち主の輪はメインの worker を選ばない (docs/spec/vm.md の「メインの worker」)。
    #[test]
    fn placement_never_picks_the_main_worker() {
        let run = separate(3, RunClock::Virtual(Arc::default()));
        run.set_live(3);
        let owners: Vec<u32> = (0..6).map(|_| run.next_owner()).collect();
        assert!(owners.iter().all(|&k| k == 1 || k == 2), "{owners:?}");
        let run = separate(1, RunClock::Virtual(Arc::default()));
        assert_eq!(run.next_owner(), 0);
    }

    /// 本物の時計では、メインの worker も自分の driver の host になる。仮想の時計では worker 0 が host である
    /// (docs/spec/vm.md の「時計と driver」)。
    #[test]
    fn io_worker_gives_the_main_worker_its_own_driver_on_the_real_clock() {
        let real = || RunClock::Real {
            epoch: std::time::Instant::now(),
        };
        let hosts = |run: &Run| {
            (0..run.slots.len())
                .map(|k| run.io_worker(k))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            hosts(&separate(3, RunClock::Virtual(Arc::default()))),
            [0, 0, 0, 0]
        );
        let apart = separate(3, real());
        assert_eq!(hosts(&apart), [0, 1, 2, 3]);
        assert!(apart.hosts_apart());
        let merged = Run::new(3, Seat::Merged, None, real(), Arc::default());
        assert_eq!(hosts(&merged), [0, 1, 2]);
    }

    /// メインの worker は眠っても空きの集まりに入らないので、`pop_idle` が選ぶのはほかの worker だけである。
    #[test]
    fn the_main_worker_never_joins_idle() {
        let control = Control::new(Limits {
            workers: 2,
            ..Limits::default()
        });
        let run = separate(2, RunClock::Virtual(Arc::default()));
        run.prepare(1);
        run.wake(2);
        assert!(run.take_wake(2));
        assert!(run.park_main(&control, 2));
        assert_eq!(run.pop_idle(), Some(1));
        assert_eq!(run.pop_idle(), None);
    }

    /// 最後に眠ったのがメインの worker なら、眠っていた worker 0 を起こす (docs/spec/vm.md の「メインの worker」)。
    #[test]
    fn the_main_worker_parking_last_wakes_worker_0() {
        let control = Control::new(Limits::default());
        let run = separate(1, RunClock::Virtual(Arc::default()));
        run.wake(1);
        assert!(run.take_wake(1));
        run.park(0);
        assert_eq!(run.running.load(Ordering::Acquire), 1);
        assert!(run.park_main(&control, 1));
        // worker 0 は起こされて動いている形に戻り、メインの worker は眠っている
        assert!(run.take_wake(0));
        assert_eq!(run.running.load(Ordering::Acquire), 1);
    }

    /// 眠っているメインの worker を2回起こしても、`running` には1回だけ数える。
    #[test]
    fn a_wake_of_the_parked_main_worker_counts_it_once() {
        let run = separate(2, RunClock::Virtual(Arc::default()));
        run.wake(2);
        run.wake(2);
        assert_eq!(run.running.load(Ordering::Acquire), 2);
        assert!(run.take_wake(2));
        assert!(!run.take_wake(2));
    }

    /// 眠る前に届いた起こしは、メインの worker を眠らせずに一度だけ素通りさせる。数は変わらない。
    #[test]
    fn a_wake_before_the_main_worker_parks_keeps_it_awake() {
        let control = Control::new(Limits {
            workers: 2,
            ..Limits::default()
        });
        let run = separate(2, RunClock::Virtual(Arc::default()));
        run.wake(2);
        assert!(run.take_wake(2));
        run.wake(2);
        assert!(!run.park_main(&control, 2));
        assert_eq!(run.running.load(Ordering::Acquire), 2);
        assert!(run.park_main(&control, 2));
        assert_eq!(run.running.load(Ordering::Acquire), 1);
    }

    /// 眠っているメインのworkerに知らせを送ると、`Park`のwakerを1回鳴らす。動いている間に届いた知らせは鳴らさない。
    /// メインのworkerは眠る前に欄のlockの下で受け箱を見るためである (docs/spec/vm.mdの「メインの worker の待ち」)。
    #[test]
    fn a_notice_to_the_parked_main_worker_rings_its_park_waker() {
        struct Count(std::sync::atomic::AtomicUsize);
        impl eml_rt::IoWaker for Count {
            fn wake(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let control = Control::new(Limits {
            workers: 2,
            ..Limits::default()
        });
        let run = separate(2, RunClock::Virtual(Arc::default()));
        let rings = Arc::new(Count(Default::default()));
        assert!(run.slots[2].park_waker.set(rings.clone()).is_ok());
        run.send(&control, 2, Notice::Limit(CellRef::decode(0)));
        assert_eq!(rings.0.load(Ordering::SeqCst), 1);
        run.send(&control, 2, Notice::Limit(CellRef::decode(0)));
        assert_eq!(rings.0.load(Ordering::SeqCst), 1);
        assert_eq!(run.take_notices(2).len(), 2);
        assert!(run.park_main(&control, 2));
        run.wake(2);
        assert_eq!(rings.0.load(Ordering::SeqCst), 2);
    }

    /// 固定したタスクの`start`はメインのworkerに置く。分かれたrunではメインのworkerの受け箱に`Place`を送り、合わせたrun
    /// ではworker 0がメインのworkerを兼ねるので、呼んだworker 0が自分で始める。固定しないタスクは輪に置き、固定した
    /// タスクの数に入れない (docs/spec/vm.mdの「`Async`のタスク」)。
    #[test]
    fn a_pinned_start_is_placed_on_the_main_worker() {
        for seat in [Seat::Merged, Seat::Separate] {
            let control = Control::new(Limits {
                workers: 3,
                ..Limits::default()
            });
            let run = Run::new(
                3,
                seat,
                None,
                RunClock::Virtual(Arc::default()),
                Arc::default(),
            );
            run.set_live(3);
            let shared = Shared::new(Registry::new(Vec::new(), run.slots.len()));
            let mut ctx = crate::cells::test_ctx();
            let post = super::Mailer::new(&run, &control);
            let here = crate::cells::Here {
                me: 0,
                host: 0,
                mail: &shared.mail,
                post: &post,
            };
            let scope = shared.lock().open_root(None, None).unwrap();
            let mut start = |owner: u32, pinned: bool| {
                shared
                    .lock()
                    .start(
                        &mut ctx,
                        &here,
                        scope,
                        0,
                        owner,
                        eml_rt::Word::UNIT,
                        &[],
                        pinned,
                    )
                    .unwrap()
                    .unwrap()
            };
            let main = run.main() as u32;
            let owner = run.place_owner(true, 0, true);
            assert_eq!(owner, main, "{seat:?}");
            let (pinned, started) = start(owner, true);
            assert_eq!(started.is_some(), seat == Seat::Merged, "{seat:?}");
            let other = run.place_owner(true, 0, false);
            assert!(other == 1 || other == 2, "{seat:?}: {other}");
            let (unpinned, _) = start(other, false);
            if seat == Seat::Separate {
                assert!(
                    matches!(
                        run.take_notices(3)[..],
                        [Notice::Place { cell, .. }] if cell == pinned
                    ),
                    "{seat:?}"
                );
            }
            let mut cells = shared.lock();
            assert!(cells.pinned(pinned).unwrap(), "{seat:?}");
            assert!(!cells.pinned(unpinned).unwrap(), "{seat:?}");
            assert_eq!(cells.pinned_tasks, 1, "{seat:?}");
        }
    }

    /// まだ始めていない worker の欄を起こしても、`running` には数えない。
    #[test]
    fn waking_an_unstarted_worker_counts_nothing() {
        let run = Run::new(
            3,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
        run.wake_all();
        assert_eq!(run.running.load(Ordering::Acquire), 1);
    }

    /// 期限も保留の操作もなく、知らせも mail もない worker 0 の空きの待ちはデッドロックを決める。登録表を読んだ後、欄を
    /// 読み直す前に mail に項が積まれれば、決めずに戻って集め直す (docs/spec/vm.md の「タスクとスケジュール」)。
    #[test]
    fn a_mail_item_pending_at_the_deadlock_check_defers_the_verdict() {
        let idle = |hook: bool| {
            let control = Arc::new(Control::new(Limits::default()));
            let driver = eml_io::TestDriver::new(None);
            let run = Run::new(
                1,
                Seat::Merged,
                None,
                RunClock::Virtual(driver.clock()),
                Arc::default(),
            );
            let mut sched = Scheduler::new(
                Sequential,
                Box::new(driver),
                Arc::new(run),
                control,
                0,
                usize::MAX,
                Arc::new(Shared::new(Registry::new(Vec::new(), 1))),
            );
            if hook {
                sched.cells.mail.set_hook(|mail| {
                    mail.push(MailItem::Cancel {
                        cell: CellRef::decode(0),
                        owner: 0,
                    });
                });
            }
            let verdict = sched.idle_wait();
            // 戻るときは眠っていない
            assert_eq!(sched.run.running.load(Ordering::Acquire), 1);
            (verdict, sched.cells.mail.pending())
        };
        assert_eq!(idle(false), (Err(VmFault::Deadlock), false));
        assert_eq!(idle(true), (Ok(()), true));
    }

    /// worker 0 が登録表を読んだ後、読み直す前に、動いていた worker 1 が自分の区画にタイマーを足して眠っても、
    /// デッドロックにしない。worker 0 は先に眠った形になっているので、最後に眠った worker 1 が worker 0 を起こす。
    /// 次の空きの待ちはすべての区画のいちばん早い期限まで仮想の時計を進め、期限の来た区画の持ち主を起こす
    /// (docs/spec/vm.md の「時計と driver」)。スレッドは作らず、worker 1 の手順をテストの差し込みで行う。
    #[test]
    fn a_timer_added_by_worker_1_before_it_parks_is_not_a_deadlock() {
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            ..Limits::default()
        }));
        let driver = eml_io::TestDriver::new(None);
        let clock = driver.clock();
        let run = Arc::new(Run::new(
            2,
            Seat::Merged,
            None,
            RunClock::Virtual(Arc::clone(&clock)),
            Arc::default(),
        ));
        let shared = Arc::new(Shared::new(Registry::new(Vec::new(), 2)));
        let mut sched = Scheduler::new(
            Sequential,
            Box::new(driver),
            Arc::clone(&run),
            Arc::clone(&control),
            0,
            usize::MAX,
            Arc::clone(&shared),
        );
        // worker 1 は始めてあり、動いている
        run.prepare(1);
        run.wake(1);
        // worker 1 は起こされた印を下ろして動き始めた
        assert!(run.take_wake(1));
        assert_eq!(run.running.load(Ordering::Acquire), 2);
        {
            let (shared, run, control) =
                (Arc::clone(&shared), Arc::clone(&run), Arc::clone(&control));
            shared.clone().mail.set_hook(move |_| {
                crate::cells::sleeping_task(&shared, 1, 5);
                assert!(run.park_idle(&control, 1));
            });
        }
        // worker 1 が眠って worker 0 を起こしたので、待たずに戻る
        assert_eq!(sched.idle_wait(), Ok(()));
        assert!(run.take_wake(0));
        // 次の空きの待ちは、worker 1 の区画の期限まで時計を進め、worker 1 を起こす
        assert_eq!(sched.idle_wait(), Ok(()));
        assert_eq!(clock.load(Ordering::SeqCst), 5);
        assert!(run.take_wake(1));
        assert_eq!(run.running.load(Ordering::Acquire), 2);
    }
}

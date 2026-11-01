//! タスクのスケジュール (docs/spec/vm.md の「タスクとスケジュール」)。方針を `Scheduler` の型の引数にするので、
//! 逐次の方針の安全点は、方針を持たない繰り返しと同じ機械語になる。

use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use eml_rt::{Completion, Control, Ctx, IoDriver, Mode, Stop};

use crate::VmFault;
use crate::cells::{self, CellRef, Here, IoWork, Runnable, Shared};
use crate::driver::DriverKind;
use crate::monitor::Monitor;
use crate::vm::Task;
use crate::workers::{Mailer, Notice, Run, Workers};

/// 待つタスクのいなかったシグナルで run を止める。待つタスクのないときにハンドラが立てるのと同じビットを、すべての
/// worker の止める語に立てるので、`eml run` の次の同じシグナルはその場で終わらせ、どの worker も次の安全点で止まる
/// (docs/spec/vm.md の「上限と割り込み」)。
#[cold]
#[inline(never)]
fn unheard_fault(control: &Control, bits: u32) -> VmFault {
    if bits & Control::TERMINATE != 0 {
        control.terminate();
    }
    if bits & Control::INTERRUPT != 0 {
        control.interrupt();
        return VmFault::Stopped(Stop::Interrupted);
    }
    VmFault::Stopped(Stop::Terminated)
}

/// スケジュールの方針の選び方。CLI の `--sched` と `RunConfig::with_sched` で選ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sched {
    /// 監視のスレッドの `PREEMPT` で横取りする。
    Normal,
    /// 横取りしない。
    #[default]
    Sequential,
    /// seed で決まる擬似乱数で、横取りする安全点と次のタスクを選ぶ。
    Seeded(u64),
}

impl Sched {
    /// テストの driver に渡す seed。seed の方針のときだけある (docs/spec/vm.md の「時計と driver」)。
    pub fn seed(self) -> Option<u64> {
        match self {
            Sched::Seeded(seed) => Some(seed),
            Sched::Normal | Sched::Sequential => None,
        }
    }
}

impl FromStr for Sched {
    type Err = String;

    /// `normal`、`sequential`、`seeded:<n>` のどれかを読む。`n` は `u64` の10進数である。
    fn from_str(text: &str) -> Result<Sched, String> {
        match text {
            "normal" => return Ok(Sched::Normal),
            "sequential" => return Ok(Sched::Sequential),
            _ => {}
        }
        // `u64::from_str` は先頭の `+` も読むので、数字だけの綴りに限る
        text.strip_prefix("seeded:")
            .filter(|seed| seed.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|seed| seed.parse().ok())
            .map(Sched::Seeded)
            .ok_or_else(|| {
                format!("expected `normal`, `sequential` or `seeded:<n>`, found `{text}`")
            })
    }
}

/// スケジューラの中のタスクの番号。
pub type TaskId = usize;

/// スケジュールの方針。いつ横取りし、次にどのタスクを動かし、起こしたタスクを列のどこに入れるかを決める。
pub trait Policy: Send {
    /// 監視のスレッドの `PREEMPT` で横取りするか。真なら、`Scheduler` は最初のタスクで監視のスレッドを始め、切り替えを
    /// 監視のスレッドに数えさせる。
    const PREEMPTS: bool;
    /// 安全点ごとに1回呼ぶ。真を返したら、横取りを試みる。
    fn tick(&mut self) -> bool;
    /// 横取りを試みた後に、次の横取りまでの予算を詰め直す。
    fn reload(&mut self);
    /// 走れるタスクの列から、次に動かすタスクを取り出す。
    fn pick(&mut self, ready: &mut VecDeque<TaskId>) -> Option<TaskId>;
    /// 起こしたタスクを、走れるタスクの列に入れる。
    fn wake(&mut self, ready: &mut VecDeque<TaskId>, task: TaskId);
    /// worker `k` の `Scheduler` が使う方針。seed を持つ方針は worker ごとに別の擬似乱数の列にする。同じ seed の run は
    /// どの worker でも同じ列を引かないためである。
    fn for_worker(&self, k: usize) -> Self
    where
        Self: Sized;
}

/// 監視のスレッドの `PREEMPT` だけで横取りする方針。列は FIFO である。
#[derive(Debug, Clone, Copy, Default)]
pub struct Normal;

impl Policy for Normal {
    const PREEMPTS: bool = true;

    // 横取りは監視のスレッドの `PREEMPT` が求めるので、安全点に分岐を残さない
    #[inline(always)]
    fn tick(&mut self) -> bool {
        false
    }

    fn reload(&mut self) {}

    fn pick(&mut self, ready: &mut VecDeque<TaskId>) -> Option<TaskId> {
        ready.pop_front()
    }

    fn wake(&mut self, ready: &mut VecDeque<TaskId>, task: TaskId) {
        ready.push_back(task);
    }

    fn for_worker(&self, _k: usize) -> Normal {
        *self
    }
}

/// 横取りしない方針。列の先頭から取り、起こしたタスクも先頭に入れる。
#[derive(Debug, Clone, Copy, Default)]
pub struct Sequential;

impl Policy for Sequential {
    const PREEMPTS: bool = false;

    // 定数の偽に畳まれ、安全点に分岐を残さない
    #[inline(always)]
    fn tick(&mut self) -> bool {
        false
    }

    fn reload(&mut self) {}

    fn pick(&mut self, ready: &mut VecDeque<TaskId>) -> Option<TaskId> {
        ready.pop_front()
    }

    fn wake(&mut self, ready: &mut VecDeque<TaskId>, task: TaskId) {
        ready.push_front(task);
    }

    fn for_worker(&self, _k: usize) -> Sequential {
        *self
    }
}

/// seed で決まる方針。擬似乱数は splitmix64 で、予算は 1 から 16 までの安全点である。
#[derive(Debug, Clone)]
pub struct Seeded {
    /// 作ったときの seed。worker ごとの方針を作るときに使う。
    seed: u64,
    state: u64,
    /// 次の横取りまでに残る安全点の数。0 のままなら、次の安全点でまた試みる。
    budget: u64,
}

impl Seeded {
    pub fn new(seed: u64) -> Seeded {
        let mut seeded = Seeded {
            seed,
            state: seed,
            budget: 0,
        };
        seeded.reload();
        seeded
    }

    fn next(&mut self) -> u64 {
        let z = splitmix64(self.state);
        self.state = self.state.wrapping_add(SPLITMIX_GAMMA);
        z
    }
}

const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// 状態 `state` の splitmix64 の次の値。
fn splitmix64(state: u64) -> u64 {
    let mut z = state.wrapping_add(SPLITMIX_GAMMA);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Policy for Seeded {
    const PREEMPTS: bool = false;

    // 予算が 0 になっても yield の途中なら切り替えないので、詰め直すのは横取りを試みたときだけにする
    fn tick(&mut self) -> bool {
        self.budget = self.budget.saturating_sub(1);
        self.budget == 0
    }

    fn reload(&mut self) {
        self.budget = 1 + self.next() % 16;
    }

    fn pick(&mut self, ready: &mut VecDeque<TaskId>) -> Option<TaskId> {
        if ready.is_empty() {
            return None;
        }
        let index = (self.next() % ready.len() as u64) as usize;
        ready.remove(index)
    }

    fn wake(&mut self, ready: &mut VecDeque<TaskId>, task: TaskId) {
        ready.push_back(task);
    }

    fn for_worker(&self, k: usize) -> Seeded {
        Seeded::new(splitmix64(self.seed ^ k as u64))
    }
}

/// join の節点の番号。境目のフレームの slot 1 に生の語で置く。
pub type JoinId = usize;

/// エントリのタスクの番号。worker 1..N-1 では、動くタスクのない間の `Task` を退避する欄で、どのタスクにも振らない。
pub(crate) const MAIN: TaskId = 0;

/// ほかの worker に置いた右の子を表す、join の節点の `right`。置いた子はこの worker の表にない。
pub(crate) const PLACED: TaskId = usize::MAX;

/// 監視のスレッドのない run で、空きのループが1回の `wait` に渡す期限の上限 (ミリ秒)。Ctrl-C と確保量の上限が眠る
/// worker を止めるまでの遅れの上限になる (docs/spec/vm.md の「時計と driver」)。
const WAIT_CAP_MS: u64 = 10;

/// driver を持たない worker が driver に触れようとした。
pub(crate) const NO_DRIVER: VmFault = VmFault::Internal("a worker without a driver");

/// worker の driver。driver を持つのは worker 0 だけなので、ほかの worker では内部の誤りにする
/// (docs/spec/vm.md の「時計と driver」)。`Scheduler` のほかの欄と同時に借りられるように、欄を受け取る関数にしてある。
pub(crate) fn driver_mut(
    driver: &mut Option<Box<dyn IoDriver>>,
) -> Result<&mut dyn IoDriver, VmFault> {
    match driver {
        Some(driver) => Ok(&mut **driver),
        None => Err(NO_DRIVER),
    }
}

/// 切り替えが続くとき、`switch_away` の頭の集める地点がこの回数に1回だけ OS を見る。driver の `completions` は
/// OS を見ないので、見る頻度は集める地点が持つ (docs/spec/vm.md の「時計と driver」)。
pub(crate) const POLL_EVERY: u32 = 16;

/// タスクの状態。動いているタスクは VM の繰り返しが持つ。
pub(crate) enum TaskState {
    Running,
    /// 走れるタスクの列にいる。
    Ready,
    /// 左の子を終えて、右の子を待つ親。起こされたら join を完了する。
    Waiting(JoinId),
    /// まだ始まっていない右の子。語は `()` を適用する閉包で、タスクがその参照を持つ。
    Unstarted(eml_rt::Word),
    /// まだ動いていない `Async` のエントリの `Task`。動き始めるときに、`args` の先頭 `argc` 個をエントリ `function` の
    /// フレームの slot 0.. に書く。語はタスクが参照を持つ (docs/spec/vm.md の「`Async` のタスク」)。
    Entry {
        function: u32,
        args: [eml_rt::Word; 3],
        argc: u8,
    },
    /// `Async.run` で、根のスコープが閉じるのをスタックを持ったまま待つ。起こされたら `Async` の命令の次から続ける。
    WaitingCell(CellRef),
}

/// 子の終わり方 (docs/spec/vm.md の「`par` の fork と join」)。語はどれも、節点が持つ数えた参照である。
pub(crate) enum Outcome {
    Value(eml_rt::Word),
    /// `never` の操作で中断した。`effect` は実行時のエフェクトの番号、`depth` は子の中でのそのエフェクトの evidence
    /// の順位で、`clause` (操作の引数を捕まえた `reperform$E.op` の閉包) に `()` を適用して親で起こし直す。
    Abort {
        effect: u16,
        depth: u32,
        clause: eml_rt::Word,
    },
    /// 右の子の実行時エラー。左の子の誤りは記録せず、すぐに評価を止める。
    Error(crate::RuntimeError),
}

/// 1つの `par` の fork から join までの記録。ヒープの物体にせず、`Drop` もヒープに触れない。
pub(crate) struct JoinNode {
    pub(crate) parent: TaskId,
    pub(crate) right: TaskId,
    pub(crate) right_worker: u32,
    /// fork のときの親の `evv`。左の子が終わったら `Ctx` に戻す。
    pub(crate) parent_evv: eml_rt::Word,
    pub(crate) left: Option<Outcome>,
    pub(crate) right_outcome: Option<Outcome>,
    /// `par` の命令の次の位置と、組を書くスロット。
    pub(crate) ret: crate::op::Pc,
    pub(crate) dst: crate::op::Slot,
    /// 親が誤りで止まった後も、置いた右の子の終わりの知らせを待つ。知らせが届いたら結果に触れずに節点を捨てる
    /// (docs/spec/vm.md の「子の誤り」)。
    pub(crate) orphan: bool,
}

/// 1回の実行のタスクを方針 `P` で切り替える。`Machine` が実行ごとに作り、実行が終わると捨てる。`Scheduler`、`Task`、
/// `JoinNode` の持つ語は数えた参照で、VM のコードが1回入れて1回出す。誤りで止まった実行では、触れずに捨てる
/// (docs/spec/vm.md の「タスクとスケジュール」)。
pub struct Scheduler<P: Policy> {
    pub(crate) policy: P,
    /// タスクの表。動いているタスクの欄は空で、そのタスクは VM の繰り返しが持つ。
    pub(crate) tasks: Vec<Option<Task>>,
    free_tasks: Vec<TaskId>,
    /// 動いているタスクの番号。
    pub(crate) current: TaskId,
    /// 走れるタスク。
    pub(crate) ready: VecDeque<TaskId>,
    pub(crate) joins: Vec<Option<JoinNode>>,
    free_joins: Vec<JoinId>,
    /// 方針が安全点で始めた切り替えの数。
    preemptions: u64,
    /// 動いていないタスクと終わったタスクが使った語の数の最大。
    peak_depth: usize,
    /// 実行を始めたときのヒープの worker。誤りで止まるときに戻す。
    pub(crate) main_worker: u32,
    /// 深さの上限。右の子のスタックの上限を、fork のときの親の深さから作る。
    pub(crate) max_depth: usize,
    /// join が見つけた右の子の誤り。`step` は `VmFault` しか返せないので、ここに置いて実行の繰り返しに渡す。
    pub(crate) failed: Option<crate::RuntimeError>,
    /// 根の表のうち、この番号より前の根はどれも印が付いている。`Machine` が実行をまたいで持つ値を受け取って返す。
    pub(crate) shared_roots: usize,
    /// `Async` のタスクとスコープのセル。run に1つで、run のすべての `Scheduler` が同じものを持つ
    /// (docs/spec/vm.md の「セルと登録表」)。
    pub(crate) cells: Arc<Shared>,
    /// 集める地点が driver から完了を受ける列。集めるたびに作らないように使い回す。
    pub(crate) completed: Vec<Completion>,
    /// 終わったエントリの `Task` のスタックと scratch。次のエントリの `Task` が使い回す。待ちから戻るたびにエントリの
    /// `Task` を作るので、毎回の確保を避けるためである。
    pub(crate) stacks: Vec<(Vec<eml_rt::Word>, Vec<eml_rt::Word>)>,
    /// 時計と待ち。worker 0 だけが持ち、`Machine` が持つものを run の間だけ預かる (docs/spec/vm.md の「時計と driver」)。
    /// 触れるときは `driver_mut` を通す。
    pub(crate) driver: Option<Box<dyn IoDriver>>,
    /// この run の worker が共有する表。
    pub(crate) run: Arc<Run>,
    /// この `Scheduler` の worker の番号。止める語と `run` の欄の添字である。
    pub(crate) index: usize,
    /// worker 1..N-1 を作るか、作ったか。worker 0 は run の始めに決め、worker 1..N-1 はいつも `Peer` である。
    /// 右の子をほかの worker に置くのは `Started` と `Peer` の `Scheduler` だけである。
    pub(crate) workers: Workers,
    /// driver の出どころ。`Real` の run だけが OS のシグナルを待つタスクへ届ける (docs/spec/stdlib.md の
    /// 「`Std.Signal`」)。空きの待ちに上限を掛けるかもこれで決まる (docs/spec/vm.md の「時計と driver」)。
    pub(crate) driver_kind: DriverKind,
    /// OS を見ずに続けた `switch_away` の切り替えの数。`POLL_EVERY` に達するか、集める地点が OS を見たら 0 に戻す。
    pub(crate) since_poll: u32,
    /// 集める地点が再開する仕事を積む列。毎回の確保を避けるために使い回し、集める地点の外ではいつも空である。
    pub(crate) runnables: Vec<Runnable>,
    /// 監視のスレッドと、それに渡すもの。
    pub(crate) watcher: Watcher,
}

/// `Scheduler` が持つ監視のスレッド。`Scheduler` を捨てるとき (成功でも誤りでも) に止める。`Scheduler` は run の後に
/// driver を取り出すので、`Drop` はこの欄に置く (docs/spec/vm.md の「タスクとスケジュール」)。
pub(crate) struct Watcher {
    pub(crate) control: Arc<Control>,
    pub(crate) monitor: Option<Monitor>,
    /// 始めるのは1回だけで、始められなかったときも試し直さない。worker 1..N-1 は始めない。
    tried: bool,
    /// worker 0 の `Watcher`。見張りの語と保留のシグナルは worker 0 のものなので、片付けるのも worker 0 だけである。
    home: bool,
}

impl Drop for Watcher {
    // run の後に待つタスクはいない。最後の集める地点の後に保留になったシグナルは、run が終わることで消費する
    fn drop(&mut self) {
        if !self.home {
            return;
        }
        self.control.set_watched(0);
        self.control.take_pending();
        if let Some(monitor) = self.monitor.take() {
            monitor.stop();
        }
    }
}

impl<P: Policy> Scheduler<P> {
    pub(crate) fn new(
        policy: P,
        driver: Box<dyn IoDriver>,
        run: Arc<Run>,
        control: Arc<Control>,
        main_worker: u32,
        max_depth: usize,
        cells: Arc<Shared>,
    ) -> Scheduler<P> {
        Scheduler::build(
            policy,
            Some(driver),
            run,
            control,
            main_worker,
            max_depth,
            cells,
        )
    }

    fn build(
        policy: P,
        driver: Option<Box<dyn IoDriver>>,
        run: Arc<Run>,
        control: Arc<Control>,
        main_worker: u32,
        max_depth: usize,
        cells: Arc<Shared>,
    ) -> Scheduler<P> {
        Scheduler {
            policy,
            tasks: vec![None],
            free_tasks: Vec::new(),
            current: MAIN,
            ready: VecDeque::new(),
            joins: Vec::new(),
            free_joins: Vec::new(),
            preemptions: 0,
            peak_depth: 0,
            main_worker,
            max_depth,
            failed: None,
            shared_roots: 0,
            cells,
            completed: Vec::new(),
            stacks: Vec::new(),
            driver,
            run,
            index: 0,
            workers: Workers::Denied,
            driver_kind: DriverKind::Virtual,
            since_poll: 0,
            runnables: Vec::new(),
            watcher: Watcher {
                control,
                monitor: None,
                tried: false,
                home: true,
            },
        }
    }

    /// worker `index` (`index` ≥ 1) の `Scheduler`。driver も監視のスレッドも持たない。登録表は run のものを共有する。
    /// worker を作る判断はせず、空いた worker があれば右の子を置く (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn worker(
        policy: P,
        run: Arc<Run>,
        control: Arc<Control>,
        index: usize,
        main_worker: u32,
        max_depth: usize,
        cells: Arc<Shared>,
    ) -> Scheduler<P> {
        let mut sched = Scheduler::build(policy, None, run, control, main_worker, max_depth, cells);
        sched.index = index;
        sched.workers = Workers::Peer;
        sched.watcher.tried = true;
        sched.watcher.home = false;
        sched
    }

    /// run の始めの準備。driver の waker を自分の欄に置き、眠る driver の run と worker を2つ以上持てる run では監視の
    /// スレッドを始める。眠る worker に Ctrl-C と止める語を届けるのは監視のスレッドの起こしなので、run の始めから
    /// 動かす。続けて worker 1..N-1 を作るかを決める。作るのは、worker を2つ以上持て、監視のスレッドが動き、
    /// セッションに根がなく、読み込んだコードが根に触れない (`roots_free`) run だけである
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn begin_run(&mut self, roots_free: bool) {
        if let Some(driver) = &self.driver {
            // run の表は run ごとに作るので、まだ空である
            let _ = self.run.slots[self.index].waker.set(driver.waker());
        }
        if self.driver_kind != DriverKind::Virtual || self.run.slots.len() >= 2 {
            self.start_monitor();
        }
        self.workers = if self.run.slots.len() >= 2 && self.watcher.monitor.is_some() && roots_free
        {
            Workers::Wanted
        } else {
            Workers::Denied
        };
        // 中継するかは監視のスレッドを始めた後で決まり、run の間は変わらない。集めるのはどの worker でもよいので、
        // 登録表に写す (docs/spec/vm.md の「上限と割り込み」)
        let relay = self.relay_on();
        self.cells.lock().relay = relay;
    }

    /// 空きのループが driver の `wait` に渡す期限。監視のスレッドのない run で眠る driver なら、止める語を読み直すために、
    /// 今から `WAIT_CAP_MS` 先までに切る。監視のスレッドがあれば、止めるビットはその起こしが届ける。テストの driver は
    /// 期限まで時計を進めるので切らない。切ると、終わらない保留のある run がデッドロックで止まらない
    /// (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn wait_deadline(&mut self, deadline: Option<u64>) -> Result<Option<u64>, VmFault> {
        if self.watcher.monitor.is_some() || self.driver_kind == DriverKind::Virtual {
            return Ok(deadline);
        }
        let cap = self
            .run
            .now_ms_ceil(self.driver.as_deref())?
            .saturating_add(WAIT_CAP_MS);
        Ok(Some(deadline.map_or(cap, |d| d.min(cap))))
    }

    /// 監視のスレッドを1回だけ始める。`begin_run` のほかに、横取りする方針で2つ目の仕事が走れるようになりうるときに
    /// 呼ぶ。どれにも当たらない run はスレッドを作らない (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn start_monitor(&mut self) {
        let watcher = &mut self.watcher;
        if std::mem::replace(&mut watcher.tried, true) {
            return;
        }
        watcher.monitor = Monitor::start(
            Arc::clone(&watcher.control),
            Arc::clone(&self.cells),
            Arc::clone(&self.run),
            P::PREEMPTS,
        );
    }

    /// OS のシグナルを待つタスクへ中継するか。本物の driver の上で、監視のスレッドが動いているときだけである。
    /// 仮想の driver の run では本物のシグナルは既定の動きをする (docs/spec/stdlib.md の「`Std.Signal`」)。
    fn relay_on(&self) -> bool {
        self.driver_kind == DriverKind::Real && self.watcher.monitor.is_some()
    }

    /// 集める地点の、登録表とほかの worker の知らせの側。どの worker も同じ形で集める。受け箱の知らせを取り、mail を
    /// 処理し、driver の完了を記録に当て、満ちたタイマーを外し、見張りの語を書いて、再開する仕事を `runnables` に積む。
    /// `look` なら、先に driver の `wait` に今を渡して OS を1回見る。driver の `completions` は OS を見ないためである。
    /// 登録表の lock は、何もない集める地点では1回だけ取り、mail の項、完了、満期では今の順を保つ短い区間に分ける。
    /// driver の呼び出しと語の破棄はどれも lock の外で行う (docs/spec/vm.md の「セルと登録表」「時計と driver」)。
    /// 聞く者のいなかったシグナルがあれば、仕事を積んだ後で止める語を立てて誤りを返す
    /// (docs/spec/vm.md の「上限と割り込み」)。
    pub(crate) fn gather<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
        look: bool,
    ) -> Result<(), VmFault> {
        let k = self.index;
        // 受け箱を見る前に下ろす。後に積まれた知らせは、また立つビットが次の安全点で集めさせる
        self.watcher.control.take_notice(k);
        // ほかの worker からの知らせは、ないときは1回の読みで済ませる (docs/spec/vm.md の「タスクとスケジュール」)
        let mut notices = Vec::new();
        if self.run.slots[k].has_notices.load(Ordering::Acquire) {
            self.receive(&mut notices);
        }
        let Scheduler {
            run,
            cells: shared,
            watcher,
            driver,
            completed,
            runnables,
            since_poll,
            ..
        } = self;
        let control = &*watcher.control;
        let post = Mailer::new(run, control);
        let here = Here {
            me: k as u32,
            mail: &shared.mail,
            post: &post,
        };
        // 列は `Scheduler` のものを使い回す。誤りで抜けると捨てるが、run も止まる
        let mut out = std::mem::take(runnables);
        let mut work = IoWork::default();
        let (in_flight, deadline, mut unheard) = {
            let mut cells = shared.lock();
            if !notices.is_empty() {
                let mut woken = Vec::new();
                for notice in notices {
                    match notice {
                        Notice::Wake(w) => woken.push(w),
                        Notice::Cancel(cell) => {
                            cells.cancel_task(cell, &here, &mut woken, &mut work)?;
                        }
                        Notice::Child(_) | Notice::Joined { .. } => {
                            return Err(VmFault::Internal("a par notice is left for the registry"));
                        }
                    }
                }
                cells.route(&mut woken, &here, &mut out)?;
            }
            cells.watch(control);
            (cells.in_flight(k), cells.next_deadline(k), cells.unheard)
        };
        work.cancel_on(driver.as_deref_mut())?;
        if look {
            *since_poll = 0;
            if in_flight > 0 {
                let now = run.now_ms_floor(driver.as_deref())?;
                // 今以前の期限なので待たない。`ceil_ms` は今より先になりうるので、driver が眠りうる
                driver_mut(driver)?
                    .wait(Some(now))
                    .map_err(|e| VmFault::Driver(e.to_string()))?;
            }
        }
        let drained = cells::drain(
            shared,
            &here,
            ctx,
            driver.as_deref_mut(),
            depth_offset,
            &mut out,
        )?;
        // mail の処理は driver の数を変えない
        if in_flight > 0 {
            cells::complete(
                shared,
                ctx,
                driver_mut(driver)?,
                here.me,
                depth_offset,
                completed,
                &mut out,
            )?;
        }
        // 最初の区間の後に登録表を変えたときだけ、満期と見張りの語のためにもう1回取る。タイマーのない run は時計を
        // 読まない
        if drained || in_flight > 0 || deadline.is_some() {
            let now = match deadline {
                Some(_) => Some(run.now_ms_floor(driver.as_deref())?),
                None => None,
            };
            let mut cells = shared.lock();
            if let Some(now) = now {
                let mut woken = Vec::new();
                cells.expire(now, k, &mut woken, &mut work)?;
                cells.route(&mut woken, &here, &mut out)?;
            }
            // ハンドラは見張りの語で保留にするか既定の動きにするかを決めるので、届けて空になった列と取り消しで外れた
            // 待ち手をここで写す (docs/spec/vm.md の「上限と割り込み」)
            cells.watch(control);
            unheard = cells.unheard;
            drop(cells);
            work.cancel_on(driver.as_deref_mut())?;
        }
        *runnables = out;
        if unheard != 0 {
            return Err(unheard_fault(control, unheard));
        }
        Ok(())
    }

    /// ほかの worker から届いた知らせを取り出す。置かれた右の子はこの worker の番号を振って列の後ろに入れ、置いた子の
    /// 終わりは join の節点に記録して親を起こす。親を失った節点なら、結果に触れずに節点を捨てる。どちらでも
    /// `in_flight` から引くのは受け取った後なので、受け取っていない終わりの知らせがある間は 0 にならない。`Async` の
    /// タスクの知らせは `rest` に残し、集める地点が登録表の区間で処理する (docs/spec/vm.md の「タスクとスケジュール」)。
    #[cold]
    #[inline(never)]
    fn receive(&mut self, rest: &mut Vec<Notice>) {
        for notice in self.run.take_notices(self.index) {
            match notice {
                Notice::Child(task) => {
                    let id = self.add_task(task);
                    self.ready.push_back(id);
                }
                Notice::Joined { join, outcome } => {
                    let node = self.join(join);
                    if node.orphan {
                        // 語は捨てる。誤りで終わる run のヒープは回復で丸ごと捨てる
                        drop(self.take_join(join));
                        drop(outcome);
                    } else {
                        node.right_outcome = Some(outcome);
                        self.wake_parent(join);
                    }
                    self.run.in_flight.fetch_sub(1, Ordering::Release);
                }
                notice @ (Notice::Wake(_) | Notice::Cancel(_)) => rest.push(notice),
            }
        }
    }

    /// 右の子が終わった join の親が、その join を待っていれば起こす。
    pub(crate) fn wake_parent(&mut self, j: JoinId) {
        let parent = self.join(j).parent;
        if let Some(task) = &self.tasks[parent]
            && matches!(task.state, TaskState::Waiting(w) if w == j)
        {
            self.policy.wake(&mut self.ready, parent);
        }
    }

    /// この `Scheduler` の worker の番号。登録表のセルの持ち主と区画の添字である。
    pub(crate) fn me(&self) -> u32 {
        self.index as u32
    }

    pub(crate) fn add_task(&mut self, task: Task) -> TaskId {
        if P::PREEMPTS {
            self.start_monitor();
        }
        match self.free_tasks.pop() {
            Some(id) => {
                self.tasks[id] = Some(task);
                id
            }
            None => {
                self.tasks.push(Some(task));
                self.tasks.len() - 1
            }
        }
    }

    /// 動いているタスクの番号を替える。`current` はここでだけ書き、横取りする方針では監視のスレッドに切り替えを伝える。
    /// 数は監視のスレッドが判定に読むだけで、ほかのメモリへの書き込みを公開しないので Relaxed で足りる。
    pub(crate) fn set_current(&mut self, id: TaskId) {
        self.current = id;
        if P::PREEMPTS {
            self.run.slots[self.index]
                .switches
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 表から外したタスクの欄を空ける。
    pub(crate) fn free_task(&mut self, id: TaskId) {
        self.tasks[id] = None;
        self.free_tasks.push(id);
    }

    pub(crate) fn add_join(&mut self, join: JoinNode) -> JoinId {
        match self.free_joins.pop() {
            Some(id) => {
                self.joins[id] = Some(join);
                id
            }
            None => {
                self.joins.push(Some(join));
                self.joins.len() - 1
            }
        }
    }

    pub(crate) fn join(&mut self, j: JoinId) -> &mut JoinNode {
        self.joins[j].as_mut().expect("a join id names a live join")
    }

    pub(crate) fn take_join(&mut self, j: JoinId) -> JoinNode {
        let join = self.joins[j].take().expect("a join id names a live join");
        self.free_joins.push(j);
        join
    }

    /// エントリのタスクのほかにタスクか join が残っているか。
    pub(crate) fn has_children(&self) -> bool {
        self.tasks.iter().any(Option::is_some) || self.joins.iter().any(Option::is_some)
    }

    pub(crate) fn count_preemption(&mut self) {
        self.preemptions += 1;
    }

    pub(crate) fn preemptions(&self) -> u64 {
        self.preemptions
    }

    /// 動くのをやめるタスクが使った語の数を覚える。スタックの長さは縮めないので、タスクの長さがそのままそのタスクの
    /// 最大である。
    pub(crate) fn note_depth(&mut self, task: &Task) {
        self.peak_depth = self.peak_depth.max(task.depth_offset + task.stack.len());
    }

    /// タスクのスタックで同時に使った語の数の最大 (docs/spec/vm.md の「`RunStats`」の `peak_stack`)。右の子の語は、
    /// fork のときの親の深さに足して数える。`current` は最後に動いていたタスクである。
    pub(crate) fn peak_stack(&self, current: &Task) -> u64 {
        self.peak_depth
            .max(current.depth_offset + current.stack.len()) as u64
    }
}

#[cfg(test)]
mod tests {
    use eml_rt::Limits;

    use super::*;
    use crate::cells::{MailItem, Registry, Woken};
    use crate::doorbell::Doorbell;
    use crate::driver::RunClock;

    /// 出どころを `kind` にした、テストの driver の `Scheduler`。テストの driver の時計は run の外では進まない。
    fn scheduler(kind: DriverKind, doorbell: bool, workers: usize) -> Scheduler<Sequential> {
        let control = Arc::new(Control::new(Limits {
            workers,
            ..Limits::default()
        }));
        let driver = eml_io::TestDriver::new(None);
        // 渡された driver の run は基準を持たない。ほかは driver と同じ仮想の基準で読む
        let clock = match kind {
            DriverKind::Injected => RunClock::Injected,
            _ => RunClock::Virtual(driver.clock()),
        };
        let run = Run::new(
            workers,
            doorbell.then(|| Doorbell::new().unwrap()),
            clock,
            Arc::default(),
        );
        let mut sched = Scheduler::new(
            Sequential,
            Box::new(driver),
            Arc::new(run),
            control,
            0,
            usize::MAX,
            Arc::new(Shared::new(Registry::new(Vec::new(), workers))),
        );
        sched.driver_kind = kind;
        sched
    }

    #[test]
    fn begin_run_starts_the_monitor_for_every_sleeping_driver() {
        let cases = [
            (DriverKind::Real, true, 1, true),
            (DriverKind::Real, false, 1, true),
            (DriverKind::Injected, true, 1, true),
            (DriverKind::Injected, false, 1, true),
            (DriverKind::Virtual, true, 1, false),
            (DriverKind::Virtual, false, 1, false),
            // worker を2つ以上持てる run は、時計によらず監視のスレッドを持つ
            (DriverKind::Virtual, false, 2, true),
        ];
        for (kind, doorbell, workers, started) in cases {
            let mut sched = scheduler(kind, doorbell, workers);
            sched.begin_run(true);
            assert_eq!(
                sched.watcher.monitor.is_some(),
                started,
                "{kind:?}, doorbell {doorbell}, {workers} workers"
            );
        }
    }

    #[test]
    fn wait_deadline_caps_only_without_a_monitor() {
        for (kind, doorbell) in [
            (DriverKind::Real, true),
            (DriverKind::Real, false),
            (DriverKind::Injected, true),
            (DriverKind::Injected, false),
        ] {
            let mut watched = scheduler(kind, doorbell, 1);
            watched.begin_run(true);
            assert!(watched.watcher.monitor.is_some());
            for deadline in [Some(5000), None, Some(u64::MAX)] {
                assert_eq!(watched.wait_deadline(deadline), Ok(deadline));
            }
            // 監視のスレッドを始められなかった run と同じ形
            let mut unwatched = scheduler(kind, doorbell, 1);
            assert!(unwatched.watcher.monitor.is_none());
            assert_eq!(unwatched.wait_deadline(None), Ok(Some(10)));
            assert_eq!(unwatched.wait_deadline(Some(5000)), Ok(Some(10)));
            assert_eq!(unwatched.wait_deadline(Some(u64::MAX)), Ok(Some(10)));
            assert_eq!(unwatched.wait_deadline(Some(3)), Ok(Some(3)));
        }
        let mut virtual_ = scheduler(DriverKind::Virtual, false, 1);
        for deadline in [Some(5000), None, Some(u64::MAX)] {
            assert_eq!(virtual_.wait_deadline(deadline), Ok(deadline));
        }
    }

    /// driver を持たない worker が driver に触れると、内部の誤りになる。
    #[test]
    fn a_worker_without_a_driver_cannot_reach_one() {
        let mut sched = scheduler(DriverKind::Injected, false, 1);
        sched.driver = None;
        assert_eq!(sched.wait_deadline(None), Err(NO_DRIVER));
    }

    /// seed の方針は worker ごとに別の列を引き、同じ worker なら同じ列を引く。ほかの方針はそのまま写す。
    #[test]
    fn for_worker_splits_only_the_seeded_stream() {
        let base = Seeded::new(7);
        let draws = |k: usize| {
            let mut policy = base.for_worker(k);
            [policy.next(), policy.next()]
        };
        assert_eq!(draws(1), draws(1));
        assert_ne!(draws(0), draws(1));
        assert_eq!(base.for_worker(3).seed, splitmix64(7 ^ 3));
        assert!(Normal.for_worker(2).pick(&mut VecDeque::from([4])) == Some(4));
        assert!(Sequential.for_worker(2).pick(&mut VecDeque::from([4])) == Some(4));
    }

    /// worker が2つの run の表と登録表と、worker 1 の `Scheduler` とその `Ctx`。スレッドは作らず、worker 1 の集める
    /// 地点を手で動かす。
    struct Worker1 {
        control: Arc<Control>,
        run: Arc<Run>,
        shared: Arc<Shared>,
        sched: Scheduler<Sequential>,
        ctx: Ctx<eml_rt::Checked>,
    }

    fn worker_1() -> Worker1 {
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            ..Limits::default()
        }));
        let run = Arc::new(Run::new(
            2,
            None,
            RunClock::Virtual(Arc::default()),
            Arc::default(),
        ));
        let shared = Arc::new(Shared::new(Registry::new(Vec::new(), 2)));
        let sched = Scheduler::worker(
            Sequential,
            Arc::clone(&run),
            Arc::clone(&control),
            1,
            0,
            usize::MAX,
            Arc::clone(&shared),
        );
        let (host, _) = crate::capture_host(Default::default());
        let ctx = Ctx::for_worker(Arc::clone(&control), host, 1);
        Worker1 {
            control,
            run,
            shared,
            sched,
            ctx,
        }
    }

    /// mail のシグナルは最初に集めた worker が届ける。worker 1 が届けた待ち手は worker 0 のタスクなので、起こしを
    /// worker 0 の受け箱に送り、待ち手の列が空になった見張りの語を書く (docs/spec/vm.md の「上限と割り込み」)。
    #[test]
    fn a_signal_drained_by_worker_1_wakes_a_waiter_owned_by_worker_0() {
        let Worker1 {
            control,
            run,
            shared,
            mut sched,
            mut ctx,
        } = worker_1();
        let cell = cells::terminate_waiter(&shared);
        shared.lock().relay = true;
        // 待ち手が待ち始めたときに書いた語と、監視のスレッドの中継
        control.set_watched(Control::TERMINATE);
        control.defer(Control::TERMINATE);
        assert_eq!(control.claim_signals(), Control::TERMINATE);
        shared.mail.push(MailItem::Signal(Control::TERMINATE));
        sched.gather(&mut ctx, 0, false).unwrap();
        assert!(sched.runnables.is_empty());
        let notices = run.take_notices(0);
        assert!(
            matches!(
                notices[..],
                [Notice::Wake(Woken::Task {
                    cell: c,
                    wake: cells::WOKE,
                    op: None,
                })] if c == cell
            ),
            "{} notices",
            notices.len()
        );
        assert_ne!(control.worker_bits(0) & Control::NOTICE, 0);
        assert_eq!(shared.lock().unheard, 0);
        assert_eq!(control.watched(), 0);
        assert_eq!(control.pending_signals(), 0);
    }

    /// worker 1 が mail で、worker 0 が持ち主の終わったタスクの落ちたハンドルを受けると、自分の `Ctx` でそのタスクの
    /// ID に替えて値を落とし、ID を退かせる。値は worker 0 のヒープの物体なので別のスレッドからの解放になり、取り込んだ後に
    /// 漏れも数の食い違いも残らない (docs/spec/vm.md の「セルと登録表」)。
    #[test]
    fn worker_1_frees_the_value_of_a_finished_task_of_worker_0() {
        let Worker1 {
            control,
            run,
            shared,
            mut sched,
            mut ctx,
        } = worker_1();
        let (host, _) = crate::capture_host(Default::default());
        let mut home = Ctx::<eml_rt::Checked>::new(Arc::clone(&control), host);
        let x = unsafe { home.heap_mut() }.fork_worker();
        let value = cells::with_worker(&mut home, x, 0, |ctx| ctx.fresh_string("left"));
        let scope = shared.lock().open_root(None, None).unwrap();
        let cell = cells::member_task(&shared, scope, x);
        {
            let post = Mailer::new(&run, &control);
            let here = Here {
                me: 0,
                mail: &shared.mail,
                post: &post,
            };
            let mut discards = cells::Discards::default();
            shared
                .lock()
                .finish(
                    &here,
                    cell,
                    cells::Outcome::Value(value),
                    &mut Vec::new(),
                    &mut discards,
                )
                .unwrap();
        }
        shared.mail.push(MailItem::HandleDropped(cell));
        sched.gather(&mut ctx, 0, false).unwrap();
        assert!(sched.runnables.is_empty());
        assert!(!shared.mail.pending());
        home.reclaim();
        ctx.reclaim();
        assert!(home.heap().stats().reclaimed >= 1);
        let leaks = home.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
        control.check_owned(&[home.heap(), ctx.heap()]).unwrap();
    }

    /// worker 0 が自分のタスクに付けた取り消しの項は、worker 1 の集める地点が取らずに mail に残す。待ちは外さず、
    /// worker 0 の受け箱にも送らない。worker 0 が次の集める地点で自分で外すためである (docs/spec/vm.md の
    /// 「取り消しの届け方」)。
    #[test]
    fn worker_1_leaves_a_cancel_of_worker_0_in_the_mail() {
        let Worker1 {
            run,
            shared,
            mut sched,
            mut ctx,
            ..
        } = worker_1();
        let cell = cells::sleeping_task(&shared, 0, 5);
        shared.mail.push(MailItem::Cancel { cell, owner: 0 });
        sched.gather(&mut ctx, 0, false).unwrap();
        assert!(sched.runnables.is_empty());
        assert!(run.take_notices(0).is_empty());
        assert!(shared.mail.pending());
        assert!(matches!(
            shared.mail.take(0)[..],
            [MailItem::Cancel { cell: c, owner: 0 }] if c == cell
        ));
    }

    /// 待ち手のいないシグナルを worker 1 が届けると、止めるビットをすべての worker の語に立てて止まる。
    #[test]
    fn an_unheard_signal_drained_by_worker_1_raises_every_word() {
        let Worker1 {
            control,
            shared,
            mut sched,
            mut ctx,
            ..
        } = worker_1();
        shared.mail.push(MailItem::Signal(Control::TERMINATE));
        assert_eq!(
            sched.gather(&mut ctx, 0, false),
            Err(VmFault::Stopped(Stop::Terminated))
        );
        for k in 0..2 {
            assert_ne!(control.worker_bits(k) & Control::TERMINATE, 0, "worker {k}");
        }
    }
}

//! タスクのスケジュール (docs/spec/vm.md の「タスクとスケジュール」)。方針を `Scheduler` の型の引数にするので、
//! 逐次の方針の安全点は、方針を持たない繰り返しと同じ機械語になる。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{Ordering, fence};

use eml_rt::{Completion, Control, Ctx, IoDriver, Mode, Stop};

use crate::cells::{self, CellRef, Here, Hold, HostBin, IoWork, Registry, Runnable, Shared, Woken};
use crate::driver::DriverKind;
use crate::monitor::{self, Monitor};
use crate::park::{Guard, PollSource};
use crate::vm::Task;
use crate::workers::{Mailer, NO_CELL, Notice, Ready, Run, Workers, cell_word};
pub(crate) use eml_sched::driver::NO_DRIVER;
use eml_sched::error::RunFault;

/// 待つタスクのいなかったシグナルで run を止める。待つタスクのないときにハンドラが立てるのと同じビットを、すべての
/// worker の止める語に立てるので、`eml run` の次の同じシグナルはその場で終わらせ、どの worker も次の安全点で止まる
/// (docs/spec/vm.md の「上限と割り込み」)。
#[cold]
#[inline(never)]
fn unheard_fault(control: &Control, bits: u32) -> RunFault {
    if bits & Control::TERMINATE != 0 {
        control.terminate();
    }
    if bits & Control::INTERRUPT != 0 {
        control.interrupt();
        return RunFault::Stopped(Stop::Interrupted);
    }
    RunFault::Stopped(Stop::Terminated)
}

/// 受け箱から取った`Async`のタスクとdriverの操作の知らせを、登録表の1回の区間で届いた順に処理する。走らせる仕事を
/// `out`に、lockを放してからdriverに出す操作と取り消しを`work`に積む。`Place`、`Wake`、`Cancel`、`Limit`は、どれも
/// 自分が持ち主のタスクか、tokenの起こしなら自分の上で待つ`Task`を名指す。送る側はタスクの持ち主か待つ`Task`の
/// workerに送り、持ち主は`start`の後に変わらないためである。`Completion`は
/// 記録の持ち主の自分が当てる。hostの知らせ (`Submit`、`CancelIo`) は自分のdriverの分で、結果を置いた後に届いた
/// `CancelIo`だけは何もしない。もう待っていないセルへの起こしと、結果のない記録への`Completion`は内部の誤りである
/// (docs/spec/vm.mdの「タスクとスケジュール」「操作の記録」)。`Limit`は旗がまだ立っていてタスクが動いていれば、
/// そのセルを`limited`に積む (docs/spec/vm.mdの「タスクの上限」)。
#[allow(clippy::too_many_arguments)]
fn take_async<M: Mode>(
    cells: &mut Registry,
    notices: Vec<Notice>,
    here: &Here,
    ctx: &mut Ctx<M>,
    depth_offset: usize,
    out: &mut Vec<Runnable>,
    work: &mut IoWork,
    limited: &mut Vec<CellRef>,
) -> Result<(), RunFault> {
    let mut woken = Vec::new();
    for notice in notices {
        match notice {
            Notice::Wake(w) => {
                debug_assert!(
                    cells.wake_is_for(&w, here.me),
                    "a wake reaches a worker that is not its destination"
                );
                woken.push(w);
            }
            Notice::Cancel(cell) => cells.unpark(cell, here, &mut woken, work)?,
            Notice::Place {
                cell,
                worker,
                closure,
            } => {
                cells.route(&mut woken, here, out)?;
                cells.placed(cell, worker, here.me)?;
                out.push(Runnable::Start {
                    cell,
                    worker,
                    closure,
                });
            }
            Notice::Submit(op) => cells.take_submit(op, here.me, work)?,
            Notice::CancelIo(op) => cells.cancel_hosted(op, here.me, work)?,
            Notice::Completion(op) => {
                woken.extend(cells.take_completion(ctx, depth_offset, op, here, work)?);
            }
            Notice::Limit(cell) => limited.extend(cells.limit_forwarded(cell, here.me)),
            Notice::Child(_) | Notice::Joined { .. } => {
                return Err(RunFault::Internal("a par notice is left for the registry"));
            }
        }
    }
    cells.route(&mut woken, here, out)?;
    Ok(())
}

/// 誰がメモリの柔らかい上限を見て、タスクに上限の旗を立てるか (docs/spec/vm.md の「タスクとスケジュール」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memory {
    /// 監視のスレッドが刻みごとに見る。
    Watched,
    /// 各 worker が集める地点の始めに見る。壁時計の監視のスレッドに頼らず、旗の地点をスケジュールで決めるためである。
    Gathered,
}

/// スケジューラの中のタスクの番号。
pub type TaskId = usize;

/// スケジュールの方針。安全点で切り替えるか、次にどのタスクを動かし、起こしたタスクを列のどこに入れるかを決める。
pub trait Policy: Send {
    /// メモリの柔らかい上限を見る者。
    const MEMORY: Memory;
    /// 安全点ごとに1回呼ぶ。真を返したら、切り替えを試みる。
    fn tick(&mut self) -> bool;
    /// 切り替えを試みた後に、次の切り替えまでの予算を詰め直す。
    fn reload(&mut self);
    /// 走れるタスクの列から、次に動かす項を取り出す。列の lock の下で呼ぶので、列のほかに触れない。
    fn pick(&mut self, ready: &mut VecDeque<Ready>) -> Option<Ready>;
    /// 起こしたタスクを、走れるタスクの列に入れる。列の lock の下で呼ぶ。
    fn wake(&mut self, ready: &mut VecDeque<Ready>, task: TaskId);
    /// worker `k` の `Scheduler` が使う方針。seed を持つ方針は worker ごとに別の擬似乱数の列にする。同じ seed の run は
    /// どの worker でも同じ列を引かないためである。
    fn for_worker(&self, k: usize) -> Self
    where
        Self: Sized;
}

/// 自分からは切り替えない方針。タスクは`Async`の操作で待つか終わるまで走る。列はFIFOである。
#[derive(Debug, Clone, Copy, Default)]
pub struct Normal;

impl Policy for Normal {
    const MEMORY: Memory = Memory::Watched;

    // 定数の偽に畳まれ、安全点に分岐を残さない
    #[inline(always)]
    fn tick(&mut self) -> bool {
        false
    }

    fn reload(&mut self) {}

    fn pick(&mut self, ready: &mut VecDeque<Ready>) -> Option<Ready> {
        ready.pop_front()
    }

    fn wake(&mut self, ready: &mut VecDeque<Ready>, task: TaskId) {
        ready.push_back(Ready::Task(task));
    }

    fn for_worker(&self, _k: usize) -> Normal {
        *self
    }
}

/// 自分からは切り替えない方針。列の先頭から取り、起こしたタスクも先頭に入れる。
#[derive(Debug, Clone, Copy, Default)]
pub struct Sequential;

impl Policy for Sequential {
    const MEMORY: Memory = Memory::Gathered;

    // 定数の偽に畳まれ、安全点に分岐を残さない
    #[inline(always)]
    fn tick(&mut self) -> bool {
        false
    }

    fn reload(&mut self) {}

    fn pick(&mut self, ready: &mut VecDeque<Ready>) -> Option<Ready> {
        ready.pop_front()
    }

    fn wake(&mut self, ready: &mut VecDeque<Ready>, task: TaskId) {
        ready.push_front(Ready::Task(task));
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
    /// 次の切り替えまでに残る安全点の数。0のままなら、次の安全点でまた試みる。
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
    const MEMORY: Memory = Memory::Gathered;

    // 予算が0になってもyieldの途中なら切り替えないので、詰め直すのは切り替えを試みたときだけにする
    fn tick(&mut self) -> bool {
        self.budget = self.budget.saturating_sub(1);
        self.budget == 0
    }

    fn reload(&mut self) {
        self.budget = 1 + self.next() % 16;
    }

    fn pick(&mut self, ready: &mut VecDeque<Ready>) -> Option<Ready> {
        if ready.is_empty() {
            return None;
        }
        let index = (self.next() % ready.len() as u64) as usize;
        ready.remove(index)
    }

    fn wake(&mut self, ready: &mut VecDeque<Ready>, task: TaskId) {
        ready.push_back(Ready::Task(task));
    }

    fn for_worker(&self, k: usize) -> Seeded {
        Seeded::new(splitmix64(self.seed ^ k as u64))
    }
}

/// join の節点の番号。境目のフレームの slot 1 に生の語で置く。
pub type JoinId = usize;

/// エントリのタスクの番号。worker 1..N-1 では、動くタスクのない間の `Task` を退避する欄で、どのタスクにも振らない。
pub(crate) const ENTRY: TaskId = 0;

/// ほかの worker に置いた右の子を表す、join の節点の `right`。置いた子はこの worker の表にない。
pub(crate) const PLACED: TaskId = usize::MAX;

/// `compute` の仕事の `Task` の `boundaries[0]` と境目のフレームの slot 1 に置く番兵の join。仕事は join の節点を
/// 持たず、終わりを仕事のセルに置く (docs/spec/vm.md の「`compute` の仕事」)。
pub(crate) const JOB: JoinId = usize::MAX;

/// 監視のスレッドのない run で、空きのループが1回の `wait` に渡す期限の上限 (ミリ秒)。Ctrl-C と確保量の上限が眠る
/// worker を止めるまでの遅れの上限になる (docs/spec/vm.md の「時計と driver」)。
const WAIT_CAP_MS: u64 = 10;

/// worker の driver。仮想の時計の run では worker 0 だけが持ち、本物の時計の run の worker `k` (`k` ≥ 1) は最初に
/// 要るときに作るので、持たない worker では内部の誤りにする (docs/spec/vm.md の「時計と driver」)。`Scheduler` の
/// ほかの欄と同時に借りられるように、欄を受け取る関数にしてある。
pub(crate) fn driver_mut(
    driver: &mut Option<Box<dyn IoDriver>>,
) -> Result<&mut (dyn IoDriver + 'static), RunFault> {
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
    /// join が見つけた右の子の誤り。`step` は `RunFault` しか返せないので、ここに置いて実行の繰り返しに渡す。
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
    /// 時計と待ち。worker 0 は `Machine` が持つものを run の間だけ預かる。本物の時計の run の worker `k` (`k` ≥ 1) は、
    /// 最初に要るときに自分のスレッドで作り、抜けるときに落とす (docs/spec/vm.md の「時計と driver」)。触れるときは
    /// `driver_mut` を通す。
    pub(crate) driver: Option<Box<dyn IoDriver>>,
    /// `own_driver`で作ったdriverが待つOSの源。メインのworkerの`Park`が`Pending::io`で読む。そのdriverがなければ
    /// `None`である (docs/spec/vm.mdの「メインのworkerの待ち」)。
    pub(crate) poll: Option<PollSource>,
    /// 最後の集める地点が登録表から読んだ、自分の区画のいちばん早い期限。本物の時計の worker `k` (`k` ≥ 1) の空きの
    /// 待ちが、登録表の lock を取らずに期限を知るためにある。区画に項を足すのはその持ち主だけで、足した後は眠る前に
    /// 集める地点を通るので、今の期限より遅くはならない (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) own_deadline: Option<u64>,
    /// この run の worker が共有する表。
    pub(crate) run: Arc<Run>,
    /// この `Scheduler` の worker の番号。止める語と `run` の欄の添字である。
    pub(crate) index: usize,
    /// worker 1..N-1 を作るか、作ったか。worker 0 は run の始めに決め、worker 1..N-1 はいつも `Peer` である。
    /// 右の子と仕事をほかの worker に置くのは `Started` と `Peer` の `Scheduler` だけである。
    pub(crate) workers: Workers,
    /// 分かれたrunのメインのworkerの`Scheduler`である。走れるタスクがなくなったら、空きの待ちに入らずに`drive`から
    /// 抜ける (docs/spec/vm.mdの「メインのworker」)。
    pub(crate) main_loop: bool,
    /// メインのworkerの`Pending::run`の状態。待ちの外では`Outside`である (docs/spec/vm.mdの「メインのworkerの待ち」)。
    pub(crate) guard: Guard,
    /// driverの出どころ。runの`Run`の時間の基準から決めるので、どのworkerでも同じである。`Real`のrunだけがOSの
    /// シグナルを待つタスクへ届ける (docs/spec/stdlib.mdの「`Std.Signal`」)。空きの待ちに上限を掛けるかと、TLSが
    /// 信頼する根もこれで決まる (docs/spec/vm.mdの「時計と driver」)。
    pub(crate) driver_kind: DriverKind,
    /// OS を見ずに続けた `switch_away` の切り替えの数。`POLL_EVERY` に達するか、集める地点が OS を見たら 0 に戻す。
    pub(crate) since_poll: u32,
    /// 集める地点が再開する仕事を積む列。毎回の確保を避けるために使い回し、集める地点の外ではいつも空である。
    pub(crate) runnables: Vec<Runnable>,
    /// 集める地点が受けた上限の旗のうち、旗を付ける VM の `Task` がこの worker にあるタスクのセル。実行系が集める地点の
    /// 後で `Task` に旗を付けて空にする (docs/spec/vm.md の「タスクの上限」)。
    pub(crate) limited: Vec<CellRef>,
    /// 待ちに入って`Finish`したエントリの`Task`のセルと、そのエントリが数えた分。次の集める地点の最初の区間で、
    /// 登録表の`add_bytes`に渡して空にする (docs/spec/vm.mdの「タスクとスケジュール」「セルと登録表」)。
    pub(crate) added: Vec<(CellRef, usize)>,
    /// 今の区間の始めに読んだ、この worker のヒープの `Control::taken`。区間は切り替えの地点と、エントリの `Task` が
    /// 動き始める地点と、旗で `ResourceLimit` を起こした地点で区切る (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) mark: usize,
    /// 今のタスクが自分の再開を実行待ちに積んだ (`yield_now`、`sleep 0`、速い道の譲り)。そのタスクは待ちに入っても
    /// `top` に残す。切り替えで下ろす。
    pub(crate) yielded: bool,
    /// 欄に最後に書いた `(current, bytes, mark)` の写し。変わらない書き込みを飛ばす。
    published: (u64, usize, usize),
    /// channel の予約から確定までに要るもの。予約と確定の間には安全点も切り替えもないので、安全点ではいつも空である
    /// (docs/spec/vm.md の「channel」)。
    pub(crate) hold: Option<Hold>,
    /// channel の行が起こした待ち手を積む列。毎回の確保を避けるために使い回し、行の外ではいつも空である。
    pub(crate) woken: Vec<Woken>,
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
        let driver_kind = run.driver_kind();
        Scheduler {
            policy,
            tasks: vec![None],
            free_tasks: Vec::new(),
            current: ENTRY,
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
            poll: None,
            own_deadline: None,
            run,
            index: 0,
            workers: Workers::Denied,
            main_loop: false,
            guard: Guard::Outside,
            driver_kind,
            since_poll: 0,
            runnables: Vec::new(),
            limited: Vec::new(),
            added: Vec::new(),
            mark: 0,
            yielded: false,
            published: (NO_CELL, 0, 0),
            hold: None,
            woken: Vec::new(),
            watcher: Watcher {
                control,
                monitor: None,
                tried: false,
                home: true,
            },
        }
    }

    /// worker `index` (`index` ≥ 1) の `Scheduler`。監視のスレッドは持たず、driver は本物の時計の run で最初に要るときに
    /// 作る。登録表は run のものを共有する。worker を作る判断はせず、空いた worker があれば右の子と仕事を置く
    /// (docs/spec/vm.md の「タスクとスケジュール」「時計と driver」)。
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

    /// runの始めの準備。driverのwakerを自分の欄に置き、眠るdriverのrunとworkerを2つ以上持てるrunでは監視の
    /// スレッドを始める。眠るworkerにCtrl-Cと止める語を届けるのは監視のスレッドの起こしなので、runの始めから
    /// 動かす。続けてworker 1..N-1を作るかを決めて返す。作るのは、workerを2つ以上持て、監視のスレッドが動き、
    /// セッションに根がなく、読み込んだコードが根に触れない (`roots_free`) runだけである。渡されたdriverのrun
    /// (`Injected`) は作らない。時刻をそのdriverに聞くので、driverを持たないworkerは時刻を読めないためである
    /// (docs/spec/vm.mdの「タスクとスケジュール」「時計と driver」)。
    pub(crate) fn begin_run(&mut self, roots_free: bool) -> bool {
        self.publish_driver();
        if self.driver_kind != DriverKind::Virtual || self.run.workers() >= 2 {
            self.start_monitor();
        }
        // 中継するかは監視のスレッドを始めた後で決まり、run の間は変わらない。集めるのはどの worker でもよいので、
        // 登録表に写す (docs/spec/vm.md の「上限と割り込み」)
        let relay = self.relay_on();
        self.cells.lock().relay = relay;
        self.run.workers() >= 2
            && self.watcher.monitor.is_some()
            && roots_free
            && self.driver_kind != DriverKind::Injected
    }

    /// 持っている driver の waker と bin を、自分の欄と登録表に置く。run の表は run ごとに作り、driver は worker ごとに
    /// 1つなので、run に1回だけ書く。bin は、ほかの worker の持ち主がこの driver の結果から作ったハンドルを落とすときに
    /// 積む先で、欄にも置くのは、ほかの worker が集める地点で印を見て起こすためである (docs/spec/vm.md の「時計と driver」)。
    fn publish_driver(&mut self) {
        if let Some(driver) = &self.driver {
            let _ = self.run.slots[self.index].waker.set(driver.waker());
            let bin = Arc::new(HostBin::new(driver.bin(), self.run.hosts_apart()));
            let _ = self.run.slots[self.index].bin.set(Arc::clone(&bin));
            self.cells.lock().set_bin(self.index, bin);
        }
    }

    /// 本物の時計の run の worker `k` (`k` ≥ 1) が、まだ driver を持たなければ、自分のスレッドで run の時間の基準から
    /// 作る。自分で受け付ける操作 (`listen` と `connect`、縛っていないストリームの操作) を出す前と、期限のある空きの
    /// 待ちの前に呼ぶ。waker は眠った形になる前に置くので、作った後の起こしを取りこぼさない。ほかの run とほかの
    /// worker では何もしない (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn own_driver(&mut self) -> Result<(), RunFault> {
        if self.driver.is_some() || self.run.io_worker(self.index) != self.index {
            return Ok(());
        }
        let Some(made) = self.run.new_driver() else {
            return Ok(());
        };
        let (driver, poll) = made.map_err(|e| RunFault::Driver(e.to_string()))?;
        self.driver = Some(driver);
        self.poll = poll;
        self.publish_driver();
        Ok(())
    }

    /// 空きのループが driver の `wait` に渡す期限。監視のスレッドのない run で眠る driver なら、止める語を読み直すために、
    /// 今から `WAIT_CAP_MS` 先までに切る。監視のスレッドがあれば、止めるビットはその起こしが届ける。テストの driver は
    /// 期限まで時計を進めるので切らない。切ると、終わらない保留のある run がデッドロックで止まらない
    /// (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn wait_deadline(&mut self, deadline: Option<u64>) -> Result<Option<u64>, RunFault> {
        if self.watcher.monitor.is_some() || self.driver_kind == DriverKind::Virtual {
            return Ok(deadline);
        }
        let cap = self
            .run
            .now_ms_ceil(self.driver.as_deref())?
            .saturating_add(WAIT_CAP_MS);
        Ok(Some(deadline.map_or(cap, |d| d.min(cap))))
    }

    /// 監視のスレッドを1回だけ始める。`begin_run`のほかに、メモリを見張る方針で最初の`Async`のタスクを列に積むときに
    /// 呼ぶ。どれにも当たらないrunはスレッドを作らない (docs/spec/vm.mdの「タスクとスケジュール」)。
    pub(crate) fn start_monitor(&mut self) {
        let watcher = &mut self.watcher;
        if std::mem::replace(&mut watcher.tried, true) {
            return;
        }
        watcher.monitor = Monitor::start(
            Arc::clone(&watcher.control),
            Arc::clone(&self.cells),
            Arc::clone(&self.run),
            P::MEMORY == Memory::Watched,
        );
    }

    /// OS のシグナルを待つタスクへ中継するか。本物の driver の上で、監視のスレッドが動いているときだけである。
    /// 仮想の driver の run では本物のシグナルは既定の動きをする (docs/spec/stdlib.md の「`Std.Signal`」)。
    fn relay_on(&self) -> bool {
        self.driver_kind == DriverKind::Real && self.watcher.monitor.is_some()
    }

    /// 集める地点の、登録表とほかのworkerの知らせの側。どのworkerも同じ形で集める。受け箱の知らせを取り、
    /// hostとして受けた操作をdriverに出し、mailを処理し、driverの完了を記録に置き (持ち主が自分なら当て、
    /// ほかのworkerなら`Completion`を送る)、満ちたタイマーを外し、見張りの語を書いて、再開する仕事を`runnables`に積む。
    /// 最初の登録表の区間では、始めに`added`の分をセルの数に足す (docs/spec/vm.mdの「タスクとスケジュール」)。
    /// `look`なら、先にdriverの`wait`に今を渡してOSを1回見る。driverの`completions`はOSを見ないためである。
    /// 登録表のlockは、何もない集める地点では1回だけ取り、mailの項、完了、満期では今の順を保つ短い区間に分ける。
    /// driverの呼び出しと語の破棄はどれもlockの外で行う (docs/spec/vm.mdの「セルと登録表」「時計とdriver」)。
    /// 聞く者のいなかったシグナルがあれば、仕事を積んだ後で止める語を立てて誤りを返す (docs/spec/vm.mdの
    /// 「上限と割り込み」)。
    pub(crate) fn gather<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
        look: bool,
    ) -> Result<(), RunFault> {
        let k = self.index;
        // 受け箱を見る前に下ろす。後に積まれた知らせは、また立つビットが次の安全点で集めさせる
        self.watcher.control.take_notice(k);
        // 決定的な方針では監視のスレッドの代わりにここで柔らかい上限を見る。積んだ旗の項は、この集める地点の mail の
        // 処理が取る。候補がなければ閾値を進める。刻みで探し直す者がいないためである (docs/spec/vm.md の
        // 「タスクとスケジュール」「上限と割り込み」)
        if P::MEMORY == Memory::Gathered {
            monitor::pressure_tick(&self.watcher.control, &self.cells, &self.run, true);
        }
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
            own_deadline,
            limited,
            added,
            ..
        } = self;
        let control = &*watcher.control;
        // 仮想の時計の run では、driver を持つのは worker 0 だけである (docs/spec/vm.md の「時計と driver」)
        debug_assert!(
            k == 0 || run.io_worker(k) == k || driver.is_none(),
            "a worker that hosts no I/O has a driver"
        );
        let post = Mailer::new(run, control);
        let here = Here {
            me: k as u32,
            host: run.io_worker(k) as u32,
            mail: &shared.mail,
            post: &post,
        };
        // 列は `Scheduler` のものを使い回す。誤りで抜けると捨てるが、run も止まる
        let mut out = std::mem::take(runnables);
        let mut work = IoWork::default();
        let (in_flight, deadline, mut unheard) = {
            let mut cells = shared.lock();
            cells.add_bytes(added)?;
            if !notices.is_empty() {
                take_async(
                    &mut cells,
                    notices,
                    &here,
                    ctx,
                    depth_offset,
                    &mut out,
                    &mut work,
                    limited,
                )?;
            }
            cells.watch(control);
            *own_deadline = cells.next_deadline(k);
            (cells.in_flight(k), *own_deadline, cells.unheard)
        };
        // host として受けた操作を driver に出し、取り消しを頼む。driver がその場で終えた操作は短い区間で結果を置く
        cells::run_io(
            shared,
            &here,
            ctx,
            driver.as_deref_mut(),
            depth_offset,
            &mut work,
            &mut out,
        )?;
        if look {
            *since_poll = 0;
            if in_flight > 0 {
                let now = run.now_ms_floor(driver.as_deref())?;
                // 今以前の期限なので待たない。`ceil_ms` は今より先になりうるので、driver が眠りうる
                driver_mut(driver)?
                    .wait(Some(now))
                    .map_err(|e| RunFault::Driver(e.to_string()))?;
            }
        }
        let drained = cells::drain(
            shared,
            &here,
            ctx,
            driver.as_deref_mut(),
            depth_offset,
            &mut out,
            limited,
        )?;
        // bin に印のあるハンドルは、driver が bin を空にするまで閉じない。完了がなくても driver に完了を問わせて空に
        // させる (docs/spec/vm.md の「時計と driver」)
        let closing = run.slots[k].bin.get().is_some_and(|bin| bin.take_dropped());
        // mail の処理は driver の数を変えない
        if in_flight > 0 || closing {
            cells::complete(
                shared,
                &here,
                ctx,
                driver_mut(driver)?,
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
                cells.expire(now, k, &here, &mut woken, &mut work)?;
                cells.route(&mut woken, &here, &mut out)?;
                *own_deadline = cells.next_deadline(k);
            }
            // ハンドラは見張りの語で保留にするか既定の動きにするかを決めるので、届けて空になった列と取り消しで外れた
            // 待ち手をここで写す (docs/spec/vm.md の「上限と割り込み」)
            cells.watch(control);
            unheard = cells.unheard;
            drop(cells);
            work.cancel_on(driver.as_deref_mut())?;
        }
        *runnables = out;
        // この worker で落とした、ほかの worker が host のハンドルを閉じさせる。どの lock も持たずに起こす
        run.ring_hosts(k);
        if unheard != 0 {
            return Err(unheard_fault(&self.watcher.control, unheard));
        }
        Ok(())
    }

    /// ほかの worker から届いた知らせを取り出す。置かれた右の子と仕事はこの worker の番号を振って列の後ろに入れ、置いた子の
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
                    self.run.push_ready(self.index, Ready::Task(id));
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
                notice @ (Notice::Place { .. }
                | Notice::Wake(_)
                | Notice::Cancel(_)
                | Notice::Submit(_)
                | Notice::CancelIo(_)
                | Notice::Completion(_)
                | Notice::Limit(_)) => rest.push(notice),
            }
        }
    }

    /// `main` の `Finish` で、自分の受け箱の知らせを処理し、再開する仕事を `out` に積む。集める地点と違って driver の完了も
    /// 満期も見ない。成功した run の終わりに走れる仕事が残っていないことを確かめるためにある (docs/spec/vm.md の
    /// 「誤り、止めること、終わり」)。
    pub(crate) fn take_slot<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
        out: &mut Vec<Runnable>,
    ) -> Result<(), RunFault> {
        if !self.run.slots[self.index]
            .has_notices
            .load(Ordering::Acquire)
        {
            return Ok(());
        }
        let mut notices = Vec::new();
        self.receive(&mut notices);
        if notices.is_empty() {
            return Ok(());
        }
        let k = self.index;
        let Scheduler {
            run,
            cells: shared,
            watcher,
            driver,
            ..
        } = self;
        let post = Mailer::new(run, &watcher.control);
        let here = Here {
            me: k as u32,
            host: run.io_worker(k) as u32,
            mail: &shared.mail,
            post: &post,
        };
        let mut work = IoWork::default();
        // 終わりでは動くタスクがないので、旗を付ける `Task` もない
        take_async(
            &mut shared.lock(),
            notices,
            &here,
            ctx,
            depth_offset,
            out,
            &mut work,
            &mut Vec::new(),
        )?;
        cells::run_io(
            shared,
            &here,
            ctx,
            driver.as_deref_mut(),
            depth_offset,
            &mut work,
            out,
        )?;
        Ok(())
    }

    /// 右の子が終わった join の親が、その join を待っていれば起こす。
    pub(crate) fn wake_parent(&mut self, j: JoinId) {
        let parent = self.join(j).parent;
        if let Some(task) = &self.tasks[parent]
            && matches!(task.state, TaskState::Waiting(w) if w == j)
        {
            self.run.wake_ready(self.index, &mut self.policy, parent);
        }
    }

    /// この `Scheduler` の worker の番号。登録表のセルの持ち主と区画の添字である。
    pub(crate) fn me(&self) -> u32 {
        self.index as u32
    }

    pub(crate) fn add_task(&mut self, task: Task) -> TaskId {
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

    /// 動いているタスクの番号を替える。`current`はここでだけ書く。
    pub(crate) fn set_current(&mut self, id: TaskId) {
        self.current = id;
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

    /// エントリのタスクのほかにタスクか join が残っているか。実行待ちの列の `Entry` もまだ表にないタスクとして数える。
    pub(crate) fn has_children(&self) -> bool {
        self.tasks.iter().any(Option::is_some)
            || self.joins.iter().any(Option::is_some)
            || !self.run.ready_is_empty(self.index)
    }

    pub(crate) fn count_preemption(&mut self) {
        self.preemptions += 1;
    }

    pub(crate) fn preemptions(&self) -> u64 {
        self.preemptions
    }

    /// 区間を閉じる。`Async` のタスクの区間なら、区間の始めからこの worker のヒープが取ったページのバイト数を、
    /// そのタスクの数 (`counted`) とこのエントリの分 (`added`) に足す。`par` の右の子、`compute` の仕事、`main` は
    /// セルを持たないので、どのタスクにも数えない (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn end_slice(&mut self, task: &mut Task) {
        let now = self.watcher.control.taken(self.index);
        if task.cell.is_some() {
            let slice = now.saturating_sub(self.mark);
            task.counted = task.counted.saturating_add(slice);
            task.added = task.added.saturating_add(slice);
        }
        self.mark = now;
    }

    /// 動いているタスク `task` を欄に公開する。動き始めた `Async` のタスクならそのセルと数を、そうでなければ
    /// `NO_CELL` を書く (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn publish(&mut self, task: &Task) {
        match task.cell {
            Some(cell) if !matches!(task.state, TaskState::Entry { .. }) => {
                self.write_published((cell_word(cell), task.counted, self.mark));
            }
            _ => self.unpublish(),
        }
    }

    /// 動いているタスクがないことを欄に公開する。待ちに入るか終わったエントリの `Task` も、切り替えの前の集める地点で
    /// 選ばれないようにここで外す (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn unpublish(&mut self) {
        let (current, bytes, mark) = self.published;
        if current != NO_CELL {
            // `current` が `NO_CELL` の間の `bytes` と `mark` は読まれないので、前の値のままにする
            self.write_published((NO_CELL, bytes, mark));
        }
    }

    /// 欄の `current`、`bytes`、`mark` を版の数 `seq` で囲んで書く。書き手は持ち主だけなので、`seq` は読んで書けば
    /// よい。手元の写しと同じ値は書かない。
    fn write_published(&mut self, want: (u64, usize, usize)) {
        if self.published == want {
            return;
        }
        let slot = &self.run.slots[self.index];
        let seq = slot.seq.load(Ordering::Relaxed);
        slot.seq.store(seq.wrapping_add(1), Ordering::Relaxed);
        // 奇数の版を3つの書き込みより前に見せる。読み手の Acquire の fence と組になる
        fence(Ordering::Release);
        slot.current.store(want.0, Ordering::Relaxed);
        slot.bytes.store(want.1, Ordering::Relaxed);
        slot.mark.store(want.2, Ordering::Relaxed);
        slot.seq.store(seq.wrapping_add(2), Ordering::Release);
        self.published = want;
    }

    /// 今のタスクが切り替えずに動き続ける。区間を閉じてから公開し直す。走れるタスクがなく眠った後で、`par` の join や
    /// 根のスコープの終わりがそのまま届いたときに使う。眠る前に `current` を消したためである
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn keep_running(&mut self, task: &mut Task) {
        self.end_slice(task);
        self.publish(task);
    }

    /// 公開した今のタスクのセルが `cell` なら、その今の数。
    pub(crate) fn published_count(&self, cell: CellRef) -> Option<usize> {
        let (current, bytes, mark) = self.published;
        (current == cell_word(cell)).then(|| {
            let now = self.watcher.control.taken(self.index);
            bytes.saturating_add(now.saturating_sub(mark))
        })
    }

    pub(crate) fn offer_top(&self, cell: CellRef, count: usize) {
        self.run.offer_top(self.index, cell, count);
    }

    pub(crate) fn clear_top(&self, cell: CellRef) {
        self.run.clear_top(self.index, cell);
    }

    /// 待ちに入ったエントリの`Task`が`Finish`した。持ち主の次の集める地点が、このエントリの数`added`をセルに足す。
    /// 自分の再開を積んだタスクは実行待ちにいるので`top`に残し、本当に待つタスクは消す (docs/spec/vm.mdの
    /// 「タスクとスケジュール」)。
    pub(crate) fn left_entry(&mut self, cell: CellRef, added: usize) {
        self.added.push((cell, added));
        if !self.yielded {
            self.clear_top(cell);
        }
    }

    /// 走れるタスクがなく眠る。動いているタスクも実行待ちもないので、どちらも消す (docs/spec/vm.md の
    /// 「タスクとスケジュール」)。
    pub(crate) fn publish_idle(&mut self) {
        self.unpublish();
        self.run.drop_top(self.index);
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
    use crate::cells::{CancelPath, MailItem, Registry, Woken};
    use crate::doorbell::Doorbell;
    use crate::driver::RunClock;
    use eml_rt::Word;
    use eml_sched::config::Seat;

    /// 監視のスレッドでメモリを見るのは `Normal` だけで、決定的な方針は集める地点で見る。
    #[test]
    fn only_normal_watches_memory() {
        assert_eq!(Normal::MEMORY, Memory::Watched);
        assert_eq!(Sequential::MEMORY, Memory::Gathered);
        assert_eq!(Seeded::MEMORY, Memory::Gathered);
    }

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
            Seat::Merged,
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

    /// driverに保留の操作があり、OSの源がなければ、メインのworkerの`Park`の待ちは10 msまでである。源があるか保留が
    /// なければ切らず、それより早い期限はそのまま渡す (docs/spec/vm.mdの「メインのworkerの待ち」)。
    #[test]
    fn main_timeout_caps_a_wait_without_a_source_while_an_op_is_in_flight() {
        use eml_rt::{Done, Op, Submit};
        use std::time::Duration;
        let ms = Duration::from_millis;
        let mut sched = scheduler(DriverKind::Virtual, false, 1);
        assert_eq!(sched.main_timeout(), None);
        sched.own_deadline = Some(30);
        assert_eq!(sched.main_timeout(), Some(ms(30)));
        let driver = sched.driver.as_deref_mut().unwrap();
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 0));
        let Submit::Done(Ok(Done::Listener { handle, .. })) = driver.submit(1, Op::Listen { addr })
        else {
            panic!("the virtual listen ends at submit");
        };
        assert!(matches!(
            driver.submit(2, Op::Accept { listener: handle }),
            Submit::Pending
        ));
        assert_eq!(sched.main_timeout(), Some(ms(10)));
        sched.own_deadline = Some(3);
        assert_eq!(sched.main_timeout(), Some(ms(3)));
        sched.own_deadline = None;
        assert_eq!(sched.main_timeout(), Some(ms(10)));
        #[cfg(unix)]
        {
            sched.poll = Some(PollSource::Fd(0));
            assert_eq!(sched.main_timeout(), None);
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
        assert!(matches!(
            Normal
                .for_worker(2)
                .pick(&mut VecDeque::from([Ready::Task(4)])),
            Some(Ready::Task(4))
        ));
        assert!(matches!(
            Sequential
                .for_worker(2)
                .pick(&mut VecDeque::from([Ready::Task(4)])),
            Some(Ready::Task(4))
        ));
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
            Seat::Merged,
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

    /// worker 0でないworkerもrunの出どころを受けるので、本物のdriverか渡したdriverのrunがテストの根を使うことはない
    /// (docs/spec/vm.mdの「時計と driver」)。
    #[test]
    fn every_worker_takes_the_driver_kind_of_the_run() {
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            ..Limits::default()
        }));
        for (clock, kind, roots) in [
            (
                RunClock::Real {
                    epoch: std::time::Instant::now(),
                },
                DriverKind::Real,
                crate::tls::Roots::Platform,
            ),
            (
                RunClock::Injected,
                DriverKind::Injected,
                crate::tls::Roots::Platform,
            ),
            (
                RunClock::Virtual(Arc::default()),
                DriverKind::Virtual,
                crate::tls::Roots::Fixtures,
            ),
        ] {
            let run = Arc::new(Run::new(2, Seat::Merged, None, clock, Arc::default()));
            let sched = Scheduler::worker(
                Sequential,
                run,
                Arc::clone(&control),
                1,
                0,
                usize::MAX,
                Arc::new(Shared::new(Registry::new(Vec::new(), 2))),
            );
            assert_eq!(sched.driver_kind, kind);
            assert_eq!(crate::tls::Roots::of(sched.driver_kind), roots);
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
                host: 0,
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

    /// worker が2つの run で、worker 0 が driver を持つ host、worker 1 が driver を持たない持ち主である。テストの driver
    /// の仮想のネットワークに接続を1つ作ってあり、持ち主は `near` の端を読む。スレッドは作らず、両方の集める地点を
    /// 手で動かす。
    struct Forwarding {
        control: Arc<Control>,
        run: Arc<Run>,
        shared: Arc<Shared>,
        host: Scheduler<Sequential>,
        host_ctx: Ctx<eml_rt::Checked>,
        owner: Scheduler<Sequential>,
        owner_ctx: Ctx<eml_rt::Checked>,
        /// 持ち主の読む端と、相手の端と、host の driver の bin。
        near: eml_rt::IoHandle,
        far: eml_rt::IoHandle,
        bin: Arc<HostBin>,
    }

    /// 記録の番号と重ならない、テストが driver に直に出す操作の鍵。
    const SETUP_KEY: eml_rt::OpKey = 1 << 40;

    fn forwarding() -> Forwarding {
        use eml_rt::{Done, Op, Submit};
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            ..Limits::default()
        }));
        let mut driver = eml_io::TestDriver::new(None);
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 0));
        let Submit::Done(Ok(Done::Listener {
            handle: listener,
            port,
        })) = driver.submit(SETUP_KEY, Op::Listen { addr })
        else {
            panic!("the virtual listen ends at submit");
        };
        let connect = Op::Connect {
            host: "localhost".to_string(),
            port,
        };
        let Submit::Done(Ok(Done::Stream { handle: near, .. })) =
            driver.submit(SETUP_KEY + 1, connect)
        else {
            panic!("the virtual connect ends at submit");
        };
        let Submit::Done(Ok(Done::Stream { handle: far, .. })) =
            driver.submit(SETUP_KEY + 2, Op::Accept { listener })
        else {
            panic!("the virtual accept ends at submit");
        };
        let bin = Arc::new(HostBin::new(driver.bin(), false));
        let run = Arc::new(Run::new(
            2,
            Seat::Merged,
            None,
            RunClock::Virtual(driver.clock()),
            Arc::default(),
        ));
        let shared = Arc::new(Shared::new(Registry::new(Vec::new(), 2)));
        shared.lock().set_bin(0, Arc::clone(&bin));
        let host = Scheduler::new(
            Sequential,
            Box::new(driver),
            Arc::clone(&run),
            Arc::clone(&control),
            0,
            usize::MAX,
            Arc::clone(&shared),
        );
        let owner = Scheduler::worker(
            Sequential,
            Arc::clone(&run),
            Arc::clone(&control),
            1,
            0,
            usize::MAX,
            Arc::clone(&shared),
        );
        let (out, _) = crate::capture_host(Default::default());
        let host_ctx = Ctx::new(Arc::clone(&control), out.clone());
        let owner_ctx = Ctx::for_worker(Arc::clone(&control), out, 1);
        Forwarding {
            control,
            run,
            shared,
            host,
            host_ctx,
            owner,
            owner_ctx,
            near,
            far,
            bin,
        }
    }

    impl Forwarding {
        /// 持ち主のタスク `me` が、空きのある buffer に `near` の端を読む操作を出して待つ。返すのは token である。
        fn read_and_wait(&mut self, me: CellRef, worker: u32) -> i64 {
            let buf = unsafe { self.owner_ctx.heap_mut() }.alloc_bytes_room(64);
            let stream = cells::stream_on(&mut self.owner_ctx, self.near, Arc::clone(&self.bin));
            let post = Mailer::new(&self.run, &self.control);
            let here = Here {
                me: 1,
                host: 0,
                mail: &self.shared.mail,
                post: &post,
            };
            let token = cells::submit_read(
                &self.shared,
                &here,
                &mut self.owner_ctx,
                None,
                worker,
                0,
                buf,
                stream,
            )
            .unwrap();
            let run = cells::park_io(
                &self.shared,
                &here,
                &mut self.owner_ctx,
                None,
                0,
                token,
                me,
                Word::UNIT,
            )
            .unwrap();
            assert!(run.is_none());
            token
        }

        /// 相手の端から `bytes` を書く。テストの driver は受け付けの時点で書き終える。
        fn write_far(&mut self, bytes: &'static [u8]) {
            let buf = eml_rt::RawBuf {
                ptr: bytes.as_ptr().cast_mut(),
                cap: bytes.len(),
            };
            let op = eml_rt::Op::Write {
                handle: self.far,
                buf,
            };
            let submitted = driver_mut(&mut self.host.driver)
                .unwrap()
                .submit(SETUP_KEY + 3, op);
            assert!(matches!(submitted, eml_rt::Submit::Done(Ok(_))));
        }
    }

    /// worker `k` の受け箱の知らせの種類と記録の番号。取り出した知らせは同じ順に積み直す。
    fn op_notices(run: &Run, control: &Control, k: usize) -> Vec<(&'static str, CellRef)> {
        let notices = run.take_notices(k);
        let kinds = notices
            .iter()
            .map(|notice| match notice {
                Notice::Submit(op) => ("submit", *op),
                Notice::CancelIo(op) => ("cancel_io", *op),
                Notice::Completion(op) => ("completion", *op),
                _ => panic!("a notice other than an op notice"),
            })
            .collect();
        for notice in notices {
            run.send(control, k, notice);
        }
        kinds
    }

    /// worker 1 のタスクの読みは、worker 1 が操作を作って記録に置き、host の worker 0 に `Submit` を送る。worker 0 は
    /// 取り出して自分の driver に出し、完了を記録に置いて、待つタスクの持ち主の worker 1 に `Completion` を送る。結果を
    /// 当てて buffer にバイトを確定し、タスクを再開するのは worker 1 で、worker 0 は記録の語に触れない
    /// (docs/spec/vm.md の「操作の記録」)。
    #[test]
    fn a_forwarded_read_is_submitted_by_the_host_and_settled_by_the_owner() {
        let mut f = forwarding();
        let worker = f.owner_ctx.heap().worker();
        let me = cells::task_on(&f.shared, 1, worker, false);
        let token = f.read_and_wait(me, worker);
        let op = CellRef::decode(token);
        assert_eq!(op_notices(&f.run, &f.control, 0), [("submit", op)]);
        // host が取り出して driver に出す。相手はまだ書いていないので保留になる
        f.host.gather(&mut f.host_ctx, 0, false).unwrap();
        assert!(f.host.runnables.is_empty());
        assert_eq!(f.shared.lock().in_flight(0), 1);
        assert!(cells::arrival_of(&f.shared, token).is_none());
        // 相手が書くと、host の次の集める地点が完了を記録に置き、持ち主へ送る
        f.write_far(b"ab");
        f.host.gather(&mut f.host_ctx, 0, true).unwrap();
        assert!(f.host.runnables.is_empty());
        assert_eq!(f.shared.lock().in_flight(0), 0);
        assert_eq!(cells::arrival_of(&f.shared, token), Some(false));
        assert_eq!(op_notices(&f.run, &f.control, 1), [("completion", op)]);
        // 持ち主が当てて、待っていたタスクを再開する
        f.owner.gather(&mut f.owner_ctx, 0, false).unwrap();
        assert!(matches!(
            f.owner.runnables[..],
            [Runnable::ResumeIo { cell, wake: cells::WOKE, op: o, .. }] if cell == me && o == token
        ));
        f.owner.runnables.clear();
        let buf = cells::take_bytes(&f.shared, &mut f.owner_ctx, token, worker).unwrap();
        assert_eq!(unsafe { &*f.owner_ctx.heap().bytes(buf).unwrap() }, b"ab");
        unsafe { f.owner_ctx.heap_mut().decref(buf) }.unwrap();
        let leaks = f.owner_ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
    }

    /// host が取り出す前に持ち主が操作を取り消すと、持ち主の道は印を立てるだけで `CancelIo` を送らない。host は
    /// 取り出す区間でその印を読み、driver に出した直後に取り消すので、届く結果は取り消しだけで終わったものである
    /// (docs/spec/vm.md の「操作の記録」)。
    #[test]
    fn a_cancel_before_the_host_submits_is_seen_by_the_host() {
        let mut f = forwarding();
        let worker = f.owner_ctx.heap().worker();
        let me = cells::task_on(&f.shared, 1, worker, true);
        // 取り消しの地点で待つので、待ちが取り消しを頼む
        let token = f.read_and_wait(me, worker);
        let op = CellRef::decode(token);
        assert_eq!(f.shared.lock().cancel_io(op, 1).unwrap(), CancelPath::Flag);
        assert_eq!(op_notices(&f.run, &f.control, 0), [("submit", op)]);
        f.host.gather(&mut f.host_ctx, 0, false).unwrap();
        f.host.gather(&mut f.host_ctx, 0, true).unwrap();
        assert_eq!(cells::arrival_of(&f.shared, token), Some(true));
        assert_eq!(op_notices(&f.run, &f.control, 1), [("completion", op)]);
        f.owner.gather(&mut f.owner_ctx, 0, false).unwrap();
        assert!(matches!(
            f.owner.runnables[..],
            [Runnable::ResumeIo { cell, wake: cells::CANCELLED, .. }] if cell == me
        ));
        f.owner.runnables.clear();
        let taken = cells::take_op(&f.shared, &mut f.owner_ctx, token, worker).unwrap();
        assert!(taken.cancelling && taken.result.is_err());
        unsafe { f.owner_ctx.heap_mut().decref(taken.buf.unwrap()) }.unwrap();
        let leaks = f.owner_ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
    }

    /// `op_done` は記録を読むだけで、host が置いた結果を当てない。持ち主が host なら受け付けで終わった設定の操作は
    /// 真で、別の worker に回した操作は、host が結果を置いた後も持ち主が当てるまで偽である。答えは置いた worker だけで
    /// 決まる (docs/spec/vm.md の「操作の記録」)。
    #[test]
    fn op_done_reads_without_settling_a_forwarded_result() {
        let mut f = forwarding();
        let worker = f.owner_ctx.heap().worker();
        cells::task_on(&f.shared, 1, worker, false);
        let stream = cells::stream_on(&mut f.owner_ctx, f.near, Arc::clone(&f.bin));
        let post = Mailer::new(&f.run, &f.control);
        let here = Here {
            me: 1,
            host: 0,
            mail: &f.shared.mail,
            post: &post,
        };
        let token = cells::submit_control(
            &f.shared,
            &here,
            &mut f.owner_ctx,
            None,
            worker,
            0,
            Some(true),
            stream,
        )
        .unwrap();
        f.host.gather(&mut f.host_ctx, 0, false).unwrap();
        assert_eq!(cells::arrival_of(&f.shared, token), Some(false));
        assert!(!f.shared.lock().op_done(token, worker).unwrap());
        // 持ち主の worker の driver に出した操作は、受け付けで終わる
        let host_worker = f.host_ctx.heap().worker();
        cells::task_on(&f.shared, 0, host_worker, false);
        let stream = cells::stream_on(&mut f.host_ctx, f.far, Arc::clone(&f.bin));
        let host_post = Mailer::new(&f.run, &f.control);
        let host_here = Here {
            me: 0,
            host: 0,
            mail: &f.shared.mail,
            post: &host_post,
        };
        let local = cells::submit_control(
            &f.shared,
            &host_here,
            &mut f.host_ctx,
            f.host.driver.as_deref_mut(),
            host_worker,
            0,
            None,
            stream,
        )
        .unwrap();
        assert!(f.shared.lock().op_done(local, host_worker).unwrap());
    }

    /// host でない worker で落としたストリームは host の bin に積まれ、印が立つ。その worker の集める地点が、眠った
    /// host を1回だけ起こし、host の集める地点は、保留の操作がなくても driver に bin を空にさせる。host のスレッドで
    /// 落としたときは印を立てない (docs/spec/vm.md の「時計と driver」)。
    #[test]
    fn a_stream_dropped_off_its_host_wakes_the_host_to_close_it() {
        let mut f = forwarding();
        let raw = Arc::clone(&f.bin.bin);
        // host のスレッドで落としたハンドルには印がない
        let stream = cells::stream_on(&mut f.host_ctx, f.far, Arc::clone(&f.bin));
        unsafe { f.host_ctx.heap_mut().decref(stream) }.unwrap();
        assert!(!f.bin.take_ring());
        raw.dropped.lock().unwrap().clear();
        // テストのスレッドを worker 1 とするため、host のスレッドが別にある bin を作り、run の始めと同じく欄に置く
        let bin = std::thread::spawn(move || Arc::new(HostBin::new(raw, false)))
            .join()
            .unwrap();
        let _ = f.run.slots[0].bin.set(Arc::clone(&bin));
        let stream = cells::stream_on(&mut f.owner_ctx, f.near, Arc::clone(&bin));
        unsafe { f.owner_ctx.heap_mut().decref(stream) }.unwrap();
        assert_eq!(*bin.bin.dropped.lock().unwrap(), [f.near]);
        f.run.park(0);
        assert_eq!(f.run.running.load(Ordering::Acquire), 0);
        f.owner.gather(&mut f.owner_ctx, 0, false).unwrap();
        assert_eq!(f.run.running.load(Ordering::Acquire), 1);
        // 起こした後の集める地点は起こし直さない
        assert!(!bin.take_ring());
        f.host.gather(&mut f.host_ctx, 0, false).unwrap();
        assert!(bin.bin.dropped.lock().unwrap().is_empty());
        assert!(!bin.take_dropped());
        let leaks = f.owner_ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
    }

    /// worker 1 のタスクの読みに host が結果を置き、持ち主が当てる前に run が終わる。結果を置いただけの記録は driver に
    /// 出したままでも置いたままでもないので終わりの確かめを通り、worker 0 の `sweep_all` が、worker 1 のヒープの buffer
    /// とストリームを記録の ID の下で落とす。取り込んだ後はどちらのヒープにも漏れがない (docs/spec/vm.md の
    /// 「誤り、止めること、終わり」)。
    #[test]
    fn an_op_record_left_on_worker_1_is_swept() {
        let mut f = forwarding();
        let worker = unsafe { f.owner_ctx.heap_mut() }.fork_worker();
        cells::task_on(&f.shared, 1, worker, false);
        let post = Mailer::new(&f.run, &f.control);
        let here = Here {
            me: 1,
            host: 0,
            mail: &f.shared.mail,
            post: &post,
        };
        let (shared, near, bin) = (&f.shared, f.near, &f.bin);
        let token = cells::with_worker(&mut f.owner_ctx, worker, 0, |ctx| {
            let buf = unsafe { ctx.heap_mut() }.alloc_bytes_room(64);
            let stream = cells::stream_on(ctx, near, Arc::clone(bin));
            cells::submit_read(shared, &here, ctx, None, worker, 0, buf, stream).unwrap()
        });
        f.host.gather(&mut f.host_ctx, 0, false).unwrap();
        f.write_far(b"ab");
        f.host.gather(&mut f.host_ctx, 0, true).unwrap();
        // 結果は置いたが、持ち主はまだ当てていない
        assert_eq!(cells::arrival_of(&f.shared, token), Some(false));
        assert_eq!(f.shared.lock().in_flight(0), 0);
        assert!(!cells::op_left(&f.shared, token));
        cells::sweep_all(&f.shared, &mut f.host_ctx, 0).unwrap();
        f.owner_ctx.reclaim();
        f.host_ctx.reclaim();
        let leaks = f.owner_ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
        let leaks = f.host_ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
    }

    /// worker 1 が持ち主の、channel の受けで待つタスクに、ほかの worker の送りが起こしを、取り消しが取り消しを送る。
    /// 送りが先に待ちを下ろしたので、持ち主の受け箱に2つがどの順で届いても、集める地点は誤りにせず、タスクを `Woke` で
    /// 再開する (docs/spec/vm.md の「取り消しの届け方」)。
    #[test]
    fn a_channel_wake_and_a_cancel_for_one_task_agree_in_either_order() {
        for wake_first in [true, false] {
            let Worker1 {
                control,
                run,
                shared,
                mut sched,
                mut ctx,
            } = worker_1();
            let c = cells::test_chan(&shared, 0);
            let r = cells::task_on(&shared, 1, 0, false);
            cells::park_receiver(&shared, c, r);
            let mut out = cells::send_now(&shared, c, 4);
            assert_eq!(out.len(), 1);
            let wake = Notice::Wake(out.remove(0));
            cells::flag_cancel(&shared, r);
            let cancel = Notice::Cancel(r);
            let notices = if wake_first {
                [wake, cancel]
            } else {
                [cancel, wake]
            };
            for notice in notices {
                run.send(&control, 1, notice);
            }
            sched.gather(&mut ctx, 0, false).unwrap();
            assert!(
                matches!(
                    sched.runnables[..],
                    [Runnable::Resume { cell, wake: cells::WOKE, .. }] if cell == r
                ),
                "wake first: {wake_first}"
            );
            sched.runnables.clear();
        }
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
            Err(RunFault::Stopped(Stop::Terminated))
        );
        for k in 0..2 {
            assert_ne!(control.worker_bits(k) & Control::TERMINATE, 0, "worker {k}");
        }
    }
}

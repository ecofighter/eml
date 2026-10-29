//! タスクのスケジュール (docs/spec/vm.md の「タスクとスケジュール」)。方針を `Scheduler` の型の引数にするので、
//! 逐次の方針の安全点は、方針を持たない繰り返しと同じ機械語になる。

use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use eml_rt::{Control, IoDriver};

use crate::cells::{CellRef, Registry};
use crate::doorbell::Doorbell;
use crate::monitor::Monitor;
use crate::vm::Task;

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
}

/// seed で決まる方針。擬似乱数は splitmix64 で、予算は 1 から 16 までの安全点である。
#[derive(Debug, Clone)]
pub struct Seeded {
    state: u64,
    /// 次の横取りまでに残る安全点の数。0 のままなら、次の安全点でまた試みる。
    budget: u64,
}

impl Seeded {
    pub fn new(seed: u64) -> Seeded {
        let mut seeded = Seeded {
            state: seed,
            budget: 0,
        };
        seeded.reload();
        seeded
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
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
}

/// join の節点の番号。境目のフレームの slot 1 に生の語で置く。
pub type JoinId = usize;

/// エントリのタスクの番号。
pub(crate) const MAIN: TaskId = 0;

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
    /// `Async` のタスクとスコープのセル。実行ごとに作る (docs/spec/vm.md の「`Async` のタスク」)。
    pub(crate) cells: Registry,
    /// 終わったエントリの `Task` のスタック。次のエントリの `Task` が使い回す。
    pub(crate) stacks: Vec<Vec<eml_rt::Word>>,
    /// 時計と待ち。`Machine` が持つものを run の間だけ預かる (docs/spec/vm.md の「時計と driver」)。
    pub(crate) driver: Box<dyn IoDriver>,
    /// `Machine` の時計が本物か。`with_driver` で差し込んだ driver でも、時計の欄で決める。本物の driver の run だけが
    /// OS のシグナルを待つタスクへ届ける (docs/spec/stdlib.md の「`Std.Signal`」)。
    pub(crate) real_driver: bool,
    /// 監視のスレッドと、それに渡すもの。
    pub(crate) watcher: Watcher,
}

/// `Scheduler` が持つ監視のスレッド。`Scheduler` を捨てるとき (成功でも誤りでも) に止める。`Scheduler` は run の後に
/// driver を取り出すので、`Drop` はこの欄に置く (docs/spec/vm.md の「タスクとスケジュール」)。
pub(crate) struct Watcher {
    control: Arc<Control>,
    doorbell: Option<Arc<Doorbell>>,
    pub(crate) monitor: Option<Monitor>,
    /// 始めるのは1回だけで、始められなかったときも試し直さない。
    tried: bool,
}

impl Drop for Watcher {
    // run の後に待つタスクはいない。最後の集める地点の後に保留になったシグナルは、run が終わることで消費する
    fn drop(&mut self) {
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
        control: Arc<Control>,
        doorbell: Option<Arc<Doorbell>>,
        main_worker: u32,
        max_depth: usize,
        generations: Vec<u32>,
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
            cells: Registry::new(generations),
            stacks: Vec::new(),
            driver,
            real_driver: false,
            watcher: Watcher {
                control,
                doorbell,
                monitor: None,
                tried: false,
            },
        }
    }

    /// 監視のスレッドを1回だけ始める。横取りする方針では2つ目の仕事が走れるようになりうるときに呼ぶので、2つ目の仕事を
    /// 持たない run はスレッドを作らない (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn start_monitor(&mut self) {
        let watcher = &mut self.watcher;
        if std::mem::replace(&mut watcher.tried, true) {
            return;
        }
        watcher.monitor = Monitor::start(
            Arc::clone(&watcher.control),
            Arc::clone(&self.cells.inbox),
            self.driver.waker(),
            watcher.doorbell.clone(),
            P::PREEMPTS,
        );
    }

    /// OS のシグナルを待つタスクへ中継するか。本物の driver の上で、監視のスレッドが動いているときだけである。
    /// 仮想の driver の run では本物のシグナルは既定の動きをする (docs/spec/stdlib.md の「`Std.Signal`」)。
    pub(crate) fn relay_on(&self) -> bool {
        self.real_driver && self.watcher.monitor.is_some()
    }

    /// 見張りの語を、待ち手の列が空でないシグナルに合わせる。`park_signal` の後と集める地点の終わりに呼ぶので、
    /// 中継する間は、待つタスクのいる種類がいつも語に立っている (docs/spec/vm.md の「上限と割り込み」)。
    pub(crate) fn sync_watched(&self) {
        let bits = if self.relay_on() {
            self.cells.signal_bits()
        } else {
            0
        };
        self.watcher.control.set_watched(bits);
    }

    /// 空きのループが OS のシグナルを待つか。待つなら、ほかに進むものがなくてもデッドロックにしない
    /// (docs/spec/vm.md の「時計と driver」)。
    pub(crate) fn listening(&self) -> bool {
        self.relay_on() && self.cells.signal_bits() != 0
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
    pub(crate) fn set_current(&mut self, id: TaskId) {
        self.current = id;
        if P::PREEMPTS
            && let Some(monitor) = &self.watcher.monitor
        {
            monitor.watch.switches.fetch_add(1, Ordering::Relaxed);
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

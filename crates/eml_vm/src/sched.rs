//! タスクのスケジュール (docs/spec/vm.md の「タスクとスケジュール」)。方針を `Scheduler` の型の引数にするので、
//! 逐次の方針の安全点は、方針を持たない繰り返しと同じ機械語になる。

use std::collections::VecDeque;
use std::str::FromStr;

use crate::vm::Task;

/// スケジュールの方針の選び方。CLI の `--sched` と `RunConfig::with_sched` で選ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sched {
    /// 横取りしない。
    #[default]
    Sequential,
    /// seed で決まる擬似乱数で、横取りする安全点と次のタスクを選ぶ。
    Seeded(u64),
}

impl FromStr for Sched {
    type Err = String;

    /// `sequential` か `seeded:<n>` を読む。`n` は `u64` の10進数である。
    fn from_str(text: &str) -> Result<Sched, String> {
        if text == "sequential" {
            return Ok(Sched::Sequential);
        }
        // `u64::from_str` は先頭の `+` も読むので、数字だけの綴りに限る
        text.strip_prefix("seeded:")
            .filter(|seed| seed.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|seed| seed.parse().ok())
            .map(Sched::Seeded)
            .ok_or_else(|| format!("expected `sequential` or `seeded:<n>`, found `{text}`"))
    }
}

/// スケジューラの中のタスクの番号。
pub type TaskId = usize;

/// スケジュールの方針。いつ横取りし、次にどのタスクを動かし、起こしたタスクを列のどこに入れるかを決める。
pub trait Policy: Send {
    /// 安全点ごとに1回呼ぶ。真を返したら、横取りを試みる。
    fn tick(&mut self) -> bool;
    /// 横取りを試みた後に、次の横取りまでの予算を詰め直す。
    fn reload(&mut self);
    /// 走れるタスクの列から、次に動かすタスクを取り出す。
    fn pick(&mut self, ready: &mut VecDeque<TaskId>) -> Option<TaskId>;
    /// 起こしたタスクを、走れるタスクの列に入れる。
    fn wake(&mut self, ready: &mut VecDeque<TaskId>, task: TaskId);
}

/// 横取りしない方針。列の先頭から取り、起こしたタスクも先頭に入れる。
#[derive(Debug, Clone, Copy, Default)]
pub struct Sequential;

impl Policy for Sequential {
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

/// 1回の実行のタスクを方針 `P` で切り替える。`Machine` が実行ごとに作り、実行が終わると捨てる。
pub struct Scheduler<P: Policy> {
    pub(crate) policy: P,
    /// 走れるタスク。
    ready: VecDeque<TaskId>,
    /// 方針が安全点で始めた切り替えの数。
    preemptions: u64,
    /// 終わったタスクと動いていないタスクが使った語の数の最大。
    peak_depth: usize,
}

impl<P: Policy> Scheduler<P> {
    pub(crate) fn new(policy: P) -> Scheduler<P> {
        Scheduler {
            policy,
            ready: VecDeque::new(),
            preemptions: 0,
            peak_depth: 0,
        }
    }

    /// 方針が安全点で横取りを求めた。走れるタスクがなければ、切り替えずに予算を詰め直す。
    #[cold]
    #[inline(never)]
    pub(crate) fn preempt(&mut self) {
        let next = self.policy.pick(&mut self.ready);
        debug_assert!(next.is_none(), "only the main task runs");
        self.policy.reload();
    }

    pub(crate) fn preemptions(&self) -> u64 {
        self.preemptions
    }

    /// タスクのスタックで同時に使った語の数の最大 (docs/spec/vm.md の「`RunStats`」の `peak_stack`)。`current` は
    /// 最後に動いていたタスクである。スタックの長さは縮めないので、タスクの長さがそのままそのタスクの最大である。
    pub(crate) fn peak_stack(&self, current: &Task) -> u64 {
        self.peak_depth.max(current.stack.len()) as u64
    }
}

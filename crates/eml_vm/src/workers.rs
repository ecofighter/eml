//! run ごとに worker の間で共有する表 `Run` と、worker の空きの待ち (docs/spec/vm.md の「タスクとスケジュール」
//! 「時計と driver」)。`Run` が持つのは atomic の数と lock の下の小さな状態だけで、ヒープの語も `Ctx` も持たない。
//! ヒープに触れるのはその持ち主の worker のスレッドだけにするためである (docs/future/multicore.md の「スレッドと worker」)。

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};

use eml_rt::IoWaker;

use crate::VmFault;
use crate::doorbell::Doorbell;
use crate::driver::DriverKind;
use crate::sched::{JoinNode, Outcome, Policy, Scheduler, driver_mut};

// 3つの型はどれも、スレッドをまたいで渡すか共有する (docs/spec/vm.md の「タスクとスケジュール」)
const _: () = {
    const fn sync<T: Sync>() {}
    const fn send<T: Send>() {}
    sync::<Run>();
    send::<JoinNode>();
    send::<Outcome>();
};

/// ほかの worker から受け箱に届く知らせ。今は送る道がないので、種類を持たない。
pub(crate) enum Notice {}

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
    /// 送った子の知らせのうち、終わりの知らせをまだ受け取っていないものの数。
    pub(crate) in_flight: AtomicUsize,
    /// 受け箱が空で待ちに入った worker の番号。重ならない。
    idle: Mutex<Vec<usize>>,
    /// 監視のスレッドの自己パイプ。眠っていた worker が動き始めて `running` が 0 から 1 になったら鳴らし、刻まずに
    /// 眠っていた監視のスレッドを起こす。
    pub(crate) bell: Option<Arc<Doorbell>>,
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
            idle: Mutex::new(Vec::new()),
            bell,
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
}

impl<P: Policy> Scheduler<P> {
    /// worker 0 の空きの待ち。走れるタスクがないときに呼び、起こされるか driver の待ちが戻ったら `Ok` で戻る。呼ぶ側は
    /// 戻るたびに止める語を読んで集め直す。デッドロックを決めるのは、受け箱が空で、期限も保留の操作も送った子もない
    /// ときだけである (docs/spec/vm.md の「タスクとスケジュール」「時計と driver」)。
    pub(crate) fn idle_wait(&mut self) -> Result<(), VmFault> {
        let run = Arc::clone(&self.run);
        let k = self.index;
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
    use std::sync::atomic::Ordering;

    use super::Run;

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

    /// まだ始めていない worker の欄を起こしても、`running` には数えない。
    #[test]
    fn waking_an_unstarted_worker_counts_nothing() {
        let run = Run::new(3, None);
        run.wake_all();
        assert_eq!(run.running.load(Ordering::Acquire), 1);
    }
}

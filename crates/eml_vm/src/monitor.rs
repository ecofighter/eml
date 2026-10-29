//! 監視のスレッド (docs/spec/vm.md の「タスクとスケジュール」)。普通の方針 `Normal` が持ち、刻みの間に動くタスクが
//! 替わらなければ worker の止める語に `PREEMPT` を立てる。スレッドが持つのは `Arc<Control>` と `Arc<Watch>` だけで、
//! ヒープの語も `Ctx` も持たない。ヒープに触れるのは worker のスレッドだけにするためである
//! (docs/future/multicore.md の「スレッドと worker」)。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eml_rt::Control;

/// 監視の刻み。
const TICK: Duration = Duration::from_millis(10);

/// 監視のスレッドのスタックの大きさ。ループは再帰も大きな局所変数も持たない。
const STACK: usize = 64 * 1024;

/// worker と監視のスレッドが共有する欄。どちらも、ほかのメモリへの書き込みを公開する役を持たないので Relaxed で足りる。
/// 止めるときの順序は `unpark` と `join` が与える。
pub(crate) struct Watch {
    /// worker が動くタスクを替えた回数。
    pub(crate) switches: AtomicU64,
    /// 立っていれば、監視のスレッドは次に起きたときに終わる。
    stop: AtomicBool,
}

/// 走っている監視のスレッド。
pub(crate) struct Monitor {
    pub(crate) watch: Arc<Watch>,
    thread: JoinHandle<()>,
}

impl Monitor {
    /// 監視のスレッドを始める。始められなければ `None` を返し、呼ぶ側は監視なしで続ける。そのときの振る舞いは
    /// FIFO の逐次と同じで、誤りにはしない (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn start(control: Arc<Control>) -> Option<Monitor> {
        let watch = Arc::new(Watch {
            switches: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let shared = Arc::clone(&watch);
        let builder = thread::Builder::new()
            .name("eml-monitor".to_string())
            .stack_size(STACK);
        // SIGINT と SIGTERM が監視のスレッドに届くと、VM のスレッドの待っている読みに `EINTR` が来なくなる
        // (docs/spec/vm.md の「上限と割り込み」)
        let thread = eml_io::spawn_masked(builder, move || watch_loop(&control, &shared)).ok()?;
        Some(Monitor { watch, thread })
    }

    /// 監視のスレッドを止めて、終わるまで待つ。
    pub(crate) fn stop(self) {
        self.watch.stop.store(true, Ordering::Relaxed);
        self.thread.thread().unpark();
        // 監視のループは panic しない。panic しても止める側に伝えることはないので、結果は捨てる
        let _ = self.thread.join();
    }
}

/// 刻みごとに、動くタスクが前の刻みから替わったかを決める。時間に依らずにテストするため、ループから分けてある。
struct Ticker {
    last: u64,
}

impl Ticker {
    /// 前の刻みから切り替えがなければ真を返す。
    fn tick(&mut self, switches: u64) -> bool {
        let same = switches == self.last;
        self.last = switches;
        same
    }
}

fn watch_loop(control: &Control, watch: &Watch) {
    let mut ticker = Ticker {
        last: watch.switches.load(Ordering::Relaxed),
    };
    let mut next = Instant::now() + TICK;
    loop {
        thread::park_timeout(next.saturating_duration_since(Instant::now()));
        if watch.stop.load(Ordering::Relaxed) {
            return;
        }
        // `park_timeout` は時間の前にも戻りうる
        if Instant::now() < next {
            continue;
        }
        if ticker.tick(watch.switches.load(Ordering::Relaxed)) {
            control.preempt();
        }
        next += TICK;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eml_rt::{Control, Limits};

    use super::Ticker;
    use crate::sched::{Normal, Policy};

    #[test]
    fn a_tick_without_a_switch_asks_for_a_switch() {
        let mut ticker = Ticker { last: 3 };
        assert!(ticker.tick(3));
        assert!(ticker.tick(3));
    }

    #[test]
    fn a_tick_after_a_switch_does_not() {
        let mut ticker = Ticker { last: 3 };
        assert!(!ticker.tick(4));
        assert!(ticker.tick(4));
    }

    /// `Drop` が `join` で待つので、捨てた後には監視のスレッドが持っていた `Watch` の参照が残らない。
    #[test]
    fn dropping_the_policy_stops_the_monitor() {
        let control = Arc::new(Control::new(Limits::default()));
        let mut normal = Normal::new(Arc::clone(&control));
        normal.task_added();
        normal.task_added();
        let watch = Arc::clone(&normal.monitor.as_ref().expect("the monitor started").watch);
        // 2回目の `task_added` は2本目のスレッドを始めない
        assert_eq!(Arc::strong_count(&watch), 3);
        drop(normal);
        assert_eq!(Arc::strong_count(&watch), 1);
        assert_eq!(Arc::strong_count(&control), 1);
    }
}

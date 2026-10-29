//! 監視のスレッド (docs/spec/vm.md の「タスクとスケジュール」)。`Scheduler` が持つ。保留のシグナルを受け箱へ中継して
//! worker の語に `NOTICE` を立て、driver の waker を鳴らす。横取りする方針では、刻みの間に動くタスクが替わらなければ
//! `PREEMPT` も立てる。スレッドが持つのは `Arc<Control>`、受け箱、waker、自己パイプ、`Arc<Watch>` だけで、ヒープの語も
//! `Ctx` も持たない。ヒープに触れるのは worker のスレッドだけにするためである
//! (docs/future/multicore.md の「スレッドと worker」)。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eml_rt::{Control, IoWaker};

use crate::cells::{Inbox, Notice};
use crate::doorbell::Doorbell;

/// 監視の刻み。自己パイプのない監視は、この刻みでシグナルを見に行く。
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

/// 監視のスレッドが中継に使うもの。
struct Relay {
    control: Arc<Control>,
    inbox: Arc<Inbox>,
    waker: Arc<dyn IoWaker>,
}

/// 走っている監視のスレッド。
pub(crate) struct Monitor {
    pub(crate) watch: Arc<Watch>,
    doorbell: Option<Arc<Doorbell>>,
    thread: JoinHandle<()>,
}

impl Monitor {
    /// 監視のスレッドを始める。`preempt` は刻みごとの横取りを求めるかである。始められなければ `None` を返し、呼ぶ側は
    /// 監視なしで続ける。そのときの振る舞いは FIFO の逐次と同じで、誤りにはしない (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn start(
        control: Arc<Control>,
        inbox: Arc<Inbox>,
        waker: Arc<dyn IoWaker>,
        doorbell: Option<Arc<Doorbell>>,
        preempt: bool,
    ) -> Option<Monitor> {
        let watch = Arc::new(Watch {
            switches: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let shared = Arc::clone(&watch);
        let relay = Relay {
            control,
            inbox,
            waker,
        };
        let bell = doorbell.clone();
        let builder = thread::Builder::new()
            .name("eml-monitor".to_string())
            .stack_size(STACK);
        // SIGINT と SIGTERM が監視のスレッドに届くと、VM のスレッドの待っている読みに `EINTR` が来なくなる
        // (docs/spec/vm.md の「上限と割り込み」)
        let thread = eml_io::spawn_masked(builder, move || {
            watch_loop(&relay, &shared, bell.as_deref(), preempt)
        })
        .ok()?;
        Some(Monitor {
            watch,
            doorbell,
            thread,
        })
    }

    /// 監視のスレッドを止めて、終わるまで待つ。自己パイプで待っていれば鳴らして起こす。
    pub(crate) fn stop(self) {
        self.watch.stop.store(true, Ordering::Relaxed);
        if let Some(doorbell) = &self.doorbell {
            doorbell.ring();
        }
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

impl Relay {
    /// まだ中継していない保留のシグナルを受け箱に積み、`NOTICE` を立てる。知らせを積んでからビットを立てるので、
    /// ビットを見た worker は集める地点で知らせを見つける。止めるビットが立っていても waker を鳴らし、眠っている worker に
    /// 止める語を読ませる (docs/spec/vm.md の「タスクとスケジュール」)。
    fn relay(&self) {
        let new = self.control.claim_signals();
        if new != 0 {
            self.inbox.push(Notice::Signal(new));
            self.control.notice();
        }
        if new != 0 || self.control.stop_bits() != 0 {
            self.waker.wake();
        }
    }
}

fn watch_loop(relay: &Relay, watch: &Watch, doorbell: Option<&Doorbell>, preempt: bool) {
    let mut ticker = Ticker {
        last: watch.switches.load(Ordering::Relaxed),
    };
    let mut next = Instant::now() + TICK;
    loop {
        // 自己パイプがあり横取りもしなければ、鳴るまで眠る。自己パイプがなければ刻みでシグナルを見に行く
        let until_next = next.saturating_duration_since(Instant::now());
        match doorbell {
            Some(doorbell) => doorbell.wait(preempt.then_some(until_next)),
            None => thread::park_timeout(until_next),
        }
        if watch.stop.load(Ordering::Relaxed) {
            return;
        }
        relay.relay();
        // 待ちは時間の前にも戻りうる
        if Instant::now() < next {
            continue;
        }
        if preempt && ticker.tick(watch.switches.load(Ordering::Relaxed)) {
            relay.control.preempt();
        }
        next += TICK;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use eml_rt::{Control, IoWaker, Limits};

    use super::{Relay, Ticker};
    use crate::cells::{Inbox, Notice};
    use crate::sched::{Normal, Scheduler};

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

    /// 鳴らされた回数を数える waker。
    #[derive(Default)]
    struct Counting(AtomicU32);

    impl IoWaker for Counting {
        fn wake(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn relay() -> (Relay, Arc<Counting>) {
        let waker = Arc::new(Counting::default());
        let relay = Relay {
            control: Arc::new(Control::new(Limits::default())),
            inbox: Arc::new(Inbox::default()),
            waker: Arc::clone(&waker) as Arc<dyn IoWaker>,
        };
        (relay, waker)
    }

    /// 中継済みの印があるので、届ける前に何度起きても知らせは1つである。
    #[test]
    fn a_claimed_signal_is_relayed_once() {
        let (relay, waker) = relay();
        relay.control.defer(Control::TERMINATE);
        relay.relay();
        relay.relay();
        let notices = relay.inbox.take();
        assert!(
            matches!(notices[..], [Notice::Signal(Control::TERMINATE)]),
            "{} notices",
            notices.len()
        );
        assert_eq!(relay.control.worker_bits(), Control::NOTICE);
        assert_eq!(waker.0.load(Ordering::Relaxed), 1);
        // 届けるまでは保留のままで、ハンドラは2回目として扱う
        assert_eq!(relay.control.pending_signals(), Control::TERMINATE);
    }

    #[test]
    fn nothing_pending_relays_nothing() {
        let (relay, waker) = relay();
        relay.relay();
        assert!(relay.inbox.take().is_empty());
        assert_eq!(relay.control.worker_bits(), 0);
        assert_eq!(waker.0.load(Ordering::Relaxed), 0);
    }

    /// `Drop` が `join` で待つので、捨てた後には監視のスレッドが持っていた `Watch` の参照が残らない。
    #[test]
    fn dropping_the_scheduler_stops_the_monitor() {
        let control = Arc::new(Control::new(Limits::default()));
        let mut sched = Scheduler::new(
            Normal,
            Box::new(eml_io::TestDriver::new(None)),
            Arc::clone(&control),
            None,
            0,
            usize::MAX,
            Vec::new(),
        );
        sched.start_monitor();
        sched.start_monitor();
        let watch = Arc::clone(
            &sched
                .watcher
                .monitor
                .as_ref()
                .expect("the monitor started")
                .watch,
        );
        // 2回目の `start_monitor` は2本目のスレッドを始めない
        assert_eq!(Arc::strong_count(&watch), 3);
        drop(sched);
        assert_eq!(Arc::strong_count(&watch), 1);
        assert_eq!(Arc::strong_count(&control), 1);
    }
}

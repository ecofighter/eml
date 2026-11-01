//! 監視のスレッド (docs/spec/vm.md の「タスクとスケジュール」)。`Scheduler` が持つ。保留のシグナルを run の mail へ中継して
//! worker 0 の語に `NOTICE` を立て、止めるビットや知らせのときにすべての worker を起こす。横取りする方針では、worker
//! ごとに、刻みの間に動くタスクが替わらなければその worker の語に `PREEMPT` も立てる。スレッドが持つのは
//! `Arc<Control>`、mail を持つ `Shared`、run の表 `Run`、`Arc<Watch>` だけで、ヒープの語も `Ctx` も持たない。ヒープに触れるのは
//! worker のスレッドだけにするためである (docs/future/multicore.md の「スレッドと worker」)。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eml_rt::Control;

use crate::cells::{MailItem, Shared};
use crate::doorbell::Doorbell;
use crate::workers::Run;

/// 監視の刻み。自己パイプのない監視は、この刻みでシグナルと止めるビットを見に行く。
const TICK: Duration = Duration::from_millis(10);

/// 監視のスレッドのスタックの大きさ。ループは再帰も大きな局所変数も持たない。
const STACK: usize = 64 * 1024;

/// 監視のスレッドを止める側と共有する欄。ほかのメモリへの書き込みを公開する役を持たないので Relaxed で足りる。止める
/// ときの順序は `unpark` と `join` が与える。
pub(crate) struct Watch {
    /// 立っていれば、監視のスレッドは次に起きたときに終わる。
    stop: AtomicBool,
}

/// 監視のスレッドが中継に使うもの。
struct Relay {
    control: Arc<Control>,
    /// run の登録表と mail。触れるのは mail だけで、登録表の lock は取らない。
    shared: Arc<Shared>,
    run: Arc<Run>,
}

/// 走っている監視のスレッド。
pub(crate) struct Monitor {
    pub(crate) watch: Arc<Watch>,
    doorbell: Option<Arc<Doorbell>>,
    thread: JoinHandle<()>,
}

impl Monitor {
    /// 監視のスレッドを始める。自己パイプは `run` のものを待つ。`preempt` は刻みごとの横取りを求めるかである。始め
    /// られなければ `None` を返し、呼ぶ側は監視なしで続ける。そのときの振る舞いは FIFO の逐次と同じで、誤りにはしない
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn start(
        control: Arc<Control>,
        cells: Arc<Shared>,
        run: Arc<Run>,
        preempt: bool,
    ) -> Option<Monitor> {
        let watch = Arc::new(Watch {
            stop: AtomicBool::new(false),
        });
        let shared = Arc::clone(&watch);
        let doorbell = run.bell.clone();
        let relay = Relay {
            control,
            shared: cells,
            run,
        };
        let builder = thread::Builder::new()
            .name("eml-monitor".to_string())
            .stack_size(STACK);
        // SIGINT と SIGTERM が監視のスレッドに届くと、VM のスレッドの待っている読みに `EINTR` が来なくなる
        // (docs/spec/vm.md の「上限と割り込み」)
        let thread =
            eml_io::spawn_masked(builder, move || watch_loop(&relay, &shared, preempt)).ok()?;
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
    /// まだ中継していない保留のシグナルを mail に積み、`NOTICE` を立てる。項を積んでからビットを立てるので、
    /// ビットを見た worker は集める地点で項を見つける。止めるビットが立っていてもすべての worker を起こし、眠って
    /// いる worker に止める語を読ませる (docs/spec/vm.md の「タスクとスケジュール」)。
    fn relay(&self) {
        let new = self.control.claim_signals();
        if new != 0 {
            // mail を集めて登録表に届けるのは worker 0 だけである
            self.shared.mail.push(MailItem::Signal(new));
            self.control.notice(0);
        }
        if new != 0 || self.control.stop_bits() != 0 {
            self.run.wake_all();
        }
    }
}

/// 自己パイプで眠るときの待ちの長さ。横取りする方針でも、動いている worker がなければ刻まずに鳴るまで眠る。動き
/// 始めた worker が自己パイプを鳴らすので、刻みを取りこぼさない (docs/spec/vm.md の「タスクとスケジュール」)。
fn bell_timeout(preempt: bool, running: usize, until_next: Duration) -> Option<Duration> {
    (preempt && running > 0).then_some(until_next)
}

fn watch_loop(relay: &Relay, watch: &Watch, preempt: bool) {
    let run = &*relay.run;
    let mut tickers: Vec<Ticker> = run
        .slots
        .iter()
        .map(|slot| Ticker {
            last: slot.switches.load(Ordering::Relaxed),
        })
        .collect();
    let mut next = Instant::now() + TICK;
    loop {
        // 自己パイプがなければ、刻みでシグナルと止めるビットを見に行く。眠る worker を起こせるのは監視のスレッドだけ
        // だからである
        let until_next = next.saturating_duration_since(Instant::now());
        match run.bell.as_deref() {
            // 鳴ったかどうかによらず、保留のシグナルを見に行く
            Some(doorbell) => {
                let timeout =
                    bell_timeout(preempt, run.running.load(Ordering::Acquire), until_next);
                doorbell.wait(timeout);
                // 刻まずに眠った後は、起きた時から刻み直す。遅れた刻みをまとめて取り戻さないためである
                if timeout.is_none() {
                    next = Instant::now() + TICK;
                }
            }
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
        if preempt {
            for (k, (slot, ticker)) in run.slots.iter().zip(&mut tickers).enumerate() {
                if ticker.tick(slot.switches.load(Ordering::Relaxed)) {
                    relay.control.preempt(k);
                }
            }
        }
        next += TICK;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    use eml_rt::{Control, IoWaker, Limits};

    use super::{Monitor, Relay, Ticker, bell_timeout};
    use crate::cells::{MailItem, Registry, Shared};
    use crate::doorbell::Doorbell;
    use crate::sched::{Normal, Scheduler};
    use crate::workers::Run;

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

    /// worker が1つの run の、空の登録表と mail。
    fn shared() -> Arc<Shared> {
        Arc::new(Shared::new(Registry::new(Vec::new(), 1)))
    }

    fn relay() -> (Relay, Arc<Counting>) {
        let waker = Arc::new(Counting::default());
        let run = Run::new(1, None, crate::driver::RunClock::Virtual(Arc::default()));
        let _ = run.slots[0]
            .waker
            .set(Arc::clone(&waker) as Arc<dyn IoWaker>);
        let relay = Relay {
            control: Arc::new(Control::new(Limits::default())),
            shared: shared(),
            run: Arc::new(run),
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
        let notices = relay.shared.mail.take();
        assert!(
            matches!(notices[..], [MailItem::Signal(Control::TERMINATE)]),
            "{} notices",
            notices.len()
        );
        assert_eq!(relay.control.worker_bits(0), Control::NOTICE);
        assert_eq!(waker.0.load(Ordering::Relaxed), 1);
        // 届けるまでは保留のままで、ハンドラは2回目として扱う
        assert_eq!(relay.control.pending_signals(), Control::TERMINATE);
    }

    #[test]
    fn nothing_pending_relays_nothing() {
        let (relay, waker) = relay();
        relay.relay();
        assert!(relay.shared.mail.take().is_empty());
        assert_eq!(relay.control.worker_bits(0), 0);
        assert_eq!(waker.0.load(Ordering::Relaxed), 0);
    }

    /// `Drop` が `join` で待つので、捨てた後には監視のスレッドが持っていた `Watch` の参照が残らない。
    #[test]
    fn dropping_the_scheduler_stops_the_monitor() {
        let control = Arc::new(Control::new(Limits::default()));
        let mut sched = Scheduler::new(
            Normal,
            Box::new(eml_io::TestDriver::new(None)),
            Arc::new(Run::new(
                1,
                None,
                crate::driver::RunClock::Virtual(Arc::default()),
            )),
            Arc::clone(&control),
            0,
            usize::MAX,
            shared(),
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

    #[test]
    fn a_monitor_with_a_doorbell_ticks_only_while_a_worker_runs() {
        let tick = super::TICK;
        assert_eq!(bell_timeout(true, 1, tick), Some(tick));
        assert_eq!(bell_timeout(true, 0, tick), None);
        assert_eq!(bell_timeout(false, 1, tick), None);
    }

    /// 自己パイプのある横取りの監視は、どの worker も動いていない間は刻まず、`PREEMPT` を立てない。worker が動き始めると
    /// 自己パイプが鳴り、刻みを始める。立たないことは 50 ms (5 刻み) で確かめ、立つことは 5 秒まで待つ。
    #[test]
    fn the_monitor_sleeps_while_every_worker_is_parked() {
        let control = Arc::new(Control::new(Limits::default()));
        let run = Arc::new(Run::new(
            1,
            Some(Doorbell::new().unwrap()),
            crate::driver::RunClock::Virtual(Arc::default()),
        ));
        run.park(0);
        let monitor = Monitor::start(Arc::clone(&control), shared(), Arc::clone(&run), true)
            .expect("the monitor started");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(control.worker_bits(0) & Control::PREEMPT, 0);
        run.wake(0);
        let start = Instant::now();
        while control.worker_bits(0) & Control::PREEMPT == 0 {
            assert!(start.elapsed() < Duration::from_secs(5), "no tick");
            std::thread::sleep(Duration::from_millis(1));
        }
        monitor.stop();
    }

    /// 自己パイプのない監視は刻みごとに止めるビットを見て、眠っている worker を起こす。シグナルのハンドラと同じく、
    /// 止めるビットを立てる側は誰も起こさない。
    #[test]
    fn a_monitor_without_a_doorbell_wakes_a_parked_worker_on_a_stop_bit() {
        let control = Arc::new(Control::new(Limits::default()));
        let run = Arc::new(Run::new(
            1,
            None,
            crate::driver::RunClock::Virtual(Arc::default()),
        ));
        run.park(0);
        let (woke, rx) = mpsc::channel();
        let worker = {
            let run = Arc::clone(&run);
            std::thread::spawn(move || {
                run.wait_parked(0);
                woke.send(()).unwrap();
            })
        };
        let monitor = Monitor::start(Arc::clone(&control), shared(), Arc::clone(&run), false)
            .expect("the monitor started");
        control.interrupt();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "not woken");
        worker.join().unwrap();
        monitor.stop();
    }
}

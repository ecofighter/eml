//! 監視のスレッド (docs/spec/vm.mdの「タスクとスケジュール」)。`Scheduler`が持つ。保留のシグナルをrunのmailへ中継して
//! すべてのworkerの語に`NOTICE`を立て、止めるビットや知らせのときにすべてのworkerを起こす。スレッドが持つのは
//! `Arc<Control>`、mailを持つ`Shared`、runの表`Run`、`Arc<Watch>`だけで、ヒープの語も`Ctx`も持たない。ヒープに触れるのは
//! workerのスレッドだけにするためである (docs/future/multicore.mdの「スレッドとworker」)。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
    /// 監視のスレッドを始める。自己パイプは`run`のものを待つ。始められなければ`None`を返し、
    /// 呼ぶ側は監視なしで続ける。そのときの振る舞いはFIFOの逐次と同じで、誤りにはしない
    /// (docs/spec/vm.mdの「タスクとスケジュール」)。
    pub(crate) fn start(
        control: Arc<Control>,
        cells: Arc<Shared>,
        run: Arc<Run>,
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
        let thread = eml_io::spawn_masked(builder, move || watch_loop(&relay, &shared)).ok()?;
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

impl Relay {
    /// まだ中継していない保留のシグナルを mail に積み、すべての worker の語に `NOTICE` を立てる。項を積んでから
    /// ビットを立てるので、ビットを見た worker は集める地点で項を見つける。mail はどの worker が処理してもよいので、
    /// 最初に集めた worker が届ける。止めるビットが立っていてもすべての worker を起こし、眠っている worker に止める語を
    /// 読ませる (docs/spec/vm.md の「タスクとスケジュール」)。
    fn relay(&self) {
        let new = self.control.claim_signals();
        if new != 0 {
            self.shared.mail.push(MailItem::Signal(new));
            for k in 0..self.run.slots.len() {
                self.control.notice(k);
            }
        }
        if new != 0 || self.control.stop_bits() != 0 {
            self.run.wake_all();
        }
    }
}

fn watch_loop(relay: &Relay, watch: &Watch) {
    loop {
        // 自己パイプがなければ、刻みでシグナルと止めるビットを見に行く。眠る worker を起こせるのは監視のスレッドだけ
        // だからである
        match relay.run.bell.as_deref() {
            Some(doorbell) => {
                doorbell.wait(None);
            }
            None => thread::park_timeout(TICK),
        }
        if watch.stop.load(Ordering::Relaxed) {
            return;
        }
        relay.relay();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use eml_rt::{Control, IoWaker, Limits};

    use super::{Monitor, Relay};
    use crate::cells::{MailItem, Registry, Shared};
    use crate::sched::{Normal, Scheduler};
    use crate::workers::Run;
    use eml_sched::config::Seat;

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
        let run = Run::new(
            1,
            Seat::Merged,
            None,
            crate::driver::RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
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
        let notices = relay.shared.mail.take(0);
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
        assert!(relay.shared.mail.take(0).is_empty());
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
                Seat::Merged,
                None,
                crate::driver::RunClock::Virtual(Arc::default()),
                Arc::default(),
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

    /// 自己パイプのない監視は刻みごとに止めるビットを見て、眠っている worker を起こす。シグナルのハンドラと同じく、
    /// 止めるビットを立てる側は誰も起こさない。
    #[test]
    fn a_monitor_without_a_doorbell_wakes_a_parked_worker_on_a_stop_bit() {
        let control = Arc::new(Control::new(Limits::default()));
        let run = Arc::new(Run::new(
            1,
            Seat::Merged,
            None,
            crate::driver::RunClock::Virtual(Arc::default()),
            Arc::default(),
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
        let monitor = Monitor::start(Arc::clone(&control), shared(), Arc::clone(&run))
            .expect("the monitor started");
        control.interrupt();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "not woken");
        worker.join().unwrap();
        monitor.stop();
    }
}

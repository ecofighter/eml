//! 監視のスレッド (docs/spec/vm.md の「タスクとスケジュール」)。`Scheduler` が持つ。保留のシグナルを run の mail へ中継して
//! すべての worker の語に `NOTICE` を立て、止めるビットや知らせのときにすべての worker を起こす。横取りする方針では、worker
//! ごとに、刻みの間に動くタスクが替わらなければその worker の語に `PREEMPT` も立てる。メモリを見張る方針では、確保量が
//! 柔らかい上限の閾値を越えたら、数のいちばん大きいタスクに上限の旗を立てる。スレッドが持つのは
//! `Arc<Control>`、mail を持つ `Shared`、run の表 `Run`、`Arc<Watch>` だけで、ヒープの語も `Ctx` も持たない。ヒープに触れるのは
//! worker のスレッドだけにするためである (docs/future/multicore.md の「スレッドと worker」)。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering, fence};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eml_rt::Control;

use crate::cells::{CellRef, MailItem, Shared};
use crate::doorbell::Doorbell;
use crate::workers::{NO_CELL, Notice, Run};

/// 監視の刻み。自己パイプのない監視は、この刻みでシグナルと止めるビットを見に行く。
const TICK: Duration = Duration::from_millis(10);

/// 監視のスレッドのスタックの大きさ。ループは再帰も大きな局所変数も持たない。
const STACK: usize = 64 * 1024;

/// 切り替えの数がこの刻みの数だけ続けて変わらない worker を、詰まったとみなす。VM のタスクは安全点で切り替わるので、
/// 2刻み変わらないのは extern の中で止まったときだけである (docs/spec/vm.md の「タスクとスケジュール」)。
const STUCK_TICKS: u32 = 2;

/// 目安 `ready_entries + inbox_work` がこの数を超える worker を、切り替えていても過負荷とみなす。
const OVERLOAD: usize = 64;

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
    /// 監視のスレッドを始める。自己パイプは `run` のものを待つ。`preempt` は刻みごとの横取りを求めるか、`rescue` は
    /// 刻みごとに救済の判定をするか、`pressure` は刻みごとにメモリの柔らかい上限を見るかである。始められなければ
    /// `None` を返し、呼ぶ側は監視なしで続ける。そのときの振る舞いは FIFO の逐次と同じで、誤りにはしない。柔らかい
    /// 上限もなくなる (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) fn start(
        control: Arc<Control>,
        cells: Arc<Shared>,
        run: Arc<Run>,
        preempt: bool,
        rescue: bool,
        pressure: bool,
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
        let thread = eml_io::spawn_masked(builder, move || {
            watch_loop(&relay, &shared, preempt, rescue, pressure)
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
    /// 切り替えの数が続けて変わらなかった刻みの数。
    still: u32,
}

impl Ticker {
    /// 前の刻みから切り替えがなければ真を返す。
    fn tick(&mut self, switches: u64) -> bool {
        let same = switches == self.last;
        self.last = switches;
        self.still = if same {
            self.still.saturating_add(1)
        } else {
            0
        };
        same
    }

    fn stuck(&self) -> bool {
        self.still >= STUCK_TICKS
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

    /// 救済を頼む worker を決め、空いている worker に `Rescue` を送る。登録表の lock も欄の lock も取らず、atomic を読むだけ
    /// である。`cursor` から回すので、頼みが先頭の worker に偏らない。空いている worker がなければ、その刻みはやめる
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    fn rescue_tick(&self, tickers: &mut [Ticker], cursor: &mut usize) {
        let run = &*self.run;
        let live = run.live().min(tickers.len());
        for i in 0..live {
            let k = (*cursor + i) % live;
            let slot = &run.slots[k];
            let ready = slot.ready_entries.load(Ordering::Relaxed);
            let inbox = slot.inbox_work.load(Ordering::Relaxed);
            let settled = slot.settled.load(Ordering::Relaxed);
            let hint = ready + inbox;
            if hint == 0 {
                continue;
            }
            // 覚えた3つの数は、`futile` の Release の前に書かれている。Acquire で `futile` を読めば、その値以後が見える
            if slot.futile.load(Ordering::Acquire)
                && (ready, inbox, settled)
                    == (
                        slot.futile_ready.load(Ordering::Relaxed),
                        slot.futile_inbox.load(Ordering::Relaxed),
                        slot.futile_settled.load(Ordering::Relaxed),
                    )
            {
                continue;
            }
            if !(tickers[k].stuck() || hint > OVERLOAD) {
                continue;
            }
            match run.pop_idle_except(k, live) {
                Some(r) => {
                    run.send(&self.control, r, Notice::Rescue { victim: k as u32 });
                    *cursor = k + 1;
                }
                None => break,
            }
        }
    }
}

/// 柔らかい上限の旗を立てるタスクを、worker が公開した値から選ぶ。slot を番号の順に見て、動いているタスク
/// (`current`) と実行待ちのいちばん大きいタスク (`top`) を候補にし、数が前の最良より真に大きく 0 より大きいものだけを
/// 残す。そのため同点なら番号の小さい worker が、同じ worker なら動いているタスクが勝つ。`current`、`bytes`、`mark` は
/// 版の数 `seq` を前後で読み、同じ偶数のときだけ組として使う (docs/spec/vm.md の「タスクとスケジュール」)。
pub(crate) fn choose(run: &Run, taken: impl Fn(usize) -> usize) -> Option<CellRef> {
    let mut best: Option<(usize, u64)> = None;
    let mut offer = |count: usize, word: u64| {
        if count > best.map_or(0, |(most, _)| most) {
            best = Some((count, word));
        }
    };
    for (k, slot) in run.slots.iter().enumerate() {
        let seq = slot.seq.load(Ordering::Acquire);
        if seq % 2 == 0 {
            let current = slot.current.load(Ordering::Relaxed);
            let bytes = slot.bytes.load(Ordering::Relaxed);
            let mark = slot.mark.load(Ordering::Relaxed);
            // 3つの読みを2回目の `seq` の読みより前に置く。書き手の奇数の版の後の Release の fence と組になる
            fence(Ordering::Acquire);
            if current != NO_CELL && slot.seq.load(Ordering::Relaxed) == seq {
                offer(bytes.saturating_add(taken(k).saturating_sub(mark)), current);
            }
        }
        let top = slot.top.load(Ordering::Acquire);
        if top != NO_CELL {
            offer(slot.top_bytes.load(Ordering::Relaxed), top);
        }
    }
    best.map(|(_, word)| CellRef::decode(word as i64))
}

/// 確保量が柔らかい上限の閾値を越えていれば、`choose` で選んだタスクに上限の旗を立てる。候補があれば閾値を進め、
/// 進めた者だけが旗を1回送る。旗は mail に積んでからすべての worker の語に `NOTICE` を立てるので、ビットを見た worker
/// は集める地点で項を見つける。立てたセルを返す。
/// 候補がないとき、監視のスレッド (`retry` が偽) は閾値を進めず、次の刻みでまた探す。決定的な方針の集める地点
/// (`retry` が真) は、旗を送らずに閾値を同じ幅だけ進める。確保器の `NOTICE` は閾値を越える1回の課金でしか立たず、
/// 切り替えないタスクは逐次の方針では集める地点に来ないので、進めなければ後から増え始めたタスクに旗が立たない。
/// 進めれば、次の閾値を越えたページの集める地点でまた探す (docs/spec/vm.md の「上限と割り込み」「タスクの上限」)。
pub(crate) fn pressure_tick(
    control: &Control,
    shared: &Shared,
    run: &Run,
    retry: bool,
) -> Option<CellRef> {
    let (next, bytes) = control.pressure()?;
    let Some(cell) = choose(run, |k| control.taken(k)) else {
        if retry {
            // ほかの worker が先に進めていれば、閾値はもう進んでいる
            control.claim_pressure(next, bytes);
        }
        return None;
    };
    if !control.claim_pressure(next, bytes) {
        return None;
    }
    shared.mail.push(MailItem::Limit(cell));
    for k in 0..run.slots.len() {
        control.notice(k);
    }
    Some(cell)
}

/// 自己パイプで眠るときの待ちの長さ。刻む務め (横取りかメモリの見張り) があっても、動いている worker がなければ刻まずに
/// 鳴るまで眠る。動き始めた worker が自己パイプを鳴らすので、刻みを取りこぼさない。動く worker のない間は確保も
/// ない (docs/spec/vm.md の「タスクとスケジュール」)。
fn bell_timeout(ticks: bool, running: usize, until_next: Duration) -> Option<Duration> {
    (ticks && running > 0).then_some(until_next)
}

fn watch_loop(relay: &Relay, watch: &Watch, preempt: bool, rescue: bool, pressure: bool) {
    let run = &*relay.run;
    let mut tickers: Vec<Ticker> = run
        .slots
        .iter()
        .map(|slot| Ticker {
            last: slot.switches.load(Ordering::Relaxed),
            still: 0,
        })
        .collect();
    let mut cursor = 0;
    let mut next = Instant::now() + TICK;
    loop {
        // 自己パイプがなければ、刻みでシグナルと止めるビットを見に行く。眠る worker を起こせるのは監視のスレッドだけ
        // だからである
        let until_next = next.saturating_duration_since(Instant::now());
        match run.bell.as_deref() {
            // 鳴ったかどうかによらず、保留のシグナルを見に行く
            Some(doorbell) => {
                let timeout = bell_timeout(
                    preempt || pressure,
                    run.running.load(Ordering::Acquire),
                    until_next,
                );
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
        if preempt || rescue {
            for (k, (slot, ticker)) in run.slots.iter().zip(&mut tickers).enumerate() {
                if ticker.tick(slot.switches.load(Ordering::Relaxed)) && preempt {
                    relay.control.preempt(k);
                }
            }
        }
        if rescue && run.live() >= 2 {
            relay.rescue_tick(&mut tickers, &mut cursor);
        }
        if pressure {
            pressure_tick(&relay.control, &relay.shared, run, false);
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

    use eml_rt::{Checked, Ctx, Word};

    use super::{Monitor, OVERLOAD, Relay, Ticker, bell_timeout, choose, pressure_tick};
    use crate::cells::{self, CellRef, MailItem, Registry, Shared};
    use crate::doorbell::Doorbell;
    use crate::sched::{Normal, Scheduler};
    use crate::workers::{NO_CELL, Notice, Run, cell_word};

    #[test]
    fn a_tick_without_a_switch_asks_for_a_switch() {
        let mut ticker = Ticker { last: 3, still: 0 };
        assert!(ticker.tick(3));
        assert!(ticker.tick(3));
    }

    #[test]
    fn a_tick_after_a_switch_does_not() {
        let mut ticker = Ticker { last: 3, still: 0 };
        assert!(!ticker.tick(4));
        assert!(ticker.tick(4));
    }

    #[test]
    fn a_worker_is_stuck_after_two_ticks_without_a_switch() {
        let mut ticker = Ticker { last: 3, still: 0 };
        assert!(ticker.tick(3));
        assert!(!ticker.stuck());
        assert!(ticker.tick(3));
        assert!(ticker.stuck());
        assert!(!ticker.tick(4));
        assert!(!ticker.stuck());
    }

    /// 作った worker が `workers` 個で、全員が詰まっている形の `Relay` と刻み。
    fn rescuer(workers: usize) -> (Relay, Vec<Ticker>) {
        let run = Run::new(
            workers,
            None,
            crate::driver::RunClock::Virtual(Arc::default()),
            Arc::default(),
        );
        run.set_live(workers);
        let limits = Limits {
            workers,
            ..Limits::default()
        };
        let relay = Relay {
            control: Arc::new(Control::new(limits)),
            shared: shared(),
            run: Arc::new(run),
        };
        let tickers = (0..workers).map(|_| Ticker { last: 0, still: 9 }).collect();
        (relay, tickers)
    }

    /// worker `k` が受けた `Rescue` の victim の並び。
    fn rescues_to(relay: &Relay, k: usize) -> Vec<u32> {
        relay
            .run
            .take_notices(k)
            .into_iter()
            .map(|notice| match notice {
                Notice::Rescue { victim } => victim,
                _ => panic!("not a rescue"),
            })
            .collect()
    }

    fn hint(relay: &Relay, k: usize, ready: usize, inbox: usize) {
        relay.run.slots[k]
            .ready_entries
            .store(ready, Ordering::Relaxed);
        relay.run.slots[k]
            .inbox_work
            .store(inbox, Ordering::Relaxed);
    }

    #[test]
    fn a_stuck_worker_without_a_hint_is_left_alone() {
        let (relay, mut tickers) = rescuer(2);
        relay.run.set_idle(&[1]);
        let mut cursor = 0;
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert!(rescues_to(&relay, 1).is_empty());
        assert_eq!(relay.run.pop_idle_except(0, 2), Some(1));
        hint(&relay, 0, 1, 0);
        relay.run.set_idle(&[1]);
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert_eq!(rescues_to(&relay, 1), [0]);
    }

    #[test]
    fn an_overloaded_worker_is_rescued_while_it_switches() {
        let (relay, mut tickers) = rescuer(2);
        relay.run.set_idle(&[1]);
        let mut cursor = 0;
        for switches in 1..=3 {
            tickers[0].tick(switches);
            assert!(!tickers[0].stuck());
            hint(&relay, 0, OVERLOAD - 1, 1);
            relay.rescue_tick(&mut tickers, &mut cursor);
            assert!(rescues_to(&relay, 1).is_empty());
        }
        tickers[0].tick(4);
        assert!(!tickers[0].stuck());
        hint(&relay, 0, OVERLOAD, 1);
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert_eq!(rescues_to(&relay, 1), [0]);
        assert_eq!(cursor, 1);
    }

    #[test]
    fn a_rescue_goes_to_an_idle_worker_other_than_the_victim() {
        let (relay, mut tickers) = rescuer(3);
        hint(&relay, 0, 1, 0);
        let mut cursor = 0;
        relay.run.set_idle(&[0]);
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert!(rescues_to(&relay, 0).is_empty());
        relay.run.set_idle(&[1, 0]);
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert_eq!(rescues_to(&relay, 1), [0]);
        assert!(rescues_to(&relay, 0).is_empty());
        // 取り出した worker は空きから外れ、victim は残る
        assert_eq!(relay.run.pop_idle_except(2, 3), Some(0));
        assert_eq!(relay.run.pop_idle_except(2, 3), None);
    }

    #[test]
    fn without_an_idle_worker_nothing_is_sent() {
        let (relay, mut tickers) = rescuer(3);
        hint(&relay, 0, 1, 0);
        hint(&relay, 1, 1, 0);
        let mut cursor = 2;
        relay.rescue_tick(&mut tickers, &mut cursor);
        for k in 0..3 {
            assert!(rescues_to(&relay, k).is_empty());
        }
        assert_eq!(cursor, 2);
    }

    /// 空振りの印は、3つの数が覚えた値と等しい間だけ効く。
    fn futile(relay: &Relay, k: usize, ready: usize, inbox: usize, settled: u64) {
        let slot = &relay.run.slots[k];
        slot.futile_ready.store(ready, Ordering::Relaxed);
        slot.futile_inbox.store(inbox, Ordering::Relaxed);
        slot.futile_settled.store(settled, Ordering::Relaxed);
        slot.futile.store(true, Ordering::Release);
    }

    #[test]
    fn a_futile_victim_is_skipped_until_its_counts_change() {
        let (relay, mut tickers) = rescuer(2);
        hint(&relay, 0, 2, 0);
        futile(&relay, 0, 2, 0, 0);
        relay.run.set_idle(&[1]);
        let mut cursor = 0;
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert!(rescues_to(&relay, 1).is_empty());
        hint(&relay, 0, 2, 1);
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert_eq!(rescues_to(&relay, 1), [0]);
    }

    #[test]
    fn a_futile_victim_is_retried_after_its_entries_settle() {
        let (relay, mut tickers) = rescuer(2);
        hint(&relay, 0, 2, 0);
        futile(&relay, 0, 2, 0, 0);
        relay.run.set_idle(&[1]);
        let mut cursor = 0;
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert!(rescues_to(&relay, 1).is_empty());
        relay.run.slots[0].settled.fetch_add(1, Ordering::Relaxed);
        relay.rescue_tick(&mut tickers, &mut cursor);
        assert_eq!(rescues_to(&relay, 1), [0]);
    }

    /// extern の中で止まった worker は、切り替えの数が変わらないので、3回の呼び出しのうちに見つかる。
    #[test]
    fn a_blocked_worker_is_rescued_within_three_ticks() {
        let (relay, mut tickers) = rescuer(2);
        tickers[0] = Ticker { last: 5, still: 0 };
        tickers[1] = Ticker { last: 0, still: 0 };
        hint(&relay, 0, 1, 0);
        relay.run.set_idle(&[1]);
        let mut cursor = 0;
        let mut sent = 0;
        for _ in 0..3 {
            // 刻み 0 の前に切り替えが1回あり、その後は止まる
            for (k, ticker) in tickers.iter_mut().enumerate() {
                ticker.tick(if k == 0 { 6 } else { 0 });
            }
            relay.rescue_tick(&mut tickers, &mut cursor);
            sent += rescues_to(&relay, 1).len();
        }
        assert_eq!(sent, 1);
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
        let run = Run::new(
            1,
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

    /// slot が `workers` 個で、何も公開していない run の表。
    fn table(workers: usize) -> Run {
        Run::new(
            workers,
            None,
            crate::driver::RunClock::Virtual(Arc::default()),
            Arc::default(),
        )
    }

    /// worker `k` が動いているタスク `cell` を、数 `bytes` と区間の始め `mark` で公開した形にする。
    fn publish(run: &Run, k: usize, cell: CellRef, bytes: usize, mark: usize) {
        let slot = &run.slots[k];
        slot.bytes.store(bytes, Ordering::Relaxed);
        slot.mark.store(mark, Ordering::Relaxed);
        slot.current.store(cell_word(cell), Ordering::Relaxed);
    }

    fn offer(run: &Run, k: usize, cell: CellRef, count: usize) {
        run.slots[k].top_bytes.store(count, Ordering::Relaxed);
        run.slots[k].top.store(cell_word(cell), Ordering::Relaxed);
    }

    /// 動いているタスクの数は `bytes + taken(k) - mark` で、実行待ちの数は `top_bytes` である。いちばん大きいものを選び、
    /// 同点なら番号の小さい worker、同じ worker なら動いているタスクが勝つ。数が 0 のものと `NO_CELL` は選ばない。
    /// 書いている途中の版 (奇数) の slot は動いているタスクを飛ばす。
    #[test]
    fn choose_picks_the_largest_count() {
        let [a, b, c, d] = [1, 2, 3, 4].map(CellRef::decode);
        let run = table(3);
        assert_eq!(choose(&run, |_| 0), None);
        publish(&run, 0, a, 0, 0);
        offer(&run, 1, b, 0);
        assert_eq!(choose(&run, |_| 0), None);
        publish(&run, 0, a, 100, 50);
        offer(&run, 1, b, 120);
        assert_eq!(choose(&run, |k| [80, 0, 0][k]), Some(a));
        assert_eq!(choose(&run, |k| [60, 0, 0][k]), Some(b));
        // 同点: 番号の小さい worker、同じ worker なら `current`
        assert_eq!(choose(&run, |k| [70, 0, 0][k]), Some(a));
        publish(&run, 2, c, 200, 0);
        offer(&run, 2, d, 200);
        assert_eq!(choose(&run, |_| 0), Some(c));
        offer(&run, 2, d, 201);
        assert_eq!(choose(&run, |_| 0), Some(d));
        run.slots[2].seq.store(1, Ordering::Relaxed);
        offer(&run, 2, d, 0);
        assert_eq!(choose(&run, |_| 0), Some(b));
        run.slots[0].current.store(NO_CELL, Ordering::Relaxed);
        run.slots[1].top.store(NO_CELL, Ordering::Relaxed);
        assert_eq!(choose(&run, |_| 0), None);
    }

    /// 柔らかい上限 64 KiB、硬い上限 64 MiB、worker 2つの `Control` と、worker 0 の検査モードの `Ctx`。
    fn pressured() -> (Arc<Control>, Ctx<Checked>) {
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            max_bytes: 64 << 20,
            soft_bytes: 64 << 10,
            ..Limits::default()
        }));
        let (host, _) = crate::capture_host(Default::default());
        let ctx = Ctx::new(Arc::clone(&control), host);
        (control, ctx)
    }

    /// `bytes` バイトの文字列を確保して持つ。
    fn grow(ctx: &mut Ctx<Checked>, bytes: usize) -> Word {
        ctx.fresh_string(&"x".repeat(bytes))
    }

    fn release(ctx: &mut Ctx<Checked>, words: Vec<Word>) {
        for word in words {
            unsafe { ctx.heap_mut().decref(word) }.unwrap();
        }
    }

    fn limits_in(shared: &Shared) -> Vec<CellRef> {
        shared
            .mail
            .take(0)
            .into_iter()
            .map(|item| match item {
                MailItem::Limit(cell) => cell,
                _ => panic!("not a limit"),
            })
            .collect()
    }

    /// 閾値を越えた後の1回目の呼び出しだけが旗を立てて mail に積み、すべての worker の語に `NOTICE` を立てる。2回目は
    /// 閾値が進んでいるので何もしない。進んだ閾値を越えるとまた立てる。
    #[test]
    fn pressure_tick_flags_once_per_threshold() {
        let (control, mut ctx) = pressured();
        let shared = shared();
        let run = table(2);
        let cell = cells::task_on(&shared, 0, 0, false);
        publish(&run, 0, cell, 0, 0);
        let mut held = vec![grow(&mut ctx, 200 << 10)];
        control.take_notice(0);
        assert_eq!(pressure_tick(&control, &shared, &run, false), Some(cell));
        assert_eq!(limits_in(&shared), [cell]);
        for k in 0..2 {
            assert_eq!(control.worker_bits(k), Control::NOTICE, "worker {k}");
            control.take_notice(k);
        }
        assert_eq!(pressure_tick(&control, &shared, &run, false), None);
        assert!(limits_in(&shared).is_empty());
        assert_eq!(control.worker_bits(1), 0);
        held.push(grow(&mut ctx, 17 << 20));
        assert_eq!(pressure_tick(&control, &shared, &run, false), Some(cell));
        assert_eq!(limits_in(&shared), [cell]);
        release(&mut ctx, held);
    }

    /// 監視のスレッドの呼び出しは候補がなければ閾値を進めないので、候補が公開された後の呼び出しが旗を立てる。
    #[test]
    fn pressure_tick_without_a_candidate_keeps_the_threshold() {
        let (control, mut ctx) = pressured();
        let shared = shared();
        let run = table(2);
        let held = vec![grow(&mut ctx, 200 << 10)];
        control.take_notice(0);
        assert_eq!(pressure_tick(&control, &shared, &run, false), None);
        assert!(limits_in(&shared).is_empty());
        assert_eq!(control.worker_bits(0), 0);
        assert!(control.pressure().is_some());
        let cell = cells::task_on(&shared, 0, 0, false);
        publish(&run, 1, cell, 1, 0);
        assert_eq!(pressure_tick(&control, &shared, &run, false), Some(cell));
        assert_eq!(limits_in(&shared), [cell]);
        release(&mut ctx, held);
    }

    /// 集める地点の呼び出しは、候補がなければ旗を送らずに閾値を進める。公開されたタスクが進んだ閾値を越えるまで確保
    /// すると、次の呼び出しがそのタスクに旗を立てる。
    #[test]
    fn a_gathered_tick_without_a_candidate_moves_the_threshold() {
        let (control, mut ctx) = pressured();
        let shared = shared();
        let run = table(2);
        let mut held = vec![grow(&mut ctx, 200 << 10)];
        control.take_notice(0);
        assert_eq!(pressure_tick(&control, &shared, &run, true), None);
        assert!(limits_in(&shared).is_empty());
        assert_eq!(control.worker_bits(1), 0);
        assert!(control.pressure().is_none(), "the threshold moves");
        let cell = cells::task_on(&shared, 0, 0, false);
        let mark = control.taken(0);
        publish(&run, 0, cell, 0, mark);
        held.push(grow(&mut ctx, 17 << 20));
        assert!(control.pressure().is_some());
        assert_eq!(pressure_tick(&control, &shared, &run, true), Some(cell));
        assert_eq!(limits_in(&shared), [cell]);
        release(&mut ctx, held);
    }

    /// 本番の道: 監視のスレッドが刻みで柔らかい上限を見て、公開されたタスクに旗を立てる。立つことは 5 秒まで待つ。
    #[test]
    fn the_monitor_thread_flags_the_published_task() {
        let tick = super::TICK;
        assert_eq!(bell_timeout(true, 1, tick), Some(tick));
        let control = Arc::new(Control::new(Limits {
            max_bytes: 64 << 20,
            soft_bytes: 64 << 10,
            ..Limits::default()
        }));
        let (host, _) = crate::capture_host(Default::default());
        let mut ctx = Ctx::<Checked>::new(Arc::clone(&control), host);
        let shared = shared();
        let run = Arc::new(table(1));
        let cell = cells::task_on(&shared, 0, 0, false);
        publish(&run, 0, cell, 0, 0);
        let held = vec![grow(&mut ctx, 200 << 10)];
        control.take_notice(0);
        let monitor = Monitor::start(
            Arc::clone(&control),
            Arc::clone(&shared),
            Arc::clone(&run),
            false,
            false,
            true,
        )
        .expect("the monitor started");
        let start = Instant::now();
        let mut flagged = Vec::new();
        while flagged.is_empty() || control.worker_bits(0) & Control::NOTICE == 0 {
            assert!(start.elapsed() < Duration::from_secs(5), "no flag");
            std::thread::sleep(Duration::from_millis(1));
            flagged.extend(limits_in(&shared));
        }
        monitor.stop();
        assert_eq!(flagged, [cell]);
        release(&mut ctx, held);
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
            Arc::default(),
        ));
        run.park(0);
        let monitor = Monitor::start(
            Arc::clone(&control),
            shared(),
            Arc::clone(&run),
            true,
            false,
            false,
        )
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
        let monitor = Monitor::start(
            Arc::clone(&control),
            shared(),
            Arc::clone(&run),
            false,
            false,
            false,
        )
        .expect("the monitor started");
        control.interrupt();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "not woken");
        worker.join().unwrap();
        monitor.stop();
    }
}

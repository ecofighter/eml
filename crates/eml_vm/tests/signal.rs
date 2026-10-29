//! シグナルを届ける道 (docs/spec/vm.md の「タスクとスケジュール」と「上限と割り込み」)。本物の driver の上で流し、
//! テストのスレッドがハンドラの代わりに `Control` を書く。

use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use eml_rt::Checked;
use eml_vm::{Clock, Control, Doorbell, Limits, Machine, RunConfig, Sched, Signal, VirtualSignal};

/// 保留のシグナルは監視のスレッドが中継し、待つタスクがいなければ集める地点で run を止める。自己パイプがないので、
/// 監視のスレッドは刻みで保留を見に行く。
#[test]
fn a_relayed_signal_without_a_waiter_stops_the_run() {
    let program = eml_test_support::lir(
        "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Async.sleep_ms 2000)
",
    );
    let control = Arc::new(Control::new(Limits::default()));
    // `begin_run` はシグナルの語に触れないので、run の前の保留が残る
    control.defer(Control::TERMINATE);
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_sched(Sched::Normal)
        .with_control(Arc::clone(&control));
    // 番犬は失敗の条件にだけ使う。割り込みで止まれば、下の検査が落ちる
    let (done, finished) = mpsc::channel::<()>();
    let watchdog = {
        let control = Arc::clone(&control);
        thread::spawn(move || {
            let start = Instant::now();
            while finished.recv_timeout(Duration::from_millis(1))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                if start.elapsed() >= Duration::from_secs(3) {
                    control.interrupt();
                    return;
                }
            }
        })
    };
    let (host, _) = eml_vm::capture_host(Default::default());
    let result = eml_vm::execute(&program, &config, host);
    drop(done);
    watchdog.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.terminated(), "{error:?}");
}

/// 番犬の締め切り。失敗の条件にだけ使い、時間の上限は確かめない。
const WATCHDOG: Duration = Duration::from_secs(3);

/// `cond` が真になるまで回る。締め切りまでに真にならなければ偽を返す。
fn until(cond: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while !cond() {
        if start.elapsed() >= WATCHDOG {
            return false;
        }
        thread::sleep(Duration::from_millis(1));
    }
    true
}

/// 本物の driver で流した run の結果。
struct Real {
    result: Result<eml_vm::Finished, eml_vm::RuntimeError>,
    stdout: String,
    /// `run` の直前から測った時間。
    elapsed: Duration,
    /// `poke` が最後まで進んだか。
    poked: bool,
    closed: Result<(), eml_vm::RuntimeError>,
}

/// `src` を本物の driver と新しい自己パイプの新しい `Machine` で流す。テストのスレッドとは別のスレッドで、ハンドラの代わりに
/// `poke` が `Control` と自己パイプを書く。`poke` が偽を返すか、run が締め切りまでに終わらなければ、割り込みで run を
/// 止める。そのときは下の検査が落ちる。
fn run_real(
    src: &str,
    sched: Sched,
    signals: Vec<VirtualSignal>,
    poke: fn(&Control, &Doorbell) -> bool,
) -> Real {
    let program = eml_test_support::lir(src);
    let control = Arc::new(Control::new(Limits::default()));
    let doorbell = Doorbell::new().unwrap();
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(Arc::clone(&control), host)
        .with_clock(Clock::Real)
        .with_sched(sched)
        .with_doorbell(Arc::clone(&doorbell))
        .with_signals(signals);
    let loaded = machine.load(&program).unwrap();
    let (done, finished) = mpsc::channel::<()>();
    let poker = thread::spawn(move || {
        let poked = poke(&control, &doorbell);
        if !poked {
            control.interrupt();
            return false;
        }
        let start = Instant::now();
        while finished.recv_timeout(Duration::from_millis(1))
            == Err(mpsc::RecvTimeoutError::Timeout)
        {
            if start.elapsed() >= WATCHDOG {
                control.interrupt();
                return true;
            }
        }
        true
    });
    let start = Instant::now();
    let result = machine.run(loaded.entry);
    let elapsed = start.elapsed();
    drop(done);
    let poked = poker.join().unwrap();
    let closed = machine.close();
    Real {
        result,
        stdout: out.contents(),
        elapsed,
        poked,
        closed,
    }
}

/// `TERMINATE` を待つタスクが現れたら、ハンドラと同じく保留にして自己パイプを鳴らす。
fn terminate_when_watched(control: &Control, doorbell: &Doorbell) -> bool {
    if !until(|| control.watched() & Control::TERMINATE != 0) {
        return false;
    }
    control.defer(Control::TERMINATE);
    doorbell.ring();
    true
}

/// 何もしない。
fn no_poke(_: &Control, _: &Doorbell) -> bool {
    true
}

/// 保留のシグナルは、監視のスレッドの中継を通って待つタスクに届く。監視のスレッドは、横取りしない方針でも
/// `park_signal` が始める (docs/spec/vm.md の「タスクとスケジュール」)。
#[test]
fn a_pending_signal_reaches_a_waiter_through_the_monitor() {
    let src = "\
import Async
import Signal

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    Signal.wait Signal.Terminate
    println \"got\")
  Async.await t)
";
    for sched in [Sched::Sequential, Sched::Normal, Sched::Seeded(1)] {
        let run = run_real(src, sched, Vec::new(), terminate_when_watched);
        assert!(run.poked, "{sched:?}: no task waited");
        assert!(run.result.is_ok(), "{sched:?}: {:?}", run.result);
        assert_eq!(run.stdout, "got\n", "{sched:?}");
        assert!(run.closed.is_ok(), "{sched:?}: {:?}", run.closed);
    }
}

/// シグナルを待つタスクだけが残る run は、本物の driver ではデッドロックにならず、シグナルが来るまで眠る
/// (docs/spec/vm.md の「時計と driver」)。
#[test]
fn a_waiter_keeps_an_otherwise_idle_run_alive() {
    let src = "\
import Async
import Signal

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Signal.wait Signal.Terminate
  println \"got\")
";
    fn poke(control: &Control, doorbell: &Doorbell) -> bool {
        if !until(|| control.watched() & Control::TERMINATE != 0) {
            return false;
        }
        // 見張りの語は `park_signal` の中で立つので、worker が空きのループで眠るまで少し待ってから送る
        thread::sleep(Duration::from_millis(20));
        terminate_when_watched(control, doorbell)
    }
    let run = run_real(src, Sched::Sequential, Vec::new(), poke);
    assert!(run.poked, "no task waited");
    assert!(run.result.is_ok(), "{:?}", run.result);
    assert_eq!(run.stdout, "got\n");
    assert!(run.closed.is_ok(), "{:?}", run.closed);
}

/// 予定の仮想のシグナルは、本物の driver でも時間の輪から満ちて、待つタスクに届く。経過時間は下限だけを確かめる
/// (docs/spec/vm.md の「時間の待ち」)。
#[test]
fn a_scheduled_signal_fires_on_the_real_driver() {
    let src = "\
import Async
import Signal

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Signal.wait Signal.Terminate
  println \"got\")
";
    let signals = vec![VirtualSignal {
        at_ms: 20,
        signal: Signal::Terminate,
    }];
    let run = run_real(src, Sched::Sequential, signals, no_poke);
    assert!(run.result.is_ok(), "{:?}", run.result);
    assert_eq!(run.stdout, "got\n");
    assert!(
        run.elapsed >= Duration::from_millis(20),
        "{:?}",
        run.elapsed
    );
}

/// 待つタスクのいない予定の `Terminate` は、本物の driver でも run を止める。
#[test]
fn an_unheard_scheduled_terminate_stops_the_run() {
    let src = "\
import Async

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Async.sleep_ms 2000)
";
    let signals = vec![VirtualSignal {
        at_ms: 5,
        signal: Signal::Terminate,
    }];
    let run = run_real(src, Sched::Sequential, signals, no_poke);
    let error = run.result.unwrap_err();
    assert!(error.terminated(), "{error:?}");
}

/// 横取りしない方針で yield せずに回る worker も、`NOTICE` で次の安全点にシグナルを集めて届ける。届けた後は待つタスクが
/// いないので、次のシグナルはハンドラが止める語に立て、run が止まる (docs/spec/vm.md の「上限と割り込み」)。
#[test]
fn a_busy_worker_collects_a_signal_and_a_second_one_stops_it() {
    let src = "\
import Async
import Signal

spin : Int -> <Panic> Unit
spin n = spin (n + 1)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.spawn (fn () ->
    Signal.wait Signal.Terminate
    println \"got\")
  Async.yield_now ()
  spin 0)
";
    fn poke(control: &Control, doorbell: &Doorbell) -> bool {
        if !terminate_when_watched(control, doorbell) {
            return false;
        }
        // main が忙しい間に、集める地点が届けて待ち手の列を空にした
        if !until(|| control.watched() == 0) {
            return false;
        }
        // 見張りの語が空のときのハンドラと同じである
        control.terminate();
        true
    }
    let run = run_real(src, Sched::Sequential, Vec::new(), poke);
    assert!(run.poked, "the signal was not collected");
    let error = run.result.unwrap_err();
    assert!(error.terminated(), "{error:?}");
}

/// `with_driver` で渡した driver の run は、時計の欄が `Clock::Real` でも OS のシグナルを届けない。テストの driver は
/// 眠らないので、シグナルだけを待つ run はデッドロックで止まり、回り続けない (docs/spec/vm.md の「時計と driver」)。
#[test]
fn an_injected_driver_does_not_listen_for_os_signals() {
    let program = eml_test_support::lir(
        "\
import Async
import Signal

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Signal.wait Signal.Terminate)
",
    );
    let control = Arc::new(Control::new(Limits::default()));
    let (host, _) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(Arc::clone(&control), host)
        .with_clock(Clock::Real)
        .with_driver(Box::new(eml_io::TestDriver::new(None)));
    let loaded = machine.load(&program).unwrap();
    // 番犬は失敗の条件にだけ使う。回り続ければ割り込みで止まり、下の検査が落ちる
    let (done, finished) = mpsc::channel::<()>();
    let watchdog = thread::spawn(move || {
        let start = Instant::now();
        while finished.recv_timeout(Duration::from_millis(1))
            == Err(mpsc::RecvTimeoutError::Timeout)
        {
            if start.elapsed() >= WATCHDOG {
                control.interrupt();
                return;
            }
        }
    });
    let result = machine.run(loaded.entry);
    drop(done);
    watchdog.join().unwrap();
    let error = result.unwrap_err();
    assert!(matches!(error, eml_vm::RuntimeError::Deadlock), "{error:?}");
}

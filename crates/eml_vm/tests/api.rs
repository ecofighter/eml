//! 公開の API の性質。後の並列 VM のための型の性質 (docs/future/multicore.md の「VM の並列化の備え」の制約1と2) と、
//! 実行の設定 `RunConfig` と、スケジュールの方針 `Sched` と、実行時エラーの `interrupted` と、`exit` が書く終わりの欄である。

use std::sync::Arc;

use eml_rt::{Checked, Fast, Fault};
use eml_vm::{
    Clock, CodeBase, Control, Limits, Machine, Normal, RunConfig, RunStats, RuntimeError, Sched,
    Stop, Task, VmFault,
};

use crate::common::parse;

fn sync<T: Sync>() {}

fn send<T: Send>() {}

fn copy<T: Copy>() {}

#[test]
fn a_code_base_is_shared_and_a_task_moves() {
    sync::<CodeBase<Fast>>();
    sync::<CodeBase<Checked>>();
    send::<Task>();
}

/// 方針の値はスレッドをまたいで読む (docs/spec/vm.md の「タスクとスケジュール」)。
#[test]
fn a_policy_moves_and_a_sched_is_shared() {
    send::<Normal>();
    send::<Sched>();
    sync::<Sched>();
    copy::<Sched>();
}

#[test]
fn the_default_config_is_fast_with_a_fresh_unbounded_control() {
    let config = RunConfig::default();
    assert!(!config.debug_heap);
    assert_eq!(config.control.limits(), Limits::default());
    assert_eq!(config.control.stop_bits(), 0);
    assert_eq!(config.control.bytes(), 0);
    assert!(!Arc::ptr_eq(&config.control, &RunConfig::default().control));
}

#[test]
fn with_debug_heap_sets_the_flag() {
    assert!(RunConfig::default().with_debug_heap(true).debug_heap);
    assert!(
        !RunConfig::default()
            .with_debug_heap(true)
            .with_debug_heap(false)
            .debug_heap
    );
}

#[test]
fn the_default_sched_is_sequential() {
    assert_eq!(RunConfig::default().sched, Sched::Sequential);
    assert_eq!(Sched::default(), Sched::Sequential);
    let config = RunConfig::default().with_sched(Sched::Seeded(3));
    assert_eq!(config.sched, Sched::Seeded(3));
    assert_eq!(
        config.with_sched(Sched::Sequential).sched,
        Sched::Sequential
    );
}

/// ライブラリの既定の時計は、すべてのタスクが待つときにだけ進む仮想の時計である (docs/spec/vm.md の「時計と driver」)。
#[test]
fn the_default_clock_is_virtual() {
    assert_eq!(RunConfig::default().clock, Clock::Virtual);
    assert_eq!(Clock::default(), Clock::Virtual);
    let config = RunConfig::default().with_clock(Clock::Real);
    assert_eq!(config.clock, Clock::Real);
    assert_eq!(config.with_clock(Clock::Virtual).clock, Clock::Virtual);
}

/// CLI の `--sched` が読む綴り (docs/spec/stdlib.md の「起動」)。seed は `u64` の10進数である。
#[test]
fn a_sched_parses_and_rejects() {
    assert_eq!("normal".parse(), Ok(Sched::Normal));
    assert_eq!("sequential".parse(), Ok(Sched::Sequential));
    assert_eq!("seeded:0".parse(), Ok(Sched::Seeded(0)));
    assert_eq!("seeded:2026".parse(), Ok(Sched::Seeded(2026)));
    assert_eq!(
        "seeded:18446744073709551615".parse(),
        Ok(Sched::Seeded(u64::MAX))
    );
    for text in [
        "",
        "bogus",
        "Sequential",
        "Normal",
        " sequential",
        "seeded",
        "seeded:",
        "seeded:-1",
        "seeded:+7",
        "seeded:18446744073709551616",
        "seeded:7x",
        "seeded: 7",
        "rescue-always",
    ] {
        let error = text.parse::<Sched>().unwrap_err();
        assert_eq!(
            error,
            format!("expected `normal`, `sequential` or `seeded:<n>`, found `{text}`")
        );
    }
}

#[test]
fn with_control_replaces_the_control() {
    let limits = Limits {
        max_depth: 1000,
        ..Limits::default()
    };
    let control = Arc::new(Control::new(limits));
    let config = RunConfig::default().with_control(Arc::clone(&control));
    assert!(Arc::ptr_eq(&config.control, &control));
    assert_eq!(config.control.limits(), limits);
}

/// 止める理由のビットは `Control::clear` でしか下りないので、`clone` した設定で続けて流すと前の実行の割り込みで止まる。
/// 実行ごとに新しい設定を作るのはこのためである。
#[test]
fn a_cloned_config_shares_its_control() {
    let config = RunConfig::default().with_debug_heap(true);
    let copy = config.clone();
    assert!(Arc::ptr_eq(&config.control, &copy.control));
    assert!(copy.debug_heap);
    config.control.interrupt();
    assert_eq!(
        copy.control.stop_bits() & Control::INTERRUPT,
        Control::INTERRUPT
    );
}

const COUNT: &str = "\
layout Bool { False, True }
fn main() -> unit {
  tail call count(0)
}
fn count(n.0: int) -> unit {
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 100000)
  switch d.1 Bool { #0 -> b1, #1 -> b2 }
b1:
  let m.2: int = extern \"Prelude.Num Int.+\"(n.0, 1)
  tail call count(m.2)
b2:
  return ()
}
";

/// タスクが1つだけなら、seed の方針の予算が尽きても切り替える先がなく、横取りは起きない。仕事の回数も逐次と同じである。
#[test]
fn a_seeded_run_of_one_task_does_not_preempt() {
    let run = |sched| {
        let config = RunConfig::default().with_debug_heap(true).with_sched(sched);
        let (host, _) = eml_vm::capture_host(Default::default());
        eml_vm::execute(&parse(COUNT), &config, host).unwrap()
    };
    let sequential = run(Sched::Sequential);
    assert_eq!(sequential.preemptions, 0);
    for seed in [1, 2026] {
        assert_eq!(run(Sched::Seeded(seed)), sequential);
    }
}

/// 実行の前に割り込まれた `Control` では、最初の安全点で止まる。
#[test]
fn an_interrupted_control_stops_at_the_first_safepoint() {
    for debug_heap in [true, false] {
        let control = Arc::new(Control::new(Limits::default()));
        control.interrupt();
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_control(control);
        let (host, _) = eml_vm::capture_host(Default::default());
        let error = eml_vm::execute(&parse(COUNT), &config, host).unwrap_err();
        assert_eq!(error.to_string(), "interrupted in `main`");
        assert!(error.interrupted());
    }
}

/// 安全点を通らずに `FileIn.read_all` に着くプログラムは、読みの中で止まる。止めたのは extern なので `Extern` の誤りになる。
#[test]
fn an_interrupt_inside_read_all_is_an_interrupt() {
    let root = std::env::temp_dir().join(format!("eml-vm-read-interrupt-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "abc").unwrap();
    let text = "\
fn main() -> unit {
  let p.0: obj = const \"a.txt\"
  let h.1: obj lin = extern \"Std.FileIn.open\"(p.0)
  let s.2: obj = extern \"Std.FileIn.read_all\"(h.1)
  decref s.2
  let u.3: unit = extern \"Std.FileIn.close\"(h.1)
  return u.3
}
";
    let program = parse(text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    let control = Arc::new(Control::new(Limits::default()));
    control.interrupt();
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(control);
    let (host, _) = eml_vm::capture_host(root.clone());
    let result = eml_vm::execute(&program, &config, host);
    std::fs::remove_dir_all(&root).unwrap();
    let error = result.unwrap_err();
    assert!(
        matches!(
            error,
            RuntimeError::Fault {
                fault: VmFault::Extern(Fault::Stopped(Stop::Interrupted)),
                ..
            }
        ),
        "{error:?}"
    );
    assert!(error.interrupted());
}

fn fault(fault: VmFault) -> RuntimeError {
    RuntimeError::Fault {
        fault,
        function: "f".to_string(),
        at: None,
    }
}

/// 割り込みだけが真になる。上限で止めたことも、終了の求めも、ほかの誤りも、リークも割り込みではない。
#[test]
fn only_an_interrupt_is_interrupted() {
    assert!(fault(VmFault::Stopped(Stop::Interrupted)).interrupted());
    assert!(fault(VmFault::Extern(Fault::Stopped(Stop::Interrupted))).interrupted());
    for stop in [Stop::DepthLimit, Stop::MemoryLimit, Stop::Terminated] {
        assert!(!fault(VmFault::Stopped(stop)).interrupted());
        assert!(!fault(VmFault::Extern(Fault::Stopped(stop))).interrupted());
    }
    assert!(!fault(VmFault::Extern(eml_rt::ArithFault::DivisionByZero.into())).interrupted());
    assert!(!fault(VmFault::Internal("x")).interrupted());
    assert!(!RuntimeError::Leak(vec![("String".to_string(), 1)]).interrupted());
}

/// 終了の求めだけが真になる。安全点で止めたときも、ハンドルの読みの中で止めたときも真で、割り込みや上限では偽である
/// (docs/spec/vm.md の「上限と割り込み」)。
#[test]
fn only_a_terminate_is_terminated() {
    assert!(fault(VmFault::Stopped(Stop::Terminated)).terminated());
    assert!(fault(VmFault::Extern(Fault::Stopped(Stop::Terminated))).terminated());
    for stop in [Stop::DepthLimit, Stop::MemoryLimit, Stop::Interrupted] {
        assert!(!fault(VmFault::Stopped(stop)).terminated());
        assert!(!fault(VmFault::Extern(Fault::Stopped(stop))).terminated());
    }
    assert!(!fault(VmFault::Internal("x")).terminated());
    assert!(!RuntimeError::Leak(vec![("String".to_string(), 1)]).terminated());
}

const EXIT: &str = "\
fn main() -> unit {
  let z.0: unit = extern Prelude.exit(3)
  return z.0
}
";

/// `main` の底は `exit` の終了コードを `Control` の終わりの欄に書き、`Finished.exit` はそこから来る
/// (docs/spec/vm.md の「`exit`」)。
#[test]
fn an_exit_is_written_to_the_end_field() {
    for debug_heap in [true, false] {
        let control = Arc::new(Control::new(Limits::default()));
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_control(Arc::clone(&control));
        let (host, _) = eml_vm::capture_host(Default::default());
        let finished = eml_vm::execute(&parse(EXIT), &config, host).unwrap();
        assert_eq!(finished.exit, Some(3));
        assert_eq!(control.exit_code(), Some(3));
    }
}

/// 終わりの欄は run ごとに開くので、`exit` で終わった `Machine` の次の run は `exit` を報告しない
/// (docs/spec/vm.md の「誤り、止めること、終わり」)。
#[test]
fn a_machine_runs_again_after_an_exit() {
    let quiet = "\
fn second() -> unit {
  return ()
}
";
    let (host, _) = eml_vm::capture_host(Default::default());
    let control = Arc::new(Control::new(Limits::default()));
    let mut machine = Machine::<Checked>::new(Arc::clone(&control), host);
    let first = machine.load(&parse(EXIT)).unwrap();
    let second = machine.load(&parse(quiet)).unwrap();
    assert_eq!(machine.run(first.entry).unwrap().exit, Some(3));
    assert_eq!(machine.run(second.entry).unwrap().exit, None);
    assert_eq!(control.exit_code(), None);
    machine.close().unwrap();
}

/// 本体が `FileIn` を持ち、別のタスクが入れ子の `run` で待つ間に、タスク `t` が `exit 3` を呼ぶ。タスクの底の `exit` は
/// ほかのタスクを動かさずに run を捨て、回復がヒープを入れ替えるので、`close` はリークを報告せず、同じ `Machine` の次の
/// run も動く (docs/spec/vm.md の「誤り、止めること、終わり」)。
#[test]
fn an_exit_from_a_task_recovers_and_reports_the_code() {
    let src = "\
import Async
import FileIn
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let f = FileIn.open \"a.txt\"
    let t = Async.start (fn () ->
      Async.yield_now ()
      println \"exiting\"
      (exit 3 : Unit))
    let u = Async.start (fn () ->
      Async.run (fn () ->
        Async.yield_now ()
        Async.yield_now ()
        println \"never\"))
    Async.await t
    Async.await u
    FileIn.close f)
";
    let quiet = "\
fn second() -> unit {
  return ()
}
";
    let root = std::env::temp_dir().join(format!("eml-vm-task-exit-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "abc").unwrap();
    let (host, out) = eml_vm::capture_host(root.clone());
    let control = Arc::new(Control::new(Limits::default()));
    let mut machine = Machine::<Checked>::new(Arc::clone(&control), host);
    let first = machine.load(&eml_test_support::lir(src)).unwrap();
    let second = machine.load(&parse(quiet)).unwrap();
    let finished = machine.run(first.entry);
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(finished.unwrap().exit, Some(3));
    assert_eq!(machine.run(second.entry).unwrap().exit, None);
    machine.close().unwrap();
    assert_eq!(out.contents(), "exiting\n");
}

/// すべてのタスクが本物の時計で眠る run は、空きのループが毎回割り込みを読むので、別のスレッドの割り込みで止まる
/// (docs/spec/vm.md の「時計と driver」)。2 秒は止まりの見張りで、本物の眠り (100 秒) よりずっと短い。
#[test]
fn an_interrupt_wakes_a_sleeping_worker() {
    use std::time::{Duration, Instant};

    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Async.sleep_ms 100000)
";
    let program = eml_test_support::lir(src);
    let control = Arc::new(Control::new(Limits::default()));
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_sched(Sched::Sequential)
        .with_control(Arc::clone(&control));
    let interrupter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        control.interrupt();
    });
    let (host, _) = eml_vm::capture_host(Default::default());
    let start = Instant::now();
    let result = eml_vm::execute(&program, &config, host);
    let elapsed = start.elapsed();
    interrupter.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
}

/// `wait` の1回の呼び出しの、期限と呼んだときの時刻。
type WaitCall = (Option<u64>, u64);

/// `wait` の呼び出しを `calls` に書き、期限まで自分の時計を進める driver。操作は受け付けない。
#[derive(Default)]
struct DeadlineRecorder {
    now_ms: u64,
    calls: Arc<std::sync::Mutex<Vec<WaitCall>>>,
}

struct NoWaker;

impl eml_rt::IoWaker for NoWaker {
    fn wake(&self) {}
}

impl eml_rt::IoDriver for DeadlineRecorder {
    fn now(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.now_ms)
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> std::io::Result<bool> {
        self.calls.lock().unwrap().push((deadline_ms, self.now_ms));
        match deadline_ms {
            Some(deadline) => {
                self.now_ms = self.now_ms.max(deadline);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn submit(&mut self, _key: eml_rt::OpKey, _op: eml_rt::Op) -> eml_rt::Submit {
        unreachable!("the program submits no ops")
    }

    fn cancel(&mut self, _key: eml_rt::OpKey) {}

    fn completions(&mut self, _out: &mut Vec<eml_rt::Completion>) {}

    fn pending(&self) -> usize {
        0
    }

    fn waker(&self) -> Arc<dyn eml_rt::IoWaker> {
        Arc::new(NoWaker)
    }

    fn bin(&self) -> Arc<eml_rt::HandleBin> {
        Arc::new(eml_rt::HandleBin::default())
    }

    fn shutdown_write(&mut self, _handle: eml_rt::IoHandle) -> std::io::Result<()> {
        Ok(())
    }

    fn set_nodelay(&mut self, _handle: eml_rt::IoHandle, _on: bool) -> std::io::Result<()> {
        Ok(())
    }

    fn close(&mut self, _handle: eml_rt::IoHandle) {}

    fn quiesce(&mut self) {}
}

/// `with_driver` で渡した driver には中継がないので、空きのループは1回の待ちを今から 10 ms 先までに切る
/// (docs/spec/vm.md の「時計と driver」)。driver の時計で確かめ、実時間は測らない。
#[test]
fn an_injected_driver_waits_at_most_10_ms_at_a_time() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Async.sleep_ms 50)
";
    let driver = DeadlineRecorder::default();
    let calls = Arc::clone(&driver.calls);
    let (host, _) = eml_vm::capture_host(Default::default());
    let mut machine =
        Machine::<Checked>::new(RunConfig::default().control, host).with_driver(Box::new(driver));
    let loaded = machine.load(&eml_test_support::lir(src)).unwrap();
    machine
        .run(loaded.entry)
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close().unwrap();
    let calls = calls.lock().unwrap();
    assert!(calls.len() >= 5, "{calls:?}");
    for &(deadline, now) in calls.iter() {
        assert!(deadline.is_some_and(|d| d <= now + 10), "{calls:?}");
    }
}

/// 自己パイプのある本物の driver の run は、タスクが1つでも run の始めから監視のスレッドを持ち、割り込みは自己パイプ
/// から監視のスレッドの waker を通って眠る worker に届く (docs/spec/vm.md の「タスクとスケジュール」)。`cargo test`
/// は1つのプロセスでテストを並べるので、ほかのテストの監視のスレッドを見つけて偽に通りうる。nextest の結果で確かめる。
#[cfg(target_os = "linux")]
#[test]
fn a_real_run_with_a_doorbell_has_a_monitor_from_its_start() {
    use std::time::{Duration, Instant};

    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Async.sleep_ms 100000)
";
    let program = eml_test_support::lir(src);
    let control = Arc::new(Control::new(Limits::default()));
    let doorbell = eml_vm::Doorbell::new().unwrap();
    let runner = {
        let control = Arc::clone(&control);
        let doorbell = Arc::clone(&doorbell);
        std::thread::spawn(move || {
            let (host, _) = eml_vm::capture_host(Default::default());
            let mut machine = Machine::<Checked>::new(control, host)
                .with_clock(Clock::Real)
                .with_sched(Sched::Sequential)
                .with_doorbell(doorbell);
            let loaded = machine.load(&program).unwrap();
            machine.run(loaded.entry)
        })
    };
    let has_monitor = || {
        std::fs::read_dir("/proc/self/task").unwrap().any(|task| {
            std::fs::read_to_string(task.unwrap().path().join("comm"))
                .is_ok_and(|comm| comm.trim_end() == "eml-monitor")
        })
    };
    // 3 秒は見つからなかったときの番犬で、そのときも割り込んで run を終わらせてから落ちる
    let start = Instant::now();
    let found = loop {
        if has_monitor() {
            break true;
        }
        if start.elapsed() >= Duration::from_secs(3) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    // シグナルのハンドラと同じ手順で、止める語を立ててから自己パイプを鳴らす
    control.interrupt();
    doorbell.ring();
    let result = runner.join().unwrap();
    assert!(found, "no eml-monitor thread appeared");
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
}

/// `run_real_on` の結果。出力、run の結果、閉じたときの結果 (検査モードの漏れの検査)、経過である。
#[cfg(unix)]
type RealRun = (
    String,
    Result<eml_vm::Finished, RuntimeError>,
    Result<(), RuntimeError>,
    std::time::Duration,
);

/// 本物の時計の検査モードの `Machine` で `src` を流し、閉じるまでの経過を測る。`interrupt_on` があれば、出力に
/// その文字列が出たときに別のスレッドが割り込む。どの run も、3 秒たって終わらなければ見張りのスレッドが割り込むので、
/// 止まったテストも待ち続けない。
#[cfg(unix)]
fn run_real(src: &str, interrupt_on: Option<&'static str>) -> RealRun {
    run_real_on(src, interrupt_on, None)
}

/// `run_real` を、`driver` があればその driver で流す。
#[cfg(unix)]
fn run_real_on(
    src: &str,
    interrupt_on: Option<&'static str>,
    driver: Option<Box<dyn eml_rt::IoDriver>>,
) -> RealRun {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let program = eml_test_support::lir(src);
    let control = Arc::new(Control::new(Limits::default()));
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(Arc::clone(&control), host).with_clock(Clock::Real);
    if let Some(driver) = driver {
        machine = machine.with_driver(driver);
    }
    let loaded = machine.load(&program).unwrap();
    let (done, finished) = mpsc::channel::<()>();
    let seen = out.clone();
    let start = Instant::now();
    let watchdog = std::thread::spawn(move || {
        while finished.recv_timeout(Duration::from_millis(1))
            == Err(mpsc::RecvTimeoutError::Timeout)
        {
            let marked = interrupt_on.is_some_and(|marker| seen.contents().contains(marker));
            if marked || start.elapsed() >= Duration::from_secs(3) {
                control.interrupt();
                return;
            }
        }
    });
    let result = machine.run(loaded.entry);
    let closed = machine.close();
    let elapsed = start.elapsed();
    drop(done);
    watchdog.join().unwrap();
    (out.contents(), result, closed, elapsed)
}

/// 本物の driver で、`listen` のポート 0 が選んだポートへの `connect` を、別のタスクの `accept` が受ける。2つの
/// ストリームの相手の番地は、互いのポートを持つ (docs/spec/stdlib.md の「`Std.Net`」)。
#[cfg(unix)]
#[test]
fn a_real_connection_is_accepted() {
    let src = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  let c = Net.connect \"127.0.0.1\" port
  let s = Async.await t
  let (a, s) = Net.peer_addr s
  let (b, c) = Net.peer_addr c
  Net.close s
  Net.close c
  println (show port)
  println a
  println b)
";
    let (out, result, closed, _) = run_real(src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    let lines: Vec<&str> = out.lines().collect();
    let [port, accepted, connected] = lines[..] else {
        panic!("{out}");
    };
    assert!(accepted.starts_with("127.0.0.1:"), "{out}");
    assert_eq!(connected, format!("127.0.0.1:{port}"), "{out}");
}

/// 本物の driver の `Net` の頭。`listen` が選んだポートへ `connect` し、別のタスクの `accept` が受けたストリームを `b`、
/// 接続した側を `a` にする。
#[cfg(unix)]
const REAL_CONNECTED: &str = "\
import Async (Async)
import Async
import Bytes
import Byte
import Net
import String
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let u = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  let a = Net.connect \"127.0.0.1\" port
  let b = Async.await u
";

/// 本物の driver の接続で、片方のストリームに書いたバイトを別のタスクが読む (docs/spec/stdlib.md の「`Std.Net`」)。
#[cfg(unix)]
#[test]
fn a_real_connection_carries_bytes_between_tasks() {
    let src = format!(
        "{REAL_CONNECTED}  let t = Async.start (fn () ->
    let (buf, b) = Net.read_chunk (Bytes.of_string \"> \") b
    Net.close b
    String.decode buf)
  let a = Net.write \"hello\" a
  Net.close a
  println (Async.await t))
"
    );
    let (out, result, closed, _) = run_real(&src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "> hello\n");
}

/// `ProbeRecorder` が記録する、driver の呼び出し。
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum DriverCall {
    /// 受け付けた操作の鍵、`Readable` か (偽なら `Read`)、`Submit::Done` を返したか。ほかの操作は記録しない。
    Submit(eml_rt::OpKey, bool, bool),
    Wait,
    /// `completions` が返した完了の鍵。
    Completed(eml_rt::OpKey),
}

/// 本物の driver を包み、読みの受け付けとその結果、`wait` の呼び出し、`completions` が返した鍵を順に記録する driver。
#[cfg(unix)]
struct ProbeRecorder {
    inner: eml_io::RealDriver,
    calls: Arc<std::sync::Mutex<Vec<DriverCall>>>,
}

#[cfg(unix)]
impl eml_rt::IoDriver for ProbeRecorder {
    fn now(&self) -> std::time::Duration {
        self.inner.now()
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> std::io::Result<bool> {
        self.calls.lock().unwrap().push(DriverCall::Wait);
        self.inner.wait(deadline_ms)
    }

    fn submit(&mut self, key: eml_rt::OpKey, op: eml_rt::Op) -> eml_rt::Submit {
        let probe = match op {
            eml_rt::Op::Readable { .. } => Some(true),
            eml_rt::Op::Read { .. } => Some(false),
            _ => None,
        };
        let submit = self.inner.submit(key, op);
        if let Some(probe) = probe {
            let done = matches!(submit, eml_rt::Submit::Done(_));
            let call = DriverCall::Submit(key, probe, done);
            self.calls.lock().unwrap().push(call);
        }
        submit
    }

    fn cancel(&mut self, key: eml_rt::OpKey) {
        self.inner.cancel(key);
    }

    fn completions(&mut self, out: &mut Vec<eml_rt::Completion>) {
        let from = out.len();
        self.inner.completions(out);
        let mut calls = self.calls.lock().unwrap();
        calls.extend(out[from..].iter().map(|c| DriverCall::Completed(c.key)));
    }

    fn pending(&self) -> usize {
        self.inner.pending()
    }

    fn waker(&self) -> Arc<dyn eml_rt::IoWaker> {
        self.inner.waker()
    }

    fn bin(&self) -> Arc<eml_rt::HandleBin> {
        self.inner.bin()
    }

    fn shutdown_write(&mut self, handle: eml_rt::IoHandle) -> std::io::Result<()> {
        self.inner.shutdown_write(handle)
    }

    fn set_nodelay(&mut self, handle: eml_rt::IoHandle, on: bool) -> std::io::Result<()> {
        self.inner.set_nodelay(handle, on)
    }

    fn close(&mut self, handle: eml_rt::IoHandle) {
        self.inner.close(handle);
    }

    fn quiesce(&mut self) {
        self.inner.quiesce();
    }
}

/// 本物の driver で、読めるようになるのを待つ操作が終わった後 (受け付けか、その鍵の完了で) に同じ鍵で出す読みは、
/// 受け付けで終わり、その後に driver を待たない。OS が約束しない「書いた直後の読みがその場で終わる」ことは確かめない
/// (docs/spec/vm.md の「時計と driver」)。`with_driver` の driver には中継がないが、この run は空きのループに入る前に
/// 終わる。
#[cfg(unix)]
#[test]
fn a_real_read_after_a_finished_probe_does_not_wait() {
    let src = format!(
        "{REAL_CONNECTED}  let a = Net.write \"hello\" a
  let (buf, b) = Net.read_chunk (Bytes.empty ()) b
  Net.close a
  Net.close b
  println (String.decode buf))
"
    );
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let driver = ProbeRecorder {
        inner: eml_io::RealDriver::new().unwrap(),
        calls: Arc::clone(&calls),
    };
    let (out, result, closed, _) = run_real_on(&src, None, Some(Box::new(driver)));
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert!(out.ends_with("hello\n"), "{out}");
    let calls = calls.lock().unwrap();
    let probe = calls
        .iter()
        .position(|c| matches!(c, DriverCall::Submit(_, true, _)))
        .unwrap_or_else(|| panic!("{calls:?}"));
    let DriverCall::Submit(key, _, done) = calls[probe] else {
        unreachable!("found above");
    };
    // 待つ操作が受け付けで終わらなければ、その鍵の完了が読みより前に届いている
    let read = calls
        .iter()
        .position(|&c| matches!(c, DriverCall::Submit(k, false, _) if k == key))
        .unwrap_or_else(|| panic!("{calls:?}"));
    let completed = calls[probe..read].contains(&DriverCall::Completed(key));
    assert!(done || completed, "{calls:?}");
    assert_eq!(
        calls[read],
        DriverCall::Submit(key, false, true),
        "{calls:?}"
    );
    assert!(!calls[read..].contains(&DriverCall::Wait), "{calls:?}");
}

/// 本物の driver で、データのないストリームの `read_chunk_within` は期限で `None` を返し、ストリームは続けて使える。後で
/// 相手のタスクが書いたバイトは、次の `read_chunk` が返す (docs/spec/stdlib.md の「`Std.Net`」)。
#[cfg(unix)]
#[test]
fn a_real_read_chunk_within_keeps_the_stream() {
    let src = format!(
        "{REAL_CONNECTED}  let (r, b) = Net.read_chunk_within 20 (Bytes.empty ()) b
  match r with
    | Some _ -> println \"Some\"
    | None -> println \"None\"
  let t = Async.start (fn () -> Net.close (Net.write \"later\" a))
  let (buf, b) = Net.read_chunk (Bytes.empty ()) b
  Net.close b
  Async.await t
  println (String.decode buf))
"
    );
    let (out, result, closed, elapsed) = run_real(&src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "None\nlater\n");
    assert!(
        elapsed >= std::time::Duration::from_millis(20),
        "{elapsed:?}"
    );
}

/// 本物の driver の `read_line` は、間を置いた2回の書きにまたがる行を1行にし、1回の書きに入った2行を分け、改行のない
/// 最後の行と EOF の `None` を返す。2本目の接続では、`read_line` が読みすぎたバイトを次の `read_chunk` が返す
/// (docs/spec/stdlib.md の「`Std.Net`」)。
#[cfg(unix)]
#[test]
fn a_real_read_line_reads_lines_split_across_writes() {
    let src = format!(
        "{REAL_CONNECTED}  let t = Async.start (fn () ->
    let a = Net.write \"ab\" a
    Async.sleep_ms 20
    let a = Net.write \"c\\nd\\n\" a
    Net.close (Net.write \"tail\" a))
  let b = lines b
  Async.await t
  let (port, l) = Net.local_port (Net.listen \"127.0.0.1\" 0)
  let u = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  let c = Net.connect \"127.0.0.1\" port
  let d = Async.await u
  let c = Net.write \"x\\nyz\" c
  let (r, d) = Net.read_line d
  println (show r)
  let (buf, d) = Net.read_chunk (Bytes.empty ()) d
  println (String.decode buf)
  Net.close b
  Net.close c
  Net.close d)

lines : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
lines s =
  let (r, s) = Net.read_line s
  println (show r)
  match r with
    | Some _ -> lines s
    | None -> s
"
    );
    let (out, result, closed, _) = run_real(&src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        out,
        "Some \"abc\"\nSome \"d\"\nSome \"tail\"\nNone\nSome \"x\"\nyz\n"
    );
}

/// 本物の driver の 8 MiB の `write_bytes` は、送りの buffer を超えて短く終わる送りを driver が出し直し、1つの完了で
/// 終わる。相手のタスクが EOF まで読んだバイトの数は、書いた数と同じである (docs/spec/vm.md の「時計と driver」)。
#[cfg(unix)]
#[test]
fn a_real_write_of_8_mib_arrives_whole() {
    let src = format!(
        "{REAL_CONNECTED}  let t = Async.start (fn () -> count 0 b)
  let (_, a) = Net.write_bytes (Bytes.make 8388608 (Byte.wrap 7)) a
  Net.close a
  println (show (Async.await t)))

count : Int -> Net.TcpStream -> <IO, Panic, Async> Int
count n s =
  let (buf, s) = Net.read_chunk (Bytes.empty ()) s
  let k = Bytes.length buf
  if k == 0 then
    Net.close s
    n
  else count (n + k) s
"
    );
    let (out, result, closed, _) = run_real(&src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "8388608\n");
}

/// 接続の来ないリスナーで受けを待つ run は、空きのループが割り込みを読むので、別のスレッドの割り込みで止まる。止める
/// ときに受けの操作を取り消して待ちきるので、検査モードの漏れの検査も通る (docs/spec/vm.md の「時計と driver」)。
/// 割り込みは `waiting` が出てから送るので、受けの待ちの間に届く。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_run_blocked_in_a_real_accept() {
    let src = "\
import Async
import Net
import Stdout
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  Async.yield_now ()
  println \"waiting\"
  Stdout.close (Stdout.flush (Stdout.open ()))
  Net.close (Async.await t))
";
    let (out, result, closed, _) = run_real(src, Some("waiting"));
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "waiting\n");
}

/// 10 秒戻らない名前の解決。名前の解決を待たずに手放す道を確かめるので、戻る前にテストが終わる。
#[cfg(unix)]
fn block_10_s(_host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
    std::thread::sleep(std::time::Duration::from_secs(10));
    Ok(vec![std::net::SocketAddr::from(([127, 0, 0, 1], port))])
}

/// 名前の解決を待つ run も、割り込みで止まる。止めるときの `quiesce` は解決を手放し、終わりを待たない
/// (docs/spec/vm.md の「時計と driver」)。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_run_waiting_on_a_lookup() {
    let src = "\
import Async
import Net
import Stdout
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () -> Net.connect \"slow.test\" 80)
  Async.yield_now ()
  println \"waiting\"
  Stdout.close (Stdout.flush (Stdout.open ()))
  Net.close (Async.await t))
";
    let driver = eml_io::RealDriver::with_lookup(block_10_s).unwrap();
    let (out, result, closed, _) = run_real_on(src, Some("waiting"), Some(Box::new(driver)));
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "waiting\n");
}

/// 名前の解決を待つ `connect` を `timeout` で囲むと、期限の取り消しが解決を手放し、`timeout` は `None` を返す
/// (docs/spec/stdlib.md の「`Std.Net`」)。
#[cfg(unix)]
#[test]
fn a_timeout_around_a_slow_lookup_returns_none() {
    let src = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match Async.timeout 20 (fn () -> Net.connect \"slow.test\" 80) with
    | Some s ->
      Net.close s
      println \"Some\"
    | None -> println \"None\")
";
    let driver = eml_io::RealDriver::with_lookup(block_10_s).unwrap();
    let (out, result, closed, _) = run_real_on(src, None, Some(Box::new(driver)));
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "None\n");
}

#[test]
fn the_stats_fields_follow_the_declaration() {
    let stats = RunStats::default();
    // Debug の `RunStats { a: 0, b: 0, ... }` から欄の名前を宣言の順に取る
    let debug = format!("{stats:?}");
    let inner = debug
        .strip_prefix("RunStats { ")
        .unwrap()
        .strip_suffix(" }")
        .unwrap();
    let declared: Vec<&str> = inner
        .split(", ")
        .map(|field| field.split(':').next().unwrap())
        .collect();
    let named: Vec<&str> = stats.fields().iter().map(|(name, _)| *name).collect();
    assert_eq!(named, declared);
}

//! メインのworkerに固定したタスク (`Async.start_on_main`、`Async.run_on_main`)。分かれたrunでは`Machine`を持つスレッド
//! で動き、ほかのworkerへ動かない。`par`の子と`compute`の仕事はほかと同じく置く (docs/spec/vm.mdの「メインのworker」、
//! docs/spec/stdlib.mdの「メインのworker」)。

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;
use std::time::Duration;

use eml_rt::{Checked, IoWaker};
use eml_vm::{
    Clock, Control, DefaultPark, Finished, Limits, Machine, Park, ParkFactory, Pending, Progress,
    RunConfig, RuntimeError,
};

use crate::common::RecordingHost;

fn limits(workers: usize) -> Arc<Control> {
    Arc::new(Control::new(Limits {
        workers,
        ..Limits::default()
    }))
}

/// 検査モードで`config`のrunを`execute`で1回行う。書いた行と、行ごとに書いたスレッドを返す。
fn run_with(
    src: &str,
    config: RunConfig,
) -> (Vec<(String, ThreadId)>, Result<Finished, RuntimeError>) {
    let (host, _) = RecordingHost::new(None);
    let result = eml_vm::execute(
        &eml_test_support::lir(src),
        &eml_vm::Linked::default(),
        &config.with_debug_heap(true),
        host.host_ref(),
    );
    (host.lines(), result)
}

/// 仮想の時計で、workerを`workers`個持てるrunを1回行う。
fn run(src: &str, workers: usize) -> (Vec<(String, ThreadId)>, Result<Finished, RuntimeError>) {
    run_with(src, RunConfig::default().with_control(limits(workers)))
}

/// 本物の時計で、workerを1つ持てるrunを1回行う。
fn run_real(src: &str) -> (Vec<(String, ThreadId)>, Result<Finished, RuntimeError>) {
    run_with(
        src,
        RunConfig::default()
            .with_clock(Clock::Real)
            .with_control(limits(1)),
    )
}

fn texts(lines: &[(String, ThreadId)]) -> Vec<&str> {
    lines.iter().map(|(line, _)| line.as_str()).collect()
}

/// 行`text`を書いたスレッド。
fn thread_of(lines: &[(String, ThreadId)], text: &str) -> ThreadId {
    lines
        .iter()
        .find(|(line, _)| line == text)
        .unwrap_or_else(|| panic!("no line {text:?} in {lines:?}"))
        .1
}

fn finished(result: Result<Finished, RuntimeError>) -> Finished {
    result.unwrap_or_else(|error| panic!("{error}"))
}

/// 固定したタスクは`execute`を呼んだスレッドで動き、`main`はworker 0のスレッドで動く。
#[test]
fn run_on_main_runs_on_the_caller_thread() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () -> println \"pinned\")
  println \"main\")
";
    let (lines, result) = run(src, 1);
    let finished = finished(result);
    assert_eq!(texts(&lines), ["pinned", "main"]);
    let me = std::thread::current().id();
    assert_eq!(thread_of(&lines, "pinned"), me);
    assert_ne!(thread_of(&lines, "main"), me);
    assert_eq!(finished.pinned_tasks, 1);
}

/// 固定したタスクが始めるタスクも、入れ子の`Async.run`の根とその子も固定する。どれもメインのworkerで動く。
#[test]
fn children_of_a_pinned_task_stay_on_the_main_worker() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    let a = Async.start (fn () -> println \"child\")
    Async.await a
    let n = Async.run (fn () ->
      let b = Async.start (fn () -> println \"grandchild\")
      Async.await b
      1)
    println \"nested \\{show n}\")
  println \"main\")
";
    let (lines, result) = run(src, 2);
    let finished = finished(result);
    assert_eq!(texts(&lines), ["child", "grandchild", "nested 1", "main"]);
    let me = std::thread::current().id();
    for text in ["child", "grandchild", "nested 1"] {
        assert_eq!(thread_of(&lines, text), me, "{text}");
    }
    assert_ne!(thread_of(&lines, "main"), me);
    assert_eq!(finished.pinned_tasks, 4);
}

/// 固定したタスクの`par`の右の子は、空いたworkerに置く。worker 1は始めから空いている。
#[test]
fn par_in_a_pinned_task_is_placed_on_a_worker() {
    let src = "\
import Std.Par
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = Async.run_on_main (fn () ->
    let (a, b) = Par.par (fn () -> 1) (fn () -> 2)
    a + b)
  println (show n))
";
    let (lines, result) = run(src, 2);
    let finished = finished(result);
    assert_eq!(texts(&lines), ["3"]);
    assert!(finished.placed >= 1, "{finished:?}");
}

/// 固定したタスクの`compute`の仕事も、空いたworkerに置く。`main`は待っていて、worker 1は始めから空いている。
#[test]
fn compute_in_a_pinned_task_is_placed_on_a_worker() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = Async.run_on_main (fn () -> Async.compute (fn () -> 20 + 1))
  println (show n))
";
    let (lines, result) = run(src, 2);
    let finished = finished(result);
    assert_eq!(texts(&lines), ["21"]);
    assert_eq!(finished.placed_jobs, 1, "{finished:?}");
}

/// 固定したタスクの`exit`は、ほかのworkerのタスクと同じくrunを捨て、runはその終了コードで終わる。
#[test]
fn exit_in_a_pinned_task_ends_the_run() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let p = Async.start_on_main (fn () -> (exit 5 : Unit))
  Async.await p
  println \"unreachable\")
";
    let (lines, result) = run(src, 1);
    assert_eq!(texts(&lines), Vec::<&str>::new());
    assert_eq!(finished(result).exit, Some(5));
}

/// 固定したタスクが眠っている間の`main`の`exit`も、runを終わらせる。メインのworkerは`END`で起きて抜ける。
#[test]
fn exit_in_main_ends_a_run_with_a_sleeping_pinned_task() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let p = Async.start_on_main (fn () ->
    Async.sleep_ms 1000
    println \"late\")
  Async.sleep_ms 1
  (exit 4 : Unit)
  Async.await p)
";
    let (lines, result) = run(src, 1);
    assert_eq!(texts(&lines), Vec::<&str>::new());
    assert_eq!(finished(result).exit, Some(4));
}

/// worker 1の致命的な誤り (`compute`の仕事の深さの上限) も、固定したタスクが眠っているrunを終わらせる。誤りは
/// その誤りのままで、メインのworkerのヒープを畳み込んでも検査モードの確かめが別の誤りに替えない。
#[test]
fn a_fatal_error_on_worker_1_ends_a_run_with_a_sleeping_pinned_task() {
    let src = "\
import Async
deep : Int -> <Panic> Int
deep n = if n == 0 then 0 else 1 + deep (n - 1)
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let p = Async.start_on_main (fn () ->
    Async.sleep_ms 1000
    println \"late\")
  Async.sleep_ms 1
  println (show (Async.compute (fn () -> deep 100000)))
  Async.await p)
";
    let control = Arc::new(Control::new(Limits {
        workers: 2,
        max_depth: 4096,
        ..Limits::default()
    }));
    let (lines, result) = run_with(src, RunConfig::default().with_control(control));
    assert_eq!(texts(&lines), Vec::<&str>::new());
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("the call depth limit is exceeded"),
        "{error}"
    );
}

/// 合わせた`Machine`ではworker 0がメインのworkerを兼ねるので、固定したタスクも`main`も呼んだスレッドで動く。
#[test]
fn run_on_main_in_a_merged_machine_runs_on_worker_0() {
    let program = eml_test_support::lir(
        "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () -> println \"pinned\")
  println \"main\")
",
    );
    let (host, _) = RecordingHost::new(None);
    let mut machine = Machine::<Checked>::new(limits(2), host.host_ref());
    let entry = machine
        .load(&program, &eml_vm::Linked::default())
        .unwrap()
        .entry;
    let finished = finished(machine.run(entry));
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    let lines = host.lines();
    assert_eq!(texts(&lines), ["pinned", "main"]);
    let me = std::thread::current().id();
    assert!(lines.iter().all(|(_, thread)| *thread == me), "{lines:?}");
    assert_eq!(finished.pinned_tasks, 1);
}

/// 固定したタスクのタイマーも仮想の時計を進める。期限の順に満ちるので、`main`の期限10が先で、固定したタスクの
/// 期限20が後である (docs/spec/vm.mdの「時計とdriver」)。
#[test]
fn a_pinned_timer_advances_the_virtual_clock() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let p = Async.start_on_main (fn () ->
    Async.sleep_ms 20
    println \"pinned\")
  Async.sleep_ms 10
  println \"main\"
  Async.await p)
";
    let (lines, result) = run(src, 1);
    finished(result);
    assert_eq!(texts(&lines), ["main", "pinned"]);
    assert_eq!(thread_of(&lines, "pinned"), std::thread::current().id());
}

/// 本物の時計でも固定したタスクのタイマーは満ちる。メインのworkerは自分のdriverで期限まで待つ。時間は確かめない。
#[test]
fn a_pinned_timer_fires_on_the_real_clock() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    Async.sleep_ms 5
    println \"timer\")
  println \"done\")
";
    let (lines, result) = run_real(src);
    finished(result);
    assert_eq!(texts(&lines), ["timer", "done"]);
}

/// 固定したタスクが作ったリスナーはメインのworkerのdriverにあり、固定したタスクがworker 0のクライアントを受ける。
#[test]
fn a_pinned_listener_serves_a_worker_0_client() {
    let src = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (port, l) = Async.run_on_main (fn () -> Net.local_port (Net.listen \"127.0.0.1\" 0))
  let server = Async.start_on_main (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let (line, s) = Net.read_line s
    Net.close s
    match line with
      | Some text -> println \"server: \\{text}\"
      | None -> println \"eof\")
  let c = Net.connect \"127.0.0.1\" port
  let c = Net.write_line \"hello\" c
  Net.close c
  Async.await server)
";
    let (lines, result) = run_real(src);
    finished(result);
    assert_eq!(texts(&lines), ["server: hello"]);
    assert_eq!(
        thread_of(&lines, "server: hello"),
        std::thread::current().id()
    );
}

/// 固定したタスクが接続したストリームは、worker 0に渡っても作ったメインのworkerのdriverに残り、worker 0の書き込みは
/// メインのworkerが出す (docs/spec/vm.mdの「時計とdriver」)。
#[test]
fn a_socket_moves_from_a_pinned_task_to_worker_0() {
    let src = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let (line, s) = Net.read_line s
    Net.close s
    match line with
      | Some text -> println \"server: \\{text}\"
      | None -> println \"eof\")
  let c = Async.run_on_main (fn () -> Net.connect \"127.0.0.1\" port)
  let c = Net.write_line \"moved\" c
  Net.close c
  Async.await server)
";
    let (lines, result) = run_real(src);
    finished(result);
    assert_eq!(texts(&lines), ["server: moved"]);
}

/// 固定したタスクの読みがdriverに出ている間に`main`のタスクが誤りで止まっても、runはその誤りで終わる。メインの
/// workerは`END`で起き、自分のスレッドでdriverを静止させてから戻り、呼んだスレッドはその後にworker 0をjoinして畳み込む。
#[test]
fn an_error_in_main_ends_a_run_with_a_pinned_read_in_flight() {
    let src = "\
import Async
import Bytes
import Net
deep : Int -> <Panic> Int
deep n = if n == 0 then 0 else 1 + deep (n - 1)
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let reader = Async.start_on_main (fn () ->
    let c = Net.connect \"127.0.0.1\" port
    let c = Net.write_line \"ready\" c
    let (buf, c) = Net.read_chunk (Bytes.empty ()) c
    Net.close c
    println \"read \\{show (Bytes.length buf)}\")
  let (s, l) = Net.accept l
  Net.close_listener l
  let (_, s) = Net.read_line s
  Async.sleep_ms 5
  println (show (deep 100000))
  Net.close s
  Async.await reader)
";
    let control = Arc::new(Control::new(Limits {
        workers: 1,
        max_depth: 4096,
        ..Limits::default()
    }));
    let (lines, result) = run_with(
        src,
        RunConfig::default()
            .with_clock(Clock::Real)
            .with_control(control),
    );
    assert_eq!(texts(&lines), Vec::<&str>::new());
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("the call depth limit is exceeded"),
        "{error}"
    );
}

/// 固定したタスクが落としたストリームは、メインのworkerが自分のdriverの待ちで閉じる。worker 0のタスクの読みは
/// EOFで戻り、眠っていたworker 0は起きる (docs/spec/vm.mdの「時計とdriver」)。
#[test]
fn a_pinned_task_closing_a_socket_wakes_a_sleeping_worker_0() {
    let src = "\
import Async
import Bytes
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let reader = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let (rest, s) = Net.read_chunk (Bytes.empty ()) s
    Net.close s
    if Bytes.length rest == 0 then println \"eof\" else println \"data\")
  Async.run_on_main (fn () ->
    let c = Net.connect \"127.0.0.1\" port
    Async.sleep_ms 5
    Net.close c)
  Async.await reader)
";
    let (lines, result) = run_real(src);
    finished(result);
    assert_eq!(texts(&lines), ["eof"]);
}

/// `Finished.pinned_tasks`は固定して始めたタスクを数える。固定したタスクが始めたタスクも数え、ほかのタスクは数えない。
/// workerの数によらない。
#[test]
fn pinned_tasks_are_counted() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let a = Async.start_on_main (fn () ->
    let c = Async.start (fn () -> 1)
    Async.await c)
  let b = Async.start (fn () -> 2)
  let x = Async.await a + Async.await b
  println (show x))
";
    for workers in [1, 2] {
        let (lines, result) = run(src, workers);
        let finished = finished(result);
        assert_eq!(texts(&lines), ["3"], "workers {workers}");
        assert_eq!(finished.pinned_tasks, 2, "workers {workers}");
    }
}

/// 鳴ったかを持つwaker。
#[derive(Default)]
struct Bell {
    rung: Mutex<bool>,
    cv: Condvar,
}

impl IoWaker for Bell {
    fn wake(&self) {
        *self.rung.lock().unwrap() = true;
        self.cv.notify_all();
    }
}

impl Bell {
    /// 鳴るか`timeout`が過ぎるまで待ち、鳴った印を下ろす。
    fn wait(&self, timeout: Option<Duration>) {
        let rung = self.rung.lock().unwrap();
        let mut rung = match timeout {
            None => self.cv.wait_while(rung, |rung| !*rung).unwrap(),
            Some(timeout) => {
                self.cv
                    .wait_timeout_while(rung, timeout, |rung| !*rung)
                    .unwrap()
                    .0
            }
        };
        *rung = false;
    }
}

/// テストの`Park`のふるまい。
#[derive(Debug, Clone, Copy)]
enum Mode {
    /// 鳴るか期限まで待ち、`Pending::run`を1回呼ぶ。
    Runs,
    /// `run`を呼ばずにすぐ戻る。
    AtOnce,
    /// 鳴るか期限まで待ち、`run`を呼ばない。
    Never,
    /// `Stopping`の後も`run`を呼び続ける。
    IgnoresStopping,
    /// 最初の`n`回の待ちの間はrunの外の待ちを抱える。どの待ちも5 msで終わる。
    Holds(usize),
}

/// テストの`Park`が書くもの。作ったスレッドと待ったスレッド、待ちの数、期限のある待ちがあったか。
#[derive(Default)]
struct Log {
    threads: Mutex<Vec<ThreadId>>,
    waits: AtomicUsize,
    timed: AtomicBool,
}

/// テストのためのメインのworkerの`Park`。
struct TestPark {
    mode: Mode,
    bell: Arc<Bell>,
    log: Arc<Log>,
}

impl Park for TestPark {
    fn wait(&mut self, timeout: Option<Duration>, pending: &mut dyn Pending) -> io::Result<()> {
        self.log
            .threads
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        let waits = self.log.waits.fetch_add(1, Ordering::SeqCst) + 1;
        if timeout.is_some() {
            self.log.timed.store(true, Ordering::SeqCst);
        }
        match self.mode {
            Mode::Runs => {
                self.bell.wait(timeout);
                pending.run();
            }
            Mode::AtOnce => {}
            Mode::Never => self.bell.wait(timeout),
            Mode::IgnoresStopping => loop {
                if let Progress::Wait(timeout) = pending.run() {
                    self.bell.wait(timeout);
                }
            },
            Mode::Holds(n) => {
                pending.set_holds(waits < n);
                self.bell.wait(Some(Duration::from_millis(5)));
            }
        }
        Ok(())
    }

    fn waker(&self) -> Option<Arc<dyn IoWaker>> {
        match self.mode {
            Mode::AtOnce => None,
            _ => Some(Arc::clone(&self.bell) as Arc<dyn IoWaker>),
        }
    }
}

/// `mode`の`Park`を作り、作ったスレッドを`log`に書くもの。
fn park(mode: Mode, log: &Arc<Log>) -> ParkFactory {
    let log = Arc::clone(log);
    Arc::new(move || {
        log.threads
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        Box::new(TestPark {
            mode,
            bell: Arc::default(),
            log: Arc::clone(&log),
        })
    })
}

/// 替えた`Park`も`execute`を呼んだスレッドで作って待ち、`Pending::run`で動かす固定したタスクもそこで動く。どちらの
/// 時計でも同じである。
#[test]
fn a_main_park_runs_on_the_caller_thread() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    Async.sleep_ms 5
    println \"pinned\")
  println \"main\")
";
    for clock in [Clock::Virtual, Clock::Real] {
        let log = Arc::new(Log::default());
        let config = RunConfig::default()
            .with_clock(clock)
            .with_control(limits(1))
            .with_main_park(park(Mode::Runs, &log));
        let (lines, result) = run_with(src, config);
        finished(result);
        assert_eq!(texts(&lines), ["pinned", "main"], "{clock:?}");
        let me = std::thread::current().id();
        assert_eq!(thread_of(&lines, "pinned"), me, "{clock:?}");
        let threads = log.threads.lock().unwrap();
        assert!(threads.len() >= 2, "{clock:?}: {threads:?}");
        assert!(threads.iter().all(|&thread| thread == me), "{clock:?}");
    }
}

/// 何も動かさずにすぐ戻る`Park`の後では、メインのworkerは積まれた仕事を`Park`の代わりに動かし、なければ眠った形の
/// ままでいる。出力もrunの数も既定の`Park`と同じである。
#[test]
fn a_park_that_returns_at_once_changes_nothing() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let p = Async.start_on_main (fn () ->
    Async.sleep_ms 10
    let xs = [1, 2, 3]
    println \"pinned \\{show xs}\")
  Async.sleep_ms 5
  println \"main\"
  Async.await p)
";
    let (default_lines, default) = run(src, 1);
    let log = Arc::new(Log::default());
    let (lines, result) = run_with(
        src,
        RunConfig::default()
            .with_control(limits(1))
            .with_main_park(park(Mode::AtOnce, &log)),
    );
    assert_eq!(texts(&lines), texts(&default_lines));
    assert_eq!(texts(&lines), ["main", "pinned [1, 2, 3]"]);
    assert_eq!(finished(result).stats, finished(default).stats);
    assert!(log.waits.load(Ordering::SeqCst) >= 1);
}

const CHAN_DEADLOCK: &str = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () -> Async.sleep_ms 1)
  let c : Chan.Chan Int = Chan.new 0
  println (show (Chan.recv c)))
";

const ACCEPT_DEADLOCK: &str = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () -> Async.sleep_ms 1)
  let l = Net.listen \"127.0.0.1\" 0
  let (s, l) = Net.accept l
  Net.close_listener l
  Net.close s)
";

/// `Park`が最初の待ちで`set_holds(true)`を書き、5回目の待ちで下ろす`clock`のrunで、`src`はデッドロックで終わる。
/// 固定したタスクはメインのworkerが`Park`に入った後にだけ動くので、最初の待ちはデッドロックになりうるより前に来る。
/// 判定は抱えるのをやめた後なので、`Park`は少なくとも5回待っている。
fn defers_the_verdict(src: &str, clock: Clock) {
    let log = Arc::new(Log::default());
    let config = RunConfig::default()
        .with_clock(clock)
        .with_control(limits(1))
        .with_main_park(park(Mode::Holds(5), &log));
    let (lines, result) = run_with(src, config);
    assert_eq!(texts(&lines), Vec::<&str>::new());
    let error = result.unwrap_err();
    assert!(matches!(error, RuntimeError::Deadlock), "{error:?}");
    let waits = log.waits.load(Ordering::SeqCst);
    assert!(waits >= 5, "{waits}");
}

/// `Park`がrunの外の待ちを抱えている間、worker 0はデッドロックと決めず、抱えるのをやめた後にrunはデッドロックで
/// 終わる。本物の時計の判定は保留の操作のない場合である。
#[test]
fn a_holding_park_defers_the_deadlock_verdict() {
    defers_the_verdict(CHAN_DEADLOCK, Clock::Real);
}

/// 仮想の時計でも同じである。channelの待ちは保留の操作のない判定で、`accept`は終わりえない操作が残る判定である。
#[test]
fn a_holding_park_defers_the_deadlock_verdict_on_the_virtual_clock() {
    defers_the_verdict(CHAN_DEADLOCK, Clock::Virtual);
    defers_the_verdict(ACCEPT_DEADLOCK, Clock::Virtual);
}

/// 窓を開いたかをHostの行で見て、開いていればrunの外の待ちを抱える`Park`。`lower_after`回抱えて待った後に下ろす。
struct WindowPark {
    host: Arc<RecordingHost>,
    bell: Arc<Bell>,
    held: usize,
    lower_after: usize,
    lowered: Arc<AtomicBool>,
}

impl Park for WindowPark {
    fn wait(&mut self, timeout: Option<Duration>, pending: &mut dyn Pending) -> io::Result<()> {
        let open = self.host.lines().iter().any(|(line, _)| line == "open");
        if open && self.held < self.lower_after {
            // 次の`run`がメインのworkerを眠らせる前に書く
            pending.set_holds(true);
            self.held += 1;
            self.bell.wait(Some(Duration::from_millis(5)));
        } else {
            if open {
                pending.set_holds(false);
                self.lowered.store(true, Ordering::SeqCst);
            }
            self.bell.wait(timeout);
        }
        pending.run();
        Ok(())
    }

    fn waker(&self) -> Option<Arc<dyn IoWaker>> {
        Some(Arc::clone(&self.bell) as Arc<dyn IoWaker>)
    }
}

/// 固定したタスクが「窓」を開いたのを見ると、`Park`は次の`run`の前に`set_holds(true)`を書き、後で下ろす。固定した
/// タスクは窓を開いた後に仮想の時計で眠るので、そのタイマーが判定をその`run`の後まで延ばす。それから届かない
/// channelを待つ。runはデッドロックで終わるが、判定は`Park`が下ろした後である。
#[test]
fn a_park_that_raises_holds_once_a_window_is_open_defers_the_verdict() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    println \"open\"
    Async.sleep_ms 10
    let c : Chan.Chan Int = Chan.new 0
    println (show (Chan.recv c))))
";
    let (host, _) = RecordingHost::new(None);
    let lowered = Arc::new(AtomicBool::new(false));
    let factory: ParkFactory = {
        let (host, lowered) = (Arc::clone(&host), Arc::clone(&lowered));
        Arc::new(move || {
            Box::new(WindowPark {
                host: Arc::clone(&host),
                bell: Arc::default(),
                held: 0,
                lower_after: 5,
                lowered: Arc::clone(&lowered),
            })
        })
    };
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(limits(1))
        .with_main_park(factory);
    let result = eml_vm::execute(
        &eml_test_support::lir(src),
        &eml_vm::Linked::default(),
        &config,
        host.host_ref(),
    );
    assert_eq!(texts(&host.lines()), ["open"]);
    let error = result.unwrap_err();
    assert!(matches!(error, RuntimeError::Deadlock), "{error:?}");
    assert!(
        lowered.load(Ordering::SeqCst),
        "the verdict came before the park lowered holds"
    );
}

/// `run`の前にいつも`set_holds(true)`を立て、`run`の後に窓が開いていなければ下ろす`Park`。`wait`から戻らずに`run`を
/// 呼び続け、窓が開いた後は`lower_after`回抱えて待ってから下ろす。`raise_before_run`が偽なら、窓を見てから立てる。
struct ProtocolPark {
    host: Arc<RecordingHost>,
    bell: Arc<Bell>,
    raise_before_run: bool,
    held: usize,
    lower_after: usize,
    lowered: Arc<AtomicBool>,
}

impl ProtocolPark {
    fn open(&self) -> bool {
        self.host.lines().iter().any(|(line, _)| line == "open")
    }
}

impl Park for ProtocolPark {
    fn wait(&mut self, timeout: Option<Duration>, pending: &mut dyn Pending) -> io::Result<()> {
        let mut timeout = timeout;
        loop {
            self.bell.wait(timeout);
            let lowered = self.lowered.load(Ordering::SeqCst);
            if !lowered && self.held >= self.lower_after {
                pending.set_holds(false);
                self.lowered.store(true, Ordering::SeqCst);
            } else if !lowered && self.raise_before_run {
                pending.set_holds(true);
            }
            let Progress::Wait(next) = pending.run() else {
                return Ok(());
            };
            if self.lowered.load(Ordering::SeqCst) {
                timeout = next;
            } else if self.open() {
                if !self.raise_before_run {
                    pending.set_holds(true);
                }
                self.held += 1;
                timeout = Some(Duration::from_millis(5));
            } else {
                pending.set_holds(false);
                timeout = next;
            }
        }
    }

    fn waker(&self) -> Option<Arc<dyn IoWaker>> {
        Some(Arc::clone(&self.bell) as Arc<dyn IoWaker>)
    }
}

/// `run`がrunの外の待ちを始めうる`Park`は、`run`の前に`set_holds(true)`を立て、何も始まらなければ後で下ろす。固定した
/// タスクは窓を開くとすぐに届かないchannelを待ち、そのときタイマーは残っていない。`run`が最後に眠らせたメインの
/// workerがworker 0を起こしても、判定は`run`の前に立てた真を読むので、runは`Park`が下ろした後にデッドロックで終わる。
/// 先頭の`sleep_ms 1`で、窓は最初の`run`でなく後の`run`で開く。最初の`run`で開く形は
/// `a_window_opened_by_the_first_pinned_task_is_held`が確かめる。
#[test]
fn a_park_that_raises_holds_before_run_defers_the_verdict() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    Async.sleep_ms 1
    println \"open\"
    let c : Chan.Chan Int = Chan.new 0
    println (show (Chan.recv c))))
";
    let (host, _) = RecordingHost::new(None);
    let lowered = Arc::new(AtomicBool::new(false));
    let factory: ParkFactory = {
        let (host, lowered) = (Arc::clone(&host), Arc::clone(&lowered));
        Arc::new(move || {
            Box::new(ProtocolPark {
                host: Arc::clone(&host),
                bell: Arc::default(),
                raise_before_run: true,
                held: 0,
                lower_after: 5,
                lowered: Arc::clone(&lowered),
            })
        })
    };
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(limits(1))
        .with_main_park(factory);
    let result = eml_vm::execute(
        &eml_test_support::lir(src),
        &eml_vm::Linked::default(),
        &config,
        host.host_ref(),
    );
    assert_eq!(texts(&host.lines()), ["open"]);
    let error = result.unwrap_err();
    assert!(matches!(error, RuntimeError::Deadlock), "{error:?}");
    assert!(
        lowered.load(Ordering::SeqCst),
        "the verdict came before the park lowered holds"
    );
}

/// `run`を呼ばずに`wait`から戻る`Park`で、規則のとおりに抱える。待ちに入るときに窓が開いていなければ下ろし、
/// 眠ってから、戻ってタスクを動かさせる前に立てる。窓が開いた後は`lower_after`回抱えて待ってから下ろす。
struct ReturningPark {
    host: Arc<RecordingHost>,
    bell: Arc<Bell>,
    held: usize,
    lower_after: usize,
    lowered: Arc<AtomicBool>,
}

impl Park for ReturningPark {
    fn wait(&mut self, timeout: Option<Duration>, pending: &mut dyn Pending) -> io::Result<()> {
        if self.lowered.load(Ordering::SeqCst) {
            self.bell.wait(timeout);
        } else if self.host.lines().iter().any(|(line, _)| line == "open") {
            self.held += 1;
            if self.held >= self.lower_after {
                pending.set_holds(false);
                self.lowered.store(true, Ordering::SeqCst);
                self.bell.wait(timeout);
            } else {
                self.bell.wait(Some(Duration::from_millis(5)));
            }
        } else {
            pending.set_holds(false);
            self.bell.wait(timeout);
            pending.set_holds(true);
        }
        Ok(())
    }

    fn waker(&self) -> Option<Arc<dyn IoWaker>> {
        Some(Arc::clone(&self.bell) as Arc<dyn IoWaker>)
    }
}

/// 最初の固定したタスクがすぐに窓を開き、そのまま届かないchannelを待つ。固定したタスクは`Park`が制御を受け取った
/// 後にだけ動くので、`run`を呼ぶ`Park`も戻る`Park`も、タスクを動かす前に立てた真で判定を止め、下ろした後にだけ
/// runはデッドロックで終わる。この試験は約束を記すものである。最初の待ちの前の`drive`を戻しても、その`drive`は
/// たいていタスクが置かれる前に動くので、タイミングの差し込みなしには落ちない。
#[test]
fn a_window_opened_by_the_first_pinned_task_is_held() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    println \"open\"
    let c : Chan.Chan Int = Chan.new 0
    println (show (Chan.recv c))))
";
    for returns in [false, true] {
        let (host, _) = RecordingHost::new(None);
        let lowered = Arc::new(AtomicBool::new(false));
        let factory: ParkFactory = {
            let (host, lowered) = (Arc::clone(&host), Arc::clone(&lowered));
            Arc::new(move || -> Box<dyn Park> {
                let (host, bell, lowered) =
                    (Arc::clone(&host), Arc::default(), Arc::clone(&lowered));
                if returns {
                    Box::new(ReturningPark {
                        host,
                        bell,
                        held: 0,
                        lower_after: 5,
                        lowered,
                    })
                } else {
                    Box::new(ProtocolPark {
                        host,
                        bell,
                        raise_before_run: true,
                        held: 0,
                        lower_after: 5,
                        lowered,
                    })
                }
            })
        };
        let config = RunConfig::default()
            .with_debug_heap(true)
            .with_control(limits(1))
            .with_main_park(factory);
        let result = eml_vm::execute(
            &eml_test_support::lir(src),
            &eml_vm::Linked::default(),
            &config,
            host.host_ref(),
        );
        assert_eq!(texts(&host.lines()), ["open"], "returns {returns}");
        let error = result.unwrap_err();
        assert!(
            matches!(error, RuntimeError::Deadlock),
            "returns {returns}: {error:?}"
        );
        assert!(
            lowered.load(Ordering::SeqCst),
            "returns {returns}: the verdict came before the park lowered holds"
        );
    }
}

/// `Stopping`の後も`run`を呼び続ける`Park`は、メインのworkerのdebugの確かめで落ち、`execute`はそのpanicで戻る。
/// メインのworkerの繰り返しはいつも`Park`の待ちから始まるので、runがすぐに終わっても`Park`に入る。
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "Park::wait ignored Progress::Stopping")]
fn a_park_that_ignores_stopping_fails_in_debug_builds() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () -> Async.sleep_ms 10)
  println \"main\")
";
    let log = Arc::new(Log::default());
    let config = RunConfig::default()
        .with_control(limits(1))
        .with_main_park(park(Mode::IgnoresStopping, &log));
    let _ = run_with(src, config);
}

/// `run`を一度も呼ばない`Park`でも`main`は止まらない。時計、判定、`END`はworker 0にあり、runはどちらの時計でも
/// いつもの出力で終わる。メインのworkerの繰り返しはいつも`Park`の待ちから始まるので、`Park`は必ず呼ばれる。固定した
/// タスクは、`Park`が戻った後にメインのworkerが代わりに動かす。`Park`を抜けるのは、起こされたときか期限を過ぎた
/// ときだけである。
#[test]
fn a_park_that_never_runs_pending_still_ends_main() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let p = Async.start_on_main (fn () -> Async.sleep_ms 3)
  let t = Async.start (fn () ->
    Async.sleep_ms 10
    1)
  Async.sleep_ms 5
  Async.await p
  println \"main \\{show (Async.await t)}\")
";
    for clock in [Clock::Virtual, Clock::Real] {
        let log = Arc::new(Log::default());
        let config = RunConfig::default()
            .with_clock(clock)
            .with_control(limits(2))
            .with_main_park(park(Mode::Never, &log));
        let (lines, result) = run_with(src, config);
        finished(result);
        assert_eq!(texts(&lines), ["main 1"], "{clock:?}");
        assert!(log.waits.load(Ordering::SeqCst) >= 1, "{clock:?}");
    }
}

/// 本物の時計で、`run`を呼ばずに`timeout`が過ぎたら戻る`Park`の下でも、固定したタスクのタイマーは満ちる。期限を
/// 過ぎるとメインのworkerは待ちを抜け、`drive`がタイマーを拾う。
#[test]
fn a_park_that_returns_at_its_timeout_collects_a_pinned_timer() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    Async.sleep_ms 5
    println \"pinned\")
  println \"main\")
";
    let log = Arc::new(Log::default());
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_control(limits(1))
        .with_main_park(park(Mode::Never, &log));
    let (lines, result) = run_with(src, config);
    finished(result);
    assert_eq!(texts(&lines), ["pinned", "main"]);
    assert!(log.timed.load(Ordering::SeqCst));
}

/// 固定したタスクのタイマーを待つメインのworkerの`Park`のwakerを、割り込みが鳴らし、runは割り込みで終わる。割り込みを
/// 送るのは、`Park`がそのタイマーの期限で待った後である。
#[test]
fn an_interrupt_reaches_a_main_worker_in_a_custom_park() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () -> Async.sleep_ms 100000))
";
    let log = Arc::new(Log::default());
    let control = limits(2);
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_control(Arc::clone(&control))
        .with_main_park(park(Mode::Runs, &log));
    let interrupter = {
        let log = Arc::clone(&log);
        std::thread::spawn(move || {
            while !log.timed.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            control.interrupt();
        })
    };
    let (_, result) = run_with(src, config);
    interrupter.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
}

/// 既定の`Park`では、メインのworkerは自分のdriverで待つので、固定したタスクのソケットの読みが終わる。
#[test]
fn a_pinned_socket_read_completes_under_the_default_park() {
    let src = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let reader = Async.start_on_main (fn () ->
    let c = Net.connect \"127.0.0.1\" port
    let (line, c) = Net.read_line c
    Net.close c
    match line with
      | Some text -> println \"pinned: \\{text}\"
      | None -> println \"eof\")
  let (s, l) = Net.accept l
  Net.close_listener l
  let s = Net.write_line \"hello\" s
  Net.close s
  Async.await reader)
";
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_control(limits(1))
        .with_main_park(Arc::new(|| Box::new(DefaultPark)));
    let (lines, result) = run_with(src, config);
    finished(result);
    assert_eq!(texts(&lines), ["pinned: hello"]);
    assert_eq!(
        thread_of(&lines, "pinned: hello"),
        std::thread::current().id()
    );
}

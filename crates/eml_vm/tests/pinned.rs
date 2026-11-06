//! メインのworkerに固定したタスク (`Async.start_on_main`、`Async.run_on_main`)。分かれたrunでは`Machine`を持つスレッド
//! で動き、ほかのworkerへ動かない。`par`の子と`compute`の仕事はほかと同じく置く (docs/spec/vm.mdの「メインのworker」、
//! docs/spec/stdlib.mdの「メインのworker」)。

use std::sync::Arc;
use std::thread::ThreadId;

use eml_rt::Checked;
use eml_vm::{Clock, Control, Finished, Limits, Machine, RunConfig, RuntimeError};

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

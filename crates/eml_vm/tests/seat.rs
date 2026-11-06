//! 分かれた run (`Seat::Separate`) の形。`Machine`を持つスレッドはメインのworkerで、`main`はrunが作るworker 0で
//! 動く (docs/spec/vm.mdの「メインの worker」)。

use std::sync::Arc;

use eml_rt::Checked;
use eml_vm::{Clock, Control, Limits, Machine, RunConfig, RuntimeError, Seat, VmFault};

use crate::common::{RecordingHost, parse};

const HELLO: &str = "\
main : Unit -> <IO, Panic> Unit
main () = println \"main\"
";

fn control() -> Arc<Control> {
    Arc::new(Control::new(Limits::default()))
}

fn internal(result: Result<eml_vm::Finished, RuntimeError>) -> &'static str {
    match result {
        Err(RuntimeError::Fault {
            fault: VmFault::Internal(what),
            ..
        }) => what,
        other => panic!("not an internal error: {other:?}"),
    }
}

/// セッションの根に触れるコードは、メインのworkerを別のスレッドに置くrunでは動かない。根が、落としたメインのworkerの
/// ヒープを指しうるためである (docs/spec/vm.mdの「誤り、止めること、終わり」)。
#[test]
fn a_separate_run_refuses_session_roots() {
    let program = parse(
        "\
fn keep() -> unit {
  let b.0: tobj = box 1
  let u.1: unit = prim root_set[0](b.0)
  return u.1
}
",
    );
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(control(), host).with_seat(Seat::Separate);
    let entry = machine
        .load(&program, &eml_vm::Linked::default())
        .unwrap()
        .entry;
    assert_eq!(
        internal(machine.run(entry)),
        "code reaching session roots cannot run with a main worker on another thread"
    );
    assert_eq!(out.contents(), "");
}

/// 渡されたdriverは作ったスレッドを離れられないので、分かれたrunは拒む。driverはworker 0が自分で作る
/// (docs/spec/vm.mdの「時計と driver」)。
#[test]
fn a_separate_machine_refuses_an_injected_driver() {
    let program = eml_test_support::lir(HELLO);
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(control(), host)
        .with_driver(Box::new(eml_io::TestDriver::new(None)))
        .with_seat(Seat::Separate);
    let entry = machine
        .load(&program, &eml_vm::Linked::default())
        .unwrap()
        .entry;
    assert_eq!(
        internal(machine.run(entry)),
        "a separate run makes its driver on worker 0"
    );
    assert_eq!(out.contents(), "");
}

/// `Async.sleep_ms 10`の後に書くプログラム。
const SLEEP: &str = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.sleep_ms 10
  println \"slept\")
";

/// 分かれた`Machine`は続けてrunできる。どちらのrunもworker 0を作り直し、driverをそのスレッドで作って落とす。
/// 本物の時計では両方のrunが終わることを確かめる。時刻を読むライブラリの関数はないので、時間の基準が続くことは
/// 仮想の時計の予定のシグナルで見る。予定の期限は基準の始まりから数えるので、1回目のrunが時計を10まで進めた後の
/// 2回目のrunでは、期限15のシグナルが2回目の`sleep_ms 10` (期限20) の前に届いて割り込む。基準を作り直していれば、
/// 2回目のrunは期限15の前に終わる (docs/spec/vm.mdの「時計と driver」)。
#[test]
fn a_separate_machine_runs_twice() {
    let program = eml_test_support::lir(SLEEP);
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(control(), host)
        .with_clock(Clock::Real)
        .with_seat(Seat::Separate);
    let entry = machine
        .load(&program, &eml_vm::Linked::default())
        .unwrap()
        .entry;
    for _ in 0..2 {
        machine.run(entry).unwrap_or_else(|error| panic!("{error}"));
    }
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "slept\nslept\n");

    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(control(), host).with_seat(Seat::Separate);
    let entry = machine
        .load(&program, &eml_vm::Linked::default())
        .unwrap()
        .entry;
    machine.run(entry).unwrap_or_else(|error| panic!("{error}"));
    let mut machine = machine.with_signals(vec![eml_vm::VirtualSignal {
        at_ms: 15,
        signal: eml_vm::Signal::Interrupt,
    }]);
    let error = machine.run(entry).unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    assert_eq!(out.contents(), "slept\n");
}

/// worker 0のpanicは、呼んだスレッドで同じ中身のまま起こし直す。メインのworkerは`END`で起き、scopeは戻る。その後の
/// `execute`は新しい`Machine`で動く。
#[test]
fn a_panic_on_worker_0_ends_a_separate_run() {
    let program = eml_test_support::lir(
        "\
main : Unit -> <IO, Panic> Unit
main () = println \"boom\"
",
    );
    let (host, _) = RecordingHost::new(Some("boom"));
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(Arc::new(Control::new(Limits {
            workers: 2,
            ..Limits::default()
        })));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eml_vm::execute(
            &program,
            &eml_vm::Linked::default(),
            &config,
            host.host_ref(),
        )
    }))
    .expect_err("the panic reaches the caller");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied());
    assert_eq!(message, Some("boom"));

    let (host, out) = RecordingHost::new(None);
    eml_vm::execute(
        &eml_test_support::lir(HELLO),
        &eml_vm::Linked::default(),
        &config,
        host.host_ref(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "main\n");
}

/// `execute`の`main`は、呼んだスレッドでなく、runが作るworker 0のスレッドで動く。
#[test]
fn worker_0_is_a_spawned_thread_in_a_separate_run() {
    let (host, _) = RecordingHost::new(None);
    eml_vm::execute(
        &eml_test_support::lir(HELLO),
        &eml_vm::Linked::default(),
        &RunConfig::default(),
        host.host_ref(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let lines = host.lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].0, "main");
    assert_ne!(lines[0].1, std::thread::current().id());
}

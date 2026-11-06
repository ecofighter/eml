//! 分かれたrun (`Seat::Separate`) の形。`Machine`を持つスレッドはメインのworkerで、`main`はrunが作るworker 0で
//! 動く (docs/spec/vm.mdの「メインのworker」)。

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

/// workerがpanicするrun。panicしたworkerは`Ctx`を漏らすので (docs/spec/vm.mdの「タスクとスケジュール」)、
/// AddressSanitizerは`worker_panic::`をleakの検出なしで流す (docs/implementation/testing.mdの「AddressSanitizer」)。
mod worker_panic {
    use super::*;

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

    /// worker 0でrunがpanicした合わせた`Machine`は、runのタスクとセルを途中のまま残しているので、後の`run`はそれらを
    /// 使わずにpanicする (docs/spec/vm.mdの「タスクとスケジュール」)。
    #[test]
    #[should_panic(expected = "a Machine is used after a run panicked")]
    fn a_machine_refuses_use_after_a_run_panicked() {
        let program = eml_test_support::lir(
            "\
main : Unit -> <IO, Panic> Unit
main () = println \"boom\"
",
        );
        let (host, _) = RecordingHost::new(Some("boom"));
        let mut machine = Machine::<Checked>::new(control(), host.host_ref());
        let entry = machine
            .load(&program, &eml_vm::Linked::default())
            .unwrap()
            .entry;
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| machine.run(entry)))
            .expect_err("the panic reaches the caller");
        assert_eq!(
            panic.downcast_ref::<String>().map(String::as_str),
            Some("boom")
        );
        let _ = machine.run(entry);
    }

    /// worker 1に置いたタスクがHostの中でpanicした合わせた`Machine`は、そのpanicを同じ中身のまま起こし直し、後の
    /// 使用を拒む (docs/spec/vm.mdの「タスクとスケジュール」)。
    #[test]
    #[should_panic(expected = "a Machine is used after a run panicked")]
    fn a_panic_on_worker_1_reaches_the_caller_of_a_merged_run() {
        let program = eml_test_support::lir(
            "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () -> println \"boom\")
  Async.await t)
",
        );
        let (host, _) = RecordingHost::new(Some("boom"));
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            ..Limits::default()
        }));
        let mut machine = Machine::<Checked>::new(control, host.host_ref());
        let entry = machine
            .load(&program, &eml_vm::Linked::default())
            .unwrap()
            .entry;
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| machine.run(entry)))
            .expect_err("the panic reaches the caller");
        assert_eq!(
            panic.downcast_ref::<String>().map(String::as_str),
            Some("boom")
        );
        let _ = machine.run(entry);
    }

    /// panicしたworkerのヒープは、`Machine`を落とすときに破棄処理を呼ぶ。panicしたrunが開いたままにした`Stdin`の
    /// ハンドルは閉じるので、同じHostの後のrunはまた標準入力を開ける (docs/spec/vm.mdの「タスクとスケジュール」)。
    #[test]
    fn a_panicked_heap_runs_its_finalizers_when_the_machine_drops() {
        finalizers_run_after_a_panic(
            1,
            "\
import Stdin
main : Unit -> <IO, Panic> Unit
main () =
  let h = Stdin.open ()
  println \"boom\"
  Stdin.close h
",
        );
    }

    /// workerが2つのときも同じである。worker 1に置いたタスクが標準入力を開いてpanicし、そのヒープはworker 1が返す
    /// `Ctx`を通って`Machine`に届く。
    #[test]
    fn a_panicked_heap_on_worker_1_runs_its_finalizers_when_the_machine_drops() {
        finalizers_run_after_a_panic(
            2,
            "\
import Async
import Stdin
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    let h = Stdin.open ()
    println \"boom\"
    Stdin.close h)
  Async.await t)
",
        );
    }

    /// `workers`個のworkerの合わせた`Machine`で`src`を動かし、Hostの`boom`でpanicさせる。`Machine`を落とした後、同じ
    /// Hostの新しい`Machine`で標準入力を開き直す。
    fn finalizers_run_after_a_panic(workers: usize, src: &str) {
        let (host, out) = RecordingHost::new(Some("boom"));
        let host_ref = host.host_ref();
        let limited = Arc::new(Control::new(Limits {
            workers,
            ..Limits::default()
        }));
        let panicking = eml_test_support::lir(src);
        let mut machine = Machine::<Checked>::new(limited, host_ref.clone());
        let entry = machine
            .load(&panicking, &eml_vm::Linked::default())
            .unwrap()
            .entry;
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| machine.run(entry)))
            .expect_err("the panic reaches the caller");
        assert_eq!(
            panic.downcast_ref::<String>().map(String::as_str),
            Some("boom")
        );
        drop(machine);

        let reopening = eml_test_support::lir(
            "\
import Stdin
main : Unit -> <IO, Panic> Unit
main () =
  let h = Stdin.open ()
  Stdin.close h
  println \"reopened\"
",
        );
        let mut machine = Machine::<Checked>::new(control(), host_ref);
        let entry = machine
            .load(&reopening, &eml_vm::Linked::default())
            .unwrap()
            .entry;
        machine.run(entry).unwrap_or_else(|error| panic!("{error}"));
        machine.close().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(out.contents(), "reopened\n");
    }
}

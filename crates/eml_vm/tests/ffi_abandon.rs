//! 静止の予算を過ぎても戻らない `blocking` の呼び出しを見捨てる run (docs/spec/vm.md の「時計と driver」)。見捨てた
//! `Machine` はヒープを意図して漏らすので、AddressSanitizer はこのモジュールだけを leak の検出なしで流す
//! (docs/implementation/testing.md の「AddressSanitizer」)。テストの書き方は docs/implementation/testing.md の
//! 「`eml_vm` のテスト」の `ffi_abandon.rs` の項に従う。
//!
//! タスクは `gate_open` で合図の鍵を開けてから、開かない鍵を `gate_wait` で待つ。2つの呼び出しの間に安全点はないので、
//! 合図を受けてから run を終わらせても、その前に仕事はもう出ている。`Machine` の既定の方針 `Sched::Sequential` は
//! 救済しないので、2 worker の run で最初に `start` したタスクは worker 1 で動き、仕事は worker 1 の driver に出る
//! (docs/spec/vm.md の「タスクとスケジュール」)。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_lir::Program;
use eml_rt::{Checked, Control, Limits};
use eml_vm::{Clock, CompileError, Linked, Machine, RunConfig, RuntimeError, capture_host};

use crate::common::c::{calls, dtors, linked, rs_gate_open, rs_gate_wait};

/// 静止が仕事を待つ長さ。1 秒の既定より短くし、テストを速く終える。
const BUDGET: Duration = Duration::from_millis(20);

const KEPT_PENDING: &str = "the driver kept pending ops after quiesce";

fn linked_all(program: &Program) -> Linked {
    linked(program, &calls(), &dtors())
}

/// 本物の時計で `workers` 個の worker を使う、検査モードの `Machine`。
fn machine(workers: usize) -> (Machine<Checked>, Arc<Control>) {
    let control = Arc::new(Control::new(Limits {
        workers,
        ..Limits::default()
    }));
    let (host, _) = capture_host(Default::default());
    let machine = Machine::<Checked>::new(Arc::clone(&control), host)
        .with_clock(Clock::Real)
        .with_job_budget(BUDGET);
    (machine, control)
}

const HEAD: &str = "\
import Async (Async)
import Async

extern \"C\" blocking symbol \"rs_gate_wait\" gate_wait : Int -> <IO, Async> Int
extern \"C\" symbol \"rs_gate_open\" gate_open : Int -> <IO> Unit

hang : Int -> Int -> <IO, Async> Int
hang ready gate =
  gate_open ready
  gate_wait gate
";

/// `hang ready gate` のタスクと、`ready` の合図を待って `exit 3` を呼ぶタスク。
fn exits_during(ready: i64, gate: i64) -> String {
    format!(
        "{HEAD}
body : Unit -> <IO, Panic, Async> Unit
body () =
  let x = Async.start (fn () -> hang {ready} {gate})
  let y = Async.start (fn () ->
    let _ = gate_wait {ready}
    (exit 3 : Unit))
  println (show (Async.await x))
  Async.await y

main : Unit -> <IO, Panic> Unit
main () = Async.run body
"
    )
}

/// `hang ready gate` のタスクだけを待つ。run を終わらせるのはテストのスレッドの割り込みである。スコープの中の
/// `Panic` では、`run` が取り消したタスクの仕事を待つので run は止まらず、静止に届かない (docs/spec/vm.md の
/// 「`blocking` の呼び出し」)。
fn hangs(ready: i64, gate: i64) -> String {
    format!(
        "{HEAD}
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> println (show (Async.await (Async.start (fn () -> hang {ready} {gate})))))
"
    )
}

/// `ready` の合図を待ってから `control` に割り込むスレッド。
fn interrupt_after(ready: i64, control: &Arc<Control>) -> thread::JoinHandle<()> {
    let control = Arc::clone(control);
    thread::spawn(move || {
        assert_eq!(rs_gate_wait(ready), 0, "the task never signalled");
        control.interrupt();
    })
}

/// `exit` で終わった run は終了コードを返し、`Machine` は poisoned になり、`close` は driver の誤りを返す。
fn exit_keeps_its_code_and_poisons(src: &str, workers: usize, gates: &[i64]) {
    let program = eml_test_support::lir(src);
    let (mut machine, _) = machine(workers);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let finished = machine.run(loaded.entry);
    let poisoned = machine.is_poisoned();
    let closed = machine.close();
    for &gate in gates {
        rs_gate_open(gate);
    }
    let finished = finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
    assert_eq!(finished.rescues, 0, "{finished:?}");
    assert!(poisoned);
    let error = closed.unwrap_err();
    assert!(error.to_string().contains(KEPT_PENDING), "{error}");
}

#[test]
fn an_exit_during_a_blocking_call_keeps_its_code_and_poisons() {
    exit_keeps_its_code_and_poisons(&exits_during(100030, 100031), 1, &[100031]);
}

#[test]
fn an_exit_during_a_blocking_call_keeps_its_code_and_poisons_on_worker_one() {
    exit_keeps_its_code_and_poisons(&exits_during(100032, 100033), 2, &[100033]);
}

/// 戻らない呼び出しを2つのタスクに置く。`x` は worker 1、`z` は worker 0 に置かれ、どちらの worker の静止も、run の
/// 1つの期限の残りを受け取る。
#[test]
fn two_hung_workers_share_one_budget() {
    let src = format!(
        "{HEAD}
body : Unit -> <IO, Panic, Async> Unit
body () =
  let x = Async.start (fn () -> hang 100034 100035)
  let z = Async.start (fn () -> hang 100036 100037)
  let y = Async.start (fn () ->
    let _ = gate_wait 100034
    let _ = gate_wait 100036
    (exit 3 : Unit))
  println (show (Async.await x))
  println (show (Async.await z))
  Async.await y

main : Unit -> <IO, Panic> Unit
main () = Async.run body
"
    );
    exit_keeps_its_code_and_poisons(&src, 2, &[100035, 100037]);
}

/// 割り込みで止めた run は、回復を試みずにその誤りを返し、`Machine` は poisoned になる。誤りに割り込みを使うのは、
/// run を止める終わりだけが静止に入って仕事を見捨てるためである (`hangs`)。
fn an_interrupt_poisons(workers: usize, ready: i64, gate: i64) {
    let program = eml_test_support::lir(&hangs(ready, gate));
    let (mut machine, control) = machine(workers);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let interrupter = interrupt_after(ready, &control);
    let result = machine.run(loaded.entry);
    let poisoned = machine.is_poisoned();
    rs_gate_open(gate);
    interrupter.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    assert!(poisoned);
}

#[test]
fn an_interrupt_during_a_blocking_call_poisons() {
    an_interrupt_poisons(1, 100040, 100041);
}

#[test]
fn an_interrupt_during_a_blocking_call_poisons_on_worker_one() {
    an_interrupt_poisons(2, 100042, 100043);
}

/// poisoned の `Machine` では `run` が回復を飛ばすので、誤りは run 自身のもので、`Unrecovered` にならない。その後の
/// `load` と `run` は、どれも poisoned の誤りで断る。run を割り込みで止めるのは `hangs` と同じ理由である。
#[test]
fn a_machine_that_abandoned_a_job_refuses_recovery() {
    let program = eml_test_support::lir(&hangs(100044, 100045));
    let (mut machine, control) = machine(1);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let interrupter = interrupt_after(100044, &control);
    let first = machine.run(loaded.entry);
    rs_gate_open(100045);
    interrupter.join().unwrap();
    let error = first.unwrap_err();
    assert!(
        !matches!(error, RuntimeError::Unrecovered { .. }),
        "{error:?}"
    );
    assert!(error.interrupted(), "{error:?}");
    assert!(machine.is_poisoned());
    let program2 = eml_test_support::lir(
        "\
main : Unit -> <IO> Unit
main () = println \"unreachable\"
",
    );
    assert_eq!(
        machine.load(&program2, &Linked::default()),
        Err(CompileError::Poisoned)
    );
    let again = machine.run(loaded.entry).unwrap_err();
    assert!(again.to_string().contains(KEPT_PENDING), "{again}");
}

/// `execute` は poisoned の `Machine` を閉じないので、`exit` の終了コードがそのまま届く。
#[test]
fn execute_keeps_the_exit_code_of_a_run_that_abandoned_a_job() {
    let program = eml_test_support::lir(&exits_during(100046, 100047));
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_job_budget(BUDGET);
    let (host, _) = capture_host(Default::default());
    let result = eml_vm::execute(&program, &linked_all(&program), &config, host);
    rs_gate_open(100047);
    let finished = result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
}

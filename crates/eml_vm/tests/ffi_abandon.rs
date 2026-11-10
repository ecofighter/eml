//! 静止の予算を過ぎても戻らない`blocking`の呼び出しを見捨てるrun (docs/spec/vm.mdの「時計と driver」)。runはほかと
//! 同じく終わり、`Machine`はヒープを回復し、閉じることも、もう一度走ることもできる。テストの書き方は
//! docs/implementation/testing.mdの「`eml_vm` のテスト」の`ffi_abandon.rs`の項に従う。
//!
//! タスクは `gate_open` で合図の鍵を開けてから、開かない鍵を `gate_wait` で待つ。2つの呼び出しの間に安全点はないので、
//! 合図を受けてから run を終わらせても、その前に仕事はもう出ている。2 workerのrunで最初に`start`したタスクは
//! worker 1に置かれてそこで動き、仕事はworker 1のdriverに出る (docs/spec/vm.mdの「`Async`のタスク」)。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_lir::Program;
use eml_rt::{Checked, Control, Limits};
use eml_vm::{Capture, Clock, Linked, Machine, RunConfig, RuntimeError, capture_host};

use crate::common::c::{calls, dtors, linked, published, rs_frees, rs_gate_open, rs_gate_wait};

/// 静止が仕事を待つ長さ。1 秒の既定より短くし、テストを速く終える。
const BUDGET: Duration = Duration::from_millis(20);

fn linked_all(program: &Program) -> Linked {
    linked(program, &calls(), &dtors())
}

/// 本物の時計で `workers` 個の worker を使う、検査モードの `Machine`。
fn machine(workers: usize) -> (Machine<Checked>, Arc<Control>) {
    let (machine, control, _) = capturing_machine(workers);
    (machine, control)
}

/// `machine`と同じで、標準出力も返す。
fn capturing_machine(workers: usize) -> (Machine<Checked>, Arc<Control>, Capture) {
    let control = Arc::new(Control::new(Limits {
        workers,
        ..Limits::default()
    }));
    let (host, out) = capture_host(Default::default());
    let machine = Machine::<Checked>::new(Arc::clone(&control), host)
        .with_clock(Clock::Real)
        .with_job_budget(BUDGET);
    (machine, control, out)
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

/// `exit`で終わったrunは終了コードを返す。`Machine`は回復し、`close`は成功する。
fn exit_keeps_its_code(src: &str, workers: usize, gates: &[i64]) {
    let program = eml_test_support::lir(src);
    let (mut machine, _) = machine(workers);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let finished = machine.run(loaded.entry);
    let closed = machine.close();
    for &gate in gates {
        rs_gate_open(gate);
    }
    let finished = finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
    closed.unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn an_exit_during_a_blocking_call_keeps_its_code() {
    exit_keeps_its_code(&exits_during(100030, 100031), 1, &[100031]);
}

#[test]
fn an_exit_during_a_blocking_call_keeps_its_code_on_worker_one() {
    exit_keeps_its_code(&exits_during(100032, 100033), 2, &[100033]);
}

/// 戻らない呼び出しを2つのタスクに置く。`x`はworker 1、`z`はworker 2に置かれ、どちらのworkerの静止も、runの
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
    exit_keeps_its_code(&src, 3, &[100035, 100037]);
}

/// 割り込みで止めたrunは、回復の後にその誤りを返し、`close`は成功する。誤りに割り込みを使うのは、runを止める
/// 終わりだけが静止に入って仕事を見捨てるためである (`hangs`)。
fn an_interrupt_recovers(workers: usize, ready: i64, gate: i64) {
    let program = eml_test_support::lir(&hangs(ready, gate));
    let (mut machine, control) = machine(workers);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let interrupter = interrupt_after(ready, &control);
    let result = machine.run(loaded.entry);
    let closed = machine.close();
    rs_gate_open(gate);
    interrupter.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    assert!(
        !matches!(error, RuntimeError::Unrecovered { .. }),
        "{error:?}"
    );
    closed.unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn an_interrupt_during_a_blocking_call_recovers() {
    an_interrupt_recovers(1, 100040, 100041);
}

#[test]
fn an_interrupt_during_a_blocking_call_recovers_on_worker_one() {
    an_interrupt_recovers(2, 100042, 100043);
}

/// 仕事を見捨てたrunの後も`Machine`は回復するので、誤りはrun自身のもので、`Unrecovered`にならない。その後の`load`と
/// `run`はふつうに動く。runを割り込みで止めるのは`hangs`と同じ理由である。
#[test]
fn a_machine_that_abandoned_a_job_runs_again() {
    let program = eml_test_support::lir(&hangs(100044, 100045));
    let (mut machine, control, out) = capturing_machine(1);
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
    // 割り込みのビットは`clear`でしか下りないので、REPLと同じく次のrunの前に下ろす
    control.clear();
    // 名前が同じなら同じコードでなければならないので、`entry$main`を持たない手書きのLIRにする
    let program2 = crate::common::parse(
        "\
fn main() -> unit {
  let s.0: obj = const \"again\"
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
",
    );
    let loaded = machine
        .load(&program2, &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    machine
        .run(loaded.entry)
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "again\n");
}

/// 仕事は貸されたハンドルの`Arc`を持つ。runの後に回復と`close`が物体を放しても、dtorは呼び出しが戻ったときに
/// プールのスレッドで1回だけ走る (docs/spec/vm.mdの「`blocking` の呼び出し」)。
#[test]
fn a_handle_lent_to_an_abandoned_call_is_destroyed_after_it_returns() {
    let src = format!(
        "{HEAD}
extern \"C\" data Handle drop \"rs_handle_free_open\"
extern \"C\" symbol \"rs_handle_new\" handle_new : Int -> Int -> <IO, Panic> Handle
extern \"C\" blocking symbol \"rs_handle_wait\" handle_wait : Handle -> Int -> <IO, Async> (Int, Handle)

wait_on : Unit -> <IO, Panic, Async> Int
wait_on () =
  let h = handle_new 100110 7
  gate_open 100111
  let (v, h) = handle_wait h 100112
  drop h
  v

body : Unit -> <IO, Panic, Async> Unit
body () =
  let x = Async.start wait_on
  let y = Async.start (fn () ->
    let _ = gate_wait 100111
    (exit 3 : Unit))
  println (show (Async.await x))
  Async.await y

main : Unit -> <IO, Panic> Unit
main () = Async.run body
"
    );
    let program = eml_test_support::lir(&src);
    let (mut machine, _) = machine(1);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let finished = machine.run(loaded.entry);
    let closed = machine.close();
    let frees_before = rs_frees(100110);
    rs_gate_open(100112);
    let destroyed = rs_gate_wait(100110);
    let finished = finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(frees_before, 0);
    assert_eq!(destroyed, 0, "the dtor never ran");
    assert_eq!(rs_frees(100110), 1);
}

/// 呼び出しは配列の写しにだけ書く。ヒープの位置を通して書けば、回復か`close`が放した記憶に当たり、
/// AddressSanitizerがそれを報告する (docs/spec/vm.mdの「`blocking` の呼び出し」)。
#[test]
fn an_array_lent_to_an_abandoned_call_is_written_to_a_copy() {
    let src = format!(
        "import MutUnboxedArray
{HEAD}
extern \"C\" blocking symbol \"rs_fill_gated\" fill_gated : MutUnboxedArray Int -> Int -> Int -> <IO, Async> (Unit, MutUnboxedArray Int)

fill : Unit -> <IO, Panic, Async> Unit
fill () =
  let m = MutUnboxedArray.make 3 0
  let ((), m) = fill_gated m 3 100120
  drop m

body : Unit -> <IO, Panic, Async> Unit
body () =
  let x = Async.start fill
  let y = Async.start (fn () ->
    let _ = gate_wait 100120
    (exit 3 : Unit))
  Async.await x
  Async.await y

main : Unit -> <IO, Panic> Unit
main () = Async.run body
"
    );
    let program = eml_test_support::lir(&src);
    let (mut machine, _) = machine(1);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let finished = machine.run(loaded.entry);
    let closed = machine.close();
    rs_gate_open(100121);
    let written = rs_gate_wait(100122);
    let finished = finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(written, 0, "C did not write the copy");
}

/// `execute`は仕事を見捨てたrunの後にも`Machine`を閉じ、`exit`の終了コードがそのまま届く。
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

/// 大きい`MutUnboxedArray`を`fill_gated`に渡すタスクと、Cが鍵`gate`を開けたら`exit 3`を呼ぶタスク。
fn exits_during_a_large_fill(gate: i64) -> String {
    format!(
        "import MutUnboxedArray
{HEAD}
extern \"C\" blocking symbol \"rs_fill_gated\" fill_gated : MutUnboxedArray Int -> Int -> Int -> <IO, Async> (Unit, MutUnboxedArray Int)

fill : Unit -> <IO, Panic, Async> Unit
fill () =
  let m = MutUnboxedArray.make 10000 0
  let ((), m) = fill_gated m 10000 {gate}
  drop m

body : Unit -> <IO, Panic, Async> Unit
body () =
  let x = Async.start fill
  let y = Async.start (fn () ->
    let _ = gate_wait {gate}
    (exit 3 : Unit))
  Async.await x
  Async.await y

main : Unit -> <IO, Panic> Unit
main () = Async.run body
"
    )
}

/// 見捨てた呼び出しに貸した大きい配列のページは、runの後に回復と`close`がヒープを放しても取り置かれ、Cはその
/// ページに書く。AddressSanitizerが書きを確かめる (docs/spec/object-model.mdの「ページの留め」)。
fn a_large_array_keeps_its_page(workers: usize, gate: i64) {
    let program = eml_test_support::lir(&exits_during_a_large_fill(gate));
    let (mut machine, _) = machine(workers);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let finished = machine.run(loaded.entry);
    let closed = machine.close();
    // 鍵を開けるとCが戻って留めを落とすので、取り置きはその前に見る
    let parked = published(gate).map(|at| eml_rt::PagePin::parked(at as usize));
    rs_gate_open(gate + 1);
    let written = rs_gate_wait(gate + 2);
    let finished = finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(parked, Some(true), "the lent page was not parked");
    assert_eq!(written, 0, "C did not write the array");
}

#[test]
fn a_large_array_lent_to_an_abandoned_call_keeps_its_page() {
    a_large_array_keeps_its_page(1, 100150);
}

/// 貸すタスクはworker 1に置かれ、配列はworker 1のヒープにある。
#[test]
fn a_large_array_lent_on_worker_one_keeps_its_page() {
    a_large_array_keeps_its_page(2, 100153);
}

/// 割り込みで止めたrunが見捨てた呼び出しに貸した大きい文字列は、回復と次のrunの後も取り置かれ、Cはそれから
/// 読む。読んだバイトの和は元の文字列のものである。
#[test]
fn a_large_string_lent_to_an_abandoned_call_is_read_after_recovery() {
    let src = format!(
        "{HEAD}
extern \"C\" blocking symbol \"rs_sum_gated\" sum_gated : String -> Int -> Int -> <IO, Async> Unit

grow : Int -> String -> <Panic> String
grow k s = if k == 0 then s else grow (k - 1) (s ++ s)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> sum_gated (grow 10 \"ab\") 2048 100156)
"
    );
    let program = eml_test_support::lir(&src);
    let (mut machine, control, out) = capturing_machine(1);
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    let interrupter = interrupt_after(100156, &control);
    let first = machine.run(loaded.entry);
    interrupter.join().unwrap();
    control.clear();
    // 名前が同じなら同じコードでなければならないので、`entry$main`を持たない手書きのLIRにする
    let program2 = crate::common::parse(
        "\
fn main() -> unit {
  let s.0: obj = const \"again\"
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
",
    );
    let second = machine
        .load(&program2, &Linked::default())
        .map_err(|error| format!("{error:?}"))
        .and_then(|loaded| machine.run(loaded.entry).map_err(|error| error.to_string()));
    // 鍵を開けるとCが戻って留めを落とすので、取り置きはその前に見る
    let parked = published(100156).map(|at| eml_rt::PagePin::parked(at as usize));
    rs_gate_open(100157);
    let read = rs_gate_wait(100158);
    let closed = machine.close();
    let error = first.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    assert!(
        !matches!(error, RuntimeError::Unrecovered { .. }),
        "{error:?}"
    );
    second.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "again\n");
    assert_eq!(parked, Some(true), "the lent page was not parked");
    assert_eq!(read, 0, "C did not read the string");
    assert_eq!(published(100158), Some(199_680));
}

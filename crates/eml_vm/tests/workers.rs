//! worker 1..N-1 の起動と止める手順 (docs/spec/vm.md の「タスクとスケジュール」)。worker は run の最初の `par` の
//! fork で作り、run の終わりに join する。根に触れる `Machine` は worker を作らない。

use std::sync::Arc;
use std::time::{Duration, Instant};

use eml_rt::Checked;
use eml_vm::{Control, Finished, Limits, Machine, RunConfig, RuntimeError};

use crate::common::parse;

/// `par` の両方の子が値を返し、親が和を表示する。
const PAR: &str = "\
import Std.Par
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> 2)
  println (show (a + b))
";

/// 根を書くだけの関数。読み込むだけで、`Machine` のコードが根に触れることになる。
const ROOTS: &str = "\
fn keep() -> unit {
  let b.0: tobj = box 1
  let u.1: unit = prim root_set[0](b.0)
  return u.1
}
";

fn limits(workers: usize) -> Arc<Control> {
    Arc::new(Control::new(Limits {
        workers,
        ..Limits::default()
    }))
}

/// 検査モードで、worker を `workers` 個まで持てる run を1回行う。
fn run(src: &str, workers: usize) -> (String, Result<Finished, RuntimeError>) {
    let (host, out) = eml_vm::capture_host(Default::default());
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(limits(workers));
    let result = eml_vm::execute(&eml_test_support::lir(src), &config, host);
    (out.contents(), result)
}

#[test]
fn a_run_without_par_starts_no_worker() {
    let src = "\
main : Unit -> <IO> Unit
main () = println \"no par\"
";
    let (out, result) = run(src, 4);
    assert_eq!(out, "no par\n");
    assert_eq!(result.unwrap().workers, 0);
}

/// 最初の fork が worker 1..N-1 をすべて作り、同じ fork をやり直す。出力は worker が1つの run と同じである。
#[test]
fn the_first_fork_starts_the_workers() {
    let (out, result) = run(PAR, 4);
    assert_eq!(out, "3\n");
    assert_eq!(result.unwrap().workers, 3);
    let (alone, result) = run(PAR, 1);
    assert_eq!(alone, out);
    assert_eq!(result.unwrap().workers, 0);
}

/// 根に触れるコードを読み込んだ `Machine` は、根がまだなくても worker を作らない。前の run が worker を作った
/// `Machine` でも、根に触れるコードを足した後の run は作らない。
#[test]
fn code_with_roots_starts_no_worker() {
    let par = eml_test_support::lir(PAR);
    let roots = parse(ROOTS);

    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(limits(4), host);
    machine.load(&roots).unwrap();
    let entry = machine.load(&par).unwrap().entry;
    assert_eq!(machine.run(entry).unwrap().workers, 0);
    machine.close().unwrap();
    assert_eq!(out.contents(), "3\n");

    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(limits(4), host);
    let entry = machine.load(&par).unwrap().entry;
    assert_eq!(machine.run(entry).unwrap().workers, 3);
    machine.load(&roots).unwrap();
    assert_eq!(machine.run(entry).unwrap().workers, 0);
    machine.close().unwrap();
    assert_eq!(out.contents(), "3\n3\n");
}

/// worker を作った後で worker 0 が回り続ける run を、別のスレッドの割り込みが止める。眠っている worker も
/// 起こされて抜けるので、run は終わる。2 秒は止まりの見張りである。
#[test]
fn an_interrupt_ends_a_run_with_idle_workers() {
    let src = "\
import Std.Par
forever : Int -> <Panic> Int
forever n = forever (n + 1)
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> 2)
  println (show (forever (a + b)))
";
    let program = eml_test_support::lir(src);
    let control = limits(2);
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(Arc::clone(&control));
    let interrupter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
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

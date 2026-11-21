//! worker 1..N-1の起動と止める手順 (docs/spec/vm.mdの「タスクとスケジュール」)。workerはrunの始め、最初の命令の前に
//! 作り、runの終わりにjoinする。根に触れる`Machine`はworkerを作らない。

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use eml_rt::{Checked, Fault};
use eml_sched::config::Sched;
use eml_sched::driver::Clock;
use eml_sched::error::RunFault;
use eml_sched::{Control, Limits};
use eml_vm::{Finished, Machine, RunConfig, RuntimeError};

use eml_test_support::schedule_independent;

use crate::common::{parse, trusted};

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
    run_program(&eml_test_support::lir(src), limits(workers))
}

/// 検査モードで、`control` の上限の run を1回行う。
fn run_program(
    program: &eml_lir::Trusted,
    control: Arc<Control>,
) -> (String, Result<Finished, RuntimeError>) {
    let (host, out) = eml_vm::capture_host(Default::default());
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(control);
    let result = eml_vm::execute(program, &eml_vm::Linked::default(), &config, host);
    (out.contents(), result)
}

/// 止まらない run を、別のスレッドから 50 ms 後に割り込んで止める。2 秒は止まりの見張りである。
fn interrupted(src: &str, config: impl FnOnce(RunConfig) -> RunConfig) {
    let program = eml_test_support::lir(src);
    let control = limits(2);
    let config = config(
        RunConfig::default()
            .with_debug_heap(true)
            .with_control(Arc::clone(&control)),
    );
    let interrupter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        control.interrupt();
    });
    let (host, _) = eml_vm::capture_host(Default::default());
    let start = Instant::now();
    let result = eml_vm::execute(&program, &eml_vm::Linked::default(), &config, host);
    let elapsed = start.elapsed();
    interrupter.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
}

/// 止まらない関数。
const SPIN: &str = "\
spin : Int -> <Panic> Int
spin n = spin (n + 1)
";

/// `par` の組の配置と `Panic` のエフェクト。LIR の頭に置く。
const HEAD: &str = "\
layout (,) { (,)(tobj, tobj) }
effect Prelude.Panic { never panicked(obj) -> never }
";

/// 左の子の`one`。エントリより後ろに置く。
const ONE: &str = "\
fn one(p.0: unit) -> tobj {
  let n.1: tobj = box 1
  return n.1
}
";

#[test]
fn a_run_with_workers_starts_them_at_the_start() {
    let src = "\
main : Unit -> <IO> Unit
main () = println \"no par\"
";
    let (out, result) = run(src, 4);
    assert_eq!(out, "no par\n");
    assert_eq!(result.unwrap().workers, 3);
}

/// worker 1..N-1は最初のforkの前にすべてある。出力はworkerが1つのrunと同じである。
#[test]
fn the_workers_exist_before_the_first_fork() {
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
    let roots = trusted(ROOTS);

    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(limits(4), host);
    machine.load(&roots, &eml_vm::Linked::default()).unwrap();
    // SAFETY: P2: 前に読み込んだプログラムと名前が同じ関数は同じコードで、根は書いた型のまま読む。debugのビルドは比べて確かめる
    let entry = unsafe { machine.extend(&par, &eml_vm::Linked::default()) }.unwrap();
    assert_eq!(machine.run(entry).unwrap().workers, 0);
    machine.close().unwrap();
    assert_eq!(out.contents(), "3\n");

    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(limits(4), host);
    let entry = machine.load(&par, &eml_vm::Linked::default()).unwrap();
    assert_eq!(machine.run(entry).unwrap().workers, 3);
    // SAFETY: P2: 前に読み込んだプログラムと名前が同じ関数は同じコードで、根は書いた型のまま読む。debugのビルドは比べて確かめる
    unsafe { machine.extend(&roots, &eml_vm::Linked::default()) }.unwrap();
    assert_eq!(machine.run(entry).unwrap().workers, 0);
    machine.close().unwrap();
    assert_eq!(out.contents(), "3\n3\n");
}

/// worker を作った後で worker 0 が回り続ける run を、別のスレッドの割り込みが止める。眠っている worker も
/// 起こされて抜けるので、run は終わる。同じ上限で `forever` の前までを流した run が worker を作るので、止めた run
/// も worker を作っている。
#[test]
fn an_interrupt_ends_a_run_with_idle_workers() {
    let (_, result) = run(PAR, 2);
    assert_eq!(result.unwrap().workers, 1);
    let src = "\
import Std.Par
forever : Int -> <Panic> Int
forever n = forever (n + 1)
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> 2)
  println (show (forever (a + b)))
";
    interrupted(src, |config| config);
}

/// 右の子がリストを作り、親が join の後にたどって和を表示する。worker が2つの run では右の子をほかの worker に
/// 置き、その子が確保した物体を親が別のスレッドから手放す。出力は worker が1つの run と同じである。
#[test]
fn a_child_runs_on_another_worker() {
    let src = "\
import Std.Par
build : Int -> <Panic> List Int
build n = if n == 0 then [] else n :: build (n - 1)
total : List Int -> <Panic> Int
total xs = match xs with
  | [] -> 0
  | y :: ys -> y + total ys
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> build 100)
  println (show (a + total b))
";
    let (alone, result) = run(src, 1);
    assert_eq!(alone, "5051\n");
    let one = result.unwrap();
    assert_eq!(one.placed, 0);
    let (out, result) = run(src, 2);
    assert_eq!(out, alone);
    let finished = result.unwrap();
    assert_eq!(finished.workers, 1);
    assert!(finished.placed >= 1, "{finished:?}");
    assert_eq!(
        schedule_independent(finished.stats),
        schedule_independent(one.stats)
    );
}

/// 右の子をほかの worker に置いても、`RunStats` の数はすべての worker の和で、worker が1つの run と同じになる。
/// 山は最大を取るので、`peak_stack` も同じである。少なくとも1つの子をほかの worker に置いたこと (`placed`) を
/// 確かめるので、その worker の仕事を足し漏らしていれば数が減る。どの worker に置くかはスレッドの順で決まる。
#[test]
fn stats_at_two_workers_equal_one_worker() {
    let src = "\
import Std.Par
tree : Int -> <Panic> Int
tree d = if d == 0 then 1 else
  let (a, b) = Par.par (fn () -> tree (d - 1)) (fn () -> tree (d - 1))
  a + b
main : Unit -> <IO, Panic> Unit
main () = println (show (tree 3))
";
    let (alone, result) = run(src, 1);
    let one = result.unwrap();
    assert_ne!(one.stats, Default::default());
    for workers in [2, 8] {
        let (out, result) = run(src, workers);
        assert_eq!(out, alone);
        let finished = result.unwrap();
        assert!(finished.placed >= 1, "{finished:?}");
        assert_eq!(
            schedule_independent(finished.stats),
            schedule_independent(one.stats),
            "workers {workers}"
        );
    }
}

/// 表示する run でも worker を作る。結果の文字列は右の子が worker 1 のヒープに作ったもので、写して手放すと
/// worker 1 のヒープに解放が積まれる。終わりの検査の前にそれを取り込むので、検査モードの確かめは通る。
#[test]
fn a_shown_result_from_a_child_is_released_before_the_checks() {
    let text = format!(
        "{HEAD}\
fn shown() -> obj {{
  let t.0: obj = par (,) #0(&one, &text)
  let (x.1: tobj, s.2: tobj) = unbox t.0 (,)
  dup s.2
  decref t.0
  return s.2
}}
fn text(p.0: unit) -> tobj {{
  let s.1: obj = extern \"Prelude.Show Int.show\"(12345)
  return s.1
}}
{ONE}"
    );
    let program = trusted(&text);
    let (host, _) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(limits(2), host);
    let entry = machine.load(&program, &eml_vm::Linked::default()).unwrap();
    // SAFETY: P4: エントリは文字列を返す
    let (finished, shown) = unsafe { machine.run_shown(entry, 100) }.unwrap();
    assert_eq!(shown.as_deref(), Some("12345"));
    assert_eq!((finished.workers, finished.placed), (1, 1));
    machine.close().unwrap();
}

/// 両方の子が回り続ける。worker 1 の右の子も自分の語で割り込みを読んで抜ける。
#[test]
fn an_interrupt_stops_a_child_on_another_worker() {
    let src = format!(
        "import Std.Par\n{SPIN}\
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> spin 0) (fn () -> spin 0)
  println (show (a + b))
"
    );
    interrupted(&src, |config| config);
}

/// 左の子はすぐに終わり、worker 0 は join で右の子を待って眠る。割り込みで worker 1 が抜け、眠っている worker 0 も
/// 止まる。worker 0 を起こすのは、止めるビットを見た監視のスレッドか、抜ける worker 1 で、このテストはどちらかを区別しない。
#[test]
fn an_interrupt_reaches_worker_0_parked_in_join_wait() {
    let src = format!(
        "import Std.Par\n{SPIN}\
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> spin 0)
  println (show (a + b))
"
    );
    interrupted(&src, |config| config);
}

/// 本物の時計で自己パイプのない run。worker 0 は driver の待ちで、worker 1 は空きの待ちで眠る。割り込みを立てた
/// 者は誰も起こさないので、刻みごとに起きる監視のスレッドがすべての worker を起こす。
#[test]
fn an_interrupt_reaches_a_parked_worker_without_a_doorbell() {
    let src = "\
import Std.Par
import Async
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> 2)
  println (show (a + b))
  Async.scope (fn _ -> Async.sleep_ms 5000)
";
    interrupted(src, |config| config.with_clock(Clock::Real));
}

/// worker 1 に置いたタスクとその `par` の子が動く間に、別のタスクの `exit` が run を捨てる。worker 1 は `END` を読んで
/// 抜け、run は `exit` の終了コードで終わる。置いたタスクも、それが fork した子も捨てる。捨てた run のヒープは2つとも
/// 回復が片付けるので、検査モードの確かめも通る。
#[test]
fn exit_abandons_a_child_on_another_worker() {
    let src = format!(
        "import Std.Par\nimport Async (Async)\n{SPIN}\
body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn (fn () ->
    let (a, b) = Par.par (fn () -> 1) (fn () -> spin 0)
    println (show (a + b)))
  Async.yield_now ()
  (exit 3 : Unit)
main : Unit -> <IO, Panic, Async> Unit
main () = body ()
"
    );
    let (out, result) = run(&src, 2);
    assert_eq!(out, "");
    let finished = result.unwrap();
    assert_eq!(finished.exit, Some(3));
    assert_eq!((finished.workers, finished.placed_tasks), (1, 1));
}

/// 置いた右の子が同じ物体を2回手放す。検査モードのヒープの誤りは worker 1 の上で起き、run の致命的な誤りの欄に
/// 書かれて worker 0 の結果より優先する。
#[test]
fn a_heap_error_in_a_placed_child_ends_the_run() {
    let text = format!(
        "{HEAD}\
fn main() -> unit {{
  let t.0: obj = par (,) #0(&one, &twice)
  decref t.0
  return ()
}}
fn twice(p.0: unit) -> tobj {{
  let n.1: obj = con (,) #0(#5, #6)
  decref n.1
  decref n.1
  let m.2: tobj = box 3
  return m.2
}}
{ONE}"
    );
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(limits(2));
    let (_, result) = crate::common::execute_unverified(&parse(&text), &config);
    let error = result.unwrap_err();
    assert!(
        matches!(
            &error,
            RuntimeError::Fault {
                fault: RunFault::Extern(Fault::Heap(_)),
                function,
                ..
            } if function == "twice"
        ),
        "{error:?}"
    );
}

/// 置いた右の子の深さの上限は、致命的な誤りの欄を通ってrunを終わらせる。文言はworkerが1つのrunと同じである。
#[test]
fn a_depth_error_in_a_placed_child_ends_the_run() {
    let src = "\
import Std.Par
deep : Int -> <Panic> Int
deep n = if n == 0 then 0 else 1 + deep (n - 1)
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> deep 100000)
  println (show (a + b))
";
    let program = eml_test_support::lir(src);
    let depth = |workers| {
        Arc::new(Control::new(Limits {
            workers,
            max_depth: 4096,
            ..Limits::default()
        }))
    };
    let (_, alone) = run_program(&program, depth(1));
    let (_, placed) = run_program(&program, depth(2));
    let (alone, placed) = (alone.unwrap_err(), placed.unwrap_err());
    assert!(alone.to_string().contains("depth"), "{alone}");
    assert_eq!(placed.to_string(), alone.to_string());
}

/// `compute` の仕事の深さの上限は、`Panic` でない実行時エラーなので、その場で run を止める。文言は worker の数によらない
/// (docs/spec/vm.md の「`compute` の仕事」)。
#[test]
fn a_depth_error_in_a_job_stops_the_run() {
    let src = "\
import Async
deep : Int -> <Panic> Int
deep n = if n == 0 then 0 else 1 + deep (n - 1)
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let d = Async.start (fn () -> ())
  Async.await d
  Async.sleep_ms 1
  println \"before\"
  println (show (Async.compute (fn () -> deep 100000)))
";
    let program = eml_test_support::lir(src);
    let depth = |workers| {
        Arc::new(Control::new(Limits {
            workers,
            max_depth: 4096,
            ..Limits::default()
        }))
    };
    let (alone_out, alone) = run_program(&program, depth(1));
    let (out, other) = run_program(&program, depth(2));
    let (alone, other) = (alone.unwrap_err(), other.unwrap_err());
    assert_eq!(alone_out, "before\n");
    assert_eq!(out, "before\n");
    assert!(
        alone
            .to_string()
            .starts_with("the call depth limit is exceeded"),
        "{alone}"
    );
    assert_eq!(other.to_string(), alone.to_string());
}

/// `compute`の仕事は`par`の右の子と同じく空いたworkerに置く。何もしないタスクをworker 1に置いて待ち、続く
/// `sleep_ms`はどのworkerも眠ったときにだけ戻るので、`fork_job`のときにはworker 1が空いている。
/// 仕事はworker 1のヒープにリストを作って返し、出したタスクがworker 0でたどってから手放す。検査モードは、
/// `take_job`が受け取った物体の持ち主の取り違えと漏れを見つける。仕事は`par`の右の子でないので`placed`は増えず、
/// `placed_jobs`だけが増える (docs/spec/vm.mdの「`compute` の仕事」)。
#[test]
fn a_job_is_placed_on_an_idle_worker() {
    let src = "\
import Async
build : Int -> <Panic> List Int
build n = if n == 0 then [] else n :: build (n - 1)
total : List Int -> <Panic> Int
total xs = match xs with
  | [] -> 0
  | y :: ys -> y + total ys
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let d = Async.start (fn () -> ())
  Async.await d
  Async.sleep_ms 1
  println (show (total (Async.compute (fn () -> build 200))))
";
    let (alone, result) = run(src, 1);
    assert_eq!(alone, "20100\n");
    assert_eq!(result.unwrap().placed_jobs, 0);
    let (out, result) = run(src, 2);
    assert_eq!(out, alone);
    let finished = result.unwrap();
    assert_eq!(
        (
            finished.workers,
            finished.placed_tasks,
            finished.placed,
            finished.placed_jobs
        ),
        (1, 1, 0, 1)
    );
}

/// 止まらない仕事がほかのworkerで動いているrunも`exit`で終わる。3つのworkerでは、置く順によりタスクAをworker 1に、
/// 何もしないタスクをworker 2に、Aが始める`exit`のタスクをworker 1に置く。Aは先に眠る。`sleep_ms`はどのworkerも
/// 眠ったときにだけ戻るので、worker 2はタスクを終えて空いたままであり、Aがforkする仕事はほかの空いたworkerに置く。
/// `exit`のタスクはforkの前にworker 1の列に入っているので、仕事をどのworkerが取っても、Aが待つ間にworker 1がそれを
/// 動かす。仕事と待つタスクは捨て、回復がヒープを片付ける。
#[test]
fn exit_abandons_a_job_on_another_worker() {
    let src = format!(
        "import Async (Async)\n{SPIN}\
body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () ->
    Async.sleep_ms 1
    Async.spawn (fn () -> (exit 3 : Unit))
    println (show (Async.compute (fn () -> spin 0))))
  let d = Async.start (fn () -> ())
  Async.await d
  Async.await a
main : Unit -> <IO, Panic, Async> Unit
main () = body ()
"
    );
    let (out, result) = run(&src, 3);
    assert_eq!(out, "");
    let finished = result.unwrap();
    assert_eq!(finished.exit, Some(3));
    assert_eq!((finished.workers, finished.placed_jobs), (2, 1));
}

/// ほかのworkerに置いた仕事でも、取り消しは仕事が終わってから出したタスクに1回だけ届く。3つのworkerでは、置く順に
/// より何もしないタスクをworker 1に、出したタスクをworker 2に置く。`sleep_ms`はどのworkerも眠ったときにだけ戻るので、
/// `fork_job`のときにはworker 1が空いていて、仕事はほかの空いたworkerに置く。仕事の終わりは、出したタスクのいる
/// worker 2へ起こしを送る (docs/spec/vm.md の「取り消しの届け方」)。
#[test]
fn a_cancelled_job_on_another_worker_counts_one_cancel() {
    let src = "\
import Async (Async)
import Async
import MutArray
spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)
main : Unit -> <IO, Panic, Async> Unit
main () =
  let d = Async.start (fn () -> ())
  Async.await d
  Async.sleep_ms 1
  let t = Async.start (fn () ->
    Async.scope (fn s ->
      Async.cancel_scope s
      match try_io (fn () -> Async.compute (fn () -> MutArray.make 2 (spin 40))) with
        | Ok a ->
          drop a
          println \"done\"
        | Err e -> println \"job: \\{show e.kind}\"))
  Async.await t
";
    let (out, result) = run(src, 3);
    assert_eq!(out, "job: Cancelled\n");
    let finished = result.unwrap();
    assert_eq!(finished.stats.cancels, 1);
    assert_eq!(finished.placed_jobs, 1);
}

/// 右の子の`Panic`は子の中で捕まえられ、worker 0でstdが上げ直してrunを終わらせる。その後`Machine`は2つの
/// ヒープを回復して続ける。worker 1のヒープは回復の後に落ちる。同じ`Machine`の次のrunもworkerを作り、閉じるときの
/// リークの確かめも通る。
#[test]
fn recovery_after_an_error_drops_n_heaps() {
    let failing = eml_test_support::lir(
        "\
import Std.Par
main : Unit -> <IO, Panic> Unit
main () =
  println \"start\"
  let (a, b) = Par.par (fn () -> 1) (fn () -> 10 / (1 - 1))
  println (show (a + b))
",
    );
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(limits(2), host);
    let entry = machine.load(&failing, &eml_vm::Linked::default()).unwrap();
    for _ in 0..2 {
        let error = machine.run(entry).unwrap_err();
        assert!(error.to_string().contains("division by zero"), "{error}");
        assert!(!error.is_broken());
    }
    machine.close().unwrap();
    assert_eq!(out.contents(), "start\nstart\n");
}

/// 置いた右の子がworker 1で深さの上限により失敗し、誤りは致命的な誤りの欄を通る。`Machine`は回復し、次のrunも同じ
/// 誤りで終わる。
#[test]
fn recovery_after_a_child_error_on_another_worker() {
    let failing = eml_test_support::lir(
        "\
import Std.Par
deep : Int -> <Panic> Int
deep n = if n == 0 then 0 else 1 + deep (n - 1)
main : Unit -> <IO, Panic> Unit
main () =
  println \"start\"
  let (a, b) = Par.par (fn () -> 1) (fn () -> deep 100000)
  println (show (a + b))
",
    );
    let (host, out) = eml_vm::capture_host(Default::default());
    let control = Arc::new(Control::new(Limits {
        workers: 2,
        max_depth: 4096,
        ..Limits::default()
    }));
    let mut machine = Machine::<Checked>::new(control, host);
    let entry = machine.load(&failing, &eml_vm::Linked::default()).unwrap();
    for _ in 0..2 {
        let error = machine.run(entry).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the call depth limit is exceeded in `deep`"
        );
        assert!(!error.is_broken());
    }
    machine.close().unwrap();
    assert_eq!(out.contents(), "start\nstart\n");
}

/// 一方の子の実行時エラーは、もう一方の子が走り続けていてもrunを終わらせる。右の子はworker 1で走る。戻らなければ
/// 止まらないので、見張りが2秒後に割り込む。
#[test]
fn a_child_error_ends_the_run_while_the_other_child_runs() {
    for (left, right) in [("spin 0", "deep 100000"), ("deep 100000", "spin 0")] {
        let src = format!(
            "import Std.Par\n{SPIN}\
deep : Int -> <Panic> Int
deep n = if n == 0 then 0 else 1 + deep (n - 1)
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> {left}) (fn () -> {right})
  println (show (a + b))
"
        );
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            max_depth: 4096,
            ..Limits::default()
        }));
        let (done, wait) = std::sync::mpsc::channel::<()>();
        let watched = Arc::clone(&control);
        let watchdog = std::thread::spawn(move || {
            if wait.recv_timeout(Duration::from_secs(2)).is_err() {
                watched.interrupt();
            }
        });
        let (out, result) = run_program(&eml_test_support::lir(&src), control);
        done.send(()).unwrap();
        watchdog.join().unwrap();
        assert_eq!(out, "", "{left}, {right}");
        assert_eq!(
            result.unwrap_err().to_string(),
            "the call depth limit is exceeded in `deep`",
            "{left}, {right}"
        );
    }
}

/// 両方の子が失敗する。左はworker 0で、右はworker 1で。runは2つの誤りのうち最初に書かれた方で終わる。止まらず、
/// 中断も報告しない。
#[test]
fn two_failing_children_on_two_workers_report_the_first_error() {
    let src = "\
import Std.Par
deep_left : Int -> <Panic> Int
deep_left n = if n == 0 then 0 else 1 + deep_left (n - 1)
deep_right : Int -> <Panic> Int
deep_right n = if n == 0 then 0 else 1 + deep_right (n - 1)
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> deep_left 100000) (fn () -> deep_right 100000)
  println (show (a + b))
";
    let program = eml_test_support::lir(src);
    for round in 0..20 {
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            max_depth: 4096,
            ..Limits::default()
        }));
        let (done, wait) = std::sync::mpsc::channel::<()>();
        let watched = Arc::clone(&control);
        let watchdog = std::thread::spawn(move || {
            if wait.recv_timeout(Duration::from_secs(2)).is_err() {
                watched.interrupt();
            }
        });
        let (out, result) = run_program(&program, control);
        done.send(()).unwrap();
        watchdog.join().unwrap();
        assert_eq!(out, "", "round {round}");
        let error = result.unwrap_err().to_string();
        assert!(
            [
                "the call depth limit is exceeded in `deep_left`",
                "the call depth limit is exceeded in `deep_right`"
            ]
            .contains(&error.as_str()),
            "round {round}: {error}"
        );
    }
}

/// worker `worker`の止める語に`END`が立つまで、つまりどれかのworkerが失敗してrunが止まりつつあるときまで、行
/// `gate`を持つhost。行`wait`は`gate`が来るまで持つので、早い子は遅い子がgateに入るまで失敗しない。10秒は止まりの
/// 見張りである。
struct EndGate {
    inner: eml_vm::CaptureHost,
    control: Arc<Control>,
    worker: usize,
    entered: std::sync::atomic::AtomicBool,
}

impl eml_rt::Host for EndGate {
    fn write_line(&self, stream: eml_rt::Stream, line: &str) -> std::io::Result<()> {
        if line == "wait" {
            let start = Instant::now();
            while !self.entered.load(std::sync::atomic::Ordering::Acquire)
                && start.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_micros(200));
            }
        }
        if line == "gate" {
            self.entered
                .store(true, std::sync::atomic::Ordering::Release);
            let word = self.control.word(self.worker);
            let start = Instant::now();
            while word.load(std::sync::atomic::Ordering::Relaxed) & Control::END == 0
                && start.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_micros(200));
            }
        }
        self.inner.write_line(stream, line)
    }
    fn write(&self, stream: eml_rt::Stream, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write(stream, bytes)
    }
    fn flush(&self) -> std::io::Result<()> {
        self.inner.flush()
    }
    fn open(&self, path: &str) -> std::io::Result<eml_rt::Opened> {
        self.inner.open(path)
    }
}

/// 一方の子は、もう一方がgateに入ってから深さで失敗し、もう一方はrunが止まりつつあるときまでgateで待ち、その後で
/// 割り算で失敗する。間に安全点はない。最初に書かれた誤りが報告され、どのworkerが書いたかによらない。左の子が早い
/// 方ならworker 0の誤り、右の子が早い方ならworker 1の誤りである。
///
/// 行("deep", "late", 1)は、`entry_body`で`sched.run.record(result)`をworkerのjoinの後ろへ動かす変更を捕まえる。
/// worker 0が先に書かず、止められたworker 1の`late`の誤りが先に書かれるからである。行("late", "deep", 0)は、
/// `record`を消す変更を捕まえる。worker 0の`late`の誤りが残り、worker 1の先に書かれた誤りを上書きするからである。
/// 早い子はgateに入るまで待つので、どちらの行もタイミングに頼らない。
#[test]
fn the_first_error_written_wins_on_either_worker() {
    for (left, right, gated) in [("deep", "late", 1), ("late", "deep", 0)] {
        let entry = |name: &str| if name == "deep" { "early" } else { "late" };
        let (entry_left, entry_right) = (entry(left), entry(right));
        let text = format!(
            "{HEAD}\
fn main() -> unit {{
  let t.0: obj = par (,) #0(&{entry_left}, &{entry_right})
  decref t.0
  return ()
}}
fn early(p.0: unit) -> tobj {{
  let s.1: obj = const \"wait\"
  let u.2: unit = extern Prelude.println(s.1)
  let t.3: tobj = call deep(())
  return t.3
}}
fn deep(p.0: unit) -> tobj {{
  let t.1: tobj = call deep(())
  return t.1
}}
fn late(p.0: unit) -> tobj {{
  let s.1: obj = const \"gate\"
  let u.2: unit = extern Prelude.println(s.1)
  let n.3: int = extern \"Prelude.Div Int.%\"(1, 0)
  let m.4: tobj = box n.3
  return m.4
}}
"
        );
        let control = Arc::new(Control::new(Limits {
            workers: 2,
            max_depth: 1000,
            ..Limits::default()
        }));
        let host = Arc::new(EndGate {
            inner: eml_vm::CaptureHost::new(Default::default()),
            control: Arc::clone(&control),
            worker: gated,
            entered: Default::default(),
        });
        let config = RunConfig::default()
            .with_debug_heap(true)
            .with_control(control);
        let error = eml_vm::execute(
            &trusted(&text),
            &eml_vm::Linked::default(),
            &config,
            eml_rt::HostRef::new(host as Arc<dyn eml_rt::Host>),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the call depth limit is exceeded in `deep`",
            "{left}, {right}"
        );
    }
}

/// worker 1..N-1のタスクのタイマーと右の子。仮想の時計は、どのworkerも動かないときだけ進むので、子が終わるまで
/// タイマーは満ちない。出力はworkerが1つのrunと同じである。何もしないタスクはworker 1に、タイマーのタスクはworker 2に
/// 置く。間の`sleep_ms`は、どのworkerも眠ったときにだけ戻るので、forkのときにはworker 1が空いていて、右の子は
/// 空いたworkerに置く。
#[test]
fn a_timer_task_and_a_child_on_other_workers_agree() {
    let src = "\
import Std.Par
import Async
count : Int -> Int -> <Panic> Int
count n acc = if n == 0 then acc else count (n - 1) (acc + 1)
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let d = Async.start (fn () -> ())
  Async.await d
  Async.sleep_ms 1
  let t = Async.start (fn () ->
    Async.sleep_ms 5
    println \"timer\")
  let (a, b) = Par.par (fn () -> 1) (fn () -> count 20000 0)
  println (show (a + b))
  Async.await t
";
    let (alone, result) = run(src, 1);
    assert_eq!(alone, "20001\ntimer\n");
    result.unwrap();
    let (out, result) = run(src, 3);
    assert_eq!(out, alone);
    let finished = result.unwrap();
    assert_eq!(finished.placed, 1);
    assert_eq!(finished.placed_tasks, 2);
}

/// 深さ 3 の `par` の木は 7 回 fork する。worker が8つあれば、空いている worker はいつも残るので、どの fork も
/// 右の子をほかの worker に置く。入れ子の子は worker 1..N-1 の fork も置く。
#[test]
fn nested_children_place_on_eight_workers() {
    let src = "\
import Std.Par
tree : Int -> <Panic> Int
tree d = if d == 0 then 1 else
  let (a, b) = Par.par (fn () -> tree (d - 1)) (fn () -> tree (d - 1))
  a + b
main : Unit -> <IO, Panic> Unit
main () = println (show (tree 3))
";
    let (alone, _) = run(src, 1);
    assert_eq!(alone, "8\n");
    let (out, result) = run(src, 8);
    assert_eq!(out, alone);
    let finished = result.unwrap();
    assert_eq!((finished.workers, finished.placed), (7, 7));
}

/// workerは最初の`start`の前にあり、最初に置くタスクはworker 1に置くので、タスクを1つしか始めないプログラムでも
/// ほかのworkerで動く。出力はworkerが1つのrunと同じである。
#[test]
fn the_first_start_places_on_worker_1() {
    let src = "\
import Async
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let t = Async.start (fn () -> 20 + 1)
  println (show (Async.await t))
";
    let (alone, result) = run(src, 1);
    assert_eq!(alone, "21\n");
    let one = result.unwrap();
    assert_eq!((one.workers, one.placed_tasks), (0, 0));
    let (out, result) = run(src, 4);
    assert_eq!(out, alone);
    let finished = result.unwrap();
    assert_eq!((finished.workers, finished.placed_tasks), (3, 1));
}

/// worker 1 に置いたタスクの `exit` は、run を捨てて `END` を立て、すべての worker を起こす。`await` で眠る worker 0
/// も止まり、run は `exit` の終了コードで終わる (docs/spec/vm.md の「誤り、止めること、終わり」)。
#[test]
fn exit_in_a_task_on_another_worker() {
    let src = "\
import Async
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let t = Async.start (fn () -> (exit 7 : Unit))
  Async.await t
  println \"unreachable\"
";
    let (out, result) = run(src, 2);
    assert_eq!(out, "");
    let finished = result.unwrap();
    assert_eq!(finished.exit, Some(7));
    assert_eq!((finished.workers, finished.placed_tasks), (1, 1));
}

/// worker 1 のタスクのタイマーも仮想の時計を進める。時計を進めるのは worker 0 で、すべての worker が眠ったときに、
/// すべての区画のいちばん早い期限まで進めて、期限の来た区画の持ち主を起こす (docs/spec/vm.md の「時計と driver」)。
#[test]
fn a_timer_on_worker_1_advances_the_virtual_clock() {
    let src = "\
import Async
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let t = Async.start (fn () ->
    Async.sleep_ms 5
    println \"timer\")
  Async.await t
  println \"done\"
";
    let (alone, _) = run(src, 1);
    assert_eq!(alone, "timer\ndone\n");
    let (out, result) = run(src, 2);
    assert_eq!(out, alone);
    assert_eq!(result.unwrap().placed_tasks, 1);
}

/// 本物の時計でも、worker 1 のタスクのタイマーは満ちる。worker 1 は自分の区画のいちばん早い期限に自分で起き、
/// worker 0 は自分の区画の期限でしか起きない (docs/spec/vm.md の「時計と driver」)。時間は確かめない。
#[test]
fn a_timer_on_worker_1_fires_on_the_real_clock() {
    let src = "\
import Async
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let t = Async.start (fn () ->
    Async.sleep_ms 5
    println \"timer\")
  Async.await t
  println \"done\"
";
    for sched in [Sched::Sequential, Sched::Normal] {
        let (host, out) = eml_vm::capture_host(Default::default());
        let config = RunConfig::default()
            .with_debug_heap(true)
            .with_clock(Clock::Real)
            .with_sched(sched)
            .with_control(limits(2));
        let result = eml_vm::execute(
            &eml_test_support::lir(src),
            &eml_vm::Linked::default(),
            &config,
            host,
        );
        assert_eq!(out.contents(), "timer\ndone\n", "{sched:?}");
        assert_eq!(result.unwrap().placed_tasks, 1, "{sched:?}");
    }
}

/// 4つの worker に置いた親のタスクが、子のハンドルを1つはすぐに、1つは譲った後に落とす。落としたハンドルは mail に
/// 積まれ、最初に集めた worker が処理するので、どの worker が処理しても子のセルを1回だけ放す。子の `Lin` の結果も
/// 落とすので、検査モードの漏れの検査が通る (docs/spec/vm.md の「タスクとスケジュール」)。
#[test]
fn handles_dropped_on_every_worker_are_freed_once() {
    let src = "\
import Async (Async)
import Async
import MutArray
spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)
child : Unit -> <IO, Panic, Async> MutArray Int
child () =
  let _ = spin 200
  Async.yield_now ()
  MutArray.make 2 0
parent : Unit -> <IO, Panic, Async> Unit
parent () =
  let a = Async.start child
  let b = Async.start child
  drop a
  Async.yield_now ()
  drop b
start_all : Int -> <IO, Panic, Async> List (Async.Task Unit)
start_all n = if n == 0 then [] else Async.start parent :: start_all (n - 1)
await_all : List (Async.Task Unit) -> <IO, Panic, Async> Unit
await_all ts = match ts with
  | [] -> ()
  | t :: rest ->
    Async.await t
    await_all rest
main : Unit -> <IO, Panic, Async> Unit
main () =
  await_all (start_all 16)
  println \"done\"
";
    let (out, result) = run(src, 4);
    assert_eq!(out, "done\n");
    let finished = result.unwrap();
    assert!(finished.placed_tasks >= 1, "{finished:?}");
}

/// 速いモードの普通の方針で、4つの worker にタスクを置く。どのプログラムも、検査モードの worker が1つの逐次の run と
/// 同じ出力で終わり、少なくとも1つのタスクをほかの worker に置く。速いモードは UI テストを N = 2 以上で流さないので、
/// ここで確かめる。
#[test]
fn the_fast_mode_runs_tasks_on_four_workers() {
    let fib = "\
import Async (Async)
import Async
fib : Int -> <Panic> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)
start_all : Int -> <IO, Panic, Async> List (Async.Task Int)
start_all n = if n == 0 then [] else Async.start (fn () -> fib (n + 4)) :: start_all (n - 1)
sum_all : List (Async.Task Int) -> Int -> <IO, Panic, Async> Int
sum_all ts acc = match ts with
  | [] -> acc
  | t :: rest -> sum_all rest (acc + Async.await t)
main : Unit -> <IO, Panic, Async> Unit
main () = println (show (sum_all (start_all 16) 0))
";
    let cancel = include_str!("../../../tests/ui/run/async/cancel.em");
    let echo = include_str!("../../../tests/ui/run/async/net_echo.em");
    for src in [fib, cancel, echo] {
        let program = eml_test_support::lir(src);
        let (alone, result) = run_program(&program, limits(1));
        result.unwrap_or_else(|error| panic!("{error}"));
        let (host, out) = eml_vm::capture_host(Default::default());
        let config = RunConfig::default()
            .with_sched(Sched::Normal)
            .with_control(limits(4));
        let finished = eml_vm::execute(&program, &eml_vm::Linked::default(), &config, host)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(out.contents(), alone);
        assert!(finished.placed_tasks >= 1, "{finished:?}");
    }
}

/// 速いモードで、大きな物体を作る子を4つの worker に置く。`Std.Par` の関数は子で配列を作るので、持ち主が、別の
/// worker の空けた大きな物体のページを取り戻す (docs/spec/object-model.md の「別のスレッドからの解放」)。出力は検査
/// モードの worker が1つの run と同じで、少なくとも1つの子をほかの worker に置く。
#[test]
fn the_fast_mode_runs_the_data_parallel_functions_on_four_workers() {
    let collections = include_str!("../../../tests/ui/run/par/collections.em");
    let boxed = include_str!("../../../tests/ui/run/par/map_boxed_input.em");
    for src in [collections, boxed] {
        let program = eml_test_support::lir(src);
        let (alone, result) = run_program(&program, limits(1));
        result.unwrap_or_else(|error| panic!("{error}"));
        let (host, out) = eml_vm::capture_host(Default::default());
        let config = RunConfig::default()
            .with_sched(Sched::Normal)
            .with_control(limits(4));
        let finished = eml_vm::execute(&program, &eml_vm::Linked::default(), &config, host)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(out.contents(), alone);
        assert!(finished.placed >= 1, "{finished:?}");
    }
}

/// 行 "a" の書き込みを、別のスレッドが "b" を書くまで止める Host。lock は持たずに条件変数で待ち、10 秒で自分から抜ける。
/// ほかは中の `CaptureHost` に任せる。
struct BlockingHost {
    inner: eml_vm::CaptureHost,
    /// "a" が `write_line` に入った時刻と、"b" を書いた時刻。
    times: Mutex<(Option<Instant>, Option<Instant>)>,
    wrote_b: Condvar,
}

impl BlockingHost {
    fn times(&self) -> MutexGuard<'_, (Option<Instant>, Option<Instant>)> {
        self.times.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl eml_rt::Host for BlockingHost {
    fn write_line(&self, stream: eml_rt::Stream, line: &str) -> std::io::Result<()> {
        if line == "a" {
            let mut times = self.times();
            times.0 = Some(Instant::now());
            let (times, _) = self
                .wrote_b
                .wait_timeout_while(times, Duration::from_secs(10), |times| times.1.is_none())
                .unwrap_or_else(PoisonError::into_inner);
            drop(times);
            self.inner.write_line(stream, line)
        } else {
            let written = self.inner.write_line(stream, line);
            self.times().1 = Some(Instant::now());
            self.wrote_b.notify_all();
            written
        }
    }
    fn write(&self, stream: eml_rt::Stream, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write(stream, bytes)
    }
    fn flush(&self) -> std::io::Result<()> {
        self.inner.flush()
    }
    fn open(&self, path: &str) -> std::io::Result<eml_rt::Opened> {
        self.inner.open(path)
    }
}

/// worker 1に置いたタスクは、`main`がworker 0をexternの中で止めている間も動く。根のタスクの`println "a"`はタスクが
/// "b"を書くまで止まるので、"b"が先に出る (docs/spec/vm.mdの「`Async`のタスク」)。
#[test]
fn tasks_on_other_workers_run_while_main_blocks_in_an_extern() {
    let src = "\
import Async
main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let t = Async.start (fn () -> println \"b\")
  println \"a\"
  Async.await t
";
    let inner = eml_vm::CaptureHost::new(Default::default());
    let out = inner.output();
    let host = Arc::new(BlockingHost {
        inner,
        times: Default::default(),
        wrote_b: Default::default(),
    });
    let config = RunConfig::default()
        .with_sched(Sched::Sequential)
        .with_control(limits(2));
    let finished = eml_vm::execute(
        &eml_test_support::lir(src),
        &eml_vm::Linked::default(),
        &config,
        eml_rt::HostRef::new(Arc::clone(&host) as Arc<dyn eml_rt::Host>),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "b\na\n");
    assert_eq!(finished.placed_tasks, 1, "{finished:?}");
}

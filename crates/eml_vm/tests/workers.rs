//! worker 1..N-1 の起動と止める手順 (docs/spec/vm.md の「タスクとスケジュール」)。worker は run の最初の `par` の
//! fork で作り、run の終わりに join する。根に触れる `Machine` は worker を作らない。

use std::sync::Arc;
use std::time::{Duration, Instant};

use eml_rt::{Checked, Fault};
use eml_vm::{Clock, Control, Finished, Limits, Machine, RunConfig, RuntimeError, VmFault};

use eml_test_support::schedule_independent;

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
    run_program(&eml_test_support::lir(src), limits(workers))
}

/// 検査モードで、`control` の上限の run を1回行う。
fn run_program(
    program: &eml_lir::Program,
    control: Arc<Control>,
) -> (String, Result<Finished, RuntimeError>) {
    let (host, out) = eml_vm::capture_host(Default::default());
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(control);
    let result = eml_vm::execute(program, &config, host);
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
    let result = eml_vm::execute(&program, &config, host);
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

/// `par` の右の子の中断に要る関数と、左の子の `one`。エントリより後ろに置く。
const FORKS: &str = "\
clause Prelude.Panic.panicked fn fork$Prelude.Panic.panicked(ev.0: obj, a.1: obj) -> obj poly {
  let c.2: tobj = closure reperform$Prelude.Panic.panicked(a.1)
  let u.3: unit = prim child_abort(ev.0, c.2)
  decref ev.0
  let d.4: obj poly = prim dummy()
  return d.4
}
uniform from 1 fn reperform$Prelude.Panic.panicked(a.0: obj, u.1: unit) -> tobj poly {
  let r.2: tobj poly = extern Prelude.panic(a.0)
  return r.2
}
fn one(p.0: unit) -> tobj {
  let n.1: tobj = box 1
  return n.1
}
";

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
/// 山は最大を取るので、`peak_stack` も同じである。`par` の木は子を worker 1..7 に散らすので、どの worker の
/// 仕事も足し漏れていれば数が減る。
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
{FORKS}"
    );
    let program = parse(&text);
    let (host, _) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(limits(2), host);
    let entry = machine.load(&program).unwrap().entry;
    let (finished, shown) = machine.run_shown(entry, 100).unwrap();
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

/// 左の子はすぐに終わり、worker 0 は join で右の子を待って眠る。割り込みは worker 1 が読み、抜けるときにすべての
/// worker を起こすので、眠っている worker 0 も止まる。
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
main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> 2)
  println (show (a + b))
  Async.run (fn () -> Async.sleep_ms 5000)
";
    interrupted(src, |config| config.with_clock(Clock::Real));
}

/// 右の子が worker 1 で回り続ける間に、別のタスクの `exit` が run を捨てる。worker 1 は `END` を読んで抜け、run は
/// `exit` の終了コードで終わる。捨てた run のヒープは2つとも回復が片付けるので、検査モードの確かめも通る。
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
main : Unit -> <IO, Panic> Unit
main () = Async.run body
"
    );
    let (out, result) = run(&src, 2);
    assert_eq!(out, "");
    let finished = result.unwrap();
    assert_eq!(finished.exit, Some(3));
    assert_eq!((finished.workers, finished.placed), (1, 1));
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
{FORKS}"
    );
    let (_, result) = run_program(&parse(&text), limits(2));
    let error = result.unwrap_err();
    assert!(
        matches!(
            &error,
            RuntimeError::Fault {
                fault: VmFault::Extern(Fault::Heap(_)),
                function,
                ..
            } if function == "twice"
        ),
        "{error:?}"
    );
}

/// 置いた右の子の深さの上限は子の誤りで、親の join が報告する。文言は worker が1つの run と同じである。
#[test]
fn a_depth_error_in_a_placed_child_is_reported_at_join() {
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

/// 置いた右の子の誤りの後、`Machine` は2つのヒープを回復して続ける。worker 1 のヒープは回復の後に落ちる。同じ
/// `Machine` の次の run も worker を作り、閉じるときのリークの確かめも通る。
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
    let entry = machine.load(&failing).unwrap().entry;
    for _ in 0..2 {
        let error = machine.run(entry).unwrap_err();
        assert!(error.to_string().contains("division by zero"), "{error}");
        assert!(!error.is_broken());
    }
    machine.close().unwrap();
    assert_eq!(out.contents(), "start\nstart\n");
}

/// worker 0 のタスクのタイマーと worker 1 の右の子。仮想の時計は、どの worker も動かないときだけ進むので、子が
/// 終わるまでタイマーは満ちない。出力は worker が1つの run と同じである。
#[test]
fn a_timer_on_worker_0_and_a_child_on_worker_1_agree() {
    let src = "\
import Std.Par
import Async
count : Int -> Int -> <Panic> Int
count n acc = if n == 0 then acc else count (n - 1) (acc + 1)
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    Async.sleep_ms 5
    println \"timer\")
  let (a, b) = Par.par (fn () -> 1) (fn () -> count 20000 0)
  println (show (a + b))
  Async.await t)
";
    let (alone, result) = run(src, 1);
    assert_eq!(alone, "20001\ntimer\n");
    result.unwrap();
    let (out, result) = run(src, 2);
    assert_eq!(out, alone);
    assert_eq!(result.unwrap().placed, 1);
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

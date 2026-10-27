//! `Std.Async` のタスクの始めと終わり、待ち、失敗と `Async.run` (docs/spec/vm.md の「`Async` のタスク」)。どのプログラムも検査モードの
//! 逐次と速いモードで流し、標準出力を比べる。

use eml_test_support::{execute, lir, run, run_stats, schedule_independent};
use eml_vm::{RunConfig, Sched};

/// 検査モードと速いモードで流し、どちらも成功して同じ標準出力を返すことを確かめて、その出力を返す。
fn run_both(src: &str) -> String {
    let (checked, result) = run(src);
    result.unwrap_or_else(|error| panic!("the checked run failed: {error}"));
    let (fast, result) = execute(&lir(src), &RunConfig::default());
    result.unwrap_or_else(|error| panic!("the fast run failed: {error}"));
    assert_eq!(fast, checked, "the fast mode printed something else");
    checked
}

#[test]
fn run_returns_the_value_of_its_body() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = println (show (Async.run (fn () -> 41 + 1)))
";
    assert_eq!(run_both(src), "42\n");
}

/// タスクの中の `Async.run` は、そのタスクのスタックの上で自分の根のスコープを待つ。
#[test]
fn a_run_inside_a_task_waits_for_its_own_scope() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    Async.run (fn () -> println \"inner\")
    println \"outer\")
";
    assert_eq!(run_both(src), "inner\nouter\n");
}

/// `Async.run` は、本体が始めた子と、子が始めた孫を待ってから戻る。孫も同じ根のスコープに入る。seed の方針は実行待ちの
/// 列から乱数で選ぶので、出力の順がスケジュールによらないように、`b` を出力するタスクは `a` を出力した後で始める。
const TWO_CHILDREN: &str = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    println \"main\"
    Async.spawn (fn () ->
      println \"a\"
      Async.spawn (fn () -> println \"b\")))
";

#[test]
fn run_waits_for_the_tasks_started_in_its_scope() {
    assert_eq!(run_both(TWO_CHILDREN), "main\na\nb\n");
}

/// seed の方針でも、出力とスケジュールによらない `RunStats` は逐次と同じである。
#[test]
fn the_seeded_runs_agree_with_the_sequential_run() {
    let program = lir(TWO_CHILDREN);
    let checked = || RunConfig::default().with_debug_heap(true);
    let (out, sequential) = execute(&program, &checked());
    let sequential = sequential.expect("the sequential run");
    for seed in [1, 2026] {
        let (seeded_out, seeded) = execute(&program, &checked().with_sched(Sched::Seeded(seed)));
        let seeded = seeded.unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert_eq!(seeded_out, out, "seed {seed}");
        assert_eq!(
            schedule_independent(seeded),
            schedule_independent(sequential),
            "seed {seed}"
        );
    }
}

/// 相手が終わる前の `await` は継続だけになって待ち、終わった後の `await` もいつも継続を渡してから再開する。
#[test]
fn await_returns_the_value_before_and_after_the_task_ends() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let before = Async.start (fn () -> \"x\")
    println (Async.await before)
    let after = Async.start (fn () -> \"x\")
    Async.yield_now ()
    println (Async.await after))
";
    assert_eq!(run_both(src), "x\nx\n");
}

/// `yield_now` は実行待ちの列の後ろへ回るので、逐次の方針では3つのタスクが順に1行ずつ出力する。
#[test]
fn yield_now_takes_turns() {
    let src = "\
import Async (Async)
import Async
twice : String -> Unit -> <IO, Async> Unit
twice name () =
  println (name ++ \"1\")
  Async.yield_now ()
  println (name ++ \"2\")
  Async.yield_now ()
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    Async.spawn (twice \"a\")
    Async.spawn (twice \"b\")
    Async.spawn (twice \"c\"))
";
    assert_eq!(run_both(src), "a1\nb1\nc1\na2\nb2\nc2\n");
}

/// 子の `panic` は `await` が文言のまま起こし直す。捕まえなければ `run` がもう一度起こし直して run の誤りになり、
/// `await` を `try_panic` で囲めば捕まえられる。
#[test]
fn await_raises_the_panic_of_the_task_again() {
    let uncaught = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> panic \"boom\")
    let n : Int = Async.await t
    println (show n))
";
    let (_, checked) = run(uncaught);
    let checked = checked.expect_err("the checked run fails").to_string();
    // 起こし直した失敗の位置は、`run` の中の std の呼び出しである (docs/spec/stdlib.md の「実行時エラー」)
    assert!(
        checked.starts_with("panicked: boom\n  at <std>/Async.em:"),
        "{checked}"
    );
    let (_, fast) = execute(&lir(uncaught), &RunConfig::default());
    assert_eq!(fast.expect_err("the fast run fails").to_string(), checked);
    let caught = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  let r = try_panic (fn () ->
    Async.run (fn () ->
      let t = Async.start (fn () -> panic \"boom\")
      match try_panic (fn () -> Async.await t) with
        | Ok n -> println (show (n + 0))
        | Err m -> println (\"Err \" ++ m)))
  match r with
    | Ok () -> ()
    | Err _ -> ()
";
    assert_eq!(run_both(caught), "Err boom\n");
}

/// 待たないタスクの `MutArray` の結果は、ハンドルを落とした知らせでセルを放すときに落ちる。
#[test]
fn an_unawaited_lin_result_does_not_leak() {
    let src = "\
import Async
import MutArray
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    drop (Async.start (fn () -> MutArray.make 3 \"a\"))
    println \"started\")
";
    assert_eq!(run_both(src), "started\n");
}

/// `Lin` の結果は thunk に包まず、値のまま預かる行を通って `await` に届く。
#[test]
fn await_returns_a_lin_result() {
    let src = "\
import Async
import MutArray
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> MutArray.empty ())
    let m : MutArray String = Async.await t
    let (n, m) = MutArray.length m
    drop m
    println (show n))
";
    assert_eq!(run_both(src), "0\n");
}

/// スコープの最初の失敗より後の失敗だけを `dropped_failures` に数える。失敗する子が2つなら、1つが残らない。
#[test]
fn the_second_failure_of_a_scope_is_counted_as_dropped() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  let r = try_panic (fn () ->
    Async.run (fn () ->
      Async.spawn (fn () -> panic \"one\")
      Async.spawn (fn () -> panic \"two\")))
  match r with
    | Ok () -> println \"ok\"
    | Err _ -> println \"failed\"
";
    let (out, stats) = run_stats(src);
    let stats = stats.expect("the checked run");
    assert_eq!(out, "failed\n");
    assert_eq!(stats.dropped_failures, 1);
    assert_eq!(stats.tasks, 3);
}

/// 本体が2つの子を始めて両方を待つと、タスクは3つ、継続を渡す待ちは2つである。`shared_objects` は、始めるときの
/// `share` が数える。4つの欄は seed によらない。
#[test]
fn the_task_counts_do_not_depend_on_the_schedule() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let x = \"a\" ++ show 1
    let y = \"b\" ++ show 2
    let a = Async.start (fn () -> x)
    let b = Async.start (fn () -> y)
    println (Async.await a)
    println (Async.await b))
";
    let program = lir(src);
    let checked = || RunConfig::default().with_debug_heap(true);
    let (out, sequential) = execute(&program, &checked());
    let sequential = sequential.expect("the sequential run");
    assert_eq!(out, "a1\nb2\n");
    assert_eq!(sequential.tasks, 3);
    assert_eq!(sequential.parks, 2);
    assert_eq!(sequential.dropped_failures, 0);
    assert!(sequential.shared_objects > 0);
    for seed in [1, 2026] {
        let (seeded_out, seeded) = execute(&program, &checked().with_sched(Sched::Seeded(seed)));
        let seeded = seeded.unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert_eq!(seeded_out, out, "seed {seed}");
        let (a, b) = (seeded, sequential);
        assert_eq!(
            (a.tasks, a.parks, a.dropped_failures, a.shared_objects),
            (b.tasks, b.parks, b.dropped_failures, b.shared_objects),
            "seed {seed}"
        );
    }
}

/// 意味の核はネイティブのコードからも呼べるように、`std` と `eml_rt` だけを import する。
#[test]
fn the_task_cells_import_only_std_and_the_runtime() {
    let source = include_str!("../src/cells.rs");
    for line in source.lines() {
        let line = line.trim_start();
        if line.starts_with("use ") {
            assert!(
                line.starts_with("use std::") || line.starts_with("use eml_rt::"),
                "{line}"
            );
        }
        assert!(
            !line.contains("crate::") && !line.contains("super::"),
            "{line}"
        );
    }
}

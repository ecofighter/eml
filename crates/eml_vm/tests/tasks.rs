//! `Std.Async` のタスクの始めと終わりと `Async.run` (docs/spec/vm.md の「`Async` のタスク」)。どのプログラムも検査モードの
//! 逐次と速いモードで流し、標準出力を比べる。

use eml_test_support::{execute, lir, run, schedule_independent};
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

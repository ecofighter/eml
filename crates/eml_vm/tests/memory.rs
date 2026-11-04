//! メモリの柔らかい上限がどのタスクに数えられ、どのタスクに旗が立つか (docs/spec/vm.md の「タスクとスケジュール」
//! 「タスクの上限」)。どれも検査モードの逐次の方針で、明示した上限で流す。

use std::sync::Arc;

use eml_vm::{Control, Finished, Limits, RunConfig, RuntimeError, Sched, Stop, VmFault};

const MIB: usize = 1 << 20;

/// 検査モードで、方針 `sched`、worker を `workers` 個まで、硬い上限 `max`、柔らかい上限 `soft` の run を1回行う。
fn run(
    src: &str,
    workers: usize,
    sched: Sched,
    max: usize,
    soft: usize,
) -> (String, Result<Finished, RuntimeError>) {
    let control = Arc::new(Control::new(Limits {
        workers,
        max_bytes: max,
        soft_bytes: soft,
        ..Limits::default()
    }));
    let (host, out) = eml_vm::capture_host(Default::default());
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_sched(sched)
        .with_control(control);
    let result = eml_vm::execute(&eml_test_support::lir(src), &config, host);
    (out.contents(), result)
}

/// list を作る補助と、終わらずに伸ばし続ける `grow`。
const LISTS: &str = "\
build : Int -> List Int -> <Panic> List Int
build n acc = if n == 0 then acc else build (n - 1) (n :: acc)

grow : Int -> List Int -> <Panic> List Int
grow n acc = grow (n + 1) (n :: acc)

count : List Int -> Int -> <Panic> Int
count xs acc = match xs with
  | [] -> acc
  | _ :: rest -> count rest (acc + 1)
";

const CAUGHT: &str = "caught: ResourceLimit: the memory soft limit is exceeded\ndone\n";

fn program(body: &str) -> String {
    format!("import Async (Async)\nimport Std.Par\n{LISTS}\n{body}")
}

fn assert_memory_limit(result: Result<Finished, RuntimeError>, workers: usize) {
    let error = result.expect_err("the run goes on past the hard limit");
    assert!(
        matches!(
            error,
            RuntimeError::Fault {
                fault: VmFault::Stopped(Stop::MemoryLimit),
                ..
            }
        ),
        "{workers} workers: {error:?}"
    );
}

/// `par` の左の子は親のタスクの `Task` のまま動くので、その確保は親のタスクに数える。子の中には `IO` の evidence が
/// ないので、旗は合流まで保留のままで、合流の後の最初の呼び出しで起きる。
#[test]
fn a_left_child_allocation_counts_to_its_task() {
    let src = program(
        "\
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    match try_io (fn () ->
        let (xs, b) = Par.par (fn () -> build 200000 []) (fn () -> 0)
        count xs b) with
      | Ok n -> println \"unreachable \\{show n}\"
      | Err e -> println \"caught: \\{show e.kind}: \\{e}\")
  Async.await t
  println \"done\"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
",
    );
    for workers in [1, 2] {
        let (out, result) = run(&src, workers, Sched::Sequential, 64 * MIB, 2 * MIB);
        assert_eq!(out, CAUGHT, "{workers} workers");
        result.unwrap_or_else(|error| panic!("{workers} workers: {error}"));
    }
}

/// `par` の右の子はセルを持たないので、どのタスクにも数えない。旗の立つタスクがなく、run は硬い上限で止まる。
#[test]
fn a_right_child_allocation_counts_to_no_task() {
    let src = program(
        "\
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    match try_io (fn () ->
        let (a, n) = Par.par (fn () -> 0) (fn () -> count (grow 0 []) 0)
        a + n) with
      | Ok n -> println \"unreachable \\{show n}\"
      | Err e -> println \"caught: \\{show e.kind}: \\{e}\")
  Async.await t

main : Unit -> <IO, Panic> Unit
main () = Async.run body
",
    );
    for workers in [1, 2] {
        let (out, result) = run(&src, workers, Sched::Sequential, 16 * MIB, 2 * MIB);
        assert_eq!(out, "", "{workers} workers");
        assert_memory_limit(result, workers);
    }
}

/// `compute` の仕事もセルを持たないので、どのタスクにも数えない。
#[test]
fn a_compute_job_counts_to_no_task() {
    let src = program(
        "\
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    match try_io (fn () -> Async.compute (fn () -> count (grow 0 []) 0)) with
      | Ok n -> println \"unreachable \\{show n}\"
      | Err e -> println \"caught: \\{show e.kind}: \\{e}\")
  Async.await t

main : Unit -> <IO, Panic> Unit
main () = Async.run body
",
    );
    for workers in [1, 2] {
        let (out, result) = run(&src, workers, Sched::Sequential, 16 * MIB, 2 * MIB);
        assert_eq!(out, "", "{workers} workers");
        assert_memory_limit(result, workers);
    }
}

/// 入れ子の `Async.run` で始めたタスクも自分のセルで数え、旗が立つ。旗が立つのは確保した内のタスクで、待っている
/// 外のタスクではない。内のタスクが自分の失敗を捕まえ、外のタスクの `try_io` は値を受け取る。
#[test]
fn a_task_in_a_nested_run_is_flagged() {
    let src = program(
        "\
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    match try_io (fn () -> Async.run (fn () ->
        let u = Async.start (fn () ->
          match try_io (fn () -> count (grow 0 []) 0) with
            | Ok n -> n
            | Err e ->
              println \"inner caught: \\{show e.kind}: \\{e}\"
              0)
        Async.await u)) with
      | Ok n -> println \"outer ok: \\{show n}\"
      | Err e -> println \"outer caught: \\{show e.kind}: \\{e}\")
  Async.await t
  println \"done\"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
",
    );
    for workers in [1, 2] {
        let (out, result) = run(&src, workers, Sched::Sequential, 64 * MIB, 2 * MIB);
        assert_eq!(
            out,
            "inner caught: ResourceLimit: the memory soft limit is exceeded\nouter ok: 0\ndone\n",
            "{workers} workers"
        );
        result.unwrap_or_else(|error| panic!("{workers} workers: {error}"));
    }
}

/// 譲るたびに救済でほかの worker へ移るタスクも、数はセルについて回り、動いている worker の公開から旗が立つ。捕まえる
/// のは確保したタスク自身の `try_io` である。
#[test]
fn a_task_moved_by_rescue_is_flagged() {
    let src = program(
        "\
churn : List Int -> <IO, Panic, Async> Int
churn acc =
  Async.yield_now ()
  churn (build 1000 acc)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    match try_io (fn () -> churn []) with
      | Ok n -> println \"unreachable \\{show n}\"
      | Err e -> println \"churn caught: \\{show e.kind}: \\{e}\")
  Async.await t
  println \"done\"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
",
    );
    let (out, result) = run(&src, 2, Sched::RescueAlways, 64 * MIB, 2 * MIB);
    assert_eq!(
        out,
        "churn caught: ResourceLimit: the memory soft limit is exceeded\ndone\n"
    );
    let finished = result.unwrap_or_else(|error| panic!("{error}"));
    assert!(finished.rescued > 0, "{finished:?}");
}

/// join で右の子を待つ `par` の親は、待ちに入る切り替えの集める地点で候補にならない。`main` がタスクの前に柔らかい
/// 上限を越えるので閾値は越えたままで、決定的な方針はその集める地点で候補を探す。待つ親のほかに数のあるタスクは
/// ないので、旗はどこにも立たず、タスクは値を返す。
#[test]
fn a_parent_waiting_on_its_join_is_not_chosen() {
    let src = program(
        "\
body : List Int -> <IO, Panic, Async> Unit
body held =
  let t = Async.start (fn () ->
    match try_io (fn () ->
        let mine = build 5000 []
        let (a, b) = Par.par (fn () -> 1) (fn () -> 2)
        count mine (a + b)) with
      | Ok n -> println \"ok \\{show n}\"
      | Err e -> println \"caught: \\{show e.kind}: \\{e}\")
  Async.await t
  println \"held \\{show (count held 0)}\"

main : Unit -> <IO, Panic> Unit
main () =
  let held = build 100000 []
  Async.run (fn () -> body held)
",
    );
    let (out, result) = run(&src, 1, Sched::Sequential, 64 * MIB, 2 * MIB);
    assert_eq!(out, "ok 5003\nheld 100000\n");
    result.unwrap_or_else(|error| panic!("{error}"));
}

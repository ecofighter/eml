//! メモリの柔らかい上限がどのタスクに数えられ、どのタスクに旗が立つか (docs/spec/vm.md の「タスクとスケジュール」
//! 「タスクの上限」)。どれも検査モードの逐次の方針で、明示した上限で流す。

use std::sync::Arc;

use eml_vm::{Control, Finished, Limits, RunConfig, RunFault, RuntimeError, Sched, Stop};

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
    let result = eml_vm::execute(
        &eml_test_support::lir(src),
        &eml_vm::Linked::default(),
        &config,
        host,
    );
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
                fault: RunFault::Stopped(Stop::MemoryLimit),
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

/// 親が join で待っている間に `par` の右の子が柔らかい上限を越えても、どのタスクにも旗は立たない。右の子は親の
/// worker で動き、セルを持たないので候補にならない。待つ親は動いておらず、実行待ちでもないので、欄の `current` にも
/// `top` にも出ない。決定的な方針は候補のないまま閾値を進め、親は join の後に確保せずに値を返す。待ちに入る切り替えの
/// 集める地点はこの越える前にあるので、そこで待つ親を候補にしないことは `vm.rs` の単体テストが確かめる。
#[test]
fn a_crossing_in_a_right_child_while_its_parent_waits_flags_no_task() {
    let src = program(
        "\
body : Unit -> <IO, Panic, Async> Int
body () =
  let t = Async.start (fn () ->
    let mine = build 5000 []
    let (a, b) = Par.par (fn () -> 1) (fn () -> count (build 100000 []) 0)
    count mine (a + b))
  Async.await t

main : Unit -> <IO, Panic> Unit
main () =
  let n = Async.run body
  println \"ok \\{show n}\"
",
    );
    let (out, result) = run(&src, 1, Sched::Sequential, 64 * MIB, 2 * MIB);
    assert_eq!(out, "ok 105001\n");
    result.unwrap_or_else(|error| panic!("{error}"));
}

/// 候補のないまま柔らかい上限を越えた後に、切り替えずに増え始めたタスクにも旗が立つ。`main` がタスクの前に越えた
/// ときは候補がなく、決定的な方針は閾値を同じ幅だけ進める。タスクは次の閾値を越えたページで集める地点に来て、そこで
/// 選ばれる (docs/spec/vm.md の「上限と割り込み」)。
#[test]
fn a_task_that_grows_without_switching_after_a_crossing_without_a_candidate_is_flagged() {
    let src = program(
        "\
body : List Int -> <IO, Panic, Async> Unit
body held =
  let t = Async.start (fn () ->
    match try_io (fn () -> count (grow 0 []) 0) with
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
    let (out, result) = run(&src, 1, Sched::Sequential, 16 * MIB, 2 * MIB);
    assert_eq!(
        out,
        "caught: ResourceLimit: the memory soft limit is exceeded\nheld 100000\n"
    );
    result.unwrap_or_else(|error| panic!("{error}"));
}

/// 待っているタスクのセルに立った旗は、再開の後にそのタスクの `try_io` が捕まえる。数の大きい `a` が `yield_now` で
/// 列に戻っている間に数の小さい `b` が閾値を越えるので、旗は待っている `a` のセルに立つ。再開の途中にある EP の補助の
/// 関数のエントリで起こすと、`a` の `try_io` でなく底の `try_all` に届いてしまう (docs/spec/vm.md の「タスクの上限」)。
#[test]
fn a_flag_delivered_to_a_parked_task_is_caught_by_its_own_try_io() {
    let src = program(
        "\
hold : Int -> List Int -> <IO, Panic, Async> Int
hold n acc =
  if n == 0 then count acc 0
  else
    Async.yield_now ()
    hold (n - 1) (1 :: acc)

churn : Int -> List Int -> <IO, Panic, Async> Int
churn n acc =
  if n == 0 then count acc 0
  else
    Async.yield_now ()
    churn (n - 1) (build 200 acc)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () ->
    match try_io (fn () -> hold 400 (build 40000 [])) with
      | Ok n -> println \"a ok \\{show n}\"
      | Err e -> println \"a caught: \\{show e.kind}: \\{e}\")
  let b = Async.start (fn () ->
    match try_io (fn () -> churn 400 []) with
      | Ok n -> println \"b ok \\{show n}\"
      | Err e -> println \"b caught: \\{show e.kind}: \\{e}\")
  Async.await a
  Async.await b
  println \"done\"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
",
    );
    let (out, result) = run(&src, 1, Sched::Sequential, 64 * MIB, 2 * MIB);
    assert_eq!(
        out,
        "a caught: ResourceLimit: the memory soft limit is exceeded\nb ok 80000\ndone\n"
    );
    result.unwrap_or_else(|error| panic!("{error}"));
}

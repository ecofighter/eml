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

/// 落としたハンドルのタスクが継続で待っていれば、受け箱の処理が待ちを外して `Cancelled` で再開する。実行待ちの列に
/// いる `yield_now` のタスクは、再開するときに `Cancelled` を受ける。逐次の方針の数を固定する。
#[test]
fn a_dropped_handle_cancels_a_parked_task() {
    let src = "\
import Async (Async)
import Async
forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> Async.await (Async.start forever))
    Async.yield_now ()
    drop t
    println \"dropped\")
";
    assert_eq!(run_both(src), "dropped\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!(
        (s.tasks, s.parks, s.dropped_failures, s.cancels),
        (3, 3, 0, 2)
    );
}

/// `check_cancelled` は 128 回目の呼び出しで予算を使い切り、1回だけ `yield_now` でほかのタスクに worker を渡す。
#[test]
fn the_budget_yields_once_every_128_checks() {
    let src = |n: u32| {
        format!(
            "\
import Async (Async)
import Async
checks : Int -> <IO, Panic, Async> Unit
checks n =
  if n == 0 then ()
  else
    Async.check_cancelled ()
    checks (n - 1)
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> checks {n})
"
        )
    };
    for (n, parks) in [(127, 0), (128, 1), (256, 2)] {
        let (_, stats) = run_stats(&src(n));
        let s = stats.expect("the checked run");
        assert_eq!((s.parks, s.cancels), (parks, 0), "{n} checks");
    }
}

/// 仮想の時計では、すべてのタスクが待つときだけ時間が進むので、眠りは待たずに終わる (docs/spec/vm.md の「時間の待ち」)。
#[test]
fn a_task_sleeps_and_prints() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    Async.sleep_ms 100
    println \"a\")
";
    assert_eq!(run_both(src), "a\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.parks, s.timers), (1, 1));
}

/// 同じ忙しい期間に登録した眠りは、どのスケジュールでも長さの順に満ちる。
#[test]
fn sleeps_registered_together_end_in_the_order_of_their_lengths() {
    let src = "\
import Async (Async)
import Async
nap : Int -> <IO, Async> Unit
nap ms =
  Async.sleep_ms ms
  println (show ms)
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let a = Async.start (fn () -> nap 50)
    let b = Async.start (fn () -> nap 100)
    let c = Async.start (fn () -> nap 70)
    Async.await a
    Async.await b
    Async.await c)
";
    assert_eq!(run_both(src), "50\n70\n100\n");
    let program = lir(src);
    let checked = || RunConfig::default().with_debug_heap(true);
    for sched in [Sched::Sequential, Sched::Seeded(1), Sched::Seeded(2026)] {
        let (out, result) = execute(&program, &checked().with_sched(sched));
        result.unwrap_or_else(|error| panic!("{sched:?}: {error}"));
        assert_eq!(out, "50\n70\n100\n", "{sched:?}");
    }
    let counts = |sched| {
        let (_, result) = execute(&program, &checked().with_sched(sched));
        let s = result.expect("the checked run");
        (s.tasks, s.parks, s.timers)
    };
    let sequential = counts(Sched::Sequential);
    assert_eq!(sequential.2, 3);
    for sched in [Sched::Seeded(1), Sched::Seeded(2026)] {
        assert_eq!(counts(sched), sequential, "{sched:?}");
    }
}

/// 0 以下の眠りは、輪に載らずに列の後ろへ回るだけである。
#[test]
fn a_sleep_of_zero_or_less_yields() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    Async.sleep_ms 0
    println \"zero\"
    Async.sleep_ms (-1)
    println \"negative\")
";
    assert_eq!(run_both(src), "zero\nnegative\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.parks, s.timers, s.cancels), (2, 0, 0));
}

/// 眠るタスクのハンドルを落とすと、受け箱の処理が輪の項ごと待ちを外し、タスクは眠り切らずに `Cancelled` で終わる。
#[test]
fn a_dropped_handle_cancels_a_sleeping_task() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () ->
      Async.sleep_ms 10000
      println \"woke\")
    Async.yield_now ()
    drop t
    println \"dropped\")
";
    assert_eq!(run_both(src), "dropped\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.tasks, s.dropped_failures, s.cancels), (2, 0, 1));
    assert_eq!((s.timers, s.cancels), (1, 1));
}

/// `shield` の中の眠りは取り消しで外れず、眠り切った後の最初の取り消しの地点でタスクが `Cancelled` で終わる。
#[test]
fn a_shielded_sleep_is_not_cancelled() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    drop (Async.start (fn () ->
      Async.shield (fn () ->
        Async.sleep_ms 10
        println \"slept\")
      Async.check_cancelled ()
      println \"not cancelled\"))
    println \"dropped\")
";
    assert_eq!(run_both(src), "dropped\nslept\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.tasks, s.dropped_failures, s.cancels), (2, 0, 1));
}

/// 期限は飽和するので、どんな長さの眠りも誤りにならない。仮想の時計が端に着いた後の眠りは、次の空きで満ちる。
#[test]
fn the_longest_sleeps_saturate() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    Async.sleep_ms 9223372036854775807
    Async.sleep_ms 9223372036854775807
    Async.sleep_ms 1
    Async.sleep_ms 1
    println \"end\")
";
    assert_eq!(run_both(src), "end\n");
}

/// 仮想の時計は 60 秒の眠りを待たない。
#[test]
fn a_long_sleep_on_the_virtual_clock_does_not_wait() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    Async.sleep_ms 60000
    println \"woke\")
";
    assert_eq!(run_both(src), "woke\n");
}

/// `timeout` は本体を呼んだタスクで動かし、期限のタスクを1つだけ始める。数え方は docs/spec/stdlib.md の「時間」のとおり。
#[test]
fn timeout_runs_the_body_in_the_calling_task() {
    let programs = [
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Async.run (fn () -> Async.timeout 100 (fn () -> 42))))\n",
            "Some 42\n",
            (2, 3, 1, 1),
        ),
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Async.run (fn () -> Async.timeout 50 (fn () ->\n  Async.sleep_ms 100\n  1))))\n",
            "None\n",
            (2, 4, 2, 1),
        ),
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Async.run (fn () -> Async.timeout 100 (fn () ->\n  Async.sleep_ms 50\n  1))))\n",
            "Some 1\n",
            (2, 4, 2, 1),
        ),
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Async.run (fn () -> Async.timeout 0 (fn () ->\n  Async.sleep_ms 50\n  1))))\n",
            "None\n",
            (1, 3, 0, 1),
        ),
    ];
    for (src, out, counts) in programs {
        let program = lir(src);
        let config = RunConfig::default()
            .with_debug_heap(true)
            .with_sched(Sched::Sequential);
        let (got, result) = execute(&program, &config);
        let s = result.expect("the checked run");
        assert_eq!(got, out, "{src}");
        assert_eq!((s.tasks, s.parks, s.timers, s.cancels), counts, "{src}");
    }
}

/// `race` は腕のタスクを2つ始め、勝ちを `claim_scope` の1行で決める。数え方は docs/spec/stdlib.md の「時間」のとおり。
#[test]
fn race_decides_the_winner_in_one_claim() {
    let programs = [
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Async.run (fn () -> Async.race (fn () ->\n  Async.sleep_ms 30\n  1) (fn () ->\n  Async.sleep_ms 60\n  2))))\n",
            "Left 1\n",
            (3, 5, 2, 1, 0),
        ),
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Async.run (fn () -> Async.try_all (fn () -> Async.race (fn () ->\n  Async.sleep_ms 30\n  let x : Int = panic \"a\"\n  x) (fn () ->\n  match try_io (fn () -> Async.sleep_ms 60) with\n    | Ok () -> 2\n    | Err _ -> panic \"b\")))))\n",
            "Err (Panicked \"a\")\n",
            (3, 5, 2, 1, 1),
        ),
    ];
    for (src, out, counts) in programs {
        let program = lir(src);
        let config = RunConfig::default()
            .with_debug_heap(true)
            .with_sched(Sched::Sequential);
        let (got, result) = execute(&program, &config);
        let s = result.expect("the checked run");
        assert_eq!(got, out, "{src}");
        assert_eq!(
            (s.tasks, s.parks, s.timers, s.cancels, s.dropped_failures),
            counts,
            "{src}"
        );
    }
}

/// `drain_within` は `s` に属するタスクがなくなるのを待ち、その待ちを `timeout` で区切る。数え方は
/// docs/spec/stdlib.md の「時間」のとおり。
#[test]
fn drain_within_waits_until_the_scope_has_no_task() {
    let programs = [
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = Async.run (fn () -> Async.scope (fn s ->\n  Async.spawn_in s (fn () -> Async.sleep_ms 30)\n  Async.drain_within 100 s\n  let t = Async.start_in s (fn () -> 5)\n  println (show (Async.await t))))\n",
            "5\n",
            (4, 7, 2, 1),
        ),
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = Async.run (fn () -> Async.scope (fn s ->\n  Async.spawn_in s (fn () -> Async.sleep_ms 300)\n  Async.drain_within 100 s\n  println \"after\"))\n",
            "after\n",
            (3, 7, 2, 2),
        ),
        (
            "import Async (Async)\nimport Async\nmain : Unit -> <IO, Panic> Unit\nmain () = Async.run (fn () ->\n  let s = Async.scope (fn s -> s)\n  Async.drain_within 100 s\n  Async.drain_within 0 s\n  println \"at once\")\n",
            "at once\n",
            (2, 9, 1, 2),
        ),
    ];
    for (src, out, counts) in programs {
        let program = lir(src);
        let config = RunConfig::default()
            .with_debug_heap(true)
            .with_sched(Sched::Sequential);
        let (got, result) = execute(&program, &config);
        let s = result.expect("the checked run");
        assert_eq!(got, out, "{src}");
        assert_eq!((s.tasks, s.parks, s.timers, s.cancels), counts, "{src}");
    }
}

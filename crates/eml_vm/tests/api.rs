//! 公開の API の性質。後の並列 VM のための型の性質 (docs/future/multicore.md の「VM の並列化の備え」の制約1と2) と、
//! 実行の設定 `RunConfig` と、スケジュールの方針 `Sched` と、実行時エラーの `interrupted` と、`exit` が書く終わりの欄である。

use std::sync::Arc;

use eml_rt::{Checked, Fast, Fault};
use eml_vm::{
    Clock, CodeBase, Control, Limits, Machine, Normal, RunConfig, RuntimeError, Sched, Stop, Task,
    VmFault,
};

use crate::common::parse;

fn sync<T: Sync>() {}

fn send<T: Send>() {}

fn copy<T: Copy>() {}

#[test]
fn a_code_base_is_shared_and_a_task_moves() {
    sync::<CodeBase<Fast>>();
    sync::<CodeBase<Checked>>();
    send::<Task>();
}

/// 方針の値はスレッドをまたいで読む (docs/spec/vm.md の「タスクとスケジュール」)。
#[test]
fn a_policy_moves_and_a_sched_is_shared() {
    send::<Normal>();
    send::<Sched>();
    sync::<Sched>();
    copy::<Sched>();
}

#[test]
fn the_default_config_is_fast_with_a_fresh_unbounded_control() {
    let config = RunConfig::default();
    assert!(!config.debug_heap);
    assert_eq!(config.control.limits(), Limits::default());
    assert_eq!(config.control.stop_bits(), 0);
    assert_eq!(config.control.bytes(), 0);
    assert!(!Arc::ptr_eq(&config.control, &RunConfig::default().control));
}

#[test]
fn with_debug_heap_sets_the_flag() {
    assert!(RunConfig::default().with_debug_heap(true).debug_heap);
    assert!(
        !RunConfig::default()
            .with_debug_heap(true)
            .with_debug_heap(false)
            .debug_heap
    );
}

#[test]
fn the_default_sched_is_sequential() {
    assert_eq!(RunConfig::default().sched, Sched::Sequential);
    assert_eq!(Sched::default(), Sched::Sequential);
    let config = RunConfig::default().with_sched(Sched::Seeded(3));
    assert_eq!(config.sched, Sched::Seeded(3));
    assert_eq!(
        config.with_sched(Sched::Sequential).sched,
        Sched::Sequential
    );
}

/// ライブラリの既定の時計は、すべてのタスクが待つときにだけ進む仮想の時計である (docs/spec/vm.md の「時計と driver」)。
#[test]
fn the_default_clock_is_virtual() {
    assert_eq!(RunConfig::default().clock, Clock::Virtual);
    assert_eq!(Clock::default(), Clock::Virtual);
    let config = RunConfig::default().with_clock(Clock::Real);
    assert_eq!(config.clock, Clock::Real);
    assert_eq!(config.with_clock(Clock::Virtual).clock, Clock::Virtual);
}

/// CLI の `--sched` が読む綴り (docs/spec/stdlib.md の「起動」)。seed は `u64` の10進数である。
#[test]
fn a_sched_parses_and_rejects() {
    assert_eq!("normal".parse(), Ok(Sched::Normal));
    assert_eq!("sequential".parse(), Ok(Sched::Sequential));
    assert_eq!("seeded:0".parse(), Ok(Sched::Seeded(0)));
    assert_eq!("seeded:2026".parse(), Ok(Sched::Seeded(2026)));
    assert_eq!(
        "seeded:18446744073709551615".parse(),
        Ok(Sched::Seeded(u64::MAX))
    );
    for text in [
        "",
        "bogus",
        "Sequential",
        "Normal",
        " sequential",
        "seeded",
        "seeded:",
        "seeded:-1",
        "seeded:+7",
        "seeded:18446744073709551616",
        "seeded:7x",
        "seeded: 7",
        "rescue-always",
    ] {
        let error = text.parse::<Sched>().unwrap_err();
        assert_eq!(
            error,
            format!("expected `normal`, `sequential` or `seeded:<n>`, found `{text}`")
        );
    }
}

#[test]
fn with_control_replaces_the_control() {
    let limits = Limits {
        max_depth: 1000,
        ..Limits::default()
    };
    let control = Arc::new(Control::new(limits));
    let config = RunConfig::default().with_control(Arc::clone(&control));
    assert!(Arc::ptr_eq(&config.control, &control));
    assert_eq!(config.control.limits(), limits);
}

/// 止める理由のビットは `Control::clear` でしか下りないので、`clone` した設定で続けて流すと前の実行の割り込みで止まる。
/// 実行ごとに新しい設定を作るのはこのためである。
#[test]
fn a_cloned_config_shares_its_control() {
    let config = RunConfig::default().with_debug_heap(true);
    let copy = config.clone();
    assert!(Arc::ptr_eq(&config.control, &copy.control));
    assert!(copy.debug_heap);
    config.control.interrupt();
    assert_eq!(
        copy.control.stop_bits() & Control::INTERRUPT,
        Control::INTERRUPT
    );
}

const COUNT: &str = "\
layout Bool { False, True }
fn main() -> unit {
  tail call count(0)
}
fn count(n.0: int) -> unit {
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 100000)
  switch d.1 Bool { #0 -> b1, #1 -> b2 }
b1:
  let m.2: int = extern \"Prelude.Num Int.+\"(n.0, 1)
  tail call count(m.2)
b2:
  return ()
}
";

/// タスクが1つだけなら、seed の方針の予算が尽きても切り替える先がなく、横取りは起きない。仕事の回数も逐次と同じである。
#[test]
fn a_seeded_run_of_one_task_does_not_preempt() {
    let run = |sched| {
        let config = RunConfig::default().with_debug_heap(true).with_sched(sched);
        let (host, _) = eml_vm::capture_host(Default::default());
        eml_vm::execute(&parse(COUNT), &config, host).unwrap()
    };
    let sequential = run(Sched::Sequential);
    assert_eq!(sequential.preemptions, 0);
    for seed in [1, 2026] {
        assert_eq!(run(Sched::Seeded(seed)), sequential);
    }
}

/// 実行の前に割り込まれた `Control` では、最初の安全点で止まる。
#[test]
fn an_interrupted_control_stops_at_the_first_safepoint() {
    for debug_heap in [true, false] {
        let control = Arc::new(Control::new(Limits::default()));
        control.interrupt();
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_control(control);
        let (host, _) = eml_vm::capture_host(Default::default());
        let error = eml_vm::execute(&parse(COUNT), &config, host).unwrap_err();
        assert_eq!(error.to_string(), "interrupted in `main`");
        assert!(error.interrupted());
    }
}

/// 安全点を通らずに `FileIn.read_all` に着くプログラムは、読みの中で止まる。止めたのは extern なので `Extern` の誤りになる。
#[test]
fn an_interrupt_inside_read_all_is_an_interrupt() {
    let root = std::env::temp_dir().join(format!("eml-vm-read-interrupt-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "abc").unwrap();
    let text = "\
fn main() -> unit {
  let p.0: obj = const \"a.txt\"
  let h.1: obj lin = extern \"Std.FileIn.open\"(p.0)
  let s.2: obj = extern \"Std.FileIn.read_all\"(h.1)
  decref s.2
  let u.3: unit = extern \"Std.FileIn.close\"(h.1)
  return u.3
}
";
    let program = parse(text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    let control = Arc::new(Control::new(Limits::default()));
    control.interrupt();
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(control);
    let (host, _) = eml_vm::capture_host(root.clone());
    let result = eml_vm::execute(&program, &config, host);
    std::fs::remove_dir_all(&root).unwrap();
    let error = result.unwrap_err();
    assert!(
        matches!(
            error,
            RuntimeError::Fault {
                fault: VmFault::Extern(Fault::Stopped(Stop::Interrupted)),
                ..
            }
        ),
        "{error:?}"
    );
    assert!(error.interrupted());
}

fn fault(fault: VmFault) -> RuntimeError {
    RuntimeError::Fault {
        fault,
        function: "f".to_string(),
        at: None,
    }
}

/// 割り込みだけが真になる。上限で止めたことも、ほかの誤りも、リークも割り込みではない。
#[test]
fn only_an_interrupt_is_interrupted() {
    assert!(fault(VmFault::Stopped(Stop::Interrupted)).interrupted());
    assert!(fault(VmFault::Extern(Fault::Stopped(Stop::Interrupted))).interrupted());
    for stop in [Stop::DepthLimit, Stop::MemoryLimit] {
        assert!(!fault(VmFault::Stopped(stop)).interrupted());
        assert!(!fault(VmFault::Extern(Fault::Stopped(stop))).interrupted());
    }
    assert!(!fault(VmFault::Extern(eml_rt::ArithFault::DivisionByZero.into())).interrupted());
    assert!(!fault(VmFault::Internal("x")).interrupted());
    assert!(!RuntimeError::Leak(vec![("String".to_string(), 1)]).interrupted());
}

const EXIT: &str = "\
fn main() -> unit {
  let z.0: unit = extern Prelude.exit(3)
  return z.0
}
";

/// `main` の底は `exit` の終了コードを `Control` の終わりの欄に書き、`Finished.exit` はそこから来る
/// (docs/spec/vm.md の「`exit`」)。
#[test]
fn an_exit_is_written_to_the_end_field() {
    for debug_heap in [true, false] {
        let control = Arc::new(Control::new(Limits::default()));
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_control(Arc::clone(&control));
        let (host, _) = eml_vm::capture_host(Default::default());
        let finished = eml_vm::execute(&parse(EXIT), &config, host).unwrap();
        assert_eq!(finished.exit, Some(3));
        assert_eq!(control.exit_code(), Some(3));
    }
}

/// 終わりの欄は run ごとに開くので、`exit` で終わった `Machine` の次の run は `exit` を報告しない
/// (docs/spec/vm.md の「誤り、止めること、終わり」)。
#[test]
fn a_machine_runs_again_after_an_exit() {
    let quiet = "\
fn second() -> unit {
  return ()
}
";
    let (host, _) = eml_vm::capture_host(Default::default());
    let control = Arc::new(Control::new(Limits::default()));
    let mut machine = Machine::<Checked>::new(Arc::clone(&control), host);
    let first = machine.load(&parse(EXIT)).unwrap();
    let second = machine.load(&parse(quiet)).unwrap();
    assert_eq!(machine.run(first.entry).unwrap().exit, Some(3));
    assert_eq!(machine.run(second.entry).unwrap().exit, None);
    assert_eq!(control.exit_code(), None);
    machine.close().unwrap();
}

/// 本体が `FileIn` を持ち、別のタスクが入れ子の `run` で待つ間に、タスク `t` が `exit 3` を呼ぶ。タスクの底の `exit` は
/// ほかのタスクを動かさずに run を捨て、回復がヒープを入れ替えるので、`close` はリークを報告せず、同じ `Machine` の次の
/// run も動く (docs/spec/vm.md の「誤り、止めること、終わり」)。
#[test]
fn an_exit_from_a_task_recovers_and_reports_the_code() {
    let src = "\
import Async
import FileIn
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let f = FileIn.open \"a.txt\"
    let t = Async.start (fn () ->
      Async.yield_now ()
      println \"exiting\"
      (exit 3 : Unit))
    let u = Async.start (fn () ->
      Async.run (fn () ->
        Async.yield_now ()
        Async.yield_now ()
        println \"never\"))
    Async.await t
    Async.await u
    FileIn.close f)
";
    let quiet = "\
fn second() -> unit {
  return ()
}
";
    let root = std::env::temp_dir().join(format!("eml-vm-task-exit-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "abc").unwrap();
    let (host, out) = eml_vm::capture_host(root.clone());
    let control = Arc::new(Control::new(Limits::default()));
    let mut machine = Machine::<Checked>::new(Arc::clone(&control), host);
    let first = machine.load(&eml_test_support::lir(src)).unwrap();
    let second = machine.load(&parse(quiet)).unwrap();
    let finished = machine.run(first.entry);
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(finished.unwrap().exit, Some(3));
    assert_eq!(machine.run(second.entry).unwrap().exit, None);
    machine.close().unwrap();
    assert_eq!(out.contents(), "exiting\n");
}

/// すべてのタスクが本物の時計で眠る run は、空きのループが毎回割り込みを読むので、別のスレッドの割り込みで止まる
/// (docs/spec/vm.md の「時計と driver」)。2 秒は止まりの見張りで、本物の眠り (100 秒) よりずっと短い。
#[test]
fn an_interrupt_wakes_a_sleeping_worker() {
    use std::time::{Duration, Instant};

    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Async.sleep_ms 100000)
";
    let program = eml_test_support::lir(src);
    let control = Arc::new(Control::new(Limits::default()));
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_sched(Sched::Sequential)
        .with_control(Arc::clone(&control));
    let interrupter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
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

/// 本物の時計の検査モードの `Machine` で `src` を流し、閉じるまでの経過を測る。返すのは出力、run の結果、閉じたときの
/// 結果 (検査モードの漏れの検査)、経過である。`interrupt_on` があれば、出力にその文字列が出たときに別のスレッドが
/// 割り込む。どの run も、3 秒たって終わらなければ見張りのスレッドが割り込むので、止まったテストも待ち続けない。
fn run_real(
    src: &str,
    interrupt_on: Option<&'static str>,
) -> (
    String,
    Result<eml_vm::Finished, RuntimeError>,
    Result<(), RuntimeError>,
    std::time::Duration,
) {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let program = eml_test_support::lir(src);
    let control = Arc::new(Control::new(Limits::default()));
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(Arc::clone(&control), host).with_clock(Clock::Real);
    let loaded = machine.load(&program).unwrap();
    let (done, finished) = mpsc::channel::<()>();
    let seen = out.clone();
    let start = Instant::now();
    let watchdog = std::thread::spawn(move || {
        while finished.recv_timeout(Duration::from_millis(1))
            == Err(mpsc::RecvTimeoutError::Timeout)
        {
            let marked = interrupt_on.is_some_and(|marker| seen.contents().contains(marker));
            if marked || start.elapsed() >= Duration::from_secs(3) {
                control.interrupt();
                return;
            }
        }
    });
    let result = machine.run(loaded.entry);
    let closed = machine.close();
    let elapsed = start.elapsed();
    drop(done);
    watchdog.join().unwrap();
    (out.contents(), result, closed, elapsed)
}

/// 本物の driver の `pair` は unix の `socketpair` で、片方の端に書いたバイトを別のタスクが読む
/// (docs/spec/stdlib.md の「`Std.IoTest`」)。
#[cfg(unix)]
#[test]
fn a_real_pair_carries_bytes_between_tasks() {
    let src = "\
import Async
import Bytes
import IoTest
import String
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let t = Async.start (fn () ->
    let (buf, b) = IoTest.read_chunk (Bytes.of_string \"> \") b
    drop b
    String.decode buf)
  let (n, a) = IoTest.write (Bytes.of_string \"hello\") a
  drop a
  println \"\\{show n} \\{Async.await t}\")
";
    let (out, result, closed, _) = run_real(src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "5 > hello\n");
}

/// 仕事は本物の driver のプールのスレッドで動き、本物の時計では遅れの分だけ眠る。並んだ2つの仕事も `await` の順に
/// 受け取る (docs/spec/stdlib.md の「`Std.IoTest`」)。経過は遅れより短くならない。
#[test]
fn a_checksum_runs_on_the_real_pool() {
    let src = "\
import Async
import Bytes
import IoTest
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  println (show (IoTest.checksum 50 (Bytes.of_string \"ab\")))
  let t = Async.start (fn () -> IoTest.checksum 50 (Bytes.of_string \"abc\"))
  let u = Async.start (fn () -> IoTest.checksum 50 (Bytes.of_string \"xyz\"))
  println (show (Async.await u))
  println (show (Async.await t)))
";
    let (out, result, closed, elapsed) = run_real(src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "195\n363\n294\n");
    assert!(
        elapsed >= std::time::Duration::from_millis(100),
        "{elapsed:?}"
    );
}

/// 仕事は止められないので、期限で取り消されたタスクは仕事が終わるのを待ってから `Cancelled` で再開する。`timeout` は
/// `None` を返すが、戻るのは 300 ms の仕事が終わってからである (docs/spec/vm.md の「時計と driver」)。
#[test]
fn a_cancelled_job_resumes_after_it_completes() {
    let src = "\
import Async
import Bytes
import IoTest
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match Async.timeout 20 (fn () -> IoTest.checksum 300 (Bytes.of_string \"ab\")) with
    | Some n -> println \"Some \\{show n}\"
    | None -> println \"None\")
";
    let (out, result, closed, elapsed) = run_real(src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "None\n");
    assert!(
        elapsed >= std::time::Duration::from_millis(300),
        "{elapsed:?}"
    );
}

/// 仕事が走っている間に `exit` で終わる run は、ヒープを落とす前に仕事を待ちきる。終了コードはそのまま報告し、検査
/// モードの漏れの検査も通る (docs/spec/vm.md の「時計と driver」)。
#[test]
fn an_exit_waits_for_a_running_job() {
    let src = "\
import Async
import Bytes
import IoTest
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () -> IoTest.checksum 300 (Bytes.of_string \"ab\"))
  Async.yield_now ()
  println \"exiting\"
  (exit 3 : Unit)
  println (show (Async.await t)))
";
    let (out, result, closed, elapsed) = run_real(src, None);
    let finished = result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
    assert_eq!(out, "exiting\n");
    assert!(
        elapsed >= std::time::Duration::from_millis(200),
        "{elapsed:?}"
    );
}

/// 書き手のいない本物の端で読みを待つ run は、空きのループが割り込みを読むので、別のスレッドの割り込みで止まる。
/// 止めるときに読みの操作を取り消して待ちきるので、検査モードの漏れの検査も通る (docs/spec/vm.md の「時計と driver」)。
/// 走れるタスクの列は FIFO なので、本体が `yield_now` から戻って `waiting` を書くときには、読むタスクはもう本物の
/// 読みで待っている。割り込みは `waiting` が出てから送るので、読みの待ちの間に届く。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_run_blocked_in_a_real_read() {
    let src = "\
import Async
import Bytes
import IoTest
import Stdout
import String
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let t = Async.start (fn () ->
    let (buf, b) = IoTest.read_chunk (Bytes.empty ()) b
    drop b
    String.decode buf)
  Async.yield_now ()
  println \"waiting\"
  Stdout.close (Stdout.flush (Stdout.open ()))
  println (Async.await t)
  drop a)
";
    let (out, result, closed, _) = run_real(src, Some("waiting"));
    let error = result.unwrap_err();
    assert!(error.interrupted(), "{error:?}");
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "waiting\n");
}

/// Windows の本物の driver はまだソケットを扱わないので、`pair` は種類 `NotAvailable` の IO の失敗になる
/// (docs/spec/stdlib.md の「`Std.IoTest`」)。
#[cfg(windows)]
#[test]
fn a_real_pair_is_not_available() {
    let src = "\
import IoTest
main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> IoTest.pair ()) with
    | Ok (a, b) ->
      drop a
      drop b
      println \"pair\"
    | Err e -> println (show e.kind)
";
    let (out, result, closed, _) = run_real(src, None);
    result.unwrap_or_else(|error| panic!("{error}"));
    closed.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "NotAvailable\n");
}

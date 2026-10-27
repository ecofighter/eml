//! `PREEMPT` の立つ安全点の扱いを確かめる (docs/spec/vm.md の「タスクとスケジュール」)。どれも逐次の方針で流し、
//! `write_line` が書くときに Host がビットを立てる。

use std::ffi::OsString;
use std::io;
use std::io::Write;
use std::sync::Arc;

use eml_rt::{
    Control, Host, HostRef, Limits, Opened, Spawn, Spawned, StdinSlot, Stream, WriteMode,
};
use eml_vm::{CaptureHost, RunConfig, RuntimeError, Sched};

use crate::common::parse;

/// `write_line` の前に、書く行を渡して `raise` を呼ぶ Host。ほかは中の `CaptureHost` に任せる。
struct RaisingHost {
    inner: CaptureHost,
    raise: Box<dyn Fn(&str) + Send + Sync>,
}

impl Host for RaisingHost {
    fn write_line(&self, stream: Stream, line: &str) -> io::Result<()> {
        (self.raise)(line);
        self.inner.write_line(stream, line)
    }
    fn write(&self, stream: Stream, bytes: &[u8]) -> io::Result<()> {
        self.inner.write(stream, bytes)
    }
    fn flush(&self) -> io::Result<()> {
        self.inner.flush()
    }
    fn open(&self, path: &str) -> io::Result<Opened> {
        self.inner.open(path)
    }
    fn args(&self) -> &[OsString] {
        self.inner.args()
    }
    fn env_var(&self, name: &str) -> Option<OsString> {
        self.inner.env_var(name)
    }
    fn stdin(&self) -> Option<Arc<StdinSlot>> {
        self.inner.stdin()
    }
    fn open_write(&self, path: &str, mode: WriteMode) -> io::Result<Box<dyn Write + Send>> {
        self.inner.open_write(path, mode)
    }
    fn spawn(&self, spawn: &Spawn<'_>) -> io::Result<Spawned> {
        self.inner.spawn(spawn)
    }
}

/// `say` が1行を書き、戻るときの安全点が書いた後の止める語を読む。
const SAY: &str = "\
fn main() -> unit {
  let u.0: unit = call say(())
  return u.0
}
fn say(p.0: unit) -> unit {
  let s.1: obj = const \"hello\"
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";

fn run_raising(
    raise: impl Fn(&Control) + Send + Sync + 'static,
) -> (Arc<Control>, Result<eml_vm::Finished, RuntimeError>) {
    let program = parse(SAY);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    let control = Arc::new(Control::new(Limits::default()));
    let raiser = Arc::clone(&control);
    let host = RaisingHost {
        inner: CaptureHost::new(Default::default()),
        raise: Box::new(move |_| raise(&raiser)),
    };
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(Arc::clone(&control))
        .with_sched(Sched::Sequential);
    let result = eml_vm::execute(&program, &config, HostRef::new(Arc::new(host)));
    (control, result)
}

/// 走れるタスクがなければ、`PREEMPT` は切り替えずにビットを下ろすだけである。
#[test]
fn a_preempt_without_a_ready_task_only_lowers_the_bit() {
    let (control, result) = run_raising(Control::preempt);
    let finished = result.expect("the run ends normally");
    assert_eq!(finished.preemptions, 0);
    assert_eq!(control.worker_bits() & Control::PREEMPT, 0);
}

/// `PREEMPT` と一緒に立った止めるビットは、切り替えより先に勝つ。
#[test]
fn a_stop_raised_with_preempt_wins() {
    let (_, result) = run_raising(|control| {
        control.preempt();
        control.interrupt();
    });
    assert!(
        result.as_ref().is_err_and(RuntimeError::interrupted),
        "{result:?}"
    );
}

/// `Async.run` の本体がタスク `t` を始め、`"a"` を出力して回る。`"a"` を書くときに `PREEMPT` を立てると、次の安全点で
/// 実行待ちの `t` に切り替わり、`"b"` が `"c"` より先に出る。切り替えは方針が始めたものなので、1回と数える
/// (docs/spec/vm.md の「タスクとスケジュール」)。
#[test]
fn a_preempt_request_switches_at_the_next_safepoint() {
    let src = "\
import Async
spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> println \"b\")
    println \"a\"
    let _ = spin 10
    println \"c\"
    Async.await t)
";
    let program = eml_test_support::lir(src);
    let control = Arc::new(Control::new(Limits::default()));
    let raiser = Arc::clone(&control);
    let inner = CaptureHost::new(Default::default());
    let out = inner.output();
    let host = RaisingHost {
        inner,
        raise: Box::new(move |line| {
            if line == "a" {
                raiser.preempt();
            }
        }),
    };
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(control)
        .with_sched(Sched::Sequential);
    let finished = eml_vm::execute(&program, &config, HostRef::new(Arc::new(host)))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "a\nb\nc\n");
    assert_eq!(finished.preemptions, 1);
}

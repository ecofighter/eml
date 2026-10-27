//! `PREEMPT` の立つ安全点の扱いを、手書きの LIR で確かめる (docs/spec/vm.md の「タスクとスケジュール」)。どちらも
//! 逐次の方針で流し、`write_line` が書くときに Host がビットを立てる。

use std::ffi::OsString;
use std::io;
use std::io::Write;
use std::sync::Arc;

use eml_rt::{
    Control, Host, HostRef, Limits, Opened, Spawn, Spawned, StdinSlot, Stream, WriteMode,
};
use eml_vm::{CaptureHost, RunConfig, RuntimeError, Sched};

use crate::common::parse;

/// `write_line` の前に `raise` を呼ぶ Host。ほかは中の `CaptureHost` に任せる。
struct RaisingHost {
    inner: CaptureHost,
    raise: Box<dyn Fn() + Send + Sync>,
}

impl Host for RaisingHost {
    fn write_line(&self, stream: Stream, line: &str) -> io::Result<()> {
        (self.raise)();
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
        raise: Box::new(move || raise(&raiser)),
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

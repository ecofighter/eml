//! `Std.IoTest` の操作の記録と driver の完了 (docs/spec/vm.md の「操作の記録」)。完了の数はテストの driver
//! (`Clock::Virtual`) の上で、取り消しの道は書きを取り消すまで終わらせない driver の上で確かめる。テストの driver の
//! 書きは次の集める地点でいつも終わるので、待っている間の取り消しに届かないためである。

use std::io;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::{Checked, Completion, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey};
use eml_test_support::{run, run_stats};
use eml_vm::{Machine, RunConfig, capture_host};

/// 書きは driver の操作を1つ出し、完了を1つ受けて、待っていたタスクを再開する。待ちは継続を1つ渡す。
#[test]
fn a_write_counts_one_completion() {
    let src = "\
import Async
import Bytes
import IoTest
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let (n, a) = IoTest.write (Bytes.of_string \"hello\") a
  drop a
  drop b
  println (show n))
";
    let (out, result) = run(src);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "5\n");
    let (_, stats) = run_stats(src);
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((s.completions, s.parks, s.cancels), (1, 1, 0));
}

/// 書きを取り消されるまで終わらせない driver。取り消した書きは、次の `completions` で誤りとして終わる。
#[derive(Default)]
struct HoldingDriver {
    next_key: OpKey,
    held: Vec<OpKey>,
    done: Vec<Completion>,
}

struct NoWaker;

impl IoWaker for NoWaker {
    fn wake(&self) {}
}

impl IoDriver for HoldingDriver {
    fn now(&self) -> Duration {
        Duration::ZERO
    }

    fn wait(&mut self, _deadline_ms: Option<u64>) -> io::Result<bool> {
        Ok(!self.done.is_empty())
    }

    fn submit(&mut self, _op: Op) -> OpKey {
        self.next_key += 1;
        self.held.push(self.next_key);
        self.next_key
    }

    fn cancel(&mut self, key: OpKey) {
        if let Some(at) = self.held.iter().position(|&k| k == key) {
            self.held.remove(at);
            self.done.push(Completion {
                key,
                result: Err(io::Error::other("cancelled")),
            });
        }
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        out.append(&mut self.done);
    }

    fn pending(&self) -> usize {
        self.held.len() + self.done.len()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        Arc::new(NoWaker)
    }

    fn bin(&self) -> Arc<HandleBin> {
        Arc::default()
    }

    fn pair(&mut self) -> io::Result<(IoHandle, IoHandle)> {
        Ok((1, 2))
    }

    fn close(&mut self, _handle: IoHandle) {}

    fn quiesce(&mut self) {
        self.held.clear();
        self.done.clear();
    }
}

/// 書きを待つタスクを取り消すと、待ちは driver の取り消しの完了まで続き、`Cancelled` で再開して種類 `Cancelled` の
/// 失敗を投げる。取り出しの行は取り消した操作の誤りを投げず、buffer とストリームは包みが落とすので、検査モードの
/// 漏れの検査も通る (docs/spec/vm.md の「取り消しの届け方」)。
#[test]
fn a_cancelled_write_wait_raises_cancelled() {
    let src = "\
import Async (Async)
import Async
import Bytes
import IoTest
writer : IoTest.Stream -> <IO, Async> Unit
writer a =
  match try_io (fn () -> IoTest.write (Bytes.of_string \"x\") a) with
    | Ok (n, a) ->
      drop a
      println \"wrote \\{show n}\"
    | Err e -> println \"\\{show e.kind}: \\{e}\"
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let t = Async.start (fn () -> writer a)
  Async.yield_now ()
  Async.cancel t
  drop b)
";
    let (host, out) = capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host)
        .with_driver(Box::new(HoldingDriver::default()));
    let loaded = machine.load(&eml_test_support::lir(src)).unwrap();
    let finished = machine
        .run(loaded.entry)
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "Cancelled: task was cancelled\n");
    let s = finished.stats;
    assert_eq!((s.completions, s.cancels), (1, 1));
}

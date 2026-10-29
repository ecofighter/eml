//! `Std.IoTest` の操作の記録と driver の完了 (docs/spec/vm.md の「操作の記録」)。完了の数はテストの driver
//! (`Clock::Virtual`) の上で、取り消しの道は書きを取り消すまで終わらせない driver の上で確かめる。テストの driver の
//! 書きは次の集める地点でいつも終わるので、待っている間の取り消しに届かないためである。

use std::io;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::{Checked, Completion, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, RawBuf};
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

/// 読みは、端が読めるようになるのを待つ操作と、buffer を貸して読む操作の2つの完了を受ける。書きと合わせて3つで、
/// 待ちはどちらも1回である (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_write_and_a_read_count_three_completions() {
    let src = "\
import Async
import Bytes
import IoTest
import String
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let (_, a) = IoTest.write (Bytes.of_string \"hello\") a
  let (buf, b) = IoTest.read_chunk (Bytes.of_string \"> \") b
  drop a
  drop b
  println (String.decode buf))
";
    let (out, result) = run(src);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "> hello\n");
    let (_, stats) = run_stats(src);
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((s.completions, s.parks, s.cancels), (3, 2, 0));
}

/// 相手の端が閉じた読みは、読めるようになった後の読みが 0 バイトで終わり、buffer の中身を変えずに返す。足した空きは
/// 中身に出ない (docs/spec/stdlib.md の「`Std.IoTest`」)。
#[test]
fn a_read_at_the_peer_eof_returns_the_buffer_unchanged() {
    let src = "\
import Async
import Bytes
import IoTest
import String
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  drop a
  let (buf, b) = IoTest.read_chunk (Bytes.of_string \"keep\") b
  drop b
  println \"\\{String.decode buf} \\{show (Bytes.length buf)}\")
";
    let (out, result) = run(src);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "keep 4\n");
    let (_, stats) = run_stats(src);
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((s.completions, s.parks), (2, 1));
}

/// プールの仕事は driver の操作を1つ出し、完了を1つ受ける。仮想の時計では遅れを渡さないので、時間の待ちはない
/// (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_checksum_counts_one_completion() {
    let src = "\
import Async
import Bytes
import IoTest
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> println (show (IoTest.checksum 1000 (Bytes.of_string \"ab\"))))
";
    let (out, result) = run(src);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "195\n");
    let (_, stats) = run_stats(src);
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((s.completions, s.parks, s.timers, s.cancels), (1, 1, 0, 0));
}

/// 書き手のない端の期限付きの読みは、期限が端の読めるようになるのを待つ操作に当たり、その取り消しの完了1つで終わる。
/// buffer を貸す操作は出さない。期限は時間の輪に1つ載り、再開は取り消しでなく `None` である
/// (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_timed_out_read_counts_one_completion_and_one_timer() {
    let src = "\
import Async
import Bytes
import IoTest
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let (r, b) = IoTest.read_chunk_within 50 (Bytes.of_string \"keep\") b
  drop a
  drop b
  match r with
    | Some _ -> println \"Some\"
    | None -> println \"None\")
";
    let (out, result) = run(src);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "None\n");
    let (_, stats) = run_stats(src);
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((s.completions, s.timers, s.parks, s.cancels), (1, 1, 1, 0));
}

/// 書きと読みを取り消されるまで終わらせない driver。端が読めるようになるのを待つ操作はすぐ終わる。取り消した書きは、
/// 次の `completions` で誤りとして終わる。取り消した読みは、取り消しが間に合わなかったものとして、貸された記憶に
/// `xy` を書いて2バイトで終わる。
#[derive(Default)]
struct HoldingDriver {
    next_key: OpKey,
    held: Vec<(OpKey, Option<RawBuf>)>,
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

    fn submit(&mut self, op: Op) -> OpKey {
        self.next_key += 1;
        let key = self.next_key;
        match op {
            Op::Readable { .. } => self.done.push(Completion { key, result: Ok(0) }),
            Op::Read { buf, .. } => self.held.push((key, Some(buf))),
            _ => self.held.push((key, None)),
        }
        key
    }

    fn cancel(&mut self, key: OpKey) {
        if let Some(at) = self.held.iter().position(|&(k, _)| k == key) {
            let result = match self.held.remove(at).1 {
                Some(buf) => {
                    assert!(buf.cap >= 2);
                    // 記録が完了を受け取るまで、貸された記憶に書いてよい
                    unsafe { std::ptr::copy_nonoverlapping(b"xy".as_ptr(), buf.ptr, 2) };
                    Ok(2)
                }
                None => Err(io::Error::other("cancelled")),
            };
            self.done.push(Completion { key, result });
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

/// 貸した記憶へ読んでいる間に取り消された読みは、取り消しが間に合わずにバイトを読んでいても、タスクを `Cancelled` で
/// 再開して種類 `Cancelled` の失敗を投げる。取り出しの行は読んだバイトを確定した buffer を返し、包みが落とすので、
/// 検査モードの漏れの検査も通る (docs/spec/stdlib.md の「`Std.IoTest`」)。
#[test]
fn a_read_cancelled_during_its_transfer_raises_cancelled() {
    let src = "\
import Async (Async)
import Async
import Bytes
import IoTest
reader : IoTest.Stream -> <IO, Async> Unit
reader b =
  match try_io (fn () -> IoTest.read_chunk (Bytes.of_string \"ab\") b) with
    | Ok (buf, b) ->
      drop b
      println \"read \\{show (Bytes.length buf)}\"
    | Err e -> println \"\\{show e.kind}: \\{e}\"
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let t = Async.start (fn () -> reader b)
  Async.yield_now ()
  Async.yield_now ()
  Async.cancel t
  drop a)
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
    // 端が読めるようになった完了と、取り消した読みの完了
    assert_eq!((s.completions, s.cancels), (2, 1));
}

/// プールの仕事はプールのスレッドで動くので、仕事を作るモジュールは `std` と `eml_rt` だけを import し、`unsafe` を
/// 持たない。ヒープにも RC にも届かないことを、import と `unsafe` のなさで確かめる (docs/spec/vm.md の「操作の記録」)。
#[test]
fn the_jobs_module_imports_only_std_and_eml_rt() {
    let source = include_str!("../src/jobs.rs");
    for line in source.lines() {
        let line = line.trim_start();
        if line.starts_with("//") {
            continue;
        }
        if line.starts_with("use ") {
            assert!(
                line.starts_with("use std::") || line.starts_with("use eml_rt::"),
                "{line}"
            );
        }
        assert!(
            !line.contains("crate::") && !line.contains("super::") && !line.contains("unsafe"),
            "{line}"
        );
    }
}

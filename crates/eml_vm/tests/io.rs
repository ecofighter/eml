//! `Std.Net` の操作の記録と driver の完了 (docs/spec/vm.md の「操作の記録」)。完了の数はテストの driver
//! (`Clock::Virtual`) の上で、取り消しの道は書きを取り消すまで終わらせない driver の上で確かめる。テストの driver の
//! 書きは次の集める地点でいつも終わるので、待っている間の取り消しに届かないためである。

use std::io;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::{
    Checked, Completion, Done, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, RawBuf,
};
use eml_test_support::{run, run_stats};
use eml_vm::{Machine, RunConfig, capture_host};

/// `Net` のつながった2つのストリーム `a` (接続した側) と `b` (受けた側) を作る本体の頭。待ち受け、受け、接続の完了を
/// 1つずつ受け、待ちは3つの操作と `await` の4回である。
const CONNECTED: &str = "\
import Async
import Byte
import Bytes
import Net
import String
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  let a = Net.connect \"127.0.0.1\" port
  let b = Async.await t
";

/// `CONNECTED` の後ろに `rest` を足したプログラムの検査モードの `RunStats`。
fn connected_stats(rest: &str) -> (String, eml_vm::RunStats) {
    let src = format!("{CONNECTED}{rest}");
    let (out, stats) = run_stats(src.as_str());
    (out, stats.unwrap_or_else(|error| panic!("{error}")))
}

/// 書きは driver の操作を1つ出し、完了を1つ受けて、待っていたタスクを再開する。待ちは継続を1つ渡す。つなぐまでの
/// 完了3つと待ち4回を合わせて数える。
#[test]
fn a_write_counts_one_completion() {
    let (out, s) = connected_stats(
        "  let a = Net.write \"hello\" a
  Net.close a
  Net.close b
  println \"wrote\")
",
    );
    assert_eq!(out, "wrote\n");
    assert_eq!((s.completions, s.parks, s.cancels), (4, 5, 0));
}

/// 相手が閉じたストリームの読みは、読めるようになった後の読みが 0 バイトで終わり、buffer の中身を変えずに返す。足した
/// 空きは中身に出ない (docs/spec/stdlib.md の「`Std.Net`」)。
#[test]
fn a_read_at_the_peer_eof_returns_the_buffer_unchanged() {
    let (out, s) = connected_stats(
        "  Net.close a
  let (buf, b) = Net.read_chunk (Bytes.of_string \"keep\") b
  Net.close b
  println \"\\{String.decode buf} \\{show (Bytes.length buf)}\")
",
    );
    assert_eq!(out, "keep 4\n");
    assert_eq!((s.completions, s.parks), (5, 5));
}

/// `Net.listen` は driver の操作を1つ出し、完了を1つ受けて、待っていたタスクを再開する。リスナーは完了の中身から
/// 集める地点で作る (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_listen_counts_one_completion() {
    let src = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  Net.close_listener l
  println (show port))
";
    let (out, result) = run(src);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "49152\n");
    let (_, stats) = run_stats(src);
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((s.completions, s.parks, s.cancels), (1, 1, 0));
}

/// 待ち受け、受け、接続はどれも1つの操作で、完了を1つずつ受ける。継続を渡す待ちは、3つの操作と `await` の4回である
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn an_accept_and_a_connect_count_three_completions() {
    let src = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  let c = Net.connect \"127.0.0.1\" port
  let s = Async.await t
  let (a, s) = Net.peer_addr s
  let (b, c) = Net.peer_addr c
  Net.close s
  Net.close c
  println \"\\{a} \\{b}\")
";
    let (out, result) = run(src);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "127.0.0.1:49153 127.0.0.1:49152\n");
    let (_, stats) = run_stats(src);
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((s.completions, s.parks, s.cancels), (3, 4, 0));
}

/// `Net.write` は driver の操作を1つ出して完了を1つ受け、`Net.read_chunk` は、ストリームが読めるようになるのを待つ
/// 操作と、buffer を貸して読む操作の2つの完了を受ける。待ちはどちらも1回である (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_net_write_and_read_count_three_more_completions_than_setup() {
    let (_, setup) = connected_stats(
        "  Net.close a
  Net.close b)
",
    );
    let (out, s) = connected_stats(
        "  let a = Net.write \"hello\" a
  let (buf, b) = Net.read_chunk (Bytes.of_string \"> \") b
  Net.close a
  Net.close b
  println (String.decode buf))
",
    );
    assert_eq!(out, "> hello\n");
    assert_eq!(
        (
            s.completions - setup.completions,
            s.parks - setup.parks,
            s.cancels
        ),
        (3, 2, 0)
    );
}

/// 書き手のない `Net` のストリームの期限付きの読みは、期限が、読めるようになるのを待つ操作に当たり、その取り消しの
/// 完了1つで終わる。buffer を貸す操作は出さない。期限は時間の輪に1つ載り、再開は取り消しでなく `None` である
/// (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_timed_out_net_read_counts_one_completion_and_one_timer() {
    let (_, setup) = connected_stats(
        "  Net.close a
  Net.close b)
",
    );
    let (out, s) = connected_stats(
        "  let (r, b) = Net.read_chunk_within 50 (Bytes.of_string \"keep\") b
  Net.close a
  Net.close b
  match r with
    | Some _ -> println \"Some\"
    | None -> println \"None\")
",
    );
    assert_eq!(out, "None\n");
    assert_eq!(
        (
            s.completions - setup.completions,
            s.timers,
            s.parks - setup.parks,
            s.cancels
        ),
        (1, 1, 1, 0)
    );
}

/// ストリームの buffer にもう1行ある `Net.read_line` は、driver の操作を出さずに受け付けの時点で終わるので、完了を
/// 受けない。待ちは、すぐ再開する待ちとして1回数える (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_buffered_read_line_counts_no_completion() {
    let (_, one) = connected_stats(
        "  let a = Net.write \"x\\ny\\n\" a
  let (r, b) = Net.read_line b
  Net.close a
  Net.close b
  println (show r))
",
    );
    let (out, two) = connected_stats(
        "  let a = Net.write \"x\\ny\\n\" a
  let (r, b) = Net.read_line b
  let (q, b) = Net.read_line b
  Net.close a
  Net.close b
  println (show r)
  println (show q))
",
    );
    assert_eq!(out, "Some \"x\"\nSome \"y\"\n");
    assert_eq!(
        (
            two.completions - one.completions,
            two.parks - one.parks,
            two.cancels
        ),
        (0, 1, 0)
    );
}

/// UTF-8 でない行の `Net.read_line` は、種類 `NotUtf8` の IO の失敗 `cannot read the stream: the line is not valid UTF-8`
/// を投げる。投げるのは `take_line` で、包みのストリームは巻き戻しが落とすので、検査モードの漏れの検査も通る
/// (docs/spec/stdlib.md の「`Std.Net`」)。
#[test]
fn a_read_line_that_is_not_utf8_raises_not_utf8() {
    let (out, _) = connected_stats(
        "  let a = Net.write_bytes (Bytes.append (Bytes.make 1 (Byte.wrap 255)) (Bytes.of_string \"\\n\")) a
  Net.close a
  match try_io (fn () -> Net.read_line b) with
    | Ok (_, b) ->
      Net.close b
      println \"read\"
    | Err e -> println \"\\{show e.kind}: \\{e}\")
",
    );
    assert_eq!(
        out,
        "NotUtf8: cannot read the stream: the line is not valid UTF-8\n"
    );
}

/// 書きと読みを取り消されるまで終わらせない driver。ストリームが読めるようになるのを待つ操作と、待ち受け、受け、
/// 接続はすぐ終わる。取り消した書きは、次の `completions` で誤りとして終わる。取り消した読みは、取り消しが
/// 間に合わなかったものとして、貸された記憶に `xy` を書いて2バイトで終わる。
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
            Op::Readable { .. } => self.done.push(Completion {
                key,
                result: Ok(Done::Size(0)),
                cancelled: false,
            }),
            Op::Read { buf, .. } => self.held.push((key, Some(buf))),
            Op::Listen { .. } => self.done.push(Completion {
                key,
                result: Ok(Done::Listener {
                    handle: key,
                    port: 1,
                }),
                cancelled: false,
            }),
            Op::Accept { .. } | Op::Connect { .. } => self.done.push(Completion {
                key,
                result: Ok(Done::Stream {
                    handle: key,
                    peer: ([127, 0, 0, 1], 1).into(),
                }),
                cancelled: false,
            }),
            _ => self.held.push((key, None)),
        }
        key
    }

    fn cancel(&mut self, key: OpKey) {
        if let Some(at) = self.held.iter().position(|&(k, _)| k == key) {
            let (result, cancelled) = match self.held.remove(at).1 {
                Some(buf) => {
                    assert!(buf.cap >= 2);
                    // 記録が完了を受け取るまで、貸された記憶に書いてよい
                    unsafe { std::ptr::copy_nonoverlapping(b"xy".as_ptr(), buf.ptr, 2) };
                    (Ok(Done::Size(2)), false)
                }
                None => (Err(io::Error::other("cancelled")), true),
            };
            self.done.push(Completion {
                key,
                result,
                cancelled,
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

    fn shutdown_write(&mut self, _handle: IoHandle) -> io::Result<()> {
        Ok(())
    }

    fn set_nodelay(&mut self, _handle: IoHandle, _on: bool) -> io::Result<()> {
        Ok(())
    }

    fn close(&mut self, _handle: IoHandle) {}

    fn quiesce(&mut self) {
        self.held.clear();
        self.done.clear();
    }
}

/// `Net.write` を待つタスクを取り消すと、待ちは driver の取り消しの完了まで続き、`Cancelled` で再開して種類
/// `Cancelled` の失敗を投げる。`finish` は取り消した操作の誤りを投げず、記録の buffer を落とし、
/// ストリームは包みが落とすので、検査モードの漏れの検査も通る (docs/spec/stdlib.md の「`Std.Net`」)。
#[test]
fn a_cancelled_net_write_wait_raises_cancelled() {
    let src = "\
import Async (Async)
import Async
import Net
writer : Net.TcpStream -> <IO, Async> Unit
writer a =
  match try_io (fn () -> Net.write \"x\" a) with
    | Ok a ->
      Net.close a
      println \"wrote\"
    | Err e -> println \"\\{show e.kind}: \\{e}\"
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let u = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  let a = Net.connect \"127.0.0.1\" 1
  let b = Async.await u
  let t = Async.start (fn () -> writer a)
  Async.yield_now ()
  Async.cancel t
  Net.close b)
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
    // 待ち受け、受け、接続の完了と、取り消した書きの完了
    assert_eq!((s.completions, s.cancels), (4, 1));
}

/// 貸した記憶へ読んでいる間に取り消された読みは、取り消しが間に合わずにバイトを読んでいても、タスクを `Cancelled` で
/// 再開して種類 `Cancelled` の失敗を投げる。取り出しの行は読んだバイトを確定した buffer を返し、包みが落とすので、
/// 検査モードの漏れの検査も通る (docs/spec/stdlib.md の「`Std.Net`」)。
#[test]
fn a_read_cancelled_during_its_transfer_raises_cancelled() {
    let src = "\
import Async (Async)
import Async
import Bytes
import Net
reader : Net.TcpStream -> <IO, Async> Unit
reader b =
  match try_io (fn () -> Net.read_chunk (Bytes.of_string \"ab\") b) with
    | Ok (buf, b) ->
      Net.close b
      println \"read \\{show (Bytes.length buf)}\"
    | Err e -> println \"\\{show e.kind}: \\{e}\"
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let u = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  let a = Net.connect \"127.0.0.1\" 1
  let b = Async.await u
  let t = Async.start (fn () -> reader b)
  Async.yield_now ()
  Async.yield_now ()
  Async.cancel t
  Net.close a)
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
    // 待ち受け、受け、接続の完了と、ストリームが読めるようになった完了と、取り消した読みの完了
    assert_eq!((s.completions, s.cancels), (5, 1));
}

/// 受けの記録がリスナーを持ったまま、本体の `exit` で run が終わる。`main` の終わりの `sweep_all` が、残った
/// 記録を放してリスナーを落とすので、検査モードの漏れの検査が通る。接続する者はいないので、`took` は出ない
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn an_op_record_left_at_an_exit_does_not_leak() {
    let src = "\
import Async
import Net
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    println \"took\"
    Net.close_listener l
    s)
  Async.yield_now ()
  println \"waiting\"
  (exit 3 : Unit)
  Net.close (Async.await t))
";
    let (host, out) = capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host);
    let loaded = machine.load(&eml_test_support::lir(src)).unwrap();
    let finished = machine
        .run(loaded.entry)
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(finished.exit, Some(3));
    assert_eq!(out.contents(), "waiting\n");
    // 待ち受けの完了だけが届き、受けは終わらないまま残った
    assert_eq!(finished.stats.completions, 1);
}

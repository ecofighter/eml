//! `Std.Net` の操作の記録と driver の完了 (docs/spec/vm.md の「操作の記録」)。完了の数はテストの driver
//! (`Clock::Virtual`) の上で、取り消しの道は書きを取り消すまで終わらせない driver の上で確かめる。テストの driver の
//! 書きは次の集める地点でいつも終わるので、待っている間の取り消しに届かないためである。

use std::io;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::{
    Checked, Completion, Done, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, RawBuf, Submit,
};
use eml_test_support::{run, run_stats};
use eml_vm::{Machine, RunConfig, Sched, capture_host};

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

    fn submit(&mut self, key: OpKey, op: Op) -> Submit {
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
        Submit::Pending
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

/// テストの driver を包み、`wait` と `completions` の呼び出しの数と、受け付けた操作の種類、操作ごとに受け付けで
/// 終わったか、読みに貸された大きさを記録する driver。`quiesce` で、`submit` がその場で終えた操作の数を写す。
/// `Machine` が driver を持つ間も、テストが記録を読めるようにする。
struct Recorder {
    inner: eml_io::TestDriver,
    log: Arc<std::sync::Mutex<Log>>,
}

#[derive(Default, Debug)]
struct Log {
    waits: usize,
    completions: usize,
    ops: Vec<&'static str>,
    /// `ops` の操作ごとに、`submit` が `Submit::Done` を返したか。
    at_submit: Vec<bool>,
    /// `Op::Read` ごとの、貸された記憶のバイト数。
    lent: Vec<usize>,
    immediate: u64,
}

impl IoDriver for Recorder {
    fn now(&self) -> Duration {
        self.inner.now()
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.log.lock().unwrap().waits += 1;
        self.inner.wait(deadline_ms)
    }

    fn submit(&mut self, key: OpKey, op: Op) -> Submit {
        let mut log = self.log.lock().unwrap();
        let kind = match op {
            Op::Readable { .. } => "readable",
            Op::Read { buf, .. } => {
                log.lent.push(buf.cap);
                "read"
            }
            Op::Write { .. } => "write",
            Op::Listen { .. } => "listen",
            Op::Accept { .. } => "accept",
            Op::Connect { .. } => "connect",
            _ => "other",
        };
        log.ops.push(kind);
        let submit = self.inner.submit(key, op);
        log.at_submit.push(matches!(submit, Submit::Done(_)));
        submit
    }

    fn cancel(&mut self, key: OpKey) {
        self.inner.cancel(key);
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        self.log.lock().unwrap().completions += 1;
        self.inner.completions(out);
    }

    fn pending(&self) -> usize {
        self.inner.pending()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        self.inner.waker()
    }

    fn bin(&self) -> Arc<HandleBin> {
        self.inner.bin()
    }

    fn shutdown_write(&mut self, handle: IoHandle) -> io::Result<()> {
        self.inner.shutdown_write(handle)
    }

    fn set_nodelay(&mut self, handle: IoHandle, on: bool) -> io::Result<()> {
        self.inner.set_nodelay(handle, on)
    }

    fn close(&mut self, handle: IoHandle) {
        self.inner.close(handle);
    }

    fn quiesce(&mut self) {
        self.inner.quiesce();
        self.log.lock().unwrap().immediate = self.inner.immediate();
    }
}

/// 1つのタスクが自分で待ち受け、接続し、受け、書いて読む。テストの driver はどれも受け付けの時点で終えるので、
/// 待ちはどれも速い道で続け、driver の `wait` も `completions` も呼ばない。`parks` は `io_wait` の呼び出しの数で、
/// `completions` は終わった操作の数である。読みは buffer に空きがないので、読めるようになるのを待つ操作と読みの2つを
/// 出す (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn one_task_of_finished_ops_never_waits_on_the_driver() {
    let src = "\
import Async
import Bytes
import Net
import String
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let a = Net.connect \"127.0.0.1\" port
  let (b, l) = Net.accept l
  Net.close_listener l
  let a = Net.write_bytes (Bytes.of_string \"hi\") a
  let (buf, b) = Net.read_chunk (Bytes.empty ()) b
  Net.close a
  Net.close b
  println (String.decode buf))
";
    let log = Arc::new(std::sync::Mutex::new(Log::default()));
    let driver = Recorder {
        inner: eml_io::TestDriver::new(None),
        log: Arc::clone(&log),
    };
    let (host, out) = capture_host(Default::default());
    let mut machine =
        Machine::<Checked>::new(RunConfig::default().control, host).with_driver(Box::new(driver));
    let loaded = machine.load(&eml_test_support::lir(src)).unwrap();
    let finished = machine
        .run(loaded.entry)
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "hi\n");
    let log = log.lock().unwrap();
    assert_eq!(
        log.ops,
        ["listen", "connect", "accept", "write", "readable", "read"]
    );
    assert_eq!((log.immediate, log.waits, log.completions), (6, 0, 0));
    let s = finished.stats;
    assert_eq!((s.parks, s.completions, s.cancels), (5, 6, 0));
}

/// 2つのタスクが 20 往復の echo をする。操作が受け付けで終わるかは順で変わるが、`parks` は `io_wait` の呼び出しの数、
/// `completions` は終わった操作の数なので、どの seed でも同じである (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn echo_counts_do_not_depend_on_the_seed() {
    let src = "\
import Async (Async)
import Async
import Bytes
import Net
import String
serve : Int -> Net.TcpStream -> <IO, Panic, Async> Unit
serve n s =
  if n == 0 then Net.close s
  else
    let (buf, s) = Net.read_chunk (Bytes.empty ()) s
    let s = Net.write_bytes buf s
    serve (n - 1) s
ask : Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Int
ask n got c =
  if n == 0 then
    Net.close c
    got
  else
    let c = Net.write \"ping\" c
    let (buf, c) = Net.read_chunk (Bytes.empty ()) c
    ask (n - 1) (got + Bytes.length buf) c
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    serve 20 s)
  let c = Net.connect \"127.0.0.1\" port
  let got = ask 20 0 c
  Async.await t
  println (show got))
";
    let program = eml_test_support::lir(src);
    let counts: Vec<_> = [1, 2, 3]
        .into_iter()
        .map(|seed| {
            let config = RunConfig::default()
                .with_debug_heap(true)
                .with_sched(Sched::Seeded(seed));
            let (out, stats) = eml_test_support::execute(&program, &config);
            let s = stats.unwrap_or_else(|error| panic!("seed {seed}: {error}"));
            assert_eq!(out, "80\n", "seed {seed}");
            (s.parks, s.completions)
        })
        .collect();
    // 待ちは待ち受け、受け、接続、`await` と、往復ごとに両側の書きと読み。完了は読みが2つずつ
    assert_eq!(counts, [(84, 123); 3]);
}

/// 自分の2つの端 `a` と `b` で、書きと読みを `n` 回繰り返す関数 `spin_io` と、それを呼ぶ前の本体の頭。テストの driver
/// では、どの操作も受け付けの時点で終わる。回るたびに `counter` に1を足す。
const SPIN_IO: &str = "\
import Async (Async)
import Async
import Atomic
import Bytes
import Net
spin_io : Int -> Atomic.Counter -> Bytes -> Net.TcpStream -> Net.TcpStream -> <IO, Panic, Async> Unit
spin_io n counter buf a b =
  if n == 0 then
    Net.close a
    Net.close b
  else
    let a = Net.write \"x\" a
    let (buf, b) = Net.read_chunk (Bytes.truncate 0 buf) b
    let _ = Atomic.add 1 counter
    spin_io (n - 1) counter buf a b
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let counter = Atomic.new 0
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let a = Net.connect \"127.0.0.1\" port
  let (b, l) = Net.accept l
  Net.close_listener l
";

/// `Sequential` で、終わった操作だけを 300 回待つループも、操作の予算を使い切るとほかのタスクに worker を渡す。先に
/// 始めたタスクは、ループの終わりより前に動く (docs/spec/stdlib.md の「操作の予算」)。
#[test]
fn a_fast_path_loop_yields_to_another_task_under_sequential() {
    let src = format!(
        "{SPIN_IO}  let t = Async.start (fn () -> println \"other\")
  spin_io 300 counter (Bytes.empty ()) a b
  println \"loop done\"
  Async.await t)
"
    );
    let (out, stats) = run_stats(src.as_str());
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "other\nloop done\n");
    assert_eq!(s.cancels, 0);
}

/// 速い道だけで回るタスクも、取り消すと次の `io_wait` で止まる。タスクは予算の 128 回の `io_wait` (64 周) で譲り、
/// その間に取り消される。譲った後は待っていた読みの結果を受け取って周を終え、次の書きの前の取り消しの地点で投げる。
/// 取り消しの後に終える操作は高々1つである (docs/spec/vm.md の「取り消しの届け方」)。
#[test]
fn a_cancelled_fast_path_loop_stops_at_its_next_wait() {
    let src = format!(
        "{SPIN_IO}  let r = try_io (fn () ->
    Async.scope (fn s ->
      let t = Async.start_in s (fn () -> spin_io 1000 counter (Bytes.empty ()) a b)
      Async.yield_now ()
      Async.cancel_scope s
      Async.await t))
  match r with
    | Ok () -> println \"finished\"
    | Err e -> println \"\\{{show e.kind}}\"
  println (show (Atomic.load counter)))
"
    );
    let (out, stats) = run_stats(src.as_str());
    stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "Cancelled\n64\n");
}

/// 2つ目の受けは、始めたタスクの接続が先に走ったかで、受け付けで終わるか待つかが変わる。その後のループは、操作の
/// 予算を越えるまで書き、`check_cancelled`、読みを繰り返す。`io_wait` はどの道でも待ちを1つだけ数えるので、予算の切れる
/// 位置は `check_cancelled` が譲る `yield_now` の待ちの数に表れる。予算は操作が終わっていたかによらず待ちごとに1つ使い、
/// 戻す時点もプログラムで決まるので、その数も `parks` も逐次と seed の run で同じである。書きを1つ足して待ちの数の偶奇を
/// 変えた形も流す (docs/spec/stdlib.md の「操作の予算」)。
#[test]
fn the_budget_runs_out_at_the_same_point_in_every_schedule() {
    for extra in [0, 1] {
        for n in [61, 62, 200] {
            let writes = "  let a = Net.write \"y\" a\n".repeat(extra);
            let src = format!(
                "{SPIN_IO}  let l2 = Net.listen \"127.0.0.1\" 0
  let (port2, l2) = Net.local_port l2
  let t = Async.start (fn () -> Net.connect \"127.0.0.1\" port2)
  let _ = spin 40
  let (s, l2) = Net.accept l2
  Net.close_listener l2
{writes}  spin_checked {n} (Bytes.empty ()) a b
  Net.close s
  Net.close (Async.await t))
spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)
spin_checked : Int -> Bytes -> Net.TcpStream -> Net.TcpStream -> <IO, Panic, Async> Unit
spin_checked n buf a b =
  if n == 0 then
    Net.close a
    Net.close b
  else
    let a = Net.write \"x\" a
    Async.check_cancelled ()
    let (buf, b) = Net.read_chunk (Bytes.truncate 0 buf) b
    spin_checked (n - 1) buf a b
"
            );
            let program = eml_test_support::lir(src.as_str());
            let parks: Vec<_> = [
                Sched::Sequential,
                Sched::Seeded(1),
                Sched::Seeded(2),
                Sched::Seeded(7),
            ]
            .into_iter()
            .map(|sched| {
                let config = RunConfig::default().with_debug_heap(true).with_sched(sched);
                let (_, stats) = eml_test_support::execute(&program, &config);
                stats
                    .unwrap_or_else(|error| panic!("{sched:?}: {error}"))
                    .parks
            })
            .collect();
            assert_eq!(parks, [parks[0]; 4], "extra {extra}, n {n}");
        }
    }
}

/// 1つのタスクが自分で待ち受け、接続し、受け、`hi` を書いてから、`buf` を buffer にして読む。返すのは出力と、
/// `Recorder` の記録である。
fn read_into(buf: &str) -> (String, Log) {
    let src = format!(
        "\
import Async
import Bytes
import Net
import String
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let a = Net.connect \"127.0.0.1\" port
  let (b, l) = Net.accept l
  Net.close_listener l
  let a = Net.write \"hi\" a
  let (buf, b) = Net.read_chunk ({buf}) b
  Net.close a
  Net.close b
  println (String.decode buf))
"
    );
    let log = Arc::new(std::sync::Mutex::new(Log::default()));
    let driver = Recorder {
        inner: eml_io::TestDriver::new(None),
        log: Arc::clone(&log),
    };
    let (host, out) = capture_host(Default::default());
    let mut machine =
        Machine::<Checked>::new(RunConfig::default().control, host).with_driver(Box::new(driver));
    let loaded = machine.load(&eml_test_support::lir(src.as_str())).unwrap();
    machine
        .run(loaded.entry)
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    let log = std::mem::take(&mut *log.lock().unwrap());
    (out.contents(), log)
}

/// 一意で空きが 8 バイト以上ある buffer の読みは、読めるようになるのを待つ操作を出さず、空きを貸す読みだけを出す
/// (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_into_room_does_not_probe() {
    let (out, log) = read_into("Bytes.with_capacity 16");
    assert_eq!(out, "hi\n");
    assert_eq!(log.ops, ["listen", "connect", "accept", "write", "read"]);
    assert!(log.lent[0] >= 16, "{log:?}");
}

/// `Bytes.of_string "> "` の空きは、確保器が長さを語に切り上げた 8 バイトより少ない余りだけである。そのため読みは、
/// 読めるようになるのを待ってから、広げた buffer の 65 536 バイトを貸す (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_into_word_slack_probes_first() {
    let (out, log) = read_into("Bytes.of_string \"> \"");
    assert_eq!(out, "> hi\n");
    assert_eq!(log.ops[3..], ["write", "readable", "read"]);
    assert_eq!(log.lent, [65_536]);
}

/// 空の `Bytes` の読みは、読めるようになるのを待ってから読む (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_read_into_empty_bytes_probes_first() {
    let (out, log) = read_into("Bytes.empty ()");
    assert_eq!(out, "hi\n");
    assert_eq!(log.ops[3..], ["write", "readable", "read"]);
}

/// 相手が書いた後の読みでは、読めるようになるのを待つ操作が受け付けで終わり、その場で続けて出した読みも受け付けで
/// 終わる。driver を待たずにバイトが返る (docs/spec/vm.md の「操作の記録」)。
#[test]
fn a_probe_done_at_submit_reads_at_once() {
    let (out, log) = read_into("Bytes.empty ()");
    assert_eq!(out, "hi\n");
    assert_eq!(log.ops[4..], ["readable", "read"]);
    assert_eq!(log.at_submit[4..], [true, true]);
    assert_eq!((log.waits, log.completions), (0, 0));
}

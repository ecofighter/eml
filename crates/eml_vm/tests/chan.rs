//! `Std.Chan` の待ちと起こし、予算、run の終わりの後始末 (docs/spec/vm.md の「channel」)。どの順で起こすかと
//! `RunStats` の数はスケジュールで変わるので、逐次の方針 (`RunConfig::default()` と `run_stats`) でだけ固定する。
//! 順によらない振る舞いは UI テスト (`tests/ui/run/async/chan_*.em`) が確かめる。

use eml_test_support::{execute, lir, run, run_stats};
use eml_vm::RunConfig;

/// 検査モードと速いモードで流し、どちらも成功して同じ標準出力を返すことを確かめて、その出力を返す。
fn run_both(src: &str) -> String {
    let (checked, result) = run(src);
    result.unwrap_or_else(|error| panic!("the checked run failed: {error}"));
    let (fast, result) = execute(&lir(src), &RunConfig::default());
    result.unwrap_or_else(|error| panic!("the fast run failed: {error}"));
    assert_eq!(fast, checked, "the fast mode printed something else");
    checked
}

/// 容量 1 の channel では、送り手は1つ積むごとに枠を待ち、受け手は1つ取るごとに品物を待つ。待つたびに継続を
/// `park_send` か `park_recv` に渡すので、`channel_waits` は `parks` にも数える。閉じて取り切った channel はその場で
/// 放すので、run の終わりに放す channel はない。
#[test]
fn send_waits_for_room_and_recv_waits_for_items() {
    let src = "\
import Async (Async)
import Async
import Chan
produce : Int -> Chan.Chan Int -> <IO, Panic, Async> Unit
produce n c =
  if n > 3 then Chan.close c
  else
    let _ = Chan.send n c
    produce (n + 1) c
consume : Chan.Chan Int -> Int -> <IO, Panic, Async> Int
consume c acc =
  match Chan.recv c with
    | Some x -> consume c (acc + x)
    | None -> acc
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c = Chan.new 1
    Async.spawn (fn () -> produce 1 c)
    println (show (consume c 0)))
";
    assert_eq!(run_both(src), "6\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!(
        (
            s.tasks,
            s.parks,
            s.channel_waits,
            s.channel_ops,
            s.channels_swept
        ),
        (2, 5, 5, 6, 0)
    );
}

/// 待っている送り手は、待ち始めた順に起こす。容量 0 の channel で、受け手が待つたびに先頭の送り手を1つ起こす。
#[test]
fn waiting_senders_are_woken_in_order() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c = Chan.new 0
    Async.spawn (fn () -> println (show (Chan.send \"a\" c)))
    Async.spawn (fn () -> println (show (Chan.send \"b\" c)))
    Async.yield_now ()
    println (show (Chan.recv c))
    println (show (Chan.recv c))
    Chan.close c)
";
    assert_eq!(run_both(src), "Sent\nSome \"a\"\nSent\nSome \"b\"\n");
}

/// 待っている受け手も、待ち始めた順に起こす。積むたびに先頭の受け手を1つ起こす。
#[test]
fn waiting_receivers_are_woken_in_order() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c = Chan.new 0
    Async.spawn (fn () -> println (\"a \" ++ show (Chan.recv c)))
    Async.spawn (fn () -> println (\"b \" ++ show (Chan.recv c)))
    Async.yield_now ()
    let _ = Chan.send 1 c
    let _ = Chan.send 2 c
    Chan.close c)
";
    assert_eq!(run_both(src), "a Some 1\nb Some 2\n");
}

/// `close` は待っている受け手と送り手をすべて起こす。起こされた受け手は閉じて空の channel で `None` を、送り手は値を
/// `Rejected` で受け取る。3つの待ち手は `yield_now` の間に待ち始めるので、`channel_waits` は 3 である。
#[test]
fn close_wakes_every_waiter() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c : Chan.Chan Int = Chan.new 0
    let d = Chan.new 0
    let a = Async.start (fn () -> Chan.recv c)
    let b = Async.start (fn () -> Chan.recv c)
    let s = Async.start (fn () -> Chan.send \"kept\" d)
    Async.yield_now ()
    Chan.close c
    Chan.close d
    println (show (Async.await a))
    println (show (Async.await b))
    println (show (Async.await s)))
";
    assert_eq!(run_both(src), "None\nNone\nRejected \"kept\"\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.channel_waits, s.channel_ops, s.cancels), (3, 0, 0));
}

/// 待っている送り手を取り消すと、送り手は送り手の列から外れ、送るはずだった `MutArray` は巻き戻しが落とす。後から
/// 来た受け手は、取り消された送り手を起こさずに待つ (docs/spec/stdlib.md の「取り消し」)。
#[test]
fn a_cancelled_waiting_sender_leaves_the_queue_and_frees_its_value() {
    let src = "\
import Async
import Chan
import MutArray
text : Option (MutArray Int) -> String
text r =
  match r with
    | Some a ->
      drop a
      \"Some\"
    | None -> \"None\"
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c = Chan.new 0
    let s = Async.start (fn () ->
      match Chan.send (MutArray.make 2 1) c with
        | Chan.Sent -> ()
        | Chan.Rejected a -> drop a)
    Async.yield_now ()
    Async.cancel s
    let r = Async.start (fn () -> Chan.recv c)
    Async.yield_now ()
    Chan.close c
    println (text (Async.await r)))
";
    assert_eq!(run_both(src), "None\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.channel_waits, s.channel_ops, s.cancels), (2, 0, 1));
}

/// 起こされた受け手より先に、速い道の受け手が品物を取ることがある。起こされた受け手はやり直しで空の channel を見て
/// 待ち直し、次の品物で起こされる。待ちは2回になる (docs/spec/stdlib.md の「`Std.Chan`」)。
#[test]
fn a_woken_receiver_waits_again_when_the_item_was_taken() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c = Chan.new 1
    let a = Async.start (fn () -> Chan.recv c)
    Async.yield_now ()
    let _ = Chan.try_send 1 c
    println (show (Chan.try_recv c))
    Async.yield_now ()
    let _ = Chan.try_send 2 c
    println (show (Async.await a))
    Chan.close c)
";
    assert_eq!(run_both(src), "Got 1\nSome 2\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.channel_waits, s.channel_ops), (2, 4));
}

/// 受け手が待つと容量 0 の channel に枠が1つ開き、待っている送り手を起こす。起こされた送り手が動く前にその受け手が
/// 取り消されると、送り手は速い道をやり直して枠のないことを知り、待ち直す。品物は積まれず、次に来た受け手が送り手を
/// 起こし直す (docs/spec/stdlib.md の「取り消し」)。
#[test]
fn a_sender_woken_for_a_cancelled_receiver_waits_again() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c = Chan.new 0
    let s = Async.start (fn () -> Chan.send 1 c)
    Async.yield_now ()
    let r = Async.start (fn () -> Chan.recv c)
    Async.yield_now ()
    Async.cancel r
    Async.yield_now ()
    println (show (Chan.try_recv c))
    println (show (Chan.recv c))
    println (show (Async.await s))
    Chan.close c)
";
    assert_eq!(run_both(src), "Empty\nSome 1\nSent\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.channel_waits, s.channel_ops, s.cancels), (4, 2, 1));
}

/// 品物を積んで起こした受け手は、動く前に取り消されても、まず受け取りをやり直して品物を受け取る。取り消しは次の
/// 取り消しの地点が投げる (docs/spec/stdlib.md の「取り消し」)。
#[test]
fn a_woken_receiver_receives_before_its_cancellation_is_raised() {
    let src = "\
import Async
import Chan
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c = Chan.new 0
    let r = Async.start (fn () ->
      println (show (Chan.recv c))
      Async.yield_now ()
      println \"unreachable\")
    Async.yield_now ()
    let _ = Chan.send 1 c
    Async.cancel r
    Async.yield_now ()
    Async.yield_now ()
    println (show (Chan.try_recv c))
    Chan.close c)
";
    assert_eq!(run_both(src), "Some 1\nEmpty\n");
}

/// 枠がなくて断られた値は channel に入らないので、印を付けない。断られる `try_send` を足しても `shared_objects` は
/// 変わらない (docs/spec/object-model.md の「`share`」)。
#[test]
fn a_refused_value_is_not_marked() {
    let src = |extra: &str| {
        format!(
            "\
import Chan
main : Unit -> <IO> Unit
main () =
  let c = Chan.new 1
  let _ = Chan.try_send (\"a\" ++ show 1) c
  {extra}
  println (show (Chan.try_recv c))
"
        )
    };
    let shared = |src: &str| {
        let (_, stats) = run_stats(src);
        stats.expect("the checked run").shared_objects
    };
    let without = src("let b = \"b\" ++ show 2");
    let with = src("let _ = Chan.try_send (\"b\" ++ show 2) c");
    assert_eq!(run_both(&with), "Got \"a1\"\n");
    assert_eq!((shared(&without), shared(&with)), (1, 1));
}

/// 速い道は予算を1つずつ使う。`try_send` は使い切っても譲らず、次に予算を見る `recv` が1回譲る。`send` は使い切った
/// 所で譲り、再開した後のやり直しでもう1つ使う (docs/spec/stdlib.md の「操作の予算」)。
#[test]
fn the_fast_paths_spend_the_budget() {
    let src = |op: &str, n: u32| {
        format!(
            "\
import Async (Async)
import Async
import Chan
fill : Int -> Chan.Chan Int -> <IO, Panic, Async> Unit
fill n c =
  if n == 0 then ()
  else
    let _ = {op} n c
    fill (n - 1) c
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let c = Chan.new 1000
    fill {n} c
    println (show (Chan.recv c)))
"
        )
    };
    for (op, n, parks) in [
        ("Chan.try_send", 100, 0),
        ("Chan.try_send", 300, 1),
        ("Chan.send", 126, 0),
        ("Chan.send", 127, 1),
        ("Chan.send", 254, 2),
    ] {
        let (_, stats) = run_stats(&src(op, n));
        let s = stats.expect("the checked run");
        assert_eq!((s.parks, s.channel_waits), (parks, 0), "{n} of {op}");
    }
}

/// run の終わりには、閉じていない channel と、閉じても品物の残った channel を放し、残った品物をそのとき落とす。
/// 閉じて取り切った channel はその場で放したので数えない。`Lin` の品物も漏れない。
#[test]
fn the_channels_left_at_the_end_are_swept() {
    let src = "\
import Chan
import MutArray
keep : Chan.TrySendResult (MutArray Int) -> <IO> Unit
keep r =
  match r with
    | Chan.Accepted -> ()
    | Chan.Full a -> drop a
    | Chan.Refused a -> drop a
main : Unit -> <IO, Panic> Unit
main () =
  let open = Chan.new 1
  keep (Chan.try_send (MutArray.make 2 0) open)
  let closed = Chan.new 1
  keep (Chan.try_send (MutArray.make 2 0) closed)
  Chan.close closed
  let drained : Chan.Chan Int = Chan.new 1
  Chan.close drained
  println \"done\"
";
    assert_eq!(run_both(src), "done\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!((s.channel_ops, s.channels_swept), (2, 2));
}

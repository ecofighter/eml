//! `Std.Gui`をstubのbackendで動かす。GUIの`Park`はメインのworkerの1回の待ちの中でネイティブのループを回す。runの外の
//! 待ちの印は`want_gui_raw`が立て、手放した`Ui`の回収か、開く前の取り消しか、作るときの誤りが下ろす
//! (docs/spec/stdlib.mdの「`Std.Gui`」、docs/spec/vm.mdの「GUI の Park」)。

use std::sync::Arc;
use std::thread::ThreadId;

use eml_vm::eml_gui::{Backend, Script, stub};
use eml_vm::{
    Clock, Control, DefaultPark, Finished, Limits, ParkFactory, RunConfig, RuntimeError, Signal,
    VirtualSignal,
};

use crate::common::RecordingHost;

fn limits(workers: usize) -> Arc<Control> {
    Arc::new(Control::new(Limits {
        workers,
        ..Limits::default()
    }))
}

/// 筋書き`script`のstubのGUIの`Park`。OSの終了の操作は`control`の割り込みになる。
fn gui_park(script: &str, control: &Arc<Control>) -> ParkFactory {
    let script = Script::parse(script).unwrap();
    let control = Arc::clone(control);
    eml_vm::gui_park(Backend::Stub(script), Arc::new(move || control.interrupt()))
}

/// 書いた行と書いたスレッド、stubの記録、runの結果。
type Ran = (
    Vec<(String, ThreadId)>,
    Vec<String>,
    Result<Finished, RuntimeError>,
);

/// 検査モードで`config`のrunを`execute`で1回行い、書いた行とstubの記録を返す。
fn run_with(src: &str, config: RunConfig) -> Ran {
    stub::take_transcript();
    let (host, _) = RecordingHost::new(None);
    let result = eml_vm::execute(
        &eml_test_support::lir(src),
        &eml_vm::Linked::default(),
        &config.with_debug_heap(true),
        host.host_ref(),
    );
    (host.lines(), stub::take_transcript(), result)
}

/// 仮想の時計で、workerを`workers`個持ち、筋書き`script`のstubで動くrun。
fn run(
    src: &str,
    script: &str,
    workers: usize,
) -> (Vec<String>, Vec<String>, Result<Finished, RuntimeError>) {
    let control = limits(workers);
    let config = RunConfig::default()
        .with_control(Arc::clone(&control))
        .with_main_park(gui_park(script, &control));
    let (lines, transcript, result) = run_with(src, config);
    (texts(&lines), transcript, result)
}

fn texts(lines: &[(String, ThreadId)]) -> Vec<String> {
    lines.iter().map(|(line, _)| line.clone()).collect()
}

fn finished(result: Result<Finished, RuntimeError>) -> Finished {
    result.unwrap_or_else(|error| panic!("{error}"))
}

fn holds(finished: &Finished) -> (u64, u64) {
    (finished.holds_raised, finished.holds_lowered)
}

fn count(transcript: &[String], line: &str) -> usize {
    transcript.iter().filter(|l| *l == line).count()
}

const COUNTER: &str = "\
import Async (Async)
import Async
import Gui (Gui(..), Track(..), Align(..), Cell(..))
import Gui

data Msg =
  | Inc
  | Dec
  | Quit

counter : Unit -> <Gui Msg, IO, Panic, Async> Unit
counter () =
  use _ <- Gui.window \"Counter\" Quit
  use Gui.grid [Fr 1, Fr 1] []
  let shown = Gui.cell { Gui.default_cell | col_span = 2 } (fn () -> Gui.label \"0\")
  let _ = Gui.button \"-\" Dec
  let _ = Gui.button \"+\" Inc
  let loop n =
    match Gui.wait () with
      | Inc ->
        Gui.text shown (show (n + 1))
        loop (n + 1)
      | Dec ->
        Gui.text shown (show (n - 1))
        loop (n - 1)
      | Quit -> println \"final \\{show n}\"
  loop 0

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run counter
";

/// 筋書きのクリックで数を変え、閉じる要求で終わる。ループは1回だけで、印は1回立てて1回下ろす。
#[test]
fn counter() {
    let (lines, transcript, result) =
        run(COUNTER, "click 1:4\nclick 1:4\nclick 1:3\nclose 1:0\n", 1);
    let finished = finished(result);
    assert_eq!(lines, ["final 1"]);
    assert_eq!(holds(&finished), (1, 1));
    insta::assert_snapshot!(transcript.join("\n"), @r#"
    run_loop
    presence on
    create_window #1:0 "Counter"
    create Grid #1:1 in #1:0
    create Label #1:2 in #1:1 "0"
    create Button #1:3 in #1:1 "-"
    create Button #1:4 in #1:1 "+"
    measure #1:4
    measure #1:3
    measure #1:2
    measure #1:2
    measure #1:2
    measure #1:3
    measure #1:3
    measure #1:4
    measure #1:4
    measure #1:2
    measure #1:3
    measure #1:4
    measure #1:2
    measure #1:3
    measure #1:4
    measure #1:4
    measure #1:3
    measure #1:2
    place #1:1 (12, 12) 54x56
    place #1:2 (0, 0) 54x24
    place #1:3 (0, 32) 23x24
    place #1:4 (31, 32) 23x24
    show_window #1:0 78x80 min 78x80
    set #1:2 Text("1")
    measure #1:2
    measure #1:2
    measure #1:2
    measure #1:2
    measure #1:2
    set #1:2 Text("2")
    measure #1:2
    measure #1:2
    measure #1:2
    measure #1:2
    measure #1:2
    set #1:2 Text("1")
    measure #1:2
    measure #1:2
    measure #1:2
    measure #1:2
    measure #1:2
    destroy #1:0
    presence off
    stop
    "#);
}

/// GUIを求めないプログラムでは、GUIの`Park`は`DefaultPark`と同じである。backendを作らず、数も変わらない。
#[test]
fn a_program_without_a_gui_runs_as_under_the_default_park() {
    let src = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let p = Async.start_on_main (fn () ->
    Async.sleep_ms 3
    println \"pinned\")
  Async.sleep_ms 1
  println \"main\"
  Async.await p)
";
    let control = limits(2);
    let gui = RunConfig::default()
        .with_control(Arc::clone(&control))
        .with_main_park(gui_park("", &control));
    let (gui_lines, transcript, gui_result) = run_with(src, gui);
    let default = RunConfig::default()
        .with_control(limits(2))
        .with_main_park(Arc::new(|| Box::new(DefaultPark)));
    let (default_lines, _, default_result) = run_with(src, default);
    let (gui, default) = (finished(gui_result), finished(default_result));
    assert_eq!(texts(&gui_lines), ["main", "pinned"]);
    assert_eq!(texts(&gui_lines), texts(&default_lines));
    assert_eq!(gui.stats, default.stats);
    assert_eq!(holds(&gui), (0, 0));
    assert!(transcript.is_empty(), "{transcript:?}");
}

/// 閉じる要求でウィンドウの名前を書いて戻るアプリ。
const APP: &str = "\
import Async (Async)
import Async
import Chan
import Gui (Gui(..))
import Gui

data Msg =
  | Click
  | Posted
  | Quit

app : String -> Unit -> <Gui Msg, IO, Panic, Async> Unit
app title () =
  use _ <- Gui.window title Quit
  let loop () =
    match Gui.wait () with
      | Quit -> println \"closed \\{title}\"
      | _ -> loop ()
  loop ()
";

fn with_app(main: &str) -> String {
    format!("{APP}\n{main}")
}

/// 1つの固定したタスクが続けて2回`Gui.run`する。2回目の`want_gui_raw`は1回目の`Ui`の回収より先に同じ`run`の中で
/// 動くので、回収は印を下ろさず、2つのGUIは1つの期間になる (docs/spec/vm.mdの「GUI の Park」)。`main`から別々に
/// 呼ぶと回収が先になりうるので、この形で数を決める。
#[test]
fn two_sequential_runs_share_one_loop_and_one_holds_period() {
    let src = with_app(
        "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    Gui.run (app \"one\")
    Gui.run (app \"two\")))
",
    );
    let (lines, transcript, result) = run(&src, "close 1:0\nclose 1:0\n", 1);
    let finished = finished(result);
    assert_eq!(lines, ["closed one", "closed two"]);
    assert_eq!(holds(&finished), (1, 1));
    assert_eq!(count(&transcript, "run_loop"), 1);
    assert_eq!(count(&transcript, "create_window #1:0 \"one\""), 1);
    assert_eq!(count(&transcript, "create_window #1:0 \"two\""), 1);
    assert_eq!(count(&transcript, "destroy #1:0"), 2);
    assert_eq!(count(&transcript, "presence off"), 2);
}

/// 本体のpanicも`Ui`を手放すので、回収がウィンドウを閉じて印を下ろす。後のデッドロックはまた判定される。
#[test]
fn a_panicking_run_is_reaped_and_a_later_deadlock_is_reported() {
    let src = with_app(
        "\
boom : Unit -> <Gui Msg, IO, Panic, Async> Unit
boom () =
  use _ <- Gui.window \"boom\" Quit
  panic \"boom\"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match try_panic (fn () -> Async.run (fn () -> Gui.run boom)) with
    | Ok () -> println \"ok\"
    | Err m -> println m
  let c : Chan.Chan Int = Chan.new 1
  let _ = Chan.recv c
  println \"unreachable\")
",
    );
    let (lines, transcript, result) = run(&src, "click 9:9\n", 1);
    let Err(error) = result else {
        panic!("the run did not fail");
    };
    assert!(matches!(error, RuntimeError::Deadlock), "{error}");
    assert_eq!(lines, ["boom"]);
    assert_eq!(count(&transcript, "destroy #1:0"), 1);
    assert_eq!(count(&transcript, "presence off"), 1);
}

/// `Async.timeout`の取り消しは`Gui.wait`の待ちに届き、回収が閉じる。後のデッドロックはまた判定される。
#[test]
fn a_cancelled_run_is_reaped_and_a_later_deadlock_is_reported() {
    let src = with_app(
        "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match Async.timeout 5 (fn () -> Gui.run (app \"slow\")) with
    | Some () -> println \"done\"
    | None -> println \"timed out\"
  let c : Chan.Chan Int = Chan.new 1
  let _ = Chan.recv c
  println \"unreachable\")
",
    );
    let (lines, transcript, result) = run(&src, "click 9:9\n", 1);
    let Err(error) = result else {
        panic!("the run did not fail");
    };
    assert!(matches!(error, RuntimeError::Deadlock), "{error}");
    assert_eq!(lines, ["timed out"]);
    assert_eq!(count(&transcript, "destroy #1:0"), 1);
    assert_eq!(count(&transcript, "presence off"), 1);
}

/// 開く前に取り消された`Gui.run`は`abandon_raw`でゲートを放し、印を下ろす。backendは作らず、次の`Gui.run`は開ける。
/// 求めた期間と開いた期間で、印は2回立てて2回下ろす。
#[test]
fn a_run_cancelled_before_the_open_leaves_the_gui_usable() {
    let src = with_app(
        "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match Async.timeout 0 (fn () -> Gui.run (app \"never\")) with
    | Some () -> println \"opened\"
    | None -> println \"timed out\"
  Gui.run (app \"later\"))
",
    );
    let (lines, transcript, result) = run(&src, "", 1);
    let finished = finished(result);
    assert_eq!(lines, ["timed out", "closed later"]);
    assert_eq!(holds(&finished), (2, 2));
    assert_eq!(count(&transcript, "run_loop"), 1);
    assert_eq!(count(&transcript, "create_window #1:0 \"never\""), 0);
    assert_eq!(count(&transcript, "create_window #1:0 \"later\""), 1);
}

/// 送ったメッセージより、待つ間に届いたネイティブのイベントが先に返る。次の`wait`は、ゲートに着く前にchannelから
/// 送ったメッセージを取る。取り出しと眠りの間の知らせがゲートの旗に残ることは、登録表の単体テスト
/// (`cells::a_poke_before_a_park_sets_the_flag`)が確かめる。
#[test]
fn native_events_come_before_posted_messages() {
    let src = with_app(
        "\
flood : Unit -> <Gui Msg, IO, Panic, Async> Unit
flood () =
  use _ <- Gui.window \"flood\" Quit
  let _ = Gui.button \"b\" Click
  let sink = Gui.sink ()
  Gui.post sink Posted
  Async.sleep_ms 10
  let name m = match m with
    | Click -> \"click\"
    | Posted -> \"posted\"
    | Quit -> \"quit\"
  println (name (Gui.wait ()))
  println (name (Gui.wait ()))

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Gui.run flood)
",
    );
    let (lines, _, result) = run(&src, "click 1:1\n", 1);
    let finished = finished(result);
    assert_eq!(lines, ["click", "posted"]);
    assert_eq!(holds(&finished), (1, 1));
}

/// 仮想のシグナルの割り込みは、`Ui`を開いたまま待つrunも終わらせる。ループは`Stop`で戻る。
#[test]
fn a_virtual_interrupt_ends_a_run_with_an_open_ui() {
    let src = with_app(
        "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Gui.run (app \"wait\"))
",
    );
    let control = limits(1);
    let config = RunConfig::default()
        .with_control(Arc::clone(&control))
        .with_main_park(gui_park("click 9:9\n", &control))
        .with_signals(vec![VirtualSignal {
            at_ms: 5,
            signal: Signal::Interrupt,
        }]);
    let (lines, transcript, result) = run_with(&src, config);
    let Err(error) = result else {
        panic!("the run did not fail");
    };
    assert!(error.interrupted(), "{error}");
    assert!(texts(&lines).is_empty());
    assert_eq!(count(&transcript, "run_loop"), 1);
    // 仮想の時計は、タスクがゲートで待つ間にも進むので、割り込みは`Ui`を開く前にも後にも届きうる。開いていれば、
    // 止めたタスクは`Ui`を手放さないまま終わるので回収は走らず、ウィンドウは`GuiPark`を落とすときに閉じる
    let after: Vec<&str> = transcript
        .iter()
        .skip_while(|line| *line != "stop")
        .map(String::as_str)
        .collect();
    if count(&transcript, "presence on") == 1 {
        assert_eq!(after, ["stop", "destroy #1:0", "presence off"]);
    } else {
        assert_eq!(transcript, ["run_loop", "stop"]);
    }
}

/// `Ui`が開いていれば、OSの終了の操作はウィンドウごとの閉じる要求になる。
#[test]
fn quit_with_an_open_ui_requests_every_window_to_close() {
    let src = with_app(
        "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Gui.run (app \"quit\"))
",
    );
    let (lines, _, result) = run(&src, "quit\n", 1);
    let finished = finished(result);
    assert_eq!(lines, ["closed quit"]);
    assert_eq!(holds(&finished), (1, 1));
}

/// `Gui.run`が戻った後のOSの終了の操作は、runの割り込みになる。
#[test]
fn quit_after_the_run_returned_interrupts_the_program() {
    let src = with_app(
        "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Gui.run (app \"first\")
  Async.run_on_main (fn () -> Async.sleep_ms 100000)
  println \"unreachable\")
",
    );
    let control = limits(2);
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_control(Arc::clone(&control))
        .with_main_park(gui_park("close 1:0\nquit\n", &control));
    let (lines, transcript, result) = run_with(&src, config);
    let Err(error) = result else {
        panic!("the run did not fail");
    };
    assert!(error.interrupted(), "{error}");
    assert_eq!(texts(&lines), ["closed first"]);
    assert_eq!(count(&transcript, "presence off"), 1);
}

/// 開きかけか開いたGUIがあるうちの2つ目の`Gui.run`は失敗する。
#[test]
fn a_second_concurrent_run_fails() {
    let src = with_app(
        "\
report : Result IOError Unit -> <IO> Unit
report r = match r with
  | Ok () -> println \"ok\"
  | Err e -> println (show e)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () ->
    let a = Async.start (fn () -> report (try_io (fn () -> Async.run (fn () -> Gui.run (app \"a\")))))
    report (try_io (fn () -> Async.run (fn () -> Gui.run (app \"b\"))))
    Async.await a))
",
    );
    let (lines, _, result) = run(&src, "", 1);
    let finished = finished(result);
    let mut lines = lines;
    lines.sort();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].starts_with("closed "), "{lines:?}");
    assert_eq!(
        lines[0],
        "IOError { kind = Other, message = \"cannot open the GUI: it is already open\" }"
    );
    assert_eq!(lines[2], "ok");
    assert_eq!(holds(&finished), (1, 1));
}

/// GUIの`Park`のないrunでは、`Gui.run`は`NotAvailable`で失敗する。
#[test]
fn without_the_gui_park_the_gui_is_not_available() {
    let src = with_app(
        "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match try_io (fn () -> Async.run (fn () -> Gui.run (app \"none\"))) with
    | Ok () -> println \"opened\"
    | Err e -> println (show e))
",
    );
    let (lines, transcript, result) = run_with(&src, RunConfig::default().with_control(limits(1)));
    let finished = finished(result);
    assert_eq!(
        texts(&lines),
        ["IOError { kind = NotAvailable, message = \"cannot open the GUI: not available\" }"]
    );
    assert_eq!(holds(&finished), (0, 0));
    assert!(transcript.is_empty());
}

/// backendを作れないrunでは、`Gui.run`は作るときの誤りで失敗する。`open_ui_raw`がゲートを放して印を下ろすので、
/// 誤りは次の`Gui.run`でも同じで、その後のプログラムは終わり、デッドロックもまた判定される。
#[cfg(not(all(feature = "gui-native", target_os = "macos")))]
#[test]
fn a_failed_backend_creation_fails_each_run_and_lowers_the_holds() {
    let attempt = "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let attempt () = match try_io (fn () -> Async.run (fn () -> Gui.run (app \"native\"))) with
    | Ok () -> println \"opened\"
    | Err e -> println (show e)
  attempt ()
  attempt ()";
    let error = "IOError { kind = NotAvailable, message = \"cannot open the GUI: built without the native toolkit\" }";
    let native = |src: &str| {
        let control = limits(1);
        let interrupt = Arc::clone(&control);
        let config = RunConfig::default()
            .with_control(control)
            .with_main_park(eml_vm::gui_park(
                Backend::Native,
                Arc::new(move || interrupt.interrupt()),
            ));
        run_with(&with_app(src), config)
    };

    let (lines, transcript, result) = native(&format!("{attempt}\n  println \"after\")\n"));
    let finished = finished(result);
    assert_eq!(texts(&lines), [error, error, "after"]);
    assert_eq!(holds(&finished), (2, 2));
    assert!(transcript.is_empty(), "{transcript:?}");

    let (lines, _, result) = native(&format!(
        "{attempt}\n  let c : Chan.Chan Int = Chan.new 1\n  let _ = Chan.recv c\n  println \"unreachable\")\n"
    ));
    let Err(error_after) = result else {
        panic!("the run did not fail");
    };
    assert!(
        matches!(error_after, RuntimeError::Deadlock),
        "{error_after}"
    );
    assert_eq!(texts(&lines), [error, error]);
}

/// AppKitのbackendはメインのスレッドでしか作れず、テストはメインのスレッドで走らない。GUIを求めないプログラムは
/// ネイティブのbackendの`Park`でも`DefaultPark`と同じに終わり、印も立たないので、backendを作ろうともしない。求めるプログラムでは
/// 作るのに失敗し、`Gui.run`ごとにその誤りが届く。
#[cfg(all(feature = "gui-native", target_os = "macos"))]
#[test]
fn the_appkit_backend_is_made_only_when_a_run_asks_for_a_gui() {
    let native = |src: &str| {
        let control = limits(1);
        let interrupt = Arc::clone(&control);
        let config = RunConfig::default()
            .with_control(control)
            .with_main_park(eml_vm::gui_park(
                Backend::Native,
                Arc::new(move || interrupt.interrupt()),
            ));
        run_with(src, config)
    };
    let plain = "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.run_on_main (fn () -> println \"pinned\"))
";
    let (lines, _, result) = native(plain);
    let default = RunConfig::default()
        .with_control(limits(1))
        .with_main_park(Arc::new(|| Box::new(DefaultPark)));
    let (_, _, default_result) = run_with(plain, default);
    let (ran, default) = (finished(result), finished(default_result));
    assert_eq!(texts(&lines), ["pinned"]);
    assert_eq!(ran.stats, default.stats);
    assert_eq!(holds(&ran), (0, 0));

    let (lines, _, result) = native(&with_app(
        "\
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let attempt () = match try_io (fn () -> Async.run (fn () -> Gui.run (app \"native\"))) with
    | Ok () -> println \"opened\"
    | Err e -> println (show e)
  attempt ()
  attempt ())
",
    ));
    let error = "IOError { kind = NotAvailable, message = \"cannot open the GUI: AppKit needs the main thread\" }";
    assert_eq!(texts(&lines), [error, error]);
    assert_eq!(holds(&finished(result)), (2, 2));
}

const PROGRESS: &str = "\
import Async (Async)
import Async
import Gui (Gui(..), Track(..), Align(..), Cell(..))
import Gui

data Msg =
  | Start
  | Step Int
  | Done Int
  | Quit

chunk : Int -> <Panic> Int
chunk i = if i == 0 then 0 else 1 + chunk (i - 1)

work : Gui.Sink Msg -> <IO, Panic, Async> Unit
work sink =
  let go i acc =
    if i == 10 then Gui.post sink (Done acc)
    else
      let r = Async.compute (fn () -> chunk 1000)
      Gui.post sink (Step (i + 1))
      go (i + 1) (acc + r)
  go 0 0

app : Unit -> <Gui Msg, IO, Panic, Async> Unit
app () =
  use _ <- Gui.window \"Progress\" Quit
  use Gui.grid [Fr 1] []
  let bar = Gui.progress_bar 0
  let start = Gui.button \"Start\" Start
  let sink = Gui.sink ()
  let loop () =
    match Gui.wait () with
      | Start ->
        Gui.enable start False
        Async.spawn_off_main (fn () -> work sink)
        loop ()
      | Step i ->
        Gui.value bar (i * 10)
        loop ()
      | Done total ->
        Gui.enable start True
        println \"total \\{show total}\"
      | Quit -> ()
  loop ()

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run app
";

/// 重い仕事はメインのworkerの外で動き、結果は`Gui.post`で戻る。
#[test]
fn progress_posts_from_off_main_work() {
    let (lines, transcript, result) = run(PROGRESS, "click 1:3\n", 2);
    let finished = finished(result);
    assert_eq!(lines, ["total 10000"]);
    assert_eq!(holds(&finished), (1, 1));
    assert_eq!(count(&transcript, "set #1:2 Value(100)"), 1);
}

/// 本物の時計では、`want_gui_raw`がメインのworkerのdriverを作るので、ループは始めからその源を受け取り、固定した
/// タスクの`Std.Net`のやりとりが終わる。源がなければ、stubはdriverの完了で起こされない。
#[test]
fn pinned_net_io_completes_inside_a_gui_run_on_the_real_clock() {
    let src = "\
import Async (Async)
import Async
import Gui (Gui(..))
import Gui
import Net

data Msg =
  | Quit

client : Int -> Unit -> <Gui Msg, IO, Panic, Async> Unit
client port () =
  let c = Net.connect \"127.0.0.1\" port
  let (line, c) = Net.read_line c
  let c = Net.write_line \"back\" c
  Net.close c
  match line with
    | Some text -> println \"pinned: \\{text}\"
    | None -> println \"eof\"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let g = Async.start (fn () -> Gui.run (client port))
  let (s, l) = Net.accept l
  Net.close_listener l
  let s = Net.write_line \"hello\" s
  let (line, s) = Net.read_line s
  Net.close s
  Async.await g
  match line with
    | Some text -> println \"main: \\{text}\"
    | None -> println \"eof\")
";
    let control = limits(1);
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_control(Arc::clone(&control))
        .with_main_park(gui_park("", &control));
    let start = std::time::Instant::now();
    let (lines, transcript, result) = run_with(src, config);
    let finished = finished(result);
    assert_eq!(texts(&lines), ["pinned: hello", "main: back"]);
    assert_eq!(lines[0].1, std::thread::current().id());
    assert_eq!(holds(&finished), (1, 1));
    assert_eq!(count(&transcript, "run_loop"), 1);
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}

//! 画面を開かないbackend。テストはこれで流す。呼び出しを1つずつ1行としてスレッドの記録に書き、筋書きのイベントを順に流す
//! (docs/implementation/architecture.md の「`eml_gui`の内部」)。

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::{
    Event, EventLoop, EventQueue, GuiError, Id, Init, IoSource, Kind, Measure, Native, Prop,
    RawEvent, Reason, Rect, Size, Step, Widgets,
};

/// 何も起きないまま待つ上限。筋書きの短すぎるテストを、止まったままにせず失敗させる。
const IDLE_LIMIT: Duration = Duration::from_secs(5);

/// I/Oの源を渡されたときの1回の待ちの上限。stubは源を見ないので、driverの操作を進めるために、間をおいて`step(Io)`を呼ぶ。
const IO_POLL: Duration = Duration::from_millis(10);

thread_local! {
    static TRANSCRIPT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// このスレッドのstubの記録を取り出して空にする。
pub fn take_transcript() -> Vec<String> {
    TRANSCRIPT.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

fn log(line: String) {
    TRANSCRIPT.with(|t| t.borrow_mut().push(line));
}

/// stubのループが流すイベントの筋書き。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script(pub Vec<RawEvent>);

impl Script {
    /// 1行に1つのイベントを書く。空の行と`#`で始まる行は飛ばす。
    pub fn parse(text: &str) -> Result<Script, String> {
        let mut events = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim_start();
            if line.trim_end().is_empty() || line.starts_with('#') {
                continue;
            }
            events.push(parse_line(line).map_err(|e| format!("line {}: {e}", n + 1))?);
        }
        Ok(Script(events))
    }
}

fn parse_line(line: &str) -> Result<RawEvent, String> {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    if word == "quit" {
        return if rest.trim().is_empty() {
            Ok(RawEvent::QuitRequested)
        } else {
            Err("`quit` takes no arguments".to_string())
        };
    }
    // `text`の値は行の残りそのままなので、行の末尾の空白も値の一部になる
    let (id, arg) = rest.split_once(' ').unwrap_or((rest, ""));
    let id = id.trim_end();
    let event = match word {
        "click" | "close" | "text" | "toggle" | "select" => {
            let w = parse_id(id)?;
            match word {
                "click" => no_arg(word, arg, Event::Click(w))?,
                "close" => no_arg(word, arg, Event::CloseRequest(w))?,
                "text" => Event::Text(w, arg.to_string()),
                "toggle" => match arg.trim() {
                    "true" => Event::Toggle(w, true),
                    "false" => Event::Toggle(w, false),
                    _ => return Err("`toggle` takes `true` or `false`".to_string()),
                },
                _ => match arg.trim().parse() {
                    Ok(i) => Event::Select(w, i),
                    Err(_) => return Err("`select` takes an integer".to_string()),
                },
            }
        }
        _ => return Err(format!("unknown event `{word}`")),
    };
    Ok(RawEvent::User(event))
}

fn no_arg(word: &str, arg: &str, event: Event) -> Result<Event, String> {
    if arg.trim().is_empty() {
        Ok(event)
    } else {
        Err(format!("`{word}` takes only a widget"))
    }
}

fn parse_id(text: &str) -> Result<Id, String> {
    let bad = || format!("`{text}` is not a widget (W:S)");
    let (w, s) = text.split_once(':').ok_or_else(bad)?;
    match (w.parse(), s.parse()) {
        (Ok(w), Ok(s)) => Ok(Id::new(w, s)),
        _ => Err(bad()),
    }
}

pub(crate) fn create(script: Script) -> Native {
    let queue = EventQueue::new();
    let shown = Rc::new(RefCell::new(BTreeSet::new()));
    let widgets = StubWidgets {
        widgets: HashMap::new(),
        shown: Rc::clone(&shown),
    };
    let event_loop = StubLoop {
        queue: queue.clone(),
        auto_close: script.0.is_empty(),
        script: script.0.into(),
        shown,
        bell: Arc::default(),
        idle_limit: IDLE_LIMIT,
    };
    Native {
        widgets: Rc::new(RefCell::new(widgets)),
        event_loop: Box::new(event_loop),
        queue,
    }
}

struct StubWidget {
    text: String,
    items: Vec<String>,
}

struct StubWidgets {
    widgets: HashMap<Id, StubWidget>,
    /// 表示したウィンドウ。空の筋書きのループは、ここに閉じる要求を送る。
    shown: Rc<RefCell<BTreeSet<Id>>>,
}

impl StubWidgets {
    fn get(&mut self, w: Id) -> Result<&mut StubWidget, GuiError> {
        self.widgets.get_mut(&w).ok_or(GuiError::UnknownId)
    }
}

impl Widgets for StubWidgets {
    fn create_window(&mut self, id: Id, title: &str) -> Result<(), GuiError> {
        log(format!("create_window {id} {title:?}"));
        let text = title.to_string();
        self.widgets.insert(
            id,
            StubWidget {
                text,
                items: Vec::new(),
            },
        );
        Ok(())
    }

    fn create(&mut self, id: Id, kind: Kind, parent: Id, init: &Init) -> Result<(), GuiError> {
        if !self.widgets.contains_key(&parent) {
            return Err(GuiError::UnknownId);
        }
        let (text, shown) = match (kind, init) {
            (Kind::Grid, Init::Grid) | (Kind::Choice, Init::Choice) => {
                (String::new(), String::new())
            }
            (Kind::Label, Init::Label(t))
            | (Kind::Button, Init::Button(t))
            | (Kind::TextField, Init::TextField(t)) => (t.clone(), format!(" {t:?}")),
            (Kind::CheckBox, Init::CheckBox(t, c)) => (t.clone(), format!(" {t:?} checked={c}")),
            (Kind::ProgressBar, Init::ProgressBar(v)) => (String::new(), format!(" value={v}")),
            (Kind::Window, _)
            | (Kind::Grid, _)
            | (Kind::Label, _)
            | (Kind::Button, _)
            | (Kind::TextField, _)
            | (Kind::CheckBox, _)
            | (Kind::Choice, _)
            | (Kind::ProgressBar, _) => return Err(GuiError::WrongKind),
        };
        log(format!("create {kind:?} {id} in {parent}{shown}"));
        self.widgets.insert(
            id,
            StubWidget {
                text,
                items: Vec::new(),
            },
        );
        Ok(())
    }

    fn add_item(&mut self, w: Id, item: &str) -> Result<(), GuiError> {
        self.get(w)?.items.push(item.to_string());
        log(format!("add_item {w} {item:?}"));
        Ok(())
    }

    fn set(&mut self, w: Id, prop: &Prop) -> Result<(), GuiError> {
        let widget = self.get(w)?;
        if let Prop::Text(t) = prop {
            widget.text = t.clone();
        }
        log(format!("set {w} {prop:?}"));
        Ok(())
    }

    /// 幅は文字数の7倍に16を足した値で、高さは24である。Choiceは一番長い項目の幅、ProgressBarは0文字の幅とする。
    fn measure(&mut self, w: Id, _width: Measure, _height: Measure) -> Size {
        log(format!("measure {w}"));
        let chars = self.widgets.get(&w).map_or(0, |widget| {
            let items = widget.items.iter().map(|i| i.chars().count()).max();
            items.unwrap_or(0).max(widget.text.chars().count())
        });
        Size {
            w: 7.0 * chars as f32 + 16.0,
            h: 24.0,
        }
    }

    fn place(&mut self, w: Id, rect: Rect) {
        log(format!(
            "place {w} ({}, {}) {}x{}",
            rect.x, rect.y, rect.w, rect.h
        ));
    }

    fn show_window(&mut self, w: Id, size: Size, min: Size) {
        log(format!(
            "show_window {w} {}x{} min {}x{}",
            size.w, size.h, min.w, min.h
        ));
        self.shown.borrow_mut().insert(w);
    }

    fn destroy(&mut self, w: Id) {
        log(format!("destroy {w}"));
        if w.is_window() {
            self.shown.borrow_mut().remove(&w);
            self.widgets.retain(|id, _| id.window_no() != w.window_no());
        } else {
            self.widgets.remove(&w);
        }
    }

    fn set_presence(&mut self, active: bool) {
        log(format!("presence {}", if active { "on" } else { "off" }));
    }
}

/// 起こされたかどうかの旗と、それを待つための条件変数。
#[derive(Default)]
struct Bell {
    rang: Mutex<bool>,
    cond: Condvar,
}

struct StubLoop {
    queue: EventQueue,
    script: VecDeque<RawEvent>,
    /// 筋書きが初めから空なら、表示したウィンドウに1度だけ閉じる要求を送り、プログラムを終わらせる。
    auto_close: bool,
    shown: Rc<RefCell<BTreeSet<Id>>>,
    bell: Arc<Bell>,
    idle_limit: Duration,
}

impl StubLoop {
    /// 起こされるか、`timeout`が過ぎるまで待つ。I/Oの源があれば`IO_POLL`で切り上げて`Io`を返す。`idle_since`から
    /// 何も起きないまま上限を過ぎたら`None`を返す。`Io`の切り上げは何かが起きたことに数えないので、源があっても止まった
    /// ループは失敗する。
    fn wait(&self, timeout: Option<Duration>, io: bool, idle_since: Instant) -> Option<Reason> {
        let idle_left = self.idle_limit.saturating_sub(idle_since.elapsed());
        let cap = if io {
            IO_POLL.min(idle_left)
        } else {
            idle_left
        };
        let limit = timeout.map_or(cap, |t| t.min(cap));
        let deadline = Instant::now() + limit;
        let mut rang = self.bell.rang.lock().unwrap();
        while !*rang {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            rang = self.bell.cond.wait_timeout(rang, deadline - now).unwrap().0;
        }
        if std::mem::take(&mut *rang) {
            Some(Reason::Woken)
        } else if timeout.is_some_and(|t| t <= cap) {
            Some(Reason::Timeout)
        } else if cap < idle_left {
            Some(Reason::Io)
        } else {
            None
        }
    }
}

impl EventLoop for StubLoop {
    fn run_loop(
        &mut self,
        io: Option<IoSource>,
        _timeout: Option<Duration>,
        step: &mut dyn FnMut(Reason) -> Step,
    ) -> io::Result<()> {
        log("run_loop".to_string());
        // 最初の`step(Woken)`は、その前に鳴った起こしもまとめて扱う
        *self.bell.rang.lock().unwrap() = false;
        let mut reason = Reason::Woken;
        let mut idle_since = Instant::now();
        loop {
            if reason != Reason::Io {
                idle_since = Instant::now();
            }
            let timeout = match step(reason) {
                Step::Stop => {
                    log("stop".to_string());
                    return Ok(());
                }
                Step::Continue(t) => t,
            };
            if let Some(event) = self.script.pop_front() {
                self.queue.push(event);
                reason = Reason::Events;
                continue;
            }
            if self.auto_close && !self.shown.borrow().is_empty() {
                self.auto_close = false;
                for &w in self.shown.borrow().iter() {
                    self.queue.push(RawEvent::User(Event::CloseRequest(w)));
                }
                reason = Reason::Events;
                continue;
            }
            reason = self
                .wait(timeout, io.is_some(), idle_since)
                .ok_or_else(|| io::Error::other("stub: nothing woke the loop for 5 s"))?;
        }
    }

    fn waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let bell = Arc::clone(&self.bell);
        Arc::new(move || {
            *bell.rang.lock().unwrap() = true;
            bell.cond.notify_one();
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Backend, Toolkit};

    fn click(w: u32, s: u32) -> RawEvent {
        RawEvent::User(Event::Click(Id::new(w, s)))
    }

    fn stub_loop(script: Vec<RawEvent>, queue: EventQueue) -> StubLoop {
        StubLoop {
            queue,
            auto_close: script.is_empty(),
            script: script.into(),
            shown: Rc::default(),
            bell: Arc::default(),
            idle_limit: Duration::from_millis(20),
        }
    }

    #[test]
    fn script_parses_every_event() {
        let text = "\
# counter
click 1:4

text 1:3 Ann Lee
toggle 1:4 true
toggle 1:4 false
select 1:5 -2
close 1:0
quit
";
        assert_eq!(
            Script::parse(text),
            Ok(Script(vec![
                click(1, 4),
                RawEvent::User(Event::Text(Id::new(1, 3), "Ann Lee".to_string())),
                RawEvent::User(Event::Toggle(Id::new(1, 4), true)),
                RawEvent::User(Event::Toggle(Id::new(1, 4), false)),
                RawEvent::User(Event::Select(Id::new(1, 5), -2)),
                RawEvent::User(Event::CloseRequest(Id::new(1, 0))),
                RawEvent::QuitRequested,
            ]))
        );
        assert_eq!(
            Script::parse("  text 1:3 a \r\ntext 1:3  "),
            Ok(Script(vec![
                RawEvent::User(Event::Text(Id::new(1, 3), "a ".to_string())),
                RawEvent::User(Event::Text(Id::new(1, 3), " ".to_string())),
            ]))
        );
        assert_eq!(
            Script::parse("text 1:3"),
            Ok(Script(vec![RawEvent::User(Event::Text(
                Id::new(1, 3),
                String::new()
            ))]))
        );
    }

    #[test]
    fn script_parse_errors() {
        let cases = [
            ("press 1:2", "line 1: unknown event `press`"),
            ("\nclick 1", "line 2: `1` is not a widget (W:S)"),
            ("click a:b", "line 1: `a:b` is not a widget (W:S)"),
            ("click", "line 1: `` is not a widget (W:S)"),
            ("click 1:2 now", "line 1: `click` takes only a widget"),
            ("close 1:0 x", "line 1: `close` takes only a widget"),
            ("toggle 1:2 yes", "line 1: `toggle` takes `true` or `false`"),
            ("select 1:2 x", "line 1: `select` takes an integer"),
            ("quit now", "line 1: `quit` takes no arguments"),
        ];
        for (text, message) in cases {
            assert_eq!(Script::parse(text), Err(message.to_string()), "{text}");
        }
    }

    #[test]
    fn transcript_lines() {
        take_transcript();
        let native = Backend::Stub(Script::default()).create().unwrap();
        let mut w = native.widgets.borrow_mut();
        let win = Id::new(1, 0);
        let grid = Id::new(1, 1);
        w.set_presence(true);
        w.create_window(win, "Counter").unwrap();
        w.create(grid, Kind::Grid, win, &Init::Grid).unwrap();
        w.create(Id::new(1, 2), Kind::Label, grid, &Init::Label("0".into()))
            .unwrap();
        w.create(Id::new(1, 3), Kind::Button, grid, &Init::Button("-".into()))
            .unwrap();
        w.create(
            Id::new(1, 4),
            Kind::CheckBox,
            grid,
            &Init::CheckBox("ok".into(), true),
        )
        .unwrap();
        w.create(Id::new(1, 5), Kind::Choice, grid, &Init::Choice)
            .unwrap();
        w.create(
            Id::new(1, 6),
            Kind::ProgressBar,
            grid,
            &Init::ProgressBar(0),
        )
        .unwrap();
        w.add_item(Id::new(1, 5), "red").unwrap();
        w.set(Id::new(1, 2), &Prop::Text("1".into())).unwrap();
        w.set(Id::new(1, 6), &Prop::Value(30)).unwrap();
        w.set(Id::new(1, 4), &Prop::Checked(true)).unwrap();
        w.set(Id::new(1, 3), &Prop::Enabled(false)).unwrap();
        w.measure(Id::new(1, 2), Measure::Unbounded, Measure::Unbounded);
        w.place(
            Id::new(1, 2),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 54.0,
                h: 24.0,
            },
        );
        let size = Size { w: 78.0, h: 80.0 };
        w.show_window(win, size, size);
        w.destroy(win);
        w.set_presence(false);
        assert_eq!(
            take_transcript(),
            [
                "presence on",
                "create_window #1:0 \"Counter\"",
                "create Grid #1:1 in #1:0",
                "create Label #1:2 in #1:1 \"0\"",
                "create Button #1:3 in #1:1 \"-\"",
                "create CheckBox #1:4 in #1:1 \"ok\" checked=true",
                "create Choice #1:5 in #1:1",
                "create ProgressBar #1:6 in #1:1 value=0",
                "add_item #1:5 \"red\"",
                "set #1:2 Text(\"1\")",
                "set #1:6 Value(30)",
                "set #1:4 Checked(true)",
                "set #1:3 Enabled(false)",
                "measure #1:2",
                "place #1:2 (0, 0) 54x24",
                "show_window #1:0 78x80 min 78x80",
                "destroy #1:0",
                "presence off",
            ]
        );
        assert_eq!(
            w.set(Id::new(1, 2), &Prop::Enabled(true)),
            Err(GuiError::UnknownId)
        );
    }

    #[test]
    fn measure_rule() {
        let native = Backend::Stub(Script::default()).create().unwrap();
        let mut w = native.widgets.borrow_mut();
        let win = Id::new(1, 0);
        let any = Measure::Unbounded;
        w.create_window(win, "W").unwrap();
        w.create(Id::new(1, 1), Kind::Label, win, &Init::Label("Name".into()))
            .unwrap();
        w.create(
            Id::new(1, 2),
            Kind::TextField,
            win,
            &Init::TextField(String::new()),
        )
        .unwrap();
        w.create(Id::new(1, 3), Kind::Choice, win, &Init::Choice)
            .unwrap();
        w.create(
            Id::new(1, 4),
            Kind::ProgressBar,
            win,
            &Init::ProgressBar(50),
        )
        .unwrap();
        w.add_item(Id::new(1, 3), "red").unwrap();
        w.add_item(Id::new(1, 3), "yellow").unwrap();
        w.add_item(Id::new(1, 3), "blue").unwrap();
        let size = |w: f32| Size { w, h: 24.0 };
        assert_eq!(w.measure(Id::new(1, 1), any, any), size(44.0));
        assert_eq!(w.measure(Id::new(1, 2), any, any), size(16.0));
        assert_eq!(w.measure(Id::new(1, 3), any, any), size(58.0));
        assert_eq!(w.measure(Id::new(1, 4), any, any), size(16.0));
        w.set(Id::new(1, 1), &Prop::Text("Name:".into())).unwrap();
        assert_eq!(w.measure(Id::new(1, 1), any, any), size(51.0));
        take_transcript();
    }

    #[test]
    fn create_has_a_line_for_every_kind() {
        let native = Backend::Stub(Script::default()).create().unwrap();
        let mut w = native.widgets.borrow_mut();
        let win = Id::new(1, 0);
        w.create_window(win, "W").unwrap();
        take_transcript();
        let kinds = [
            Kind::Window,
            Kind::Grid,
            Kind::Label,
            Kind::Button,
            Kind::TextField,
            Kind::CheckBox,
            Kind::Choice,
            Kind::ProgressBar,
        ];
        for (seq, kind) in (1..).zip(kinds) {
            let id = Id::new(1, seq);
            // 種類を足すと、この`match`が網羅でなくなり、stubと各backendを見直すことになる
            let (init, line) = match kind {
                Kind::Window => {
                    assert_eq!(
                        w.create(id, kind, win, &Init::Grid),
                        Err(GuiError::WrongKind)
                    );
                    continue;
                }
                Kind::Grid => (Init::Grid, format!("create Grid {id} in #1:0")),
                Kind::Label => (
                    Init::Label("a".into()),
                    format!("create Label {id} in #1:0 \"a\""),
                ),
                Kind::Button => (
                    Init::Button("b".into()),
                    format!("create Button {id} in #1:0 \"b\""),
                ),
                Kind::TextField => (
                    Init::TextField("c".into()),
                    format!("create TextField {id} in #1:0 \"c\""),
                ),
                Kind::CheckBox => (
                    Init::CheckBox("d".into(), false),
                    format!("create CheckBox {id} in #1:0 \"d\" checked=false"),
                ),
                Kind::Choice => (Init::Choice, format!("create Choice {id} in #1:0")),
                Kind::ProgressBar => (
                    Init::ProgressBar(7),
                    format!("create ProgressBar {id} in #1:0 value=7"),
                ),
            };
            w.create(id, kind, win, &init).unwrap();
            assert_eq!(take_transcript(), [line]);
        }
    }

    #[test]
    fn set_and_add_item_emit_nothing() {
        let native = Backend::Stub(Script::default()).create().unwrap();
        let mut toolkit = Toolkit::new(Rc::clone(&native.widgets), native.queue.clone());
        toolkit.begin_window("W").unwrap();
        let field = toolkit.create(Init::TextField(String::new())).unwrap();
        let check = toolkit.create(Init::CheckBox("c".into(), false)).unwrap();
        let choice = toolkit.create(Init::Choice).unwrap();
        toolkit.add_item(choice, "a").unwrap();
        toolkit.set(field, Prop::Text("x".into())).unwrap();
        toolkit.set(check, Prop::Checked(true)).unwrap();
        toolkit.set(choice, Prop::Value(0)).unwrap();
        assert!(!toolkit.has_events());
        assert!(native.queue.take_control().is_empty());
        assert_eq!(toolkit.poll(), None);
        take_transcript();
    }

    #[test]
    fn run_loop_steps_woken_first_even_after_an_early_ring() {
        let mut lp = stub_loop(vec![click(1, 1)], EventQueue::new());
        lp.waker()();
        let mut reasons = Vec::new();
        lp.run_loop(None, None, &mut |r| {
            reasons.push(r);
            if reasons.len() == 2 {
                Step::Stop
            } else {
                Step::Continue(None)
            }
        })
        .unwrap();
        assert_eq!(reasons, [Reason::Woken, Reason::Events]);
        assert_eq!(take_transcript(), ["run_loop", "stop"]);
    }

    #[test]
    fn run_loop_feeds_one_scripted_event_per_step_before_rings_and_timeouts() {
        let queue = EventQueue::new();
        queue.attach();
        let mut lp = stub_loop(vec![click(1, 1), click(1, 2)], queue.clone());
        let wake = lp.waker();
        let mut seen = Vec::new();
        lp.run_loop(None, Some(Duration::ZERO), &mut |r| {
            seen.push((r, queue.pop_user()));
            wake();
            if seen.len() == 4 {
                Step::Stop
            } else {
                Step::Continue(Some(Duration::ZERO))
            }
        })
        .unwrap();
        let click = |s| Some(Event::Click(Id::new(1, s)));
        assert_eq!(
            seen,
            [
                (Reason::Woken, None),
                (Reason::Events, click(1)),
                (Reason::Events, click(2)),
                (Reason::Woken, None),
            ]
        );
        take_transcript();
    }

    #[test]
    fn run_loop_times_out_and_wakes() {
        let mut lp = stub_loop(vec![], EventQueue::new());
        let wake = lp.waker();
        let mut reasons = Vec::new();
        lp.run_loop(None, None, &mut |r| {
            reasons.push(r);
            match reasons.len() {
                1 => Step::Continue(Some(Duration::from_millis(1))),
                2 => {
                    wake();
                    Step::Continue(None)
                }
                _ => Step::Stop,
            }
        })
        .unwrap();
        assert_eq!(reasons, [Reason::Woken, Reason::Timeout, Reason::Woken]);
        take_transcript();
    }

    #[cfg(unix)]
    #[test]
    fn run_loop_with_an_io_source_steps_io_every_10_ms() {
        let mut lp = stub_loop(vec![], EventQueue::new());
        lp.idle_limit = IDLE_LIMIT;
        let mut reasons = Vec::new();
        lp.run_loop(Some(IoSource::Fd(-1)), None, &mut |r| {
            reasons.push(r);
            match reasons.len() {
                1 | 2 => Step::Continue(None),
                3 => Step::Continue(Some(Duration::from_millis(1))),
                4 => Step::Continue(Some(Duration::from_secs(60))),
                _ => Step::Stop,
            }
        })
        .unwrap();
        assert_eq!(
            reasons,
            [
                Reason::Woken,
                Reason::Io,
                Reason::Io,
                Reason::Timeout,
                Reason::Io,
            ]
        );
        take_transcript();
    }

    /// I/Oの源があっても、`step(Io)`だけが続くループは、何も起きないままの上限 (このテストでは20 ms) で失敗する。
    /// 起こし、筋書きのイベント、期限は数え直させる。
    #[cfg(unix)]
    #[test]
    fn run_loop_with_an_io_source_fails_when_only_io_steps_follow() {
        let mut lp = stub_loop(vec![], EventQueue::new());
        let mut reasons = Vec::new();
        let start = Instant::now();
        let err = lp
            .run_loop(Some(IoSource::Fd(-1)), None, &mut |r| {
                reasons.push(r);
                Step::Continue(None)
            })
            .unwrap_err();
        assert_eq!(err.to_string(), "stub: nothing woke the loop for 5 s");
        assert!(start.elapsed() >= Duration::from_millis(20));
        assert_eq!(reasons[0], Reason::Woken);
        assert!(reasons.len() >= 2);
        assert!(reasons[1..].iter().all(|&r| r == Reason::Io), "{reasons:?}");
        let mut lp = stub_loop(vec![], EventQueue::new());
        let mut steps = 0;
        let start = Instant::now();
        lp.run_loop(Some(IoSource::Fd(-1)), None, &mut |_| {
            steps += 1;
            if start.elapsed() >= Duration::from_millis(60) {
                Step::Stop
            } else {
                Step::Continue(Some(Duration::from_millis(5)))
            }
        })
        .unwrap();
        assert!(steps > 1);
        take_transcript();
    }

    #[test]
    fn run_loop_fails_when_nothing_wakes_it() {
        let mut lp = stub_loop(vec![], EventQueue::new());
        let err = lp
            .run_loop(None, None, &mut |_| Step::Continue(None))
            .unwrap_err();
        assert_eq!(err.to_string(), "stub: nothing woke the loop for 5 s");
        let mut lp = stub_loop(vec![], EventQueue::new());
        let long = Some(Duration::from_secs(60));
        let err = lp
            .run_loop(None, None, &mut |_| Step::Continue(long))
            .unwrap_err();
        assert_eq!(err.to_string(), "stub: nothing woke the loop for 5 s");
        take_transcript();
    }

    #[test]
    fn empty_script_closes_every_shown_window_once() {
        let Native {
            widgets,
            mut event_loop,
            queue,
        } = Backend::Stub(Script::default()).create().unwrap();
        let mut toolkit = Toolkit::new(widgets, queue);
        let mut seen = Vec::new();
        let mut steps = 0;
        event_loop
            .run_loop(None, None, &mut |r| {
                steps += 1;
                while let Some(e) = toolkit.poll() {
                    seen.push((r, e));
                }
                match steps {
                    // 表示したウィンドウがなければ、閉じる要求は送らない
                    1 => Step::Continue(Some(Duration::ZERO)),
                    2 => {
                        toolkit.begin_window("A").unwrap();
                        toolkit.end().unwrap();
                        toolkit.begin_window("B").unwrap();
                        toolkit.end().unwrap();
                        toolkit.flush();
                        Step::Continue(None)
                    }
                    3 => Step::Continue(Some(Duration::ZERO)),
                    _ => Step::Stop,
                }
            })
            .unwrap();
        assert_eq!(steps, 4);
        assert_eq!(
            seen,
            [
                (Reason::Events, Event::CloseRequest(Id::new(1, 0))),
                (Reason::Events, Event::CloseRequest(Id::new(2, 0))),
            ]
        );
        take_transcript();
    }

    #[test]
    fn a_non_empty_script_does_not_close_windows() {
        let Native {
            widgets,
            mut event_loop,
            queue,
        } = Backend::Stub(Script(vec![click(1, 1)])).create().unwrap();
        let mut toolkit = Toolkit::new(widgets, queue);
        toolkit.begin_window("A").unwrap();
        let button = toolkit.create(Init::Button("b".into())).unwrap();
        toolkit.flush();
        let mut seen = Vec::new();
        event_loop
            .run_loop(None, None, &mut |r| {
                while let Some(e) = toolkit.poll() {
                    seen.push(e);
                }
                if r == Reason::Timeout {
                    Step::Stop
                } else {
                    Step::Continue(Some(Duration::ZERO))
                }
            })
            .unwrap();
        assert_eq!(seen, [Event::Click(button)]);
        take_transcript();
    }
}

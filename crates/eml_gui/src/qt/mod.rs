//! LinuxのQt 6のbackend。ウィジェットとループはC++のshim (`shim.cpp`) がQtで作り、Rustとはcxxの橋でつなぐ。Qtは
//! メインのスレッドで、画面があるときにだけ作る (docs/implementation/architecture.mdの「`eml_gui`の内部」)。

mod event_loop;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::rc::Rc;
use std::sync::Arc;

use cxx::UniquePtr;

use crate::{
    Event, EventQueue, GuiError, Id, Init, Kind, Measure, Native, Prop, RawEvent, Rect, Size,
    Widgets,
};

#[cxx::bridge(namespace = "eml_gui")]
mod ffi {
    /// `Grid`から`ProgressBar`までの、ウィンドウでないウィジェットの種類。
    enum WidgetKind {
        Grid,
        Label,
        Button,
        TextField,
        CheckBox,
        Choice,
        ProgressBar,
    }

    /// `minimumSizeHint`と`sizeHint`。無効な大きさは0になる。
    struct Hints {
        min_w: f64,
        min_h: f64,
        w: f64,
        h: f64,
    }

    extern "Rust" {
        type Events;
        fn click(self: &Events, id: u64);
        fn text(self: &Events, id: u64, text: &str);
        fn toggle(self: &Events, id: u64, on: bool);
        fn select(self: &Events, id: u64, index: i32);
        fn close_request(self: &Events, id: u64);
        fn resized(self: &Events, id: u64, w: f64, h: f64);
        fn invalidate(self: &Events, id: u64);
    }

    unsafe extern "C++" {
        include!("eml_gui/src/qt/shim.hpp");

        type Shim;

        fn on_main_thread() -> bool;
        fn app_exists() -> bool;
        fn new_shim(events: Box<Events>) -> UniquePtr<Shim>;
        /// # Safety
        /// `dispatcher`は`Shim::dispatcher`が返したもので、その`Shim`はまだ生きている。
        unsafe fn wake(dispatcher: usize);

        fn create_window(self: &Shim, id: u64, title: &str);
        fn create(self: &Shim, id: u64, kind: WidgetKind, parent: u64, text: &str, value: i64);
        fn add_item(self: &Shim, id: u64, item: &str);
        fn set_text(self: &Shim, id: u64, text: &str);
        fn set_value(self: &Shim, id: u64, value: i64);
        fn set_checked(self: &Shim, id: u64, on: bool);
        fn set_enabled(self: &Shim, id: u64, on: bool);
        fn hints(self: &Shim, id: u64) -> Hints;
        fn one_line(self: &Shim, id: u64) -> Hints;
        fn height_for_width(self: &Shim, id: u64, width: f64) -> f64;
        fn place(self: &Shim, id: u64, x: f64, y: f64, w: f64, h: f64);
        fn show_window(self: &Shim, id: u64, w: f64, h: f64, min_w: f64, min_h: f64);
        fn destroy(self: &Shim, id: u64);

        fn dispatcher(self: &Shim) -> usize;
        fn watch(self: &Shim, fd: i32);
        fn unwatch(self: &Shim);
        fn arm(self: &Shim, ms: i64);
        fn process(self: &Shim, wait: bool);
        fn take_io(self: &Shim) -> bool;
        fn take_timeout(self: &Shim) -> bool;
    }
}

const NO_DISPLAY: &str = "cannot open the GUI: no display (DISPLAY and WAYLAND_DISPLAY are unset)";

pub(crate) fn create() -> Result<Native, GuiError> {
    // QApplicationはmain()のスレッドで作るものとされ、ほかのスレッドで作るとQtが警告する
    if !ffi::on_main_thread() {
        return Err(GuiError::NotAvailable(
            "cannot open the GUI: Qt needs the main thread".to_string(),
        ));
    }
    // 画面がないと、QApplicationはプロセスを止めてしまう
    let display = std::env::var_os("DISPLAY");
    let wayland = std::env::var_os("WAYLAND_DISPLAY");
    if !has_display(display.as_deref(), wayland.as_deref()) {
        return Err(GuiError::Backend(NO_DISPLAY.to_string()));
    }
    // QApplicationはプロセスに1つで、backendはrunに1つだけ作る
    debug_assert!(!ffi::app_exists(), "a QApplication is already alive");
    let queue = EventQueue::new();
    let shim = ffi::new_shim(Box::new(Events(queue.clone())));
    let bell = Arc::new(event_loop::Bell::new(shim.dispatcher()));
    let qt = Rc::new(Qt { shim, bell });
    let widgets = QtWidgets {
        qt: Rc::clone(&qt),
        kinds: HashMap::new(),
    };
    Ok(Native {
        widgets: Rc::new(RefCell::new(widgets)),
        event_loop: Box::new(event_loop::QtLoop::new(qt, queue.clone())),
        queue,
    })
}

fn has_display(display: Option<&OsStr>, wayland: Option<&OsStr>) -> bool {
    [display, wayland]
        .into_iter()
        .any(|v| v.is_some_and(|v| !v.is_empty()))
}

/// ウィジェットとループが共有するshim。
struct Qt {
    shim: UniquePtr<ffi::Shim>,
    bell: Arc<event_loop::Bell>,
}

impl Qt {
    fn shim(&self) -> &ffi::Shim {
        self.shim.as_ref().expect("new_shim returns a shim")
    }
}

impl Drop for Qt {
    /// 起こしは`Qt`より長く生きうるので、QApplicationとそのdispatcherを壊す前に、起こしからdispatcherを外す。
    fn drop(&mut self) {
        self.bell.forget();
    }
}

/// Qtのcallbackが呼ぶ、`EventQueue`に積むだけのもの。
struct Events(EventQueue);

impl Events {
    fn user(&self, event: Event) {
        self.0.push(RawEvent::User(event));
    }

    fn click(&self, id: u64) {
        self.user(Event::Click(Id(id)));
    }

    fn text(&self, id: u64, text: &str) {
        self.user(Event::Text(Id(id), text.to_string()));
    }

    fn toggle(&self, id: u64, on: bool) {
        self.user(Event::Toggle(Id(id), on));
    }

    fn select(&self, id: u64, index: i32) {
        self.user(Event::Select(Id(id), i64::from(index)));
    }

    fn close_request(&self, id: u64) {
        self.user(Event::CloseRequest(Id(id)));
    }

    fn resized(&self, id: u64, w: f64, h: f64) {
        let size = Size {
            w: w as f32,
            h: h as f32,
        };
        self.0.push(RawEvent::Resized(Id(id), size));
    }

    fn invalidate(&self, id: u64) {
        self.0.push(RawEvent::Invalidate(Id(id)));
    }
}

struct QtWidgets {
    qt: Rc<Qt>,
    kinds: HashMap<Id, Kind>,
}

impl QtWidgets {
    fn shim(&self) -> &ffi::Shim {
        self.qt.shim()
    }

    /// 折り返すラベルを並べる幅。上限がなければ1行の幅で、min-contentの問い合わせ (`AtMost(0.0)`) なら一番長い語の
    /// 幅 (`minimumSizeHint`) である。
    fn label_width(&self, w: Id, width: Measure, hints: &ffi::Hints) -> f64 {
        match width {
            Measure::Exact(v) => f64::from(v),
            Measure::AtMost(v) if v > 0.0 => self
                .shim()
                .one_line(w.0)
                .w
                .min(f64::from(v))
                .max(hints.min_w),
            Measure::AtMost(_) => hints.min_w,
            Measure::Unbounded => self.shim().one_line(w.0).w,
        }
    }
}

fn widget_kind(kind: Kind) -> Option<ffi::WidgetKind> {
    match kind {
        Kind::Window => None,
        Kind::Grid => Some(ffi::WidgetKind::Grid),
        Kind::Label => Some(ffi::WidgetKind::Label),
        Kind::Button => Some(ffi::WidgetKind::Button),
        Kind::TextField => Some(ffi::WidgetKind::TextField),
        Kind::CheckBox => Some(ffi::WidgetKind::CheckBox),
        Kind::Choice => Some(ffi::WidgetKind::Choice),
        Kind::ProgressBar => Some(ffi::WidgetKind::ProgressBar),
    }
}

impl Widgets for QtWidgets {
    fn create_window(&mut self, id: Id, title: &str) -> Result<(), GuiError> {
        self.shim().create_window(id.0, title);
        self.kinds.insert(id, Kind::Window);
        Ok(())
    }

    fn create(&mut self, id: Id, kind: Kind, parent: Id, init: &Init) -> Result<(), GuiError> {
        match self.kinds.get(&parent) {
            Some(Kind::Window | Kind::Grid) => {}
            Some(_) => return Err(GuiError::WrongKind),
            None => return Err(GuiError::UnknownId),
        }
        let (Some(code), true) = (widget_kind(kind), init.kind() == kind) else {
            return Err(GuiError::WrongKind);
        };
        let (text, value) = match init {
            Init::Grid | Init::Choice => ("", 0),
            Init::Label(text) | Init::Button(text) | Init::TextField(text) => (text.as_str(), 0),
            Init::CheckBox(text, checked) => (text.as_str(), i64::from(*checked)),
            Init::ProgressBar(value) => ("", *value),
        };
        self.shim().create(id.0, code, parent.0, text, value);
        self.kinds.insert(id, kind);
        Ok(())
    }

    fn add_item(&mut self, w: Id, item: &str) -> Result<(), GuiError> {
        match self.kinds.get(&w) {
            Some(Kind::Choice) => {
                self.shim().add_item(w.0, item);
                Ok(())
            }
            Some(_) => Err(GuiError::WrongKind),
            None => Err(GuiError::UnknownId),
        }
    }

    fn set(&mut self, w: Id, prop: &Prop) -> Result<(), GuiError> {
        let kind = *self.kinds.get(&w).ok_or(GuiError::UnknownId)?;
        let shim = self.shim();
        match (kind, prop) {
            (
                Kind::Window | Kind::Label | Kind::Button | Kind::TextField | Kind::CheckBox,
                Prop::Text(text),
            ) => shim.set_text(w.0, text),
            (Kind::ProgressBar | Kind::Choice, Prop::Value(v)) => shim.set_value(w.0, *v),
            (Kind::CheckBox, Prop::Checked(on)) => shim.set_checked(w.0, *on),
            (
                Kind::Button | Kind::TextField | Kind::CheckBox | Kind::Choice | Kind::ProgressBar,
                Prop::Enabled(on),
            ) => shim.set_enabled(w.0, *on),
            (
                Kind::Window
                | Kind::Grid
                | Kind::Label
                | Kind::Button
                | Kind::TextField
                | Kind::CheckBox
                | Kind::Choice
                | Kind::ProgressBar,
                _,
            ) => return Err(GuiError::WrongKind),
        }
        Ok(())
    }

    /// 折り返すラベルは並べる幅を決めてから`heightForWidth`で高さを測る。ほかのウィジェットの幅は、min-contentの
    /// 問い合わせなら`minimumSizeHint`、それ以外は`sizeHint`である。`Exact`の軸は与えた値を返す。
    fn measure(&mut self, w: Id, width: Measure, height: Measure) -> Size {
        let Some(&kind) = self.kinds.get(&w) else {
            return Size::default();
        };
        let hints = self.shim().hints(w.0);
        let (natural_w, natural_h) = if kind == Kind::Label {
            let laid = self.label_width(w, width, &hints);
            (laid, self.shim().height_for_width(w.0, laid))
        } else {
            let laid = match width {
                Measure::Exact(v) => f64::from(v),
                Measure::AtMost(v) if v > 0.0 => hints.w.min(f64::from(v)).max(hints.min_w),
                Measure::AtMost(_) => hints.min_w,
                Measure::Unbounded => hints.w,
            };
            (laid, hints.h)
        };
        let exact = |m: Measure, v: f64| match m {
            Measure::Exact(e) => e,
            Measure::AtMost(_) | Measure::Unbounded => v as f32,
        };
        Size {
            w: exact(width, natural_w),
            h: exact(height, natural_h),
        }
    }

    fn place(&mut self, w: Id, rect: Rect) {
        if self.kinds.contains_key(&w) {
            let [x, y, width, height] = [rect.x, rect.y, rect.w, rect.h].map(f64::from);
            self.shim().place(w.0, x, y, width, height);
        }
    }

    fn show_window(&mut self, w: Id, size: Size, min: Size) {
        if self.kinds.get(&w) == Some(&Kind::Window) {
            let [width, height, min_w, min_h] = [size.w, size.h, min.w, min.h].map(f64::from);
            self.shim().show_window(w.0, width, height, min_w, min_h);
        }
    }

    fn destroy(&mut self, w: Id) {
        if self.kinds.remove(&w).is_none() {
            return;
        }
        if w.is_window() {
            self.kinds.retain(|id, _| id.window_no() != w.window_no());
        }
        self.shim().destroy(w.0);
    }

    /// Qtには、Dockのような、アプリケーションの出入りの表示がない。
    fn set_presence(&mut self, _active: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テストはメインのスレッドで走らないので、Qtのbackendは作れない。そのときQApplicationも作らない。
    #[test]
    fn off_the_main_thread_the_backend_is_not_available_and_touches_nothing() {
        assert!(!ffi::on_main_thread());
        assert_eq!(
            create().err(),
            Some(GuiError::NotAvailable(
                "cannot open the GUI: Qt needs the main thread".to_string()
            ))
        );
        assert!(!ffi::app_exists());
    }

    #[test]
    fn a_display_is_x11_or_wayland() {
        let set = Some(OsStr::new(":0"));
        let empty = Some(OsStr::new(""));
        assert!(has_display(set, None));
        assert!(has_display(None, Some(OsStr::new("wayland-0"))));
        assert!(has_display(empty, set));
        assert!(!has_display(None, None));
        assert!(!has_display(empty, empty));
    }
}

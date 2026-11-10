//! macOSのAppKitのbackend。AppKitはメインのスレッドでしか使えないので、`create`はそこでしか成功しない
//! (docs/implementation/architecture.mdの「`eml_gui`の内部」)。

mod event_loop;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSBackingStoreType, NSButton, NSControlStateValueOff,
    NSControlStateValueOn, NSControlTextEditingDelegate, NSMenu, NSMenuItem, NSPopUpButton,
    NSProgressIndicator, NSProgressIndicatorStyle, NSTextField, NSTextFieldDelegate, NSView,
    NSWindow, NSWindowAnimationBehavior, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

use crate::{
    Event, EventQueue, GuiError, Id, Init, Kind, Measure, Native, Prop, RawEvent, Rect, Size,
    Widgets,
};

/// AppKitは編集する欄と進み具合の棒に自然な幅を与えない (`fittingSize`の幅が0になる)。この幅を自然な幅とする。
const FIELD_WIDTH: f64 = 160.0;

pub(crate) fn create() -> Result<Native, GuiError> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err(GuiError::NotAvailable(
            "cannot open the GUI: AppKit needs the main thread".to_string(),
        ));
    };
    let queue = EventQueue::new();
    let app = NSApplication::sharedApplication(mtm);
    // Dockのアイコンは`Ui`が開いている間だけ出す (`set_presence`)
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let delegate = AppDelegate::new(mtm, queue.clone());
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.setMainMenu(Some(&main_menu(mtm)));
    let measurer = NSTextField::labelWithString(&NSString::new(), mtm);
    let widgets = AppKitWidgets {
        mtm,
        app: app.clone(),
        queue: queue.clone(),
        windows: HashMap::new(),
        controls: HashMap::new(),
        measurer,
        _delegate: delegate,
    };
    let event_loop = event_loop::AppKitLoop::new(app, queue.clone());
    Ok(Native {
        widgets: Rc::new(RefCell::new(widgets)),
        event_loop: Box::new(event_loop),
        queue,
    })
}

/// ⌘Qで終了を求められるように、アプリケーションのメニューに「Quit」だけを置く。終了の求めは`AppDelegate`が
/// `QuitRequested`に変える。
fn main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let bar = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    bar.addItem(&app_item);
    let app_menu = NSMenu::new(mtm);
    // SAFETY: `terminate:`はNSApplicationのメソッドで、メニューはそれを最初の応答者の連鎖に送る
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Quit"),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    app_menu.addItem(&quit);
    app_item.setSubmenu(Some(&app_menu));
    bar
}

struct Ivars {
    id: Id,
    queue: EventQueue,
}

define_class!(
    /// 上を原点にする`NSView`。ウィンドウの中身と格子はこれで、子を`Rect`のとおりに左上から置ける。
    // SAFETY: NSViewを継承するときの決まりはなく、`Drop`も実装しない
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct FlippedView;

    impl FlippedView {
        // SAFETY: シグネチャは`isFlipped`と同じである
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    fn new(mtm: MainThreadMarker) -> Retained<FlippedView> {
        // SAFETY: NSObjectの`init`のシグネチャは正しい
        unsafe { msg_send![FlippedView::alloc(mtm), init] }
    }
}

define_class!(
    /// 1つのウィジェットの操作を`EventQueue`に積むもの。コントロールのtargetか、テキストの欄のdelegateになる。
    // SAFETY: NSObjectを継承するときの決まりはなく、`Drop`も実装しない
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Target;

    impl Target {
        // SAFETY: アクションのシグネチャは`- (void)name:(id)sender`である
        #[unsafe(method(click:))]
        fn click(&self, _sender: Option<&AnyObject>) {
            self.push(Event::Click(self.ivars().id));
        }

        // SAFETY: 同上
        #[unsafe(method(toggle:))]
        fn toggle(&self, sender: Option<&AnyObject>) {
            if let Some(button) = sender.and_then(|s| s.downcast_ref::<NSButton>()) {
                let on = button.state() == NSControlStateValueOn;
                self.push(Event::Toggle(self.ivars().id, on));
            }
        }

        // SAFETY: 同上
        #[unsafe(method(select:))]
        fn select(&self, sender: Option<&AnyObject>) {
            if let Some(choice) = sender.and_then(|s| s.downcast_ref::<NSPopUpButton>()) {
                let index = choice.indexOfSelectedItem();
                self.push(Event::Select(self.ivars().id, index as i64));
            }
        }
    }

    unsafe impl NSObjectProtocol for Target {}

    // SAFETY: このprotocolに安全のための決まりはない
    unsafe impl NSControlTextEditingDelegate for Target {
        // SAFETY: シグネチャはprotocolのものと同じである
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            let field = notification.object();
            if let Some(field) = field.as_deref().and_then(|f| f.downcast_ref::<NSTextField>()) {
                let text = field.stringValue().to_string();
                self.push(Event::Text(self.ivars().id, text));
            }
        }
    }

    // SAFETY: このprotocolに安全のための決まりはない
    unsafe impl NSTextFieldDelegate for Target {}
);

impl Target {
    fn new(mtm: MainThreadMarker, id: Id, queue: EventQueue) -> Retained<Target> {
        let this = Target::alloc(mtm).set_ivars(Ivars { id, queue });
        // SAFETY: NSObjectの`init`のシグネチャは正しい
        unsafe { msg_send![super(this), init] }
    }

    fn push(&self, event: Event) {
        self.ivars().queue.push(RawEvent::User(event));
    }
}

define_class!(
    /// ウィンドウの閉じる操作と大きさの変化を`EventQueue`に積むdelegate。ウィンドウを閉じるのはプログラムなので、閉じる
    /// ボタンはここで止める。
    // SAFETY: NSObjectを継承するときの決まりはなく、`Drop`も実装しない
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct WindowWatcher;

    unsafe impl NSObjectProtocol for WindowWatcher {}

    // SAFETY: このprotocolに安全のための決まりはない
    unsafe impl NSWindowDelegate for WindowWatcher {
        // SAFETY: シグネチャはprotocolのものと同じである
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            let ivars = self.ivars();
            ivars
                .queue
                .push(RawEvent::User(Event::CloseRequest(ivars.id)));
            false
        }

        // SAFETY: 同上
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, notification: &NSNotification) {
            let window = notification.object();
            let Some(window) = window.as_deref().and_then(|w| w.downcast_ref::<NSWindow>()) else {
                return;
            };
            let Some(content) = window.contentView() else {
                return;
            };
            let size = content.frame().size;
            let size = Size {
                w: size.width as f32,
                h: size.height as f32,
            };
            self.ivars()
                .queue
                .push(RawEvent::Resized(self.ivars().id, size));
        }
    }
);

impl WindowWatcher {
    fn new(mtm: MainThreadMarker, id: Id, queue: EventQueue) -> Retained<WindowWatcher> {
        let this = WindowWatcher::alloc(mtm).set_ivars(Ivars { id, queue });
        // SAFETY: NSObjectの`init`のシグネチャは正しい
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    /// OSの終了の操作 (⌘Q、Dockの「終了」) を`QuitRequested`に変え、アプリケーションの終了は取りやめる。ループを
    /// 終えるのはrunだけである (docs/spec/vm.mdの「GUIのPark」)。
    // SAFETY: NSObjectを継承するときの決まりはなく、`Drop`も実装しない
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = EventQueue]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    // SAFETY: このprotocolに安全のための決まりはない
    unsafe impl NSApplicationDelegate for AppDelegate {
        // SAFETY: シグネチャはprotocolのものと同じである
        #[unsafe(method(applicationShouldTerminate:))]
        fn application_should_terminate(
            &self,
            _sender: &NSApplication,
        ) -> NSApplicationTerminateReply {
            self.ivars().push(RawEvent::QuitRequested);
            NSApplicationTerminateReply::TerminateCancel
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, queue: EventQueue) -> Retained<AppDelegate> {
        let this = AppDelegate::alloc(mtm).set_ivars(queue);
        // SAFETY: NSObjectの`init`のシグネチャは正しい
        unsafe { msg_send![super(this), init] }
    }
}

struct AppKitWindow {
    window: Retained<NSWindow>,
    content: Retained<FlippedView>,
    /// ウィンドウはdelegateを弱く持つので、ここで持つ。
    _watcher: Retained<WindowWatcher>,
}

/// ウィジェットのネイティブのオブジェクト。
enum Control {
    Grid(Retained<FlippedView>),
    Label(Retained<NSTextField>),
    Button(Retained<NSButton>),
    TextField(Retained<NSTextField>),
    CheckBox(Retained<NSButton>),
    Choice(Retained<NSPopUpButton>),
    ProgressBar(Retained<NSProgressIndicator>),
}

impl Control {
    fn view(&self) -> &NSView {
        match self {
            Control::Grid(v) => v,
            Control::Label(v) | Control::TextField(v) => v,
            Control::Button(v) | Control::CheckBox(v) => v,
            Control::Choice(v) => v,
            Control::ProgressBar(v) => v,
        }
    }
}

struct Held {
    control: Control,
    /// コントロールはtargetとdelegateを弱く持つので、ここで持つ。
    _target: Option<Retained<Target>>,
}

struct AppKitWidgets {
    mtm: MainThreadMarker,
    app: Retained<NSApplication>,
    queue: EventQueue,
    windows: HashMap<Id, AppKitWindow>,
    controls: HashMap<Id, Held>,
    /// ラベルの一番長い語の幅を測るための、画面に出さないラベル。
    measurer: Retained<NSTextField>,
    /// アプリケーションはdelegateを弱く持つので、ここで持つ。
    _delegate: Retained<AppDelegate>,
}

impl AppKitWidgets {
    fn parent_view(&self, parent: Id) -> Result<&NSView, GuiError> {
        if let Some(window) = self.windows.get(&parent) {
            return Ok(&window.content);
        }
        match self.controls.get(&parent).map(|held| &held.control) {
            Some(Control::Grid(view)) => Ok(view),
            Some(_) => Err(GuiError::WrongKind),
            None => Err(GuiError::UnknownId),
        }
    }

    /// 折り返すラベルの大きさ。幅の上限がなければ1行の大きさを、min-contentの問い合わせ (`AtMost(0.0)`) なら一番長い
    /// 語の幅で折り返した大きさを返す。
    fn label_size(&self, label: &NSTextField, width: Measure) -> NSSize {
        let limit = match width {
            Measure::Exact(v) => f64::from(v),
            Measure::AtMost(v) if v > 0.0 => f64::from(v),
            Measure::AtMost(_) => self.widest_word(&label.stringValue().to_string()),
            Measure::Unbounded => 0.0,
        };
        label.setPreferredMaxLayoutWidth(limit);
        label.fittingSize()
    }

    fn widest_word(&self, text: &str) -> f64 {
        let widest = text
            .split_whitespace()
            .map(|word| {
                self.measurer.setStringValue(&NSString::from_str(word));
                self.measurer.fittingSize().width
            })
            .fold(0.0, f64::max);
        // 0は「上限なし」になるので、空のラベルも1以上で折り返す
        widest.max(1.0)
    }
}

fn ns_string(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

fn state(on: bool) -> isize {
    if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    }
}

impl Widgets for AppKitWidgets {
    fn create_window(&mut self, id: Id, title: &str) -> Result<(), GuiError> {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        // SAFETY: 閉じても解放させない設定をすぐ下で行い、ウィンドウは`Retained`で持つ
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(self.mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: ウィンドウコントローラーなしで作ったウィンドウは、これがないと閉じたときに解放されすぎる
        unsafe { window.setReleasedWhenClosed(false) };
        // 既定の閉じるアニメーションでは、runが続く間 (`Gui.run`の後) も閉じたウィンドウが画面に残った
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        window.setTitle(&ns_string(title));
        let content = FlippedView::new(self.mtm);
        window.setContentView(Some(&content));
        let watcher = WindowWatcher::new(self.mtm, id, self.queue.clone());
        window.setDelegate(Some(ProtocolObject::from_ref(&*watcher)));
        self.windows.insert(
            id,
            AppKitWindow {
                window,
                content,
                _watcher: watcher,
            },
        );
        Ok(())
    }

    fn create(&mut self, id: Id, kind: Kind, parent: Id, init: &Init) -> Result<(), GuiError> {
        let mtm = self.mtm;
        let target = matches!(
            kind,
            Kind::Button | Kind::TextField | Kind::CheckBox | Kind::Choice
        )
        .then(|| Target::new(mtm, id, self.queue.clone()));
        let control = match (kind, init, &target) {
            (Kind::Grid, Init::Grid, _) => Control::Grid(FlippedView::new(mtm)),
            (Kind::Label, Init::Label(text), _) => {
                let label = NSTextField::wrappingLabelWithString(&ns_string(text), mtm);
                label.setSelectable(false);
                Control::Label(label)
            }
            (Kind::Button, Init::Button(text), Some(target)) => {
                // SAFETY: `click:`は`Target`のアクションで、ボタンが弱く持つ`Target`は`Control`が持つ
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &ns_string(text),
                        Some(target),
                        Some(sel!(click:)),
                        mtm,
                    )
                };
                Control::Button(button)
            }
            (Kind::TextField, Init::TextField(text), Some(target)) => {
                let field = NSTextField::textFieldWithString(&ns_string(text), mtm);
                // SAFETY: 欄が弱く持つ`Target`は`Control`が持ち、欄より先に落ちない
                unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&**target))) };
                Control::TextField(field)
            }
            (Kind::CheckBox, Init::CheckBox(text, checked), Some(target)) => {
                // SAFETY: `toggle:`は`Target`のアクションで、`Target`は`Control`が持つ
                let check = unsafe {
                    NSButton::checkboxWithTitle_target_action(
                        &ns_string(text),
                        Some(target),
                        Some(sel!(toggle:)),
                        mtm,
                    )
                };
                check.setState(state(*checked));
                Control::CheckBox(check)
            }
            (Kind::Choice, Init::Choice, Some(target)) => {
                let choice = NSPopUpButton::initWithFrame_pullsDown(
                    NSPopUpButton::alloc(mtm),
                    NSRect::ZERO,
                    false,
                );
                // SAFETY: `select:`は`Target`のアクションで、`Target`は`Control`が持つ
                unsafe {
                    choice.setTarget(Some(target));
                    choice.setAction(Some(sel!(select:)));
                }
                Control::Choice(choice)
            }
            (Kind::ProgressBar, Init::ProgressBar(value), _) => {
                let bar = NSProgressIndicator::new(mtm);
                bar.setStyle(NSProgressIndicatorStyle::Bar);
                bar.setIndeterminate(false);
                bar.setMinValue(0.0);
                bar.setMaxValue(100.0);
                bar.setDoubleValue(*value as f64);
                Control::ProgressBar(bar)
            }
            (Kind::Window, ..)
            | (Kind::Grid, ..)
            | (Kind::Label, ..)
            | (Kind::Button, ..)
            | (Kind::TextField, ..)
            | (Kind::CheckBox, ..)
            | (Kind::Choice, ..)
            | (Kind::ProgressBar, ..) => return Err(GuiError::WrongKind),
        };
        self.parent_view(parent)?.addSubview(control.view());
        self.controls.insert(
            id,
            Held {
                control,
                _target: target,
            },
        );
        Ok(())
    }

    fn add_item(&mut self, w: Id, item: &str) -> Result<(), GuiError> {
        let Some(Control::Choice(choice)) = self.controls.get(&w).map(|held| &held.control) else {
            return Err(GuiError::WrongKind);
        };
        // `addItemWithTitle:`は同じ名前の項目を消して末尾に足すので、番号がずれないようにメニューに直接足す
        // SAFETY: アクションを持たない項目で、選ばれたことはポップアップボタンのアクションが知らせる
        let entry = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm),
                &ns_string(item),
                None,
                &NSString::new(),
            )
        };
        if let Some(menu) = choice.menu() {
            menu.addItem(&entry);
        }
        Ok(())
    }

    fn set(&mut self, w: Id, prop: &Prop) -> Result<(), GuiError> {
        if let Some(window) = self.windows.get(&w) {
            return match prop {
                Prop::Text(text) => {
                    window.window.setTitle(&ns_string(text));
                    Ok(())
                }
                Prop::Value(_) | Prop::Checked(_) | Prop::Enabled(_) => Err(GuiError::WrongKind),
            };
        }
        let held = self.controls.get(&w).ok_or(GuiError::UnknownId)?;
        match (&held.control, prop) {
            (Control::Label(field) | Control::TextField(field), Prop::Text(text)) => {
                field.setStringValue(&ns_string(text));
            }
            (Control::Button(button) | Control::CheckBox(button), Prop::Text(text)) => {
                button.setTitle(&ns_string(text));
            }
            (Control::ProgressBar(bar), Prop::Value(v)) => bar.setDoubleValue(*v as f64),
            (Control::Choice(choice), Prop::Value(v)) => choice.selectItemAtIndex(*v as isize),
            (Control::CheckBox(check), Prop::Checked(on)) => check.setState(state(*on)),
            (Control::Button(button) | Control::CheckBox(button), Prop::Enabled(on)) => {
                button.setEnabled(*on);
            }
            (Control::TextField(field), Prop::Enabled(on)) => field.setEnabled(*on),
            (Control::Choice(choice), Prop::Enabled(on)) => choice.setEnabled(*on),
            // NSProgressIndicatorは入力を受けないコントロールで、使えない状態を持たない
            (Control::ProgressBar(_), Prop::Enabled(_)) => {}
            (
                Control::Grid(_)
                | Control::Label(_)
                | Control::Button(_)
                | Control::TextField(_)
                | Control::CheckBox(_)
                | Control::Choice(_)
                | Control::ProgressBar(_),
                _,
            ) => return Err(GuiError::WrongKind),
        }
        Ok(())
    }

    /// `fittingSize`はフレームの大きさ (枠の飾りを含む) を返し、`place`もフレームを置くので、両者は同じ大きさを指す。
    fn measure(&mut self, w: Id, width: Measure, height: Measure) -> Size {
        let Some(held) = self.controls.get(&w) else {
            return Size::default();
        };
        let natural = match &held.control {
            Control::Label(label) => self.label_size(label, width),
            Control::TextField(field) => {
                let fit = field.fittingSize();
                NSSize::new(fit.width.max(FIELD_WIDTH), fit.height)
            }
            Control::ProgressBar(bar) => {
                let fit = bar.fittingSize();
                NSSize::new(fit.width.max(FIELD_WIDTH), fit.height)
            }
            Control::Grid(view) => view.fittingSize(),
            Control::Button(button) | Control::CheckBox(button) => button.fittingSize(),
            Control::Choice(choice) => choice.fittingSize(),
        };
        let exact = |m: Measure, v: f64| match m {
            Measure::Exact(e) => e,
            Measure::AtMost(_) | Measure::Unbounded => v as f32,
        };
        Size {
            w: exact(width, natural.width),
            h: exact(height, natural.height),
        }
    }

    fn place(&mut self, w: Id, rect: Rect) {
        if let Some(held) = self.controls.get(&w) {
            let frame = NSRect::new(
                NSPoint::new(f64::from(rect.x), f64::from(rect.y)),
                NSSize::new(f64::from(rect.w), f64::from(rect.h)),
            );
            held.control.view().setFrame(frame);
        }
    }

    fn show_window(&mut self, w: Id, size: Size, min: Size) {
        let Some(window) = self.windows.get(&w) else {
            return;
        };
        let window = &window.window;
        window.setContentMinSize(NSSize::new(f64::from(min.w), f64::from(min.h)));
        window.setContentSize(NSSize::new(f64::from(size.w), f64::from(size.h)));
        window.center();
        window.makeKeyAndOrderFront(None);
    }

    fn destroy(&mut self, w: Id) {
        if w.is_window() {
            self.controls
                .retain(|id, _| id.window_no() != w.window_no());
            if let Some(window) = self.windows.remove(&w) {
                window.window.setDelegate(None);
                window.window.close();
            }
        } else if let Some(held) = self.controls.remove(&w) {
            held.control.view().removeFromSuperview();
        }
    }

    fn set_presence(&mut self, active: bool) {
        if active {
            self.app
                .setActivationPolicy(NSApplicationActivationPolicy::Regular);
            #[allow(deprecated, reason = "`activate` needs macOS 14")]
            self.app.activateIgnoringOtherApps(true);
        } else {
            self.app
                .setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;

    use super::*;

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {
        static NSApp: *const c_void;
    }

    /// テストはメインのスレッドで走らないので、AppKitのbackendは作れない。そのときNSApplicationにも触れない。
    #[test]
    fn off_the_main_thread_the_backend_is_not_available_and_touches_nothing() {
        assert!(MainThreadMarker::new().is_none());
        assert_eq!(
            create().err(),
            Some(GuiError::NotAvailable(
                "cannot open the GUI: AppKit needs the main thread".to_string()
            ))
        );
        // SAFETY: AppKitの大域変数を読むだけで、NSApplicationを作るまではnilである
        assert!(unsafe { NSApp }.is_null());
    }
}

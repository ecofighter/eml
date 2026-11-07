//! Windows 11のWinUI 3のbackend。Windows App SDK 2のruntimeはemlが配らず、使う時に`AddPackageDependency`で
//! 読み込む。WinUIのオブジェクトは`Application::Start`の中でしか作れないので、ウィジェットはどれもループの中で作る
//! (docs/implementation/architecture.mdの「`eml_gui`の内部」)。

mod event_loop;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::OnceLock;

use windows::Graphics::SizeInt32;
use windows::Win32::System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize};
use windows_core::{EventRevoker, HSTRING, IInspectable, Interface};
use windows_reference::IReference;
use winui3::Microsoft::UI::Windowing::{AppWindow, OverlappedPresenter};
use winui3::Microsoft::UI::Xaml::Controls::{
    Button, Canvas, CheckBox, ComboBox, ComboBoxItem, Control, ProgressBar, TextBlock, TextBox,
};
use winui3::Microsoft::UI::Xaml::{
    FrameworkElement, IWindowNative, TextWrapping, UIElement, Window,
};
use winui3::{PackageDependency, WindowsAppSDKVersion};

use crate::{
    Event, EventQueue, GuiError, Id, Init, Kind, Measure, Native, Prop, RawEvent, Rect, Size,
    Widgets,
};

const NO_RUNTIME: &str = "cannot open the GUI: the Windows App SDK 2 runtime is not installed (https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads)";

/// WinUIは編集する欄と進み具合の棒に、中身によらない自然な幅を与えない。この幅を自然な幅とする。
const FIELD_WIDTH: f32 = 160.0;

windows_core::link!("user32.dll" "system" fn GetDpiForWindow(hwnd: *mut core::ffi::c_void) -> u32);

pub(crate) fn create() -> Result<Native, GuiError> {
    // WinUIが求めるのはシングルスレッドのアパートメントだけだが、emlはGUIをメインのスレッドでだけ開く。`eml run`では
    // Machineのスレッドがプロセスのメインのスレッドで、Rustのruntimeはそれを`main`と名づける。ほかのスレッド (テスト) では
    // runtimeを読み込まず、ウィンドウも開かない
    if std::thread::current().name() != Some("main") {
        return Err(GuiError::NotAvailable(
            "cannot open the GUI: the GUI opens only on the main thread".to_string(),
        ));
    }
    // Windows 11より古いと、bootstrapはruntimeのDLLを名前だけで探し、作業のディレクトリや`PATH`に置かれた同じ名前の
    // DLLを読み込みうる。bootstrapの前に断る
    if !is_windows_11(os_build()) {
        return Err(GuiError::NotAvailable(
            "cannot open the GUI: WinUI needs Windows 11 or later".to_string(),
        ));
    }
    bootstrap()?;
    // SAFETY: このスレッドのWindows Runtimeをシングルスレッドのアパートメントで始めるだけである
    unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }
        .map_err(|e| GuiError::Backend(format!("cannot open the GUI: {e}")))?;
    let queue = EventQueue::new();
    let shared = Rc::new(event_loop::Shared::new(queue.clone()));
    SHARED.with(|s| *s.borrow_mut() = Rc::downgrade(&shared));
    let widgets = WinUiWidgets {
        shared: Rc::clone(&shared),
        parts: HashMap::new(),
        measurer: None,
    };
    Ok(Native {
        widgets: Rc::new(RefCell::new(widgets)),
        event_loop: Box::new(event_loop::WinUiLoop::new(shared)),
        queue,
    })
}

/// Windows 11の最初のビルドは22000である。
fn is_windows_11(build: u32) -> bool {
    build >= 22000
}

#[repr(C)]
struct OsVersionInfo {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    service_pack: [u16; 128],
}

windows_core::link!("ntdll.dll" "system" fn RtlGetVersion(info: *mut OsVersionInfo) -> i32);

/// OSのビルドの番号。`GetVersionEx`はmanifestによって古い版を返すので、`RtlGetVersion`で読む。読めなければ0である。
fn os_build() -> u32 {
    let mut info = OsVersionInfo {
        size: size_of::<OsVersionInfo>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    // SAFETY: `info`は`OSVERSIONINFOW`と同じ形で、`size`にその大きさを入れてある
    let status = unsafe { RtlGetVersion(&mut info) };
    if status == 0 { info.build } else { 0 }
}

/// Windows App SDK 2のruntimeのパッケージをプロセスに加える。結果はプロセスで1度だけ求める。加えた依存はプロセスの終わり
/// まで外さない。外すと、読み込んだWinUIのDLLを使えなくなるからである。
pub(crate) fn bootstrap() -> Result<(), GuiError> {
    static DONE: OnceLock<Result<(), GuiError>> = OnceLock::new();
    DONE.get_or_init(
        || match PackageDependency::initialize_version(WindowsAppSDKVersion::V2) {
            Ok(dependency) => {
                std::mem::forget(dependency);
                Ok(())
            }
            Err(_) => Err(GuiError::NotAvailable(NO_RUNTIME.to_string())),
        },
    )
    .clone()
}

fn winui_error(e: windows_core::Error) -> GuiError {
    GuiError::Backend(format!("gui: WinUI failed: {e}"))
}

thread_local! {
    /// WinUIのcallbackが積む先。callbackは`Send`でなければならず、`Rc`の列を持てないので、メインのスレッドのここから引く。
    static SHARED: RefCell<Weak<event_loop::Shared>> = const { RefCell::new(Weak::new()) };
}

/// callbackから`Shared`を使う。backendがもうなければ何もしない。
fn with_shared(f: impl FnOnce(&event_loop::Shared)) {
    if let Some(shared) = SHARED.with(|s| s.borrow().upgrade()) {
        f(&shared);
    }
}

fn push(event: RawEvent) {
    with_shared(|s| s.push(event));
}

/// ウィジェットの値として最後に知らせたか置いたもの。WinUIは、プログラムが値を変えても`TextBox`の`TextChanged`と
/// `ComboBox`の`SelectionChanged`を出し、`TextChanged`は後から出す。そのcallbackは出した時の値でなく今の値を読むので、
/// 1回の変更に何回出るかは決まらない。
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Text(String),
    Index(i32),
}

/// 今の値が最後に知らせたか置いた値と違うときだけ、その値を覚えてイベントを積む。`set`の後の知らせも、同じ変更を2度
/// 知らせるイベントも、これで捨てる。ユーザーの変更は値を今の値と違うものにするので、失われない。
fn push_if_changed(id: Id, now: Value, event: Event) {
    with_shared(|s| {
        if s.known.borrow_mut().insert(id, now.clone()).as_ref() != Some(&now) {
            s.push(RawEvent::User(event));
        }
    });
}

enum Part {
    Window { window: Window, root: Canvas },
    Grid(Canvas),
    Label(TextBlock),
    Button(Button),
    TextField(TextBox),
    CheckBox(CheckBox),
    Choice(ComboBox),
    ProgressBar(ProgressBar),
}

impl Part {
    fn element(&self) -> windows_core::Result<FrameworkElement> {
        match self {
            Part::Window { root, .. } | Part::Grid(root) => root.cast(),
            Part::Label(label) => label.cast(),
            Part::Button(button) => button.cast(),
            Part::TextField(field) => field.cast(),
            Part::CheckBox(check) => check.cast(),
            Part::Choice(choice) => choice.cast(),
            Part::ProgressBar(bar) => bar.cast(),
        }
    }

    fn container(&self) -> Option<&Canvas> {
        match self {
            Part::Window { root, .. } | Part::Grid(root) => Some(root),
            Part::Label(_)
            | Part::Button(..)
            | Part::TextField(..)
            | Part::CheckBox(..)
            | Part::Choice(..)
            | Part::ProgressBar(_) => None,
        }
    }
}

struct Held {
    part: Part,
    /// 子を外す時の親。ウィンドウにはない。
    parent: Option<Canvas>,
    /// callbackの登録。落とすと外す呼び出しをするので、ループが終わった後は`release`が忘れる。
    events: Vec<EventRevoker>,
}

/// 手放した`Held`を片付ける。ループが終わった後は、XAMLの核はもう止まっているので、登録を外す呼び出しもせず、参照を
/// 手放すだけにする。
fn release(held: Held, exited: bool) {
    if exited {
        for registration in held.events {
            registration.forget();
        }
    }
}

struct WinUiWidgets {
    shared: Rc<event_loop::Shared>,
    parts: HashMap<Id, Held>,
    /// min-contentの問い合わせで、ラベルの語を1つずつ測る、画面に出さない`TextBlock`。ループの中で初めて要る時に作る。
    measurer: Option<TextBlock>,
}

impl Drop for WinUiWidgets {
    fn drop(&mut self) {
        let exited = self.shared.exited.get();
        for (_, held) in self.parts.drain() {
            release(held, exited);
        }
    }
}

/// `ContentControl`の中身にする文字列。
fn content(text: &str) -> IInspectable {
    IReference::<HSTRING>::from(text).into()
}

impl WinUiWidgets {
    fn new_part(&mut self, id: Id, init: &Init) -> windows_core::Result<(Part, Vec<EventRevoker>)> {
        Ok(match init {
            Init::Grid => (Part::Grid(Canvas::new()?), vec![]),
            Init::Label(text) => {
                let label = TextBlock::new()?;
                // 語の途中では折り返さない。一番長い語の幅がmin-contentの幅になる
                label.SetTextWrapping(TextWrapping::WrapWholeWords)?;
                label.SetText(&HSTRING::from(text.as_str()))?;
                (Part::Label(label), vec![])
            }
            Init::Button(text) => {
                let button = Button::new()?;
                button.SetContent(&content(text))?;
                let click = button.Click(move |_, _| {
                    push(RawEvent::User(Event::Click(id)));
                    Ok(())
                })?;
                (Part::Button(button), vec![click])
            }
            Init::TextField(text) => {
                let field = TextBox::new()?;
                let changed = field.TextChanged(move |sender, _| {
                    if let Ok(text) = sender.ok().and_then(|s| s.cast::<TextBox>()?.Text()) {
                        let text = text.to_string_lossy();
                        push_if_changed(id, Value::Text(text.clone()), Event::Text(id, text));
                    }
                    Ok(())
                })?;
                self.remember(id, Value::Text(text.clone()));
                field.SetText(&HSTRING::from(text.as_str()))?;
                (Part::TextField(field), vec![changed])
            }
            Init::CheckBox(text, checked) => {
                let check = CheckBox::new()?;
                check.SetContent(&content(text))?;
                check.SetIsChecked(Some(*checked))?;
                // `Click`はユーザーの操作でだけ出て、`SetIsChecked`では出ない
                let click = check.Click(move |sender, _| {
                    let on = sender
                        .ok()
                        .and_then(|s| s.cast::<CheckBox>()?.IsChecked())
                        .unwrap_or(false);
                    push(RawEvent::User(Event::Toggle(id, on)));
                    Ok(())
                })?;
                (Part::CheckBox(check), vec![click])
            }
            Init::Choice => {
                let choice = ComboBox::new()?;
                let changed = choice.SelectionChanged(move |sender, _| {
                    if let Ok(index) = sender
                        .ok()
                        .and_then(|s| s.cast::<ComboBox>()?.SelectedIndex())
                        && index >= 0
                    {
                        push_if_changed(
                            id,
                            Value::Index(index),
                            Event::Select(id, i64::from(index)),
                        );
                    }
                    Ok(())
                })?;
                (Part::Choice(choice), vec![changed])
            }
            Init::ProgressBar(value) => {
                let bar = ProgressBar::new()?;
                bar.SetMinimum(0.0)?;
                bar.SetMaximum(100.0)?;
                bar.SetValue(*value as f64)?;
                (Part::ProgressBar(bar), vec![])
            }
        })
    }

    /// プログラムが値を置く前に呼ぶ。
    fn remember(&self, id: Id, value: Value) {
        self.shared.known.borrow_mut().insert(id, value);
    }

    fn set_part(
        &self,
        part: &Part,
        w: Id,
        prop: &Prop,
    ) -> Result<windows_core::Result<()>, GuiError> {
        Ok(match (part, prop) {
            (Part::Window { window, .. }, Prop::Text(text)) => {
                window.SetTitle(&HSTRING::from(text.as_str()))
            }
            (Part::Label(label), Prop::Text(text)) => label.SetText(&HSTRING::from(text.as_str())),
            (Part::Button(button), Prop::Text(text)) => button.SetContent(&content(text)),
            (Part::CheckBox(check), Prop::Text(text)) => check.SetContent(&content(text)),
            (Part::TextField(field), Prop::Text(text)) => {
                self.remember(w, Value::Text(text.clone()));
                field.SetText(&HSTRING::from(text.as_str()))
            }
            (Part::ProgressBar(bar), Prop::Value(v)) => bar.SetValue(*v as f64),
            (Part::Choice(choice), Prop::Value(v)) => {
                let index = i32::try_from(*v).map_err(|_| GuiError::OutOfRange)?;
                self.remember(w, Value::Index(index));
                choice.SetSelectedIndex(index)
            }
            (Part::CheckBox(check), Prop::Checked(on)) => check.SetIsChecked(Some(*on)),
            (
                Part::Button(..)
                | Part::TextField(..)
                | Part::CheckBox(..)
                | Part::Choice(..)
                | Part::ProgressBar(_),
                Prop::Enabled(on),
            ) => part
                .element()
                .and_then(|e| e.cast::<Control>()?.SetIsEnabled(*on)),
            (
                Part::Window { .. }
                | Part::Grid(_)
                | Part::Label(_)
                | Part::Button(..)
                | Part::TextField(..)
                | Part::CheckBox(..)
                | Part::Choice(..)
                | Part::ProgressBar(_),
                _,
            ) => return Err(GuiError::WrongKind),
        })
    }

    /// 一番長い語の幅。画面に出さない`TextBlock`で1語ずつ測る。
    fn longest_word(&mut self, text: &str) -> windows_core::Result<f32> {
        let measurer = match &self.measurer {
            Some(m) => m.clone(),
            None => {
                let m = TextBlock::new()?;
                self.measurer = Some(m.clone());
                m
            }
        };
        let mut widest = 0.0f32;
        for word in text.split_whitespace() {
            measurer.SetText(&HSTRING::from(word))?;
            widest = widest.max(desired(&measurer.cast()?, f32::INFINITY)?.w);
        }
        Ok(widest)
    }

    fn natural(&mut self, w: Id, width: Measure) -> windows_core::Result<Size> {
        let Some(held) = self.parts.get(&w) else {
            return Ok(Size::default());
        };
        let element = held.part.element()?;
        Ok(match &held.part {
            Part::Label(label) => {
                let laid = match width {
                    Measure::Exact(v) => v,
                    Measure::AtMost(v) if v > 0.0 => desired(&element, v)?.w,
                    Measure::AtMost(_) => {
                        let text = label.Text()?.to_string_lossy();
                        self.longest_word(&text)?
                    }
                    Measure::Unbounded => desired(&element, f32::INFINITY)?.w,
                };
                Size {
                    w: laid,
                    h: desired(&element, laid)?.h,
                }
            }
            Part::TextField(..) | Part::ProgressBar(_) => {
                let fit = desired(&element, f32::INFINITY)?;
                Size {
                    w: fit.w.max(FIELD_WIDTH),
                    h: fit.h,
                }
            }
            Part::Window { .. }
            | Part::Grid(_)
            | Part::Button(..)
            | Part::CheckBox(..)
            | Part::Choice(..) => desired(&element, f32::INFINITY)?,
        })
    }

    fn show(&self, window: &Window, size: Size, min: Size) -> windows_core::Result<()> {
        let app_window: AppWindow = window.AppWindow()?;
        // AppWindowの大きさは物理的なpxなので、ここでだけ論理pxに倍率を掛ける
        // SAFETY: `IWindowNative`はWinUIの`Window`が持つ相互運用のインターフェースである
        let hwnd = unsafe { window.cast::<IWindowNative>()?.WindowHandle()? };
        // SAFETY: 生きているウィンドウのハンドルを渡す
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let scale = if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 };
        let px = |v: f32| (f64::from(v) * scale).ceil() as i32;
        app_window.ResizeClient(SizeInt32 {
            Width: px(size.w),
            Height: px(size.h),
        })?;
        // 最小の大きさは枠を含むウィンドウ全体の大きさで置く
        let (outer, client) = (app_window.Size()?, app_window.ClientSize()?);
        let presenter = app_window.Presenter()?.cast::<OverlappedPresenter>()?;
        presenter.SetPreferredMinimumWidth(Some(px(min.w) + outer.Width - client.Width))?;
        presenter.SetPreferredMinimumHeight(Some(px(min.h) + outer.Height - client.Height))?;
        window.Activate()
    }
}

/// 要素が与えた幅の下で求める大きさ。`place`が置いた幅と高さは測る間だけ外す。外さないと、それがそのまま求める大きさに
/// なるからである。
fn desired(element: &FrameworkElement, width: f32) -> windows_core::Result<Size> {
    let (w, h) = (element.Width()?, element.Height()?);
    element.SetWidth(f64::NAN)?;
    element.SetHeight(f64::NAN)?;
    let measured = element.cast::<UIElement>().and_then(|e| {
        e.Measure(windows::Foundation::Size {
            Width: width,
            Height: f32::INFINITY,
        })?;
        e.DesiredSize()
    });
    element.SetWidth(w)?;
    element.SetHeight(h)?;
    let measured = measured?;
    Ok(Size {
        w: measured.Width,
        h: measured.Height,
    })
}

impl Widgets for WinUiWidgets {
    fn create_window(&mut self, id: Id, title: &str) -> Result<(), GuiError> {
        let window = Window::new().map_err(winui_error)?;
        let part = (|| {
            window.SetTitle(&HSTRING::from(title))?;
            let root = Canvas::new()?;
            window.SetContent(&root)?;
            // 閉じるのはプログラムだけにする
            let closing = window.AppWindow()?.Closing(move |_, args| {
                if let Ok(args) = args.ok() {
                    let _ = args.SetCancel(true);
                }
                push(RawEvent::User(Event::CloseRequest(id)));
                Ok(())
            })?;
            let resized = window.SizeChanged(move |_, args| {
                if let Ok(size) = args.ok().and_then(|a| a.Size()) {
                    push(RawEvent::Resized(
                        id,
                        Size {
                            w: size.Width,
                            h: size.Height,
                        },
                    ));
                }
                Ok(())
            })?;
            let theme = root.ActualThemeChanged(move |_, _| {
                push(RawEvent::Invalidate(id));
                Ok(())
            })?;
            Ok((
                Part::Window {
                    window: window.clone(),
                    root,
                },
                vec![closing, resized, theme],
            ))
        })()
        .map_err(winui_error)?;
        let (part, events) = part;
        self.parts.insert(
            id,
            Held {
                part,
                parent: None,
                events,
            },
        );
        Ok(())
    }

    fn create(&mut self, id: Id, kind: Kind, parent: Id, init: &Init) -> Result<(), GuiError> {
        let canvas = match self.parts.get(&parent) {
            Some(held) => held.part.container().ok_or(GuiError::WrongKind)?.clone(),
            None => return Err(GuiError::UnknownId),
        };
        if kind == Kind::Window || init.kind() != kind {
            return Err(GuiError::WrongKind);
        }
        let (part, events) = self.new_part(id, init).map_err(winui_error)?;
        part.element()
            .and_then(|e| canvas.Children()?.Append(&e))
            .map_err(winui_error)?;
        self.parts.insert(
            id,
            Held {
                part,
                parent: Some(canvas),
                events,
            },
        );
        Ok(())
    }

    fn add_item(&mut self, w: Id, item: &str) -> Result<(), GuiError> {
        match self.parts.get(&w).map(|h| &h.part) {
            Some(Part::Choice(choice)) => (|| {
                let entry = ComboBoxItem::new()?;
                entry.SetContent(&content(item))?;
                choice.Items()?.Append(&entry.cast::<IInspectable>()?)
            })()
            .map_err(winui_error),
            Some(_) => Err(GuiError::WrongKind),
            None => Err(GuiError::UnknownId),
        }
    }

    fn set(&mut self, w: Id, prop: &Prop) -> Result<(), GuiError> {
        let held = self.parts.get(&w).ok_or(GuiError::UnknownId)?;
        let applied = self.set_part(&held.part, w, prop)?;
        applied.map_err(winui_error)
    }

    /// 折り返すラベルは並べる幅を決めてから、その幅での高さを測る。ほかのウィジェットは制約によらない自然な大きさで、
    /// `Exact`の軸は与えた値を返す。
    fn measure(&mut self, w: Id, width: Measure, height: Measure) -> Size {
        let natural = self.natural(w, width).unwrap_or_default();
        let exact = |m: Measure, v: f32| match m {
            Measure::Exact(e) => e,
            Measure::AtMost(_) | Measure::Unbounded => v,
        };
        Size {
            w: exact(width, natural.w),
            h: exact(height, natural.h),
        }
    }

    fn place(&mut self, w: Id, rect: Rect) {
        if let Some(held) = self.parts.get(&w) {
            let _ = held.part.element().and_then(|e| {
                Canvas::SetLeft(&e, f64::from(rect.x))?;
                Canvas::SetTop(&e, f64::from(rect.y))?;
                e.SetWidth(f64::from(rect.w))?;
                e.SetHeight(f64::from(rect.h))
            });
        }
    }

    fn show_window(&mut self, w: Id, size: Size, min: Size) {
        if let Some(Held {
            part: Part::Window { window, .. },
            ..
        }) = self.parts.get(&w)
        {
            let _ = self.show(window, size, min);
        }
    }

    fn destroy(&mut self, w: Id) {
        let Some(held) = self.parts.remove(&w) else {
            return;
        };
        let exited = self.shared.exited.get();
        if w.is_window() {
            let children: Vec<Id> = self
                .parts
                .keys()
                .filter(|id| id.window_no() == w.window_no())
                .copied()
                .collect();
            for id in children {
                if let Some(child) = self.parts.remove(&id) {
                    release(child, exited);
                }
            }
        }
        let parts = &self.parts;
        self.shared
            .known
            .borrow_mut()
            .retain(|id, _| parts.contains_key(id));
        if exited {
            release(held, exited);
            return;
        }
        match (&held.part, &held.parent) {
            (Part::Window { window, .. }, _) => {
                let window = window.clone();
                // 閉じる求めの見張りを先に外すので、`Close`は`CloseRequest`にならない
                drop(held);
                let _ = window.Close();
            }
            (part, Some(parent)) => {
                let _ = part.element().and_then(|e| {
                    let children = parent.Children()?;
                    let mut index = 0;
                    if children.IndexOf(&e, &mut index)? {
                        children.RemoveAt(index)?;
                    }
                    Ok(())
                });
            }
            (_, None) => {}
        }
    }

    /// Windowsには、Dockのような、アプリケーションの出入りの表示がない。
    fn set_presence(&mut self, _active: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テストはメインのスレッドで走らないので、WinUIのbackendは作れない。そのときruntimeも読み込まない。
    #[test]
    fn off_the_main_thread_the_backend_is_not_available() {
        assert_ne!(std::thread::current().name(), Some("main"));
        assert_eq!(
            create().err(),
            Some(GuiError::NotAvailable(
                "cannot open the GUI: the GUI opens only on the main thread".to_string()
            ))
        );
    }

    #[test]
    fn winui_needs_windows_11() {
        assert!(!is_windows_11(0));
        assert!(!is_windows_11(19045));
        assert!(is_windows_11(22000));
        assert!(is_windows_11(26100));
        assert!(os_build() > 0, "RtlGetVersion reads the build number");
    }

    /// runtimeがないWindowsでは、bootstrapの誤りはMicrosoftの配布のページを示す。runtimeのあるWindowsでは確かめられない。
    #[test]
    fn bootstrap_names_the_download_page_when_the_runtime_is_missing() {
        match bootstrap() {
            Ok(()) => println!("skipped: the Windows App SDK 2 runtime is installed"),
            Err(GuiError::NotAvailable(m)) => assert!(
                m.contains("https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads"),
                "{m}"
            ),
            Err(e) => panic!("unexpected bootstrap error: {e:?}"),
        }
    }
}

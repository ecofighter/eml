//! ネイティブのGUIのtoolkitの共通の層と、テスト用のstubのbackend (docs/implementation/architecture.md の「`eml_gui`の内部」)。

mod backend;
mod layout;
mod queue;
pub mod stub;
mod toolkit;

use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

pub use backend::{Backend, Native};
pub use queue::EventQueue;
pub use stub::Script;
pub use toolkit::Toolkit;

/// ウィジェットの番号。上位32ビットはウィンドウの番号 (`Toolkit`ごとに1から)、下位32ビットはウィンドウの中の連番 (0はウィンドウ自身) で、
/// どちらも再利用しない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub u64);

impl Id {
    pub fn new(window_no: u32, seq: u32) -> Id {
        Id((u64::from(window_no) << 32) | u64::from(seq))
    }

    pub fn window_no(self) -> u32 {
        (self.0 >> 32) as u32
    }

    pub fn seq(self) -> u32 {
        self.0 as u32
    }

    pub fn window(self) -> Id {
        Id::new(self.window_no(), 0)
    }

    pub fn is_window(self) -> bool {
        self.seq() == 0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}:{}", self.window_no(), self.seq())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Window,
    Grid,
    Label,
    Button,
    TextField,
    CheckBox,
    Choice,
    ProgressBar,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prop {
    Text(String),
    Value(i64),
    Checked(bool),
    Enabled(bool),
}

/// ウィジェットを作るときに使うもの。`Grid`に中身はないが、`Widgets::create`がどの種類も1つの形で受け取れるように変種を置く。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Init {
    Grid,
    Label(String),
    Button(String),
    TextField(String),
    CheckBox(String, bool),
    Choice,
    ProgressBar(i64),
}

impl Init {
    pub fn kind(&self) -> Kind {
        match self {
            Init::Grid => Kind::Grid,
            Init::Label(_) => Kind::Label,
            Init::Button(_) => Kind::Button,
            Init::TextField(_) => Kind::TextField,
            Init::CheckBox(..) => Kind::CheckBox,
            Init::Choice => Kind::Choice,
            Init::ProgressBar(_) => Kind::ProgressBar,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Click(Id),
    Text(Id, String),
    Toggle(Id, bool),
    Select(Id, i64),
    CloseRequest(Id),
}

impl Event {
    pub fn id(&self) -> Id {
        match *self {
            Event::Click(w)
            | Event::Text(w, _)
            | Event::Toggle(w, _)
            | Event::Select(w, _)
            | Event::CloseRequest(w) => w,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RawEvent {
    User(Event),
    Resized(Id, Size),
    Invalidate(Id),
    QuitRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Auto,
    Fr(u16),
    Px(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub col_span: u16,
    pub row_span: u16,
    pub halign: Align,
    pub valign: Align,
}

impl Default for Cell {
    fn default() -> Cell {
        Cell {
            col_span: 1,
            row_span: 1,
            halign: Align::Stretch,
            valign: Align::Stretch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuiError {
    NoContainer,
    NothingOpen,
    UnknownId,
    WrongKind,
    OutOfRange,
    NotAvailable(String),
    Backend(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Woken,
    Events,
    Io,
    Timeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Continue(Option<Duration>),
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Measure {
    Exact(f32),
    /// 最大でこの論理px。どちらの軸でも`AtMost(0.0)`は、その軸に沿った最小の自然な大きさを求める (Taffyのmin-contentの問い合わせ)。
    /// 0という厳密な上限になることはない。
    AtMost(f32),
    Unbounded,
}

/// 論理px。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

/// 親を基準にした論理pxで、原点は左上にある。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// ネイティブのループが待つOSの源。`eml_gui`はworkspaceのcrateに依存しないので、VMの`PollSource`と同じ形の型を自分で持つ
/// (docs/implementation/architecture.md の「crate の依存関係」)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoSource {
    #[cfg(unix)]
    Fd(std::os::fd::RawFd),
}

/// ウィジェットを作って変えるbackendの側。`add_item`と`set`はイベントを出さない。
pub trait Widgets {
    fn create_window(&mut self, id: Id, title: &str) -> Result<(), GuiError>;
    fn create(&mut self, id: Id, kind: Kind, parent: Id, init: &Init) -> Result<(), GuiError>;
    fn add_item(&mut self, w: Id, item: &str) -> Result<(), GuiError>;
    fn set(&mut self, w: Id, prop: &Prop) -> Result<(), GuiError>;
    /// 与えられた制約の下でのウィジェットの大きさを論理pxで返す。`AtMost(0.0)`は、その軸に沿った最小の自然な大きさを意味し、
    /// 0という上限ではない (`Measure`を参照)。
    fn measure(&mut self, w: Id, width: Measure, height: Measure) -> Size;
    fn place(&mut self, w: Id, rect: Rect);
    fn show_window(&mut self, w: Id, size: Size, min: Size);
    fn destroy(&mut self, w: Id);
    /// AppKitでは、activation policyを`Regular`と`Prohibited`の間で切り替える。ほかのbackendは何もしない。
    fn set_presence(&mut self, active: bool);
}

pub trait EventLoop {
    /// ループの準備が整ったらすぐに`step(Woken)`を1回呼び、`step`が`Stop`を返すか、OSの誤りが起きたときにだけ戻る。
    fn run_loop(
        &mut self,
        io: Option<IoSource>,
        timeout: Option<Duration>,
        step: &mut dyn FnMut(Reason) -> Step,
    ) -> io::Result<()>;
    fn waker(&self) -> Arc<dyn Fn() + Send + Sync>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_encodes_window_and_seq() {
        let id = Id::new(3, 7);
        assert_eq!(id.0, (3 << 32) | 7);
        assert_eq!((id.window_no(), id.seq()), (3, 7));
        assert_eq!(id.window(), Id::new(3, 0));
        assert!(!id.is_window());
        assert!(Id::new(1, 0).is_window());
        assert_eq!(id.to_string(), "#3:7");
    }

    #[test]
    fn init_names_its_kind() {
        assert_eq!(Init::Grid.kind(), Kind::Grid);
        assert_eq!(Init::CheckBox("a".into(), true).kind(), Kind::CheckBox);
        assert_eq!(Init::ProgressBar(0).kind(), Kind::ProgressBar);
    }
}

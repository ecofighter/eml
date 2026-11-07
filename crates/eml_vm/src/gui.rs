//! `Std.Gui`のexternの腕と、メインのworkerの`Park`であるGUIの`Park` (docs/spec/stdlib.mdの「`Std.Gui`」、
//! docs/spec/vm.mdの「GUI の Park」)。toolkitはメインのスレッドのスロット`GUI_SLOT`にあり、腕はそこからだけ触れる。
//! ネイティブのループは1回の`Park::wait`の中で動き、VMには`Pending`とwakerを通してだけ入る。
//!
//! feature `gui`を外した処理系では、`UiToken`を値のない型にし、`want`がいつも`not available`で失敗する。`Ui`を
//! 作れないので、ほかの腕には届かない。`vm.rs`にcfgを置かないため、両方で同じ名前を使う。

use eml_extern::Extern;
use eml_rt::externs::{self, Fault, Raised};
use eml_rt::{Ctx, Mode, Word};

use crate::cells::Raise;
use crate::error::VmFault;
use crate::sched::{Policy, Scheduler};

/// GUIを開けないときの文言 (docs/spec/stdlib.mdの「`Std.Gui`」)。
const NOT_AVAILABLE: &str = "cannot open the GUI: not available";

fn not_available() -> Raise {
    Raise::Io {
        kind: externs::IO_NOT_AVAILABLE,
        message: NOT_AVAILABLE.to_string(),
    }
}

/// 腕がIOの失敗として起こす誤り。`failed`が`try_io`に届ける。
fn raised(kind: i64, message: &str) -> VmFault {
    VmFault::Extern(Fault::Raised(Box::new(Raised {
        kind,
        message: message.into(),
    })))
}

/// `Std.Gui.abandon_raw`。開く前に取り消されたタスクのゲートを放す。開きかけの印も下ろすので、次の`want_gui_raw`は
/// 開ける。
pub(crate) fn abandon<P: Policy>(sched: &mut Scheduler<P>, gate: i64) -> Result<(), VmFault> {
    let cleared = forget_opening(gate);
    sched.cells.lock().release_gate(gate)?;
    if cleared {
        lower_holds(sched);
    }
    Ok(())
}

/// GUIを求めても開いてもいなくなったので、runの外の待ちの印を下ろす。`Pending::set_holds`と同じく、下ろしたら
/// worker 0に判定し直させる (docs/spec/vm.mdの「GUI の Park」)。
#[cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "only an opened GUI raises holds")
)]
fn lower_holds<P: Policy>(sched: &mut Scheduler<P>) {
    if sched.run.swap_holds(false) {
        sched.run.wake(0);
    }
}

#[cfg(not(feature = "gui"))]
pub(crate) use off::*;
#[cfg(feature = "gui")]
pub use on::park;
#[cfg(feature = "gui")]
pub(crate) use on::*;

#[cfg(not(feature = "gui"))]
mod off {
    use super::*;

    /// GUIのない処理系の`Ui`の中身。値を持たない。
    #[allow(dead_code, reason = "the payload of a Ui that is never made")]
    pub(crate) enum UiToken {}

    /// `Std.Gui.want_gui_raw`。いつも`NotAvailable`で失敗する。
    pub(crate) fn want<P: Policy>(_: &mut Scheduler<P>) -> Result<Result<i64, Raise>, VmFault> {
        Ok(Err(not_available()))
    }

    /// `Std.Gui.open_ui_raw`。`want`が失敗するので届かないが、届けばゲートを放して失敗する。
    pub(crate) fn open_ui<M: Mode, P: Policy>(
        _: &mut Ctx<M>,
        sched: &mut Scheduler<P>,
        gate: i64,
    ) -> Result<Result<Word, Raise>, VmFault> {
        sched.cells.lock().release_gate(gate)?;
        Ok(Err(not_available()))
    }

    pub(crate) fn forget_opening(_: i64) -> bool {
        false
    }

    /// toolkitに触れる行。`Ui`を作れないので届かない。
    ///
    /// # Safety
    /// `args`は行の引数の語である。
    pub(crate) unsafe fn arm<M: Mode>(
        _: &mut Ctx<M>,
        _: Extern,
        _: &[Word],
    ) -> Result<Word, VmFault> {
        let _ = raised;
        Err(VmFault::Internal("a Std.Gui row runs without the GUI"))
    }
}

#[cfg(feature = "gui")]
mod on {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use eml_gui::{
        Backend, Event, EventLoop, EventQueue, GuiError, Id, Init, IoSource, Prop, RawEvent,
        Reason, Step, Toolkit, Track, Widgets,
    };
    use eml_rt::IoWaker;

    use super::*;
    use crate::park::{Park, ParkFactory, Pending, PollSource, Progress};

    /// `Std.Gui.Ui`の`Foreign`の中身。落とすと回収の旗を立てるだけで、ウィンドウを閉じるのは`Park`の次の段である。
    /// `Ui`がどのworkerで落ちても、触れるのはこの旗だけである。
    pub(crate) struct UiToken {
        generation: u32,
        reap: Arc<AtomicBool>,
    }

    impl Drop for UiToken {
        fn drop(&mut self) {
            self.reap.store(true, Ordering::Release);
        }
    }

    /// 開いている`Ui`。ゲートは`Ui`ごとに持ち、回収で放す。
    struct UiState {
        generation: u32,
        gate: i64,
        reap: Arc<AtomicBool>,
    }

    /// 作ったbackendのうち、`Toolkit`が使うもの。ループは`GuiPark`が持つ。
    struct Parts {
        widgets: Rc<RefCell<dyn Widgets>>,
        queue: EventQueue,
    }

    /// メインのスレッドのGUIの状態。腕と`GuiPark`が分け合う。借りるのは腕か段の1回の中だけで、`Pending`を呼ぶ間は
    /// 借りない。
    pub(crate) struct Slot {
        /// 最初に作るときに取る。
        backend: Cell<Option<Backend>>,
        parts: RefCell<Option<Parts>>,
        /// 作るのに失敗した誤り。runの間は残り、作り直さない。
        create_error: RefCell<Option<GuiError>>,
        /// まだ開いていない`want_gui_raw`のゲート。
        opening: Cell<Option<i64>>,
        /// 段が`opening`に知らせた。
        poked: Cell<bool>,
        toolkit: RefCell<Option<Toolkit>>,
        ui: RefCell<Option<UiState>>,
        /// `poll_raw`が取ったイベント。`event_int_raw`と`event_text_raw`が読む。
        event: RefCell<Option<Event>>,
        gens: Cell<u32>,
        waker: Arc<GuiWaker>,
    }

    thread_local! {
        static GUI_SLOT: RefCell<Option<Rc<Slot>>> = const { RefCell::new(None) };
    }

    fn current_slot() -> Option<Rc<Slot>> {
        GUI_SLOT.with(|slot| slot.borrow().clone())
    }

    impl Slot {
        /// 開いていて、回収の旗の立っていない`Ui`のゲート。
        fn live_gate(&self) -> Option<i64> {
            self.ui
                .borrow()
                .as_ref()
                .filter(|ui| !ui.reap.load(Ordering::Acquire))
                .map(|ui| ui.gate)
        }

        fn reap_flagged(&self) -> bool {
            self.ui
                .borrow()
                .as_ref()
                .is_some_and(|ui| ui.reap.load(Ordering::Acquire))
        }

        fn set_presence(&self, active: bool) {
            if let Some(parts) = &*self.parts.borrow() {
                parts.widgets.borrow_mut().set_presence(active);
            }
        }

        /// 回収。手放した`Ui`のウィンドウを閉じ、presenceを下ろし、ゲートを放し、開きかけがなければrunの外の待ちの印を下ろす。正常に
        /// 終わったとき、panicしたとき、取り消したときのどれもここを通る。`Park`の側で`Pending::release_gate`と
        /// `Pending::set_holds`を呼ぶのはここだけである(docs/spec/vm.mdの「GUI の Park」)。
        fn reap(&self, pending: &mut dyn Pending) {
            if !self.reap_flagged() {
                return;
            }
            let toolkit = self.toolkit.borrow_mut().take();
            if let Some(mut toolkit) = toolkit {
                toolkit.close_all();
            }
            self.set_presence(false);
            *self.event.borrow_mut() = None;
            let ui = self.ui.borrow_mut().take();
            if let Some(ui) = ui {
                pending.release_gate(ui.gate);
                // 次の`Gui.run`がもう開きかけなら、印はその期間のために立てたままにする。ここで下ろすと、ゲートに
                // 知らせるまでの間にworker 0がデッドロックと決めうる
                if self.opening.get().is_none() {
                    pending.set_holds(false);
                }
            }
        }

        /// ループが取り出した制御のイベントを扱う。`Ui`が開いていればOSの終了の操作をウィンドウごとの閉じる要求に
        /// 変え、開いていなければrunの割り込みにする。
        fn settle(&self, interrupt: &(dyn Fn() + Send + Sync)) {
            let events = match &*self.parts.borrow() {
                Some(parts) => parts.queue.take_control(),
                None => return,
            };
            for event in events {
                match event {
                    RawEvent::QuitRequested => {
                        if self.live_gate().is_some()
                            && let Some(toolkit) = &mut *self.toolkit.borrow_mut()
                        {
                            toolkit.request_close_all();
                        } else {
                            interrupt();
                        }
                    }
                    RawEvent::Resized(..) | RawEvent::Invalidate(_) => {
                        if let Some(toolkit) = &mut *self.toolkit.borrow_mut() {
                            toolkit.settle(&event);
                        }
                    }
                    RawEvent::User(_) => {}
                }
            }
        }

        fn has_events(&self) -> bool {
            self.toolkit
                .borrow()
                .as_ref()
                .is_some_and(Toolkit::has_events)
        }
    }

    /// GUIの`Park`のwaker。ループができる前の起こしは印に残し、ループのwakerを置いたときに鳴らす。
    #[derive(Default)]
    struct GuiWaker {
        rang: AtomicBool,
        inner: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    }

    impl GuiWaker {
        fn install(&self, wake: Arc<dyn Fn() + Send + Sync>) {
            *self.inner.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&wake));
            if self.rang.swap(false, Ordering::AcqRel) {
                wake();
            }
        }
    }

    impl IoWaker for GuiWaker {
        fn wake(&self) {
            let inner = self
                .inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            match inner {
                Some(wake) => wake(),
                None => self.rang.store(true, Ordering::Release),
            }
        }
    }

    /// メインのworkerの`Park`。GUIを求められるまでは`DefaultPark`と同じく`default_wait`だけで待つ。求められたら
    /// backendを作り、runに1回だけネイティブのループに入る (docs/spec/vm.mdの「GUI の Park」)。
    pub(crate) struct GuiPark {
        slot: Rc<Slot>,
        event_loop: Option<Box<dyn EventLoop>>,
        interrupt: Arc<dyn Fn() + Send + Sync>,
    }

    /// `eml run`がrunごとに置くGUIの`Park`を作るもの。`interrupt`は、`Ui`が開いていないときのOSの終了の操作で呼ぶ。
    pub fn park(backend: Backend, interrupt: Arc<dyn Fn() + Send + Sync>) -> ParkFactory {
        let backend = Arc::new(backend);
        Arc::new(move || {
            let backend = match &*backend {
                Backend::Native => Backend::Native,
                Backend::Stub(script) => Backend::Stub(script.clone()),
            };
            let slot = Rc::new(Slot {
                backend: Cell::new(Some(backend)),
                parts: RefCell::new(None),
                create_error: RefCell::new(None),
                opening: Cell::new(None),
                poked: Cell::new(false),
                toolkit: RefCell::new(None),
                ui: RefCell::new(None),
                event: RefCell::new(None),
                gens: Cell::new(0),
                waker: Arc::default(),
            });
            GUI_SLOT.with(|s| *s.borrow_mut() = Some(Rc::clone(&slot)));
            Box::new(GuiPark {
                slot,
                event_loop: None,
                interrupt: Arc::clone(&interrupt),
            })
        })
    }

    fn io_source(source: PollSource) -> IoSource {
        match source {
            #[cfg(unix)]
            PollSource::Fd(fd) => IoSource::Fd(fd),
        }
    }

    impl GuiPark {
        /// 開きかけのゲートがあるとき、まだならbackendを作る。作れなければ誤りを残し、ゲートに知らせて1回だけ`run`を
        /// 呼ぶ。誤りは`open_ui_raw`が起こす。
        fn create(&mut self, gate: i64, pending: &mut dyn Pending) -> bool {
            let slot = &self.slot;
            if slot.create_error.borrow().is_none()
                && let Some(backend) = slot.backend.take()
            {
                match backend.create() {
                    Ok(native) => {
                        *slot.parts.borrow_mut() = Some(Parts {
                            widgets: native.widgets,
                            queue: native.queue,
                        });
                        slot.waker.install(native.event_loop.waker());
                        self.event_loop = Some(native.event_loop);
                        return true;
                    }
                    Err(e) => *slot.create_error.borrow_mut() = Some(e),
                }
            }
            if slot.create_error.borrow().is_none() {
                return false;
            }
            pending.poke(gate);
            slot.poked.set(true);
            pending.run();
            false
        }
    }

    /// ループの1つの段。回収、制御のイベント、開きかけのゲートと`Ui`のゲートへの知らせの後に`run`を呼ぶ。回収か
    /// まだ知らせていない開きかけが残る間は繰り返す。`Stopping`なら回収してから`Stop`を返す。
    fn step(
        slot: &Slot,
        interrupt: &(dyn Fn() + Send + Sync),
        pending: &mut dyn Pending,
        reason: Reason,
    ) -> Step {
        let mut last = None;
        loop {
            slot.reap(pending);
            slot.settle(interrupt);
            if let Some(gate) = slot.opening.get()
                && !slot.poked.get()
            {
                pending.poke(gate);
                slot.poked.set(true);
            }
            if let Some(gate) = slot.live_gate()
                && (reason == Reason::Events || slot.has_events())
            {
                pending.poke(gate);
            }
            match pending.run() {
                Progress::Stopping => {
                    slot.reap(pending);
                    return Step::Stop;
                }
                Progress::Wait(timeout) => last = timeout,
                Progress::Busy => {}
            }
            let unpoked = slot.opening.get().is_some() && !slot.poked.get();
            if !(slot.reap_flagged() || unpoked) {
                return Step::Continue(last);
            }
        }
    }

    impl Park for GuiPark {
        fn wait(
            &mut self,
            timeout: Option<Duration>,
            pending: &mut dyn Pending,
        ) -> std::io::Result<()> {
            if self.event_loop.is_none() {
                let Some(gate) = self.slot.opening.get() else {
                    return pending.default_wait();
                };
                if !self.create(gate, pending) {
                    return Ok(());
                }
            }
            let Some(mut event_loop) = self.event_loop.take() else {
                return Ok(());
            };
            let slot = Rc::clone(&self.slot);
            let interrupt = Arc::clone(&self.interrupt);
            let io = pending.io().map(io_source);
            let ran = event_loop.run_loop(io, timeout, &mut |reason| {
                step(&slot, &*interrupt, &mut *pending, reason)
            });
            self.event_loop = Some(event_loop);
            ran
        }

        fn waker(&self) -> Option<Arc<dyn IoWaker>> {
            Some(Arc::clone(&self.slot.waker) as Arc<dyn IoWaker>)
        }
    }

    impl Drop for GuiPark {
        /// runが`Ui`を開いたまま終わった (`exit`など) ときは、ここでウィンドウを閉じる。残ったゲートは`sweep_all`が放す。
        fn drop(&mut self) {
            if self.slot.ui.borrow_mut().take().is_some() {
                if let Some(mut toolkit) = self.slot.toolkit.borrow_mut().take() {
                    toolkit.close_all();
                }
                self.slot.set_presence(false);
            }
            GUI_SLOT.with(|s| *s.borrow_mut() = None);
        }
    }

    /// `Std.Gui.want_gui_raw`。ゲートを開いて開きかけの印にする。本物の時計では、ネイティブのループがその源を待てる
    /// ように、メインのworkerのdriverをここで作る。
    pub(crate) fn want<P: Policy>(sched: &mut Scheduler<P>) -> Result<Result<i64, Raise>, VmFault> {
        let Some(slot) = current_slot() else {
            return Ok(Err(not_available()));
        };
        if slot.opening.get().is_some() || slot.live_gate().is_some() {
            return Ok(Err(Raise::Io {
                kind: externs::IO_OTHER,
                message: "cannot open the GUI: it is already open".to_string(),
            }));
        }
        sched.own_driver()?;
        let gate = sched.cells.lock().open_gate().encode();
        // ゲートで待つタスクはデッドロックの判定に数えないので、メインのworkerがこの後に眠る前に印を立てる
        // (docs/spec/vm.mdの「GUI の Park」)
        sched.run.swap_holds(true);
        slot.opening.set(Some(gate));
        slot.poked.set(false);
        Ok(Ok(gate))
    }

    /// `Std.Gui.open_ui_raw`。`Park`が知らせたゲートで、ループの中から`Toolkit`を作り、`Ui`を返す。runの外の待ちの
    /// 印は`want_gui_raw`が立てたままで、`Ui`の回収まで残す。backendを作れなかったら、ゲートを放し、印を下ろしてその誤りを
    /// 起こす。
    pub(crate) fn open_ui<M: Mode, P: Policy>(
        ctx: &mut Ctx<M>,
        sched: &mut Scheduler<P>,
        gate: i64,
    ) -> Result<Result<Word, Raise>, VmFault> {
        let Some(slot) = current_slot() else {
            sched.cells.lock().release_gate(gate)?;
            return Ok(Err(not_available()));
        };
        if slot.opening.get() != Some(gate) {
            return Err(VmFault::Internal(
                "open_ui_raw names a gate that is not opening",
            ));
        }
        slot.opening.set(None);
        slot.poked.set(false);
        if let Some(e) = slot.create_error.borrow().clone() {
            sched.cells.lock().release_gate(gate)?;
            lower_holds(sched);
            let (kind, message) = error_kind(e);
            return Ok(Err(Raise::Io { kind, message }));
        }
        if slot.ui.borrow().is_some() {
            return Err(VmFault::Internal(
                "a Ui opens before the last one is reaped",
            ));
        }
        let toolkit = match &*slot.parts.borrow() {
            Some(parts) => Toolkit::new(Rc::clone(&parts.widgets), parts.queue.clone()),
            None => return Err(VmFault::Internal("open_ui_raw runs without a backend")),
        };
        *slot.toolkit.borrow_mut() = Some(toolkit);
        slot.set_presence(true);
        let generation = slot.gens.get().wrapping_add(1);
        slot.gens.set(generation);
        let reap = Arc::new(AtomicBool::new(false));
        *slot.ui.borrow_mut() = Some(UiState {
            generation,
            gate,
            reap: Arc::clone(&reap),
        });
        debug_assert!(sched.run.holds(), "a GUI opens without holds");
        Ok(Ok(
            unsafe { ctx.heap_mut() }.alloc_foreign(UiToken { generation, reap }, true)
        ))
    }

    /// 開きかけの印がゲート`gate`なら下ろし、下ろしたかを返す。
    pub(crate) fn forget_opening(gate: i64) -> bool {
        let Some(slot) = current_slot() else {
            return false;
        };
        if slot.opening.get() != Some(gate) {
            return false;
        }
        slot.opening.set(None);
        slot.poked.set(false);
        true
    }

    /// `GuiError`のIOの失敗の種類と文言 (docs/spec/stdlib.mdの「`Std.Gui`」)。
    fn error_kind(e: GuiError) -> (i64, String) {
        let other = |m: &str| (externs::IO_OTHER, m.to_string());
        match e {
            GuiError::NoContainer | GuiError::NothingOpen => other("gui: no open container"),
            GuiError::UnknownId => (
                externs::IO_NOT_FOUND,
                "gui: unknown or closed widget".to_string(),
            ),
            GuiError::WrongKind => other("gui: the widget has no such property"),
            GuiError::OutOfRange => other("gui: a value is out of range"),
            GuiError::NotAvailable(m) => (externs::IO_NOT_AVAILABLE, m),
            GuiError::Backend(m) => (externs::IO_OTHER, m),
        }
    }

    /// 借りた文字列の中身。参照は手放さない。
    ///
    /// # Safety
    /// `w`は呼ぶ側が持つ生きた文字列である。
    unsafe fn text<M: Mode>(ctx: &Ctx<M>, w: Word) -> Result<String, VmFault> {
        Ok(unsafe { (*ctx.heap().string(w)?).to_owned() })
    }

    /// `"a" | "f<n>" | "p<n>"`を`,`で並べた列。数が範囲の外なら`OutOfRange`である。
    fn tracks(text: &str) -> Result<Result<Vec<Track>, GuiError>, VmFault> {
        let mut out = Vec::new();
        for t in text.split(',').filter(|t| !t.is_empty()) {
            let (tag, n) = t.split_at(1);
            let track = match (tag, n) {
                ("a", "") => Track::Auto,
                ("f", n) => match n.parse() {
                    Ok(n) if n > 0 => Track::Fr(n),
                    _ => return Ok(Err(GuiError::OutOfRange)),
                },
                ("p", n) => match n.parse() {
                    Ok(n) => Track::Px(n),
                    Err(_) => return Ok(Err(GuiError::OutOfRange)),
                },
                _ => {
                    return Err(VmFault::Internal(
                        "a Std.Gui track is not `a`, `f<n>` or `p<n>`",
                    ));
                }
            };
            out.push(track);
        }
        Ok(Ok(out))
    }

    fn align(n: i64) -> Result<eml_gui::Align, GuiError> {
        Ok(match n {
            0 => eml_gui::Align::Start,
            1 => eml_gui::Align::Center,
            2 => eml_gui::Align::End,
            3 => eml_gui::Align::Stretch,
            _ => return Err(GuiError::OutOfRange),
        })
    }

    fn span(n: i64) -> Result<u16, GuiError> {
        u16::try_from(n).map_err(|_| GuiError::OutOfRange)
    }

    fn id(w: Word) -> Id {
        Id(w.bits() as u64)
    }

    fn int(n: i64) -> Word {
        Word::from_bits(n as usize)
    }

    /// toolkitに触れる行。`Ui`を消費する行は、失敗して起こす前に`Ui`を手放す (docs/spec/lir.mdの「extern の借用」)。
    ///
    /// # Safety
    /// `args`は行の引数の語で、`Ui`は呼ぶ側が持つ生きた`Std.Gui.Ui`、文字列は生きた文字列である。
    pub(crate) unsafe fn arm<M: Mode>(
        ctx: &mut Ctx<M>,
        ext: Extern,
        args: &[Word],
    ) -> Result<Word, VmFault> {
        let ui = args[ext.row().params.len() - 1];
        if ext == Extern::GuiCloseUi {
            unsafe { ctx.heap_mut().decref(ui) }?;
            return Ok(Word::UNIT);
        }
        let consumes = !ext.row().borrowed.last().copied().unwrap_or(false);
        let done = unsafe { toolkit_row(ctx, ext, args, ui) };
        let failure = match done {
            Ok(Ok(w)) => return Ok(w),
            Ok(Err(e)) => {
                let (kind, message) = error_kind(e);
                raised(kind, &message)
            }
            Err(fault) => fault,
        };
        if consumes {
            unsafe { ctx.heap_mut().decref(ui) }?;
        }
        Err(failure)
    }

    /// `arm`の本体。`Ui`は手放さない。
    ///
    /// # Safety
    /// `arm`と同じ。
    unsafe fn toolkit_row<M: Mode>(
        ctx: &mut Ctx<M>,
        ext: Extern,
        args: &[Word],
        ui: Word,
    ) -> Result<Result<Word, GuiError>, VmFault> {
        let generation = unsafe { (*ctx.heap().foreign::<UiToken>(ui)?).generation };
        let Some(slot) = current_slot() else {
            let (kind, message) = (externs::IO_NOT_AVAILABLE, NOT_AVAILABLE);
            return Err(raised(kind, message));
        };
        if slot
            .ui
            .borrow()
            .as_ref()
            .is_none_or(|state| state.generation != generation)
        {
            return Err(VmFault::Internal("a Std.Gui.Ui names a closed GUI"));
        }
        let raw = |i: usize| args[i].bits() as i64;
        let mut guard = slot.toolkit.borrow_mut();
        let toolkit = guard
            .as_mut()
            .ok_or(VmFault::Internal("an open Ui has no toolkit"))?;
        let same = Ok(ui);
        let done: Result<Word, GuiError> = match ext {
            Extern::GuiBeginWindowRaw => {
                let title = unsafe { text(ctx, args[0]) }?;
                toolkit.begin_window(&title).map(|w| int(w.0 as i64))
            }
            Extern::GuiBeginGridRaw => {
                let cols = tracks(&unsafe { text(ctx, args[0]) }?)?;
                let rows = tracks(&unsafe { text(ctx, args[1]) }?)?;
                cols.and_then(|cols| rows.map(|rows| (cols, rows)))
                    .and_then(|(cols, rows)| toolkit.begin_grid(&cols, &rows))
                    .map(|w| int(w.0 as i64))
            }
            Extern::GuiEndRaw => toolkit.end().and(same),
            Extern::GuiNextCellRaw => (|| {
                let cell = eml_gui::Cell {
                    col_span: span(raw(0))?,
                    row_span: span(raw(1))?,
                    halign: align(raw(2))?,
                    valign: align(raw(3))?,
                };
                toolkit.next_cell(cell)
            })()
            .and(same),
            Extern::GuiCreateRaw => {
                let s = unsafe { text(ctx, args[1]) }?;
                let n = raw(2);
                let init = match raw(0) {
                    1 => Init::Label(s),
                    2 => Init::Button(s),
                    3 => Init::TextField(s),
                    4 => Init::CheckBox(s, n != 0),
                    5 => Init::Choice,
                    6 => Init::ProgressBar(n),
                    _ => return Err(VmFault::Internal("create_raw names no widget kind")),
                };
                toolkit.create(init).map(|w| int(w.0 as i64))
            }
            Extern::GuiAddItemRaw => {
                let item = unsafe { text(ctx, args[1]) }?;
                toolkit.add_item(id(args[0]), &item).and(same)
            }
            Extern::GuiSetTextRaw => {
                let s = unsafe { text(ctx, args[1]) }?;
                toolkit.set(id(args[0]), Prop::Text(s)).and(same)
            }
            Extern::GuiSetIntRaw => {
                let v = raw(2);
                let prop = match raw(1) {
                    1 => Prop::Value(v),
                    2 => Prop::Checked(v != 0),
                    3 => Prop::Enabled(v != 0),
                    _ => return Err(VmFault::Internal("set_int_raw names no property")),
                };
                toolkit.set(id(args[0]), prop).and(same)
            }
            Extern::GuiCloseWindowRaw => toolkit.close_window(id(args[0])).and(same),
            Extern::GuiOpenWindowsRaw => Ok(int(toolkit.open_windows() as i64)),
            Extern::GuiFlushRaw => {
                toolkit.flush();
                same
            }
            Extern::GuiPollRaw => {
                let event = toolkit.poll();
                let w = event.as_ref().map_or(0, |e| e.id().0 as i64);
                *slot.event.borrow_mut() = event;
                Ok(int(w))
            }
            Extern::GuiEventIntRaw => match &*slot.event.borrow() {
                Some(Event::Toggle(_, b)) => Ok(int(i64::from(*b))),
                Some(Event::Select(_, i)) => Ok(int(*i)),
                _ => {
                    return Err(VmFault::Internal(
                        "event_int_raw reads an event without an Int",
                    ));
                }
            },
            Extern::GuiEventTextRaw => match &*slot.event.borrow() {
                Some(Event::Text(_, s)) => Ok(ctx.fresh_string(s)),
                _ => {
                    return Err(VmFault::Internal(
                        "event_text_raw reads an event without a String",
                    ));
                }
            },
            _ => {
                return Err(VmFault::Internal(
                    "a row outside Std.Gui reached the GUI arm",
                ));
            }
        };
        Ok(done)
    }

    #[cfg(test)]
    mod tests {
        use eml_gui::Script;

        use super::*;

        /// `Send`な型では両方の実装が当てはまるので、型の推論が決まらず、コンパイルが止まる。
        trait AmbiguousIfSend<A> {
            fn some_item() {}
        }
        impl<T: ?Sized> AmbiguousIfSend<()> for T {}
        impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}

        #[test]
        fn the_slot_and_the_park_are_not_send() {
            <Slot as AmbiguousIfSend<_>>::some_item();
            <GuiPark as AmbiguousIfSend<_>>::some_item();
        }

        /// 枠は`Park`を作ったスレッドにだけあるので、ほかのスレッドの腕は`NotAvailable`になる。
        #[test]
        fn the_slot_is_seen_only_on_the_thread_that_made_the_park() {
            let park = park(Backend::Stub(Script::default()), Arc::new(|| {}))();
            assert!(current_slot().is_some());
            let elsewhere = std::thread::spawn(|| current_slot().is_none()).join();
            assert!(matches!(elsewhere, Ok(true)));
            drop(park);
            assert!(current_slot().is_none());
        }
    }
}

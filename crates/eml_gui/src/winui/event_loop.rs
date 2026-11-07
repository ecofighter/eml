//! `Application::Start`の上のループ。VMの`step`は、どれもメインのスレッドの`DispatcherQueue`に積んだ仕事から呼ぶ。
//! 起こしは`TryEnqueue`、時間切れは1回だけの`DispatcherQueueTimer`で、callbackが積んだイベントはcallbackが予約した
//! 仕事が拾う (docs/implementation/architecture.mdの「`eml_gui`の内部」)。
//!
//! `Application`の子のクラスとXAMLの型の情報の渡し方は、winio (MIT) の`winio-ui-winui`の`runtime.rs`に倣った。

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use windows::UI::Xaml::Interop::TypeName;
use windows_core::{Array, EventRevoker, HSTRING, Ref, implement};
use windows_time::TimeSpan;
use winui3::Microsoft::UI::Dispatching::{
    DispatcherQueue, DispatcherQueueHandler, DispatcherQueueTimer,
};
use winui3::Microsoft::UI::Xaml::Controls::XamlControlsResources;
use winui3::Microsoft::UI::Xaml::Markup::{
    IXamlMetadataProvider, IXamlMetadataProvider_Impl, IXamlType, XmlnsDefinition,
};
use winui3::Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider;
use winui3::Microsoft::UI::Xaml::{
    Application, ApplicationInitializationCallback, DispatcherShutdownMode, IApplicationOverrides,
    IApplicationOverrides_Impl, LaunchActivatedEventArgs,
};

use super::{Value, with_shared};
use crate::{EventLoop, EventQueue, Id, IoSource, RawEvent, Reason, Step};

/// backendのメインのスレッドの側。ウィジェット、ループ、callbackが共有する。
pub(super) struct Shared {
    queue: EventQueue,
    bell: Arc<Bell>,
    /// ウィジェットの値として最後に知らせたか置いたもの。`super::push_if_changed`が見る。
    pub(super) known: RefCell<HashMap<Id, Value>>,
    /// `Application::Start`が戻った。その後はWinUIのオブジェクトに触れない。
    pub(super) exited: Cell<bool>,
    dispatcher: RefCell<Option<DispatcherQueue>>,
    timer: RefCell<Option<(DispatcherQueueTimer, EventRevoker)>>,
    /// callbackが積んだイベントのための`step`を、もう予約した。
    nudged: Cell<bool>,
    /// `run_loop`の間だけ置く`step`。`run_loop`が戻る前に外す。
    step: Cell<Option<*mut (dyn FnMut(Reason) -> Step + 'static)>>,
    /// `step`を呼んでいる最中。その間に来た理由は`deferred`に残し、`step`が戻ってから呼ぶ。
    running: Cell<bool>,
    deferred: Cell<Option<Reason>>,
    stopped: Cell<bool>,
    panic: RefCell<Option<Box<dyn Any + Send>>>,
    error: RefCell<Option<io::Error>>,
}

impl Shared {
    pub(super) fn new(queue: EventQueue) -> Shared {
        Shared {
            queue,
            bell: Arc::new(Bell {
                rang: AtomicBool::new(false),
                queued: AtomicBool::new(false),
                dispatcher: Mutex::new(None),
            }),
            known: RefCell::new(HashMap::new()),
            exited: Cell::new(false),
            dispatcher: RefCell::new(None),
            timer: RefCell::new(None),
            nudged: Cell::new(false),
            step: Cell::new(None),
            running: Cell::new(false),
            deferred: Cell::new(None),
            stopped: Cell::new(false),
            panic: RefCell::new(None),
            error: RefCell::new(None),
        }
    }

    /// callbackが呼ぶ。列に積み、それを扱う`step(Events)`の仕事をまだなければ1つ予約する。`step`はcallbackの中では
    /// 呼ばない。
    pub(super) fn push(&self, event: RawEvent) {
        self.queue.push(event);
        if self.nudged.replace(true) {
            return;
        }
        let queued = self.dispatcher.borrow().as_ref().is_some_and(|d| {
            d.TryEnqueue(&DispatcherQueueHandler::new(|| {
                with_shared(|s| {
                    s.nudged.set(false);
                    if s.queue.take_pushed() {
                        s.drive(Reason::Events);
                    }
                });
                Ok(())
            }))
            .unwrap_or(false)
        });
        // 予約できなければ、最初の`step`か次の予約が拾う
        if !queued {
            self.nudged.set(false);
        }
    }

    fn drive(&self, reason: Reason) {
        if self.stopped.get() {
            return;
        }
        if self.running.get() {
            self.deferred
                .set(Some(self.deferred.get().unwrap_or(reason)));
            return;
        }
        let Some(step) = self.step.get() else {
            return;
        };
        self.running.set(true);
        let mut reason = reason;
        loop {
            // `step`が列を見るので、それより前に積まれた分の印は消す
            self.queue.take_pushed();
            // SAFETY: `step`は`run_loop`の引数で、`run_loop`が戻る前に外す。`running`で入れ子の呼び出しを防ぐ
            let ran = panic::catch_unwind(AssertUnwindSafe(|| unsafe { (*step)(reason) }));
            match ran {
                Ok(Step::Continue(timeout)) => self.arm(timeout),
                Ok(Step::Stop) => {
                    self.stop();
                    break;
                }
                Err(payload) => {
                    *self.panic.borrow_mut() = Some(payload);
                    self.stop();
                    break;
                }
            }
            match self.deferred.take() {
                Some(next) => reason = next,
                None => break,
            }
        }
        self.running.set(false);
    }

    fn arm(&self, timeout: Option<Duration>) {
        if let Some((timer, _)) = &*self.timer.borrow() {
            let _ = timer.Stop();
            if let Some(t) = timeout {
                // 早く鳴って空回りしないように、100 nsの刻みに切り上げる
                let ticks = i64::try_from(t.as_nanos().div_ceil(100)).unwrap_or(i64::MAX);
                let _ = timer.SetInterval(TimeSpan::from_ticks(ticks));
                let _ = timer.Start();
            }
        }
    }

    fn stop(&self) {
        self.stopped.set(true);
        if let Some((timer, _)) = &*self.timer.borrow() {
            let _ = timer.Stop();
        }
        if let Err(e) = Application::Current().and_then(|app| app.Exit()) {
            self.fail(e);
        }
    }

    fn fail(&self, e: windows_core::Error) {
        self.error
            .borrow_mut()
            .get_or_insert_with(|| io::Error::other(format!("WinUI failed: {e}")));
    }

    /// `OnLaunched`から呼ぶ。ループの準備をして、最初の`step(Woken)`を呼ぶ。
    fn launched(&self) {
        if let Err(e) = self.prepare() {
            self.fail(e);
            self.stop();
            return;
        }
        // 最初の`step(Woken)`は、それより前に鳴った起こしもまとめて扱う
        self.bell.rang.store(false, Ordering::Release);
        self.drive(Reason::Woken);
    }

    fn prepare(&self) -> windows_core::Result<()> {
        let dispatcher = DispatcherQueue::GetForCurrentThread()?;
        let timer = dispatcher.CreateTimer()?;
        timer.SetIsRepeating(false)?;
        let tick = timer.Tick(|_, _| {
            with_shared(|s| s.drive(Reason::Timeout));
            Ok(())
        })?;
        *self.timer.borrow_mut() = Some((timer, tick));
        *self.dispatcher.borrow_mut() = Some(dispatcher.clone());
        self.bell.listen(Some(dispatcher));
        Ok(())
    }
}

/// どのスレッドからも鳴らせる起こし。鳴らすとメインのスレッドに`step(Woken)`の仕事を1つ積む。積んだ仕事がまだ
/// 走っていなければ、重ねて積まない。
struct Bell {
    rang: AtomicBool,
    queued: AtomicBool,
    /// ループが走る間だけ置く、メインのスレッドの`DispatcherQueue`。
    dispatcher: Mutex<Option<DispatcherQueue>>,
}

impl Bell {
    fn ring(self: &Arc<Bell>) {
        self.rang.store(true, Ordering::Release);
        let dispatcher = self
            .dispatcher
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(dispatcher) = &*dispatcher else {
            return;
        };
        if self.queued.swap(true, Ordering::AcqRel) {
            return;
        }
        let bell = Arc::clone(self);
        let queued = dispatcher
            .TryEnqueue(&DispatcherQueueHandler::new(move || {
                bell.queued.store(false, Ordering::Release);
                if bell.rang.swap(false, Ordering::AcqRel) {
                    with_shared(|s| s.drive(Reason::Woken));
                }
                Ok(())
            }))
            .unwrap_or(false);
        if !queued {
            self.queued.store(false, Ordering::Release);
        }
    }

    fn listen(&self, dispatcher: Option<DispatcherQueue>) {
        *self
            .dispatcher
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = dispatcher;
        self.queued.store(false, Ordering::Release);
    }
}

pub(super) struct WinUiLoop {
    shared: Rc<Shared>,
}

impl WinUiLoop {
    pub(super) fn new(shared: Rc<Shared>) -> WinUiLoop {
        WinUiLoop { shared }
    }
}

/// `run_loop`が置いたものを、`Application::Start`が戻った後に外す。起こしは`DispatcherQueue`から外す。XAMLの核はもう
/// 止まっているので、時計には触れず (止めるのは`Stop`の`stop`がすでにした)、登録も外す呼び出しをせずに忘れる。
struct Running<'a>(&'a Shared);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let shared = self.0;
        shared.bell.listen(None);
        if let Some((_timer, tick)) = shared.timer.borrow_mut().take() {
            tick.forget();
        }
        shared.dispatcher.borrow_mut().take();
        shared.step.set(None);
        shared.deferred.set(None);
        shared.exited.set(true);
    }
}

impl EventLoop for WinUiLoop {
    fn run_loop(
        &mut self,
        _io: Option<IoSource>,
        _timeout: Option<Duration>,
        step: &mut dyn FnMut(Reason) -> Step,
    ) -> io::Result<()> {
        let shared = &*self.shared;
        let step: *mut (dyn FnMut(Reason) -> Step + '_) = step;
        // SAFETY: 寿命を消すだけで、`step`はこの関数を出る前に`Running`の`Drop`が外す
        let step: *mut (dyn FnMut(Reason) -> Step + 'static) = unsafe { std::mem::transmute(step) };
        let running = Running(shared);
        shared.step.set(Some(step));
        shared.stopped.set(false);
        // `Start`は`Exit`まで戻らない。`OnLaunched`が最初の`step`を呼ぶ
        let started = Application::Start(&ApplicationInitializationCallback::new(|_| {
            let app = App::compose()?;
            // 最後のウィンドウを閉じてもループを終えない。終えるのは`Stop`の`Exit`だけである
            app.SetDispatcherShutdownMode(DispatcherShutdownMode::OnExplicitShutdown)
        }));
        drop(running);
        if let Some(payload) = shared.panic.take() {
            panic::resume_unwind(payload);
        }
        started.map_err(|e| io::Error::other(format!("WinUI failed: {e}")))?;
        match shared.error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let bell = Arc::clone(&self.shared.bell);
        Arc::new(move || bell.ring())
    }
}

/// WinUIの`Application`の子のクラス。XAMLのコントロールの見た目の資源を読み込み、XAMLの型の情報を
/// `XamlControlsXamlMetaDataProvider`から渡す。
#[implement(IApplicationOverrides, IXamlMetadataProvider)]
struct App {
    provider: XamlControlsXamlMetaDataProvider,
}

impl App {
    fn compose() -> windows_core::Result<Application> {
        Application::compose(App {
            provider: XamlControlsXamlMetaDataProvider::new()?,
        })
    }
}

impl IApplicationOverrides_Impl for App_Impl {
    fn OnLaunched(&self, _: Ref<LaunchActivatedEventArgs>) -> windows_core::Result<()> {
        let resources = Application::Current().and_then(|app| {
            app.Resources()?
                .MergedDictionaries()?
                .Append(&XamlControlsResources::new()?)
        });
        with_shared(|s| match resources {
            Ok(()) => s.launched(),
            Err(e) => {
                s.fail(e);
                s.stop();
            }
        });
        Ok(())
    }
}

impl IXamlMetadataProvider_Impl for App_Impl {
    fn GetXamlType(&self, ty: &TypeName) -> windows_core::Result<IXamlType> {
        self.provider.GetXamlType(ty)
    }

    fn GetXamlTypeByFullName(&self, name: &HSTRING) -> windows_core::Result<IXamlType> {
        self.provider.GetXamlTypeByFullName(name)
    }

    fn GetXmlnsDefinitions(&self) -> windows_core::Result<Array<XmlnsDefinition>> {
        self.provider.GetXmlnsDefinitions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_before_the_loop_listens_queues_nothing_and_stays_marked() {
        let bell = Arc::new(Bell {
            rang: AtomicBool::new(false),
            queued: AtomicBool::new(false),
            dispatcher: Mutex::new(None),
        });
        bell.ring();
        assert!(bell.rang.load(Ordering::Acquire));
        assert!(!bell.queued.load(Ordering::Acquire));
    }
}

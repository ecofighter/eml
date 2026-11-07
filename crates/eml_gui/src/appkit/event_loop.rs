//! `NSApplication::run`の上のループ。VMの`step`はrun loopの源から呼ぶ。起こしは`CFRunLoopSource`、I/Oは
//! `CFFileDescriptor`、時間切れは`CFRunLoopTimer`で、callbackが積んだイベントは眠る前の`CFRunLoopObserver`が拾う
//! (docs/implementation/architecture.mdの「`eml_gui`の内部」)。

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::sync::Arc;
use std::time::Duration;

use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFFileDescriptor, CFFileDescriptorContext, CFIndex, CFOptionFlags,
    CFRetained, CFRunLoop, CFRunLoopActivity, CFRunLoopMode, CFRunLoopObserver,
    CFRunLoopObserverContext, CFRunLoopSource, CFRunLoopSourceContext, CFRunLoopTimer,
    CFRunLoopTimerContext, kCFFileDescriptorReadCallBack, kCFRunLoopCommonModes,
};
use objc2_foundation::NSPoint;

use crate::{EventLoop, EventQueue, IoSource, Reason, Step};

/// 待つものがないときの時計の次の発火。runより長く、実際には来ない。
const NEVER: f64 = 1.0e10;

/// 1回の`drive`で、`step`の最中に積まれたイベントのために続けて呼ぶ`step(Events)`の上限。ふつうは、表示したウィンドウの
/// 大きさの知らせのような1回で収まる。上限は、積むことが積むことを呼び続ける場合に、1つのcallbackの中にrun loopを
/// 閉じ込めないためにある。超えた分は印に残り、次に眠る前のobserverが拾う。
const PUSHED_STEPS: u32 = 8;

/// runのループの状態。run loopの源のcallbackは`info`としてこれを指す。`AppKitLoop`が箱に入れて持つので、場所は動かない。
struct State {
    app: Retained<NSApplication>,
    queue: EventQueue,
    /// `run_loop`の間だけ置く`step`。`run_loop`が戻る前に外す。
    step: Cell<Option<*mut (dyn FnMut(Reason) -> Step + 'static)>>,
    /// `step`を呼んでいる最中。その間に来た源は`deferred`に残し、`step`が戻ってから呼ぶ。
    running: Cell<bool>,
    deferred: Cell<Option<Reason>>,
    stopped: Cell<bool>,
    panic: RefCell<Option<Box<dyn Any + Send>>>,
    timer: RefCell<Option<CFRetained<CFRunLoopTimer>>>,
    fd: RefCell<Option<CFRetained<CFFileDescriptor>>>,
}

impl State {
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
        let mut pushed_steps = 0;
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
            // `step`の最中に積まれたイベントは、この`step`がobserverから呼ばれていればそのobserverはもう過ぎているので、
            // ここで拾う
            if let Some(next) = self.deferred.take() {
                reason = next;
            } else if pushed_steps < PUSHED_STEPS && self.queue.take_pushed() {
                pushed_steps += 1;
                reason = Reason::Events;
            } else {
                break;
            }
        }
        self.running.set(false);
    }

    fn arm(&self, timeout: Option<Duration>) {
        if let Some(timer) = &*self.timer.borrow() {
            let after = timeout.map_or(NEVER, |t| t.as_secs_f64());
            timer.set_next_fire_date(CFAbsoluteTimeGetCurrent() + after);
        }
    }

    /// `stop:`は次のイベントを扱ってから効くので、何もしないイベントを1つ送る。
    fn stop(&self) {
        self.stopped.set(true);
        self.app.stop(None);
        let wake = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::ApplicationDefined,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            0,
            0,
            0,
        );
        if let Some(wake) = wake {
            self.app.postEvent_atStart(&wake, false);
        }
    }
}

/// # Safety
/// `info`は`run_loop`の間生きている`State`を指す。
unsafe fn state<'a>(info: *mut c_void) -> &'a State {
    // SAFETY: 呼び出し側の約束による
    unsafe { &*(info as *const State) }
}

unsafe extern "C-unwind" fn woken(info: *mut c_void) {
    // SAFETY: 起こしの源の`info`は`State`である
    unsafe { state(info) }.drive(Reason::Woken);
}

unsafe extern "C-unwind" fn timed_out(_: *mut CFRunLoopTimer, info: *mut c_void) {
    // SAFETY: 時計の`info`は`State`である
    unsafe { state(info) }.drive(Reason::Timeout);
}

unsafe extern "C-unwind" fn readable(
    fd: *mut CFFileDescriptor,
    _: CFOptionFlags,
    info: *mut c_void,
) {
    // SAFETY: fdの`info`は`State`である
    let state = unsafe { state(info) };
    state.drive(Reason::Io);
    // callbackは1回ごとに切れるので、また有効にする
    // SAFETY: CFが渡したfdで、callbackの間は生きている
    if let Some(fd) = unsafe { fd.as_ref() } {
        fd.enable_call_backs(kCFFileDescriptorReadCallBack);
    }
}

unsafe extern "C-unwind" fn before_waiting(
    _: *mut CFRunLoopObserver,
    _: CFRunLoopActivity,
    info: *mut c_void,
) {
    // SAFETY: observerの`info`は`State`である
    let state = unsafe { state(info) };
    if state.queue.take_pushed() {
        state.drive(Reason::Events);
    }
}

/// どのスレッドからも鳴らせる起こし。
struct Bell {
    source: CFRetained<CFRunLoopSource>,
    run_loop: CFRetained<CFRunLoop>,
}

// SAFETY: `CFRunLoopSourceSignal`と`CFRunLoopWakeUp`はどのスレッドから呼んでもよく、CFの参照の数え方もスレッドをまたいで
// 正しい
unsafe impl Send for Bell {}
unsafe impl Sync for Bell {}

impl Bell {
    fn ring(&self) {
        self.source.signal();
        self.run_loop.wake_up();
    }
}

pub(super) struct AppKitLoop {
    state: Box<State>,
    bell: Arc<Bell>,
}

impl AppKitLoop {
    pub(super) fn new(app: Retained<NSApplication>, queue: EventQueue) -> AppKitLoop {
        let state = Box::new(State {
            app,
            queue,
            step: Cell::new(None),
            running: Cell::new(false),
            deferred: Cell::new(None),
            stopped: Cell::new(false),
            panic: RefCell::new(None),
            timer: RefCell::new(None),
            fd: RefCell::new(None),
        });
        let mut context = CFRunLoopSourceContext {
            version: 0,
            info: info(&state),
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform: Some(woken),
        };
        // SAFETY: `context`は正しい形で、`info`の`State`は源を外すまで生きる (`Drop`で源を無効にする)
        let source = unsafe { CFRunLoopSource::new(None, 0, &mut context) }
            .expect("CFRunLoopSourceCreate failed");
        let run_loop = CFRunLoop::main().expect("the main thread has a run loop");
        AppKitLoop {
            state,
            bell: Arc::new(Bell { source, run_loop }),
        }
    }
}

fn info(state: &State) -> *mut c_void {
    ptr::from_ref(state).cast_mut().cast()
}

impl Drop for AppKitLoop {
    fn drop(&mut self) {
        // 起こしは`State`より長く生きうるので、源を無効にして`perform`がもう呼ばれないようにする
        self.bell.source.invalidate();
    }
}

/// `run_loop`がrun loopに置いたものと`step`。どの出口でも (`Err`、panic) `Drop`で外して無効にし、`step`を外すので、
/// 失敗した`run_loop`もrun loopに何も残さず、`State`は外した後の`step`を指さない。
struct Scheduled<'a> {
    state: &'a State,
    run_loop: &'a CFRunLoop,
    modes: Option<&'a CFRunLoopMode>,
    bell: Option<&'a CFRunLoopSource>,
    fd_source: Option<CFRetained<CFRunLoopSource>>,
    observer: Option<CFRetained<CFRunLoopObserver>>,
}

impl Drop for Scheduled<'_> {
    fn drop(&mut self) {
        if let Some(bell) = self.bell {
            self.run_loop.remove_source(Some(bell), self.modes);
        }
        if let Some(observer) = self.observer.take() {
            observer.invalidate();
        }
        if let Some(source) = self.fd_source.take() {
            source.invalidate();
        }
        if let Some(watch) = self.state.fd.borrow_mut().take() {
            watch.invalidate();
        }
        if let Some(timer) = self.state.timer.borrow_mut().take() {
            timer.invalidate();
        }
        self.state.step.set(None);
        self.state.deferred.set(None);
    }
}

impl EventLoop for AppKitLoop {
    fn run_loop(
        &mut self,
        io: Option<IoSource>,
        _timeout: Option<Duration>,
        step: &mut dyn FnMut(Reason) -> Step,
    ) -> io::Result<()> {
        let state = &*self.state;
        let run_loop = &self.bell.run_loop;
        // SAFETY: 定数はCFが定める
        let modes = unsafe { kCFRunLoopCommonModes };
        let step: *mut (dyn FnMut(Reason) -> Step + '_) = step;
        // SAFETY: 寿命を消すだけで、`step`はこの関数を出る前に`Scheduled`の`Drop`が外す
        let step: *mut (dyn FnMut(Reason) -> Step + 'static) = unsafe { std::mem::transmute(step) };
        let mut scheduled = Scheduled {
            state,
            run_loop,
            modes,
            bell: None,
            fd_source: None,
            observer: None,
        };
        state.step.set(Some(step));
        state.stopped.set(false);

        let mut timer_context = CFRunLoopTimerContext {
            version: 0,
            info: info(state),
            retain: None,
            release: None,
            copyDescription: None,
        };
        // SAFETY: `context`は正しい形で、時計はこの関数を出る前に無効にする
        let timer = unsafe {
            CFRunLoopTimer::new(
                None,
                CFAbsoluteTimeGetCurrent() + NEVER,
                NEVER,
                0,
                0,
                Some(timed_out),
                &mut timer_context,
            )
        }
        .ok_or_else(|| io::Error::other("CFRunLoopTimerCreate failed"))?;
        *state.timer.borrow_mut() = Some(timer.clone());
        run_loop.add_timer(Some(&timer), modes);

        if let Some(IoSource::Fd(fd)) = io {
            let context = CFFileDescriptorContext {
                version: 0,
                info: info(state),
                retain: None,
                release: None,
                copyDescription: None,
            };
            // fdはdriverのものなので、無効にするときに閉じさせない
            // SAFETY: `context`は正しい形で、fdはこの関数を出る前に無効にする
            let watch = unsafe { CFFileDescriptor::new(None, fd, false, Some(readable), &context) }
                .ok_or_else(|| io::Error::other("CFFileDescriptorCreate failed"))?;
            *state.fd.borrow_mut() = Some(watch.clone());
            watch.enable_call_backs(kCFFileDescriptorReadCallBack);
            let source = CFFileDescriptor::new_run_loop_source(None, Some(&watch), 0)
                .ok_or_else(|| io::Error::other("CFFileDescriptorCreateRunLoopSource failed"))?;
            run_loop.add_source(Some(&source), modes);
            scheduled.fd_source = Some(source);
        }

        let mut observer_context = CFRunLoopObserverContext {
            version: 0,
            info: info(state),
            retain: None,
            release: None,
            copyDescription: None,
        };
        // SAFETY: `context`は正しい形で、observerはこの関数を出る前に無効にする
        let observer = unsafe {
            CFRunLoopObserver::new(
                None,
                CFRunLoopActivity::BeforeWaiting.0,
                true,
                0 as CFIndex,
                Some(before_waiting),
                &mut observer_context,
            )
        }
        .ok_or_else(|| io::Error::other("CFRunLoopObserverCreate failed"))?;
        run_loop.add_observer(Some(&observer), modes);
        scheduled.observer = Some(observer);
        run_loop.add_source(Some(&self.bell.source), modes);
        scheduled.bell = Some(&self.bell.source);

        // 最初の`step(Woken)`は、それより前に鳴った起こしもまとめて扱う
        self.bell.ring();
        state.app.run();

        drop(scheduled);
        if let Some(payload) = state.panic.take() {
            panic::resume_unwind(payload);
        }
        Ok(())
    }

    fn waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let bell = Arc::clone(&self.bell);
        Arc::new(move || bell.ring())
    }
}

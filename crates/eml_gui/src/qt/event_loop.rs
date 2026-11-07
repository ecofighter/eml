//! `QCoreApplication::processEvents`を呼ぶループ。Qtのcallbackは`EventQueue`に積むか印を立てるだけで、`step`は
//! `processEvents`が戻った後にこのループが呼ぶ。起こしはdispatcherの`wakeUp`、I/Oは`QSocketNotifier`、時間切れは
//! 1回だけの`QTimer`である (docs/implementation/architecture.mdの「`eml_gui`の内部」)。

use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::{Qt, ffi};
use crate::{EventLoop, EventQueue, IoSource, Reason, Step};

/// どのスレッドからも鳴らせる起こし。
pub(super) struct Bell {
    rang: AtomicBool,
    /// メインのスレッドの`QAbstractEventDispatcher`。QApplicationを壊す前に0にする。
    dispatcher: Mutex<usize>,
}

impl Bell {
    pub(super) fn new(dispatcher: usize) -> Bell {
        Bell {
            rang: AtomicBool::new(false),
            dispatcher: Mutex::new(dispatcher),
        }
    }

    fn ring(&self) {
        self.rang.store(true, Ordering::Release);
        let dispatcher = self
            .dispatcher
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *dispatcher != 0 {
            // SAFETY: 0でないdispatcherは生きている。壊す前に`forget`が同じ錠の下で0にする。`wakeUp`はどのスレッドから
            // 呼んでもよい
            unsafe { ffi::wake(*dispatcher) };
        }
    }

    fn take(&self) -> bool {
        self.rang.swap(false, Ordering::Acquire)
    }

    pub(super) fn forget(&self) {
        *self
            .dispatcher
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = 0;
    }
}

pub(super) struct QtLoop {
    qt: Rc<Qt>,
    queue: EventQueue,
}

impl QtLoop {
    pub(super) fn new(qt: Rc<Qt>, queue: EventQueue) -> QtLoop {
        QtLoop { qt, queue }
    }
}

/// `run_loop`が置いたI/Oの見張りと時計を、どの出口でも (`Stop`、`step`のpanic) 外す。
struct Watching<'a>(&'a ffi::Shim);

impl Drop for Watching<'_> {
    fn drop(&mut self) {
        self.0.unwatch();
    }
}

/// 時計に置く期限。期限がなければ-1で、時計を止める。早く鳴って空回りしないように、ミリ秒に切り上げる。
fn millis(timeout: Option<Duration>) -> i64 {
    timeout.map_or(-1, |t| {
        let ms = t.as_nanos().div_ceil(1_000_000);
        i64::try_from(ms)
            .unwrap_or(i64::MAX)
            .min(i64::from(i32::MAX))
    })
}

impl EventLoop for QtLoop {
    fn run_loop(
        &mut self,
        io: Option<IoSource>,
        _timeout: Option<Duration>,
        step: &mut dyn FnMut(Reason) -> Step,
    ) -> io::Result<()> {
        let shim = self.qt.shim();
        let bell = &self.qt.bell;
        let _watching = Watching(shim);
        if let Some(IoSource::Fd(fd)) = io {
            shim.watch(fd);
        }
        // 最初の`step(Woken)`は、それより前に鳴った起こしもまとめて扱う
        bell.take();
        let mut reason = Reason::Woken;
        loop {
            // `step`が列を見るので、それより前に積まれた分の印は消す
            self.queue.take_pushed();
            match step(reason) {
                Step::Stop => return Ok(()),
                Step::Continue(timeout) => shim.arm(millis(timeout)),
            }
            // `step`の最中に積まれたイベント (表示したウィンドウの大きさの知らせなど) は、関係のない起こしを待たずに扱う
            let mut pushed = self.queue.take_pushed();
            reason = loop {
                shim.process(!pushed);
                pushed |= self.queue.take_pushed();
                // `step`は1回ごとにVMを1回動かし、それが起こし、I/O、期限をすべて見るので、立った印はまとめて1回の
                // `step`で扱う
                let rang = bell.take();
                let ready = shim.take_io();
                let timed_out = shim.take_timeout();
                if rang {
                    break Reason::Woken;
                } else if pushed {
                    break Reason::Events;
                } else if ready {
                    break Reason::Io;
                } else if timed_out {
                    break Reason::Timeout;
                }
            };
        }
    }

    fn waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let bell = Arc::clone(&self.qt.bell);
        Arc::new(move || bell.ring())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millis_rounds_up_and_fits_a_qt_timer() {
        assert_eq!(millis(None), -1);
        assert_eq!(millis(Some(Duration::ZERO)), 0);
        assert_eq!(millis(Some(Duration::from_micros(1))), 1);
        assert_eq!(millis(Some(Duration::from_millis(10))), 10);
        assert_eq!(millis(Some(Duration::MAX)), i64::from(i32::MAX));
    }
}

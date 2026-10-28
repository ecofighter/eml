//! 本物の driver。単調な時計 `Instant` で読み、スレッドを止めて待つ。I/O の操作はまだ扱わず、受け付けた操作は
//! `Unsupported` の誤りで終わる (docs/spec/vm.md の「時計と driver」)。

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eml_rt::{Completion, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey};

/// 本物の driver が1回の `wait` で眠る長さの上限。空きのループは起きるたびに止める語を読むので、Ctrl-C と確保量の
/// 上限はどの方針でもこの長さのうちに眠る worker を止める (docs/spec/vm.md の「時計と driver」)。
pub const IDLE_TICK: Duration = Duration::from_millis(10);

/// 本物の時計で待つ driver。
pub struct RealDriver {
    epoch: Instant,
    next_key: OpKey,
    /// 受け付けたが扱えない操作。次の `completions` で誤りとして返す。
    unsupported: Vec<OpKey>,
    bin: Arc<HandleBin>,
    waker: Arc<Flag>,
}

impl RealDriver {
    pub fn new() -> io::Result<RealDriver> {
        Ok(RealDriver {
            epoch: Instant::now(),
            next_key: 0,
            unsupported: Vec::new(),
            bin: Arc::default(),
            waker: Arc::default(),
        })
    }

    /// 落とされたハンドルを捨てる。この driver はハンドルを作らないので、閉じるものはない。
    fn empty_bin(&self) {
        self.bin
            .dropped
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

impl IoDriver for RealDriver {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.empty_bin();
        if !self.unsupported.is_empty() || self.waker.take() {
            return Ok(true);
        }
        let Some(deadline_ms) = deadline_ms else {
            // 終わる操作も期限もない。今は waker を鳴らす者もいない
            return Ok(false);
        };
        let now = self.now();
        let target = Duration::from_millis(deadline_ms);
        if now < target {
            std::thread::sleep((target - now).min(IDLE_TICK));
        }
        Ok(true)
    }

    fn submit(&mut self, op: Op) -> OpKey {
        drop(op);
        let key = self.next_key;
        self.next_key += 1;
        self.unsupported.push(key);
        key
    }

    fn cancel(&mut self, _key: OpKey) {}

    fn completions(&mut self, out: &mut Vec<Completion>) {
        self.empty_bin();
        out.extend(self.unsupported.drain(..).map(|key| Completion {
            key,
            result: Err(io::Error::from(io::ErrorKind::Unsupported)),
        }));
    }

    fn pending(&self) -> usize {
        self.unsupported.len()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        Arc::clone(&self.waker) as Arc<dyn IoWaker>
    }

    fn bin(&self) -> Arc<HandleBin> {
        Arc::clone(&self.bin)
    }

    fn pair(&mut self) -> io::Result<(IoHandle, IoHandle)> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn close(&mut self, _handle: IoHandle) {}

    fn quiesce(&mut self) {
        self.unsupported.clear();
        self.empty_bin();
    }
}

/// 鳴ったことだけを覚える waker。眠るスレッドは起こさず、次の `wait` がすぐに戻る (docs/spec/vm.md の「時計と driver」)。
#[derive(Default)]
pub(crate) struct Flag {
    rung: AtomicBool,
}

impl Flag {
    /// 鳴っていたかを返し、旗を下ろす。
    pub(crate) fn take(&self) -> bool {
        self.rung.swap(false, Ordering::Relaxed)
    }
}

impl IoWaker for Flag {
    fn wake(&self) {
        self.rung.store(true, Ordering::Relaxed);
    }
}

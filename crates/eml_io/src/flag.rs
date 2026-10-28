//! 2つの driver が共有する、旗だけの waker。

use std::sync::atomic::{AtomicBool, Ordering};

use eml_rt::IoWaker;

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

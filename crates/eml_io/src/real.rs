//! 本物の driver。OS を呼ばない核を、OS ごとの backend (Linux は epoll、Apple は kqueue、Windows は IOCP) の上で
//! 動かす (docs/spec/vm.md の「時計と driver」、docs/implementation/architecture.md の「`eml_io` の内部」)。
//!
//! 貸された記憶の番地を読むのは backend の OS を呼ぶ関数だけで、driver は slice を作らない。

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::{Completion, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, Submit};

use crate::driver::Driver;
use crate::sys;

pub struct RealDriver(Driver<sys::Os>);

impl RealDriver {
    pub fn new() -> io::Result<RealDriver> {
        RealDriver::with_lookup(sys::lookup)
    }

    /// 名前の解決に OS の解決器の代わりに `lookup` を使う driver。テストが解決を止めたり番地を選んだりするのに使う。
    pub fn with_lookup(
        lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>,
    ) -> io::Result<RealDriver> {
        Ok(RealDriver(Driver::new(sys::Os::new()?, lookup)))
    }
}

impl IoDriver for RealDriver {
    fn now(&self) -> Duration {
        self.0.now()
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.0.wait(deadline_ms)
    }

    fn submit(&mut self, key: OpKey, op: Op) -> Submit {
        self.0.submit(key, op)
    }

    fn cancel(&mut self, key: OpKey) {
        self.0.cancel(key);
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        self.0.completions(out);
    }

    fn pending(&self) -> usize {
        self.0.pending()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        self.0.waker()
    }

    fn bin(&self) -> Arc<HandleBin> {
        self.0.bin()
    }

    fn shutdown_write(&mut self, handle: IoHandle) -> io::Result<()> {
        self.0.shutdown_write(handle)
    }

    fn set_nodelay(&mut self, handle: IoHandle, on: bool) -> io::Result<()> {
        self.0.set_nodelay(handle, on)
    }

    fn close(&mut self, handle: IoHandle) {
        self.0.close(handle);
    }

    fn quiesce(&mut self) {
        self.0.quiesce();
    }
}

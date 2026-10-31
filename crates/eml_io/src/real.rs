//! 本物の driver。OS を呼ばない核を、OS ごとの backend (`sys/`) の上で動かす (docs/spec/vm.md の「時計と driver」、
//! docs/implementation/architecture.md の「`eml_io` の内部」)。
//!
//! 貸された記憶の番地を読むのは backend の OS を呼ぶ関数だけで、driver は slice を作らない。

#[cfg(target_os = "linux")]
use std::ffi::OsStr;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::{Completion, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, Submit};

use crate::driver::Driver;
use crate::sys;

/// 本物の driver が使う backend。変数 `EML_IO_BACKEND` の値に対応する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IoBackend {
    /// OS の既定。Linux では epoll
    #[default]
    Auto,
    /// Linux の epoll
    Epoll,
    /// Linux の io_uring。歩ごとに epoll と同じ同期の試みを先に出す
    IoUring,
    /// Linux の io_uring。読めると分かっていない行では同期の試みを省く。計測のための腕
    IoUringDirect,
}

impl IoBackend {
    /// 変数 `EML_IO_BACKEND` から選ぶ。変数を読むのはここだけで、Linux のほかでは読まずに `Auto` を返す。
    pub fn from_env() -> io::Result<IoBackend> {
        #[cfg(target_os = "linux")]
        {
            IoBackend::parse(std::env::var_os("EML_IO_BACKEND").as_deref())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(IoBackend::Auto)
        }
    }

    /// 変数の値を読む。値のないときは `Auto` で、知らない値は誤りにする。
    #[cfg(target_os = "linux")]
    pub(crate) fn parse(v: Option<&OsStr>) -> io::Result<IoBackend> {
        let Some(v) = v else {
            return Ok(IoBackend::Auto);
        };
        match v.to_str() {
            Some("epoll") => Ok(IoBackend::Epoll),
            Some("io_uring") => Ok(IoBackend::IoUring),
            Some("io_uring_direct") => Ok(IoBackend::IoUringDirect),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "EML_IO_BACKEND must be epoll, io_uring or io_uring_direct, not {:?}",
                    v.to_string_lossy()
                ),
            )),
        }
    }
}

pub struct RealDriver(Driver<sys::Os>);

impl RealDriver {
    /// 根の構築子。backend の値と名前の解決を受け、環境を読まない。
    pub fn with_backend_and_lookup(
        backend: IoBackend,
        lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>,
    ) -> io::Result<RealDriver> {
        Ok(RealDriver(Driver::new(sys::os(backend)?, lookup)))
    }

    /// 変数 `EML_IO_BACKEND` が選ぶ backend の driver。
    pub fn new() -> io::Result<RealDriver> {
        RealDriver::with_backend_and_lookup(IoBackend::from_env()?, sys::lookup)
    }

    /// 値で選んだ backend の driver。
    pub fn with_backend(backend: IoBackend) -> io::Result<RealDriver> {
        RealDriver::with_backend_and_lookup(backend, sys::lookup)
    }

    /// 名前の解決に OS の解決器の代わりに `lookup` を使う driver。テストが解決を止めたり番地を選んだりするのに使う。
    /// backend は `new` と同じく変数で選ぶ。
    pub fn with_lookup(
        lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>,
    ) -> io::Result<RealDriver> {
        RealDriver::with_backend_and_lookup(IoBackend::from_env()?, lookup)
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

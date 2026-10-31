//! Linux の2つの backend を並べる (docs/spec/vm.md の「時計と driver」)。driver を作るたびに、受けた値と ring の
//! 作り方だけで backend を決め、環境は読まない。以後の呼び出しは選んだ側へそのまま渡す。

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_rt::{IoWaker, OpKey};

use super::Readiness;
use super::unix::Sock;
use super::uring::{Arm, Uring};
use crate::driver::backend::{Backend, Cancel, Event, Start, Step};
use crate::real::IoBackend;

pub(crate) enum Linux {
    Epoll(Readiness),
    Uring(Uring),
}

impl Linux {
    /// `b` が名指す backend を作る。io_uring の値は `ring` で ring を作り、作れなければその誤りで driver を失敗させる。
    pub(crate) fn pick(
        b: IoBackend,
        ring: impl FnOnce(Arm) -> io::Result<Uring>,
    ) -> io::Result<Linux> {
        match b {
            IoBackend::Auto | IoBackend::Epoll => Ok(Linux::Epoll(Readiness::new()?)),
            IoBackend::IoUring => ring(Arm::TryFirst)
                .map(Linux::Uring)
                .map_err(|e| pinned(e, "io_uring")),
            IoBackend::IoUringDirect => ring(Arm::Direct)
                .map(Linux::Uring)
                .map_err(|e| pinned(e, "io_uring_direct")),
        }
    }
}

/// 変数で固めた io_uring を作れなかった誤り。種類はそのままにし、変数の値を添える。
fn pinned(e: io::Error, value: &str) -> io::Error {
    io::Error::new(
        e.kind(),
        format!("EML_IO_BACKEND is {value}, but io_uring cannot be used here: {e}"),
    )
}

/// 選んだ backend の同じ関数を呼ぶ。
macro_rules! each {
    ($self:expr, $b:ident => $call:expr) => {
        match $self {
            Linux::Epoll($b) => $call,
            Linux::Uring($b) => $call,
        }
    };
}

impl Backend for Linux {
    type Sock = Sock;

    fn now(&self) -> Duration {
        each!(self, b => b.now())
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        each!(self, b => b.waker())
    }

    fn spawn(
        &self,
        builder: thread::Builder,
        body: Box<dyn FnOnce() + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<()>> {
        each!(self, b => b.spawn(builder, body))
    }

    fn listen(&mut self, addr: SocketAddr) -> io::Result<(Sock, u16)> {
        each!(self, b => b.listen(addr))
    }

    fn socket(&mut self, peer: SocketAddr) -> io::Result<Sock> {
        each!(self, b => b.socket(peer))
    }

    fn shutdown_write(&mut self, sock: &mut Sock) -> io::Result<()> {
        each!(self, b => b.shutdown_write(sock))
    }

    fn set_nodelay(&mut self, sock: &mut Sock, on: bool) -> io::Result<()> {
        each!(self, b => b.set_nodelay(sock, on))
    }

    fn close(&mut self, sock: Sock) {
        each!(self, b => b.close(sock))
    }

    fn start(&mut self, key: OpKey, step: Step<'_, Sock>) -> Start<Sock> {
        each!(self, b => b.start(key, step))
    }

    fn cancel(&mut self, key: OpKey, step: Step<'_, Sock>) -> io::Result<Cancel> {
        each!(self, b => b.cancel(key, step))
    }

    fn wait(&mut self, timeout: Option<Duration>, events: &mut Vec<Event<Sock>>) -> io::Result<()> {
        each!(self, b => b.wait(timeout, events))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use super::*;

    #[test]
    fn parse_maps_each_value() {
        let parse = |v: Option<&str>| IoBackend::parse(v.map(OsStr::new));
        assert_eq!(parse(None).unwrap(), IoBackend::Auto);
        assert_eq!(parse(Some("epoll")).unwrap(), IoBackend::Epoll);
        assert_eq!(parse(Some("io_uring")).unwrap(), IoBackend::IoUring);
        assert_eq!(
            parse(Some("io_uring_direct")).unwrap(),
            IoBackend::IoUringDirect
        );
        for bad in ["bogus", "", "EPOLL", "io-uring"] {
            let e = parse(Some(bad)).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(
                e.to_string(),
                format!("EML_IO_BACKEND must be epoll, io_uring or io_uring_direct, not {bad:?}")
            );
        }
        let e = IoBackend::parse(Some(OsStr::from_bytes(b"\xff"))).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn pick_builds_the_named_backend() {
        for b in [IoBackend::Auto, IoBackend::Epoll] {
            let built = Linux::pick(b, |_| panic!("{b:?} built a ring"));
            assert!(matches!(built, Ok(Linux::Epoll(_))), "{b:?}");
        }
        if IoBackend::from_env().expect("EML_IO_BACKEND") == IoBackend::Epoll {
            eprintln!("EML_IO_BACKEND=epoll: ring tests do not run");
            return;
        }
        for (b, arm) in [
            (IoBackend::IoUring, Arm::TryFirst),
            (IoBackend::IoUringDirect, Arm::Direct),
        ] {
            match Linux::pick(b, Uring::new) {
                Ok(Linux::Uring(u)) => assert_eq!(u.arm(), arm),
                Ok(Linux::Epoll(_)) => panic!("{b:?} built epoll"),
                Err(e) => panic!(
                    "io_uring cannot be used here ({e}); set EML_IO_BACKEND=epoll to run the tests on epoll"
                ),
            }
        }
    }

    #[test]
    fn a_pinned_ring_that_cannot_be_built_keeps_the_kind() {
        let refused = |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "refused"));
        let Err(e) = Linux::pick(IoBackend::IoUring, refused) else {
            panic!("a refused ring built a backend");
        };
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            e.to_string()
                .starts_with("EML_IO_BACKEND is io_uring, but io_uring cannot be used here: "),
            "{e}"
        );
    }
}

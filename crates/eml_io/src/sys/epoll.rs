//! Linux の登録、待ち、起こし (docs/spec/vm.md の「時計と driver」)。ソケットは edge で1回だけ登録し、起こしは level
//! で登録した eventfd である。`epoll_ctl` を出すのは driver を作るときの eventfd と、ソケットごとの登録だけである。

#![allow(unsafe_code)]

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::ptr;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::IoWaker;
use libc::c_int;

use super::unix::{Poller, Ready, WAKE, syscall};

/// 1回の待ちで取り出す event の数。
const EVENTS: usize = 256;

/// 起こしの eventfd。level で登録するので、鳴らした印は次の待ちが読むまで残る。
pub(super) struct EventFd(OwnedFd);

impl IoWaker for EventFd {
    fn wake(&self) {
        let one: u64 = 1;
        // SAFETY: 局所の8バイトを読むだけである。数が上限なら既に鳴っているので、`EAGAIN` は捨ててよい
        let _ = syscall(|| unsafe {
            libc::write(
                self.0.as_raw_fd(),
                (&raw const one).cast(),
                size_of::<u64>(),
            )
        });
    }
}

impl EventFd {
    /// 起こしの eventfd を作る。
    pub(super) fn new() -> io::Result<Arc<EventFd>> {
        // SAFETY: 引数は整数である
        let fd = syscall(|| unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) })?;
        // SAFETY: `eventfd` が返したばかりの fd で、ほかに持ち主はない
        Ok(Arc::new(EventFd(unsafe { OwnedFd::from_raw_fd(fd) })))
    }

    /// 鳴らした印を消す。読むものがなければ `EAGAIN` で、それも捨ててよい。
    pub(super) fn drain(&self) {
        let mut count: u64 = 0;
        // SAFETY: 局所の8バイトに書くだけである
        let _ = syscall(|| unsafe {
            libc::read(
                self.0.as_raw_fd(),
                (&raw mut count).cast(),
                size_of::<u64>(),
            )
        });
    }
}

pub(crate) struct Epoll {
    ep: OwnedFd,
    wake: Arc<EventFd>,
    /// 取り出しの配列。待ちのたびに確保しないよう使い回す
    events: Vec<libc::epoll_event>,
}

impl Poller for Epoll {
    fn new() -> io::Result<(Self, Arc<dyn IoWaker>)> {
        // SAFETY: 引数は整数である
        let ep = syscall(|| unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) })?;
        // SAFETY: `epoll_create1` が返したばかりの fd で、ほかに持ち主はない
        let ep = unsafe { OwnedFd::from_raw_fd(ep) };
        let wake = EventFd::new()?;
        let mut event = libc::epoll_event {
            events: libc::EPOLLIN as u32,
            u64: WAKE,
        };
        // SAFETY: `event` は局所の値で、カーネルは読むだけである
        syscall(|| unsafe {
            libc::epoll_ctl(
                ep.as_raw_fd(),
                libc::EPOLL_CTL_ADD,
                wake.0.as_raw_fd(),
                &raw mut event,
            )
        })?;
        let poller = Epoll {
            ep,
            wake: Arc::clone(&wake),
            events: vec![libc::epoll_event { events: 0, u64: 0 }; EVENTS],
        };
        Ok((poller, wake))
    }

    fn register(&mut self, fd: BorrowedFd<'_>, token: u64, write: bool) -> io::Result<()> {
        let mut flags = libc::EPOLLIN | libc::EPOLLRDHUP | libc::EPOLLET;
        if write {
            flags |= libc::EPOLLOUT;
        }
        let mut event = libc::epoll_event {
            events: flags as u32,
            u64: token,
        };
        // SAFETY: `event` は局所の値で、カーネルは読むだけである
        syscall(|| unsafe {
            libc::epoll_ctl(
                self.ep.as_raw_fd(),
                libc::EPOLL_CTL_ADD,
                fd.as_raw_fd(),
                &raw mut event,
            )
        })?;
        Ok(())
    }

    fn poll(
        &mut self,
        timeout: Option<Duration>,
        mut deliver: impl FnMut(u64, Ready),
    ) -> io::Result<()> {
        // 切り上げるので期限より早くは戻らない。上限で切った待ちは早く戻るだけである
        let ms = timeout.map_or(-1, |d| super::millis(d, i32::MAX as u32) as c_int);
        // SAFETY: `events` は `EVENTS` 個の要素を持ち、カーネルはその数までしか書かない。sigmask が null なので、
        // シグナルの塞ぎは変えない
        let n = unsafe {
            libc::epoll_pwait(
                self.ep.as_raw_fd(),
                self.events.as_mut_ptr(),
                EVENTS as c_int,
                ms,
                ptr::null(),
            )
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            // 割り込みは VM が確かめ直すだけである (docs/spec/vm.md の「上限と割り込み」)
            return if e.kind() == io::ErrorKind::Interrupted {
                Ok(())
            } else {
                Err(e)
            };
        }
        for i in 0..n as usize {
            // x86_64 の `epoll_event` は packed なので、欄の参照を取らずに値で写す
            let event = self.events[i];
            let (token, flags) = (event.u64, event.events as c_int);
            if token == WAKE {
                self.wake.drain();
                continue;
            }
            let read = libc::EPOLLIN | libc::EPOLLRDHUP | libc::EPOLLHUP | libc::EPOLLERR;
            let write = libc::EPOLLOUT | libc::EPOLLHUP | libc::EPOLLERR;
            // epoll は取り出すときに状態を調べ直すので、読みの知らせは取り出した時点で読める
            let read = flags & read != 0;
            deliver(
                token,
                Ready {
                    read,
                    write: flags & write != 0,
                    fresh: read,
                },
            );
        }
        Ok(())
    }
}

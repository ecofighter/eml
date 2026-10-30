//! Apple の登録、待ち、起こし (docs/spec/vm.md の「時計と driver」)。ソケットは `EV_CLEAR` の filter で1回だけ登録し、
//! 起こしは `EVFILT_USER` の `NOTE_TRIGGER` である。

#![allow(unsafe_code)]

use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::ptr;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::IoWaker;
use libc::c_int;

use super::unix::{Poller, Ready, WAKE, syscall};

/// 1回の待ちで取り出す event の数。
const EVENTS: usize = 256;

// 識別子を `udata` のポインタの幅に詰めるので、64 ビットに限る
const _: () = assert!(usize::BITS == 64);

fn change(ident: usize, filter: i16, flags: u16, fflags: u32, token: u64) -> libc::kevent {
    libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data: 0,
        udata: ptr::without_provenance_mut(token as usize),
    }
}

/// 変更だけを出し、取り出さない。`EV_ADD` も `NOTE_TRIGGER` も2回出して同じ結果になるので、割り込まれたら
/// 出し直す。
fn apply(kq: BorrowedFd<'_>, changes: &[libc::kevent]) -> io::Result<()> {
    let zero = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `changes` はその長さだけ読まれ、受けの配列は渡さない。`zero` は局所の値である
    syscall(|| unsafe {
        libc::kevent(
            kq.as_raw_fd(),
            changes.as_ptr(),
            changes.len() as c_int,
            ptr::null_mut(),
            0,
            &zero,
        )
    })?;
    Ok(())
}

/// 起こし。kqueue の fd を backend と共に持つので、driver が先に落ちても鳴らす先は生きている。
struct User(Arc<OwnedFd>);

impl IoWaker for User {
    fn wake(&self) {
        // 印は次の取り出しで `EV_CLEAR` により消えるので、読みの syscall は要らない
        let _ = apply(
            self.0.as_fd(),
            &[change(0, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER, WAKE)],
        );
    }
}

pub(crate) struct Kqueue {
    kq: Arc<OwnedFd>,
    /// 取り出しの配列。待ちのたびに確保しないよう使い回す
    events: Vec<libc::kevent>,
}

impl Poller for Kqueue {
    fn new() -> io::Result<(Self, Arc<dyn IoWaker>)> {
        // SAFETY: 引数はない
        let kq = syscall(|| unsafe { libc::kqueue() })?;
        // SAFETY: `kqueue` が返したばかりの fd で、ほかに持ち主はない
        let kq = Arc::new(unsafe { OwnedFd::from_raw_fd(kq) });
        // SAFETY: 引数はどれも整数である
        syscall(|| unsafe { libc::fcntl(kq.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) })?;
        apply(
            kq.as_fd(),
            &[change(
                0,
                libc::EVFILT_USER,
                libc::EV_ADD | libc::EV_CLEAR,
                0,
                WAKE,
            )],
        )?;
        let poller = Kqueue {
            kq: Arc::clone(&kq),
            events: vec![change(0, 0, 0, 0, 0); EVENTS],
        };
        Ok((poller, Arc::new(User(kq))))
    }

    fn register(&mut self, fd: BorrowedFd<'_>, token: u64, write: bool) -> io::Result<()> {
        let ident = fd.as_raw_fd() as usize;
        let flags = libc::EV_ADD | libc::EV_CLEAR;
        let changes = [
            change(ident, libc::EVFILT_READ, flags, 0, token),
            change(ident, libc::EVFILT_WRITE, flags, 0, token),
        ];
        let n = if write { 2 } else { 1 };
        apply(self.kq.as_fd(), &changes[..n])
    }

    fn poll(
        &mut self,
        timeout: Option<Duration>,
        mut deliver: impl FnMut(u64, Ready),
    ) -> io::Result<()> {
        // 秒を `i32::MAX` で切るのは、カーネルがナノ秒に直すときの桁あふれを避けるためで、早く戻る向きだけである
        let ts = timeout.map(|d| libc::timespec {
            tv_sec: d.as_secs().min(i32::MAX as u64) as libc::time_t,
            tv_nsec: libc::c_long::from(d.subsec_nanos()),
        });
        let ts_ptr = ts.as_ref().map_or(ptr::null(), ptr::from_ref);
        // SAFETY: `events` は `EVENTS` 個の要素を持ち、カーネルはその数までしか書かない。`ts` は局所の値である
        let n = unsafe {
            libc::kevent(
                self.kq.as_raw_fd(),
                ptr::null(),
                0,
                self.events.as_mut_ptr(),
                EVENTS as c_int,
                ts_ptr,
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
        for event in &self.events[..n as usize] {
            let token = event.udata.addr() as u64;
            if token == WAKE {
                continue;
            }
            // 各 filter は自分の向きだけを起こす。`EVFILT_WRITE` の `EV_EOF` は自分の `shutdown` でも立つので、
            // 読みを立てない
            let ready = match event.filter {
                libc::EVFILT_READ => Ready {
                    read: true,
                    write: false,
                    fresh: true,
                },
                libc::EVFILT_WRITE => Ready {
                    read: false,
                    write: true,
                    fresh: false,
                },
                _ => continue,
            };
            deliver(token, ready);
        }
        Ok(())
    }
}

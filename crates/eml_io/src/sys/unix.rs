//! epoll と kqueue に共通の backend (docs/spec/vm.md の「時計と driver」)。fd ごとの状態 (読みの記憶、向きごとの
//! 待ち手、接続中の印、世代) を行の表に持ち、`start` 1回で歩の1回の試みだけを OS に出す。ソケットはどれも非ブロックで
//! 作り、1回だけ edge で登録して以後変えない。OS ごとのファイルは `Poller` の登録、待ち、起こしだけを持つ。

#![allow(unsafe_code)]

use std::io;
use std::mem::{self, MaybeUninit};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use eml_rt::{IoWaker, OpKey, RawBuf};
use libc::c_int;

use crate::driver::backend::{Arrival, Backend, Cancel, Event, Hold, Outcome, Start, Step};

/// 起こしの識別子。行 `u32::MAX` は作らないので、どの行の識別子とも重ならない。
pub(super) const WAKE: u64 = u64::MAX;

pub(super) const BACKLOG: c_int = 1024;

// ソケットは作るときに非ブロックにするが、旗でも重ねて守る。旗は syscall を増やさない
pub(super) const RECV: c_int = libc::MSG_DONTWAIT;
pub(super) const PEEK: c_int = libc::MSG_PEEK | libc::MSG_DONTWAIT;
#[cfg(target_os = "linux")]
pub(super) const SEND: c_int = libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT;
// Apple はソケットに SO_NOSIGPIPE を立てる (docs/spec/vm.md の「時計と driver」)
#[cfg(target_vendor = "apple")]
pub(super) const SEND: c_int = libc::MSG_DONTWAIT;

/// 1つの event が起こす向き。`fresh` は、読みの知らせが取り出した時点で読めることを poller が保証するとき真である。
#[derive(Clone, Copy)]
pub(crate) struct Ready {
    pub(crate) read: bool,
    pub(crate) write: bool,
    pub(crate) fresh: bool,
}

/// OS ごとの登録、待ち、起こし。`pub(super)` にすると `Unix` の実装の境界が `private_bounds` に当たるので
/// `pub(crate)` にする。`mod unix` が private なので外へは出ない。
pub(crate) trait Poller: Sized {
    /// poller と、鳴らす OS の物体を自ら持つ waker。waker の登録もここで済ませる。
    fn new() -> io::Result<(Self, Arc<dyn IoWaker>)>;
    /// `fd` を edge で1回だけ登録する。`write` が偽ならリスナーで、読みの関心だけを登録する。
    fn register(&mut self, fd: BorrowedFd<'_>, token: u64, write: bool) -> io::Result<()>;
    /// `timeout` まで待ち、届いた event を `deliver` に渡す。起こしの印はここで消費し、渡さない。時間切れ、起こし、
    /// 割り込みは `Ok(())` である。
    fn poll(
        &mut self,
        timeout: Option<Duration>,
        deliver: impl FnMut(u64, Ready),
    ) -> io::Result<()>;
}

/// 核が持つソケット。核は中を見ず、`fd` を閉じるのは `Backend::close` だけである。
pub(crate) struct Sock {
    pub(super) fd: OwnedFd,
    pub(super) row: u32,
    pub(super) generation: u32,
}

#[derive(Default)]
struct Row {
    generation: u32,
    /// 読みの向き (`Peek`、`Recv`、`Accept`) の待ち手。1つの向きに保留は1つなので列を持たない
    read: Option<OpKey>,
    /// 書きの向き (`Send`、`Connect`) の待ち手
    write: Option<OpKey>,
    /// 読みの記憶。event で立ち、読みの向きの `WouldBlock` でだけ下りる
    ready: bool,
    /// 読めると保証された event の後、まだ読みの向きに触れていない
    fresh: bool,
    /// `connect` を出して、結果をまだ読んでいない
    connecting: bool,
}

#[derive(Default)]
struct Rows {
    rows: Vec<Row>,
    free: Vec<u32>,
}

/// OS に渡す識別子を作る唯一の所。世代を含むので、閉じた行の古い event は新しい行に当たらない。
fn token(row: u32, generation: u32) -> u64 {
    (u64::from(generation) << 32) | u64::from(row)
}

fn untoken(token: u64) -> (u32, u32) {
    (token as u32, (token >> 32) as u32)
}

impl Rows {
    /// 新しい行。登録がその時点の状態を event にするので、記憶は下ろしたまま始めてよい。
    fn adopt(&mut self) -> (u32, u32) {
        if let Some(row) = self.free.pop() {
            return (row, self.rows[row as usize].generation);
        }
        let row = u32::try_from(self.rows.len()).expect("too many sockets");
        assert!(row != u32::MAX, "too many sockets");
        self.rows.push(Row::default());
        (row, 0)
    }

    /// 行を放す。登録は fd を閉じれば消え、残った event は世代が合わずに捨てられるので、登録を外す syscall は出さない。
    fn release(&mut self, row: u32, generation: u32) {
        let r = &mut self.rows[row as usize];
        debug_assert_eq!(r.generation, generation);
        *r = Row {
            generation: generation.wrapping_add(1),
            ..Row::default()
        };
        self.free.push(row);
    }

    fn of(&mut self, sock: &Sock) -> &mut Row {
        let r = &mut self.rows[sock.row as usize];
        debug_assert_eq!(r.generation, sock.generation);
        r
    }

    /// 1つの event を行に写し、その向きの待ち手に `Retry` を出す。待ち手は忘れない。
    fn deliver(&mut self, token: u64, ready: Ready, events: &mut Vec<Event<Sock>>) {
        let (row, generation) = untoken(token);
        let Some(r) = self.rows.get_mut(row as usize) else {
            return;
        };
        if r.generation != generation {
            return;
        }
        if ready.read {
            r.ready = true;
            r.fresh = ready.fresh;
            if let Some(key) = r.read {
                events.push(Event {
                    key,
                    arrival: Arrival::Retry,
                });
            }
        }
        if ready.write
            && let Some(key) = r.write
        {
            events.push(Event {
                key,
                arrival: Arrival::Retry,
            });
        }
    }
}

/// `f` を呼び、`EINTR` のときだけその場で繰り返す。backend が試し直すのはここだけで、ほかの続きは核が出す。
pub(super) fn syscall<T: Copy + PartialEq + From<i8>>(mut f: impl FnMut() -> T) -> io::Result<T> {
    loop {
        let r = f();
        if r != T::from(-1) {
            return Ok(r);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

pub(super) fn would_block(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::WouldBlock
}

fn pending() -> Start<Sock> {
    Start::Pending(Hold::None)
}

/// 読みの向きの1回の試みの結果を行に写す。
fn read_result(r: &mut Row, key: OpKey, result: io::Result<Outcome<Sock>>) -> Start<Sock> {
    match result {
        Err(e) if would_block(&e) => {
            r.ready = false;
            r.read = Some(key);
            pending()
        }
        result => {
            r.read = None;
            Start::Done(result)
        }
    }
}

/// 書きの向きの1回の試みの結果を行に写す。書きの記憶は持たない。
fn write_result(r: &mut Row, key: OpKey, result: io::Result<Outcome<Sock>>) -> Start<Sock> {
    match result {
        Err(e) if would_block(&e) => {
            r.write = Some(key);
            pending()
        }
        result => {
            r.write = None;
            Start::Done(result)
        }
    }
}

/// 貸された記憶へ受ける。`RawBuf` のポインタを読むのはこの関数と `send_raw` だけで、slice は作らない。
pub(super) fn recv_raw(fd: BorrowedFd<'_>, buf: RawBuf, flags: c_int) -> io::Result<usize> {
    // SAFETY: `RawBuf` の約束 (docs/spec/vm.md の「時計と driver」) により、`start` の間 `buf` は `cap` バイトの一意の
    // 記憶を指す。カーネルは `cap` バイトまでしか書かず、呼び出しは同期なので、戻った後にポインタを持たない
    let n = syscall(|| unsafe { libc::recv(fd.as_raw_fd(), buf.ptr.cast(), buf.cap, flags) })?;
    Ok(n as usize)
}

/// 貸された記憶から送る。
pub(super) fn send_raw(fd: BorrowedFd<'_>, buf: RawBuf, flags: c_int) -> io::Result<usize> {
    // SAFETY: `RawBuf` の約束により、`start` の間 `buf` は `cap` バイトの読める記憶を指す。カーネルは読むだけで、
    // 呼び出しは同期である
    let n = syscall(|| unsafe { libc::send(fd.as_raw_fd(), buf.ptr.cast(), buf.cap, flags) })?;
    Ok(n as usize)
}

pub(super) fn setsockopt_int(
    fd: BorrowedFd<'_>,
    level: c_int,
    name: c_int,
    value: c_int,
) -> io::Result<()> {
    // SAFETY: 値は局所の `c_int` で、その大きさを渡す。カーネルは読むだけである
    syscall(|| unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            level,
            name,
            (&raw const value).cast(),
            size_of::<c_int>() as libc::socklen_t,
        )
    })?;
    Ok(())
}

fn getsockopt_int(fd: BorrowedFd<'_>, level: c_int, name: c_int) -> io::Result<c_int> {
    let mut value: c_int = 0;
    let mut len = size_of::<c_int>() as libc::socklen_t;
    // SAFETY: 書き先は局所の `c_int` で、`len` がその大きさを渡す
    syscall(|| unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            level,
            name,
            (&raw mut value).cast(),
            &mut len,
        )
    })?;
    Ok(value)
}

/// `getsockname` か `getpeername` で読んだ番地。IP の番地でなければ `None`。
pub(super) fn name_of(fd: BorrowedFd<'_>, peer: bool) -> io::Result<Option<SocketAddr>> {
    // SAFETY: `sockaddr_storage` は整数と配列だけの C の構造体で、すべて 0 は正しい値である
    let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
    let mut len = size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let call = if peer {
        libc::getpeername
    } else {
        libc::getsockname
    };
    // SAFETY: 書き先は局所の `sockaddr_storage` で、`len` がその大きさを渡す
    syscall(|| unsafe { call(fd.as_raw_fd(), (&raw mut storage).cast(), &mut len) })?;
    Ok(from_storage(&storage, len))
}

/// ブロックせず、exec で閉じる TCP のソケット。
#[cfg(target_os = "linux")]
pub(super) fn stream_socket(addr: &SocketAddr) -> io::Result<OwnedFd> {
    let domain = if addr.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    let kind = libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC;
    // SAFETY: 引数はどれも整数である
    let fd = syscall(|| unsafe { libc::socket(domain, kind, libc::IPPROTO_TCP) })?;
    // SAFETY: `socket` が返したばかりの fd で、ほかに持ち主はない
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_vendor = "apple")]
pub(super) fn stream_socket(addr: &SocketAddr) -> io::Result<OwnedFd> {
    let domain = if addr.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    // SAFETY: 引数はどれも整数である
    let fd = syscall(|| unsafe { libc::socket(domain, libc::SOCK_STREAM, libc::IPPROTO_TCP) })?;
    // SAFETY: `socket` が返したばかりの fd で、ほかに持ち主はない。以後の失敗では `OwnedFd` の drop が閉じる
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    prepare(fd.as_fd())?;
    Ok(fd)
}

/// Apple には `SOCK_NONBLOCK` と `SOCK_CLOEXEC` がないので、作った後で立てる。`SO_NOSIGPIPE` で、閉じた相手への
/// 送りをシグナルでなく誤りにする。
#[cfg(target_vendor = "apple")]
fn prepare(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: 引数はどれも整数である
    syscall(|| unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) })?;
    let mut on: c_int = 1;
    // SAFETY: `FIONBIO` は局所の `c_int` を読むだけである
    syscall(|| unsafe { libc::ioctl(fd.as_raw_fd(), libc::FIONBIO, &raw mut on) })?;
    setsockopt_int(fd, libc::SOL_SOCKET, libc::SO_NOSIGPIPE, 1)
}

/// 待ち行列から1本受ける。受けたソケットはブロックせず、exec で閉じる。
#[cfg(target_os = "linux")]
pub(super) fn accept_raw(
    fd: BorrowedFd<'_>,
    storage: &mut libc::sockaddr_storage,
    len: &mut libc::socklen_t,
) -> io::Result<OwnedFd> {
    let flags = libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC;
    let storage: *mut libc::sockaddr_storage = storage;
    // SAFETY: 書き先は呼び手の `sockaddr_storage` で、`len` がその大きさを渡す
    let fd =
        syscall(|| unsafe { libc::accept4(fd.as_raw_fd(), storage.cast(), &mut *len, flags) })?;
    // SAFETY: `accept4` が返したばかりの fd で、ほかに持ち主はない
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_vendor = "apple")]
pub(super) fn accept_raw(
    fd: BorrowedFd<'_>,
    storage: &mut libc::sockaddr_storage,
    len: &mut libc::socklen_t,
) -> io::Result<OwnedFd> {
    let storage: *mut libc::sockaddr_storage = storage;
    // SAFETY: 書き先は呼び手の `sockaddr_storage` で、`len` がその大きさを渡す
    let fd = syscall(|| unsafe { libc::accept(fd.as_raw_fd(), storage.cast(), &mut *len) })?;
    // SAFETY: `accept` が返したばかりの fd で、ほかに持ち主はない。以後の失敗では `OwnedFd` の drop が閉じる
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // 受けた後の設定の失敗 (受ける前に切られた接続など) はその1本だけの誤りなので、受けの出し直しに乗せる
    prepare(fd.as_fd()).map_err(|e| io::Error::new(io::ErrorKind::ConnectionAborted, e))?;
    Ok(fd)
}

/// 来ていた接続1本だけの誤りで、accept(2) が試し直せと挙げるもの。核の受けの出し直しに乗せる。
#[cfg(target_os = "linux")]
pub(super) fn transient(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(
            libc::ENETDOWN
                | libc::EPROTO
                | libc::ENOPROTOOPT
                | libc::EHOSTDOWN
                | libc::ENONET
                | libc::EHOSTUNREACH
                | libc::EOPNOTSUPP
                | libc::ENETUNREACH
        )
    )
}

/// Apple の accept(2) はこの勧めを持たず、`ECONNABORTED` だけが一時の誤りである。
#[cfg(target_vendor = "apple")]
pub(super) fn transient(_: &io::Error) -> bool {
    false
}

/// C の番地の構造体。`as_ptr` の間だけ生かして OS に渡す。
pub(super) enum RawAddr {
    V4(libc::sockaddr_in),
    V6(libc::sockaddr_in6),
}

impl RawAddr {
    pub(super) fn new(addr: SocketAddr) -> RawAddr {
        match addr {
            SocketAddr::V4(a) => {
                // SAFETY: `sockaddr_in` は整数だけの C の構造体で、すべて 0 は正しい値である
                let mut s: libc::sockaddr_in = unsafe { mem::zeroed() };
                #[cfg(target_vendor = "apple")]
                {
                    s.sin_len = size_of::<libc::sockaddr_in>() as u8;
                }
                s.sin_family = libc::AF_INET as libc::sa_family_t;
                s.sin_port = a.port().to_be();
                s.sin_addr = libc::in_addr {
                    s_addr: u32::from_ne_bytes(a.ip().octets()),
                };
                RawAddr::V4(s)
            }
            SocketAddr::V6(a) => {
                // SAFETY: `sockaddr_in6` は整数と配列だけの C の構造体で、すべて 0 は正しい値である
                let mut s: libc::sockaddr_in6 = unsafe { mem::zeroed() };
                #[cfg(target_vendor = "apple")]
                {
                    s.sin6_len = size_of::<libc::sockaddr_in6>() as u8;
                }
                s.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                s.sin6_port = a.port().to_be();
                // std と同じく、flowinfo の並びは変えない
                s.sin6_flowinfo = a.flowinfo();
                s.sin6_addr = libc::in6_addr {
                    s6_addr: a.ip().octets(),
                };
                s.sin6_scope_id = a.scope_id();
                RawAddr::V6(s)
            }
        }
    }

    pub(super) fn as_ptr(&self) -> (*const libc::sockaddr, libc::socklen_t) {
        match self {
            RawAddr::V4(s) => (
                (s as *const libc::sockaddr_in).cast(),
                size_of::<libc::sockaddr_in>() as libc::socklen_t,
            ),
            RawAddr::V6(s) => (
                (s as *const libc::sockaddr_in6).cast(),
                size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            ),
        }
    }
}

pub(super) fn from_storage(s: &libc::sockaddr_storage, len: libc::socklen_t) -> Option<SocketAddr> {
    match c_int::from(s.ss_family) {
        libc::AF_INET if len as usize >= size_of::<libc::sockaddr_in>() => {
            // SAFETY: 族が AF_INET で長さが足りる。`sockaddr_storage` は `sockaddr_in` より大きく、整列も強い
            let a = unsafe { &*(s as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>() };
            Some(SocketAddr::from((
                Ipv4Addr::from(a.sin_addr.s_addr.to_ne_bytes()),
                u16::from_be(a.sin_port),
            )))
        }
        libc::AF_INET6 if len as usize >= size_of::<libc::sockaddr_in6>() => {
            // SAFETY: 族が AF_INET6 で長さが足りる。整列は上と同じ
            let a = unsafe { &*(s as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>() };
            Some(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(a.sin6_addr.s6_addr),
                u16::from_be(a.sin6_port),
                a.sin6_flowinfo,
                a.sin6_scope_id,
            )))
        }
        _ => None,
    }
}

pub(crate) struct Unix<P> {
    poller: P,
    /// poller が作った waker の写し
    waker: Arc<dyn IoWaker>,
    rows: Rows,
    epoch: Instant,
}

impl<P: Poller> Unix<P> {
    pub(crate) fn new() -> io::Result<Self> {
        let (poller, waker) = P::new()?;
        Ok(Unix {
            poller,
            waker,
            rows: Rows::default(),
            epoch: Instant::now(),
        })
    }

    /// ソケットを登録する唯一の所。`listen`、`Accept`、`Connect` が呼び、以後 `close` まで登録を変えない。
    fn register_row(
        &mut self,
        fd: BorrowedFd<'_>,
        row: u32,
        generation: u32,
        write: bool,
    ) -> io::Result<()> {
        self.poller.register(fd, token(row, generation), write)
    }

    fn peek(&mut self, key: OpKey, sock: &Sock) -> Start<Sock> {
        let r = self.rows.of(sock);
        if !r.ready {
            r.read = Some(key);
            return pending();
        }
        // 記憶で答えた後も鮮度を下ろす。RST の後の2回目の覗きが誤りを読めるようにするためである
        let fresh = mem::replace(&mut r.fresh, false);
        if fresh {
            r.read = None;
            return Start::Done(Ok(Outcome::Size(0)));
        }
        let mut byte = MaybeUninit::<u8>::uninit();
        let buf = RawBuf {
            ptr: byte.as_mut_ptr(),
            cap: 1,
        };
        // 1バイトか EOF が読めれば終わる。覗いた誤りは消費される
        let result = recv_raw(sock.fd.as_fd(), buf, PEEK).map(|_| Outcome::Size(0));
        read_result(self.rows.of(sock), key, result)
    }

    fn recv(&mut self, key: OpKey, sock: &Sock, buf: RawBuf) -> Start<Sock> {
        // 記憶によらず必ず試すので、データのある読みは1回の syscall で終わる
        self.rows.of(sock).fresh = false;
        let result = recv_raw(sock.fd.as_fd(), buf, RECV).map(Outcome::Size);
        read_result(self.rows.of(sock), key, result)
    }

    fn send(&mut self, key: OpKey, sock: &Sock, buf: RawBuf) -> Start<Sock> {
        let result = send_raw(sock.fd.as_fd(), buf, SEND).map(Outcome::Size);
        write_result(self.rows.of(sock), key, result)
    }

    fn accept(&mut self, key: OpKey, listener: &Sock) -> Start<Sock> {
        self.rows.of(listener).fresh = false;
        // SAFETY: `sockaddr_storage` は整数と配列だけの C の構造体で、すべて 0 は正しい値である
        let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
        let mut len = size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        // 記憶によらず必ず試すので、資源の誤りの後も待ち行列の接続は次の受けで受けられる
        let result = accept_raw(listener.fd.as_fd(), &mut storage, &mut len).map_err(|e| {
            if transient(&e) {
                io::Error::new(io::ErrorKind::ConnectionAborted, e)
            } else {
                e
            }
        });
        let fd = match result {
            Ok(fd) => {
                self.rows.of(listener).read = None;
                fd
            }
            Err(e) => return read_result(self.rows.of(listener), key, Err(e)),
        };
        let Some(peer) = from_storage(&storage, len) else {
            return Start::Done(Err(io::Error::other("the peer has no IP address")));
        };
        let (row, generation) = self.rows.adopt();
        if let Err(e) = self.register_row(fd.as_fd(), row, generation, true) {
            self.rows.release(row, generation);
            return Start::Done(Err(e));
        }
        Start::Done(Ok(Outcome::Accepted {
            sock: Sock {
                fd,
                row,
                generation,
            },
            peer,
        }))
    }

    fn connect(&mut self, key: OpKey, sock: &Sock, peer: SocketAddr) -> Start<Sock> {
        if !self.rows.of(sock).connecting {
            let addr = RawAddr::new(peer);
            let (ptr, len) = addr.as_ptr();
            // SAFETY: `ptr` は生きている `addr` を指し、`len` がその大きさである。カーネルは読むだけである
            let status = unsafe { libc::connect(sock.fd.as_raw_fd(), ptr, len) };
            // 割り込まれた接続は裏で続くので、進行中と読んで出し直さない
            let connected = if status == 0 {
                true
            } else {
                let e = io::Error::last_os_error();
                match e.raw_os_error() {
                    Some(libc::EINPROGRESS | libc::EINTR) => false,
                    _ => return Start::Done(Err(e)),
                }
            };
            // 前に登録すると余分な event が来るので、`connect` の直後に登録する
            if let Err(e) = self.register_row(sock.fd.as_fd(), sock.row, sock.generation, true) {
                return Start::Done(Err(e));
            }
            if connected {
                return Start::Done(Ok(Outcome::Connected));
            }
            let r = self.rows.of(sock);
            r.connecting = true;
            r.write = Some(key);
            return pending();
        }
        // `Retry` の後は `connect` を出し直さず、結果を読む
        let fd = sock.fd.as_fd();
        let result = match getsockopt_int(fd, libc::SOL_SOCKET, libc::SO_ERROR) {
            Err(e) => Some(Err(e)),
            Ok(0) => match name_of(fd, true) {
                Ok(_) => Some(Ok(Outcome::Connected)),
                // 早すぎた知らせ。まだつながっていない
                Err(e) if e.raw_os_error() == Some(libc::ENOTCONN) => None,
                Err(e) => Some(Err(e)),
            },
            Ok(err) => Some(Err(io::Error::from_raw_os_error(err))),
        };
        let r = self.rows.of(sock);
        match result {
            None => {
                r.write = Some(key);
                pending()
            }
            Some(result) => {
                r.connecting = false;
                r.write = None;
                Start::Done(result)
            }
        }
    }
}

impl<P: Poller> Backend for Unix<P> {
    type Sock = Sock;

    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        Arc::clone(&self.waker)
    }

    fn spawn(
        &self,
        builder: thread::Builder,
        body: Box<dyn FnOnce() + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<()>> {
        crate::mask::spawn_masked(builder, body)
    }

    fn listen(&mut self, addr: SocketAddr) -> io::Result<(Sock, u16)> {
        let fd = stream_socket(&addr)?;
        setsockopt_int(fd.as_fd(), libc::SOL_SOCKET, libc::SO_REUSEADDR, 1)?;
        let raw = RawAddr::new(addr);
        let (ptr, len) = raw.as_ptr();
        // SAFETY: `ptr` は生きている `raw` を指し、`len` がその大きさである
        syscall(|| unsafe { libc::bind(fd.as_raw_fd(), ptr, len) })?;
        // SAFETY: 引数はどれも整数である
        syscall(|| unsafe { libc::listen(fd.as_raw_fd(), BACKLOG) })?;
        let port = name_of(fd.as_fd(), false)?.map_or(addr.port(), |local| local.port());
        let (row, generation) = self.rows.adopt();
        if let Err(e) = self.register_row(fd.as_fd(), row, generation, false) {
            self.rows.release(row, generation);
            return Err(e);
        }
        Ok((
            Sock {
                fd,
                row,
                generation,
            },
            port,
        ))
    }

    fn socket(&mut self, peer: SocketAddr) -> io::Result<Sock> {
        let fd = stream_socket(&peer)?;
        let (row, generation) = self.rows.adopt();
        Ok(Sock {
            fd,
            row,
            generation,
        })
    }

    fn shutdown_write(&mut self, sock: &mut Sock) -> io::Result<()> {
        // SAFETY: 引数はどれも整数である
        syscall(|| unsafe { libc::shutdown(sock.fd.as_raw_fd(), libc::SHUT_WR) })?;
        Ok(())
    }

    fn set_nodelay(&mut self, sock: &mut Sock, on: bool) -> io::Result<()> {
        setsockopt_int(
            sock.fd.as_fd(),
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            c_int::from(on),
        )
    }

    fn close(&mut self, sock: Sock) {
        self.rows.release(sock.row, sock.generation);
        drop(sock.fd);
    }

    fn start(&mut self, key: OpKey, step: Step<'_, Sock>) -> Start<Sock> {
        match step {
            Step::Peek { sock } => self.peek(key, sock),
            Step::Recv { sock, buf } => self.recv(key, sock, buf),
            Step::Send { sock, buf } => self.send(key, sock, buf),
            Step::Accept { listener } => self.accept(key, listener),
            Step::Connect { sock, peer } => self.connect(key, sock, peer),
        }
    }

    fn cancel(&mut self, key: OpKey, step: Step<'_, Sock>) -> io::Result<Cancel> {
        let (sock, read) = match step {
            Step::Peek { sock } | Step::Recv { sock, .. } | Step::Accept { listener: sock } => {
                (sock, true)
            }
            Step::Send { sock, .. } | Step::Connect { sock, .. } => (sock, false),
        };
        let r = self.rows.of(sock);
        let waiter = if read { &mut r.read } else { &mut r.write };
        if *waiter == Some(key) {
            *waiter = None;
        }
        Ok(Cancel::Done)
    }

    fn wait(&mut self, timeout: Option<Duration>, events: &mut Vec<Event<Sock>>) -> io::Result<()> {
        let rows = &mut self.rows;
        self.poller
            .poll(timeout, |token, ready| rows.deliver(token, ready, events))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::TcpStream;
    use std::time::Duration;

    use super::*;

    type Os = crate::sys::Readiness;

    const LIMIT: Duration = Duration::from_secs(1);

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    fn backend() -> Os {
        Os::new().unwrap()
    }

    fn buf(bytes: &mut [u8]) -> RawBuf {
        RawBuf {
            ptr: bytes.as_mut_ptr(),
            cap: bytes.len(),
        }
    }

    /// `key` に `Retry` が届くまで待つ。1 秒で届かなければ落ちる。
    fn wait_for(backend: &mut Os, key: OpKey) {
        let start = Instant::now();
        let mut events = Vec::new();
        loop {
            let left = LIMIT
                .checked_sub(start.elapsed())
                .expect("no Retry arrived");
            backend.wait(Some(left), &mut events).unwrap();
            if events.drain(..).any(|e| e.key == key) {
                return;
            }
        }
    }

    fn done(start: Start<Sock>) -> Outcome<Sock> {
        match start {
            Start::Done(result) => result.unwrap(),
            Start::Pending(_) => panic!("the step is pending"),
        }
    }

    fn is_pending(start: &Start<Sock>) -> bool {
        matches!(start, Start::Pending(Hold::None))
    }

    /// リスナーと、それにつないだ std のストリームと、受けたソケット。
    fn pair(backend: &mut Os) -> (Sock, TcpStream, Sock) {
        let (mut listener, port) = backend.listen(addr("127.0.0.1:0")).unwrap();
        let peer = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let sock = accept(backend, &mut listener);
        (listener, peer, sock)
    }

    fn accept(backend: &mut Os, listener: &mut Sock) -> Sock {
        let key = 90;
        let mut start = backend.start(key, Step::Accept { listener });
        if is_pending(&start) {
            wait_for(backend, key);
            start = backend.start(key, Step::Accept { listener });
        }
        match done(start) {
            Outcome::Accepted { sock, .. } => sock,
            _ => panic!("not an accepted socket"),
        }
    }

    fn connect(backend: &mut Os, port: u16) -> Sock {
        let peer = addr(&format!("127.0.0.1:{port}"));
        let mut sock = backend.socket(peer).unwrap();
        let key = 91;
        let mut start = backend.start(
            key,
            Step::Connect {
                sock: &mut sock,
                peer,
            },
        );
        while is_pending(&start) {
            wait_for(backend, key);
            start = backend.start(
                key,
                Step::Connect {
                    sock: &mut sock,
                    peer,
                },
            );
        }
        assert!(matches!(done(start), Outcome::Connected));
        sock
    }

    fn row<'a>(backend: &'a mut Os, sock: &Sock) -> &'a Row {
        backend.rows.of(sock)
    }

    /// 相手の書いたデータが自分の側で読めるようになるまで待つ。event を取り出さずに確かめるために使う。
    fn wait_readable(sock: &Sock) {
        let start = Instant::now();
        loop {
            let mut n: c_int = 0;
            // SAFETY: `FIONREAD` は局所の `c_int` に書くだけである
            let status = unsafe { libc::ioctl(sock.fd.as_raw_fd(), libc::FIONREAD, &raw mut n) };
            assert_eq!(status, 0);
            if n > 0 {
                return;
            }
            assert!(start.elapsed() < LIMIT, "the data never arrived");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_token_keeps_its_row_and_generation() {
        for (row, generation) in [(0, 0), (1, 0), (0, 1), (u32::MAX - 1, u32::MAX), (7, 9)] {
            let t = token(row, generation);
            assert_eq!(untoken(t), (row, generation));
            assert_ne!(t, WAKE);
        }
    }

    #[test]
    fn an_event_of_a_released_row_is_dropped() {
        let mut rows = Rows::default();
        let (row, old) = rows.adopt();
        rows.rows[row as usize].read = Some(1);
        rows.release(row, old);
        let (again, new) = rows.adopt();
        assert_eq!(again, row);
        assert_ne!(new, old);
        rows.rows[row as usize].read = Some(2);
        rows.rows[row as usize].write = Some(3);
        let mut events = Vec::new();
        let ready = Ready {
            read: true,
            write: true,
            fresh: true,
        };
        rows.deliver(token(row, old), ready, &mut events);
        assert!(events.is_empty());
        assert!(!rows.rows[row as usize].ready);
        rows.deliver(token(row, new), ready, &mut events);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn a_probe_after_an_edge_answers_from_memory_once() {
        let mut backend = backend();
        let (_listener, mut peer, mut sock) = pair(&mut backend);
        let key = 1;
        assert!(is_pending(
            &backend.start(key, Step::Peek { sock: &mut sock })
        ));
        peer.write_all(b"ab").unwrap();
        wait_for(&mut backend, key);
        assert!(row(&mut backend, &sock).fresh);
        assert!(matches!(
            done(backend.start(key, Step::Peek { sock: &mut sock })),
            Outcome::Size(0)
        ));
        let r = row(&mut backend, &sock);
        assert!(!r.fresh && r.ready);
        assert!(matches!(
            done(backend.start(key, Step::Peek { sock: &mut sock })),
            Outcome::Size(0)
        ));
        let mut bytes = [0; 16];
        let read = done(backend.start(
            key,
            Step::Recv {
                sock: &mut sock,
                buf: buf(&mut bytes),
            },
        ));
        assert!(matches!(read, Outcome::Size(2)));
        assert!(is_pending(
            &backend.start(key, Step::Peek { sock: &mut sock })
        ));
        assert!(!row(&mut backend, &sock).ready);
    }

    #[test]
    fn a_drained_event_does_not_make_a_probe_fresh() {
        let mut backend = backend();
        let (_listener, mut peer, mut sock) = pair(&mut backend);
        let key = 1;
        let mut bytes = [0; 16];
        assert!(is_pending(&backend.start(
            key,
            Step::Recv {
                sock: &mut sock,
                buf: buf(&mut bytes),
            },
        )));
        peer.write_all(b"a").unwrap();
        wait_for(&mut backend, key);
        assert!(matches!(
            done(backend.start(
                key,
                Step::Recv {
                    sock: &mut sock,
                    buf: buf(&mut bytes),
                },
            )),
            Outcome::Size(1)
        ));
        assert!(row(&mut backend, &sock).ready);
        // 着いた event を取り出す前に読み尽くす
        peer.write_all(b"b").unwrap();
        wait_readable(&sock);
        assert!(matches!(
            done(backend.start(
                key,
                Step::Recv {
                    sock: &mut sock,
                    buf: buf(&mut bytes),
                },
            )),
            Outcome::Size(1)
        ));
        backend.wait(Some(Duration::ZERO), &mut Vec::new()).unwrap();
        assert!(!row(&mut backend, &sock).fresh);
        assert!(is_pending(
            &backend.start(key, Step::Peek { sock: &mut sock })
        ));
    }

    #[test]
    fn a_retry_without_news_keeps_a_read_pending() {
        let mut backend = backend();
        let (_listener, _peer, mut sock) = pair(&mut backend);
        let key = 1;
        let mut bytes = [0; 16];
        for _ in 0..2 {
            assert!(is_pending(&backend.start(
                key,
                Step::Recv {
                    sock: &mut sock,
                    buf: buf(&mut bytes),
                },
            )));
        }
        assert_eq!(row(&mut backend, &sock).read, Some(key));
    }

    #[test]
    fn sockets_are_close_on_exec_and_nonblocking() {
        let mut backend = backend();
        let (mut listener, port) = backend.listen(addr("127.0.0.1:0")).unwrap();
        let connected = connect(&mut backend, port);
        let accepted = accept(&mut backend, &mut listener);
        for sock in [&listener, &connected, &accepted] {
            let fd = sock.fd.as_raw_fd();
            // SAFETY: 引数はどれも整数である
            let (fd_flags, fl_flags) = unsafe {
                (
                    libc::fcntl(fd, libc::F_GETFD),
                    libc::fcntl(fd, libc::F_GETFL),
                )
            };
            assert!(fd_flags & libc::FD_CLOEXEC != 0);
            assert!(fl_flags & libc::O_NONBLOCK != 0);
        }
    }

    #[cfg(target_vendor = "apple")]
    #[test]
    fn an_accepted_socket_does_not_raise_sigpipe() {
        let mut backend = backend();
        let (_listener, _peer, sock) = pair(&mut backend);
        let on = getsockopt_int(sock.fd.as_fd(), libc::SOL_SOCKET, libc::SO_NOSIGPIPE).unwrap();
        assert_eq!(on, 1);
    }

    #[test]
    fn a_wake_is_kept_until_the_next_wait() {
        let mut backend = backend();
        backend.waker().wake();
        let mut events = Vec::new();
        backend
            .wait(Some(Duration::from_secs(5)), &mut events)
            .unwrap();
        assert!(events.is_empty());
        backend.wait(Some(Duration::ZERO), &mut events).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn a_socket_address_survives_the_round_trip() {
        let v6 = SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1),
            1,
            7,
            3,
        ));
        for a in [addr("127.0.0.1:80"), addr("[::1]:4242"), v6] {
            let raw = RawAddr::new(a);
            let (ptr, len) = raw.as_ptr();
            // SAFETY: `sockaddr_storage` は整数と配列だけの C の構造体で、すべて 0 は正しい値である
            let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
            // SAFETY: `ptr` は生きている `raw` の `len` バイトを指し、`sockaddr_storage` はどの番地より大きい
            unsafe {
                std::ptr::copy_nonoverlapping(
                    ptr.cast::<u8>(),
                    (&raw mut storage).cast::<u8>(),
                    len as usize,
                );
            }
            assert_eq!(from_storage(&storage, len), Some(a));
        }
    }
}

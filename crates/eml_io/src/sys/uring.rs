//! Linux の io_uring の backend (docs/spec/vm.md の「時計と driver」)。歩ごとに epoll と同じ同期の試みを先に出し、
//! 塞がれたときだけ ring に要求を積む。積んだ要求は次の `wait` の1回の `io_uring_enter` が出し、待ち、刈り取る。
//! カーネルが書くのは、貸された buffer と、op の表の箱に置いた番地だけである。io_uring の ABI は `libc` にないので、
//! ここで `libc::syscall` の上に定義する。

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::mem::{self, MaybeUninit};
use std::net::SocketAddr;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::process;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use eml_rt::{IoWaker, OpKey, RawBuf};
use libc::{c_int, c_long};

use super::epoll::EventFd;
use super::unix::{
    PEEK, RECV, RawAddr, SEND, Sock, accept_raw, from_storage, listener, recv_raw, send_raw,
    set_nodelay, shutdown_write, stream_socket, syscall, transient, would_block,
};
use crate::driver::backend::{Arrival, Backend, Cancel, Event, Hold, Outcome, Start, Step};

/// 記憶を持つ要求を残したまま backend を落とすときに書く文言。
pub(super) const URING_HELD_ABORT: &str = "eml_io: an io_uring operation in flight may still write memory of a dropped I/O backend; aborting";

// io_uring の ABI (include/uapi/linux/io_uring.h)。大きさは assert で固める

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Sqe {
    opcode: u8,
    flags: u8,
    ioprio: u16,
    fd: i32,
    /// `off` と `addr2` の共用の欄
    off: u64,
    addr: u64,
    len: u32,
    /// `msg_flags`、`poll32_events`、`accept_flags`、`cancel_flags` の共用の欄
    op_flags: u32,
    user_data: u64,
    buf_index: u16,
    personality: u16,
    file_index: u32,
    addr3: u64,
    pad: u64,
}
const _: () = assert!(size_of::<Sqe>() == 64);

#[repr(C)]
#[derive(Clone, Copy)]
struct Cqe {
    user_data: u64,
    res: i32,
    flags: u32,
}
const _: () = assert!(size_of::<Cqe>() == 16);

#[repr(C)]
#[derive(Default)]
struct SqOffsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    flags: u32,
    dropped: u32,
    array: u32,
    resv1: u32,
    user_addr: u64,
}
const _: () = assert!(size_of::<SqOffsets>() == 40);

#[repr(C)]
#[derive(Default)]
struct CqOffsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    overflow: u32,
    cqes: u32,
    flags: u32,
    resv1: u32,
    user_addr: u64,
}
const _: () = assert!(size_of::<CqOffsets>() == 40);

#[repr(C)]
#[derive(Default)]
struct Params {
    sq_entries: u32,
    cq_entries: u32,
    flags: u32,
    sq_thread_cpu: u32,
    sq_thread_idle: u32,
    features: u32,
    wq_fd: u32,
    resv: [u32; 3],
    sq_off: SqOffsets,
    cq_off: CqOffsets,
}
const _: () = assert!(size_of::<Params>() == 120);

#[repr(C)]
struct GeteventsArg {
    sigmask: u64,
    sigmask_sz: u32,
    pad: u32,
    ts: u64,
}
const _: () = assert!(size_of::<GeteventsArg>() == 24);

#[repr(C)]
struct KernelTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}
const _: () = assert!(size_of::<KernelTimespec>() == 16);

#[repr(C)]
struct RsrcUpdate {
    offset: u32,
    resv: u32,
    data: u64,
}
const _: () = assert!(size_of::<RsrcUpdate>() == 16);

const OP_POLL_ADD: u8 = 6;
const OP_ACCEPT: u8 = 13;
const OP_ASYNC_CANCEL: u8 = 14;
const OP_CONNECT: u8 = 16;
const OP_SEND: u8 = 26;
const OP_RECV: u8 = 27;

const SETUP_CQSIZE: u32 = 1 << 3;
const SETUP_SUBMIT_ALL: u32 = 1 << 7;
const SETUP_SINGLE_ISSUER: u32 = 1 << 12;
const SETUP_DEFER_TASKRUN: u32 = 1 << 13;

const FEAT_SINGLE_MMAP: u32 = 1 << 0;
const FEAT_NODROP: u32 = 1 << 1;
const FEAT_FAST_POLL: u32 = 1 << 5;
const FEAT_EXT_ARG: u32 = 1 << 8;

const ENTER_GETEVENTS: u32 = 1 << 0;
const ENTER_EXT_ARG: u32 = 1 << 3;
const ENTER_REGISTERED_RING: u32 = 1 << 4;

const REGISTER_RING_FDS: u32 = 20;
const UNREGISTER_RING_FDS: u32 = 21;

/// `SINGLE_MMAP` の下では、完了の列は要求の列と同じ写しに入る
const OFF_SQ_RING: i64 = 0;
const OFF_SQES: i64 = 0x1000_0000;

const CQE_F_SOCK_NONEMPTY: u32 = 1 << 2;

/// 起こしの見張りの `user_data`。op の識別子は番号を `u32::MAX - 1` より小さく保つので、これと重ならない。
const WAKE: u64 = u64::MAX;
/// 取り消しの要求の `user_data`。終わりは event にしない。
const CANCEL: u64 = u64::MAX - 1;

/// 要求の列と完了の列の大きさ。完了の列を大きく取り、溢れの道に入りにくくする
const SQ_ENTRIES: u32 = 256;
const CQ_ENTRIES: u32 = 1024;

/// カーネルと分け合う写し。落とすと外す。
struct Map {
    ptr: *mut c_void,
    len: usize,
}

impl Map {
    fn new(fd: BorrowedFd<'_>, len: usize, offset: i64) -> io::Result<Map> {
        // SAFETY: 新しい写しを作るだけで、こちらの記憶には触れない。失敗は `MAP_FAILED` で返る
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd.as_raw_fd(),
                offset,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Map { ptr, len })
    }

    /// 写しの先頭から `offset` バイトの番地。読み書きはしない。
    fn at<T>(&self, offset: u32) -> *mut T {
        self.ptr.cast::<u8>().wrapping_add(offset as usize).cast()
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        // SAFETY: `mmap` が返した写しで、ほかに外す所はない。外した後はどのポインタも読まない
        unsafe { libc::munmap(self.ptr, self.len) };
    }
}

/// 1つの ring。要求の列に積み、`enter` で出し、完了の列から読む。fd は登録した番号で名指す。
struct Ring {
    /// 欄は宣言の順に落ちるので、写しを外してから fd を閉じる
    rings: Map,
    sqe_map: Map,
    fd: OwnedFd,
    /// `REGISTER_RING_FDS` が返した番号。登録はスレッドごとなので、driver を作ったスレッドでだけ使う
    index: u32,
    sq_head: *mut u32,
    sq_tail: *mut u32,
    sq_mask: u32,
    sq_entries: u32,
    cq_head: *mut u32,
    cq_tail: *mut u32,
    cq_mask: u32,
    /// 完了の列の、写しの中の位置
    cqes: u32,
    /// 積んだ要求の末尾。カーネルの `sq_tail` を書くのはこちらだけである
    tail: u32,
}

/// 番号で名指す呼び出しの誤りを、6.1 より古い kernel の印として読み替える。
fn too_old(e: io::Error) -> io::Error {
    if e.raw_os_error() == Some(libc::EINVAL) {
        io::Error::new(io::ErrorKind::Unsupported, "io_uring needs Linux 6.1")
    } else {
        e
    }
}

impl Ring {
    fn new(sq: u32, cq: u32) -> io::Result<Ring> {
        let mut p = Params {
            cq_entries: cq,
            flags: SETUP_CQSIZE | SETUP_SUBMIT_ALL | SETUP_SINGLE_ISSUER | SETUP_DEFER_TASKRUN,
            ..Params::default()
        };
        // SAFETY: `p` は局所の `Params` で、カーネルはその大きさだけを読み書きする
        let fd = syscall(|| unsafe {
            libc::syscall(libc::SYS_io_uring_setup, c_long::from(sq), &raw mut p)
        })
        .map_err(too_old)?;
        // SAFETY: `io_uring_setup` が返したばかりの fd で、ほかに持ち主はない。exec で閉じる印は kernel が立てる
        let fd = unsafe { OwnedFd::from_raw_fd(fd as c_int) };
        for (feature, name) in [
            (FEAT_SINGLE_MMAP, "SINGLE_MMAP"),
            (FEAT_NODROP, "NODROP"),
            (FEAT_FAST_POLL, "FAST_POLL"),
            (FEAT_EXT_ARG, "EXT_ARG"),
        ] {
            if p.features & feature == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("io_uring lacks the feature {name}"),
                ));
            }
        }
        let sq_len = p.sq_off.array as usize + 4 * p.sq_entries as usize;
        let cq_len = p.cq_off.cqes as usize + size_of::<Cqe>() * p.cq_entries as usize;
        let rings = Map::new(fd.as_fd(), sq_len.max(cq_len), OFF_SQ_RING)?;
        let sqe_map = Map::new(
            fd.as_fd(),
            size_of::<Sqe>() * p.sq_entries as usize,
            OFF_SQES,
        )?;
        // 要求の列の番号の表は、どの位置も同じ番号の要求を指すように1回だけ埋める
        let array = rings.at::<u32>(p.sq_off.array);
        for i in 0..p.sq_entries {
            // SAFETY: 表は写しの中の `sq_entries` 個の整列した `u32` で、カーネルは `enter` の中でしか読まない
            unsafe { array.add(i as usize).write(i) };
        }
        let mut update = RsrcUpdate {
            offset: u32::MAX,
            resv: 0,
            data: fd.as_raw_fd() as u64,
        };
        // SAFETY: `update` は局所の値で、カーネルは1つ分を読み、`offset` に番号を書く
        let registered = syscall(|| unsafe {
            libc::syscall(
                libc::SYS_io_uring_register,
                c_long::from(fd.as_raw_fd()),
                c_long::from(REGISTER_RING_FDS),
                &raw mut update,
                1 as c_long,
            )
        })
        .map_err(too_old)?;
        if registered != 1 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "io_uring did not register its ring",
            ));
        }
        let sq_head = rings.at(p.sq_off.head);
        let sq_tail = rings.at(p.sq_off.tail);
        // SAFETY: 写しの中の整列した `u32` で、写しは `rings` が生きている間ある
        let sq_mask = unsafe { *rings.at::<u32>(p.sq_off.ring_mask) };
        // SAFETY: 同上
        let cq_mask = unsafe { *rings.at::<u32>(p.cq_off.ring_mask) };
        let cq_head = rings.at(p.cq_off.head);
        let cq_tail = rings.at(p.cq_off.tail);
        // カーネルは列の末尾を 0 から始める
        Ok(Ring {
            rings,
            sqe_map,
            fd,
            index: update.offset,
            sq_head,
            sq_tail,
            sq_mask,
            sq_entries: p.sq_entries,
            cq_head,
            cq_tail,
            cq_mask,
            cqes: p.cq_off.cqes,
            tail: 0,
        })
    }

    /// カーネルと分け合う語。
    fn word(&self, p: *mut u32) -> &AtomicU32 {
        // SAFETY: `p` はどれも `Ring::new` が `rings` の写しの中から取った整列した `u32` を指し、写しは `self` より
        // 長く生きる。カーネルも同時に読み書きするので、atomic としてだけ触れる
        unsafe { AtomicU32::from_ptr(p) }
    }

    /// 積んだが、まだカーネルが取っていない要求の数。
    fn unsubmitted(&self) -> u32 {
        self.tail
            .wrapping_sub(self.word(self.sq_head).load(Ordering::Acquire))
    }

    /// 要求を1つ積む。列が満ちていれば、待たずに出すだけの `enter` で空ける。
    fn push(&mut self, sqe: Sqe) -> io::Result<()> {
        if self.unsubmitted() >= self.sq_entries {
            loop {
                match self.enter(self.unsubmitted(), 0, 0, None) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    r => break r,
                }
            }?;
            if self.unsubmitted() >= self.sq_entries {
                return Err(io::Error::other(
                    "io_uring took none of the queued requests",
                ));
            }
        }
        let at = self.tail & self.sq_mask;
        // SAFETY: `at` は `sq_mask` で切った位置で、要求の写しの中にある。カーネルが取るのは `sq_tail` を進めた後なので、
        // まだ誰も読んでいない
        unsafe { self.sqe_map.at::<Sqe>(0).add(at as usize).write(sqe) };
        self.tail = self.tail.wrapping_add(1);
        self.word(self.sq_tail).store(self.tail, Ordering::Release);
        Ok(())
    }

    /// 最後に積んでまだカーネルが取っていない要求を、列から外す。
    fn retract(&mut self) {
        debug_assert!(self.unsubmitted() > 0);
        self.tail = self.tail.wrapping_sub(1);
        self.word(self.sq_tail).store(self.tail, Ordering::Release);
    }

    /// `io_uring_enter` を登録した番号で呼ぶ。`EINTR` もそのまま返す。
    fn enter(
        &self,
        to_submit: u32,
        min: u32,
        flags: u32,
        arg: Option<&GeteventsArg>,
    ) -> io::Result<u32> {
        let (arg, size) = match arg {
            Some(a) => (ptr::from_ref(a).cast::<c_void>(), size_of::<GeteventsArg>()),
            None => (ptr::null(), 0),
        };
        // SAFETY: `arg` は null か、呼び出しの間生きている局所の `GeteventsArg` を指し、`size` がその大きさである。
        // カーネルが読み書きするのは写しと、積んだ要求が名指す記憶だけで、後者は要求を積んだ所が守る
        let n = unsafe {
            libc::syscall(
                libc::SYS_io_uring_enter,
                c_long::from(self.index),
                c_long::from(to_submit),
                c_long::from(min),
                c_long::from(flags | ENTER_REGISTERED_RING),
                arg,
                size,
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n as u32)
    }

    /// 読める完了の範囲 (先頭, 末尾)。
    fn completions(&self) -> (u32, u32) {
        (
            self.word(self.cq_head).load(Ordering::Relaxed),
            self.word(self.cq_tail).load(Ordering::Acquire),
        )
    }

    fn cqe(&self, i: u32) -> Cqe {
        // SAFETY: `i & cq_mask` は完了の列の中の位置で、`completions` が `Acquire` で読んだ末尾より前なので、カーネルは
        // 書き終えている。先頭を進めるまでカーネルはこの位置に書かない
        unsafe {
            self.rings
                .at::<Cqe>(self.cqes)
                .add((i & self.cq_mask) as usize)
                .read()
        }
    }

    /// `head` までの完了を読み終えた。
    fn consumed(&self, head: u32) {
        self.word(self.cq_head).store(head, Ordering::Release);
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        // 番号を登録したスレッドは ring を持ち続けるので、登録を外さないと fd を閉じても ring が残り、掛けたままの poll が
        // ソケットを生かし続ける
        let mut update = RsrcUpdate {
            offset: self.index,
            resv: 0,
            data: 0,
        };
        // SAFETY: `update` は局所の値で、カーネルは1つ分を読むだけである
        let _ = syscall(|| unsafe {
            libc::syscall(
                libc::SYS_io_uring_register,
                c_long::from(self.fd.as_raw_fd()),
                c_long::from(UNREGISTER_RING_FDS),
                &raw mut update,
                1 as c_long,
            )
        });
    }
}

/// 同期の試みを省くかどうか。どちらも同じ要求を積み、違うのは塞がれる前に試すかだけである。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arm {
    /// epoll と同じ同期の試みを必ず先に出す
    TryFirst,
    /// 行が読めると分かっていなければ、同期の `recv` と `accept4` を省く
    Direct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum What {
    Peek,
    Recv,
    Send,
    Accept,
    Connect,
}

impl What {
    fn reads(self) -> bool {
        matches!(self, What::Peek | What::Recv | What::Accept)
    }
}

/// ring に出した1つの要求。
struct InFlight {
    key: OpKey,
    what: What,
    row: u32,
    row_generation: u32,
    cancelled: bool,
}

/// カーネルが受けと接続で読み書きする番地。箱に置くので、表が伸びても番地は動かない。
struct Mem {
    addr: libc::sockaddr_storage,
    len: libc::socklen_t,
}

struct Entry {
    generation: u32,
    op: Option<InFlight>,
    /// 1つ前の世代の要求の種類。届くはずのない完了が受けたソケットを運んでも、閉じられるようにする
    last: Option<What>,
    mem: Box<Mem>,
}

/// op の表。番号と世代を `user_data` にするので、使い回した番号に古い完了は当たらない。
#[derive(Default)]
struct Ops {
    entries: Vec<Entry>,
    free: Vec<u32>,
}

fn token(index: u32, generation: u32) -> u64 {
    (u64::from(generation) << 32) | u64::from(index)
}

fn untoken(token: u64) -> (u32, u32) {
    (token as u32, (token >> 32) as u32)
}

impl Ops {
    /// 空いた番号に `op` を置き、その `user_data` を返す。
    fn take(&mut self, op: InFlight) -> (u32, u64) {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                let index = u32::try_from(self.entries.len()).expect("too many operations");
                assert!(index < u32::MAX - 1, "too many operations");
                // SAFETY: `sockaddr_storage` は整数と配列だけの C の構造体で、すべて 0 は正しい値である
                let addr = unsafe { mem::zeroed() };
                self.entries.push(Entry {
                    generation: 0,
                    op: None,
                    last: None,
                    mem: Box::new(Mem { addr, len: 0 }),
                });
                index
            }
        };
        let entry = &mut self.entries[index as usize];
        debug_assert!(entry.op.is_none());
        entry.op = Some(op);
        (index, token(index, entry.generation))
    }

    /// `token` が名指す、出したままの要求の番号。放した番号や古い世代には `None`。
    fn live(&self, token: u64) -> Option<u32> {
        let (index, generation) = untoken(token);
        let entry = self.entries.get(index as usize)?;
        (entry.generation == generation && entry.op.is_some()).then_some(index)
    }

    /// 終わった要求を取り出し、番号を放す。
    fn finish(&mut self, index: u32) -> InFlight {
        let entry = &mut self.entries[index as usize];
        let op = entry.op.take().expect("a live entry");
        entry.generation = entry.generation.wrapping_add(1);
        entry.last = Some(op.what);
        self.free.push(index);
        op
    }

    /// `token` が、終わったばかりの1つ前の世代の受けを名指すか。
    fn was_accept(&self, token: u64) -> bool {
        let (index, generation) = untoken(token);
        self.entries.get(index as usize).is_some_and(|e| {
            e.last == Some(What::Accept) && e.generation == generation.wrapping_add(1)
        })
    }

    fn in_flight(&self) -> impl Iterator<Item = &InFlight> {
        self.entries.iter().filter_map(|e| e.op.as_ref())
    }
}

#[derive(Default)]
struct Row {
    generation: u32,
    /// 読みの向きに出した要求の番号
    read: Option<u32>,
    /// 書きの向きに出した要求の番号
    write: Option<u32>,
    /// 読めるという見込み。外れても1回の syscall か1回の待ちで済むので、正しさはこれに頼らない
    ready: bool,
}

#[derive(Default)]
struct Rows {
    rows: Vec<Row>,
    free: Vec<u32>,
}

impl Rows {
    fn adopt(&mut self) -> (u32, u32) {
        if let Some(row) = self.free.pop() {
            return (row, self.rows[row as usize].generation);
        }
        let row = u32::try_from(self.rows.len()).expect("too many sockets");
        self.rows.push(Row::default());
        (row, 0)
    }

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

    /// 世代の合う行。放した行には `None`。
    fn live(&mut self, row: u32, generation: u32) -> Option<&mut Row> {
        self.rows
            .get_mut(row as usize)
            .filter(|r| r.generation == generation)
    }
}

fn poll_sqe(fd: BorrowedFd<'_>, events: c_int, user_data: u64) -> Sqe {
    Sqe {
        opcode: OP_POLL_ADD,
        fd: fd.as_raw_fd(),
        op_flags: events as u32,
        user_data,
        ..Sqe::default()
    }
}

/// 長さは `i32` に収まるよう切る。短い読み書きになるだけで、核が続きを出す。
fn clamp(cap: usize) -> u32 {
    cap.min(i32::MAX as usize) as u32
}

fn os_error(res: i32) -> io::Error {
    io::Error::from_raw_os_error(-res)
}

pub(crate) struct Uring {
    /// 最初に落ちるよう最初に置く。ring を閉じてから op の表の記憶を放す
    ring: Ring,
    waker: Arc<EventFd>,
    rows: Rows,
    ops: Ops,
    arm: Arm,
    /// 起こしの見張りが終わった。次の `wait` が掛け直す
    rearm: bool,
    epoch: Instant,
}

impl Uring {
    pub(crate) fn new(arm: Arm) -> io::Result<Uring> {
        Uring::with_sizes(SQ_ENTRIES, CQ_ENTRIES, arm)
    }

    /// 列の大きさを選べる構築子。溢れと満ちた列の道をテストで通すのに使う。
    pub(super) fn with_sizes(sq: u32, cq: u32, arm: Arm) -> io::Result<Uring> {
        let mut ring = Ring::new(sq, cq)?;
        let waker = EventFd::new()?;
        ring.push(poll_sqe(waker.fd(), libc::POLLIN.into(), WAKE))?;
        // 作るときに1回だけ待たずに出す。ring を作れても出す呼び出しを拒む sandbox を、ここで誤りにするためである
        let taken = loop {
            match ring.enter(ring.unsubmitted(), 0, ENTER_GETEVENTS, None) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                r => break r.map_err(too_old)?,
            }
        };
        if taken != 1 {
            return Err(io::Error::other("io_uring did not take the waker's poll"));
        }
        Ok(Uring {
            ring,
            waker,
            rows: Rows::default(),
            ops: Ops::default(),
            arm,
            rearm: false,
            epoch: Instant::now(),
        })
    }

    #[cfg(test)]
    pub(super) fn arm(&self) -> Arm {
        self.arm
    }

    /// 要求を表に置いて ring に積む。積めなければ、その誤りで歩を終える。
    fn queue(
        &mut self,
        key: OpKey,
        what: What,
        sock: &Sock,
        sqe: impl FnOnce(&mut Mem, u64) -> Sqe,
    ) -> Start<Sock> {
        let (index, token) = self.ops.take(InFlight {
            key,
            what,
            row: sock.row,
            row_generation: sock.generation,
            cancelled: false,
        });
        let sqe = sqe(&mut self.ops.entries[index as usize].mem, token);
        if let Err(e) = self.ring.push(sqe) {
            self.ops.finish(index);
            return Start::Done(Err(e));
        }
        let r = self.rows.of(sock);
        if what.reads() {
            r.read = Some(index);
        } else {
            r.write = Some(index);
        }
        // カーネルがこちらの記憶を名指さないのは poll だけである
        Start::Pending(if what == What::Peek {
            Hold::None
        } else {
            Hold::Memory
        })
    }

    fn peek(&mut self, key: OpKey, sock: &Sock) -> Start<Sock> {
        if self.rows.of(sock).ready {
            let mut byte = MaybeUninit::<u8>::uninit();
            let buf = RawBuf {
                ptr: byte.as_mut_ptr(),
                cap: 1,
            };
            // 1バイトか EOF が読めれば終わる。覗いた誤りは消費される
            match recv_raw(sock.fd.as_fd(), buf, PEEK) {
                Ok(_) => return Start::Done(Ok(Outcome::Size(0))),
                Err(e) if would_block(&e) => self.rows.of(sock).ready = false,
                Err(e) => return Start::Done(Err(e)),
            }
        }
        let fd = sock.fd.as_fd();
        self.queue(key, What::Peek, sock, |_, token| {
            poll_sqe(fd, (libc::POLLIN | libc::POLLRDHUP).into(), token)
        })
    }

    fn recv(&mut self, key: OpKey, sock: &Sock, buf: RawBuf) -> Start<Sock> {
        // `TryFirst` は epoll と同じく必ず試すので、データのある読みは1回の syscall で終わる
        if self.arm == Arm::TryFirst || self.rows.of(sock).ready {
            match recv_raw(sock.fd.as_fd(), buf, RECV) {
                Ok(n) => {
                    // 短い読みは受けの列を読み尽くした印と見る
                    if self.arm == Arm::Direct && n < buf.cap {
                        self.rows.of(sock).ready = false;
                    }
                    return Start::Done(Ok(Outcome::Size(n)));
                }
                Err(e) if would_block(&e) => self.rows.of(sock).ready = false,
                Err(e) => return Start::Done(Err(e)),
            }
        }
        let fd = sock.fd.as_raw_fd();
        self.queue(key, What::Recv, sock, |_, token| Sqe {
            opcode: OP_RECV,
            fd,
            addr: buf.ptr.expose_provenance() as u64,
            len: clamp(buf.cap),
            user_data: token,
            ..Sqe::default()
        })
    }

    fn send(&mut self, key: OpKey, sock: &Sock, buf: RawBuf) -> Start<Sock> {
        // 書きの見込みは持たないので、どちらの腕も必ず試す
        match send_raw(sock.fd.as_fd(), buf, SEND) {
            Ok(n) => return Start::Done(Ok(Outcome::Size(n))),
            Err(e) if would_block(&e) => {}
            Err(e) => return Start::Done(Err(e)),
        }
        let fd = sock.fd.as_raw_fd();
        self.queue(key, What::Send, sock, |_, token| Sqe {
            opcode: OP_SEND,
            fd,
            addr: buf.ptr.expose_provenance() as u64,
            len: clamp(buf.cap),
            op_flags: libc::MSG_NOSIGNAL as u32,
            user_data: token,
            ..Sqe::default()
        })
    }

    fn accept(&mut self, key: OpKey, listener: &Sock) -> Start<Sock> {
        if self.arm == Arm::TryFirst || self.rows.of(listener).ready {
            // SAFETY: `sockaddr_storage` は整数と配列だけの C の構造体で、すべて 0 は正しい値である
            let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
            let mut len = size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            match accept_raw(listener.fd.as_fd(), &mut storage, &mut len) {
                Ok(fd) => return self.accepted(fd, &storage, len),
                Err(e) if would_block(&e) => self.rows.of(listener).ready = false,
                Err(e) => return Start::Done(Err(aborted(e))),
            }
        }
        let fd = listener.fd.as_raw_fd();
        self.queue(key, What::Accept, listener, |mem, token| {
            mem.len = size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            Sqe {
                opcode: OP_ACCEPT,
                fd,
                addr: (&raw mut mem.addr).expose_provenance() as u64,
                off: (&raw mut mem.len).expose_provenance() as u64,
                op_flags: (libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC) as u32,
                user_data: token,
                ..Sqe::default()
            }
        })
    }

    /// 受けたソケットに行を与える。番地が IP でなければ閉じて誤りにする。
    fn accepted(
        &mut self,
        fd: OwnedFd,
        storage: &libc::sockaddr_storage,
        len: libc::socklen_t,
    ) -> Start<Sock> {
        let Some(peer) = from_storage(storage, len) else {
            return Start::Done(Err(io::Error::other("the peer has no IP address")));
        };
        let (row, generation) = self.rows.adopt();
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
        let fd = sock.fd.as_raw_fd();
        let start = self.queue(key, What::Connect, sock, |mem, token| {
            let raw = RawAddr::new(peer);
            let (from, len) = raw.as_ptr();
            // SAFETY: `from` は生きている `raw` の `len` バイトを指し、`sockaddr_storage` はどの番地より大きい
            unsafe {
                ptr::copy_nonoverlapping(
                    from.cast::<u8>(),
                    (&raw mut mem.addr).cast::<u8>(),
                    len as usize,
                );
            }
            mem.len = len;
            Sqe {
                opcode: OP_CONNECT,
                fd,
                addr: (&raw mut mem.addr).expose_provenance() as u64,
                off: u64::from(len),
                user_data: token,
                ..Sqe::default()
            }
        });
        if !matches!(start, Start::Pending(_)) {
            return start;
        }
        // 接続は同期の試みを持たず、SYN が出るまで相手には何も見えないので、ここで待たずに出す。`submit` が歩の
        // 1回の試みを OS に出すためである (docs/spec/vm.md の「時計と driver」)。ほかの歩は次の待ちでまとめて出す。
        // 同期の試みを済ませたか、省いても相手が待たないためである (受けない接続は backlog で、読まないデータは受けの
        // 列で待つ)
        let entered = loop {
            match self.ring.enter(self.ring.unsubmitted(), 0, 0, None) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                r => break r,
            }
        };
        match entered {
            Ok(_) => start,
            Err(e) => {
                // 誤りの enter は何も取らないので、積んだばかりの接続は列の末尾に残っている。列から外して表も戻す
                let index = self
                    .rows
                    .of(sock)
                    .write
                    .take()
                    .expect("the connect is queued");
                self.ring.retract();
                self.ops.finish(index);
                Start::Done(Err(e))
            }
        }
    }

    /// 1つの完了を表と行に写し、要るなら event を積む。
    fn complete(&mut self, cqe: Cqe, events: &mut Vec<Event<Sock>>) {
        match cqe.user_data {
            WAKE => {
                self.waker.drain();
                self.rearm = true;
            }
            CANCEL => {
                // 対象がもう終わっていれば `ENOENT` か `EALREADY` で、終わりは対象の完了が運ぶ
                debug_assert!(
                    matches!(-cqe.res, 0 | libc::ENOENT | libc::EALREADY),
                    "io_uring refused a cancel: {}",
                    os_error(cqe.res)
                );
            }
            token => {
                let Some(index) = self.ops.live(token) else {
                    debug_assert!(false, "a completion names no operation");
                    if self.ops.was_accept(token) {
                        close_accepted(cqe.res);
                    }
                    return;
                };
                let entry = &self.ops.entries[index as usize];
                let (addr, len) = (entry.mem.addr, entry.mem.len);
                let op = self.ops.finish(index);
                let Some(r) = self.rows.live(op.row, op.row_generation) else {
                    // driver を落とすときに忘れた poll だけがここに来る。鍵はもう核にない
                    debug_assert_eq!(op.what, What::Peek);
                    if op.what == What::Accept {
                        close_accepted(cqe.res);
                    }
                    return;
                };
                let waiter = if op.what.reads() {
                    &mut r.read
                } else {
                    &mut r.write
                };
                if *waiter == Some(index) {
                    *waiter = None;
                }
                let res = cqe.res;
                let interrupted = res == -libc::ECANCELED || res == -libc::EINTR;
                let arrival = if op.cancelled && interrupted {
                    Arrival::Cancelled
                } else {
                    match op.what {
                        What::Peek if res < 0 => Arrival::Done(Err(os_error(res))),
                        What::Peek => {
                            r.ready = true;
                            // 誤りは覗きの `recv` で読ませる。RST の後の覗きが誤りを1回だけ返すためである
                            if res & c_int::from(libc::POLLERR) != 0 || res == 0 {
                                Arrival::Retry
                            } else {
                                Arrival::Done(Ok(Outcome::Size(0)))
                            }
                        }
                        What::Recv if res >= 0 => {
                            r.ready = cqe.flags & CQE_F_SOCK_NONEMPTY != 0;
                            Arrival::Done(Ok(Outcome::Size(res as usize)))
                        }
                        What::Send if res >= 0 => Arrival::Done(Ok(Outcome::Size(res as usize))),
                        What::Recv | What::Send => Arrival::Done(Err(os_error(res))),
                        What::Accept if res >= 0 => {
                            // SAFETY: 受けの完了が返したばかりの fd で、ほかに持ち主はない
                            let fd = unsafe { OwnedFd::from_raw_fd(res) };
                            match self.accepted(fd, &addr, len) {
                                Start::Done(result) => Arrival::Done(result),
                                Start::Pending(_) => unreachable!("accepting never waits"),
                            }
                        }
                        What::Accept => Arrival::Done(Err(aborted(os_error(res)))),
                        // 出し直した接続の `EISCONN` は、先の接続が通った印である
                        What::Connect if res == 0 || res == -libc::EISCONN => {
                            Arrival::Done(Ok(Outcome::Connected))
                        }
                        What::Connect => Arrival::Done(Err(os_error(res))),
                    }
                };
                events.push(Event {
                    key: op.key,
                    arrival,
                });
            }
        }
    }

    /// 完了の列を読み切る。
    fn reap(&mut self, events: &mut Vec<Event<Sock>>) {
        let (mut head, tail) = self.ring.completions();
        while head != tail {
            let cqe = self.ring.cqe(head);
            head = head.wrapping_add(1);
            self.complete(cqe, events);
        }
        self.ring.consumed(tail);
    }
}

/// 渡す先のない受けの完了が運んだソケットを閉じる。
fn close_accepted(res: i32) {
    if res >= 0 {
        // SAFETY: 受けの完了が返したばかりの fd で、ほかに持ち主はない。落とすと閉じる
        drop(unsafe { OwnedFd::from_raw_fd(res) });
    }
}

/// 来ていた接続1本だけの誤りを、核の受けの出し直しに乗せる。
fn aborted(e: io::Error) -> io::Error {
    if transient(&e) {
        io::Error::new(io::ErrorKind::ConnectionAborted, e)
    } else {
        e
    }
}

impl Backend for Uring {
    type Sock = Sock;

    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        self.waker.clone()
    }

    fn spawn(
        &self,
        builder: thread::Builder,
        body: Box<dyn FnOnce() + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<()>> {
        crate::mask::spawn_masked(builder, body)
    }

    fn listen(&mut self, addr: SocketAddr) -> io::Result<(Sock, u16)> {
        let (fd, port) = listener(addr)?;
        let (row, generation) = self.rows.adopt();
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
        shutdown_write(sock.fd.as_fd())
    }

    fn set_nodelay(&mut self, sock: &mut Sock, on: bool) -> io::Result<()> {
        set_nodelay(sock.fd.as_fd(), on)
    }

    fn close(&mut self, sock: Sock) {
        let r = self.rows.of(&sock);
        // 核は保留の歩があるソケットを閉じない。残ってよいのは、driver を落とすときに忘れた poll だけである
        debug_assert!(r.write.is_none(), "a socket closed with a write in flight");
        debug_assert!(
            r.read.is_none_or(|index| {
                self.ops.entries[index as usize]
                    .op
                    .as_ref()
                    .is_none_or(|op| op.what == What::Peek)
            }),
            "a socket closed with a read in flight"
        );
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
        let Some(index) = (if read { r.read } else { r.write }) else {
            debug_assert!(false, "the core cancelled a step that is not in flight");
            return Ok(Cancel::Done);
        };
        let entry = &self.ops.entries[index as usize];
        let target = token(index, entry.generation);
        debug_assert_eq!(entry.op.as_ref().map(|op| op.key), Some(key));
        self.ring.push(Sqe {
            opcode: OP_ASYNC_CANCEL,
            fd: -1,
            addr: target,
            user_data: CANCEL,
            ..Sqe::default()
        })?;
        if let Some(op) = self.ops.entries[index as usize].op.as_mut() {
            op.cancelled = true;
        }
        Ok(Cancel::Pending)
    }

    fn wait(&mut self, timeout: Option<Duration>, events: &mut Vec<Event<Sock>>) -> io::Result<()> {
        if self.rearm {
            // poll は積んだ時点の状態を調べるので、見張りが終わってからの起こしもここで拾う
            self.ring
                .push(poll_sqe(self.waker.fd(), libc::POLLIN.into(), WAKE))?;
            self.rearm = false;
        }
        let n = self.ring.unsubmitted();
        let entered = match timeout {
            Some(Duration::ZERO) => self.ring.enter(n, 0, ENTER_GETEVENTS, None),
            Some(d) => {
                // 期限はそのまま渡すので、早くは戻らない
                let ts = KernelTimespec {
                    tv_sec: i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
                    tv_nsec: i64::from(d.subsec_nanos()),
                };
                let arg = GeteventsArg {
                    sigmask: 0,
                    sigmask_sz: 8,
                    pad: 0,
                    ts: ptr::from_ref(&ts).expose_provenance() as u64,
                };
                self.ring
                    .enter(n, 1, ENTER_GETEVENTS | ENTER_EXT_ARG, Some(&arg))
            }
            None => self.ring.enter(n, 1, ENTER_GETEVENTS, None),
        };
        if let Err(e) = entered {
            match e.raw_os_error() {
                // 割り込みは VM が確かめ直すだけである (docs/spec/vm.md の「上限と割り込み」)。`EAGAIN` は要求を
                // 積んだまま、次の待ちで出し直す
                Some(libc::EINTR | libc::ETIME | libc::EAGAIN) => {}
                // 溢れた完了が残っている。読み切ってから、溢れを列へ移す呼び出しを1回出す
                Some(libc::EBUSY | libc::EBADR) => {
                    let before = events.len();
                    self.reap(events);
                    if let Err(e) = self.ring.enter(0, 0, ENTER_GETEVENTS, None)
                        && e.kind() != io::ErrorKind::Interrupted
                        && events.len() == before
                    {
                        return Err(e);
                    }
                }
                _ => return Err(e),
            }
        }
        self.reap(events);
        Ok(())
    }
}

impl Drop for Uring {
    fn drop(&mut self) {
        // カーネルが書きうる記憶を持つ要求を残して放すと、放した記憶にカーネルが書く。核は終わりを読むまで backend を
        // 落とさないので、ここに来るのは核の待ちが尽きたときだけである (docs/spec/vm.md の「時計と driver」)
        if self.ops.in_flight().any(|op| op.what != What::Peek) {
            eprintln!("{URING_HELD_ABORT}");
            process::abort();
        }
    }
}

/// backend を本物のループバックのソケットで確かめる。どのテストも2つの腕で流し、待ちは 1 秒で切る。経過時間は下限だけを
/// 確かめる (docs/implementation/testing.md の「性能のテスト」)。記憶を持つ要求を残したテストは、`Uring` を落とす前に
/// `settle` で終わりを読み切る。読み切らずに落とすとテストのバイナリが止まる。
#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    use super::*;
    use crate::real::IoBackend;

    const LIMIT: Duration = Duration::from_secs(1);
    const ARMS: [Arm; 2] = [Arm::TryFirst, Arm::Direct];

    fn ring(arm: Arm) -> Option<Uring> {
        ring_sized(SQ_ENTRIES, CQ_ENTRIES, arm)
    }

    fn ring_sized(sq: u32, cq: u32, arm: Arm) -> Option<Uring> {
        if IoBackend::from_env().expect("EML_IO_BACKEND") == IoBackend::Epoll {
            eprintln!("EML_IO_BACKEND=epoll: ring tests do not run");
            return None;
        }
        Some(Uring::with_sizes(sq, cq, arm).unwrap_or_else(|e| {
            panic!(
                "io_uring cannot be used here ({e}); set EML_IO_BACKEND=epoll to run the tests on epoll"
            )
        }))
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    fn buf(bytes: &mut [u8]) -> RawBuf {
        RawBuf {
            ptr: bytes.as_mut_ptr(),
            cap: bytes.len(),
        }
    }

    /// 1 秒の間に届いた event を集め、`stop` が真になったら返す。
    fn gather(u: &mut Uring, mut stop: impl FnMut(&[Event<Sock>]) -> bool) -> Vec<Event<Sock>> {
        let start = Instant::now();
        let mut got = Vec::new();
        while !stop(&got) {
            let left = LIMIT
                .checked_sub(start.elapsed())
                .expect("the events did not arrive");
            u.wait(Some(left), &mut got).unwrap();
        }
        got
    }

    /// `key` に届いた知らせ。1 秒で届かなければ落ちる。
    fn next(u: &mut Uring, key: OpKey) -> Arrival<Sock> {
        let got = gather(u, |got| got.iter().any(|e| e.key == key));
        let mut found = None;
        for e in got {
            if e.key == key {
                found = Some(e.arrival);
            } else {
                panic!("an event for another key {}", e.key);
            }
        }
        found.unwrap()
    }

    /// 出したままの要求がなくなるまで待つ。
    fn settle(u: &mut Uring) {
        let start = Instant::now();
        while u.ops.in_flight().any(|op| op.what != What::Peek) {
            let left = LIMIT
                .checked_sub(start.elapsed())
                .expect("an operation did not finish");
            u.wait(Some(left), &mut Vec::new()).unwrap();
        }
    }

    fn done(start: Start<Sock>) -> Outcome<Sock> {
        match start {
            Start::Done(result) => result.unwrap(),
            Start::Pending(_) => panic!("the step is pending"),
        }
    }

    fn arrived(arrival: Arrival<Sock>) -> Outcome<Sock> {
        match arrival {
            Arrival::Done(result) => result.unwrap(),
            Arrival::Retry => panic!("a retry arrived"),
            Arrival::Cancelled => panic!("the step was cancelled"),
        }
    }

    fn holds_none(start: &Start<Sock>) -> bool {
        matches!(start, Start::Pending(Hold::None))
    }

    fn holds_memory(start: &Start<Sock>) -> bool {
        matches!(start, Start::Pending(Hold::Memory))
    }

    fn accept(u: &mut Uring, listener: &mut Sock) -> Sock {
        let key = 90;
        let start = u.start(key, Step::Accept { listener });
        let outcome = match start {
            Start::Done(result) => result.unwrap(),
            Start::Pending(_) => arrived(next(u, key)),
        };
        match outcome {
            Outcome::Accepted { sock, .. } => sock,
            _ => panic!("not an accepted socket"),
        }
    }

    /// リスナーと、それにつないだ std のストリームと、受けたソケット。
    fn pair(u: &mut Uring) -> (Sock, TcpStream, Sock) {
        let (mut listener, port) = u.listen(addr("127.0.0.1:0")).unwrap();
        let peer = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let sock = accept(u, &mut listener);
        (listener, peer, sock)
    }

    /// 相手の書いたデータが自分の側で読めるようになるまで待つ。
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
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// 相手のストリームを RST で閉じる。
    fn reset(peer: TcpStream) {
        let linger = libc::linger {
            l_onoff: 1,
            l_linger: 0,
        };
        // SAFETY: `linger` は局所の値で、その大きさを渡す。カーネルは読むだけである
        let status = unsafe {
            libc::setsockopt(
                peer.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                (&raw const linger).cast(),
                size_of::<libc::linger>() as libc::socklen_t,
            )
        };
        assert_eq!(status, 0);
        drop(peer);
    }

    /// 送りの buffer が満ちて送りが ring に出るまで送る。出た送りの鍵は `key` で、`data` を貸したままにする。
    fn fill(u: &mut Uring, sock: &mut Sock, key: OpKey, data: &mut [u8]) {
        // 送りの buffer を小さくして、すぐに満ちるようにする
        super::super::unix::setsockopt_int(
            sock.fd.as_fd(),
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            4096,
        )
        .unwrap();
        let start = Instant::now();
        loop {
            match u.start(
                key,
                Step::Send {
                    sock: &mut *sock,
                    buf: buf(data),
                },
            ) {
                Start::Done(result) => assert!(result.unwrap_size() > 0),
                pending => {
                    assert!(holds_memory(&pending));
                    return;
                }
            }
            assert!(start.elapsed() < LIMIT, "the send buffer never filled");
        }
    }

    fn recv_step<'a>(sock: &'a mut Sock, bytes: &mut [u8]) -> Step<'a, Sock> {
        Step::Recv {
            sock,
            buf: buf(bytes),
        }
    }

    trait Size {
        fn unwrap_size(self) -> usize;
    }

    impl Size for io::Result<Outcome<Sock>> {
        fn unwrap_size(self) -> usize {
            match self.unwrap() {
                Outcome::Size(n) => n,
                _ => panic!("not a size"),
            }
        }
    }

    #[test]
    fn a_token_keeps_its_index_and_generation() {
        for (index, generation) in [(0, 0), (1, 0), (0, 1), (u32::MAX - 2, u32::MAX)] {
            assert_eq!(untoken(token(index, generation)), (index, generation));
        }
        // op の番号は `u32::MAX - 1` より小さいので、どの op の識別子も2つと重ならない
        for special in [WAKE, CANCEL] {
            assert!(untoken(special).0 >= u32::MAX - 1);
        }
    }

    #[test]
    fn a_stale_generation_names_no_entry() {
        let mut ops = Ops::default();
        let op = |key| InFlight {
            key,
            what: What::Recv,
            row: 0,
            row_generation: 0,
            cancelled: false,
        };
        let (index, old) = ops.take(op(1));
        assert_eq!(ops.live(old), Some(index));
        ops.finish(index);
        assert_eq!(ops.live(old), None);
        let (again, new) = ops.take(op(2));
        assert_eq!(again, index);
        assert_ne!(new, old);
        assert_eq!(ops.live(old), None);
        assert_eq!(ops.live(new), Some(index));
        assert_eq!(ops.live(token(index + 1, 0)), None);
        let accept = InFlight {
            what: What::Accept,
            ..op(3)
        };
        assert!(!ops.was_accept(new));
        ops.finish(index);
        assert!(!ops.was_accept(new));
        let (index, newer) = ops.take(accept);
        ops.finish(index);
        assert!(ops.was_accept(newer));
        assert!(!ops.was_accept(new));
    }

    #[test]
    fn a_pending_peek_completes_when_data_arrives() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, mut peer, mut sock) = pair(&mut u);
            let key = 1;
            assert!(holds_none(&u.start(key, Step::Peek { sock: &mut sock })));
            peer.write_all(b"ab").unwrap();
            assert!(matches!(arrived(next(&mut u, key)), Outcome::Size(0)));
            assert!(u.rows.of(&sock).ready);
            // 読める見込みのある行の覗きは、同期の覗きで答える
            assert!(matches!(
                done(u.start(key, Step::Peek { sock: &mut sock })),
                Outcome::Size(0)
            ));
        }
    }

    #[test]
    fn peeks_after_a_reset_report_the_error_once_then_eof() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, peer, mut sock) = pair(&mut u);
            let key = 1;
            assert!(holds_none(&u.start(key, Step::Peek { sock: &mut sock })));
            reset(peer);
            assert!(matches!(next(&mut u, key), Arrival::Retry));
            match u.start(key, Step::Peek { sock: &mut sock }) {
                Start::Done(Err(e)) => assert_eq!(e.kind(), io::ErrorKind::ConnectionReset),
                _ => panic!("the reset was not reported"),
            }
            assert!(matches!(
                done(u.start(key, Step::Peek { sock: &mut sock })),
                Outcome::Size(0)
            ));
        }
    }

    #[test]
    fn a_blocked_recv_is_completed_into_the_lent_buffer() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, mut peer, mut sock) = pair(&mut u);
            let key = 1;
            let mut bytes = [0; 16];
            let start = u.start(
                key,
                Step::Recv {
                    sock: &mut sock,
                    buf: buf(&mut bytes),
                },
            );
            assert!(holds_memory(&start));
            peer.write_all(b"hello").unwrap();
            assert!(matches!(arrived(next(&mut u, key)), Outcome::Size(5)));
            assert_eq!(&bytes[..5], b"hello");
            // 読み尽くした受けの完了は、行を読めないと見る
            assert!(!u.rows.of(&sock).ready);
        }
    }

    #[test]
    fn a_direct_recv_skips_the_sync_attempt() {
        let Some(mut u) = ring(Arm::Direct) else {
            return;
        };
        let (_listener, mut peer, mut sock) = pair(&mut u);
        peer.write_all(b"abc").unwrap();
        wait_readable(&sock);
        let key = 1;
        let mut bytes = [0; 16];
        // データはあるが、行が読めると分かっていないので ring に出す
        let start = u.start(
            key,
            Step::Recv {
                sock: &mut sock,
                buf: buf(&mut bytes),
            },
        );
        assert!(holds_memory(&start));
        assert!(matches!(arrived(next(&mut u, key)), Outcome::Size(3)));
        assert_eq!(&bytes[..3], b"abc");
    }

    #[test]
    fn a_short_sync_recv_clears_ready_in_direct() {
        let Some(mut u) = ring(Arm::Direct) else {
            return;
        };
        let (_listener, mut peer, mut sock) = pair(&mut u);
        let key = 1;
        assert!(holds_none(&u.start(key, Step::Peek { sock: &mut sock })));
        peer.write_all(b"ab").unwrap();
        assert!(matches!(arrived(next(&mut u, key)), Outcome::Size(0)));
        assert!(u.rows.of(&sock).ready);
        let mut bytes = [0; 16];
        let read = done(u.start(
            key,
            Step::Recv {
                sock: &mut sock,
                buf: buf(&mut bytes),
            },
        ));
        assert!(matches!(read, Outcome::Size(2)));
        assert!(!u.rows.of(&sock).ready);
        let start = u.start(
            key,
            Step::Recv {
                sock: &mut sock,
                buf: buf(&mut bytes),
            },
        );
        assert!(holds_memory(&start));
        assert!(matches!(
            u.cancel(key, recv_step(&mut sock, &mut bytes)),
            Ok(Cancel::Pending)
        ));
        settle(&mut u);
    }

    #[test]
    fn a_blocked_send_finishes_after_the_peer_drains() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, mut peer, mut sock) = pair(&mut u);
            let key = 1;
            let mut data = vec![7u8; 1 << 20];
            fill(&mut u, &mut sock, key, &mut data);
            let reader = thread::spawn(move || {
                let mut all = Vec::new();
                peer.read_to_end(&mut all).unwrap();
                all.len()
            });
            let sent = match next(&mut u, key) {
                Arrival::Done(result) => result.unwrap_size(),
                _ => panic!("the send did not finish"),
            };
            assert!(sent > 0);
            u.close(sock);
            assert!(reader.join().unwrap() >= sent);
        }
    }

    #[test]
    fn a_cancelled_recv_ends_cancelled() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, _peer, mut sock) = pair(&mut u);
            let key = 1;
            let mut bytes = [0; 16];
            assert!(holds_memory(
                &u.start(key, recv_step(&mut sock, &mut bytes))
            ));
            // 出してから取り消す
            u.wait(Some(Duration::ZERO), &mut Vec::new()).unwrap();
            assert!(matches!(
                u.cancel(key, recv_step(&mut sock, &mut bytes)),
                Ok(Cancel::Pending)
            ));
            assert!(matches!(next(&mut u, key), Arrival::Cancelled));
            assert!(u.rows.of(&sock).read.is_none());
        }
    }

    #[test]
    fn a_cancel_queued_before_its_op_was_submitted_still_cancels() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, _peer, mut sock) = pair(&mut u);
            let key = 1;
            let mut bytes = [0; 16];
            assert!(holds_memory(
                &u.start(key, recv_step(&mut sock, &mut bytes))
            ));
            assert!(matches!(
                u.cancel(key, recv_step(&mut sock, &mut bytes)),
                Ok(Cancel::Pending)
            ));
            assert!(matches!(next(&mut u, key), Arrival::Cancelled));
        }
    }

    #[test]
    fn accept_and_connect_complete_through_the_ring() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (mut listener, port) = u.listen(addr("127.0.0.1:0")).unwrap();
            let peer = addr(&format!("127.0.0.1:{port}"));
            let mut conn = u.socket(peer).unwrap();
            let (connecting, accepting) = (1, 2);
            // 受けを先に出すので、どちらの腕でも受けは ring に出る
            let start = u.start(
                accepting,
                Step::Accept {
                    listener: &mut listener,
                },
            );
            assert!(holds_memory(&start));
            let start = u.start(
                connecting,
                Step::Connect {
                    sock: &mut conn,
                    peer,
                },
            );
            assert!(holds_memory(&start));
            let got = gather(&mut u, |got| got.len() == 2);
            let mut accepted = None;
            for e in got {
                match (e.key, arrived(e.arrival)) {
                    (1, Outcome::Connected) => {}
                    (2, Outcome::Accepted { sock, peer }) => accepted = Some((sock, peer)),
                    _ => panic!("an unexpected outcome"),
                }
            }
            let (accepted, from) = accepted.expect("nothing was accepted");
            assert_eq!(
                Some(from),
                super::super::unix::name_of(conn.fd.as_fd(), false).unwrap()
            );
            for sock in [&conn, &accepted] {
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
    }

    #[test]
    fn an_overflowing_completion_queue_loses_nothing() {
        for arm in ARMS {
            let Some(mut u) = ring_sized(4, 8, arm) else {
                return;
            };
            let (mut listener, port) = u.listen(addr("127.0.0.1:0")).unwrap();
            let mut socks = Vec::new();
            let mut peers = Vec::new();
            for _ in 0..32 {
                let mut peer = TcpStream::connect(("127.0.0.1", port)).unwrap();
                peer.write_all(b"x").unwrap();
                socks.push(accept(&mut u, &mut listener));
                peers.push(peer);
            }
            for (key, sock) in socks.iter_mut().enumerate() {
                wait_readable(sock);
                assert!(holds_none(&u.start(key as OpKey, Step::Peek { sock })));
            }
            let start = Instant::now();
            let mut got = 0;
            while got < 32 {
                assert!(start.elapsed() < LIMIT, "the completions did not arrive");
                let mut events = Vec::new();
                u.wait(Some(Duration::ZERO), &mut events).unwrap();
                assert!(!events.is_empty(), "a wait came back empty after {got}");
                for e in events {
                    assert!(matches!(arrived(e.arrival), Outcome::Size(0)));
                    got += 1;
                }
            }
        }
    }

    #[test]
    fn a_full_submission_queue_is_flushed() {
        for arm in ARMS {
            let Some(mut u) = ring_sized(4, 16, arm) else {
                return;
            };
            let mut pairs: Vec<_> = (0..12).map(|_| pair(&mut u)).collect();
            for (key, (_, _, sock)) in pairs.iter_mut().enumerate() {
                assert!(holds_none(&u.start(key as OpKey, Step::Peek { sock })));
            }
            for (_, peer, _) in &mut pairs {
                peer.write_all(b"x").unwrap();
            }
            let got = gather(&mut u, |got| got.len() == 12);
            let mut keys: Vec<OpKey> = got.iter().map(|e| e.key).collect();
            keys.sort_unstable();
            assert_eq!(keys, (0..12).collect::<Vec<OpKey>>());
        }
    }

    #[test]
    fn a_connect_reaches_the_peer_before_the_next_wait() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let server = TcpListener::bind("127.0.0.1:0").unwrap();
            server.set_nonblocking(true).unwrap();
            let peer = server.local_addr().unwrap();
            let mut sock = u.socket(peer).unwrap();
            let key = 1;
            let start = u.start(
                key,
                Step::Connect {
                    sock: &mut sock,
                    peer,
                },
            );
            assert!(holds_memory(&start));
            let started = Instant::now();
            while server.accept().is_err() {
                assert!(started.elapsed() < LIMIT, "the connect was not issued");
                thread::sleep(Duration::from_millis(1));
            }
            assert!(matches!(arrived(next(&mut u, key)), Outcome::Connected));
        }
    }

    #[test]
    fn a_refused_connect_reports_connection_refused() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let port = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let peer = addr(&format!("127.0.0.1:{port}"));
            let mut sock = u.socket(peer).unwrap();
            let key = 1;
            let start = u.start(
                key,
                Step::Connect {
                    sock: &mut sock,
                    peer,
                },
            );
            assert!(holds_memory(&start));
            match next(&mut u, key) {
                Arrival::Done(Err(e)) => assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused),
                _ => panic!("the connect was not refused"),
            }
        }
    }

    #[test]
    fn dropping_with_a_pending_peek_closes_the_socket() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, mut peer, mut sock) = pair(&mut u);
            assert!(holds_none(&u.start(1, Step::Peek { sock: &mut sock })));
            u.wait(Some(Duration::ZERO), &mut Vec::new()).unwrap();
            u.close(sock);
            drop(u);
            peer.set_read_timeout(Some(LIMIT)).unwrap();
            let mut rest = Vec::new();
            assert_eq!(peer.read_to_end(&mut rest).unwrap(), 0);
        }
    }

    #[test]
    fn a_wake_is_kept_until_the_next_wait() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            u.waker().wake();
            let start = Instant::now();
            let mut events = Vec::new();
            u.wait(Some(Duration::from_secs(5)), &mut events).unwrap();
            assert!(events.is_empty());
            assert!(start.elapsed() < LIMIT);
            u.wait(Some(Duration::ZERO), &mut events).unwrap();
            assert!(events.is_empty());
            // 掛け直した見張りも起こしを拾う
            u.waker().wake();
            let start = Instant::now();
            u.wait(Some(Duration::from_secs(5)), &mut events).unwrap();
            assert!(start.elapsed() < LIMIT);
        }
    }

    #[test]
    fn a_wake_from_another_thread_ends_a_blocking_wait() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let waker = u.waker();
            let waking = thread::spawn(move || {
                thread::sleep(Duration::from_millis(20));
                waker.wake();
            });
            let start = Instant::now();
            u.wait(Some(LIMIT), &mut Vec::new()).unwrap();
            let waited = start.elapsed();
            assert!(waited >= Duration::from_millis(20));
            assert!(waited < LIMIT, "the wake did not end the wait");
            waking.join().unwrap();
        }
    }

    #[test]
    fn a_zero_wait_does_not_block() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, _peer, mut sock) = pair(&mut u);
            assert!(holds_none(&u.start(1, Step::Peek { sock: &mut sock })));
            let mut events = Vec::new();
            u.wait(Some(Duration::ZERO), &mut events).unwrap();
            assert!(events.is_empty());
        }
    }

    #[test]
    fn a_timed_wait_does_not_return_early() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let start = Instant::now();
            u.wait(Some(Duration::from_millis(20)), &mut Vec::new())
                .unwrap();
            assert!(start.elapsed() >= Duration::from_millis(20));
        }
    }

    /// SIGPIPE をこのスレッドで塞ぎ、`body` の間に届いたかを返す。塞いだシグナルは無視の扱いでも保留に残る。
    fn raises_sigpipe(body: impl FnOnce()) -> bool {
        // SAFETY: 集合はどれも局所の値で、0 で埋めた `sigset_t` は `sigemptyset` の前の値として正しい。
        // `pthread_sigmask`、`sigpending` と `sigtimedwait` は渡した集合を読み書きするだけである
        unsafe {
            let mut pipe: libc::sigset_t = mem::zeroed();
            let mut old: libc::sigset_t = mem::zeroed();
            libc::sigemptyset(&mut pipe);
            libc::sigaddset(&mut pipe, libc::SIGPIPE);
            assert_eq!(libc::pthread_sigmask(libc::SIG_BLOCK, &pipe, &mut old), 0);
            body();
            let mut pending: libc::sigset_t = mem::zeroed();
            assert_eq!(libc::sigpending(&mut pending), 0);
            let raised = libc::sigismember(&pending, libc::SIGPIPE) == 1;
            if raised {
                let zero = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                libc::sigtimedwait(&pipe, ptr::null_mut(), &zero);
            }
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_SETMASK, &old, ptr::null_mut()),
                0
            );
            raised
        }
    }

    #[test]
    fn a_send_to_a_closed_peer_raises_no_sigpipe() {
        for arm in ARMS {
            let Some(mut u) = ring(arm) else { return };
            let (_listener, _peer, mut sock) = pair(&mut u);
            let key = 1;
            let mut data = vec![7u8; 1 << 20];
            let raised = raises_sigpipe(|| {
                // ring に出た送りは、書きを閉じると EPIPE で終わる
                fill(&mut u, &mut sock, key, &mut data);
                u.wait(Some(Duration::ZERO), &mut Vec::new()).unwrap();
                u.shutdown_write(&mut sock).unwrap();
                match next(&mut u, key) {
                    Arrival::Done(Err(e)) => assert_eq!(e.kind(), io::ErrorKind::BrokenPipe),
                    _ => panic!("the queued send did not fail"),
                }
                // 同期の送りも同じ
                match u.start(
                    key,
                    Step::Send {
                        sock: &mut sock,
                        buf: buf(&mut data),
                    },
                ) {
                    Start::Done(Err(e)) => assert_eq!(e.kind(), io::ErrorKind::BrokenPipe),
                    _ => panic!("the send did not fail"),
                }
            });
            assert!(!raised, "a send raised SIGPIPE");
        }
    }
}

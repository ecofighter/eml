//! 本物の driver。compio-driver の `Proactor` の上で、unix は polling (epoll と kqueue)、Windows は IOCP を使う。
//! ブロックする呼び出しのプールも `Proactor` のものを使う (docs/spec/vm.md の「時計と driver」、
//! docs/implementation/architecture.md の「外部 crate」)。
//!
//! `Proactor` は `Send` でないので、この driver も作ったスレッドから動かない。eml の側では `Send` を足さない。

use std::collections::HashMap;
use std::io;
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::task::Waker;
use std::time::{Duration, Instant};

use crate::mask::Masked;
use compio_buf::{BufResult, IoBuf, IoBufMut, SetLen};
use compio_driver::op::{Asyncify, Recv, RecvFlags, Send, SendFlags};
use compio_driver::{Key, OpCode, Proactor, PushEntry, SharedFd};
use eml_rt::{Completion, Done, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, RawBuf, Submit};

#[cfg(unix)]
use crate::resolve::{Lookup, Resolver, std_lookup};
#[cfg(unix)]
use compio_buf::IntoInner;
#[cfg(unix)]
use compio_driver::op::{Accept, Connect, Interest, PollOnce};
#[cfg(unix)]
use socket2::{Domain, Protocol, SockAddr, SockRef, Socket, Type};
#[cfg(unix)]
use std::net::{IpAddr, SocketAddr};
#[cfg(unix)]
use std::os::fd::OwnedFd as Fd;
#[cfg(windows)]
use std::os::windows::io::OwnedSocket as Fd;

/// 1回の `poll` に渡す待ちの長さの上限 (約 24.8 日)。compio は待ちの長さをミリ秒の `i32` で OS に渡すので、遠い
/// 期限はここで切る。切った待ちは早く戻るだけなので、`wait` の約束に合う。
const MAX_POLL: Duration = Duration::from_millis(i32::MAX as u64);

/// `quiesce` が取り消しの完了を待つ1回の `poll` の長さ。`CANCEL_LIMIT` を過ぎたかを、この間隔で確かめる。
const CANCEL_POLL: Duration = Duration::from_millis(10);

/// プールのスレッドの数の上限。仕事がこれより多ければ、仕事の `submit` はスレッドが空くまで、安全点を通らずに
/// 呼んだスレッドの上で待つ (docs/spec/vm.md の「時計と driver」)。
const POOL_LIMIT: usize = 32;

/// `quiesce` が取り消した I/O の操作の完了を待つ長さの上限。取り消しの完了はふつうすぐに届くので、これを
/// 過ぎたら届かないものとみなす。
const CANCEL_LIMIT: Duration = Duration::from_secs(5);

/// リスナーの backlog。
#[cfg(unix)]
const BACKLOG: i32 = 1024;

/// 送りの旗。Linux では、閉じた相手への送りに SIGPIPE を出させない。Apple の OS は旗の代わりにソケットごとに
/// `SO_NOSIGPIPE` を立てる (`stream_socket`)。
#[cfg(target_os = "linux")]
const SEND_FLAGS: SendFlags = SendFlags::NOSIGNAL;
#[cfg(not(target_os = "linux"))]
const SEND_FLAGS: SendFlags = SendFlags::empty();

type Job = Box<dyn FnOnce() -> BufResult<usize, ()> + std::marker::Send>;

/// 出した操作の今の compio の鍵。1つの `OpKey` の操作は、書きの出し直し、受けの再試行、接続の次の試行で、
/// compio の鍵をいくつか通ることがある。`wait` で `pop` するまでここに置く。
enum Slot {
    #[cfg(unix)]
    Readable(Key<PollOnce<SharedFd<Fd>>>),
    /// IOCP では、長さ0の `WSARecv` が読めるようになったことを知らせる。
    #[cfg(windows)]
    Readable(Key<Recv<Mem, SharedFd<Fd>>>),
    Read(Key<Recv<Mem, SharedFd<Fd>>>),
    Write(Key<Send<Mem, SharedFd<Fd>>>, Writing),
    Job(Key<Asyncify<Job, ()>>),
    /// 2つ目はリスナーの写しで、受けを出し直すときに使う。
    #[cfg(unix)]
    Accept(Key<Accept<SharedFd<Fd>>>, SharedFd<Fd>),
    #[cfg(unix)]
    Connect(Key<Connect<SharedFd<Fd>>>, Attempt),
}

/// 表に置いた操作。
struct Entry {
    slot: Slot,
    /// `cancel` を頼まれた。出し直しも次の試行もしない。
    cancelled: bool,
}

/// すべてを書くまで続く書き。
struct Writing {
    fd: SharedFd<Fd>,
    buf: RawBuf,
    /// 書き終えたバイトの数。
    written: usize,
}

/// 接続の1つの試行。
#[cfg(unix)]
struct Attempt {
    socket: SharedFd<Fd>,
    peer: SocketAddr,
    /// まだ試していない番地。解決器が返した順である。
    rest: std::vec::IntoIter<SocketAddr>,
}

/// 操作を進めた後の状態。
enum Step {
    Wait(Slot),
    /// 2つ目は `Completion::cancelled` である。
    Done(io::Result<Done>, bool),
}

/// compio の buffer の trait を `RawBuf` に当てる包み。`RawBuf` は別の crate の型なので、ここで包む。
/// `len` は初期化済みの長さで、書きでは `cap`、読みでは 0 から始まる。
struct Mem {
    buf: RawBuf,
    len: usize,
}

impl IoBuf for Mem {
    fn as_init(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: `len <= cap` で、貸した側は、`submit` が `Submit::Done` を返すか完了を取り出すまで記憶を生かし、
        // 一意に保ち、動かさない。記憶は `Bytes` の
        // 物体の空きか、操作の記録が持つ `Foreign` の `Vec<u8>` の空きである。書きの出し直しの `RawBuf` は貸した記憶の
        // 後ろの部分を指す。driver は貸した側と同じスレッドで動く (`RawBuf`。docs/spec/vm.md の「時計と driver」、
        // docs/spec/object-model.md の T10 の行)
        unsafe { std::slice::from_raw_parts(self.buf.ptr, self.len) }
    }
}

impl SetLen for Mem {
    unsafe fn set_len(&mut self, len: usize) {
        debug_assert!(len <= self.buf.cap);
        self.len = len;
    }
}

impl IoBufMut for Mem {
    fn as_uninit(&mut self) -> &mut [MaybeUninit<u8>] {
        // `cap == 0` の `ptr` は記憶を指さない (`RawBuf`)
        if self.buf.cap == 0 {
            return &mut [];
        }
        // SAFETY: `as_init` と同じ。読みの間は貸した側も記憶に触れない。外が書いた長さは貸した側が `bytes_commit`
        // で確定する (docs/spec/object-model.md の T10 の行)
        unsafe { std::slice::from_raw_parts_mut(self.buf.ptr.cast(), self.buf.cap) }
    }
}

/// compio の `Waker` を包む。どのスレッドからも `poll` を起こせる。
struct ProactorWaker(Waker);

impl IoWaker for ProactorWaker {
    fn wake(&self) {
        self.0.wake_by_ref();
    }
}

/// 本物の時計で待ち、OS で読み書きする driver。
pub struct RealDriver {
    proactor: Proactor,
    /// 出してまだ終わったと分かっていない操作。鍵は呼ぶ側が振ったもので、compio の鍵はすべてここにある。
    ops: HashMap<OpKey, Entry>,
    /// `wait` が集めたか、`cancel` がその場で終えた操作。次の `completions` で返す。
    done: Vec<Completion>,
    handles: HashMap<IoHandle, SharedFd<Fd>>,
    bin: Arc<HandleBin>,
    waker: Arc<ProactorWaker>,
    /// 名前の解決のスレッド。接続の名前は compio のプールでなくここで解決する (docs/spec/vm.md の「時計と driver」)。
    #[cfg(unix)]
    resolver: Resolver,
    /// `poll` が想定外の誤りを返した。以後は待たず、新しい完了も返さない。保留の操作の buffer は貸したまま残る。
    broken: Option<(io::ErrorKind, String)>,
    epoch: Instant,
    #[cfg_attr(
        windows,
        expect(
            dead_code,
            reason = "Windows の driver はソケットを扱わず、ハンドルを作らない"
        )
    )]
    next_handle: IoHandle,
}

impl RealDriver {
    #[cfg(unix)]
    pub fn new() -> io::Result<RealDriver> {
        RealDriver::with_lookup(std_lookup)
    }

    #[cfg(windows)]
    pub fn new() -> io::Result<RealDriver> {
        RealDriver::build()
    }

    /// 名前の解決に `lookup` を使う driver を作る。テストと組み込み先が、機械の解決器の代わりを差すためにある。
    /// 状態を持たない `fn` なので、解決のスレッドへ送るものは名前とポートだけで済む。
    #[cfg(unix)]
    pub fn with_lookup(lookup: Lookup) -> io::Result<RealDriver> {
        RealDriver::build(lookup)
    }

    fn build(#[cfg(unix)] lookup: Lookup) -> io::Result<RealDriver> {
        let proactor = Proactor::builder().thread_pool_limit(POOL_LIMIT).build()?;
        let waker = proactor.waker();
        Ok(RealDriver {
            #[cfg(unix)]
            resolver: Resolver::new(lookup, waker.clone()),
            proactor,
            ops: HashMap::new(),
            done: Vec::new(),
            handles: HashMap::new(),
            bin: Arc::default(),
            waker: Arc::new(ProactorWaker(waker)),
            broken: None,
            epoch: Instant::now(),
            next_handle: 0,
        })
    }

    /// 落とされたハンドルを閉じる。
    fn empty_bin(&mut self) {
        let dropped = std::mem::take(
            &mut *self
                .bin
                .dropped
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        for handle in dropped {
            self.close(handle);
        }
    }

    /// `push` が `Ready` を返したら終わった操作とし、`Pending` なら鍵を待つ。polling では `Send` などが `push` の
    /// 中で終わることがある。
    fn push<T: OpCode + 'static>(&mut self, op: T, slot: fn(Key<T>) -> Slot) -> Step {
        match self.proactor.push(op) {
            PushEntry::Pending(k) => Step::Wait(slot(k)),
            PushEntry::Ready(BufResult(result, _)) => Step::Done(result.map(Done::Size), false),
        }
    }

    /// 書きの残りを出す。`push` の中で終わった短い書きは、その場で続きを出す。`cancelled` なら出し直さずに終える。
    fn send(&mut self, mut writing: Writing, cancelled: bool) -> Step {
        loop {
            // poll の backend では、`wait` が `poll` と `pop` を1回で行い、`cancel` は待っている送りをその場で
            // `ECANCELED` にするので、ここに取り消した書きの短い結果は来ない。IOCP は取り消しの前に終わった短い送りの
            // 結果を返しうるので、続きを出さずに終える道を残す
            if cancelled {
                return Step::Done(Err(cancelled_error()), true);
            }
            let left = writing.buf.cap - writing.written;
            // 残りの位置は貸した記憶の中にあるので、指すだけで読み書きしない (`RawBuf`)
            let buf = RawBuf {
                ptr: writing.buf.ptr.wrapping_add(writing.written),
                cap: left,
            };
            let mem = Mem { buf, len: left };
            match self
                .proactor
                .push(Send::new(writing.fd.clone(), mem, SEND_FLAGS))
            {
                PushEntry::Pending(k) => return Step::Wait(Slot::Write(k, writing)),
                PushEntry::Ready(BufResult(result, _)) => {
                    if let Some(result) = sent(result, &mut writing) {
                        return Step::Done(result, false);
                    }
                }
            }
        }
    }

    /// 来た接続を1つ受ける。`push` の中で受けが中止された接続 (`ECONNABORTED`) なら、その場で出し直す。
    #[cfg(unix)]
    fn accept(&mut self, listener: SharedFd<Fd>) -> Step {
        loop {
            match self.proactor.push(Accept::new(listener.clone())) {
                PushEntry::Pending(k) => return Step::Wait(Slot::Accept(k, listener)),
                PushEntry::Ready(BufResult(result, op)) => {
                    if let Some(result) = self.accepted(result, op) {
                        return Step::Done(result, false);
                    }
                }
            }
        }
    }

    /// 受けの結果。出し直すなら `None` である。
    #[cfg(unix)]
    fn accepted(
        &mut self,
        result: io::Result<usize>,
        op: Accept<SharedFd<Fd>>,
    ) -> Option<io::Result<Done>> {
        match result {
            // 相手が受ける前に接続を捨てた。次の接続を待つ
            Err(e) if e.raw_os_error() == Some(libc::ECONNABORTED) => None,
            Err(e) => Some(Err(e)),
            Ok(_) => {
                let (socket, peer) = op.into_inner();
                Some(self.stream(socket, peer.as_socket()))
            }
        }
    }

    /// `addrs` を順に試す。どれもつながらなければ、最後の試行の誤りで終わる。
    #[cfg(unix)]
    fn connect(
        &mut self,
        mut addrs: std::vec::IntoIter<SocketAddr>,
        mut last: Option<io::Error>,
    ) -> Step {
        while let Some(peer) = addrs.next() {
            let socket = match stream_socket(Domain::for_address(peer)) {
                Ok(socket) => SharedFd::new(Fd::from(socket)),
                Err(e) => {
                    last = Some(e);
                    continue;
                }
            };
            match self
                .proactor
                .push(Connect::new(socket.clone(), SockAddr::from(peer)))
            {
                PushEntry::Pending(k) => {
                    let attempt = Attempt {
                        socket,
                        peer,
                        rest: addrs,
                    };
                    return Step::Wait(Slot::Connect(k, attempt));
                }
                PushEntry::Ready(BufResult(Ok(_), _)) => {
                    return Step::Done(Ok(self.connected(socket, peer)), false);
                }
                PushEntry::Ready(BufResult(Err(e), _)) => last = Some(e),
            }
        }
        let e = last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address"));
        Step::Done(Err(e), false)
    }

    /// 受けたソケットをハンドルにする。
    #[cfg(unix)]
    fn stream(&mut self, socket: Socket, peer: Option<SocketAddr>) -> io::Result<Done> {
        #[cfg(target_vendor = "apple")]
        socket.set_nosigpipe(true)?;
        let peer = peer.ok_or_else(|| io::Error::other("the peer has no IP address"))?;
        Ok(self.connected(SharedFd::new(Fd::from(socket)), peer))
    }

    #[cfg(unix)]
    fn connected(&mut self, socket: SharedFd<Fd>, peer: SocketAddr) -> Done {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.handles.insert(handle, socket);
        Done::Stream { handle, peer }
    }

    /// 持ち主のスレッドの上で待ち受けを作る。bind と listen はブロックしないので、プールに回さない。
    #[cfg(unix)]
    fn listen(&mut self, addr: SocketAddr) -> io::Result<Done> {
        let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
        socket.set_reuse_address(true)?;
        socket.set_nonblocking(true)?;
        socket.bind(&SockAddr::from(addr))?;
        socket.listen(BACKLOG)?;
        let port = socket
            .local_addr()?
            .as_socket()
            .map_or(addr.port(), |local| local.port());
        let handle = self.next_handle;
        self.next_handle += 1;
        self.handles.insert(handle, SharedFd::new(Fd::from(socket)));
        Ok(Done::Listener { handle, port })
    }

    /// `handle` の fd を写して返す。保留の操作が写しを持つので、`close` の後も操作が終わるまで OS の資源は残る。
    fn fd(&self, handle: IoHandle) -> Option<SharedFd<Fd>> {
        self.handles.get(&handle).cloned()
    }

    /// `timeout` まで (`None` なら起こされるか操作が終わるまで) `poll` する。`Ok` と、時間切れと、割り込みは戻った
    /// だけと読み、ほかの誤りで壊れた状態になる。polling の backend は起こされたときも時間切れを返す。
    fn poll(&mut self, timeout: Option<Duration>) {
        if self.broken.is_some() {
            return;
        }
        match self.proactor.poll(timeout) {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => self.broken = Some((e.kind(), e.to_string())),
        }
    }

    /// 終わった解決から接続の試行を始め、終わった操作の鍵を `pop` して、結果を `out` に移す。出し直す操作は新しい
    /// 鍵で表に戻す。出し直しはここで行うので、2つの compio の鍵の間に `cancel` が入ることはない。壊れた driver では
    /// 何もしない。
    fn reap(&mut self, out: &mut Vec<Completion>) {
        if self.broken.is_some() {
            return;
        }
        let ops = std::mem::take(&mut self.ops);
        for (key, entry) in ops {
            let cancelled = entry.cancelled;
            let step = match entry.slot {
                Slot::Readable(k) => self.simple(k, cancelled, Slot::Readable),
                Slot::Read(k) => self.simple(k, cancelled, Slot::Read),
                Slot::Job(k) => self.simple(k, cancelled, Slot::Job),
                Slot::Write(k, mut writing) => match pop(&mut self.proactor, k) {
                    Err(k) => Step::Wait(Slot::Write(k, writing)),
                    Ok(BufResult(result, _)) => match sent(result, &mut writing) {
                        Some(result) => finished(result, cancelled),
                        None => self.send(writing, cancelled),
                    },
                },
                #[cfg(unix)]
                Slot::Accept(k, listener) => match pop(&mut self.proactor, k) {
                    Err(k) => Step::Wait(Slot::Accept(k, listener)),
                    Ok(BufResult(result, op)) => match self.accepted(result, op) {
                        Some(result) => finished(result, cancelled),
                        None if cancelled => Step::Done(Err(cancelled_error()), true),
                        None => self.accept(listener),
                    },
                },
                #[cfg(unix)]
                Slot::Connect(k, attempt) => match pop(&mut self.proactor, k) {
                    Err(k) => Step::Wait(Slot::Connect(k, attempt)),
                    Ok(BufResult(Ok(_), _)) => {
                        Step::Done(Ok(self.connected(attempt.socket, attempt.peer)), false)
                    }
                    Ok(BufResult(Err(e), _)) if attempt.rest.len() == 0 => {
                        finished(Err(e), cancelled)
                    }
                    Ok(BufResult(Err(_), _)) if cancelled => {
                        Step::Done(Err(cancelled_error()), true)
                    }
                    Ok(BufResult(Err(e), _)) => self.connect(attempt.rest, Some(e)),
                },
            };
            match step {
                Step::Wait(slot) => {
                    self.ops.insert(key, Entry { slot, cancelled });
                }
                Step::Done(result, cancelled) => out.push(Completion {
                    key,
                    result,
                    cancelled,
                }),
            }
        }
        #[cfg(unix)]
        for (key, answer) in self.resolver.answers() {
            let step = match answer {
                Ok(addrs) => self.connect(addrs.into_iter(), None),
                Err(e) => Step::Done(Err(e), false),
            };
            match step {
                Step::Done(result, cancelled) => out.push(Completion {
                    key,
                    result,
                    cancelled,
                }),
                Step::Wait(slot) => {
                    self.ops.insert(
                        key,
                        Entry {
                            slot,
                            cancelled: false,
                        },
                    );
                }
            }
        }
    }

    /// 続きを持たない操作の鍵を `pop` する。
    fn simple<T: OpCode>(&mut self, k: Key<T>, cancelled: bool, slot: fn(Key<T>) -> Slot) -> Step {
        match pop(&mut self.proactor, k) {
            Err(k) => Step::Wait(slot(k)),
            Ok(BufResult(result, _)) => finished(result.map(Done::Size), cancelled),
        }
    }

    /// 解決の結果を受け取る。
    fn receive_lookups(&mut self) {
        #[cfg(unix)]
        self.resolver.receive();
    }

    /// 受け取って、まだ接続の試行を始めていない解決の答えがあるか。
    fn answered(&self) -> bool {
        #[cfg(unix)]
        {
            self.resolver.answered()
        }
        #[cfg(windows)]
        {
            false
        }
    }

    /// 解決が `key` をまだ持つか。手放した解決も、スレッドが答えるまで鍵を持つ。
    fn resolver_holds(&self, key: OpKey) -> bool {
        #[cfg(unix)]
        {
            self.resolver.holds(key)
        }
        #[cfg(windows)]
        {
            let _ = key;
            false
        }
    }

    /// 待ちか実行中の、放棄していない解決と、受け取ってまだ試行を始めていない解決の数。
    fn lookups(&self) -> usize {
        #[cfg(unix)]
        {
            self.resolver.pending()
        }
        #[cfg(windows)]
        {
            0
        }
    }

    /// 捨てる完了の中身のハンドルを閉じる。
    fn discard(&mut self, completions: impl IntoIterator<Item = Completion>) {
        for completion in completions {
            if let Ok(Done::Listener { handle, .. } | Done::Stream { handle, .. }) =
                completion.result
            {
                self.close(handle);
            }
        }
    }
}

/// 書きの結果を `writing` に足す。すべてを書いたか失敗したら結果を、続きを出すなら `None` を返す。
fn sent(result: io::Result<usize>, writing: &mut Writing) -> Option<io::Result<Done>> {
    match result {
        Err(e) => Some(Err(e)),
        Ok(n) => {
            writing.written += n;
            if writing.written >= writing.buf.cap {
                Some(Ok(Done::Size(writing.buf.cap)))
            } else if n == 0 {
                // 何も書けない送りを出し直すと終わらない
                Some(Err(io::Error::from(io::ErrorKind::WriteZero)))
            } else {
                None
            }
        }
    }
}

/// 操作が着いた結果。取り消しを頼んだ操作が取り消しの誤りで終わったときだけ、取り消しで終わったとする。そのときの
/// 誤りは OS のものでなく `cancelled_error()` にする。実行系は旗で見分けるので、続きをやめた取り消しと同じ文言に
/// そろえておく (docs/spec/vm.md の「時計と driver」)。
fn finished(result: io::Result<Done>, cancelled: bool) -> Step {
    if cancelled && result.as_ref().is_err_and(is_cancel) {
        return Step::Done(Err(cancelled_error()), true);
    }
    Step::Done(result, false)
}

/// compio の取り消しの誤り。poll の backend は取り消した操作を `ECANCELED` で、IOCP は
/// `ERROR_OPERATION_ABORTED` (995) で終える。
fn is_cancel(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::ECANCELED)
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(995)
    }
}

fn cancelled_error() -> io::Error {
    io::Error::other("cancelled")
}

/// ブロックしない TCP のソケット。socket2 は作るときに CLOEXEC を立てる。
#[cfg(unix)]
fn stream_socket(domain: Domain) -> io::Result<Socket> {
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_nonblocking(true)?;
    #[cfg(target_vendor = "apple")]
    socket.set_nosigpipe(true)?;
    Ok(socket)
}

/// 終わっていれば結果と操作を、終わっていなければ鍵を返す。
fn pop<T: OpCode>(proactor: &mut Proactor, key: Key<T>) -> Result<BufResult<usize, T>, Key<T>> {
    match proactor.pop(key) {
        PushEntry::Ready(result) => Ok(result),
        PushEntry::Pending(key) => Err(key),
    }
}

fn no_handle() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no such handle")
}

impl IoDriver for RealDriver {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.empty_bin();
        if let Some((kind, message)) = &self.broken {
            return Err(io::Error::new(*kind, message.clone()));
        }
        self.receive_lookups();
        // 届いた完了か解決の答えがあれば待たずに OS を1回だけ見る。解決の答えは `reap` が受けるまで残るので、
        // 見ずに戻ると呼ぶ側は回り続ける。`Ok(false)` の確かめを `u64::MAX` の写しより先に置くので、
        // `Some(u64::MAX)` は保留がなくても起こしを待つ
        let timeout = if !self.done.is_empty() || self.answered() {
            Some(Duration::ZERO)
        } else {
            match deadline_ms {
                None if self.ops.is_empty() && self.lookups() == 0 => return Ok(false),
                None | Some(u64::MAX) => None,
                Some(deadline_ms) => Some(
                    Duration::from_millis(deadline_ms)
                        .saturating_sub(self.now())
                        .min(MAX_POLL),
                ),
            }
        };
        self.poll(timeout);
        self.receive_lookups();
        let mut done = std::mem::take(&mut self.done);
        self.reap(&mut done);
        self.done = done;
        match &self.broken {
            Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
            None => Ok(true),
        }
    }

    fn submit(&mut self, key: OpKey, op: Op) -> Submit {
        // 渡していない完了の鍵と、解決が持つ鍵もまだ保留である。手放した解決の鍵は、スレッドが答えるまで使い回せない
        // (`Resolver::cancel`)
        debug_assert!(
            !self.ops.contains_key(&key)
                && self.done.iter().all(|c| c.key != key)
                && !self.resolver_holds(key),
            "a pending key is submitted again"
        );
        // 受け付けで終わる操作が続くと `wait` も `completions` も呼ばれないので、落とされたハンドルはここでも閉じる
        self.empty_bin();
        let step = match op {
            Op::Readable { handle } | Op::Read { handle, .. } | Op::Write { handle, .. }
                if !self.handles.contains_key(&handle) =>
            {
                Step::Done(Err(no_handle()), false)
            }
            #[cfg(unix)]
            Op::Readable { handle } => {
                let fd = self.fd(handle).expect("the handle is checked above");
                self.push(PollOnce::new(fd, Interest::Readable), Slot::Readable)
            }
            #[cfg(windows)]
            Op::Readable { handle } => {
                let fd = self.fd(handle).expect("the handle is checked above");
                let mem = Mem {
                    buf: RawBuf {
                        ptr: std::ptr::NonNull::dangling().as_ptr(),
                        cap: 0,
                    },
                    len: 0,
                };
                self.push(Recv::new(fd, mem, RecvFlags::empty()), Slot::Readable)
            }
            Op::Read { handle, buf } => {
                let fd = self.fd(handle).expect("the handle is checked above");
                let mem = Mem { buf, len: 0 };
                self.push(Recv::new(fd, mem, RecvFlags::empty()), Slot::Read)
            }
            Op::Write { handle, buf } => {
                let fd = self.fd(handle).expect("the handle is checked above");
                let writing = Writing {
                    fd,
                    buf,
                    written: 0,
                };
                self.send(writing, false)
            }
            Op::Job(job) => {
                let job: Job = Box::new(move || BufResult(job(), ()));
                // プールのスレッドは `push` を呼んだスレッドから生まれ、塞ぎを受け継ぐ (docs/spec/vm.md の「上限と割り込み」)
                let _masked = Masked::new();
                self.push(Asyncify::new(job), Slot::Job)
            }
            #[cfg(unix)]
            Op::Listen { addr } => Step::Done(self.listen(addr), false),
            #[cfg(unix)]
            Op::Accept { listener } => match self.fd(listener) {
                Some(fd) => self.accept(fd),
                None => Step::Done(Err(no_handle()), false),
            },
            #[cfg(unix)]
            Op::Connect { host, port } => match host.parse::<IpAddr>() {
                Ok(ip) => self.connect(vec![SocketAddr::new(ip, port)].into_iter(), None),
                Err(_) => match self.resolver.submit(key, host, port) {
                    Ok(()) => return Submit::Pending,
                    Err(e) => Step::Done(Err(e), false),
                },
            },
            // Windows のソケットは `net-windows` で扱う
            #[cfg(windows)]
            Op::Listen { .. } | Op::Accept { .. } | Op::Connect { .. } => {
                Step::Done(Err(io::Error::from(io::ErrorKind::Unsupported)), false)
            }
        };
        match step {
            Step::Done(result, _) => Submit::Done(result),
            Step::Wait(slot) => {
                self.ops.insert(
                    key,
                    Entry {
                        slot,
                        cancelled: false,
                    },
                );
                Submit::Pending
            }
        }
    }

    fn cancel(&mut self, key: OpKey) {
        let Some(entry) = self.ops.get_mut(&key) else {
            // 名前の解決は結果を待たずに手放す (docs/spec/vm.md の「時計と driver」)。手放したスレッドの答えは鍵で
            // 捨てるので、その鍵はスレッドが答えるまで次の操作に使い回せない。VM は記録を放すたびに generation を
            // 進めるので、鍵を使い回さない
            #[cfg(unix)]
            if self.resolver.cancel(key) {
                self.done.push(Completion {
                    key,
                    result: Err(cancelled_error()),
                    cancelled: true,
                });
            }
            return;
        };
        // 鍵を残したまま取り消し、完了は `pop` で受ける。プールの仕事は取り消せない
        let token = match &entry.slot {
            Slot::Readable(k) => self.proactor.register_cancel(k),
            Slot::Read(k) => self.proactor.register_cancel(k),
            Slot::Write(k, _) => self.proactor.register_cancel(k),
            #[cfg(unix)]
            Slot::Accept(k, _) => self.proactor.register_cancel(k),
            #[cfg(unix)]
            Slot::Connect(k, _) => self.proactor.register_cancel(k),
            Slot::Job(_) => return,
        };
        entry.cancelled = true;
        self.proactor.cancel_token(token);
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        self.empty_bin();
        // 壊れた後は、集めてあった結果も含めて何も渡さない。保留は `quiesce` の後も残り、実行系はそれを見て
        // 機械を poisoned の状態にする (docs/spec/vm.md の「時計と driver」)
        if self.broken.is_some() {
            return;
        }
        out.append(&mut self.done);
    }

    fn pending(&self) -> usize {
        self.ops.len() + self.done.len() + self.lookups()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        Arc::clone(&self.waker) as Arc<dyn IoWaker>
    }

    fn bin(&self) -> Arc<HandleBin> {
        Arc::clone(&self.bin)
    }

    #[cfg(unix)]
    fn shutdown_write(&mut self, handle: IoHandle) -> io::Result<()> {
        let fd = self.fd(handle).ok_or_else(no_handle)?;
        SockRef::from(&fd).shutdown(std::net::Shutdown::Write)
    }

    /// Windows の driver はストリームを作らないので、呼ばれない。
    #[cfg(windows)]
    fn shutdown_write(&mut self, _handle: IoHandle) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    #[cfg(unix)]
    fn set_nodelay(&mut self, handle: IoHandle, on: bool) -> io::Result<()> {
        let fd = self.fd(handle).ok_or_else(no_handle)?;
        SockRef::from(&fd).set_tcp_nodelay(on)
    }

    /// Windows の driver はストリームを作らないので、呼ばれない。
    #[cfg(windows)]
    fn set_nodelay(&mut self, _handle: IoHandle, _on: bool) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn close(&mut self, handle: IoHandle) {
        self.handles.remove(&handle);
    }

    fn quiesce(&mut self) {
        self.empty_bin();
        // 名前の解決は待たずに手放す。走っているスレッドは結果を送れずに自分で終わる
        #[cfg(unix)]
        self.resolver.abandon_all();
        let done = std::mem::take(&mut self.done);
        self.discard(done);
        let keys: Vec<OpKey> = self.ops.keys().copied().collect();
        for key in keys {
            self.cancel(key);
        }
        // 取り消しの完了も、走っているプールの仕事も、届くまで待つ。壊れた driver は待てないので、保留を残して戻る
        let mut limit = None;
        let mut discarded = Vec::new();
        while !self.ops.is_empty() && self.broken.is_none() {
            self.poll(Some(CANCEL_POLL));
            self.reap(&mut discarded);
            let completions = std::mem::take(&mut discarded);
            self.discard(completions);
            // polling の backend は、取り消しで関心を外す `epoll_ctl` などが失敗すると、完了を出さないまま操作を
            // 落とす。その操作は二度と終わらないので、期限を過ぎても I/O の操作が残れば壊れた状態にして戻る。
            // 仕事はいつか終わるので、仕事が残る間は待ち続け、期限は仕事がなくなってから数える
            let jobs_left = self
                .ops
                .values()
                .any(|entry| matches!(entry.slot, Slot::Job(_)));
            if jobs_left || self.ops.is_empty() {
                continue;
            }
            let limit = *limit.get_or_insert_with(|| Instant::now() + CANCEL_LIMIT);
            if Instant::now() >= limit {
                self.broken = Some((
                    io::ErrorKind::Other,
                    "a cancelled op did not complete".to_string(),
                ));
            }
        }
    }
}

impl Drop for RealDriver {
    /// 保留の仕事の鍵を、プールのスレッドと持ち主のスレッドが同時に落とさないようにする。compio は仕事を出すとき、
    /// 鍵の写しを凍らせてプールのスレッドへ送る。凍らせた鍵の間は、ほかのスレッドがその数え方の状態 (ただのセルで、
    /// atomic でない) に触れてはならない。仕事が終わったとき、`Proactor` がなければ、プールのスレッドはその写しを
    /// 自分で落とす。そのため、ここで仕事を待たずに `ops` の鍵を落とすと、2つのスレッドが同じ数を書き換える。
    /// 壊れていなければ `quiesce` で仕事を待ちきる。壊れていて待てなければ、残った鍵を手放さずに漏らす
    /// (docs/spec/vm.md の「時計と driver」)。
    fn drop(&mut self) {
        if self.broken.is_none() {
            self.quiesce();
        }
        if !self.ops.is_empty() {
            std::mem::forget(std::mem::take(&mut self.ops));
        }
        self.empty_bin();
    }
}

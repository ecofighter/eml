# 付録 A2 (S2): epoll と kqueue の backend

> この付録は計画の本体 (`../2026-10-30-io-backends.md`) の一部である。本体の「5.3 方針の修正と食い違いの解き方」がこの付録より優先する。`[V]` は設計の時に scratch の試作や probe で確かめた事実を指す。試作と probe のファイル (`s1-probe/` などの名前で引いているもの) は残っていない。S1〜S4 は付録 A1〜A4 を指す。


[V] は確かめた事実 (方法を併記)、[I] は推論。S1 第1部 (§1〜§8) を前提とする。S1 の配置からの細かな変更は §6 の U-0 に書く。probe は `s2-probe/` にある (Linux 6.18.44、x86_64、glibc 2.39、libc 0.2.191)。r1 からの変更は第2部の表のとおり。

# 第1部: 改訂した成果物 (全文)

## 1. 結論

### 全体の形 (Q6 を先に)

ソケット、歩、fd の表は epoll と kqueue で同じなので、すべて `sys/unix.rs` に置く。OS ごとのファイルは「登録、待ち、waker」の3つを `Poller` trait で実装するだけにする。

`sys/mod.rs` は S3 が持つ (`type Os` の Windows の行、`lookup`、`millis`)。S2 がそこに足すのは次の5行だけである。

```rust
#[cfg(unix)] mod unix;
#[cfg(target_os = "linux")] mod epoll;
#[cfg(target_vendor = "apple")] mod kqueue;
#[cfg(target_os = "linux")] pub(crate) type Os = unix::Unix<epoll::Epoll>;
#[cfg(target_vendor = "apple")] pub(crate) type Os = unix::Unix<kqueue::Kqueue>;
```

S3 の `fn millis(d: Duration, max: u32) -> u32` (切り上げ、`max` で切る) を epoll が `super::millis(d, i32::MAX as u32)` で使う。

```
sys/unix.rs     #![allow(unsafe_code)]  Unix<P>: Backend、Sock、Row/Rows、token、Poller と Ready、歩、ソケットの作成、
                sockaddr の写し、errno の扱い、Instant の時計、spawn (mask::spawn_masked)
sys/epoll.rs    #![allow(unsafe_code)]  Epoll: Poller (epoll_create1 / epoll_ctl / epoll_pwait)、EventFd: IoWaker
sys/kqueue.rs   #![allow(unsafe_code)]  Kqueue: Poller (kqueue / kevent)、User: IoWaker (EVFILT_USER)
```

構築は inherent の `pub(crate) fn new() -> io::Result<Self>` (S3 の `Iocp::new` も同じ形。合意済み)。`real.rs` は `Driver::new(sys::Os::new()?, lookup)` と呼ぶ。

```rust
// sys/unix.rs
pub(crate) struct Unix<P> {
    poller: P,
    waker: Arc<dyn IoWaker>,   // poller が作った waker の写し (G-11)
    rows: Rows,
    epoch: Instant,
}

/// 核が持つソケット。核は中を見ない (S1 §1)。`fd` を閉じるのは `Backend::close` だけ
pub(crate) struct Sock { fd: OwnedFd, row: u32, gen: u32 }

/// 1つの event が起こす向き。`fresh` は「この読みの知らせは取り出した時点で読める」と poller が保証するとき真
#[derive(Clone, Copy)]
pub(crate) struct Ready { pub read: bool, pub write: bool, pub fresh: bool }

// `pub(crate)` にする: `pub(super)` では `impl<P: Poller> Unix<P>` が `-D warnings` の下で
// `private_bounds` に当たる [V: s2-probe/lintcheck で `cargo clippy --all-targets -- -D warnings`。
// pub(super) は error、pub(crate) は通る]。`mod unix` 自体は private なので外へは出ない
pub(crate) trait Poller: Sized {
    /// poller と、鳴らす OS の物体を自ら持つ waker。waker の登録もここで行う
    fn new() -> io::Result<(Self, Arc<dyn IoWaker>)>;
    /// `fd` を edge で1回だけ登録する。`write` が偽ならリスナーで、読みの関心だけ
    fn register(&mut self, fd: BorrowedFd<'_>, token: u64, write: bool) -> io::Result<()>;
    /// `timeout` まで待ち、届いた event を `deliver(token, ready)` で渡す。起こしの印はここで消費し、渡さない。
    /// 時間切れ、起こし、EINTR は Ok(())。
    fn poll(&mut self, timeout: Option<Duration>, deliver: impl FnMut(u64, Ready)) -> io::Result<()>;
}
```

### Q1. fd の表、登録、`wait` の写し方

```rust
struct Row {
    gen: u32,
    read: Option<OpKey>,    // 読みの向き (Peek/Recv/Accept) の待ち手。列は持たない (G-5)
    write: Option<OpKey>,   // 書きの向き (Send/Connect) の待ち手
    ready: bool,            // 読みの記憶。event で立ち、読みの向きの EAGAIN でだけ下りる
    fresh: bool,            // 読めると保証された event の後、まだ読みの向きを触っていない
    connecting: bool,       // connect() を出して結果を読んでいない
}
#[derive(Default)]
struct Rows { rows: Vec<Row>, free: Vec<u32> }

const WAKE: u64 = u64::MAX;                       // 行 u32::MAX は作らない (adopt で assert)
fn token(row: u32, gen: u32) -> u64 { (u64::from(gen) << 32) | u64::from(row) }   // OS に渡す値を作る唯一の所
```

- 行を作る (`Rows::adopt() -> (row, gen)`): `free.pop()` があればそれ、なければ `rows.push`。新しい行は `read/write = None`、`ready = fresh = connecting = false`。作るのは `listen`、`socket`、`Accept` の `Accepted` の3か所だけ。
- 行を放す (`Rows::release(row, gen)`): `Backend::close` (と、`listen`/`Accept` の中で登録に失敗したとき) だけ。`debug_assert_eq!(gen, rows[row].gen)`、全欄を初期値に戻して `gen = gen.wrapping_add(1)`、`free.push(row)`。その後 `drop(sock.fd)` (close(2))。`EPOLL_CTL_DEL` も `EV_DELETE` も出さない。閉じれば登録は消えるからである。fork の子が exec の前に fd の写しを持つ間は epoll の登録が残るが、その event は古い世代を運ぶので `deliver` が捨てる。新しいソケットは別の file なので、`EPOLL_CTL_ADD` は衝突しない [I: epoll の鍵は (file, fd)。epoll(7)]。
- 初期値を `ready = false` にしてよい理由: 登録 (`EPOLL_CTL_ADD`、`EV_ADD`) はその時点の状態を調べ、読めれば event を1つ積む。登録の前に着いたデータも event になる [V: probe 1。データのあるソケットの ADD の直後に `IN|OUT`、データのないソケットは `OUT` だけ。kqueue は I で、§4 の仮定1]。よって「`ready == false` のとき、データがあれば OS に未読の event がある」が常に成り立つ。
- 登録の旗:
  - epoll: ストリームは `EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLET`、リスナーは `EPOLLIN | EPOLLRDHUP | EPOLLET`。`u64 = token`。
  - kqueue: 1回の `kevent(kq, changes, n, null, 0, &ZERO)` で、`EVFILT_READ` と (ストリームなら) `EVFILT_WRITE` を `EV_ADD | EV_CLEAR` で登録する。`ident = fd`、`udata = ptr::without_provenance_mut(token as usize)`。`EV_RECEIPT` は使わない (`nevents == 0` なので、変更の誤りは `-1` と errno で返る)。`EINTR` が返ったら同じ `kevent` を出し直す。`EV_ADD` は既にある登録を同じ値で上書きするだけなので、適用済みでも二重にならない (R3 §5 の kqueue(2)「EV_ADD of an existing event modifies it」)。G-2 の `EINTR` の例外の内である。
  - 登録する時点: リスナーは `listen` の中、受けたソケットは `start(Accept)` の中、接続のソケットは最初の `start(Connect)` の `connect()` の直後 (S1 の規則)。以後 `close` まで変えない。登録の呼び出しは `unix.rs` の `fn register_row(&mut self, fd, row, gen, write)` の1か所に集める (`listen`、`Accept`、`Connect` がそれを呼ぶ)。
- `wait(timeout, events)`: `self.poller.poll(timeout, |token, ready| self.rows.deliver(token, ready, events))`。closure は `rows` と `poller` を別々に借りる。`events` は核が空にして渡し (合意済み)、backend は足すだけ。

```rust
fn deliver(&mut self, token: u64, ready: Ready, events: &mut Vec<Event<Sock>>) {
    let (row, gen) = (token as u32, (token >> 32) as u32);
    let Some(r) = self.rows.get_mut(row as usize) else { return };
    if r.gen != gen { return }                                  // 閉じた行の古い event (G-6)
    if ready.read {
        r.ready = true;
        r.fresh = ready.fresh;
        if let Some(key) = r.read { events.push(Event { key, arrival: Arrival::Retry }) }
    }
    if ready.write {
        if let Some(key) = r.write { events.push(Event { key, arrival: Arrival::Retry }) }
    }
}
```

- OS の event から `Ready` への写し:
  - epoll: `read = events & (EPOLLIN | EPOLLRDHUP | EPOLLHUP | EPOLLERR) != 0`、`write = events & (EPOLLOUT | EPOLLHUP | EPOLLERR) != 0`、`fresh = read`。`EPOLLERR`/`EPOLLHUP` は両方の向きを起こす。RST では `IN|OUT|ERR|HUP|RDHUP` が来る [V: probe 5]。相手の `shutdown(WR)` は `IN|OUT|RDHUP` [V: probe 7]。自分の `shutdown(WR)` は `OUT` だけで、読みを立てない [V: probe 6]。epoll は取り出すときに状態を調べ直すので、`IN` があれば取り出した時点で読める [V: probe 2。積んだ後に読み尽くすと `OUT` だけで取り出される]。
  - kqueue: `EVFILT_READ` は `read` (`EV_EOF` も、誤りの `fflags` も含む)、`EVFILT_WRITE` は `write`、`EVFILT_USER` は何も渡さない。`fresh = read` を既定にする。kqueue が取り出しで filter を評価し直すことは [I] なので、単体テスト `a_drained_event_does_not_make_a_probe_fresh` (Q7) を macOS で流して確かめる。落ちたときの代わり (`Readable` を正しく保つ) は `kqueue.rs` の1か所を `fresh: false` にすることである。そうすると `Peek` は記憶では終わらず必ず覗く。`ready` は保ったままなので正しさは変わらず、macOS の待つ読み (空きなし) が 3 から 4 syscall になるだけである。`EVFILT_WRITE` の `EV_EOF` では読みを立てない。自分の `shutdown(WR)` で立つので、読みを立てると `Peek` が誤って `Size(0)` を返す [I: XNU の `filt_sowrite` は `SS_CANTSENDMORE` で `EV_EOF`]。RST では両方の filter が自分で発火する [I]。
- 待ち手を忘れるのは、`start` が `Done` を返したとき、`cancel` のとき、`close` (行の初期化) のときだけ。`Retry` を出しても消さない (S1 G-4)。

### Q2. 各歩 (`start`) と `cancel`

errno の扱いは共通である。呼び出しは `fn syscall(f: impl FnMut() -> isize) -> io::Result<usize>` (と `c_int` 版) を通し、`EINTR` だけをその場で繰り返す (G-2 の唯一の例外。`connect` は下の扱い)。誤りは `io::Error::last_os_error()` のまま返し、核は `ErrorKind` だけを見る (std の対応: `ECONNABORTED` → `ConnectionAborted`、`ECONNREFUSED` → `ConnectionRefused`、`ECONNRESET` → `ConnectionReset`、`EPIPE` → `BrokenPipe`、`EAGAIN`/`EWOULDBLOCK` → `WouldBlock`)。

送り受けの旗 (すべての `recv`/`send` に `MSG_DONTWAIT` を付ける。ソケットは作るときにすべて非ブロックにするので念のためだが、旗は syscall を増やさない):

```rust
const RECV: c_int = libc::MSG_DONTWAIT;
const PEEK: c_int = libc::MSG_PEEK | libc::MSG_DONTWAIT;
#[cfg(target_os = "linux")]      const SEND: c_int = libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT;
#[cfg(target_vendor = "apple")]  const SEND: c_int = libc::MSG_DONTWAIT;   // SO_NOSIGPIPE をソケットに立てる
```

各歩の擬似コード (どれも試し直しを持たない。`r = rows.get(sock)` は `debug_assert_eq!(r.gen, sock.gen)` を含む):

```
Peek { sock }:                                   // 読みの向き
  if r.ready && r.fresh  { r.fresh = false; r.read = None; return Done(Ok(Size(0))) }   // syscall 0
  if !r.ready            { r.read = Some(key); return Pending(Hold::None) }             // syscall 0
  r.fresh = false
  match recv(fd, &mut [MaybeUninit<u8>; 1] (局所), 1, PEEK):
    Ok(_)          => { r.read = None; Done(Ok(Size(0))) }      // 1 バイトか EOF (0)
    Err(WouldBlock)=> { r.ready = false; r.read = Some(key); Pending(Hold::None) }
    Err(e)         => { r.read = None; Done(Err(e)) }           // ECONNRESET など。覗きは誤りを消費する

Recv { sock, buf }:                              // 読みの向き。記憶によらず必ず試す
  r.fresh = false
  match recv(fd, buf.ptr, buf.cap, RECV):
    Ok(n)          => { r.read = None; Done(Ok(Size(n))) }      // n == 0 は EOF。ready は下ろさない
    Err(WouldBlock)=> { r.ready = false; r.read = Some(key); Pending(Hold::None) }
    Err(e)         => { r.read = None; Done(Err(e)) }

Send { sock, buf }:                              // 書きの向き。書きの記憶は持たない
  match send(fd, buf.ptr, buf.cap, SEND):
    Ok(n)          => { r.write = None; Done(Ok(Size(n))) }     // 短い書きの残りは核が出す (G-2)
    Err(WouldBlock)=> { r.write = Some(key); Pending(Hold::None) }
    Err(e)         => { r.write = None; Done(Err(e)) }          // EPIPE、ECONNRESET

Accept { listener }:                             // 読みの向き。記憶によらず必ず試す
  r.fresh = false
  (fd2, storage, len) = match accept_raw(listener.fd):
    Linux: accept4(fd, &storage, &len, SOCK_NONBLOCK | SOCK_CLOEXEC)
    Apple: accept(fd, &storage, &len); fcntl(fd2, F_SETFD, FD_CLOEXEC); ioctl(fd2, FIONBIO, &1);
           setsockopt(fd2, SOL_SOCKET, SO_NOSIGPIPE, 1)
    Err(WouldBlock)            => { r.ready = false; r.read = Some(key); return Pending(Hold::None) }
    Err(e) if transient(&e)    => { r.read = None; return Done(Err(io::Error::new(ConnectionAborted, e))) }
    Err(e)                     => { r.read = None; return Done(Err(e)) }   // ECONNABORTED はそのまま ConnectionAborted
  r.read = None
  peer = from_storage(&storage, len) else { drop(fd2); return Done(Err(other("the peer has no IP address"))) }
  (row, gen) = rows.adopt()
  if let Err(e) = self.register_row(fd2, row, gen, true) { rows.release(row, gen); drop(fd2); return Done(Err(e)) }
  Done(Ok(Accepted { sock: Sock { fd: fd2, row, gen }, peer }))

Connect { sock, peer }:                          // 書きの向き
  if !r.connecting:
    let done = match connect(fd, RawAddr::new(peer)) {   // EINTR を繰り返さない (下)
      Ok(())                        => true,
      Err(EINPROGRESS) | Err(EINTR) => false,
      Err(e)                        => return Done(Err(e)),          // 登録しない。ソケットは核が close する
    };
    if let Err(e) = self.register_row(fd, sock.row, sock.gen, true) { return Done(Err(e)) }   // 登録の呼び出しは1つ
    if done { return Done(Ok(Connected)) }
    r.connecting = true; r.write = Some(key); Pending(Hold::None)
  else:                                          // Retry の後。connect() を出し直さない (G-4)
    err = getsockopt(fd, SOL_SOCKET, SO_ERROR)
    if err != 0                     => { r.connecting = false; r.write = None; Done(Err(from_raw_os_error(err))) }
    match getpeername(fd):
      Ok(_)                         => { r.connecting = false; r.write = None; Done(Ok(Connected)) }
      Err(ENOTCONN)                 => { r.write = Some(key); Pending(Hold::None) }   // 早すぎた Retry
      Err(e)                        => { r.connecting = false; r.write = None; Done(Err(e)) }
```

- `transient(e)` (受けの出し直しに回す誤り。Linux だけ): `ENETDOWN`、`EPROTO`、`ENOPROTOOPT`、`EHOSTDOWN`、`ENONET`、`EHOSTUNREACH`、`EOPNOTSUPP`、`ENETUNREACH`。accept(2) が「TCP/IP では `EAGAIN` のように扱って試し直せ」と挙げるもので (R3 §2)、来ていた接続1本の誤りである。backend はこれを `ConnectionAborted` の kind に包んで返し、核の今の規則 (`ConnectionAborted` は同じ鍵で受けを出し直す) に乗せる。核への要望はなく、G-2 (backend は試し直さない) と G-16 (核は errno を見ない) を保つ。元の誤りは `io::Error::new` の中身として文言に残る。Apple は `ECONNABORTED` のほかに足さない (`ENONET` などの定数がなく、accept(2) にこの勧めがない)。`EMFILE`/`ENFILE`/`ENOBUFS`/`ENOMEM` は資源の誤りなので、そのまま返して eml のプログラムに見せる。待ち行列の接続は失われない。次の `Accept` は記憶によらず `accept4` を試すので、ET の辺が来なくても受けられる。今の driver は `ECONNABORTED` だけを出し直す [V: `real.rs:307`]。この写しで振る舞いが少し広がるが、テストの成否は変わらない。
- `connect` の `EINTR` は進行中と読む。POSIX の connect() は、割り込まれた接続の要求は取り消されず裏で続くと定める [I: POSIX の記述の記憶。この箱に man 3p がなく確かめていない]。非ブロックのソケットでは事実上起きない守りの分岐で、テストはない。誤りと読むと、裏でつながったソケットを捨てて次の番地へ進むので、こちらにする。
- `Peek` を記憶で答えたときにも `fresh` を下ろす (S1 の文言への追加。§6 の U-1)。
- `cancel(key, step)`: 歩の向きの待ち手が `Some(key)` なら `None` にし、`Ok(Cancel::Done)` を返す。OS は呼ばない。`Connect` の `connecting` はそのまま残す (核がソケットを `close` する)。`Err` を返す道はない。
- `RawBuf` の `ptr` を読むのは `recv_raw(fd, buf: RawBuf, flags)` と `send_raw(fd, buf, flags)` の2つの関数の中だけで、`libc::recv(fd, buf.ptr.cast(), buf.cap, flags)` に生のポインタと長さを渡す。slice は作らない。`cap == 0` の `Send` はそのまま `send(…, 0)` を出す (カーネルは触れない。核が先に終えてもよい)。

### Q3. `listen`、`socket`、`shutdown_write`、`set_nodelay`、`close`

```rust
/// ブロックせず CLOEXEC の TCP ソケット
fn stream_socket(addr: &SocketAddr) -> io::Result<OwnedFd> {
    let domain = if addr.is_ipv4() { libc::AF_INET } else { libc::AF_INET6 };
    #[cfg(target_os = "linux")]
    let fd = owned(syscall(|| socket(domain, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, IPPROTO_TCP))?);   // 1 syscall
    #[cfg(target_vendor = "apple")]
    let fd = {
        let fd = owned(socket(domain, SOCK_STREAM, IPPROTO_TCP)?);   // 以後の失敗では OwnedFd の drop が閉じる
        fcntl(fd, F_SETFD, FD_CLOEXEC)?;
        ioctl(fd, FIONBIO, &1)?;                                    // std の set_nonblocking と同じ
        setsockopt_int(fd, SOL_SOCKET, SO_NOSIGPIPE, 1)?;
        fd
    };
    Ok(fd)
}
```

- `listen(addr)`: `stream_socket` → `setsockopt(SOL_SOCKET, SO_REUSEADDR, 1)` → `bind(RawAddr::new(addr))` → `listen(BACKLOG = 1024)` → `getsockname` でポートを読む (IP でなければ `addr.port()`。今と同じ) → `adopt` → `register_row(write = false)` (失敗なら `release` して `Err`)。`IPV6_V6ONLY` は触らない (今と同じ)。
- `socket(peer)`: `stream_socket(&peer)` → `adopt` (登録しない)。
- `shutdown_write(sock)`: `shutdown(fd, SHUT_WR)`。`set_nodelay(sock, on)`: `setsockopt(IPPROTO_TCP, TCP_NODELAY, on as c_int)`。どちらも行に触れない。
- `close(sock)`: Q1 の `release` の後に `drop(sock.fd)`。
- `sockaddr` の写し (全文。Linux の型で通り、v4 は bind と getsockname、v6 はソケットなしの往復で確かめた [V: `s2-probe/src/bin/addr.rs`]。Apple の `sin_len`/`sin6_len` は I):

```rust
enum RawAddr { V4(libc::sockaddr_in), V6(libc::sockaddr_in6) }

impl RawAddr {
    fn new(addr: SocketAddr) -> RawAddr {
        match addr {
            SocketAddr::V4(a) => {
                // SAFETY: sockaddr_in は整数だけの C の構造体で、すべて 0 は正しい値である
                let mut s: libc::sockaddr_in = unsafe { mem::zeroed() };
                #[cfg(target_vendor = "apple")]
                { s.sin_len = size_of::<libc::sockaddr_in>() as u8; }
                s.sin_family = libc::AF_INET as libc::sa_family_t;
                s.sin_port = a.port().to_be();
                s.sin_addr = libc::in_addr { s_addr: u32::from_ne_bytes(a.ip().octets()) };
                RawAddr::V4(s)
            }
            SocketAddr::V6(a) => {
                // SAFETY: sockaddr_in6 は整数と配列だけの C の構造体で、すべて 0 は正しい値である
                let mut s: libc::sockaddr_in6 = unsafe { mem::zeroed() };
                #[cfg(target_vendor = "apple")]
                { s.sin6_len = size_of::<libc::sockaddr_in6>() as u8; }
                s.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                s.sin6_port = a.port().to_be();
                s.sin6_flowinfo = a.flowinfo();          // std と同じく並べ替えない
                s.sin6_addr = libc::in6_addr { s6_addr: a.ip().octets() };
                s.sin6_scope_id = a.scope_id();
                RawAddr::V6(s)
            }
        }
    }

    fn as_ptr(&self) -> (*const libc::sockaddr, libc::socklen_t) {
        match self {
            RawAddr::V4(s) => ((s as *const libc::sockaddr_in).cast(), size_of::<libc::sockaddr_in>() as libc::socklen_t),
            RawAddr::V6(s) => ((s as *const libc::sockaddr_in6).cast(), size_of::<libc::sockaddr_in6>() as libc::socklen_t),
        }
    }
}

fn from_storage(s: &libc::sockaddr_storage, len: libc::socklen_t) -> Option<SocketAddr> {
    match libc::c_int::from(s.ss_family) {
        libc::AF_INET if len as usize >= size_of::<libc::sockaddr_in>() => {
            // SAFETY: 族が AF_INET で長さが足りる。sockaddr_storage は sockaddr_in より大きく、整列も強い
            let a = unsafe { &*(s as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>() };
            Some(SocketAddr::from((Ipv4Addr::from(a.sin_addr.s_addr.to_ne_bytes()), u16::from_be(a.sin_port))))
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
```

### Q4. waker、待ち、時計

- Linux (`epoll.rs`):
  - `struct EventFd(OwnedFd)` と `Epoll { ep: OwnedFd, wake: Arc<EventFd>, events: Vec<libc::epoll_event> }`。`events` は長さ `EVENTS = 256` で作って使い回す。`new` は `epoll_create1(EPOLL_CLOEXEC)`、`eventfd(0, EFD_CLOEXEC | EFD_NONBLOCK)` を作り、eventfd を `EPOLLIN` (level。`EPOLLET` なし) と `u64 = WAKE` で登録する。これが driver の作成ごとの `epoll_ctl` 1回である (Q5 の数に入れる)。waker として `Arc<EventFd>` の写しを `Arc<dyn IoWaker>` で返す。
  - `wake()`: `write(fd, &1u64, 8)` を `EINTR` のときだけ繰り返す。`EAGAIN` (数が上限) は無視する (既に鳴っている)。
  - `poll`: `epoll_pwait(ep, events, 256, ms, null())`。sigmask が null なので `epoll_wait` と同じ意味で、strace では `epoll_pwait` と出る [V: x86_64。aarch64 は `epoll_wait` の syscall がないので I]。`ms = timeout.map_or(-1, |d| super::millis(d, i32::MAX as u32) as c_int)` で、切り上げ、上限で切るのは早く戻る向きだけ (G-8)。`n < 0` は `EINTR` なら `Ok(())`、ほかは `Err`。各 event は `let token = ev.u64; let flags = ev.events;` と値で写す (x86_64 の `epoll_event` は packed なので参照を取らない)。`token == WAKE` なら eventfd を8バイト読んで消す (`EAGAIN` は無視)。
  - eventfd は level なので、読むまで毎回報告され、読めば消える [V: probe 8。2回の `write` の値 2 が1回の読みで消えた]。鳴らした印は次の `wait` が読むまで残る (G-11)。
  - `epoll_pwait2` は使わない。VM の期限はミリ秒で、切り上げれば早く戻らない。5.11 より古い kernel の代わりの道 (`ENOSYS`) も書かずに済む。
- Apple (`kqueue.rs`):
  - `Kqueue { kq: Arc<OwnedFd>, events: Vec<libc::kevent> }` (長さ 256)。`struct User(Arc<OwnedFd>)` が waker で、kqueue の fd を backend と共に所有する (G-11)。`new` は `kqueue()`、`fcntl(F_SETFD, FD_CLOEXEC)` の後、`EVFILT_USER` を `ident = 0`、`EV_ADD | EV_CLEAR`、`udata = WAKE` で登録する (作成ごとの `kevent` の変更1回)。
  - `wake()`: `kevent(kq, [{ident: 0, filter: EVFILT_USER, flags: 0, fflags: NOTE_TRIGGER, udata: WAKE}], 1, null, 0, &ZERO)`。`EINTR` なら同じ呼び出しを出し直す (`NOTE_TRIGGER` を2回出しても印が1つ立つだけ)。印は次の `kevent` の取り出しで `EV_CLEAR` により消えるので、読みの syscall は要らない。
  - `poll`: `kevent(kq, null, 0, events, 256, ts)`。`ts = timeout.map(|d| timespec { tv_sec: d.as_secs().min(i32::MAX as u64) as time_t, tv_nsec: d.subsec_nanos() as c_long })`、`None` は null。秒を `i32::MAX` で切るのは、XNU がナノ秒に直すときの桁あふれを避けるためで [I]、早く戻る向きだけである。`EINTR` は `Ok(())`。`token = ev.udata.addr() as u64`。`const _: () = assert!(usize::BITS == 64);` を置く。
- 時計: `now() = self.epoch.elapsed()` (`Instant`。Linux は CLOCK_MONOTONIC で、`epoll_pwait` の timeout と同じ時計)。`Instant` を名指しするのは `sys/*` だけである (G-1)。
- `spawn(builder, body)`: `crate::mask::spawn_masked(builder, body)`。

### Q5. syscall の予算

用語: 「空きあり」は VM が読みに貸す buffer が一意で、長さの後ろに `MIN_ROOM` バイト以上の空きがある場合である。VM は `Readable` を出さずに直接 `Read` を出す。「空きなし」はそれ以外で、VM は `Readable` を出し、終わってから `Read` を出す [V: `crates/eml_vm/src/cells.rs:1959-1980`、`spare_room` は 2862-2864]。bench の対応: echo-*-reuse は `Bytes.truncate 0 b` を読みに渡し直すので、2回目以降は空きあり。echo-*-fresh は毎回 `Bytes.empty ()` なので空きなし [V: `bench/net/echo.em`]。

歩の擬似コードから数える。Linux の strace での名前は、`recv` → `recvfrom`、`send` → `sendto`。

| 場合 | 歩 | syscall | 計 |
|---|---|---|---|
| データのある読み (空きあり) | `Recv` | `recvfrom` | 1 |
| データのある読み (空きなし、辺の直後) | `Peek` (記憶、0) + `Recv` | `recvfrom` | 1 |
| データのある読み (空きなし、前の読みの後) | `Peek` (覗き) + `Recv` | `recvfrom` ×2 | 2 |
| 書き (送りの buffer に入る) | `Send` | `sendto` | 1 |
| 待つ読み (空きあり) | `Recv` EAGAIN → `wait` → `Retry` → `Recv` | `recvfrom` + `epoll_pwait` + `recvfrom` | 3 |
| 待つ読み (空きなし、定常) | `Peek` の覗き EAGAIN → `wait` → `Retry` → `Peek` (記憶、0) → `Recv` | 同上 | 3 |
| 待つ読み (空きなし、行を作った直後か EAGAIN の後) | `Peek` (`!ready`、0) → `wait` → `Retry` → `Peek` (記憶) → `Recv` | `epoll_pwait` + `recvfrom` | 2 |
| backlog のある受け | `Accept` | `accept4` + `epoll_ctl` | 2 |
| 接続 (Linux) | `socket` + 最初の `Connect` + `Retry` の `Connect` | `socket` + `connect` + `epoll_ctl` + (`epoll_pwait`) + `getsockopt` + `getpeername` | 6 (待ちはほかと共有) |
| driver の作成 | `Epoll::new` | `epoll_create1` + `eventfd` + `epoll_ctl` | 3 (run ごとに1回) |
| 操作ごとの `epoll_ctl` | — | 0 | 0 |
| 起こしの消費 (Linux) | `wait` の中 | eventfd の `read` | 起こしごとに 1 |

- `epoll_ctl` の数え方 (C-1 を受けて書き直した): main スレッドの総数 = 1 (driver の作成。eventfd) + 登録したソケットの数 (リスナー、受けたソケット、接続のソケットが各 1)。往復あたり 0。bench の各件ではこうなる。
  - echo-py-reuse: eml は接続する側だけ (`port` が正なので `listen` しない [V: `bench/net/echo.em`]) なので、総数 2 (eventfd + 接続のソケット)。段の基準「総数が 2 以下」はこのまま成り立つ。G-7 の確かめ方にある「リスナー 1 + 受けたソケット 1」は内訳が違うので、「eventfd 1 + 接続のソケット 1」と書き直す (S4 が段の計画に)。
  - base: 2 (eventfd + リスナー)。echo-self-*: 4 (eventfd、リスナー、接続、受けたソケット)。
  - conn-self-32: 接続あたり 2 (受けたソケットと接続のソケット) で、ほかに run ごとに 2 (eventfd とリスナー)。
  - kqueue では同じ所で `kevent` の変更の呼び出しになる (作成時に `EVFILT_USER` 1、ソケットごとに 1)。
- 最小の echo (設計の `Recv`/`Send` の歩をなぞり、ソケットを1回 ET で登録する。waker はない) で、1000 往復の main スレッドが `recvfrom` 1995 (うち EAGAIN 995)、`epoll_pwait` 996、`sendto` 1000、`epoll_ctl` 1 だった [V: `s2-probe/src/bin/echo.rs` を `strace -f`]。待つ読みは 3、書きは 1、往復あたりの `epoll_ctl` は 0 で、余分な起床はほぼない (996 回の待ちに対して EAGAIN は 995 回)。この probe は waker を登録しないので、本物の driver では作成時の `epoll_ctl` が1つ増える。
- S4 への確かめ方 (`bench/net` の strace、main スレッドだけ):
  - echo-py-reuse: `epoll_ctl` は総数 2、往復あたり 0。`sendto` 1。空きありの読みなので、`recvfrom` はデータがあれば 1、待てば 2 (EAGAIN 1) で、待てば `epoll_pwait` が 1。
  - echo-self-fresh: 空きなし。待つ読み1回あたり、`recvfrom` (覗きの EAGAIN を含む) + `epoll_pwait` + `recvfrom` が 3 以下 (行を作った直後の読みは 2)。
  - conn-self-32: 接続あたり `epoll_ctl` 2、`accept4` 1、`connect` 1、`socket` 1、`getsockopt` 1 以下、`getpeername` 1 以下。
  - 待ちは x86_64 でも aarch64 でも `epoll_pwait` と出る。eventfd の消費は `read` と出る。
- 構造の確かめ (コード): `grep -c "epoll_ctl(" crates/eml_io/src/sys/epoll.rs` が `2` (`new` の eventfd と `register`)。`grep -c "register_row(" crates/eml_io/src/sys/unix.rs` が `4` (定義1 + `listen`、`Accept`、`Connect` の呼び出し3)。
- 余分な event と、それが無害な理由 (正しさは非ブロックの呼び出しの結果だけで決まる。G-4):
  1. epoll は取り出すときに状態を調べ直し、今の mask を返す。データの到着の event は `IN|OUT` で来る [V: probe 3]。書きの待ち手がいれば `Retry` が届くが、そのとき送りの buffer に空きがあるので `send` は進む。待ち手がいなければ何もしない。
  2. 取り出す前にデータを読み尽くした行は、`IN` なしで報告される (`OUT` だけ) [V: probe 2]。そのため `fresh` は古い `IN` では立たない。kqueue は Q1 のとおり単体テストで確かめ、落ちれば `fresh: false` にする。
  3. 消えた接続 (受ける前に相手が捨てた) によるリスナーの `IN`: `accept4` は `EAGAIN` (`Pending`) か、`ConnectionAborted` (核が出し直す) を返す。
  4. `ERR`/`HUP` は両方の向きを起こす。どちらの試みも、誤りか EOF をその場で返す。
  5. 閉じた行の古い世代の event は `deliver` が捨てる。
  6. 待ち手のいない起床 (起こしや、macOS で `EVFILT_WRITE` が ACK ごとに発火しうること [I]): `wait` は events が空の `Ok(())` を返し、核は `Ok(true)` を返して、VM は確かめ直すだけである。
  7. 接続の早すぎる `Retry`: `SO_ERROR == 0` で `getpeername` が `ENOTCONN` なら `Pending` のまま。

### Q6. 共有と `unsafe`

- `sys/unix.rs` の `unsafe`: `socket`、`setsockopt`/`getsockopt`、`bind`、`listen`、`getsockname`、`accept4`/`accept`、`fcntl`、`ioctl`、`connect`、`getpeername`、`recv`、`send`、`shutdown`、`OwnedFd::from_raw_fd`、`mem::zeroed` (`sockaddr_*`、`sockaddr_storage`)、`from_storage` の読み替え。
- `sys/epoll.rs`: `epoll_create1`、`eventfd`、`epoll_ctl`、`epoll_pwait`、eventfd の `read`/`write`、`from_raw_fd`。`sys/kqueue.rs`: `kqueue`、`fcntl`、`kevent`、`from_raw_fd`。
- 3つのファイルは1行目を `#![allow(unsafe_code)]` にし、`unsafe` のブロックごとに `// SAFETY:` を書く。要の論は次のとおり。
  - `recv_raw`/`send_raw`: `RawBuf` の約束 (`eml_rt::RawBuf`: 貸した側は `Submit::Done` か完了の取り出しまで生かし、動かさない。読みの貸しは一意、書きの貸しは書き換えない) は `start` の間成り立つ。カーネルは `cap` バイトまでしか触れず、readiness の呼び出しは同期なので、戻った後にポインタを持たない。slice は作らない。
  - 覗きの1バイトは `start` の局所の `MaybeUninit<u8>` で、同期の呼び出しの間だけ渡す (G-16 の「OS が終わりを告げるまで」は呼び出しが戻ることで満たす)。
  - `from_raw_fd`: 直前の呼び出しが返した、ほかに持ち主のない fd である。
  - `from_storage`: 族と長さを確かめてから、整列の強い `sockaddr_storage` を読み替える。
- `RawBuf` の `ptr` に触れるのは `recv_raw` と `send_raw` の2か所だけ (`grep -n "\.ptr" crates/eml_io/src/sys/unix.rs` で確かめる)。
- cfg: OS の差は、`#[cfg(target_os = "linux")]`/`#[cfg(target_vendor = "apple")]` を付けた小さな関数の対 (`stream_socket`、`accept_raw`、`transient`、`SEND`、`sin_len`) にだけ置く。歩の本体には cfg を書かない。
- 単体テストは `sys/unix.rs` の末尾の `#[cfg(test)] mod tests` に置く (`unsafe` の `fcntl`/`getsockopt` を使うため。新しいファイルを足すと S1 の `UNSAFE_FILES` が変わる)。

### Q7. backend のテスト (S4 が段の計画に統合する)

単体テスト (`src/sys/unix.rs` の `#[cfg(test)] mod tests`。`Unix::<P>` を `Backend` として直接動かす。Linux と macOS の CI と ASan で流れ、Miri の `driver::tests::` の外にある):

| 名前 | 確かめること |
|---|---|
| `a_token_keeps_its_row_and_generation` | `token` の往復。どの行の `token` も `WAKE` と違う |
| `an_event_of_a_released_row_is_dropped` | `release` の後、古い世代の `token` を `deliver` しても `events` が空で、同じ行を使い回した新しい待ち手に当たらない |
| `a_probe_after_an_edge_answers_from_memory_once` | 相手が書いた後、`wait` で `Retry` が来たら `Peek` が `Done` を返し、`fresh == false`、`ready == true` になる。2つ目の `Peek` も (覗きで) `Done`。データを読み尽くした後の `Peek` は `Pending` で、`ready == false` |
| `a_drained_event_does_not_make_a_probe_fresh` | 行を `ready` にした後、相手が書く。`ioctl(FIONREAD)` が正になるまで (1 s を上限に) 待ち、`wait` を呼ばずに `Recv` で読み尽くす。その後 `wait(Some(0))` を呼ぶと行の `fresh` は偽で、`Peek` は `Pending` になる (取り出しの評価し直し。kqueue の確かめ。落ちれば Q1 の代わりにする) |
| `a_retry_without_news_keeps_a_read_pending` | 待ち手のある `Recv` に `start` をもう一度呼んでも `Pending` で、完了は出ない (G-4 の backend 側) |
| `sockets_are_close_on_exec_and_nonblocking` | リスナー、接続のソケット、受けたソケットのどれも、`fcntl(F_GETFD)` に `FD_CLOEXEC`、`F_GETFL` に `O_NONBLOCK` がある |
| `an_accepted_socket_does_not_raise_sigpipe` (`target_vendor = "apple"`) | 受けたソケットの `SO_NOSIGPIPE` が 1 |
| `a_wake_is_kept_until_the_next_wait` | 先に `wake` してから呼んだ `wait(Some(5 s))` が、events の空の `Ok` で戻る。続く `wait(Some(0))` も空 (印は消費された)。時間は比べない |
| `a_socket_address_survives_the_round_trip` | `RawAddr::new` → `as_ptr` の中身を `sockaddr_storage` に写して `from_storage` で戻すと元に戻る。対象は `127.0.0.1:80`、`[::1]:4242`、flowinfo 7 と scope_id 3 を持つ `[fe80::1]:1`。ソケットを作らないので v6 のない機械でも流れる |

結合テスト (`crates/eml_io/tests/real.rs` に足す。すべて `#[cfg(unix)]`):

| 名前 | 確かめること |
|---|---|
| `a_probe_on_a_stream_that_had_data_before_accept_finishes` | 相手が接続して書いた後に `Accept` すると、受けたストリームの `Readable` が終わる (登録の時点の event。kqueue の確かめ) |
| `a_pending_read_and_a_pending_write_on_one_stream_both_finish` | 1つのストリームで `Readable` と書きを同時に保留にし (書きは下の「保留になるまで書く」で作る)、相手が読み書きすると両方が終わる (向きごとの待ち手) |
| `queued_connections_are_accepted_one_per_op` | 2本つないでから `Accept` を2回出すと、両方が終わる (辺が1つでも、`Accept` は記憶によらず試す) |
| `a_refused_connect_completes_once` | 閉じたポートへの `Connect` が `ConnectionRefused` で終わる。その後 `pending() == 0` で、20 ms 待っても完了が来ない (`connect` を出し直さない結果) |
| `a_reset_ends_a_pending_write_with_an_error` | std のリスナーに接続し、保留になるまで書く。相手を linger 0 で閉じると、保留の `Write` が `ConnectionReset` か `BrokenPipe` で終わる (`ERR\|HUP` が書きの向きを起こす) |
| `a_signal_during_a_wait_leaves_the_driver_working` | `SIGUSR1` に何もしない handler を `sa_flags = 0` で入れる。保留を持つ driver の `wait(Some(u64::MAX))` を、別スレッドが `pthread_kill` で 5 ms ごとに叩く (終わりの印が立つまで)。`wait` が `Ok(true)` で戻り、続く `submit` と `wait` が通る |

- 「保留になるまで書く」(B-2 を受けた形): 64 KiB の `Write` を出し、`Submit::Done` が続く間は次を出す。最初の `Submit::Pending` でやめ、その鍵を保留の書きとして使う。4096 回 (256 MiB) で保留にならなければ、テストは落ちる。相手は読まない。どの機械でも送りと受けの buffer の和は有限なので、どこかで保留になる。buffer の大きさには頼らない。
- `SIGUSR1` の handler はプロセス全体に残すが、何もしない関数なので、ほかのテストが `SIGUSR1` を受けても害はない。`eml_io` のテストのバイナリで `SIGUSR1` と `sigaction` を使う所はほかにない [V: `grep -rn "SIGUSR\|sigaction" crates --include=*.rs` (`eml_cli/src` を除く) が空]。シグナルは `pthread_kill` でテストのスレッドだけに送る。nextest でも `cargo test` (1つのプロセスでスレッドを分ける形) でも同じに流れる。handler を戻さないのは、戻した後に遅れて届いた1つが既定の動き (プロセスの終わり) になる道を残さないためである。
- 機械的な追随 (種類 3): `a_probe_on_a_reset_stream_fails_at_submit` の `socket2::SockRef::set_linger` (`tests/real.rs:394`) を、テストの中の小さな関数 `fn reset_on_close(s: &std::net::TcpStream)` (`libc::setsockopt(SOL_SOCKET, SO_LINGER, linger { l_onoff: 1, l_linger: 0 })`) に替える。socket2 が workspace から消えるためで、期待値は変わらない。`a_reset_ends_a_pending_write_with_an_error` も同じ関数を使う。
- macOS の CI で確かめる差 (どれも上のテストか既存のテストが落ちて見える):
  - 接続の直後の `EV_ADD` で `EVFILT_WRITE` が届くこと (`an_accept_returns_the_connected_stream_and_its_peer`、`a_refused_connect_completes_once`)
  - 拒否の後の `SO_ERROR` が `ECONNREFUSED` であること (`a_connect_to_a_closed_port_is_refused`)
  - 登録の時点のデータの event (`a_probe_on_a_stream_that_had_data_before_accept_finishes`)
  - 取り出しの評価し直し (`a_drained_event_does_not_make_a_probe_fresh`)
  - `EVFILT_USER` の起こし (`a_wake_*` の3件)
  - `kevent` の `EINTR` (`a_signal_during_a_wait_leaves_the_driver_working`)
  - RST の後の覗き (`a_probe_on_a_reset_stream_fails_at_submit`)
  - 受けたソケットの CLOEXEC、`O_NONBLOCK`、`SO_NOSIGPIPE` (単体)

## 2. 根拠

- 歩と表を `unix.rs` にまとめる: epoll と kqueue の差は登録、待ち、起こしだけである。歩の規則 (記憶、鮮度、待ち手、接続中) を2回書くと食い違う。`Poller` は関数3つで、OS ごとのファイルはそれぞれ 100 行ほどに収まる [I]。
- `fresh` を記憶の答えでも下ろす: RST の後、1回目の `Readable` が記憶で終わると、S1 の文言どおりなら2回目も記憶で終わって `Ok(0)` を返す。ところが `a_probe_on_a_reset_stream_fails_at_submit` は2回目に `ConnectionReset` を期待する。下ろせば2回目は覗きになり、`ECONNRESET` を返す [V: probe 5。RST の後の覗きは1回目が `ConnectionReset`、2回目が `Ok(0)`]。
- `Ready.fresh` を poller が決める: epoll は取り出しの評価し直しを確かめた。kqueue は確かめていないので、テスト1件と1行の代わりを用意する。どちらに転んでも `Readable` の意味は保たれる。
- 初期値 `ready = false`: 登録が今の状態を報告するので正しく、行を作った直後の `Readable` に syscall を使わない [V: probe 1]。
- `Recv`/`Accept`/`Send` は記憶によらず試す: データのある読みと書きを 1 syscall に保つ。`Accept` が記憶によらず試すので、資源の誤り (`EMFILE`) の後も、待ち行列の接続は次の `Accept` で受けられる。
- 受けの一時の誤りを `ConnectionAborted` に包む: 核の続きの規則 (`ConnectionAborted` で出し直す) をそのまま使え、核に errno を持ち込まない。
- Apple の受けたソケットにも `FIONBIO` を出す: 継承に頼らず、後のパイプや `shutdown` も非ブロックを前提にできる。syscall が1つ増えるのは Apple だけで、目標は epoll の数である。
- `epoll_pwait` (sigmask null): 待ちの syscall の名前が x86_64 と aarch64 で同じになり [V: x86_64]、`bench/net` の今の表 (`epoll_pwait`) と並べられる。
- eventfd を level で登録: 読むまで残り、読めば消える [V: probe 8]。ET にすると、数が上限に達して辺が出なくなる道を考えることになる。
- リスナーは読みの関心だけ: 書きの向きを持たないからである。kqueue でリスナーに `EVFILT_WRITE` を付けたときの振る舞いも確かめずに済む。
- `kevent` の `EINTR` で出し直す: `EV_ADD` と `NOTE_TRIGGER` はどちらも2回出しても同じ結果になるので、「`EINTR` でも適用済み」という仮定に頼らない。

## 3. 退けた案

- epoll と kqueue に別々の `Backend` を実装する: 歩の規則が2つになる。
- `epoll_pwait2` (ナノ秒の timeout): 期限はミリ秒で、5.11 未満の代わりの道が要る。
- 書きの記憶 (短い書きで「満ちた」とみなし、次の `Send` を syscall なしで `Pending` にする): 長い書きで 1 syscall 減るだけで、Linux の空きの通知 (`SOCK_NOSPACE`) の細部に正しさを預けることになる。目標は「入る書きが 1」で、これは既に満たす。
- 最初の `start(Connect)` で `connect` の直後に `getpeername` も試し、その場で終える: S1 が決めた規則 (最初の歩は `Pending`) を変える。
- `EPOLL_CTL_DEL`/`EV_DELETE` を `close` で出す: 閉じれば消え、古い event は世代で捨てる。syscall が1つ増える。
- eventfd を登録しない起こし (`epoll_pwait` の sigmask でシグナルを使う、など): シグナルは VM の Ctrl-C と衝突する。作成時の `epoll_ctl` 1回は run に1回で、往復の数に入らない。
- 受けの一時の誤りを核で広げる (`ErrorKind::NetworkDown` などを出し直しの集合に足す): `EPROTO` や `ENONET` には対応する kind がなく、核が OS ごとの集合を知ることになる。
- kqueue の `EV_RECEIPT`: `nevents == 0` なら誤りは `-1` で返り、受け取り用の配列が要らない。
- kqueue の `fresh` に `EVFILT_READ` の `data` を使う: 取り出しで評価し直されないなら `data` も古い値なので、代わりにならない。代わりは `fresh: false` にする (Q1)。
- Apple で `MSG_NOSIGNAL` を使う (libc に定数はある [V: libc 0.2.191 の apple/mod.rs:3240]): spec (vm.md 466) は `SO_NOSIGPIPE` と書いており、XNU がこの旗を守るかは確かめていない。
- 起こしをまとめる旗 (compio の `AwakeFlag`): 起こしは別スレッドの `write` 1回で、main の予算に入らない。

## 4. 置いた仮定

1. kqueue の `EV_ADD` は、登録の時点で読めれば event を積む [I: XNU/FreeBSD の knote の作り。tokio/mio が同じことに頼る]。偽なら、初期値 `ready = false` のせいで登録前のデータを待ち続ける。`a_probe_on_a_stream_that_had_data_before_accept_finishes` が macOS で落ちて分かる。そのときは kqueue だけ新しい行を `ready = true, fresh = false` で作る (1回覗く)。
2. kqueue は取り出すときに filter を評価し直す [I]。偽なら `a_drained_event_does_not_make_a_probe_fresh` が落ちる。そのときは `kqueue.rs` で `fresh: false` にする (Q1)。
3. 拒否された接続の後、macOS の `SO_ERROR` は `ECONNREFUSED` を返す [I]。偽なら `a_connect_to_a_closed_port_is_refused` が落ちる。そのときの代わりを §6 の U-3 に擬似コードで書く。
4. `EVFILT_WRITE` の `EV_EOF` は自分の `shutdown(WR)` でも立つ [I]。だから読みを立てない。偽でも害はない。
5. Rust の main は SIGPIPE を無視するので、`MSG_NOSIGNAL`/`SO_NOSIGPIPE` は埋め込み先のための保険である [I]。
6. Apple では `accept` から `fcntl(FD_CLOEXEC)` までの間に別スレッドが fork+exec すると、fd が子に漏れる (mio と std も同じ)。プールの仕事が子を起こすようになったとき (`async-files` 以後) に効きうる。
7. POSIX の connect() は `EINTR` の後も接続を裏で続ける [I]。偽でも、その分岐は非ブロックのソケットでは事実上通らない。

## 5. 実装への含意

S1 の作業の単位 2 を、次の2つの commit に分ける。

**U-S2-1 (S1 の単位 1 の後): `sys/` の unix の部分を足す (差し替えの前)**
- `lib.rs` に `#[cfg(test)] mod sys;` を置く (S3 の commit が先にあればそのまま使う)。`sys/mod.rs` がまだなければ、この commit が作る。中身は先頭の `#![allow(dead_code)]` (理由: 差し替えまで `real.rs` が使わない。差し替えの commit で外す)、§1 の5行、S3 の `millis` (S3 の単体テストと共に)。Windows の行と `lookup` は S3 の commit が足す。
- 単体テスト9件を先に書き、`unix.rs`、`epoll.rs`、`kqueue.rs` を書く。
- 完了の基準:
  - `cargo nextest run -p eml_io --lib sys::` で、Linux では Apple だけの1件を除く 8 件が通る。
  - `cargo clippy -p eml_io --all-targets -- -D warnings` が通る (`private_bounds` を含む)。
  - `grep -n "Arrival::Done\|Arrival::Cancelled\|Hold::Buffer\|Cancel::Pending" crates/eml_io/src/sys/unix.rs crates/eml_io/src/sys/epoll.rs crates/eml_io/src/sys/kqueue.rs` が空。
  - `grep -rn "from_raw_parts" crates/eml_io/src/sys` が空。
  - `grep -c "epoll_ctl(" crates/eml_io/src/sys/epoll.rs` が `2`。`grep -c "register_row(" crates/eml_io/src/sys/unix.rs` が `4`。
  - `awk '/^mod tests/{exit} /loop \{|while /{print FILENAME": "FNR": "$0}' crates/eml_io/src/sys/unix.rs` の出力が、`EINTR` を繰り返す関数 `syscall` (と `c_int` 版) の行だけ。テストの中のループは数えない。
  - macOS は、push した CI の `macos-15` の `test` が緑。

**U-S2-2 (差し替えの commit。S1 の単位 2。S3 と共有)**
- `lib.rs` の `mod sys;` を常に置き、`sys/mod.rs` の `allow(dead_code)` を外す。`real.rs` を `Driver<sys::Os>` にする。`Cargo.toml` から `compio-buf`、`compio-driver`、`socket2` を外す (`eml_io` の unix の依存は `libc` だけになる)。
- `tests/real.rs`: 結合テスト6件を足し、`socket2` の linger を `reset_on_close` に替える。
- 完了の基準:
  - `cargo nextest run -p eml_io` が全件通る (今の 68 件から、S1 が削る1件と、S1・S3・S2 が足す件の分だけ増減する)。
  - `cargo nextest run -j 1 -p eml_io` で `SLOW` が出ない。
  - `cargo nextest run` (workspace) が全件通る。
  - `grep -n "compio\|socket2" Cargo.toml crates/*/Cargo.toml Cargo.lock crates/eml_io/tests/real.rs` が空。
  - CI の `test` が4つの対象で緑 (S4)。strace の数は S4 の手順で Q5 の値に合う。

実装者に任せること: `syscall` の helper の正確な形 (戻り値が `c_int` と `isize` の2つ)、`Rows` のメソッド名、テストの小さな helper、`EVENTS` (256) を変えるか。

## 6. 未決の項目

- **U-0 (S1 §7 の配置の改め。G-1 と S1 の trait は変えない)**: S1 §7 は fd の表を `epoll.rs`/`kqueue.rs` に、ソケットを `unix.rs` に置く。この設計では表、歩、`Backend` の実装を `unix.rs` の `Unix<P: Poller>` にまとめ、OS ごとのファイルは `Poller` (登録、待ち、waker) だけを持つ。変わらないもの: ファイルの集合 (`trust.rs` の `FILES` と `UNSAFE_FILES` は S1 のまま)、`Backend` の trait、`unsafe` を書けるファイル。変わるもの: `type Os` の右辺 (`unix::Unix<epoll::Epoll>`。S3 の `sys/mod.rs` に S2 の5行として入る)、`Poller`/`Ready` を `pub(crate)` にすること (`private_bounds` [V])、文書の `eml_io` の内部の説明 (S4 が `architecture.md` に「`unix.rs` が表と歩を持ち、`epoll.rs`/`kqueue.rs` は待ちと起こしだけ」と書く)。
- **U-1 (S1 への小さな異議。証拠付き)**: S1 Q1 の「`fresh` は読みの向きの syscall で下りる」に、「記憶で答えた `Peek` でも下りる」を足す。証拠は §2 の2つ目 (probe 5 と、`tests/real.rs:376-415` の期待)。足さないと既存のテストが成否の変更になる。ほかの規則 (`ready` は EAGAIN でだけ下りる、syscall の予算) は変わらない。段の計画の逸脱の項「`Readable` の覗きの省き方」に、この1文と「kqueue は確かめのテストが落ちれば記憶で答えない」を加える。
- **U-2**: G-7 は「読み書き両方の関心を登録」と書くが、リスナーは読みだけを登録する。既定はこの設計 (リスナーに書きの向きはない)。また G-7 の確かめ方の echo-py-reuse の内訳を「eventfd 1 + 接続のソケット 1」に直す (Q5)。数の基準 (総数 2 以下、往復あたり 0) はそのまま成り立つ。
- **U-3 (macOS の拒否された接続。CI で落ちたときの代わり)**: 既定は `SO_ERROR` + `getpeername`。macOS で `SO_ERROR` が 0 のまま `getpeername` が `ENOTCONN` を返し続けるなら、kqueue が `EVFILT_WRITE` の `EV_EOF` の `fflags` に載せる誤りを使う。変更は次の3か所で、G-2 (1回の試み) と `connecting` の規則はそのまま保つ。

```
Ready に error: i32 を足す (epoll は常に 0。kqueue は EVFILT_WRITE かつ EV_EOF かつ fflags != 0 のとき fflags)
Row に error: i32 を足し、deliver で ready.write && ready.error != 0 なら r.error = ready.error
Connect の Retry:
  err = getsockopt(SO_ERROR); if err == 0 { err = r.error }
  if err != 0 => { r.connecting = false; r.write = None; Done(Err(from_raw_os_error(err))) }
  (以下は今と同じ)
```

  macOS で `getpeername` が `EINVAL` を返す場合 (つながった直後に相手が閉じたとき [I]) は、今の擬似コードでは `Done(Err(EINVAL))` になる。CI でそれが見えたら、`SO_ERROR == 0` かつ `EINVAL` を `Done(Ok(Connected))` と読む (以後の読み書きが閉じたことを報告する)。
- **U-4**: macOS では `EVFILT_WRITE` が ACK ごとに起床させうる (bulk で `kevent` の起床が増える) [I]。目標は epoll だけなので、既定では何もしない。S4 が macOS の `bench/net` を見るときに分かる。
- **S1/S3 との取り決め (合意済み)**: (a) どの backend も inherent の `fn new() -> io::Result<Self>` を持ち、`real.rs` がそれを呼ぶ。(b) 核の `turn` は `wait` に空の `events` を渡す。(c) 核は、`Connect` の歩が `Done` を返したソケットに `start(Connect)` を出し直さず、失敗したソケットは `close` する (S1 のとおり)。核への新しい要望はない (C-4 は backend の写しで済ませた)。

## 7. 追加で調べたこと

- `s2-probe/src/main.rs` (Rust、libc 0.2.191、ET の epoll) [V]:
  1. データのあるソケットの ADD の直後は `IN|OUT`、データのないソケットは `OUT`。
  2. event を積んだ後に読み尽くすと、取り出しは `OUT` だけ。
  3. データの到着は `IN|OUT`。
  4. 短い読みの後は新しい辺がなく、新しいデータで `IN|OUT` が来る。
  5. RST は `IN|OUT|ERR|HUP|RDHUP`。覗きは1回目が `ConnectionReset`、2回目が `Ok(0)`。その後の `send(MSG_NOSIGNAL)` は `BrokenPipe`。
  6. 自分の `shutdown(WR)` は `OUT` だけ。
  7. 相手の `shutdown(WR)` は `IN|OUT|RDHUP`。
  8. level の eventfd は読むまで報告され、2回の `write` は1回の読みで消える。
  9. リスナーは、前の接続を受けていなくても、接続ごとに `IN` の辺を出す。
  10. 非ブロックの `connect` は、ループバックでは拒否でも成功でも `EINPROGRESS` を返す。拒否では `IN|OUT|ERR|HUP|RDHUP`、`SO_ERROR` 111、`getpeername` は `ENOTCONN`。成功では `OUT`、`SO_ERROR` 0。
- `s2-probe/src/bin/echo.rs` を `strace -f` [V]: Q5 の数。`epoll_pwait` (sigmask null) は strace で `epoll_pwait` と出る。
- `s2-probe/src/bin/addr.rs` [V]: `RawAddr`/`from_storage` は Linux の libc の型で通り、v4 の bind と getsockname、v6 の往復 (ソケットなし) が合う。この箱では IPv6 のソケットを作れない。
- `s2-probe/lintcheck/` [V]: `pub(super) trait Poller` を境界に持つ `pub(crate)` の `impl<P: Poller> Unix<P>` は、`cargo clippy --all-targets -- -D warnings` で `private_bounds` の誤りになる。`pub(crate) trait Poller` にすると通る。
- libc 0.2.191 [V: registry のソース]: Apple に `EVFILT_USER`、`NOTE_TRIGGER`、`EV_CLEAR`、`SO_NOSIGPIPE`、`MSG_NOSIGNAL`、`kevent.udata: *mut c_void` がある。Linux に `accept4`、`epoll_pwait`、`epoll_pwait2` (gnu) がある。`in6_addr` の欄は `s6_addr` だけ。
- `bench/net/echo.em` と `cases.tsv` [V]: echo-py-* では eml は接続する側だけで、reuse は `Bytes.truncate 0 b` を渡し直す。
- `crates/eml_vm/src/cells.rs:1959-1980` [V]: 空きのある一意の buffer は直接 `Read`、それ以外は `Readable` (Probe)。
- `repo-s2` の `crates/eml_io/tests/real.rs` を新しい規則で机上で追った [I: 実装はまだない]。`a_probe_with_data_finishes_at_submit`、`a_read_after_a_finished_probe_returns_done`、`a_cancel_after_completion_does_not_touch_the_next_op_of_the_key`、`a_readable_probe_stays_pending_without_data`、`a_cancelled_connect_does_not_try_the_next_address` は通る筋。`a_probe_on_a_reset_stream_fails_at_submit` は U-1 があるので通る筋。`tests/real.rs:394` が `socket2` を使う。`real.rs:307` は `ECONNABORTED` だけを出し直す [V]。
- `docs/spec/vm.md` 365、485 [V]: Ctrl-C は waker と 10 ms の上限で届くので、`EINTR` で `Ok` を返せば足りる。
- macOS と aarch64-linux は手元で動かせない。kqueue の主張は [I] で、Q7 のテストを CI で流して確かめる。

## 8. 適合の自己点検

- G-1: 適合。`Instant` と `libc` は `sys/*` だけにある。核に渡すのは `Backend` の関数だけ。
- G-2: 適合。各歩の擬似コードに試し直しがない。ループは `EINTR` の `syscall` (と kqueue の `EINTR` の出し直し) だけ。`connect` の `EINTR` は進行中と読み、繰り返さない。受けの一時の誤りは `ConnectionAborted` に包んで核に出し直させる。
- G-3: 適合。使うのは `Retry`、`Pending(Hold::None)`、`Cancel::Done` だけ。grep の対象に `unix.rs` も入れる。
- G-4: 適合。待ち手は `start` の `Done`、`cancel`、`close` でだけ消え、`Retry` は今の待ち手にだけ出す。`Connect` は `connecting` を見て `SO_ERROR` + `getpeername` に進む。
- G-5: 適合。`read`/`write` の `Option<OpKey>` が1つずつ。
- G-6: 適合。行に記憶、鮮度、待ち手、接続中、世代を置く。`Sock { fd, row, gen }`。OS に渡す値は `token(row, gen)` の1か所で作り、`deliver` が世代を比べる。`ready` は読みの向きの EAGAIN でだけ下りる (`fresh` の扱いは U-1)。
- G-7: 適合 (リスナーは読みだけ。U-2)。登録は `register_row` の1か所 (呼び出しは3つ) で、`close` まで変えない。操作ごとの `epoll_ctl` は 0 [V: probe の echo]。driver の作成ごとに eventfd の `epoll_ctl` が1回あり、echo-py-reuse の総数は 2 (Q5)。
- G-8: 適合 (backend 側)。`wait` は event が名指しした行だけに触れ、event の配列を使い回して確保しない。timeout は `millis` で切り上げ、上限で切るのは早く戻る向きだけ。
- G-9、G-10: 対象外 (核)。`spawn` は `spawn_masked`。
- G-11: 適合。waker が `Arc<EventFd>` と `Arc<OwnedFd>` (kqueue) を所有し、backend は写しを持つ。印は eventfd の値と `EV_CLEAR` の `EVFILT_USER` で、次の `wait` が消費する。
- G-12、G-13、G-14: 核の規則。backend の `cancel` は `Err` を返さず、`close` は fd を1回だけ閉じる。
- G-15: 適合。3つのファイルは `#![allow(unsafe_code)]` で始まる。`ptr` は `recv_raw`/`send_raw` の中でだけ読み、slice を作らない。
- G-16: 適合。backend は errno を見るが、核へは `io::Error` (と `ConnectionAborted` の kind) で渡す。覗きの1バイトと `sockaddr` は同期の呼び出しの間だけ渡す。
- G-17: 適合。単体テストは Linux と macOS の CI と ASan で流れ、Miri の対象外である。どのテストも 1 s 以下で、時間は下限か「終わること」だけを見る。書きを保留にするテストは buffer の大きさに頼らない。
- G-18: 適合。unix の依存は `libc` だけ。対応しない OS は `compile_error!` (S3 の `sys/mod.rs`)。
- G-19: S4 の範囲。コードのコメントは `docs/spec/vm.md` の「時計と driver」を引く。U-0、U-1、U-2 の文を段の計画と文書に。


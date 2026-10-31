//! Windows の backend。ソケットを扱わず、IOCP のポートで起こしと時間切れだけを待つ
//! (docs/spec/vm.md の「時計と driver」)。ソケットは docs/future/roadmap.md の `net-windows` の項目である。
#![allow(unsafe_code)]

use std::io;
use std::net::SocketAddr;
use std::os::windows::io::{AsRawHandle, HandleOrNull, OwnedHandle};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use eml_rt::{IoWaker, OpKey};
use windows_sys::Win32::Foundation::{FALSE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED_ENTRY,
    PostQueuedCompletionStatus,
};
use windows_sys::Win32::System::Threading::INFINITE;

use crate::driver::backend::{Backend, Cancel, Event, Start, Step};

/// 起こしの packet の completion key。ソケットの packet を足すときは別の key を使う
const WAKE: usize = 0;
/// 1回の `GetQueuedCompletionStatusEx` で受け取る packet の数
const ENTRIES: usize = 64;
/// 起こしの packet を積めなかったとき、出し直すまでの間
const POST_RETRY: Duration = Duration::from_millis(1);

/// ソケットを作らないので値がない。`Sock` を受け取る関数が呼ばれないことを、実行時の約束でなく型で示す
pub(crate) enum Sock {}

pub(crate) struct Iocp {
    waker: Arc<PortWaker>,
    epoch: Instant,
}

/// ポートを所有する起こし。ほかのスレッドが持つ写しがポートを生かすので、driver が落ちた後の鳴らしも生きた
/// ポートに書くだけで済む。
struct PortWaker {
    port: OwnedHandle,
    /// 起こしの packet がポートにあるか、積めるまで出し直す鳴らしがいる。鳴らしを1つの packet にまとめ、
    /// packet が積もらないようにする
    posted: AtomicBool,
}

impl IoWaker for PortWaker {
    fn wake(&self) {
        // 立っていれば、packet があるか、ほかの鳴らしが積むまで出し直しているので、その packet がこの分も届ける
        if self.posted.swap(true, Ordering::AcqRel) {
            return;
        }
        loop {
            // SAFETY: `port` はこの物体が所有していて開いている。OVERLAPPED は null で、OS は読まずにそのまま返す
            let ok = unsafe {
                PostQueuedCompletionStatus(self.port.as_raw_handle(), 0, WAKE, ptr::null())
            };
            if ok != 0 {
                return;
            }
            // 失敗の原因によらず、立てたまま出し直す。ここで `posted` を下ろすと、同時に来たほかの鳴らしの分が
            // 失われる
            thread::sleep(POST_RETRY);
        }
    }
}

impl Iocp {
    pub(crate) fn new() -> io::Result<Self> {
        // SAFETY: 既存のハンドルを結び付けず、新しいポートを作るだけである。渡すポインタはなく、失敗は null で返る
        let raw = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, 1) };
        // SAFETY: `raw` は今作ったポートか null で、ほかに所有者がいない
        let port = OwnedHandle::try_from(unsafe { HandleOrNull::from_raw_handle(raw) })
            .map_err(|_| io::Error::last_os_error())?;
        Ok(Iocp {
            waker: Arc::new(PortWaker {
                port,
                posted: AtomicBool::new(false),
            }),
            epoch: Instant::now(),
        })
    }
}

impl Backend for Iocp {
    type Sock = Sock;
    const SOCKETS: bool = false;

    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        Arc::clone(&self.waker) as Arc<dyn IoWaker>
    }

    fn spawn(
        &self,
        builder: thread::Builder,
        body: Box<dyn FnOnce() + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<()>> {
        // Windows では塞ぐシグナルがないが、スレッドはどの backend でも同じ関数で作る
        // (docs/spec/vm.md の「上限と割り込み」)
        crate::mask::spawn_masked(builder, body)
    }

    fn listen(&mut self, _addr: SocketAddr) -> io::Result<(Sock, u16)> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn socket(&mut self, _peer: SocketAddr) -> io::Result<Sock> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn shutdown_write(&mut self, sock: &mut Sock) -> io::Result<()> {
        match *sock {}
    }

    fn set_nodelay(&mut self, sock: &mut Sock, _on: bool) -> io::Result<()> {
        match *sock {}
    }

    fn close(&mut self, sock: Sock) {
        match sock {}
    }

    fn start(&mut self, _key: OpKey, step: Step<'_, Sock>) -> Start<Sock> {
        match step {
            Step::Peek { sock }
            | Step::Recv { sock, .. }
            | Step::Send { sock, .. }
            | Step::Accept { listener: sock }
            | Step::Connect { sock, .. } => match *sock {},
        }
    }

    fn cancel(&mut self, _key: OpKey, step: Step<'_, Sock>) -> io::Result<Cancel> {
        match step {
            Step::Peek { sock }
            | Step::Recv { sock, .. }
            | Step::Send { sock, .. }
            | Step::Accept { listener: sock }
            | Step::Connect { sock, .. } => match *sock {},
        }
    }

    fn wait(
        &mut self,
        timeout: Option<Duration>,
        _events: &mut Vec<Event<Sock>>,
    ) -> io::Result<()> {
        let mut entries = [OVERLAPPED_ENTRY::default(); ENTRIES];
        let mut removed = 0u32;
        let ms = timeout.map_or(INFINITE, |d| super::millis(d, INFINITE - 1));
        // SAFETY: ポートは `waker` が所有していて開いている。`entries` は長さ `ENTRIES` の局所の配列、`removed` は
        // 局所変数で、OS が書くのは戻るまでの間だけである。`alertable` が `FALSE` なので APC は走らない
        let ok = unsafe {
            GetQueuedCompletionStatusEx(
                self.waker.port.as_raw_handle(),
                entries.as_mut_ptr(),
                ENTRIES as u32,
                &mut removed,
                ms,
                FALSE,
            )
        };
        if ok == 0 {
            let e = io::Error::last_os_error();
            // std は `TimedOut` にほかの誤りも写すので、時間切れは番号で見分ける
            return if e.raw_os_error() == Some(WAIT_TIMEOUT as i32) {
                Ok(())
            } else {
                Err(e)
            };
        }
        for entry in &entries[..removed as usize] {
            // ポートにハンドルを結び付けず、起こしの key でしか積まないので、来るのは起こしの packet だけである
            debug_assert_eq!(entry.lpCompletionKey, WAKE);
            // packet を取り出してから下ろす。間に来た鳴らしの答えは、核がこの後で受け箱を引き取るときに拾う
            self.waker.posted.store(false, Ordering::Release);
        }
        Ok(())
    }
}

/// Winsock の振る舞いのうち、Microsoft の文書に書かれていないのにこの backend が頼るものを、本物のソケットで確かめる
/// (docs/implementation/testing.md の「テストの置き場所」)。どの待ちも 1 秒で切り、経過時間は確かめない
/// (docs/implementation/testing.md の「性能のテスト」)。
#[cfg(test)]
mod tests {
    use std::cell::UnsafeCell;
    use std::io::{Read, Write};
    use std::mem::size_of;
    use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
    use std::os::windows::io::{AsRawSocket, FromRawSocket, OwnedSocket};
    use std::sync::Once;

    use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, HANDLE};
    use windows_sys::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, FIONBIO, INVALID_SOCKET, IPPROTO_TCP, LINGER, LPFN_ACCEPTEX,
        LPFN_CONNECTEX, MSG_PEEK, SD_SEND, SIO_GET_EXTENSION_FUNCTION_POINTER, SIO_TCP_INITIAL_RTO,
        SO_LINGER, SO_UPDATE_ACCEPT_CONTEXT, SO_UPDATE_CONNECT_CONTEXT, SOCK_STREAM, SOCKADDR,
        SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_STORAGE, SOCKET, SOCKET_ERROR, SOL_SOCKET,
        TCP_INITIAL_RTO_DEFAULT_RTT, TCP_INITIAL_RTO_PARAMETERS, TCP_NODELAY,
        WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSA_IO_PENDING, WSA_OPERATION_ABORTED,
        WSABUF, WSADATA, WSAECONNABORTED, WSAECONNREFUSED, WSAECONNRESET, WSAEWOULDBLOCK,
        WSAGetLastError, WSAGetOverlappedResult, WSAID_ACCEPTEX, WSAID_CONNECTEX, WSAIoctl,
        WSARecv, WSASend, WSASocketW, WSAStartup, accept, bind, getpeername, getsockname,
        ioctlsocket, recv, send, setsockopt, shutdown,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, OVERLAPPED};
    use windows_sys::core::GUID;

    use super::*;

    /// ソケットを結び付けるときの completion key
    const SOCK: usize = 1;
    /// `Rig::only_marks` が積む印の packet の key
    const MARK: usize = 2;
    /// 待ちの上限
    const LIMIT: Duration = Duration::from_secs(1);
    /// `Rig` が貸せる OVERLAPPED の数
    const SLOTS: usize = 8;
    const ERROR_NETNAME_DELETED: i32 = 64;
    const ERROR_CONNECTION_REFUSED: i32 = 1225;
    /// windows-sys はこの定数を u16 の 65534 で持つが、`MaxSynRetransmissions` は u8 なので (UCHAR)-2 を書く
    const NO_SYN_RETRANSMISSIONS: u8 = 254;
    /// AcceptEx の番地の欄1つの長さ。AcceptEx は番地の最大の長さに 16 バイトを足すことを求める
    const ADDR_LEN: usize = size_of::<SOCKADDR_STORAGE>() + 16;

    /// AcceptEx が自分と相手の番地を書く buffer
    type Addrs = [u8; 2 * ADDR_LEN];

    fn startup() {
        static STARTED: Once = Once::new();
        STARTED.call_once(|| {
            let mut data = WSADATA::default();
            // SAFETY: `data` は局所変数で、OS が書くのは戻るまでの間だけである
            assert_eq!(unsafe { WSAStartup(0x202, &mut data) }, 0);
        });
    }

    /// このスレッドで直前に失敗した Winsock の呼び出しの番号
    fn last() -> i32 {
        // SAFETY: 引数はなく、呼んだスレッドの値を読むだけである
        unsafe { WSAGetLastError() }
    }

    struct Packet {
        key: usize,
        ov: *mut OVERLAPPED,
        bytes: u32,
    }

    /// 試しのポートと、番号で貸す OVERLAPPED の組。貸した OVERLAPPED には、それを名指す packet を取り出すまで OS が
    /// 書きうる。OVERLAPPED は `Box` の中に置き、`Rig` が動いても番地が変わらないようにする
    struct Rig {
        port: OwnedHandle,
        ovs: Box<[UnsafeCell<OVERLAPPED>; SLOTS]>,
        lent: [bool; SLOTS],
    }

    impl Rig {
        fn new() -> Rig {
            startup();
            // SAFETY: 既存のハンドルを結び付けず、新しいポートを作るだけである。渡すポインタはなく、失敗は null で返る
            let raw =
                unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, 1) };
            // SAFETY: `raw` は今作ったポートか null で、ほかに所有者がいない
            let port = OwnedHandle::try_from(unsafe { HandleOrNull::from_raw_handle(raw) })
                .expect("CreateIoCompletionPort failed");
            Rig {
                port,
                ovs: Box::new(std::array::from_fn(|_| {
                    UnsafeCell::new(OVERLAPPED::default())
                })),
                lent: [false; SLOTS],
            }
        }

        fn associate(&self, s: SOCKET) {
            // SAFETY: `s` は呼ぶ側が開いたまま持つソケットで、ポートは `self` が持っていて開いている。渡すポインタは
            // ない
            let r =
                unsafe { CreateIoCompletionPort(s as HANDLE, self.port.as_raw_handle(), SOCK, 0) };
            assert!(!r.is_null(), "{}", io::Error::last_os_error());
        }

        /// 欄 `i` の OVERLAPPED を 0 にして貸す
        fn lend(&mut self, i: usize) -> *mut OVERLAPPED {
            assert!(!self.lent[i], "slot {i} is still lent");
            self.lent[i] = true;
            let ov = self.ovs[i].get();
            // SAFETY: 欄 `i` は貸していなかったので OS は触れていない。`UnsafeCell` の中なので、ほかの参照とも重ならない
            unsafe { ov.write(OVERLAPPED::default()) };
            ov
        }

        /// 欄 `i` で出した重なった呼び出しの戻りを受ける。`Err` はその場の失敗で、packet は来ないので欄を返してもらう
        fn issued(&mut self, i: usize, r: Result<i32, i32>) -> Result<i32, i32> {
            if r.is_err() {
                self.lent[i] = false;
            }
            r
        }

        fn ov(&self, i: usize) -> *mut OVERLAPPED {
            self.ovs[i].get()
        }

        /// `timeout` まで次の packet を待つ。貸した欄を名指す packet なら、その欄を返してもらう
        fn next(&mut self, timeout: Duration) -> Option<Packet> {
            let mut entry = [OVERLAPPED_ENTRY::default(); 1];
            let mut removed = 0;
            let ms = crate::sys::millis(timeout, INFINITE - 1);
            // SAFETY: ポートは `self` が持っていて開いている。`entry` と `removed` は局所変数で、OS が書くのは戻るまでの
            // 間だけである。`alertable` が `FALSE` なので APC は走らない
            let ok = unsafe {
                GetQueuedCompletionStatusEx(
                    self.port.as_raw_handle(),
                    entry.as_mut_ptr(),
                    1,
                    &mut removed,
                    ms,
                    FALSE,
                )
            };
            if ok == 0 {
                let e = io::Error::last_os_error();
                assert_eq!(e.raw_os_error(), Some(WAIT_TIMEOUT as i32), "{e}");
                return None;
            }
            let e = entry[0];
            if let Some(i) = (0..SLOTS).find(|&i| self.ov(i) == e.lpOverlapped) {
                assert!(self.lent[i], "a packet named slot {i}, which was not lent");
                self.lent[i] = false;
            }
            Some(Packet {
                key: e.lpCompletionKey,
                ov: e.lpOverlapped,
                bytes: e.dwNumberOfBytesTransferred,
            })
        }

        /// 1 秒以内に、欄 `i` を名指すソケットの packet が来ることを確かめる
        fn packet_of(&mut self, i: usize) -> Packet {
            let p = self.next(LIMIT).expect("no packet arrived within 1 s");
            assert_eq!((p.key, p.ov), (SOCK, self.ov(i)));
            p
        }

        /// 印の packet を積み、それが次の packet で、その後の列が空であることを確かめる。packet がないと言い切れるのは、
        /// 戻る前に終わった呼び出しについてだけである。OS がまだ積んでいない packet は印の後に来うる
        fn only_marks(&mut self) {
            // SAFETY: ポートは `self` が持っていて開いている。OVERLAPPED は null で、OS は読まずにそのまま返す
            let ok = unsafe {
                PostQueuedCompletionStatus(self.port.as_raw_handle(), 0, MARK, ptr::null())
            };
            assert_ne!(ok, 0, "{}", io::Error::last_os_error());
            let p = self
                .next(LIMIT)
                .expect("the mark did not arrive within 1 s");
            assert_eq!((p.key, p.ov), (MARK, ptr::null_mut()));
            assert!(
                self.next(Duration::ZERO).is_none(),
                "a packet followed the mark"
            );
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            // 貸したままの OVERLAPPED には OS がまだ書きうるので、放さずにプロセスごと終える。nextest はテストごとに
            // 別のプロセスで流すので、ほかのテストを巻き込まない
            if self.lent.iter().any(|&l| l) {
                eprintln!("a probe left an OVERLAPPED with the kernel; aborting");
                std::process::abort();
            }
        }
    }

    /// 取り出した packet の操作の結果を読む。`Ok` は運んだバイト数、`Err` は誤りの番号
    fn result(s: SOCKET, ov: *mut OVERLAPPED) -> Result<u32, i32> {
        let mut bytes = 0;
        let mut flags = 0;
        // SAFETY: `ov` は取り出した packet が名指した欄で、操作は終わっているので OS はもう書かない。`bytes` と `flags`
        // は局所変数で、待たない (`FALSE`) ので戻るまでの間だけ書かれる
        let ok = unsafe { WSAGetOverlappedResult(s, ov, &mut bytes, FALSE, &mut flags) };
        if ok != 0 { Ok(bytes) } else { Err(last()) }
    }

    fn raw(s: &impl AsRawSocket) -> SOCKET {
        s.as_raw_socket() as SOCKET
    }

    fn nonblocking(s: SOCKET) {
        let mut on = 1u32;
        // SAFETY: `s` は呼ぶ側が開いたまま持つソケットで、`on` は局所変数である
        assert_eq!(unsafe { ioctlsocket(s, FIONBIO, &mut on) }, 0, "{}", last());
    }

    /// 重なった操作を出せるソケットを作る。backend と同じく継承させない
    fn socket(v6: bool) -> OwnedSocket {
        let af = if v6 { AF_INET6 } else { AF_INET };
        // SAFETY: protocol info は null で、渡すポインタはない。失敗は `INVALID_SOCKET` で返る
        let s = unsafe {
            WSASocketW(
                i32::from(af),
                SOCK_STREAM,
                IPPROTO_TCP,
                ptr::null(),
                0,
                WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
            )
        };
        assert_ne!(s, INVALID_SOCKET, "{}", last());
        // SAFETY: `s` は今作ったソケットで、ほかに所有者がいない
        unsafe { OwnedSocket::from_raw_socket(s as _) }
    }

    fn sockaddr(addr: SocketAddr) -> (SOCKADDR_STORAGE, i32) {
        let mut storage = SOCKADDR_STORAGE::default();
        let len = match addr {
            SocketAddr::V4(a) => {
                let mut sin = SOCKADDR_IN {
                    sin_family: AF_INET,
                    sin_port: a.port().to_be(),
                    ..Default::default()
                };
                sin.sin_addr.S_un.S_addr = u32::from_ne_bytes(a.ip().octets());
                // SAFETY: `SOCKADDR_STORAGE` はどの番地の型より大きく、どの番地の型の整列も満たす
                unsafe { (&raw mut storage).cast::<SOCKADDR_IN>().write(sin) };
                size_of::<SOCKADDR_IN>()
            }
            SocketAddr::V6(a) => {
                let mut sin6 = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_port: a.port().to_be(),
                    sin6_flowinfo: a.flowinfo(),
                    ..Default::default()
                };
                sin6.sin6_addr.u.Byte = a.ip().octets();
                sin6.Anonymous.sin6_scope_id = a.scope_id();
                // SAFETY: `SOCKADDR_STORAGE` はどの番地の型より大きく、どの番地の型の整列も満たす
                unsafe { (&raw mut storage).cast::<SOCKADDR_IN6>().write(sin6) };
                size_of::<SOCKADDR_IN6>()
            }
        };
        (storage, len as i32)
    }

    fn bind_to(s: SOCKET, addr: SocketAddr) {
        let (storage, len) = sockaddr(addr);
        // SAFETY: `storage` は局所変数で、`len` はその中の番地の長さである。OS は戻るまでの間だけ読む
        let r = unsafe { bind(s, (&raw const storage).cast::<SOCKADDR>(), len) };
        assert_eq!(r, 0, "{}", last());
    }

    /// ソケットの自分のポート
    fn port_of(s: SOCKET) -> u16 {
        let mut storage = SOCKADDR_STORAGE::default();
        let mut len = size_of::<SOCKADDR_STORAGE>() as i32;
        // SAFETY: `storage` と `len` は局所変数で、`len` は `storage` の大きさである
        let r = unsafe { getsockname(s, (&raw mut storage).cast(), &mut len) };
        assert_eq!(r, 0, "{}", last());
        // SAFETY: OS が書いた番地は `SOCKADDR_IN` か `SOCKADDR_IN6` で、どちらも族の欄の直後にポートを置くので、
        // `SOCKADDR_IN` として読んでもポートは正しい
        u16::from_be(unsafe { (&raw const storage).cast::<SOCKADDR_IN>().read() }.sin_port)
    }

    /// IPv4 のソケットの相手の番地
    fn peer_of(s: SOCKET) -> SocketAddr {
        let mut storage = SOCKADDR_STORAGE::default();
        let mut len = size_of::<SOCKADDR_STORAGE>() as i32;
        // SAFETY: `storage` と `len` は局所変数で、`len` は `storage` の大きさである
        let r = unsafe { getpeername(s, (&raw mut storage).cast(), &mut len) };
        assert_eq!(r, 0, "{}", last());
        // SAFETY: `SOCKADDR_STORAGE` は `SOCKADDR_IN` より大きく整列も満たす。族は下で確かめる
        let sin = unsafe { (&raw const storage).cast::<SOCKADDR_IN>().read() };
        assert_eq!(sin.sin_family, AF_INET);
        // SAFETY: `S_un` のどの欄も同じ 4 バイトを読む
        let ip = unsafe { sin.sin_addr.S_un.S_addr };
        SocketAddr::from((ip.to_ne_bytes(), u16::from_be(sin.sin_port)))
    }

    fn recv_on(s: SOCKET, buf: &mut [u8], flags: i32) -> Result<usize, i32> {
        // SAFETY: `buf` は戻るまで借りていて、長さは `buf` の長さである
        let n = unsafe { recv(s, buf.as_mut_ptr(), buf.len() as i32, flags) };
        if n == SOCKET_ERROR {
            Err(last())
        } else {
            Ok(n as usize)
        }
    }

    fn send_on(s: SOCKET, buf: &[u8]) -> Result<usize, i32> {
        // SAFETY: `buf` は戻るまで借りていて、長さは `buf` の長さである
        let n = unsafe { send(s, buf.as_ptr(), buf.len() as i32, 0) };
        if n == SOCKET_ERROR {
            Err(last())
        } else {
            Ok(n as usize)
        }
    }

    fn accept_on(l: SOCKET) -> Result<OwnedSocket, i32> {
        // SAFETY: 相手の番地は求めないので、渡すポインタはない
        let s = unsafe { accept(l, ptr::null_mut(), ptr::null_mut()) };
        if s == INVALID_SOCKET {
            Err(last())
        } else {
            // SAFETY: `s` は今受けたソケットで、ほかに所有者がいない
            Ok(unsafe { OwnedSocket::from_raw_socket(s as _) })
        }
    }

    /// ブロックしないリスナーで、来ているはずの接続を 1 秒まで受け直す
    fn accept_within(l: SOCKET) -> OwnedSocket {
        let start = Instant::now();
        loop {
            match accept_on(l) {
                Ok(s) => return s,
                Err(WSAEWOULDBLOCK) if start.elapsed() < LIMIT => thread::yield_now(),
                Err(e) => panic!("accept: {e}"),
            }
        }
    }

    /// 重なった呼び出しの戻り。0 は同期の成功、`WSA_IO_PENDING` は保留で、どちらでも packet が来る。`Err` はその場の
    /// 失敗の番号で、packet は来ない
    fn overlapped(r: i32) -> Result<i32, i32> {
        if r == 0 {
            return Ok(0);
        }
        match last() {
            WSA_IO_PENDING => Ok(WSA_IO_PENDING),
            e => Err(e),
        }
    }

    /// 長さ 0 の読みを出す。`ov` は `Rig::lend` で借りた欄で、packet を取り出すまで貸したままにする
    fn zero_read(s: SOCKET, ov: *mut OVERLAPPED) -> Result<i32, i32> {
        let buf = WSABUF {
            len: 0,
            buf: ptr::null_mut(),
        };
        let mut flags = 0u32;
        // SAFETY: `ov` は貸した欄で、packet を取り出すまで生きている。`buf` は長さ 0 で、OS は何も書かない。`buf` と
        // `flags` を OS が読み書きするのは戻るまでの間だけで、終わりの flags は `WSAGetOverlappedResult` で読む
        overlapped(unsafe { WSARecv(s, &buf, 1, ptr::null_mut(), &mut flags, ov, None) })
    }

    /// 重なった書きを出す。`data` と `ov` は packet を取り出すまで生かしておく
    fn wsa_send(s: SOCKET, data: &[u8], ov: *mut OVERLAPPED) -> Result<i32, i32> {
        let buf = WSABUF {
            len: data.len() as u32,
            buf: data.as_ptr().cast_mut(),
        };
        // SAFETY: `ov` は貸した欄で、`data` は呼ぶ側が packet を取り出すまで生かす。OS は `data` を読むだけである。
        // `buf` は戻るまでの間だけ読まれる
        overlapped(unsafe { WSASend(s, &buf, 1, ptr::null_mut(), 0, ov, None) })
    }

    /// 拡張関数のポインタを読む
    fn ext<T: Default>(s: SOCKET, guid: GUID) -> T {
        let mut f = T::default();
        let mut bytes = 0;
        // SAFETY: 重ならない呼び出しで、`guid`、`f`、`bytes` は戻るまでの間だけ読み書きされる。出力の長さは `T` の
        // 大きさで、`T` は関数ポインタの `Option` である
        let r = unsafe {
            WSAIoctl(
                s,
                SIO_GET_EXTENSION_FUNCTION_POINTER,
                (&raw const guid).cast(),
                size_of::<GUID>() as u32,
                (&raw mut f).cast(),
                size_of::<T>() as u32,
                &mut bytes,
                ptr::null_mut(),
                None,
            )
        };
        assert_eq!(r, 0, "{}", last());
        f
    }

    /// ループバックの接続。サーバの端はブロックせず、ポートに結び付ける。相手の読みは 1 秒で切る
    fn pair(rig: &Rig) -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client.set_read_timeout(Some(LIMIT)).unwrap();
        let (server, _) = listener.accept().unwrap();
        server.set_nonblocking(true).unwrap();
        rig.associate(raw(&server));
        (client, server)
    }

    /// 相手が書いたデータが届くまで、1 秒まで覗き直す
    fn wait_readable(s: SOCKET) {
        let start = Instant::now();
        let mut b = [0u8; 1];
        while recv_on(s, &mut b, MSG_PEEK) == Err(WSAEWOULDBLOCK) {
            assert!(
                start.elapsed() < LIMIT,
                "the data did not arrive within 1 s"
            );
            thread::yield_now();
        }
    }

    fn cancel(s: SOCKET, ov: *mut OVERLAPPED) -> Result<(), u32> {
        // SAFETY: `ov` は貸した欄の番地で、OS は番地を操作の照合に使うだけである
        let ok = unsafe { CancelIoEx(s as HANDLE, ov) };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error().raw_os_error().unwrap() as u32)
        }
    }

    #[test]
    fn a_zero_read_waits_for_data_and_leaves_it() {
        let mut rig = Rig::new();
        let (mut client, server) = pair(&rig);
        let ov = rig.lend(0);
        let r = zero_read(raw(&server), ov);
        assert_eq!(rig.issued(0, r), Ok(WSA_IO_PENDING));
        rig.only_marks();
        client.write_all(b"ab").unwrap();
        let p = rig.packet_of(0);
        assert_eq!(p.bytes, 0);
        assert_eq!(result(raw(&server), p.ov), Ok(0));
        let mut buf = [0u8; 8];
        assert_eq!(recv_on(raw(&server), &mut buf, 0), Ok(2));
        assert_eq!(&buf[..2], b"ab");
        assert_eq!(recv_on(raw(&server), &mut buf, 0), Err(WSAEWOULDBLOCK));
    }

    #[test]
    fn a_zero_read_completes_on_a_closed_peer() {
        let mut rig = Rig::new();
        let (client, server) = pair(&rig);
        let ov = rig.lend(0);
        let r = zero_read(raw(&server), ov);
        assert_eq!(rig.issued(0, r), Ok(WSA_IO_PENDING));
        client.shutdown(Shutdown::Write).unwrap();
        let p = rig.packet_of(0);
        assert_eq!(result(raw(&server), p.ov), Ok(0));
        assert_eq!(recv_on(raw(&server), &mut [0u8; 8], 0), Ok(0));
    }

    #[test]
    fn a_zero_read_ends_on_a_reset() {
        let mut rig = Rig::new();
        let (client, server) = pair(&rig);
        let ov = rig.lend(0);
        let r = zero_read(raw(&server), ov);
        assert_eq!(rig.issued(0, r), Ok(WSA_IO_PENDING));
        let linger = LINGER {
            l_onoff: 1,
            l_linger: 0,
        };
        // SAFETY: `linger` は局所変数で、長さはその大きさである。OS は戻るまでの間だけ読む
        let r = unsafe {
            setsockopt(
                raw(&client),
                SOL_SOCKET,
                SO_LINGER,
                (&raw const linger).cast(),
                size_of::<LINGER>() as i32,
            )
        };
        assert_eq!(r, 0, "{}", last());
        // 長さ 0 の linger で閉じると RST が出る
        drop(client);
        let p = rig.packet_of(0);
        let code = match result(raw(&server), p.ov) {
            Err(code) => code,
            Ok(_) => recv_on(raw(&server), &mut [0u8; 8], 0).unwrap_err(),
        };
        assert!(
            [WSAECONNRESET, WSAECONNABORTED, ERROR_NETNAME_DELETED].contains(&code),
            "{code}"
        );
    }

    #[test]
    fn a_zero_read_on_buffered_data_queues_a_packet_that_a_cancel_does_not_find() {
        let mut rig = Rig::new();
        let (mut client, server) = pair(&rig);
        client.write_all(b"ab").unwrap();
        wait_readable(raw(&server));
        let ov = rig.lend(0);
        let r = zero_read(raw(&server), ov);
        assert_eq!(rig.issued(0, r), Ok(0));
        // 同期に終わった操作は取り消す相手にならないが、packet はもう列にある
        assert_eq!(cancel(raw(&server), ov), Err(ERROR_NOT_FOUND));
        let p = rig.packet_of(0);
        assert_eq!(result(raw(&server), p.ov), Ok(0));
        rig.only_marks();
        assert_eq!(recv_on(raw(&server), &mut [0u8; 8], 0), Ok(2));
    }

    #[test]
    fn a_cancelled_zero_read_ends_aborted() {
        let mut rig = Rig::new();
        let (_client, server) = pair(&rig);
        let ov = rig.lend(0);
        let r = zero_read(raw(&server), ov);
        assert_eq!(rig.issued(0, r), Ok(WSA_IO_PENDING));
        assert_eq!(cancel(raw(&server), ov), Ok(()));
        let p = rig.packet_of(0);
        assert_eq!(result(raw(&server), p.ov), Err(WSA_OPERATION_ABORTED));
    }

    #[test]
    fn calls_without_an_overlapped_queue_no_packet() {
        let mut rig = Rig::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let l = raw(&listener);
        rig.associate(l);
        assert_eq!(accept_on(l).err(), Some(WSAEWOULDBLOCK));
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client.set_read_timeout(Some(LIMIT)).unwrap();
        let server = accept_within(l);
        let s = raw(&server);
        nonblocking(s);
        // 受けたソケットはリスナーの結び付けを受け継がないので、ここで結び付けられる
        rig.associate(s);
        let mut buf = [0u8; 8];
        assert_eq!(recv_on(s, &mut buf, 0), Err(WSAEWOULDBLOCK));
        client.write_all(b"ab").unwrap();
        wait_readable(s);
        assert_eq!(recv_on(s, &mut buf, 0), Ok(2));
        assert_eq!(send_on(s, b"cd"), Ok(2));
        let on = 1u32;
        // SAFETY: `on` は局所変数で、長さはその大きさである。OS は戻るまでの間だけ読む
        let r = unsafe { setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (&raw const on).cast(), 4) };
        assert_eq!(r, 0, "{}", last());
        assert_eq!(port_of(s), listener.local_addr().unwrap().port());
        assert_eq!(peer_of(s), client.local_addr().unwrap());
        let _: LPFN_CONNECTEX = ext(s, WSAID_CONNECTEX);
        // SAFETY: `s` は `server` が開いたまま持つソケットである
        assert_eq!(unsafe { shutdown(s, SD_SEND) }, 0, "{}", last());
        rig.only_marks();
        let mut got = [0u8; 2];
        client.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"cd");
    }

    #[test]
    fn an_overlapped_send_that_completes_at_once_still_queues_a_packet() {
        let mut rig = Rig::new();
        let (mut client, server) = pair(&rig);
        let ov = rig.lend(0);
        let r = wsa_send(raw(&server), b"abcd", ov);
        assert_eq!(rig.issued(0, r), Ok(0));
        let p = rig.packet_of(0);
        assert_eq!((p.bytes, result(raw(&server), p.ov)), (4, Ok(4)));
        rig.only_marks();
        let mut got = [0u8; 4];
        client.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"abcd");
    }

    #[test]
    fn an_overlapped_send_on_a_full_socket_pends_and_cancels() {
        // OS が読む buffer は `rig` より先に作り、`rig` が abort するまで放さない
        let big = vec![7u8; 8 << 20];
        let mut rig = Rig::new();
        let (mut client, server) = pair(&rig);
        let chunk = vec![7u8; 1 << 16];
        let mut filled = 0usize;
        loop {
            match send_on(raw(&server), &chunk) {
                Ok(n) => filled += n,
                Err(WSAEWOULDBLOCK) => break,
                Err(e) => panic!("send: {e}"),
            }
            assert!(
                filled < 1 << 28,
                "the send buffer did not fill within 256 MiB"
            );
        }
        // 送りの buffer は広がりうるので、塞がった後の重なった書きがその場で終わることもある。確かめるのは
        // `WSAEWOULDBLOCK` で断られないことである
        let ov = rig.lend(0);
        let r = wsa_send(raw(&server), &big, ov);
        assert!(rig.issued(0, r).is_ok(), "{r:?}");
        let mut drained = 0;
        let mut sink = vec![0u8; 1 << 16];
        while drained < 1 << 20 {
            drained += client.read(&mut sink).unwrap();
        }
        let c = cancel(raw(&server), ov);
        assert!(matches!(c, Ok(()) | Err(ERROR_NOT_FOUND)), "{c:?}");
        let p = rig.packet_of(0);
        match result(raw(&server), p.ov) {
            Ok(n) => {
                assert!(n as usize <= big.len());
                eprintln!("the cancelled send ended Ok({n})");
            }
            Err(e) => {
                assert_eq!(e, WSA_OPERATION_ABORTED);
                eprintln!("the cancelled send ended aborted ({e})");
            }
        }
        rig.only_marks();
    }

    /// AcceptEx を出す。受けのデータは求めず、番地の欄だけを `addrs` に置く
    fn accept_ex(
        rig: &mut Rig,
        i: usize,
        l: SOCKET,
        a: SOCKET,
        addrs: &mut Addrs,
    ) -> Result<i32, i32> {
        let f: LPFN_ACCEPTEX = ext(l, WSAID_ACCEPTEX);
        let f = f.expect("no AcceptEx");
        let ov = rig.lend(i);
        let mut bytes = 0;
        // SAFETY: `ov` は貸した欄で、`addrs` は呼ぶ側が packet を取り出すまで生かす。受けのデータの長さは 0 なので、
        // OS が `addrs` に書くのは2つの番地の欄だけで、どちらも `ADDR_LEN` に収まる。`bytes` は同期に終わったときだけ
        // 戻るまでの間に書かれる
        let ok = unsafe {
            f(
                l,
                a,
                addrs.as_mut_ptr().cast(),
                0,
                ADDR_LEN as u32,
                ADDR_LEN as u32,
                &mut bytes,
                ov,
            )
        };
        // AcceptEx は BOOL を返すので、0 でない値が成功である
        let r = overlapped(if ok != 0 { 0 } else { SOCKET_ERROR });
        rig.issued(i, r)
    }

    fn update_accept(l: SOCKET, a: SOCKET) {
        // SAFETY: `l` はこの関数の引数で、長さはその大きさである。OS は戻るまでの間だけ読む
        let r = unsafe {
            setsockopt(
                a,
                SOL_SOCKET,
                SO_UPDATE_ACCEPT_CONTEXT,
                (&raw const l).cast(),
                size_of::<SOCKET>() as i32,
            )
        };
        assert_eq!(r, 0, "{}", last());
    }

    #[test]
    fn accept_and_accept_ex_share_a_listener() {
        // OS が書く buffer は `rig` より先に作り、`rig` が abort するまで放さない
        let mut addrs = Box::new([0u8; 2 * ADDR_LEN]);
        let mut rig = Rig::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let l = raw(&listener);
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        rig.associate(l);
        // 先に `accept` を試し、塞がれたら AcceptEx を出す
        assert_eq!(accept_on(l).err(), Some(WSAEWOULDBLOCK));
        let a = socket(false);
        assert_eq!(
            accept_ex(&mut rig, 0, l, raw(&a), &mut addrs),
            Ok(WSA_IO_PENDING)
        );
        rig.only_marks();
        let c1 = TcpStream::connect(addr).unwrap();
        let p = rig.packet_of(0);
        assert_eq!(result(l, p.ov), Ok(0));
        update_accept(l, raw(&a));
        nonblocking(raw(&a));
        rig.associate(raw(&a));
        assert_eq!(peer_of(raw(&a)), c1.local_addr().unwrap());
        // AcceptEx の後に溜まった接続は `accept` が来た順に受ける
        let c2 = TcpStream::connect(addr).unwrap();
        let c3 = TcpStream::connect(addr).unwrap();
        for c in [&c2, &c3] {
            let s = accept_within(l);
            assert_eq!(peer_of(raw(&s)), c.local_addr().unwrap());
        }
        assert_eq!(accept_on(l).err(), Some(WSAEWOULDBLOCK));
        rig.only_marks();
        // `accept` が塞がれた後、AcceptEx を出す前に来た接続も AcceptEx が受ける
        let c4 = TcpStream::connect(addr).unwrap();
        let b = socket(false);
        let r = accept_ex(&mut rig, 1, l, raw(&b), &mut addrs);
        assert!(r.is_ok(), "{r:?}");
        let p = rig.packet_of(1);
        assert_eq!(result(l, p.ov), Ok(0));
        update_accept(l, raw(&b));
        assert_eq!(peer_of(raw(&b)), c4.local_addr().unwrap());
        rig.only_marks();
    }

    #[test]
    fn a_cancelled_accept_ex_ends_aborted() {
        // OS が書く buffer は `rig` より先に作り、`rig` が abort するまで放さない
        let mut addrs = Box::new([0u8; 2 * ADDR_LEN]);
        let mut rig = Rig::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let l = raw(&listener);
        listener.set_nonblocking(true).unwrap();
        rig.associate(l);
        let a = socket(false);
        assert_eq!(
            accept_ex(&mut rig, 0, l, raw(&a), &mut addrs),
            Ok(WSA_IO_PENDING)
        );
        // AcceptEx の操作はリスナーの上にあるので、リスナーで取り消す
        assert_eq!(cancel(l, rig.ov(0)), Ok(()));
        let p = rig.packet_of(0);
        assert_eq!(result(l, p.ov), Err(WSA_OPERATION_ABORTED));
        drop(a);
        let c = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let s = accept_within(l);
        assert_eq!(peer_of(raw(&s)), c.local_addr().unwrap());
    }

    /// ConnectEx に渡すソケット。backend と同じく any に bind し、`rto` なら SYN を送り直さない設定にして、
    /// ブロックしないようにしてからポートに結び付ける
    fn connector(rig: &Rig, v6: bool, rto: bool) -> OwnedSocket {
        let s = socket(v6);
        let any: SocketAddr = if v6 { "[::]:0" } else { "0.0.0.0:0" }.parse().unwrap();
        bind_to(raw(&s), any);
        if rto {
            // 拒まれた接続を、送り直しの間 (約 2 秒) 待たずに知る
            let p = TCP_INITIAL_RTO_PARAMETERS {
                Rtt: TCP_INITIAL_RTO_DEFAULT_RTT as u16,
                MaxSynRetransmissions: NO_SYN_RETRANSMISSIONS,
            };
            let mut bytes = 0;
            // SAFETY: 重ならない呼び出しで、`p` と `bytes` は戻るまでの間だけ読み書きされる。出力の buffer はない
            let r = unsafe {
                WSAIoctl(
                    raw(&s),
                    SIO_TCP_INITIAL_RTO,
                    (&raw const p).cast(),
                    size_of::<TCP_INITIAL_RTO_PARAMETERS>() as u32,
                    ptr::null_mut(),
                    0,
                    &mut bytes,
                    ptr::null_mut(),
                    None,
                )
            };
            assert_eq!(r, 0, "{}", last());
        }
        nonblocking(raw(&s));
        rig.associate(raw(&s));
        s
    }

    /// 欄 `i` で ConnectEx を出す
    fn connect_ex(rig: &mut Rig, i: usize, s: SOCKET, peer: SocketAddr) -> Result<i32, i32> {
        let f: LPFN_CONNECTEX = ext(s, WSAID_CONNECTEX);
        let f = f.expect("no ConnectEx");
        let (to, len) = sockaddr(peer);
        let ov = rig.lend(i);
        // SAFETY: `ov` は貸した欄で、呼ぶ側が packet を取り出すまで生かす。`to` は局所変数で、`len` はその中の番地の
        // 長さである。送るデータはない
        let ok = unsafe {
            f(
                s,
                (&raw const to).cast(),
                len,
                ptr::null(),
                0,
                ptr::null_mut(),
                ov,
            )
        };
        // ConnectEx は BOOL を返すので、0 でない値が成功である
        let r = overlapped(if ok != 0 { 0 } else { SOCKET_ERROR });
        rig.issued(i, r)
    }

    /// 欄 `i` の ConnectEx の終わりを待つ。同期の失敗ならその番号を返す
    fn connected(rig: &mut Rig, i: usize, s: SOCKET, issued: Result<i32, i32>) -> Result<u32, i32> {
        issued?;
        let p = rig.packet_of(i);
        result(s, p.ov)
    }

    /// `ip` のポート 0 に bind だけして listen しないソケットと、その番地。ここへの接続は拒まれる。ソケットを持つ間は
    /// 並行するテストにポートを取られない
    fn refusing(ip: &str) -> (OwnedSocket, SocketAddr) {
        let ip: IpAddr = ip.parse().unwrap();
        let s = socket(ip.is_ipv6());
        bind_to(raw(&s), SocketAddr::new(ip, 0));
        let port = port_of(raw(&s));
        (s, SocketAddr::new(ip, port))
    }

    #[test]
    fn a_refused_loopback_connect_ex_fails_fast() {
        for ip in ["127.0.0.1", "::1"] {
            let mut rig = Rig::new();
            let (_held, to) = refusing(ip);
            let s = connector(&rig, to.is_ipv6(), true);
            let start = Instant::now();
            // SYN を送り直せば約 2 秒かかり、`packet_of` の 1 秒の待ちで落ちる
            let issued = connect_ex(&mut rig, 0, raw(&s), to);
            let code = connected(&mut rig, 0, raw(&s), issued).unwrap_err();
            let took = start.elapsed();
            assert!(
                [WSAECONNREFUSED, ERROR_CONNECTION_REFUSED].contains(&code),
                "{ip}: {code}"
            );
            eprintln!("{ip}: refused ({code}) after {took:?}");
        }
    }

    #[test]
    fn a_connect_ex_connects_after_a_bind_to_any() {
        let mut rig = Rig::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let s = connector(&rig, false, true);
        let issued = connect_ex(&mut rig, 0, raw(&s), addr);
        assert_eq!(connected(&mut rig, 0, raw(&s), issued), Ok(0));
        // SAFETY: 値を渡さない設定で、渡すポインタは null である
        let r = unsafe {
            setsockopt(
                raw(&s),
                SOL_SOCKET,
                SO_UPDATE_CONNECT_CONTEXT,
                ptr::null(),
                0,
            )
        };
        assert_eq!(r, 0, "{}", last());
        assert_eq!(peer_of(raw(&s)), addr);
        let (mut server, _) = listener.accept().unwrap();
        server.set_read_timeout(Some(LIMIT)).unwrap();
        // SAFETY: `s` は開いたまま持つソケットである
        assert_eq!(unsafe { shutdown(raw(&s), SD_SEND) }, 0, "{}", last());
        assert_eq!(server.read(&mut [0u8; 8]).unwrap(), 0);
        rig.only_marks();
    }

    #[test]
    fn large_nonblocking_sends_block_within_eight_calls() {
        let rig = Rig::new();
        let (_client, server) = pair(&rig);
        let chunk = vec![7u8; 8 << 20];
        let mut taken = Vec::new();
        loop {
            match send_on(raw(&server), &chunk) {
                Ok(n) => taken.push(n),
                Err(WSAEWOULDBLOCK) => break,
                Err(e) => panic!("send: {e}"),
            }
            assert!(
                taken.len() < 8,
                "no WSAEWOULDBLOCK within 8 sends: {taken:?}"
            );
        }
        eprintln!("bytes taken before WSAEWOULDBLOCK: {taken:?}");
    }

    /// 受けの列が満ちたリスナーへの接続がどう終わるかを表示する。操作が終わることだけを確かめ、どの終わり方かは
    /// 決めない
    #[test]
    fn a_connect_to_a_full_accept_queue_is_refused_or_stays_pending() {
        let mut rig = Rig::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut fillers = Vec::new();
        while fillers.len() < 1024 {
            match TcpStream::connect_timeout(&addr, Duration::from_millis(50)) {
                Ok(c) => fillers.push(c),
                Err(_) => break,
            }
        }
        let n = fillers.len();
        let s = connector(&rig, false, false);
        let issued = connect_ex(&mut rig, 0, raw(&s), addr);
        if let Err(code) = issued {
            eprintln!("after {n} connections: refused at once ({code})");
            return;
        }
        match rig.next(Duration::from_millis(100)) {
            Some(p) => {
                assert_eq!((p.key, p.ov), (SOCK, rig.ov(0)));
                eprintln!(
                    "after {n} connections: ended {:?} within 100 ms",
                    result(raw(&s), p.ov)
                );
            }
            None => {
                let c = cancel(raw(&s), rig.ov(0));
                assert!(matches!(c, Ok(()) | Err(ERROR_NOT_FOUND)), "{c:?}");
                let p = rig.packet_of(0);
                eprintln!(
                    "after {n} connections: pending after 100 ms, then {:?} on a cancel",
                    result(raw(&s), p.ov)
                );
            }
        }
    }

    #[test]
    fn an_op_outlives_the_thread_that_issued_it() {
        let mut rig = Rig::new();
        let (mut client, server) = pair(&rig);
        let s = raw(&server);
        // 生のポインタは `Send` でないので、番地を数に直してスレッドへ渡す
        let ov = rig.lend(0).expose_provenance();
        let r = thread::spawn(move || zero_read(s, ptr::with_exposed_provenance_mut(ov)))
            .join()
            .unwrap();
        assert_eq!(rig.issued(0, r), Ok(WSA_IO_PENDING));
        // ポートに結び付いたソケットの操作は、出したスレッドが終わっても取り消されない
        client.write_all(b"a").unwrap();
        let p = rig.packet_of(0);
        assert_eq!(result(s, p.ov), Ok(0));
    }
}

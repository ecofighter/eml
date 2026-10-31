//! Windows の backend (IOCP)。ソケットをポートに結び付け、重なった操作の終わりをポートの packet で読む
//! (docs/spec/vm.md の「時計と driver」)。
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::io;
use std::mem::size_of;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};
use std::os::windows::io::{
    AsRawHandle, AsRawSocket, FromRawSocket, HandleOrNull, OwnedHandle, OwnedSocket,
};
use std::process;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use eml_rt::{IoWaker, OpKey, RawBuf};
use windows_sys::Win32::Foundation::{
    ERROR_NOT_FOUND, FALSE, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    SetHandleInformation, WAIT_TIMEOUT,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, FIONBIO, INVALID_SOCKET, IPPROTO_TCP, LPFN_ACCEPTEX, LPFN_CONNECTEX,
    MSG_PEEK, SD_SEND, SIO_GET_EXTENSION_FUNCTION_POINTER, SIO_TCP_INITIAL_RTO,
    SO_UPDATE_ACCEPT_CONTEXT, SO_UPDATE_CONNECT_CONTEXT, SOCK_STREAM, SOCKADDR, SOCKADDR_IN,
    SOCKADDR_IN6, SOCKADDR_STORAGE, SOCKET, SOCKET_ERROR, SOL_SOCKET, TCP_INITIAL_RTO_DEFAULT_RTT,
    TCP_INITIAL_RTO_PARAMETERS, TCP_NODELAY, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED,
    WSA_IO_PENDING, WSA_OPERATION_ABORTED, WSABUF, WSADATA, WSAEWOULDBLOCK, WSAGetLastError,
    WSAGetOverlappedResult, WSAID_ACCEPTEX, WSAID_CONNECTEX, WSAIoctl, WSARecv, WSASend,
    WSASocketW, WSAStartup, accept, bind, getpeername, getsockname, ioctlsocket, listen, recv,
    send, setsockopt, shutdown,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED, OVERLAPPED_ENTRY,
    PostQueuedCompletionStatus,
};
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::core::GUID;

use crate::driver::backend::{Arrival, Backend, Cancel, Event, Hold, Outcome, Start, Step};

/// 起こしの packet の completion key
const WAKE: usize = 0;
/// ソケットを結び付けるときの completion key。この key の packet は `OVERLAPPED` の箱を名指す
const SOCKET_KEY: usize = 1;
/// 1回の `GetQueuedCompletionStatusEx` で受け取る packet の数
const ENTRIES: usize = 64;
/// 起こしの packet を積めなかったとき、出し直すまでの間
const POST_RETRY: Duration = Duration::from_millis(1);
const BACKLOG: i32 = 1024;
/// AcceptEx の番地の欄1つの長さ。AcceptEx は番地の最大の長さに 16 バイトを足すことを求める
const ADDR_LEN: usize = size_of::<SOCKADDR_STORAGE>() + 16;
/// windows-sys はこの定数を u16 の 65534 で持つが、`MaxSynRetransmissions` は u8 なので (UCHAR)-2 を書く
const NO_SYN_RETRANSMISSIONS: u8 = 254;
/// Winsock は重なった操作の終わりの誤りを、WSA の番号でなく Win32 の番号で返すことがある
const ERROR_NETNAME_DELETED: i32 = 64;
const ERROR_CONNECTION_REFUSED: i32 = 1225;
const ERROR_CONNECTION_ABORTED: i32 = 1236;
const BOX_ABORT: &str =
    "eml_io: an OVERLAPPED box of a dropped I/O driver is still held by the kernel; aborting";

/// ポートに結び付けたソケットと、そのアドレスの族。AcceptEx の受けのソケットはリスナーと同じ族で作る
pub(crate) struct Sock {
    s: OwnedSocket,
    v6: bool,
}

impl Sock {
    fn raw(&self) -> SOCKET {
        self.s.as_raw_socket() as SOCKET
    }
}

type AcceptExFn = unsafe extern "system" fn(
    SOCKET,
    SOCKET,
    *mut core::ffi::c_void,
    u32,
    u32,
    u32,
    *mut u32,
    *mut OVERLAPPED,
) -> windows_sys::core::BOOL;
type ConnectExFn = unsafe extern "system" fn(
    SOCKET,
    *const SOCKADDR,
    i32,
    *const core::ffi::c_void,
    u32,
    *mut u32,
    *mut OVERLAPPED,
) -> windows_sys::core::BOOL;

/// `WSAIoctl` で得た拡張関数。windows-sys は `ConnectEx` を関数として持たず、mswsock.dll から link しないため
#[derive(Clone, Copy)]
struct Ext {
    accept_ex: AcceptExFn,
    connect_ex: ConnectExFn,
}

/// OS に出した重なった操作1つの箱。OS に預けてから取り戻すまでは OS が持ち、その間はどのフィールドへも `&` と
/// `&mut` を作らない。OS が書く記憶はどれもこの箱のフィールドに置く。
#[repr(C)]
struct Over {
    /// 先頭に置くので、packet が名指す番地は箱の番地と同じである
    ov: OVERLAPPED,
    /// 呼び出しを出したソケット。AcceptEx ではリスナーである
    on: SOCKET,
    /// `WSARecv` の lpFlags
    flags: u32,
    /// AcceptEx が自分と相手の番地を書く
    addrs: [u8; 2 * ADDR_LEN],
    /// ConnectEx の行き先
    peer: SOCKADDR_STORAGE,
    /// OS は触れない
    what: What,
}

enum What {
    /// 長さ 0 の `WSARecv`
    Zero,
    Send,
    /// AcceptEx。受けのソケットは成功の packet でだけ箱を出て、ほかの終わりでは箱と一緒に閉じる
    Accept {
        sock: Sock,
    },
    Connect,
}

/// OS が持つ箱の記録
struct Lent {
    key: OpKey,
    /// 箱を預けたときのポインタ。箱を取り戻すときは、packet の番地をこの表で確かめてから、このポインタを使う
    over: *mut Over,
    on: SOCKET,
    /// 核が `cancel` で取り消しを頼んだ
    cancelled: bool,
}

pub(crate) struct Iocp {
    waker: Arc<PortWaker>,
    epoch: Instant,
    /// 最初に作ったソケットで読む
    ext: Option<Ext>,
    /// OS が持つ箱すべてを番地で引く。packet の番地は、箱の中を読む前にここで確かめる
    boxes: HashMap<usize, Lent>,
    /// 保留の鍵からその箱の番地へ。`cancel` は鍵しか受け取らない
    waiters: HashMap<OpKey, usize>,
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

/// このスレッドで直前に失敗した Winsock の呼び出しの番号
fn last() -> i32 {
    // SAFETY: 引数はなく、呼んだスレッドの値を読むだけである
    unsafe { WSAGetLastError() }
}

/// Winsock をプロセスに1回だけ始める。`WSACleanup` は呼ばず、std と同じくプロセスと同じだけ生かす
fn startup() -> io::Result<()> {
    static STARTED: OnceLock<i32> = OnceLock::new();
    let code = *STARTED.get_or_init(|| {
        let mut data = WSADATA::default();
        // SAFETY: `data` は局所変数で、OS が書くのは戻るまでの間だけである
        unsafe { WSAStartup(0x202, &mut data) }
    });
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

/// OS の番号を誤りにする。Win32 の番号のうち std が種類に写さないものを、同じ意味の WSA の番号の種類にそろえる
/// (docs/spec/vm.md の「時計と driver」)。995 (中断) はここへ渡さない。std はそれを `TimedOut` にする
fn os_error(code: i32) -> io::Error {
    let kind = match code {
        ERROR_CONNECTION_REFUSED => io::ErrorKind::ConnectionRefused,
        ERROR_CONNECTION_ABORTED => io::ErrorKind::ConnectionAborted,
        ERROR_NETNAME_DELETED => io::ErrorKind::ConnectionReset,
        _ => return io::Error::from_raw_os_error(code),
    };
    io::Error::new(kind, io::Error::from_raw_os_error(code))
}

/// 読み書きの誤り。ストリームでは相手が切った接続を、unix と同じく `ConnectionReset` にする
fn stream_error(code: i32) -> io::Error {
    let e = os_error(code);
    if e.kind() == io::ErrorKind::ConnectionAborted {
        io::Error::new(io::ErrorKind::ConnectionReset, e)
    } else {
        e
    }
}

fn accept_error(code: i32) -> io::Error {
    gone(os_error(code))
}

/// 受ける前か受けた直後に相手がいなくなった接続を `ConnectionAborted` にする。核はそれだけを受け直す
fn gone(e: io::Error) -> io::Error {
    match e.kind() {
        io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::NotConnected => io::Error::new(io::ErrorKind::ConnectionAborted, e),
        _ => e,
    }
}

/// `SOCKET_ERROR` を返す呼び出しの戻りを読む
fn check(r: i32) -> io::Result<()> {
    if r == SOCKET_ERROR {
        Err(os_error(last()))
    } else {
        Ok(())
    }
}

/// 重なった呼び出しが失敗を返したとき、その番号を読む。`WSA_IO_PENDING` は出せたことを表し、packet が来る。
/// 成功も packet が来る (成功のときに packet を省くモードは使わない)
fn issued(failed: bool) -> Result<(), i32> {
    if !failed {
        return Ok(());
    }
    match last() {
        WSA_IO_PENDING => Ok(()),
        code => Err(code),
    }
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

fn from_storage(storage: &SOCKADDR_STORAGE) -> Option<SocketAddr> {
    match storage.ss_family {
        AF_INET => {
            // SAFETY: 族が `AF_INET` なので OS は `SOCKADDR_IN` を書いた。`SOCKADDR_STORAGE` は大きさも整列も足りる
            let sin = unsafe { (&raw const *storage).cast::<SOCKADDR_IN>().read() };
            // SAFETY: `S_un` のどの欄も同じ 4 バイトを読む
            let ip = unsafe { sin.sin_addr.S_un.S_addr };
            Some(SocketAddr::V4(SocketAddrV4::new(
                ip.to_ne_bytes().into(),
                u16::from_be(sin.sin_port),
            )))
        }
        AF_INET6 => {
            // SAFETY: 族が `AF_INET6` なので OS は `SOCKADDR_IN6` を書いた。`SOCKADDR_STORAGE` は大きさも整列も足りる
            let sin6 = unsafe { (&raw const *storage).cast::<SOCKADDR_IN6>().read() };
            // SAFETY: どちらの共用体も、どの欄も同じバイトを整数として読む
            let (ip, scope) = unsafe { (sin6.sin6_addr.u.Byte, sin6.Anonymous.sin6_scope_id) };
            Some(SocketAddr::V6(SocketAddrV6::new(
                ip.into(),
                u16::from_be(sin6.sin6_port),
                sin6.sin6_flowinfo,
                scope,
            )))
        }
        _ => None,
    }
}

fn no_ip() -> io::Error {
    io::Error::other("the peer has no IP address")
}

/// ソケットの自分の番地 (`peer` が偽) か相手の番地
fn name_of(s: SOCKET, peer: bool) -> io::Result<SocketAddr> {
    let mut storage = SOCKADDR_STORAGE::default();
    let mut len = size_of::<SOCKADDR_STORAGE>() as i32;
    let p = (&raw mut storage).cast::<SOCKADDR>();
    // SAFETY: `storage` と `len` は局所変数で、`len` は `storage` の大きさである。OS が書くのは戻るまでの間だけである
    let r = unsafe {
        if peer {
            getpeername(s, p, &mut len)
        } else {
            getsockname(s, p, &mut len)
        }
    };
    check(r)?;
    from_storage(&storage).ok_or_else(no_ip)
}

fn bind_to(s: SOCKET, addr: SocketAddr) -> io::Result<()> {
    let (storage, len) = sockaddr(addr);
    // SAFETY: `storage` は局所変数で、`len` はその中の番地の長さである。OS は戻るまでの間だけ読む
    check(unsafe { bind(s, (&raw const storage).cast::<SOCKADDR>(), len) })
}

fn nonblocking(s: SOCKET) -> io::Result<()> {
    let mut on = 1u32;
    // SAFETY: `on` は局所変数で、OS は戻るまでの間だけ読み書きする
    check(unsafe { ioctlsocket(s, FIONBIO, &mut on) })
}

/// 重なった操作を出せるソケットを作る。子のプロセスには受け継がせない
fn new_socket(v6: bool) -> io::Result<OwnedSocket> {
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
    if s == INVALID_SOCKET {
        return Err(os_error(last()));
    }
    // SAFETY: `s` は今作ったソケットで、ほかに所有者がいない
    Ok(unsafe { OwnedSocket::from_raw_socket(s as _) })
}

/// 拡張関数のポインタを読む。`T` は windows-sys の関数ポインタの `Option` である
fn load<T: Default>(s: SOCKET, guid: GUID) -> io::Result<T> {
    let mut f = T::default();
    let mut bytes = 0;
    // SAFETY: 重ならない呼び出しで、`guid`、`f`、`bytes` は戻るまでの間だけ読み書きされる。出力の長さは `T` の
    // 大きさで、OS はそこへ関数ポインタを書く
    check(unsafe {
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
    })?;
    Ok(f)
}

fn extensions(s: SOCKET) -> io::Result<Ext> {
    let accept_ex: LPFN_ACCEPTEX = load(s, WSAID_ACCEPTEX)?;
    let connect_ex: LPFN_CONNECTEX = load(s, WSAID_CONNECTEX)?;
    match (accept_ex, connect_ex) {
        (Some(accept_ex), Some(connect_ex)) => Ok(Ext {
            accept_ex,
            connect_ex,
        }),
        _ => Err(io::Error::other("Winsock lacks AcceptEx or ConnectEx")),
    }
}

/// ループバックへの接続で SYN を送り直さない設定にする。拒まれた接続を、送り直しの間 (約 2 秒) 待たずに知るため
fn no_syn_retransmissions(s: SOCKET) -> io::Result<()> {
    let p = TCP_INITIAL_RTO_PARAMETERS {
        Rtt: TCP_INITIAL_RTO_DEFAULT_RTT as u16,
        MaxSynRetransmissions: NO_SYN_RETRANSMISSIONS,
    };
    let mut bytes = 0;
    // SAFETY: 重ならない呼び出しで、`p` と `bytes` は戻るまでの間だけ読み書きされる。出力の buffer はない
    check(unsafe {
        WSAIoctl(
            s,
            SIO_TCP_INITIAL_RTO,
            (&raw const p).cast(),
            size_of::<TCP_INITIAL_RTO_PARAMETERS>() as u32,
            ptr::null_mut(),
            0,
            &mut bytes,
            ptr::null_mut(),
            None,
        )
    })
}

fn setsockopt_bytes(s: SOCKET, level: i32, name: i32, value: &[u8]) -> io::Result<()> {
    // SAFETY: `value` は戻るまで借りていて、長さは `value` の長さである。OS は戻るまでの間だけ読む
    check(unsafe { setsockopt(s, level, name, value.as_ptr(), value.len() as i32) })
}

/// 1回の `recv` と `send` に渡せる長さ
fn len(cap: usize) -> i32 {
    i32::try_from(cap).unwrap_or(i32::MAX)
}

/// 取り戻した箱の操作の結果。`Ok` は運んだバイト数、`Err` は誤りの番号
fn status(over: &Over) -> Result<u32, i32> {
    let mut bytes = 0;
    let mut flags = 0;
    // SAFETY: 箱は packet を取り出して取り戻したので、OS はもう書かない。`bytes` と `flags` は局所変数で、待たない
    // (`FALSE`) ので戻るまでの間だけ書かれる
    let ok = unsafe { WSAGetOverlappedResult(over.on, &over.ov, &mut bytes, FALSE, &mut flags) };
    if ok != 0 { Ok(bytes) } else { Err(last()) }
}

impl Iocp {
    pub(crate) fn new() -> io::Result<Self> {
        startup()?;
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
            ext: None,
            boxes: HashMap::new(),
            waiters: HashMap::new(),
        })
    }

    /// ソケットをポートに結び付ける。受けたソケットはリスナーの結び付けを受け継がないので、それぞれ結び付ける
    fn associate(&self, s: SOCKET) -> io::Result<()> {
        // SAFETY: `s` は呼ぶ側が開いたまま持つソケットで、ポートは `waker` が持っていて開いている。渡すポインタはない
        let r = unsafe {
            CreateIoCompletionPort(s as HANDLE, self.waker.port.as_raw_handle(), SOCKET_KEY, 0)
        };
        if r.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// 結び付ける前のソケット。拡張関数は、最初に作ったソケットで読む
    fn stream_socket(&mut self, v6: bool) -> io::Result<Sock> {
        let s = new_socket(v6)?;
        if self.ext.is_none() {
            self.ext = Some(extensions(s.as_raw_socket() as SOCKET)?);
        }
        Ok(Sock { s, v6 })
    }

    /// 拡張関数。歩を受けるソケットはどれもこの backend が作ったので、もう読んである
    fn ext(&self) -> Ext {
        self.ext
            .expect("the extension functions are read with the first socket")
    }

    /// 箱を作って `call` で重なった呼び出しを出す。箱を OS に預けるのはここだけで、出せなかった箱はここで取り戻す
    /// (docs/spec/vm.md の「時計と driver」)。出せたか同期に成功したなら packet が来るので、箱を OS に預けたまま
    /// 2つの表に入れる。
    fn issue(
        &mut self,
        key: OpKey,
        on: SOCKET,
        what: What,
        error: fn(i32) -> io::Error,
        call: impl FnOnce(*mut Over) -> Result<(), i32>,
    ) -> Start<Sock> {
        // 保留の鍵を使い回すのは核の契約の破れである。前の箱を表から失わないよう、OS を呼ばずに断る
        if self.waiters.contains_key(&key) {
            debug_assert!(false, "a pending key is started again");
            return Start::Done(Err(io::Error::other("the key already has a pending op")));
        }
        let over = Box::into_raw(Box::new(Over {
            ov: OVERLAPPED::default(),
            on,
            flags: 0,
            addrs: [0; 2 * ADDR_LEN],
            peer: SOCKADDR_STORAGE::default(),
            what,
        }));
        match call(over) {
            Ok(()) => {
                self.boxes.insert(
                    over.addr(),
                    Lent {
                        key,
                        over,
                        on,
                        cancelled: false,
                    },
                );
                self.waiters.insert(key, over.addr());
                Start::Pending(Hold::Memory)
            }
            Err(code) => {
                // SAFETY: `over` は上で預けた箱で、呼び出しは同期に失敗したので packet は来ず、OS は
                // 箱を持たない。受けのソケットもここで閉じる
                drop(unsafe { Box::from_raw(over) });
                Start::Done(Err(error(code)))
            }
        }
    }

    /// 長さ 0 の `WSARecv` で、読めるデータか EOF か誤りを待つ。読みの buffer は OS に渡さない
    fn zero_read(&mut self, key: OpKey, sock: &Sock) -> Start<Sock> {
        let s = sock.raw();
        self.issue(key, s, What::Zero, stream_error, |over| {
            let buf = WSABUF {
                len: 0,
                buf: ptr::null_mut(),
            };
            // SAFETY: `over` は `issue` が作った生きた箱で、番地は `&raw` で作り参照を作らない。箱は packet を取り出す
            // まで放さないので、OS が `flags` と `ov` に書く間も生きている。`buf` は長さ 0 で、OS は戻るまでの間だけ
            // 読む
            let r = unsafe {
                WSARecv(
                    s,
                    &buf,
                    1,
                    ptr::null_mut(),
                    &raw mut (*over).flags,
                    &raw mut (*over).ov,
                    None,
                )
            };
            issued(r != 0)
        })
    }

    /// 塞がれた書きを `WSASend` で出す。貸された `Write` の記憶をそのまま OS に渡し、`Hold::Memory` が覆う
    fn send_ex(&mut self, key: OpKey, sock: &Sock, buf: RawBuf) -> Start<Sock> {
        let s = sock.raw();
        self.issue(key, s, What::Send, stream_error, |over| {
            let wsabuf = WSABUF {
                len: u32::try_from(buf.cap).unwrap_or(u32::MAX),
                buf: buf.ptr,
            };
            // SAFETY: `over` は `issue` が作った生きた箱で、番地は `&raw` で作る。`buf` の記憶は貸した側が操作の
            // 完了を取り出すまで生かし、OS は読むだけである。`wsabuf` は戻るまでの間だけ読まれる
            let r =
                unsafe { WSASend(s, &wsabuf, 1, ptr::null_mut(), 0, &raw mut (*over).ov, None) };
            issued(r != 0)
        })
    }

    /// 塞がれた受けを AcceptEx で出す。受けのソケットは箱の中に置き、結び付けない。packet はリスナーを通って来る
    fn accept_ex(&mut self, key: OpKey, listener: &Sock) -> Start<Sock> {
        let sock = match self.stream_socket(listener.v6) {
            Ok(sock) => sock,
            Err(e) => return Start::Done(Err(e)),
        };
        let s = sock.raw();
        let l = listener.raw();
        let accept_ex = self.ext().accept_ex;
        self.issue(key, l, What::Accept { sock }, accept_error, |over| {
            let mut bytes = 0;
            // SAFETY: `over` は `issue` が作った生きた箱で、番地は `&raw` で作る。受けのデータの長さは 0 なので、OS が
            // `addrs` に書くのは2つの番地の欄だけで、どちらも `ADDR_LEN` に収まる。`s` は箱の中のソケットで、箱と
            // 一緒に生きる。`bytes` は同期に終わったときだけ戻るまでの間に書かれる
            let ok = unsafe {
                accept_ex(
                    l,
                    s,
                    (&raw mut (*over).addrs).cast(),
                    0,
                    ADDR_LEN as u32,
                    ADDR_LEN as u32,
                    &mut bytes,
                    &raw mut (*over).ov,
                )
            };
            issued(ok == 0)
        })
    }

    /// AcceptEx の成功の後処理。どの失敗でも受けたソケットは閉じる。相手が読めない失敗だけを受け直しにする
    fn accepted(&self, listener: SOCKET, sock: Sock) -> io::Result<Outcome<Sock>> {
        let s = sock.raw();
        setsockopt_bytes(
            s,
            SOL_SOCKET,
            SO_UPDATE_ACCEPT_CONTEXT,
            &listener.to_ne_bytes(),
        )?;
        nonblocking(s)?;
        self.associate(s)?;
        let peer = name_of(s, true).map_err(gone)?;
        Ok(Outcome::Accepted { sock, peer })
    }

    /// 取り戻した箱の終わりを核の知らせにする (docs/spec/vm.md の「時計と driver」)
    fn arrival(&self, over: Box<Over>, cancelled: bool) -> Arrival<Sock> {
        let status = status(&over);
        let Over { on, what, .. } = *over;
        match (what, status) {
            (_, Err(WSA_OPERATION_ABORTED)) if cancelled => Arrival::Cancelled,
            (_, Err(WSA_OPERATION_ABORTED)) => {
                Arrival::Done(Err(io::Error::other("the OS aborted the operation")))
            }
            (What::Zero, Ok(_)) => Arrival::Retry,
            (What::Zero, Err(code)) => Arrival::Done(Err(stream_error(code))),
            (What::Send, r) => {
                Arrival::Done(r.map(|n| Outcome::Size(n as usize)).map_err(stream_error))
            }
            (What::Accept { sock }, Ok(_)) => Arrival::Done(self.accepted(on, sock)),
            (What::Accept { .. }, Err(code)) => Arrival::Done(Err(accept_error(code))),
            (What::Connect, Ok(_)) => Arrival::Done(
                // SAFETY: 値を渡さない設定で、渡すポインタは null である
                check(unsafe {
                    setsockopt(on, SOL_SOCKET, SO_UPDATE_CONNECT_CONTEXT, ptr::null(), 0)
                })
                .map(|()| Outcome::Connected),
            ),
            (What::Connect, Err(code)) => Arrival::Done(Err(os_error(code))),
        }
    }

    fn accept(&mut self, key: OpKey, listener: &Sock) -> Start<Sock> {
        let mut storage = SOCKADDR_STORAGE::default();
        let mut n = size_of::<SOCKADDR_STORAGE>() as i32;
        // SAFETY: `storage` と `n` は局所変数で、`n` は `storage` の大きさである。OS が書くのは戻るまでの間だけである
        let s = unsafe { accept(listener.raw(), (&raw mut storage).cast(), &mut n) };
        if s == INVALID_SOCKET {
            return match last() {
                WSAEWOULDBLOCK => self.accept_ex(key, listener),
                code => Start::Done(Err(accept_error(code))),
            };
        }
        let sock = Sock {
            // SAFETY: `s` は今受けたソケットで、ほかに所有者がいない
            s: unsafe { OwnedSocket::from_raw_socket(s as _) },
            v6: listener.v6,
        };
        Start::Done(self.prepare(sock, &storage))
    }

    /// `accept` で受けたソケットの後処理。どの失敗もそのまま返し、受けたソケットは閉じる
    fn prepare(&self, sock: Sock, peer: &SOCKADDR_STORAGE) -> io::Result<Outcome<Sock>> {
        let s = sock.raw();
        nonblocking(s)?;
        // `accept` のソケットは、リスナーの継承しない設定を受け継がない
        // SAFETY: `s` は `sock` が開いたまま持つソケットで、渡すポインタはない
        if unsafe { SetHandleInformation(s as HANDLE, HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        self.associate(s)?;
        let peer = from_storage(peer).ok_or_else(no_ip)?;
        Ok(Outcome::Accepted { sock, peer })
    }

    fn connect(&mut self, key: OpKey, sock: &Sock, peer: SocketAddr) -> Start<Sock> {
        let (addr, n) = sockaddr(peer);
        let connect_ex = self.ext().connect_ex;
        let s = sock.raw();
        self.issue(key, s, What::Connect, os_error, |over| {
            // SAFETY: `over` は `issue` が作った生きた箱で、番地は `&raw` で作る。行き先は箱の中に書くので、OS が
            // 読む間も生きている。送るデータはない
            let ok = unsafe {
                let to = &raw mut (*over).peer;
                to.write(addr);
                connect_ex(
                    s,
                    to.cast(),
                    n,
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    &raw mut (*over).ov,
                )
            };
            issued(ok == 0)
        })
    }
}

impl Backend for Iocp {
    type Sock = Sock;

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

    fn listen(&mut self, addr: SocketAddr) -> io::Result<(Sock, u16)> {
        // SO_REUSEADDR は使用中のポートを別のソケットに取らせ、SO_EXCLUSIVEADDRUSE は古い接続が TIME_WAIT にある間
        // 立ち直したサーバを AddrInUse にするので、どちらも立てない (docs/spec/vm.md の「時計と driver」)
        let sock = self.stream_socket(addr.is_ipv6())?;
        let s = sock.raw();
        bind_to(s, addr)?;
        // SAFETY: 引数はどれも整数である
        check(unsafe { listen(s, BACKLOG) })?;
        let port = name_of(s, false)?.port();
        nonblocking(s)?;
        self.associate(s)?;
        Ok((sock, port))
    }

    fn socket(&mut self, peer: SocketAddr) -> io::Result<Sock> {
        let sock = self.stream_socket(peer.is_ipv6())?;
        let s = sock.raw();
        // ConnectEx は bind したソケットを求める
        let any = if peer.is_ipv6() {
            SocketAddr::from(([0u16; 8], 0))
        } else {
            SocketAddr::from(([0u8; 4], 0))
        };
        bind_to(s, any)?;
        if peer.ip().is_loopback() {
            // 速さのためだけなので、失敗しても接続はできる
            let _ = no_syn_retransmissions(s);
        }
        nonblocking(s)?;
        self.associate(s)?;
        Ok(sock)
    }

    fn shutdown_write(&mut self, sock: &mut Sock) -> io::Result<()> {
        // SAFETY: 引数はどれも整数である
        check(unsafe { shutdown(sock.raw(), SD_SEND) })
    }

    fn set_nodelay(&mut self, sock: &mut Sock, on: bool) -> io::Result<()> {
        setsockopt_bytes(
            sock.raw(),
            IPPROTO_TCP,
            TCP_NODELAY,
            &i32::from(on).to_ne_bytes(),
        )
    }

    fn close(&mut self, sock: Sock) {
        // 核は保留の歩があるソケットを閉じない (docs/spec/vm.md の「時計と driver」)
        debug_assert!(
            self.boxes.values().all(|l| l.on != sock.raw()),
            "a socket is closed with a box the kernel holds"
        );
        drop(sock);
    }

    fn start(&mut self, key: OpKey, step: Step<'_, Sock>) -> Start<Sock> {
        match step {
            Step::Peek { sock } => {
                let mut b = 0u8;
                // SAFETY: `b` は局所変数で、長さは 1 である。OS が書くのは戻るまでの間だけである
                match unsafe { recv(sock.raw(), &mut b, 1, MSG_PEEK) } {
                    SOCKET_ERROR => match last() {
                        WSAEWOULDBLOCK => self.zero_read(key, sock),
                        code => Start::Done(Err(stream_error(code))),
                    },
                    _ => Start::Done(Ok(Outcome::Size(0))),
                }
            }
            Step::Recv { sock, buf } => {
                // SAFETY: `buf` は貸された記憶で、`cap` バイトまで書ける。OS が書くのは戻るまでの間だけである
                match unsafe { recv(sock.raw(), buf.ptr, len(buf.cap), 0) } {
                    SOCKET_ERROR => match last() {
                        WSAEWOULDBLOCK => self.zero_read(key, sock),
                        code => Start::Done(Err(stream_error(code))),
                    },
                    n => Start::Done(Ok(Outcome::Size(n as usize))),
                }
            }
            Step::Send { sock, buf } => {
                // SAFETY: `buf` は貸された記憶で、`cap` バイトまで読める。OS が読むのは戻るまでの間だけである
                match unsafe { send(sock.raw(), buf.ptr, len(buf.cap), 0) } {
                    SOCKET_ERROR => match last() {
                        WSAEWOULDBLOCK => self.send_ex(key, sock, buf),
                        code => Start::Done(Err(stream_error(code))),
                    },
                    n => Start::Done(Ok(Outcome::Size(n as usize))),
                }
            }
            Step::Accept { listener } => self.accept(key, listener),
            Step::Connect { sock, peer } => self.connect(key, sock, peer),
        }
    }

    fn cancel(&mut self, key: OpKey, _step: Step<'_, Sock>) -> io::Result<Cancel> {
        let Some(&addr) = self.waiters.get(&key) else {
            debug_assert!(false, "a cancel for a key with no box");
            return Ok(Cancel::Done);
        };
        let lent = self
            .boxes
            .get_mut(&addr)
            .expect("the two tables name the same boxes");
        let over = lent.over;
        // SAFETY: `over` は OS が持つ生きた箱で、番地は `&raw` で作り参照を作らない。OS は番地を操作の照合に使う
        let ok = unsafe { CancelIoEx(lent.on as HANDLE, &raw const (*over).ov) };
        // `ERROR_NOT_FOUND` は終わりがすでに列にあることを表し、packet は来る
        if ok == 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                return Err(e);
            }
        }
        lent.cancelled = true;
        Ok(Cancel::Pending)
    }

    fn wait(&mut self, timeout: Option<Duration>, events: &mut Vec<Event<Sock>>) -> io::Result<()> {
        // 保留の鍵ごとに、OS が持つ箱がちょうど1つある
        debug_assert_eq!(self.boxes.len(), self.waiters.len());
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
        // 迷った packet があっても束の残りの箱は取り戻し、確かめは束を読み終えてからにする。途中で panic すると、
        // 取り出し済みの packet の箱が表に残り、`Drop` が abort して panic の理由が見えなくなる
        let mut stray = None;
        for entry in &entries[..removed as usize] {
            match (entry.lpCompletionKey, entry.lpOverlapped.is_null()) {
                // packet を取り出してから下ろす。間に来た鳴らしの答えは、核がこの後で受け箱を引き取るときに拾う
                (WAKE, _) => self.waker.posted.store(false, Ordering::Release),
                (SOCKET_KEY, false) => {
                    // 中を読む前に、この backend が預けた箱かを確かめる。知らない番地は放さずに捨てる
                    let addr = entry.lpOverlapped.addr();
                    let Some(lent) = self.boxes.remove(&addr) else {
                        stray = Some("a packet names a box the backend does not hold");
                        continue;
                    };
                    let waiter = self.waiters.remove(&lent.key);
                    debug_assert_eq!(waiter, Some(addr), "the two tables name the same boxes");
                    // SAFETY: `lent.over` は `issue` が箱を預けたときのポインタで、番地は今取り出した packet の番地と
                    // `boxes` で照らし合わせた。packet を取り出したので OS はもう箱に書かず、表から外したので
                    // ほかに持ち主はいない
                    let over = unsafe { Box::from_raw(lent.over) };
                    let arrival = self.arrival(over, lent.cancelled);
                    events.push(Event {
                        key: lent.key,
                        arrival,
                    });
                }
                // OVERLAPPED なしの呼び出しは packet を積まないので、来れば放さずに捨てる
                _ => stray = Some("a packet the backend did not ask for"),
            }
        }
        if let Some(what) = stray {
            debug_assert!(false, "{what}");
        }
        Ok(())
    }
}

impl Drop for Iocp {
    fn drop(&mut self) {
        // OS が書きうる箱を放すと、解放した記憶に OS が書く。核は箱が残るうちは backend を落とさないので、ここに
        // 来るのは核の待ちが尽きたときだけである (docs/spec/vm.md の「時計と driver」)
        if !self.boxes.is_empty() {
            eprintln!("{BOX_ABORT}");
            process::abort();
        }
    }
}

/// backend を本物のソケットで確かめる。前半は、Winsock の振る舞いのうち Microsoft の文書に書かれていないのにこの
/// backend が頼るものを、OS を直に呼んで確かめる。後半は backend の関数を通して確かめる
/// (docs/implementation/testing.md の「テストの置き場所」)。どの待ちも 1 秒で切り、経過時間は確かめない
/// (docs/implementation/testing.md の「性能のテスト」)。
#[cfg(test)]
mod tests {
    use std::cell::UnsafeCell;
    use std::io::{Read, Write};
    use std::net::{IpAddr, Shutdown, TcpListener, TcpStream};

    use windows_sys::Win32::Foundation::GetHandleInformation;
    use windows_sys::Win32::Networking::WinSock::{
        LINGER, POLLRDNORM, SO_LINGER, WSAEADDRINUSE, WSAECONNABORTED, WSAECONNREFUSED,
        WSAECONNRESET, WSAENOBUFS, WSAESHUTDOWN, WSAPOLLFD, WSAPoll,
    };

    use super::*;

    /// ソケットを結び付けるときの completion key
    const SOCK: usize = 1;
    /// `Rig::only_marks` が積む印の packet の key
    const MARK: usize = 2;
    /// 待ちの上限
    const LIMIT: Duration = Duration::from_secs(1);
    /// `Rig` が貸せる OVERLAPPED の数
    const SLOTS: usize = 8;

    /// AcceptEx が自分と相手の番地を書く buffer
    type Addrs = [u8; 2 * ADDR_LEN];

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
            startup().expect("WSAStartup failed");
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

    /// ソケットの自分のポート
    fn port_of(s: SOCKET) -> u16 {
        name_of(s, false).unwrap().port()
    }

    /// ソケットの相手の番地
    fn peer_of(s: SOCKET) -> SocketAddr {
        name_of(s, true).unwrap()
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
        nonblocking(s).unwrap();
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
        let _: LPFN_CONNECTEX = load(s, WSAID_CONNECTEX).unwrap();
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
        let f: LPFN_ACCEPTEX = load(l, WSAID_ACCEPTEX).unwrap();
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
        let a = new_socket(false).unwrap();
        assert_eq!(
            accept_ex(&mut rig, 0, l, raw(&a), &mut addrs),
            Ok(WSA_IO_PENDING)
        );
        rig.only_marks();
        let c1 = TcpStream::connect(addr).unwrap();
        let p = rig.packet_of(0);
        assert_eq!(result(l, p.ov), Ok(0));
        update_accept(l, raw(&a));
        nonblocking(raw(&a)).unwrap();
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
        let b = new_socket(false).unwrap();
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
        let a = new_socket(false).unwrap();
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
        let s = new_socket(v6).unwrap();
        let any: SocketAddr = if v6 { "[::]:0" } else { "0.0.0.0:0" }.parse().unwrap();
        bind_to(raw(&s), any).unwrap();
        if rto {
            no_syn_retransmissions(raw(&s)).unwrap();
        }
        nonblocking(raw(&s)).unwrap();
        rig.associate(raw(&s));
        s
    }

    /// 欄 `i` で ConnectEx を出す
    fn connect_ex(rig: &mut Rig, i: usize, s: SOCKET, peer: SocketAddr) -> Result<i32, i32> {
        let f: LPFN_CONNECTEX = load(s, WSAID_CONNECTEX).unwrap();
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
        let s = new_socket(ip.is_ipv6()).unwrap();
        bind_to(raw(&s), SocketAddr::new(ip, 0)).unwrap();
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

    fn backend() -> Iocp {
        Iocp::new().unwrap()
    }

    /// 1 秒まで待って、鍵 `key` の知らせを取り出す
    fn arrival_of(b: &mut Iocp, key: OpKey) -> Arrival<Sock> {
        let start = Instant::now();
        let mut events = Vec::new();
        loop {
            let left = LIMIT
                .checked_sub(start.elapsed())
                .expect("no event arrived within 1 s");
            b.wait(Some(left), &mut events).unwrap();
            if let Some(i) = events.iter().position(|e| e.key == key) {
                return events.swap_remove(i).arrival;
            }
        }
    }

    fn accepted(r: io::Result<Outcome<Sock>>) -> Sock {
        match r {
            Ok(Outcome::Accepted { sock, .. }) => sock,
            Ok(_) => panic!("not an accept"),
            Err(e) => panic!("accept: {e}"),
        }
    }

    /// backend のリスナーと、つなぐ先の番地
    fn listening(b: &mut Iocp) -> (Sock, SocketAddr) {
        let (l, port) = b.listen("127.0.0.1:0".parse().unwrap()).unwrap();
        (l, SocketAddr::from(([127, 0, 0, 1], port)))
    }

    /// 来た接続を backend で1つ受ける。その場で受けても、AcceptEx で受けてもよい
    fn accept_one(b: &mut Iocp, key: OpKey, l: &mut Sock) -> Sock {
        match b.start(key, Step::Accept { listener: l }) {
            Start::Done(r) => accepted(r),
            Start::Pending(_) => match arrival_of(b, key) {
                Arrival::Done(r) => accepted(r),
                _ => panic!("the accept did not end"),
            },
        }
    }

    /// backend のリスナー、それが受けたストリーム、std でつないだ相手。相手の読みは 1 秒で切る
    fn served(b: &mut Iocp) -> (Sock, Sock, TcpStream) {
        let (mut l, addr) = listening(b);
        let client = TcpStream::connect(addr).unwrap();
        client.set_read_timeout(Some(LIMIT)).unwrap();
        let s = accept_one(b, 1, &mut l);
        (l, s, client)
    }

    /// リスナーに受けられる接続が来るまで、1 秒まで待つ
    fn wait_acceptable(l: SOCKET) {
        let mut fd = WSAPOLLFD {
            fd: l,
            events: POLLRDNORM,
            revents: 0,
        };
        // SAFETY: `fd` は局所変数で、数は 1 である。OS が書くのは戻るまでの間だけである
        let n = unsafe { WSAPoll(&mut fd, 1, 1000) };
        assert_eq!(n, 1, "no connection arrived within 1 s ({})", last());
    }

    fn inherits(s: SOCKET) -> bool {
        let mut flags = 0;
        // SAFETY: `s` は呼ぶ側が開いたまま持つソケットで、`flags` は局所変数である
        let ok = unsafe { GetHandleInformation(s as HANDLE, &mut flags) };
        assert_ne!(ok, 0, "{}", io::Error::last_os_error());
        flags & HANDLE_FLAG_INHERIT != 0
    }

    #[test]
    fn sockets_are_not_inherited() {
        let mut b = backend();
        let (mut l, addr) = listening(&mut b);
        let _c1 = TcpStream::connect(addr).unwrap();
        wait_acceptable(l.raw());
        let by_accept = match b.start(1, Step::Accept { listener: &mut l }) {
            Start::Done(r) => accepted(r),
            Start::Pending(_) => panic!("a queued connection was not accepted at once"),
        };
        assert!(matches!(
            b.start(2, Step::Accept { listener: &mut l }),
            Start::Pending(Hold::Memory)
        ));
        let _c2 = TcpStream::connect(addr).unwrap();
        let by_accept_ex = match arrival_of(&mut b, 2) {
            Arrival::Done(r) => accepted(r),
            _ => panic!("the AcceptEx did not end"),
        };
        let connecting = b.socket(addr).unwrap();
        for s in [&l, &by_accept, &by_accept_ex, &connecting] {
            assert!(!inherits(s.raw()));
        }
        for s in [l, by_accept, by_accept_ex, connecting] {
            b.close(s);
        }
    }

    #[test]
    fn a_cancelled_pending_probe_ends_cancelled() {
        let mut b = backend();
        let (l, mut s, _client) = served(&mut b);
        assert!(matches!(
            b.start(2, Step::Peek { sock: &mut s }),
            Start::Pending(Hold::Memory)
        ));
        assert!(matches!(
            b.cancel(2, Step::Peek { sock: &mut s }),
            Ok(Cancel::Pending)
        ));
        assert!(matches!(arrival_of(&mut b, 2), Arrival::Cancelled));
        b.close(s);
        b.close(l);
    }

    #[test]
    fn a_cancel_after_the_packet_is_queued_is_pending() {
        let mut b = backend();
        let (l, mut s, mut client) = served(&mut b);
        client.write_all(b"ab").unwrap();
        wait_readable(s.raw());
        assert!(matches!(b.zero_read(2, &s), Start::Pending(Hold::Memory)));
        assert!(matches!(
            b.cancel(2, Step::Peek { sock: &mut s }),
            Ok(Cancel::Pending)
        ));
        // 長さ 0 の読みが同期に終わっていれば取り消す相手はなく、`Retry` が届く。保留になっていれば取り消しが勝ちうる
        assert!(matches!(
            arrival_of(&mut b, 2),
            Arrival::Retry | Arrival::Cancelled
        ));
        b.close(s);
        b.close(l);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "a cancel for a key with no box")]
    fn a_cancel_without_a_box_is_a_contract_violation() {
        let mut b = backend();
        let (mut l, _) = listening(&mut b);
        let _ = b.cancel(1, Step::Accept { listener: &mut l });
    }

    #[test]
    fn an_overlapped_send_through_the_backend_ends_with_its_size() {
        // OS が読む記憶は backend より先に作り、backend が落ちるまで放さない
        let mut data = *b"abcd";
        let mut b = backend();
        let (l, s, mut client) = served(&mut b);
        let buf = RawBuf {
            ptr: data.as_mut_ptr(),
            cap: data.len(),
        };
        // 空のストリームへの書きは同期に終わるが、それでも packet で読む
        assert!(matches!(
            b.send_ex(2, &s, buf),
            Start::Pending(Hold::Memory)
        ));
        assert!(matches!(
            arrival_of(&mut b, 2),
            Arrival::Done(Ok(Outcome::Size(4)))
        ));
        let mut got = [0u8; 4];
        client.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"abcd");
        b.close(s);
        b.close(l);
    }

    #[test]
    fn a_cancelled_pending_write_ends_once() {
        // OS が読む記憶は backend より先に作り、backend が落ちるまで放さない
        let mut chunk = vec![7u8; 8 << 20];
        let mut b = backend();
        let (l, mut s, _client) = served(&mut b);
        let buf = RawBuf {
            ptr: chunk.as_mut_ptr(),
            cap: chunk.len(),
        };
        let mut key = 10;
        while !matches!(
            b.start(key, Step::Send { sock: &mut s, buf }),
            Start::Pending(Hold::Memory)
        ) {
            key += 1;
            assert!(key < 18, "8 sends did not go pending");
        }
        assert!(matches!(
            b.cancel(key, Step::Send { sock: &mut s, buf }),
            Ok(Cancel::Pending)
        ));
        assert!(matches!(
            arrival_of(&mut b, key),
            Arrival::Cancelled | Arrival::Done(Ok(Outcome::Size(_)))
        ));
        b.close(s);
        b.close(l);
    }

    #[test]
    fn a_cancelled_pending_accept_ends_cancelled() {
        let mut b = backend();
        let (mut l, _) = listening(&mut b);
        assert!(matches!(
            b.start(1, Step::Accept { listener: &mut l }),
            Start::Pending(Hold::Memory)
        ));
        assert!(matches!(
            b.cancel(1, Step::Accept { listener: &mut l }),
            Ok(Cancel::Pending)
        ));
        assert!(matches!(arrival_of(&mut b, 1), Arrival::Cancelled));
        b.close(l);
    }

    #[test]
    fn a_refused_loopback_connect_ends_refused() {
        for ip in ["127.0.0.1", "::1"] {
            let mut b = backend();
            let (_held, to) = refusing(ip);
            let mut s = b.socket(to).unwrap();
            let e = match b.start(
                1,
                Step::Connect {
                    sock: &mut s,
                    peer: to,
                },
            ) {
                Start::Done(r) => r.err().expect("a refused connect succeeded"),
                Start::Pending(_) => match arrival_of(&mut b, 1) {
                    Arrival::Done(Err(e)) => e,
                    _ => panic!("{ip}: the connect was not refused"),
                },
            };
            assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused, "{ip}: {e}");
            b.close(s);
        }
    }

    #[test]
    fn a_connect_ends_connected() {
        let mut b = backend();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let to = listener.local_addr().unwrap();
        let mut s = b.socket(to).unwrap();
        assert!(matches!(
            b.start(
                1,
                Step::Connect {
                    sock: &mut s,
                    peer: to
                }
            ),
            Start::Pending(Hold::Memory)
        ));
        assert!(matches!(
            arrival_of(&mut b, 1),
            Arrival::Done(Ok(Outcome::Connected))
        ));
        b.shutdown_write(&mut s).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server.set_read_timeout(Some(LIMIT)).unwrap();
        assert_eq!(server.read(&mut [0u8; 4]).unwrap(), 0);
        b.close(s);
    }

    #[test]
    fn an_abort_nobody_asked_for_is_other() {
        let mut b = backend();
        let (l, s, _client) = served(&mut b);
        assert!(matches!(b.zero_read(2, &s), Start::Pending(Hold::Memory)));
        let over = b.boxes[&b.waiters[&2]].over;
        // SAFETY: `over` は backend が OS に預けた生きた箱で、番地は `&raw` で作る。OS は番地を操作の照合に使う
        let ok = unsafe { CancelIoEx(s.raw() as HANDLE, &raw const (*over).ov) };
        assert_ne!(ok, 0, "{}", io::Error::last_os_error());
        match arrival_of(&mut b, 2) {
            Arrival::Done(Err(e)) => assert_eq!(e.kind(), io::ErrorKind::Other, "{e}"),
            _ => panic!("the abort was not an error"),
        }
        b.close(s);
        b.close(l);
    }

    #[test]
    fn os_errors_take_the_kinds_of_the_table() {
        use io::ErrorKind as K;
        assert_eq!(
            os_error(ERROR_CONNECTION_REFUSED).kind(),
            K::ConnectionRefused
        );
        assert_eq!(os_error(WSAECONNREFUSED).kind(), K::ConnectionRefused);
        assert_eq!(
            os_error(ERROR_CONNECTION_ABORTED).kind(),
            K::ConnectionAborted
        );
        assert_eq!(os_error(ERROR_NETNAME_DELETED).kind(), K::ConnectionReset);
        assert_eq!(os_error(WSAEADDRINUSE).kind(), K::AddrInUse);
        for code in [
            ERROR_NETNAME_DELETED,
            ERROR_CONNECTION_ABORTED,
            WSAECONNRESET,
            WSAECONNABORTED,
        ] {
            assert_eq!(stream_error(code).kind(), K::ConnectionReset, "{code}");
        }
        assert_eq!(stream_error(WSAESHUTDOWN).kind(), K::BrokenPipe);
        for code in [
            ERROR_NETNAME_DELETED,
            ERROR_CONNECTION_ABORTED,
            WSAECONNRESET,
            WSAECONNABORTED,
        ] {
            assert_eq!(accept_error(code).kind(), K::ConnectionAborted, "{code}");
        }
        let not_connected = io::Error::from(K::NotConnected);
        assert_eq!(gone(not_connected).kind(), K::ConnectionAborted);
        assert_eq!(accept_error(WSAENOBUFS).kind(), os_error(WSAENOBUFS).kind());
        assert_ne!(accept_error(WSAENOBUFS).kind(), K::ConnectionAborted);
    }
}

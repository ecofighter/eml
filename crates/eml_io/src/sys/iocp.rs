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

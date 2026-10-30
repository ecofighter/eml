//! 核と backend の境界 (docs/spec/vm.md の「時計と driver」)。核は続きの規則と操作の表を持ち、backend は OS の資源と
//! fd ごとの状態を持つ。backend は `start` 1回につき、その歩の1回の試みだけを OS に出す。
//!
//! 完了型の値 (`Hold::Buffer`、`Cancel::Pending`、`Arrival::Done` と `Arrival::Cancelled`) は暫定である。今それを返すのは
//! 核のテストの偽の backend だけで、最初の完了型の backend (IOCP のソケットか io_uring) を載せるときに形を変えてよい。

// Windows の backend はソケットを扱わないので、歩と結果の値を作らない。それを作るのは、そこでは核のテストの偽の
// backend だけである
#![cfg_attr(all(windows, not(test)), expect(dead_code))]

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_rt::{IoWaker, OpKey, RawBuf};

/// 1つの操作の1歩。核が出し、backend は OS にその歩の1回の試みを出す。
pub(crate) enum Step<'a, S> {
    /// `Readable` の1歩。読めるデータか EOF があれば `Size(0)` で終わる。
    Peek {
        sock: &'a mut S,
    },
    Recv {
        sock: &'a mut S,
        buf: RawBuf,
    },
    Send {
        sock: &'a mut S,
        buf: RawBuf,
    },
    Accept {
        listener: &'a mut S,
    },
    Connect {
        sock: &'a mut S,
        peer: SocketAddr,
    },
}

pub(crate) enum Outcome<S> {
    Size(usize),
    Accepted { sock: S, peer: SocketAddr },
    Connected,
}

/// `Pending` の間、カーネルが `Step` の buffer に触れうるか。`Buffer` は暫定で、最初の完了型の backend を載せるときに
/// 形を見直す。
pub(crate) enum Hold {
    None,
    // 今これを返すのは核のテストの偽の backend だけである
    #[cfg_attr(not(test), expect(dead_code))]
    Buffer,
}

pub(crate) enum Start<S> {
    Done(io::Result<Outcome<S>>),
    Pending(Hold),
}

/// `Done`: backend は鍵と待ち手を忘れ、OS も buffer に触れない。`Pending`: 終わりは `wait` の `Event` で届く。
/// `Pending` は暫定で、最初の完了型の backend を載せるときに形を見直す。
pub(crate) enum Cancel {
    Done,
    // 今これを返すのは核のテストの偽の backend だけである
    #[cfg_attr(not(test), expect(dead_code))]
    Pending,
}

/// `wait` が届ける知らせ。`Done` と `Cancelled` は暫定で、最初の完了型の backend を載せるときに形を見直す。
pub(crate) enum Arrival<S> {
    /// readiness: 同じ `Step` をもう1度 `start` せよ。余分に届いても害がない
    Retry,
    /// 完了型: OS がこの歩を終えた (暫定)
    #[cfg_attr(not(test), expect(dead_code))]
    Done(io::Result<Outcome<S>>),
    /// 完了型: 核が頼んだ取り消しで終わった (暫定)
    #[cfg_attr(not(test), expect(dead_code))]
    Cancelled,
}

pub(crate) struct Event<S> {
    pub key: OpKey,
    pub arrival: Arrival<S>,
}

/// OS の資源を持ち、核の歩を OS に出す。readiness の backend は `Arrival::Retry`、`Start::Pending(Hold::None)`、
/// `Cancel::Done` だけを使う。完了型の値 (`Hold::Buffer`、`Cancel::Pending`、`Arrival::Done` と
/// `Arrival::Cancelled`) は暫定で、最初の完了型の backend (IOCP のソケットか io_uring) を載せるときに形を変えてよい。
pub(crate) trait Backend: Sized {
    /// backend が所有する OS のハンドル。核はフィールドを読まず、写しを作らず、`close` 以外で落とさない
    type Sock;
    /// ソケットを扱うか。偽なら核はソケットの操作を `Unsupported` で終え、backend を呼ばない
    const SOCKETS: bool;

    fn now(&self) -> Duration;
    fn waker(&self) -> Arc<dyn IoWaker>;
    /// プールと解決器のスレッドの作り方。核がスレッドを作る道はこれだけで、本物の backend はシグナルを塞いで始める
    /// (docs/spec/vm.md の「上限と割り込み」)
    fn spawn(
        &self,
        builder: thread::Builder,
        body: Box<dyn FnOnce() + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<()>>;

    /// 作って登録まで済ませたリスナーと、待ち受けるポート。
    fn listen(&mut self, addr: SocketAddr) -> io::Result<(Self::Sock, u16)>;
    /// `peer` へ接続するための、作ってブロックしない設定にしたソケット。
    fn socket(&mut self, peer: SocketAddr) -> io::Result<Self::Sock>;
    fn shutdown_write(&mut self, sock: &mut Self::Sock) -> io::Result<()>;
    fn set_nodelay(&mut self, sock: &mut Self::Sock, on: bool) -> io::Result<()>;
    /// backend の表の行を放し、OS の資源を閉じる。
    fn close(&mut self, sock: Self::Sock);

    fn start(&mut self, key: OpKey, step: Step<'_, Self::Sock>) -> Start<Self::Sock>;
    fn cancel(&mut self, key: OpKey, step: Step<'_, Self::Sock>) -> io::Result<Cancel>;
    /// `timeout` まで、起こしか event まで待つ。時間切れ、起こし、割り込みは `Ok(())` で、`events` は空のまま。
    /// `Err` は backend が壊れたことを表す。
    fn wait(
        &mut self,
        timeout: Option<Duration>,
        events: &mut Vec<Event<Self::Sock>>,
    ) -> io::Result<()>;
}

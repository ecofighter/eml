//! 核と backend の境界 (docs/spec/vm.md の「時計と driver」)。核は続きの規則と操作の表を持ち、backend は OS の資源と
//! fd ごとの状態を持つ。backend は `start` 1回につき、その歩の1回の試みだけを OS に出す。

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_rt::{IoWaker, OpKey, RawBuf};

/// 1つの操作の1歩。核が出し、backend は OS にその歩の1回の試みを出す。
// Windows の backend はソケットを扱わないので、Windows ではテストのほかにこれを作る所がない
#[cfg_attr(all(windows, not(test)), expect(dead_code))]
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

// Windows の backend はソケットを扱わないので、Windows ではテストのほかにこれを作る所がない
#[cfg_attr(all(windows, not(test)), expect(dead_code))]
pub(crate) enum Outcome<S> {
    Size(usize),
    Accepted { sock: S, peer: SocketAddr },
    Connected,
}

/// `Pending` の間に OS がこちらの記憶に書きうるか。核は driver を落とすときだけ読む。
pub(crate) enum Hold {
    /// `start` が戻った後、OS はこちらの記憶に触れない。落とすときは待たずに忘れてよい。忘れた欄に遅れて届く
    /// event は backend が捨てる (readiness の backend は世代で捨てる)。
    None,
    /// 貸された buffer か backend の記憶 (IOCP の重なった操作の箱、AcceptEx の番地の buffer、ConnectEx の
    /// sockaddr) に、完了まで OS が書きうる。核は終わりの event を読むまで欄を放さず、落とすときは 1 秒まで待って
    /// から abort する。
    // 今これを返すのは核のテストの偽の backend だけである
    #[cfg_attr(not(test), expect(dead_code))]
    Memory,
}

// Windows の backend はソケットを扱わないので、Windows ではテストのほかにこれを作る所がない
#[cfg_attr(all(windows, not(test)), expect(dead_code))]
pub(crate) enum Start<S> {
    /// 歩はその場で終わった。この歩を名指す event は届かない。
    Done(io::Result<Outcome<S>>),
    /// 歩は OS に出た。`cancel` が `Done` を返さない限り、この鍵を名指す event がちょうど1つ届く。
    Pending(Hold),
}

pub(crate) enum Cancel {
    /// backend は鍵と待ち手を忘れ、OS はこちらの記憶に触れず、event は届かない。核はその場で `cancelled` の完了を
    /// 積む。readiness の backend はいつもこれを返す。完了型の backend がこれを返すのは鍵の操作が OS に残って
    /// いないときだけで、保留の鍵には起こらない (起これば契約の破れで、backend は debug_assert の上でこれを返す)。
    // Windows の backend はソケットを扱わないので、Windows ではテストのほかにこの値を作る所がない
    #[cfg_attr(all(windows, not(test)), expect(dead_code))]
    Done,
    /// OS に取り消しを頼んだか、終わりがすでに列にある。終わりは `wait` の `Event` で届く: `Cancelled`、取り消しが
    /// 間に合わなかった `Done(結果)`、または `Retry`。
    // 今これを返すのは核のテストの偽の backend だけである
    #[cfg_attr(not(test), expect(dead_code))]
    Pending,
}

/// `wait` が届ける知らせ。
pub(crate) enum Arrival<S> {
    /// 同じ `Step` をもう1度 `start` せよ。readiness の知らせと、IOCP の長さ 0 の `WSARecv` の成功の packet。
    /// 余分に届いても害がない。
    // Windows の backend はソケットを扱わないので、Windows ではテストのほかにこの値を作る所がない
    #[cfg_attr(all(windows, not(test)), expect(dead_code))]
    Retry,
    /// OS がこの歩を終えた結果。取り消しを頼んだ後に届くこともある。長さ 0 の `WSARecv` の誤りの packet もこれで届く。
    // 今これを返すのは核のテストの偽の backend だけである
    #[cfg_attr(not(test), expect(dead_code))]
    Done(io::Result<Outcome<S>>),
    /// 核が `cancel` で頼んだ取り消しで終わった。結果はない。`cancel` が `Pending` を返した鍵にだけ届く。
    // 今これを返すのは核のテストの偽の backend だけである
    #[cfg_attr(not(test), expect(dead_code))]
    Cancelled,
}

pub(crate) struct Event<S> {
    pub key: OpKey,
    pub arrival: Arrival<S>,
}

/// OS の資源を持ち、核の歩を OS に出す。readiness の backend は `Arrival::Retry`、`Start::Pending(Hold::None)`、
/// `Cancel::Done` だけを使い、完了型の backend はすべての値を使う。`wait` は event を積んだ後に `Err` を返さない。
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

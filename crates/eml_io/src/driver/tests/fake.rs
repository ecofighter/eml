//! 核のテストの偽の backend。鍵ごとの台本で `start` に答え、呼ばれた順を記録する。`RawBuf` への読み書きはカーネルの
//! 代わりに貸された記憶に触れるので、このファイルは `unsafe` を持つ (docs/implementation/testing.md の「核の Miri」)。
//! slice は作らず、生のポインタの写しだけで触れる。

#![allow(unsafe_code)]

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use eml_rt::{IoWaker, OpKey};

use crate::driver::backend::{Backend, Cancel, Event, Hold, Outcome, Start, Step};

/// 行番号だけを持つ。`Clone` も `Copy` もしないので、核がソケットを写せないことを型で守る。
#[derive(Debug, PartialEq, Eq)]
pub(super) struct FakeSock(pub u32);

/// `start` の1回分の台本。台本が空なら `Pending(Hold::None)` で答える。
pub(super) enum Reply {
    Pending(Hold),
    Done(io::Result<Outcome<FakeSock>>),
    /// `Recv` の buffer の先頭へ写して `Size(len)` で終わる。
    Fill(Vec<u8>),
    /// `Send` の buffer の先頭 n バイトを `sent` へ写して `Size(n)` で終わる。
    Take(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StepKind {
    Peek,
    Recv,
    Send,
    Accept,
    Connect(SocketAddr),
}

/// 呼ばれた順の記録。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Call {
    Listen(SocketAddr),
    Socket(SocketAddr),
    Close(u32),
    ShutdownWrite(u32),
    SetNodelay(u32, bool),
    Start(OpKey, StepKind),
    Cancel(OpKey, StepKind),
    Wait(Option<Duration>),
}

/// 偽の起こし。鳴らした印は次の `wait` が下ろすまで残る。
#[derive(Default)]
pub(super) struct Bell {
    rung: Mutex<bool>,
    cv: Condvar,
}

impl IoWaker for Bell {
    fn wake(&self) {
        *self.rung.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.cv.notify_all();
    }
}

impl Bell {
    /// 鳴るまで待って印を下ろす。鳴らなければ止まったテストとして落ちる。
    pub(super) fn wait(&self) {
        let rung = self.rung.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut rung, waited) = self
            .cv
            .wait_timeout_while(rung, Duration::from_secs(10), |rung| !*rung)
            .unwrap_or_else(PoisonError::into_inner);
        assert!(!waited.timed_out(), "the fake wait was never woken");
        *rung = false;
    }

    pub(super) fn clear(&self) {
        *self.rung.lock().unwrap_or_else(PoisonError::into_inner) = false;
    }
}

/// 終わった数とその知らせ。
#[derive(Default)]
pub(super) struct Counter {
    count: Mutex<usize>,
    cv: Condvar,
}

impl Counter {
    fn bump(&self) {
        *self.count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.cv.notify_all();
    }

    pub(super) fn get(&self) -> usize {
        *self.count.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 数が `n` 以上になるまで待つ。
    pub(super) fn reach(&self, n: usize) {
        let count = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        let (_guard, waited) = self
            .cv
            .wait_timeout_while(count, Duration::from_secs(10), |count| *count < n)
            .unwrap_or_else(PoisonError::into_inner);
        assert!(!waited.timed_out(), "the threads did not finish");
    }
}

pub(super) struct Fake<const SOCKETS: bool = true> {
    /// 偽の時計。`wait(Some(d))` でだけ進む。
    pub now: Duration,
    pub bell: Arc<Bell>,
    /// `spawn` した本体が終わった数。
    pub finished: Arc<Counter>,
    pub spawned: Cell<usize>,
    pub starts: HashMap<OpKey, VecDeque<Reply>>,
    /// 空なら `Ok(Cancel::Done)` で答える。
    pub cancels: VecDeque<io::Result<Cancel>>,
    /// 次の `wait` で全部を渡す。
    pub events: Vec<Event<FakeSock>>,
    pub wait_error: Option<io::Error>,
    pub spawn_error: bool,
    pub listen_port: u16,
    pub next_sock: u32,
    pub sent: Vec<u8>,
    pub calls: Vec<Call>,
    /// `close` されたソケット。driver を落とした後も読めるように分けて持つ。
    pub closed: Arc<Mutex<Vec<u32>>>,
    /// `cancel` された鍵。`closed` と同じく、driver を落とした後も読める。
    pub cancelled_keys: Arc<Mutex<Vec<OpKey>>>,
}

impl<const SOCKETS: bool> Fake<SOCKETS> {
    pub(super) fn new() -> Self {
        Fake {
            now: Duration::ZERO,
            bell: Arc::default(),
            finished: Arc::default(),
            spawned: Cell::new(0),
            starts: HashMap::new(),
            cancels: VecDeque::new(),
            events: Vec::new(),
            wait_error: None,
            spawn_error: false,
            listen_port: 4000,
            next_sock: 0,
            sent: Vec::new(),
            calls: Vec::new(),
            closed: Arc::default(),
            cancelled_keys: Arc::default(),
        }
    }

    /// `key` の台本の後ろに積む。
    pub(super) fn script(&mut self, key: OpKey, replies: impl IntoIterator<Item = Reply>) {
        self.starts.entry(key).or_default().extend(replies);
    }

    fn sock(&mut self) -> FakeSock {
        let sock = FakeSock(self.next_sock);
        self.next_sock += 1;
        sock
    }
}

fn kind_of(step: &Step<'_, FakeSock>) -> StepKind {
    match step {
        Step::Peek { .. } => StepKind::Peek,
        Step::Recv { .. } => StepKind::Recv,
        Step::Send { .. } => StepKind::Send,
        Step::Accept { .. } => StepKind::Accept,
        Step::Connect { peer, .. } => StepKind::Connect(*peer),
    }
}

impl<const SOCKETS: bool> Backend for Fake<SOCKETS> {
    type Sock = FakeSock;
    const SOCKETS: bool = SOCKETS;

    fn now(&self) -> Duration {
        self.now
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        Arc::clone(&self.bell) as Arc<dyn IoWaker>
    }

    fn spawn(
        &self,
        builder: thread::Builder,
        body: Box<dyn FnOnce() + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<()>> {
        if self.spawn_error {
            return Err(io::Error::other("no threads"));
        }
        self.spawned.set(self.spawned.get() + 1);
        let finished = Arc::clone(&self.finished);
        builder.spawn(move || {
            body();
            finished.bump();
        })
    }

    fn listen(&mut self, addr: SocketAddr) -> io::Result<(FakeSock, u16)> {
        self.calls.push(Call::Listen(addr));
        Ok((self.sock(), self.listen_port))
    }

    fn socket(&mut self, peer: SocketAddr) -> io::Result<FakeSock> {
        self.calls.push(Call::Socket(peer));
        Ok(self.sock())
    }

    fn shutdown_write(&mut self, sock: &mut FakeSock) -> io::Result<()> {
        self.calls.push(Call::ShutdownWrite(sock.0));
        Ok(())
    }

    fn set_nodelay(&mut self, sock: &mut FakeSock, on: bool) -> io::Result<()> {
        self.calls.push(Call::SetNodelay(sock.0, on));
        Ok(())
    }

    fn close(&mut self, sock: FakeSock) {
        self.calls.push(Call::Close(sock.0));
        self.closed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(sock.0);
    }

    fn start(&mut self, key: OpKey, step: Step<'_, FakeSock>) -> Start<FakeSock> {
        self.calls.push(Call::Start(key, kind_of(&step)));
        let reply = self.starts.get_mut(&key).and_then(VecDeque::pop_front);
        match reply {
            None => Start::Pending(Hold::None),
            Some(Reply::Pending(hold)) => Start::Pending(hold),
            Some(Reply::Done(result)) => Start::Done(result),
            Some(Reply::Fill(bytes)) => {
                let Step::Recv { buf, .. } = step else {
                    panic!("Fill answers only a Recv");
                };
                assert!(bytes.len() <= buf.cap, "Fill is longer than the buffer");
                // SAFETY: 貸した側は完了まで `buf.ptr[0..cap]` を生かし、一意に保つ (`RawBuf`)。写すのは cap 以下で、
                // `bytes` は別の確保なので重ならない
                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.ptr, bytes.len()) };
                Start::Done(Ok(Outcome::Size(bytes.len())))
            }
            Some(Reply::Take(n)) => {
                let Step::Send { buf, .. } = step else {
                    panic!("Take answers only a Send");
                };
                assert!(n <= buf.cap, "Take is longer than the buffer");
                let len = self.sent.len();
                self.sent.reserve(n);
                // SAFETY: 貸した側は完了まで `buf.ptr[0..cap]` を生かし、書き換えない (`RawBuf`)。`sent` は
                // `reserve` で `len + n` まで空きがあり、貸した記憶とは別の確保なので重ならない。写した後で長さを確定する
                unsafe {
                    std::ptr::copy_nonoverlapping(buf.ptr, self.sent.as_mut_ptr().add(len), n);
                    self.sent.set_len(len + n);
                }
                Start::Done(Ok(Outcome::Size(n)))
            }
        }
    }

    fn cancel(&mut self, key: OpKey, step: Step<'_, FakeSock>) -> io::Result<Cancel> {
        self.calls.push(Call::Cancel(key, kind_of(&step)));
        self.cancelled_keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(key);
        self.cancels.pop_front().unwrap_or(Ok(Cancel::Done))
    }

    fn wait(
        &mut self,
        timeout: Option<Duration>,
        events: &mut Vec<Event<FakeSock>>,
    ) -> io::Result<()> {
        self.calls.push(Call::Wait(timeout));
        if let Some(e) = self.wait_error.take() {
            return Err(e);
        }
        if !self.events.is_empty() {
            events.append(&mut self.events);
            return Ok(());
        }
        match timeout {
            None => self.bell.wait(),
            Some(d) => {
                // 本物のスレッドが走る間に偽の時計を進めると、合否がスケジュールに依る
                let running = self.spawned.get() - self.finished.get();
                assert!(
                    d.is_zero() || running == 0,
                    "a timed fake wait while a thread runs"
                );
                self.bell.clear();
                self.now += d;
            }
        }
        Ok(())
    }
}

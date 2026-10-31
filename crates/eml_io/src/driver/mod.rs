//! 本物の driver の核 (docs/spec/vm.md の「時計と driver」)。OS を呼ばず、操作の表と続きの規則 (短い書きの残り、
//! 中止された受けの出し直し、接続の次の番地、解決の答えからの接続) を持つ。OS に触れるのは `Backend` だけである。
//! 核のテストは偽の backend の上で Miri でも流す (docs/implementation/testing.md の「核の Miri」)。

pub(crate) mod backend;
mod inbox;
mod pool;
mod resolve;
#[cfg(test)]
mod tests;

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::mem;
use std::net::{IpAddr, SocketAddr};
use std::panic;
use std::process;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use eml_rt::{Completion, Done, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, RawBuf, Submit};

use backend::{Arrival, Backend, Cancel, Event, Hold, Outcome, Start, Step};
use inbox::{Answer, Inbox, Ticket};
use pool::{POOL_LIMIT, Pool};
pub(crate) use resolve::Lookup;
use resolve::Resolver;

/// `quiesce` が、OS に出した取り消しの完了を待つ長さの上限。過ぎたら完了は失われたとみなし、壊れた状態にする。
const CANCEL_LIMIT: Duration = Duration::from_secs(1);

/// driver を落とすときに、OS がこちらの記憶に書きうる操作の終わりを待つ長さの上限。過ぎたら、その記憶を放せないので
/// プロセスを止める。
const HELD_LIMIT: Duration = Duration::from_secs(1);

/// 落とすときに OS がこちらの記憶に書きうる操作が終わらなかったときの文言。
pub(super) const HELD_ABORT: &str =
    "eml_io: the kernel may still write memory of a dropped I/O driver; aborting";

/// 保留の操作の向き。1つのハンドルの1つの向きには保留を1つしか置かない。std のストリームとリスナーは `Lin` で、VM は
/// 1つのハンドルに同時に2つの操作を出さないので、backend の待ち手は向きごとに1つで済む。
#[derive(Clone, Copy)]
enum Dir {
    Read,
    Write,
}

struct Broken {
    kind: io::ErrorKind,
    msg: String,
}

impl Broken {
    fn error(&self) -> io::Error {
        io::Error::new(self.kind, self.msg.clone())
    }
}

/// driver が持つソケット。保留の操作がある間は、`close` されても閉じずに残す。
struct Handle<S> {
    sock: S,
    reading: bool,
    writing: bool,
    closed: bool,
}

struct Slot<S> {
    kind: Kind<S>,
    /// 取り消しを頼んだ。続きを出さない。
    cancelled: bool,
    hold: Hold,
    /// OS に取り消しを出して終わりを待っている。出した時刻である。
    cancel_sent: Option<Duration>,
}

enum Kind<S> {
    Readable {
        handle: IoHandle,
    },
    Read {
        handle: IoHandle,
        buf: RawBuf,
    },
    Write {
        handle: IoHandle,
        buf: RawBuf,
        written: usize,
    },
    Accept {
        listener: IoHandle,
    },
    Connect {
        sock: S,
        peer: SocketAddr,
        rest: std::vec::IntoIter<SocketAddr>,
    },
    Lookup {
        ticket: Ticket,
    },
    /// 仕事の答えは札で欄に当たり、仕事は取り消せないので、欄は札を持たない。
    Job,
}

impl<S> Kind<S> {
    /// 操作が占めるハンドルの向き。
    fn owner(&self) -> Option<(IoHandle, Dir)> {
        match *self {
            Kind::Readable { handle } | Kind::Read { handle, .. } => Some((handle, Dir::Read)),
            Kind::Accept { listener } => Some((listener, Dir::Read)),
            Kind::Write { handle, .. } => Some((handle, Dir::Write)),
            Kind::Connect { .. } | Kind::Lookup { .. } | Kind::Job => None,
        }
    }
}

/// 1つの結果に続きの規則を当てた後。
enum Flow<S> {
    /// 同じ鍵の下で次の歩を出す。
    Again(Kind<S>),
    /// 終わった。2つ目は `Completion::cancelled` である。
    Finish(io::Result<Done>, bool),
}

/// 歩を出した後。
enum Next<S> {
    Wait(Kind<S>, Hold),
    Done(io::Result<Done>, bool),
}

pub(crate) struct Driver<B: Backend> {
    backend: B,
    /// 保留の操作。作り直さずに使い回し、`turn` は event と札が名指した欄にだけ触れる。
    slots: HashMap<OpKey, Slot<B::Sock>>,
    done: Vec<Completion>,
    handles: HashMap<IoHandle, Handle<B::Sock>>,
    next_handle: IoHandle,
    tickets: HashMap<Ticket, OpKey>,
    next_ticket: Ticket,
    inbox: Arc<Inbox>,
    arrived: VecDeque<(Ticket, Answer)>,
    events: Vec<Event<B::Sock>>,
    /// `HandleBin` から引き取ったハンドル。使い回して、引き取りのたびに確保しない。
    binned: Vec<IoHandle>,
    pool: Pool,
    resolver: Resolver,
    bin: Arc<HandleBin>,
    /// 仕事の panic の payload。答えの束を処理しきってから、持ち主のスレッドで起こし直す。
    panic: Option<Box<dyn Any + Send>>,
    broken: Option<Broken>,
}

fn cancelled_error() -> io::Error {
    io::Error::other("cancelled")
}

fn no_handle() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no such handle")
}

fn busy() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "the handle has a pending op in this direction",
    )
}

/// `kind` の歩。ハンドルを引く操作の欄は、ハンドルの向きを占めている間ハンドルを表に残す。
fn step_of<'a, S>(
    kind: &'a mut Kind<S>,
    handles: &'a mut HashMap<IoHandle, Handle<S>>,
) -> Step<'a, S> {
    fn sock<S>(handles: &mut HashMap<IoHandle, Handle<S>>, handle: IoHandle) -> &mut S {
        &mut handles
            .get_mut(&handle)
            .expect("a pending op keeps its handle")
            .sock
    }
    match kind {
        Kind::Readable { handle } => Step::Peek {
            sock: sock(handles, *handle),
        },
        Kind::Read { handle, buf } => Step::Recv {
            sock: sock(handles, *handle),
            buf: *buf,
        },
        Kind::Write {
            handle,
            buf,
            written,
        } => Step::Send {
            sock: sock(handles, *handle),
            // 残りの位置は貸した記憶の中にあり、指すだけで読み書きしない (docs/spec/vm.md の「時計と driver」)
            buf: RawBuf {
                ptr: buf.ptr.wrapping_add(*written),
                cap: buf.cap - *written,
            },
        },
        Kind::Accept { listener } => Step::Accept {
            listener: sock(handles, *listener),
        },
        Kind::Connect { sock, peer, .. } => Step::Connect { sock, peer: *peer },
        Kind::Lookup { .. } | Kind::Job => unreachable!("a thread's op has no step"),
    }
}

impl<B: Backend> Driver<B> {
    pub(crate) fn new(backend: B, lookup: Lookup) -> Driver<B> {
        let inbox = Inbox::new(backend.waker());
        Driver {
            pool: Pool::new(Arc::clone(&inbox), POOL_LIMIT),
            resolver: Resolver::new(lookup, Arc::clone(&inbox)),
            backend,
            slots: HashMap::new(),
            done: Vec::new(),
            handles: HashMap::new(),
            next_handle: 0,
            tickets: HashMap::new(),
            next_ticket: 1,
            inbox,
            arrived: VecDeque::new(),
            events: Vec::new(),
            binned: Vec::new(),
            bin: Arc::default(),
            panic: None,
            broken: None,
        }
    }

    /// 落とされたハンドルを閉じる。
    fn empty_bin(&mut self) {
        self.binned.extend(
            self.bin
                .dropped
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..),
        );
        while let Some(handle) = self.binned.pop() {
            self.close(handle);
        }
    }

    fn ticket(&mut self, key: OpKey) -> Ticket {
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.tickets.insert(ticket, key);
        ticket
    }

    fn adopt(&mut self, sock: B::Sock) -> IoHandle {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.handles.insert(
            handle,
            Handle {
                sock,
                reading: false,
                writing: false,
                closed: false,
            },
        );
        handle
    }

    /// `handle` の `dir` の向きを占める。
    fn claim(&mut self, handle: IoHandle, dir: Dir) -> io::Result<()> {
        let entry = match self.handles.get_mut(&handle) {
            Some(entry) if !entry.closed => entry,
            _ => return Err(no_handle()),
        };
        let flag = match dir {
            Dir::Read => &mut entry.reading,
            Dir::Write => &mut entry.writing,
        };
        if *flag {
            return Err(busy());
        }
        *flag = true;
        Ok(())
    }

    /// 終わった操作の向きを放す。閉じたハンドルは、最後の操作が終わったここで閉じる。
    fn release(&mut self, owner: Option<(IoHandle, Dir)>) {
        let Some((handle, dir)) = owner else {
            return;
        };
        let Some(entry) = self.handles.get_mut(&handle) else {
            return;
        };
        match dir {
            Dir::Read => entry.reading = false,
            Dir::Write => entry.writing = false,
        }
        if entry.closed
            && !entry.reading
            && !entry.writing
            && let Some(entry) = self.handles.remove(&handle)
        {
            self.backend.close(entry.sock);
        }
    }

    /// 欄の持つソケットを閉じる。
    fn drop_kind(&mut self, kind: Kind<B::Sock>) {
        if let Kind::Connect { sock, .. } = kind {
            self.backend.close(sock);
        }
    }

    /// 1つの結果に続きの規則を当てる。取り消しを頼んだ操作は続きを出さず、`cancelled` で終える。
    fn settle(
        &mut self,
        kind: Kind<B::Sock>,
        result: io::Result<Outcome<B::Sock>>,
        cancelled: bool,
    ) -> Flow<B::Sock> {
        let stop = || Flow::Finish(Err(cancelled_error()), true);
        match (kind, result) {
            (Kind::Readable { .. }, Ok(Outcome::Size(_))) => Flow::Finish(Ok(Done::Size(0)), false),
            (Kind::Read { .. }, Ok(Outcome::Size(n))) => Flow::Finish(Ok(Done::Size(n)), false),
            (
                Kind::Write {
                    handle,
                    buf,
                    written,
                },
                Ok(Outcome::Size(n)),
            ) => {
                let written = written + n;
                if written >= buf.cap {
                    Flow::Finish(Ok(Done::Size(buf.cap)), false)
                } else if n == 0 {
                    // 何も書けない送りを出し直すと終わらない
                    Flow::Finish(Err(io::Error::from(io::ErrorKind::WriteZero)), false)
                } else if cancelled {
                    stop()
                } else {
                    Flow::Again(Kind::Write {
                        handle,
                        buf,
                        written,
                    })
                }
            }
            (Kind::Accept { .. }, Ok(Outcome::Accepted { sock, peer })) => {
                let handle = self.adopt(sock);
                Flow::Finish(Ok(Done::Stream { handle, peer }), false)
            }
            // 相手が受ける前に捨てた接続。次の接続を待つ
            (Kind::Accept { listener }, Err(e)) if e.kind() == io::ErrorKind::ConnectionAborted => {
                if cancelled {
                    stop()
                } else {
                    Flow::Again(Kind::Accept { listener })
                }
            }
            (Kind::Connect { sock, peer, .. }, Ok(Outcome::Connected)) => {
                let handle = self.adopt(sock);
                Flow::Finish(Ok(Done::Stream { handle, peer }), false)
            }
            (Kind::Connect { sock, rest, .. }, Err(e)) => {
                self.backend.close(sock);
                if cancelled {
                    stop()
                } else {
                    self.next_address(rest, e)
                }
            }
            (kind, Err(e)) => {
                self.drop_kind(kind);
                Flow::Finish(Err(e), false)
            }
            (kind, Ok(outcome)) => {
                self.drop_kind(kind);
                if let Outcome::Accepted { sock, .. } = outcome {
                    self.backend.close(sock);
                }
                panic!("the I/O backend answered a step with an outcome of another step");
            }
        }
    }

    /// 次の番地への接続の歩を作る。番地がなくなれば、最後の誤りで終わる。
    fn next_address(
        &mut self,
        mut rest: std::vec::IntoIter<SocketAddr>,
        mut last: io::Error,
    ) -> Flow<B::Sock> {
        while let Some(peer) = rest.next() {
            match self.backend.socket(peer) {
                Ok(sock) => return Flow::Again(Kind::Connect { sock, peer, rest }),
                Err(e) => last = e,
            }
        }
        Flow::Finish(Err(last), false)
    }

    /// `kind` の歩を出し、その場で終わった結果に続きの規則を当てて、待つか終わるまで進める。
    fn advance(&mut self, key: OpKey, mut kind: Kind<B::Sock>) -> Next<B::Sock> {
        loop {
            let start = self
                .backend
                .start(key, step_of(&mut kind, &mut self.handles));
            match start {
                Start::Pending(hold) => return Next::Wait(kind, hold),
                Start::Done(result) => match self.settle(kind, result, false) {
                    Flow::Again(next) => kind = next,
                    Flow::Finish(result, cancelled) => return Next::Done(result, cancelled),
                },
            }
        }
    }

    fn proceed(&mut self, key: OpKey, flow: Flow<B::Sock>) -> Next<B::Sock> {
        match flow {
            Flow::Again(kind) => self.advance(key, kind),
            Flow::Finish(result, cancelled) => Next::Done(result, cancelled),
        }
    }

    /// 待つ操作を表に置き、終わった操作を `done` に積む。
    fn place(&mut self, key: OpKey, owner: Option<(IoHandle, Dir)>, next: Next<B::Sock>) {
        match next {
            Next::Wait(kind, hold) => {
                self.slots.insert(
                    key,
                    Slot {
                        kind,
                        cancelled: false,
                        hold,
                        cancel_sent: None,
                    },
                );
            }
            Next::Done(result, cancelled) => {
                self.release(owner);
                self.done.push(Completion {
                    key,
                    result,
                    cancelled,
                });
            }
        }
    }

    /// 受け付けで歩を出す。終わればその場で返す。
    fn begin(&mut self, key: OpKey, owner: Option<(IoHandle, Dir)>, flow: Flow<B::Sock>) -> Submit {
        match self.proceed(key, flow) {
            Next::Done(result, _) => {
                self.release(owner);
                Submit::Done(result)
            }
            next @ Next::Wait(..) => {
                self.place(key, owner, next);
                Submit::Pending
            }
        }
    }

    /// backend の event を、名指しされた欄に当てる。欄がなければ捨てる。
    fn arrive(&mut self, event: Event<B::Sock>) {
        let Event { key, arrival } = event;
        let Some(slot) = self.slots.remove(&key) else {
            if let Arrival::Done(Ok(Outcome::Accepted { sock, .. })) = arrival {
                self.backend.close(sock);
            }
            return;
        };
        if matches!(slot.kind, Kind::Lookup { .. } | Kind::Job) {
            // スレッドの操作は札で終わる。使い回した鍵に古い event が来ても触れない
            self.slots.insert(key, slot);
            if let Arrival::Done(Ok(Outcome::Accepted { sock, .. })) = arrival {
                self.backend.close(sock);
            }
            return;
        }
        let owner = slot.kind.owner();
        let next = match arrival {
            Arrival::Retry if !slot.cancelled => self.advance(key, slot.kind),
            Arrival::Retry => {
                self.drop_kind(slot.kind);
                Next::Done(Err(cancelled_error()), true)
            }
            Arrival::Cancelled => {
                debug_assert!(
                    slot.cancelled,
                    "the backend reported a cancel the core did not ask for"
                );
                self.drop_kind(slot.kind);
                Next::Done(Err(cancelled_error()), true)
            }
            Arrival::Done(result) => {
                let flow = self.settle(slot.kind, result, slot.cancelled);
                self.proceed(key, flow)
            }
        };
        self.place(key, owner, next);
    }

    /// プールと解決器の答えを引き取り、札で欄に当てる。外した札の答えは捨てる。
    fn receive(&mut self) {
        // 引き取りの中で解決器が次のスレッドを始められなければ、その答えは鳴らさずに受け箱に積まれる。受け箱が空に
        // なるまで引き取り直すので、鳴らない答えを残したまま眠らない
        loop {
            self.inbox.take(&mut self.arrived);
            if self.arrived.is_empty() {
                break;
            }
            self.answer_arrived();
        }
    }

    /// 引き取った答えの束を処理しきる。
    fn answer_arrived(&mut self) {
        while let Some((ticket, answer)) = self.arrived.pop_front() {
            if let Answer::Lookup(_) = &answer {
                // 札を引く前に、答えたスレッドを join する。手放した解決のスレッドもここで数えから外れる
                self.resolver.answered(&self.backend, ticket);
            }
            let Some(key) = self.tickets.remove(&ticket) else {
                continue;
            };
            self.slots.remove(&key);
            match answer {
                Answer::Job(Ok(result)) => self.done.push(Completion {
                    key,
                    result: result.map(Done::Size),
                    cancelled: false,
                }),
                // 束の残りの答えを失わないよう、起こし直すのは処理しきった後である
                Answer::Job(Err(payload)) => {
                    self.panic.get_or_insert(payload);
                }
                Answer::Lookup(Err(e)) => self.done.push(Completion {
                    key,
                    result: Err(e),
                    cancelled: false,
                }),
                Answer::Lookup(Ok(addrs)) => {
                    let flow = self.next_address(addrs.into_iter(), no_address());
                    let next = self.proceed(key, flow);
                    self.place(key, None, next);
                }
            }
        }
    }

    /// 1回分の待ち。`wait`、`quiesce`、`Drop` が共有する。
    fn turn(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.receive();
        let timeout = if self.done.is_empty() {
            timeout
        } else {
            Some(Duration::ZERO)
        };
        debug_assert!(self.events.is_empty());
        if let Err(e) = self.backend.wait(timeout, &mut self.events) {
            self.broken = Some(Broken {
                kind: e.kind(),
                msg: e.to_string(),
            });
            // backend は event を積んだ後に誤りを返さない。破られたときも、受けたソケットだけは閉じる
            debug_assert!(
                self.events.is_empty(),
                "the backend failed a wait after queueing events"
            );
            while let Some(event) = self.events.pop() {
                if let Arrival::Done(Ok(Outcome::Accepted { sock, .. })) = event.arrival {
                    self.backend.close(sock);
                }
            }
            return Err(e);
        }
        // 1つの束の中の順は意味を持たない
        while let Some(event) = self.events.pop() {
            self.arrive(event);
        }
        self.receive();
        Ok(())
    }

    /// 捨てる完了の中身のハンドルを閉じる。
    fn discard_done(&mut self) {
        while let Some(completion) = self.done.pop() {
            if let Ok(Done::Listener { handle, .. } | Done::Stream { handle, .. }) =
                completion.result
            {
                self.close(handle);
            }
        }
    }

    /// 持ち主のスレッドで、仕事の panic を起こし直す。
    fn raise(&mut self) {
        if let Some(payload) = self.panic.take() {
            panic::resume_unwind(payload);
        }
    }

    /// ソケットを引く操作を受け付ける。
    fn submit_io(&mut self, key: OpKey, handle: IoHandle, dir: Dir, kind: Kind<B::Sock>) -> Submit {
        match self.claim(handle, dir) {
            Ok(()) => self.begin(key, Some((handle, dir)), Flow::Again(kind)),
            Err(e) => Submit::Done(Err(e)),
        }
    }
}

fn abort() -> ! {
    eprintln!("{HELD_ABORT}");
    process::abort();
}

fn no_address() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "no address")
}

impl<B: Backend> IoDriver for Driver<B> {
    fn now(&self) -> Duration {
        self.backend.now()
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.empty_bin();
        if let Some(broken) = &self.broken {
            return Err(broken.error());
        }
        // `Ok(false)` の確かめを `u64::MAX` の写しより先に置くので、`Some(u64::MAX)` は保留がなくても起こしを待つ
        let timeout = match deadline_ms {
            None if self.slots.is_empty() && self.done.is_empty() => return Ok(false),
            None | Some(u64::MAX) => None,
            Some(ms) => Some(Duration::from_millis(ms).saturating_sub(self.backend.now())),
        };
        let turned = self.turn(timeout);
        self.raise();
        turned.map(|()| true)
    }

    fn submit(&mut self, key: OpKey, op: Op) -> Submit {
        debug_assert!(
            !self.slots.contains_key(&key) && self.done.iter().all(|c| c.key != key),
            "a pending key is submitted again"
        );
        // 受け付けで終わる操作が続くと `wait` も `completions` も呼ばれないので、落とされたハンドルはここでも閉じる
        self.empty_bin();
        if let Some(broken) = &self.broken {
            // 壊れた後は OS の資源も仕事も新しく作らない
            return Submit::Done(Err(broken.error()));
        }
        match op {
            Op::Readable { handle } => {
                self.submit_io(key, handle, Dir::Read, Kind::Readable { handle })
            }
            Op::Read { handle, buf } => {
                self.submit_io(key, handle, Dir::Read, Kind::Read { handle, buf })
            }
            Op::Write { handle, buf } => self.submit_io(
                key,
                handle,
                Dir::Write,
                Kind::Write {
                    handle,
                    buf,
                    written: 0,
                },
            ),
            Op::Accept { listener } => {
                self.submit_io(key, listener, Dir::Read, Kind::Accept { listener })
            }
            Op::Listen { addr } => match self.backend.listen(addr) {
                Ok((sock, port)) => {
                    let handle = self.adopt(sock);
                    Submit::Done(Ok(Done::Listener { handle, port }))
                }
                Err(e) => Submit::Done(Err(e)),
            },
            Op::Connect { host, port } => match host.parse::<IpAddr>() {
                Ok(ip) => {
                    let addrs = vec![SocketAddr::new(ip, port)];
                    let flow = self.next_address(addrs.into_iter(), no_address());
                    self.begin(key, None, flow)
                }
                Err(_) => {
                    let ticket = self.ticket(key);
                    match self.resolver.submit(&self.backend, ticket, host, port) {
                        Ok(()) => {
                            self.slots.insert(
                                key,
                                Slot {
                                    kind: Kind::Lookup { ticket },
                                    cancelled: false,
                                    hold: Hold::None,
                                    cancel_sent: None,
                                },
                            );
                            Submit::Pending
                        }
                        Err(e) => {
                            self.tickets.remove(&ticket);
                            Submit::Done(Err(e))
                        }
                    }
                }
            },
            Op::Job(job) => {
                let ticket = self.ticket(key);
                self.slots.insert(
                    key,
                    Slot {
                        kind: Kind::Job,
                        cancelled: false,
                        hold: Hold::None,
                        cancel_sent: None,
                    },
                );
                self.pool.submit(&self.backend, ticket, job);
                Submit::Pending
            }
        }
    }

    fn cancel(&mut self, key: OpKey) {
        let broken = self.broken.is_some();
        let Some(slot) = self.slots.get_mut(&key) else {
            return;
        };
        if slot.cancelled || matches!(slot.kind, Kind::Job) {
            return;
        }
        if broken {
            // 壊れた OS の資源には触れない
            slot.cancelled = true;
            return;
        }
        if let Kind::Lookup { ticket } = slot.kind {
            // 解決は結果を待たずに手放す。札を外すので、遅れた答えは使い回した鍵に当たらない
            self.slots.remove(&key);
            self.tickets.remove(&ticket);
            self.resolver.cancel(&self.backend, ticket);
            self.done.push(Completion {
                key,
                result: Err(cancelled_error()),
                cancelled: true,
            });
            return;
        }
        let step = step_of(&mut slot.kind, &mut self.handles);
        match self.backend.cancel(key, step) {
            Ok(Cancel::Done) => {
                let slot = self.slots.remove(&key).expect("the slot is above");
                let owner = slot.kind.owner();
                self.drop_kind(slot.kind);
                self.place(key, owner, Next::Done(Err(cancelled_error()), true));
            }
            Ok(Cancel::Pending) => {
                let now = self.backend.now();
                let slot = self.slots.get_mut(&key).expect("the slot is above");
                slot.cancelled = true;
                slot.cancel_sent = Some(now);
            }
            Err(e) => {
                if let Some(slot) = self.slots.get_mut(&key) {
                    slot.cancelled = true;
                }
                self.broken = Some(Broken {
                    kind: e.kind(),
                    msg: e.to_string(),
                });
            }
        }
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
        self.slots.len() + self.done.len()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        self.backend.waker()
    }

    fn bin(&self) -> Arc<HandleBin> {
        Arc::clone(&self.bin)
    }

    fn shutdown_write(&mut self, handle: IoHandle) -> io::Result<()> {
        match self.handles.get_mut(&handle) {
            Some(entry) if !entry.closed => self.backend.shutdown_write(&mut entry.sock),
            _ => Err(no_handle()),
        }
    }

    fn set_nodelay(&mut self, handle: IoHandle, on: bool) -> io::Result<()> {
        match self.handles.get_mut(&handle) {
            Some(entry) if !entry.closed => self.backend.set_nodelay(&mut entry.sock, on),
            _ => Err(no_handle()),
        }
    }

    fn close(&mut self, handle: IoHandle) {
        let Some(entry) = self.handles.get_mut(&handle) else {
            return;
        };
        if entry.reading || entry.writing {
            // 保留の操作は終わるまで OS の資源を生かす。最後の操作が終わったときに閉じる
            entry.closed = true;
        } else if let Some(entry) = self.handles.remove(&handle) {
            self.backend.close(entry.sock);
        }
    }

    fn quiesce(&mut self) {
        self.empty_bin();
        self.discard_done();
        if self.broken.is_some() {
            return;
        }
        // 名前の解決は待たずに手放す。札を外すので、遅れた答えは捨てる
        self.resolver.abandon_all();
        let tickets = &mut self.tickets;
        self.slots.retain(|_, slot| match slot.kind {
            Kind::Lookup { ticket } => {
                tickets.remove(&ticket);
                false
            }
            _ => true,
        });
        let keys: Vec<OpKey> = self.slots.keys().copied().collect();
        for key in keys {
            self.cancel(key);
        }
        // 仕事は取り消せないので待ちきる。OS に出した取り消しは `CANCEL_LIMIT` まで待つ
        loop {
            self.discard_done();
            if self.broken.is_some() || self.slots.is_empty() {
                break;
            }
            let oldest = self
                .slots
                .values()
                .filter_map(|slot| slot.cancel_sent)
                .min();
            let timeout = match oldest {
                Some(sent) => {
                    let left = (sent + CANCEL_LIMIT).saturating_sub(self.backend.now());
                    if left.is_zero() {
                        self.broken = Some(Broken {
                            kind: io::ErrorKind::Other,
                            msg: "a cancelled op did not complete".to_string(),
                        });
                        break;
                    }
                    Some(left)
                }
                None => None,
            };
            if self.turn(timeout).is_err() {
                break;
            }
        }
        self.discard_done();
        self.raise();
    }
}

impl<B: Backend> Drop for Driver<B> {
    /// 仕事と解決を待たない。OS がこちらの記憶に書きうる操作だけ `HELD_LIMIT` まで待ち、終わらないか待ちが失敗すれば
    /// プロセスを止める。その記憶を放せないまま戻らないためである。壊れた driver も同じ道で落とす
    /// (docs/spec/vm.md の「時計と driver」)。
    fn drop(&mut self) {
        self.empty_bin();
        drop(self.panic.take());
        // 札を外すので、仕事と解決の答えは引き取っても捨てる。解決の列も空にするので、引き取りが新しい解決の
        // スレッドを始めない
        self.tickets.clear();
        self.resolver.abandon_all();
        // 渡していない完了が残ると、待ちの `turn` が待たずに回り続ける
        self.discard_done();
        let backend = &mut self.backend;
        for (_, slot) in self
            .slots
            .extract_if(|_, slot| !matches!(slot.hold, Hold::Memory))
        {
            if let Kind::Connect { sock, .. } = slot.kind {
                backend.close(sock);
            }
        }
        if !self.slots.is_empty() {
            // 印だけ立った欄 (壊れた後の `cancel` と、断られた取り消し) には、OS の取り消しがまだ出ていない
            let keys: Vec<OpKey> = self
                .slots
                .iter()
                .filter(|(_, slot)| slot.cancel_sent.is_none())
                .map(|(key, _)| *key)
                .collect();
            for key in keys {
                let slot = self.slots.get_mut(&key).expect("the key is listed above");
                let step = step_of(&mut slot.kind, &mut self.handles);
                // 取り消しを断られても、届く終わりを待つ
                match self.backend.cancel(key, step) {
                    Ok(Cancel::Pending) | Err(_) => slot.cancelled = true,
                    Ok(Cancel::Done) => {
                        let slot = self.slots.remove(&key).expect("the key is listed above");
                        self.drop_kind(slot.kind);
                    }
                }
            }
            let started = self.backend.now();
            while !self.slots.is_empty() {
                let left = HELD_LIMIT.saturating_sub(self.backend.now().saturating_sub(started));
                if left.is_zero() || self.turn(Some(left)).is_err() {
                    abort();
                }
                self.discard_done();
            }
        }
        self.discard_done();
        let handles = mem::take(&mut self.handles);
        for (_, entry) in handles {
            self.backend.close(entry.sock);
        }
    }
}

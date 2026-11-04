//! テストの driver。仮想の時計と仮想のネットワークで動き、操作を決まった時点でだけ決着させるので、結果は
//! プログラムと方針と seed で決まる。受け付けの時点で決まる操作は `submit` の中で終え、ほかの操作は `wait` の
//! 始めにだけ決着させる (docs/spec/vm.md の「時計と driver」)。

#![allow(unsafe_code)]

mod net;
#[cfg(feature = "tls")]
mod tls;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use eml_rt::{
    Completion, Done, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, PipePair, RawBuf,
    RawResource, Submit,
};

use crate::flag::Flag;
use net::{Connection, Network, check_virtual_host, no_handle};

/// 終わる操作がないときにだけ、期限まで進む仮想の時計の driver。時計は 0 から始まり、ミリ秒の整数で進む。
pub struct TestDriver {
    /// run の driver が共有する時計。ミリ秒の整数で、どの driver の `wait` で進めても、ほかの driver が同じ時刻を読む。
    now_ms: Arc<AtomicU64>,
    /// seed の方針のときだけある擬似乱数の状態。1回の決着で終わった操作の組を混ぜる。
    rng: Option<u64>,
    /// `submit` の中で終えた操作の数。
    immediate: u64,
    /// まだ終わっていない操作。受け付けの順に並ぶ。
    ops: VecDeque<Pending>,
    /// 終わったが、まだ `completions` で返していない操作。
    done: Vec<Completion>,
    net: Network,
    /// `adopt` で取り込んだファイル。ハンドルは `net` と同じ空間から振る。
    files: HashMap<IoHandle, fs::File>,
    /// 閉じられたが、決着待ちの読み書きが残るファイルのハンドル。その操作が終わるまで `File` を保つ。
    closing: HashSet<IoHandle>,
    /// `tls.localhost` への接続ごとの TLS の終端器。
    #[cfg(feature = "tls")]
    tls: tls::Terminators,
    bin: Arc<HandleBin>,
    waker: Arc<Flag>,
}

struct Pending {
    key: OpKey,
    op: Op,
    cancelled: bool,
}

impl TestDriver {
    pub fn new(seed: Option<u64>) -> TestDriver {
        TestDriver::new_shared(seed, Arc::new(AtomicU64::new(0)))
    }

    /// 時計を `clock` と共有する driver。1つの run の driver は同じ `clock` で作り、時計は1つの数として進む
    /// (docs/spec/vm.md の「時計と driver」)。
    pub fn new_shared(seed: Option<u64>, clock: Arc<AtomicU64>) -> TestDriver {
        TestDriver {
            now_ms: clock,
            rng: seed,
            immediate: 0,
            ops: VecDeque::new(),
            done: Vec::new(),
            net: Network::new(),
            files: HashMap::new(),
            closing: HashSet::new(),
            #[cfg(feature = "tls")]
            tls: tls::Terminators::default(),
            bin: Arc::default(),
            waker: Arc::default(),
        }
    }

    /// この driver が読み書きする時計。同じ run の別の driver を `new_shared` で作るのに渡す。
    pub fn clock(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.now_ms)
    }

    /// `submit` が `Submit::Done` で返した操作の数。Rust のテストが、終わった操作を待たない道を確かめるために読む。
    pub fn immediate(&self) -> u64 {
        self.immediate
    }

    /// 取り込んだファイルのうち、まだ `File` を落としていないものの数。閉じられて読み書きの決着を待つものも数える。
    /// Rust のテストが、`quiesce` の後にファイルが残らないことを確かめるために読む。
    pub fn open_files(&self) -> usize {
        self.files.len()
    }

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

    /// 落とされたハンドルを閉じ、受け付けの順に操作を決着させる。ハンドルを先に閉じるので、同じ決着の中の操作は
    /// 閉じた後の状態を見る。TLS の終端器は、閉じた後で操作の前に汲むので、操作は汲んだ後の状態を見る。
    fn settle(&mut self) {
        self.empty_bin();
        #[cfg(feature = "tls")]
        self.tls.pump(&mut self.net);
        let mut finished = Vec::new();
        let mut waiting = VecDeque::new();
        while let Some(pending) = self.ops.pop_front() {
            match self.try_finish(pending) {
                Ok(completion) => finished.push(completion),
                Err(pending) => waiting.push_back(pending),
            }
        }
        self.ops = waiting;
        self.release_closed_files();
        if let Some(state) = &mut self.rng {
            // Fisher-Yates
            for i in (1..finished.len()).rev() {
                let j = (splitmix64(state) % (i as u64 + 1)) as usize;
                finished.swap(i, j);
            }
        }
        self.done.extend(finished);
    }

    /// 閉じられたファイルのうち、決着待ちの読み書きがもう残っていないものの `File` を落とす。実際の driver が、
    /// 保留の操作があるハンドルの閉じるのを操作の終わりまで延ばすのと同じである (docs/spec/vm.md の「時計と driver」)。
    fn release_closed_files(&mut self) {
        let ops = &self.ops;
        let files = &mut self.files;
        self.closing.retain(|handle| {
            let queued = ops.iter().any(|pending| {
                matches!(&pending.op, Op::Read { handle: h, .. } | Op::Write { handle: h, .. } if h == handle)
            });
            if !queued {
                files.remove(handle);
            }
            queued
        });
    }

    /// 今の状態で終わる操作なら完了を返し、終わらなければ操作を返す。
    fn try_finish(&mut self, pending: Pending) -> Result<Completion, Pending> {
        let key = pending.key;
        let done = |result: io::Result<Done>| -> Result<Completion, Pending> {
            Ok(Completion {
                key,
                result,
                cancelled: false,
            })
        };
        if pending.cancelled {
            return Ok(Completion {
                key,
                result: Err(io::Error::other("cancelled")),
                cancelled: true,
            });
        }
        let wait = |op: Op| -> Result<Completion, Pending> {
            Err(Pending {
                key,
                op,
                cancelled: false,
            })
        };
        match pending.op {
            Op::Job(job) => done(job().map(Done::Size)),
            Op::Adopt { res } => done(self.adopt(res).map(Done::Handle)),
            Op::Wait { .. } => done(Err(no_processes())),
            Op::Read { handle, buf } if self.files.contains_key(&handle) => {
                done(self.file_io(handle, buf, false).map(Done::Size))
            }
            Op::Write { handle, buf } if self.files.contains_key(&handle) => {
                done(self.file_io(handle, buf, true).map(Done::Size))
            }
            Op::ShutdownWrite { handle } => {
                done(self.net.shutdown_write(handle).map(|()| Done::Size(0)))
            }
            Op::SetNodelay { handle, .. } => done(if self.net.is_stream(handle) {
                Ok(Done::Size(0))
            } else {
                Err(no_handle())
            }),
            Op::Write { handle, buf } => {
                let Some(end) = self.net.ends.get(&handle) else {
                    return done(Err(no_handle()));
                };
                let (peer, shut) = (end.peer, end.shut);
                match self.net.ends.get_mut(&peer) {
                    Some(peer) if !peer.closed && !shut => {
                        // 貸した側は、`submit` が `Submit::Done` を返すか完了を取り出すまで記憶を生かし、書き換えない
                        // (`RawBuf`。docs/spec/vm.md の「時計と driver」、docs/spec/object-model.md の T10 の行)
                        peer.inbox.extend(unsafe { bytes(buf) });
                        done(Ok(Done::Size(buf.cap)))
                    }
                    _ => done(Err(io::Error::from(io::ErrorKind::BrokenPipe))),
                }
            }
            Op::Readable { handle } => match self.readable(handle) {
                Some(Ok(())) => done(Ok(Done::Size(0))),
                Some(Err(e)) => done(Err(e)),
                None => wait(Op::Readable { handle }),
            },
            Op::Read { handle, buf } => match self.readable(handle) {
                Some(Ok(())) => {
                    let inbox = &mut self
                        .net
                        .ends
                        .get_mut(&handle)
                        .expect("a readable end")
                        .inbox;
                    let n = buf.cap.min(inbox.len());
                    for (i, byte) in inbox.drain(..n).enumerate() {
                        // 貸した側は、`submit` が `Submit::Done` を返すか完了を取り出すまで記憶を生かし、一意に保ち、
                        // 動かさない。`i < n <= cap` である (`RawBuf`。docs/spec/vm.md の「時計と driver」、docs/spec/object-model.md の T10 の行)
                        unsafe { buf.ptr.add(i).write(byte) };
                    }
                    done(Ok(Done::Size(n)))
                }
                Some(Err(e)) => done(Err(e)),
                None => wait(Op::Read { handle, buf }),
            },
            Op::Listen { addr } => done(self.net.listen(addr.port())),
            Op::Accept { listener } => match self.net.accept(listener) {
                Some(Ok((handle, port))) => done(Ok(Done::Stream {
                    handle,
                    peer: localhost(port),
                })),
                Some(Err(e)) => done(Err(e)),
                None => wait(Op::Accept { listener }),
            },
            Op::Connect { host, port } => match self.connect(&host, port) {
                Ok(Connection::Connected { client_end }) => done(Ok(Done::Stream {
                    handle: client_end,
                    peer: localhost(port),
                })),
                Ok(Connection::Pending) => wait(Op::Connect { host, port }),
                Ok(Connection::Refused) => {
                    done(Err(io::Error::from(io::ErrorKind::ConnectionRefused)))
                }
                Err(e) => done(Err(e)),
            },
        }
    }

    /// ファイルのハンドル `handle` の読みか書きを、その場で OS に対して行う。決着の中と `quiesce` からだけ呼ぶ。
    fn file_io(&mut self, handle: IoHandle, buf: RawBuf, write: bool) -> io::Result<usize> {
        let file = self.files.get_mut(&handle).ok_or_else(no_handle)?;
        if write {
            // 貸した側は、完了を取り出すまで記憶を生かし、書き換えない (`RawBuf`)
            file.write_all(&unsafe { bytes(buf) })?;
            return Ok(buf.cap);
        }
        // 貸した側は、完了を取り出すまで記憶を生かし、一意に保ち、動かさない。`ptr` は `cap` バイトを指し、`cap` が 0 の
        // ときも空でない番地である (`u8` の slice に整列は要らない)
        // (`RawBuf`。docs/spec/vm.md の「時計と driver」、docs/spec/object-model.md の T10 の行)
        let room = unsafe { std::slice::from_raw_parts_mut(buf.ptr, buf.cap) };
        loop {
            match file.read(room) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }

    /// ファイルのハンドルの読み書きか。仕事と同じく受け付けでは動かさず、決着の組に入れる。
    fn is_file_io(&self, op: &Op) -> bool {
        matches!(op, Op::Read { handle, .. } | Op::Write { handle, .. }
            if self.files.contains_key(handle) && !self.closing.contains(handle))
    }

    /// TLS の名前は終端器を挟み、ほかの名前は仮想のホストへ直に接続する。
    fn connect(&mut self, host: &str, port: u16) -> io::Result<Connection> {
        #[cfg(feature = "tls")]
        if let Some(cut) = tls::tls_host(host) {
            return self.tls.connect(&mut self.net, port, cut);
        }
        check_virtual_host(host)?;
        self.net.virtual_connect(port)
    }

    /// `handle` の読みがブロックしないか。データがあるか、相手が閉じているか書く側を閉じていれば読める。読めなければ
    /// `None` である。
    fn readable(&self, handle: IoHandle) -> Option<io::Result<()>> {
        let Some(end) = self.net.ends.get(&handle) else {
            return Some(Err(no_handle()));
        };
        let peer_open = self
            .net
            .ends
            .get(&end.peer)
            .is_some_and(|peer| !peer.closed && !peer.shut);
        (!end.inbox.is_empty() || !peer_open).then_some(Ok(()))
    }
}

/// テストの driver は子のプロセスを仮想にできないので、パイプも子の終わりの待ちも持たない
/// (docs/spec/vm.md の「時計と driver」)。
fn no_processes() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the test driver has no processes",
    )
}

/// 仮想のホストの `port`。仮想のホストの番地はいつも `127.0.0.1` と書く。
fn localhost(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

/// `buf` の指す `cap` バイト。前提は `RawBuf` の約束である。
unsafe fn bytes(buf: RawBuf) -> Vec<u8> {
    if buf.cap == 0 {
        return Vec::new();
    }
    // `cap > 0` の `ptr` は、貸した側が完了まで生かす `cap` バイトを指す (docs/spec/vm.md の「時計と driver」)
    unsafe { std::slice::from_raw_parts(buf.ptr, buf.cap) }.to_vec()
}

/// 小さく決定的な擬似乱数 (splitmix64)。
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl IoDriver for TestDriver {
    fn now(&self) -> Duration {
        Duration::from_millis(self.now_ms.load(Ordering::SeqCst))
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.settle();
        if !self.done.is_empty() || self.waker.take() {
            return Ok(true);
        }
        match deadline_ms {
            // 時計は戻らないので、過ぎた期限では進めない
            Some(deadline_ms) => {
                self.now_ms.fetch_max(deadline_ms, Ordering::SeqCst);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn submit(&mut self, key: OpKey, op: Op) -> Submit {
        // 渡していない完了の鍵もまだ保留である (`IoDriver::submit`)
        debug_assert!(
            self.ops.iter().all(|pending| pending.key != key)
                && self.done.iter().all(|c| c.key != key),
            "a pending key is submitted again"
        );
        // 決着と同じく、落とされたハンドルを閉じた後の状態を見る
        self.empty_bin();
        let pending = Pending {
            key,
            op,
            cancelled: false,
        };
        // 仕事とファイルの読み書きは決着の組に入れ、受け付けの時点では動かさない
        if matches!(pending.op, Op::Job(_)) || self.is_file_io(&pending.op) {
            self.ops.push_back(pending);
            return Submit::Pending;
        }
        match self.try_finish(pending) {
            Ok(completion) => {
                self.immediate += 1;
                Submit::Done(completion.result)
            }
            Err(pending) => {
                self.ops.push_back(pending);
                Submit::Pending
            }
        }
    }

    fn cancel(&mut self, key: OpKey) {
        let files = &self.files;
        if let Some(pending) = self.ops.iter_mut().find(|pending| pending.key == key)
            && !matches!(pending.op, Op::Job(_))
            && !matches!(&pending.op, Op::Read { handle, .. } | Op::Write { handle, .. } if files.contains_key(handle))
        {
            pending.cancelled = true;
        }
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        self.empty_bin();
        out.append(&mut self.done);
    }

    fn pending(&self) -> usize {
        self.ops.len() + self.done.len()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        Arc::clone(&self.waker) as Arc<dyn IoWaker>
    }

    fn bin(&self) -> Arc<HandleBin> {
        Arc::clone(&self.bin)
    }

    fn close(&mut self, handle: IoHandle) {
        if self.files.contains_key(&handle) {
            self.closing.insert(handle);
            self.release_closed_files();
        }
        self.net.close(handle);
    }

    /// 仮想のネットワークの端は driver の外に出せない。そのため受けは `Done::Stream` で終え、`Done::Accepted` の
    /// ソケットを作らない。渡されたソケットは OS のもので、この driver のネットワークにないので断る。ファイルは
    /// 持ち、ネットワークと同じハンドルの空間から番号を振る (docs/spec/vm.md の「時計と driver」)。
    fn adopt(&mut self, res: RawResource) -> io::Result<IoHandle> {
        match res {
            RawResource::Socket(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the test driver adopts no sockets",
            )),
            RawResource::PipeRead(_) | RawResource::PipeWrite(_) => Err(no_processes()),
            RawResource::File(file) => {
                let file = crate::sys::regular_handle(file)?;
                let handle = self.net.handle();
                self.files.insert(handle, fs::File::from(file));
                Ok(handle)
            }
        }
    }

    fn pipe(&mut self, _parent_reads: bool) -> io::Result<PipePair> {
        Err(no_processes())
    }

    fn quiesce(&mut self) {
        // 仕事とファイルの読み書きは取り消せないので、ほかの driver と同じく最後まで動かしてから結果を捨てる
        for pending in std::mem::take(&mut self.ops) {
            match pending.op {
                Op::Job(job) => {
                    let _ = job();
                }
                Op::Read { handle, buf } if self.files.contains_key(&handle) => {
                    let _ = self.file_io(handle, buf, false);
                }
                Op::Write { handle, buf } if self.files.contains_key(&handle) => {
                    let _ = self.file_io(handle, buf, true);
                }
                _ => {}
            }
        }
        self.release_closed_files();
        // 捨てる完了が作ったリスナーとストリームは、受け取る者がいないので閉じる
        for completion in std::mem::take(&mut self.done) {
            if let Ok(Done::Listener { handle, .. } | Done::Stream { handle, .. }) =
                completion.result
            {
                self.net.close(handle);
            }
        }
        self.empty_bin();
    }
}

//! テストの driver。仮想の時計と仮想のネットワークで動き、操作を決まった時点でだけ決着させるので、結果は
//! プログラムと方針と seed で決まる。受け付けの時点で決まる操作は `submit` の中で終え、ほかの操作は `wait` の
//! 始めにだけ決着させる (docs/spec/vm.md の「時計と driver」)。

#![allow(unsafe_code)]

mod net;
#[cfg(feature = "tls")]
mod tls;

use std::collections::VecDeque;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use eml_rt::{Completion, Done, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, RawBuf, Submit};

use crate::flag::Flag;
use net::{Connection, Network, check_virtual_host, no_handle};

/// 終わる操作がないときにだけ、期限まで進む仮想の時計の driver。時計は 0 から始まり、ミリ秒の整数で進む。
pub struct TestDriver {
    now_ms: u64,
    /// seed の方針のときだけある擬似乱数の状態。1回の決着で終わった操作の組を混ぜる。
    rng: Option<u64>,
    /// `submit` の中で終えた操作の数。
    immediate: u64,
    /// まだ終わっていない操作。受け付けの順に並ぶ。
    ops: VecDeque<Pending>,
    /// 終わったが、まだ `completions` で返していない操作。
    done: Vec<Completion>,
    net: Network,
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
        TestDriver {
            now_ms: 0,
            rng: seed,
            immediate: 0,
            ops: VecDeque::new(),
            done: Vec::new(),
            net: Network::new(),
            #[cfg(feature = "tls")]
            tls: tls::Terminators::default(),
            bin: Arc::default(),
            waker: Arc::default(),
        }
    }

    /// `submit` が `Submit::Done` で返した操作の数。Rust のテストが、終わった操作を待たない道を確かめるために読む。
    pub fn immediate(&self) -> u64 {
        self.immediate
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
        if let Some(state) = &mut self.rng {
            // Fisher-Yates
            for i in (1..finished.len()).rev() {
                let j = (splitmix64(state) % (i as u64 + 1)) as usize;
                finished.swap(i, j);
            }
        }
        self.done.extend(finished);
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
        Duration::from_millis(self.now_ms)
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.settle();
        if !self.done.is_empty() || self.waker.take() {
            return Ok(true);
        }
        match deadline_ms {
            // 時計は戻らないので、過ぎた期限では進めない
            Some(deadline_ms) => {
                self.now_ms = self.now_ms.max(deadline_ms);
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
        // 仕事は決着の組に入れ、受け付けの時点では動かさない
        if matches!(pending.op, Op::Job(_)) {
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
        if let Some(pending) = self.ops.iter_mut().find(|pending| pending.key == key)
            && !matches!(pending.op, Op::Job(_))
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

    fn shutdown_write(&mut self, handle: IoHandle) -> io::Result<()> {
        self.net.shutdown_write(handle)
    }

    fn set_nodelay(&mut self, handle: IoHandle, _on: bool) -> io::Result<()> {
        if self.net.is_stream(handle) {
            Ok(())
        } else {
            Err(no_handle())
        }
    }

    fn close(&mut self, handle: IoHandle) {
        self.net.close(handle);
    }

    fn quiesce(&mut self) {
        // 仕事は取り消せないので、ほかの driver と同じく最後まで動かしてから結果を捨てる
        for pending in self.ops.drain(..) {
            if let Op::Job(job) = pending.op {
                let _ = job();
            }
        }
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

//! テストの driver。仮想の時計で動き、操作を決まった時点でだけ決着させるので、結果はプログラムと方針と seed で
//! 決まる (docs/spec/vm.md の「時計と driver」)。

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::Arc;
use std::time::Duration;

use eml_rt::{Completion, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, RawBuf};

use crate::flag::Flag;

/// 終わる操作がないときにだけ、期限まで進む仮想の時計の driver。時計は 0 から始まり、ミリ秒の整数で進む。
pub struct TestDriver {
    now_ms: u64,
    /// seed の方針のときだけある擬似乱数の状態。1回の決着で終わった操作の組を混ぜる。
    rng: Option<u64>,
    next_key: OpKey,
    next_handle: IoHandle,
    /// まだ終わっていない操作。受け付けの順に並ぶ。
    ops: VecDeque<Pending>,
    /// 終わったが、まだ `completions` で返していない操作。
    done: Vec<Completion>,
    ends: HashMap<IoHandle, End>,
    bin: Arc<HandleBin>,
    waker: Arc<Flag>,
}

struct Pending {
    key: OpKey,
    op: Op,
    cancelled: bool,
}

/// `pair` の片方の端。
struct End {
    peer: IoHandle,
    /// 相手が書き、この端がまだ読んでいないバイト。
    inbox: VecDeque<u8>,
    /// driver の参照を落とした。相手は `inbox` の残りを読みきってから EOF を受ける。
    closed: bool,
}

impl TestDriver {
    pub fn new(seed: Option<u64>) -> TestDriver {
        TestDriver {
            now_ms: 0,
            rng: seed,
            next_key: 0,
            next_handle: 0,
            ops: VecDeque::new(),
            done: Vec::new(),
            ends: HashMap::new(),
            bin: Arc::default(),
            waker: Arc::default(),
        }
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
    /// 閉じた後の状態を見る。
    fn settle(&mut self) {
        self.empty_bin();
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
        let done = |result: io::Result<usize>| Ok(Completion { key, result });
        if pending.cancelled {
            return done(Err(io::Error::other("cancelled")));
        }
        match pending.op {
            Op::Job(job) => done(job()),
            Op::Write { handle, buf } => {
                let Some(peer) = self.ends.get(&handle).map(|end| end.peer) else {
                    return done(Err(no_handle()));
                };
                match self.ends.get_mut(&peer) {
                    Some(peer) if !peer.closed => {
                        // 貸した側は完了を取り出すまで記憶を生かし、書き換えない (`RawBuf`)
                        peer.inbox.extend(unsafe { bytes(buf) });
                        done(Ok(buf.cap))
                    }
                    _ => done(Err(io::Error::from(io::ErrorKind::BrokenPipe))),
                }
            }
            Op::Readable { handle } => match self.readable(handle) {
                Some(Ok(())) => done(Ok(0)),
                Some(Err(e)) => done(Err(e)),
                None => Err(Pending {
                    key,
                    op: Op::Readable { handle },
                    cancelled: false,
                }),
            },
            Op::Read { handle, buf } => match self.readable(handle) {
                Some(Ok(())) => {
                    let inbox = &mut self.ends.get_mut(&handle).expect("a readable end").inbox;
                    let n = buf.cap.min(inbox.len());
                    for (i, byte) in inbox.drain(..n).enumerate() {
                        // 貸した側は完了を取り出すまで記憶を生かし、一意に保ち、動かさない (`RawBuf`)
                        unsafe { buf.ptr.add(i).write(byte) };
                    }
                    done(Ok(n))
                }
                Some(Err(e)) => done(Err(e)),
                None => Err(Pending {
                    key,
                    op: Op::Read { handle, buf },
                    cancelled: false,
                }),
            },
        }
    }

    /// `handle` の読みがブロックしないか。データがあるか、相手が閉じていれば読める。読めなければ `None` である。
    fn readable(&self, handle: IoHandle) -> Option<io::Result<()>> {
        let Some(end) = self.ends.get(&handle) else {
            return Some(Err(no_handle()));
        };
        let peer_open = self.ends.get(&end.peer).is_some_and(|peer| !peer.closed);
        (!end.inbox.is_empty() || !peer_open).then_some(Ok(()))
    }
}

/// `buf` の指す `cap` バイト。前提は `RawBuf` の約束である。
unsafe fn bytes(buf: RawBuf) -> Vec<u8> {
    if buf.cap == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(buf.ptr, buf.cap) }.to_vec()
}

fn no_handle() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no such handle")
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

    fn submit(&mut self, op: Op) -> OpKey {
        let key = self.next_key;
        self.next_key += 1;
        self.ops.push_back(Pending {
            key,
            op,
            cancelled: false,
        });
        key
    }

    fn cancel(&mut self, key: OpKey) {
        if let Some(pending) = self.ops.iter_mut().find(|pending| pending.key == key)
            && !matches!(pending.op, Op::Job(_))
        {
            pending.cancelled = true;
        }
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        self.settle();
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

    fn pair(&mut self) -> io::Result<(IoHandle, IoHandle)> {
        let (a, b) = (self.next_handle, self.next_handle + 1);
        self.next_handle += 2;
        for (handle, peer) in [(a, b), (b, a)] {
            self.ends.insert(
                handle,
                End {
                    peer,
                    inbox: VecDeque::new(),
                    closed: false,
                },
            );
        }
        Ok((a, b))
    }

    fn close(&mut self, handle: IoHandle) {
        if let Some(end) = self.ends.get_mut(&handle) {
            end.closed = true;
        }
    }

    fn quiesce(&mut self) {
        // 仕事は取り消せないので、ほかの driver と同じく最後まで動かしてから結果を捨てる
        for pending in self.ops.drain(..) {
            if let Op::Job(job) = pending.op {
                let _ = job();
            }
        }
        self.done.clear();
        self.empty_bin();
    }
}

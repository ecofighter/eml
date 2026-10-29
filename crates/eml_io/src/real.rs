//! 本物の driver。compio-driver の `Proactor` の上で、unix は polling (epoll と kqueue)、Windows は IOCP を使う。
//! ブロックする呼び出しのプールも `Proactor` のものを使う (docs/spec/vm.md の「時計と driver」、
//! docs/implementation/architecture.md の「外部 crate」)。
//!
//! `Proactor` は `Send` でないので、この driver も作ったスレッドから動かない。eml の側では `Send` を足さない。

use std::collections::HashMap;
use std::io;
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::task::Waker;
use std::time::{Duration, Instant};

use crate::mask::Masked;
use compio_buf::{BufResult, IoBuf, IoBufMut, SetLen};
use compio_driver::op::{Asyncify, Recv, RecvFlags, Send, SendFlags};
use compio_driver::{Key, OpCode, Proactor, PushEntry, SharedFd};
use eml_rt::{Completion, HandleBin, IoDriver, IoHandle, IoWaker, Op, OpKey, RawBuf};

#[cfg(unix)]
use compio_driver::op::{Interest, PollOnce};
#[cfg(unix)]
use std::os::fd::OwnedFd as Fd;
#[cfg(windows)]
use std::os::windows::io::OwnedSocket as Fd;

/// 本物の driver が1回の `wait` で眠る長さの上限。空きのループは起きるたびに止める語を読むので、Ctrl-C と確保量の
/// 上限はどの方針でもこの長さのうちに眠る worker を止める (docs/spec/vm.md の「時計と driver」)。
pub const IDLE_TICK: Duration = Duration::from_millis(10);

/// プールのスレッドの数の上限。仕事がこれより多ければ、`submit` はスレッドが空くまで待つ。
const POOL_LIMIT: usize = 32;

/// `quiesce` が取り消した I/O の操作の完了を待つ長さの上限。取り消しの完了はふつうすぐに届くので、これを
/// 過ぎたら届かないものとみなす。
const CANCEL_LIMIT: Duration = Duration::from_secs(5);

type Job = Box<dyn FnOnce() -> BufResult<usize, ()> + std::marker::Send>;

/// compio の操作の鍵。`completions` で `pop` するまでここに置く。
enum Slot {
    #[cfg(unix)]
    Readable(Key<PollOnce<SharedFd<Fd>>>),
    /// IOCP では、長さ0の `WSARecv` が読めるようになったことを知らせる。
    #[cfg(windows)]
    Readable(Key<Recv<Mem, SharedFd<Fd>>>),
    Read(Key<Recv<Mem, SharedFd<Fd>>>),
    Write(Key<Send<Mem, SharedFd<Fd>>>),
    Job(Key<Asyncify<Job, ()>>),
}

/// compio の buffer の trait を `RawBuf` に当てる包み。`RawBuf` は別の crate の型なので、ここで包む。
/// `len` は初期化済みの長さで、書きでは `cap`、読みでは 0 から始まる。
struct Mem {
    buf: RawBuf,
    len: usize,
}

impl IoBuf for Mem {
    fn as_init(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: `len <= cap` で、貸した側は完了を取り出すまで記憶を生かし、一意に保ち、動かさない。driver は
        // 貸した側と同じスレッドで動く (`RawBuf`。docs/spec/vm.md の「時計と driver」、docs/spec/object-model.md の
        // T10 の行)
        unsafe { std::slice::from_raw_parts(self.buf.ptr, self.len) }
    }
}

impl SetLen for Mem {
    unsafe fn set_len(&mut self, len: usize) {
        debug_assert!(len <= self.buf.cap);
        self.len = len;
    }
}

impl IoBufMut for Mem {
    fn as_uninit(&mut self) -> &mut [MaybeUninit<u8>] {
        // `cap == 0` の `ptr` は記憶を指さない (`RawBuf`)
        if self.buf.cap == 0 {
            return &mut [];
        }
        // SAFETY: `as_init` と同じ。読みの間は貸した側も記憶に触れない。外が書いた長さは貸した側が `bytes_commit`
        // で確定する (docs/spec/object-model.md の T10 の行)
        unsafe { std::slice::from_raw_parts_mut(self.buf.ptr.cast(), self.buf.cap) }
    }
}

/// compio の `Waker` を包む。どのスレッドからも `poll` を起こせる。
struct ProactorWaker(Waker);

impl IoWaker for ProactorWaker {
    fn wake(&self) {
        self.0.wake_by_ref();
    }
}

/// 本物の時計で待ち、OS で読み書きする driver。
pub struct RealDriver {
    proactor: Proactor,
    /// 出してまだ終わったと分かっていない操作。compio の鍵はすべてここにある。
    ops: HashMap<OpKey, Slot>,
    /// `push` の中で終わったか、出せなかった操作。次の `completions` で返す。
    done: Vec<Completion>,
    handles: HashMap<IoHandle, SharedFd<Fd>>,
    bin: Arc<HandleBin>,
    waker: Arc<ProactorWaker>,
    /// `poll` が想定外の誤りを返した。以後は待たず、新しい完了も返さない。保留の操作の buffer は貸したまま残る。
    broken: Option<(io::ErrorKind, String)>,
    epoch: Instant,
    next_key: OpKey,
    #[cfg_attr(
        windows,
        expect(dead_code, reason = "Windows の `pair` はハンドルを作らない")
    )]
    next_handle: IoHandle,
}

impl RealDriver {
    pub fn new() -> io::Result<RealDriver> {
        let proactor = Proactor::builder().thread_pool_limit(POOL_LIMIT).build()?;
        let waker = Arc::new(ProactorWaker(proactor.waker()));
        Ok(RealDriver {
            proactor,
            ops: HashMap::new(),
            done: Vec::new(),
            handles: HashMap::new(),
            bin: Arc::default(),
            waker,
            broken: None,
            epoch: Instant::now(),
            next_key: 0,
            next_handle: 0,
        })
    }

    /// 落とされたハンドルを閉じる。
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

    /// `push` が `Ready` を返したら完了として残し、`Pending` なら鍵を覚える。polling では `Send` などが `push` の
    /// 中で終わることがある。
    fn push<T: OpCode + 'static>(&mut self, key: OpKey, op: T, slot: fn(Key<T>) -> Slot) {
        match self.proactor.push(op) {
            PushEntry::Pending(k) => {
                self.ops.insert(key, slot(k));
            }
            PushEntry::Ready(BufResult(result, _)) => self.done.push(Completion { key, result }),
        }
    }

    /// `handle` の fd を写して返す。保留の操作が写しを持つので、`close` の後も操作が終わるまで OS の資源は残る。
    fn fd(&self, handle: IoHandle) -> Option<SharedFd<Fd>> {
        self.handles.get(&handle).cloned()
    }

    /// `timeout` まで `poll` する。`Ok` と、時間切れと、割り込みは戻っただけと読み、ほかの誤りで壊れた状態になる。
    /// polling の backend は起こされたときも時間切れを返す。
    fn poll(&mut self, timeout: Duration) {
        if self.broken.is_some() {
            return;
        }
        match self.proactor.poll(Some(timeout)) {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => self.broken = Some((e.kind(), e.to_string())),
        }
    }

    /// 終わった操作の鍵を `pop` し、結果を `out` に移す。壊れた driver では何もしない。
    fn reap(&mut self, out: &mut Vec<Completion>) {
        if self.broken.is_some() {
            return;
        }
        let ops = std::mem::take(&mut self.ops);
        for (key, slot) in ops {
            let result = match slot {
                Slot::Readable(k) => pop(&mut self.proactor, k).map_err(Slot::Readable),
                Slot::Read(k) => pop(&mut self.proactor, k).map_err(Slot::Read),
                Slot::Write(k) => pop(&mut self.proactor, k).map_err(Slot::Write),
                Slot::Job(k) => pop(&mut self.proactor, k).map_err(Slot::Job),
            };
            match result {
                Ok(result) => out.push(Completion { key, result }),
                Err(slot) => {
                    self.ops.insert(key, slot);
                }
            }
        }
    }
}

/// 終わっていれば結果を、終わっていなければ鍵を返す。
fn pop<T: OpCode>(proactor: &mut Proactor, key: Key<T>) -> Result<io::Result<usize>, Key<T>> {
    match proactor.pop(key) {
        PushEntry::Ready(BufResult(result, _)) => Ok(result),
        PushEntry::Pending(key) => Err(key),
    }
}

fn no_handle() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no such handle")
}

impl IoDriver for RealDriver {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.empty_bin();
        if let Some((kind, message)) = &self.broken {
            return Err(io::Error::new(*kind, message.clone()));
        }
        if !self.done.is_empty() {
            return Ok(true);
        }
        // 別のスレッドからはまだ起こさないので、空きのループが止める語を読めるよう、1回の待ちを短く切る
        let timeout = match deadline_ms {
            Some(deadline_ms) => Duration::from_millis(deadline_ms)
                .saturating_sub(self.now())
                .min(IDLE_TICK),
            None if self.ops.is_empty() => return Ok(false),
            None => IDLE_TICK,
        };
        self.poll(timeout);
        match &self.broken {
            Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
            None => Ok(true),
        }
    }

    fn submit(&mut self, op: Op) -> OpKey {
        let key = self.next_key;
        self.next_key += 1;
        let fd = match &op {
            Op::Readable { handle } | Op::Read { handle, .. } | Op::Write { handle, .. } => {
                match self.fd(*handle) {
                    Some(fd) => Some(fd),
                    None => {
                        self.done.push(Completion {
                            key,
                            result: Err(no_handle()),
                        });
                        return key;
                    }
                }
            }
            Op::Job(_) => None,
        };
        match (op, fd) {
            #[cfg(unix)]
            (Op::Readable { .. }, Some(fd)) => {
                self.push(key, PollOnce::new(fd, Interest::Readable), Slot::Readable)
            }
            #[cfg(windows)]
            (Op::Readable { .. }, Some(fd)) => {
                let mem = Mem {
                    buf: RawBuf {
                        ptr: std::ptr::NonNull::dangling().as_ptr(),
                        cap: 0,
                    },
                    len: 0,
                };
                self.push(key, Recv::new(fd, mem, RecvFlags::empty()), Slot::Readable)
            }
            (Op::Read { buf, .. }, Some(fd)) => {
                let mem = Mem { buf, len: 0 };
                self.push(key, Recv::new(fd, mem, RecvFlags::empty()), Slot::Read)
            }
            (Op::Write { buf, .. }, Some(fd)) => {
                let mem = Mem { buf, len: buf.cap };
                self.push(key, Send::new(fd, mem, SendFlags::empty()), Slot::Write)
            }
            (Op::Job(job), _) => {
                let job: Job = Box::new(move || BufResult(job(), ()));
                // プールのスレッドは `push` を呼んだスレッドから生まれ、塞ぎを受け継ぐ (docs/spec/vm.md の「上限と割り込み」)
                let _masked = Masked::new();
                self.push(key, Asyncify::new(job), Slot::Job)
            }
            (_, None) => unreachable!("an I/O op has a handle"),
        }
        key
    }

    fn cancel(&mut self, key: OpKey) {
        // 鍵を残したまま取り消し、完了は `pop` で受ける。プールの仕事は取り消せない
        let token = match self.ops.get(&key) {
            Some(Slot::Readable(k)) => self.proactor.register_cancel(k),
            Some(Slot::Read(k)) => self.proactor.register_cancel(k),
            Some(Slot::Write(k)) => self.proactor.register_cancel(k),
            Some(Slot::Job(_)) | None => return,
        };
        self.proactor.cancel_token(token);
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        self.empty_bin();
        // 壊れた後は、`push` の中で終わっていた結果も含めて何も渡さない。保留は `quiesce` の後も残り、実行系はそれを見て
        // 機械を poisoned の状態にする (docs/spec/vm.md の「時計と driver」)
        if self.broken.is_some() {
            return;
        }
        let start = out.len();
        out.append(&mut self.done);
        if !self.ops.is_empty() {
            self.poll(Duration::ZERO);
            self.reap(out);
        }
        // `HashMap` の順は走るたびに変わるので、`push` の中で終わったものも含めて受け付けの順にそろえる
        out[start..].sort_by_key(|completion| completion.key);
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

    #[cfg(unix)]
    fn pair(&mut self) -> io::Result<(IoHandle, IoHandle)> {
        let (a, b) = std::os::unix::net::UnixStream::pair()?;
        // polling の backend は `push` の中でまず読み書きを試すので、ブロックしない fd を求める
        a.set_nonblocking(true)?;
        b.set_nonblocking(true)?;
        let (ha, hb) = (self.next_handle, self.next_handle + 1);
        self.next_handle += 2;
        self.handles.insert(ha, SharedFd::new(Fd::from(a)));
        self.handles.insert(hb, SharedFd::new(Fd::from(b)));
        Ok((ha, hb))
    }

    /// Windows のソケットは `net-windows` で扱う。
    #[cfg(windows)]
    fn pair(&mut self) -> io::Result<(IoHandle, IoHandle)> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn close(&mut self, handle: IoHandle) {
        self.handles.remove(&handle);
    }

    fn quiesce(&mut self) {
        self.empty_bin();
        self.done.clear();
        let keys: Vec<OpKey> = self.ops.keys().copied().collect();
        for key in keys {
            self.cancel(key);
        }
        // 取り消しの完了も、走っているプールの仕事も、届くまで待つ。壊れた driver は待てないので、保留を残して戻る
        let mut limit = None;
        let mut discarded = Vec::new();
        while !self.ops.is_empty() && self.broken.is_none() {
            self.poll(IDLE_TICK);
            self.reap(&mut discarded);
            discarded.clear();
            // polling の backend は、取り消しで関心を外す `epoll_ctl` などが失敗すると、完了を出さないまま操作を
            // 落とす。その操作は二度と終わらないので、期限を過ぎても I/O の操作が残れば壊れた状態にして戻る。
            // 仕事はいつか終わるので、仕事が残る間は待ち続け、期限は仕事がなくなってから数える
            let jobs_left = self.ops.values().any(|slot| matches!(slot, Slot::Job(_)));
            if jobs_left || self.ops.is_empty() {
                continue;
            }
            let limit = *limit.get_or_insert_with(|| Instant::now() + CANCEL_LIMIT);
            if Instant::now() >= limit {
                self.broken = Some((
                    io::ErrorKind::Other,
                    "a cancelled op did not complete".to_string(),
                ));
            }
        }
    }
}

impl Drop for RealDriver {
    /// 保留の仕事の鍵を、プールのスレッドと持ち主のスレッドが同時に落とさないようにする。compio は仕事を出すとき、
    /// 鍵の写しを凍らせてプールのスレッドへ送る。凍らせた鍵の間は、ほかのスレッドがその数え方の状態 (ただのセルで、
    /// atomic でない) に触れてはならない。仕事が終わったとき、`Proactor` がなければ、プールのスレッドはその写しを
    /// 自分で落とす。そのため、ここで仕事を待たずに `ops` の鍵を落とすと、2つのスレッドが同じ数を書き換える。
    /// 壊れていなければ `quiesce` で仕事を待ちきる。壊れていて待てなければ、残った鍵を手放さずに漏らす
    /// (docs/spec/vm.md の「時計と driver」)。
    fn drop(&mut self) {
        if self.broken.is_none() {
            self.quiesce();
        }
        if !self.ops.is_empty() {
            std::mem::forget(std::mem::take(&mut self.ops));
        }
        self.empty_bin();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// waker は上限なしの長い `poll` を別のスレッドから起こす。`wait` は1回の待ちを `IDLE_TICK` で切るので、
    /// 起こしは `Proactor` を直に待って確かめる。
    #[test]
    fn a_wake_from_another_thread_ends_a_long_poll() {
        let mut driver = RealDriver::new().unwrap();
        let waker = driver.waker();
        let start = Instant::now();
        let ringer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            waker.wake();
        });
        // polling の backend は起こされた待ちでも時間切れを返す
        match driver.proactor.poll(Some(Duration::from_millis(900))) {
            Ok(()) => {}
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::TimedOut),
        }
        let elapsed = start.elapsed();
        ringer.join().unwrap();
        assert!(elapsed >= Duration::from_millis(50), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(500), "{elapsed:?}");
    }
}

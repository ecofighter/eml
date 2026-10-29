//! 実行の制御。セッションに1つあり、CLI と、どの worker のヒープも `Arc` で同じものを見る。実行を止める理由の語と、
//! セッションのヒープが OS から取ったページのバイト数と、上限を持つ (docs/spec/object-model.md の「実行の制御」)。

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicUsize, Ordering};

use vstd::prelude::*;

verus! {

/// 実行の上限。`Control` を作るときに決め、変えない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// セッションのヒープが OS から取るページのバイト数の上限。
    pub max_bytes: usize,
    /// 深さの上限。VM のタスクのスタックの語の数で数える。
    pub max_depth: usize,
    /// `evv` の長さの上限。
    pub max_evv: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_bytes: usize::MAX, max_depth: usize::MAX, max_evv: usize::MAX }
    }
}

/// 実行を止めた理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Interrupted,
    Terminated,
    DepthLimit,
    MemoryLimit,
    Ended,
    Preempt,
    /// worker が次の安全点で知らせを集めること。止めずに続けるので、利用者には見せない。
    Notice,
}

pub struct Control {
    stop: AtomicU32,
    worker: AtomicU32,
    end: AtomicU32,
    code: AtomicI64,
    bytes: AtomicUsize,
    signals: AtomicU32,
    watched: AtomicU32,
    limits: Limits,
}

impl Control {
    pub fn new(limits: Limits) -> Control {
        Control {
            stop: AtomicU32::new(0),
            worker: AtomicU32::new(0),
            end: AtomicU32::new(0),
            code: AtomicI64::new(0),
            bytes: AtomicUsize::new(0),
            signals: AtomicU32::new(0),
            watched: AtomicU32::new(0),
            limits,
        }
    }

    /// 上限のない新しい `Control`。
    // `Heap::new` はモードごとに呼ぶ側の crate で写される。`Arc` を作る処理をその写しに含めると、計測のプログラムの
    // コード生成単位の分け方が変わり、速い道が呼ぶ側に展開されなくなったので、モードによらないこの関数に置く
    // (docs/implementation/benchmarks.md の「`eml_rt` のインライン化」)
    pub(crate) fn unbounded() -> Arc<Control> {
        Arc::new(Control::new(Limits::default()))
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }
}

} // verus!
// VM も、安全点で止めたときにこの文言を出す
impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Stop::Interrupted => "interrupted",
            Stop::Terminated => "terminated",
            Stop::DepthLimit => "the call depth limit is exceeded",
            Stop::MemoryLimit => "the memory limit is exceeded",
            Stop::Ended => "the run has ended",
            Stop::Preempt => "preempted",
            Stop::Notice => "a signal notice",
        })
    }
}

// 止める語が伝えるのは「止めよ」のビットだけで、確保量の数が伝えるのもその値だけである。シグナルの語と見張りの語も
// ビットだけを伝える。どれにも、ほかのメモリへの書き込みを公開する役はない。そのため、どの読み書きも Relaxed で足りる
impl Control {
    pub const INTERRUPT: u32 = 1;
    pub const MEMORY: u32 = 2;
    pub const TERMINATE: u32 = 8;
    /// worker の止める語にだけ立つ。次の安全点で知らせを集めよ、という意味である。
    pub const NOTICE: u32 = 1 << 30;
    /// worker の止める語にだけ立つ。`stop` には立てない (host の読み手が見る語を変えないため)。
    pub const PREEMPT: u32 = 1 << 31;
    /// シグナルの語で、中継済みのビットを保留のビットから離す幅。
    const RELAYED_SHIFT: u32 = 16;
    const PENDING_MASK: u32 = (1 << Self::RELAYED_SHIFT) - 1;

    /// 止めるビットを `stop` と worker の語の両方に直接書く。worker の語へ写す仲介を置かず、止まる時点を書いた時点で決める。
    fn raise(&self, bits: u32) {
        self.stop.fetch_or(bits, Ordering::Relaxed);
        self.worker.fetch_or(bits, Ordering::Relaxed);
    }

    pub fn interrupt(&self) {
        self.raise(Self::INTERRUPT);
    }

    pub fn terminate(&self) {
        self.raise(Self::TERMINATE);
    }

    /// worker に、次の安全点で切り替えるよう求める。
    pub fn preempt(&self) {
        self.worker.fetch_or(Self::PREEMPT, Ordering::Relaxed);
    }

    /// 切り替えを受けたときに `PREEMPT` を下ろす。
    pub fn take_preempt(&self) {
        self.worker.fetch_and(!Self::PREEMPT, Ordering::Relaxed);
    }

    /// worker に、次の安全点で知らせを集めるよう求める。
    pub fn notice(&self) {
        self.worker.fetch_or(Self::NOTICE, Ordering::Relaxed);
    }

    /// 知らせを集めるときに `NOTICE` を下ろす。
    pub fn take_notice(&self) {
        self.worker.fetch_and(!Self::NOTICE, Ordering::Relaxed);
    }

    /// シグナル `bits` (`INTERRUPT` か `TERMINATE`) を、待つタスクへ届けるまで保留にする。
    pub fn defer(&self, bits: u32) {
        self.signals.fetch_or(bits, Ordering::Relaxed);
    }

    /// まだ届けていない (保留の) シグナルのビット。中継済みのものも含む。
    pub fn pending_signals(&self) -> u32 {
        self.signals.load(Ordering::Relaxed) & Self::PENDING_MASK
    }

    /// 保留のうちまだ中継していないビットを中継済みにして返す。1回の保留につき、返すのは1回だけである。
    pub fn claim_signals(&self) -> u32 {
        let mut new = 0;
        let _ = self
            .signals
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |s| {
                new = s & Self::PENDING_MASK & !(s >> Self::RELAYED_SHIFT);
                (new != 0).then_some(s | new << Self::RELAYED_SHIFT)
            });
        new
    }

    /// 届けたシグナル `bits` の保留と中継済みを下ろす。
    pub fn settle(&self, bits: u32) {
        self.signals
            .fetch_and(!(bits | bits << Self::RELAYED_SHIFT), Ordering::Relaxed);
    }

    /// シグナルの語を空にし、保留だったビットを返す。
    pub fn take_pending(&self) -> u32 {
        self.signals.swap(0, Ordering::Relaxed) & Self::PENDING_MASK
    }

    /// 待つタスクのいるシグナルのビットを書く。handler はこれを読んで、止めるか保留にするかを決める。
    pub fn set_watched(&self, bits: u32) {
        self.watched.store(bits, Ordering::Relaxed);
    }

    pub fn watched(&self) -> u32 {
        self.watched.load(Ordering::Relaxed)
    }

    // `Ctx::poll` が読む。`stop_bits` と同じく、`#[inline]` がないとほかの crate から展開できない
    #[inline]
    pub fn worker_bits(&self) -> u32 {
        self.worker.load(Ordering::Relaxed)
    }

    /// `exit n` の終わりを書く。最初の書き手だけが `code` を書いて真を返し、後の書き手は何も変えずに偽を返す。
    pub fn exit(&self, code: i64) -> bool {
        if self
            .end
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        self.code.store(code, Ordering::Relaxed);
        true
    }

    /// 終わりが書かれていれば、その終了コード。worker が戻った後で host が読む。
    pub fn exit_code(&self) -> Option<i64> {
        match self.end.load(Ordering::Relaxed) {
            0 => None,
            _ => Some(self.code.load(Ordering::Relaxed)),
        }
    }

    /// run の始めに終わりの欄を開き、`PREEMPT` と `NOTICE` を下ろす。止めるビットとシグナルの語は下ろさない。
    pub fn begin_run(&self) {
        self.end.store(0, Ordering::Relaxed);
        self.worker
            .fetch_and(!(Self::PREEMPT | Self::NOTICE), Ordering::Relaxed);
    }

    /// 止める理由のビットとシグナルの語と見張りの語をすべて下ろし、終わりの欄を開く。確保量が上限を下回ってもビットは
    /// 下りないので、止めるビットを下ろすのはこの関数だけである。`PREEMPT` と `NOTICE` は `take_preempt`、`take_notice`、
    /// `begin_run` も下ろす。
    pub fn clear(&self) {
        self.stop.store(0, Ordering::Relaxed);
        self.worker.store(0, Ordering::Relaxed);
        self.end.store(0, Ordering::Relaxed);
        self.signals.store(0, Ordering::Relaxed);
        self.watched.store(0, Ordering::Relaxed);
    }

    // `Ctx::poll` は VM の呼び出しと戻りごとにこれを読む。総称でない関数は、`#[inline]` がないとほかの crate から
    // 展開できない
    #[inline]
    pub fn stop_bits(&self) -> u32 {
        self.stop.load(Ordering::Relaxed)
    }

    /// セッションのヒープが OS から取ったページのバイト数。
    pub fn bytes(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }

    /// ページを取った分を足し、上限を超えたら `MEMORY` を立てる。
    pub(crate) fn charge(&self, bytes: usize) {
        let total = self
            .bytes
            .fetch_add(bytes, Ordering::Relaxed)
            .saturating_add(bytes);
        if total > self.limits.max_bytes {
            self.raise(Self::MEMORY);
        }
    }

    /// 今の確保量に `bytes` を足すと上限を超えるか。確保より前に大きさが分かるとき、確保を試す前に止めるのに使う。
    pub(crate) fn over_budget(&self, bytes: usize) -> bool {
        self.bytes().saturating_add(bytes) > self.limits.max_bytes
    }

    pub(crate) fn refund(&self, bytes: usize) {
        self.bytes.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// 止める理由。割り込み、終了の求め (`TERMINATE`)、確保量、知らせ、切り替え、深さの順に先に報告する。ビットがどれも立っていなければ、
/// 深さの上限で止めたとする。ビットは呼ぶ側が速い道で読んだ値を受け、読み直さない。読み直すと、その間にほかのスレッドが `clear`
/// したとき、深さの上限を超えていなくても `DepthLimit` を返すためである。
#[cold]
pub(crate) fn stop_reason(bits: u32) -> Stop {
    if bits & Control::INTERRUPT != 0 {
        Stop::Interrupted
    } else if bits & Control::TERMINATE != 0 {
        Stop::Terminated
    } else if bits & Control::MEMORY != 0 {
        Stop::MemoryLimit
    } else if bits & Control::NOTICE != 0 {
        Stop::Notice
    } else if bits & Control::PREEMPT != 0 {
        Stop::Preempt
    } else {
        Stop::DepthLimit
    }
}

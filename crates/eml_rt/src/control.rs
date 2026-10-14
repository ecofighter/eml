//! 実行の制御。セッションに1つあり、CLI と、どの worker のヒープも `Arc` で同じものを見る。実行を止める理由の語と、
//! セッションのヒープが OS から取ったページのバイト数と、上限を持つ (docs/spec/object-model.md の「実行の制御」)。

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

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
    DepthLimit,
    MemoryLimit,
}

pub struct Control {
    stop: AtomicU32,
    bytes: AtomicUsize,
    limits: Limits,
}

impl Control {
    pub fn new(limits: Limits) -> Control {
        Control { stop: AtomicU32::new(0), bytes: AtomicUsize::new(0), limits }
    }

    /// 上限のない新しい `Control`。
    // `Heap::new` はモードごとに呼ぶ側の crate で写される。`Arc` を作る処理をその写しに含めると、計測のプログラムの
    // コード生成単位の分け方が変わり、速い道が呼ぶ側に展開されなくなったので、モードによらないこの関数に置く
    // (docs/implementation/benchmarks.md の「S9b-2a (凍結した領域)」)
    pub(crate) fn unbounded() -> Arc<Control> {
        Arc::new(Control::new(Limits::default()))
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }
}

} // verus!

// 止める語が伝えるのは「止めよ」のビットだけで、ほかのメモリへの書き込みを公開する役はない。そのため、どの読み書きも
// Relaxed で足りる
impl Control {
    pub const INTERRUPT: u32 = 1;
    pub const MEMORY: u32 = 2;

    pub fn interrupt(&self) {
        self.stop.fetch_or(Self::INTERRUPT, Ordering::Relaxed);
    }

    /// 止める理由のビットをすべて下ろす。確保量が上限を下回ってもビットは下りないので、下ろすのはこの関数だけである。
    pub fn clear(&self) {
        self.stop.store(0, Ordering::Relaxed);
    }

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
            self.stop.fetch_or(Self::MEMORY, Ordering::Relaxed);
        }
    }

    pub(crate) fn refund(&self, bytes: usize) {
        self.bytes.fetch_sub(bytes, Ordering::Relaxed);
    }
}

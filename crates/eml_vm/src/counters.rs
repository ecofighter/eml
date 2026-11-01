//! `Std.Atomic.Counter` の表 (docs/spec/vm.md の「セルと登録表」)。登録表の外にあり、`Machine` が `Arc` で持って
//! run に渡す。どの worker のタスクも lock を取らずに読み書きできるように、項は動かない `AtomicI64` の塊に置く。

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering::SeqCst};

use crate::error::VmFault;

/// 塊 b は `FIRST << b` 個の項を持つ。
const FIRST: usize = 64;
/// 塊の数。合わせて `FIRST * (2^26 - 1)` 個の項を持つ。
const BUCKETS: usize = 26;
const FULL: VmFault = VmFault::Internal("the counter table is full");
const OUT: VmFault = VmFault::Internal("a counter index is out of range");

pub(crate) struct Counters {
    /// 次に渡す添字。`push` だけが進める。
    len: AtomicUsize,
    /// 塊は使い始めるときに1回だけ作り、その後は動かさない。作る `push` が終わるまで、同じ塊の初めての `push` は待つ。
    buckets: [OnceLock<Box<[AtomicI64]>>; BUCKETS],
}

impl Default for Counters {
    fn default() -> Counters {
        Counters {
            len: AtomicUsize::new(0),
            buckets: std::array::from_fn(|_| OnceLock::new()),
        }
    }
}

/// 添字 `i` の塊の番号と塊の中の位置。大きすぎる添字は `None` にして、あふれさせない。
fn locate(i: usize) -> Option<(usize, usize)> {
    let j = i.checked_add(FIRST)?;
    let b = (j.ilog2() - FIRST.ilog2()) as usize;
    Some((b, j - (FIRST << b)))
}

impl Counters {
    /// 新しい項に `n` を置いて、その添字を返す。
    pub(crate) fn push(&self, n: i64) -> Result<i64, VmFault> {
        let i = self.len.fetch_add(1, SeqCst);
        let (b, o) = locate(i).ok_or(FULL)?;
        let bucket = self
            .buckets
            .get(b)
            .ok_or(FULL)?
            .get_or_init(|| (0..FIRST << b).map(|_| AtomicI64::new(0)).collect());
        bucket[o].store(n, SeqCst);
        i64::try_from(i).map_err(|_| FULL)
    }

    /// 添字の範囲は塊があるかで確かめ、`len` は読まない。作っていない塊の添字は表の外である。検査を通ったプログラムは
    /// 渡された添字しか持たないので、ここへは届かない。
    fn slot(&self, c: i64) -> Result<&AtomicI64, VmFault> {
        let (b, o) = usize::try_from(c).ok().and_then(locate).ok_or(OUT)?;
        self.buckets
            .get(b)
            .and_then(OnceLock::get)
            .and_then(|s| s.get(o))
            .ok_or(OUT)
    }

    /// `n` を足して、足した後の値を返す。64 ビットで巻き戻る。
    pub(crate) fn add(&self, c: i64, n: i64) -> Result<i64, VmFault> {
        Ok(self.slot(c)?.fetch_add(n, SeqCst).wrapping_add(n))
    }

    pub(crate) fn load(&self, c: i64) -> Result<i64, VmFault> {
        Ok(self.slot(c)?.load(SeqCst))
    }

    pub(crate) fn store(&self, c: i64, n: i64) -> Result<(), VmFault> {
        self.slot(c)?.store(n, SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_out<T>(r: Result<T, VmFault>) -> bool {
        matches!(r, Err(VmFault::Internal(m)) if m == "a counter index is out of range")
    }

    #[test]
    fn locate_numbers_every_slot_once() {
        let mut next = (0, 0);
        for i in 0..100_000 {
            let (b, o) = locate(i).unwrap();
            assert_eq!((b, o), next, "index {i}");
            next = if o + 1 == FIRST << b {
                (b + 1, 0)
            } else {
                (b, o + 1)
            };
        }
    }

    #[test]
    fn push_hands_out_consecutive_indices() {
        let t = Counters::default();
        for k in 0..200 {
            assert_eq!(t.push(k * 3).unwrap(), k);
        }
        assert_eq!(t.load(0).unwrap(), 0);
        assert_eq!(t.load(199).unwrap(), 597);
    }

    #[test]
    fn an_index_outside_the_table_is_out_of_range() {
        let t = Counters::default();
        t.push(1).unwrap();
        assert!(is_out(t.load(-1)));
        assert!(is_out(t.load(64)));
        assert!(is_out(t.load(i64::MAX)));
        assert!(is_out(t.add(i64::MAX, 1)));
        assert!(is_out(t.store(64, 1)));
    }

    #[test]
    fn add_wraps_around() {
        let t = Counters::default();
        let c = t.push(i64::MAX).unwrap();
        assert_eq!(t.add(c, 1).unwrap(), i64::MIN);
        assert_eq!(t.load(c).unwrap(), i64::MIN);
        t.store(c, 5).unwrap();
        assert_eq!(t.add(c, -7).unwrap(), -2);
    }

    #[test]
    fn concurrent_pushes_and_adds_agree() {
        let t = Counters::default();
        let shared = t.push(0).unwrap();
        let got: Vec<Vec<(i64, i64)>> = std::thread::scope(|s| {
            let hands: Vec<_> = (0..4)
                .map(|k| {
                    let t = &t;
                    s.spawn(move || {
                        let mut mine = Vec::new();
                        for j in 0..70 {
                            mine.push((t.push(k * 1000 + j).unwrap(), k * 1000 + j));
                        }
                        for _ in 0..200 {
                            t.add(shared, 1).unwrap();
                        }
                        mine
                    })
                })
                .collect();
            hands.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let mut seen = std::collections::HashSet::new();
        for (i, v) in got.into_iter().flatten() {
            assert!(seen.insert(i), "index {i} was handed out twice");
            assert_eq!(t.load(i).unwrap(), v);
        }
        assert_eq!(seen.len(), 280);
        assert_eq!(t.load(shared).unwrap(), 800);
    }
}

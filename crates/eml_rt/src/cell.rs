//! rc と検査モードの前置きの atomic の読み書き (docs/spec/object-model.md の「`unsafe` の核」)。欄は整数で持ち、ここだけが
//! `from_ptr` で atomic の型として触る。vstd の atomic は SeqCst だけを扱い、核では SeqCst を使わないので、Verus には
//! 中身を見せず、権限を1つのスレッドが持つときの逐次の値を約束する (信頼の一覧の T0 と T2)。

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use vstd::prelude::*;
use vstd::raw_ptr::PointsTo;

verus! {

#[verifier::external_body]
#[inline]
pub(crate) fn rc_load(p: *mut i32, Tracked(perm): Tracked<&PointsTo<i32>>) -> (r: i32)
    requires
        perm.ptr() == p,
        perm.is_init(),
    ensures
        r == perm.value(),
{
    let cell = unsafe { AtomicI32::from_ptr(p) };
    cell.load(Ordering::Relaxed)
}

#[verifier::external_body]
#[inline]
pub(crate) fn rc_store(p: *mut i32, Tracked(perm): Tracked<&mut PointsTo<i32>>, v: i32)
    requires
        old(perm).ptr() == p,
    ensures
        final(perm).ptr() == p,
        final(perm).is_init(),
        final(perm).value() == v,
{
    let cell = unsafe { AtomicI32::from_ptr(p) };
    cell.store(v, Ordering::Relaxed)
}

/// 共有の `dup`。数を負の向きに1つ進める。順序は要らないので Relaxed で足りる (docs/spec/object-model.md の「共有の側」)。
#[verifier::external_body]
#[inline]
pub(crate) fn rc_dup_shared(p: *mut i32, Tracked(perm): Tracked<&mut PointsTo<i32>>)
    requires
        old(perm).ptr() == p,
        old(perm).is_init(),
        old(perm).value() > i32::MIN,
    ensures
        final(perm).ptr() == p,
        final(perm).is_init(),
        final(perm).value() == old(perm).value() - 1,
{
    let cell = unsafe { AtomicI32::from_ptr(p) };
    cell.fetch_sub(1, Ordering::Relaxed);
}

/// 共有の数を1つ減らし (正の向きに進め)、最後の参照なら真を返す。`fence(Acquire)` を使わず、AcqRel の加算で最後の解放を
/// 順序付ける (docs/spec/object-model.md の「共有の側」)。
#[verifier::external_body]
#[inline]
pub(crate) fn rc_decref_shared(p: *mut i32, Tracked(perm): Tracked<&mut PointsTo<i32>>) -> (r: bool)
    requires
        old(perm).ptr() == p,
        old(perm).is_init(),
        old(perm).value() < 0,
    ensures
        final(perm).ptr() == p,
        final(perm).is_init(),
        final(perm).value() == old(perm).value() + 1,
        r == (old(perm).value() == -1),
{
    let cell = unsafe { AtomicI32::from_ptr(p) };
    cell.fetch_add(1, Ordering::AcqRel) == -1
}

/// 手放そうとしている参照が最後のものか (rc が 1 か -1)。rc は Relaxed で読み、-1 のときだけ Acquire で読み直す。
/// ふだんの rc が 1 の道に Acquire の読み込みの費用を足さず、-1 のときは最後の AcqRel の `decref` と同期するためである。
#[verifier::external_body]
#[inline]
pub(crate) fn rc_is_last(p: *mut i32, Tracked(perm): Tracked<&PointsTo<i32>>) -> (r: bool)
    requires
        perm.ptr() == p,
        perm.is_init(),
    ensures
        r == (perm.value() == 1 || perm.value() == -1),
{
    let rc = unsafe { AtomicI32::from_ptr(p) };
    match rc.load(Ordering::Relaxed) {
        1 => true,
        -1 => rc.load(Ordering::Acquire) == -1,
        _ => false,
    }
}

/// 検査モードの前置きの欄を読む。前置きは検査モードの `share` が所有者を書き換えるので、rc と同じく atomic で触る。
#[verifier::external_body]
#[inline]
pub(crate) fn u32_load(p: *mut u32, Tracked(perm): Tracked<&PointsTo<u32>>) -> (r: u32)
    requires
        perm.ptr() == p,
        perm.is_init(),
    ensures
        r == perm.value(),
{
    let cell = unsafe { AtomicU32::from_ptr(p) };
    cell.load(Ordering::Relaxed)
}

#[verifier::external_body]
#[inline]
pub(crate) fn u32_store(p: *mut u32, Tracked(perm): Tracked<&mut PointsTo<u32>>, v: u32)
    requires
        old(perm).ptr() == p,
    ensures
        final(perm).ptr() == p,
        final(perm).is_init(),
        final(perm).value() == v,
{
    let cell = unsafe { AtomicU32::from_ptr(p) };
    cell.store(v, Ordering::Relaxed)
}

} // verus!

#[cfg(test)]
mod tests {
    use super::*;

    // 権限は ghost なので、テストは Tracked::assume_new() で作る
    fn cell(v: i32) -> Box<i32> {
        Box::new(v)
    }

    #[test]
    fn shared_counts_move_atomically_and_find_the_last_reference() {
        let mut rc = cell(-2);
        let p: *mut i32 = &mut *rc;
        rc_dup_shared(p, Tracked::assume_new());
        assert_eq!(rc_load(p, Tracked::assume_new()), -3);
        assert!(!rc_decref_shared(p, Tracked::assume_new()));
        assert!(!rc_decref_shared(p, Tracked::assume_new()));
        assert!(rc_decref_shared(p, Tracked::assume_new()));
        assert_eq!(rc_load(p, Tracked::assume_new()), 0);
    }

    #[test]
    fn is_last_is_true_only_at_one_and_minus_one() {
        for (n, last) in [(1, true), (-1, true), (2, false), (-2, false), (0, false)] {
            let mut rc = cell(n);
            let p: *mut i32 = &mut *rc;
            assert_eq!(rc_is_last(p, Tracked::assume_new()), last, "{n}");
        }
    }
}

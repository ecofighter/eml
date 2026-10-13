//! RC の遷移 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「遷移の関数」)。rc の値だけを受けて次の
//! 状態を返す純粋な関数にし、ヒープ、単体テスト、Kani が同じ関数を使う。rc は 0 が不死、正が局所、負が共有である。

use crate::sync::{AtomicI32, Ordering};

/// 局所の数の上限。ここに達した数は動かさない (spec の「上限」)。
pub(crate) const LOCAL_MAX: i32 = i32::MAX;
/// 共有の `dup` は、これより小さい数を動かさない。
pub(crate) const DUP_LIMIT: i32 = i32::MIN + (1 << 16);
/// 共有の `decref` は、これより小さい数を動かさない。2つの境目の間の幅が、並行の加減算で数が戻ってくるのを防ぐ
/// (spec の「上限」)。
pub(crate) const DROP_LIMIT: i32 = i32::MIN + (1 << 17);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DupStep {
    /// 局所の数。この値を書き戻す。
    Store(i32),
    /// 共有の数。atomic に増やす。
    Shared,
    /// 不死か、張り付いた数。何もしない。
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecrefStep {
    Store(i32),
    /// 局所の最後の参照。
    Free,
    Shared,
    Skip,
}

/// 局所の速い道は比較1回で決める。rc を符号なしで見て、1 以上 `LOCAL_MAX - 1` 以下かを1回で確かめる。
#[inline]
pub(crate) fn dup_step(rc: i32) -> DupStep {
    if (rc as u32).wrapping_sub(1) < (LOCAL_MAX - 1) as u32 {
        DupStep::Store(rc + 1)
    } else if (DUP_LIMIT..0).contains(&rc) {
        DupStep::Shared
    } else {
        DupStep::Skip
    }
}

/// 2 以上 `LOCAL_MAX - 1` 以下を比較1回で確かめ、1 なら解放する。
#[inline]
pub(crate) fn decref_step(rc: i32) -> DecrefStep {
    if (rc as u32).wrapping_sub(2) < (LOCAL_MAX - 2) as u32 {
        DecrefStep::Store(rc - 1)
    } else if rc == 1 {
        DecrefStep::Free
    } else if (DROP_LIMIT..0).contains(&rc) {
        DecrefStep::Shared
    } else {
        DecrefStep::Skip
    }
}

/// `share` の1物体分の印付け。局所の数 n を −n にし、`DROP_LIMIT` より小さくなるなら `MIN` に張り付ける。局所でない数は
/// たどりを止めるので `None` を返す。
#[allow(dead_code)]
pub(crate) fn mark_step(rc: i32) -> Option<i32> {
    if rc <= 0 {
        return None;
    }
    let shared = -rc;
    Some(if shared < DROP_LIMIT {
        i32::MIN
    } else {
        shared
    })
}

pub(crate) fn dup_shared(rc: &AtomicI32) {
    rc.fetch_sub(1, Ordering::Relaxed);
}

/// 共有の数を1つ減らし、最後の参照なら真を返す。`fence(Acquire)` を使わず、AcqRel の加算で最後の解放を順序付ける。
pub(crate) fn decref_shared(rc: &AtomicI32) -> bool {
    rc.fetch_add(1, Ordering::AcqRel) == -1
}

/// 手放そうとしている参照が最後のものか。rc は Relaxed で読み、−1 のときだけ Acquire で読み直す。ふだんの rc が 1 の
/// 道に Acquire の読み込みの費用を足さず、−1 のときは最後の AcqRel の `decref` と同期するためである。
pub(crate) fn is_last(rc: &AtomicI32) -> bool {
    match rc.load(Ordering::Relaxed) {
        1 => true,
        -1 => rc.load(Ordering::Acquire) == -1,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dup_takes_the_fast_path_only_below_the_local_cap() {
        assert_eq!(dup_step(1), DupStep::Store(2));
        assert_eq!(dup_step(LOCAL_MAX - 1), DupStep::Store(LOCAL_MAX));
        assert_eq!(dup_step(LOCAL_MAX), DupStep::Skip);
        assert_eq!(dup_step(0), DupStep::Skip);
        assert_eq!(dup_step(-1), DupStep::Shared);
        assert_eq!(dup_step(DUP_LIMIT), DupStep::Shared);
        assert_eq!(dup_step(DUP_LIMIT - 1), DupStep::Skip);
        assert_eq!(dup_step(i32::MIN), DupStep::Skip);
    }

    #[test]
    fn decref_frees_at_one_and_skips_stuck_counts() {
        assert_eq!(decref_step(2), DecrefStep::Store(1));
        assert_eq!(decref_step(1), DecrefStep::Free);
        assert_eq!(decref_step(LOCAL_MAX - 1), DecrefStep::Store(LOCAL_MAX - 2));
        assert_eq!(decref_step(LOCAL_MAX), DecrefStep::Skip);
        assert_eq!(decref_step(0), DecrefStep::Skip);
        assert_eq!(decref_step(-1), DecrefStep::Shared);
        assert_eq!(decref_step(DROP_LIMIT), DecrefStep::Shared);
        assert_eq!(decref_step(DROP_LIMIT - 1), DecrefStep::Skip);
    }

    #[test]
    fn marking_flips_local_counts_and_leaves_the_rest() {
        assert_eq!(mark_step(3), Some(-3));
        assert_eq!(mark_step(LOCAL_MAX), Some(i32::MIN));
        assert_eq!(mark_step(-DROP_LIMIT), Some(DROP_LIMIT));
        assert_eq!(mark_step(-DROP_LIMIT + 1), Some(i32::MIN));
        assert_eq!(mark_step(0), None);
        assert_eq!(mark_step(-2), None);
    }

    #[test]
    fn shared_counts_move_atomically_and_find_the_last_reference() {
        let rc = AtomicI32::new(-2);
        dup_shared(&rc);
        assert_eq!(rc.load(Ordering::Relaxed), -3);
        assert!(!decref_shared(&rc));
        assert!(!decref_shared(&rc));
        assert!(decref_shared(&rc));
        assert_eq!(rc.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn is_last_is_true_only_at_one_and_minus_one() {
        for (n, last) in [(1, true), (-1, true), (2, false), (-2, false), (0, false)] {
            assert_eq!(is_last(&AtomicI32::new(n)), last, "{n}");
        }
    }

    #[test]
    fn a_skipped_shared_dup_never_comes_back_to_the_decref_range() {
        // dup を1回でも飛ばした数は DUP_LIMIT より小さく、decref はもう数を動かさない
        let mut rc = DUP_LIMIT;
        assert_eq!(dup_step(rc), DupStep::Shared);
        rc -= 1;
        assert_eq!(dup_step(rc), DupStep::Skip);
        assert_eq!(decref_step(rc), DecrefStep::Skip);
    }
}

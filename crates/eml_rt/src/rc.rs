//! RC の遷移 (docs/spec/object-model.md の「遷移の関数」)。rc の値だけを受けて次の
//! 状態を返す純粋な関数にし、ヒープと単体テストが同じ関数を使い、Verus が同じ関数を証明する。rc は 0 が不死、
//! 正が局所、負が共有である。

use vstd::prelude::*;

verus! {

/// 局所の数の上限。ここに達した数は動かさない (docs/spec/object-model.md の「上限」)。
pub(crate) const LOCAL_MAX: i32 = i32::MAX;

/// 共有の `dup` は、これより小さい数を動かさない。
/// 値は MIN + (1 << 16) である。16 進で書くのは、Verus が定数の式のシフトを桁あふれのおそれと報告するためである。
pub(crate) const DUP_LIMIT: i32 = i32::MIN + 0x1_0000;

/// 共有の `decref` は、これより小さい数を動かさない。2つの境目の間の幅が、並行の加減算で数が戻ってくるのを防ぐ
/// (docs/spec/object-model.md の「上限」)。
/// 値は MIN + (1 << 17) である。16 進で書く理由は `DUP_LIMIT` と同じである。`pub` にするのは、`pub open spec fn
/// mark_spec` がこの定数を使い、Verus が open な spec fn の使う定数にも `pub` を求めるためである。この crate の `rc` は
/// private な module なので、外には見えない。
pub const DROP_LIMIT: i32 = i32::MIN + 0x2_0000;

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

/// `dup_step` の結果を範囲で書いたもの。本体の符号なしの比較1回が、この範囲の判定と同じであることを示す
pub(crate) open spec fn dup_spec(rc: i32) -> DupStep {
    if 1 <= rc < LOCAL_MAX {
        DupStep::Store((rc + 1) as i32)
    } else if DUP_LIMIT <= rc < 0 {
        DupStep::Shared
    } else {
        DupStep::Skip
    }
}

pub(crate) open spec fn decref_spec(rc: i32) -> DecrefStep {
    if 2 <= rc < LOCAL_MAX {
        DecrefStep::Store((rc - 1) as i32)
    } else if rc == 1 {
        DecrefStep::Free
    } else if DROP_LIMIT <= rc < 0 {
        DecrefStep::Shared
    } else {
        DecrefStep::Skip
    }
}

/// `mark_step` の結果を値で書いたもの。`share_walk` の証明が、実装を開かずにこの値を使えるようにする
pub open spec fn mark_spec(rc: i32) -> i32 {
    if -rc < DROP_LIMIT {
        i32::MIN
    } else {
        (-rc) as i32
    }
}

/// 負の数を符号なしで見ると `2^31` 以上になるので、本体の比較1回は負の数を速い道に入れない
proof fn lemma_negative_as_u32(rc: i32)
    by (bit_vector)
    ensures
        rc < 0 ==> rc as u32 >= 0x8000_0000u32,
{
}

/// 局所の速い道は比較1回で決める。rc を符号なしで見て、1 以上 `LOCAL_MAX - 1` 以下かを1回で確かめる。
// 範囲を比較で書くのは、vstd の `Range::contains` の約束を信頼せずに済むようにするためである
#[allow(clippy::manual_range_contains)]
#[inline]
pub(crate) fn dup_step(rc: i32) -> (r: DupStep)
    ensures
        r == dup_spec(rc),
{
    proof {
        lemma_negative_as_u32(rc);
    }
    if (rc as u32).wrapping_sub(1) < (LOCAL_MAX - 1) as u32 {
        DupStep::Store(rc + 1)
    } else if DUP_LIMIT <= rc && rc < 0 {
        DupStep::Shared
    } else {
        DupStep::Skip
    }
}

/// 2 以上 `LOCAL_MAX - 1` 以下を比較1回で確かめ、1 なら解放する。
// 範囲を比較で書く理由は `dup_step` と同じである
#[allow(clippy::manual_range_contains)]
#[inline]
pub(crate) fn decref_step(rc: i32) -> (r: DecrefStep)
    ensures
        r == decref_spec(rc),
{
    proof {
        lemma_negative_as_u32(rc);
    }
    if (rc as u32).wrapping_sub(2) < (LOCAL_MAX - 2) as u32 {
        DecrefStep::Store(rc - 1)
    } else if rc == 1 {
        DecrefStep::Free
    } else if DROP_LIMIT <= rc && rc < 0 {
        DecrefStep::Shared
    } else {
        DecrefStep::Skip
    }
}

/// `share` の1物体分の印付け。局所の数 n を -n にし、`DROP_LIMIT` より小さくなるなら `MIN` に張り付ける。局所でない数は
/// たどりを止めるので `None` を返す。
pub(crate) fn mark_step(rc: i32) -> (r: Option<i32>)
    ensures
        rc <= 0 ==> r is None,
        rc > 0 ==> r == Some(mark_spec(rc)),
{
    if rc <= 0 {
        return None;
    }
    let shared = -rc;
    Some(
        if shared < DROP_LIMIT {
            i32::MIN
        } else {
            shared
        },
    )
}

// RC のモデル。物体の状態は、rc、本当の参照の数、張り付きの旗である
// 仕様だけで使う型。Verus の外のビルドでも型は残るので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) ghost struct Obj {
    pub rc: i32,
    pub count: nat,
    pub stuck: bool,
}

/// 1回の遷移の結果。`freed` は、ヒープがその遷移で物体を解放するか
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) ghost struct Next {
    pub obj: Obj,
    pub freed: bool,
}

pub(crate) open spec fn magnitude(rc: i32) -> int {
    if rc < 0 {
        -rc
    } else {
        rc as int
    }
}

/// 張り付く数。局所の数は `LOCAL_MAX` で、共有の数は `DROP_LIMIT` より小さいところで張り付く。`DUP_LIMIT` 以上
/// `DROP_LIMIT` 未満では `dup` が数えても `decref` は飛ばすので、そこも張り付いたものとして扱う
pub(crate) open spec fn pinned(rc: i32) -> bool {
    rc == LOCAL_MAX || rc < DROP_LIMIT
}

/// モデルとの対応。張り付いていない間は、符号を除いた rc が数に一致し、数は正である。張り付いた物体の rc は、張り付く
/// 数の範囲から出ない
pub(crate) open spec fn corresponds(s: Obj) -> bool {
    if s.stuck {
        pinned(s.rc)
    } else {
        !pinned(s.rc) && magnitude(s.rc) == s.count && s.count > 0
    }
}

/// 始めの状態。数は符号を除いた rc で、旗は rc が張り付く数のときに立つ
pub(crate) open spec fn start(rc: i32) -> Obj {
    Obj { rc, count: magnitude(rc) as nat, stuck: pinned(rc) }
}

/// 遷移の後の状態。rc が張り付く数になったら旗を立て、立った旗は下ろさない
pub(crate) open spec fn settle(rc: i32, count: nat, stuck: bool, freed: bool) -> Next {
    Next { obj: Obj { rc, count, stuck: stuck || pinned(rc) }, freed }
}

/// `dup` は数を1増やす。共有の数は `dup_shared` が1減らす (負の向きに進める)
pub(crate) open spec fn dup_next(s: Obj) -> Next {
    let rc = match dup_spec(s.rc) {
        DupStep::Store(n) => n,
        DupStep::Shared => (s.rc - 1) as i32,
        DupStep::Skip => s.rc,
    };
    settle(rc, s.count + 1, s.stuck, false)
}

/// `decref` は数を1減らす。共有の数は `decref_shared` が1増やし、0 になれば最後の参照なので解放する
pub(crate) open spec fn decref_next(s: Obj) -> Next {
    let count = (s.count - 1) as nat;
    match decref_spec(s.rc) {
        DecrefStep::Store(n) => settle(n, count, s.stuck, false),
        DecrefStep::Free => settle(s.rc, count, s.stuck, true),
        DecrefStep::Shared => settle((s.rc + 1) as i32, count, s.stuck, s.rc + 1 == 0),
        DecrefStep::Skip => settle(s.rc, count, s.stuck, false),
    }
}

/// `share` の印付けは数を変えず、`mark_step` が `Some(n)` なら rc を n にする
pub(crate) open spec fn mark_next(s: Obj) -> Next {
    let rc = if s.rc > 0 {
        mark_spec(s.rc)
    } else {
        s.rc
    };
    settle(rc, s.count, s.stuck, false)
}

// 定理。1回の遷移ごとに対応が保たれるので、任意の i32 から始めた任意の長さの列でも保たれる
pub(crate) proof fn lemma_start(rc: i32)
    requires
        rc != 0,
    ensures
        corresponds(start(rc)),
{
}

/// 共有の数を動かす枝は負の数の上でだけ選ばれるので、`rc - 1` と `rc + 1` は i32 の範囲に収まる
pub(crate) proof fn lemma_shared_steps_stay_in_range(rc: i32)
    ensures
        dup_spec(rc) is Shared ==> DUP_LIMIT <= rc < 0 && i32::MIN <= rc - 1,
        decref_spec(rc) is Shared ==> DROP_LIMIT <= rc < 0 && rc + 1 <= 0,
{
}

pub(crate) proof fn lemma_dup(s: Obj)
    requires
        corresponds(s),
    ensures
        corresponds(dup_next(s).obj),
        !dup_next(s).freed,
{
}

pub(crate) proof fn lemma_decref(s: Obj)
    requires
        corresponds(s),
        s.count > 0,
    ensures
        decref_next(s).obj.count == s.count - 1,
        decref_next(s).freed ==> !s.stuck && decref_next(s).obj.count == 0,
        !s.stuck && decref_next(s).obj.count == 0 ==> decref_next(s).freed,
        !decref_next(s).freed ==> corresponds(decref_next(s).obj),
{
}

pub(crate) proof fn lemma_mark(s: Obj)
    requires
        corresponds(s),
    ensures
        corresponds(mark_next(s).obj),
        !mark_next(s).freed,
{
}

/// 不死の数 0 は、どの遷移でも動かず、解放もされない
pub(crate) proof fn lemma_immortal(s: Obj)
    requires
        s.rc == 0,
    ensures
        dup_next(s).obj.rc == 0 && !dup_next(s).freed,
        decref_next(s).obj.rc == 0 && !decref_next(s).freed,
        mark_next(s).obj.rc == 0 && !mark_next(s).freed,
{
}

} // verus!
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
    fn a_skipped_shared_dup_never_comes_back_to_the_decref_range() {
        // dup を1回でも飛ばした数は DUP_LIMIT より小さく、decref はもう数を動かさない
        let mut rc = DUP_LIMIT;
        assert_eq!(dup_step(rc), DupStep::Shared);
        rc -= 1;
        assert_eq!(dup_step(rc), DupStep::Skip);
        assert_eq!(decref_step(rc), DecrefStep::Skip);
    }
}

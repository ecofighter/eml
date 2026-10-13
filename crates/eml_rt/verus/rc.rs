//! `rc.rs` の定数と遷移の関数の写しと、RC のモデルとの対応の証明 (spec の「Verus」)。

use vstd::prelude::*;

verus! {

pub(crate) const LOCAL_MAX: i32 = i32::MAX;

pub(crate) const DUP_LIMIT: i32 = i32::MIN + 0x1_0000;

/// 共有の `decref` が動かさない数の下限。`share` が張り付かせる値の基準にもなる
pub const DROP_LIMIT: i32 = i32::MIN + 0x2_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DupStep {
    Store(i32),
    Shared,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecrefStep {
    Store(i32),
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
    if -rc < DROP_LIMIT { i32::MIN } else { (-rc) as i32 }
}

/// 負の数を符号なしで見ると `2^31` 以上になるので、本体の比較1回は負の数を速い道に入れない
proof fn lemma_negative_as_u32(rc: i32)
    by (bit_vector)
    ensures
        rc < 0 ==> rc as u32 >= 0x8000_0000u32,
{
}

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

pub(crate) fn mark_step(rc: i32) -> (r: Option<i32>)
    ensures
        rc <= 0 ==> r is None,
        rc > 0 ==> r == Some(mark_spec(rc)),
{
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

// RC のモデル (spec の「Verus」)。物体の状態は、rc、本当の参照の数、張り付きの旗である

pub struct Obj {
    pub rc: i32,
    pub count: nat,
    pub stuck: bool,
}

/// 1回の遷移の結果。`freed` は、ヒープがその遷移で物体を解放するか
pub struct Next {
    pub obj: Obj,
    pub freed: bool,
}

pub(crate) open spec fn magnitude(rc: i32) -> int {
    if rc < 0 { -rc } else { rc as int }
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
    let rc = if s.rc > 0 { mark_spec(s.rc) } else { s.rc };
    settle(rc, s.count, s.stuck, false)
}

// 定理。1回の遷移ごとに対応が保たれるので、任意の i32 から始めた任意の長さの列でも保たれる

proof fn lemma_start(rc: i32)
    requires
        rc != 0,
    ensures
        corresponds(start(rc)),
{
}

/// 共有の数を動かす枝は負の数の上でだけ選ばれるので、`rc - 1` と `rc + 1` は i32 の範囲に収まる
proof fn lemma_shared_steps_stay_in_range(rc: i32)
    ensures
        dup_spec(rc) is Shared ==> DUP_LIMIT <= rc < 0 && i32::MIN <= rc - 1,
        decref_spec(rc) is Shared ==> DROP_LIMIT <= rc < 0 && rc + 1 <= 0,
{
}

proof fn lemma_dup(s: Obj)
    requires
        corresponds(s),
    ensures
        corresponds(dup_next(s).obj),
        !dup_next(s).freed,
{
}

proof fn lemma_decref(s: Obj)
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

proof fn lemma_mark(s: Obj)
    requires
        corresponds(s),
    ensures
        corresponds(mark_next(s).obj),
        !mark_next(s).freed,
{
}

/// 不死の数 0 は、どの遷移でも動かず、解放もされない
proof fn lemma_immortal(s: Obj)
    requires
        s.rc == 0,
    ensures
        dup_next(s).obj.rc == 0 && !dup_next(s).freed,
        decref_next(s).obj.rc == 0 && !decref_next(s).freed,
        mark_next(s).obj.rc == 0 && !mark_next(s).freed,
{
}

} // verus!

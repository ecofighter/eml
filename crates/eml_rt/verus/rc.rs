//! `rc.rs` の `DROP_LIMIT` と `mark_step` の写し (spec の「Verus」)。

use vstd::prelude::*;

verus! {

/// 共有の `decref` が動かさない数の下限。`share` が張り付かせる値の基準にもなる
pub const DROP_LIMIT: i32 = i32::MIN + 0x2_0000;

/// `mark_step` の結果を値で書いたもの。`share_walk` の証明が、実装を開かずにこの値を使えるようにする
pub open spec fn mark_spec(rc: i32) -> i32 {
    if -rc < DROP_LIMIT { i32::MIN } else { (-rc) as i32 }
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

} // verus!

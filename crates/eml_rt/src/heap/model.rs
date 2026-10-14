//! ヒープの抽象のモデル (docs/spec/object-model.md の「`unsafe` の核」)。ブロックの状態、参照の数、ヒープの中の辺を
//! ghost の値で持つ。外の呼び出し側が持つ数えた参照の数は、参照の数から辺の数を引いた値である。

use vstd::prelude::*;

use crate::meta::Header;

verus! {

/// ブロックの状態。速いモードの空けたブロックは確保器に返すので、`Freed` は検査モードだけに現れる。
// 仕様だけで使う型。Verus の外のビルドでも型は残るので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) ghost enum BlockState {
    /// `alloc`、`alloc_immortal`、`con_at`、`reuse_in_place` が作り、呼ぶ側がペイロードを書いている途中の物体。
    Building,
    Live,
    /// `release_reuse` と `into_token` が作る token の箱。
    Token,
    /// 最後の参照を手放すと決め、解放の連鎖がまだ空けていない物体。
    Dying,
    Freed,
}

/// 辺の位置。親のヘッダの番地と、ペイロードの物理的な語の番号である。
pub(crate) type Pos = (usize, int);

/// ヒープの見方。キーはどれもブロックのヘッダの番地である。
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) ghost struct Model {
    pub(crate) state: Map<usize, BlockState>,
    /// 数えた参照の数 (ヒープの中の辺と、外の呼び出し側の参照の和)。不死の物体では使わない。
    pub(crate) refs: Map<usize, nat>,
    /// ヒープの中の辺。生きている物体の走査する語のうち、物体を指すものである。
    pub(crate) edges: Map<Pos, usize>,
    /// rc が張り付いたか (`rc.rs` の理想のモデルの旗)。
    pub(crate) stuck: Map<usize, bool>,
    /// 持ち主の worker。速いモードでも ghost の値として持つ。
    pub(crate) owner: Map<usize, u32>,
    /// 構築中の物体の、確保の後に書いた語。
    pub(crate) written: Map<usize, Set<int>>,
    /// token の箱と、それを使い回した構築中の物体の「残した」語。呼ぶ側がその参照を持つので、辺に数えない。
    pub(crate) kept: Set<Pos>,
    pub(crate) building: Option<usize>,
    /// 検査モードの token の表 (`Heap` の欄) の写し。項目は箱と、箱にしたときの世代の番号である。
    pub(crate) tokens: Seq<Option<(*mut Header, u32)>>,
}

/// `o` を指す辺の位置。
pub(crate) open spec fn targets(edges: Map<Pos, usize>, o: usize) -> Set<Pos> {
    edges.dom().filter(|p: Pos| edges[p] == o)
}

/// `o` を指す辺の数。
pub(crate) open spec fn indeg(edges: Map<Pos, usize>, o: usize) -> nat {
    targets(edges, o).len()
}

/// 辺を1つ足すと、その行き先の数だけが1つ増える。
pub(crate) proof fn lemma_indeg_insert(edges: Map<Pos, usize>, p: Pos, o: usize)
    requires
        !edges.dom().contains(p),
    ensures
        indeg(edges.insert(p, o), o) == indeg(edges, o) + 1,
        forall|x: usize| x != o ==> #[trigger] indeg(edges.insert(p, o), x) == indeg(edges, x),
{
    let e2 = edges.insert(p, o);
    assert(targets(e2, o) =~= targets(edges, o).insert(p));
    assert(!targets(edges, o).contains(p));
    assert forall|x: usize| x != o implies #[trigger] indeg(e2, x) == indeg(edges, x) by {
        assert(targets(e2, x) =~= targets(edges, x));
    }
}

/// 辺を1つ外すと、その行き先の数だけが1つ減る。
pub(crate) proof fn lemma_indeg_remove(edges: Map<Pos, usize>, p: Pos)
    requires
        edges.dom().contains(p),
    ensures
        indeg(edges.remove(p), edges[p]) + 1 == indeg(edges, edges[p]),
        forall|x: usize| x != edges[p] ==> #[trigger] indeg(edges.remove(p), x) == indeg(edges, x),
{
    let o = edges[p];
    let e2 = edges.remove(p);
    assert(targets(edges, o) =~= targets(e2, o).insert(p));
    assert(!targets(e2, o).contains(p));
    assert forall|x: usize| x != o implies #[trigger] indeg(e2, x) == indeg(edges, x) by {
        assert(targets(e2, x) =~= targets(edges, x));
    }
}

/// 辺の数が 0 なら、その物体を指す辺はない。
pub(crate) proof fn lemma_indeg_zero(edges: Map<Pos, usize>, o: usize, p: Pos)
    requires
        indeg(edges, o) == 0,
        edges.dom().contains(p),
    ensures
        edges[p] != o,
{
    if edges[p] == o {
        assert(targets(edges, o).contains(p));
        vstd::set_lib::lemma_set_empty_equivalency_len(targets(edges, o));
    }
}

/// 位置の重ならない辺を足すと、行き先ごとの数は和になる。
pub(crate) proof fn lemma_indeg_union(e1: Map<Pos, usize>, e2: Map<Pos, usize>, o: usize)
    requires
        e1.dom().disjoint(e2.dom()),
    ensures
        indeg(e1.union_prefer_right(e2), o) == indeg(e1, o) + indeg(e2, o),
{
    let u = e1.union_prefer_right(e2);
    assert(targets(u, o) =~= targets(e1, o) + targets(e2, o));
    vstd::set_lib::lemma_set_disjoint_lens(targets(e1, o), targets(e2, o));
}

/// 辺の数が正なら、その物体を指す辺がある。
pub(crate) proof fn lemma_indeg_witness(edges: Map<Pos, usize>, o: usize) -> (p: Pos)
    requires
        indeg(edges, o) > 0,
    ensures
        edges.dom().contains(p),
        edges[p] == o,
{
    let s = targets(edges, o);
    vstd::set_lib::lemma_set_empty_equivalency_len(s);
    s.choose()
}

/// 物体を指す辺があれば、辺の数は正である。
pub(crate) proof fn lemma_indeg_pos(edges: Map<Pos, usize>, p: Pos)
    requires
        edges.dom().contains(p),
    ensures
        indeg(edges, edges[p]) > 0,
{
    assert(targets(edges, edges[p]).contains(p));
    vstd::set_lib::lemma_set_empty_equivalency_len(targets(edges, edges[p]));
}

} // verus!

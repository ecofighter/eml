//! `share` (docs/spec/object-model.md の「`share`」)。たどりは物体のグラフ (`graph.rs`) の上で書く。

use std::marker::PhantomData;
use std::mem;
use vstd::prelude::*;

use super::Heap;
#[cfg(verus_only)]
use super::graph::{
    GView, edge, graph_of, is_path, lemma_graph_base, lemma_is_node, lemma_reach_child,
    lemma_reached_root, lemma_reached_weaken, lemma_same_frame_graph, lemma_shaped, node_ok,
    owners_ok, reached, same_frame, same_shape, shaped,
};
use super::graph::{Graph, Objects};
#[cfg(verus_only)]
use super::inv::{p_code, p_init, p_owner, p_scanned, prefix_recounted};
#[cfg(verus_only)]
use super::model::{BlockState, Model, Pos};
#[cfg(verus_only)]
use super::{Core, Env};
use crate::error::HeapError;
#[cfg(verus_only)]
use crate::meta::{BlockPerm, Header};
use crate::mode::Mode;
use crate::rc;
#[cfg(verus_only)]
use crate::rc::{Obj, lemma_mark, mark_spec, pinned};
use crate::word::Word;

verus! {

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareError {
    /// 生きていない物体に届いた。
    Dead,
    /// 所有を移す物体に2回届いた。
    LinReachedTwice,
    /// すでに共有の側にある `lin` の物体に届いた。
    LinShared,
}

// 仕様
/// 印付けと移し替えのたどりが展開するのは局所の物体だけである。不死の物体と共有の物体では、`lin` ビットによらず止まる
pub open spec fn expands<N>(g: GView<N>, n: N) -> bool {
    g.rc[n] > 0 && g.live[n]
}

pub open spec fn local<N>(g: GView<N>) -> spec_fn(N) -> bool {
    |n: N| expands(g, n)
}

/// どれかのスタックのたどりが展開しうる物体。局所の物体と、`lin` でない共有の物体である。検査モードの所有者だけの
/// たどりは、`lin` でない共有の物体の先にも進むためである
pub open spec fn passes<N>(g: GView<N>) -> spec_fn(N) -> bool {
    |n: N| g.rc[n] > 0 || (g.rc[n] < 0 && !g.lin[n])
}

/// 取り出しても印付けも移し替えもせず、誤りにもしない物体。`check_lin` なら、共有の `lin` の物体は `LinShared` に
/// なるので入らない
pub open spec fn stops<N>(g: GView<N>, check_lin: bool, n: N) -> bool {
    g.rc[n] <= 0 && !(check_lin && g.lin[n] && g.rc[n] < 0)
}

/// 根から、すでに共有の側にある `lin` の物体に届く。`check_lin` のたどりがそこに着けば `LinShared` を返す
pub open spec fn reaches_shared_lin<N>(g: GView<N>, root: N) -> bool {
    exists|n: N| #[trigger] reached(g, passes(g), root, n) && g.lin[n] && g.rc[n] < 0
}

/// 届く物体がどれも生きているという前提を除いた、正しいグラフの前提。
pub open spec fn sound<N>(g: GView<N>, root: N) -> bool {
    &&& shaped(
        g,
        root,
    )
    // 不死の lin の物体 (rc 0) と共有の lin の物体 (rc が負) には、この前提を課さない。たどりはそこで止まるか LinShared を返す
    &&& forall|n: N|
        g.nodes.contains(n) && #[trigger] g.lin[n] && g.rc[n] > 0 ==> g.rc[n]
            == 1
        // rc 1 の物体は、根か、生きている物体のただ1つの辺から1回だけ届く。rc は参照の数なので、正しいヒープでは成り立つ。
        // 生きていない物体 (検査モードの token の箱や空ける途中の物体) の語は、もう手渡した参照を指しうるので数えない
    &&& forall|p: N, k: int|
        g.nodes.contains(p) && g.live[p] && 0 <= k < g.children[p].len()
            && g.rc[#[trigger] g.children[p][k]] == 1 ==> g.children[p][k] != root
    &&& forall|p1: N, k1: int, p2: N, k2: int|
        g.nodes.contains(p1) && g.live[p1] && 0 <= k1 < g.children[p1].len() && g.nodes.contains(p2)
            && g.live[p2] && 0 <= k2 < g.children[p2].len() && g.rc[#[trigger] g.children[p1][k1]]
            == 1 && g.children[p1][k1] == #[trigger] g.children[p2][k2] ==> p1 == p2 && k1
            == k2
        // 印付けは共有の物体で止まるので、その先に印の付かない局所の物体が残らないようにする前提である
    &&& forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() && !g.lin[p] && g.rc[p] < 0 ==> (
        g.rc[#[trigger] g.children[p][k]] <= 0 || g.lin[g.children[p][k]])
}

pub open spec fn well_formed<N>(g: GView<N>, root: N) -> bool {
    &&& sound(g, root)
    &&& forall|n: N| #[trigger] reached(g, passes(g), root, n) ==> g.live[n]
}

/// 所有を移す物体。根からの道の上の `lin` の物体から先が、どれも rc 1 である
pub open spec fn moves<N>(g: GView<N>, root: N, n: N) -> bool {
    exists|path: Seq<N>, i: int|
        #![trigger is_path(g, local(g), root, path), g.lin[path[i]]]
        is_path(g, local(g), root, path) && path.last() == n && 0 <= i < path.len()
            && g.lin[path[i]] && forall|j: int|
            i <= j < path.len() ==> g.rc[#[trigger] path[j]] == 1
}

/// rc 1 か `lin` の局所の物体なら、印付けと移し替えのたどりからは届かない。
pub open spec fn aside<N>(g: GView<N>, root: N, n: N) -> bool {
    g.rc[n] > 0 && (g.lin[n] || g.rc[n] == 1) ==> !reached(g, local(g), root, n)
}

pub open spec fn shared_points_to_no_local<N>(g: GView<N>) -> bool {
    forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() && !g.lin[p] && g.rc[p] < 0 ==> (
        g.rc[#[trigger] g.children[p][k]] <= 0 || g.lin[g.children[p][k]])
}

/// 1つの節点の、たどりの後の rc と持ち主。所有を移した物体は rc がそのままで持ち主が `receiver` になり、届いたほかの
/// 局所の物体には印が付く。届かない物体は rc が変わらず、持ち主は検査モードでだけ `receiver` になりうる
pub open spec fn node_post<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    checked: bool,
    n: N,
) -> bool {
    if reached(g0, local(g0), root, n) && g0.rc[n] > 0 {
        if moves(g0, root, n) {
            g.rc[n] == g0.rc[n] && g.owner[n] == receiver
        } else {
            g.rc[n] == mark_spec(g0.rc[n]) && g.owner[n] == g0.owner[n]
        }
    } else {
        &&& g.rc[n] == g0.rc[n]
        &&& g.owner[n] == g0.owner[n] || (checked && g0.rc[n] > 0 && g.owner[n] == receiver)
    }
}

/// たどりが変えるのは、局所の物体の印と、局所の物体の持ち主だけである。
pub open spec fn marked_only<N>(g0: GView<N>, g: GView<N>, receiver: u32) -> bool {
    forall|n: N| #[trigger]
        g0.nodes.contains(n) ==> {
            &&& g.rc[n] == g0.rc[n] || (g0.live[n] && g0.rc[n] > 0 && !g0.lin[n] && g.rc[n]
                == mark_spec(g0.rc[n]))
            &&& g.owner[n] == g0.owner[n] || (g0.live[n] && g0.rc[n] > 0 && g.owner[n] == receiver)
        }
}

// ループの不変条件の部品
pub open spec fn count<N>(s: Seq<N>, x: N) -> nat
    decreases s.len(),
{
    if s.len() == 0 {
        0
    } else {
        count(s.drop_last(), x) + if s.last() == x {
            1nat
        } else {
            0nat
        }
    }
}

pub open spec fn b2n(b: bool) -> nat {
    if b {
        1
    } else {
        0
    }
}

/// たどりの途中の状態。3つのスタックと、印を付けた物体、所有を移した物体、所有者だけのたどりが訪れた物体の集まりである
// 仕様だけで使う型。Verus の外のビルドでも型は残るので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
#[verifier::reject_recursive_types(N)]
pub ghost struct Walk<N> {
    pub mark: Seq<N>,
    pub moves: Seq<N>,
    pub own: Seq<N>,
    pub moved: Set<N>,
    pub marked: Set<N>,
    pub passed: Set<N>,
}

impl<N> Walk<N> {
    /// 印付けか移し替えで展開した物体。
    pub open spec fn done(self) -> Set<N> {
        self.moved.union(self.marked)
    }

    pub open spec fn stacked(self, n: N) -> bool {
        self.mark.contains(n) || self.moves.contains(n)
    }

    pub open spec fn pending(self, n: N) -> nat {
        count(self.mark, n) + count(self.moves, n)
    }

    pub open spec fn has_top(self, from_move: bool) -> bool {
        if from_move {
            self.moves.len() > 0
        } else {
            self.mark.len() > 0
        }
    }

    pub open spec fn top(self, from_move: bool) -> N {
        if from_move {
            self.moves.last()
        } else {
            self.mark.last()
        }
    }

    pub open spec fn pop(self, from_move: bool) -> Self {
        if from_move {
            Walk { moves: self.moves.drop_last(), ..self }
        } else {
            Walk { mark: self.mark.drop_last(), ..self }
        }
    }

    pub open spec fn pop_own(self) -> Self {
        Walk { own: self.own.drop_last(), ..self }
    }

    /// `n` を移し替えか印付けで展開した後。子は、移し替えなら移し替えの、印付けなら印付けのスタックに積む
    pub open spec fn expand(self, ch: Seq<N>, n: N, to_move: bool) -> Self {
        if to_move {
            Walk { moves: self.moves + ch, moved: self.moved.insert(n), ..self }
        } else {
            Walk { mark: self.mark + ch, marked: self.marked.insert(n), ..self }
        }
    }

    /// `n` を所有者だけのたどりで訪れた後。
    pub open spec fn visit(self, ch: Seq<N>, n: N) -> Self {
        Walk { own: self.own + ch, passed: self.passed.insert(n), ..self }
    }
}

/// 積み場に積まれたことのある物体。rc 1 の物体が1回しか積まれないことを数えるときの基準にする
pub open spec fn pushed<N>(g0: GView<N>, root: N, e: Set<N>, n: N) -> bool {
    n == root || exists|p: N, k: int|
        #![trigger e.contains(p), g0.children[p][k]]
        e.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == n
}

/// `s` のどれかの物体の子である。
pub open spec fn pushed_by<N>(g0: GView<N>, s: Set<N>, n: N) -> bool {
    exists|p: N, k: int|
        #![trigger s.contains(p), g0.children[p][k]]
        s.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == n
}

pub open spec fn node_inv<N>(g0: GView<N>, g: GView<N>, receiver: u32, w: Walk<N>, n: N) -> bool {
    &&& if w.marked.contains(n) {
        g0.rc[n] > 0 && !g0.lin[n] && g.rc[n] == mark_spec(g0.rc[n])
    } else {
        g.rc[n] == g0.rc[n]
    }
    &&& if w.moved.contains(n) {
        g.owner[n] == receiver
    } else {
        g.owner[n] == g0.owner[n] || (w.passed.contains(n) && g0.rc[n] > 0 && g.owner[n]
            == receiver)
    }
}

/// たどりの間いつも成り立つ不変条件。グラフの前提によらず、たどりが節点の中だけを動き、止まることを示す
pub open spec fn base_inv<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
) -> bool {
    &&& shaped(g0, root)
    &&& same_shape(g0, g)
    &&& w.moved.subset_of(g0.nodes)
    &&& w.marked.subset_of(g0.nodes)
    &&& w.passed.subset_of(g0.nodes)
    &&& forall|n: N| g0.nodes.contains(n) ==> #[trigger] node_inv(g0, g, receiver, w, n)
    &&& forall|i: int|
        0 <= i < w.mark.len() ==> g0.nodes.contains(#[trigger] w.mark[i]) && reached(
            g0,
            local(g0),
            root,
            w.mark[i],
        ) && (w.mark[i] == root || pushed_by(g0, w.marked, w.mark[i]))
    &&& forall|i: int|
        0 <= i < w.moves.len() ==> g0.nodes.contains(#[trigger] w.moves[i]) && reached(
            g0,
            local(g0),
            root,
            w.moves[i],
        ) && pushed_by(g0, w.moved, w.moves[i])
    &&& forall|i: int|
        0 <= i < w.own.len() ==> g0.nodes.contains(#[trigger] w.own[i]) && reached(
            g0,
            passes(g0),
            root,
            w.own[i],
        )
    &&& forall|n: N| #[trigger]
        w.moved.contains(n) ==> reached(g0, local(g0), root, n) && expands(g0, n) && g0.live[n]
    &&& forall|n: N| #[trigger]
        w.marked.contains(n) ==> reached(g0, local(g0), root, n) && expands(g0, n) && g0.live[n]
    &&& forall|n: N| #[trigger]
        w.passed.contains(n) ==> reached(g0, passes(g0), root, n) && g0.live[n] && (g0.rc[n] > 0
            || (g0.rc[n] < 0 && !g0.lin[n]))
    &&& check_lin ==> seen == w.moved.union(w.passed)
    &&& !check_lin ==> w.passed == Set::<N>::empty() && w.own.len() == 0
}

/// 正しいグラフで成り立つ不変条件。rc 1 の物体は1回しか積まれず、所有を移すかどうかは `moves` のとおりで、所有者だけの
/// たどりは印付けと移し替えのたどりが展開する物体に触れない
pub open spec fn inv<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
) -> bool {
    &&& base_inv(g0, g, root, receiver, check_lin, seen, w)
    &&& sound(g0, root)
    &&& forall|n: N| #[trigger]
        pushed(g0, root, w.done(), n) ==> (w.done().contains(n) || stops(g0, check_lin, n)
            || w.stacked(n))
    &&& forall|n: N|
        g0.nodes.contains(n) && #[trigger] g0.rc[n] == 1 ==> w.pending(n) + b2n(
            w.done().contains(n),
        ) == b2n(pushed(g0, root, w.done(), n))
    &&& forall|n: N| #[trigger] w.moved.contains(n) ==> moves(g0, root, n)
    &&& forall|n: N| #[trigger] w.marked.contains(n) ==> !moves(g0, root, n)
    &&& forall|i: int| 0 <= i < w.own.len() ==> aside(g0, root, #[trigger] w.own[i])
    &&& forall|n: N| #[trigger]
        w.passed.contains(n) ==> (g0.rc[n] < 0 && !g0.lin[n]) || (g0.rc[n] > 0 && (g0.lin[n]
            || g0.rc[n] == 1) && !reached(g0, local(g0), root, n))
}

// 補題
proof fn lemma_count_append<N>(a: Seq<N>, b: Seq<N>, x: N)
    ensures
        count(a + b, x) == count(a, x) + count(b, x),
    decreases b.len(),
{
    if b.len() == 0 {
        assert(a + b =~= a);
    } else {
        lemma_count_append(a, b.drop_last(), x);
        assert((a + b).drop_last() =~= a + b.drop_last());
    }
}

proof fn lemma_count_absent<N>(s: Seq<N>, x: N)
    requires
        !s.contains(x),
    ensures
        count(s, x) == 0,
    decreases s.len(),
{
    if s.len() > 0 {
        assert(s.drop_last().len() < s.len());
        assert forall|i: int| 0 <= i < s.drop_last().len() implies s.drop_last()[i] != x by {
            assert(s[i] == s.drop_last()[i]);
        }
        lemma_count_absent(s.drop_last(), x);
        assert(s[s.len() - 1] == s.last());
    }
}

proof fn lemma_count_unique<N>(s: Seq<N>, x: N, k: int)
    requires
        0 <= k < s.len(),
        s[k] == x,
        forall|j: int| 0 <= j < s.len() && s[j] == x ==> j == k,
    ensures
        count(s, x) == 1,
    decreases s.len(),
{
    let t = s.drop_last();
    if k == s.len() - 1 {
        assert forall|i: int| 0 <= i < t.len() implies t[i] != x by {
            assert(t[i] == s[i]);
        }
        lemma_count_absent(t, x);
    } else {
        assert(t[k] == s[k]);
        assert forall|j: int| 0 <= j < t.len() && t[j] == x implies j == k by {
            assert(t[j] == s[j]);
        }
        lemma_count_unique(t, x, k);
        assert(s.last() == s[s.len() - 1]);
    }
}

proof fn lemma_contains_drop_last<N>(s: Seq<N>, n: N)
    requires
        s.len() > 0,
        s.contains(n),
        n != s.last(),
    ensures
        s.drop_last().contains(n),
{
    let i = choose|i: int| 0 <= i < s.len() && s[i] == n;
    assert(i != s.len() - 1);
    assert(s.drop_last()[i] == n);
}

proof fn lemma_last_counted<N>(s: Seq<N>)
    requires
        s.len() > 0,
    ensures
        count(s, s.last()) >= 1,
        forall|m: N| m != s.last() ==> #[trigger] count(s, m) == count(s.drop_last(), m),
{
}

proof fn lemma_mark_negative(rc: i32)
    requires
        rc > 0,
    ensures
        mark_spec(rc) < 0,
        mark_spec(rc) == -1 ==> rc == 1,
{
}

/// 局所の物体だけを通る道で届く物体には、たどりのどれかのスタックが届く。
proof fn lemma_reached_linked<N>(g: GView<N>, root: N, n: N)
    requires
        reached(g, local(g), root, n),
    ensures
        reached(g, passes(g), root, n),
{
    assert forall|m: N| #[trigger] local(g)(m) implies passes(g)(m) by {}
    lemma_reached_weaken(g, local(g), passes(g), root, n);
}

/// 止まったときに、届く物体がどれも積まれたことがあると言うために、経路を根から順にたどって示す
proof fn lemma_path_pushed<N>(g0: GView<N>, root: N, e: Set<N>, path: Seq<N>, i: int)
    requires
        forall|m: N| #[trigger] pushed(g0, root, e, m) ==> (e.contains(m) || !expands(g0, m)),
        is_path(g0, local(g0), root, path),
        0 <= i < path.len(),
    ensures
        pushed(g0, root, e, path[i]),
    decreases i,
{
    if i > 0 {
        lemma_path_pushed(g0, root, e, path, i - 1);
        let p = path[i - 1];
        assert(local(g0)(p) && edge(g0, p, path[i]));
        let k = choose|k: int|
            0 <= k < g0.children[p].len() && #[trigger] g0.children[p][k] == path[i];
        assert(e.contains(p));
        assert(pushed(g0, root, e, path[i]));
    }
}

proof fn lemma_reached_pushed<N>(g0: GView<N>, root: N, e: Set<N>, n: N)
    requires
        forall|m: N| #[trigger] pushed(g0, root, e, m) ==> (e.contains(m) || !expands(g0, m)),
        reached(g0, local(g0), root, n),
    ensures
        pushed(g0, root, e, n),
{
    let path = choose|path: Seq<N>| #[trigger]
        is_path(g0, local(g0), root, path) && path.last() == n;
    lemma_path_pushed(g0, root, e, path, path.len() - 1);
}

/// 所有を移す物体は rc 1 である。
proof fn lemma_moves_rc<N>(g: GView<N>, root: N, n: N)
    requires
        moves(g, root, n),
    ensures
        g.rc[n] == 1,
{
    let (path, i) = choose|path: Seq<N>, i: int|
        #![trigger is_path(g, local(g), root, path), g.lin[path[i]]]
        is_path(g, local(g), root, path) && path.last() == n && 0 <= i < path.len()
            && g.lin[path[i]] && forall|j: int|
            i <= j < path.len() ==> g.rc[#[trigger] path[j]] == 1;
    assert(g.rc[path[path.len() - 1]] == 1);
}

/// 局所の物体だけを通る道で届く局所の `lin` の物体は、所有を移す物体である。
proof fn lemma_moves_lin<N>(g: GView<N>, root: N, n: N)
    requires
        sound(g, root),
        reached(g, local(g), root, n),
        g.nodes.contains(n),
        g.lin[n],
        g.rc[n] > 0,
    ensures
        moves(g, root, n),
{
    let path = choose|path: Seq<N>| #[trigger] is_path(g, local(g), root, path) && path.last() == n;
    let i = path.len() - 1;
    assert(path[i] == n);
    assert forall|j: int| i <= j < path.len() implies g.rc[#[trigger] path[j]] == 1 by {
        assert(j == i);
    }
    assert(g.lin[path[i]]);
}

/// 所有を移す物体の rc 1 の子も、所有を移す物体である。
proof fn lemma_moves_child<N>(g: GView<N>, root: N, p: N, k: int)
    requires
        moves(g, root, p),
        g.nodes.contains(p),
        g.live[p],
        0 <= k < g.children[p].len(),
        g.rc[g.children[p][k]] == 1,
    ensures
        moves(g, root, g.children[p][k]),
{
    let (path, i) = choose|path: Seq<N>, i: int|
        #![trigger is_path(g, local(g), root, path), g.lin[path[i]]]
        is_path(g, local(g), root, path) && path.last() == p && 0 <= i < path.len()
            && g.lin[path[i]] && forall|j: int|
            i <= j < path.len() ==> g.rc[#[trigger] path[j]] == 1;
    let c = g.children[p][k];
    assert(g.rc[path[path.len() - 1]] == 1);
    let longer = path.push(c);
    assert forall|j: int| 0 <= j < longer.len() - 1 implies g.nodes.contains(#[trigger] longer[j])
        && local(g)(longer[j]) && edge(g, longer[j], longer[j + 1]) by {
        if j < path.len() - 1 {
            assert(longer[j] == path[j] && longer[j + 1] == path[j + 1]);
        } else {
            assert(longer[j] == p && longer[j + 1] == c);
        }
    }
    assert(is_path(g, local(g), root, longer));
    assert forall|j: int| i <= j < longer.len() implies g.rc[#[trigger] longer[j]] == 1 by {
        if j < path.len() {
            assert(longer[j] == path[j]);
        }
    }
    assert(longer[i] == path[i]);
    assert(g.lin[longer[i]]);
}

/// 所有を移す物体への道を1つ短くした道の終わりも、所有を移す物体である。
proof fn lemma_moves_prefix<N>(g: GView<N>, root: N, path: Seq<N>, i: int)
    requires
        is_path(g, local(g), root, path),
        path.len() >= 2,
        0 <= i < path.len() - 1,
        g.lin[path[i]],
        forall|j: int| i <= j < path.len() ==> g.rc[#[trigger] path[j]] == 1,
    ensures
        moves(g, root, path[path.len() - 2]),
{
    let prefix = path.drop_last();
    assert forall|j: int| 0 <= j < prefix.len() - 1 implies g.nodes.contains(#[trigger] prefix[j])
        && local(g)(prefix[j]) && edge(g, prefix[j], prefix[j + 1]) by {
        assert(prefix[j] == path[j] && prefix[j + 1] == path[j + 1]);
    }
    assert(is_path(g, local(g), root, prefix));
    assert forall|j: int| i <= j < prefix.len() implies g.rc[#[trigger] prefix[j]] == 1 by {
        assert(prefix[j] == path[j]);
    }
    assert(prefix[i] == path[i]);
    assert(prefix.last() == path[path.len() - 2]);
}

/// 所有を移す `lin` でない物体の親は、所有を移す物体である。rc 1 の物体を指す辺はただ1つなので、道の上の親が
/// その親である
proof fn lemma_moves_parent<N>(g: GView<N>, root: N, p: N, k: int)
    requires
        sound(g, root),
        g.nodes.contains(p),
        g.live[p],
        0 <= k < g.children[p].len(),
        moves(g, root, g.children[p][k]),
        !g.lin[g.children[p][k]],
    ensures
        moves(g, root, p),
{
    let c = g.children[p][k];
    let (path, i) = choose|path: Seq<N>, i: int|
        #![trigger is_path(g, local(g), root, path), g.lin[path[i]]]
        is_path(g, local(g), root, path) && path.last() == c && 0 <= i < path.len()
            && g.lin[path[i]] && forall|j: int|
            i <= j < path.len() ==> g.rc[#[trigger] path[j]] == 1;
    let n = path.len();
    assert(path[n - 1] == c);
    assert(i < n - 1);
    assert(g.rc[path[n - 1]] == 1);
    let q = path[n - 2];
    assert(g.nodes.contains(q) && edge(g, q, path[n - 1]));
    let k2 = choose|k2: int| 0 <= k2 < g.children[q].len() && #[trigger] g.children[q][k2] == c;
    lemma_one_parent(g, root, q, k2, p, k);
    lemma_moves_prefix(g, root, path, i);
}

/// rc 1 の物体を指す、生きている物体の辺はただ1つである。
proof fn lemma_one_parent<N>(g: GView<N>, root: N, p1: N, k1: int, p2: N, k2: int)
    requires
        sound(g, root),
        g.nodes.contains(p1),
        g.nodes.contains(p2),
        g.live[p1],
        g.live[p2],
        0 <= k1 < g.children[p1].len(),
        0 <= k2 < g.children[p2].len(),
        g.children[p1][k1] == g.children[p2][k2],
        g.rc[g.children[p1][k1]] == 1,
    ensures
        p1 == p2,
        k1 == k2,
{
}

/// `lin` でない根の所有は移さない。rc 1 の物体を指す辺は根に向かないので、根に着く道は根1つだけである
proof fn lemma_moves_root<N>(g: GView<N>, root: N)
    requires
        sound(g, root),
        !g.lin[root],
    ensures
        !moves(g, root, root),
{
    if moves(g, root, root) {
        let (path, i) = choose|path: Seq<N>, i: int|
            #![trigger is_path(g, local(g), root, path), g.lin[path[i]]]
            is_path(g, local(g), root, path) && path.last() == root && 0 <= i < path.len()
                && g.lin[path[i]] && forall|j: int|
                i <= j < path.len() ==> g.rc[#[trigger] path[j]] == 1;
        let n = path.len();
        assert(path[n - 1] == root);
        assert(i < n - 1);
        assert(g.rc[path[n - 1]] == 1);
        let q = path[n - 2];
        assert(g.nodes.contains(path[n - 2]) && edge(g, path[n - 2], path[n - 1]));
        let k2 = choose|k2: int|
            0 <= k2 < g.children[q].len() && #[trigger] g.children[q][k2] == root;
        assert(g.rc[g.children[q][k2]] == 1);
    }
}

/// 根から局所の物体だけを通る道で届かない物体か共有の物体の子は、rc 1 か `lin` の局所の物体なら、やはり届かない。
/// その子を指す辺はこの親からの1つだけだからである
proof fn lemma_aside_child<N>(g: GView<N>, root: N, x: N, k: int)
    requires
        sound(g, root),
        g.nodes.contains(x),
        g.live[x],
        0 <= k < g.children[x].len(),
        g.rc[x] < 0 || !reached(g, local(g), root, x),
    ensures
        aside(g, root, g.children[x][k]),
{
    let c = g.children[x][k];
    assert(g.nodes.contains(c));
    if g.rc[c] > 0 && (g.lin[c] || g.rc[c] == 1) && reached(g, local(g), root, c) {
        assert(g.rc[c] == 1);
        assert(g.children[x][k] != root);
        let path = choose|path: Seq<N>| #[trigger]
            is_path(g, local(g), root, path) && path.last() == c;
        let n = path.len();
        assert(n > 1) by {
            if n == 1 {
                assert(path[0] == root);
            }
        }
        let q = path[n - 2];
        assert(g.nodes.contains(path[n - 2]) && local(g)(path[n - 2]) && edge(
            g,
            path[n - 2],
            path[n - 1],
        ));
        let k2 = choose|k2: int| 0 <= k2 < g.children[q].len() && #[trigger] g.children[q][k2] == c;
        assert(g.children[q][k2] == g.children[x][k]);
        assert(q == x);
        let prefix = path.drop_last();
        assert forall|j: int| 0 <= j < prefix.len() - 1 implies g.nodes.contains(
            #[trigger] prefix[j],
        ) && local(g)(prefix[j]) && edge(g, prefix[j], prefix[j + 1]) by {
            assert(prefix[j] == path[j] && prefix[j + 1] == path[j + 1]);
        }
        assert(is_path(g, local(g), root, prefix));
        assert(prefix.last() == x);
    }
}

/// どのたどりも、変えたのは印と持ち主だけである。
proof fn lemma_marked<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
)
    requires
        base_inv(g0, g, root, receiver, check_lin, seen, w),
    ensures
        marked_only(g0, g, receiver),
{
    assert forall|n: N| #[trigger] g0.nodes.contains(n) implies {
        &&& g.rc[n] == g0.rc[n] || (g0.live[n] && g0.rc[n] > 0 && !g0.lin[n] && g.rc[n]
            == mark_spec(g0.rc[n]))
        &&& g.owner[n] == g0.owner[n] || (g0.live[n] && g0.rc[n] > 0 && g.owner[n] == receiver)
    } by {
        assert(node_inv(g0, g, receiver, w, n));
    }
}

/// 取り出して印付けも移し替えもしない物体は、印を付けたか止まる物体である。
proof fn lemma_skippable<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
    x: N,
)
    requires
        base_inv(g0, g, root, receiver, check_lin, seen, w),
        g0.nodes.contains(x),
        g.rc[x] <= 0,
        !(check_lin && g0.lin[x] && g.rc[x] < 0),
    ensures
        w.done().contains(x) || stops(g0, check_lin, x),
        !w.marked.contains(x) ==> g0.rc[x] == g.rc[x],
{
    assert(node_inv(g0, g, receiver, w, x));
}

/// スタックだけが違う2つの状態では、節点ごとの不変条件は同じである。
proof fn lemma_same_sets<N>(g0: GView<N>, g: GView<N>, receiver: u32, w1: Walk<N>, w2: Walk<N>)
    requires
        w1.moved == w2.moved,
        w1.marked == w2.marked,
        w1.passed == w2.passed,
        forall|n: N| g0.nodes.contains(n) ==> #[trigger] node_inv(g0, g, receiver, w1, n),
    ensures
        forall|n: N| g0.nodes.contains(n) ==> #[trigger] node_inv(g0, g, receiver, w2, n),
{
    assert forall|n: N| g0.nodes.contains(n) implies #[trigger] node_inv(
        g0,
        g,
        receiver,
        w2,
        n,
    ) by {
        assert(node_inv(g0, g, receiver, w1, n));
    }
}

/// 印付けか移し替えのスタックから取り出したとき。
proof fn lemma_base_pop<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
    from_move: bool,
)
    requires
        base_inv(g0, g, root, receiver, check_lin, seen, w),
        w.has_top(from_move),
    ensures
        base_inv(g0, g, root, receiver, check_lin, seen, w.pop(from_move)),
        g0.nodes.contains(w.top(from_move)),
        reached(g0, local(g0), root, w.top(from_move)),
        reached(g0, passes(g0), root, w.top(from_move)),
        from_move ==> pushed_by(g0, w.moved, w.top(from_move)),
        !from_move ==> w.top(from_move) == root || pushed_by(g0, w.marked, w.top(from_move)),
{
    let w2 = w.pop(from_move);
    lemma_same_sets(g0, g, receiver, w, w2);
    if from_move {
        let s = w.moves;
        assert(s[s.len() - 1] == w.top(from_move));
        assert forall|i: int| 0 <= i < w2.moves.len() implies g0.nodes.contains(
            #[trigger] w2.moves[i],
        ) && reached(g0, local(g0), root, w2.moves[i]) && pushed_by(g0, w2.moved, w2.moves[i]) by {
            assert(w2.moves[i] == s[i]);
        }
    } else {
        let s = w.mark;
        assert(s[s.len() - 1] == w.top(from_move));
        assert forall|i: int| 0 <= i < w2.mark.len() implies g0.nodes.contains(
            #[trigger] w2.mark[i],
        ) && reached(g0, local(g0), root, w2.mark[i]) && (w2.mark[i] == root || pushed_by(
            g0,
            w2.marked,
            w2.mark[i],
        )) by {
            assert(w2.mark[i] == s[i]);
        }
    }
    lemma_reached_linked(g0, root, w.top(from_move));
}

/// 所有者だけのスタックから取り出したとき。
proof fn lemma_base_pop_own<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
)
    requires
        base_inv(g0, g, root, receiver, check_lin, seen, w),
        w.own.len() > 0,
    ensures
        base_inv(g0, g, root, receiver, check_lin, seen, w.pop_own()),
        g0.nodes.contains(w.own.last()),
        reached(g0, passes(g0), root, w.own.last()),
{
    let s = w.own;
    let w2 = w.pop_own();
    lemma_same_sets(g0, g, receiver, w, w2);
    assert(s[s.len() - 1] == s.last());
    assert forall|i: int| 0 <= i < w2.own.len() implies g0.nodes.contains(#[trigger] w2.own[i])
        && reached(g0, passes(g0), root, w2.own[i]) by {
        assert(w2.own[i] == s[i]);
    }
}

/// 子を積んだ物体の集まりが広がっても、子であることは変わらない。
proof fn lemma_pushed_by_grow<N>(g0: GView<N>, s: Set<N>, x: N, n: N)
    requires
        pushed_by(g0, s, n),
    ensures
        pushed_by(g0, s.insert(x), n),
{
    let (p, k) = choose|p: N, k: int|
        #![trigger s.contains(p), g0.children[p][k]]
        s.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == n;
    assert(s.insert(x).contains(p));
}

/// 取り出した物体 `x` を移し替えか印付けで展開したとき。`w` は取り出した後の状態である
proof fn lemma_base_expand<N>(
    g0: GView<N>,
    g1: GView<N>,
    g2: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s1: Set<N>,
    s2: Set<N>,
    w: Walk<N>,
    x: N,
    to_move: bool,
)
    requires
        base_inv(g0, g1, root, receiver, check_lin, s1, w),
        g0.nodes.contains(x),
        reached(g0, local(g0), root, x),
        g0.live[x],
        g1.rc[x] > 0,
        to_move ==> g2 == g1.with_owner(x, receiver),
        to_move ==> s2 == if check_lin {
            s1.insert(x)
        } else {
            s1
        },
        !to_move ==> !g0.lin[x] && g2 == g1.with_rc(x, mark_spec(g1.rc[x])) && s2 == s1,
    ensures
        base_inv(g0, g2, root, receiver, check_lin, s2, w.expand(g0.children[x], x, to_move)),
        !w.marked.contains(x),
        g0.rc[x] == g1.rc[x],
{
    let ch = g0.children[x];
    let w2 = w.expand(ch, x, to_move);
    assert(node_inv(g0, g1, receiver, w, x));
    if w.marked.contains(x) {
        lemma_mark_negative(g0.rc[x]);
    }
    assert(g2.rc.dom() =~= g0.rc.dom());
    assert(g2.owner.dom() =~= g0.owner.dom());
    assert forall|n: N| g0.nodes.contains(n) implies #[trigger] node_inv(
        g0,
        g2,
        receiver,
        w2,
        n,
    ) by {
        assert(node_inv(g0, g1, receiver, w, n));
    }
    assert forall|k: int| 0 <= k < ch.len() implies reached(
        g0,
        local(g0),
        root,
        #[trigger] ch[k],
    ) by {
        lemma_reach_child(g0, local(g0), root, x, k);
    }
    if to_move {
        assert forall|i: int| 0 <= i < w2.moves.len() implies g0.nodes.contains(
            #[trigger] w2.moves[i],
        ) && reached(g0, local(g0), root, w2.moves[i]) && pushed_by(g0, w2.moved, w2.moves[i]) by {
            if i < w.moves.len() {
                assert(w2.moves[i] == w.moves[i]);
                lemma_pushed_by_grow(g0, w.moved, x, w.moves[i]);
            } else {
                assert(w2.moves[i] == ch[i - w.moves.len()]);
                assert(w2.moved.contains(x) && g0.children[x][i - w.moves.len()] == w2.moves[i]);
            }
        }
        if check_lin {
            assert(s2 =~= w2.moved.union(w2.passed));
        }
    } else {
        assert forall|i: int| 0 <= i < w2.mark.len() implies g0.nodes.contains(
            #[trigger] w2.mark[i],
        ) && reached(g0, local(g0), root, w2.mark[i]) && (w2.mark[i] == root || pushed_by(
            g0,
            w2.marked,
            w2.mark[i],
        )) by {
            if i < w.mark.len() {
                assert(w2.mark[i] == w.mark[i]);
                if w.mark[i] != root {
                    lemma_pushed_by_grow(g0, w.marked, x, w.mark[i]);
                }
            } else {
                assert(w2.mark[i] == ch[i - w.mark.len()]);
                assert(w2.marked.contains(x) && g0.children[x][i - w.mark.len()] == w2.mark[i]);
            }
        }
    }
}

/// 所有者だけのたどりで `x` を訪れたとき。`w` は取り出した後の状態である
proof fn lemma_base_visit<N>(
    g0: GView<N>,
    g1: GView<N>,
    g2: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s1: Set<N>,
    s2: Set<N>,
    w: Walk<N>,
    x: N,
)
    requires
        base_inv(g0, g1, root, receiver, check_lin, s1, w),
        check_lin,
        g0.nodes.contains(x),
        reached(g0, passes(g0), root, x),
        g0.live[x],
        !s1.contains(x),
        (g1.rc[x] < 0 && !g0.lin[x]) || g1.rc[x] > 0,
        g2 == if g1.rc[x] > 0 {
            g1.with_owner(x, receiver)
        } else {
            g1
        },
        s2 == s1.insert(x),
    ensures
        base_inv(g0, g2, root, receiver, check_lin, s2, w.visit(g0.children[x], x)),
        !w.marked.contains(x) ==> g0.rc[x] == g1.rc[x],
{
    let ch = g0.children[x];
    let w2 = w.visit(ch, x);
    assert(node_inv(g0, g1, receiver, w, x));
    if w.marked.contains(x) {
        lemma_mark_negative(g0.rc[x]);
    }
    assert(passes(g0)(x));
    assert(g2.rc.dom() =~= g0.rc.dom());
    assert(g2.owner.dom() =~= g0.owner.dom());
    assert forall|n: N| g0.nodes.contains(n) implies #[trigger] node_inv(
        g0,
        g2,
        receiver,
        w2,
        n,
    ) by {
        assert(node_inv(g0, g1, receiver, w, n));
    }
    assert forall|i: int| 0 <= i < w2.own.len() implies g0.nodes.contains(#[trigger] w2.own[i])
        && reached(g0, passes(g0), root, w2.own[i]) by {
        if i < w.own.len() {
            assert(w2.own[i] == w.own[i]);
        } else {
            assert(w2.own[i] == ch[i - w.own.len()]);
            lemma_reach_child(g0, passes(g0), root, x, i - w.own.len());
        }
    }
    assert(s2 =~= w2.moved.union(w2.passed));
}

/// 印付けか移し替えのスタックから取り出した物体を、展開せずに捨てるとき。正しいグラフでは、その物体は rc 1 でない
proof fn lemma_pop_skip<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
    from_move: bool,
)
    requires
        inv(g0, g, root, receiver, check_lin, seen, w),
        w.has_top(from_move),
        w.done().contains(w.top(from_move)) || stops(g0, check_lin, w.top(from_move)),
    ensures
        inv(g0, g, root, receiver, check_lin, seen, w.pop(from_move)),
        g0.rc[w.top(from_move)] != 1,
{
    let x = w.top(from_move);
    let w2 = w.pop(from_move);
    lemma_base_pop(g0, g, root, receiver, check_lin, seen, w, from_move);
    let s = if from_move {
        w.moves
    } else {
        w.mark
    };
    lemma_last_counted(s);
    assert(s.last() == x);
    if g0.rc[x] == 1 {
        assert(w.pending(x) >= 1);
        assert(w.pending(x) + b2n(w.done().contains(x)) == b2n(pushed(g0, root, w.done(), x)));
    }
    assert(w2.done() == w.done());
    assert forall|m: N| #[trigger] pushed(g0, root, w2.done(), m) implies (w2.done().contains(m)
        || stops(g0, check_lin, m) || w2.stacked(m)) by {
        if m != x && s.contains(m) {
            lemma_contains_drop_last(s, m);
        }
    }
    assert forall|m: N| g0.nodes.contains(m) && #[trigger] g0.rc[m] == 1 implies w2.pending(m)
        + b2n(w2.done().contains(m)) == b2n(pushed(g0, root, w2.done(), m)) by {
        assert(m != x);
        assert(count(s, m) == count(s.drop_last(), m));
    }
}

/// 所有者だけのスタックから取り出したとき。
proof fn lemma_pop_own<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
)
    requires
        inv(g0, g, root, receiver, check_lin, seen, w),
        w.own.len() > 0,
    ensures
        inv(g0, g, root, receiver, check_lin, seen, w.pop_own()),
        aside(g0, root, w.own.last()),
{
    let s = w.own;
    let w2 = w.pop_own();
    lemma_base_pop_own(g0, g, root, receiver, check_lin, seen, w);
    assert(s[s.len() - 1] == s.last());
    assert forall|i: int| 0 <= i < w2.own.len() implies aside(g0, root, #[trigger] w2.own[i]) by {
        assert(w2.own[i] == s[i]);
    }
    assert(w2.done() == w.done());
    assert forall|m: N| #[trigger] pushed(g0, root, w2.done(), m) implies (w2.done().contains(m)
        || stops(g0, check_lin, m) || w2.stacked(m)) by {}
    assert forall|m: N| g0.nodes.contains(m) && #[trigger] g0.rc[m] == 1 implies w2.pending(m)
        + b2n(w2.done().contains(m)) == b2n(pushed(g0, root, w2.done(), m)) by {}
}

/// 印付けか移し替えのスタックの先頭の局所の物体は、正しいグラフでは、まだ展開しておらず、所有者だけのたどりも
/// 訪れていない
proof fn lemma_fresh<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
    from_move: bool,
)
    requires
        inv(g0, g, root, receiver, check_lin, seen, w),
        w.has_top(from_move),
        g.rc[w.top(from_move)] > 0,
    ensures
        !w.done().contains(w.top(from_move)),
        !w.passed.contains(w.top(from_move)),
        check_lin ==> !seen.contains(w.top(from_move)),
        g0.rc[w.top(from_move)] == g.rc[w.top(from_move)],
{
    let x = w.top(from_move);
    lemma_base_pop(g0, g, root, receiver, check_lin, seen, w, from_move);
    assert(node_inv(g0, g, receiver, w, x));
    if w.marked.contains(x) {
        lemma_mark_negative(g0.rc[x]);
    }
    let s = if from_move {
        w.moves
    } else {
        w.mark
    };
    lemma_last_counted(s);
    assert(s.last() == x);
    if g0.rc[x] == 1 {
        assert(w.pending(x) >= 1);
        assert(w.pending(x) + b2n(w.done().contains(x)) == b2n(pushed(g0, root, w.done(), x)));
    } else if w.moved.contains(x) {
        lemma_moves_rc(g0, root, x);
    }
}

/// スタックから `x` を取り出し、子 `ch` を積んだとき、積まれている数と、積まれているかがどう変わるか。
proof fn lemma_walk_counts<N>(w: Walk<N>, ch: Seq<N>, from_move: bool, to_move: bool)
    requires
        w.has_top(from_move),
    ensures
        forall|m: N| #[trigger]
            w.pop(from_move).expand(ch, w.top(from_move), to_move).pending(m) + b2n(
                m == w.top(from_move),
            ) == w.pending(m) + count(ch, m),
        forall|m: N|
            (w.stacked(m) && m != w.top(from_move)) || ch.contains(m) ==> #[trigger] w.pop(
                from_move,
            ).expand(ch, w.top(from_move), to_move).stacked(m),
{
    let x = w.top(from_move);
    let w1 = w.pop(from_move);
    let w2 = w1.expand(ch, x, to_move);
    let s = if from_move {
        w.moves
    } else {
        w.mark
    };
    let t = if to_move {
        w1.moves
    } else {
        w1.mark
    };
    lemma_last_counted(s);
    assert(s.last() == x);
    assert forall|m: N| #[trigger] w2.pending(m) + b2n(m == x) == w.pending(m) + count(ch, m) by {
        lemma_count_append(t, ch, m);
        if m == x {
            assert(count(s, m) == count(s.drop_last(), m) + 1) by {
                reveal_with_fuel(count, 1);
            }
        } else {
            assert(count(s, m) == count(s.drop_last(), m));
        }
    }
    assert forall|m: N| (w.stacked(m) && m != x) || ch.contains(m) implies #[trigger] w2.stacked(
        m,
    ) by {
        if ch.contains(m) {
            let j = choose|j: int| 0 <= j < ch.len() && ch[j] == m;
            assert((t + ch)[t.len() + j] == m);
        } else {
            if s.contains(m) {
                lemma_contains_drop_last(s, m);
            }
            if t.contains(m) {
                let j = choose|j: int| 0 <= j < t.len() && t[j] == m;
                assert((t + ch)[j] == m);
            }
        }
    }
}

/// 取り出した物体を移し替えるかどうかは、`moves` のとおりである。
proof fn lemma_expand_class<N>(
    g0: GView<N>,
    g1: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s1: Set<N>,
    w: Walk<N>,
    from_move: bool,
    to_move: bool,
)
    requires
        inv(g0, g1, root, receiver, check_lin, s1, w),
        w.has_top(from_move),
        g1.rc[w.top(from_move)] > 0,
        to_move == (g0.lin[w.top(from_move)] || (from_move && g1.rc[w.top(from_move)] == 1)),
    ensures
        to_move == moves(g0, root, w.top(from_move)),
{
    let x = w.top(from_move);
    lemma_fresh(g0, g1, root, receiver, check_lin, s1, w, from_move);
    lemma_base_pop(g0, g1, root, receiver, check_lin, s1, w, from_move);
    if to_move {
        if g0.lin[x] {
            lemma_moves_lin(g0, root, x);
        } else {
            let (p, k) = choose|p: N, k: int|
                #![trigger w.moved.contains(p), g0.children[p][k]]
                w.moved.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == x;
            lemma_moves_child(g0, root, p, k);
        }
    } else if moves(g0, root, x) {
        lemma_moves_rc(g0, root, x);
        if x == root {
            lemma_moves_root(g0, root);
        } else {
            let (p, k) = choose|p: N, k: int|
                #![trigger w.marked.contains(p), g0.children[p][k]]
                w.marked.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == x;
            lemma_moves_parent(g0, root, p, k);
        }
    }
}

/// 展開した物体 `x` の子を積んだ後も、積まれたことのある物体はスタックにあるか展開したか止まる物体で、rc 1 の物体は
/// 1回だけ積まれる。rc 1 の子を指す辺はただ1つなので、その子はまだ積まれていない
proof fn lemma_expand_count<N>(
    g0: GView<N>,
    root: N,
    check_lin: bool,
    w: Walk<N>,
    w2: Walk<N>,
    x: N,
)
    requires
        sound(g0, root),
        g0.nodes.contains(x),
        g0.live[x],
        w.done().subset_of(g0.nodes),
        forall|p: N| #[trigger] w.done().contains(p) ==> g0.live[p],
        !w.done().contains(x),
        w2.done() == w.done().insert(x),
        forall|m: N| #[trigger]
            pushed(g0, root, w.done(), m) ==> (w.done().contains(m) || stops(g0, check_lin, m)
                || w.stacked(m)),
        forall|m: N|
            g0.nodes.contains(m) && #[trigger] g0.rc[m] == 1 ==> w.pending(m) + b2n(
                w.done().contains(m),
            ) == b2n(pushed(g0, root, w.done(), m)),
        forall|m: N| #[trigger]
            w2.pending(m) + b2n(m == x) == w.pending(m) + count(g0.children[x], m),
        forall|m: N|
            (w.stacked(m) && m != x) || g0.children[x].contains(m) ==> #[trigger] w2.stacked(m),
        w.pending(x) >= 1,
    ensures
        forall|m: N| #[trigger]
            pushed(g0, root, w2.done(), m) ==> (w2.done().contains(m) || stops(g0, check_lin, m)
                || w2.stacked(m)),
        forall|m: N|
            g0.nodes.contains(m) && #[trigger] g0.rc[m] == 1 ==> w2.pending(m) + b2n(
                w2.done().contains(m),
            ) == b2n(pushed(g0, root, w2.done(), m)),
{
    let ch = g0.children[x];
    let e = w.done();
    let e2 = w2.done();
    assert forall|m: N| #[trigger] pushed(g0, root, e2, m) implies (e2.contains(m) || stops(
        g0,
        check_lin,
        m,
    ) || w2.stacked(m)) by {
        if m != root {
            let (p, k) = choose|p: N, k: int|
                #![trigger e2.contains(p), g0.children[p][k]]
                e2.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == m;
            if p == x {
                assert(ch[k] == m);
                assert(ch.contains(m));
            } else {
                assert(e.contains(p));
                assert(pushed(g0, root, e, m));
            }
        } else {
            assert(pushed(g0, root, e, m));
        }
    }
    assert forall|m: N| g0.nodes.contains(m) && #[trigger] g0.rc[m] == 1 implies w2.pending(m)
        + b2n(e2.contains(m)) == b2n(pushed(g0, root, e2, m)) by {
        assert(w.pending(m) + b2n(e.contains(m)) == b2n(pushed(g0, root, e, m)));
        assert(w2.pending(m) + b2n(m == x) == w.pending(m) + count(ch, m));
        if pushed(g0, root, e, m) {
            if m != root {
                let (p, k) = choose|p: N, k: int|
                    #![trigger e.contains(p), g0.children[p][k]]
                    e.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == m;
                assert(e2.contains(p) && g0.children[p][k] == m);
            }
            assert(pushed(g0, root, e2, m));
        }
        if ch.contains(m) {
            let k = choose|k: int| 0 <= k < ch.len() && ch[k] == m;
            assert(g0.children[x][k] == m);
            // rc 1 の m を指す辺はただ1つなので、積まれるのはその1回だけである
            assert forall|j: int| 0 <= j < ch.len() && ch[j] == m implies j == k by {
                assert(g0.children[x][j] == g0.children[x][k]);
            }
            lemma_count_unique(ch, m, k);
            assert(!pushed(g0, root, e, m)) by {
                assert(m != root);
                if pushed(g0, root, e, m) {
                    let (p, k2) = choose|p: N, k2: int|
                        #![trigger e.contains(p), g0.children[p][k2]]
                        e.contains(p) && 0 <= k2 < g0.children[p].len() && g0.children[p][k2] == m;
                    assert(g0.children[p][k2] == g0.children[x][k]);
                }
            }
            assert(m != x);
            assert(pushed(g0, root, e2, m)) by {
                assert(e2.contains(x) && g0.children[x][k] == m);
            }
        } else {
            lemma_count_absent(ch, m);
            if pushed(g0, root, e2, m) && m != root {
                let (p, k) = choose|p: N, k: int|
                    #![trigger e2.contains(p), g0.children[p][k]]
                    e2.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == m;
                if p == x {
                    assert(ch[k] == m);
                }
                assert(e.contains(p) && g0.children[p][k] == m);
                assert(pushed(g0, root, e, m));
            }
            if m == x {
                assert(pushed(g0, root, e, x));
                assert(pushed(g0, root, e2, x)) by {
                    if x != root {
                        let (p, k) = choose|p: N, k: int|
                            #![trigger e.contains(p), g0.children[p][k]]
                            e.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k]
                                == x;
                        assert(e2.contains(p) && g0.children[p][k] == x);
                    }
                }
            }
        }
    }
}

/// 取り出した物体 `x` を移し替えか印付けで展開したとき。所有を移すかどうかは `moves` のとおりで、rc 1 の子は
/// まだ積まれていないので、1回だけ積まれる
proof fn lemma_expand<N>(
    g0: GView<N>,
    g1: GView<N>,
    g2: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s1: Set<N>,
    s2: Set<N>,
    w: Walk<N>,
    from_move: bool,
    to_move: bool,
)
    requires
        inv(g0, g1, root, receiver, check_lin, s1, w),
        w.has_top(from_move),
        g0.live[w.top(from_move)],
        g1.rc[w.top(from_move)] > 0,
        to_move == (g0.lin[w.top(from_move)] || (from_move && g1.rc[w.top(from_move)] == 1)),
        to_move ==> g2 == g1.with_owner(w.top(from_move), receiver),
        to_move ==> s2 == if check_lin {
            s1.insert(w.top(from_move))
        } else {
            s1
        },
        !to_move ==> g2 == g1.with_rc(w.top(from_move), mark_spec(g1.rc[w.top(from_move)])) && s2
            == s1,
    ensures
        inv(
            g0,
            g2,
            root,
            receiver,
            check_lin,
            s2,
            w.pop(from_move).expand(g0.children[w.top(from_move)], w.top(from_move), to_move),
        ),
{
    let x = w.top(from_move);
    let ch = g0.children[x];
    let w1 = w.pop(from_move);
    let w2 = w1.expand(ch, x, to_move);
    lemma_fresh(g0, g1, root, receiver, check_lin, s1, w, from_move);
    lemma_base_pop(g0, g1, root, receiver, check_lin, s1, w, from_move);
    lemma_base_expand(g0, g1, g2, root, receiver, check_lin, s1, s2, w1, x, to_move);
    lemma_expand_class(g0, g1, root, receiver, check_lin, s1, w, from_move, to_move);
    assert forall|n: N| #[trigger] w2.moved.contains(n) implies moves(g0, root, n) by {
        if n != x {
            assert(w.moved.contains(n));
        }
    }
    assert forall|n: N| #[trigger] w2.marked.contains(n) implies !moves(g0, root, n) by {
        if n != x {
            assert(w.marked.contains(n));
        }
    }
    lemma_walk_counts(w, ch, from_move, to_move);
    assert(w2.done() =~= w.done().insert(x));
    let s = if from_move {
        w.moves
    } else {
        w.mark
    };
    lemma_last_counted(s);
    assert(s.last() == x);
    lemma_expand_count(g0, root, check_lin, w, w2, x);
    assert forall|i: int| 0 <= i < w2.own.len() implies aside(g0, root, #[trigger] w2.own[i]) by {
        assert(w2.own[i] == w.own[i]);
    }
    assert forall|n: N| #[trigger] w2.passed.contains(n) implies (g0.rc[n] < 0 && !g0.lin[n]) || (
    g0.rc[n] > 0 && (g0.lin[n] || g0.rc[n] == 1) && !reached(g0, local(g0), root, n)) by {
        assert(w.passed.contains(n));
    }
}

/// 所有者だけのたどりで `x` を訪れたとき。`w` は取り出した後の状態である。訪れる物体と積む子は、印付けと移し替えの
/// たどりが展開する物体でない
proof fn lemma_visit<N>(
    g0: GView<N>,
    g1: GView<N>,
    g2: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s1: Set<N>,
    s2: Set<N>,
    w: Walk<N>,
    x: N,
)
    requires
        inv(g0, g1, root, receiver, check_lin, s1, w),
        check_lin,
        g0.nodes.contains(x),
        reached(g0, passes(g0), root, x),
        g0.live[x],
        !s1.contains(x),
        (g1.rc[x] == -1 && !g0.lin[x] && (g0.rc[x] != 1 || aside(g0, root, x))) || (g1.rc[x] > 0
            && (g0.lin[x] || g1.rc[x] == 1) && aside(g0, root, x)),
        g2 == if g1.rc[x] > 0 {
            g1.with_owner(x, receiver)
        } else {
            g1
        },
        s2 == s1.insert(x),
    ensures
        inv(g0, g2, root, receiver, check_lin, s2, w.visit(g0.children[x], x)),
{
    let ch = g0.children[x];
    let w2 = w.visit(ch, x);
    lemma_base_visit(g0, g1, g2, root, receiver, check_lin, s1, s2, w, x);
    assert(node_inv(g0, g1, receiver, w, x));
    if w.marked.contains(x) {
        lemma_mark_negative(g0.rc[x]);
    }
    assert(!w.marked.contains(x));
    assert((g0.rc[x] < 0 && !g0.lin[x]) || (g0.rc[x] > 0 && (g0.lin[x] || g0.rc[x] == 1)
        && !reached(g0, local(g0), root, x)));
    assert forall|i: int| 0 <= i < w2.own.len() implies aside(g0, root, #[trigger] w2.own[i]) by {
        if i < w.own.len() {
            assert(w2.own[i] == w.own[i]);
        } else {
            assert(w2.own[i] == ch[i - w.own.len()]);
            lemma_aside_child(g0, root, x, i - w.own.len());
        }
    }
    assert(w2.done() == w.done());
    assert forall|m: N| #[trigger] pushed(g0, root, w2.done(), m) implies (w2.done().contains(m)
        || stops(g0, check_lin, m) || w2.stacked(m)) by {}
    assert forall|m: N| g0.nodes.contains(m) && #[trigger] g0.rc[m] == 1 implies w2.pending(m)
        + b2n(w2.done().contains(m)) == b2n(pushed(g0, root, w2.done(), m)) by {}
}

/// たどりが止まったとき。スタックが空なので、局所の物体だけを通る道で届く局所の物体は、どれも展開済みである
proof fn lemma_finish<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    seen: Set<N>,
    w: Walk<N>,
)
    requires
        inv(g0, g, root, receiver, check_lin, seen, w),
        w.mark.len() == 0,
        w.moves.len() == 0,
    ensures
        forall|n: N|
            g0.nodes.contains(n) ==> #[trigger] node_post(g0, g, root, receiver, check_lin, n),
        shared_points_to_no_local(g),
{
    let e = w.done();
    assert forall|m: N| #[trigger] pushed(g0, root, e, m) implies (e.contains(m) || !expands(
        g0,
        m,
    )) by {
        assert(!w.stacked(m));
        assert(e.contains(m) || stops(g0, check_lin, m));
    }
    assert forall|n: N| g0.nodes.contains(n) implies #[trigger] node_post(
        g0,
        g,
        root,
        receiver,
        check_lin,
        n,
    ) by {
        assert(node_inv(g0, g, receiver, w, n));
        if reached(g0, local(g0), root, n) && g0.rc[n] > 0 {
            lemma_reached_pushed(g0, root, e, n);
            assert(e.contains(n));
        }
        if w.passed.contains(n) {
            assert(!check_lin ==> w.passed == Set::<N>::empty());
        }
    }
    assert forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() && !g.lin[p] && g.rc[p] < 0 implies (
    g.rc[#[trigger] g.children[p][k]] <= 0 || g.lin[g.children[p][k]]) by {
        let c = g0.children[p][k];
        assert(g0.nodes.contains(c));
        assert(node_inv(g0, g, receiver, w, p));
        assert(node_inv(g0, g, receiver, w, c));
        if w.marked.contains(p) {
            assert(e.contains(p));
            assert(pushed(g0, root, e, c));
            if w.marked.contains(c) {
                lemma_mark_negative(g0.rc[c]);
            } else if w.moved.contains(c) && !g0.lin[c] {
                lemma_moves_parent(g0, root, p, k);
            }
        }
    }
}

/// 前提が満たせることの確かめ。満たすグラフがなければ、`share_walk` の結論は何も言わない。根 0 が局所の
/// `lin` の子 1 を指すグラフで示す
proof fn lemma_well_formed_is_satisfiable() {
    let g = GView::<int> {
        nodes: set![0int, 1int],
        live: map![0int => true, 1int => true],
        rc: map![0int => 1i32, 1int => 1i32],
        lin: map![0int => false, 1int => true],
        owner: map![0int => 0u32, 1int => 0u32],
        children: map![0int => seq![1int], 1int => Seq::<int>::empty()],
    };
    assert(g.live.dom() =~= g.nodes);
    assert(g.rc.dom() =~= g.nodes);
    assert(g.lin.dom() =~= g.nodes);
    assert(g.owner.dom() =~= g.nodes);
    assert(g.children.dom() =~= g.nodes);
    assert(g.children[0int][0] == 1int);
    assert forall|n: int| #[trigger] reached(g, passes(g), 0int, n) implies g.live[n] by {
        let path = choose|path: Seq<int>| #[trigger]
            is_path(g, passes(g), 0int, path) && path.last() == n;
        if path.len() > 1 {
            let i = path.len() - 2;
            assert(g.nodes.contains(path[i]) && edge(g, path[i], path[i + 1]));
            let k = choose|k: int|
                0 <= k < g.children[path[i]].len() && #[trigger] g.children[path[i]][k] == path[i
                    + 1];
            assert(path[i] == 0int);
            assert(path[i + 1] == 1int);
        }
    }
    assert(well_formed(g, 0int));
}

// たどり
/// 根から届く物体を受け手に渡す形にする (docs/spec/object-model.md の「`share`」)。たどりは3つのスタックを持つ。
/// 印付けのスタック (根はここから始める) と移し替えのスタックから取り出した局所の物体は、`lin` の物体か、移し替えの
/// スタックから取り出した rc 1 の物体なら、印を付けずに所有者を `receiver` に移し、子を移し替えのスタックに積む。
/// ほかの局所の物体には印を付け、子を印付けのスタックに積む。不死の物体と共有の物体では止まる。
/// 検査モードは所有者だけのスタックも持つ。共有の `lin` でない rc -1 の物体からその下の `lin` の物体と rc 1 の物体へ
/// 進み、所有者を `receiver` にする。このスタックは rc を書かず、ほかの2つのスタックに積まないので、2つのモードの rc の
/// 変化は同じになる。
/// 届いた物体が生きていなければ `Dead` を返す。`check_lin` なら、すでに共有の `lin` の物体に届いたときは `LinShared`、
/// 所有を移す物体に2回届いたときは `LinReachedTwice` を返す。
// 取り出す前のスタックに名前を付けて証明するので、`while let` でなく `loop` と `match` で書く
#[verifier::spinoff_prover]
pub(crate) fn share_walk<G: Graph>(
    graph: &mut G,
    root: G::Node,
    receiver: u32,
    check_lin: bool,
    mark: &mut Vec<G::Node>,
    moves: &mut Vec<G::Node>,
    own: &mut Vec<G::Node>,
) -> (r: Result<(), ShareError>)
    requires
        shaped(old(graph).gv(), root),
        check_lin ==> old(graph).seen() == Set::<G::Node>::empty(),
        !check_lin ==> well_formed(old(graph).gv(), root),
    ensures
        same_shape(old(graph).gv(), final(graph).gv()),
        marked_only(old(graph).gv(), final(graph).gv(), receiver),
        final(graph).kept(*old(graph)),
        r == Err::<(), ShareError>(ShareError::Dead) ==> exists|n: G::Node| #[trigger]
            reached(old(graph).gv(), passes(old(graph).gv()), root, n) && old(
                graph,
            ).gv().nodes.contains(n) && !old(graph).gv().live[n],
        r == Err::<(), ShareError>(ShareError::LinShared) ==> check_lin && reaches_shared_lin(
            old(graph).gv(),
            root,
        ),
        sound(old(graph).gv(), root) ==> r != Err::<(), ShareError>(ShareError::LinReachedTwice),
        well_formed(old(graph).gv(), root) && !(check_lin && reaches_shared_lin(
            old(graph).gv(),
            root,
        )) ==> r is Ok,
        r is Ok && sound(old(graph).gv(), root) ==> forall|n: G::Node|
            old(graph).gv().nodes.contains(n) ==> #[trigger] node_post(
                old(graph).gv(),
                final(graph).gv(),
                root,
                receiver,
                check_lin,
                n,
            ),
        r is Ok && sound(old(graph).gv(), root) ==> shared_points_to_no_local(final(graph).gv()),
{
    let ghost g0 = graph.gv();
    let ghost start = *graph;
    proof {
        start.lemma_kept_refl();
    }
    mark.clear();
    moves.clear();
    own.clear();
    mark.push(root);
    let ghost mut w = Walk::<G::Node> {
        mark: mark@,
        moves: moves@,
        own: own@,
        moved: Set::empty(),
        marked: Set::empty(),
        passed: Set::empty(),
    };
    proof {
        reveal_with_fuel(count, 2);
        lemma_reached_root(g0, local(g0), root);
        assert(mark@[0] == root);
        assert(w.done() =~= Set::<G::Node>::empty());
        assert forall|n: G::Node|
            g0.nodes.contains(n) && #[trigger] g0.rc[n] == 1 implies w.pending(n) + b2n(
            w.done().contains(n),
        ) == b2n(pushed(g0, root, w.done(), n)) by {
            assert(mark@.drop_last() =~= Seq::<G::Node>::empty());
            assert(moves@ =~= Seq::<G::Node>::empty());
        }
        assert forall|n: G::Node| #[trigger] pushed(g0, root, w.done(), n) implies (
        w.done().contains(n) || stops(g0, check_lin, n) || w.stacked(n)) by {
            assert(mark@[0] == root);
        }
        if check_lin {
            assert(graph.seen() =~= w.moved.union(w.passed));
        }
    }
    loop
        invariant
            base_inv(g0, graph.gv(), root, receiver, check_lin, graph.seen(), w),
            sound(g0, root) ==> inv(g0, graph.gv(), root, receiver, check_lin, graph.seen(), w),
            !check_lin ==> well_formed(g0, root),
            g0 == old(graph).gv(),
            start == *old(graph),
            graph.kept(start),
            w.mark == mark@,
            w.moves == moves@,
            w.own == own@,
        ensures
            graph.kept(start),
            base_inv(g0, graph.gv(), root, receiver, check_lin, graph.seen(), w),
            sound(g0, root) ==> inv(g0, graph.gv(), root, receiver, check_lin, graph.seen(), w),
            w.mark.len() == 0,
            w.moves.len() == 0,
        decreases
                (g0.nodes.len() - w.marked.len()) + (g0.nodes.len() - w.moved.union(
                    w.passed,
                ).len()),
                w.mark.len() + w.moves.len() + w.own.len(),
    {
        let ghost w0 = w;
        let ghost g1 = graph.gv();
        let ghost s1 = graph.seen();
        proof {
            vstd::set_lib::lemma_len_subset(w.marked, g0.nodes);
            vstd::set_lib::lemma_len_subset(w.moved.union(w.passed), g0.nodes);
        }
        let mut from_move = false;
        let mut on_own = false;
        let node = match moves.pop() {
            Some(n) => {
                from_move = true;
                n
            },
            None => match mark.pop() {
                Some(n) => n,
                None => match own.pop() {
                    Some(n) => {
                        on_own = true;
                        n
                    },
                    None => break,
                },
            },
        };
        proof {
            if on_own {
                assert(own@ =~= w0.own.drop_last());
                lemma_base_pop_own(g0, g1, root, receiver, check_lin, s1, w0);
                w = w0.pop_own();
            } else {
                if from_move {
                    assert(moves@ =~= w0.moves.drop_last());
                } else {
                    assert(mark@ =~= w0.mark.drop_last());
                }
                lemma_base_pop(g0, g1, root, receiver, check_lin, s1, w0, from_move);
                w = w0.pop(from_move);
            }
            lemma_marked(g0, g1, root, receiver, check_lin, s1, w);
        }
        if !graph.live(node) {
            return Err(ShareError::Dead);
        }
        let rc = graph.rc(node);
        let lin = graph.lin(node);
        if !on_own && rc > 0 {
            let to_move = lin || (from_move && rc == 1);
            proof {
                if sound(g0, root) {
                    lemma_fresh(g0, g1, root, receiver, check_lin, s1, w0, from_move);
                }
            }
            if to_move {
                if check_lin {
                    let ghost before = *graph;
                    let fresh = graph.see(node);
                    proof {
                        graph.lemma_kept_trans(before, start);
                    }
                    if !fresh {
                        return Err(ShareError::LinReachedTwice);
                    }
                }
                let ghost before = *graph;
                graph.set_owner(node, receiver);
                proof {
                    graph.lemma_kept_trans(before, start);
                }
                graph.push_children(node, moves);
            } else {
                let ghost before = *graph;
                graph.set_rc(node, rc::mark_step(rc).unwrap());
                proof {
                    graph.lemma_kept_trans(before, start);
                }
                graph.push_children(node, mark);
            }
            proof {
                let ghost s2 = graph.seen();
                if sound(g0, root) {
                    lemma_expand(
                        g0,
                        g1,
                        graph.gv(),
                        root,
                        receiver,
                        check_lin,
                        s1,
                        s2,
                        w0,
                        from_move,
                        to_move,
                    );
                }
                lemma_base_expand(
                    g0,
                    g1,
                    graph.gv(),
                    root,
                    receiver,
                    check_lin,
                    s1,
                    s2,
                    w,
                    node,
                    to_move,
                );
                if to_move && !check_lin {
                    assert(sound(g0, root));
                }
                w = w.expand(g0.children[node], node, to_move);
                if to_move {
                    assert(w.moved.union(w.passed) =~= w0.moved.union(w0.passed).insert(node));
                }
                vstd::set_lib::lemma_len_subset(w.marked, g0.nodes);
                vstd::set_lib::lemma_len_subset(w.moved.union(w.passed), g0.nodes);
            }
        } else if lin && rc < 0 && check_lin {
            proof {
                assert(node_inv(g0, g1, receiver, w, node));
                assert(reached(g0, passes(g0), root, node) && g0.lin[node] && g0.rc[node] < 0);
            }
            return Err(ShareError::LinShared);
        } else if check_lin && (rc == -1 || (on_own && rc > 0 && (lin || rc == 1))) {
            // rc -1 の物体はここでは `lin` でない。共有の `lin` の物体は上で断った
            let ghost before = *graph;
            let fresh = graph.see(node);
            proof {
                graph.lemma_kept_trans(before, start);
            }
            if fresh {
                if rc > 0 {
                    let ghost before = *graph;
                    graph.set_owner(node, receiver);
                    proof {
                        graph.lemma_kept_trans(before, start);
                    }
                }
                graph.push_children(node, own);
                proof {
                    let ghost s2 = graph.seen();
                    if sound(g0, root) {
                        if on_own {
                            lemma_pop_own(g0, g1, root, receiver, check_lin, s1, w0);
                        } else {
                            lemma_skippable(g0, g1, root, receiver, check_lin, s1, w, node);
                            lemma_pop_skip(g0, g1, root, receiver, check_lin, s1, w0, from_move);
                        }
                        lemma_visit(g0, g1, graph.gv(), root, receiver, check_lin, s1, s2, w, node);
                    }
                    lemma_base_visit(
                        g0,
                        g1,
                        graph.gv(),
                        root,
                        receiver,
                        check_lin,
                        s1,
                        s2,
                        w,
                        node,
                    );
                    w = w.visit(g0.children[node], node);
                    assert(w.moved.union(w.passed) =~= w0.moved.union(w0.passed).insert(node));
                    vstd::set_lib::lemma_len_subset(w.moved.union(w.passed), g0.nodes);
                }
            } else {
                proof {
                    assert(graph.seen() =~= s1);
                    if sound(g0, root) {
                        if on_own {
                            lemma_pop_own(g0, g1, root, receiver, check_lin, s1, w0);
                        } else {
                            lemma_skippable(g0, g1, root, receiver, check_lin, s1, w, node);
                            lemma_pop_skip(g0, g1, root, receiver, check_lin, s1, w0, from_move);
                        }
                    }
                }
            }
        } else {
            proof {
                if sound(g0, root) {
                    if on_own {
                        lemma_pop_own(g0, g1, root, receiver, check_lin, s1, w0);
                    } else {
                        lemma_skippable(g0, g1, root, receiver, check_lin, s1, w, node);
                        lemma_pop_skip(g0, g1, root, receiver, check_lin, s1, w0, from_move);
                    }
                }
            }
        }
    }
    proof {
        if sound(g0, root) {
            lemma_finish(g0, graph.gv(), root, receiver, check_lin, graph.seen(), w);
        }
        lemma_marked(g0, graph.gv(), root, receiver, check_lin, graph.seen(), w);
    }
    Ok(())
}

/// たどりの後のヒープ。権限は rc の値と前置きの所有者の値だけが変わり、モデルは持ち主と張り付きの旗を替える。
pub(crate) proof fn lemma_shared_wf(s0: Core, c1: Core, receiver: u32)
    requires
        s0.full(),
        s0.model@.building is None,
        s0.quiet(),
        c1.pages == s0.pages,
        c1.env == s0.env,
        same_frame(s0, c1.perms@, s0.perms@, s0.checked()),
        marked_only(
            graph_of(s0, s0.perms@, s0.model@.owner, s0.checked()),
            graph_of(s0, c1.perms@, c1.model@.owner, s0.checked()),
            receiver,
        ),
        c1.model@.owner.dom() == s0.model@.owner.dom(),
        owners_ok(s0, c1.perms@, c1.model@.owner, s0.checked()),
        forall|a: usize|
            #![trigger c1.model@.owner[a]]
            !node_ok(s0, s0.perms@, s0.checked(), a) ==> c1.model@.owner[a] == s0.model@.owner[a],
        c1.model@.stuck.dom() == s0.dom(),
        forall|b: usize|
            #![trigger c1.model@.stuck[b]]
            s0.dom().contains(b) ==> c1.model@.stuck[b] == (s0.model@.stuck[b] || pinned(
                c1.rc_of(b),
            )),
        c1.model@ == (Model { owner: c1.model@.owner, stuck: c1.model@.stuck, ..s0.model@ }),
    ensures
        c1.full(),
{
    reveal(graph_of);
    reveal(Core::blocks_wf);
    reveal(Core::edges_wf);
    reveal(Core::refs_wf);
    reveal(Core::building_wf);
    let checked = s0.checked();
    let g0 = graph_of(s0, s0.perms@, s0.model@.owner, checked);
    let g1 = graph_of(s0, c1.perms@, c1.model@.owner, checked);
    lemma_same_frame_graph(s0, c1.perms@, s0.perms@, c1.model@.owner, s0.model@.owner, checked);
    assert(c1.dom() == s0.dom());
    assert(c1.model@.stuck.dom() =~= c1.dom());
    assert forall|b: usize| #[trigger] c1.dom().contains(b) implies c1.block_wf(b) && c1.counted_wf(
        b,
    ) by {
        assert(s0.block_wf(b));
        assert(s0.counted_wf(b));
        let x = c1.perm(b);
        let y = s0.perm(b);
        if node_ok(s0, s0.perms@, checked, b) {
            let n = s0.block(b);
            lemma_is_node(s0, s0.perms@, checked, b);
            assert(g0.nodes.contains(n));
            assert(n@.addr == b);
            assert(g1.rc[n] == x.rc.value());
            assert(g0.rc[n] == y.rc.value());
            if s0.state(b) == BlockState::Live {
                assert forall|i: int| #[trigger] p_scanned(x, i) implies c1.slot_word(x, b, i) by {
                    assert(p_scanned(y, i));
                    assert(s0.slot_word(y, b, i));
                }
            }
            if s0.state(b) == BlockState::Dying || s0.state(b) == BlockState::Token {
                assert forall|i: int| #[trigger] p_scanned(x, i) implies p_init(x, i) by {
                    assert(p_scanned(y, i));
                }
            }
            let rc0 = y.rc.value();
            if x.rc.value() != rc0 {
                let o = Obj { rc: rc0, count: s0.model@.refs[b], stuck: s0.model@.stuck[b] };
                lemma_mark(o);
                lemma_mark_negative(rc0);
            }
            assert(node_ok(s0, c1.perms@, checked, b));
            if x.prefix is Some {
                assert(p_code(x) == p_code(y));
                if x.rc.value() > 0 && (s0.state(b) == BlockState::Live || s0.state(b)
                    == BlockState::Building) {
                    assert(x.rc.value() == rc0);
                    assert(s0.rc_of(b) > 0);
                    assert(p_owner(x) == c1.model@.owner[b]);
                }
            }
            assert(c1.perm_wf(x, b, c1.state(b), c1.model@.written[b], c1.model@.owner[b]));
            assert(c1.block_wf(b));
            assert(c1.counted_wf(b));
        } else {
            assert(x == y);
            if s0.state(b) == BlockState::Live {
                assert forall|i: int| #[trigger] p_scanned(x, i) implies c1.slot_word(x, b, i) by {
                    assert(s0.slot_word(y, b, i));
                }
            }
            assert(c1.block_wf(b));
            assert(c1.counted_wf(b));
        }
    }
    assert forall|e: Pos| #[trigger] c1.model@.kept.contains(e) implies c1.kept_wf(e) by {
        assert(s0.kept_wf(e));
    }
    assert forall|e: Pos| #[trigger] c1.model@.edges.dom().contains(e) implies c1.edge_wf(e) by {
        assert(s0.edge_wf(e));
    }
}

impl<M: Mode> Heap<M> {
    /// たどりの始まり。預けた権限のグラフは、操作の前のヒープのグラフで、形の前提を満たす。`share` と `transfer_back`
    /// が使う。
    pub(super) proof fn lemma_share_start(
        &self,
        old: Heap<M>,
        mid: Heap<M>,
        o0: Objects<M>,
        h: *mut Header,
        w: Word,
    )
        requires
            old.wf(),
            w.is_ptr_spec(),
            old.word_in(w),
            !M::CHECKED ==> old.consistent() && old.settleable() && old.readable(w),
            mid.core == old.core.settled_view(),
            mid.wf(),
            mid.core.model@.building is None,
            h == mid.core.block(w.bits_spec()),
            h@.addr == w.bits_spec(),
            o0.base@ == mid.core,
            o0.env == mid.core.env,
            o0.perms@ == mid.core.perms@,
            o0.owner@ == mid.core.model@.owner,
        ensures
            shaped(o0.gv(), h),
            old.share_graph() == o0.gv(),
            old.share_root(w) == h,
            mid.core.has(h@.addr),
            old.consistent() && old.readable(w) ==> mid.core.state(h@.addr) == BlockState::Live,
    {
        reveal(Core::building_wf);
        let s0 = mid.core;
        assert(o0.view() == s0);
        assert(s0.has(h@.addr));
        lemma_shaped(s0, h@.addr);
    }

    /// たどりの後で、権限を戻してモデルを替えたヒープは不変条件を満たし、`shared` の状態にある。
    proof fn lemma_share_end(
        &self,
        old: Heap<M>,
        mid: Heap<M>,
        o0: Objects<M>,
        o1: Objects<M>,
        h: *mut Header,
        w: Word,
        receiver: u32,
        ok: bool,
        c: bool,
    )
        requires
            self.building_tied(),
            self.tokens@ == self.core.model@.tokens,
            old.wf(),
            !M::CHECKED ==> c,
            c ==> old.consistent() && mid.consistent() && mid.core.state(h@.addr)
                == BlockState::Live,
            mid.core == old.core.settled_view(),
            mid.wf(),
            mid.core.model@.building is None,
            mid.dummy == old.dummy,
            mid.region == old.region,
            mid.worker == old.worker,
            w.is_ptr_spec(),
            h == mid.core.block(w.bits_spec()),
            h@.addr == w.bits_spec(),
            mid.core.has(h@.addr),
            o0.base@ == mid.core,
            o0.env == mid.core.env,
            o0.perms@ == mid.core.perms@,
            o0.owner@ == mid.core.model@.owner,
            o1.kept(o0),
            same_shape(o0.gv(), o1.gv()),
            marked_only(o0.gv(), o1.gv(), receiver),
            ok && sound(o0.gv(), h) ==> forall|n: *mut Header|
                o0.gv().nodes.contains(n) ==> #[trigger] node_post(
                    o0.gv(),
                    o1.gv(),
                    h,
                    receiver,
                    M::CHECKED,
                    n,
                ),
            ok && sound(o0.gv(), h) ==> shared_points_to_no_local(o1.gv()),
            self.dummy == mid.dummy,
            self.region == mid.region,
            self.worker == mid.worker,
            self.core.pages == mid.core.pages,
            self.core.env == mid.core.env,
            self.core.perms@ == o1.perms@,
            self.core.model@ == (Model {
                owner: o1.owner@,
                stuck: self.core.model@.stuck,
                ..mid.core.model@
            }),
            self.core.model@.stuck.dom() == mid.core.dom(),
            forall|b: usize|
                #![trigger self.core.model@.stuck[b]]
                mid.core.dom().contains(b) ==> self.core.model@.stuck[b] == (
                mid.core.model@.stuck[b] || pinned(self.core.rc_of(b))),
        ensures
            self.wf(),
            c ==> self.consistent(),
            self.shared(old, w, receiver),
            ok ==> self.shared_all(old, w, receiver),
    {
        reveal(graph_of);
        let s0 = mid.core;
        let checked = M::CHECKED;
        assert(o0.view() == s0);
        assert(o1.view() == s0);
        lemma_shaped(s0, h@.addr);
        lemma_same_frame_graph(s0, o1.perms@, s0.perms@, o1.owner@, s0.model@.owner, checked);
        if c {
            reveal(Core::blocks_wf);
            assert(owners_ok(s0, s0.perms@, s0.model@.owner, checked)) by {
                assert forall|a: usize| #[trigger]
                    node_ok(s0, s0.perms@, checked, a) && s0.state(a) == BlockState::Live
                        && s0.rc_of(a) > 0 && s0.perms@[a].prefix is Some implies p_owner(
                    s0.perms@[a],
                ) == s0.model@.owner[a] by {
                    if s0.in_region(a) {
                        s0.lemma_region_rc(a);
                    }
                    assert(s0.block_wf(a));
                }
            }
            assert(self.core.model@.owner.dom() =~= s0.model@.owner.dom()) by {
                assert forall|a: usize| #[trigger]
                    self.core.model@.owner.dom().contains(a) implies s0.model@.owner.dom().contains(
                    a,
                ) by {
                    if !o0.owner@.dom().contains(a) {
                        assert(node_ok(s0, o0.perms@, checked, a));
                    }
                }
            }
            lemma_shared_wf(s0, self.core, receiver);
        }
        if checked {
            reveal(Core::safe_blocks_wf);
            assert forall|b: usize| #[trigger] s0.dom().contains(b) implies {
                &&& self.core.perm(b).rc.ptr() == s0.perm(b).rc.ptr()
                &&& self.core.perm(b).rc.is_init()
                &&& self.core.perm(b).meta == s0.perm(b).meta
                &&& self.core.perm(b).slots == s0.perm(b).slots
                &&& self.core.perm(b).text == s0.perm(b).text
                &&& prefix_recounted(self.core.perm(b), s0.perm(b), true)
            } by {
                assert(s0.safe_block_wf(b));
                assert(o1.perms@[b] == self.core.perms@[b]);
            }
            self.core.lemma_safe_values(s0);
        }
        lemma_graph_base(self.core, s0, self.core.perms@, self.core.model@.owner, checked);
        // ダミーの物体は不死なので、印も持ち主も変わらない
        let d = self.dummy.bits_spec();
        assert(self.core.word_ok(self.dummy) == s0.word_ok(self.dummy));
        if c {
            assert(s0.state(d) == BlockState::Live);
            lemma_is_node(s0, s0.perms@, checked, d);
            assert(s0.block(d)@.addr == d);
            assert(o1.gv().rc[s0.block(d)] == o0.gv().rc[s0.block(d)]);
            assert(self.core.rc_of(d) == 0);
            assert forall|b: usize| #[trigger] self.core.dom().contains(b) implies self.core.state(
                b,
            ) != BlockState::Dying by {
                assert(s0.dom().contains(b));
            }
        }
    }

    /// `share` の前提がみな成り立ち、モデルが合っている。検査モードでは、共有の側の `lin` の物体に届かないことも
    /// 前提である (`LinShared`)。
    pub closed spec fn share_all_ok(&self, w: Word) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.readable(w)
        &&& w.is_ptr_spec() ==> self.share_ok(w)
        &&& M::CHECKED && w.is_ptr_spec() ==> !self.reaches_shared_lin_from(w)
    }

    /// `share` と `transfer_back` のたどりが見る物体のグラフ。
    pub(super) open spec fn share_graph(&self) -> GView<*mut Header> {
        let v = self.core.settled_view();
        graph_of(v, v.perms@, v.model@.owner, v.checked())
    }

    /// `w` から届く物体は、`share` の前提 (`share_walk` の `well_formed`) を満たす。
    pub closed spec fn share_ok(&self, w: Word) -> bool {
        well_formed(self.share_graph(), self.core.block(w.bits_spec()))
    }

    /// `w` の物体の根。
    pub(super) open spec fn share_root(&self, w: Word) -> *mut Header {
        self.core.block(w.bits_spec())
    }

    /// `w` から、すでに共有の側にある `lin` の物体に届く。
    pub closed spec fn reaches_shared_lin_from(&self, w: Word) -> bool {
        reaches_shared_lin(self.share_graph(), self.share_root(w))
    }

    /// `w` から、生きていない物体に届く。
    pub closed spec fn reaches_dead_from(&self, w: Word) -> bool {
        exists|n: *mut Header| #[trigger]
            reached(self.share_graph(), passes(self.share_graph()), self.share_root(w), n)
                && self.share_graph().nodes.contains(n) && !self.share_graph().live[n]
    }

    /// `w` から届く物体は、生きていることのほかは `share` の前提を満たす。
    pub closed spec fn share_sound(&self, w: Word) -> bool {
        sound(self.share_graph(), self.share_root(w))
    }

    /// `share` の後の状態。変わったのは、局所の物体の共有の印と、局所の物体の持ち主だけである。
    pub closed spec fn shared(&self, old: Heap<M>, w: Word, receiver: u32) -> bool {
        let g0 = old.share_graph();
        let g = self.share_graph();
        &&& self.dummy == old.dummy
        &&& self.region == old.region
        &&& self.worker == old.worker
        &&& same_shape(g0, g)
        &&& marked_only(g0, g, receiver)
    }

    /// `share` が終わった後の状態。所有を移す物体の持ち主が `receiver` になり、届いたほかの局所の物体に共有の印が
    /// 付いた。
    pub closed spec fn shared_all(&self, old: Heap<M>, w: Word, receiver: u32) -> bool {
        let g0 = old.share_graph();
        let g = self.share_graph();
        let root = old.core.block(w.bits_spec());
        &&& sound(g0, root) ==> forall|n: *mut Header|
            g0.nodes.contains(n) ==> #[trigger] node_post(g0, g, root, receiver, M::CHECKED, n)
        &&& sound(g0, root) ==> shared_points_to_no_local(g)
    }

    /// w から届く物体を、`receiver` に渡す形にする。所有を移す物体の持ち主を `receiver` にし、ほかの局所の物体を
    /// 共有にする。送り手のスレッドが、値を公開する前に呼ぶ。
    ///
    /// # Safety
    /// `w` は即値か、このヒープか付けた塊の生きている物体を指し、そこから届く物体はどれも生きている。所有を移す物体
    /// (局所の `lin` の物体と、その下の rc 1 の物体) には1回だけ届き、共有の側の `lin` の物体には届かない。
    #[verifier::spinoff_prover]
    pub unsafe fn share(&mut self, w: Word, receiver: u32) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).share_all_ok(w),
        ensures
            !M::CHECKED ==> r is Ok,
            final(self).wf(),
            old(self).share_all_ok(w) ==> r is Ok && final(self).consistent(),
            w.is_ptr_spec() ==> final(self).shared(*old(self), w, receiver),
            w.is_ptr_spec() && r is Ok ==> final(self).shared_all(*old(self), w, receiver),
            w.is_ptr_spec() && r == Err::<(), HeapError>(HeapError::LinShared) ==> old(
                self,
            ).reaches_shared_lin_from(w),
            w.is_ptr_spec() && r == Err::<(), HeapError>(HeapError::UseAfterFree) ==> old(
                self,
            ).reaches_dead_from(w),
            w.is_ptr_spec() && old(self).share_sound(w) ==> r != Err::<(), HeapError>(
                HeapError::LinReachedTwice,
            ),
    {
        let ghost pre_ok = self.share_all_ok(w);
        self.admit_worker(receiver);
        if !w.is_ptr() {
            return Ok(());
        }
        self.settle();
        let ghost mid = *self;
        let h = self.header_of(w);
        let ghost s0 = self.core;
        let mut mark = Vec::new();
        mem::swap(&mut mark, &mut self.mark_stack);
        let mut moves = Vec::new();
        mem::swap(&mut moves, &mut self.move_stack);
        let mut own = Vec::new();
        mem::swap(&mut own, &mut self.own_stack);
        // 訪問済みの集まりは検査モードだけが使う。速いモードでは作らず、触れない
        let mut seen = None;
        if M::CHECKED {
            mem::swap(&mut seen, &mut self.seen);
            seen =
            match seen {
                Some(mut set) => {
                    set.clear();
                    Some(set)
                },
                None => None,
            };
        }
        let mut owned = Vec::new();
        if M::CHECKED {
            mem::swap(&mut owned, &mut self.owned);
        }
        let tracked mut perms = Map::<usize, BlockPerm>::tracked_empty();
        let tracked mut env = Env::new(M::CHECKED);
        proof {
            vstd::modes::tracked_swap(&mut perms, self.core.perms.borrow_mut());
            vstd::modes::tracked_swap(&mut env, self.core.env.borrow_mut());
        }
        let mut objects = Objects::<M> {
            perms: Tracked(perms),
            owned,
            seen,
            env: Tracked(env),
            owner: Ghost(s0.model@.owner),
            base: Ghost(s0),
            _mode: PhantomData,
        };
        let ghost o0 = objects;
        proof {
            self.lemma_share_start(*old(self), mid, o0, h, w);
            assert(objects.seen() =~= Set::<*mut Header>::empty());
            if !M::CHECKED {
                assert(old(self).share_ok(w));
            }
        }
        let result = share_walk(
            &mut objects,
            h,
            receiver,
            M::CHECKED,
            &mut mark,
            &mut moves,
            &mut own,
        );
        let ghost o1 = objects;
        proof {
            vstd::modes::tracked_swap(objects.perms.borrow_mut(), self.core.perms.borrow_mut());
            vstd::modes::tracked_swap(objects.env.borrow_mut(), self.core.env.borrow_mut());
            let c = self.core;
            self.core.model = Ghost(
                Model {
                    owner: o1.owner@,
                    stuck: Map::new(s0.dom(), |b: usize| s0.model@.stuck[b] || pinned(c.rc_of(b))),
                    ..s0.model@
                },
            );
            self.lemma_share_end(*old(self), mid, o0, o1, h, w, receiver, result is Ok, pre_ok);
        }
        mem::swap(&mut mark, &mut self.mark_stack);
        mem::swap(&mut moves, &mut self.move_stack);
        mem::swap(&mut own, &mut self.own_stack);
        if M::CHECKED {
            mem::swap(&mut objects.seen, &mut self.seen);
        }
        if M::CHECKED {
            mem::swap(&mut objects.owned, &mut self.owned);
        }
        match result {
            Ok(()) => Ok(()),
            Err(ShareError::Dead) => Err(HeapError::UseAfterFree),
            Err(ShareError::LinReachedTwice) => Err(HeapError::LinReachedTwice),
            Err(ShareError::LinShared) => Err(HeapError::LinShared),
        }
    }
}

} // verus!

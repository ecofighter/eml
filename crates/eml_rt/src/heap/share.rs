//! `share` (docs/spec/object-model.md の「`share`」)。たどりは物体への読み書きを
//! `Graph` の裏に置いて書き、Verus がたどりを trait のメソッドの約束の上で証明する。ヒープの上の `Graph` は
//! `Objects` で、たどりの間ヒープのブロックの権限を預かる。

use std::marker::PhantomData;
use std::mem;
use vstd::prelude::*;
use vstd::raw_ptr::ptr_ref;

use super::check::ALIVE_STATE;
#[cfg(verus_only)]
use super::inv::{
    p_code, p_count, p_extended, p_first, p_init, p_meta, p_owner, p_scanned, p_word,
    prefix_recounted, state_code,
};
#[cfg(verus_only)]
use super::model::{BlockState, Model, Pos};
use super::{Core, Env, Heap, count_owned, move_owned};
use crate::cell::{rc_load, rc_store, u32_load, u32_store};
use crate::error::HeapError;
use crate::meta::{
    BlockPerm, Header, Meta, meta_ptr, prefix_first_ptr, prefix_state_ptr, rc_ptr, slot_ptr,
};
#[cfg(verus_only)]
use crate::meta::{Kind, bits_lin, bits_wf};
use crate::mode::Mode;
use crate::rc;
#[cfg(verus_only)]
use crate::rc::{Obj, lemma_mark, mark_spec, pinned};
use crate::word::Word;

verus! {

/// グラフ全体を集合と写像で表した見方。trait のメソッドの約束を、この値の言葉で書くために置く
// 仕様だけで使う型。Verus の外のビルドでも型は残るので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
#[verifier::reject_recursive_types(N)]
pub ghost struct GView<N> {
    pub nodes: Set<N>,
    pub live: Map<N, bool>,
    pub rc: Map<N, i32>,
    pub lin: Map<N, bool>,
    pub owner: Map<N, u32>,
    pub children: Map<N, Seq<N>>,
}

impl<N> GView<N> {
    pub open spec fn with_rc(self, n: N, v: i32) -> Self {
        GView {
            nodes: self.nodes,
            live: self.live,
            rc: self.rc.insert(n, v),
            lin: self.lin,
            owner: self.owner,
            children: self.children,
        }
    }

    pub open spec fn with_owner(self, n: N, v: u32) -> Self {
        GView {
            nodes: self.nodes,
            live: self.live,
            rc: self.rc,
            lin: self.lin,
            owner: self.owner.insert(n, v),
            children: self.children,
        }
    }
}

/// 各メソッドの約束だけを頼りにたどりを証明するので、グラフの大きさにも実装にも依らない結論になる
pub(crate) trait Graph: Sized {
    type Node: Copy + PartialEq;

    spec fn gv(&self) -> GView<Self::Node>;

    /// たどりが変えない部分 (`gv` に現れない部分を含む) が `old` と同じである。
    spec fn kept(&self, old: Self) -> bool;

    proof fn lemma_kept_refl(&self)
        ensures
            self.kept(*self),
    ;

    proof fn lemma_kept_trans(&self, mid: Self, old: Self)
        requires
            mid.kept(old),
            self.kept(mid),
        ensures
            self.kept(old),
    ;

    /// 検査モードの前置きが生存を示すか。速いモードと、前置きのないグラフのモデルでは常に真である。
    fn live(&self, node: Self::Node) -> (r: bool)
        requires
            self.gv().nodes.contains(node),
        ensures
            r == self.gv().live[node],
    ;

    fn rc(&self, node: Self::Node) -> (r: i32)
        requires
            self.gv().nodes.contains(node),
        ensures
            r == self.gv().rc[node],
    ;

    fn set_rc(&mut self, node: Self::Node, rc: i32)
        requires
            old(self).gv().nodes.contains(node),
            old(self).gv().rc[node] != 0,
        ensures
            final(self).gv() == old(self).gv().with_rc(node, rc),
            final(self).kept(*old(self)),
    ;

    fn lin(&self, node: Self::Node) -> (r: bool)
        requires
            self.gv().nodes.contains(node),
        ensures
            r == self.gv().lin[node],
    ;

    fn set_owner(&mut self, node: Self::Node, owner: u32)
        requires
            old(self).gv().nodes.contains(node),
            old(self).gv().rc[node] != 0,
        ensures
            final(self).gv() == old(self).gv().with_owner(node, owner),
            final(self).kept(*old(self)),
    ;

    /// 子のうち物体を指すものを `out` に積む。
    fn push_children(&self, node: Self::Node, out: &mut Vec<Self::Node>)
        requires
            self.gv().nodes.contains(node),
            self.gv().live[node],
        ensures
            final(out)@ == old(out)@ + self.gv().children[node],
    ;

    /// 2つの節点が同じか。節点の型の `==` が値の等しさであることを信頼せずに済むように、比べ方を実装に任せ、結果を
    /// 仕様の等しさで約束させる
    fn same(&self, a: Self::Node, b: Self::Node) -> (r: bool)
        requires
            self.gv().nodes.contains(a),
            self.gv().nodes.contains(b),
        ensures
            r == (a == b),
    ;
}

/// 列に節点があるか。`<[T]>::contains` は vstd に約束がないので、`Graph::same` で比べるループで書く
fn contains_node<G: Graph>(graph: &G, s: &[G::Node], x: G::Node) -> (r: bool)
    requires
        graph.gv().nodes.contains(x),
        forall|j: int| 0 <= j < s@.len() ==> graph.gv().nodes.contains(#[trigger] s@[j]),
    ensures
        r == s@.contains(x),
{
    let mut i = 0;
    while i < s.len()
        invariant
            0 <= i <= s.len(),
            forall|j: int| 0 <= j < i ==> s@[j] != x,
            graph.gv().nodes.contains(x),
            forall|j: int| 0 <= j < s@.len() ==> graph.gv().nodes.contains(#[trigger] s@[j]),
        decreases s.len() - i,
    {
        if graph.same(s[i], x) {
            return true;
        }
        i += 1;
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareError {
    /// 生きていない物体に届いた。
    Dead,
    /// 同じ `lin` の物体に2回届いた。
    LinReachedTwice,
    /// すでに共有の側にある `lin` の物体に届いた。
    LinShared,
}

// 仕様
/// たどりが展開するのは局所の物体だけである。不死の物体と共有の物体では、`lin` ビットによらず止まる
pub open spec fn expands<N>(g: GView<N>, n: N) -> bool {
    g.rc[n] > 0
}

/// 取り出しても展開せず、誤りにもしない物体。`check_lin` なら、共有の `lin` の物体は `LinShared` になるので入らない
pub open spec fn stops<N>(g: GView<N>, check_lin: bool, n: N) -> bool {
    g.rc[n] <= 0 && !(check_lin && g.lin[n] && g.rc[n] < 0)
}

/// 根から、すでに共有の側にある `lin` の物体に届く。`check_lin` のたどりは、このときちょうど `LinShared` を返す
pub open spec fn reaches_shared_lin<N>(g: GView<N>, root: N) -> bool {
    exists|n: N| #[trigger] reached(g, root, n) && g.lin[n] && g.rc[n] < 0
}

/// グラフの形。根と子はどれも節点で、各写像の定義域は節点の集合である。たどりが節点の外に触れないための前提である
pub open spec fn shaped<N>(g: GView<N>, root: N) -> bool {
    &&& g.nodes.contains(root)
    &&& g.live.dom() == g.nodes
    &&& g.rc.dom() == g.nodes
    &&& g.lin.dom() == g.nodes
    &&& g.owner.dom() == g.nodes
    &&& g.children.dom() == g.nodes
    &&& forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() ==> g.nodes.contains(
            #[trigger] g.children[p][k],
        )
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
        // 局所の lin の物体は根かただ1つの辺から1回だけ届く。2回届く入力は LinReachedTwice になるので、この証明の対象に入れない
    &&& forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() && g.lin[#[trigger] g.children[p][k]]
            && g.rc[g.children[p][k]] > 0 ==> g.children[p][k] != root
    &&& forall|p1: N, k1: int, p2: N, k2: int|
        g.nodes.contains(p1) && 0 <= k1 < g.children[p1].len() && g.nodes.contains(p2) && 0 <= k2
            < g.children[p2].len() && g.lin[#[trigger] g.children[p1][k1]]
            && g.rc[g.children[p1][k1]] > 0 && g.children[p1][k1] == #[trigger] g.children[p2][k2]
            ==> p1 == p2 && k1
            == k2
        // たどりは共有の物体で止まるので、その先に印の付かない局所の物体が残らないようにする前提である
    &&& forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() && !g.lin[p] && g.rc[p] < 0 ==> (
        g.rc[#[trigger] g.children[p][k]] <= 0 || g.lin[g.children[p][k]])
}

pub open spec fn well_formed<N>(g: GView<N>, root: N) -> bool {
    &&& sound(g, root)
    &&& forall|n: N| #[trigger] reached(g, root, n) ==> g.live[n]
}

pub open spec fn edge<N>(g: GView<N>, p: N, n: N) -> bool {
    exists|k: int| 0 <= k < g.children[p].len() && #[trigger] g.children[p][k] == n
}

/// 展開する物体だけを通る経路。不死の物体と共有の物体の先へは進まないので、届く物体はこの経路で定める
pub open spec fn is_path<N>(g: GView<N>, root: N, path: Seq<N>) -> bool {
    &&& path.len() > 0
    &&& path[0] == root
    &&& forall|i: int|
        0 <= i < path.len() - 1 ==> g.nodes.contains(#[trigger] path[i]) && expands(g, path[i])
            && edge(g, path[i], path[i + 1])
}

pub open spec fn reached<N>(g: GView<N>, root: N, n: N) -> bool {
    exists|path: Seq<N>| #[trigger] is_path(g, root, path) && path.last() == n
}

pub open spec fn shared_points_to_no_local<N>(g: GView<N>) -> bool {
    forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() && !g.lin[p] && g.rc[p] < 0 ==> (
        g.rc[#[trigger] g.children[p][k]] <= 0 || g.lin[g.children[p][k]])
}

pub open spec fn node_post<N>(g0: GView<N>, g: GView<N>, root: N, receiver: u32, n: N) -> bool {
    if reached(g0, root, n) && g0.lin[n] && g0.rc[n] > 0 {
        g.owner[n] == receiver && g.rc[n] == g0.rc[n]
    } else if reached(g0, root, n) && g0.rc[n] > 0 {
        g.rc[n] == mark_spec(g0.rc[n]) && g.owner[n] == g0.owner[n]
    } else {
        g.rc[n] == g0.rc[n] && g.owner[n] == g0.owner[n]
    }
}

/// たどりが変えるのは、局所の物体の印と、`lin` の物体の持ち主だけである。
pub open spec fn marked_only<N>(g0: GView<N>, g: GView<N>, receiver: u32) -> bool {
    forall|n: N| #[trigger]
        g0.nodes.contains(n) ==> {
            &&& g.rc[n] == g0.rc[n] || (g0.live[n] && g0.rc[n] > 0 && !g0.lin[n] && g.rc[n]
                == mark_spec(g0.rc[n]))
            &&& g.owner[n] == g0.owner[n] || (g0.live[n] && g0.lin[n] && g.owner[n] == receiver)
        }
}

pub open spec fn same_shape<N>(g0: GView<N>, g: GView<N>) -> bool {
    &&& g.nodes == g0.nodes
    &&& g.live == g0.live
    &&& g.lin == g0.lin
    &&& g.children == g0.children
    &&& g.rc.dom() == g0.rc.dom()
    &&& g.owner.dom() == g0.owner.dom()
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

/// 積み場に積まれたことのある物体。`lin` の物体が1回しか積まれないことを数えるときの基準にする
pub open spec fn pushed<N>(g0: GView<N>, root: N, e: Set<N>, n: N) -> bool {
    n == root || exists|p: N, k: int|
        #![trigger e.contains(p), g0.children[p][k]]
        e.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == n
}

pub open spec fn node_inv<N>(g0: GView<N>, g: GView<N>, receiver: u32, e: Set<N>, n: N) -> bool {
    if !e.contains(n) {
        g.rc[n] == g0.rc[n] && g.owner[n] == g0.owner[n]
    } else if g0.lin[n] {
        g.rc[n] == g0.rc[n] && g.owner[n] == receiver
    } else {
        g0.rc[n] > 0 && g.rc[n] == mark_spec(g0.rc[n]) && g.owner[n] == g0.owner[n]
    }
}

pub open spec fn b2n(b: bool) -> nat {
    if b {
        1
    } else {
        0
    }
}

/// たどりの間いつも成り立つ不変条件。グラフの前提によらず、たどりが節点の中だけを動き、止まることを示す
pub open spec fn base_inv<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    stack: Seq<N>,
    visited: Seq<N>,
    e: Set<N>,
) -> bool {
    &&& shaped(g0, root)
    &&& same_shape(g0, g)
    &&& e.subset_of(g0.nodes)
    &&& forall|n: N| g0.nodes.contains(n) ==> #[trigger] node_inv(g0, g, receiver, e, n)
    &&& forall|i: int|
        0 <= i < stack.len() ==> g0.nodes.contains(#[trigger] stack[i]) && reached(
            g0,
            root,
            stack[i],
        )
    &&& forall|n: N| #[trigger]
        e.contains(n) ==> reached(g0, root, n) && expands(g0, n) && g0.live[n]
    &&& check_lin ==> forall|n: N| #[trigger] visited.contains(n) ==> e.contains(n) && g0.lin[n]
    &&& check_lin ==> forall|n: N| #[trigger] e.contains(n) && g0.lin[n] ==> visited.contains(n)
}

pub open spec fn inv<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    stack: Seq<N>,
    visited: Seq<N>,
    e: Set<N>,
) -> bool {
    &&& base_inv(g0, g, root, receiver, check_lin, stack, visited, e)
    &&& sound(g0, root)
    &&& forall|n: N| #[trigger]
        pushed(g0, root, e, n) ==> (e.contains(n) || stops(g0, check_lin, n) || stack.contains(n))
    &&& forall|n: N|
        g0.nodes.contains(n) && #[trigger] g0.lin[n] && g0.rc[n] > 0 ==> count(stack, n) + b2n(
            e.contains(n),
        ) == b2n(pushed(g0, root, e, n))
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

proof fn lemma_reached_root<N>(g: GView<N>, root: N)
    ensures
        reached(g, root, root),
{
    let path = seq![root];
    assert(is_path(g, root, path));
}

/// 展開のたびに、積み場の物体がどれも届く物体であることを保つ
proof fn lemma_reach_child<N>(g: GView<N>, root: N, p: N, k: int)
    requires
        reached(g, root, p),
        g.nodes.contains(p),
        expands(g, p),
        0 <= k < g.children[p].len(),
    ensures
        reached(g, root, g.children[p][k]),
{
    let path = choose|path: Seq<N>| #[trigger] is_path(g, root, path) && path.last() == p;
    let c = g.children[p][k];
    let longer = path.push(c);
    assert forall|i: int| 0 <= i < longer.len() - 1 implies g.nodes.contains(#[trigger] longer[i])
        && expands(g, longer[i]) && edge(g, longer[i], longer[i + 1]) by {
        if i < path.len() - 1 {
            assert(longer[i] == path[i] && longer[i + 1] == path[i + 1]);
        } else {
            assert(longer[i] == p && longer[i + 1] == c);
        }
    }
    assert(is_path(g, root, longer));
}

/// 止まったときに、届く物体がどれも積まれたことがあると言うために、経路を根から順にたどって示す
proof fn lemma_path_pushed<N>(g0: GView<N>, root: N, e: Set<N>, path: Seq<N>, i: int)
    requires
        forall|m: N| #[trigger] pushed(g0, root, e, m) ==> (e.contains(m) || !expands(g0, m)),
        is_path(g0, root, path),
        0 <= i < path.len(),
    ensures
        pushed(g0, root, e, path[i]),
    decreases i,
{
    if i > 0 {
        lemma_path_pushed(g0, root, e, path, i - 1);
        let p = path[i - 1];
        assert(expands(g0, p) && edge(g0, p, path[i]));
        let k = choose|k: int|
            0 <= k < g0.children[p].len() && #[trigger] g0.children[p][k] == path[i];
        assert(e.contains(p));
        assert(pushed(g0, root, e, path[i]));
    }
}

proof fn lemma_reached_pushed<N>(g0: GView<N>, root: N, e: Set<N>, n: N)
    requires
        forall|m: N| #[trigger] pushed(g0, root, e, m) ==> (e.contains(m) || !expands(g0, m)),
        reached(g0, root, n),
    ensures
        pushed(g0, root, e, n),
{
    let path = choose|path: Seq<N>| #[trigger] is_path(g0, root, path) && path.last() == n;
    lemma_path_pushed(g0, root, e, path, path.len() - 1);
}

proof fn lemma_mark_negative(rc: i32)
    requires
        rc > 0,
    ensures
        mark_spec(rc) < 0,
{
}

proof fn lemma_last_counted<N>(s: Seq<N>)
    requires
        s.len() > 0,
    ensures
        count(s, s.last()) >= 1,
        forall|m: N| m != s.last() ==> #[trigger] count(s, m) == count(s.drop_last(), m),
{
}

/// 取り出した物体を展開しないとき。積み場の先頭を落とすだけで不変条件が保たれる
proof fn lemma_skip<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s: Seq<N>,
    visited: Seq<N>,
    e: Set<N>,
)
    requires
        inv(g0, g, root, receiver, check_lin, s, visited, e),
        s.len() > 0,
        !(g0.lin[s.last()] && g0.rc[s.last()] > 0),
        e.contains(s.last()) || stops(g0, check_lin, s.last()),
    ensures
        inv(g0, g, root, receiver, check_lin, s.drop_last(), visited, e),
{
    let t = s.drop_last();
    let node = s.last();
    lemma_last_counted(s);
    assert forall|i: int| 0 <= i < t.len() implies g0.nodes.contains(#[trigger] t[i]) && reached(
        g0,
        root,
        t[i],
    ) by {
        assert(t[i] == s[i]);
    }
    assert forall|m: N| #[trigger] pushed(g0, root, e, m) implies (e.contains(m) || stops(
        g0,
        check_lin,
        m,
    ) || t.contains(m)) by {
        if m != node && s.contains(m) {
            lemma_contains_drop_last(s, m);
        }
    }
}

/// 取り出した物体を展開するとき。`lin` の物体はただ1つの辺から1回だけ積まれるので、積み場にも訪問済みにも重複が入らない
proof fn lemma_expand<N>(
    g0: GView<N>,
    g1: GView<N>,
    g2: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s: Seq<N>,
    v1: Seq<N>,
    v2: Seq<N>,
    e: Set<N>,
)
    requires
        inv(g0, g1, root, receiver, check_lin, s, v1, e),
        s.len() > 0,
        !e.contains(s.last()),
        expands(g0, s.last()),
        g0.live[s.last()],
        g0.lin[s.last()] ==> g2 == g1.with_owner(s.last(), receiver),
        !g0.lin[s.last()] ==> g2 == g1.with_rc(s.last(), mark_spec(g0.rc[s.last()])),
        v2 == if check_lin && g0.lin[s.last()] {
            v1.push(s.last())
        } else {
            v1
        },
    ensures
        inv(
            g0,
            g2,
            root,
            receiver,
            check_lin,
            s.drop_last() + g0.children[s.last()],
            v2,
            e.insert(s.last()),
        ),
{
    let node = s.last();
    let t = s.drop_last();
    let ch = g0.children[node];
    let s2 = t + ch;
    let e2 = e.insert(node);
    assert(g0.nodes.contains(node)) by {
        assert(s[s.len() - 1] == node);
    }
    assert(reached(g0, root, node)) by {
        assert(s[s.len() - 1] == node);
    }
    lemma_last_counted(s);
    assert(g2.rc.dom() =~= g0.rc.dom());
    assert(g2.owner.dom() =~= g0.owner.dom());
    assert(same_shape(g0, g2));
    assert forall|n: N| g0.nodes.contains(n) implies #[trigger] node_inv(
        g0,
        g2,
        receiver,
        e2,
        n,
    ) by {
        if n != node {
            assert(node_inv(g0, g1, receiver, e, n));
        } else {
            assert(node_inv(g0, g1, receiver, e, n));
        }
    }
    assert forall|i: int| 0 <= i < s2.len() implies g0.nodes.contains(#[trigger] s2[i]) && reached(
        g0,
        root,
        s2[i],
    ) by {
        if i < t.len() {
            assert(s2[i] == s[i]);
        } else {
            assert(s2[i] == ch[i - t.len()]);
            lemma_reach_child(g0, root, node, i - t.len());
        }
    }
    assert forall|m: N| #[trigger] pushed(g0, root, e2, m) implies (e2.contains(m) || stops(
        g0,
        check_lin,
        m,
    ) || s2.contains(m)) by {
        if m != root {
            let (p, k) = choose|p: N, k: int|
                #![trigger e2.contains(p), g0.children[p][k]]
                e2.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == m;
            if p == node {
                assert(s2[t.len() + k] == m);
            } else {
                assert(pushed(g0, root, e, m));
                if m != node && s.contains(m) {
                    lemma_contains_drop_last(s, m);
                    let j = choose|j: int| 0 <= j < t.len() && t[j] == m;
                    assert(s2[j] == m);
                }
            }
        } else {
            assert(pushed(g0, root, e, m));
            if m != node && s.contains(m) {
                lemma_contains_drop_last(s, m);
                let j = choose|j: int| 0 <= j < t.len() && t[j] == m;
                assert(s2[j] == m);
            }
        }
    }
    assert forall|m: N| g0.nodes.contains(m) && #[trigger] g0.lin[m] && g0.rc[m] > 0 implies count(
        s2,
        m,
    ) + b2n(e2.contains(m)) == b2n(pushed(g0, root, e2, m)) by {
        lemma_count_append(t, ch, m);
        assert(count(s, m) + b2n(e.contains(m)) == b2n(pushed(g0, root, e, m)));
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
            assert(g0.children[node][k] == m);
            // lin の m を指す辺はただ1つなので、積まれるのはその1回だけである
            assert forall|j: int| 0 <= j < ch.len() && ch[j] == m implies j == k by {
                assert(g0.children[node][j] == g0.children[node][k]);
            }
            lemma_count_unique(ch, m, k);
            assert(!pushed(g0, root, e, m)) by {
                assert(m != root);
                if pushed(g0, root, e, m) {
                    let (p, k2) = choose|p: N, k2: int|
                        #![trigger e.contains(p), g0.children[p][k2]]
                        e.contains(p) && 0 <= k2 < g0.children[p].len() && g0.children[p][k2] == m;
                    assert(g0.children[p][k2] == g0.children[node][k]);
                }
            }
            assert(m != node);
            assert(pushed(g0, root, e2, m)) by {
                assert(e2.contains(node) && g0.children[node][k] == m);
            }
        } else {
            lemma_count_absent(ch, m);
            if pushed(g0, root, e2, m) && m != root {
                let (p, k) = choose|p: N, k: int|
                    #![trigger e2.contains(p), g0.children[p][k]]
                    e2.contains(p) && 0 <= k < g0.children[p].len() && g0.children[p][k] == m;
                if p == node {
                    assert(ch[k] == m);
                }
                assert(e.contains(p) && g0.children[p][k] == m);
                assert(pushed(g0, root, e, m));
            }
        }
    }
    assert forall|n: N| #[trigger] e2.contains(n) implies reached(g0, root, n) && expands(
        g0,
        n,
    ) by {
        if n != node {
            assert(e.contains(n));
        }
    }
    if check_lin {
        assert forall|n: N| #[trigger] v2.contains(n) implies e2.contains(n) && g0.lin[n] by {
            if g0.lin[node] && n == node {
            } else if g0.lin[node] {
                let j = choose|j: int| 0 <= j < v2.len() && v2[j] == n;
                assert(v1[j] == n);
                assert(v1.contains(n));
            } else {
                assert(v1.contains(n));
            }
        }
        assert forall|n: N| #[trigger] e2.contains(n) && g0.lin[n] implies v2.contains(n) by {
            if n == node {
                assert(v2[v2.len() - 1] == node);
            } else {
                assert(v1.contains(n));
                let j = choose|j: int| 0 <= j < v1.len() && v1[j] == n;
                if g0.lin[node] {
                    assert(v2[j] == n);
                }
            }
        }
    }
}

/// たどりのどの時点でも、変わったのは印と持ち主だけである。
proof fn lemma_marked<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s: Seq<N>,
    visited: Seq<N>,
    e: Set<N>,
)
    requires
        base_inv(g0, g, root, receiver, check_lin, s, visited, e),
    ensures
        marked_only(g0, g, receiver),
{
    assert forall|n: N| #[trigger] g0.nodes.contains(n) implies {
        &&& g.rc[n] == g0.rc[n] || (g0.live[n] && g0.rc[n] > 0 && !g0.lin[n] && g.rc[n]
            == mark_spec(g0.rc[n]))
        &&& g.owner[n] == g0.owner[n] || (g0.live[n] && g0.lin[n] && g.owner[n] == receiver)
    } by {
        assert(node_inv(g0, g, receiver, e, n));
    }
}

/// `lemma_skip` の、グラフの前提によらない部分。
proof fn lemma_base_skip<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s: Seq<N>,
    visited: Seq<N>,
    e: Set<N>,
)
    requires
        base_inv(g0, g, root, receiver, check_lin, s, visited, e),
        s.len() > 0,
    ensures
        base_inv(g0, g, root, receiver, check_lin, s.drop_last(), visited, e),
{
    let t = s.drop_last();
    assert forall|i: int| 0 <= i < t.len() implies g0.nodes.contains(#[trigger] t[i]) && reached(
        g0,
        root,
        t[i],
    ) by {
        assert(t[i] == s[i]);
    }
}

/// `lemma_expand` の、グラフの前提によらない部分。
proof fn lemma_base_expand<N>(
    g0: GView<N>,
    g1: GView<N>,
    g2: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s: Seq<N>,
    v1: Seq<N>,
    v2: Seq<N>,
    e: Set<N>,
)
    requires
        base_inv(g0, g1, root, receiver, check_lin, s, v1, e),
        s.len() > 0,
        !e.contains(s.last()),
        expands(g0, s.last()),
        g0.live[s.last()],
        g0.lin[s.last()] ==> g2 == g1.with_owner(s.last(), receiver),
        !g0.lin[s.last()] ==> g2 == g1.with_rc(s.last(), mark_spec(g0.rc[s.last()])),
        v2 == if check_lin && g0.lin[s.last()] {
            v1.push(s.last())
        } else {
            v1
        },
    ensures
        base_inv(
            g0,
            g2,
            root,
            receiver,
            check_lin,
            s.drop_last() + g0.children[s.last()],
            v2,
            e.insert(s.last()),
        ),
{
    let node = s.last();
    let t = s.drop_last();
    let ch = g0.children[node];
    let s2 = t + ch;
    let e2 = e.insert(node);
    assert(g0.nodes.contains(node)) by {
        assert(s[s.len() - 1] == node);
    }
    assert(reached(g0, root, node)) by {
        assert(s[s.len() - 1] == node);
    }
    assert(g2.rc.dom() =~= g0.rc.dom());
    assert(g2.owner.dom() =~= g0.owner.dom());
    assert forall|n: N| g0.nodes.contains(n) implies #[trigger] node_inv(
        g0,
        g2,
        receiver,
        e2,
        n,
    ) by {
        assert(node_inv(g0, g1, receiver, e, n));
    }
    assert forall|i: int| 0 <= i < s2.len() implies g0.nodes.contains(#[trigger] s2[i]) && reached(
        g0,
        root,
        s2[i],
    ) by {
        if i < t.len() {
            assert(s2[i] == s[i]);
        } else {
            assert(s2[i] == ch[i - t.len()]);
            lemma_reach_child(g0, root, node, i - t.len());
        }
    }
    assert forall|n: N| #[trigger] e2.contains(n) implies reached(g0, root, n) && expands(
        g0,
        n,
    ) by {
        if n != node {
            assert(e.contains(n));
        }
    }
    if check_lin {
        assert forall|n: N| #[trigger] v2.contains(n) implies e2.contains(n) && g0.lin[n] by {
            if g0.lin[node] && n == node {
            } else if g0.lin[node] {
                let j = choose|j: int| 0 <= j < v2.len() && v2[j] == n;
                assert(v1[j] == n);
                assert(v1.contains(n));
            } else {
                assert(v1.contains(n));
            }
        }
        assert forall|n: N| #[trigger] e2.contains(n) && g0.lin[n] implies v2.contains(n) by {
            if n == node {
                assert(v2[v2.len() - 1] == node);
            } else {
                assert(v1.contains(n));
                let j = choose|j: int| 0 <= j < v1.len() && v1[j] == n;
                if g0.lin[node] {
                    assert(v2[j] == n);
                }
            }
        }
    }
}

/// たどりが止まったとき。積み場が空なので、届く物体はどれも展開済みである
proof fn lemma_finish<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    receiver: u32,
    check_lin: bool,
    s: Seq<N>,
    visited: Seq<N>,
    e: Set<N>,
)
    requires
        inv(g0, g, root, receiver, check_lin, s, visited, e),
        s.len() == 0,
    ensures
        forall|n: N| g0.nodes.contains(n) ==> #[trigger] node_post(g0, g, root, receiver, n),
        shared_points_to_no_local(g),
        check_lin ==> !reaches_shared_lin(g0, root),
{
    assert forall|m: N| #[trigger] pushed(g0, root, e, m) implies (e.contains(m) || stops(
        g0,
        check_lin,
        m,
    )) by {
        assert(!s.contains(m));
    }
    assert forall|m: N| #[trigger] pushed(g0, root, e, m) implies (e.contains(m) || !expands(
        g0,
        m,
    )) by {
        assert(e.contains(m) || stops(g0, check_lin, m));
    }
    if check_lin {
        assert forall|n: N| #[trigger] reached(g0, root, n) implies !(g0.lin[n] && g0.rc[n]
            < 0) by {
            lemma_reached_pushed(g0, root, e, n);
            assert(e.contains(n) || stops(g0, check_lin, n));
        }
    }
    assert forall|n: N| g0.nodes.contains(n) implies #[trigger] node_post(
        g0,
        g,
        root,
        receiver,
        n,
    ) by {
        assert(node_inv(g0, g, receiver, e, n));
        if reached(g0, root, n) && expands(g0, n) {
            lemma_reached_pushed(g0, root, e, n);
        }
    }
    assert forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() && !g.lin[p] && g.rc[p] < 0 implies (
    g.rc[#[trigger] g.children[p][k]] <= 0 || g.lin[g.children[p][k]]) by {
        let j = g0.children[p][k];
        assert(g0.nodes.contains(j));
        assert(node_inv(g0, g, receiver, e, p));
        assert(node_inv(g0, g, receiver, e, j));
        if e.contains(p) {
            assert(pushed(g0, root, e, j));
            if e.contains(j) && !g0.lin[j] {
                lemma_mark_negative(g0.rc[j]);
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
    assert forall|n: int| #[trigger] reached(g, 0int, n) implies g.live[n] by {
        let path = choose|path: Seq<int>| #[trigger] is_path(g, 0int, path) && path.last() == n;
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
/// 根から届く物体に共有の印を付ける。不死の物体と共有の物体でたどりを止め、不死の物体は `lin` ビットが立っていても
/// 変えない。`lin` の物体には印を付けずに所有者を移し、その子はたどり続ける。`Lin` の物体の中の `Unr` の子は、
/// 送り手がほかの所からも参照していることがあるためである。
/// 届いた物体が生きていなければ `Dead` を返す。`check_lin` なら、すでに共有の `lin` の物体に届いたときは `LinShared`、
/// 同じ `lin` の物体に2回届いたときは `LinReachedTwice` を返す。
// 取り出す前の積み場に名前を付けて証明するので、`while let` でなく `loop` と `match` で書く
#[allow(clippy::while_let_loop)]
pub(crate) fn share_walk<G: Graph>(
    graph: &mut G,
    root: G::Node,
    receiver: u32,
    check_lin: bool,
    stack: &mut Vec<G::Node>,
    visited_lin: &mut Vec<G::Node>,
) -> (r: Result<(), ShareError>)
    requires
        shaped(old(graph).gv(), root),
        !check_lin ==> well_formed(old(graph).gv(), root),
    ensures
        same_shape(old(graph).gv(), final(graph).gv()),
        marked_only(old(graph).gv(), final(graph).gv(), receiver),
        final(graph).kept(*old(graph)),
        r == Err::<(), ShareError>(ShareError::Dead) ==> exists|n: G::Node| #[trigger]
            reached(old(graph).gv(), root, n) && old(graph).gv().nodes.contains(n) && !old(
                graph,
            ).gv().live[n],
        r == Err::<(), ShareError>(ShareError::LinShared) ==> check_lin && reaches_shared_lin(
            old(graph).gv(),
            root,
        ),
        sound(old(graph).gv(), root) ==> r != Err::<(), ShareError>(ShareError::LinReachedTwice),
        well_formed(old(graph).gv(), root) ==> (r is Ok <==> !(check_lin && reaches_shared_lin(
            old(graph).gv(),
            root,
        ))),
        r is Ok && sound(old(graph).gv(), root) ==> forall|n: G::Node|
            old(graph).gv().nodes.contains(n) ==> #[trigger] node_post(
                old(graph).gv(),
                final(graph).gv(),
                root,
                receiver,
                n,
            ),
        r is Ok && sound(old(graph).gv(), root) ==> shared_points_to_no_local(final(graph).gv()),
{
    let ghost g0 = graph.gv();
    let ghost start = *graph;
    let ghost mut e: Set<G::Node> = Set::empty();
    proof {
        start.lemma_kept_refl();
    }
    stack.clear();
    visited_lin.clear();
    stack.push(root);
    proof {
        reveal_with_fuel(count, 2);
        assert(count(stack@, root) == 1) by {
            assert(stack@.drop_last() =~= Seq::<G::Node>::empty());
        }
        assert forall|n: G::Node| g0.nodes.contains(n) && #[trigger] g0.lin[n] implies count(
            stack@,
            n,
        ) + b2n(e.contains(n)) == b2n(pushed(g0, root, e, n)) by {
            assert(stack@.drop_last() =~= Seq::<G::Node>::empty());
        }
        lemma_reached_root(g0, root);
        assert(stack@[0] == root);
    }
    loop
        invariant
            base_inv(g0, graph.gv(), root, receiver, check_lin, stack@, visited_lin@, e),
            sound(g0, root) ==> inv(
                g0,
                graph.gv(),
                root,
                receiver,
                check_lin,
                stack@,
                visited_lin@,
                e,
            ),
            !check_lin ==> well_formed(g0, root),
            g0 == old(graph).gv(),
            start == *old(graph),
            graph.kept(start),
        ensures
            graph.kept(start),
            base_inv(g0, graph.gv(), root, receiver, check_lin, stack@, visited_lin@, e),
            sound(g0, root) ==> inv(
                g0,
                graph.gv(),
                root,
                receiver,
                check_lin,
                stack@,
                visited_lin@,
                e,
            ),
            stack@.len() == 0,
        decreases g0.nodes.len() - e.len(), stack@.len(),
    {
        let ghost s = stack@;
        let node = match stack.pop() {
            Some(node) => node,
            None => break,
        };
        proof {
            assert(stack@ =~= s.drop_last());
            assert(node == s.last());
            assert(s[s.len() - 1] == node);
            assert(node_inv(g0, graph.gv(), receiver, e, node));
            assert(reached(g0, root, node));
            lemma_last_counted(s);
            vstd::set_lib::lemma_len_subset(e, g0.nodes);
        }
        proof {
            lemma_marked(g0, graph.gv(), root, receiver, check_lin, s, visited_lin@, e);
        }
        if !graph.live(node) {
            return Err(ShareError::Dead);
        }
        let rc = graph.rc(node);
        if rc == 0 {
            proof {
                if sound(g0, root) {
                    lemma_skip(g0, graph.gv(), root, receiver, check_lin, s, visited_lin@, e);
                }
                lemma_base_skip(g0, graph.gv(), root, receiver, check_lin, s, visited_lin@, e);
            }
            continue;
        }
        if graph.lin(node) {
            if rc < 0 {
                if check_lin {
                    proof {
                        assert(reached(g0, root, node) && g0.lin[node] && g0.rc[node] < 0);
                    }
                    return Err(ShareError::LinShared);
                }
                proof {
                    lemma_skip(g0, graph.gv(), root, receiver, check_lin, s, visited_lin@, e);
                }
                continue;
            }
            let ghost v1 = visited_lin@;
            if check_lin {
                proof {
                    assert forall|j: int|
                        0 <= j < visited_lin@.len() implies graph.gv().nodes.contains(
                        #[trigger] visited_lin@[j],
                    ) by {
                        assert(visited_lin@.contains(visited_lin@[j]));
                    }
                }
                if contains_node(graph, visited_lin.as_slice(), node) {
                    proof {
                        // 正しいグラフでは、`lin` の物体は1回しか積まれないので、訪問済みにはない
                        if sound(g0, root) {
                            assert(g0.lin[node] && g0.rc[node] > 0);
                            assert(!e.contains(node));
                        }
                    }
                    return Err(ShareError::LinReachedTwice);
                }
                visited_lin.push(node);
            }
            proof {
                if check_lin {
                    assert(!v1.contains(node));
                }
                assert(!e.contains(node));
            }
            let ghost g1 = graph.gv();
            let ghost before = *graph;
            graph.set_owner(node, receiver);
            proof {
                graph.lemma_kept_trans(before, start);
            }
            graph.push_children(node, stack);
            proof {
                if sound(g0, root) {
                    lemma_expand(
                        g0,
                        g1,
                        graph.gv(),
                        root,
                        receiver,
                        check_lin,
                        s,
                        v1,
                        visited_lin@,
                        e,
                    );
                }
                lemma_base_expand(
                    g0,
                    g1,
                    graph.gv(),
                    root,
                    receiver,
                    check_lin,
                    s,
                    v1,
                    visited_lin@,
                    e,
                );
                e = e.insert(node);
                vstd::set_lib::lemma_len_subset(e, g0.nodes);
            }
        } else if let Some(shared) = rc::mark_step(rc) {
            proof {
                if e.contains(node) {
                    lemma_mark_negative(g0.rc[node]);
                }
            }
            let ghost g1 = graph.gv();
            let ghost before = *graph;
            graph.set_rc(node, shared);
            proof {
                graph.lemma_kept_trans(before, start);
            }
            graph.push_children(node, stack);
            proof {
                if sound(g0, root) {
                    lemma_expand(
                        g0,
                        g1,
                        graph.gv(),
                        root,
                        receiver,
                        check_lin,
                        s,
                        visited_lin@,
                        visited_lin@,
                        e,
                    );
                }
                lemma_base_expand(
                    g0,
                    g1,
                    graph.gv(),
                    root,
                    receiver,
                    check_lin,
                    s,
                    visited_lin@,
                    visited_lin@,
                    e,
                );
                e = e.insert(node);
                vstd::set_lib::lemma_len_subset(e, g0.nodes);
            }
        } else {
            proof {
                if sound(g0, root) {
                    lemma_skip(g0, graph.gv(), root, receiver, check_lin, s, visited_lin@, e);
                }
                lemma_base_skip(g0, graph.gv(), root, receiver, check_lin, s, visited_lin@, e);
            }
        }
    }
    proof {
        if sound(g0, root) {
            lemma_finish(g0, graph.gv(), root, receiver, check_lin, stack@, visited_lin@, e);
        }
        lemma_marked(g0, graph.gv(), root, receiver, check_lin, stack@, visited_lin@, e);
    }
    Ok(())
}

} // verus!

verus! {

/// 語 `w` が指すヘッダのポインタ。`Word::as_ptr` が返す値である。
pub(crate) open spec fn ptr_of(w: Word) -> *mut Header {
    vstd::raw_ptr::ptr_mut_from_data::<Header>(
        vstd::raw_ptr::PtrData {
            addr: w.bits_spec(),
            provenance: w.ptr_spec()@.provenance,
            metadata: (),
        },
    )
}

/// 権限 `p` のブロックの、走査する語の i 番目より前の語のうち、物体を指すものの列。
pub(crate) open spec fn kids(p: BlockPerm, first: int, i: int) -> Seq<*mut Header>
    decreases i,
{
    if i <= 0 {
        Seq::empty()
    } else {
        let w = p_word(p, first + i - 1);
        kids(p, first, i - 1) + if w.is_ptr_spec() {
            seq![ptr_of(w)]
        } else {
            Seq::empty()
        }
    }
}

/// 検査モードの前置きが生存を示す。
pub(crate) open spec fn prefix_alive(p: BlockPerm) -> bool {
    p.prefix matches Some(q) && q.state.value() & 3 == ALIVE_STATE
}

/// 節点 `a` の権限。ヒープのブロックなら預かった権限 `perms`、付けた塊のブロックなら塊の権限である。
pub(crate) open spec fn node_perm(base: Core, perms: Map<usize, BlockPerm>, a: usize) -> BlockPerm {
    if base.in_region(a) {
        base.perm(a)
    } else {
        perms[a]
    }
}

/// `share` の節点にするブロック。生きている物体で、権限は物体の形をしている。たどりはこの形を変えない。付けた塊の
/// 物体も節点で、rc 0 なのでたどりはそこで止まる。
pub(crate) open spec fn node_ok(
    base: Core,
    perms: Map<usize, BlockPerm>,
    checked: bool,
    a: usize,
) -> bool {
    let p = node_perm(base, perms, a);
    let st = base.state(a);
    &&& base.has(a)
    &&& if checked {
        st != BlockState::Building
    } else {
        st == BlockState::Live
    }
    &&& base.in_region(a) ==> base.region_wf()
    &&& !base.in_region(a) ==> perms.dom().contains(a)
    &&& base.block(a)@.addr == a
    &&& a % 8 == 0
    &&& a >= 8
    &&& a + 8 * (1 + base.cap(a)) <= usize::MAX
    &&& p.wf(base.block(a), base.cap(a), checked)
    &&& p.rc.is_init()
    &&& p.meta.is_init()
    &&& bits_wf(p_meta(p))
    &&& p.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
    &&& p.prefix is Some ==> p_code(p) == state_code(st)
    &&& st != BlockState::Freed ==> {
        &&& p_extended(p) ==> p_init(p, 0)
        &&& p_first(p) + p_count(p) <= base.cap(a)
    }
    &&& st != BlockState::Freed ==> forall|i: int| #[trigger] p_scanned(p, i) ==> p_init(p, i)
}

/// 節点のヘッダの集合。
pub(crate) open spec fn nodes_of(base: Core, perms: Map<usize, BlockPerm>, checked: bool) -> Set<
    *mut Header,
> {
    base.dom().union(base.region_blocks()).filter(|a: usize| node_ok(base, perms, checked, a)).map(
        |a: usize| base.block(a),
    )
}

/// ヒープの物体のグラフの見方。`base` はたどりの前のヒープで、ブロックの位置を決める。
// 中身を開くと `share` の証明の量化子が増えすぎるので、使う所でだけ開く
#[verifier::opaque]
pub(crate) open spec fn graph_of(
    base: Core,
    perms: Map<usize, BlockPerm>,
    owner: Map<usize, u32>,
    checked: bool,
) -> GView<*mut Header> {
    let ns = nodes_of(base, perms, checked);
    GView {
        nodes: ns,
        live: Map::new(
            ns,
            |n: *mut Header| !checked || prefix_alive(node_perm(base, perms, n@.addr)),
        ),
        rc: Map::new(ns, |n: *mut Header| node_perm(base, perms, n@.addr).rc.value()),
        lin: Map::new(ns, |n: *mut Header| bits_lin(p_meta(node_perm(base, perms, n@.addr)))),
        owner: Map::new(ns, |n: *mut Header| owner[n@.addr]),
        children: Map::new(
            ns,
            |n: *mut Header|
                if base.state(n@.addr) == BlockState::Freed {
                    Seq::empty()
                } else {
                    let p = node_perm(base, perms, n@.addr);
                    kids(p, p_first(p), p_count(p))
                },
        ),
    }
}

/// 節点は、形の正しいブロックのヘッダである。
pub(crate) proof fn lemma_node(
    base: Core,
    perms: Map<usize, BlockPerm>,
    checked: bool,
    n: *mut Header,
)
    requires
        nodes_of(base, perms, checked).contains(n),
    ensures
        node_ok(base, perms, checked, n@.addr),
        n == base.block(n@.addr),
{
    let f = |a: usize| base.block(a);
    let s = base.dom().union(base.region_blocks()).filter(
        |a: usize| node_ok(base, perms, checked, a),
    );
    s.lemma_map_contains(f, n);
    let a = choose|a: usize| s.contains(a) && n == f(a);
    assert(node_ok(base, perms, checked, a));
}

/// 形の正しいブロックのヘッダは、節点である。
pub(crate) proof fn lemma_is_node(base: Core, perms: Map<usize, BlockPerm>, checked: bool, a: usize)
    requires
        node_ok(base, perms, checked, a),
    ensures
        nodes_of(base, perms, checked).contains(base.block(a)),
{
    let f = |b: usize| base.block(b);
    let s = base.dom().union(base.region_blocks()).filter(
        |b: usize| node_ok(base, perms, checked, b),
    );
    base.lemma_region_blocks();
    s.lemma_map_contains(f, base.block(a));
    assert(s.contains(a));
}

/// `as_ptr` の結果は `ptr_of` である。
pub(crate) proof fn lemma_ptr_of(w: Word, h: *mut Header)
    requires
        h@.addr == w.bits_spec(),
        h@.provenance == w.ptr_spec()@.provenance,
    ensures
        h == ptr_of(w),
{
    vstd::raw_ptr::ptrs_mut_eq_sized(h);
    vstd::raw_ptr::ptrs_mut_eq_sized(ptr_of(w));
}

/// 検査モードの前置きの所有者は、たどりの前に rc が正だった節点で、持ち主に等しい。
pub(crate) open spec fn owners_ok(
    base: Core,
    perms: Map<usize, BlockPerm>,
    owner: Map<usize, u32>,
    checked: bool,
) -> bool {
    forall|a: usize| #[trigger]
        node_ok(base, perms, checked, a) && base.state(a) == BlockState::Live && base.rc_of(a) > 0
            && perms[a].prefix is Some ==> p_owner(perms[a]) == owner[a]
}

/// 2つの権限の写像で、たどりが変えない部分 (rc の値と、節点の前置きの所有者の値を除くすべて) が同じである。
pub(crate) open spec fn same_frame(
    base: Core,
    p1: Map<usize, BlockPerm>,
    p0: Map<usize, BlockPerm>,
    checked: bool,
) -> bool {
    &&& p1.dom() == p0.dom()
    &&& forall|a: usize|
        #![trigger p1[a]]
        p0.dom().contains(a) ==> {
            let x = p1[a];
            let y = p0[a];
            &&& x.meta == y.meta
            &&& x.slots == y.slots
            &&& x.text == y.text
            &&& x.rc.ptr() == y.rc.ptr()
            &&& x.rc.is_init() == y.rc.is_init()
            &&& (x.prefix is Some) == (y.prefix is Some)
            &&& x.prefix matches Some(q1) ==> {
                let q0 = y.prefix->0;
                &&& q1.state == q0.state
                &&& q1.first.ptr() == q0.first.ptr()
                &&& q1.first.is_init() == q0.first.is_init()
            }
            &&& !node_ok(base, p0, checked, a) ==> x == y
        }
}

/// 子の列は、権限のペイロードの語だけで決まる。
pub(crate) proof fn lemma_kids_same(x: BlockPerm, y: BlockPerm, first: int, i: int)
    requires
        x.slots == y.slots,
    ensures
        kids(x, first, i) == kids(y, first, i),
    decreases i,
{
    if i > 0 {
        lemma_kids_same(x, y, first, i - 1);
    }
}

/// 形が同じなら、節点の集合と、節点の生存、`lin`、子は変わらない。
pub(crate) proof fn lemma_same_frame_graph(
    base: Core,
    p1: Map<usize, BlockPerm>,
    p0: Map<usize, BlockPerm>,
    o1: Map<usize, u32>,
    o0: Map<usize, u32>,
    checked: bool,
)
    requires
        same_frame(base, p1, p0, checked),
    ensures
        nodes_of(base, p1, checked) == nodes_of(base, p0, checked),
        forall|a: usize| #[trigger] node_ok(base, p1, checked, a) == node_ok(base, p0, checked, a),
        graph_of(base, p1, o1, checked).live == graph_of(base, p0, o0, checked).live,
        graph_of(base, p1, o1, checked).lin == graph_of(base, p0, o0, checked).lin,
        graph_of(base, p1, o1, checked).children == graph_of(base, p0, o0, checked).children,
{
    reveal(graph_of);
    assert forall|a: usize| #[trigger]
        node_ok(base, p1, checked, a) == node_ok(base, p0, checked, a) by {
        if p0.dom().contains(a) {
            let x = p1[a];
            let y = p0[a];
            assert forall|i: int| #[trigger] p_scanned(x, i) == p_scanned(y, i) by {}
        }
    }
    assert(base.dom().union(base.region_blocks()).filter(|a: usize| node_ok(base, p1, checked, a))
        =~= base.dom().union(base.region_blocks()).filter(
        |a: usize| node_ok(base, p0, checked, a),
    ));
    let ns = nodes_of(base, p0, checked);
    let g1 = graph_of(base, p1, o1, checked);
    let g0 = graph_of(base, p0, o0, checked);
    assert forall|n: *mut Header| #[trigger] ns.contains(n) implies g1.live[n] == g0.live[n]
        && g1.lin[n] == g0.lin[n] && g1.children[n] == g0.children[n] by {
        lemma_node(base, p0, checked, n);
        let a = n@.addr;
        let x = node_perm(base, p1, a);
        let y = node_perm(base, p0, a);
        lemma_kids_same(x, y, p_first(y), p_count(y));
        assert(p_first(x) == p_first(y));
        assert(p_count(x) == p_count(y));
    }
    assert(g1.live =~= g0.live);
    assert(g1.lin =~= g0.lin);
    assert(g1.children =~= g0.children);
}

/// ヒープの物体のグラフ。`share` のたどりの間、ヒープのブロックの権限と、検査モードの ID ごとの数を預かる。節点は
/// 生きている物体のヘッダである。
// 数の欄は速いモードでも `Vec` の大きさを持つが、空のまま触れない。ほかの欄はどれも ghost と権限で、実行の時には
// 大きさを持たない
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) struct Objects<M> {
    pub(crate) perms: Tracked<Map<usize, BlockPerm>>,
    /// 検査モードの ID ごとの数 (`Heap` の `owned`)。rc の符号か、rc が正の物体の持ち主が変わると数え直す。
    /// 速いモードでは空のまま触れない。
    pub(crate) owned: Vec<u64>,
    /// たどりの間ヒープから預かる、付けた塊の権限の写し。塊の節点はここから読む。
    pub(crate) env: Tracked<Env>,
    /// 物体の持ち主。速いモードでも ghost の値として持ち、たどりの後でモデルに戻す。
    pub(crate) owner: Ghost<Map<usize, u32>>,
    /// たどりの前のヒープ。
    pub(crate) base: Ghost<Core>,
    pub(crate) _mode: PhantomData<M>,
}

impl<M> Objects<M> {
    /// 節点を決めるヒープ。たどりの前のヒープに、預かった塊の権限を戻したものである。
    pub(crate) open spec fn view(&self) -> Core {
        Core { env: self.env, ..self.base@ }
    }

    /// 節点 `h` の権限を借りる。ヒープのブロックなら預かった権限から、塊のブロックなら塊の権限から借りる。どちらから
    /// 借りるかは ghost の条件で決まる。
    proof fn tracked_node(tracked &self, h: usize, checked: bool) -> (tracked p: &BlockPerm)
        requires
            node_ok(self.view(), self.perms@, checked, h),
        ensures
            *p == node_perm(self.view(), self.perms@, h),
    {
        if self.view().in_region(h) {
            self.env.borrow().tracked_region().tracked_perm(h, self.view().checked())
        } else {
            self.perms.borrow().tracked_borrow(h)
        }
    }
}

impl<M: Mode> Graph for Objects<M> {
    type Node = *mut Header;

    open spec fn gv(&self) -> GView<*mut Header> {
        graph_of(self.view(), self.perms@, self.owner@, M::CHECKED)
    }

    open spec fn kept(&self, old: Self) -> bool {
        &&& self.base == old.base
        &&& self.env == old.env
        &&& same_frame(self.view(), self.perms@, old.perms@, M::CHECKED)
        &&& old.owner@.dom().subset_of(self.owner@.dom())
        &&& forall|a: usize|
            #![trigger self.owner@.dom().contains(a)]
            self.owner@.dom().contains(a) ==> old.owner@.dom().contains(a) || (node_ok(
                self.view(),
                old.perms@,
                M::CHECKED,
                a,
            ) && !self.view().in_region(a))
        &&& forall|a: usize|
            #![trigger self.owner@[a]]
            !node_ok(self.view(), old.perms@, M::CHECKED, a) ==> self.owner@[a] == old.owner@[a]
        &&& owners_ok(self.view(), old.perms@, old.owner@, M::CHECKED) ==> owners_ok(
            self.view(),
            self.perms@,
            self.owner@,
            M::CHECKED,
        )
    }

    proof fn lemma_kept_refl(&self) {
    }

    proof fn lemma_kept_trans(&self, mid: Self, old: Self) {
        lemma_same_frame_graph(
            self.view(),
            mid.perms@,
            old.perms@,
            mid.owner@,
            old.owner@,
            M::CHECKED,
        );
        lemma_same_frame_graph(
            self.view(),
            self.perms@,
            mid.perms@,
            self.owner@,
            mid.owner@,
            M::CHECKED,
        );
    }

    fn live(&self, h: *mut Header) -> (r: bool) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
        }
        if !M::CHECKED {
            return true;
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        let tracked q = p.prefix.tracked_borrow();
        let state = u32_load(prefix_state_ptr(h), Tracked(&q.state));
        state & 3 == ALIVE_STATE
    }

    fn rc(&self, h: *mut Header) -> (r: i32) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        rc_load(rc_ptr(h), Tracked(&p.rc))
    }

    fn set_rc(&mut self, h: *mut Header, rc: i32) {
        let ghost old_self = *self;
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
            // rc が 0 でない節点は、塊の物体でない
            if self.view().in_region(h@.addr) {
                self.view().lemma_region_rc(h@.addr);
            }
        }
        if M::CHECKED {
            let tracked p = self.perms.borrow().tracked_borrow(h@.addr);
            let before = rc_load(rc_ptr(h), Tracked(&p.rc));
            let tracked q = p.prefix.tracked_borrow();
            let owner = u32_load(prefix_first_ptr(h), Tracked(&q.first));
            if before > 0 && rc <= 0 {
                count_owned(&mut self.owned, owner, false);
            } else if before <= 0 && rc > 0 {
                count_owned(&mut self.owned, owner, true);
            }
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        rc_store(rc_ptr(h), Tracked(&mut p.rc), rc);
        proof {
            reveal(graph_of);
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
            assert(self.perms@.dom() =~= old_self.perms@.dom());
            lemma_same_frame_graph(
                self.view(),
                self.perms@,
                old_self.perms@,
                self.owner@,
                old_self.owner@,
                M::CHECKED,
            );
            let ns = nodes_of(self.view(), old_self.perms@, M::CHECKED);
            assert forall|n: *mut Header| #[trigger] ns.contains(n) implies self.gv().rc[n]
                == old_self.gv().with_rc(h, rc).rc[n] by {
                lemma_node(self.view(), old_self.perms@, M::CHECKED, n);
            }
            assert(self.gv().rc =~= old_self.gv().with_rc(h, rc).rc);
            assert(self.gv().owner =~= old_self.gv().owner);
            if owners_ok(self.view(), old_self.perms@, old_self.owner@, M::CHECKED) {
                assert forall|a: usize| #[trigger]
                    node_ok(self.view(), self.perms@, M::CHECKED, a) && self.view().state(a)
                        == BlockState::Live && self.view().rc_of(a) > 0
                        && self.perms@[a].prefix is Some implies p_owner(self.perms@[a])
                    == self.owner@[a] by {
                    assert(node_ok(self.view(), old_self.perms@, M::CHECKED, a));
                }
            }
        }
    }

    fn lin(&self, h: *mut Header) -> (r: bool) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        let bits = *ptr_ref(meta_ptr(h), Tracked(&p.meta));
        Meta::from_bits(bits).lin()
    }

    fn set_owner(&mut self, h: *mut Header, owner: u32) {
        let ghost old_self = *self;
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
            // rc が 0 でない節点は、塊の物体でない
            if self.view().in_region(h@.addr) {
                self.view().lemma_region_rc(h@.addr);
            }
        }
        if M::CHECKED {
            let tracked p = self.perms.borrow().tracked_borrow(h@.addr);
            if rc_load(rc_ptr(h), Tracked(&p.rc)) > 0 {
                let tracked q = p.prefix.tracked_borrow();
                let before = u32_load(prefix_first_ptr(h), Tracked(&q.first));
                move_owned(&mut self.owned, before, owner);
            }
            let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
            let tracked mut q = p.prefix.tracked_unwrap();
            u32_store(prefix_first_ptr(h), Tracked(&mut q.first), owner);
            proof {
                reveal(graph_of);
                p.prefix = Some(q);
                self.perms.borrow_mut().tracked_insert(h@.addr, p);
                assert(self.perms@.dom() =~= old_self.perms@.dom());
            }
        }
        proof {
            self.owner = Ghost(self.owner@.insert(h@.addr, owner));
            lemma_same_frame_graph(
                self.view(),
                self.perms@,
                old_self.perms@,
                self.owner@,
                old_self.owner@,
                M::CHECKED,
            );
            let ns = nodes_of(self.view(), old_self.perms@, M::CHECKED);
            assert forall|n: *mut Header| #[trigger] ns.contains(n) implies self.gv().owner[n]
                == old_self.gv().with_owner(h, owner).owner[n] && self.gv().rc[n]
                == old_self.gv().rc[n] by {
                lemma_node(self.view(), old_self.perms@, M::CHECKED, n);
            }
            assert(self.gv().owner =~= old_self.gv().with_owner(h, owner).owner);
            assert(self.gv().rc =~= old_self.gv().rc);
            if owners_ok(self.view(), old_self.perms@, old_self.owner@, M::CHECKED) {
                assert forall|a: usize| #[trigger]
                    node_ok(self.view(), self.perms@, M::CHECKED, a) && self.view().state(a)
                        == BlockState::Live && self.view().rc_of(a) > 0
                        && self.perms@[a].prefix is Some implies p_owner(self.perms@[a])
                    == self.owner@[a] by {
                    assert(node_ok(self.view(), old_self.perms@, M::CHECKED, a));
                }
            }
        }
    }

    fn push_children(&self, h: *mut Header, out: &mut Vec<*mut Header>) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
            // たどりは生きている節点だけを展開する。前置きの状態はブロックの状態を表すので、その節点は生きている物体である
            assert(self.view().state(h@.addr) == BlockState::Live);
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        let bits = *ptr_ref(meta_ptr(h), Tracked(&p.meta));
        let m = Meta::from_bits(bits);
        let (first, n) = if m.scan() == Meta::EXTENDED_SCAN {
            let w = *ptr_ref(slot_ptr(h, 0), Tracked(p.slots.tracked_borrow(0)));
            (1usize, w.bits())
        } else {
            (0usize, m.scan() as usize)
        };
        let mut i: usize = 0;
        while i < n
            invariant
                out@ == old(out)@ + kids(*p, first as int, i as int),
                i <= n,
                first == p_first(*p),
                n == p_count(*p),
                node_ok(self.view(), self.perms@, M::CHECKED, h@.addr),
                self.view().state(h@.addr) == BlockState::Live,
                *p == node_perm(self.view(), self.perms@, h@.addr),
                h == self.view().block(h@.addr),
            decreases n - i,
        {
            proof {
                assert(p_scanned(*p, (first + i) as int));
            }
            let w = *ptr_ref(
                slot_ptr(h, first + i),
                Tracked(p.slots.tracked_borrow((first + i) as int)),
            );
            let ghost before = out@;
            if w.is_ptr() {
                let c = w.as_ptr();
                proof {
                    lemma_ptr_of(w, c);
                }
                out.push(c);
            }
            proof {
                assert(out@ =~= old(out)@ + kids(*p, first as int, i + 1));
            }
            i += 1;
        }
    }

    fn same(&self, a: *mut Header, b: *mut Header) -> (r: bool) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, a);
            lemma_node(self.view(), self.perms@, M::CHECKED, b);
        }
        a.addr() == b.addr()
    }
}

} // verus!

verus! {

/// 走査する語のうち物体を指すもののヘッダがどれも `pred` を満たすなら、子の列の要素もどれも満たす。
pub(crate) proof fn lemma_kids(p: BlockPerm, first: int, i: int, pred: spec_fn(*mut Header) -> bool)
    requires
        forall|j: int|
            #![trigger p_word(p, j)]
            first <= j < first + i && p_word(p, j).is_ptr_spec() ==> pred(ptr_of(p_word(p, j))),
    ensures
        forall|k: int| 0 <= k < kids(p, first, i).len() ==> pred(#[trigger] kids(p, first, i)[k]),
    decreases i,
{
    if i > 0 {
        lemma_kids(p, first, i - 1, pred);
        let prev = kids(p, first, i - 1);
        assert forall|k: int| 0 <= k < kids(p, first, i).len() implies pred(
            #[trigger] kids(p, first, i)[k],
        ) by {
            if k < prev.len() {
                assert(kids(p, first, i)[k] == prev[k]);
            } else {
                assert(kids(p, first, i)[k] == ptr_of(p_word(p, first + i - 1)));
            }
        }
    }
}

/// 不変条件を満たすヒープの生きている物体は節点で、そのグラフは形の前提を満たす。
pub(crate) proof fn lemma_shaped(s0: Core, a: usize)
    requires
        s0.full() || s0.checked(),
        s0.checked() ==> s0.safe_wf(),
        s0.model@.building is None,
        s0.has(a),
        !s0.checked() ==> s0.state(a) == BlockState::Live,
    ensures
        shaped(graph_of(s0, s0.perms@, s0.model@.owner, s0.checked()), s0.block(a)),
        forall|b: usize|
            #![trigger s0.state(b)]
            s0.dom().contains(b) && s0.state(b) == BlockState::Live ==> node_ok(
                s0,
                s0.perms@,
                s0.checked(),
                b,
            ),
{
    reveal(graph_of);
    reveal(Core::building_wf);
    let checked = s0.checked();
    // 検査モードの節点は構築中でないブロックのすべてで、そのどれもが安全の条件を満たす (`Core::safe_wf`)
    assert forall|b: usize|
        s0.has(b) && (if checked {
            s0.state(b) != BlockState::Building
        } else {
            s0.state(b) == BlockState::Live
        }) implies #[trigger] node_ok(s0, s0.perms@, checked, b) by {
        s0.lemma_shape(b);
        let p = s0.perm(b);
        if !s0.dom().contains(b) {
            s0.lemma_region_at(b);
        } else if checked {
            s0.lemma_safe_at(b);
            if s0.state(b) != BlockState::Freed {
                assert forall|i: int| #[trigger] p_scanned(p, i) implies p_init(p, i) by {
                    assert(s0.safe_slot(p, i));
                }
            }
        } else {
            s0.lemma_at(b);
            assert forall|i: int| #[trigger] p_scanned(p, i) implies p_init(p, i) by {
                assert(s0.slot_word(p, b, i));
            }
        }
    }
    assert forall|b: usize|
        #![trigger s0.state(b)]
        s0.dom().contains(b) && s0.state(b) == BlockState::Live implies node_ok(
        s0,
        s0.perms@,
        checked,
        b,
    ) by {}
    let g = graph_of(s0, s0.perms@, s0.model@.owner, checked);
    lemma_is_node(s0, s0.perms@, checked, a);
    assert forall|n: *mut Header, k: int|
        g.nodes.contains(n) && 0 <= k < g.children[n].len() implies g.nodes.contains(
        #[trigger] g.children[n][k],
    ) by {
        lemma_node(s0, s0.perms@, checked, n);
        let b = n@.addr;
        let p = s0.perm(b);
        assert(s0.state(b) != BlockState::Freed);
        let pred = |x: *mut Header| g.nodes.contains(x);
        assert forall|j: int|
            #![trigger p_word(p, j)]
            p_first(p) <= j < p_first(p) + p_count(p) && p_word(p, j).is_ptr_spec() implies pred(
            ptr_of(p_word(p, j)),
        ) by {
            assert(p_scanned(p, j));
            let t = p_word(p, j).bits_spec();
            if !s0.dom().contains(b) {
                s0.lemma_region_at(b);
            } else if checked {
                s0.lemma_safe_at(b);
                assert(s0.safe_slot(p, j));
            } else {
                s0.lemma_at(b);
                reveal(Core::edges_wf);
                assert(s0.slot_word(p, b, j));
                assert(s0.edge_wf((b, j)));
            }
            assert(s0.has(t));
            s0.lemma_shape(t);
            vstd::raw_ptr::ptrs_mut_eq_sized(s0.block(t));
            vstd::raw_ptr::ptrs_mut_eq_sized(ptr_of(p_word(p, j)));
            assert(ptr_of(p_word(p, j)) == s0.block(t));
            assert(s0.has(t) && (if checked {
                s0.state(t) != BlockState::Building
            } else {
                s0.state(t) == BlockState::Live
            }));
            lemma_is_node(s0, s0.perms@, checked, t);
        }
        lemma_kids(p, p_first(p), p_count(p), pred);
    }
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

/// たどりの前後で、ヒープの基準のブロックの位置と状態が同じなら、物体のグラフも同じ形で読める。
pub(crate) proof fn lemma_graph_base(
    b1: Core,
    b0: Core,
    perms: Map<usize, BlockPerm>,
    owner: Map<usize, u32>,
    checked: bool,
)
    requires
        b1.dom() == b0.dom(),
        b1.pages == b0.pages,
        b1.env == b0.env,
        b1.model@.state == b0.model@.state,
    ensures
        graph_of(b1, perms, owner, checked) == graph_of(b0, perms, owner, checked),
{
    reveal(graph_of);
    assert forall|a: usize| #[trigger]
        node_ok(b1, perms, checked, a) == node_ok(b0, perms, checked, a) by {}
    assert(b1.dom().union(b1.region_blocks()).filter(|a: usize| node_ok(b1, perms, checked, a))
        =~= b0.dom().union(b0.region_blocks()).filter(|a: usize| node_ok(b0, perms, checked, a)));
    assert(nodes_of(b1, perms, checked) =~= nodes_of(b0, perms, checked)) by {
        let s = b0.dom().union(b0.region_blocks()).filter(
            |a: usize| node_ok(b0, perms, checked, a),
        );
        assert forall|n: *mut Header|
            nodes_of(b1, perms, checked).contains(n) == nodes_of(b0, perms, checked).contains(
                n,
            ) by {
            s.lemma_map_contains(|a: usize| b1.block(a), n);
            s.lemma_map_contains(|a: usize| b0.block(a), n);
        }
    }
    let g1 = graph_of(b1, perms, owner, checked);
    let g0 = graph_of(b0, perms, owner, checked);
    assert(g1.children =~= g0.children) by {
        assert forall|n: *mut Header| #[trigger]
            g1.children.dom().contains(n) implies g1.children[n] == g0.children[n] by {
            assert(b1.state(n@.addr) == b0.state(n@.addr));
        }
    }
    assert(g1.live =~= g0.live);
    assert(g1.rc =~= g0.rc);
    assert(g1.lin =~= g0.lin);
    assert(g1.owner =~= g0.owner);
}

impl<M: Mode> Heap<M> {
    /// たどりの始まり。預けた権限のグラフは、`share` の前のヒープのグラフで、たどりの前提を満たす。
    proof fn lemma_share_start(
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
            !M::CHECKED ==> old.consistent() && old.settleable() && old.readable(w) && old.share_ok(
                w,
            ),
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
            !M::CHECKED ==> well_formed(o0.gv(), h),
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

    /// `share` のたどりが見る物体のグラフ。
    closed spec fn share_graph(&self) -> GView<*mut Header> {
        let v = self.core.settled_view();
        graph_of(v, v.perms@, v.model@.owner, v.checked())
    }

    /// `w` から届く物体は、`share` の前提 (`share_walk` の `well_formed`) を満たす。
    pub closed spec fn share_ok(&self, w: Word) -> bool {
        well_formed(self.share_graph(), self.core.block(w.bits_spec()))
    }

    /// `w` の物体の根。
    closed spec fn share_root(&self, w: Word) -> *mut Header {
        self.core.block(w.bits_spec())
    }

    /// `w` から、すでに共有の側にある `lin` の物体に届く。
    pub closed spec fn reaches_shared_lin_from(&self, w: Word) -> bool {
        reaches_shared_lin(self.share_graph(), self.share_root(w))
    }

    /// `w` から、生きていない物体に届く。
    pub closed spec fn reaches_dead_from(&self, w: Word) -> bool {
        exists|n: *mut Header| #[trigger]
            reached(self.share_graph(), self.share_root(w), n) && self.share_graph().nodes.contains(
                n,
            ) && !self.share_graph().live[n]
    }

    /// `w` から届く物体は、生きていることのほかは `share` の前提を満たす。
    pub closed spec fn share_sound(&self, w: Word) -> bool {
        sound(self.share_graph(), self.share_root(w))
    }

    /// `share` の後の状態。変わったのは、局所の物体の共有の印と、`lin` の物体の持ち主だけである。
    pub closed spec fn shared(&self, old: Heap<M>, w: Word, receiver: u32) -> bool {
        let g0 = old.share_graph();
        let g = self.share_graph();
        &&& self.dummy == old.dummy
        &&& self.region == old.region
        &&& self.worker == old.worker
        &&& same_shape(g0, g)
        &&& marked_only(g0, g, receiver)
    }

    /// `share` が終わった後の状態。届いた局所の物体に共有の印が付き、`lin` の物体の持ち主が `receiver` になった。
    pub closed spec fn shared_all(&self, old: Heap<M>, w: Word, receiver: u32) -> bool {
        let g0 = old.share_graph();
        let g = self.share_graph();
        let root = old.core.block(w.bits_spec());
        &&& sound(g0, root) ==> forall|n: *mut Header|
            g0.nodes.contains(n) ==> #[trigger] node_post(g0, g, root, receiver, n)
        &&& sound(g0, root) ==> shared_points_to_no_local(g)
    }

    /// w から届く物体を、`receiver` に渡すために共有にする。送り手のスレッドが、値を公開する前に呼ぶ。
    ///
    /// # Safety
    /// `w` は即値か、このヒープか付けた塊の生きている物体を指し、そこから届く物体はどれも生きている。局所の `lin` の
    /// 物体には1回だけ届き、共有の側の `lin` の物体には届かない。
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
        if !w.is_ptr() {
            return Ok(());
        }
        self.settle();
        let ghost mid = *self;
        let h = self.header_of(w);
        let ghost s0 = self.core;
        let mut stack = Vec::new();
        mem::swap(&mut stack, &mut self.share_stack);
        let mut visited = Vec::new();
        mem::swap(&mut visited, &mut self.visited_lin);
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
            env: Tracked(env),
            owner: Ghost(s0.model@.owner),
            base: Ghost(s0),
            _mode: PhantomData,
        };
        let ghost o0 = objects;
        proof {
            self.lemma_share_start(*old(self), mid, o0, h, w);
        }
        let result = share_walk(&mut objects, h, receiver, M::CHECKED, &mut stack, &mut visited);
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
        mem::swap(&mut stack, &mut self.share_stack);
        mem::swap(&mut visited, &mut self.visited_lin);
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

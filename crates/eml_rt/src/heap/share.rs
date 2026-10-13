//! `share` (docs/spec/object-model.md の「`share`」)。たどりは物体への読み書きを
//! `Graph` の裏に置いて書き、Verus がたどりを trait のメソッドの約束の上で証明する。

use std::marker::PhantomData;
use std::mem;
use std::ptr::NonNull;

use super::check::State;
use super::{Heap, check, scan_count, scan_slot};
use crate::error::HeapError;
use crate::meta::{self, Header};
use crate::mode::Mode;
use crate::rc;
#[cfg(verus_only)]
use crate::rc::mark_spec;
use crate::word::Word;
use std::sync::atomic::Ordering;
use vstd::prelude::*;

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
pub(crate) trait Graph {
    type Node: Copy + PartialEq;

    spec fn gv(&self) -> GView<Self::Node>;

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
        ensures
            final(self).gv() == old(self).gv().with_rc(node, rc),
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
        ensures
            final(self).gv() == old(self).gv().with_owner(node, owner),
    ;

    /// 子のうち物体を指すものを `out` に積む。
    fn push_children(&self, node: Self::Node, out: &mut Vec<Self::Node>)
        requires
            self.gv().nodes.contains(node),
        ensures
            final(out)@ == old(out)@ + self.gv().children[node],
    ;

    /// 2つの節点が同じか。節点の型の `==` が値の等しさであることを信頼せずに済むように、比べ方を実装に任せ、結果を
    /// 仕様の等しさで約束させる
    fn same(&self, a: Self::Node, b: Self::Node) -> (r: bool)
        ensures
            r == (a == b),
    ;
}

/// 列に節点があるか。`<[T]>::contains` は vstd に約束がないので、`Graph::same` で比べるループで書く
fn contains_node<G: Graph>(graph: &G, s: &[G::Node], x: G::Node) -> (r: bool)
    ensures
        r == s@.contains(x),
{
    let mut i = 0;
    while i < s.len()
        invariant
            0 <= i <= s.len(),
            forall|j: int| 0 <= j < i ==> s@[j] != x,
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

pub open spec fn well_formed<N>(g: GView<N>, root: N) -> bool {
    &&& g.nodes.contains(root)
    &&& g.live.dom() == g.nodes
    &&& g.rc.dom() == g.nodes
    &&& g.lin.dom() == g.nodes
    &&& g.owner.dom() == g.nodes
    &&& g.children.dom() == g.nodes
    &&& forall|n: N| #[trigger] reached(g, root, n) ==> g.live[n]
    &&& forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() ==> g.nodes.contains(
            #[trigger] g.children[p][k],
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
    &&& well_formed(g0, root)
    &&& same_shape(g0, g)
    &&& e.subset_of(g0.nodes)
    &&& forall|n: N| g0.nodes.contains(n) ==> #[trigger] node_inv(g0, g, receiver, e, n)
    &&& forall|i: int|
        0 <= i < stack.len() ==> g0.nodes.contains(#[trigger] stack[i]) && reached(
            g0,
            root,
            stack[i],
        )
    &&& forall|n: N| #[trigger] e.contains(n) ==> reached(g0, root, n) && expands(g0, n)
    &&& forall|n: N| #[trigger]
        pushed(g0, root, e, n) ==> (e.contains(n) || stops(g0, check_lin, n) || stack.contains(n))
    &&& forall|n: N|
        g0.nodes.contains(n) && #[trigger] g0.lin[n] && g0.rc[n] > 0 ==> count(stack, n) + b2n(
            e.contains(n),
        ) == b2n(pushed(g0, root, e, n))
    &&& check_lin ==> forall|n: N| #[trigger] visited.contains(n) ==> e.contains(n) && g0.lin[n]
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
        well_formed(old(graph).gv(), root),
    ensures
        same_shape(old(graph).gv(), final(graph).gv()),
        r is Ok <==> !(check_lin && reaches_shared_lin(old(graph).gv(), root)),
        r is Err ==> r == Err::<(), ShareError>(ShareError::LinShared),
        r is Ok ==> forall|n: G::Node|
            old(graph).gv().nodes.contains(n) ==> #[trigger] node_post(
                old(graph).gv(),
                final(graph).gv(),
                root,
                receiver,
                n,
            ),
        r is Ok ==> shared_points_to_no_local(final(graph).gv()),
{
    let ghost g0 = graph.gv();
    let ghost mut e: Set<G::Node> = Set::empty();
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
    }
    loop
        invariant
            inv(g0, graph.gv(), root, receiver, check_lin, stack@, visited_lin@, e),
            g0 == old(graph).gv(),
        ensures
            inv(g0, graph.gv(), root, receiver, check_lin, stack@, visited_lin@, e),
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
            lemma_last_counted(s);
            vstd::set_lib::lemma_len_subset(e, g0.nodes);
        }
        if !graph.live(node) {
            return Err(ShareError::Dead);
        }
        let rc = graph.rc(node);
        if rc == 0 {
            proof {
                lemma_skip(g0, graph.gv(), root, receiver, check_lin, s, visited_lin@, e);
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
                if contains_node(graph, visited_lin.as_slice(), node) {
                    return Err(ShareError::LinReachedTwice);
                }
                visited_lin.push(node);
            }
            let ghost g1 = graph.gv();
            graph.set_owner(node, receiver);
            graph.push_children(node, stack);
            proof {
                lemma_expand(g0, g1, graph.gv(), root, receiver, check_lin, s, v1, visited_lin@, e);
                e = e.insert(node);
                vstd::set_lib::lemma_len_subset(e, g0.nodes);
            }
        } else if let Some(shared) = rc::mark_step(rc) {
            let ghost g1 = graph.gv();
            graph.set_rc(node, shared);
            graph.push_children(node, stack);
            proof {
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
                e = e.insert(node);
                vstd::set_lib::lemma_len_subset(e, g0.nodes);
            }
        } else {
            proof {
                lemma_skip(g0, graph.gv(), root, receiver, check_lin, s, visited_lin@, e);
            }
        }
    }
    proof {
        lemma_finish(g0, graph.gv(), root, receiver, check_lin, stack@, visited_lin@, e);
    }
    Ok(())
}

} // verus!

verus! {

/// ヒープの物体のグラフ。`share` の安全の条件 (届く物体はどれも生きている) の下でだけ使う。検査モードは前置きで生存を確かめる。
struct Objects<M>(PhantomData<M>);

// 物体の読み書きはまだ証明していない (信頼の一覧のテストの「未証明」)
impl<M: Mode> Graph for Objects<M> {
    type Node = *mut Header;

    closed spec fn gv(&self) -> GView<*mut Header> {
        arbitrary()
    }

    #[verifier::external_body]
    fn live(&self, h: *mut Header) -> bool {
        let prefix = unsafe { check::prefix(NonNull::new_unchecked(h)) };
        !M::CHECKED || prefix.state() == State::Alive
    }

    #[verifier::external_body]
    fn rc(&self, h: *mut Header) -> i32 {
        let rc = unsafe { meta::rc(NonNull::new_unchecked(h)) };
        rc.load(Ordering::Relaxed)
    }

    #[verifier::external_body]
    fn set_rc(&mut self, h: *mut Header, rc: i32) {
        let cell = unsafe { meta::rc(NonNull::new_unchecked(h)) };
        cell.store(rc, Ordering::Relaxed);
    }

    #[verifier::external_body]
    fn lin(&self, h: *mut Header) -> bool {
        let m = unsafe { meta::meta(NonNull::new_unchecked(h)) };
        m.lin()
    }

    #[verifier::external_body]
    fn set_owner(&mut self, h: *mut Header, owner: u32) {
        if M::CHECKED {
            let prefix = unsafe { check::prefix(NonNull::new_unchecked(h)) };
            prefix.first.store(owner, Ordering::Relaxed);
        }
    }

    #[verifier::external_body]
    fn push_children(&self, h: *mut Header, out: &mut Vec<*mut Header>) {
        let h = unsafe { NonNull::new_unchecked(h) };
        let n = unsafe { scan_count(h) };
        for i in 0..n {
            let child = unsafe { scan_slot(h, i).read() };
            if child.is_ptr() {
                out.push(child.header().as_ptr());
            }
        }
    }

    #[verifier::external_body]
    fn same(&self, a: *mut Header, b: *mut Header) -> bool {
        a == b
    }
}

} // verus!

impl<M: Mode> Heap<M> {
    /// w から届く物体を、`receiver` に渡すために共有にする。送り手のスレッドが、値を公開する前に呼ぶ。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、そこから届く物体はどれも生きている。局所の `lin` の物体には
    /// 1回だけ届き、共有の側の `lin` の物体には届かない。
    pub unsafe fn share(&mut self, w: Word, receiver: u32) -> Result<(), HeapError> {
        if !w.is_ptr() {
            return Ok(());
        }
        unsafe { self.check_live(w.header())? };
        let mut stack = mem::take(&mut self.share_stack);
        let mut visited = mem::take(&mut self.visited_lin);
        let result = share_walk(
            &mut Objects::<M>(PhantomData),
            w.header().as_ptr(),
            receiver,
            M::CHECKED,
            &mut stack,
            &mut visited,
        );
        self.share_stack = stack;
        self.visited_lin = visited;
        result.map_err(|error| match error {
            ShareError::Dead => HeapError::UseAfterFree,
            ShareError::LinReachedTwice => HeapError::LinReachedTwice,
            ShareError::LinShared => HeapError::LinShared,
        })
    }
}

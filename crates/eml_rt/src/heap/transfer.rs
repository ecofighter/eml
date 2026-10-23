//! `transfer_back` (docs/spec/object-model.md の「`transfer_back`」)。たどりは物体のグラフ (`graph.rs`) の上で書く。

use std::marker::PhantomData;
use std::mem;
use vstd::prelude::*;

use super::Heap;
#[cfg(verus_only)]
use super::graph::{
    GView, edge, graph_of, is_path, lemma_graph_base, lemma_is_node, lemma_reach_child,
    lemma_same_frame_graph, lemma_shaped, node_ok, owners_ok, reached, same_frame, same_shape,
    shaped,
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
use crate::word::Word;

verus! {

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferError {
    /// 生きていない物体に届いた。
    Dead,
    /// 子でも今の worker でもない ID の局所の物体に届いた。
    WrongOwner(u32),
}

// 仕様
/// 裏返しのたどりが展開する物体。rc -1 の物体である
pub open spec fn negs<N>(g: GView<N>) -> spec_fn(N) -> bool {
    |n: N| g.rc[n] == -1
}

/// どちらかのスタックのたどりが展開しうる物体。rc -1 の物体と局所の物体である
pub open spec fn tpasses<N>(g: GView<N>) -> spec_fn(N) -> bool {
    |n: N| g.rc[n] == -1 || g.rc[n] > 0
}

/// 裏返す物体。根から rc -1 の物体だけを通る道で届く rc -1 の物体である
pub open spec fn flips<N>(g: GView<N>, root: N, n: N) -> bool {
    reached(g, negs(g), root, n) && g.rc[n] == -1
}

/// たどりが変えるのは、生きている rc -1 の物体を rc 1 にして所有者を `worker` にすることと、生きている物体の所有者を
/// `worker` にすることだけである。所有者を書き換えるのは検査モード (`rewrites`) の局所の物体だけで、所有者を読める
/// (`known`) なら、書き換えた物体の所有者は `from` だった
pub open spec fn returned_only<N>(
    g0: GView<N>,
    g: GView<N>,
    worker: u32,
    from: u32,
    rewrites: bool,
    known: bool,
) -> bool {
    forall|n: N| #[trigger]
        g0.nodes.contains(n) ==> {
            &&& g.rc[n] == g0.rc[n] || (g0.live[n] && g0.rc[n] == -1 && g.rc[n] == 1 && g.owner[n]
                == worker)
            &&& g.owner[n] == g0.owner[n] || (g0.live[n] && g.owner[n] == worker && (g0.rc[n] == -1
                || (rewrites && g0.rc[n] > 0 && (known ==> g0.owner[n] == from))))
        }
}

/// 1つの節点の、たどりが終わった後の rc と所有者。裏返す物体は rc 1 で所有者が `worker` になり、ほかの物体の rc は
/// 変わらない
pub open spec fn flip_post<N>(g0: GView<N>, g: GView<N>, root: N, worker: u32, n: N) -> bool {
    if flips(g0, root, n) {
        g.rc[n] == 1 && g.owner[n] == worker
    } else {
        g.rc[n] == g0.rc[n]
    }
}

/// たどりの途中の1つの節点。裏返した物体 (`flipped`) は rc 1 で所有者が `worker` になり、ほかの物体は rc が変わらず、
/// 所有者は変わらないか、検査モードで `worker` に書き換えた
pub open spec fn node_inv<N>(
    g0: GView<N>,
    g: GView<N>,
    worker: u32,
    from: u32,
    check: bool,
    known: bool,
    flipped: Set<N>,
    n: N,
) -> bool {
    if flipped.contains(n) {
        g.rc[n] == 1 && g.owner[n] == worker && g0.rc[n] == -1 && g0.live[n]
    } else {
        &&& g.rc[n] == g0.rc[n]
        &&& g.owner[n] == g0.owner[n] || (g0.live[n] && g.owner[n] == worker && check && g0.rc[n]
            > 0 && (known ==> g0.owner[n] == from))
    }
}

/// 裏返しのスタックが空になれば、根から rc -1 の物体だけを通って届く rc -1 の物体はどれも裏返してある。そのための、
/// 裏返した物体の子がどれも、裏返したか、スタックにあるか、rc -1 でないことである
pub open spec fn closed_under<N>(g0: GView<N>, root: N, flipped: Set<N>, fs: Seq<N>) -> bool {
    &&& flipped.contains(root) || fs.contains(root) || g0.rc[root] != -1
    &&& forall|p: N, k: int|
        #![trigger flipped.contains(p), g0.children[p][k]]
        flipped.contains(p) && 0 <= k < g0.children[p].len() ==> flipped.contains(g0.children[p][k])
            || fs.contains(g0.children[p][k]) || g0.rc[g0.children[p][k]] != -1
}

/// たどりの間いつも成り立つ不変条件。
pub open spec fn walk_inv<N>(
    g0: GView<N>,
    g: GView<N>,
    root: N,
    worker: u32,
    from: u32,
    check: bool,
    known: bool,
    seen: Set<N>,
    flipped: Set<N>,
    passed: Set<N>,
    fs: Seq<N>,
    os: Seq<N>,
) -> bool {
    &&& shaped(g0, root)
    &&& same_shape(g0, g)
    &&& flipped.subset_of(g0.nodes)
    &&& passed.subset_of(g0.nodes)
    &&& forall|n: N|
        g0.nodes.contains(n) ==> #[trigger] node_inv(g0, g, worker, from, check, known, flipped, n)
    &&& forall|i: int|
        0 <= i < fs.len() ==> g0.nodes.contains(#[trigger] fs[i]) && reached(
            g0,
            negs(g0),
            root,
            fs[i],
        )
    &&& forall|i: int|
        0 <= i < os.len() ==> g0.nodes.contains(#[trigger] os[i]) && reached(
            g0,
            tpasses(g0),
            root,
            os[i],
        )
    &&& forall|n: N| #[trigger] flipped.contains(n) ==> flips(g0, root, n)
    &&& closed_under(g0, root, flipped, fs)
    &&& check ==> seen == passed
    &&& !check ==> os.len() == 0
}

// 補題
/// スタックから取り出して、何もせずに捨てたとき。捨てた物体は裏返したか rc -1 でないので、閉じた形は保たれる
proof fn lemma_drop<N>(g0: GView<N>, root: N, flipped: Set<N>, fs: Seq<N>, x: N)
    requires
        closed_under(g0, root, flipped, fs),
        fs.len() > 0,
        fs.last() == x,
        flipped.contains(x) || g0.rc[x] != -1,
    ensures
        closed_under(g0, root, flipped, fs.drop_last()),
{
    let t = fs.drop_last();
    assert forall|c: N| fs.contains(c) && c != x implies t.contains(c) by {
        let i = choose|i: int| 0 <= i < fs.len() && fs[i] == c;
        assert(i != fs.len() - 1);
        assert(t[i] == c);
    }
}

/// 取り出した `x` を裏返し、子を積んだとき。
proof fn lemma_flip_closed<N>(g0: GView<N>, root: N, flipped: Set<N>, fs: Seq<N>, x: N)
    requires
        closed_under(g0, root, flipped, fs),
        fs.len() > 0,
        fs.last() == x,
    ensures
        closed_under(g0, root, flipped.insert(x), fs.drop_last() + g0.children[x]),
{
    let t = fs.drop_last();
    let f2 = flipped.insert(x);
    let s2 = t + g0.children[x];
    assert forall|c: N| fs.contains(c) && c != x implies s2.contains(c) by {
        let i = choose|i: int| 0 <= i < fs.len() && fs[i] == c;
        assert(i != fs.len() - 1);
        assert(s2[i] == c);
    }
    assert forall|p: N, k: int|
        #![trigger f2.contains(p), g0.children[p][k]]
        f2.contains(p) && 0 <= k < g0.children[p].len() implies f2.contains(g0.children[p][k])
        || s2.contains(g0.children[p][k]) || g0.rc[g0.children[p][k]] != -1 by {
        if p == x {
            assert(s2[t.len() + k] == g0.children[x][k]);
        }
    }
}

/// 裏返しのスタックが空なら、根から rc -1 の物体だけを通って届く rc -1 の物体はどれも裏返してある。道を根から順に
/// たどって示す
proof fn lemma_closed_path<N>(g0: GView<N>, root: N, flipped: Set<N>, path: Seq<N>, i: int)
    requires
        closed_under(g0, root, flipped, Seq::empty()),
        is_path(g0, negs(g0), root, path),
        0 <= i < path.len(),
        g0.rc[path[i]] == -1,
    ensures
        flipped.contains(path[i]),
    decreases i,
{
    if i > 0 {
        let p = path[i - 1];
        assert(negs(g0)(p) && edge(g0, p, path[i]));
        lemma_closed_path(g0, root, flipped, path, i - 1);
        let k = choose|k: int|
            0 <= k < g0.children[p].len() && #[trigger] g0.children[p][k] == path[i];
        assert(flipped.contains(p) && 0 <= k < g0.children[p].len());
        assert(!Seq::<N>::empty().contains(g0.children[p][k]));
    } else {
        assert(!Seq::<N>::empty().contains(root));
    }
}

// たどり
/// `root` から届く物体を、子 `from` から今の worker `worker` へ戻す (docs/spec/object-model.md の「`transfer_back`」)。
/// たどりは2つのスタックを持つ。裏返しのスタック (根はここから始める) から取り出した rc -1 の物体は、rc を 1 にし、
/// 所有者を `worker` にして、子を裏返しのスタックに積む。ほかの物体では止まる。
/// 検査モードでは、裏返しのスタックで止まる局所の物体と、所有者だけのスタックの物体を所有者で見分ける。所有者が
/// `worker` なら止まり、`from` なら所有者を `worker` に書き換えて子を所有者だけのスタックに積み、ほかの ID なら
/// `WrongOwner` を返す。所有者だけのスタックの `lin` でない rc -1 の物体は、子を所有者だけのスタックに積むだけである。
/// 所有者だけのスタックは rc を書かず、裏返しのスタックに積まないので、2つのモードの rc の変化は同じになる。
/// 届いた物体が生きていなければ `Dead` を返す。
// 取り出す前のスタックに名前を付けて証明するので、`while let` でなく `loop` と `match` で書く
#[verifier::spinoff_prover]
pub(crate) fn transfer_walk<G: Graph>(
    graph: &mut G,
    root: G::Node,
    worker: u32,
    from: u32,
    check: bool,
    flip: &mut Vec<G::Node>,
    own: &mut Vec<G::Node>,
) -> (r: Result<(), TransferError>)
    requires
        shaped(old(graph).gv(), root),
        check ==> old(graph).checked() && from != worker && old(graph).seen() == Set::<
            G::Node,
        >::empty(),
        !check ==> forall|n: G::Node|
            old(graph).gv().nodes.contains(n) ==> #[trigger] old(graph).gv().live[n],
    ensures
        same_shape(old(graph).gv(), final(graph).gv()),
        final(graph).kept(*old(graph)),
        returned_only(
            old(graph).gv(),
            final(graph).gv(),
            worker,
            from,
            check,
            check && old(graph).owners_known(),
        ),
        !check ==> r is Ok,
        r == Err::<(), TransferError>(TransferError::Dead) ==> exists|n: G::Node| #[trigger]
            reached(old(graph).gv(), tpasses(old(graph).gv()), root, n) && old(
                graph,
            ).gv().nodes.contains(n) && !old(graph).gv().live[n],
        r matches Err(TransferError::WrongOwner(o)) ==> o != worker && o != from,
        r is Ok ==> forall|n: G::Node|
            old(graph).gv().nodes.contains(n) ==> #[trigger] flip_post(
                old(graph).gv(),
                final(graph).gv(),
                root,
                worker,
                n,
            ),
{
    let ghost g0 = graph.gv();
    let ghost start = *graph;
    let ghost known = check && start.owners_known();
    proof {
        start.lemma_kept_refl();
    }
    flip.clear();
    own.clear();
    flip.push(root);
    let ghost mut flipped = Set::<G::Node>::empty();
    let ghost mut passed = Set::<G::Node>::empty();
    proof {
        lemma_reached_root_negs(g0, root);
        assert(flip@[0] == root);
        assert(flip@.contains(root));
        if check {
            assert(graph.seen() =~= passed);
        }
    }
    loop
        invariant
            walk_inv(
                g0,
                graph.gv(),
                root,
                worker,
                from,
                check,
                known,
                graph.seen(),
                flipped,
                passed,
                flip@,
                own@,
            ),
            check ==> graph.checked() && start.checked() && from != worker,
            known ==> graph.owners_known(),
            known == (check && start.owners_known()),
            !check ==> forall|n: G::Node| g0.nodes.contains(n) ==> #[trigger] g0.live[n],
            g0 == old(graph).gv(),
            start == *old(graph),
            graph.kept(start),
        ensures
            graph.kept(start),
            walk_inv(
                g0,
                graph.gv(),
                root,
                worker,
                from,
                check,
                known,
                graph.seen(),
                flipped,
                passed,
                flip@,
                own@,
            ),
            flip@.len() == 0,
        decreases
                (g0.nodes.len() - flipped.len()) + (g0.nodes.len() - passed.len()),
                flip@.len() + own@.len(),
    {
        let ghost g1 = graph.gv();
        let ghost fs1 = flip@;
        let ghost os1 = own@;
        proof {
            vstd::set_lib::lemma_len_subset(flipped, g0.nodes);
            vstd::set_lib::lemma_len_subset(passed, g0.nodes);
        }
        let mut on_own = false;
        let node = match flip.pop() {
            Some(n) => n,
            None => match own.pop() {
                Some(n) => {
                    on_own = true;
                    n
                },
                None => break,
            },
        };
        proof {
            if on_own {
                assert(fs1.len() == 0);
                assert(own@ =~= os1.drop_last());
                assert(os1[os1.len() - 1] == node);
                assert forall|i: int| 0 <= i < own@.len() implies g0.nodes.contains(
                    #[trigger] own@[i],
                ) && reached(g0, tpasses(g0), root, own@[i]) by {
                    assert(own@[i] == os1[i]);
                }
            } else {
                assert(flip@ =~= fs1.drop_last());
                assert(fs1[fs1.len() - 1] == node);
                assert forall|i: int| 0 <= i < flip@.len() implies g0.nodes.contains(
                    #[trigger] flip@[i],
                ) && reached(g0, negs(g0), root, flip@[i]) by {
                    assert(flip@[i] == fs1[i]);
                }
            }
            assert(g0.nodes.contains(node));
            assert(node_inv(g0, g1, worker, from, check, known, flipped, node));
            if !on_own {
                lemma_negs_tpasses(g0, root, node);
            }
            assert(reached(g0, tpasses(g0), root, node));
        }
        if !graph.live(node) {
            proof {
                lemma_returned(g0, graph.gv(), worker, from, check, known, flipped);
                assert(reached(g0, tpasses(g0), root, node) && g0.nodes.contains(node)
                    && !g0.live[node]);
            }
            return Err(TransferError::Dead);
        }
        let rc = graph.rc(node);
        if !on_own && rc == -1 {
            let ghost before = *graph;
            graph.flip(node, worker);
            proof {
                graph.lemma_kept_trans(before, start);
                before.lemma_kept_known(start);
                graph.lemma_kept_known(start);
            }
            let ghost g2 = graph.gv();
            graph.push_children(node, flip);
            proof {
                // 裏返した物体はどれも rc 1 なので、rc -1 の `node` はまだ裏返していない
                assert(!flipped.contains(node));
                lemma_flip_closed(g0, root, flipped, fs1, node);
                assert(flip@ =~= fs1.drop_last() + g0.children[node]);
                assert forall|n: G::Node| g0.nodes.contains(n) implies #[trigger] node_inv(
                    g0,
                    g2,
                    worker,
                    from,
                    check,
                    known,
                    flipped.insert(node),
                    n,
                ) by {
                    assert(node_inv(g0, g1, worker, from, check, known, flipped, n));
                }
                assert forall|i: int| 0 <= i < flip@.len() implies g0.nodes.contains(
                    #[trigger] flip@[i],
                ) && reached(g0, negs(g0), root, flip@[i]) by {
                    let t = fs1.drop_last();
                    if i < t.len() {
                        assert(flip@[i] == fs1[i]);
                    } else {
                        assert(flip@[i] == g0.children[node][i - t.len()]);
                        lemma_reach_child(g0, negs(g0), root, node, i - t.len());
                    }
                }
                flipped = flipped.insert(node);
                assert(g2.rc.dom() =~= g0.rc.dom());
                assert(g2.owner.dom() =~= g0.owner.dom());
                vstd::set_lib::lemma_len_subset(flipped, g0.nodes);
            }
        } else if check && rc > 0 {
            let owner = graph.owner(node);
            proof {
                if known {
                    assert(owner == g1.owner[node]);
                }
            }
            if owner == worker {
                proof {
                    if !on_own {
                        lemma_drop(g0, root, flipped, fs1, node);
                    }
                }
            } else if owner != from {
                proof {
                    lemma_returned(g0, graph.gv(), worker, from, check, known, flipped);
                }
                return Err(TransferError::WrongOwner(owner));
            } else {
                let ghost before = *graph;
                let fresh = graph.see(node);
                proof {
                    graph.lemma_kept_trans(before, start);
                    graph.lemma_kept_known(start);
                    if !on_own {
                        lemma_drop(g0, root, flipped, fs1, node);
                    }
                }
                if fresh {
                    let ghost before = *graph;
                    graph.set_owner(node, worker);
                    proof {
                        graph.lemma_kept_trans(before, start);
                        graph.lemma_kept_known(start);
                    }
                    let ghost g2 = graph.gv();
                    let ghost os2 = own@;
                    graph.push_children(node, own);
                    proof {
                        assert forall|n: G::Node| g0.nodes.contains(n) implies #[trigger] node_inv(
                            g0,
                            g2,
                            worker,
                            from,
                            check,
                            known,
                            flipped,
                            n,
                        ) by {
                            assert(node_inv(g0, g1, worker, from, check, known, flipped, n));
                        }
                        assert forall|i: int| 0 <= i < own@.len() implies g0.nodes.contains(
                            #[trigger] own@[i],
                        ) && reached(g0, tpasses(g0), root, own@[i]) by {
                            if i < os2.len() {
                                assert(own@[i] == os2[i]);
                            } else {
                                assert(own@[i] == g0.children[node][i - os2.len()]);
                                lemma_reach_child(g0, tpasses(g0), root, node, i - os2.len());
                            }
                        }
                        passed = passed.insert(node);
                        assert(g2.rc.dom() =~= g0.rc.dom());
                        assert(g2.owner.dom() =~= g0.owner.dom());
                        vstd::set_lib::lemma_len_subset(passed, g0.nodes);
                    }
                } else {
                    proof {
                        assert(graph.seen() =~= passed);
                    }
                }
            }
        } else if check && rc == -1 && !graph.lin(node) {
            let ghost before = *graph;
            let fresh = graph.see(node);
            proof {
                graph.lemma_kept_trans(before, start);
                graph.lemma_kept_known(start);
            }
            if fresh {
                let ghost os2 = own@;
                graph.push_children(node, own);
                proof {
                    assert forall|i: int| 0 <= i < own@.len() implies g0.nodes.contains(
                        #[trigger] own@[i],
                    ) && reached(g0, tpasses(g0), root, own@[i]) by {
                        if i < os2.len() {
                            assert(own@[i] == os2[i]);
                        } else {
                            assert(own@[i] == g0.children[node][i - os2.len()]);
                            lemma_reach_child(g0, tpasses(g0), root, node, i - os2.len());
                        }
                    }
                    passed = passed.insert(node);
                    vstd::set_lib::lemma_len_subset(passed, g0.nodes);
                }
            } else {
                proof {
                    assert(graph.seen() =~= passed);
                }
            }
        } else {
            proof {
                if !on_own {
                    lemma_drop(g0, root, flipped, fs1, node);
                }
            }
        }
    }
    proof {
        let g = graph.gv();
        assert(flip@ =~= Seq::<G::Node>::empty());
        assert forall|n: G::Node| g0.nodes.contains(n) implies #[trigger] flip_post(
            g0,
            g,
            root,
            worker,
            n,
        ) by {
            assert(node_inv(g0, g, worker, from, check, known, flipped, n));
            if flips(g0, root, n) {
                let path = choose|path: Seq<G::Node>| #[trigger]
                    is_path(g0, negs(g0), root, path) && path.last() == n;
                lemma_closed_path(g0, root, flipped, path, path.len() - 1);
            }
        }
        lemma_returned(g0, g, worker, from, check, known, flipped);
    }
    Ok(())
}

/// rc -1 の物体だけを通る道で届く物体には、どちらかのスタックのたどりが届く。
proof fn lemma_negs_tpasses<N>(g: GView<N>, root: N, n: N)
    requires
        reached(g, negs(g), root, n),
    ensures
        reached(g, tpasses(g), root, n),
{
    assert forall|m: N| #[trigger] negs(g)(m) implies tpasses(g)(m) by {}
    super::graph::lemma_reached_weaken(g, negs(g), tpasses(g), root, n);
}

proof fn lemma_reached_root_negs<N>(g: GView<N>, root: N)
    ensures
        reached(g, negs(g), root, root),
{
    super::graph::lemma_reached_root(g, negs(g), root);
}

/// 節点ごとの不変条件から、たどりの変えたものを言う。
proof fn lemma_returned<N>(
    g0: GView<N>,
    g: GView<N>,
    worker: u32,
    from: u32,
    check: bool,
    known: bool,
    flipped: Set<N>,
)
    requires
        forall|n: N|
            g0.nodes.contains(n) ==> #[trigger] node_inv(
                g0,
                g,
                worker,
                from,
                check,
                known,
                flipped,
                n,
            ),
    ensures
        returned_only(g0, g, worker, from, check, known),
{
    assert forall|n: N| #[trigger] g0.nodes.contains(n) implies {
        &&& g.rc[n] == g0.rc[n] || (g0.live[n] && g0.rc[n] == -1 && g.rc[n] == 1 && g.owner[n]
            == worker)
        &&& g.owner[n] == g0.owner[n] || (g0.live[n] && g.owner[n] == worker && (g0.rc[n] == -1 || (
        check && g0.rc[n] > 0 && (known ==> g0.owner[n] == from))))
    } by {
        assert(node_inv(g0, g, worker, from, check, known, flipped, n));
    }
}

/// たどりの後のヒープ。権限は rc の値と前置きの所有者の値だけが変わり、モデルは持ち主だけを替える。rc -1 と rc 1 は
/// どちらも張り付く数でなく、符号を除いた値が同じなので、裏返してもモデルの参照の数との対応は崩れない
proof fn lemma_returned_wf(s0: Core, c1: Core, worker: u32, from: u32, known: bool)
    requires
        s0.full(),
        s0.model@.building is None,
        s0.quiet(),
        c1.pages == s0.pages,
        c1.env == s0.env,
        same_frame(s0, c1.perms@, s0.perms@, s0.checked()),
        returned_only(
            graph_of(s0, s0.perms@, s0.model@.owner, s0.checked()),
            graph_of(s0, c1.perms@, c1.model@.owner, s0.checked()),
            worker,
            from,
            s0.checked(),
            known,
        ),
        c1.model@.owner.dom() == s0.model@.owner.dom(),
        owners_ok(s0, c1.perms@, c1.model@.owner, s0.checked()),
        forall|a: usize|
            #![trigger c1.model@.owner[a]]
            !node_ok(s0, s0.perms@, s0.checked(), a) ==> c1.model@.owner[a] == s0.model@.owner[a],
        c1.model@ == (Model { owner: c1.model@.owner, ..s0.model@ }),
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
            assert(node_ok(s0, c1.perms@, checked, b));
            if x.prefix is Some {
                assert(p_code(x) == p_code(y));
                if x.rc.value() > 0 && (s0.state(b) == BlockState::Live || s0.state(b)
                    == BlockState::Building) {
                    if s0.in_region(b) {
                        s0.lemma_region_rc(b);
                    }
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
    /// `transfer_back` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn transfer_ok(&self, w: Word) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.readable(w)
    }

    /// `w` から、`transfer_back` のたどりが生きていない物体に届く。
    pub closed spec fn transfer_reaches_dead(&self, w: Word) -> bool {
        let g = self.share_graph();
        exists|n: *mut Header| #[trigger]
            reached(g, tpasses(g), self.share_root(w), n) && g.nodes.contains(n) && !g.live[n]
    }

    /// `transfer_back` の後の状態。変えたのは、生きている rc -1 の物体を今の worker の局所の物体にしたことと、生きて
    /// いる局所の物体の所有者を今の worker にしたことだけである。検査モードで前提が成り立っていれば、所有者を書き換えた
    /// 物体の所有者は `from` だった。
    pub closed spec fn returned(&self, old: Heap<M>, w: Word, from: u32) -> bool {
        let g0 = old.share_graph();
        let g = self.share_graph();
        &&& self.dummy == old.dummy
        &&& self.region == old.region
        &&& self.worker == old.worker
        &&& same_shape(g0, g)
        &&& returned_only(g0, g, old.worker, from, M::CHECKED, M::CHECKED && old.transfer_ok(w))
    }

    /// `transfer_back` が終わった後の状態。根から rc -1 の物体だけを通って届く rc -1 の物体が rc 1 になって所有者が
    /// 今の worker になり、ほかの物体の rc は変わらない。
    pub closed spec fn returned_all(&self, old: Heap<M>, w: Word) -> bool {
        let g0 = old.share_graph();
        let g = self.share_graph();
        let root = old.share_root(w);
        forall|n: *mut Header|
            g0.nodes.contains(n) ==> #[trigger] flip_post(g0, g, root, old.worker, n)
    }

    /// たどりの後で、権限を戻してモデルを替えたヒープは不変条件を満たし、`returned` の状態にある。
    proof fn lemma_transfer_end(
        &self,
        old: Heap<M>,
        mid: Heap<M>,
        o0: Objects<M>,
        o1: Objects<M>,
        h: *mut Header,
        w: Word,
        from: u32,
        ok: bool,
        c: bool,
    )
        requires
            self.building_tied(),
            self.tokens@ == self.core.model@.tokens,
            old.wf(),
            !M::CHECKED ==> c,
            c == old.transfer_ok(w),
            c ==> mid.consistent(),
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
            old.share_graph() == o0.gv(),
            old.share_root(w) == h,
            returned_only(
                o0.gv(),
                o1.gv(),
                mid.worker,
                from,
                M::CHECKED,
                M::CHECKED && o0.owners_known(),
            ),
            ok ==> forall|n: *mut Header|
                o0.gv().nodes.contains(n) ==> #[trigger] flip_post(
                    o0.gv(),
                    o1.gv(),
                    h,
                    mid.worker,
                    n,
                ),
            self.dummy == mid.dummy,
            self.region == mid.region,
            self.worker == mid.worker,
            self.core.pages == mid.core.pages,
            self.core.env == mid.core.env,
            self.core.perms@ == o1.perms@,
            self.core.model@ == (Model { owner: o1.owner@, ..mid.core.model@ }),
        ensures
            self.wf(),
            c ==> self.consistent(),
            self.returned(old, w, from),
            ok ==> self.returned_all(old, w),
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
                    node_ok(s0, s0.perms@, checked, a) && s0.state(a) == BlockState::Live && (
                    s0.rc_of(a) > 0 || (!s0.in_region(a) && s0.perms@[a].rc.value() > 0))
                        && s0.perms@[a].prefix is Some implies p_owner(s0.perms@[a])
                    == s0.model@.owner[a] by {
                    if s0.in_region(a) {
                        s0.lemma_region_rc(a);
                    }
                    assert(s0.block_wf(a));
                }
            }
            assert(o0.owners_known());
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
            lemma_returned_wf(s0, self.core, mid.worker, from, M::CHECKED && o0.owners_known());
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
        // ダミーの物体は不死なので、rc も持ち主も変わらない
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
        assert(self.share_graph() == o1.gv());
    }

    /// 子 `from` の値 `w` を、今の worker の局所の値に戻す (docs/spec/object-model.md の「`transfer_back`」)。join で、
    /// 右の子の値を受け取った後、`retire_worker(from)` の前に呼ぶ。根から rc -1 の物体だけを通って届く rc -1 の物体を
    /// rc 1 の局所の物体にし、所有者を今の worker にする。検査モードでは、届いた局所の物体の所有者が `from` なら今の
    /// worker に書き換え、ほかの ID なら `WrongOwner` を返す。`from` が今の worker なら、何も変えずに `WrongOwner` を
    /// 返す。
    ///
    /// # Safety
    /// `w` は即値か、このヒープか付けた塊の生きている物体を指し、そこから届く物体はどれも生きている。裏返す rc -1 の物体を
    /// 指す参照は、呼ぶ側が持つ値の中にしかない。
    #[verifier::spinoff_prover]
    pub unsafe fn transfer_back(&mut self, w: Word, from: u32) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).transfer_ok(w),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            old(self).transfer_ok(w) ==> final(self).consistent(),
            M::CHECKED && from == old(self).worker_spec() ==> r == Err::<(), HeapError>(
                HeapError::WrongOwner { owner: from, worker: from },
            ) && *final(self) == *old(self),
            w.is_ptr_spec() && !(M::CHECKED && from == old(self).worker_spec())
                ==> final(self).returned(*old(self), w, from),
            w.is_ptr_spec() && r is Ok ==> final(self).returned_all(*old(self), w),
            r == Err::<(), HeapError>(HeapError::UseAfterFree) ==> w.is_ptr_spec() && old(
                self,
            ).transfer_reaches_dead(w),
            M::CHECKED && old(self).transfer_ok(w) && !(w.is_ptr_spec() && old(
                self,
            ).transfer_reaches_dead(w)) ==> r is Ok || r matches Err(HeapError::WrongOwner { .. }),
    {
        let ghost pre_ok = self.transfer_ok(w);
        let worker = self.worker;
        // 子の値を親へ戻す操作なので、子と親が同じ ID なのは呼ぶ側の誤りである。何も変えずに返す
        if M::CHECKED && from == worker {
            return Err(HeapError::WrongOwner { owner: from, worker });
        }
        if !w.is_ptr() {
            return Ok(());
        }
        self.settle();
        let ghost mid = *self;
        let h = self.header_of(w);
        let ghost s0 = self.core;
        let mut flip = Vec::new();
        mem::swap(&mut flip, &mut self.mark_stack);
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
            reveal(graph_of);
        }
        let result = transfer_walk(&mut objects, h, worker, from, M::CHECKED, &mut flip, &mut own);
        let ghost o1 = objects;
        proof {
            vstd::modes::tracked_swap(objects.perms.borrow_mut(), self.core.perms.borrow_mut());
            vstd::modes::tracked_swap(objects.env.borrow_mut(), self.core.env.borrow_mut());
            self.core.model = Ghost(Model { owner: o1.owner@, ..s0.model@ });
            self.lemma_transfer_end(*old(self), mid, o0, o1, h, w, from, result is Ok, pre_ok);
        }
        mem::swap(&mut flip, &mut self.mark_stack);
        mem::swap(&mut own, &mut self.own_stack);
        if M::CHECKED {
            mem::swap(&mut objects.seen, &mut self.seen);
        }
        if M::CHECKED {
            mem::swap(&mut objects.owned, &mut self.owned);
        }
        match result {
            Ok(()) => Ok(()),
            Err(TransferError::Dead) => Err(HeapError::UseAfterFree),
            Err(TransferError::WrongOwner(owner)) => Err(HeapError::WrongOwner { owner, worker }),
        }
    }
}

} // verus!

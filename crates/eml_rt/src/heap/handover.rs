//! `release` と `release_reuse` の証明の部品。残すフィールドの参照は、どちらの道でも呼ぶ側に1つずつ渡る。最後の参照の
//! 道では辺が外の参照に移り、そうでない道では `dup` が外の参照を足す。どちらも `evolved_by` の同じ数の変化になる。

use vstd::prelude::*;

use super::Core;
#[cfg(verus_only)]
use super::chain::listed;
#[cfg(verus_only)]
use super::inv::{p_count, p_first, p_word};
#[cfg(verus_only)]
use super::model::{BlockState, Model, Pos, indeg, lemma_indeg_insert};
#[cfg(verus_only)]
use super::states::{lemma_pos_set, pos_set};
#[cfg(verus_only)]
use crate::meta::Header;
#[cfg(verus_only)]
use crate::word::Word;

verus! {

/// 走査する語の i 番目より前のうち、残すフィールドの物理的な添字。
pub(crate) open spec fn kept_set(first: int, keep: Seq<bool>, i: int) -> Set<int> {
    vstd::set_lib::set_int_range(first, first + i).filter(|j: int| keep[j - first])
}

/// 外の参照の数の変化。残すフィールドの辺 `k` の行き先は1つずつ増え、手放す物体 `h` は1つ減る。
pub(crate) open spec fn handover(k: Map<Pos, usize>, h: usize) -> spec_fn(usize) -> int {
    |b: usize|
        indeg(k, b) - if b == h {
            1int
        } else {
            0int
        }
}

/// `kept_set` の要素は、範囲の中の残すフィールドの添字である。
pub(crate) proof fn lemma_kept_set(first: int, keep: Seq<bool>, i: int)
    requires
        0 <= i,
    ensures
        forall|j: int| #[trigger]
            kept_set(first, keep, i).contains(j) <==> first <= j < first + i && keep[j - first],
{
    vstd::set_lib::lemma_int_range(first, first + i);
}

impl Core {
    /// `h` から出る辺。
    pub(crate) open spec fn out_edges(&self, h: usize) -> Map<Pos, usize> {
        self.model@.edges.restrict(self.model@.edges.dom().filter(|e: Pos| e.0 == h))
    }

    /// `h` の残すフィールドの辺。
    pub(crate) open spec fn kept_out(&self, h: usize, keep: Seq<bool>) -> Map<Pos, usize> {
        let first = p_first(self.perm(h));
        self.model@.edges.restrict(pos_set(h, kept_set(first, keep, keep.len() as int)))
    }

    /// `old` の token の箱は、同じ権限のまま token の箱である。
    pub(crate) open spec fn tokens_kept(&self, old: Core) -> bool {
        forall|b: usize|
            #![trigger old.state(b)]
            old.dom().contains(b) && old.state(b) == BlockState::Token ==> self.dom().contains(b)
                && self.state(b) == BlockState::Token && self.perm(b) == old.perm(b)
    }

    /// `evolved` の、物体ごとに数の変化 `d(b)` が違う形。
    pub(crate) open spec fn evolved_by(&self, old: Core, d: spec_fn(usize) -> int) -> bool {
        &&& self.checked == old.checked
        &&& forall|b: usize|
            #![trigger old.dom().contains(b)]
            old.dom().contains(b) && old.settled_state(b) == BlockState::Live && (old.rc_of(b) == 0
                || old.ext(b) + d(b) >= 1) ==> {
                &&& self.dom().contains(b)
                &&& self.state(b) == BlockState::Live
                &&& self.perm(b).meta == old.perm(b).meta
                &&& self.perm(b).slots == old.perm(b).slots
                &&& self.block(b) == old.block(b)
                &&& self.cap(b) == old.cap(b)
                &&& (old.rc_of(b) == 0) == (self.rc_of(b) == 0)
                &&& old.rc_of(b) != 0 ==> self.ext(b) == old.ext(b) + d(b)
                &&& self.model@.owner[b] == old.model@.owner[b]
            }
    }

    /// `dup` の後の状態。`h` の rc と数だけが動く (不死の物体なら数も動かない)。
    pub(crate) open spec fn bumped(&self, old: Core, h: usize) -> bool {
        &&& self.pages == old.pages
        &&& self.checked == old.checked
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& old.dom().contains(h) ==> {
            &&& self.perm(h).meta == old.perm(h).meta
            &&& self.perm(h).slots == old.perm(h).slots
            &&& self.perm(h).prefix == old.perm(h).prefix
            &&& (old.rc_of(h) == 0) == (self.rc_of(h) == 0)
            &&& (old.rc_of(h) > 0) == (self.rc_of(h) > 0)
            &&& self.model@.refs[h] == old.model@.refs[h] + if old.rc_of(h) != 0 {
                1nat
            } else {
                0nat
            }
        }
        &&& self.model@.refs.dom() == old.model@.refs.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.model@.refs[b] == old.model@.refs[b]
        &&& self.model@.state == old.model@.state
        &&& self.model@.edges == old.model@.edges
        &&& self.model@.owner == old.model@.owner
        &&& self.model@.written == old.model@.written
        &&& self.model@.kept == old.model@.kept
        &&& self.model@.building == old.model@.building
    }

    /// `old` から、辺 `k` の行き先ごとに1つずつ `dup` した状態。
    pub(crate) open spec fn dupped(&self, old: Core, k: Map<Pos, usize>) -> bool {
        &&& self.pages == old.pages
        &&& self.checked == old.checked
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| #[trigger]
            old.dom().contains(b) ==> {
                &&& self.perm(b).meta == old.perm(b).meta
                &&& self.perm(b).slots == old.perm(b).slots
                &&& self.perm(b).prefix == old.perm(b).prefix
                &&& (old.rc_of(b) == 0) == (self.rc_of(b) == 0)
                &&& (old.rc_of(b) > 0) == (self.rc_of(b) > 0)
                &&& self.model@.refs[b] == old.model@.refs[b] + if old.rc_of(b) != 0 {
                    indeg(k, b)
                } else {
                    0nat
                }
            }
        &&& self.model@.state == old.model@.state
        &&& self.model@.edges == old.model@.edges
        &&& self.model@.owner == old.model@.owner
        &&& self.model@.written == old.model@.written
        &&& self.model@.kept == old.model@.kept
        &&& self.model@.building == old.model@.building
    }

    /// 外の参照の数を変えない変化を後に続けても、`evolved_by` は変わらない。
    pub(crate) proof fn lemma_by_trans(
        &self,
        mid: Core,
        s0: Core,
        d: spec_fn(usize) -> int,
        x: usize,
    )
        requires
            mid.evolved_by(s0, d),
            self.evolved(mid, x, 0),
        ensures
            self.evolved_by(s0, d),
    {
        assert forall|b: usize|
            #![trigger s0.dom().contains(b)]
            s0.dom().contains(b) && s0.settled_state(b) == BlockState::Live && (s0.rc_of(b) == 0
                || s0.ext(b) + d(b) >= 1) implies {
            &&& self.dom().contains(b)
            &&& self.state(b) == BlockState::Live
            &&& self.perm(b).meta == s0.perm(b).meta
            &&& self.perm(b).slots == s0.perm(b).slots
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
            &&& (s0.rc_of(b) == 0) == (self.rc_of(b) == 0)
            &&& s0.rc_of(b) != 0 ==> self.ext(b) == s0.ext(b) + d(b)
            &&& self.model@.owner[b] == s0.model@.owner[b]
        } by {
            assert(mid.dom().contains(b));
            assert(mid.state(b) == BlockState::Live);
            assert(mid.settled_state(b) == BlockState::Live);
            assert(mid.rc_of(b) == 0 || mid.ext(b) + (if b == x {
                0int
            } else {
                0
            }) >= 1);
        }
    }

    /// `begin_free` の後。残すフィールドの辺 `gone` の行き先は外の参照が増え、`h` はもう生きていない。
    pub(crate) proof fn lemma_begin_evolved(&self, s0: Core, h: usize, ps: Set<Pos>)
        requires
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            s0.model@.building is None,
            s0.indeg(h) == 0,
            s0.rc_of(h) != 0,
            s0.model@.refs[h] == 1,
            self.perms == s0.perms,
            self.pages == s0.pages,
            self.checked == s0.checked,
            self.model@ == (Model {
                state: s0.model@.state.insert(h, BlockState::Dying),
                edges: s0.model@.edges.remove_keys(ps),
                ..s0.model@
            }),
            forall|t: usize| #[trigger]
                self.indeg(t) + indeg(s0.model@.edges.restrict(ps), t) == s0.indeg(t),
        ensures
            self.evolved_by(s0, handover(s0.model@.edges.restrict(ps), h)),
    {
        let gone = s0.model@.edges.restrict(ps);
        let d = handover(gone, h);
        assert(indeg(gone, h) == 0) by {
            assert(self.indeg(h) + indeg(gone, h) == s0.indeg(h));
        }
        assert forall|b: usize|
            #![trigger s0.dom().contains(b)]
            s0.dom().contains(b) && s0.settled_state(b) == BlockState::Live && (s0.rc_of(b) == 0
                || s0.ext(b) + d(b) >= 1) implies {
            &&& self.dom().contains(b)
            &&& self.state(b) == BlockState::Live
            &&& self.perm(b).meta == s0.perm(b).meta
            &&& self.perm(b).slots == s0.perm(b).slots
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
            &&& (s0.rc_of(b) == 0) == (self.rc_of(b) == 0)
            &&& s0.rc_of(b) != 0 ==> self.ext(b) == s0.ext(b) + d(b)
            &&& self.model@.owner[b] == s0.model@.owner[b]
        } by {
            assert(b != h);
            assert(self.indeg(b) + indeg(gone, b) == s0.indeg(b));
        }
    }

    /// 呼ぶ側が最後の参照を持つ物体 (rc が 1 か -1) は、数えた参照が1つで、それを指す辺はない。
    pub(crate) proof fn lemma_last_ref(&self, h: usize)
        requires
            self.wf(),
            self.dom().contains(h),
            self.state(h) == BlockState::Live,
            self.rc_of(h) == 1 || self.rc_of(h) == -1,
            self.ext(h) >= 1,
        ensures
            self.indeg(h) == 0,
            self.model@.refs[h] == 1,
    {
        reveal(Core::refs_wf);
        assert(self.counted_wf(h));
    }

    /// 走査する語を順に足すと、残すフィールドの辺は1つずつ増える。
    pub(crate) proof fn lemma_kept_out_step(&self, a: usize, first: int, keep: Seq<bool>, i: int)
        requires
            0 <= i < keep.len(),
        ensures
            ({
                let k0 = self.model@.edges.restrict(pos_set(a, kept_set(first, keep, i)));
                let k1 = self.model@.edges.restrict(pos_set(a, kept_set(first, keep, i + 1)));
                let p = (a, first + i);
                &&& !k0.dom().contains(p)
                &&& k1 == if keep[i] && self.model@.edges.dom().contains(p) {
                    k0.insert(p, self.model@.edges[p])
                } else {
                    k0
                }
            }),
    {
        let e = self.model@.edges;
        let k0 = e.restrict(pos_set(a, kept_set(first, keep, i)));
        let k1 = e.restrict(pos_set(a, kept_set(first, keep, i + 1)));
        let p = (a, first + i);
        lemma_kept_set(first, keep, i);
        lemma_kept_set(first, keep, i + 1);
        lemma_pos_set(a, kept_set(first, keep, i));
        lemma_pos_set(a, kept_set(first, keep, i + 1));
        if keep[i] && e.dom().contains(p) {
            assert(k1 =~= k0.insert(p, e[p]));
        } else {
            assert(k1 =~= k0);
        }
    }

    /// 物体を `release` で手放すとき、残すフィールドを全部にすると、その物体から出る辺が全部呼ぶ側に渡る。`is_last` と
    /// `free_block` を並べた展開は、この `release` と同じ数の変化になる。
    pub(crate) proof fn lemma_release_unrolled(&self, h: usize, n: int)
        requires
            self.wf(),
            self.dom().contains(h),
            self.state(h) == BlockState::Live,
            n == p_count(self.perm(h)),
        ensures
            self.kept_out(h, Seq::new(n as nat, |_i: int| true)) == self.out_edges(h),
    {
        reveal(Core::edges_wf);
        let keep = Seq::new(n as nat, |_i: int| true);
        let first = p_first(self.perm(h));
        lemma_kept_set(first, keep, n);
        lemma_pos_set(h, kept_set(first, keep, n));
        assert forall|e: Pos| #[trigger]
            self.model@.edges.dom().contains(e) && e.0 == h implies pos_set(
            h,
            kept_set(first, keep, n),
        ).contains(e) by {
            assert(self.edge_wf(e));
        }
        assert(self.kept_out(h, keep) =~= self.out_edges(h));
    }

    /// 空けた箱の後も、ほかの物体は変わらない。
    pub(crate) proof fn lemma_boxed_evolved(&self, pre: Core, a: usize)
        requires
            self.boxed_off(pre, a),
            pre.model@.building is None,
            pre.dom().contains(a),
            pre.state(a) == BlockState::Dying,
        ensures
            self.evolved(pre, 0, 0),
            (forall|b: usize| #[trigger]
                pre.dom().contains(b) && b != a ==> pre.state(b) != BlockState::Dying)
                ==> self.quiet(),
    {
        assert forall|b: usize|
            #![trigger pre.dom().contains(b)]
            pre.dom().contains(b) && pre.settled_state(b) == BlockState::Live && (pre.rc_of(b) == 0
                || pre.ext(b) + (if b == 0 {
                0int
            } else {
                0
            }) >= 1) implies self.dom().contains(b) && self.perm(b) == pre.perm(b) && self.ext(b)
            == pre.ext(b) by {
            assert(b != a);
        }
        if forall|b: usize| #[trigger]
            pre.dom().contains(b) && b != a ==> pre.state(b) != BlockState::Dying {
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.state(b)
                != BlockState::Dying by {
                assert(pre.dom().contains(b));
            }
        }
    }

    /// `dup` の列の始まり。
    pub(crate) proof fn lemma_dupped_start(&self)
        ensures
            self.dupped(*self, Map::<Pos, usize>::empty()),
    {
        assert forall|b: usize| #[trigger] indeg(Map::<Pos, usize>::empty(), b) == 0 by {
            assert(super::model::targets(Map::<Pos, usize>::empty(), b) =~= Set::empty());
        }
    }

    /// 残すフィールド `p` の行き先 `c` を `dup` した。
    pub(crate) proof fn lemma_dupped_step(
        &self,
        s0: Core,
        mid: Core,
        k: Map<Pos, usize>,
        p: Pos,
        c: usize,
    )
        requires
            mid.dupped(s0, k),
            self.bumped(mid, c),
            !k.dom().contains(p),
            s0.dom().contains(c),
        ensures
            self.dupped(s0, k.insert(p, c)),
    {
        lemma_indeg_insert(k, p, c);
    }

    /// 即値の残すフィールドの `dup` は、何も変えない。
    pub(crate) proof fn lemma_dupped_skip(&self, s0: Core, mid: Core, k: Map<Pos, usize>, c: usize)
        requires
            mid.dupped(s0, k),
            self.bumped(mid, c),
            !s0.dom().contains(c),
        ensures
            self.dupped(s0, k),
    {
    }

    /// 残すフィールドを `dup` し終え、`h` の参照を手放した。
    pub(crate) proof fn lemma_dupped_end(&self, s0: Core, mid: Core, k: Map<Pos, usize>, h: usize)
        requires
            s0.model@.building is None,
            mid.dupped(s0, k),
            self.evolved(mid, h, -1),
        ensures
            self.evolved_by(s0, handover(k, h)),
    {
        let d = handover(k, h);
        assert forall|b: usize|
            #![trigger s0.dom().contains(b)]
            s0.dom().contains(b) && s0.settled_state(b) == BlockState::Live && (s0.rc_of(b) == 0
                || s0.ext(b) + d(b) >= 1) implies {
            &&& self.dom().contains(b)
            &&& self.state(b) == BlockState::Live
            &&& self.perm(b).meta == s0.perm(b).meta
            &&& self.perm(b).slots == s0.perm(b).slots
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
            &&& (s0.rc_of(b) == 0) == (self.rc_of(b) == 0)
            &&& s0.rc_of(b) != 0 ==> self.ext(b) == s0.ext(b) + d(b)
            &&& self.model@.owner[b] == s0.model@.owner[b]
        } by {
            assert(mid.dom().contains(b));
            assert(mid.state(b) == BlockState::Live);
            assert(mid.settled_state(b) == BlockState::Live);
            assert(mid.indeg(b) == s0.indeg(b));
        }
    }

    /// 最後の参照の道の始まり。`begin_free` で `skip` の位置の辺を外した後、`h` の子を走査する語の終わりから手放し始める。
    pub(crate) proof fn lemma_release_start(&self, s0: Core, h: *mut Header, skip: Set<int>)
        requires
            s0.wf(),
            s0.quiet(),
            s0.dom().contains(h@.addr),
            h == s0.block(h@.addr),
            s0.state(h@.addr) == BlockState::Live,
            self.perms == s0.perms,
            self.pages == s0.pages,
            self.model@ == (Model {
                state: s0.model@.state.insert(h@.addr, BlockState::Dying),
                edges: s0.model@.edges.remove_keys(pos_set(h@.addr, skip)),
                ..s0.model@
            }),
        ensures
            self.draining(
                h,
                p_first(s0.perm(h@.addr)),
                p_count(s0.perm(h@.addr)),
                skip,
                Seq::empty(),
            ),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        let a = h@.addr;
        let p = s0.perm(a);
        let first = p_first(p);
        let n = p_count(p);
        lemma_pos_set(a, skip);
        assert(s0.block_wf(a));
        assert forall|j: int|
            #![trigger self.model@.edges.dom().contains((a, j))]
            self.model@.edges.dom().contains((a, j)) implies first <= j < first + n
            && !skip.contains(j) by {
            assert(s0.edge_wf((a, j)));
        }
        assert forall|j: int|
            #![trigger p_word(self.perm(a), j)]
            first <= j < first + n && !skip.contains(j) && p_word(
                self.perm(a),
                j,
            ).is_ptr_spec() implies self.model@.edges.dom().contains((a, j)) by {
            assert(s0.scanned(a, j));
            assert(s0.slot_word(p, a, j));
        }
        assert forall|b: usize|
            #![trigger self.state(b)]
            self.dom().contains(b) && self.state(b) == BlockState::Dying && Some(a) != Some(
                b,
            ) implies listed(Seq::<Word>::empty(), b) by {
            assert(s0.dom().contains(b));
        }
    }

    /// 子を手放し終えた空ける途中の物体 `h` を token の箱にした。作業リストはまた揃う。
    pub(crate) proof fn lemma_tokened(
        &self,
        pre: Core,
        h: *mut Header,
        first: int,
        skip: Set<int>,
        work: Seq<Word>,
    )
        requires
            pre.draining(h, first, 0, skip, work),
            pre.model@.building is None,
            self.perms == pre.perms,
            self.pages == pre.pages,
            self.checked == pre.checked,
            self.model@.state == pre.model@.state.insert(h@.addr, BlockState::Token),
            self.model@.edges == pre.model@.edges,
            self.model@.refs == pre.model@.refs,
            self.model@.owner == pre.model@.owner,
            self.model@.building == pre.model@.building,
        ensures
            self.work_ok(work),
            self.evolved(pre, 0, 0),
    {
        let a = h@.addr;
        assert forall|k: int| #![trigger work[k]] 0 <= k < work.len() implies self.dying_word(
            work[k],
        ) && None::<usize> != Some(work[k].bits_spec()) by {
            assert(pre.dying_word(work[k]));
            let b = work[k].bits_spec();
            assert forall|j: int|
                #![trigger self.scanned(b, j)]
                self.scanned(b, j) && p_word(
                    self.perm(b),
                    j,
                ).is_ptr_spec() implies self.model@.edges.dom().contains((b, j)) by {
                assert(pre.scanned(b, j));
            }
        }
        assert forall|b: usize|
            #![trigger self.state(b)]
            self.dom().contains(b) && self.state(b) == BlockState::Dying && None::<usize> != Some(
                b,
            ) implies listed(work, b) by {
            assert(b != a);
            assert(pre.state(b) == BlockState::Dying);
        }
    }
}

} // verus!

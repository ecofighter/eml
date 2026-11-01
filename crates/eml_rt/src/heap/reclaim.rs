//! 別のスレッドからの解放の取り込み (docs/spec/object-model.md の「別のスレッドからの解放」)。ほかのスレッドがこの
//! ヒープのページに積んだブロックを、持ち主のスレッドが取り出して空きリストに戻す (検査モードでは隔離する)。
//!
//! 持ち主のモデルでは、ほかのスレッドが空けたブロックは取り込むまで生きているままである。取り込むときに、
//! `remote_take` の約束する集合 (`Core::freed_set`) を空けた状態にし、その集合から出る辺を外す。速いモードでは、その後で
//! 取り出したブロックをモデルから外して確保器に返す。

use vstd::prelude::*;

use super::Core;
use super::Heap;
use super::check::mark_freed;
#[cfg(verus_only)]
use super::inv::{p_scanned, p_word};
#[cfg(verus_only)]
use super::model::{BlockState, Model};
#[cfg(verus_only)]
use super::model::{Pos, indeg, targets};
use crate::meta::Header;
use crate::mode::Mode;
#[cfg(verus_only)]
use crate::remote::taken;
use crate::remote::{
    remote_large_take, remote_take, sink_count, sink_count_take, sink_large_pending,
};
#[cfg(verus_only)]
use crate::word::Word;

verus! {

/// 受けた解放の数がこれに届いたら、`reclaim_if_due` が取り込む (docs/spec/object-model.md の「別のスレッドからの解放」)。
pub(crate) const REMOTE_RECLAIM: usize = 256;

impl Core {
    /// `r` は、ほかのスレッドが空けたブロックを含む、持ち主から見て空けたブロックの集合である (信頼の一覧の T14 の
    /// `remote_take` の約束)。どれもこのヒープのブロックで、生きているか空けた状態にあり、生きているものは rc が 0 で
    /// なく、`held` (呼ぶ側の持つ物体) と重ならない。速いモードでは、辺でそこへ届くブロックはみな `r` にある。
    pub(crate) open spec fn freed_set(&self, r: Set<usize>, held: Set<usize>) -> bool {
        &&& r.subset_of(self.dom())
        &&& forall|b: usize| #[trigger]
            r.contains(b) ==> !held.contains(b) && (self.state(b) == BlockState::Freed || (
            self.state(b) == BlockState::Live && self.rc_of(b) != 0))
        &&& !self.checked() ==> forall|e: Pos| #[trigger]
            self.model@.edges.dom().contains(e) && r.contains(self.model@.edges[e]) ==> r.contains(
                e.0,
            )
    }

    /// `r` のブロックを空けた状態にし、そこから出る辺を外したモデル。
    pub(crate) open spec fn freed_model(&self, r: Set<usize>) -> Model {
        Model {
            state: Map::new(
                self.model@.state.dom(),
                |b: usize|
                    if r.contains(b) {
                        BlockState::Freed
                    } else {
                        self.model@.state[b]
                    },
            ),
            edges: self.model@.edges.restrict(
                self.model@.edges.dom().filter(|e: Pos| !r.contains(e.0)),
            ),
            ..self.model@
        }
    }

    /// 辺を外しても、どの物体を指す辺の数も増えない。
    proof fn lemma_freed_indeg(&self, r: Set<usize>, o: usize)
        ensures
            indeg(self.freed_model(r).edges, o) <= indeg(self.model@.edges, o),
    {
        let e1 = self.freed_model(r).edges;
        assert(targets(e1, o).subset_of(targets(self.model@.edges, o)));
        vstd::set_lib::lemma_len_subset(targets(e1, o), targets(self.model@.edges, o));
    }

    /// 速いモードで、`freed_set` の集合 `r` を空けた状態にしても、モデルとの対応は保たれる。`r` のブロックを指す辺も
    /// `r` から出る辺もなくなる。
    pub(crate) proof fn lemma_freed_remotely(&self, s0: Core, r: Set<usize>, held: Set<usize>)
        requires
            s0.full(),
            s0.quiet(),
            !s0.checked(),
            s0.freed_set(r, held),
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms == s0.perms,
            self.model@ == s0.freed_model(r),
        ensures
            self.full(),
            self.quiet(),
            self.dom() == s0.dom(),
            forall|b: usize|
                s0.dom().contains(b) && !r.contains(b) ==> #[trigger] self.state(b) == s0.state(b),
            forall|b: usize| r.contains(b) ==> #[trigger] self.state(b) == BlockState::Freed,
            forall|b: usize| #[trigger] self.indeg(b) <= s0.indeg(b),
            forall|b: usize| r.contains(b) ==> #[trigger] self.indeg(b) == 0,
            forall|b: usize, j: int|
                r.contains(b) ==> !(#[trigger] self.model@.edges.dom().contains((b, j))),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        let m0 = s0.model@;
        let m1 = self.model@;
        assert(m1.state.dom() =~= s0.dom());
        assert forall|b: usize| #[trigger] self.indeg(b) <= s0.indeg(b) by {
            s0.lemma_freed_indeg(r, b);
        }
        assert forall|b: usize| r.contains(b) implies #[trigger] self.indeg(b) == 0 by {
            if self.indeg(b) > 0 {
                let e = super::model::lemma_indeg_witness(m1.edges, b);
                assert(m0.edges.dom().contains(e) && m0.edges[e] == b);
                assert(r.contains(e.0));
            }
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            let p = self.perm(b);
            if !r.contains(b) {
                if s0.state(b) == BlockState::Live {
                    assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                        p,
                        b,
                        i,
                    ) by {
                        assert(s0.slot_word(p, b, i));
                    }
                }
                if s0.state(b) == BlockState::Building {
                    assert forall|i: int| #[trigger]
                        self.model@.written[b].contains(i) implies p_scanned(p, i)
                        && self.slot_word(p, b, i) by {
                        assert(s0.slot_word(p, b, i));
                    }
                }
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(e) by {
            assert(s0.kept_wf(e));
        }
        assert forall|e: Pos| #[trigger] self.model@.edges.dom().contains(e) implies self.edge_wf(
            e,
        ) by {
            assert(s0.edge_wf(e));
            let t = m0.edges[e];
            if r.contains(t) {
                assert(r.contains(e.0));
            }
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
        assert forall|b: usize, j: int| r.contains(b) implies !(
        #[trigger] self.model@.edges.dom().contains((b, j))) by {}
    }
}

impl<M: Mode> Heap<M> {
    /// ほかのスレッドから受けた解放を、すべて取り込む。持ち主のスレッドならいつ呼んでもよい
    /// (docs/spec/object-model.md の「別のスレッドからの解放」)。
    pub fn reclaim(&mut self)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).reclaimed(*old(self), Set::empty()),
    {
        self.reclaim_held(Ghost(Set::empty()));
    }

    /// 受けた解放が `REMOTE_RECLAIM` 個に届いたか、大きい物体のページが返ってきていれば取り込み、真を返す。そうで
    /// なければ何もせずに偽を返す。
    pub fn reclaim_if_due(&mut self) -> (r: bool)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).reclaimed(*old(self), Set::empty()),
    {
        self.reclaim_if_due_held(Ghost(Set::empty()))
    }

    pub(crate) fn reclaim_if_due_held(&mut self, Ghost(held): Ghost<Set<usize>>) -> (r: bool)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).reclaimed(*old(self), held),
    {
        let sink = self.core.pages.sink();
        if sink_large_pending(sink) || sink_count(sink) >= REMOTE_RECLAIM {
            self.reclaim_held(Ghost(held));
            true
        } else {
            proof {
                self.lemma_reclaimed_refl(held);
            }
            false
        }
    }

    /// 取り込みの前後の関係。空けたことにした集合 `r` のほかのブロックは、同じ状態、同じ権限で残り、外の参照の数は
    /// 減らない。`r` は呼ぶ側の持つ物体 `held` と重ならない。
    pub closed spec fn reclaimed(&self, old: Heap<M>, held: Set<usize>) -> bool {
        &&& self.dummy == old.dummy
        &&& self.region == old.region
        &&& self.worker == old.worker
        &&& self.building == old.building
        &&& self.tokens == old.tokens
        &&& self.core.env == old.core.env
        &&& self.core.model@.building == old.core.model@.building
        &&& self.core.model@.tokens == old.core.model@.tokens
        &&& self.core.pages.prefixed() == old.core.pages.prefixed()
        &&& self.core.model@.kept == old.core.model@.kept
        &&& self.core.dom().subset_of(old.core.dom())
        &&& exists|r: Set<usize>|
            #![trigger old.core.freed_set(r, held)]
            {
                &&& old.core.freed_set(r, held)
                &&& forall|b: usize|
                    #![trigger old.core.dom().contains(b)]
                    old.core.dom().contains(b) && !r.contains(b) ==> {
                        &&& self.core.dom().contains(b)
                        &&& self.core.state(b) == old.core.state(b)
                        &&& self.core.perm(b) == old.core.perm(b)
                        &&& self.core.block(b) == old.core.block(b)
                        &&& self.core.cap(b) == old.core.cap(b)
                        &&& self.core.model@.refs[b] == old.core.model@.refs[b]
                        &&& self.core.model@.owner[b] == old.core.model@.owner[b]
                        &&& self.core.indeg(b) <= old.core.indeg(b)
                    }
                &&& forall|w: Word|
                    #![trigger self.core.word_ok(w)]
                    old.core.word_ok(w) && (w.is_ptr_spec() ==> !r.contains(w.bits_spec()))
                        ==> self.core.word_ok(w)
                &&& forall|b: usize|
                    #![trigger r.contains(b)]
                    r.contains(b) ==> !(self.core.has(b) && self.core.settled_state(b)
                        == BlockState::Live)
            }
    }

    /// `reclaimed` の中身を、ヒープの外の証明 (`Ctx`) に見せる。
    pub(super) proof fn lemma_reclaimed(&self, old: Heap<M>, held: Set<usize>) -> (r: Set<usize>)
        requires
            self.reclaimed(old, held),
        ensures
            self.dummy == old.dummy,
            self.region == old.region,
            self.worker == old.worker,
            self.building == old.building,
            self.tokens == old.tokens,
            self.core.env == old.core.env,
            self.core.model@.building == old.core.model@.building,
            self.core.model@.tokens == old.core.model@.tokens,
            self.core.model@.kept == old.core.model@.kept,
            self.core.dom().subset_of(old.core.dom()),
            old.core.freed_set(r, held),
            forall|b: usize|
                #![trigger old.core.dom().contains(b)]
                old.core.dom().contains(b) && !r.contains(b) ==> {
                    &&& self.core.dom().contains(b)
                    &&& self.core.state(b) == old.core.state(b)
                    &&& self.core.perm(b) == old.core.perm(b)
                    &&& self.core.block(b) == old.core.block(b)
                    &&& self.core.cap(b) == old.core.cap(b)
                    &&& self.core.model@.refs[b] == old.core.model@.refs[b]
                    &&& self.core.model@.owner[b] == old.core.model@.owner[b]
                    &&& self.core.indeg(b) <= old.core.indeg(b)
                },
            forall|w: Word|
                #![trigger self.core.word_ok(w)]
                old.core.word_ok(w) && (w.is_ptr_spec() ==> !r.contains(w.bits_spec()))
                    ==> self.core.word_ok(w),
            forall|b: usize|
                #![trigger r.contains(b)]
                r.contains(b) ==> !(self.core.has(b) && self.core.settled_state(b)
                    == BlockState::Live),
    {
        choose|r: Set<usize>|
            #![trigger old.core.freed_set(r, held)]
            {
                &&& old.core.freed_set(r, held)
                &&& forall|b: usize|
                    #![trigger old.core.dom().contains(b)]
                    old.core.dom().contains(b) && !r.contains(b) ==> {
                        &&& self.core.dom().contains(b)
                        &&& self.core.state(b) == old.core.state(b)
                        &&& self.core.perm(b) == old.core.perm(b)
                        &&& self.core.block(b) == old.core.block(b)
                        &&& self.core.cap(b) == old.core.cap(b)
                        &&& self.core.model@.refs[b] == old.core.model@.refs[b]
                        &&& self.core.model@.owner[b] == old.core.model@.owner[b]
                        &&& self.core.indeg(b) <= old.core.indeg(b)
                    }
                &&& forall|w: Word|
                    #![trigger self.core.word_ok(w)]
                    old.core.word_ok(w) && (w.is_ptr_spec() ==> !r.contains(w.bits_spec()))
                        ==> self.core.word_ok(w)
                &&& forall|b: usize|
                    #![trigger r.contains(b)]
                    r.contains(b) ==> !(self.core.has(b) && self.core.settled_state(b)
                        == BlockState::Live)
            }
    }

    proof fn lemma_reclaimed_refl(&self, held: Set<usize>)
        ensures
            self.reclaimed(*self, held),
    {
        let r = Set::<usize>::empty();
        assert(self.core.freed_set(r, held));
        assert forall|b: usize| #[trigger] r.contains(b) implies !(self.core.has(b)
            && self.core.settled_state(b) == BlockState::Live) by {}
    }

    /// `reclaim` の本体。`held` は呼ぶ側 (`Ctx`) の持つ物体で、取り込みはそれを空けたことにしない。先にすべての
    /// ページの遠隔の空きリストと大きい物体の列を取り出し、その後で1つずつ返す。取り出す間はモデルを変えないので、
    /// 約束の集合はどれも呼んだときのモデルについて言える。
    pub(crate) fn reclaim_held(&mut self, Ghost(held): Ghost<Set<usize>>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).reclaimed(*old(self), held),
    {
        let ghost s0 = *self;
        let ghost c0 = self.core;
        let sink = self.core.pages.sink();
        sink_count_take(sink);
        let mut out: Vec<*mut Header> = Vec::new();
        let ghost mut acc: Set<usize> = Set::empty();
        proof {
            assert(c0.freed_set(acc, held));
        }
        let n = self.core.pages.pages_len();
        let mut i: usize = 0;
        while i < n
            invariant
                *self == s0,
                n == self.core.pages.page_count(),
                i <= n,
                c0.freed_set(acc, held),
                Self::out_ok(out@, c0, acc),
            decreases n - i,
        {
            let page = self.core.pages.page_at(i);
            let ghost before = out@;
            remote_take(page, &mut out, Ghost(c0), Ghost(held));
            proof {
                let r = choose|r: Set<usize>|
                    #![trigger c0.freed_set(r, held)]
                    c0.freed_set(r, held) && forall|k: int|
                        before.len() <= k < out@.len() ==> r.contains(#[trigger] out@[k]@.addr);
                Self::lemma_freed_union(c0, acc, r, held);
                Self::lemma_out_grow(before, out@, c0, acc, r, held);
                acc = acc.union(r);
            }
            i += 1;
        }
        let small = out.len();
        {
            let ghost before = out@;
            remote_large_take(
                sink,
                self.core.pages.prefix_words(),
                &mut out,
                Ghost(c0),
                Ghost(held),
            );
            proof {
                let r = choose|r: Set<usize>|
                    #![trigger c0.freed_set(r, held)]
                    c0.freed_set(r, held) && forall|k: int|
                        before.len() <= k < out@.len() ==> r.contains(#[trigger] out@[k]@.addr);
                Self::lemma_freed_union(c0, acc, r, held);
                Self::lemma_out_grow(before, out@, c0, acc, r, held);
                acc = acc.union(r);
            }
        }
        proof {
            if !M::CHECKED {
                self.core.model = Ghost(c0.freed_model(acc));
                self.core.lemma_freed_remotely(c0, acc, held);
            }
        }
        let mut k: usize = 0;
        while k < out.len()
            invariant
                self.wf(),
                k <= out@.len(),
                small <= out@.len(),
                Self::out_ok(out@, c0, acc),
                c0.freed_set(acc, held),
                M::CHECKED == c0.checked(),
                self.dummy == s0.dummy,
                self.region == s0.region,
                self.worker == s0.worker,
                self.building == s0.building,
                self.tokens == s0.tokens,
                self.core.env == c0.env,
                self.core.model@.building == c0.model@.building,
                self.core.model@.tokens == c0.model@.tokens,
                self.core.model@.kept == c0.model@.kept,
                self.core.pages.prefixed() == c0.pages.prefixed(),
                self.core.dom().subset_of(c0.dom()),
                forall|j: int|
                    k <= j < out@.len() ==> {
                        let h = #[trigger] out@[j]@.addr;
                        &&& self.core.dom().contains(h)
                        &&& self.core.block(h) == c0.block(h)
                        &&& self.core.perm(h) == c0.perm(h)
                        &&& small <= j ==> self.core.pages.large(h)
                        &&& !M::CHECKED ==> self.core.state(h) == BlockState::Freed
                            && self.core.indeg(h) == 0 && forall|i: int|
                            !(#[trigger] self.core.model@.edges.dom().contains((h, i)))
                    },
                !M::CHECKED ==> c0.full(),
                !M::CHECKED ==> forall|b: usize|
                    #![trigger acc.contains(b)]
                    acc.contains(b) ==> !self.core.dom().contains(b) || self.core.state(b)
                        == BlockState::Freed,
                M::CHECKED ==> c0.safe_wf(),
                M::CHECKED ==> self.core.pages == c0.pages,
                M::CHECKED ==> self.core.dom() == c0.dom(),
                M::CHECKED ==> self.core.model@.edges == c0.model@.edges,
                M::CHECKED ==> self.core.model@.refs == c0.model@.refs,
                M::CHECKED ==> self.core.model@.owner == c0.model@.owner,
                M::CHECKED ==> forall|j: int|
                    0 <= j < k ==> self.core.state(#[trigger] out@[j]@.addr) == BlockState::Freed,
                M::CHECKED ==> forall|b: usize|
                    #![trigger c0.dom().contains(b)]
                    c0.dom().contains(b) && !Self::processed(out@, k as int, b) ==> {
                        &&& self.core.state(b) == c0.state(b)
                        &&& self.core.perm(b) == c0.perm(b)
                    },
                forall|b: usize|
                    #![trigger c0.dom().contains(b)]
                    c0.dom().contains(b) && !acc.contains(b) ==> {
                        &&& self.core.dom().contains(b)
                        &&& self.core.state(b) == c0.state(b)
                        &&& self.core.perm(b) == c0.perm(b)
                        &&& self.core.block(b) == c0.block(b)
                        &&& self.core.cap(b) == c0.cap(b)
                        &&& self.core.model@.refs[b] == c0.model@.refs[b]
                        &&& self.core.model@.owner[b] == c0.model@.owner[b]
                        &&& self.core.indeg(b) <= c0.indeg(b)
                    },
                forall|w: Word|
                    #![trigger self.core.word_ok(w)]
                    c0.word_ok(w) && (w.is_ptr_spec() ==> !acc.contains(w.bits_spec()))
                        ==> self.core.word_ok(w),
            decreases out@.len() - k,
        {
            let h = out[k];
            let ghost s1 = self.core;
            proof {
                s1.lemma_shape(h@.addr);
                assert(Self::out_ok(out@, c0, acc));
                assert(out@[k as int]@.addr == h@.addr);
                assert forall|j: int| k < j < out@.len() implies #[trigger] out@[j]@.addr
                    != h@.addr by {}
            }
            if M::CHECKED {
                let bytes = self.core.pages.block_bytes(h);
                let tracked mut perm = self.core.perms.borrow_mut().tracked_remove(h@.addr);
                let tracked mut pre = perm.prefix.tracked_unwrap();
                mark_freed(h, Tracked(&mut pre));
                proof {
                    perm.prefix = Some(pre);
                    self.core.perms.borrow_mut().tracked_insert(h@.addr, perm);
                    self.core.model = Ghost(
                        Model {
                            state: s1.model@.state.insert(h@.addr, BlockState::Freed),
                            ..s1.model@
                        },
                    );
                    assert(self.core.perms@.dom() =~= s1.perms@.dom());
                    self.core.lemma_safe_freed(s1, h@.addr);
                }
                self.quarantined = self.quarantined.wrapping_add(bytes);
                self.stats.live = self.stats.live.wrapping_sub(1);
                self.stats.reclaimed = self.stats.reclaimed.wrapping_add(1);
            } else {
                let tracked perm = self.core.perms.borrow_mut().tracked_remove(h@.addr);
                proof {
                    s1.lemma_at(h@.addr);
                    self.core.model = Ghost(
                        Model {
                            state: s1.model@.state.remove(h@.addr),
                            refs: s1.model@.refs.remove(h@.addr),
                            stuck: s1.model@.stuck.remove(h@.addr),
                            owner: s1.model@.owner.remove(h@.addr),
                            written: s1.model@.written.remove(h@.addr),
                            ..s1.model@
                        },
                    );
                }
                if k < small {
                    self.core.pages.free(h, Tracked(perm));
                } else {
                    self.core.pages.release_large(h, Tracked(perm), false);
                }
                proof {
                    self.core.lemma_block_released(s1, h@.addr);
                }
            }
            k += 1;
        }
        proof {
            // 検査モードは取り出したブロックだけを空けた状態にするので、その集合を証人にする
            let w = if M::CHECKED {
                acc.filter(|a: usize| Self::processed(out@, out@.len() as int, a))
            } else {
                acc
            };
            reveal(Core::building_wf);
            assert(w.subset_of(acc));
            assert(c0.freed_set(w, held));
            assert forall|b: usize| #![trigger w.contains(b)] w.contains(b) implies !(self.core.has(
                b,
            ) && self.core.settled_state(b) == BlockState::Live) by {
                assert(acc.contains(b));
                assert(c0.dom().contains(b));
                if c0.model@.building == Some(b) {
                    assert(c0.state(b) == BlockState::Building);
                }
                if M::CHECKED {
                    let j = choose|j: int| 0 <= j < out@.len() && #[trigger] out@[j]@.addr == b;
                    assert(self.core.state(out@[j]@.addr) == BlockState::Freed);
                } else if !self.core.dom().contains(b) {
                    assert(c0.pages.in_use().contains(b));
                    assert(!c0.region_has(b));
                }
            }
            assert forall|b: usize|
                #![trigger c0.dom().contains(b)]
                c0.dom().contains(b) && !w.contains(b) implies {
                &&& self.core.dom().contains(b)
                &&& self.core.state(b) == c0.state(b)
                &&& self.core.perm(b) == c0.perm(b)
                &&& self.core.block(b) == c0.block(b)
                &&& self.core.cap(b) == c0.cap(b)
                &&& self.core.model@.refs[b] == c0.model@.refs[b]
                &&& self.core.model@.owner[b] == c0.model@.owner[b]
                &&& self.core.indeg(b) <= c0.indeg(b)
            } by {
                if M::CHECKED {
                    if Self::processed(out@, out@.len() as int, b) {
                        let j = choose|j: int| 0 <= j < out@.len() && #[trigger] out@[j]@.addr == b;
                        assert(acc.contains(out@[j]@.addr));
                    }
                }
            }
            assert forall|w2: Word|
                #![trigger self.core.word_ok(w2)]
                c0.word_ok(w2) && (w2.is_ptr_spec() ==> !w.contains(
                    w2.bits_spec(),
                )) implies self.core.word_ok(w2) by {
                if !M::CHECKED {
                    assert(w2.is_ptr_spec() ==> !acc.contains(w2.bits_spec()));
                }
            }
        }
    }

    /// `b` は `out` の k 番より前にある。
    spec fn processed(out: Seq<*mut Header>, k: int, b: usize) -> bool {
        exists|j: int| 0 <= j < k && #[trigger] out[j]@.addr == b
    }

    /// `out` のブロックは互いに違い、`c0` のこのヒープのブロックで、`acc` にある。
    spec fn out_ok(out: Seq<*mut Header>, c0: Core, acc: Set<usize>) -> bool {
        &&& forall|i: int, j: int|
            #![trigger out[i], out[j]]
            0 <= i < j < out.len() ==> out[i]@.addr != out[j]@.addr
        &&& forall|i: int|
            0 <= i < out.len() ==> c0.dom().contains(#[trigger] out[i]@.addr) && out[i] == c0.block(
                out[i]@.addr,
            ) && acc.contains(out[i]@.addr)
    }

    proof fn lemma_freed_union(c0: Core, a: Set<usize>, b: Set<usize>, held: Set<usize>)
        requires
            c0.freed_set(a, held),
            c0.freed_set(b, held),
        ensures
            c0.freed_set(a.union(b), held),
    {
    }

    proof fn lemma_out_grow(
        before: Seq<*mut Header>,
        out: Seq<*mut Header>,
        c0: Core,
        acc: Set<usize>,
        r: Set<usize>,
        held: Set<usize>,
    )
        requires
            Self::out_ok(before, c0, acc),
            taken(before, out, c0, held),
            forall|k: int| before.len() <= k < out.len() ==> r.contains(#[trigger] out[k]@.addr),
        ensures
            Self::out_ok(out, c0, acc.union(r)),
    {
        assert forall|i: int| 0 <= i < out.len() implies c0.dom().contains(#[trigger] out[i]@.addr)
            && out[i] == c0.block(out[i]@.addr) && acc.union(r).contains(out[i]@.addr) by {
            if i < before.len() {
                assert(out.subrange(0, before.len() as int)[i] == out[i]);
            }
        }
        assert forall|i: int, j: int|
            #![trigger out[i], out[j]]
            0 <= i < j < out.len() implies out[i]@.addr != out[j]@.addr by {
            if j < before.len() {
                assert(out.subrange(0, before.len() as int)[i] == out[i]);
                assert(out.subrange(0, before.len() as int)[j] == out[j]);
            }
        }
    }
}

} // verus!

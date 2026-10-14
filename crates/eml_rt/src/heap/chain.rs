//! 解放の連鎖の証明の部品。作業リストの述語、親の辺を1つずつ手放す手順の補題と、`evolved` をつなぐ補題を置く。

use vstd::prelude::*;

use super::Core;
#[cfg(verus_only)]
use super::inv::{p_count, p_first, p_word};
#[cfg(verus_only)]
use super::model::{BlockState, Pos};
#[cfg(verus_only)]
use crate::meta::Header;
#[cfg(verus_only)]
use crate::word::Word;

verus! {

/// 作業リストに `b` を指す語がある。
pub(crate) open spec fn listed(work: Seq<Word>, b: usize) -> bool {
    exists|k: int| #![trigger work[k]] 0 <= k < work.len() && work[k].bits_spec() == b
}

impl Core {
    /// `h` の走査する語のうち、ポインタの語はどれも辺である。
    pub(crate) open spec fn edges_complete(&self, h: usize) -> bool {
        forall|i: int|
            #![trigger self.scanned(h, i)]
            self.scanned(h, i) && p_word(self.perm(h), i).is_ptr_spec()
                ==> self.model@.edges.dom().contains((h, i))
    }

    /// 作業リストの語。空ける途中の物体を指し、その子の辺はまだ揃っている。
    pub(crate) open spec fn dying_word(&self, w: Word) -> bool {
        &&& w.is_ptr_spec()
        &&& self.word_ok(w)
        &&& self.state(w.bits_spec()) == BlockState::Dying
        &&& self.edges_complete(w.bits_spec())
    }

    /// 作業リストは、空ける途中の物体を1つずつ持つ。`except` は、取り出して子を手放している途中の物体である。
    pub(crate) open spec fn work_wf(&self, work: Seq<Word>, except: Option<usize>) -> bool {
        &&& forall|k: int|
            #![trigger work[k]]
            0 <= k < work.len() ==> self.dying_word(work[k]) && except != Some(work[k].bits_spec())
        &&& forall|j: int, k: int|
            #![trigger work[j], work[k]]
            0 <= j < k < work.len() ==> work[j].bits_spec() != work[k].bits_spec()
        &&& forall|b: usize|
            #![trigger self.state(b)]
            self.dom().contains(b) && self.state(b) == BlockState::Dying && except != Some(b)
                ==> listed(work, b)
    }

    pub(crate) open spec fn work_ok(&self, work: Seq<Word>) -> bool {
        self.work_wf(work, None)
    }

    /// 解放の連鎖が `h` の子を `first + i` 語目より下だけ手放し残している。`skip` の語は残すフィールドで、辺はもうない。
    pub(crate) open spec fn draining(
        &self,
        h: *mut Header,
        first: int,
        i: int,
        skip: Set<int>,
        work: Seq<Word>,
    ) -> bool {
        let a = h@.addr;
        &&& self.dom().contains(a)
        &&& h == self.block(a)
        &&& self.state(a) == BlockState::Dying
        &&& first == p_first(self.perm(a))
        &&& 0 <= i <= p_count(self.perm(a))
        &&& forall|j: int|
            #![trigger self.model@.edges.dom().contains((a, j))]
            self.model@.edges.dom().contains((a, j)) ==> first <= j < first + i && !skip.contains(j)
        &&& forall|j: int|
            #![trigger p_word(self.perm(a), j)]
            first <= j < first + i && !skip.contains(j) && p_word(self.perm(a), j).is_ptr_spec()
                ==> self.model@.edges.dom().contains((a, j))
        &&& self.work_wf(work, Some(a))
    }

    /// 参照を1つ手放した後の状態 (`Heap::release_ref`)。`last` なら、`h` は空ける途中になった。
    pub(crate) open spec fn released(
        &self,
        old: Core,
        h: usize,
        edge: Option<Pos>,
        last: bool,
    ) -> bool {
        &&& self.pages == old.pages
        &&& self.checked == old.checked
        &&& self.dom() == old.dom()
        &&& forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w)
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& self.perm(h).meta == old.perm(h).meta
        &&& self.perm(h).slots == old.perm(h).slots
        &&& self.perm(h).text == old.perm(h).text
        &&& self.state(h) == if last {
            BlockState::Dying
        } else {
            BlockState::Live
        }
        &&& forall|b: usize| b != h ==> #[trigger] self.state(b) == old.state(b)
        &&& last ==> self.edges_complete(h)
        &&& old.quiet() && !last ==> self.quiet()
        &&& self.model@.edges == match edge {
            Some(e) => old.model@.edges.remove(e),
            None => old.model@.edges,
        }
        &&& self.model@.building == old.model@.building
        &&& self.evolved(
            old,
            h,
            if edge is None {
                -1
            } else {
                0
            },
        )
    }

    /// 空ける途中の `h` の箱を空けた後の状態 (`Heap::free_box`)。
    pub(crate) open spec fn boxed_off(&self, old: Core, h: usize) -> bool {
        &&& self.checked == old.checked
        &&& self.model@.edges == old.model@.edges
        &&& self.model@.building == old.model@.building
        &&& self.active() == old.active().remove(h)
        &&& self.dom().subset_of(old.dom())
        &&& self.dom().contains(h) ==> self.state(h) == BlockState::Freed
        &&& forall|b: usize| #[trigger]
            old.dom().contains(b) && b != h ==> {
                &&& self.dom().contains(b)
                &&& self.perm(b) == old.perm(b)
                &&& self.state(b) == old.state(b)
                &&& self.block(b) == old.block(b)
                &&& self.cap(b) == old.cap(b)
                &&& self.model@.refs[b] == old.model@.refs[b]
                &&& self.model@.owner[b] == old.model@.owner[b]
            }
        &&& forall|w: Word|
            old.word_ok(w) && (w.is_ptr_spec() ==> w.bits_spec() != h) ==> #[trigger] self.word_ok(
                w,
            )
    }

    // `evolved` をつなぐ
    /// 構築中の物体がなければ、何も変えないことは `frame` である。
    pub(crate) proof fn lemma_frame_refl(&self)
        requires
            self.model@.building is None,
        ensures
            self.frame(*self),
    {
    }

    pub(crate) proof fn lemma_evolved_self(&self, w: usize)
        requires
            self.model@.building is None,
        ensures
            self.evolved(*self, w, 0),
    {
    }

    /// 構築中の物体を生きている物体にしただけの変化 (`frame`) の後に続けても、`evolved` は変わらない。
    pub(crate) proof fn lemma_frame_evolved(&self, mid: Core, s0: Core, w: usize, delta: int)
        requires
            self.evolved(mid, w, delta),
            mid.frame(s0),
        ensures
            self.evolved(s0.settled_view(), w, delta),
    {
        let v = s0.settled_view();
        assert forall|b: usize|
            #![trigger v.dom().contains(b)]
            v.dom().contains(b) && v.settled_state(b) == BlockState::Live && (v.rc_of(b) == 0
                || v.ext(b) + (if b == w {
                delta
            } else {
                0
            }) >= 1) implies {
            &&& self.dom().contains(b)
            &&& self.state(b) == BlockState::Live
            &&& self.perm(b).meta == v.perm(b).meta
            &&& self.perm(b).slots == v.perm(b).slots
            &&& self.perm(b).text == v.perm(b).text
            &&& self.block(b) == v.block(b)
            &&& self.cap(b) == v.cap(b)
            &&& (v.rc_of(b) == 0) == (self.rc_of(b) == 0)
            &&& v.rc_of(b) != 0 ==> self.ext(b) == v.ext(b) + if b == w {
                delta
            } else {
                0
            }
            &&& self.model@.owner[b] == v.model@.owner[b]
        } by {
            assert(s0.dom().contains(b));
            assert(mid.dom().contains(b));
            assert(mid.perm(b) == v.perm(b));
            assert(mid.rc_of(b) == v.rc_of(b));
            assert(mid.state(b) == BlockState::Live);
            assert(mid.settled_state(b) == BlockState::Live);
            assert(mid.ext(b) == v.ext(b));
        }
    }

    /// 外の参照の数を変えない変化を後に続けても、`evolved` は変わらない。
    pub(crate) proof fn lemma_evolved_trans(
        &self,
        mid: Core,
        s0: Core,
        w: usize,
        delta: int,
        x: usize,
    )
        requires
            mid.evolved(s0, w, delta),
            self.evolved(mid, x, 0),
        ensures
            self.evolved(s0, w, delta),
    {
        assert forall|b: usize|
            #![trigger s0.dom().contains(b)]
            s0.dom().contains(b) && s0.settled_state(b) == BlockState::Live && (s0.rc_of(b) == 0
                || s0.ext(b) + (if b == w {
                delta
            } else {
                0
            }) >= 1) implies {
            &&& self.dom().contains(b)
            &&& self.state(b) == BlockState::Live
            &&& self.perm(b).meta == s0.perm(b).meta
            &&& self.perm(b).slots == s0.perm(b).slots
            &&& self.perm(b).text == s0.perm(b).text
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
            &&& (s0.rc_of(b) == 0) == (self.rc_of(b) == 0)
            &&& s0.rc_of(b) != 0 ==> self.ext(b) == s0.ext(b) + if b == w {
                delta
            } else {
                0
            }
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

    // 作業リスト
    /// 作業リストの最後の語を取り出すと、その物体は作業リストの外の、子を手放している途中の物体になる。
    pub(crate) proof fn lemma_work_pop(&self, before: Seq<Word>, after: Seq<Word>, w: Word)
        requires
            self.work_ok(before),
            before.len() > 0,
            after == before.subrange(0, before.len() - 1),
            w == before[before.len() - 1],
        ensures
            self.dying_word(w),
            self.work_wf(after, Some(w.bits_spec())),
    {
        let n = before.len() - 1;
        assert(self.dying_word(before[n]));
        assert forall|k: int| #![trigger after[k]] 0 <= k < after.len() implies self.dying_word(
            after[k],
        ) && Some(w.bits_spec()) != Some(after[k].bits_spec()) by {
            assert(after[k] == before[k]);
            assert(before[k].bits_spec() != before[n].bits_spec());
        }
        assert forall|j: int, k: int|
            #![trigger after[j], after[k]]
            0 <= j < k < after.len() implies after[j].bits_spec() != after[k].bits_spec() by {
            assert(after[j] == before[j] && after[k] == before[k]);
            assert(before[j].bits_spec() != before[k].bits_spec());
        }
        assert forall|b: usize|
            #![trigger self.state(b)]
            self.dom().contains(b) && self.state(b) == BlockState::Dying && Some(w.bits_spec())
                != Some(b) implies listed(after, b) by {
            assert(listed(before, b));
            let k = choose|k: int|
                #![trigger before[k]]
                0 <= k < before.len() && before[k].bits_spec() == b;
            assert(k != n);
            assert(after[k] == before[k]);
        }
    }

    /// 作業リストの語はどれも、空ける途中の物体を指し、同じ物体を2回指さない。`except` は、取り出して子を手放している
    /// 途中の物体である。モデルが合わない検査モードでも成り立つ `work_wf` の弱い形で、前の連鎖が `Err` で止まって残した
    /// 空ける途中の物体は載らなくてよい。
    pub(crate) open spec fn safe_work(&self, work: Seq<Word>, except: Option<usize>) -> bool {
        &&& forall|k: int|
            #![trigger work[k]]
            0 <= k < work.len() ==> {
                &&& work[k].is_ptr_spec()
                &&& self.word_ok(work[k])
                &&& self.state(work[k].bits_spec()) == BlockState::Dying
                &&& except != Some(work[k].bits_spec())
            }
        &&& forall|j: int, k: int|
            #![trigger work[j], work[k]]
            0 <= j < k < work.len() ==> work[j].bits_spec() != work[k].bits_spec()
    }

    pub(crate) proof fn lemma_safe_work_pop(&self, before: Seq<Word>, after: Seq<Word>, w: Word)
        requires
            self.safe_work(before, None),
            before.len() > 0,
            after == before.subrange(0, before.len() - 1),
            w == before[before.len() - 1],
        ensures
            w.is_ptr_spec(),
            self.word_ok(w),
            self.state(w.bits_spec()) == BlockState::Dying,
            self.safe_work(after, Some(w.bits_spec())),
    {
        let n = before.len() - 1;
        assert(before[n] == w);
        assert forall|k: int| #![trigger after[k]] 0 <= k < after.len() implies {
            &&& after[k].is_ptr_spec()
            &&& self.word_ok(after[k])
            &&& self.state(after[k].bits_spec()) == BlockState::Dying
            &&& Some(w.bits_spec()) != Some(after[k].bits_spec())
        } by {
            assert(after[k] == before[k]);
            assert(before[k].bits_spec() != before[n].bits_spec());
        }
        assert forall|j: int, k: int|
            #![trigger after[j], after[k]]
            0 <= j < k < after.len() implies after[j].bits_spec() != after[k].bits_spec() by {
            assert(after[j] == before[j] && after[k] == before[k]);
        }
    }

    /// 生きている子 `c` の参照を手放した後。最後の参照なら `c` は空ける途中になり、作業リストに積む。
    pub(crate) proof fn lemma_safe_work_step(
        &self,
        old: Core,
        before: Seq<Word>,
        after: Seq<Word>,
        h: usize,
        child: Word,
        last: bool,
    )
        requires
            old.safe_work(before, Some(h)),
            old.state(h) == BlockState::Dying,
            child.is_ptr_spec(),
            old.word_ok(child),
            old.state(child.bits_spec()) == BlockState::Live,
            forall|b: usize| b != child.bits_spec() ==> #[trigger] self.state(b) == old.state(b),
            self.state(child.bits_spec()) == if last {
                BlockState::Dying
            } else {
                BlockState::Live
            },
            forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w),
            after == if last {
                before.push(child)
            } else {
                before
            },
        ensures
            self.safe_work(after, Some(h)),
    {
        let c = child.bits_spec();
        assert forall|k: int| #![trigger after[k]] 0 <= k < after.len() implies {
            &&& after[k].is_ptr_spec()
            &&& self.word_ok(after[k])
            &&& self.state(after[k].bits_spec()) == BlockState::Dying
            &&& Some(h) != Some(after[k].bits_spec())
        } by {
            if k < before.len() {
                assert(after[k] == before[k]);
                assert(before[k].bits_spec() != c);
            }
        }
        assert forall|j: int, k: int|
            #![trigger after[j], after[k]]
            0 <= j < k < after.len() implies after[j].bits_spec() != after[k].bits_spec() by {
            assert(after[j] == before[j]);
            if k < before.len() {
                assert(after[k] == before[k]);
            } else {
                assert(before[j].bits_spec() != c);
            }
        }
    }

    /// 取り出した物体 `h` を空けた後。作業リストのほかの物体は、空ける途中のままである。
    pub(crate) proof fn lemma_safe_work_freed(&self, old: Core, work: Seq<Word>, h: usize)
        requires
            old.safe_work(work, Some(h)),
            forall|b: usize| b != h ==> #[trigger] self.state(b) == old.state(b),
            forall|w: Word|
                old.word_ok(w) && (w.is_ptr_spec() ==> w.bits_spec() != h)
                    ==> #[trigger] self.word_ok(w),
        ensures
            self.safe_work(work, None),
    {
        assert forall|k: int| #![trigger work[k]] 0 <= k < work.len() implies {
            &&& work[k].is_ptr_spec()
            &&& self.word_ok(work[k])
            &&& self.state(work[k].bits_spec()) == BlockState::Dying
            &&& None::<usize> != Some(work[k].bits_spec())
        } by {
            assert(work[k].bits_spec() != h);
        }
    }

    /// 作業リストが空なら、空ける途中の物体はない。
    pub(crate) proof fn lemma_work_empty(&self, work: Seq<Word>)
        requires
            self.work_ok(work),
            work.len() == 0,
        ensures
            self.quiet(),
    {
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.state(b)
            != BlockState::Dying by {
            if self.state(b) == BlockState::Dying {
                assert(listed(work, b));
            }
        }
    }

    /// 取り出した物体の子を、走査する語の終わりから手放し始める。
    pub(crate) proof fn lemma_draining_start(
        &self,
        h: *mut Header,
        w: Word,
        first: int,
        n: int,
        work: Seq<Word>,
    )
        requires
            self.full(),
            self.dying_word(w),
            w.bits_spec() == h@.addr,
            h == self.block(h@.addr),
            self.work_wf(work, Some(h@.addr)),
            first == p_first(self.perm(h@.addr)),
            n == p_count(self.perm(h@.addr)),
        ensures
            self.draining(h, first, n, Set::empty(), work),
    {
        reveal(Core::edges_wf);
        let a = h@.addr;
        assert forall|j: int|
            #![trigger self.model@.edges.dom().contains((a, j))]
            self.model@.edges.dom().contains((a, j)) implies first <= j < first + n && !Set::<
            int,
        >::empty().contains(j) by {
            assert(self.edge_wf((a, j)));
        }
        assert forall|j: int|
            #![trigger p_word(self.perm(a), j)]
            first <= j < first + n && !Set::<int>::empty().contains(j) && p_word(
                self.perm(a),
                j,
            ).is_ptr_spec() implies self.model@.edges.dom().contains((a, j)) by {
            assert(self.scanned(a, j));
        }
    }

    /// 手放す次の語は書いてあり、ポインタなら、生きている子を指す辺である。
    pub(crate) proof fn lemma_draining_slot(
        &self,
        h: *mut Header,
        first: int,
        i: int,
        skip: Set<int>,
        work: Seq<Word>,
    )
        requires
            self.full(),
            self.draining(h, first, i + 1, skip, work),
            0 <= i,
            !skip.contains(first + i),
            self.model@.building is None,
        ensures
            super::inv::p_init(self.perm(h@.addr), first + i),
            self.word_ok(p_word(self.perm(h@.addr), first + i)),
            p_word(self.perm(h@.addr), first + i).is_ptr_spec() ==> {
                let c = p_word(self.perm(h@.addr), first + i).bits_spec();
                &&& self.model@.edges.dom().contains((h@.addr, first + i))
                &&& self.model@.edges[(h@.addr, first + i)] == c
                &&& self.dom().contains(c)
                &&& self.state(c) == BlockState::Live
                &&& c != h@.addr
            },
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::building_wf);
        let a = h@.addr;
        assert(self.block_wf(a));
        assert(self.scanned(a, first + i));
        if p_word(self.perm(a), first + i).is_ptr_spec() {
            assert(self.edge_wf((a, first + i)));
        }
    }

    /// 手放す次の語が即値なら、辺はない。
    pub(crate) proof fn lemma_draining_skip(
        &self,
        h: *mut Header,
        first: int,
        i: int,
        skip: Set<int>,
        work: Seq<Word>,
    )
        requires
            self.full(),
            self.draining(h, first, i + 1, skip, work),
            0 <= i,
            !p_word(self.perm(h@.addr), first + i).is_ptr_spec() || skip.contains(first + i),
        ensures
            self.draining(h, first, i, skip, work),
    {
        reveal(Core::edges_wf);
        let a = h@.addr;
        assert forall|j: int|
            #![trigger self.model@.edges.dom().contains((a, j))]
            self.model@.edges.dom().contains((a, j)) implies first <= j < first + i
            && !skip.contains(j) by {
            assert(self.edge_wf((a, j)));
        }
    }

    /// 子 `c` の辺を手放した後。最後の参照なら、子を作業リストに積んである。
    pub(crate) proof fn lemma_draining_step(
        &self,
        s3: Core,
        s0: Core,
        h: *mut Header,
        first: int,
        i: int,
        skip: Set<int>,
        child: Word,
        last: bool,
        before: Seq<Word>,
        work: Seq<Word>,
    )
        requires
            s3.draining(h, first, i + 1, skip, before),
            0 <= i,
            !skip.contains(first + i),
            s3.model@.edges.dom().contains((h@.addr, first + i)),
            s3.model@.edges[(h@.addr, first + i)] == child.bits_spec(),
            s3.state(child.bits_spec()) == BlockState::Live,
            child.bits_spec() != h@.addr,
            child.is_ptr_spec(),
            s3.word_ok(child),
            self.released(s3, child.bits_spec(), Some((h@.addr, first + i)), last),
            work == if last {
                before.push(child)
            } else {
                before
            },
            s3.evolved(s0, 0, 0),
        ensures
            self.draining(h, first, i, skip, work),
            self.active() == s3.active(),
            self.evolved(s0, 0, 0),
    {
        let a = h@.addr;
        let c = child.bits_spec();
        let e = (a, first + i);
        assert(self.perms@[a] == s3.perms@[a]);
        assert forall|j: int|
            #![trigger self.model@.edges.dom().contains((a, j))]
            self.model@.edges.dom().contains((a, j)) implies first <= j < first + i
            && !skip.contains(j) by {
            assert(s3.model@.edges.dom().contains((a, j)));
        }
        assert forall|j: int|
            #![trigger p_word(self.perm(a), j)]
            first <= j < first + i && !skip.contains(j) && p_word(
                self.perm(a),
                j,
            ).is_ptr_spec() implies self.model@.edges.dom().contains((a, j)) by {
            assert(p_word(s3.perm(a), j) == p_word(self.perm(a), j));
            assert(s3.model@.edges.dom().contains((a, j)));
        }
        // 作業リストのほかの物体は空ける途中なので、生きている子 c とも、子を手放している a とも違う
        assert forall|k: int| #![trigger before[k]] 0 <= k < before.len() implies self.dying_word(
            before[k],
        ) && Some(a) != Some(before[k].bits_spec()) by {
            let b = before[k].bits_spec();
            assert(s3.dying_word(before[k]));
            assert(b != c);
            assert(self.perms@[b] == s3.perms@[b]);
            assert forall|j: int|
                #![trigger self.scanned(b, j)]
                self.scanned(b, j) && p_word(
                    self.perm(b),
                    j,
                ).is_ptr_spec() implies self.model@.edges.dom().contains((b, j)) by {
                assert(s3.scanned(b, j));
            }
        }
        assert forall|k: int| #![trigger work[k]] 0 <= k < work.len() implies self.dying_word(
            work[k],
        ) && Some(a) != Some(work[k].bits_spec()) by {
            if k < before.len() {
                assert(work[k] == before[k]);
            }
        }
        assert forall|j: int, k: int|
            #![trigger work[j], work[k]]
            0 <= j < k < work.len() implies work[j].bits_spec() != work[k].bits_spec() by {
            if k < before.len() {
                assert(work[j] == before[j] && work[k] == before[k]);
            } else {
                assert(work[j] == before[j]);
                assert(s3.dying_word(before[j]));
            }
        }
        assert forall|b: usize|
            #![trigger self.state(b)]
            self.dom().contains(b) && self.state(b) == BlockState::Dying && Some(a) != Some(
                b,
            ) implies listed(work, b) by {
            if b == c {
                assert(work[before.len() as int] == child);
            } else {
                assert(s3.state(b) == BlockState::Dying);
                assert(listed(before, b));
                let k = choose|k: int|
                    #![trigger before[k]]
                    0 <= k < before.len() && before[k].bits_spec() == b;
                assert(work[k] == before[k]);
            }
        }
        assert(self.active() =~= s3.active());
        self.lemma_evolved_trans(s3, s0, 0, 0, c);
    }

    /// 子を手放し終えた物体の箱を空けた。作業リストはまた揃い、空ける途中でないブロックは1つ減る。
    pub(crate) proof fn lemma_drained_one(
        &self,
        pre: Core,
        s0: Core,
        h: *mut Header,
        first: int,
        skip: Set<int>,
        work: Seq<Word>,
    )
        requires
            pre.draining(h, first, 0, skip, work),
            pre.model@.building is None,
            self.boxed_off(pre, h@.addr),
            pre.evolved(s0, 0, 0),
        ensures
            self.work_ok(work),
            self.active().len() < pre.active().len(),
            self.evolved(s0, 0, 0),
    {
        let a = h@.addr;
        assert forall|k: int| #![trigger work[k]] 0 <= k < work.len() implies self.dying_word(
            work[k],
        ) && None::<usize> != Some(work[k].bits_spec()) by {
            let b = work[k].bits_spec();
            assert(pre.dying_word(work[k]));
            assert(pre.dom().contains(b));
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
            assert(pre.dom().contains(b));
            assert(b != a);
            assert(pre.state(b) == BlockState::Dying);
        }
        assert(pre.active().contains(a));
        assert(self.active().len() == pre.active().len() - 1);
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
        assert(self.evolved(pre, 0, 0));
        self.lemma_evolved_trans(pre, s0, 0, 0, 0);
    }
}

} // verus!

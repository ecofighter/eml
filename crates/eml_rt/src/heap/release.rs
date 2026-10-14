//! LIR の `release` (docs/spec/object-model.md の「核の操作」)。判定は `is_last` で、
//! rc が -1 の物体も最後の参照として、残すフィールドを `dup` せずに受け取る。共有の親から `lin` の子が届く形
//! (docs/spec/object-model.md の「`share`」) で、正しいプログラムの `File` を `dup` しないためである。

use std::mem;
use vstd::prelude::*;

#[cfg(verus_only)]
use super::Core;
use super::Heap;
use super::check::layout_check;
#[cfg(verus_only)]
use super::handover::{handover, kept_set, lemma_kept_set};
#[cfg(verus_only)]
use super::inv::{p_count, p_first};
#[cfg(verus_only)]
use super::model::{BlockState, Pos};
#[cfg(verus_only)]
use super::states::{lemma_pos_set, pos_set};
use crate::error::HeapError;
use crate::meta::Header;
#[cfg(verus_only)]
use crate::meta::{Kind, bits_tag};
use crate::mode::Mode;
use crate::word::Word;

verus! {

impl<M: Mode> Heap<M> {
    /// `keep` は走査する語ごとに、その子の参照を呼ぶ側に残すかを持つ。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側がその参照を1つ持っている。物体は種類 `Data` で、tag が `tag`、
    /// 走査する語の数が `keep.len()` である。共有の物体は `lin` ビットを持たない。rc が正なら、持ち主は今の worker である。
    /// `is_last` が偽のときの残す子は `dup` の安全の条件を満たす。
    #[inline]
    pub unsafe fn release(&mut self, w: Word, tag: u16, keep: &[bool]) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).settleable(),
            w.is_ptr_spec(),
            old(self).holds(w),
            !M::CHECKED ==> old(self).layout_of(w, tag, keep@.len() as int),
            !M::CHECKED ==> (old(self).rc_value(w) < 0 ==> !old(self).lin_of(w)),
            !M::CHECKED ==> old(self).owned(w),
            !M::CHECKED ==> forall|i: int|
                #![trigger keep@[i]]
                0 <= i < keep@.len() && keep@[i] ==> {
                    let c = old(self).scanned_word(w, i);
                    &&& c.is_ptr_spec() ==> !old(self).lin_of(c)
                    &&& old(self).owned(c)
                },
        ensures
            !M::CHECKED ==> r is Ok,
            r is Ok ==> final(self).wf() && final(self).handed_over(*old(self), w, keep@),
    {
        self.settle();
        let h = self.header_of(w);
        self.release_check(h, tag, keep.len())?;
        if self.core.rc_last(h) {
            proof {
                self.core.lemma_last_ref(h@.addr);
            }
            self.release_children(h, keep, false)?;
            if M::CHECKED {
                self.stats.rc_decrements = self.stats.rc_decrements.wrapping_add(1);
            }
            Ok(())
        } else {
            self.release_shared(h, w, keep)
        }
    }

    /// `release` の検査モードの検査。速いモードでは何も確かめない。
    #[inline]
    pub(super) fn release_check(&self, h: *mut Header, tag: u16, n: usize) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            self.core.full(),
            self.core.checked@ == M::CHECKED,
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
            self.core.state(h@.addr) == BlockState::Live,
        ensures
            !M::CHECKED ==> r is Ok,
            r is Ok && M::CHECKED ==> {
                let a = h@.addr;
                &&& self.core.kind(a) == Kind::Data
                &&& bits_tag(self.core.meta_bits(a)) == tag
                &&& self.core.count(a) == n
                &&& self.core.rc_of(a) < 0 ==> !self.core.lin(a)
            },
    {
        if !M::CHECKED {
            return Ok(());
        }
        self.checked_live(h)?;
        let m = self.core.meta_get(h);
        let (_, count) = self.core.scan_get(h);
        layout_check(m, count, tag, n)?;
        let rc = self.core.rc_get(h);
        if rc < 0 && m.lin() {
            return Err(HeapError::LinShared);
        }
        self.checked_owner(h, rc)
    }

    /// 最後の参照でないときの `release`。残す子を `dup` してから、物体の参照を手放す。
    #[inline]
    fn release_shared(&mut self, h: *mut Header, w: Word, keep: &[bool]) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            w.is_ptr_spec(),
            w.bits_spec() == h@.addr,
            old(self).core.word_ok(w),
            h == old(self).core.block(h@.addr),
            old(self).core.dom().contains(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Live,
            old(self).core.rc_of(h@.addr) != 0 ==> old(self).core.ext(h@.addr) >= 1,
            old(self).core.count(h@.addr) == keep@.len(),
            !M::CHECKED ==> (old(self).rc_value(w) < 0 ==> !old(self).lin_of(w)),
            !M::CHECKED ==> old(self).owned(w),
            !M::CHECKED ==> forall|i: int|
                #![trigger keep@[i]]
                0 <= i < keep@.len() && keep@[i] ==> {
                    let c = old(self).scanned_word(w, i);
                    &&& c.is_ptr_spec() ==> !old(self).lin_of(c)
                    &&& old(self).owned(c)
                },
        ensures
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            !M::CHECKED ==> r is Ok,
            r is Ok ==> final(self).wf() && final(self).core.evolved_by(
                old(self).core,
                handover(old(self).core.kept_out(h@.addr, keep@), h@.addr),
            ) && final(self).dummy == old(self).dummy && final(self).worker == old(self).worker,
    {
        let ghost s0 = self.core;
        let ghost a = h@.addr;
        let (first, n) = self.core.scan_get(h);
        proof {
            s0.lemma_dupped_start();
            lemma_kept_set(first as int, keep@, 0);
            lemma_pos_set(a, kept_set(first as int, keep@, 0));
            assert(s0.model@.edges.restrict(pos_set(a, kept_set(first as int, keep@, 0))) =~= Map::<
                Pos,
                usize,
            >::empty());
        }
        let mut i: usize = 0;
        while i < n
            invariant
                self.tokens == old(self).tokens,
                self.core.model@.tokens == old(self).core.model@.tokens,
                self.wf(),
                self.core.model@.building is None,
                self.core.dupped(
                    s0,
                    s0.model@.edges.restrict(pos_set(a, kept_set(first as int, keep@, i as int))),
                ),
                s0.full(),
                s0 == old(self).core,
                a == h@.addr,
                w.is_ptr_spec(),
                w.bits_spec() == a,
                s0.word_ok(w),
                old(self).core.rc_of(a) != 0 ==> old(self).core.ext(a) >= 1,
                s0.model@.building is None,
                s0.dom().contains(a),
                s0.state(a) == BlockState::Live,
                h == s0.block(a),
                first == p_first(s0.perm(a)),
                n == p_count(s0.perm(a)),
                n == keep@.len(),
                first + n <= s0.cap(a),
                i <= n,
                self.dummy == old(self).dummy,
                self.worker == old(self).worker,
                !M::CHECKED ==> forall|j: int|
                    #![trigger keep@[j]]
                    0 <= j < keep@.len() && keep@[j] ==> {
                        let c = old(self).scanned_word(w, j);
                        &&& c.is_ptr_spec() ==> !old(self).lin_of(c)
                        &&& old(self).owned(c)
                    },
            decreases n - i,
        {
            let ghost s1 = self.core;
            let ghost k0 = s0.model@.edges.restrict(
                pos_set(a, kept_set(first as int, keep@, i as int)),
            );
            assert(s1.dom() == s0.dom());
            assert(s1.dom().contains(a));
            assert(s1.state(a) == BlockState::Live);
            assert(s1.perm(a).slots == s0.perm(a).slots);
            assert(s1.perm(a).meta == s0.perm(a).meta);
            proof {
                s0.lemma_kept_out_step(a, first as int, keep@, i as int);
            }
            if keep[i] {
                proof {
                    s1.lemma_live_slot(a, (first + i) as int);
                    s0.lemma_live_slot(a, (first + i) as int);
                    reveal(Core::blocks_wf);
                    assert(s0.block_wf(a));
                    assert(s0.scanned(a, (first + i) as int));
                    assert(s0.slot_word(s0.perm(a), a, (first + i) as int));
                    reveal(Core::edges_wf);
                }
                let child = self.core.slot_get(h, first + i);
                let ghost mid = *self;
                let r = unsafe { self.dup(child) };
                r?;
                proof {
                    if child.is_ptr_spec() {
                        assert(s0.edge_wf((a, (first + i) as int)));
                        self.core.lemma_dupped_step(
                            s0,
                            s1,
                            k0,
                            (a, (first + i) as int),
                            child.bits_spec(),
                        );
                    } else {
                        if s0.dom().contains(child.bits_spec()) {
                            s0.lemma_at(child.bits_spec());
                            crate::word::lemma_aligned_is_ptr(child.bits_spec());
                        }
                        self.core.lemma_dupped_skip(s0, s1, k0, child.bits_spec());
                        assert(!s0.model@.edges.dom().contains((a, (first + i) as int))) by {
                            if s0.model@.edges.dom().contains((a, (first + i) as int)) {
                                assert(s0.edge_wf((a, (first + i) as int)));
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        let ghost s2 = self.core;
        proof {
            assert(s0.kept_out(a, keep@) == s0.model@.edges.restrict(
                pos_set(a, kept_set(first as int, keep@, n as int)),
            ));
        }
        let r = unsafe { self.decref(w) };
        proof {
            if r is Ok {
                self.core.lemma_dupped_end(s0, s2, s0.kept_out(a, keep@), a);
            }
        }
        r
    }

    /// 最後の参照の `release`。物体を空ける途中にして残すフィールドの辺を呼ぶ側に渡し、残さない子を手放す。`to_token`
    /// なら箱を token にし、そうでなければ空ける。
    pub(super) fn release_children(&mut self, h: *mut Header, keep: &[bool], to_token: bool) -> (r:
        Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Live,
            old(self).core.indeg(h@.addr) == 0,
            old(self).core.model@.refs[h@.addr] == 1,
            old(self).core.rc_of(h@.addr) != 0,
            old(self).core.count(h@.addr) == keep@.len(),
            to_token ==> old(self).core.kind(h@.addr) == Kind::Data && old(self).core.rc_of(h@.addr)
                == 1,
        ensures
            M::CHECKED ==> final(self).core.pages == old(self).core.pages,
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            !M::CHECKED ==> r is Ok,
            r is Ok ==> {
                &&& final(self).wf()
                &&& final(self).core.evolved_by(
                    old(self).core,
                    handover(old(self).core.kept_out(h@.addr, keep@), h@.addr),
                )
                &&& final(self).dummy == old(self).dummy
                &&& final(self).worker == old(self).worker
                &&& to_token ==> final(self).core.dom().contains(h@.addr) && final(self).core.state(
                    h@.addr,
                ) == BlockState::Token
                &&& to_token && M::CHECKED ==> !final(self).core.registered(h@.addr)
            },
    {
        let ghost s0 = self.core;
        let ghost a = h@.addr;
        let (first, _) = self.core.scan_get(h);
        let ghost skip = kept_set(first as int, keep@, keep@.len() as int);
        self.core.begin_free(h, Ghost(pos_set(a, skip)), M::CHECKED);
        let ghost s1 = self.core;
        proof {
            lemma_pos_set(a, skip);
            self.core.lemma_begin_evolved(s0, a, pos_set(a, skip));
            self.core.lemma_release_start(s0, h, skip);
            assert(s0.kept_out(a, keep@) == s0.model@.edges.restrict(pos_set(a, skip)));
        }
        let mut work = Vec::new();
        mem::swap(&mut work, &mut self.work);
        work.clear();
        let r = self.release_unkept(h, keep, first, to_token, &mut work, Ghost(skip));
        work.clear();
        mem::swap(&mut work, &mut self.work);
        proof {
            if r is Ok {
                self.core.lemma_by_trans(s1, s0, handover(s0.kept_out(a, keep@), a), 0);
                self.lemma_dummy_by(*old(self), handover(s0.kept_out(a, keep@), a));
            }
        }
        r
    }

    /// 残さない子を、走査の逆の順に手放す (`drain` と同じ順である)。手放し終えたら、箱を token にするか空け、最後の
    /// 参照を失った子を続けて空ける。
    fn release_unkept(
        &mut self,
        h: *mut Header,
        keep: &[bool],
        first: usize,
        to_token: bool,
        work: &mut Vec<Word>,
        Ghost(skip): Ghost<Set<int>>,
    ) -> (r: Result<(), HeapError>)
        requires
            old(self).core.full(),
            M::CHECKED ==> old(self).core.safe_wf(),
            old(self).core.checked@ == M::CHECKED,
            old(self).core.model@.building is None,
            old(self).core.draining(h, first as int, keep@.len() as int, skip, old(work)@),
            skip == kept_set(first as int, keep@, keep@.len() as int),
            keep@.len() == p_count(old(self).core.perm(h@.addr)),
            first + keep@.len() <= old(self).core.cap(h@.addr),
            to_token ==> old(self).core.kind(h@.addr) == Kind::Data && old(self).core.rc_of(h@.addr)
                == 1,
        ensures
            M::CHECKED ==> final(self).core.pages == old(self).core.pages,
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            !M::CHECKED ==> r is Ok,
            final(self).core.full(),
            M::CHECKED ==> final(self).core.safe_wf(),
            final(self).core.checked@ == M::CHECKED,
            r is Ok ==> {
                &&& final(self).core.quiet()
                &&& final(self).core.evolved(old(self).core, 0, 0)
                &&& to_token ==> final(self).core.dom().contains(h@.addr) && final(self).core.state(
                    h@.addr,
                ) == BlockState::Token
                &&& to_token && M::CHECKED ==> !final(self).core.registered(h@.addr)
            },
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
    {
        let ghost s1 = self.core;
        let n = keep.len();
        proof {
            s1.lemma_evolved_self(0);
            lemma_kept_set(first as int, keep@, n as int);
        }
        let mut i = n;
        while i > 0
            invariant
                M::CHECKED ==> self.core.pages == old(self).core.pages,
                self.tokens == old(self).tokens,
                self.core.model@.tokens == old(self).core.model@.tokens,
                self.core.full(),
                M::CHECKED ==> self.core.safe_wf(),
                self.core.checked@ == M::CHECKED,
                self.core.draining(h, first as int, i as int, skip, work@),
                skip == kept_set(first as int, keep@, n as int),
                forall|j: int| #[trigger]
                    skip.contains(j) <==> first <= j < first + n && keep@[j - first],
                n == keep@.len(),
                n == p_count(self.core.perm(h@.addr)),
                first + n <= self.core.cap(h@.addr),
                i <= n,
                self.core.evolved(s1, 0, 0),
                self.core.model@.building is None,
                self.dummy == old(self).dummy,
                self.worker == old(self).worker,
                to_token ==> self.core.kind(h@.addr) == Kind::Data && self.core.rc_of(h@.addr) == 1,
                s1.full(),
            decreases i,
        {
            i -= 1;
            if !keep[i] {
                proof {
                    self.core.lemma_draining_slot(h, first as int, i as int, skip, work@);
                }
                let child = self.core.slot_get(h, first + i);
                if child.is_ptr() {
                    let c = self.header_of(child);
                    let ghost s3 = self.core;
                    let ghost wb = work@;
                    let last = self.release_ref(c, Ghost(Some((h@.addr, (first + i) as int))))?;
                    if last {
                        work.push(child);
                    }
                    proof {
                        self.core.lemma_draining_step(
                            s3,
                            s1,
                            h,
                            first as int,
                            i as int,
                            skip,
                            child,
                            last,
                            wb,
                            work@,
                        );
                    }
                } else {
                    proof {
                        self.core.lemma_draining_skip(h, first as int, i as int, skip, work@);
                    }
                }
            } else {
                proof {
                    self.core.lemma_draining_skip(h, first as int, i as int, skip, work@);
                }
            }
        }
        let ghost pre = self.core;
        if to_token {
            self.core.finish_token(h, Ghost(skip), M::CHECKED);
            proof {
                self.core.lemma_tokened(pre, h, first as int, skip, work@);
                self.core.lemma_evolved_trans(pre, s1, 0, 0, 0);
            }
        } else {
            self.free_box(h);
            proof {
                self.core.lemma_drained_one(pre, s1, h, first as int, skip, work@);
            }
        }
        let ghost s4 = self.core;
        let r = self.drain(work);
        proof {
            if r is Ok {
                self.core.lemma_evolved_trans(s4, s1, 0, 0, 0);
            }
        }
        r
    }

    /// `is_last` が真で、子をすでに処理した物体の箱を空ける。ヘッダの `scan` を読まない。子の参照は、残すフィールドと
    /// して呼ぶ側に渡る (`release` で全部を残すのと同じ数の変化になる)。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側が最後の参照を持ち (rc が 1 か -1)、子の参照はもう処理してある。
    /// 種類は破棄処理を持たない (`Foreign` でない)。rc が正なら、持ち主は今の worker である。
    pub unsafe fn free_block(&mut self, w: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).settleable(),
            w.is_ptr_spec(),
            old(self).holds(w),
            !M::CHECKED ==> old(self).rc_value(w) == 1 || old(self).rc_value(w) == -1,
            !M::CHECKED ==> old(self).kind_of(w) != Kind::Foreign,
            !M::CHECKED ==> old(self).owned(w),
        ensures
            !M::CHECKED ==> r is Ok,
            r is Ok ==> final(self).wf() && final(self).handed_over(
                *old(self),
                w,
                Seq::new(old(self).count_of(w) as nat, |_i: int| true),
            ),
    {
        self.settle();
        let h = self.header_of(w);
        if M::CHECKED {
            self.checked_live(h)?;
            if self.core.meta_get(h).kind().has_finalizer() {
                return Err(HeapError::FinalizerSkipped);
            }
            if !self.core.rc_last(h) {
                return Err(HeapError::NotLast);
            }
            let rc = self.core.rc_get(h);
            self.checked_owner(h, rc)?;
            self.stats.rc_decrements = self.stats.rc_decrements.wrapping_add(1);
        }
        let ghost s0 = self.core;
        let ghost a = h@.addr;
        let ghost ps = s0.model@.edges.dom().filter(|e: Pos| e.0 == a);
        proof {
            s0.lemma_last_ref(a);
            s0.lemma_release_unrolled(a, p_count(s0.perm(a)));
            assert(s0.model@.edges.restrict(ps) == s0.out_edges(a));
        }
        self.core.begin_free(h, Ghost(ps), M::CHECKED);
        let ghost s1 = self.core;
        proof {
            self.core.lemma_begin_evolved(s0, a, ps);
        }
        self.free_box(h);
        proof {
            self.core.lemma_boxed_evolved(s1, a);
            self.core.lemma_by_trans(s1, s0, handover(s0.out_edges(a), a), 0);
            self.lemma_dummy_by(*old(self), handover(s0.out_edges(a), a));
        }
        Ok(())
    }
}

} // verus!

//! LIR の `release` (docs/spec/object-model.md の「核の操作」)。判定は `is_last` で、
//! rc が -1 の物体も最後の参照として、残すフィールドを `dup` せずに受け取る。共有の親から `lin` の子が届く形
//! (docs/spec/object-model.md の「`share`」) で、正しいプログラムの `File` を `dup` しないためである。

use std::mem;
use vstd::prelude::*;

#[cfg(verus_only)]
use super::Core;
use super::Heap;
#[cfg(verus_only)]
use super::chain_err;
use super::check::layout_check;
#[cfg(verus_only)]
use super::handover::{handover, kept_set, lemma_kept_set};
#[cfg(verus_only)]
use super::inv::{p_count, p_first, p_owner, p_scanned};
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
    /// `w` はこのヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ持っている。物体は種類 `Data` で、tag が
    /// `tag`、走査する語の数が `keep.len()` である。共有の物体は `lin` ビットを持たない。rc が正なら、持ち主は今の
    /// worker である。`is_last` が偽のときの残す子は `dup` の安全の条件を満たす。
    #[inline]
    pub unsafe fn release(&mut self, w: Word, tag: u16, keep: &[bool]) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            w.is_ptr_spec(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).release_ok(w, tag, keep@),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            old(self).release_ok(w, tag, keep@) ==> r is Ok || chain_err(r),
            old(self).release_ok(w, tag, keep@) && r is Ok ==> final(self).consistent()
                && final(self).handed_over(*old(self), w, keep@),
            final(self).kept_live(*old(self)),
            M::CHECKED ==> final(self).words_kept(*old(self)),
            final(self).settled(),
    {
        let ghost c = self.release_ok(w, tag, keep@);
        proof {
            self.core.lemma_kept_live_settled();
        }
        self.settle();
        let h = self.header_of(w);
        proof {
            if c {
                self.core.lemma_readable(w);
                self.lemma_checks_pass(w);
                if M::CHECKED {
                    self.core.lemma_code_state(h@.addr);
                }
            }
        }
        self.release_check(h, tag, keep.len())?;
        proof {
            if M::CHECKED {
                reveal(Core::building_wf);
            }
        }
        if self.core.rc_last(h) {
            proof {
                // 最後の参照を持つ物体は rc が 1 か -1 なので、塊の物体でない
                if !self.core.dom().contains(h@.addr) {
                    self.core.lemma_region_at(h@.addr);
                }
                if c {
                    self.core.lemma_last_ref(h@.addr);
                }
            }
            self.release_children(h, keep, false, Ghost(c))?;
            if M::CHECKED {
                self.stats.rc_decrements = self.stats.rc_decrements.wrapping_add(1);
            }
            Ok(())
        } else {
            self.release_shared(h, w, keep, Ghost(c))
        }
    }

    /// `release` の検査モードの検査。速いモードでは何も確かめない。
    #[inline]
    pub(super) fn release_check(&self, h: *mut Header, tag: u16, n: usize) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            self.core.sound(),
            self.core.checked() == M::CHECKED,
            self.core.has(h@.addr),
            h == self.core.block(h@.addr),
        ensures
            !M::CHECKED ==> r is Ok,
            r is Ok && M::CHECKED ==> {
                let a = h@.addr;
                &&& self.core.state(a) == BlockState::Live || self.core.state(a)
                    == BlockState::Building
                &&& self.core.kind(a) == Kind::Data
                &&& bits_tag(self.core.meta_bits(a)) == tag
                &&& self.core.count(a) == n
                &&& self.core.rc_of(a) < 0 ==> !self.core.lin(a)
            },
            ({
                let a = h@.addr;
                &&& self.core.state(a) == BlockState::Live || self.core.state(a)
                    == BlockState::Building
                &&& self.core.kind(a) == Kind::Data
                &&& bits_tag(self.core.meta_bits(a)) == tag
                &&& self.core.count(a) == n
                &&& self.core.rc_of(a) < 0 ==> !self.core.lin(a)
                &&& self.core.rc_of(a) > 0 ==> p_owner(self.core.perm(a)) == self.worker
            }) ==> r is Ok,
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

    /// 最後の参照でない `release`。残すフィールドを `dup` してから、物体の参照を手放す。
    #[inline]
    fn release_shared(
        &mut self,
        h: *mut Header,
        w: Word,
        keep: &[bool],
        Ghost(c): Ghost<bool>,
    ) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            w.is_ptr_spec(),
            w.bits_spec() == h@.addr,
            old(self).core.word_ok(w),
            h == old(self).core.block(h@.addr),
            old(self).core.has(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Live,
            old(self).core.count(h@.addr) == keep@.len(),
            !M::CHECKED ==> c,
            c ==> old(self).consistent(),
            c ==> (old(self).core.rc_of(h@.addr) != 0 ==> old(self).core.ext(h@.addr) >= 1),
            c ==> (old(self).rc_value(w) < 0 ==> !old(self).lin_of(w)),
            c ==> old(self).owned(w),
            c ==> forall|i: int|
                #![trigger keep@[i]]
                0 <= i < keep@.len() && keep@[i] ==> {
                    let c = old(self).scanned_word(w, i);
                    &&& c.is_ptr_spec() ==> !old(self).lin_of(c)
                    &&& old(self).owned(c)
                },
        ensures
            final(self).wf(),
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            !M::CHECKED ==> r is Ok,
            c ==> r is Ok || chain_err(r),
            c && r is Ok ==> final(self).consistent() && final(self).core.evolved_by(
                old(self).core,
                handover(old(self).core.kept_out(h@.addr, keep@), h@.addr),
            ) && final(self).dummy == old(self).dummy && final(self).region == old(self).region
                && final(self).worker == old(self).worker,
            final(self).kept_live(*old(self)),
            M::CHECKED ==> final(self).words_kept(*old(self)),
            final(self).core.model@.building is None,
    {
        let ghost s0 = self.core;
        let ghost a = h@.addr;
        proof {
            s0.lemma_shape(a);
            if M::CHECKED {
                s0.lemma_safe_live_block(a);
            }
            if c && s0.dom().contains(a) {
                s0.lemma_at(a);
            }
        }
        let (first, n) = self.core.scan_get(h);
        proof {
            s0.lemma_values_refl();
            s0.lemma_kept_live_refl();
            if c {
                s0.lemma_dupped_start();
            }
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
                !M::CHECKED ==> c,
                self.tokens == old(self).tokens,
                self.core.model@.tokens == old(self).core.model@.tokens,
                self.wf(),
                c ==> self.consistent(),
                self.core.model@.building is None,
                self.core.values_changed(s0),
                self.core.kept_live(s0),
                c ==> self.core.dupped(
                    s0,
                    s0.model@.edges.restrict(pos_set(a, kept_set(first as int, keep@, i as int))),
                ),
                c ==> s0.full(),
                M::CHECKED ==> s0.safe_wf(),
                s0 == old(self).core,
                a == h@.addr,
                w.is_ptr_spec(),
                w.bits_spec() == a,
                s0.word_ok(w),
                c ==> (old(self).core.rc_of(a) != 0 ==> old(self).core.ext(a) >= 1),
                s0.model@.building is None,
                s0.has(a),
                s0.state(a) == BlockState::Live,
                h == s0.block(a),
                first == p_first(s0.perm(a)),
                n == p_count(s0.perm(a)),
                n == keep@.len(),
                first + n <= s0.cap(a),
                i <= n,
                self.dummy == old(self).dummy,
                self.region == old(self).region,
                self.worker == old(self).worker,
                c ==> forall|j: int|
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
            assert(s1.has(a));
            assert(s1.state(a) == BlockState::Live);
            assert(s1.perm(a).slots == s0.perm(a).slots);
            assert(s1.perm(a).meta == s0.perm(a).meta);
            proof {
                if c {
                    s0.lemma_kept_out_step(a, first as int, keep@, i as int);
                }
            }
            if keep[i] {
                proof {
                    if c {
                        s1.lemma_live_slot(a, (first + i) as int);
                        s0.lemma_live_slot(a, (first + i) as int);
                        reveal(Core::blocks_wf);
                        reveal(Core::edges_wf);
                        if s0.dom().contains(a) {
                            assert(s0.block_wf(a));
                            assert(s0.scanned(a, (first + i) as int));
                            assert(s0.slot_word(s0.perm(a), a, (first + i) as int));
                        } else {
                            s0.lemma_region_at(a);
                        }
                    }
                    if M::CHECKED {
                        s0.lemma_safe_live_slot(a, (first + i) as int);
                    }
                }
                let child = self.core.slot_get(h, first + i);
                let ghost mid = *self;
                proof {
                    assert(self.core.word_ok(child) == s0.word_ok(child));
                }
                let r = unsafe { self.dup(child) };
                proof {
                    if r is Err {
                        self.core.lemma_values_trans(mid.core, s0);
                        self.core.lemma_kept_live_trans(mid.core, s0);
                        self.core.lemma_same_words(s0);
                    }
                }
                r?;
                proof {
                    self.core.lemma_values_trans(mid.core, s0);
                    self.core.lemma_kept_live_trans(mid.core, s0);
                    if c {
                        if !s0.dom().contains(a) {
                            // 塊の物体から出る辺はない。子も塊の物体なので、数えない
                            assert(s0.scanned(a, (first + i) as int));
                            if !child.is_ptr_spec() && s0.dom().contains(child.bits_spec()) {
                                s0.lemma_at(child.bits_spec());
                                crate::word::lemma_aligned_is_ptr(child.bits_spec());
                            }
                            self.core.lemma_dupped_skip(s0, s1, k0, child.bits_spec());
                            assert(!s0.model@.edges.dom().contains((a, (first + i) as int))) by {
                                if s0.model@.edges.dom().contains((a, (first + i) as int)) {
                                    assert(s0.edge_wf((a, (first + i) as int)));
                                }
                            }
                        } else if child.is_ptr_spec() {
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
            }
            i += 1;
        }
        let ghost s2 = self.core;
        proof {
            assert(s0.kept_out(a, keep@) == s0.model@.edges.restrict(
                pos_set(a, kept_set(first as int, keep@, n as int)),
            ));
            assert(self.core.word_ok(w) == s0.word_ok(w));
            // 塊の物体は rc 0 なので、外の参照を数えない
            if !s0.dom().contains(a) {
                s0.lemma_region_at(a);
            }
        }
        let r = unsafe { self.decref(w) };
        proof {
            if c && r is Ok {
                self.core.lemma_dupped_end(s0, s2, s0.kept_out(a, keep@), a);
            }
            self.core.lemma_kept_live_trans(s2, s0);
            if M::CHECKED {
                s2.lemma_same_words(s0);
            }
        }
        r
    }

    /// 最後の参照の `release`。物体を空ける途中にして残すフィールドの辺を呼ぶ側に渡し、残さない子を手放す。`to_token`
    /// なら箱を token にし、そうでなければ空ける。
    // 付けないと、文字列の操作を同じバイナリで使ったときに `release_reuse` の中へ展開されなくなり、一意な箱を token に
    // する道が `keep` を数える一般のループになって、`map-reuse` の命令の数が2倍を超えた
    // (docs/implementation/benchmarks.md の「S9b-1 (文字列)」)
    #[inline]
    pub(super) fn release_children(
        &mut self,
        h: *mut Header,
        keep: &[bool],
        to_token: bool,
        Ghost(c): Ghost<bool>,
    ) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Live,
            old(self).core.count(h@.addr) == keep@.len(),
            to_token ==> old(self).core.kind(h@.addr) == Kind::Data && old(self).core.rc_of(h@.addr)
                == 1,
            !M::CHECKED ==> c,
            c ==> old(self).consistent(),
            c ==> old(self).core.indeg(h@.addr) == 0,
            c ==> old(self).core.model@.refs[h@.addr] == 1,
            c ==> old(self).core.rc_of(h@.addr) != 0,
        ensures
            final(self).wf(),
            M::CHECKED ==> final(self).core.pages == old(self).core.pages,
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
            final(self).worker == old(self).worker,
            !M::CHECKED ==> r is Ok,
            c ==> r is Ok || chain_err(r),
            c && r is Ok ==> {
                &&& final(self).consistent()
                &&& final(self).core.evolved_by(
                    old(self).core,
                    handover(old(self).core.kept_out(h@.addr, keep@), h@.addr),
                )
            },
            r is Ok && to_token ==> {
                &&& final(self).core.dom().contains(h@.addr)
                &&& final(self).core.state(h@.addr) == BlockState::Token
                &&& M::CHECKED ==> !final(self).core.registered(h@.addr)
            },
            final(self).kept_live(*old(self)),
            M::CHECKED ==> final(self).words_kept(*old(self)),
            final(self).core.model@.building is None,
    {
        let ghost s0 = self.core;
        let ghost a = h@.addr;
        proof {
            s0.lemma_shape(a);
        }
        let (first, _) = self.core.scan_get(h);
        let ghost skip = kept_set(first as int, keep@, keep@.len() as int);
        self.core.begin_free(h, Ghost(pos_set(a, skip)), M::CHECKED, Ghost(c));
        let ghost s1 = self.core;
        proof {
            lemma_pos_set(a, skip);
            lemma_kept_set(first as int, keep@, keep@.len() as int);
            if c {
                self.core.lemma_begin_evolved(s0, a, pos_set(a, skip));
                self.core.lemma_release_start(s0, h, skip);
                assert(s0.kept_out(a, keep@) == s0.model@.edges.restrict(pos_set(a, skip)));
            }
        }
        let mut work = Vec::new();
        mem::swap(&mut work, &mut self.work);
        work.clear();
        let r = self.release_unkept(h, keep, first, to_token, &mut work, Ghost(c));
        work.clear();
        mem::swap(&mut work, &mut self.work);
        proof {
            if c && r is Ok {
                self.core.lemma_by_trans(s1, s0, handover(s0.kept_out(a, keep@), a), 0);
                self.lemma_dummy_by(*old(self), handover(s0.kept_out(a, keep@), a));
            }
            if M::CHECKED {
                self.lemma_safe_frame(*old(self));
                self.core.lemma_same_words(s0);
            }
            s1.lemma_kept_live_at(s0, a);
            self.core.lemma_kept_live_trans(s1, s0);
        }
        r
    }

    /// 残さない子を、走査の逆の順に手放す (`drain` と同じ順である)。手放し終えたら、箱を token にするか空け、最後の
    /// 参照を失った子を続けて空ける。
    #[inline]
    fn release_unkept(
        &mut self,
        h: *mut Header,
        keep: &[bool],
        first: usize,
        to_token: bool,
        work: &mut Vec<Word>,
        Ghost(c): Ghost<bool>,
    ) -> (r: Result<(), HeapError>)
        requires
            !M::CHECKED ==> c,
            c ==> old(self).core.full(),
            c ==> old(self).core.draining(
                h,
                first as int,
                keep@.len() as int,
                kept_set(first as int, keep@, keep@.len() as int),
                old(work)@,
            ),
            M::CHECKED ==> old(self).core.safe_wf(),
            M::CHECKED ==> old(self).core.safe_work(old(work)@, Some(h@.addr)),
            old(self).core.checked() == M::CHECKED,
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Dying,
            first == p_first(old(self).core.perm(h@.addr)),
            keep@.len() == p_count(old(self).core.perm(h@.addr)),
            first + keep@.len() <= old(self).core.cap(h@.addr),
            to_token ==> old(self).core.kind(h@.addr) == Kind::Data && old(self).core.rc_of(h@.addr)
                == 1,
        ensures
            final(self).building == old(self).building,
            M::CHECKED ==> final(self).core.pages == old(self).core.pages,
            M::CHECKED ==> final(self).core.dom() == old(self).core.dom(),
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            !M::CHECKED ==> r is Ok,
            c ==> r is Ok || chain_err(r),
            c ==> final(self).core.full(),
            M::CHECKED ==> final(self).core.safe_wf(),
            final(self).core.env == old(self).core.env,
            final(self).core.model@.building is None,
            c && r is Ok ==> final(self).core.quiet() && final(self).core.evolved(
                old(self).core,
                0,
                0,
            ),
            r is Ok && to_token ==> {
                &&& final(self).core.dom().contains(h@.addr)
                &&& final(self).core.state(h@.addr) == BlockState::Token
                &&& M::CHECKED ==> !final(self).core.registered(h@.addr)
            },
            final(self).core.kept_live(old(self).core),
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
            final(self).worker == old(self).worker,
    {
        let ghost skip = kept_set(first as int, keep@, keep@.len() as int);
        let ghost s1 = self.core;
        let n = keep.len();
        proof {
            if c {
                s1.lemma_evolved_self(0);
            }
            lemma_kept_set(first as int, keep@, n as int);
        }
        let mut i = n;
        while i > 0
            invariant
                !M::CHECKED ==> c,
                M::CHECKED ==> self.core.pages == old(self).core.pages,
                M::CHECKED ==> self.core.dom() == old(self).core.dom(),
                self.building == old(self).building,
                self.tokens == old(self).tokens,
                self.core.model@.tokens == old(self).core.model@.tokens,
                c ==> self.core.full(),
                M::CHECKED ==> self.core.safe_wf(),
                self.core.checked() == M::CHECKED,
                self.core.env == old(self).core.env,
                c ==> self.core.draining(h, first as int, i as int, skip, work@),
                M::CHECKED ==> self.core.safe_work(work@, Some(h@.addr)),
                self.core.dom().contains(h@.addr),
                h == self.core.block(h@.addr),
                self.core.state(h@.addr) == BlockState::Dying,
                self.core.perms@[h@.addr] == s1.perms@[h@.addr],
                skip == kept_set(first as int, keep@, n as int),
                forall|j: int| #[trigger]
                    skip.contains(j) <==> first <= j < first + n && keep@[j - first],
                n == keep@.len(),
                first == p_first(s1.perm(h@.addr)),
                n == p_count(s1.perm(h@.addr)),
                first + n <= self.core.cap(h@.addr),
                i <= n,
                c ==> self.core.evolved(s1, 0, 0),
                self.core.kept_live(s1),
                self.core.model@.building is None,
                self.dummy == old(self).dummy,
                self.region == old(self).region,
                self.worker == old(self).worker,
                to_token ==> s1.kind(h@.addr) == Kind::Data && s1.rc_of(h@.addr) == 1,
                c ==> s1.full(),
                s1 == old(self).core,
            decreases i,
        {
            i -= 1;
            if !keep[i] {
                proof {
                    if c {
                        self.core.lemma_draining_slot(h, first as int, i as int, skip, work@);
                    }
                    if M::CHECKED {
                        self.core.lemma_safe_at(h@.addr);
                        assert(p_scanned(self.core.perm(h@.addr), (first + i) as int));
                    }
                }
                let child = self.core.slot_get(h, first + i);
                if child.is_ptr() {
                    let c_h = self.header_of(child);
                    let ghost s3 = self.core;
                    let ghost wb = work@;
                    let last = self.release_ref(
                        c_h,
                        Ghost(Some((h@.addr, (first + i) as int))),
                        Ghost(c),
                    )?;
                    if last {
                        work.push(child);
                    }
                    proof {
                        self.core.lemma_kept_live_trans(s3, s1);
                        if c {
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
                        if M::CHECKED {
                            self.core.lemma_safe_work_step(s3, wb, work@, h@.addr, child, last);
                        }
                    }
                } else {
                    proof {
                        if c {
                            self.core.lemma_draining_skip(h, first as int, i as int, skip, work@);
                        }
                    }
                }
            } else {
                proof {
                    if c {
                        self.core.lemma_draining_skip(h, first as int, i as int, skip, work@);
                    }
                }
            }
        }
        let ghost pre = self.core;
        if to_token {
            proof {
                assert forall|j: int| #[trigger] skip.contains(j) implies pre.scanned(
                    h@.addr,
                    j,
                ) by {}
            }
            self.core.finish_token(h, Ghost(skip), M::CHECKED, Ghost(c));
            proof {
                self.core.lemma_kept_live_at(pre, h@.addr);
                self.core.lemma_kept_live_trans(pre, s1);
                if c {
                    self.core.lemma_tokened(pre, h, first as int, skip, work@);
                    self.core.lemma_evolved_trans(pre, s1, 0, 0, 0);
                }
                if M::CHECKED {
                    assert forall|w: Word| #[trigger] self.core.word_ok(w) == pre.word_ok(w) by {}
                    self.core.lemma_safe_work_freed(pre, work@, h@.addr);
                }
            }
        } else {
            self.free_box(h, Ghost(c));
            proof {
                self.core.lemma_kept_live_trans(pre, s1);
                if c {
                    self.core.lemma_drained_one(pre, s1, h, first as int, skip, work@);
                }
                if M::CHECKED {
                    self.core.lemma_safe_work_freed(pre, work@, h@.addr);
                }
            }
        }
        let ghost s4 = self.core;
        let r = self.drain(work, Ghost(c));
        proof {
            self.core.lemma_kept_live_trans(s4, s1);
            if c && r is Ok {
                self.core.lemma_evolved_trans(s4, s1, 0, 0, 0);
            }
            if to_token {
                assert(s4.state(h@.addr) == BlockState::Token);
                assert(self.core.tokens_kept(s4));
            }
        }
        r
    }

    /// `free_block` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn free_block_ok(&self, w: Word) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.holds(w)
        &&& self.rc_value(w) == 1 || self.rc_value(w) == -1
        &&& self.kind_of(w) != Kind::Foreign
        &&& self.owned(w)
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
            old(self).safe_settleable(),
            w.is_ptr_spec(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).free_block_ok(w),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            old(self).free_block_ok(w) ==> r is Ok && final(self).consistent()
                && final(self).handed_over(
                *old(self),
                w,
                Seq::new(old(self).count_of(w) as nat, |_i: int| true),
            ),
            final(self).kept_live(*old(self)),
            M::CHECKED ==> final(self).words_kept(*old(self)),
            final(self).settled(),
    {
        let ghost c = self.free_block_ok(w);
        proof {
            self.core.lemma_kept_live_settled();
        }
        self.settle();
        let h = self.header_of(w);
        proof {
            if c {
                self.core.lemma_readable(w);
                self.lemma_checks_pass(w);
            }
        }
        if M::CHECKED {
            self.checked_live(h)?;
            if self.core.meta_get(h).has_finalizer() {
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
            if M::CHECKED {
                reveal(Core::building_wf);
            }
            if !s0.dom().contains(a) {
                // 最後の参照を持つ物体は rc が 1 か -1 なので、塊の物体でない
                s0.lemma_region_at(a);
            }
            if c {
                s0.lemma_last_ref(a);
                s0.lemma_release_unrolled(a, p_count(s0.perm(a)));
                assert(s0.model@.edges.restrict(ps) == s0.out_edges(a));
            }
        }
        self.core.begin_free(h, Ghost(ps), M::CHECKED, Ghost(c));
        let ghost s1 = self.core;
        proof {
            if c {
                self.core.lemma_begin_evolved(s0, a, ps);
            }
        }
        self.free_box(h, Ghost(c));
        proof {
            if c {
                self.core.lemma_boxed_evolved(s1, a);
                self.core.lemma_by_trans(s1, s0, handover(s0.out_edges(a), a), 0);
                self.lemma_dummy_by(*old(self), handover(s0.out_edges(a), a));
            }
            if M::CHECKED {
                self.lemma_safe_frame(*old(self));
                self.core.lemma_same_words(s0);
            }
            s1.lemma_kept_live_at(s0, a);
            self.core.lemma_kept_live_trans(s1, s0);
            self.core.lemma_kept_live_trans(s0, old(self).core);
        }
        Ok(())
    }
}

} // verus!

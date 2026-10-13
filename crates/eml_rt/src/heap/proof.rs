//! ヒープの証明の部品。確保器の変化の枠の補題、新しいブロックをヒープに入れる手順、構築中の物体を生きている物体に
//! する手順を置く。`publish` と `settle` は ghost の値だけを変える関数で、実行の時には何もしない。

use vstd::prelude::*;

use super::Core;
#[cfg(verus_only)]
use super::inv::{p_init, p_scanned, p_word};
use super::model::BlockState;
#[cfg(verus_only)]
use super::model::{Model, lemma_indeg_remove, lemma_indeg_zero};
#[cfg(verus_only)]
use crate::meta::HEADER_WORDS;
use crate::meta::{BlockPerm, Header};
#[cfg(verus_only)]
use crate::rc::{magnitude, pinned};
#[cfg(verus_only)]
use crate::word::Word;

verus! {

impl Core {
    /// `old` の生きている物体のうち、不死か、外の参照を持ち続けるものは、同じ中身で生きている。外の参照の数は、`w` だけ
    /// `delta` 変わる。
    pub(crate) open spec fn evolved(&self, old: Core, w: usize, delta: int) -> bool {
        &&& self.checked == old.checked
        &&& forall|b: usize|
            #![trigger old.dom().contains(b)]
            old.dom().contains(b) && old.settled_state(b) == BlockState::Live && (old.rc_of(b) == 0
                || old.ext(b) + (if b == w {
                delta
            } else {
                0
            }) >= 1) ==> {
                &&& self.dom().contains(b)
                &&& self.state(b) == BlockState::Live
                &&& self.perm(b).meta == old.perm(b).meta
                &&& self.perm(b).slots == old.perm(b).slots
                &&& self.block(b) == old.block(b)
                &&& self.cap(b) == old.cap(b)
                &&& (old.rc_of(b) == 0) == (self.rc_of(b) == 0)
                &&& old.rc_of(b) != 0 ==> self.ext(b) == old.ext(b) + if b == w {
                    delta
                } else {
                    0
                }
                &&& self.model@.owner[b] == old.model@.owner[b]
            }
    }

    /// 空ける途中でも隔離したのでもないブロック。解放の連鎖の止まることの尺度に使う。
    pub(crate) open spec fn active(&self) -> Set<usize> {
        self.dom().filter(|b: usize| self.state(b) != BlockState::Freed)
    }

    /// 空ける途中のブロックはない。
    pub(crate) open spec fn quiet(&self) -> bool {
        forall|b: usize| #[trigger] self.dom().contains(b) ==> self.state(b) != BlockState::Dying
    }

    /// `settle` の後の状態。構築中の物体は生きている物体になる。
    pub(crate) open spec fn settled_state(&self, b: usize) -> BlockState {
        if self.model@.building == Some(b) {
            BlockState::Live
        } else {
            self.state(b)
        }
    }

    /// `old` のブロックは、どれも同じ権限と形と数を持ち続ける。構築中の物体は、生きている物体になってよい。
    pub(crate) open spec fn frame(&self, old: Core) -> bool {
        &&& forall|b: usize| #[trigger]
            old.dom().contains(b) ==> {
                &&& self.dom().contains(b)
                &&& self.perm(b) == old.perm(b)
                &&& self.state(b) == old.settled_state(b)
                &&& self.block(b) == old.block(b)
                &&& self.cap(b) == old.cap(b)
                &&& self.model@.refs[b] == old.model@.refs[b]
                &&& self.model@.stuck[b] == old.model@.stuck[b]
                &&& self.model@.owner[b] == old.model@.owner[b]
            }
        &&& self.model@.edges == old.model@.edges
        &&& self.checked == old.checked
    }

    /// 枠をつなぐ。`settle` の後 (`s1`)、確保器からブロックを取った後 (`mid`)、そのブロックを入れた後 (`self`) の3つの変化を
    /// 合わせると、`s0` からの枠になる。
    pub(crate) proof fn lemma_frame_chain(&self, mid: Core, s1: Core, s0: Core)
        requires
            s1.frame(s0),
            s1.dom() == s0.dom(),
            s1.model@.building is None,
            mid.perms == s1.perms,
            mid.model == s1.model,
            mid.checked == s1.checked,
            forall|b: usize| #[trigger]
                s1.dom().contains(b) ==> mid.block(b) == s1.block(b) && mid.cap(b) == s1.cap(b),
            self.frame(mid),
        ensures
            self.frame(s0),
    {
        assert forall|b: usize| #[trigger] s0.dom().contains(b) implies {
            &&& self.dom().contains(b)
            &&& self.perm(b) == s0.perm(b)
            &&& self.state(b) == s0.settled_state(b)
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
            &&& self.model@.refs[b] == s0.model@.refs[b]
            &&& self.model@.stuck[b] == s0.model@.stuck[b]
            &&& self.model@.owner[b] == s0.model@.owner[b]
        } by {
            assert(s1.dom().contains(b));
            assert(mid.dom().contains(b));
        }
    }

    /// 確保器が変わっても、ヒープのブロックの形が同じなら、不変条件の残りは変わらない。
    pub(crate) proof fn lemma_pages_frame(&self, s0: Core)
        requires
            s0.rest_wf(),
            self.perms == s0.perms,
            self.model == s0.model,
            self.checked == s0.checked,
            self.pages.wf(),
            self.pages.prefixed() == s0.pages.prefixed(),
            forall|b: usize| #[trigger]
                s0.dom().contains(b) ==> self.block(b) == s0.block(b) && self.cap(b) == s0.cap(b)
                    && self.pages.carved_block(b),
        ensures
            self.rest_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w) by {
            if w.is_ptr_spec() && s0.dom().contains(w.bits_spec()) {
                assert(self.block(w.bits_spec()) == s0.block(w.bits_spec()));
            }
        }
        assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.block_wf(h) by {
            assert(s0.block_wf(h));
            let p = self.perm(h);
            assert forall|i: int| #[trigger]
                p_scanned(p, i) && s0.slot_word(p, h, i) implies self.slot_word(p, h, i) by {
                assert(self.word_ok(p_word(p, i)) == s0.word_ok(p_word(p, i)));
            }
            assert forall|i: int| #[trigger]
                self.model@.written[h].contains(i) && s0.slot_word(p, h, i) implies self.slot_word(
                p,
                h,
                i,
            ) by {
                assert(self.word_ok(p_word(p, i)) == s0.word_ok(p_word(p, i)));
            }
        }
    }

    /// 空のヒープは、不変条件を満たす。
    pub(crate) proof fn lemma_empty(&self)
        requires
            self.pages.wf(),
            self.pages.prefixed() == self.checked@,
            self.pages.in_use() == Set::<usize>::empty(),
            self.perms@ == Map::<usize, BlockPerm>::empty(),
            self.model@.state == Map::<usize, BlockState>::empty(),
            self.model@.refs == Map::<usize, nat>::empty(),
            self.model@.edges == Map::<(usize, int), usize>::empty(),
            self.model@.stuck == Map::<usize, bool>::empty(),
            self.model@.owner == Map::<usize, u32>::empty(),
            self.model@.written == Map::<usize, Set<int>>::empty(),
            self.model@.building is None,
        ensures
            self.wf(),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(self.dom() =~= self.pages.in_use());
    }

    /// 確保器からブロックを取った後も、ヒープの不変条件は保たれ、取ったブロックはまだヒープの外にある。
    pub(crate) proof fn lemma_alloc(&self, s0: Core, words: usize, h: *mut Header, perm: BlockPerm)
        requires
            s0.wf(),
            self.perms == s0.perms,
            self.model == s0.model,
            self.checked == s0.checked,
            self.pages.alloc_post(s0.pages, words, h, perm),
        ensures
            self.pending(h@.addr),
            h == self.block(h@.addr),
            self.pages.carved_block(h@.addr),
            perm.wf(h, self.cap(h@.addr), self.checked@),
            self.cap(h@.addr) + HEADER_WORDS >= words,
            forall|b: usize| #[trigger]
                s0.dom().contains(b) ==> self.block(b) == s0.block(b) && self.cap(b) == s0.cap(b)
                    && self.pages.carved_block(b),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        assert forall|b: usize| #[trigger] s0.dom().contains(b) implies self.block(b) == s0.block(b)
            && self.cap(b) == s0.cap(b) && self.pages.carved_block(b) by {
            assert(s0.pages.in_use().contains(b));
        }
        self.lemma_pages_frame(s0);
        assert(self.dom().insert(h@.addr) =~= self.pages.in_use());
    }

    /// 書き終えた構築中の物体を、生きている物体にする。
    #[inline(always)]
    pub(crate) fn settle(&mut self)
        requires
            old(self).wf(),
            old(self).settleable(),
        ensures
            final(self).wf(),
            final(self).model@.building is None,
            final(self).frame(*old(self)),
            final(self).dom() == old(self).dom(),
            final(self).pages == old(self).pages,
            forall|w: Word| #[trigger] final(self).word_ok(w) == old(self).word_ok(w),
    {
        proof {
            let s0 = *self;
            if let Some(b) = self.model@.building {
                self.model = Ghost(
                    Model {
                        state: self.model@.state.insert(b, BlockState::Live),
                        building: None,
                        ..self.model@
                    },
                );
                self.lemma_settle(s0, b);
            }
        }
    }

    pub(crate) proof fn lemma_settle(&self, s0: Core, b: usize)
        requires
            s0.wf(),
            s0.settleable(),
            s0.model@.building == Some(b),
            self.perms == s0.perms,
            self.pages == s0.pages,
            self.checked == s0.checked,
            self.model@ == (Model {
                state: s0.model@.state.insert(b, BlockState::Live),
                building: None,
                ..s0.model@
            }),
        ensures
            self.wf(),
            self.frame(s0),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(s0.state(b) == BlockState::Building);
        assert(self.model@.state.dom() =~= self.dom());
        assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.block_wf(h) by {
            assert(s0.block_wf(h));
            if h == b {
                let p = self.perm(b);
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.scanned(b, i));
                    assert(s0.model@.written[b].contains(i));
                }
            }
        }
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
        }
        assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.counted_wf(h) by {
            assert(s0.counted_wf(h));
        }
    }

    /// 確保器から取ったブロック `h` を、状態 `st` の物体としてヒープに入れる。ペイロードは書いてあり、ポインタの語はない。
    #[inline(always)]
    pub(crate) fn publish(
        &mut self,
        Ghost(h): Ghost<*mut Header>,
        Tracked(perm): Tracked<BlockPerm>,
        Ghost(st): Ghost<BlockState>,
        Ghost(owner): Ghost<u32>,
    )
        requires
            old(self).pending(h@.addr),
            old(self).model@.building is None,
            h == old(self).block(h@.addr),
            old(self).pages.carved_block(h@.addr),
            old(self).perm_wf(perm, h@.addr, st, Set::empty()),
            st == BlockState::Live || st == BlockState::Building,
            perm.rc.value() == 0 || perm.rc.value() == 1,
        ensures
            final(self).wf(),
            final(self).dom() == old(self).dom().insert(h@.addr),
            final(self).perm(h@.addr) == perm,
            final(self).state(h@.addr) == st,
            final(self).model@.refs[h@.addr] == if perm.rc.value() == 1 {
                1nat
            } else {
                0nat
            },
            final(self).model@.written[h@.addr] == Set::<int>::empty(),
            final(self).model@.building == if st == BlockState::Building {
                Some(h@.addr)
            } else {
                None
            },
            final(self).frame(*old(self)),
            final(self).pages == old(self).pages,
            final(self).indeg(h@.addr) == 0,
            forall|w: Word| old(self).word_ok(w) ==> #[trigger] final(self).word_ok(w),
    {
        proof {
            let s0 = *self;
            let hs = h@.addr;
            self.perms.borrow_mut().tracked_insert(hs, perm);
            self.model = Ghost(
                Model {
                    state: self.model@.state.insert(hs, st),
                    refs: self.model@.refs.insert(
                        hs,
                        if perm.rc.value() == 1 {
                            1nat
                        } else {
                            0nat
                        },
                    ),
                    stuck: self.model@.stuck.insert(hs, false),
                    owner: self.model@.owner.insert(hs, owner),
                    written: self.model@.written.insert(hs, Set::empty()),
                    building: if st == BlockState::Building {
                        Some(hs)
                    } else {
                        None
                    },
                    ..self.model@
                },
            );
            self.lemma_publish(s0, hs);
        }
    }

    proof fn lemma_publish(&self, s0: Core, h: usize)
        requires
            s0.pending(h),
            s0.model@.building is None,
            s0.pages.carved_block(h),
            self.pages == s0.pages,
            self.checked == s0.checked,
            self.perms@ == s0.perms@.insert(h, self.perm(h)),
            s0.perm_wf(self.perm(h), h, self.state(h), Set::empty()),
            self.state(h) == BlockState::Live || self.state(h) == BlockState::Building,
            self.rc_of(h) == 0 || self.rc_of(h) == 1,
            self.model@.state == s0.model@.state.insert(h, self.state(h)),
            self.model@.refs == s0.model@.refs.insert(
                h,
                if self.rc_of(h) == 1 {
                    1nat
                } else {
                    0nat
                },
            ),
            self.model@.stuck == s0.model@.stuck.insert(h, false),
            self.model@.owner.dom() == s0.model@.owner.dom().insert(h),
            forall|b: usize| b != h ==> #[trigger] self.model@.owner[b] == s0.model@.owner[b],
            self.model@.written == s0.model@.written.insert(h, Set::empty()),
            self.model@.edges == s0.model@.edges,
            self.model@.building == if self.state(h) == BlockState::Building {
                Some(h)
            } else {
                None
            },
        ensures
            self.wf(),
            self.frame(s0),
            self.indeg(h) == 0,
            forall|w: Word| s0.word_ok(w) ==> #[trigger] self.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        // 新しいブロックを指す辺はない
        assert forall|e: (usize, int)| #[trigger]
            s0.model@.edges.dom().contains(e) implies s0.model@.edges[e] != h by {
            assert(s0.edge_wf(e));
        }
        assert(super::model::targets(self.model@.edges, h) =~= Set::empty());
        assert(self.dom() =~= self.pages.in_use());
        assert(self.model@.state.dom() =~= self.dom());
        assert(self.model@.refs.dom() =~= self.dom());
        assert(self.model@.stuck.dom() =~= self.dom());
        assert(self.model@.owner.dom() =~= self.dom());
        assert(self.model@.written.dom() =~= self.dom());
        assert forall|w: Word| s0.word_ok(w) implies #[trigger] self.word_ok(w) by {}
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            let p = self.perm(b);
            if b == h {
                assert forall|i: int| #[trigger]
                    p_scanned(p, i) && s0.slot_word(p, h, i) implies self.slot_word(p, h, i) by {}
            } else {
                assert(s0.block_wf(b));
                assert forall|i: int| #[trigger]
                    p_scanned(p, i) && s0.slot_word(p, b, i) implies self.slot_word(p, b, i) by {}
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) && s0.slot_word(
                        p,
                        b,
                        i,
                    ) implies self.slot_word(p, b, i) by {}
            }
        }
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            if b != h {
                assert(s0.counted_wf(b));
            }
        }
    }

    /// 物体 `h` の rc と数を替える。`dying` なら、最後の参照を手放したので空ける途中にする。
    pub(crate) proof fn lemma_count_step(&self, s0: Core, h: usize, dying: bool)
        requires
            s0.wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            self.pages == s0.pages,
            self.checked == s0.checked,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(h).meta == s0.perm(h).meta,
            self.perm(h).slots == s0.perm(h).slots,
            self.perm(h).prefix == s0.perm(h).prefix,
            self.perm(h).rc.ptr() == s0.perm(h).rc.ptr(),
            self.perm(h).rc.is_init(),
            self.model@.state == s0.model@.state.insert(
                h,
                if dying {
                    BlockState::Dying
                } else {
                    BlockState::Live
                },
            ),
            self.model@.refs.dom() == s0.model@.refs.dom(),
            forall|b: usize| b != h ==> #[trigger] self.model@.refs[b] == s0.model@.refs[b],
            self.model@.stuck.dom() == s0.model@.stuck.dom(),
            forall|b: usize| b != h ==> #[trigger] self.model@.stuck[b] == s0.model@.stuck[b],
            self.model@.edges == s0.model@.edges,
            self.model@.owner == s0.model@.owner,
            self.model@.written == s0.model@.written,
            self.model@.building == s0.model@.building,
            dying ==> s0.indeg(h) == 0,
            !dying && self.rc_of(h) != 0 ==> {
                let rc = self.rc_of(h);
                &&& self.model@.refs[h] >= s0.indeg(h)
                &&& self.model@.stuck[h] ==> pinned(rc)
                &&& !self.model@.stuck[h] ==> !pinned(rc) && magnitude(rc) == self.model@.refs[h]
                    && self.model@.refs[h] > 0
            },
        ensures
            self.wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
            forall|b: usize| #[trigger] self.indeg(b) == s0.indeg(b),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(self.model@.state.dom() =~= self.dom());
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            let p = self.perm(b);
            let q = s0.perm(b);
            if b == h {
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(p, b, i)
                    && p_init(p, i) by {
                    assert(p_scanned(q, i));
                    assert(s0.slot_word(q, b, i));
                }
            } else {
                assert(p == q);
            }
        }
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
            if dying && s0.model@.edges[e] == h {
                lemma_indeg_zero(s0.model@.edges, h, e);
            }
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 空ける途中の親の辺 `e` を外す。
    pub(crate) proof fn lemma_edge_removed(&self, s0: Core, e: (usize, int))
        requires
            s0.wf(),
            s0.model@.edges.dom().contains(e),
            s0.state(e.0) == BlockState::Dying,
            self.pages == s0.pages,
            self.checked == s0.checked,
            self.perms == s0.perms,
            self.model@ == (Model { edges: s0.model@.edges.remove(e), ..s0.model@ }),
        ensures
            self.wf(),
            self.indeg(s0.model@.edges[e]) + 1 == s0.indeg(s0.model@.edges[e]),
            forall|b: usize| b != s0.model@.edges[e] ==> #[trigger] self.indeg(b) == s0.indeg(b),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        lemma_indeg_remove(s0.model@.edges, e);
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            let p = self.perm(b);
            if s0.state(b) == BlockState::Live {
                assert(b != e.0);
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.slot_word(p, b, i));
                }
            }
            if s0.state(b) == BlockState::Building {
                assert(b != e.0);
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) implies p_scanned(p, i) && self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.slot_word(p, b, i));
                }
            }
        }
        assert forall|f: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(f) implies self.edge_wf(f) by {
            assert(s0.edge_wf(f));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 速いモードで、空ける途中のブロック `h` を確保器に返した後の状態。
    pub(crate) proof fn lemma_block_released(&self, s0: Core, h: usize)
        requires
            s0.wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Dying,
            s0.indeg(h) == 0,
            forall|j: int| !(#[trigger] s0.model@.edges.dom().contains((h, j))),
            self.checked == s0.checked,
            self.perms@ == s0.perms@.remove(h),
            self.model@ == (Model {
                state: s0.model@.state.remove(h),
                refs: s0.model@.refs.remove(h),
                stuck: s0.model@.stuck.remove(h),
                owner: s0.model@.owner.remove(h),
                written: s0.model@.written.remove(h),
                ..s0.model@
            }),
            self.pages.wf(),
            self.pages.prefixed() == s0.pages.prefixed(),
            self.pages.in_use() == s0.pages.in_use().remove(h),
            forall|b: usize| #[trigger]
                self.pages.in_use().contains(b) ==> self.pages.same_block(s0.pages, b),
        ensures
            self.wf(),
            self.dom() == s0.dom().remove(h),
            forall|b: usize| #[trigger] self.indeg(b) == s0.indeg(b),
            forall|w: Word|
                s0.word_ok(w) && (w.is_ptr_spec() ==> w.bits_spec() != h)
                    ==> #[trigger] self.word_ok(w),
            forall|b: usize|
                b != h && s0.dom().contains(b) ==> #[trigger] self.block(b) == s0.block(b)
                    && self.cap(b) == s0.cap(b),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(self.dom() =~= s0.dom().remove(h));
        assert(self.dom() =~= self.pages.in_use());
        assert forall|b: usize| b != h && s0.dom().contains(b) implies #[trigger] self.block(b)
            == s0.block(b) && self.cap(b) == s0.cap(b) by {
            assert(self.pages.in_use().contains(b));
        }
        assert forall|w: Word|
            s0.word_ok(w) && (w.is_ptr_spec() ==> w.bits_spec()
                != h) implies #[trigger] self.word_ok(w) by {
            if w.is_ptr_spec() {
                assert(self.pages.in_use().contains(w.bits_spec()));
            }
        }
        assert(self.model@.state.dom() =~= self.dom());
        assert(self.model@.refs.dom() =~= self.dom());
        assert(self.model@.stuck.dom() =~= self.dom());
        assert(self.model@.owner.dom() =~= self.dom());
        assert(self.model@.written.dom() =~= self.dom());
        // 残ったブロックの語は、h を指さない
        assert forall|e: (usize, int)| #[trigger]
            s0.model@.edges.dom().contains(e) implies s0.model@.edges[e] != h && e.0 != h by {
            lemma_indeg_zero(s0.model@.edges, h, e);
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            assert(self.pages.in_use().contains(b));
            let p = self.perm(b);
            if s0.state(b) == BlockState::Live {
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.slot_word(p, b, i));
                    if p_word(p, i).is_ptr_spec() {
                        assert(s0.edge_wf((b, i)));
                    }
                }
            }
            if s0.state(b) == BlockState::Building {
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) implies p_scanned(p, i) && self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.slot_word(p, b, i));
                    if p_word(p, i).is_ptr_spec() {
                        assert(s0.edge_wf((b, i)));
                    }
                }
            }
        }
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
            assert(self.pages.in_use().contains(s0.model@.edges[e]));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 検査モードで、空ける途中のブロック `h` を隔離した後の状態。権限はヒープに残し、前置きの状態だけを替える。
    pub(crate) proof fn lemma_block_freed(&self, s0: Core, h: usize)
        requires
            s0.wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Dying,
            s0.indeg(h) == 0,
            forall|j: int| !(#[trigger] s0.model@.edges.dom().contains((h, j))),
            self.checked == s0.checked,
            self.pages == s0.pages,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(h).wf(self.block(h), self.cap(h), self.checked@),
            self.perm(h).prefix matches Some(q) ==> q.first.is_init() && q.state.is_init(),
            self.perm(h).rc.is_init(),
            self.perm(h).meta == s0.perm(h).meta,
            self.model@ == (Model {
                state: s0.model@.state.insert(h, BlockState::Freed),
                ..s0.model@
            }),
        ensures
            self.wf(),
            self.dom() == s0.dom(),
            forall|b: usize| #[trigger] self.indeg(b) == s0.indeg(b),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(self.model@.state.dom() =~= self.dom());
        assert forall|e: (usize, int)| #[trigger]
            s0.model@.edges.dom().contains(e) implies s0.model@.edges[e] != h && e.0 != h by {
            lemma_indeg_zero(s0.model@.edges, h, e);
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            if b != h {
                let p = self.perm(b);
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
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }
}

} // verus!

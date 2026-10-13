//! ヒープの証明の部分 (`Core`) の読み書き。ブロックの欄を権限の上で読み書きし、RC の遷移のモデルの側を更新する。

use vstd::prelude::*;
use vstd::raw_ptr::{ptr_mut_write, ptr_ref};

use super::Core;
#[cfg(verus_only)]
use super::inv::{p_count, p_first, p_init, p_scanned, p_word};
use super::model::Pos;
#[cfg(verus_only)]
use super::model::{BlockState, Model, lemma_indeg_pos};
use crate::cell::{rc_decref_shared, rc_dup_shared, rc_is_last, rc_load, rc_store};
use crate::meta::{Header, Meta, meta_ptr, rc_ptr, slot_ptr};
#[cfg(verus_only)]
use crate::meta::{bits_kind, bits_scan, bits_tag};
#[cfg(verus_only)]
use crate::rc::{Obj, decref_next, dup_next, lemma_decref, lemma_dup};
use crate::word::Word;

verus! {

impl Core {
    /// ブロック `h` の位置と大きさの事実。
    pub(crate) proof fn lemma_at(&self, h: usize)
        requires
            self.wf(),
            self.dom().contains(h),
        ensures
            self.block_wf(h),
            self.pages.carved_block(h),
            self.block(h)@.addr == h,
            h % 8 == 0,
            h >= 8,
            h + 8 * (1 + self.cap(h)) <= usize::MAX,
    {
        reveal(Core::blocks_wf);
        assert(self.block_wf(h));
        self.pages.lemma_block_bounds(h);
    }

    #[inline]
    pub(crate) fn rc_get(&self, h: *mut Header) -> (r: i32)
        requires
            self.wf(),
            self.dom().contains(h@.addr),
            h == self.block(h@.addr),
        ensures
            r == self.rc_of(h@.addr),
    {
        proof {
            self.lemma_at(h@.addr);
        }
        let tracked p = self.perms.borrow().tracked_borrow(h@.addr);
        rc_load(rc_ptr(h), Tracked(&p.rc))
    }

    #[inline]
    pub(crate) fn meta_get(&self, h: *mut Header) -> (r: Meta)
        requires
            self.wf(),
            self.dom().contains(h@.addr),
            h == self.block(h@.addr),
        ensures
            r.bits_spec() == self.meta_bits(h@.addr),
            r.kind_spec() == self.kind(h@.addr),
            r.scan_spec() == bits_scan(self.meta_bits(h@.addr)),
            r.lin_spec() == self.lin(h@.addr),
            r.tag_spec() == bits_tag(self.meta_bits(h@.addr)),
    {
        proof {
            self.lemma_at(h@.addr);
        }
        let tracked p = self.perms.borrow().tracked_borrow(h@.addr);
        let bits = *ptr_ref(meta_ptr(h), Tracked(&p.meta));
        Meta::from_bits(bits)
    }

    /// 語を読む。
    #[inline]
    pub(crate) fn slot_get(&self, h: *mut Header, i: usize) -> (r: Word)
        requires
            self.wf(),
            self.dom().contains(h@.addr),
            h == self.block(h@.addr),
            p_init(self.perm(h@.addr), i as int),
        ensures
            r == p_word(self.perm(h@.addr), i as int),
    {
        proof {
            self.lemma_at(h@.addr);
            assert(self.perm(h@.addr).slots.contains_key(i as int));
        }
        let tracked p = self.perms.borrow().tracked_borrow(h@.addr);
        *ptr_ref(slot_ptr(h, i), Tracked(p.slots.tracked_borrow(i as int)))
    }

    /// 走査する語の始まりと数。
    #[inline]
    pub(crate) fn scan_get(&self, h: *mut Header) -> (r: (usize, usize))
        requires
            self.wf(),
            self.dom().contains(h@.addr),
            h == self.block(h@.addr),
            self.state(h@.addr) != BlockState::Freed,
        ensures
            r.0 == p_first(self.perm(h@.addr)),
            r.1 == p_count(self.perm(h@.addr)),
            r.0 + r.1 <= self.cap(h@.addr),
    {
        proof {
            self.lemma_at(h@.addr);
        }
        let m = self.meta_get(h);
        if m.scan() == Meta::EXTENDED_SCAN {
            (1, self.slot_get(h, 0).bits())
        } else {
            (0, m.scan() as usize)
        }
    }

    /// rc が 1 か -1 か。
    #[inline]
    pub(crate) fn rc_last(&self, h: *mut Header) -> (r: bool)
        requires
            self.wf(),
            self.dom().contains(h@.addr),
            h == self.block(h@.addr),
        ensures
            r == (self.rc_of(h@.addr) == 1 || self.rc_of(h@.addr) == -1),
    {
        proof {
            self.lemma_at(h@.addr);
        }
        let tracked p = self.perms.borrow().tracked_borrow(h@.addr);
        rc_is_last(rc_ptr(h), Tracked(&p.rc))
    }

    /// ペイロードの語の数。
    #[inline]
    pub(crate) fn cap_get(&self, h: *mut Header) -> (r: usize)
        requires
            self.wf(),
            self.dom().contains(h@.addr),
            h == self.block(h@.addr),
        ensures
            r == self.cap(h@.addr),
    {
        proof {
            self.lemma_at(h@.addr);
        }
        self.pages.cap_of(h)
    }

    /// i 語目に `v` を書く。モデルはまだ替えない。
    #[inline]
    pub(crate) fn slot_put(&mut self, h: *mut Header, i: usize, v: Word)
        requires
            old(self).wf(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            i < old(self).cap(h@.addr),
        ensures
            final(self).slot_changed(*old(self), h@.addr, i as int),
            p_word(final(self).perm(h@.addr), i as int) == v,
            final(self).model == old(self).model,
    {
        let ghost s0 = *self;
        proof {
            self.lemma_at(h@.addr);
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        let tracked mut slot = p.slots.tracked_remove(i as int);
        ptr_mut_write(slot_ptr(h, i), Tracked(&mut slot), v);
        proof {
            p.slots.tracked_insert(i as int, slot);
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
            assert(self.perms@.dom() =~= s0.perms@.dom());
            assert(self.perm(h@.addr).slots.dom() =~= s0.perm(h@.addr).slots.dom());
        }
    }

    /// rc を書く。モデルはまだ替えない。
    #[inline]
    pub(crate) fn rc_put(&mut self, h: *mut Header, v: i32)
        requires
            old(self).wf(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
        ensures
            final(self).rc_changed(*old(self), h@.addr),
            final(self).rc_of(h@.addr) == v,
    {
        proof {
            self.lemma_at(h@.addr);
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        rc_store(rc_ptr(h), Tracked(&mut p.rc), v);
        proof {
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
        }
    }

    /// 共有の数を1つ進める (`dup`)。
    #[inline]
    pub(crate) fn rc_up_shared(&mut self, h: *mut Header)
        requires
            old(self).wf(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            old(self).rc_of(h@.addr) > i32::MIN,
        ensures
            final(self).rc_changed(*old(self), h@.addr),
            final(self).rc_of(h@.addr) == old(self).rc_of(h@.addr) - 1,
    {
        proof {
            self.lemma_at(h@.addr);
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        rc_dup_shared(rc_ptr(h), Tracked(&mut p.rc));
        proof {
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
        }
    }

    /// 共有の数を1つ戻し (`decref`)、最後の参照なら真を返す。
    #[inline]
    pub(crate) fn rc_down_shared(&mut self, h: *mut Header) -> (r: bool)
        requires
            old(self).wf(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            old(self).rc_of(h@.addr) < 0,
        ensures
            final(self).rc_changed(*old(self), h@.addr),
            final(self).rc_of(h@.addr) == old(self).rc_of(h@.addr) + 1,
            r == (old(self).rc_of(h@.addr) == -1),
    {
        proof {
            self.lemma_at(h@.addr);
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        let r = rc_decref_shared(rc_ptr(h), Tracked(&mut p.rc));
        proof {
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
        }
        r
    }

    /// `old` から、ブロック `h` の rc の値だけが変わった。
    pub(crate) open spec fn rc_changed(&self, old: Core, h: usize) -> bool {
        &&& self.pages == old.pages
        &&& self.checked == old.checked
        &&& self.model == old.model
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& self.perm(h).meta == old.perm(h).meta
        &&& self.perm(h).slots == old.perm(h).slots
        &&& self.perm(h).prefix == old.perm(h).prefix
        &&& self.perm(h).rc.ptr() == old.perm(h).rc.ptr()
        &&& self.perm(h).rc.is_init()
    }

    /// RC の遷移のモデルの側。`s0` は遷移の前の状態で、メモリの rc はもう書いてある。`up` なら `dup`、そうでなければ
    /// `decref` で、`edge` は解放の連鎖が手放す辺である。`decref` で最後の参照なら、空ける途中にする。
    #[inline(always)]
    pub(crate) fn count_ghost(
        &mut self,
        Ghost(s0): Ghost<Core>,
        Ghost(h): Ghost<usize>,
        Ghost(up): Ghost<bool>,
        Ghost(edge): Ghost<Option<Pos>>,
    )
        requires
            s0.wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            s0.model@.building is None,
            old(self).rc_changed(s0, h),
            up ==> edge is None,
            edge matches Some(e) ==> s0.model@.edges.dom().contains(e) && s0.model@.edges[e] == h
                && s0.state(e.0) == BlockState::Dying,
            s0.rc_of(h) != 0 ==> {
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                &&& !up && edge is None ==> s0.ext(h) >= 1
                &&& up ==> old(self).rc_of(h) == dup_next(s).obj.rc
                &&& !up ==> old(self).rc_of(h) == decref_next(s).obj.rc
            },
            s0.rc_of(h) == 0 ==> old(self).rc_of(h) == 0,
        ensures
            final(self).wf(),
            final(self).pages == s0.pages,
            final(self).checked == s0.checked,
            final(self).perms == old(self).perms,
            final(self).dom() == s0.dom(),
            forall|w: Word| #[trigger] final(self).word_ok(w) == s0.word_ok(w),
            ({
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                let dying = !up && s0.rc_of(h) != 0 && decref_next(s).freed;
                &&& final(self).state(h) == if dying {
                    BlockState::Dying
                } else {
                    BlockState::Live
                }
                &&& dying ==> final(self).edges_complete(h)
                &&& s0.quiet() && !dying ==> final(self).quiet()
            }),
            forall|b: usize| b != h ==> #[trigger] final(self).state(b) == s0.state(b),
            final(self).model@.edges == match edge {
                Some(e) => s0.model@.edges.remove(e),
                None => s0.model@.edges,
            },
            final(self).model@.building == s0.model@.building,
            final(self).evolved(
                s0,
                h,
                if up {
                    1int
                } else if edge is None {
                    -1int
                } else {
                    0int
                },
            ),
            up ==> final(self).bumped(s0, h),
    {
        self.count_model(Ghost(s0), Ghost(h), Ghost(up), Ghost(edge));
        proof {
            self.lemma_count_extra(s0, h, up, edge);
            if up && s0.rc_of(h) != 0 {
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                reveal(Core::refs_wf);
                assert(s0.counted_wf(h));
                lemma_dup(s);
            }
        }
    }

    proof fn lemma_count_extra(&self, s0: Core, h: usize, up: bool, edge: Option<Pos>)
        requires
            s0.wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            s0.model@.building is None,
            self.pages == s0.pages,
            self.checked == s0.checked,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(h).meta == s0.perm(h).meta,
            self.perm(h).slots == s0.perm(h).slots,
            up ==> edge is None,
            edge matches Some(e) ==> s0.model@.edges.dom().contains(e) && s0.model@.edges[e] == h
                && s0.state(e.0) == BlockState::Dying,
            s0.rc_of(h) != 0 && !up && edge is None ==> s0.ext(h) >= 1,
            s0.rc_of(h) == 0 ==> self.rc_of(h) == 0,
            ({
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                let dying = !up && s0.rc_of(h) != 0 && decref_next(s).freed;
                &&& s0.rc_of(h) != 0 && up ==> self.rc_of(h) == dup_next(s).obj.rc
                &&& s0.rc_of(h) != 0 && !up ==> self.rc_of(h) == decref_next(s).obj.rc
                &&& self.state(h) == if dying {
                    BlockState::Dying
                } else {
                    BlockState::Live
                }
                &&& s0.rc_of(h) != 0 && !dying ==> self.ext(h) == s0.ext(h) + if up {
                    1int
                } else if edge is None {
                    -1int
                } else {
                    0int
                }
            }),
            forall|b: usize| b != h ==> #[trigger] self.state(b) == s0.state(b),
            forall|b: usize| b != h ==> #[trigger] self.ext(b) == s0.ext(b),
            self.model@.owner == s0.model@.owner,
            self.model@.edges == match edge {
                Some(e) => s0.model@.edges.remove(e),
                None => s0.model@.edges,
            },
        ensures
            ({
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                let dying = !up && s0.rc_of(h) != 0 && decref_next(s).freed;
                &&& dying ==> self.edges_complete(h)
                &&& s0.quiet() && !dying ==> self.quiet()
            }),
            self.evolved(
                s0,
                h,
                if up {
                    1int
                } else if edge is None {
                    -1int
                } else {
                    0int
                },
            ),
    {
        let rc = s0.rc_of(h);
        let s = Obj { rc, count: s0.model@.refs[h], stuck: s0.model@.stuck[h] };
        let dying = !up && rc != 0 && decref_next(s).freed;
        let delta = if up {
            1int
        } else if edge is None {
            -1int
        } else {
            0int
        };
        reveal(Core::blocks_wf);
        reveal(Core::refs_wf);
        assert(s0.block_wf(h));
        assert(s0.counted_wf(h));
        if dying {
            lemma_decref(s);
            if let Some(e) = edge {
                lemma_indeg_pos(s0.model@.edges, e);
            }
            assert forall|i: int|
                #![trigger self.scanned(h, i)]
                self.scanned(h, i) && p_word(
                    self.perm(h),
                    i,
                ).is_ptr_spec() implies self.model@.edges.dom().contains((h, i)) by {
                assert(s0.scanned(h, i));
                assert(s0.slot_word(s0.perm(h), h, i));
                if let Some(e) = edge {
                    assert(e.0 != h);
                }
            }
        }
        if s0.quiet() && !dying {
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.state(b)
                != BlockState::Dying by {
                assert(s0.dom().contains(b));
            }
        }
        assert forall|b: usize|
            #![trigger s0.dom().contains(b)]
            s0.dom().contains(b) && s0.settled_state(b) == BlockState::Live && (s0.rc_of(b) == 0
                || s0.ext(b) + (if b == h {
                delta
            } else {
                0
            }) >= 1) implies {
            &&& self.dom().contains(b)
            &&& self.state(b) == BlockState::Live
            &&& self.perm(b).meta == s0.perm(b).meta
            &&& self.perm(b).slots == s0.perm(b).slots
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
            &&& (s0.rc_of(b) == 0) == (self.rc_of(b) == 0)
            &&& s0.rc_of(b) != 0 ==> self.ext(b) == s0.ext(b) + if b == h {
                delta
            } else {
                0
            }
            &&& self.model@.owner[b] == s0.model@.owner[b]
        } by {
            if b == h && rc != 0 {
                if up {
                    lemma_dup(s);
                } else {
                    lemma_decref(s);
                }
                if let Some(e) = edge {
                    lemma_indeg_pos(s0.model@.edges, e);
                }
            }
        }
    }

    /// `count_ghost` の本体。
    #[inline(always)]
    fn count_model(
        &mut self,
        Ghost(s0): Ghost<Core>,
        Ghost(h): Ghost<usize>,
        Ghost(up): Ghost<bool>,
        Ghost(edge): Ghost<Option<Pos>>,
    )
        requires
            s0.wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            s0.model@.building is None,
            old(self).rc_changed(s0, h),
            up ==> edge is None,
            edge matches Some(e) ==> s0.model@.edges.dom().contains(e) && s0.model@.edges[e] == h
                && s0.state(e.0) == BlockState::Dying,
            s0.rc_of(h) != 0 ==> {
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                &&& !up && edge is None ==> s0.ext(h) >= 1
                &&& up ==> old(self).rc_of(h) == dup_next(s).obj.rc
                &&& !up ==> old(self).rc_of(h) == decref_next(s).obj.rc
            },
            s0.rc_of(h) == 0 ==> old(self).rc_of(h) == 0,
        ensures
            final(self).wf(),
            final(self).pages == s0.pages,
            final(self).perms == old(self).perms,
            final(self).dom() == s0.dom(),
            forall|w: Word| #[trigger] final(self).word_ok(w) == s0.word_ok(w),
            ({
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                let dying = !up && s0.rc_of(h) != 0 && decref_next(s).freed;
                &&& final(self).state(h) == if dying {
                    BlockState::Dying
                } else {
                    BlockState::Live
                }
                &&& dying ==> final(self).indeg(h) == 0
                &&& s0.rc_of(h) != 0 && !dying ==> final(self).ext(h) == s0.ext(h) + if up {
                    1int
                } else if edge is None {
                    -1int
                } else {
                    0int
                }
            }),
            forall|b: usize| b != h ==> #[trigger] final(self).state(b) == s0.state(b),
            forall|b: usize| b != h ==> #[trigger] final(self).model@.refs[b] == s0.model@.refs[b],
            forall|b: usize|
                b != h && (edge matches Some(e) ==> b != s0.model@.edges[e])
                    ==> #[trigger] final(self).indeg(b) == s0.indeg(b),
            forall|b: usize| b != h ==> #[trigger] final(self).ext(b) == s0.ext(b),
            final(self).model@.written == s0.model@.written,
            final(self).model@.kept == s0.model@.kept,
            final(self).model@.refs.dom() == s0.model@.refs.dom(),
            up ==> final(self).model@.refs[h] == s0.model@.refs[h] + if s0.rc_of(h) != 0 {
                1nat
            } else {
                0nat
            },
            up ==> final(self).model@.state == s0.model@.state,
            final(self).model@.owner == s0.model@.owner,
            final(self).model@.building == s0.model@.building,
            final(self).checked == s0.checked,
            final(self).model@.edges == match edge {
                Some(e) => s0.model@.edges.remove(e),
                None => s0.model@.edges,
            },
    {
        proof {
            let s1 = match edge {
                Some(e) => Core {
                    model: Ghost(Model { edges: s0.model@.edges.remove(e), ..s0.model@ }),
                    ..s0
                },
                None => s0,
            };
            if let Some(e) = edge {
                s1.lemma_edge_removed(s0, e);
            }
            let rc = s0.rc_of(h);
            let s = Obj { rc, count: s0.model@.refs[h], stuck: s0.model@.stuck[h] };
            reveal(Core::refs_wf);
            reveal(Core::blocks_wf);
            assert(s0.counted_wf(h));
            let (count, stuck, dying) = if rc == 0 {
                (s.count, s.stuck, false)
            } else if up {
                lemma_dup(s);
                (dup_next(s).obj.count, dup_next(s).obj.stuck, false)
            } else {
                lemma_decref(s);
                (decref_next(s).obj.count, decref_next(s).obj.stuck, decref_next(s).freed)
            };
            self.model = Ghost(
                Model {
                    state: s1.model@.state.insert(
                        h,
                        if dying {
                            BlockState::Dying
                        } else {
                            BlockState::Live
                        },
                    ),
                    refs: s1.model@.refs.insert(h, count),
                    stuck: s1.model@.stuck.insert(h, stuck),
                    ..s1.model@
                },
            );
            assert(self.model@.refs.dom() =~= s1.model@.refs.dom());
            assert(self.model@.stuck.dom() =~= s1.model@.stuck.dom());
            if up {
                assert(self.model@.state =~= s0.model@.state);
            }
            self.lemma_count_step(s1, h, dying);
        }
    }
}

} // verus!

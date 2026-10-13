//! ブロックの状態の遷移の ghost の手順と補題。最後の参照を手放すと決めた物体を空ける途中にして残すフィールドの辺を
//! 外の参照に移す手順と、token の箱を作る、使い回す、空ける手順を置く。`token_build` のほかは、実行の時には何もしない。

use vstd::prelude::*;
use vstd::raw_ptr::ptr_mut_write;

use super::Core;
#[cfg(verus_only)]
use super::check::{lemma_code, lemma_set_code};
#[cfg(verus_only)]
use super::inv::{p_code, p_count, p_first, p_init, p_kind, p_owner, p_scanned};
use super::model::Pos;
#[cfg(verus_only)]
use super::model::{BlockState, Model, indeg, lemma_indeg_union, lemma_indeg_zero};
use crate::meta::{Header, Meta, meta_ptr};
#[cfg(verus_only)]
use crate::meta::{Kind, bits_kind, bits_scan, bits_wf};

verus! {

/// ブロック `h` の、`s` の語の位置。
pub(crate) open spec fn pos_set(h: usize, s: Set<int>) -> Set<Pos> {
    s.map(|j: int| (h, j))
}

pub(crate) proof fn lemma_pos_set(h: usize, s: Set<int>)
    ensures
        forall|e: Pos| #[trigger] pos_set(h, s).contains(e) <==> e.0 == h && s.contains(e.1),
{
    assert forall|e: Pos| #[trigger] pos_set(h, s).contains(e) <==> e.0 == h && s.contains(e.1) by {
        s.lemma_map_contains(|j: int| (h, j), e);
        if e.0 == h && s.contains(e.1) {
            assert(e == (|j: int| (h, j))(e.1));
        }
    }
}

impl Core {
    /// 最後の参照を手放すと決めた生きている物体 `h` を空ける途中にし、`ps` の位置の辺を外す。外した辺の数え分は、
    /// 行き先の外の参照に移る (残すフィールドを呼ぶ側が受け取る)。検査モードでは前置きも「空ける途中」にする。同じ物体に
    /// 2回届いたときに、2回目を生きていない物体として断るためである (docs/spec/object-model.md の「報告する誤り」)。
    #[inline(always)]
    pub(crate) fn begin_free(&mut self, h: *mut Header, Ghost(ps): Ghost<Set<Pos>>, checked: bool)
        requires
            old(self).wf(),
            checked == old(self).checked@,
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            old(self).state(h@.addr) == BlockState::Live,
            old(self).indeg(h@.addr) == 0,
            old(self).model@.building is None,
            forall|e: Pos| #[trigger] ps.contains(e) ==> e.0 == h@.addr,
        ensures
            final(self).wf(),
            final(self).pages == old(self).pages,
            final(self).prefix_moved(*old(self), h@.addr),
            final(self).checked == old(self).checked,
            final(self).model@ == (Model {
                state: old(self).model@.state.insert(h@.addr, BlockState::Dying),
                edges: old(self).model@.edges.remove_keys(ps),
                ..old(self).model@
            }),
            forall|t: usize| #[trigger]
                final(self).indeg(t) + indeg(old(self).model@.edges.restrict(ps), t) == old(
                    self,
                ).indeg(t),
    {
        let ghost s0 = *self;
        proof {
            self.lemma_at(h@.addr);
        }
        if checked {
            let v = self.prefix_state_get(h);
            proof {
                lemma_set_code(v, 0);
            }
            self.prefix_state_put(h, v & !3u32);
        }
        proof {
            let a = h@.addr;
            self.model = Ghost(
                Model {
                    state: s0.model@.state.insert(a, BlockState::Dying),
                    edges: s0.model@.edges.remove_keys(ps),
                    ..s0.model@
                },
            );
            assert(self.perms@.dom() =~= s0.perms@.dom());
            self.lemma_begin_free(s0, a, ps);
        }
    }

    proof fn lemma_begin_free(&self, s0: Core, h: usize, ps: Set<Pos>)
        requires
            s0.wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            s0.indeg(h) == 0,
            s0.model@.building is None,
            forall|e: Pos| #[trigger] ps.contains(e) ==> e.0 == h,
            self.pages == s0.pages,
            self.prefix_moved(s0, h),
            self.perm(h).prefix is Some ==> p_code(self.perm(h)) == 0,
            self.checked == s0.checked,
            self.model@ == (Model {
                state: s0.model@.state.insert(h, BlockState::Dying),
                edges: s0.model@.edges.remove_keys(ps),
                ..s0.model@
            }),
        ensures
            self.wf(),
            forall|t: usize| #[trigger]
                self.indeg(t) + indeg(s0.model@.edges.restrict(ps), t) == s0.indeg(t),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        let rest = s0.model@.edges.remove_keys(ps);
        let gone = s0.model@.edges.restrict(ps);
        assert(rest.union_prefer_right(gone) =~= s0.model@.edges);
        assert(rest.dom().disjoint(gone.dom()));
        assert forall|t: usize| #[trigger] self.indeg(t) + indeg(gone, t) == s0.indeg(t) by {
            lemma_indeg_union(rest, gone, t);
        }
        assert(self.model@.state.dom() =~= self.dom());
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            let p = self.perm(b);
            if b == h {
                let q = s0.perm(h);
                assert forall|i: int| #[trigger] p_scanned(p, i) implies p_init(p, i) by {
                    assert(p_scanned(q, i));
                    assert(s0.slot_word(q, h, i));
                }
            } else {
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
                    assert(s0.model@.building == Some(b));
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
            lemma_indeg_zero(s0.model@.edges, h, e);
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 子を手放し終えた空ける途中の物体 `h` を token の箱にする。`ks` の語は「残した」語になる。
    #[inline(always)]
    pub(crate) fn finish_token(&mut self, h: *mut Header, Ghost(ks): Ghost<Set<int>>, checked: bool)
        requires
            old(self).wf(),
            checked == old(self).checked@,
            h == old(self).block(h@.addr),
            old(self).dom().contains(h@.addr),
            old(self).state(h@.addr) == BlockState::Dying,
            forall|j: int| !(#[trigger] old(self).model@.edges.dom().contains((h@.addr, j))),
            old(self).kind(h@.addr) == Kind::Data,
            old(self).rc_of(h@.addr) == 1,
            forall|j: int| #[trigger] ks.contains(j) ==> old(self).scanned(h@.addr, j),
        ensures
            final(self).wf(),
            final(self).pages == old(self).pages,
            final(self).prefix_moved(*old(self), h@.addr),
            final(self).checked == old(self).checked,
            final(self).model@ == (Model {
                state: old(self).model@.state.insert(h@.addr, BlockState::Token),
                kept: old(self).model@.kept + pos_set(h@.addr, ks),
                ..old(self).model@
            }),
    {
        let ghost s0 = *self;
        proof {
            self.lemma_at(h@.addr);
        }
        // 世代の番号を進める。使い終えた token の表の項目を、箱がまた token になった後も断るためである
        if checked {
            let v = self.prefix_state_get(h);
            let g = (v >> 2u32).wrapping_add(1);
            proof {
                lemma_code(g, 2);
            }
            self.prefix_state_put(h, (g << 2u32) | 2);
        }
        proof {
            assert(self.perms@.dom() =~= s0.perms@.dom());
            self.model = Ghost(
                Model {
                    state: s0.model@.state.insert(h@.addr, BlockState::Token),
                    kept: s0.model@.kept + pos_set(h@.addr, ks),
                    ..s0.model@
                },
            );
            reveal(Core::blocks_wf);
            reveal(Core::edges_wf);
            reveal(Core::refs_wf);
            reveal(Core::building_wf);
            lemma_pos_set(h@.addr, ks);
            assert(self.model@.state.dom() =~= self.dom());
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
                assert(s0.block_wf(b));
                let p = self.perm(b);
                if b != h@.addr {
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
                            && self.slot_word(p, b, i) && !self.model@.kept.contains((b, i)) by {
                            assert(s0.slot_word(p, b, i));
                        }
                    }
                } else {
                    let q = s0.perm(h@.addr);
                    assert forall|i: int| #[trigger] p_scanned(p, i) implies p_init(p, i) by {
                        assert(p_scanned(q, i));
                    }
                }
            }
            assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(
                e,
            ) by {
                if s0.model@.kept.contains(e) {
                    assert(s0.kept_wf(e));
                    if e.0 == h@.addr {
                        assert(s0.state(h@.addr) == BlockState::Dying);
                    }
                } else {
                    assert(s0.block_wf(h@.addr));
                    assert(p_scanned(s0.perm(h@.addr), e.1));
                }
            }
            assert forall|e: Pos| #[trigger]
                self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
                assert(s0.edge_wf(e));
            }
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(
                b,
            ) by {
                assert(s0.counted_wf(b));
            }
        }
    }

    /// 使わなかった token の箱 `h` を、空ける途中の物体にする。「残した」語は、もう呼ぶ側のものである。
    #[inline(always)]
    pub(crate) fn token_free(&mut self, h: *mut Header, checked: bool)
        requires
            old(self).wf(),
            checked == old(self).checked@,
            h == old(self).block(h@.addr),
            old(self).dom().contains(h@.addr),
            old(self).state(h@.addr) == BlockState::Token,
        ensures
            final(self).wf(),
            final(self).pages == old(self).pages,
            final(self).prefix_moved(*old(self), h@.addr),
            final(self).checked == old(self).checked,
            final(self).state(h@.addr) == BlockState::Dying,
            forall|b: usize| b != h@.addr ==> #[trigger] final(self).state(b) == old(self).state(b),
            final(self).dom() == old(self).dom(),
            final(self).model@.edges == old(self).model@.edges,
            final(self).model@.building == old(self).model@.building,
            final(self).model@.refs == old(self).model@.refs,
            final(self).model@.owner == old(self).model@.owner,
            forall|j: int| !(#[trigger] final(self).model@.edges.dom().contains((h@.addr, j))),
    {
        let ghost s0 = *self;
        proof {
            self.lemma_at(h@.addr);
        }
        if checked {
            let v = self.prefix_state_get(h);
            proof {
                lemma_set_code(v, 0);
            }
            self.prefix_state_put(h, v & !3u32);
        }
        proof {
            assert(self.perms@.dom() =~= s0.perms@.dom());
            self.model = Ghost(
                Model {
                    state: s0.model@.state.insert(h@.addr, BlockState::Dying),
                    kept: s0.model@.kept.filter(|e: Pos| e.0 != h@.addr),
                    ..s0.model@
                },
            );
            reveal(Core::blocks_wf);
            reveal(Core::edges_wf);
            reveal(Core::refs_wf);
            reveal(Core::building_wf);
            assert(self.model@.state.dom() =~= self.dom());
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
                assert(s0.block_wf(b));
                let p = self.perm(b);
                if b != h@.addr {
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
                            && self.slot_word(p, b, i) && !self.model@.kept.contains((b, i)) by {
                            assert(s0.slot_word(p, b, i));
                        }
                    }
                } else {
                    let q = s0.perm(h@.addr);
                    assert forall|i: int| #[trigger] p_scanned(p, i) implies p_init(p, i) by {
                        assert(p_scanned(q, i));
                    }
                }
            }
            assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(
                e,
            ) by {
                assert(s0.kept_wf(e));
                if e.0 != h@.addr {
                    assert(self.perms@[e.0] == s0.perms@[e.0]);
                }
            }
            assert forall|e: Pos| #[trigger]
                self.model@.edges.dom().contains(e) implies self.edge_wf(e) && e.0 != h@.addr by {
                assert(s0.edge_wf(e));
            }
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(
                b,
            ) by {
                assert(s0.counted_wf(b));
            }
        }
    }

    /// token の箱 `h` を構築中の物体にする。`meta` があればヘッダに書く。前の物体の「残した」語のうち、新しい `meta` の
    /// 走査する語の位置にあるものだけが「残した」のまま残る。
    #[inline(always)]
    pub(crate) fn token_build(
        &mut self,
        h: *mut Header,
        meta: Option<Meta>,
        owner: u32,
        checked: bool,
    )
        requires
            old(self).wf(),
            checked == old(self).checked@,
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            old(self).state(h@.addr) == BlockState::Token,
            old(self).model@.building is None,
            meta matches Some(m) ==> {
                &&& m.kind_spec() != Kind::Free
                &&& m.scan_spec() != 255
                &&& m.scan_spec() <= old(self).cap(h@.addr)
            },
        ensures
            final(self).wf(),
            final(self).pages == old(self).pages,
            final(self).checked == old(self).checked,
            final(self).dom() == old(self).dom(),
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).perms@[b] == old(self).perms@[b],
            final(self).perm(h@.addr).rc == old(self).perm(h@.addr).rc,
            final(self).perm(h@.addr).slots == old(self).perm(h@.addr).slots,
            final(self).meta_bits(h@.addr) == match meta {
                Some(m) => m.bits_spec(),
                None => old(self).meta_bits(h@.addr),
            },
            final(self).state(h@.addr) == BlockState::Building,
            forall|b: usize| b != h@.addr ==> #[trigger] final(self).state(b) == old(self).state(b),
            final(self).model@.building == Some(h@.addr),
            final(self).model@.written[h@.addr] == Set::<int>::empty(),
            final(self).model@.edges == old(self).model@.edges,
            final(self).model@.kept == old(self).model@.kept.filter(
                |e: Pos| e.0 != h@.addr || final(self).scanned(h@.addr, e.1),
            ),
            final(self).model@.refs == old(self).model@.refs.insert(h@.addr, 1),
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).model@.owner[b] == old(
                    self,
                ).model@.owner[b],
            final(self).rc_of(h@.addr) == 1,
            final(self).indeg(h@.addr) == 0,
    {
        let ghost s0 = *self;
        proof {
            self.lemma_at(h@.addr);
        }
        let v = if checked {
            self.prefix_state_get(h)
        } else {
            0
        };
        if let Some(m) = meta {
            let bits = m.bits();
            let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
            ptr_mut_write(meta_ptr(h), Tracked(&mut p.meta), bits);
            proof {
                self.perms.borrow_mut().tracked_insert(h@.addr, p);
                assert(self.perms@.dom() =~= s0.perms@.dom());
            }
        }
        // 使い回した箱は、今の worker の新しい物体である

        if checked {
            proof {
                lemma_set_code(v, 1);
            }
            self.prefix_state_put(h, (v & !3u32) | 1);
            self.prefix_first_put(h, owner);
        }
        proof {
            let s1 = *self;
            let a = h@.addr;
            self.model = Ghost(
                Model {
                    state: s0.model@.state.insert(a, BlockState::Building),
                    refs: s0.model@.refs.insert(a, 1),
                    stuck: s0.model@.stuck.insert(a, false),
                    owner: s0.model@.owner.insert(a, owner),
                    written: s0.model@.written.insert(a, Set::empty()),
                    kept: s0.model@.kept.filter(|e: Pos| e.0 != a || p_scanned(s1.perm(a), e.1)),
                    building: Some(a),
                    ..s0.model@
                },
            );
            reveal(Core::blocks_wf);
            assert(self.model@.owner.dom() =~= s0.model@.owner.dom());
            self.lemma_token_build(s0, a, meta);
            assert(self.model@.kept =~= s0.model@.kept.filter(
                |e: Pos| e.0 != a || self.scanned(a, e.1),
            ));
        }
    }

    /// `begin_free` と `finish_token` で `a` を token の箱にした後。ほかの物体は変わらない。
    pub(crate) proof fn lemma_after_token(&self, s0: Core, s1: Core, a: usize)
        requires
            s1.dom().contains(a),
            s1.state(a) == BlockState::Dying,
            s1.model@.building is None,
            forall|b: usize| b != a ==> #[trigger] s1.state(b) == s0.state(b),
            s1.dom() == s0.dom(),
            self.prefix_moved(s1, a),
            self.pages == s1.pages,
            self.checked == s1.checked,
            self.model@.state == s1.model@.state.insert(a, BlockState::Token),
            self.model@.refs == s1.model@.refs,
            self.model@.edges == s1.model@.edges,
            self.model@.owner == s1.model@.owner,
            self.model@.building == s1.model@.building,
        ensures
            self.evolved(s1, 0, 0),
            s0.quiet() ==> self.quiet(),
    {
        if s0.quiet() {
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.state(b)
                != BlockState::Dying by {
                if b != a {
                    assert(s0.dom().contains(b));
                }
            }
        }
    }

    /// 使わなかった token の箱 `a` を空けた後。ほかの物体は変わらない。
    pub(crate) proof fn lemma_token_dropped(&self, s1: Core, s2: Core, a: usize)
        requires
            s1.dom().contains(a),
            s1.state(a) == BlockState::Token,
            s1.model@.building is None,
            s2.state(a) == BlockState::Dying,
            forall|b: usize| b != a ==> #[trigger] s2.state(b) == s1.state(b),
            s2.dom() == s1.dom(),
            s2.prefix_moved(s1, a),
            s2.pages == s1.pages,
            s2.checked == s1.checked,
            s2.model@.edges == s1.model@.edges,
            s2.model@.refs == s1.model@.refs,
            s2.model@.owner == s1.model@.owner,
            s2.model@.building == s1.model@.building,
            self.boxed_off(s2, a),
        ensures
            self.evolved(s1, 0, 0),
            s1.quiet() ==> self.quiet(),
    {
        assert(s2.evolved(s1, 0, 0));
        self.lemma_boxed_evolved(s2, a);
        self.lemma_evolved_trans(s2, s1, 0, 0, 0);
        if s1.quiet() {
            assert forall|b: usize| #[trigger] s2.dom().contains(b) && b != a implies s2.state(b)
                != BlockState::Dying by {
                assert(s1.dom().contains(b));
            }
        }
    }

    proof fn lemma_token_build(&self, s0: Core, a: usize, meta: Option<Meta>)
        requires
            s0.wf(),
            s0.dom().contains(a),
            s0.state(a) == BlockState::Token,
            s0.model@.building is None,
            self.pages == s0.pages,
            self.checked == s0.checked,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != a ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(a).rc == s0.perm(a).rc,
            self.perm(a).slots == s0.perm(a).slots,
            (self.perm(a).prefix is Some) == (s0.perm(a).prefix is Some),
            self.perm(a).prefix matches Some(q) ==> {
                &&& q.first.ptr() == s0.perm(a).prefix->0.first.ptr()
                &&& q.state.ptr() == s0.perm(a).prefix->0.state.ptr()
                &&& q.first.is_init()
                &&& q.state.is_init()
                &&& p_code(self.perm(a)) == 1
                &&& p_owner(self.perm(a)) == self.model@.owner[a]
            },
            self.perm(a).meta.ptr() == s0.perm(a).meta.ptr(),
            self.perm(a).meta.is_init(),
            match meta {
                Some(m) => {
                    &&& self.meta_bits(a) == m.bits_spec()
                    &&& bits_wf(m.bits_spec())
                    &&& bits_kind(m.bits_spec()) != Kind::Free
                    &&& bits_scan(m.bits_spec()) != 255
                    &&& bits_scan(m.bits_spec()) <= s0.cap(a)
                },
                None => self.perm(a).meta == s0.perm(a).meta,
            },
            self.model@ == (Model {
                state: s0.model@.state.insert(a, BlockState::Building),
                refs: s0.model@.refs.insert(a, 1),
                stuck: s0.model@.stuck.insert(a, false),
                owner: self.model@.owner,
                written: s0.model@.written.insert(a, Set::empty()),
                kept: s0.model@.kept.filter(|e: Pos| e.0 != a || p_scanned(self.perm(a), e.1)),
                building: Some(a),
                ..s0.model@
            }),
            self.model@.owner.dom() == s0.model@.owner.dom(),
            forall|b: usize| b != a ==> #[trigger] self.model@.owner[b] == s0.model@.owner[b],
        ensures
            self.wf(),
            self.indeg(a) == 0,
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(s0.block_wf(a));
        assert(s0.counted_wf(a));
        assert(self.model@.state.dom() =~= self.dom());
        assert(self.model@.refs.dom() =~= self.dom());
        assert(self.model@.stuck.dom() =~= self.dom());
        assert(self.model@.written.dom() =~= self.dom());
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            let p = self.perm(b);
            if b != a {
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
                    assert(s0.model@.building == Some(b));
                }
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(e) by {
            assert(s0.kept_wf(e));
            if e.0 != a {
                assert(self.perms@[e.0] == s0.perms@[e.0]);
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.edges.dom().contains(e) implies self.edge_wf(
            e,
        ) by {
            assert(s0.edge_wf(e));
            assert(e.0 != a);
            assert(self.perms@[e.0] == s0.perms@[e.0]);
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }
}

} // verus!

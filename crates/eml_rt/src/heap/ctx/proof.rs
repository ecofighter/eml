//! `Ctx` の基本操作の証明の部品。ヒープの変化を `moved` で表してつなぎ、`Ctx` の持つ語の数と `evv` のリストが
//! 変化の後も保たれることを示す。

use vstd::prelude::*;

use super::super::Core;
use super::super::Heap;
use super::super::field::field_edges;
use super::super::inv::{p_extended, p_init, p_word};
use super::super::model::{BlockState, indeg, lemma_indeg_insert, lemma_indeg_remove};
use super::*;
use crate::meta::{Kind, bits_kind, bits_lin, bits_scan, bits_tag, bits_wf};
use crate::mode::Mode;
use crate::word::{Word, lemma_unit_bits};

verus! {

impl<M: Mode> Ctx<M> {
    /// `()` だけのリストは、正しい空のリストである。
    pub(super) proof fn lemma_unit_listed(&self, w: Word)
        requires
            w.bits_spec() == 1,
        ensures
            listed(self.heap, w, self.max_evv),
            seq_of(self.heap, w) == Seq::<(u16, Word)>::empty(),
    {
        assert(path(self.heap, w, 0) is Some);
        lemma_seq_of(self.heap, w, 0);
    }

    /// 検査モードの操作は語を `word_in` のまま保つので、`Ctx` の持つ語も `word_in` のままである。
    pub(super) proof fn lemma_words_kept(&self, old: Ctx<M>)
        requires
            self.heap.words_kept(old.heap),
            old.heap.word_in(old.evv),
            old.heap.word_in(old.yielding.target),
            old.heap.word_in(old.yielding.clause),
            old.heap.word_in(old.yielding.cont),
            old.heap.word_in(self.evv),
            self.yielding == old.yielding || (self.yielding.target.bits_spec() == 1
                && self.yielding.clause.bits_spec() == 1 && self.yielding.cont.bits_spec() == 1),
        ensures
            self.heap.word_in(self.evv),
            self.heap.word_in(self.yielding.target),
            self.heap.word_in(self.yielding.clause),
            self.heap.word_in(self.yielding.cont),
    {
        lemma_unit_bits();
    }

    /// `Ctx` が `ws` と合わせて持つ語 `w` は、`dup` も `decref` もできる。
    pub(super) proof fn lemma_held_dup_ok(&self, w: Word, ws: Seq<Word>)
        requires
            self.held_ok() || (self.heap.consistent() && self.heap.settleable()),
            self.holds_args(ws),
            w == self.evv || w == self.yielding.target || w == self.yielding.clause || w
                == self.yielding.cont,
            w == self.evv ==> self.evv_wf(w),
        ensures
            self.heap.decref_ok(w),
            w == self.evv ==> self.heap.dup_ok(w),
    {
        lemma_points_at(ws, 0, w.bits_spec());
        assert(self.need(ws, w.bits_spec()) >= points(w, w.bits_spec()));
        if w == self.evv {
            lemma_listed_head(self.heap, w, self.max_evv);
        }
    }

    /// 文脈 `self` は、`old` からヒープを `moved(d, x)` で変え、語を替えたものである。新しい語はどれも `old` で
    /// 持たれていた語で、物体ごとの語の数は、参照の数の変化 `d` を足した `old` の数を超えない。
    pub(super) proof fn lemma_holds_moved(
        &self,
        old: Ctx<M>,
        ws0: Seq<Word>,
        ws1: Seq<Word>,
        d: spec_fn(usize) -> int,
        x: Set<usize>,
    )
        requires
            old.holds_args(ws0),
            old.heap.consistent(),
            old.heap.settleable(),
            moved(self.heap, old.heap, d, x),
            held_word(old.heap, self.evv),
            held_word(old.heap, self.yielding.target),
            held_word(old.heap, self.yielding.clause),
            held_word(old.heap, self.yielding.cont),
            forall|i: int| 0 <= i < ws1.len() ==> held_word(old.heap, #[trigger] ws1[i]),
            forall|b: usize| #[trigger]
                self.need(ws1, b) > 0 ==> old.need(ws0, b) + d(b) >= self.need(ws1, b),
        ensures
            self.holds_args(ws1),
    {
        let h0 = old.heap;
        let h1 = self.heap;
        assert forall|w: Word|
            held_word(h0, w) && (w.is_ptr_spec() ==> self.need(ws1, w.bits_spec())
                >= 1) implies #[trigger] held_word(h1, w) by {
            self.lemma_word_moved(old, ws0, ws1, d, x, w);
        }
        assert forall|i: int| 0 <= i < ws1.len() implies held_word(h1, #[trigger] ws1[i]) by {
            lemma_points_at(ws1, i, ws1[i].bits_spec());
            lemma_points_at(ws1, 0, ws1[i].bits_spec());
        }
        assert(held_word(h1, self.evv)) by {
            lemma_points_at(ws1, 0, self.evv.bits_spec());
        }
        assert(held_word(h1, self.yielding.target)) by {
            lemma_points_at(ws1, 0, self.yielding.target.bits_spec());
        }
        assert(held_word(h1, self.yielding.clause)) by {
            lemma_points_at(ws1, 0, self.yielding.clause.bits_spec());
        }
        assert(held_word(h1, self.yielding.cont)) by {
            lemma_points_at(ws1, 0, self.yielding.cont.bits_spec());
        }
        let v0 = h0.core.settled_view();
        let v1 = h1.core.settled_view();
        assert forall|b: usize| #[trigger] self.need(ws1, b) > 0 && v1.rc_of(b) != 0 implies v1.ext(
            b,
        ) >= self.need(ws1, b) by {
            let w = self.word_at_need(ws1, b);
            self.lemma_word_moved(old, ws0, ws1, d, x, w);
            assert(live(h0, b) && live(h1, b));
            if v0.rc_of(b) == 0 {
                assert(same_head(h1, h0, b));
            } else {
                if old.need(ws0, b) <= 0 {
                    lemma_ext_nonneg(h0, b);
                }
            }
        }
    }

    /// `ws1` と `Ctx` の語のうち、物体 `b` を指す1つ。
    pub(super) proof fn word_at_need(&self, ws1: Seq<Word>, b: usize) -> (w: Word)
        requires
            self.need(ws1, b) > 0,
        ensures
            w == self.evv || w == self.yielding.target || w == self.yielding.clause || w
                == self.yielding.cont || exists|i: int| 0 <= i < ws1.len() && ws1[i] == w,
            w.is_ptr_spec(),
            w.bits_spec() == b,
    {
        if points(self.evv, b) > 0 {
            self.evv
        } else if points(self.yielding.target, b) > 0 {
            self.yielding.target
        } else if points(self.yielding.clause, b) > 0 {
            self.yielding.clause
        } else if points(self.yielding.cont, b) > 0 {
            self.yielding.cont
        } else {
            let i = lemma_points_witness(ws1, b);
            ws1[i]
        }
    }

    /// `lemma_holds_moved` の1語分。`old` で持たれていて、新しい文脈でも数に入る語は、新しいヒープで持たれている。
    pub(super) proof fn lemma_word_moved(
        &self,
        old: Ctx<M>,
        ws0: Seq<Word>,
        ws1: Seq<Word>,
        d: spec_fn(usize) -> int,
        x: Set<usize>,
        w: Word,
    )
        requires
            old.holds_args(ws0),
            old.heap.consistent(),
            old.heap.settleable(),
            moved(self.heap, old.heap, d, x),
            held_word(old.heap, w),
            w.is_ptr_spec() ==> self.need(ws1, w.bits_spec()) >= 1,
            forall|b: usize| #[trigger]
                self.need(ws1, b) > 0 ==> old.need(ws0, b) + d(b) >= self.need(ws1, b),
        ensures
            held_word(self.heap, w),
            w.is_ptr_spec() ==> live(old.heap, w.bits_spec()) && live(self.heap, w.bits_spec()),
    {
        if w.is_ptr_spec() {
            let h0 = old.heap;
            let h1 = self.heap;
            let b = w.bits_spec();
            let v0 = h0.core.settled_view();
            assert(live(h0, b));
            if v0.rc_of(b) != 0 {
                if old.need(ws0, b) <= 0 {
                    lemma_ext_nonneg(h0, b);
                }
                assert(v0.ext(b) + d(b) >= 1);
            }
            assert(live(h1, b));
            assert(same_head(h1, h0, b));
        }
    }
}

impl<M: Mode> Ctx<M> {
    /// `ev_new` の、モデルが合っている道。
    pub(super) proof fn lemma_ev_new_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        h3: Heap<M>,
        h4: Heap<M>,
        table: Word,
        ev: Word,
        effect: u16,
        n_ops: usize,
        state: Word,
        outer: Word,
    )
        requires
            s0.held_ok(),
            s0.evv_wf(outer),
            s0.holds_args(seq![state, outer]),
            effect < u16::MAX,
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
            h1.made(s0.heap, table),
            h1.kind_of(table) == Kind::Data,
            h1.count_of(table) == n_ops,
            h1.tag_of(table) == 0,
            h1.lin_of(table) == false,
            h1.owned(table),
            forall|i: int|
                0 <= i < n_ops ==> (#[trigger] h1.scanned_word(table, i)).bits_spec() == 1,
            h2.made(h1, ev),
            h2.kind_of(ev) == Kind::Data,
            h2.count_of(ev) == 3,
            h2.scan_of(ev) == 3,
            h2.tag_of(ev) == effect + 1,
            h2.lin_of(ev) == false,
            h2.owned(ev),
            forall|i: int| 0 <= i < 3 ==> (#[trigger] h2.scanned_word(ev, i)).bits_spec() == 1,
            h3.field_set(h2, ev, 0, table),
            h4.field_set(h3, ev, 1, state),
            self.heap.field_set(h4, ev, 2, outer),
            h3.consistent(),
            h4.consistent(),
            self.heap.consistent(),
        ensures
            self.held_ok(),
            self.holds(ev),
            self.ev_shape(ev),
            self.ev_tag(ev) == effect + 1,
            self.ev_state(ev) == state,
            self.ev_outer(ev) == outer,
            self.ev_ops(ev) == n_ops,
            forall|i: int| 0 <= i < n_ops ==> (#[trigger] self.ev_entry(ev, i)).bits_spec() == 1,
            self.evolved_by(s0, |b: usize| -points(state, b) - points(outer, b), Set::empty()),
    {
        let h0 = s0.heap;
        let h5 = self.heap;
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let x = none.insert(ev.bits_spec());
        let d4 = |b: usize| -points(state, b);
        let d = |b: usize| -points(state, b) - points(outer, b);
        // 古い物体についての変化
        lemma_alloc_moved(h1, h0, table);
        lemma_alloc_moved(h2, h1, ev);
        lemma_moved_chain(h2, h1, h0, z, z, z, none, none, none);
        lemma_unextended(h2, ev);
        assert(h2.scanned_word(ev, 0).bits_spec() == 1);
        assert(h2.scanned_word(ev, 1).bits_spec() == 1);
        assert(h2.scanned_word(ev, 2).bits_spec() == 1);
        lemma_set_unit_moved(h3, h2, ev, 0, table);
        lemma_moved_chain(h3, h2, h0, z, |b: usize| -points(table, b), z, none, x, none);
        lemma_unextended(h3, ev);
        assert(h3.word_at(ev, 1) == h2.word_at(ev, 1));
        assert(h3.word_at(ev, 2) == h2.word_at(ev, 2));
        lemma_set_unit_moved(h4, h3, ev, 1, state);
        lemma_moved_chain(h4, h3, h0, z, d4, d4, none, x, none);
        lemma_unextended(h4, ev);
        assert(h4.word_at(ev, 2) == h3.word_at(ev, 2));
        lemma_set_unit_moved(h5, h4, ev, 2, outer);
        lemma_moved_chain(h5, h4, h0, d4, |b: usize| -points(outer, b), d, none, x, none);
        // 新しい evidence についての変化
        let e3 = |b: usize| -points(table, b);
        let e4 = |b: usize| -points(table, b) - points(state, b);
        let e5 = |b: usize| -points(table, b) - points(state, b) - points(outer, b);
        lemma_moved_chain(h4, h3, h2, e3, d4, e4, x, x, x);
        lemma_moved_chain(h5, h4, h2, e4, |b: usize| -points(outer, b), e5, x, x, x);
        let eb = ev.bits_spec();
        assert(live(h2, eb));
        assert(!live(h1, eb));
        assert(live(h1, table.bits_spec()));
        assert(e5(eb) == 0);
        assert(live(h5, eb));
        lemma_unextended(h5, ev);
        assert(h5.scanned_word(ev, 0) == table);
        assert(h5.scanned_word(ev, 1) == state);
        assert(h5.scanned_word(ev, 2) == outer);
        // Ctx の語と引数
        let ws = seq![state, outer];
        assert(held_word(h0, ws[0]) && held_word(h0, ws[1]));
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b) + d(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, none);
        self.lemma_evv_kept(s0, d, none);
        // 新しい evidence の形
        h5.lemma_readable_scanned(ev, 0);
        h5.lemma_readable_scanned(ev, 1);
        h5.lemma_readable_scanned(ev, 2);
        assert(live(h2, table.bits_spec()));
        assert(live(h5, table.bits_spec()));
        assert(same_head(h5, h2, table.bits_spec()));
        assert(h5.core.perm(table.bits_spec()).slots == h2.core.perm(table.bits_spec()).slots);
        assert(same_head(h2, h1, table.bits_spec()));
        assert(h2.core.perm(table.bits_spec()).slots == h1.core.perm(table.bits_spec()).slots);
        assert forall|i: int| 0 <= i < n_ops implies (#[trigger] h5.scanned_word(
            table,
            i,
        )).bits_spec() == 1 by {
            assert(h1.scanned_word(table, i).bits_spec() == 1);
        }
        lemma_unit_bits();
        lemma_listed_head(h0, outer, s0.max_evv);
        lemma_word_kept(h5, h0, d, none, state);
        lemma_word_kept(h5, h0, d, none, outer);
        assert(ev_ok(h5, ev));
        // 呼ぶ側が新しい evidence の参照を1つ持つ
        self.lemma_fresh_holds(s0, ev);
    }
}

impl<M: Mode> Ctx<M> {
    /// `evv_push` の、モデルが合っている道。
    pub(super) proof fn lemma_evv_push_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        node: Word,
        l: Word,
        e: Word,
    )
        requires
            s0.held_ok(),
            s0.evv_wf(l),
            s0.evv_seq(l).len() < s0.max_evv,
            s0.ev_shape(e),
            s0.holds_args(seq![l, e]),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
            h1.made(s0.heap, node),
            h1.kind_of(node) == Kind::Data,
            h1.scan_of(node) == 2,
            h1.tag_of(node) == s0.heap.tag_of(e),
            h1.lin_of(node) == false,
            h1.owned(node),
            forall|i: int| 0 <= i < 2 ==> (#[trigger] h1.scanned_word(node, i)).bits_spec() == 1,
            h2.field_set(h1, node, 0, e),
            self.heap.field_set(h2, node, 1, l),
            h2.consistent(),
            self.heap.consistent(),
        ensures
            self.held_ok(),
            self.holds(node),
            self.evv_wf(node),
            self.evv_seq(node) == seq![(s0.ev_tag(e), e)] + s0.evv_seq(l),
            self.evolved_by(s0, |b: usize| -points(l, b) - points(e, b), Set::empty()),
    {
        let h0 = s0.heap;
        let h3 = self.heap;
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let x = none.insert(node.bits_spec());
        let d2 = |b: usize| -points(e, b);
        let d = |b: usize| -points(l, b) - points(e, b);
        lemma_alloc_moved(h1, h0, node);
        lemma_unextended(h1, node);
        assert(h1.scanned_word(node, 0).bits_spec() == 1);
        assert(h1.scanned_word(node, 1).bits_spec() == 1);
        lemma_set_unit_moved(h2, h1, node, 0, e);
        lemma_moved_chain(h2, h1, h0, z, d2, d2, none, x, none);
        lemma_unextended(h2, node);
        assert(h2.word_at(node, 1) == h1.word_at(node, 1));
        lemma_set_unit_moved(h3, h2, node, 1, l);
        lemma_moved_chain(h3, h2, h0, d2, |b: usize| -points(l, b), d, none, x, none);
        // 新しい節点についての変化
        let e3 = |b: usize| -points(e, b) - points(l, b);
        lemma_moved_chain(h3, h2, h1, d2, |b: usize| -points(l, b), e3, x, x, x);
        let nb = node.bits_spec();
        assert(live(h1, nb));
        assert(!live(h0, nb));
        assert(e3(nb) == 0);
        assert(live(h3, nb));
        lemma_unextended(h3, node);
        assert(h3.scanned_word(node, 0) == e);
        assert(h3.scanned_word(node, 1) == l);
        // Ctx の語と引数
        let ws = seq![l, e];
        assert(held_word(h0, ws[0]) && held_word(h0, ws[1]));
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b) + d(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, none);
        self.lemma_evv_kept(s0, d, none);
        // 新しいリスト
        h3.lemma_readable_scanned(node, 0);
        h3.lemma_readable_scanned(node, 1);
        lemma_ev_kept(h3, h0, d, none, e);
        assert(node_ok(h3, node));
        let f = choose|f: nat| #[trigger] path(h0, l, f) is Some;
        lemma_seq_of(h0, l, f);
        lemma_path_kept(h3, h0, d, none, l, f);
        assert(path(h3, l, f) == Some(seq_of(h0, l)));
        assert(same_head(h3, h1, nb));
        assert(h3.tag_of(node) == h0.tag_of(e));
        assert(h3.tag_of(e) == h3.tag_of(node));
        lemma_unit_bits();
        assert(node.bits_spec() != 1);
        assert(path(h3, node, f + 1) == Some(seq![(h3.tag_of(node), e)] + seq_of(h0, l)));
        lemma_seq_of(h3, node, f + 1);
        // 呼ぶ側が新しい節点の参照を1つ持つ
        self.lemma_fresh_holds(s0, node);
    }

    /// `evv` の物体の外の参照は、`Ctx` の参照と、引数のうちその物体を指す語の分を合わせた数以上ある。
    pub(super) proof fn lemma_evv_counted(&self, ws: Seq<Word>)
        requires
            self.holds_args(ws),
        ensures
            self.evv.is_ptr_spec() && self.heap.core.settled_view().rc_of(self.evv.bits_spec()) != 0
                ==> self.heap.core.settled_view().ext(self.evv.bits_spec()) >= 1 + points_in(
                ws,
                self.evv.bits_spec(),
            ),
    {
        let b = self.evv.bits_spec();
        lemma_points_at(ws, 0, b);
        if self.evv.is_ptr_spec() {
            assert(self.need(ws, b) >= 1 + points_in(ws, b));
        }
    }

    /// 引数の i 語目の物体の外の参照は、引数のうちその物体を指す語の数以上ある。
    pub(super) proof fn lemma_arg_counted(&self, ws: Seq<Word>, i: int)
        requires
            self.holds_args(ws),
            0 <= i < ws.len(),
        ensures
            held_word(self.heap, ws[i]),
            ws[i].is_ptr_spec() && self.heap.core.settled_view().rc_of(ws[i].bits_spec()) != 0
                ==> self.heap.core.settled_view().ext(ws[i].bits_spec()) >= points_in(
                ws,
                ws[i].bits_spec(),
            ) >= 1,
    {
        let b = ws[i].bits_spec();
        lemma_points_at(ws, i, b);
        if ws[i].is_ptr_spec() {
            assert(self.need(ws, b) >= points_in(ws, b));
        }
    }

    /// `settle` の後も、局所の evidence は書ける evidence のままである。
    pub(super) proof fn lemma_settled_ev(&self, s0: Ctx<M>, ev: Word)
        requires
            s0.held_ok(),
            s0.ev_shape(ev),
            self.heap == s0.heap.settled_heap(),
            self.heap.consistent(),
            self.heap.settled(),
        ensures
            ev_ok(self.heap, ev),
            s0.heap.rc_value(ev) > 0 ==> {
                &&& self.heap.writable(ev)
                &&& self.heap.rc_value(ev) > 0
                &&& self.heap.core.dom().contains(ev.bits_spec())
                &&& self.heap.core.state(ev.bits_spec()) == BlockState::Live
            },
            self.heap.settleable(),
            forall|i: int| #[trigger] self.heap.scanned_word(ev, i) == s0.heap.scanned_word(ev, i),
            self.heap.scanned_at(ev, 0) && self.heap.scanned_at(ev, 1) && self.heap.scanned_at(
                ev,
                2,
            ),
            self.heap.scanned_word(ev, 1) == self.heap.word_at(ev, 1),
            self.heap.scanned_word(ev, 0) == self.heap.word_at(ev, 0),
    {
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        lemma_settle_moved(self.heap, s0.heap);
        lemma_readable_kept(self.heap, s0.heap, ev);
        lemma_ev_kept(self.heap, s0.heap, z, none, ev);
        assert(same_head(self.heap, s0.heap, ev.bits_spec()));
        lemma_unextended(self.heap, ev);
        lemma_view_live(s0.heap, ev.bits_spec());
        if !self.heap.core.dom().contains(ev.bits_spec()) {
            self.heap.core.lemma_apart(ev.bits_spec());
            self.heap.core.lemma_region_at(ev.bits_spec());
        }
    }

    /// `ev_take_state` の、モデルが合っている道。
    pub(super) proof fn lemma_take_state_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        ev: Word,
        s: Word,
        unit: Word,
    )
        requires
            s0.held_ok(),
            s0.ev_shape(ev),
            s0.heap.rc_value(ev) > 0,
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.settled(),
            ev_ok(h1, ev),
            h1.core.dom().contains(ev.bits_spec()),
            h1.core.state(ev.bits_spec()) == BlockState::Live,
            forall|i: int| #[trigger] h1.scanned_word(ev, i) == s0.heap.scanned_word(ev, i),
            h1.scanned_at(ev, 1),
            h1.scanned_word(ev, 1) == h1.word_at(ev, 1),
            s == h1.word_at(ev, 1),
            unit.bits_spec() == 1,
            self.heap.field_set(h1, ev, 1, unit),
            self.heap.consistent(),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            s == s0.ev_state(ev),
            self.ev_state(ev).bits_spec() == 1,
            self.ev_table(ev) == s0.ev_table(ev),
            self.ev_outer(ev) == s0.ev_outer(ev),
            self.held_ok(),
            self.holds(s),
            self.ev_shape(ev),
            self.evolved_by(s0, |b: usize| points(s, b), Set::empty().insert(ev.bits_spec())),
    {
        let h0 = s0.heap;
        let h2 = self.heap;
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let x = none.insert(ev.bits_spec());
        let d = |b: usize| points(s, b);
        lemma_unit_bits();
        lemma_settle_moved(h1, h0);
        lemma_set_moved(h2, h1, ev, 1, unit);
        lemma_moved_chain(
            h2,
            h1,
            h0,
            z,
            |b: usize| points(h1.word_at(ev, 1), b) - points(unit, b),
            d,
            none,
            x,
            x,
        );
        lemma_unextended(h1, ev);
        lemma_unextended(h2, ev);
        assert forall|i: int| i != 1 implies #[trigger] h2.scanned_word(ev, i) == h0.scanned_word(
            ev,
            i,
        ) by {
            assert(h2.word_at(ev, i) == h1.word_at(ev, i));
        }
        assert(h2.scanned_word(ev, 1) == unit);
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(seq![], b) + d(
            b,
        ) >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
        }
        self.lemma_holds_moved(s0, seq![], seq![], d, x);
        h0.lemma_readable_scanned(ev, 1);
        self.lemma_dupped_holds(s0, s, x);
        lemma_readable_kept(h1, h0, ev);
        lemma_readable_kept(h2, h1, ev);
        lemma_state_written(h2, h0, d, ev, unit);
        lemma_listed_kept(h2, h0, d, x, self.evv, self.max_evv);
    }

    /// `ev_set_state` で状態を書いた後、元の状態は `decref` できる。新しい状態が元の状態と同じ物体でも、呼ぶ側の
    /// 参照が残る。
    pub(super) proof fn lemma_set_state_mid(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        ev: Word,
        s: Word,
        old_state: Word,
    )
        requires
            s0.held_ok(),
            s0.ev_shape(ev),
            s0.heap.rc_value(ev) > 0,
            s0.holds_args(seq![ev, s]),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.settled(),
            ev_ok(h1, ev),
            h1.core.dom().contains(ev.bits_spec()),
            h1.core.state(ev.bits_spec()) == BlockState::Live,
            h1.holds(s),
            held_word(h1, s),
            h1.scanned_at(ev, 1),
            h1.scanned_word(ev, 1) == h1.word_at(ev, 1),
            old_state == h1.word_at(ev, 1),
            self.heap.field_set(h1, ev, 1, s),
            self.heap.consistent(),
        ensures
            self.heap.decref_ok(old_state),
    {
        let h2 = self.heap;
        let x = Set::<usize>::empty().insert(ev.bits_spec());
        let d = |b: usize| points(old_state, b) - points(s, b);
        lemma_set_moved(h2, h1, ev, 1, s);
        h1.lemma_readable_scanned(ev, 1);
        let ob = old_state.bits_spec();
        if old_state.is_ptr_spec() && h1.core.settled_view().rc_of(ob) != 0 {
            assert(live(h1, ob));
            lemma_ext_nonneg(h1, ob);
            if s.is_ptr_spec() && s.bits_spec() == ob {
                assert(h1.core.settled_view().holds(s));
            }
        }
        lemma_arg_moved(h2, h1, d, x, old_state);
        if old_state.is_ptr_spec() {
            assert(same_head(h2, h1, ob));
        }
    }

    /// `ev_set_state` の、元の状態を手放した後の、モデルが合っている道。
    pub(super) proof fn lemma_set_state_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        ev: Word,
        s: Word,
        old_state: Word,
    )
        requires
            s0.held_ok(),
            s0.ev_shape(ev),
            s0.heap.rc_value(ev) > 0,
            s0.holds_args(seq![ev, s]),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.settled(),
            ev_ok(h1, ev),
            h1.core.dom().contains(ev.bits_spec()),
            h1.core.state(ev.bits_spec()) == BlockState::Live,
            forall|i: int| #[trigger] h1.scanned_word(ev, i) == s0.heap.scanned_word(ev, i),
            h1.scanned_at(ev, 1),
            h1.scanned_word(ev, 1) == h1.word_at(ev, 1),
            old_state == h1.word_at(ev, 1),
            h2.field_set(h1, ev, 1, s),
            h2.consistent(),
            h2.decref_ok(old_state),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.evolved(h2, old_state, -1),
            self.heap.kept_live(h2),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.ev_state(ev) == s,
            self.ev_table(ev) == s0.ev_table(ev),
            self.ev_outer(ev) == s0.ev_outer(ev),
            self.held_ok(),
            self.holds(ev),
            self.ev_shape(ev),
            self.evolved_by(s0, |b: usize| -points(s, b), Set::empty().insert(ev.bits_spec())),
    {
        let h0 = s0.heap;
        let h3 = self.heap;
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let x = none.insert(ev.bits_spec());
        let d12 = |b: usize| points(h1.word_at(ev, 1), b) - points(s, b);
        let d23 = |b: usize| -points(old_state, b);
        let d = |b: usize| -points(s, b);
        lemma_settle_moved(h1, h0);
        lemma_set_moved(h2, h1, ev, 1, s);
        lemma_decref_moved(h3, h2, old_state);
        lemma_moved_chain(h3, h2, h1, d12, d23, d, x, none, x);
        lemma_moved_chain(h3, h1, h0, z, d, d, none, x, x);
        // Ctx の語と、呼ぶ側の持つ ev
        let ws = seq![ev, s];
        assert(held_word(h0, ws[0]) && held_word(h0, ws[1]));
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b) + d(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, x);
        assert forall|i: int| 0 <= i < seq![ev].len() implies held_word(
            h0,
            #[trigger] seq![ev][i],
        ) by {}
        assert forall|b: usize| #[trigger] self.need(seq![ev], b) > 0 implies s0.need(ws, b) + d(b)
            >= self.need(seq![ev], b) by {
            lemma_points_in(seq![ev], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![ev], d, x);
        assert(held_word(h3, seq![ev][0]));
        // 状態の欄だけが変わった
        lemma_unextended(h1, ev);
        lemma_unextended(h2, ev);
        assert(live(h2, ev.bits_spec()) && live(h3, ev.bits_spec())) by {
            assert(live(h1, ev.bits_spec()));
        }
        assert(same_head(h3, h2, ev.bits_spec()));
        lemma_unextended(h3, ev);
        assert forall|i: int| i != 1 implies #[trigger] h3.scanned_word(ev, i) == h0.scanned_word(
            ev,
            i,
        ) by {
            assert(h2.word_at(ev, i) == h1.word_at(ev, i));
        }
        assert(h3.scanned_word(ev, 1) == s);
        lemma_state_written(h3, h0, d, ev, s);
        lemma_listed_kept(h3, h0, d, x, self.evv, self.max_evv);
    }

    /// 入口の表の欄 `op` を書く物理的な位置は `i` で、その欄は書ける。
    pub(super) proof fn lemma_entry_slot(&self, s0: Ctx<M>, ev: Word, op: int, i: int)
        requires
            s0.held_ok(),
            s0.ev_shape(ev),
            0 <= op < s0.ev_ops(ev),
            s0.heap.rc_value(s0.ev_table(ev)) > 0,
            i == if s0.heap.scan_of(s0.ev_table(ev)) == 255 {
                op + 1
            } else {
                op
            },
            self.heap == s0.heap.settled_heap(),
            self.heap.consistent(),
            self.heap.settled(),
        ensures
            self.heap.scanned_at(s0.ev_table(ev), i),
            self.heap.word_at(s0.ev_table(ev), i) == s0.ev_entry(ev, op),
            self.heap.writable(s0.ev_table(ev)),
            self.heap.owned(s0.ev_table(ev)),
            self.heap.core.dom().contains(s0.ev_table(ev).bits_spec()),
            self.heap.core.state(s0.ev_table(ev).bits_spec()) == BlockState::Live,
    {
        let t = s0.ev_table(ev);
        let tb = t.bits_spec();
        lemma_settle_moved(self.heap, s0.heap);
        lemma_readable_kept(self.heap, s0.heap, t);
        assert(same_head(self.heap, s0.heap, tb));
        lemma_view_live(s0.heap, tb);
        if !self.heap.core.dom().contains(tb) {
            self.heap.core.lemma_apart(tb);
            self.heap.core.lemma_region_at(tb);
        }
    }

    /// `ev_set_clause` の、モデルが合っている道。
    pub(super) proof fn lemma_set_clause_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        ev: Word,
        op: int,
        i: int,
        c: Word,
    )
        requires
            s0.held_ok(),
            s0.ev_shape(ev),
            0 <= op < s0.ev_ops(ev),
            s0.ev_entry(ev, op).bits_spec() == 1,
            s0.holds(c),
            c.is_ptr_spec() ==> !s0.heap.lin_of(c),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.settled(),
            h1.scanned_at(s0.ev_table(ev), i),
            h1.word_at(s0.ev_table(ev), i) == s0.ev_entry(ev, op),
            h1.core.dom().contains(s0.ev_table(ev).bits_spec()),
            h1.core.state(s0.ev_table(ev).bits_spec()) == BlockState::Live,
            i == if s0.heap.scan_of(s0.ev_table(ev)) == 255 {
                op + 1
            } else {
                op
            },
            self.heap.field_set(h1, s0.ev_table(ev), i, c),
            self.heap.consistent(),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.ev_entry(ev, op) == c,
            forall|j: int| j != op ==> #[trigger] self.ev_entry(ev, j) == s0.ev_entry(ev, j),
            self.ev_ops(ev) == s0.ev_ops(ev),
            self.held_ok(),
            self.ev_shape(ev),
            self.evolved_by(
                s0,
                |b: usize| -points(c, b),
                Set::empty().insert(s0.ev_table(ev).bits_spec()),
            ),
    {
        let h0 = s0.heap;
        let h2 = self.heap;
        let t = s0.ev_table(ev);
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let x = none.insert(t.bits_spec());
        let d = |b: usize| -points(c, b);
        lemma_unit_bits();
        lemma_unextended(h0, ev);
        lemma_settle_moved(h1, h0);
        lemma_set_moved(h2, h1, t, i, c);
        lemma_moved_chain(
            h2,
            h1,
            h0,
            z,
            |b: usize| points(h1.word_at(t, i), b) - points(c, b),
            d,
            none,
            x,
            x,
        );
        // Ctx の語
        let ws = seq![c];
        assert(held_word(h0, ws[0]));
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b) + d(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, x);
        // ev は書いていない。表は欄 op だけが変わった
        lemma_readable_kept(h1, h0, ev);
        lemma_readable_kept(h2, h1, ev);
        assert(live(h0, ev.bits_spec()) && live(h2, ev.bits_spec()));
        assert(ev.bits_spec() != t.bits_spec());
        assert(h2.scanned_word(ev, 0) == t);
        h2.lemma_readable_scanned(ev, 0);
        h0.lemma_readable_scanned(ev, 0);
        assert(live(h0, t.bits_spec()) && live(h2, t.bits_spec()));
        assert(same_head(h2, h0, t.bits_spec()));
        assert(h2.count_of(t) == h0.count_of(t));
        assert forall|j: int| j != op implies #[trigger] h2.scanned_word(t, j) == h0.scanned_word(
            t,
            j,
        ) by {}
        assert(h2.scanned_word(t, op) == c);
        // c は表から読めて、`lin` ビットがない
        h2.lemma_readable_scanned(t, op);
        if c.is_ptr_spec() {
            assert(live(h0, c.bits_spec()) && live(h2, c.bits_spec()));
            assert(same_head(h2, h0, c.bits_spec()));
        }
        lemma_entry_written(h2, h0, d, t, op, c);
        lemma_listed_kept(h2, h0, d, x, self.evv, self.max_evv);
        lemma_ev_kept(h2, h0, d, x, ev);
    }

    /// `ev_renew` の、モデルが合っている道。
    pub(super) proof fn lemma_ev_renew_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        h3: Heap<M>,
        h4: Heap<M>,
        h5: Heap<M>,
        ev: Word,
        t: Word,
        l: Word,
        e: Word,
        state: Word,
    )
        requires
            s0.held_ok(),
            s0.ev_shape(ev),
            s0.holds(state),
            t == s0.ev_table(ev),
            l == s0.evv,
            h1.bumped(s0.heap, t),
            h1.settled(),
            h2.bumped(h1, l),
            h2.settled(),
            h3.made(h2, e),
            h3.kind_of(e) == Kind::Data,
            h3.scan_of(e) == 3,
            h3.tag_of(e) == s0.heap.tag_of(ev),
            h3.lin_of(e) == false,
            h3.owned(e),
            forall|i: int| 0 <= i < 3 ==> (#[trigger] h3.scanned_word(e, i)).bits_spec() == 1,
            h4.field_set(h3, e, 0, t),
            h5.field_set(h4, e, 1, state),
            self.heap.field_set(h5, e, 2, l),
            h1.consistent(),
            h2.consistent(),
            h4.consistent(),
            h5.consistent(),
            self.heap.consistent(),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(e),
            self.ev_shape(e),
            self.ev_shape(ev),
            self.ev_tag(e) == s0.ev_tag(ev),
            self.ev_table(e) == s0.ev_table(ev),
            self.ev_state(e) == state,
            self.ev_outer(e) == s0.evv,
            self.evolved_by(s0, |b: usize| -points(state, b), Set::empty()),
    {
        let h0 = s0.heap;
        let h6 = self.heap;
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let x = none.insert(e.bits_spec());
        let d1 = |b: usize| points(t, b);
        let d2 = |b: usize| points(t, b) + points(l, b);
        let d4 = |b: usize| points(l, b);
        let d5 = |b: usize| points(l, b) - points(state, b);
        let d = |b: usize| -points(state, b);
        // 古い物体についての変化
        lemma_dup_moved(h1, h0, t);
        lemma_dup_moved(h2, h1, l);
        lemma_moved_chain(h2, h1, h0, d1, |b: usize| points(l, b), d2, none, none, none);
        lemma_alloc_moved(h3, h2, e);
        lemma_moved_chain(h3, h2, h0, d2, z, d2, none, none, none);
        lemma_unextended(h3, e);
        assert(h3.scanned_word(e, 0).bits_spec() == 1);
        assert(h3.scanned_word(e, 1).bits_spec() == 1);
        assert(h3.scanned_word(e, 2).bits_spec() == 1);
        lemma_set_unit_moved(h4, h3, e, 0, t);
        lemma_moved_chain(h4, h3, h0, d2, |b: usize| -points(t, b), d4, none, x, none);
        lemma_unextended(h4, e);
        assert(h4.word_at(e, 1) == h3.word_at(e, 1));
        assert(h4.word_at(e, 2) == h3.word_at(e, 2));
        lemma_set_unit_moved(h5, h4, e, 1, state);
        lemma_moved_chain(h5, h4, h0, d4, |b: usize| -points(state, b), d5, none, x, none);
        lemma_unextended(h5, e);
        assert(h5.word_at(e, 2) == h4.word_at(e, 2));
        lemma_set_unit_moved(h6, h5, e, 2, l);
        lemma_moved_chain(h6, h5, h0, d5, |b: usize| -points(l, b), d, none, x, none);
        // 新しい evidence についての変化
        let e4 = |b: usize| -points(t, b);
        let e5 = |b: usize| -points(t, b) - points(state, b);
        let e6 = |b: usize| -points(t, b) - points(state, b) - points(l, b);
        lemma_moved_chain(h5, h4, h3, e4, |b: usize| -points(state, b), e5, x, x, x);
        lemma_moved_chain(h6, h5, h3, e5, |b: usize| -points(l, b), e6, x, x, x);
        let eb = e.bits_spec();
        assert(live(h3, eb));
        assert(!live(h2, eb));
        assert(e6(eb) == 0) by {
            lemma_readable_kept(h2, h0, ev);
            h0.lemma_readable_scanned(ev, 0);
            lemma_readable_kept(h2, h0, t);
            lemma_listed_head(h0, l, s0.max_evv);
            lemma_readable_kept(h2, h0, l);
            assert(held_word(h0, seq![state][0]));
            lemma_readable_kept(h2, h0, state);
        }
        assert(live(h6, eb));
        lemma_unextended(h6, e);
        assert(h6.scanned_word(e, 0) == t);
        assert(h6.scanned_word(e, 1) == state);
        assert(h6.scanned_word(e, 2) == l);
        // Ctx の語と引数
        let ws = seq![state];
        assert(held_word(h0, ws[0]));
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b) + d(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, none);
        self.lemma_evv_kept(s0, d, none);
        // 借りた evidence と、新しい evidence の形
        lemma_unextended(h0, ev);
        lemma_readable_kept(h3, h0, ev);
        lemma_readable_kept(h4, h3, ev);
        lemma_readable_kept(h5, h4, ev);
        lemma_readable_kept(h6, h5, ev);
        lemma_ev_kept(h6, h0, d, none, ev);
        h6.lemma_readable_scanned(e, 0);
        h6.lemma_readable_scanned(e, 1);
        h6.lemma_readable_scanned(e, 2);
        h0.lemma_readable_scanned(ev, 0);
        lemma_table_kept(h6, h0, d, none, t);
        lemma_listed_head(h0, l, s0.max_evv);
        lemma_word_kept(h6, h0, d, none, state);
        lemma_word_kept(h6, h0, d, none, l);
        assert(same_head(h6, h3, eb));
        assert(ev_ok(h6, e));
        // 呼ぶ側が新しい evidence の参照を1つ持つ
        self.lemma_fresh_holds(s0, e);
    }

    /// evidence から読んだ `w` を `dup` した後の、モデルが合っている道。
    pub(super) proof fn lemma_dupped_ok(&self, s0: Ctx<M>, w: Word, ev: Word)
        requires
            s0.held_ok(),
            s0.ev_shape(ev),
            s0.heap.readable(w),
            dupable(s0.heap, w),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
            self.heap.bumped(s0.heap, w),
            self.heap.settled(),
            self.heap.consistent(),
        ensures
            self.held_ok(),
            self.holds(w),
            self.ev_shape(ev),
            self.evolved_by(s0, |b: usize| points(w, b), Set::empty()),
    {
        let d = |b: usize| points(w, b);
        let none = Set::<usize>::empty();
        lemma_dup_moved(self.heap, s0.heap, w);
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(seq![], b) + d(
            b,
        ) >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
        }
        self.lemma_holds_moved(s0, seq![], seq![], d, none);
        self.lemma_evv_kept(s0, d, none);
        self.lemma_dupped_holds(s0, w, none);
        lemma_readable_kept(self.heap, s0.heap, ev);
        lemma_ev_kept(self.heap, s0.heap, d, none, ev);
    }

    /// `old` から `w` を `dup` しただけの変化の後、呼ぶ側は `w` の参照を `Ctx` と別に1つ持つ。
    pub(super) proof fn lemma_dupped_holds(&self, old: Ctx<M>, w: Word, x: Set<usize>)
        requires
            old.holds_args(seq![]),
            self.holds_args(seq![]),
            old.heap.consistent(),
            old.heap.settleable(),
            self.evv == old.evv,
            self.yielding == old.yielding,
            moved(self.heap, old.heap, |b: usize| points(w, b), x),
            held_word(old.heap, w),
        ensures
            self.holds(w),
    {
        let ws = seq![w];
        if w.is_ptr_spec() && old.heap.core.settled_view().rc_of(w.bits_spec()) != 0 {
            assert(live(old.heap, w.bits_spec()));
            lemma_ext_nonneg(old.heap, w.bits_spec());
        }
        lemma_arg_moved(self.heap, old.heap, |b: usize| points(w, b), x, w);
        assert(held_word(self.heap, ws[0]));
        let v0 = old.heap.core.settled_view();
        let v1 = self.heap.core.settled_view();
        assert forall|c: usize| #[trigger] self.need(ws, c) > 0 && v1.rc_of(c) != 0 implies v1.ext(
            c,
        ) >= self.need(ws, c) by {
            lemma_points_in(ws, c);
            lemma_points_in(seq![], c);
            if points(w, c) > 0 {
                assert(live(old.heap, c));
                if v0.rc_of(c) == 0 {
                    assert(live(self.heap, c));
                    assert(same_head(self.heap, old.heap, c));
                } else {
                    if old.need(seq![], c) <= 0 {
                        lemma_ext_nonneg(old.heap, c);
                    }
                    assert(old.need(seq![], c) > 0 ==> v0.ext(c) >= old.need(seq![], c));
                }
            } else {
                assert(self.need(seq![], c) > 0);
            }
        }
    }

    /// `old` で生きていなかった物体 `r` の参照を1つ呼ぶ側が持てば、`holds(r)` である。`Ctx` の語は `old` で生きて
    /// いた物体を指すので、`r` を指さない。
    pub(super) proof fn lemma_fresh_holds(&self, old: Ctx<M>, r: Word)
        requires
            self.holds_args(seq![]),
            old.holds_args(seq![]),
            self.evv == old.evv,
            self.yielding == old.yielding,
            r.is_ptr_spec(),
            !live(old.heap, r.bits_spec()),
            held_word(self.heap, r),
            self.heap.core.settled_view().ext(r.bits_spec()) >= 1,
        ensures
            self.holds(r),
    {
        let b = r.bits_spec();
        assert(self.need(seq![], b) == 0) by {
            lemma_points_in(seq![], b);
        }
        assert forall|c: usize| #[trigger]
            self.need(seq![r], c) > 0 && self.heap.core.settled_view().rc_of(c)
                != 0 implies self.heap.core.settled_view().ext(c) >= self.need(seq![r], c) by {
            lemma_points_in(seq![r], c);
            lemma_points_in(seq![], c);
            assert(self.need(seq![], c) > 0 ==> self.need(seq![], c) == self.need(seq![r], c) || c
                == b);
        }
        let ws = seq![r];
        assert(held_word(self.heap, ws[0]));
    }
}

/// `ws` が3語までのとき、物体を指す語の数を語ごとに足す。
pub(super) proof fn lemma_points_in(ws: Seq<Word>, b: usize)
    ensures
        ws.len() == 0 ==> points_in(ws, b) == 0,
        ws.len() == 1 ==> points_in(ws, b) == points(ws[0], b),
        ws.len() == 2 ==> points_in(ws, b) == points(ws[0], b) + points(ws[1], b),
        ws.len() == 3 ==> points_in(ws, b) == points(ws[0], b) + points(ws[1], b) + points(
            ws[2],
            b,
        ),
{
    reveal_with_fuel(points_in, 4);
    if ws.len() >= 1 {
        let a = ws.drop_last();
        if ws.len() >= 2 {
            assert(a[0] == ws[0]);
            let c = a.drop_last();
            if ws.len() == 3 {
                assert(a[1] == ws[1]);
                assert(c[0] == ws[0]);
            }
        }
    }
}

/// 物体を指す語の数は負でなく、どの語の分も含む。
pub(super) proof fn lemma_points_at(ws: Seq<Word>, i: int, b: usize)
    ensures
        points_in(ws, b) >= 0,
        0 <= i < ws.len() ==> points_in(ws, b) >= points(ws[i], b),
    decreases ws.len(),
{
    if ws.len() > 0 {
        let a = ws.drop_last();
        if 0 <= i < ws.len() - 1 {
            lemma_points_at(a, i, b);
            assert(a[i] == ws[i]);
        } else {
            lemma_points_at(a, 0, b);
        }
    }
}

/// 物体を指す語が数に入るなら、その語がある。
pub(super) proof fn lemma_points_witness(ws: Seq<Word>, b: usize) -> (i: int)
    requires
        points_in(ws, b) > 0,
    ensures
        0 <= i < ws.len(),
        ws[i].is_ptr_spec(),
        ws[i].bits_spec() == b,
    decreases ws.len(),
{
    if points(ws.last(), b) > 0 {
        ws.len() - 1
    } else {
        let a = ws.drop_last();
        let j = lemma_points_witness(a, b);
        assert(a[j] == ws[j]);
        j
    }
}

/// モデルが合っていれば、生きている物体の外の参照の数は負でない。
pub(super) proof fn lemma_ext_nonneg<M: Mode>(h: Heap<M>, b: usize)
    requires
        h.consistent(),
        h.settleable(),
        live(h, b),
        h.core.settled_view().rc_of(b) != 0,
    ensures
        h.core.settled_view().ext(b) >= 0,
{
    let v = h.core.settled_view();
    h.core.lemma_settled_view();
    if !v.dom().contains(b) {
        v.lemma_region_at(b);
    }
    reveal(Core::refs_wf);
    assert(v.counted_wf(b));
}

/// 読める語の物体の外の参照の数は負でない。
pub(super) proof fn lemma_ext_nonneg_word<M: Mode>(h: Heap<M>, w: Word)
    requires
        h.consistent(),
        h.settleable(),
        h.readable(w),
    ensures
        w.is_ptr_spec() && h.core.settled_view().rc_of(w.bits_spec()) != 0
            ==> h.core.settled_view().ext(w.bits_spec()) >= 0,
{
    if w.is_ptr_spec() && h.core.settled_view().rc_of(w.bits_spec()) != 0 {
        assert(live(h, w.bits_spec()));
        lemma_ext_nonneg(h, w.bits_spec());
    }
}

/// 欄が同じ `meta` の値は同じである。速いモードの `ev_lookup` は、節点の `meta` 全体を1回比べる。
pub(super) proof fn lemma_meta_eq(a: u32, b: u32)
    requires
        bits_wf(a),
        bits_wf(b),
        bits_tag(a) == bits_tag(b),
        bits_scan(a) == bits_scan(b),
        bits_kind(a) == bits_kind(b),
        bits_lin(a) == bits_lin(b),
    ensures
        a == b,
{
    let ka = (a >> 24) & 0xF;
    let kb = (b >> 24) & 0xF;
    assert(ka == kb);
    assert(a == b) by (bit_vector)
        requires
            a & 0x7000_0000 == 0,
            b & 0x7000_0000 == 0,
            (a >> 24) & 0xF == (b >> 24) & 0xF,
            (a >> 16) as u8 == (b >> 16) as u8,
            a as u16 == b as u16,
            (a & 0x8000_0000 != 0) == (b & 0x8000_0000 != 0),
    ;
}

// `moved` の補題
/// 何も変えない変化。
pub(super) proof fn lemma_moved_refl<M: Mode>(h: Heap<M>)
    ensures
        moved(h, h, |b: usize| 0int, Set::empty()),
        no_free(h, h),
        no_alloc(h, h),
        exact(h, h, |b: usize| 0int),
{
}

/// 例外の物体のうち `h0` で生きていたものが `y` に入り、参照の数の変化が同じなら、同じ変化と言える。
pub(super) proof fn lemma_moved_weaken<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    e: spec_fn(usize) -> int,
    x: Set<usize>,
    y: Set<usize>,
)
    requires
        moved(h1, h0, d, x),
        forall|b: usize| live(h0, b) ==> #[trigger] d(b) == e(b),
        forall|b: usize| x.contains(b) && live(h0, b) ==> #[trigger] y.contains(b),
    ensures
        moved(h1, h0, e, y),
{
    let v0 = h0.core.settled_view();
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + e(b) >= 1) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& v0.rc_of(b) != 0 ==> h1.core.settled_view().ext(b) == v0.ext(b) + e(b)
    } by {
        assert(d(b) == e(b));
    }
}

/// 検査モードの `word_in` は、続けた操作の後も保たれる。
pub(super) proof fn lemma_words_trans<M: Mode>(h2: Heap<M>, h1: Heap<M>, h0: Heap<M>)
    requires
        h2.words_kept(h1),
        h1.words_kept(h0),
    ensures
        h2.words_kept(h0),
{
}

/// まだ書いていない欄 (`()` の欄) への `set_field` の変化。`v` の外の参照が辺に移る。
pub(super) proof fn lemma_set_unit_moved<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    w: Word,
    i: int,
    v: Word,
)
    requires
        h0.consistent(),
        h0.settled(),
        w.is_ptr_spec(),
        h0.core.dom().contains(w.bits_spec()),
        h0.core.state(w.bits_spec()) == BlockState::Live,
        h0.scanned_at(w, i),
        h0.word_at(w, i).bits_spec() == 1,
        h1.field_set(h0, w, i, v),
    ensures
        moved(h1, h0, |b: usize| -points(v, b), Set::empty().insert(w.bits_spec())),
        no_free(h1, h0),
        no_alloc(h1, h0),
        exact(h1, h0, |b: usize| -points(v, b)),
{
    lemma_set_moved(h1, h0, w, i, v);
    lemma_unit_bits();
    let x = Set::empty().insert(w.bits_spec());
    lemma_moved_weaken(
        h1,
        h0,
        |b: usize| points(h0.word_at(w, i), b) - points(v, b),
        |b: usize| -points(v, b),
        x,
        x,
    );
}

/// `lemma_moved_trans` と `lemma_moved_weaken` を続けて使う。`d` と `x` は `h0` で生きている物体について合えばよい。
pub(super) proof fn lemma_moved_chain<M: Mode>(
    h2: Heap<M>,
    h1: Heap<M>,
    h0: Heap<M>,
    d1: spec_fn(usize) -> int,
    d2: spec_fn(usize) -> int,
    d: spec_fn(usize) -> int,
    x1: Set<usize>,
    x2: Set<usize>,
    x: Set<usize>,
)
    requires
        moved(h1, h0, d1, x1),
        moved(h2, h1, d2, x2),
        no_free(h1, h0) || no_alloc(h2, h1),
        (forall|b: usize| #[trigger] d2(b) <= 0) || (no_free(h1, h0) && exact(h1, h0, d1)),
        forall|b: usize| live(h0, b) ==> #[trigger] d(b) == d1(b) + d2(b),
        forall|b: usize|
            (x1.contains(b) || x2.contains(b)) && live(h0, b) ==> #[trigger] x.contains(b),
    ensures
        moved(h2, h0, d, x),
        no_free(h1, h0) && no_free(h2, h1) ==> no_free(h2, h0),
        no_alloc(h1, h0) && no_alloc(h2, h1) ==> no_alloc(h2, h0),
        no_free(h1, h0) && no_free(h2, h1) && exact(h1, h0, d1) && exact(h2, h1, d2) ==> exact(
            h2,
            h0,
            d,
        ),
{
    let v0 = h0.core.settled_view();
    let v1 = h1.core.settled_view();
    assert forall|b: usize| live(h0, b) && live(h2, b) implies #[trigger] same_head(h2, h0, b) && (
    !x.contains(b) ==> h2.core.perm(b).slots == h0.core.perm(b).slots) by {
        assert(live(h1, b));
        assert(same_head(h1, h0, b));
        assert(same_head(h2, h1, b));
    }
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + d(b) >= 1) implies {
        &&& live(h2, b)
        &&& h2.core.block(b) == h0.core.block(b)
        &&& v0.rc_of(b) != 0 ==> h2.core.settled_view().ext(b) == v0.ext(b) + d(b)
    } by {
        assert(d(b) == d1(b) + d2(b));
        if forall|c: usize| #[trigger] d2(c) <= 0 {
            assert(d2(b) <= 0);
        }
        assert(live(h1, b));
        assert(same_head(h1, h0, b));
        assert(live(h2, b));
    }
    if no_free(h1, h0) && no_free(h2, h1) && exact(h1, h0, d1) && exact(h2, h1, d2) {
        assert forall|b: usize|
            #![trigger live(h0, b)]
            live(h0, b) && v0.rc_of(b) != 0 implies h2.core.settled_view().ext(b) == v0.ext(b) + d(
            b,
        ) by {
            assert(live(h1, b));
            assert(same_head(h1, h0, b));
        }
    }
}

/// 引数 `w` は、外の参照の数が `d` 変わっても 1 以上残れば、変化の後も持たれている。
pub(super) proof fn lemma_arg_moved<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    x: Set<usize>,
    w: Word,
)
    requires
        moved(h1, h0, d, x),
        held_word(h0, w),
        w.is_ptr_spec() && h0.core.settled_view().rc_of(w.bits_spec()) != 0
            ==> h0.core.settled_view().ext(w.bits_spec()) + d(w.bits_spec()) >= 1,
    ensures
        held_word(h1, w),
        h1.holds(w),
        w.is_ptr_spec() ==> live(h1, w.bits_spec()),
{
    if w.is_ptr_spec() {
        let b = w.bits_spec();
        assert(live(h0, b));
        assert(live(h1, b));
        assert(same_head(h1, h0, b));
        lemma_view_live(h1, b);
    }
}

/// `scan` が 255 でない物体は、走査する語の数を `meta` に持ち、走査する語は先頭の語から並ぶ。
pub(super) proof fn lemma_unextended<M: Mode>(h: Heap<M>, w: Word)
    requires
        h.scan_of(w) != 255,
    ensures
        h.count_of(w) == h.scan_of(w) as int,
        forall|i: int| #[trigger] h.scanned_word(w, i) == h.word_at(w, i),
        forall|i: int| #[trigger] h.scanned_at(w, i) <==> 0 <= i < h.scan_of(w),
{
}

/// 物体を空けない変化の後も、読める語は読める。
pub(super) proof fn lemma_readable_kept<M: Mode>(h1: Heap<M>, h0: Heap<M>, w: Word)
    requires
        no_free(h1, h0),
        h0.readable(w),
    ensures
        h1.readable(w),
        w.is_ptr_spec() ==> live(h0, w.bits_spec()) && live(h1, w.bits_spec()),
{
    if w.is_ptr_spec() {
        assert(live(h0, w.bits_spec()));
    }
}

/// 欄を1つ書いても、同じ物体のほかの欄の `set_field` の確かめは通るままである。基本操作は、確保したばかりの物体に
/// 続けて書くので、後の `set_field` が `Err` を返さないことをこれで言う。
pub(super) proof fn lemma_set_passes_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    w: Word,
    i: int,
    v: Word,
    j: int,
)
    requires
        h1.field_set(h0, w, i, v),
        h0.set_passes(w, j),
        h0.scan_of(w) != 255,
    ensures
        h1.set_passes(w, j),
{
}

/// `settle` を2回しても、1回と同じである。
pub(super) proof fn lemma_settled_twice<M: Mode>(h: Heap<M>)
    ensures
        h.settled_heap().settled_heap() == h.settled_heap(),
{
}

/// 構築中の物体を生きている物体にするだけの変化 (`settle`)。
pub(super) proof fn lemma_settle_moved<M: Mode>(h1: Heap<M>, h0: Heap<M>)
    requires
        h1 == h0.settled_heap(),
    ensures
        moved(h1, h0, |b: usize| 0int, Set::empty()),
        no_free(h1, h0),
        no_alloc(h1, h0),
        exact(h1, h0, |b: usize| 0int),
{
    assert forall|b: usize| #[trigger] live(h1, b) == live(h0, b) by {}
}

/// `dup` の変化。
pub(super) proof fn lemma_dup_moved<M: Mode>(h1: Heap<M>, h0: Heap<M>, w: Word)
    requires
        h0.consistent(),
        h0.settleable(),
        h1.bumped(h0, w),
        h1.settled(),
    ensures
        moved(h1, h0, |b: usize| points(w, b), Set::empty()),
        no_free(h1, h0),
        no_alloc(h1, h0),
        exact(h1, h0, |b: usize| points(w, b)),
{
    let v0 = h0.core.settled_view();
    let hw = w.bits_spec();
    h0.core.lemma_settled_view();
    if !w.is_ptr_spec() && v0.dom().contains(hw) {
        // ブロックの番地は物体を指す語の値なので、即値と重ならない
        v0.lemma_at(hw);
        crate::word::lemma_aligned_is_ptr(hw);
    }
    assert forall|b: usize| #[trigger] live(h1, b) == live(h0, b) by {
        lemma_view_live(h0, b);
    }
    assert forall|b: usize| live(h0, b) && live(h1, b) implies #[trigger] same_head(h1, h0, b)
        && h1.core.perm(b).slots == h0.core.perm(b).slots by {
        if v0.dom().contains(b) && b != hw {
            assert(h1.core.perms@[b] == v0.perms@[b]);
        }
    }
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + points(w, b) >= 1) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& v0.rc_of(b) != 0 ==> h1.core.settled_view().ext(b) == v0.ext(b) + points(w, b)
    } by {
        lemma_view_live(h0, b);
        if v0.dom().contains(b) && b != hw {
            assert(h1.core.perms@[b] == v0.perms@[b]);
        }
        if !v0.dom().contains(b) {
            v0.lemma_apart(b);
            v0.lemma_region_at(b);
        }
    }
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && v0.rc_of(b) != 0 implies h1.core.settled_view().ext(b) == v0.ext(b) + points(
        w,
        b,
    ) by {
        lemma_view_live(h0, b);
        if v0.dom().contains(b) && b != hw {
            assert(h1.core.perms@[b] == v0.perms@[b]);
        }
        if !v0.dom().contains(b) {
            v0.lemma_apart(b);
            v0.lemma_region_at(b);
        }
    }
}

/// 生きているかは、構築中の物体を生きている物体にした見方でも同じである。
pub(super) proof fn lemma_view_live<M: Mode>(h: Heap<M>, b: usize)
    ensures
        live(h, b) == (h.core.settled_view().has(b) && h.core.settled_view().state(b)
            == BlockState::Live),
        h.core.settled_view().settled_state(b) == h.core.settled_view().state(b),
{
}

/// `decref` の変化。
pub(super) proof fn lemma_decref_moved<M: Mode>(h1: Heap<M>, h0: Heap<M>, w: Word)
    requires
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.settled(),
        h1.evolved(h0, w, -1),
        h1.kept_live(h0),
    ensures
        moved(h1, h0, |b: usize| -points(w, b), Set::empty()),
        no_alloc(h1, h0),
{
    let v0 = h0.core.settled_view();
    let d = |b: usize| -points(w, b);
    h0.core.lemma_settled_view();
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + d(b) >= 1) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& v0.rc_of(b) != 0 ==> h1.core.settled_view().ext(b) == v0.ext(b) + d(b)
    } by {
        if v0.dom().contains(b) {
            assert(v0.settled_state(b) == BlockState::Live);
        } else {
            // 付けた塊の物体は、どのヒープのブロックにもならない
            v0.lemma_region_at(b);
            h1.core.lemma_apart(b);
        }
    }
}

/// `alloc_data` の変化。新しい物体は前に生きていなかった。
pub(super) proof fn lemma_alloc_moved<M: Mode>(h1: Heap<M>, h0: Heap<M>, r: Word)
    requires
        h0.consistent(),
        h0.settleable(),
        h1.made(h0, r),
    ensures
        moved(h1, h0, |b: usize| 0int, Set::empty()),
        no_free(h1, h0),
        exact(h1, h0, |b: usize| 0int),
        !live(h0, r.bits_spec()),
        live(h1, r.bits_spec()),
        h1.core.settled_view().ext(r.bits_spec()) == 1,
        h1.consistent(),
{
    let v0 = h0.core.settled_view();
    let v1 = h1.core.settled_view();
    let hr = r.bits_spec();
    h0.core.lemma_settled_view();
    assert(v1 == h1.core);
    if h0.core.region_has(hr) {
        h1.core.lemma_apart(hr);
    }
    assert forall|b: usize| #[trigger] live(h0, b) implies live(h1, b) && b != hr && h1.core.block(
        b,
    ) == h0.core.block(b) && h1.core.perm(b) == h0.core.perm(b) && (v0.dom().contains(b)
        ==> h1.core.model@.owner[b] == h0.core.model@.owner[b] && v1.ext(b) == v0.ext(b)) && (
    !v0.dom().contains(b) ==> h0.core.rc_of(b) == 0) by {
        lemma_view_live(h0, b);
        if v0.dom().contains(b) {
            assert(h1.core.dom().contains(b));
        } else {
            h0.core.lemma_apart(b);
            h0.core.lemma_region_at(b);
            assert(!h1.core.dom().contains(b));
        }
    }
    assert forall|b: usize| live(h0, b) && live(h1, b) implies #[trigger] same_head(h1, h0, b)
        && h1.core.perm(b).slots == h0.core.perm(b).slots by {
        assert(live(h0, b));
    }
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + 0 >= 1) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& v0.rc_of(b) != 0 ==> v1.ext(b) == v0.ext(b) + 0
    } by {
        if !v0.dom().contains(b) && v0.rc_of(b) != 0 {
            v0.lemma_region_at(b);
        }
    }
}

/// `set_field` の変化。書いた物体 `w` の語のほかは変えず、`v` の外の参照が辺に、前の語の辺が外の参照に移る。
pub(super) proof fn lemma_set_moved<M: Mode>(h1: Heap<M>, h0: Heap<M>, w: Word, i: int, v: Word)
    requires
        h0.consistent(),
        h0.settled(),
        w.is_ptr_spec(),
        h0.core.dom().contains(w.bits_spec()),
        h0.core.state(w.bits_spec()) == BlockState::Live,
        h0.scanned_at(w, i),
        h1.field_set(h0, w, i, v),
    ensures
        moved(
            h1,
            h0,
            |b: usize| points(h0.word_at(w, i), b) - points(v, b),
            Set::empty().insert(w.bits_spec()),
        ),
        no_free(h1, h0),
        no_alloc(h1, h0),
        exact(h1, h0, |b: usize| points(h0.word_at(w, i), b) - points(v, b)),
{
    let s0 = h0.core;
    let s1 = h1.core;
    let hw = w.bits_spec();
    let old_word = h0.word_at(w, i);
    let e0 = s0.model@.edges;
    let pos = (hw, i);
    assert(s0.settled_view() == s0);
    assert(s1.settled_view() == s1);
    reveal(Core::blocks_wf);
    reveal(Core::edges_wf);
    assert(s0.block_wf(hw));
    assert(s0.slot_word(s0.perm(hw), hw, i));
    if e0.dom().contains(pos) {
        assert(s0.edge_wf(pos));
    }
    // 辺の数の変化: i 語目の前の辺を外し、`v` がポインタなら足す

    let mid = e0.remove(pos);
    if e0.dom().contains(pos) {
        lemma_indeg_remove(e0, pos);
    } else {
        assert(mid =~= e0);
    }
    if v.is_ptr_spec() {
        lemma_indeg_insert(mid, pos, v.bits_spec());
    }
    assert forall|b: usize| #[trigger]
        indeg(s1.model@.edges, b) == indeg(e0, b) - points(old_word, b) + points(v, b) by {}
    assert forall|b: usize| #[trigger] live(h1, b) == live(h0, b) by {
        if s0.dom().contains(b) && b != hw {
            assert(s1.perms@[b] == s0.perms@[b]);
        }
    }
    assert forall|b: usize| live(h0, b) && live(h1, b) implies #[trigger] same_head(h1, h0, b) && (
    !Set::empty().insert(hw).contains(b) ==> h1.core.perm(b).slots == h0.core.perm(b).slots) by {
        if s0.dom().contains(b) && b != hw {
            assert(s1.perms@[b] == s0.perms@[b]);
        }
    }
    let d = |b: usize| points(old_word, b) - points(v, b);
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (s0.rc_of(b) == 0 || s0.ext(b) + d(b) >= 1) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& s0.rc_of(b) != 0 ==> s1.ext(b) == s0.ext(b) + d(b)
    } by {
        if s0.dom().contains(b) && b != hw {
            assert(s1.perms@[b] == s0.perms@[b]);
        }
        assert(indeg(s1.model@.edges, b) == indeg(e0, b) - points(old_word, b) + points(v, b));
    }
    assert forall|b: usize| #![trigger live(h0, b)] live(h0, b) && s0.rc_of(b) != 0 implies s1.ext(
        b,
    ) == s0.ext(b) + d(b) by {
        assert(indeg(s1.model@.edges, b) == indeg(e0, b) - points(old_word, b) + points(v, b));
    }
}

// `evv` のリストの補題
/// 燃料を増やしても、着いたリストの列は変わらない。
pub(super) proof fn lemma_path_mono<M: Mode>(h: Heap<M>, w: Word, f: nat, g: nat)
    requires
        path(h, w, f) is Some,
        f <= g,
    ensures
        path(h, w, g) == path(h, w, f),
    decreases f,
{
    if w.bits_spec() != 1 && f > 0 {
        lemma_path_mono(h, h.scanned_word(w, 1), (f - 1) as nat, (g - 1) as nat);
    }
}

/// ある燃料で着いたリストの列は、`seq_of` である。
pub(super) proof fn lemma_seq_of<M: Mode>(h: Heap<M>, w: Word, f: nat)
    requires
        path(h, w, f) is Some,
    ensures
        exists|g: nat| #[trigger] path(h, w, g) is Some,
        path(h, w, f) == Some(seq_of(h, w)),
{
    let g = choose|g: nat| #[trigger] path(h, w, g) is Some;
    if f <= g {
        lemma_path_mono(h, w, f, g);
    } else {
        lemma_path_mono(h, w, g, f);
    }
}

/// 正しいリストの先頭は、`()` か `dup` も `decref` もできる節点である。
pub(super) proof fn lemma_listed_head<M: Mode>(h: Heap<M>, w: Word, max: usize)
    requires
        listed(h, w, max),
    ensures
        dupable(h, w),
        held_word(h, w),
        w.bits_spec() != 1 ==> node_ok(h, w),
{
    let f = choose|f: nat| #[trigger] path(h, w, f) is Some;
    lemma_unit_bits();
}

/// 両方で読める語は、`dup` と `decref` の条件を保つ。
pub(super) proof fn lemma_word_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    x: Set<usize>,
    w: Word,
)
    requires
        moved(h1, h0, d, x),
        h0.readable(w),
        h1.readable(w),
    ensures
        dupable(h0, w) ==> dupable(h1, w),
        droppable(h0, w) ==> droppable(h1, w),
        w.is_ptr_spec() ==> h1.meta_of(w) == h0.meta_of(w),
{
    if w.is_ptr_spec() {
        assert(live(h0, w.bits_spec()) && live(h1, w.bits_spec()));
    }
}

/// evidence の形は、語を変えない変化で保たれる。入口の表が書いた物体なら、その表の形は呼ぶ側が示す。
pub(super) proof fn lemma_ev_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    x: Set<usize>,
    e: Word,
)
    requires
        moved(h1, h0, d, x),
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.settleable(),
        ev_ok(h0, e),
        h1.readable(e),
        !x.contains(e.bits_spec()),
        x.contains(h0.scanned_word(e, 0).bits_spec()) ==> table_ok(h1, h0.scanned_word(e, 0)),
    ensures
        ev_ok(h1, e),
        h1.tag_of(e) == h0.tag_of(e),
        forall|i: int| #[trigger] h1.scanned_word(e, i) == h0.scanned_word(e, i),
{
    let b = e.bits_spec();
    assert(live(h0, b) && live(h1, b));
    assert(same_head(h1, h0, b));
    assert(h1.core.perm(b).slots == h0.core.perm(b).slots);
    assert forall|i: int| #[trigger] h1.scanned_word(e, i) == h0.scanned_word(e, i) by {}
    assert(h1.count_of(e) == 3);
    let t = h0.scanned_word(e, 0);
    h1.lemma_readable_scanned(e, 0);
    h1.lemma_readable_scanned(e, 1);
    h1.lemma_readable_scanned(e, 2);
    h0.lemma_readable_scanned(e, 1);
    h0.lemma_readable_scanned(e, 2);
    lemma_word_kept(h1, h0, d, x, h0.scanned_word(e, 1));
    lemma_word_kept(h1, h0, d, x, h0.scanned_word(e, 2));
    if !x.contains(t.bits_spec()) {
        lemma_table_kept(h1, h0, d, x, t);
    }
}

/// 入口の表の形は、語を変えない変化で保たれる。
pub(super) proof fn lemma_table_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    x: Set<usize>,
    t: Word,
)
    requires
        moved(h1, h0, d, x),
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.settleable(),
        table_ok(h0, t),
        h1.readable(t),
        !x.contains(t.bits_spec()),
    ensures
        table_ok(h1, t),
        forall|i: int| #[trigger] h1.scanned_word(t, i) == h0.scanned_word(t, i),
{
    let b = t.bits_spec();
    assert(live(h0, b) && live(h1, b));
    assert(same_head(h1, h0, b));
    assert forall|i: int| #[trigger] h1.scanned_word(t, i) == h0.scanned_word(t, i) by {}
    assert forall|i: int| 0 <= i < h1.count_of(t) implies dupable(
        h1,
        #[trigger] h1.scanned_word(t, i),
    ) by {
        h0.lemma_readable_scanned(t, i);
        h1.lemma_readable_scanned(t, i);
        lemma_word_kept(h1, h0, d, x, h0.scanned_word(t, i));
    }
}

/// evidence `ev` の状態の欄だけを `v` に書いた変化。`ev` は evidence のままで、節点や入口の表は書いていない。
pub(super) proof fn lemma_state_written<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    ev: Word,
    v: Word,
)
    requires
        moved(h1, h0, d, Set::empty().insert(ev.bits_spec())),
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.settleable(),
        ev_ok(h0, ev),
        h1.readable(ev),
        forall|i: int| i != 1 ==> #[trigger] h1.scanned_word(ev, i) == h0.scanned_word(ev, i),
        h1.scanned_word(ev, 1) == v,
        held_word(h0, v),
    ensures
        ev_ok(h1, ev),
        forall|n: Word| #[trigger]
            node_ok(h0, n) ==> !Set::empty().insert(ev.bits_spec()).contains(n.bits_spec()),
        forall|e: Word|
            #![trigger ev_ok(h0, e)]
            ev_ok(h0, e) && h1.readable(e) && Set::empty().insert(ev.bits_spec()).contains(
                e.bits_spec(),
            ) ==> ev_ok(h1, e) && h1.tag_of(e) == h0.tag_of(e),
        forall|t: Word|
            #![trigger table_ok(h0, t)]
            table_ok(h0, t) && h1.readable(t) && Set::empty().insert(ev.bits_spec()).contains(
                t.bits_spec(),
            ) ==> table_ok(h1, t),
{
    let x = Set::<usize>::empty().insert(ev.bits_spec());
    let b = ev.bits_spec();
    assert(live(h0, b) && live(h1, b));
    assert(same_head(h1, h0, b));
    let t = h0.scanned_word(ev, 0);
    h1.lemma_readable_scanned(ev, 0);
    h1.lemma_readable_scanned(ev, 1);
    h1.lemma_readable_scanned(ev, 2);
    h0.lemma_readable_scanned(ev, 2);
    assert(h1.scanned_word(ev, 0) == t);
    assert(h1.scanned_word(ev, 2) == h0.scanned_word(ev, 2));
    assert(t.bits_spec() != b);
    lemma_table_kept(h1, h0, d, x, t);
    lemma_word_kept(h1, h0, d, x, v);
    lemma_word_kept(h1, h0, d, x, h0.scanned_word(ev, 2));
    assert(ev_ok(h1, ev));
    assert forall|e: Word|
        #![trigger ev_ok(h0, e)]
        ev_ok(h0, e) && h1.readable(e) && x.contains(e.bits_spec()) implies ev_ok(h1, e)
        && h1.tag_of(e) == h0.tag_of(e) by {}
}

/// 入口の表 `t` の欄 `op` だけを `c` に書いた変化。表は入口の表のままで、節点や evidence は書いていない。
pub(super) proof fn lemma_entry_written<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    t: Word,
    op: int,
    c: Word,
)
    requires
        moved(h1, h0, d, Set::empty().insert(t.bits_spec())),
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.settleable(),
        table_ok(h0, t),
        h1.readable(t),
        h1.count_of(t) == h0.count_of(t),
        forall|j: int| j != op ==> #[trigger] h1.scanned_word(t, j) == h0.scanned_word(t, j),
        h1.scanned_word(t, op) == c,
        dupable(h1, c),
    ensures
        table_ok(h1, t),
        forall|n: Word| #[trigger]
            node_ok(h0, n) ==> !Set::empty().insert(t.bits_spec()).contains(n.bits_spec()),
        forall|e: Word|
            #![trigger ev_ok(h0, e)]
            ev_ok(h0, e) && h1.readable(e) && Set::empty().insert(t.bits_spec()).contains(
                e.bits_spec(),
            ) ==> ev_ok(h1, e) && h1.tag_of(e) == h0.tag_of(e),
        forall|u: Word|
            #![trigger table_ok(h0, u)]
            table_ok(h0, u) && h1.readable(u) && Set::empty().insert(t.bits_spec()).contains(
                u.bits_spec(),
            ) ==> table_ok(h1, u),
{
    let x = Set::<usize>::empty().insert(t.bits_spec());
    let b = t.bits_spec();
    assert(live(h0, b) && live(h1, b));
    assert(same_head(h1, h0, b));
    assert forall|j: int| 0 <= j < h1.count_of(t) implies dupable(
        h1,
        #[trigger] h1.scanned_word(t, j),
    ) by {
        if j != op {
            h0.lemma_readable_scanned(t, j);
            h1.lemma_readable_scanned(t, j);
            lemma_word_kept(h1, h0, d, x, h0.scanned_word(t, j));
        }
    }
    assert(table_ok(h1, t));
    assert forall|u: Word|
        #![trigger table_ok(h0, u)]
        table_ok(h0, u) && h1.readable(u) && x.contains(u.bits_spec()) implies table_ok(h1, u) by {
        assert forall|j: int| 0 <= j < h1.count_of(u) implies dupable(
            h1,
            #[trigger] h1.scanned_word(u, j),
        ) by {
            assert(h1.scanned_word(u, j) == h1.scanned_word(t, j));
        }
    }
}

/// リストの節点と evidence が保たれる変化では、リストの列も変わらない。書いた物体 `x` は節点でなく、evidence か
/// 入口の表なら、その形は呼ぶ側が示す。
pub(super) proof fn lemma_path_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    x: Set<usize>,
    w: Word,
    f: nat,
)
    requires
        moved(h1, h0, d, x),
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.settleable(),
        path(h0, w, f) is Some,
        h1.readable(w),
        forall|n: Word| #[trigger] node_ok(h0, n) ==> !x.contains(n.bits_spec()),
        forall|e: Word|
            #![trigger ev_ok(h0, e)]
            ev_ok(h0, e) && h1.readable(e) && x.contains(e.bits_spec()) ==> ev_ok(h1, e)
                && h1.tag_of(e) == h0.tag_of(e),
        forall|t: Word|
            #![trigger table_ok(h0, t)]
            table_ok(h0, t) && h1.readable(t) && x.contains(t.bits_spec()) ==> table_ok(h1, t),
    ensures
        path(h1, w, f) == path(h0, w, f),
    decreases f,
{
    if w.bits_spec() != 1 && f > 0 {
        let b = w.bits_spec();
        let e = h0.scanned_word(w, 0);
        let n = h0.scanned_word(w, 1);
        assert(node_ok(h0, w));
        assert(live(h0, b) && live(h1, b));
        assert(same_head(h1, h0, b));
        assert(!x.contains(b));
        assert(h1.scanned_word(w, 0) == e);
        assert(h1.scanned_word(w, 1) == n);
        assert(node_ok(h1, w));
        h1.lemma_readable_scanned(w, 0);
        h1.lemma_readable_scanned(w, 1);
        if x.contains(e.bits_spec()) {
            assert(ev_ok(h1, e));
        } else {
            let t = h0.scanned_word(e, 0);
            if x.contains(t.bits_spec()) {
                assert(live(h0, e.bits_spec()) && live(h1, e.bits_spec()));
                assert(same_head(h1, h0, e.bits_spec()));
                assert(h1.scanned_word(e, 0) == t);
                h1.lemma_readable_scanned(e, 0);
                assert(table_ok(h0, t));
            }
            lemma_ev_kept(h1, h0, d, x, e);
        }
        lemma_path_kept(h1, h0, d, x, n, (f - 1) as nat);
    }
}

/// `lemma_path_kept` の、正しいリストについての形。
pub(super) proof fn lemma_listed_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    x: Set<usize>,
    w: Word,
    max: usize,
)
    requires
        moved(h1, h0, d, x),
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.settleable(),
        listed(h0, w, max),
        h1.readable(w),
        forall|n: Word| #[trigger] node_ok(h0, n) ==> !x.contains(n.bits_spec()),
        forall|e: Word|
            #![trigger ev_ok(h0, e)]
            ev_ok(h0, e) && h1.readable(e) && x.contains(e.bits_spec()) ==> ev_ok(h1, e)
                && h1.tag_of(e) == h0.tag_of(e),
        forall|t: Word|
            #![trigger table_ok(h0, t)]
            table_ok(h0, t) && h1.readable(t) && x.contains(t.bits_spec()) ==> table_ok(h1, t),
    ensures
        listed(h1, w, max),
        seq_of(h1, w) == seq_of(h0, w),
{
    let f = choose|f: nat| #[trigger] path(h0, w, f) is Some;
    lemma_seq_of(h0, w, f);
    lemma_path_kept(h1, h0, d, x, w, f);
    lemma_seq_of(h1, w, f);
}

impl<M: Mode> Ctx<M> {
    /// ヒープを `moved(d, x)` で変えた後も、`evv` は同じ列の正しいリストである。`x` は空か、`evv` から読めない物体
    /// である。
    pub(super) proof fn lemma_evv_kept(&self, old: Ctx<M>, d: spec_fn(usize) -> int, x: Set<usize>)
        requires
            old.held_ok(),
            self.heap.consistent(),
            self.heap.settleable(),
            self.holds_args(seq![]),
            self.evv == old.evv,
            self.max_evv == old.max_evv,
            moved(self.heap, old.heap, d, x),
            forall|b: usize| #[trigger] x.contains(b) ==> !live(old.heap, b),
        ensures
            self.held_ok(),
            self.evv_seq(self.evv) == old.evv_seq(old.evv),
    {
        lemma_listed_kept(self.heap, old.heap, d, x, self.evv, self.max_evv);
    }
}

} // verus!

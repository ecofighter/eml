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

    /// ダミーの物体は物体を指す `word_in` の語である。yield していない速いモードの文脈は、ダミーと2つの `()` を
    /// 宛先、節、κ として持てる。ダミーの rc は 0 なので、`owned` と数の条件は効かない。
    pub(super) proof fn lemma_dummy_held(&self, unit: Word)
        requires
            self.wf(),
            unit.bits_spec() == 1,
        ensures
            self.heap_spec().word_in(self.heap_spec().dummy_word()),
            self.heap_spec().dummy_word().is_ptr_spec(),
            !M::CHECKED && !self.is_yielding() ==> self.holds_args(
                seq![self.heap_spec().dummy_word(), unit, unit],
            ),
    {
        lemma_unit_bits();
        let d = self.heap.dummy_word();
        if !M::CHECKED && !self.is_yielding() {
            let ws = seq![d, unit, unit];
            let v = self.heap.core.settled_view();
            assert(self.heap.dummy_wf());
            assert(v.rc_of(d.bits_spec()) == 0);
            assert forall|i: int| 0 <= i < ws.len() implies held_word(
                self.heap,
                #[trigger] ws[i],
            ) by {}
            assert forall|b: usize| #[trigger]
                self.need(ws, b) > 0 && v.rc_of(b) != 0 implies v.ext(b) >= self.need(ws, b) by {
                lemma_points_in(ws, b);
                lemma_points_in(seq![], b);
                assert(self.need(ws, b) == self.need(seq![], b));
            }
        }
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
        lemma_ev_field_written(h2, h0, d, ev, 1, unit);
        lemma_listed_kept(h2, h0, d, x, self.evv, self.max_evv);
    }

    /// `ev_take_evv` の、モデルが合っている道。外側の欄は `dupable` で、`droppable` より強いので、取り出した語を
    /// 呼ぶ側が持てる。
    pub(super) proof fn lemma_take_evv_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        ev: Word,
        w: Word,
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
            h1.scanned_at(ev, 2),
            h1.scanned_word(ev, 2) == h1.word_at(ev, 2),
            w == h1.word_at(ev, 2),
            unit.bits_spec() == 1,
            self.heap.field_set(h1, ev, 2, unit),
            self.heap.consistent(),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            w == s0.ev_outer(ev),
            self.ev_outer(ev).bits_spec() == 1,
            self.ev_table(ev) == s0.ev_table(ev),
            self.ev_state(ev) == s0.ev_state(ev),
            self.held_ok(),
            self.holds(w),
            self.ev_shape(ev),
            self.evolved_by(s0, |b: usize| points(w, b), Set::empty().insert(ev.bits_spec())),
    {
        let h0 = s0.heap;
        let h2 = self.heap;
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let x = none.insert(ev.bits_spec());
        let d = |b: usize| points(w, b);
        lemma_unit_bits();
        lemma_settle_moved(h1, h0);
        lemma_set_moved(h2, h1, ev, 2, unit);
        lemma_moved_chain(
            h2,
            h1,
            h0,
            z,
            |b: usize| points(h1.word_at(ev, 2), b) - points(unit, b),
            d,
            none,
            x,
            x,
        );
        lemma_unextended(h1, ev);
        lemma_unextended(h2, ev);
        assert forall|i: int| i != 2 implies #[trigger] h2.scanned_word(ev, i) == h0.scanned_word(
            ev,
            i,
        ) by {
            assert(h2.word_at(ev, i) == h1.word_at(ev, i));
        }
        assert(h2.scanned_word(ev, 2) == unit);
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(seq![], b) + d(
            b,
        ) >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
        }
        self.lemma_holds_moved(s0, seq![], seq![], d, x);
        h0.lemma_readable_scanned(ev, 2);
        self.lemma_dupped_holds(s0, w, x);
        lemma_readable_kept(h1, h0, ev);
        lemma_readable_kept(h2, h1, ev);
        lemma_ev_field_written(h2, h0, d, ev, 2, unit);
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
        lemma_ev_field_written(h3, h0, d, ev, 1, s);
        lemma_listed_kept(h3, h0, d, x, self.evv, self.max_evv);
    }

    /// エントリの表の欄 `op` を書く物理的な位置は `i` で、その欄は書ける。
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

/// `ws` が4語までのとき、物体を指す語の数を語ごとに足す。
pub(super) proof fn lemma_points_in(ws: Seq<Word>, b: usize)
    ensures
        ws.len() == 0 ==> points_in(ws, b) == 0,
        ws.len() == 1 ==> points_in(ws, b) == points(ws[0], b),
        ws.len() == 2 ==> points_in(ws, b) == points(ws[0], b) + points(ws[1], b),
        ws.len() == 3 ==> points_in(ws, b) == points(ws[0], b) + points(ws[1], b) + points(
            ws[2],
            b,
        ),
        ws.len() == 4 ==> points_in(ws, b) == points(ws[0], b) + points(ws[1], b) + points(ws[2], b)
            + points(ws[3], b),
{
    reveal_with_fuel(points_in, 5);
    if ws.len() >= 1 {
        let a = ws.drop_last();
        if ws.len() >= 2 {
            assert(a[0] == ws[0]);
            let c = a.drop_last();
            if ws.len() >= 3 {
                assert(a[1] == ws[1]);
                assert(c[0] == ws[0]);
                let e = c.drop_last();
                if ws.len() == 4 {
                    assert(a[2] == ws[2]);
                    assert(c[1] == ws[1]);
                    assert(e[0] == ws[0]);
                }
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

/// 取り込みの変化。空けたことにした集合 `r` の物体は外の参照をみな失い、ほかの物体は外の参照の数が辺の減った分だけ
/// 増える。
pub(super) proof fn lemma_reclaim_moved<M: Mode>(
    h2: Heap<M>,
    h1: Heap<M>,
    r: Set<usize>,
    d: spec_fn(usize) -> int,
)
    requires
        h1.wf(),
        h1.core.model@.building is None,
        h2.core.model@.building is None,
        h2.dummy == h1.dummy,
        h2.region == h1.region,
        h2.worker == h1.worker,
        h2.core.env == h1.core.env,
        h2.core.dom().subset_of(h1.core.dom()),
        forall|b: usize|
            #![trigger h1.core.dom().contains(b)]
            h1.core.dom().contains(b) && !r.contains(b) ==> {
                &&& h2.core.dom().contains(b)
                &&& h2.core.state(b) == h1.core.state(b)
                &&& h2.core.perm(b) == h1.core.perm(b)
                &&& h2.core.block(b) == h1.core.block(b)
                &&& h2.core.model@.refs[b] == h1.core.model@.refs[b]
                &&& h2.core.model@.owner[b] == h1.core.model@.owner[b]
            },
        forall|b: usize|
            #![trigger r.contains(b)]
            r.contains(b) ==> !(h2.core.has(b) && h2.core.settled_state(b) == BlockState::Live),
        forall|b: usize|
            #![trigger r.contains(b)]
            r.contains(b) ==> h1.core.dom().contains(b) && (h1.core.state(b) == BlockState::Live
                ==> h1.core.rc_of(b) != 0),
        forall|b: usize| #[trigger]
            d(b) == if r.contains(b) {
                -h1.core.ext(b)
            } else if h1.core.dom().contains(b) {
                h2.core.ext(b) - h1.core.ext(b)
            } else {
                0int
            },
    ensures
        moved(h2, h1, d, Set::empty()),
{
    assert(h1.core.settled_view() == h1.core);
    assert(h2.core.settled_view() == h2.core);
    assert forall|b: usize| #![trigger live(h1, b)] live(h1, b) && live(h2, b) implies same_head(
        h2,
        h1,
        b,
    ) && h2.core.perm(b).slots == h1.core.perm(b).slots by {
        if r.contains(b) {
            assert(!(h2.core.has(b) && h2.core.settled_state(b) == BlockState::Live));
        }
        if h1.core.dom().contains(b) {
            assert(h2.core.perm(b) == h1.core.perm(b));
        } else {
            assert(!h2.core.dom().contains(b));
            assert(h2.core.in_region(b) && h1.core.in_region(b));
            assert(h2.core.region() == h1.core.region());
            h1.core.lemma_region_at(b);
        }
    }
    assert forall|b: usize|
        #![trigger live(h1, b)]
        live(h1, b) && (h1.core.rc_of(b) == 0 || h1.core.ext(b) + d(b) >= 1) implies {
        &&& live(h2, b)
        &&& h2.core.block(b) == h1.core.block(b)
        &&& h1.core.rc_of(b) != 0 ==> h2.core.ext(b) == h1.core.ext(b) + d(b)
    } by {
        assert(d(b) == if r.contains(b) {
            -h1.core.ext(b)
        } else if h1.core.dom().contains(b) {
            h2.core.ext(b) - h1.core.ext(b)
        } else {
            0int
        });
        if r.contains(b) {
            assert(h1.core.state(b) == BlockState::Live);
            assert(h1.core.rc_of(b) != 0);
        } else if h1.core.dom().contains(b) {
            assert(h2.core.dom().contains(b));
            assert(h2.core.state(b) == h1.core.state(b));
        } else {
            assert(!h2.core.dom().contains(b));
            assert(h2.core.in_region(b) && h1.core.in_region(b));
            assert(h2.core.region() == h1.core.region());
            h1.core.lemma_region_at(b);
        }
    }
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

/// evidence の形は、語を変えない変化で保たれる。エントリの表が書いた物体なら、その表の形は呼ぶ側が示す。
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

/// エントリの表の形は、語を変えない変化で保たれる。
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

/// evidence `ev` の状態の欄 (`f` = 1) か外側の欄 (`f` = 2) だけを `v` に書いた変化。`ev` は evidence のままで、
/// 節点やエントリの表は書いていない。
pub(super) proof fn lemma_ev_field_written<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    ev: Word,
    f: int,
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
        f == 1 || f == 2,
        forall|i: int| i != f ==> #[trigger] h1.scanned_word(ev, i) == h0.scanned_word(ev, i),
        h1.scanned_word(ev, f) == v,
        held_word(h0, v),
        f == 2 ==> dupable(h0, v),
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
    let k = 3 - f;
    h1.lemma_readable_scanned(ev, 0);
    h1.lemma_readable_scanned(ev, 1);
    h1.lemma_readable_scanned(ev, 2);
    h0.lemma_readable_scanned(ev, k);
    assert(h1.scanned_word(ev, 0) == t);
    assert(h1.scanned_word(ev, k) == h0.scanned_word(ev, k));
    assert(t.bits_spec() != b);
    lemma_table_kept(h1, h0, d, x, t);
    lemma_word_kept(h1, h0, d, x, v);
    lemma_word_kept(h1, h0, d, x, h0.scanned_word(ev, k));
    assert(ev_ok(h1, ev));
    assert forall|e: Word|
        #![trigger ev_ok(h0, e)]
        ev_ok(h0, e) && h1.readable(e) && x.contains(e.bits_spec()) implies ev_ok(h1, e)
        && h1.tag_of(e) == h0.tag_of(e) by {}
}

/// エントリの表 `t` の欄 `op` だけを `c` に書いた変化。表はエントリの表のままで、節点や evidence は書いていない。
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
/// エントリの表なら、その形は呼ぶ側が示す。
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
    /// `reclaim_when` の後の文脈。`c1` は `c0` のヒープの構築中の物体を生きている物体にしたもので、`self` はそこから
    /// `Ctx` の持つ物体 (`held`) を空けたことにせずに取り込んだものである。
    pub(super) proof fn lemma_reclaimed_ctx(&self, c1: Ctx<M>, c0: Ctx<M>, held: Set<usize>)
        requires
            c0.wf(),
            c1.heap == c0.heap.settled_heap(),
            c1.heap.wf(),
            c1.heap.core.model@.building is None,
            forall|w: Word| #[trigger] c1.heap.core.word_ok(w) == c0.heap.core.word_ok(w),
            c1 == (Ctx { heap: c1.heap, ..c0 }),
            *self == (Ctx { heap: self.heap, ..c1 }),
            self.heap.wf(),
            self.heap.reclaimed(c1.heap, held),
            held == c0.held_set(),
        ensures
            self.wf(),
            self.evv_spec() == c0.evv_spec(),
            self.yield_spec() == c0.yield_spec(),
    {
        let h0 = c0.heap;
        let h1 = c1.heap;
        let h2 = self.heap;
        let r = h2.lemma_reclaimed(h1, held);
        assert(h2.core.model@.building is None);
        assert forall|w: Word|
            w == c0.evv || w == c0.yielding.target || w == c0.yielding.clause || w
                == c0.yielding.cont implies #[trigger] h2.word_in(w) by {
            assert(held.contains(w.bits_spec()));
            assert(h1.core.word_ok(w));
        }
        if !M::CHECKED {
            let d = |b: usize|
                if r.contains(b) {
                    -h1.core.ext(b)
                } else if h1.core.dom().contains(b) {
                    h2.core.ext(b) - h1.core.ext(b)
                } else {
                    0int
                };
            lemma_settle_moved(h1, h0);
            lemma_reclaim_moved(h2, h1, r, d);
            lemma_moved_chain(
                h2,
                h1,
                h0,
                |b: usize| 0int,
                d,
                d,
                Set::empty(),
                Set::empty(),
                Set::empty(),
            );
            assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies c0.need(seq![], b)
                + d(b) >= self.need(seq![], b) by {
                assert(held.contains(b));
                if h1.core.dom().contains(b) && !r.contains(b) {
                    assert(h2.core.indeg(b) <= h1.core.indeg(b));
                }
            }
            self.lemma_holds_moved(c0, seq![], seq![], d, Set::empty());
            self.lemma_evv_kept(c0, d, Set::empty());
        }
    }

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

// `evv_mask` の部品。写しの途中のヒープの変化を `kept_for` でつなぎ、たどる節点が読めるままであることを言う
pub(super) proof fn lemma_kept_refl<M: Mode>(h: Heap<M>)
    ensures
        kept_for(h, h, Set::empty()),
{
}

pub(super) proof fn lemma_kept_trans<M: Mode>(
    h2: Heap<M>,
    h1: Heap<M>,
    h0: Heap<M>,
    x1: Set<usize>,
    x2: Set<usize>,
    x: Set<usize>,
)
    requires
        kept_for(h1, h0, x1),
        kept_for(h2, h1, x2),
        forall|b: usize|
            (x1.contains(b) || x2.contains(b)) && live(h0, b) ==> #[trigger] x.contains(b),
    ensures
        kept_for(h2, h0, x),
{
    assert forall|b: usize| #![trigger live(h0, b)] live(h0, b) implies {
        &&& live(h2, b)
        &&& h2.core.block(b) == h0.core.block(b)
        &&& same_head(h2, h0, b)
        &&& !x.contains(b) ==> h2.core.perm(b).slots == h0.core.perm(b).slots
    } by {
        assert(live(h1, b));
    }
}

/// `dup` の変化。rc と前置きの数のほかは変えず、物体を空けない。
pub(super) proof fn lemma_kept_dup<M: Mode>(h1: Heap<M>, h0: Heap<M>)
    requires
        h1.values_moved(h0),
        h1.settled(),
        h1.kept_live(h0),
        M::CHECKED ==> forall|v: Word| #[trigger] h0.dup_passes(v) ==> h1.dup_passes(v),
    ensures
        kept_for(h1, h0, Set::empty()),
{
    let v0 = h0.core.settled_view();
    assert forall|b: usize| #![trigger live(h0, b)] live(h0, b) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& same_head(h1, h0, b)
        &&& h1.core.perm(b).slots == h0.core.perm(b).slots
    } by {
        assert(v0.dom() == h0.core.dom());
        assert(h1.core.has(b));
        assert(h1.core.settled_state(b) == BlockState::Live);
    }
}

/// 確保の変化。前の物体には触れない。
pub(super) proof fn lemma_kept_alloc<M: Mode>(h1: Heap<M>, h0: Heap<M>, c: Word)
    requires
        h1.grew(h0, c),
        h1.made(h0, c),
    ensures
        kept_for(h1, h0, Set::empty()),
        !live(h0, c.bits_spec()),
        live(h1, c.bits_spec()),
{
    assert forall|b: usize| #![trigger live(h0, b)] live(h0, b) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& same_head(h1, h0, b)
        &&& h1.core.perm(b).slots == h0.core.perm(b).slots
    } by {
        assert(h1.grown_block(h0, b));
    }
    if M::CHECKED {
        assert forall|v: Word| #[trigger] h0.dup_passes(v) implies h1.dup_passes(v) by {
            if v.is_ptr_spec() {
                assert(live(h0, v.bits_spec()));
                assert(h1.grown_block(h0, v.bits_spec()));
            }
        }
    }
}

/// `set_field` の変化。書いた物体の語のほかは変えない。
pub(super) proof fn lemma_kept_set<M: Mode>(h1: Heap<M>, h0: Heap<M>, w: Word, i: int, v: Word)
    requires
        h1.field_set(h0, w, i, v),
    ensures
        kept_for(h1, h0, Set::empty().insert(w.bits_spec())),
        h1.settled() == h0.settled(),
        forall|j: int| j != i ==> #[trigger] h1.word_at(w, j) == h0.word_at(w, j),
        h1.word_at(w, i) == v,
        h1.core.perm(w.bits_spec()).meta == h0.core.perm(w.bits_spec()).meta,
{
    let hw = w.bits_spec();
    assert forall|b: usize| #[trigger]
        h1.core.perm(b).meta == h0.core.perm(b).meta && h1.core.perm(b).rc == h0.core.perm(b).rc
            && h1.core.perm(b).prefix == h0.core.perm(b).prefix by {
        if b != hw {
            if h0.core.in_region(b) {
            } else {
                assert(h1.core.perms@[b] == h0.core.perms@[b]);
            }
        }
    }
    assert forall|b: usize| #![trigger live(h0, b)] live(h0, b) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& same_head(h1, h0, b)
        &&& !Set::empty().insert(hw).contains(b) ==> h1.core.perm(b).slots == h0.core.perm(b).slots
    } by {
        assert(h1.core.perm(b).meta == h0.core.perm(b).meta);
        assert(h1.core.perm(b).rc == h0.core.perm(b).rc);
        if b != hw && !h0.core.in_region(b) {
            assert(h1.core.perms@[b] == h0.core.perms@[b]);
        }
    }
    if M::CHECKED {
        assert forall|u: Word| #[trigger] h0.dup_passes(u) implies h1.dup_passes(u) by {
            if u.is_ptr_spec() {
                assert(h1.core.perm(u.bits_spec()).meta == h0.core.perm(u.bits_spec()).meta);
                assert(h1.core.perm(u.bits_spec()).prefix == h0.core.perm(u.bits_spec()).prefix);
            }
        }
    }
}

/// 検査モードで、変化の後も `set_field` の確かめが通る。`dup` の確かめが通る物体は、持ち主が変わらない。
pub(super) proof fn lemma_set_passes_moved<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    x: Set<usize>,
    u: Word,
    j: int,
)
    requires
        M::CHECKED,
        kept_for(h1, h0, x),
        h0.set_passes(u, j),
        h0.dup_passes(u),
        !x.contains(u.bits_spec()),
        h1.settled(),
    ensures
        h1.set_passes(u, j),
        h1.dup_passes(u),
{
    assert(live(h0, u.bits_spec()));
}

/// 確保したばかりの物体は、`lin` ビットがなければ `dup` の確かめも通る。
pub(super) proof fn lemma_fresh_dup_passes<M: Mode>(h: Heap<M>, c: Word)
    requires
        h.set_passes(c, 0),
        !h.lin_of(c),
    ensures
        h.dup_passes(c),
{
}

/// 物体を空けない変化の後も、読める節点は同じ形と語を持つ。
pub(super) proof fn lemma_node_kept<M: Mode>(h1: Heap<M>, h0: Heap<M>, x: Set<usize>, n: Word)
    requires
        kept_for(h1, h0, x),
        n.is_ptr_spec(),
        h0.readable(n),
    ensures
        h1.readable(n),
        h1.meta_of(n) == h0.meta_of(n),
        h1.tag_of(n) == h0.tag_of(n),
        h1.scan_of(n) == h0.scan_of(n),
        h1.kind_of(n) == h0.kind_of(n),
        h1.lin_of(n) == h0.lin_of(n),
        !x.contains(n.bits_spec()) ==> h1.count_of(n) == h0.count_of(n),
        h0.owned(n) ==> h1.owned(n),
        (h1.rc_value(n) > 0) == (h0.rc_value(n) > 0),
        (h1.rc_value(n) < 0) == (h0.rc_value(n) < 0),
        !x.contains(n.bits_spec()) ==> forall|i: int| #[trigger]
            h1.scanned_word(n, i) == h0.scanned_word(n, i),
        !x.contains(n.bits_spec()) ==> forall|i: int| #[trigger]
            h1.word_at(n, i) == h0.word_at(n, i),
{
    let b = n.bits_spec();
    assert(live(h0, b));
    assert(live(h1, b));
    assert(same_head(h1, h0, b));
}

/// 物体を空けない変化の後も、`w` からたどる節点は同じで、読める。
pub(super) proof fn lemma_walk_kept<M: Mode>(h1: Heap<M>, h0: Heap<M>, w: Word, k: nat)
    requires
        kept_for(h1, h0, Set::empty()),
        walkable(h0, w, k),
    ensures
        walkable(h1, w, k),
        agree(h1, h0, w, k),
    decreases k,
{
    if k > 0 {
        let k1 = (k - 1) as nat;
        assert(walkable(h0, w, k1));
        lemma_walk_kept(h1, h0, w, k1);
        let n = nth(h0, w, k1);
        assert(walk_node(h0, n));
        lemma_node_kept(h1, h0, Set::empty(), n);
        assert(nth(h1, w, k1) == n);
        assert(nth(h1, w, k) == h1.scanned_word(n, 1));
        assert(nth(h0, w, k) == h0.scanned_word(n, 1));
        assert forall|j: nat| j < k implies walk_node(h1, #[trigger] nth(h1, w, j)) by {
            if j < k1 {
            } else {
                assert(j == k1);
            }
        }
        assert forall|j: nat| j <= k implies {
            &&& #[trigger] nth(h1, w, j) == nth(h0, w, j)
            &&& j < k ==> h1.tag_of(nth(h1, w, j)) == h0.tag_of(nth(h0, w, j))
            &&& j < k ==> h1.scanned_word(nth(h1, w, j), 0) == h0.scanned_word(nth(h0, w, j), 0)
        } by {
            if j <= k1 {
            }
        }
    } else {
        assert(nth(h1, w, 0) == w);
        assert(nth(h0, w, 0) == w);
    }
}

/// 写しの列は、写しを書かない変化の後も同じ形である。
pub(super) proof fn lemma_copies_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    x: Set<usize>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
)
    requires
        kept_for(h1, h0, x),
        copies_ok(h0, cs, out),
        forall|k: int| 0 <= k < cs.len() ==> !x.contains(#[trigger] cs[k].bits_spec()),
    ensures
        copies_ok(h1, cs, out),
{
    assert forall|k: int| 0 <= k < cs.len() implies {
        &&& node_ok(h1, #[trigger] cs[k])
        &&& h1.rc_value(cs[k]) > 0
        &&& h1.tag_of(cs[k]) == out[k].0
        &&& h1.scanned_word(cs[k], 0) == out[k].1
        &&& k + 1 < cs.len() ==> h1.scanned_word(cs[k], 1) == cs[k + 1]
    } by {
        assert(!x.contains(cs[k].bits_spec()));
        lemma_node_kept(h1, h0, x, cs[k]);
    }
}

/// 着いたリストの1歩。列が空なのは `()` のときだけで、空でなければ先頭の節点の組と、次の節点からの列に分かれる。
pub(super) proof fn lemma_path_step<M: Mode>(h: Heap<M>, n: Word, g: nat, r: Seq<(u16, Word)>)
    requires
        path(h, n, g) == Some(r),
    ensures
        (r.len() == 0) == (n.bits_spec() == 1),
        r.len() > 0 ==> {
            &&& g > 0
            &&& node_ok(h, n)
            &&& ev_ok(h, h.scanned_word(n, 0))
            &&& h.tag_of(h.scanned_word(n, 0)) == h.tag_of(n)
            &&& r[0] == (h.tag_of(n), h.scanned_word(n, 0))
            &&& path(h, h.scanned_word(n, 1), (g - 1) as nat) == Some(r.drop_first())
        },
{
    if n.bits_spec() != 1 {
        let next = path(h, h.scanned_word(n, 1), (g - 1) as nat);
        let x = (h.tag_of(n), h.scanned_word(n, 0));
        assert(r =~= seq![x] + next->0);
        assert((seq![x] + next->0).drop_first() =~= next->0);
    }
}

/// 着いたリストの `j` 個目の節点。そこからの列は元の列の後ろで、節点の組は元の列の `j` 個目の組である。
pub(super) proof fn lemma_path_nth<M: Mode>(
    h: Heap<M>,
    w: Word,
    f: nat,
    s: Seq<(u16, Word)>,
    j: nat,
)
    requires
        path(h, w, f) == Some(s),
        j <= s.len(),
    ensures
        j <= f,
        path(h, nth(h, w, j), (f - j) as nat) == Some(s.subrange(j as int, s.len() as int)),
        j < s.len() ==> {
            &&& node_ok(h, nth(h, w, j))
            &&& walk_node(h, nth(h, w, j))
            &&& ev_ok(h, s[j as int].1)
            &&& s[j as int] == (h.tag_of(nth(h, w, j)), h.scanned_word(nth(h, w, j), 0))
            &&& h.tag_of(s[j as int].1) == s[j as int].0
        },
        j == s.len() ==> nth(h, w, j).bits_spec() == 1,
    decreases j,
{
    if j == 0 {
        assert(s.subrange(0, s.len() as int) =~= s);
    } else {
        let j1 = (j - 1) as nat;
        lemma_path_nth(h, w, f, s, j1);
        let n = nth(h, w, j1);
        let r = s.subrange(j1 as int, s.len() as int);
        lemma_path_step(h, n, (f - j1) as nat, r);
        assert(r.drop_first() =~= s.subrange(j as int, s.len() as int));
    }
    let r = s.subrange(j as int, s.len() as int);
    lemma_path_step(h, nth(h, w, j), (f - j) as nat, r);
    if j < s.len() {
        assert(r[0] == s[j as int]);
        lemma_unextended(h, nth(h, w, j));
    }
}

/// 着いたリストの節点は、どれも読める。
pub(super) proof fn lemma_path_walkable<M: Mode>(h: Heap<M>, w: Word, f: nat, s: Seq<(u16, Word)>)
    requires
        path(h, w, f) == Some(s),
    ensures
        walkable(h, w, s.len()),
        forall|j: int| 0 <= j < s.len() ==> #[trigger] s[j].0 == h.tag_of(nth(h, w, j as nat)),
{
    assert forall|j: nat| j < s.len() implies walk_node(h, #[trigger] nth(h, w, j)) by {
        lemma_path_nth(h, w, f, s, j);
    }
    assert forall|j: int| 0 <= j < s.len() implies #[trigger] s[j].0 == h.tag_of(
        nth(h, w, j as nat),
    ) by {
        lemma_path_nth(h, w, f, s, j as nat);
    }
}

/// 先頭の `j + 1` 個の項目の `skip_total` は、先頭の `j` 個のものに項目 `j` の分を足した数である。
pub(super) proof fn lemma_skip_step(items: Seq<MaskItem>, j: int)
    requires
        0 <= j < items.len(),
    ensures
        skip_total(items.subrange(0, j + 1)) == skip_total(items.subrange(0, j)) + if repeated(
            items,
            j,
        ) {
            0int
        } else {
            items[j].skip as int
        },
{
    let p = items.subrange(0, j + 1);
    assert(p.drop_last() =~= items.subrange(0, j));
    if repeated(items, j) {
        let k = choose|k: int| 0 <= k < j && #[trigger] items[k].effect == items[j].effect;
        assert(p[k].effect == p[j].effect);
    }
    if repeated(p, j) {
        let k = choose|k: int| 0 <= k < j && #[trigger] p[k].effect == p[j].effect;
        assert(items[k].effect == items[j].effect);
    }
}

/// 先頭の `j` 個の項目の `skip_total` は、全体の `skip_total` を超えない。
pub(super) proof fn lemma_skip_prefix(items: Seq<MaskItem>, j: int)
    requires
        0 <= j <= items.len(),
    ensures
        skip_total(items.subrange(0, j)) <= skip_total(items),
    decreases items.len() - j,
{
    if j < items.len() {
        lemma_skip_step(items, j);
        lemma_skip_prefix(items, j + 1);
    } else {
        assert(items.subrange(0, j) =~= items);
    }
}

/// 組の列の tag が節点の tag と同じなら、数える数も同じである。
pub(super) proof fn lemma_seen_eq<M: Mode>(s: Seq<(u16, Word)>, h: Heap<M>, w: Word, i: nat, t: u16)
    requires
        i <= s.len(),
        forall|j: int| 0 <= j < s.len() ==> #[trigger] s[j].0 == h.tag_of(nth(h, w, j as nat)),
    ensures
        seen(s, i as int, t) == wseen(h, w, i, t),
    decreases i,
{
    if i > 0 {
        lemma_seen_eq(s, h, w, (i - 1) as nat, t);
        assert(s[i - 1].0 == h.tag_of(nth(h, w, (i - 1) as nat)));
    }
}

/// 組の列とヒープの上のたどりで、mask の残す組は同じである。
pub(super) proof fn lemma_kept_eq<M: Mode>(
    items: Seq<MaskItem>,
    s: Seq<(u16, Word)>,
    h: Heap<M>,
    w: Word,
    i: nat,
)
    requires
        i < s.len(),
        forall|j: int| 0 <= j < s.len() ==> #[trigger] s[j].0 == h.tag_of(nth(h, w, j as nat)),
    ensures
        kept_at(items, s, i as int) == wkept(items, h, w, i),
{
    assert(s[i as int].0 == h.tag_of(nth(h, w, i)));
    lemma_seen_eq(s, h, w, i, s[i as int].0);
}

/// 組 `i` から後ろで残す組の数と除く組の数を足すと、後ろの組の数になる。
pub(super) proof fn lemma_mask_count<M: Mode>(
    items: Seq<MaskItem>,
    s: Seq<(u16, Word)>,
    h: Heap<M>,
    w: Word,
    i: nat,
)
    requires
        i <= s.len(),
        forall|j: int| 0 <= j < s.len() ==> #[trigger] s[j].0 == h.tag_of(nth(h, w, j as nat)),
    ensures
        mask_from(items, s, i as int).len() + wdropped(items, h, w, s.len()) - wdropped(
            items,
            h,
            w,
            i,
        ) == s.len() - i,
    decreases s.len() - i,
{
    if i < s.len() {
        lemma_mask_count(items, s, h, w, i + 1);
        lemma_kept_eq(items, s, h, w, i);
        assert(wdropped(items, h, w, i + 1) == wdropped(items, h, w, i) + if wkept(items, h, w, i) {
            0int
        } else {
            1int
        });
    }
}

/// 組 `i` から後ろで残す組は、後ろの組の数を超えない。同じ数なら、どの組も残す。
pub(super) proof fn lemma_mask_full(items: Seq<MaskItem>, s: Seq<(u16, Word)>, i: int)
    requires
        0 <= i <= s.len(),
    ensures
        mask_from(items, s, i).len() <= s.len() - i,
        mask_from(items, s, i).len() == s.len() - i ==> mask_from(items, s, i) =~= s.subrange(
            i,
            s.len() as int,
        ),
    decreases s.len() - i,
{
    if i < s.len() {
        lemma_mask_full(items, s, i + 1);
        if mask_from(items, s, i).len() == s.len() - i {
            assert(kept_at(items, s, i));
            assert(s.subrange(i, s.len() as int) =~= seq![s[i]] + s.subrange(
                i + 1,
                s.len() as int,
            ));
        }
    }
}

/// 除く組がちょうど尽きる最初の節点。除く数は1歩に1つまでしか増えないので、全体で `m` 除くなら、どこかで初めて
/// `m` になる。
pub(super) proof fn lemma_cut<M: Mode>(
    items: Seq<MaskItem>,
    h: Heap<M>,
    w: Word,
    n: nat,
    i: nat,
) -> (k: nat)
    requires
        i <= n,
        wdropped(items, h, w, n) == skip_total(items),
        wdropped(items, h, w, i) <= skip_total(items),
        forall|j: nat| j < i ==> #[trigger] wdropped(items, h, w, j) < skip_total(items),
    ensures
        i <= k <= n,
        wdropped(items, h, w, k) == skip_total(items),
        forall|j: nat| j < k ==> #[trigger] wdropped(items, h, w, j) < skip_total(items),
    decreases n - i,
{
    if wdropped(items, h, w, i) == skip_total(items) {
        i
    } else {
        assert(wdropped(items, h, w, i + 1) <= wdropped(items, h, w, i) + 1);
        lemma_cut(items, h, w, n, i + 1)
    }
}

/// 写しの列の最後を `rest` につないだ後、どの写しからも、写した組の続きと `rest` の列に着く。
pub(super) proof fn lemma_copies_path<M: Mode>(
    h: Heap<M>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
    rest: Word,
    g: nat,
    tail: Seq<(u16, Word)>,
    k: int,
)
    requires
        copies_ok(h, cs, out),
        0 <= k < cs.len(),
        h.scanned_word(cs.last(), 1) == rest,
        path(h, rest, g) == Some(tail),
        forall|q: int|
            0 <= q < out.len() ==> ev_ok(h, #[trigger] out[q].1) && h.tag_of(out[q].1) == out[q].0,
    ensures
        path(h, cs[k], (g + cs.len() - k) as nat) == Some(out.subrange(k, out.len() as int) + tail),
    decreases cs.len() - k,
{
    let n = cs[k];
    let fuel = (g + cs.len() - k) as nat;
    let next = h.scanned_word(n, 1);
    assert(node_ok(h, n));
    lemma_unit_bits();
    if k + 1 < cs.len() {
        lemma_copies_path(h, cs, out, rest, g, tail, k + 1);
        assert(next == cs[k + 1]);
        assert(path(h, next, (fuel - 1) as nat) == Some(
            out.subrange(k + 1, out.len() as int) + tail,
        ));
        assert(out.subrange(k, out.len() as int) + tail =~= seq![out[k]] + (out.subrange(
            k + 1,
            out.len() as int,
        ) + tail));
    } else {
        assert(next == rest);
        assert(out.subrange(k, out.len() as int) + tail =~= seq![out[k]] + tail);
    }
    assert(ev_ok(h, out[k].1));
}

pub(super) proof fn lemma_wseen_mono<M: Mode>(h: Heap<M>, w: Word, j: nat, i: nat, t: u16)
    requires
        j <= i,
    ensures
        wseen(h, w, j, t) <= wseen(h, w, i, t),
    decreases i,
{
    if j < i {
        lemma_wseen_mono(h, w, j, (i - 1) as nat, t);
    }
}

pub(super) proof fn lemma_agree_refl<M: Mode>(h: Heap<M>, w: Word, k: nat)
    ensures
        agree(h, h, w, k),
{
}

/// 写しの列の最後の写しの次を `v` にする。ほかの写しは変わらない。
pub(super) proof fn lemma_copies_link<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
    v: Word,
)
    requires
        copies_ok(h0, cs, out),
        cs.len() > 0,
        h0.scanned_word(cs.last(), 1).bits_spec() == 1,
        h1.field_set(h0, cs.last(), 1, v),
    ensures
        copies_ok(h1, cs, out),
        h1.scanned_word(cs.last(), 1) == v,
        kept_for(h1, h0, Set::empty().insert(cs.last().bits_spec())),
{
    let last = cs.last();
    let x = Set::empty().insert(last.bits_spec());
    lemma_kept_set(h1, h0, last, 1, v);
    assert(node_ok(h0, cs[cs.len() - 1]));
    lemma_unextended(h0, last);
    lemma_node_kept(h1, h0, x, last);
    lemma_unextended(h1, last);
    lemma_unit_bits();
    assert forall|k: int| 0 <= k < cs.len() implies {
        &&& node_ok(h1, #[trigger] cs[k])
        &&& h1.rc_value(cs[k]) > 0
        &&& h1.tag_of(cs[k]) == out[k].0
        &&& h1.scanned_word(cs[k], 0) == out[k].1
        &&& k + 1 < cs.len() ==> h1.scanned_word(cs[k], 1) == cs[k + 1]
    } by {
        lemma_node_kept(h1, h0, x, cs[k]);
        if k + 1 < cs.len() {
            assert(node_ok(h0, cs[k + 1]));
            assert(cs[k].bits_spec() != last.bits_spec());
        } else {
            assert(cs[k] == last);
        }
    }
}

/// 写しの列の最後に、新しい写し `cp` を足す。
pub(super) proof fn lemma_copies_push<M: Mode>(
    h: Heap<M>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
    cp: Word,
    pair: (u16, Word),
)
    requires
        copies_ok(h, cs, out),
        cs.len() > 0 ==> h.scanned_word(cs.last(), 1) == cp,
        node_ok(h, cp),
        h.rc_value(cp) > 0,
        h.tag_of(cp) == pair.0,
        h.scanned_word(cp, 0) == pair.1,
    ensures
        copies_ok(h, cs.push(cp), out.push(pair)),
{
    let cs1 = cs.push(cp);
    let out1 = out.push(pair);
    assert forall|k: int| 0 <= k < cs1.len() implies {
        &&& node_ok(h, #[trigger] cs1[k])
        &&& h.rc_value(cs1[k]) > 0
        &&& h.tag_of(cs1[k]) == out1[k].0
        &&& h.scanned_word(cs1[k], 0) == out1[k].1
        &&& k + 1 < cs1.len() ==> h.scanned_word(cs1[k], 1) == cs1[k + 1]
    } by {
        if k < cs.len() {
            assert(cs1[k] == cs[k]);
            if k + 1 == cs.len() {
                assert(cs[k] == cs.last());
            }
        }
    }
}

/// mask の列の1歩。組 `i` を残すなら `out` に足し、除くなら除いた数を1つ増やす。
pub(super) proof fn lemma_copy_seq_step(
    items: Seq<MaskItem>,
    s: Seq<(u16, Word)>,
    out: Seq<(u16, Word)>,
    js: Seq<int>,
    i: int,
    removed: int,
)
    requires
        copy_seq(items, s, out, js, i, removed),
        0 <= i < s.len(),
    ensures
        kept_at(items, s, i) ==> copy_seq(items, s, out.push(s[i]), js.push(i), i + 1, removed),
        !kept_at(items, s, i) ==> copy_seq(items, s, out, js, i + 1, removed + 1),
{
    if kept_at(items, s, i) {
        assert(mask_from(items, s, i) == seq![s[i]] + mask_from(items, s, i + 1));
        assert(out + (seq![s[i]] + mask_from(items, s, i + 1)) =~= out.push(s[i]) + mask_from(
            items,
            s,
            i + 1,
        ));
        let js1 = js.push(i);
        let out1 = out.push(s[i]);
        assert forall|k: int| 0 <= k < out1.len() implies 0 <= #[trigger] js1[k] < i + 1 && out1[k]
            == s[js1[k]] by {
            if k < out.len() {
                assert(js1[k] == js[k]);
            }
        }
    } else {
        assert(mask_from(items, s, i) == mask_from(items, s, i + 1));
    }
}

/// 写しの途中で `ev` を `dup` する。`ev` は `hs` で生きていた物体で、写しではない。
pub(super) proof fn lemma_copy_dup<M: Mode>(
    hs: Heap<M>,
    h0: Heap<M>,
    h1: Heap<M>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
    ev: Word,
)
    requires
        copy_heap(hs, h0, cs, out, |b: usize| 0int),
        h1.bumped(h0, ev),
        h1.settled(),
        h1.consistent(),
        kept_for(h1, h0, Set::empty()),
        ev.is_ptr_spec() ==> live(hs, ev.bits_spec()),
    ensures
        copy_heap(hs, h1, cs, out, |b: usize| points(ev, b)),
{
    let z = |b: usize| 0int;
    let d = |b: usize| points(ev, b);
    let none = Set::<usize>::empty();
    lemma_dup_moved(h1, h0, ev);
    lemma_moved_chain(h1, h0, hs, z, d, d, none, none, none);
    lemma_copies_kept(h1, h0, none, cs, out);
    if cs.len() > 0 {
        let b0 = cs[0].bits_spec();
        assert(live(h0, b0));
        assert(h0.core.settled_view().rc_of(b0) != 0) by {
            assert(node_ok(h0, cs[0]));
            assert(h0.rc_value(cs[0]) > 0);
        }
        assert(ev.bits_spec() != b0);
        assert(live(h0, cs.last().bits_spec()));
        assert(cs[cs.len() - 1] == cs.last());
        lemma_node_kept(h1, h0, none, cs.last());
    }
}

/// 写しの途中で、写しの節点 `cp` を確保する。
pub(super) proof fn lemma_copy_alloc<M: Mode>(
    hs: Heap<M>,
    h1: Heap<M>,
    h2: Heap<M>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
    ev: Word,
    cp: Word,
)
    requires
        copy_heap(hs, h1, cs, out, |b: usize| points(ev, b)),
        h1.settled(),
        h2.made(h1, cp),
        kept_for(h2, h1, Set::empty()),
        !live(h1, cp.bits_spec()),
    ensures
        copy_heap(hs, h2, cs, out, |b: usize| points(ev, b)),
        h2.core.settled_view().ext(cp.bits_spec()) == 1,
        !live(hs, cp.bits_spec()),
{
    let d = |b: usize| points(ev, b);
    let z = |b: usize| 0int;
    let none = Set::<usize>::empty();
    lemma_alloc_moved(h2, h1, cp);
    lemma_moved_chain(h2, h1, hs, d, z, d, none, none, none);
    lemma_copies_kept(h2, h1, none, cs, out);
    if cs.len() > 0 {
        let b0 = cs[0].bits_spec();
        assert(live(h1, b0));
        assert(h1.core.settled_view().rc_of(b0) != 0) by {
            assert(node_ok(h1, cs[0]));
            assert(h1.rc_value(cs[0]) > 0);
        }
        assert(cs[cs.len() - 1] == cs.last());
        lemma_node_kept(h2, h1, none, cs.last());
    }
    assert(no_free(h1, hs));
}

/// 写しの途中で、確保した写し `cp` の先頭の欄に `ev` を書く。`ev` の参照は辺に移る。
pub(super) proof fn lemma_copy_set<M: Mode>(
    hs: Heap<M>,
    h2: Heap<M>,
    h3: Heap<M>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
    ev: Word,
    cp: Word,
)
    requires
        copy_heap(hs, h2, cs, out, |b: usize| points(ev, b)),
        h2.settled(),
        live(h2, cp.bits_spec()),
        !live(hs, cp.bits_spec()),
        forall|k: int| 0 <= k < cs.len() ==> (#[trigger] cs[k]).bits_spec() != cp.bits_spec(),
        cp.is_ptr_spec(),
        h2.core.dom().contains(cp.bits_spec()),
        h2.core.state(cp.bits_spec()) == BlockState::Live,
        h2.scanned_at(cp, 0),
        h2.word_at(cp, 0).bits_spec() == 1,
        h2.rc_value(cp) > 0,
        h3.field_set(h2, cp, 0, ev),
        h3.consistent(),
        ev.is_ptr_spec(),
        live(hs, ev.bits_spec()),
    ensures
        copy_heap(hs, h3, cs, out, |b: usize| 0int),
        h3.core.settled_view().ext(cp.bits_spec()) == h2.core.settled_view().ext(cp.bits_spec()),
{
    let d = |b: usize| points(ev, b);
    let e = |b: usize| -points(ev, b);
    let z = |b: usize| 0int;
    let none = Set::<usize>::empty();
    let x = none.insert(cp.bits_spec());
    lemma_set_unit_moved(h3, h2, cp, 0, ev);
    lemma_moved_chain(h3, h2, hs, d, e, z, none, x, none);
    lemma_kept_set(h3, h2, cp, 0, ev);
    lemma_copies_kept(h3, h2, x, cs, out);
    assert(h2.core.settled_view().rc_of(cp.bits_spec()) != 0);
    if cs.len() > 0 {
        let b0 = cs[0].bits_spec();
        assert(live(h2, b0));
        assert(h2.core.settled_view().rc_of(b0) != 0) by {
            assert(node_ok(h2, cs[0]));
            assert(h2.rc_value(cs[0]) > 0);
        }
        assert(cs[cs.len() - 1] == cs.last());
        lemma_node_kept(h3, h2, x, cs.last());
    }
}

/// mask の写しで `dup` する evidence。`hs` で読んだ残す節点の evidence で、モデルが合っていれば `dup` の前提を満たし、
/// 検査モードでは前のたどりが `dup` を確かめている。
pub(super) proof fn lemma_mask_ev<M: Mode>(
    items: Seq<MaskItem>,
    hs: Heap<M>,
    h0: Heap<M>,
    w: Word,
    f: nat,
    s: Seq<(u16, Word)>,
    bound: nat,
    i: nat,
    ev: Word,
    c: bool,
)
    requires
        kept_for(h0, hs, Set::empty()),
        i < bound,
        ev == hs.scanned_word(nth(hs, w, i), 0),
        wkept(items, hs, w, i),
        M::CHECKED ==> mask_dups(items, hs, w, bound),
        c ==> {
            &&& mask_path(hs, w, f, s)
            &&& bound <= s.len()
            &&& h0.consistent()
            &&& h0.settleable()
        },
    ensures
        M::CHECKED ==> h0.dup_passes(ev),
        c ==> {
            &&& ev == s[i as int].1
            &&& ev.is_ptr_spec()
            &&& live(hs, ev.bits_spec())
            &&& ev_ok(hs, ev)
            &&& held_word(h0, ev)
            &&& h0.dup_ok(ev)
        },
{
    if c {
        lemma_path_nth(hs, w, f, s, i);
        lemma_node_kept(h0, hs, Set::empty(), ev);
    }
}

/// 局所の物体 (rc が正) は、このヒープのブロックで、塊のブロックでない。
pub(super) proof fn lemma_local_in_dom<M: Mode>(h: Heap<M>, n: Word)
    requires
        h.consistent(),
        h.settled(),
        n.is_ptr_spec(),
        h.readable(n),
        h.rc_value(n) > 0,
    ensures
        h.core.dom().contains(n.bits_spec()),
        h.core.state(n.bits_spec()) == BlockState::Live,
        h.writable(n),
{
    if !h.core.dom().contains(n.bits_spec()) {
        h.core.lemma_apart(n.bits_spec());
        h.core.lemma_region_at(n.bits_spec());
    }
}

/// 最後の写しの次の欄は、まだ書いていない書ける欄である。
pub(super) proof fn lemma_last_ready<M: Mode>(h: Heap<M>, cs: Seq<Word>, out: Seq<(u16, Word)>)
    requires
        h.consistent(),
        h.settled(),
        copies_ok(h, cs, out),
        cs.len() > 0,
        h.scanned_word(cs.last(), 1).bits_spec() == 1,
    ensures
        h.writable(cs.last()),
        h.owned(cs.last()),
        h.scanned_at(cs.last(), 1),
        h.word_at(cs.last(), 1).bits_spec() == 1,
        h.core.dom().contains(cs.last().bits_spec()),
        h.core.state(cs.last().bits_spec()) == BlockState::Live,
        h.readable(cs.last()),
{
    let last = cs.last();
    assert(cs[cs.len() - 1] == last);
    assert(node_ok(h, last));
    lemma_unextended(h, last);
    lemma_local_in_dom(h, last);
}

/// 写しの途中で、書き終えた写し `cp` を写しの列の最後につなぐ。列が空なら、`cp` が最初の写しになる。
pub(super) proof fn lemma_copy_push<M: Mode>(
    hs: Heap<M>,
    h3: Heap<M>,
    h4: Heap<M>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
    cp: Word,
    pair: (u16, Word),
)
    requires
        copy_heap(hs, h3, cs, out, |b: usize| 0int),
        h3.settled(),
        cp.is_ptr_spec(),
        live(h3, cp.bits_spec()),
        !live(hs, cp.bits_spec()),
        forall|k: int| 0 <= k < cs.len() ==> (#[trigger] cs[k]).bits_spec() != cp.bits_spec(),
        node_ok(h3, cp),
        h3.rc_value(cp) > 0,
        h3.tag_of(cp) == pair.0,
        h3.scanned_word(cp, 0) == pair.1,
        h3.scanned_word(cp, 1).bits_spec() == 1,
        h3.core.settled_view().ext(cp.bits_spec()) == 1,
        cs.len() == 0 ==> h4 == h3,
        cs.len() > 0 ==> h4.field_set(h3, cs.last(), 1, cp) && h4.consistent(),
    ensures
        copy_heap(hs, h4, cs.push(cp), out.push(pair), |b: usize| 0int),
        kept_for(h4, h3, Set::empty().insert(cs.last().bits_spec())),
{
    let z = |b: usize| 0int;
    let none = Set::<usize>::empty();
    let cs1 = cs.push(cp);
    if cs.len() == 0 {
        lemma_kept_refl(h3);
        lemma_copies_push(h4, cs, out, cp, pair);
        assert(cs1[0] == cp);
        assert(cs1.last() == cp);
    } else {
        let last = cs.last();
        let x = none.insert(last.bits_spec());
        let e = |b: usize| -points(cp, b);
        lemma_last_ready(h3, cs, out);
        lemma_set_unit_moved(h4, h3, last, 1, cp);
        assert(cs[cs.len() - 1] == last);
        assert(!live(hs, last.bits_spec()));
        lemma_moved_chain(h4, h3, hs, z, e, z, none, x, none);
        lemma_copies_link(h4, h3, cs, out, cp);
        assert(cp.bits_spec() != last.bits_spec());
        lemma_node_kept(h4, h3, x, cp);
        lemma_copies_push(h4, cs, out, cp, pair);
        let b0 = cs[0].bits_spec();
        assert(live(h3, b0));
        assert(h3.core.settled_view().rc_of(b0) != 0) by {
            assert(node_ok(h3, cs[0]));
            assert(h3.rc_value(cs[0]) > 0);
        }
        assert(cs1[0] == cs[0]);
        assert(cs1.last() == cp);
        assert forall|k: int| 0 <= k < cs1.len() implies {
            &&& (#[trigger] cs1[k]).is_ptr_spec()
            &&& !live(hs, cs1[k].bits_spec())
            &&& live(h4, cs1[k].bits_spec())
        } by {
            if k < cs.len() {
                assert(cs1[k] == cs[k]);
                assert(live(h3, cs[k].bits_spec()));
            }
        }
    }
}

/// 写しの最後に、共有する後ろの先頭 `rest` をつなぐ。`rest` の参照は辺に移り、元の物体の参照の数は `hs` と同じに戻る。
pub(super) proof fn lemma_copy_close<M: Mode>(
    hs: Heap<M>,
    h6: Heap<M>,
    h7: Heap<M>,
    cs: Seq<Word>,
    out: Seq<(u16, Word)>,
    rest: Word,
)
    requires
        copy_heap(hs, h6, cs, out, |b: usize| points(rest, b)),
        cs.len() > 0,
        h7.field_set(h6, cs.last(), 1, rest),
        h7.consistent(),
        rest.is_ptr_spec() ==> live(hs, rest.bits_spec()),
    ensures
        moved(h7, hs, |b: usize| points(cs[0], b), Set::empty()),
        no_free(h7, hs),
        copies_ok(h7, cs, out),
        h7.scanned_word(cs.last(), 1) == rest,
        h7.core.settled_view().ext(cs[0].bits_spec()) == 1,
        h7.settled(),
        live(h7, cs[0].bits_spec()),
        !live(hs, cs[0].bits_spec()),
        cs[0].is_ptr_spec(),
{
    let d = |b: usize| points(rest, b);
    let e = |b: usize| -points(rest, b);
    let z = |b: usize| 0int;
    let none = Set::<usize>::empty();
    let last = cs.last();
    let x = none.insert(last.bits_spec());
    lemma_last_ready(h6, cs, out);
    lemma_set_unit_moved(h7, h6, last, 1, rest);
    assert(cs[cs.len() - 1] == last);
    assert(!live(hs, last.bits_spec()));
    lemma_moved_chain(h7, h6, hs, d, e, z, none, x, none);
    lemma_copies_link(h7, h6, cs, out, rest);
    let b0 = cs[0].bits_spec();
    assert(live(h6, b0));
    assert(h6.core.settled_view().rc_of(b0) != 0) by {
        assert(node_ok(h6, cs[0]));
        assert(h6.rc_value(cs[0]) > 0);
    }
    lemma_moved_weaken(h7, hs, z, |b: usize| points(cs[0], b), none, none);
}

impl<M: Mode> Ctx<M> {
    /// `evv_mask` の、モデルが合っている道。写しの列を `rest` につないだ (写しがなければ `rest` を `dup` した) 後の
    /// ヒープで、結果 `r` は mask した列のリストである。
    pub(super) proof fn lemma_evv_mask_ok(
        &self,
        s0: Ctx<M>,
        items: Seq<MaskItem>,
        w: Word,
        f: nat,
        s: Seq<(u16, Word)>,
        bound: nat,
        cs: Seq<Word>,
        out: Seq<(u16, Word)>,
        js: Seq<int>,
        rest: Word,
        r: Word,
    )
        requires
            s0.held_ok(),
            s0.evv_wf(w),
            s == s0.evv_seq(w),
            mask_spec(items, s).len() + skip_total(items) == s.len(),
            mask_path(s0.heap, w, f, s),
            bound <= s.len(),
            copy_seq(items, s, out, js, bound as int, skip_total(items) as int),
            rest == nth(s0.heap, w, bound),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
            self.heap.consistent(),
            self.heap.settled(),
            moved(self.heap, s0.heap, |b: usize| points(r, b), Set::empty()),
            no_free(self.heap, s0.heap),
            cs.len() == out.len(),
            cs.len() == 0 ==> r == rest,
            cs.len() > 0 ==> {
                &&& r == cs[0]
                &&& copies_ok(self.heap, cs, out)
                &&& self.heap.scanned_word(cs.last(), 1) == rest
                &&& self.heap.core.settled_view().ext(cs[0].bits_spec()) >= 1
                &&& !live(s0.heap, cs[0].bits_spec())
                &&& cs[0].is_ptr_spec()
            },
        ensures
            self.held_ok(),
            self.holds(r),
            self.evv_wf(r),
            self.evv_seq(r) == mask_spec(items, s0.evv_seq(w)),
            self.evv_wf(w),
            self.evv_seq(w) == s0.evv_seq(w),
            self.evolved_by(s0, |b: usize| points(r, b), Set::empty()),
    {
        let hs = s0.heap;
        let h = self.heap;
        let d = |b: usize| points(r, b);
        let none = Set::<usize>::empty();
        let tail = s.subrange(bound as int, s.len() as int);
        // 元のリストと、共有する後ろ
        lemma_path_nth(hs, w, f, s, bound);
        let g = (f - bound) as nat;
        lemma_listed_head(hs, w, s0.max_evv);
        lemma_readable_kept(h, hs, w);
        lemma_path_kept(h, hs, d, none, w, f);
        lemma_unit_bits();
        if rest.bits_spec() != 1 {
            lemma_path_step(hs, rest, g, tail);
        }
        lemma_readable_kept(h, hs, rest);
        lemma_path_kept(h, hs, d, none, rest, g);
        // mask した列は、残した組と共有する後ろである。除く数は負でないので、元の列より長くない
        lemma_mask_full(items, s, bound as int);
        lemma_skip_prefix(items, 0);
        assert(mask_from(items, s, bound as int) =~= tail);
        assert(mask_spec(items, s) == out + tail);
        // Ctx の語
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(seq![], b) + d(
            b,
        ) >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
        }
        self.lemma_holds_moved(s0, seq![], seq![], d, none);
        self.lemma_evv_kept(s0, d, none);
        lemma_listed_kept(h, hs, d, none, w, s0.max_evv);
        if cs.len() == 0 {
            assert(out.len() == 0);
            assert(out + tail =~= tail);
            lemma_seq_of(h, rest, g);
            if rest.bits_spec() != 1 {
                lemma_path_step(hs, rest, g, tail);
            }
            self.lemma_dupped_holds(s0, rest, none);
        } else {
            lemma_path_walkable(h, w, f, s);
            assert forall|q: int| 0 <= q < out.len() implies ev_ok(h, #[trigger] out[q].1)
                && h.tag_of(out[q].1) == out[q].0 by {
                lemma_path_nth(h, w, f, s, js[q] as nat);
            }
            lemma_copies_path(h, cs, out, rest, g, tail, 0);
            assert(out.subrange(0, out.len() as int) =~= out);
            let fuel = (g + cs.len()) as nat;
            lemma_seq_of(h, cs[0], fuel);
            assert(node_ok(h, cs[0]));
            self.lemma_fresh_holds(s0, cs[0]);
        }
    }
}

} // verus!

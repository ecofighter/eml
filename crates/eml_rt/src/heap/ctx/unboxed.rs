//! 生の要素の配列の extern (docs/spec/object-model.md の「配列」)。`UnboxedArray a` と `MutUnboxedArray a` は同じ形の
//! `Raw` の物体で、`MutUnboxedArray` は `lin` ビットを持つ。要素は走査しない 64 ビットの語なので、`Float` と `Int` の
//! 行が同じ操作を使う。`get` と `length` は配列を借り、`set`、`fill`、`freeze`、`thaw` は配列を消費して返す。一意な
//! 配列はその場で書き換え、共有の配列は写す。

#[cfg(verus_only)]
use super::arrays::{
    lemma_held_word_relinked, lemma_moved_to_lin, lemma_path_relinked, lemma_relinked_same,
    lemma_unique,
};
#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::alloc::over_budget;
use crate::externs::{Fault, IndexFault};

verus! {

/// 生の要素の配列を書かない変化の後も、`a` は同じ形と要素の配列である。
proof fn lemma_unboxed_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    x: Set<usize>,
    a: Word,
)
    requires
        moved(h1, h0, d, x),
        h0.is_unboxed(a),
        h1.readable(a),
        !x.contains(a.bits_spec()),
    ensures
        h1.is_unboxed(a),
        h1.unboxed_len_spec(a) == h0.unboxed_len_spec(a),
        forall|i: int| #[trigger] h1.unboxed_at(a, i) == h0.unboxed_at(a, i),
        h1.lin_of(a) == h0.lin_of(a),
{
    let b = a.bits_spec();
    h0.lemma_unboxed_spec(a);
    h1.lemma_unboxed_spec(a);
    assert(live(h0, b) && live(h1, b));
    assert(same_head(h1, h0, b));
}

/// `settle` の後も、`a` は同じ形と要素の配列である。検査モードでモデルが合わなくても言える。
proof fn lemma_unboxed_settle<M: Mode>(h1: Heap<M>, h0: Heap<M>, a: Word)
    requires
        h1 == h0.settled_heap(),
        h0.is_unboxed(a),
    ensures
        moved(h1, h0, |b: usize| 0int, Set::empty()),
        no_free(h1, h0),
        exact(h1, h0, |b: usize| 0int),
        h1.is_unboxed(a),
        h1.unboxed_len_spec(a) == h0.unboxed_len_spec(a),
        forall|i: int| #[trigger] h1.unboxed_at(a, i) == h0.unboxed_at(a, i),
        h1.lin_of(a) == h0.lin_of(a),
        h1.rc_value(a) == h0.rc_value(a),
        h1.word_in(a),
{
    lemma_settle_moved(h1, h0);
    h0.lemma_unboxed_spec(a);
    lemma_readable_kept(h1, h0, a);
    lemma_unboxed_kept(h1, h0, |b: usize| 0int, Set::empty(), a);
}

/// 確保は、前に確かめた配列 `a` に触れない。新しい物体 `t` は `a` と別の物体である。
proof fn lemma_unboxed_grew<M: Mode>(h1: Heap<M>, h0: Heap<M>, a: Word, t: Word)
    requires
        h0.wf(),
        h0.settled(),
        h0.is_unboxed(a),
        h1.grew(h0, t),
    ensures
        h1.is_unboxed(a),
        h1.unboxed_len_spec(a) == h0.unboxed_len_spec(a),
        forall|i: int| #[trigger] h1.unboxed_at(a, i) == h0.unboxed_at(a, i),
        h1.lin_of(a) == h0.lin_of(a),
        h1.word_in(a),
        a.bits_spec() != t.bits_spec(),
{
    let b = a.bits_spec();
    h0.lemma_unboxed_spec(a);
    h1.lemma_unboxed_spec(a);
    h0.core.lemma_shape(b);
    assert(h1.grown_block(h0, b));
}

/// 要素の語だけを書いた変化 (`unboxed_written`)。`a` の語のほかは変わらない。
proof fn lemma_written_moved<M: Mode>(h1: Heap<M>, h0: Heap<M>, a: Word)
    requires
        h0.settled(),
        h1.unboxed_written(h0, a),
    ensures
        moved(h1, h0, |b: usize| 0int, Set::empty().insert(a.bits_spec())),
        no_free(h1, h0),
        no_alloc(h1, h0),
        exact(h1, h0, |b: usize| 0int),
        h1.settled(),
{
    let h = a.bits_spec();
    h1.lemma_unboxed_written(h0, a);
    assert forall|b: usize| #[trigger] live(h1, b) == live(h0, b) by {}
    assert forall|b: usize| live(h0, b) && live(h1, b) && b != h implies #[trigger] h1.core.perm(b)
        == h0.core.perm(b) by {
        if h0.core.dom().contains(b) {
            assert(h1.core.perms@[b] == h0.core.perms@[b]);
        }
    }
}

/// 新しい配列 `t` を作り、その要素だけを書いた後も、呼ぶ側の持つ `a` は手放せる。
proof fn lemma_drop_ready<M: Mode>(s1: Ctx<M>, h2: Heap<M>, h3: Heap<M>, a: Word, t: Word)
    requires
        s1.held_ok(),
        s1.holds(a),
        s1.heap.is_unboxed(a),
        s1.heap.settled(),
        h2.made(s1.heap, t),
        h3 == h2 || h3.unboxed_written(h2, t),
        h3.consistent(),
    ensures
        h3.decref_ok(a),
        h3.word_in(a),
        moved(h3, s1.heap, |b: usize| 0int, Set::empty()),
        no_free(h3, s1.heap),
        exact(h3, s1.heap, |b: usize| 0int),
        moved(h3, h2, |b: usize| 0int, Set::empty().insert(t.bits_spec())),
        no_free(h3, h2),
        exact(h3, h2, |b: usize| 0int),
        !live(s1.heap, t.bits_spec()),
        live(h2, t.bits_spec()),
        h2.core.settled_view().ext(t.bits_spec()) == 1,
        a.bits_spec() != t.bits_spec(),
        h3.settled(),
{
    let h1 = s1.heap;
    let z = |b: usize| 0int;
    let none = Set::<usize>::empty();
    let xt = none.insert(t.bits_spec());
    let ws = seq![a];
    lemma_alloc_moved(h2, h1, t);
    if h3 == h2 {
        lemma_moved_refl(h2);
        lemma_moved_weaken(h2, h2, z, z, none, xt);
    } else {
        lemma_written_moved(h3, h2, t);
    }
    lemma_moved_chain(h3, h2, h1, z, z, z, none, xt, none);
    assert(ws[0] == a);
    s1.lemma_arg_ready(ws, 0);
    lemma_points_in(ws, a.bits_spec());
    lemma_arg_moved(h3, h1, z, none, a);
}

impl<M: Mode> Ctx<M> {
    /// `a` は生の要素の配列を指す。読める tag 0 の `Raw` の物体である。
    pub closed spec fn is_unboxed(&self, a: Word) -> bool {
        self.heap.is_unboxed(a)
    }

    /// 生の要素の配列 `a` の長さ。
    pub closed spec fn unboxed_array_len(&self, a: Word) -> int {
        self.heap.unboxed_len_spec(a)
    }

    /// 生の要素の配列 `a` の i 番目の要素のビット列。
    pub closed spec fn unboxed_array_at(&self, a: Word, i: int) -> u64 {
        self.heap.unboxed_at(a, i)
    }

    /// 生の要素の配列 `a` は `lin` ビットを持つ (`MutUnboxedArray`)。
    pub closed spec fn unboxed_array_lin(&self, a: Word) -> bool {
        self.heap.lin_of(a)
    }

    /// `old` から `moved(d, x)` で変えたヒープで、`Ctx` の語は持たれたままで、`evv` も同じ列のリストである。`x` は
    /// `Raw` の物体だけなので、`evv` の節点でも evidence でもエントリの表でもない。`d` は、引数 `ws` の参照を手放す
    /// 分を超えて減らさない。
    proof fn lemma_raw_kept_ok(
        &self,
        old: Ctx<M>,
        ws: Seq<Word>,
        d: spec_fn(usize) -> int,
        x: Set<usize>,
    )
        requires
            old.held_ok(),
            old.holds_args(ws),
            self.heap.consistent(),
            self.heap.settleable(),
            self.evv == old.evv,
            self.yielding == old.yielding,
            self.max_evv == old.max_evv,
            moved(self.heap, old.heap, d, x),
            forall|b: usize| #[trigger] d(b) >= -points_in(ws, b),
            forall|b: usize| #[trigger] x.contains(b) ==> old.heap.core.kind(b) == Kind::Raw,
        ensures
            self.held_ok(),
            self.evolved_by(old, d, x),
    {
        let h0 = old.heap;
        let h1 = self.heap;
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies old.need(ws, b) + d(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            assert(d(b) >= -points_in(ws, b));
        }
        self.lemma_holds_moved(old, ws, seq![], d, x);
        assert forall|n: Word| #[trigger] node_ok(h0, n) implies !x.contains(n.bits_spec()) by {
            if x.contains(n.bits_spec()) {
                assert(h0.core.kind(n.bits_spec()) == Kind::Raw);
            }
        }
        assert forall|e: Word|
            #![trigger ev_ok(h0, e)]
            ev_ok(h0, e) && h1.readable(e) && x.contains(e.bits_spec()) implies ev_ok(h1, e)
            && h1.tag_of(e) == h0.tag_of(e) by {
            assert(h0.core.kind(e.bits_spec()) == Kind::Raw);
        }
        assert forall|t: Word|
            #![trigger table_ok(h0, t)]
            table_ok(h0, t) && h1.readable(t) && x.contains(t.bits_spec()) implies table_ok(
            h1,
            t,
        ) by {
            assert(h0.core.kind(t.bits_spec()) == Kind::Raw);
        }
        lemma_listed_kept(h1, h0, d, x, self.evv, self.max_evv);
    }

    /// 配列を受ける操作で `settle` した後の文脈は、前の文脈と同じものを持つ。
    proof fn lemma_unboxed_settled(&self, s0: Ctx<M>, a: Word)
        requires
            s0.held_ok(),
            s0.heap.is_unboxed(a),
            s0.holds(a),
            self.heap == s0.heap.settled_heap(),
            self.heap.consistent(),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(a),
    {
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        lemma_settle_moved(self.heap, s0.heap);
        self.lemma_kept_ok(s0, seq![], z);
        assert forall|b: usize| #[trigger] self.need(seq![a], b) > 0 implies s0.need(seq![a], b)
            + z(b) >= self.need(seq![a], b) by {}
        self.lemma_holds_moved(s0, seq![a], seq![a], z, none);
    }

    /// その場で要素を書いた道。`s1` は `settle` の後の文脈で、そこから `a` の要素の語だけを書いた。
    proof fn lemma_written_ok(&self, s1: Ctx<M>, a: Word)
        requires
            s1.held_ok(),
            s1.holds(a),
            s1.heap.is_unboxed(a),
            s1.heap.settled(),
            self.heap == s1.heap || self.heap.unboxed_written(s1.heap, a),
            self.heap.consistent(),
            self.evv == s1.evv,
            self.yielding == s1.yielding,
            self.max_evv == s1.max_evv,
        ensures
            self.held_ok(),
            self.holds(a),
            moved(self.heap, s1.heap, |b: usize| 0int, Set::empty().insert(a.bits_spec())),
            no_free(self.heap, s1.heap),
            exact(self.heap, s1.heap, |b: usize| 0int),
    {
        let h1 = s1.heap;
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let xa = none.insert(a.bits_spec());
        let ws = seq![a];
        if self.heap == h1 {
            lemma_moved_refl(h1);
            lemma_moved_weaken(h1, h1, z, z, none, xa);
        } else {
            lemma_written_moved(self.heap, h1, a);
        }
        h1.lemma_unboxed_spec(a);
        assert forall|b: usize| #[trigger] xa.contains(b) implies h1.core.kind(b) == Kind::Raw by {}
        assert forall|b: usize| #[trigger] z(b) >= -points_in(ws, b) by {
            lemma_points_in(ws, b);
        }
        self.lemma_raw_kept_ok(s1, ws, z, xa);
        assert forall|b: usize| #[trigger] self.need(ws, b) > 0 implies s1.need(ws, b) + z(b)
            >= self.need(ws, b) by {}
        assert(ws[0] == a);
        s1.lemma_arg_ready(ws, 0);
        self.lemma_holds_moved(s1, ws, ws, z, xa);
    }

    /// 共有の配列の代わりに新しい配列 `t` を作り、その要素だけを書き、`a` を手放した道。`s1` は `settle` の後の
    /// 文脈で、`h2` は確保の直後、`h3` は要素を書いた後である。
    proof fn lemma_copied_ok(&self, s1: Ctx<M>, h2: Heap<M>, h3: Heap<M>, a: Word, t: Word)
        requires
            s1.held_ok(),
            s1.holds(a),
            s1.heap.is_unboxed(a),
            s1.heap.settled(),
            h2.made(s1.heap, t),
            h2.owned(t),
            h3 == h2 || h3.unboxed_written(h2, t),
            h3.consistent(),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.evolved(h3, a, -1),
            self.heap.kept_live(h3),
            self.evv == s1.evv,
            self.yielding == s1.yielding,
            self.max_evv == s1.max_evv,
        ensures
            self.held_ok(),
            self.holds(t),
            self.evolved_by(s1, |b: usize| points(t, b) - points(a, b), Set::empty()),
            h3.is_unboxed(t) ==> {
                &&& self.heap.is_unboxed(t)
                &&& self.heap.unboxed_len_spec(t) == h3.unboxed_len_spec(t)
                &&& forall|i: int| #[trigger] self.heap.unboxed_at(t, i) == h3.unboxed_at(t, i)
                &&& self.heap.lin_of(t) == h3.lin_of(t)
            },
    {
        let h1 = s1.heap;
        let h4 = self.heap;
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        let xt = none.insert(t.bits_spec());
        let ws = seq![a];
        let pa = |b: usize| -points(a, b);
        let d = |b: usize| points(t, b) - points(a, b);
        lemma_drop_ready(s1, h2, h3, a, t);
        lemma_decref_moved(h4, h3, a);
        lemma_moved_chain(h4, h3, h1, z, pa, pa, none, none, none);
        assert forall|b: usize| #[trigger] pa(b) >= -points_in(ws, b) by {
            lemma_points_in(ws, b);
        }
        assert forall|b: usize| #[trigger] none.contains(b) implies h1.core.kind(b)
            == Kind::Raw by {}
        self.lemma_raw_kept_ok(s1, ws, pa, none);
        assert forall|b: usize| live(h1, b) implies #[trigger] pa(b) == d(b) by {}
        lemma_moved_weaken(h4, h1, pa, d, none, none);
        lemma_moved_chain(h4, h3, h2, z, pa, pa, xt, none, xt);
        lemma_arg_moved(h4, h2, pa, xt, t);
        assert(live(h2, t.bits_spec()) && live(h4, t.bits_spec()));
        assert(same_head(h4, h2, t.bits_spec()));
        self.lemma_fresh_holds(s1, t);
        if h3.is_unboxed(t) {
            lemma_unboxed_kept(h4, h3, pa, none, t);
        }
    }

    /// 一意な配列の `lin` ビットだけを変えた道。`a` はほかの物体から辺で指されないので、`Ctx` の語と `evv` の
    /// リストはそのままである。
    proof fn lemma_unboxed_relinked(&self, s1: Ctx<M>, a: Word, lin: bool)
        requires
            s1.held_ok(),
            s1.holds(a),
            s1.heap.is_unboxed(a),
            s1.heap.rc_value(a) == 1,
            s1.heap.settled(),
            self.heap.relinked(s1.heap, a, lin),
            self.heap.consistent(),
            self.evv == s1.evv,
            self.yielding == s1.yielding,
            self.max_evv == s1.max_evv,
        ensures
            self.held_ok(),
            self.holds(a),
            self.heap.is_unboxed(a),
            self.heap.unboxed_len_spec(a) == s1.heap.unboxed_len_spec(a),
            forall|i: int| #[trigger] self.heap.unboxed_at(a, i) == s1.heap.unboxed_at(a, i),
            self.heap.lin_of(a) == lin,
            moved_lin(self.heap, s1.heap, |b: usize| 0int, Set::empty(), a.bits_spec()),
            no_free(self.heap, s1.heap),
            forall|b: usize|
                #![trigger live(s1.heap, b)]
                live(s1.heap, b) ==> self.heap.core.settled_view().ext(b)
                    == s1.heap.core.settled_view().ext(b),
    {
        let h0 = s1.heap;
        let h1 = self.heap;
        let ab = a.bits_spec();
        lemma_relinked_same(h1, h0, a, lin);
        h0.lemma_unboxed_spec(a);
        h1.lemma_unboxed_spec(a);
        lemma_unique(h0, a);
        lemma_points_at(seq![a], 0, ab);
        assert(s1.need(seq![a], ab) >= 1);
        assert(h0.core.settled_view() == h0.core);
        assert(h0.core.indeg(ab) == 0);
        // `Ctx` の語の数え方は、モデルが同じなので変わらない
        assert(held_word(h1, self.evv)) by {
            lemma_held_word_relinked(h1, h0, a, lin, self.evv);
        }
        assert(held_word(h1, self.yielding.target)) by {
            lemma_held_word_relinked(h1, h0, a, lin, self.yielding.target);
        }
        assert(held_word(h1, self.yielding.clause)) by {
            lemma_held_word_relinked(h1, h0, a, lin, self.yielding.clause);
        }
        assert(held_word(h1, self.yielding.cont)) by {
            lemma_held_word_relinked(h1, h0, a, lin, self.yielding.cont);
        }
        assert(held_word(h1, a)) by {
            lemma_held_word_relinked(h1, h0, a, lin, a);
        }
        assert(self.holds_args(seq![])) by {
            assert forall|b: usize| #[trigger]
                self.need(seq![], b) > 0 && h1.core.settled_view().rc_of(b)
                    != 0 implies h1.core.settled_view().ext(b) >= self.need(seq![], b) by {
                assert(s1.need(seq![], b) == self.need(seq![], b));
            }
        }
        assert(self.holds_args(seq![a])) by {
            let ws = seq![a];
            assert forall|i: int| 0 <= i < ws.len() implies held_word(h1, #[trigger] ws[i]) by {}
            assert forall|b: usize| #[trigger]
                self.need(ws, b) > 0 && h1.core.settled_view().rc_of(b)
                    != 0 implies h1.core.settled_view().ext(b) >= self.need(ws, b) by {
                assert(s1.need(ws, b) == self.need(ws, b));
            }
        }
        // `evv` のリスト
        let w = self.evv;
        let f = choose|f: nat| #[trigger] path(h0, w, f) is Some;
        lemma_seq_of(h0, w, f);
        lemma_listed_head(h0, w, self.max_evv);
        lemma_path_relinked(h1, h0, a, lin, w, f);
        lemma_seq_of(h1, w, f);
        assert(listed(h1, w, self.max_evv));
        assert(h1.core.perm(ab).slots == h0.core.perm(ab).slots);
        assert(moved_lin(h1, h0, |b: usize| 0int, Set::empty(), ab)) by {
            assert forall|b: usize| live(h0, b) && live(h1, b) implies {
                &&& b != ab ==> same_head(h1, h0, b)
                &&& #[trigger] same_head_lin(h1, h0, b)
                &&& h1.core.perm(b).slots == h0.core.perm(b).slots
            } by {
                if b != ab {
                    assert(h1.core.perm(b) == h0.core.perm(b));
                }
            }
        }
    }

    /// 検査モードで、`a` が tag 0 の `Raw` の物体を指すかを確かめる。違えば `WrongKind` を返す。
    fn checked_unboxed(&self, a: Word) -> (r: Result<(), HeapError>)
        requires
            M::CHECKED,
            self.wf(),
            self.heap.word_in(a),
        ensures
            r is Ok ==> a.is_ptr_spec(),
            self.held_ok() && self.is_unboxed(a) ==> r is Ok,
    {
        proof {
            self.heap.lemma_unboxed_spec(a);
        }
        if !a.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        let m = (unsafe { self.heap.meta(a) })?;
        proof {
            m.lemma_fields();
        }
        if !matches!(m.kind(), Kind::Raw) || m.tag() != 0 {
            return Err(HeapError::WrongKind);
        }
        Ok(())
    }

    /// 生の要素の配列 `a` の長さを読む。`a` は借りる。
    fn unboxed_length_of(&self, a: Word) -> (r: Result<usize, HeapError>)
        requires
            self.wf(),
            self.heap.word_in(a),
            !M::CHECKED ==> self.is_unboxed(a),
        ensures
            r matches Ok(n) ==> n == self.unboxed_array_len(a) && a.is_ptr_spec()
                && self.is_unboxed(a),
            !M::CHECKED ==> r is Ok,
            self.held_ok() && self.is_unboxed(a) ==> r is Ok,
    {
        if M::CHECKED {
            self.checked_unboxed(a)?;
        }
        proof {
            self.heap.lemma_unboxed_spec(a);
        }
        unsafe { self.heap.unboxed_len(a) }
    }

    /// `Std.UnboxedArray.length` と `Std.MutUnboxedArray.length`。`a` は借りる。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`a` は即値か、このヒープか付けた塊の生きている物体を指す。速いモードでは、`a`
    /// は生の要素の配列である (検査モードは形を確かめ、違えば `WrongKind` を返す)。
    pub unsafe fn unboxed_length(&mut self, a: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_unboxed(a),
        ensures
            *final(self) == *old(self),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_unboxed(a) ==> r == Ok::<i64, Fault>(
                old(self).unboxed_array_len(a) as i64,
            ),
    {
        match self.unboxed_length_of(a) {
            Ok(n) => Ok(n as i64),
            Err(e) => Err(Fault::Heap(e)),
        }
    }

    /// `get` と `read` の行。`a` は借り、位置 `i` の要素のビット列を返す。範囲の外なら `Fault::Index` を返す。
    ///
    /// # Safety
    /// `unboxed_length` と同じ。
    pub unsafe fn unboxed_get(&mut self, a: Word, i: i64) -> (r: Result<u64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_unboxed(a),
        ensures
            *final(self) == *old(self),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_unboxed(a) ==> {
                &&& !(r matches Err(Fault::Heap(_)))
                &&& 0 <= i < old(self).unboxed_array_len(a) ==> r == Ok::<u64, Fault>(
                    old(self).unboxed_array_at(a, i as int),
                )
                &&& !(0 <= i < old(self).unboxed_array_len(a)) ==> (r matches Err(Fault::Index(_)))
            },
    {
        let n = match self.unboxed_length_of(a) {
            Ok(n) => n,
            Err(e) => return Err(Fault::Heap(e)),
        };
        if !(0 <= i && (i as u64) < (n as u64)) {
            return Err(Fault::Index(Box::new(IndexFault::Array { index: i, len: n as i64 })));
        }
        match unsafe { self.heap.unboxed_word(a, i as usize) } {
            Ok(x) => Ok(x),
            Err(e) => Err(Fault::Heap(e)),
        }
    }

    /// 長さ `n` の、どの要素も `x` のビット列の新しい配列を返す。`MutUnboxedArray` なら `lin` を真にする。`n` が負なら
    /// `Fault::Index` を返す。確保量の上限を超える長さは、確保を試す前に `Stopped(MemoryLimit)` で止める。
    pub fn unboxed_make(&mut self, n: i64, x: u64, lin: bool) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || r == Err::<Word, Fault>(
                Fault::Stopped(Stop::MemoryLimit),
            ),
            n < 0 ==> (r matches Err(Fault::Index(_))),
            n >= 0 ==> !(r matches Err(Fault::Index(_))),
            r is Err ==> *final(self) == *old(self),
            r matches Ok(w) ==> {
                &&& final(self).is_unboxed(w)
                &&& final(self).unboxed_array_len(w) == n
                &&& final(self).unboxed_array_lin(w) == lin
                &&& forall|j: int| 0 <= j < n ==> #[trigger] final(self).unboxed_array_at(w, j) == x
            },
            r matches Ok(w) ==> (old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |b: usize| 0int, Set::empty())
            }),
    {
        let ghost s0 = *self;
        if n < 0 {
            return Err(Fault::Index(Box::new(IndexFault::Length { len: n })));
        }
        let k = n as usize;
        // 確保器が作れない大きさと、確保量の上限を超える大きさは、確保を試す前に止める。確保器に任せると、プロセスが
        // panic で落ちるか、上限を報告する前に OS に止められるためである (docs/spec/object-model.md の「配列」)
        if k > MAX_LEN || over_budget(&self.control, (k + 4) * 8) {
            return Err(Fault::Stopped(Stop::MemoryLimit));
        }
        let w = self.heap.alloc_unboxed(k, x, lin);
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                self.lemma_made_ok(s0, w);
            }
        }
        Ok(w)
    }

    /// `Std.MutUnboxedArray.empty`。長さ 0 の新しい配列を返す。`lin` が真なら `MutUnboxedArray` である。
    pub fn unboxed_empty(&mut self, lin: bool) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok,
            r matches Ok(w) ==> {
                &&& final(self).is_unboxed(w)
                &&& final(self).unboxed_array_len(w) == 0
                &&& final(self).unboxed_array_lin(w) == lin
            },
            r matches Ok(w) ==> (old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |b: usize| 0int, Set::empty())
            }),
    {
        let ghost s0 = *self;
        let w = self.heap.alloc_unboxed(0, 0, lin);
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                self.lemma_made_ok(s0, w);
            }
        }
        Ok(w)
    }

    /// `set` と `write` の行。`a` の参照を受け取り、位置 `i` を `x` のビット列にした配列を返す。`a` が一意ならその場で
    /// 書き換えて `a` を返す。共有なら、要素を写した新しい配列に書いて返し、`a` を手放す。写しは `a` の `lin` ビットを
    /// 引き継ぐ。範囲の外なら、`a` を手放して `Fault::Index` を返す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`a` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。速いモードでは、`a` は生の要素の配列である (検査モードは形を確かめ、違えば `WrongKind` を返す)。
    pub unsafe fn unboxed_set(&mut self, a: Word, i: i64, x: u64) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_unboxed(a) && old(self).holds(a),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_unboxed(a) && old(self).holds(a) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e))
                &&& 0 <= i < old(self).unboxed_array_len(a) && !(r matches Err(Fault::Heap(_)))
                    ==> (r matches Ok(w) && {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_unboxed(w)
                    &&& final(self).unboxed_array_len(w) == old(self).unboxed_array_len(a)
                    &&& final(self).unboxed_array_lin(w) == old(self).unboxed_array_lin(a)
                    &&& final(self).unboxed_array_at(w, i as int) == x
                    &&& forall|j: int|
                        0 <= j < old(self).unboxed_array_len(a) && j != i
                            ==> #[trigger] final(self).unboxed_array_at(w, j) == old(
                            self,
                        ).unboxed_array_at(a, j)
                    &&& final(self).evolved_by(
                        *old(self),
                        |b: usize| points(w, b) - points(a, b),
                        Set::empty().insert(a.bits_spec()),
                    )
                })
                &&& !(0 <= i < old(self).unboxed_array_len(a)) && !(r matches Err(Fault::Heap(_)))
                    ==> {
                    &&& r matches Err(Fault::Index(_))
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |b: usize| -points(a, b), Set::empty())
                }
            },
    {
        let ghost s0 = *self;
        let ghost h0 = self.heap;
        let ghost ws = seq![a];
        let ghost c = s0.held_ok() && s0.is_unboxed(a) && s0.holds(a);
        let ghost z = |b: usize| 0int;
        let ghost none = Set::<usize>::empty();
        let ghost xa = none.insert(a.bits_spec());
        proof {
            assert(ws[0] == a);
            if c {
                s0.lemma_arg_ready(ws, 0);
            }
        }
        let n = match self.unboxed_length_of(a) {
            Ok(n) => n,
            Err(e) => return Err(Fault::Heap(e)),
        };
        if !(0 <= i && (i as u64) < (n as u64)) {
            return unsafe {
                self.refuse(
                    a,
                    Fault::Index(Box::new(IndexFault::Array { index: i, len: n as i64 })),
                )
            };
        }
        let idx = i as usize;
        self.heap.settle();
        let ghost s1 = *self;
        let ghost h1 = self.heap;
        proof {
            lemma_unboxed_settle(h1, h0, a);
            assert(h1.words_kept(h0));
            if c {
                s1.lemma_unboxed_settled(s0, a);
            }
        }
        let unique = match unsafe { self.heap.is_unique(a) } {
            Ok(u) => u,
            Err(e) => {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if unique {
            proof {
                h1.lemma_unboxed_spec(a);
            }
            if let Err(e) = unsafe { self.heap.unboxed_put(a, idx, x) } {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            proof {
                self.heap.lemma_unboxed_written(h1, a);
                if M::CHECKED {
                    lemma_words_trans(self.heap, h1, h0);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_written_ok(s1, a);
                    lemma_moved_chain(
                        self.heap,
                        h1,
                        h0,
                        z,
                        z,
                        |b: usize| points(a, b) - points(a, b),
                        none,
                        xa,
                        xa,
                    );
                }
            }
            return Ok(a);
        }
        // 共有の配列は写してから書く

        let m = match unsafe { self.heap.meta(a) } {
            Ok(m) => m,
            Err(e) => {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        proof {
            m.lemma_fields();
            h1.lemma_unboxed_spec(a);
        }
        let t = self.heap.alloc_unboxed(n, 0, m.lin());
        let ghost h2 = self.heap;
        proof {
            lemma_unboxed_grew(h2, h1, a, t);
            h2.lemma_unboxed_spec(t);
            if M::CHECKED {
                lemma_words_trans(h2, h1, h0);
            }
        }
        if let Err(e) = unsafe { self.heap.unboxed_copy(t, a, n) } {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let ghost h3 = self.heap;
        proof {
            h3.lemma_unboxed_written(h2, t);
            if M::CHECKED {
                lemma_words_trans(h3, h2, h0);
            }
            h3.lemma_unboxed_spec(t);
        }
        if let Err(e) = unsafe { self.heap.unboxed_put(t, idx, x) } {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let ghost h4 = self.heap;
        proof {
            h4.lemma_unboxed_written_trans(h3, h2, t);
            h4.lemma_unboxed_written(h2, t);
            if M::CHECKED {
                lemma_words_trans(h4, h3, h0);
            }
            if c {
                lemma_drop_ready(s1, h2, h4, a, t);
            }
        }
        if let Err(e) = unsafe { self.heap.decref(a) } {
            proof {
                lemma_words_trans(self.heap, h4, h0);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h4, h0);
                self.lemma_words_kept(s0);
            }
            if c {
                let d = |b: usize| points(t, b) - points(a, b);
                self.lemma_copied_ok(s1, h2, h4, a, t);
                lemma_moved_chain(self.heap, h1, h0, z, d, d, none, none, none);
                lemma_moved_weaken(self.heap, h0, d, d, none, xa);
            }
        }
        Ok(t)
    }

    /// `Std.MutUnboxedArray.MutUnboxed` の `fill`。`a` の参照を受け取り、どの要素も `x` のビット列にした配列を返す。
    /// `a` が一意ならその場で書いて `a` を返す。共有なら、検査モードは `NotUnique` で断る。`MutUnboxedArray` はいつも
    /// 一意だからである。速いモードは同じ長さと `lin` ビットの新しい配列を作り、`a` を手放す (`freeze` と同じ扱い)。
    ///
    /// # Safety
    /// `unboxed_set` と同じ。
    pub unsafe fn unboxed_fill(&mut self, a: Word, x: u64) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_unboxed(a) && old(self).holds(a),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_unboxed(a) && old(self).holds(a) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e)) || (
                M::CHECKED && e == HeapError::NotUnique)
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_unboxed(w)
                    &&& final(self).unboxed_array_len(w) == old(self).unboxed_array_len(a)
                    &&& final(self).unboxed_array_lin(w) == old(self).unboxed_array_lin(a)
                    &&& forall|j: int|
                        0 <= j < old(self).unboxed_array_len(a)
                            ==> #[trigger] final(self).unboxed_array_at(w, j) == x
                    &&& final(self).evolved_by(
                        *old(self),
                        |b: usize| points(w, b) - points(a, b),
                        Set::empty().insert(a.bits_spec()),
                    )
                }
            },
    {
        let ghost s0 = *self;
        let ghost h0 = self.heap;
        let ghost ws = seq![a];
        let ghost c = s0.held_ok() && s0.is_unboxed(a) && s0.holds(a);
        let ghost z = |b: usize| 0int;
        let ghost none = Set::<usize>::empty();
        let ghost xa = none.insert(a.bits_spec());
        proof {
            assert(ws[0] == a);
            if c {
                s0.lemma_arg_ready(ws, 0);
            }
        }
        let n = match self.unboxed_length_of(a) {
            Ok(n) => n,
            Err(e) => return Err(Fault::Heap(e)),
        };
        self.heap.settle();
        let ghost s1 = *self;
        let ghost h1 = self.heap;
        proof {
            lemma_unboxed_settle(h1, h0, a);
            assert(h1.words_kept(h0));
            if c {
                s1.lemma_unboxed_settled(s0, a);
            }
        }
        let unique = match unsafe { self.heap.is_unique(a) } {
            Ok(u) => u,
            Err(e) => {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if unique {
            proof {
                h1.lemma_unboxed_spec(a);
            }
            if let Err(e) = fill_elements(&mut self.heap, a, n, x) {
                proof {
                    lemma_words_trans(self.heap, h1, h0);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, h1, h0);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_written_ok(s1, a);
                    lemma_moved_chain(
                        self.heap,
                        h1,
                        h0,
                        z,
                        z,
                        |b: usize| points(a, b) - points(a, b),
                        none,
                        xa,
                        xa,
                    );
                }
            }
            return Ok(a);
        }
        if M::CHECKED {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(HeapError::NotUnique));
        }
        // 速いモードで共有の配列を受けたら、写さずに新しい配列を作る

        let m = match unsafe { self.heap.meta(a) } {
            Ok(m) => m,
            Err(e) => return Err(Fault::Heap(e)),
        };
        proof {
            m.lemma_fields();
            h1.lemma_unboxed_spec(a);
        }
        let t = self.heap.alloc_unboxed(n, x, m.lin());
        let ghost h2 = self.heap;
        proof {
            lemma_unboxed_grew(h2, h1, a, t);
            lemma_drop_ready(s1, h2, h2, a, t);
        }
        if let Err(e) = unsafe { self.heap.decref(a) } {
            return Err(Fault::Heap(e));
        }
        proof {
            let d = |b: usize| points(t, b) - points(a, b);
            self.lemma_copied_ok(s1, h2, h2, a, t);
            lemma_moved_chain(self.heap, h1, h0, z, d, d, none, none, none);
            lemma_moved_weaken(self.heap, h0, d, d, none, xa);
        }
        Ok(t)
    }

    /// `freeze` と `thaw` の核。`a` の参照を受け取り、`lin` ビットを `lin` にした同じ要素の配列を返す。`a` が一意なら、
    /// 写さずにその場で `lin` ビットを変えて `a` を返す。共有なら、要素を写した新しい配列を返し、`a` を手放す。ただし
    /// 検査モードでは、共有の `MutUnboxedArray` (`freeze` の `a`) は `NotUnique` で断る。
    unsafe fn relin_unboxed(&mut self, a: Word, lin: bool) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_unboxed(a) && old(self).holds(a),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_unboxed(a) && old(self).holds(a) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e)) || (
                M::CHECKED && !lin && e == HeapError::NotUnique)
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_unboxed(w)
                    &&& final(self).unboxed_array_len(w) == old(self).unboxed_array_len(a)
                    &&& final(self).unboxed_array_lin(w) == lin
                    &&& forall|j: int|
                        0 <= j < old(self).unboxed_array_len(a)
                            ==> #[trigger] final(self).unboxed_array_at(w, j) == old(
                            self,
                        ).unboxed_array_at(a, j)
                    &&& final(self).evolved_lin(
                        *old(self),
                        |b: usize| points(w, b) - points(a, b),
                        Set::empty(),
                        a,
                    )
                }
            },
    {
        let ghost s0 = *self;
        let ghost h0 = self.heap;
        let ghost ws = seq![a];
        let ghost c = s0.held_ok() && s0.is_unboxed(a) && s0.holds(a);
        let ghost z = |b: usize| 0int;
        let ghost none = Set::<usize>::empty();
        proof {
            assert(ws[0] == a);
            if c {
                s0.lemma_arg_ready(ws, 0);
            }
        }
        let n = match self.unboxed_length_of(a) {
            Ok(n) => n,
            Err(e) => return Err(Fault::Heap(e)),
        };
        self.heap.settle();
        let ghost s1 = *self;
        let ghost h1 = self.heap;
        proof {
            lemma_unboxed_settle(h1, h0, a);
            lemma_settled_twice(h0);
            assert(h1.words_kept(h0));
            if c {
                s1.lemma_unboxed_settled(s0, a);
            }
        }
        let unique = match unsafe { self.heap.is_unique(a) } {
            Ok(u) => u,
            Err(e) => {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if unique {
            proof {
                h1.lemma_unboxed_spec(a);
            }
            if let Err(e) = unsafe { self.heap.set_lin(a, lin) } {
                proof {
                    lemma_words_trans(self.heap, h1, h0);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, h1, h0);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_unboxed_relinked(s1, a, lin);
                    self.lemma_relinked_from(s0, s1, a);
                }
            }
            return Ok(a);
        }
        if M::CHECKED && !lin {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(HeapError::NotUnique));
        }
        proof {
            h1.lemma_unboxed_spec(a);
        }
        let t = self.heap.alloc_unboxed(n, 0, lin);
        let ghost h2 = self.heap;
        proof {
            lemma_unboxed_grew(h2, h1, a, t);
            h2.lemma_unboxed_spec(t);
            if M::CHECKED {
                lemma_words_trans(h2, h1, h0);
            }
        }
        if let Err(e) = unsafe { self.heap.unboxed_copy(t, a, n) } {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let ghost h3 = self.heap;
        proof {
            h3.lemma_unboxed_written(h2, t);
            if M::CHECKED {
                lemma_words_trans(h3, h2, h0);
            }
            if c {
                lemma_drop_ready(s1, h2, h3, a, t);
            }
        }
        if let Err(e) = unsafe { self.heap.decref(a) } {
            proof {
                lemma_words_trans(self.heap, h3, h0);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h3, h0);
                self.lemma_words_kept(s0);
            }
            if c {
                let d = |b: usize| points(t, b) - points(a, b);
                self.lemma_copied_ok(s1, h2, h3, a, t);
                lemma_moved_chain(self.heap, h1, h0, z, d, d, none, none, none);
                lemma_moved_to_lin(self.heap, h0, d, d, none, a.bits_spec());
            }
        }
        Ok(t)
    }

    /// `Std.MutUnboxedArray.freeze`。`m` の参照を受け取り、写さずに `lin` ビットを下ろして `UnboxedArray` として返す。
    ///
    /// # Safety
    /// `unboxed_set` と同じ (`a` が `m`)。検査モードは、一意でなければ `NotUnique` を返す。速いモードで共有の `m` を
    /// 受けたときは、写して返す。
    pub unsafe fn unboxed_freeze(&mut self, m: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(m),
            !M::CHECKED ==> old(self).is_unboxed(m) && old(self).holds(m),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_unboxed(m) && old(self).holds(m) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e)) || e
                    == HeapError::NotUnique
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_unboxed(w)
                    &&& final(self).unboxed_array_len(w) == old(self).unboxed_array_len(m)
                    &&& !final(self).unboxed_array_lin(w)
                    &&& forall|j: int|
                        0 <= j < old(self).unboxed_array_len(m)
                            ==> #[trigger] final(self).unboxed_array_at(w, j) == old(
                            self,
                        ).unboxed_array_at(m, j)
                    &&& final(self).evolved_lin(
                        *old(self),
                        |b: usize| points(w, b) - points(m, b),
                        Set::empty(),
                        m,
                    )
                }
            },
    {
        unsafe { self.relin_unboxed(m, false) }
    }

    /// `Std.UnboxedArray.thaw`。`a` の参照を受け取り、`lin` ビットを立てて `MutUnboxedArray` として返す。一意ならその場で
    /// 立て、共有なら写した物体に立てる。
    ///
    /// # Safety
    /// `unboxed_set` と同じ。検査モードも、共有の `a` は写す。
    pub unsafe fn unboxed_thaw(&mut self, a: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_unboxed(a) && old(self).holds(a),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_unboxed(a) && old(self).holds(a) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e))
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_unboxed(w)
                    &&& final(self).unboxed_array_len(w) == old(self).unboxed_array_len(a)
                    &&& final(self).unboxed_array_lin(w)
                    &&& forall|j: int|
                        0 <= j < old(self).unboxed_array_len(a)
                            ==> #[trigger] final(self).unboxed_array_at(w, j) == old(
                            self,
                        ).unboxed_array_at(a, j)
                    &&& final(self).evolved_lin(
                        *old(self),
                        |b: usize| points(w, b) - points(a, b),
                        Set::empty(),
                        a,
                    )
                }
            },
    {
        unsafe { self.relin_unboxed(a, true) }
    }
}

/// 一意な配列 `a` の要素 0..n をどれも `x` のビット列にする。
fn fill_elements<M: Mode>(heap: &mut Heap<M>, a: Word, n: usize, x: u64) -> (r: Result<
    (),
    HeapError,
>)
    requires
        old(heap).wf(),
        old(heap).settled(),
        a.is_ptr_spec(),
        old(heap).word_in(a),
        !M::CHECKED ==> old(heap).consistent() && old(heap).writable(a) && old(heap).is_unboxed(a)
            && n <= old(heap).unboxed_len_spec(a) && old(heap).owned(a),
    ensures
        final(heap).wf(),
        final(heap).settled(),
        *final(heap) == *old(heap) || final(heap).unboxed_written(*old(heap), a),
        M::CHECKED ==> final(heap).words_kept(*old(heap)),
        old(heap).consistent() && old(heap).writable(a) && old(heap).is_unboxed(a) && n <= old(
            heap,
        ).unboxed_len_spec(a) && old(heap).owned(a) ==> r is Ok && final(heap).consistent(),
        r is Ok ==> {
            &&& final(heap).is_unboxed(a) == old(heap).is_unboxed(a)
            &&& final(heap).lin_of(a) == old(heap).lin_of(a)
            &&& final(heap).unboxed_len_spec(a) == old(heap).unboxed_len_spec(a)
            &&& forall|j: int| 0 <= j < n ==> #[trigger] final(heap).unboxed_at(a, j) == x
        },
{
    let ghost h0 = *heap;
    let ghost c = h0.consistent() && h0.writable(a) && h0.is_unboxed(a) && n <= h0.unboxed_len_spec(
        a,
    ) && h0.owned(a);
    let mut k: usize = 0;
    while k < n
        invariant
            h0 == *old(heap),
            c == (h0.consistent() && h0.writable(a) && h0.is_unboxed(a) && n <= h0.unboxed_len_spec(
                a,
            ) && h0.owned(a)),
            heap.wf(),
            heap.settled(),
            *heap == h0 || heap.unboxed_written(h0, a),
            M::CHECKED ==> heap.words_kept(h0),
            heap.word_in(a),
            a.is_ptr_spec(),
            k <= n,
            !M::CHECKED ==> c,
            c ==> heap.consistent() && heap.writable(a) && n <= heap.unboxed_len_spec(a)
                && heap.owned(a),
            heap.is_unboxed(a) == h0.is_unboxed(a),
            heap.lin_of(a) == h0.lin_of(a),
            heap.unboxed_len_spec(a) == h0.unboxed_len_spec(a),
            forall|j: int| 0 <= j < k ==> #[trigger] heap.unboxed_at(a, j) == x,
        decreases n - k,
    {
        let ghost before = *heap;
        (unsafe { heap.unboxed_put(a, k, x) })?;
        proof {
            heap.lemma_unboxed_written(before, a);
            if before != h0 {
                heap.lemma_unboxed_written_trans(before, h0, a);
            }
            if M::CHECKED {
                lemma_words_trans(*heap, before, h0);
            }
        }
        k += 1;
    }
    Ok(())
}

} // verus!

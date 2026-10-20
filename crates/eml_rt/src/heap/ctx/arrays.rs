//! 配列の extern (docs/spec/object-model.md の「配列」)。`Array a` と `MutArray a` は同じ形の物体で、要素を走査する
//! 語として並べた tag 0 の `Data` である。`MutArray` は `lin` ビットを持つ。`get` と `length` は配列を借り、`set`、
//! `freeze`、`thaw` は配列を消費して返す。一意な配列はその場で書き換え、共有の配列は写す。

#[cfg(verus_only)]
use super::super::Core;
#[cfg(verus_only)]
use super::super::inv::{p_first, p_scanned};
#[cfg(verus_only)]
use super::super::model::lemma_indeg_pos;
#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::alloc::over_budget;
use crate::externs::{Fault, IndexFault};

verus! {

/// `a` は配列を指す。読める tag 0 の `Data` の物体で、持ち主が今の worker で、どの要素も `dup` できる。
spec fn array_ok<M: Mode>(h: Heap<M>, a: Word) -> bool {
    &&& a.is_ptr_spec()
    &&& h.readable(a)
    &&& h.kind_of(a) == Kind::Data
    &&& h.tag_of(a) == 0
    &&& h.owned(a)
    &&& forall|i: int| 0 <= i < h.count_of(a) ==> dupable(h, #[trigger] h.scanned_word(a, i))
}

/// 配列の形は配列を書かない変化で保たれる。
proof fn lemma_array_kept<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    x: Set<usize>,
    a: Word,
)
    requires
        moved(h1, h0, d, x),
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.settleable(),
        array_ok(h0, a),
        h1.readable(a),
        !x.contains(a.bits_spec()),
    ensures
        array_ok(h1, a),
        h1.count_of(a) == h0.count_of(a),
        h1.lin_of(a) == h0.lin_of(a),
        forall|i: int| #[trigger] h1.scanned_word(a, i) == h0.scanned_word(a, i),
{
    let b = a.bits_spec();
    assert(live(h0, b) && live(h1, b));
    assert(same_head(h1, h0, b));
    assert forall|i: int| #[trigger] h1.scanned_word(a, i) == h0.scanned_word(a, i) by {}
    assert forall|i: int| 0 <= i < h1.count_of(a) implies dupable(
        h1,
        #[trigger] h1.scanned_word(a, i),
    ) by {
        h0.lemma_readable_scanned(a, i);
        h1.lemma_readable_scanned(a, i);
        lemma_word_kept(h1, h0, d, x, h0.scanned_word(a, i));
    }
}

impl<M: Mode> Ctx<M> {
    /// `a` は配列を指す。
    pub closed spec fn is_array(&self, a: Word) -> bool {
        array_ok(self.heap, a)
    }

    /// 配列 `a` の長さ。
    pub closed spec fn array_len(&self, a: Word) -> int {
        self.heap.count_of(a)
    }

    /// 配列 `a` の i 番目の要素。
    pub closed spec fn array_at(&self, a: Word, i: int) -> Word {
        self.heap.scanned_word(a, i)
    }

    /// 配列 `a` は `lin` ビットを持つ (`MutArray`)。
    pub closed spec fn array_lin(&self, a: Word) -> bool {
        self.heap.lin_of(a)
    }

    /// 検査モードで、`a` が tag 0 の `Data` の物体を指すかを確かめる。違えば `WrongKind` を返す。
    fn checked_array(&self, a: Word) -> (r: Result<(), HeapError>)
        requires
            M::CHECKED,
            self.wf(),
            self.heap.word_in(a),
        ensures
            r is Ok ==> a.is_ptr_spec(),
            self.held_ok() && self.is_array(a) ==> r is Ok,
    {
        if !a.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        let m = (unsafe { self.heap.meta(a) })?;
        proof {
            m.lemma_fields();
        }
        if !matches!(m.kind(), Kind::Data) || m.tag() != 0 {
            return Err(HeapError::WrongKind);
        }
        Ok(())
    }

    /// 配列 `a` の長さを読む。`a` は借りる。
    fn length_of(&self, a: Word) -> (r: Result<usize, HeapError>)
        requires
            self.wf(),
            self.heap.word_in(a),
            !M::CHECKED ==> self.is_array(a),
        ensures
            r matches Ok(n) ==> n == self.array_len(a) && a.is_ptr_spec(),
            !M::CHECKED ==> r is Ok,
            self.held_ok() && self.is_array(a) ==> r is Ok,
    {
        if M::CHECKED {
            self.checked_array(a)?;
        }
        unsafe { self.heap.scanned_len(a) }
    }

    /// `Array.length` と `MutArray.length`。`a` は借りる。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`a` は即値か、このヒープか付けた塊の生きている物体を指す。速いモードでは、`a`
    /// は配列である (検査モードは形を確かめ、違えば `WrongKind` を返す)。
    pub unsafe fn array_length(&mut self, a: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_array(a),
        ensures
            *final(self) == *old(self),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_array(a) ==> r == Ok::<i64, Fault>(
                old(self).array_len(a) as i64,
            ),
    {
        match self.length_of(a) {
            Ok(n) => Ok(n as i64),
            Err(e) => Err(Fault::Heap(e)),
        }
    }

    /// `Array.get` と `MutArray.read`。`a` は借り、位置 `i` の要素を `dup` して返す。範囲の外なら、何も変えずに
    /// `Fault::Index` を返す。
    ///
    /// # Safety
    /// `array_length` と同じ。
    pub unsafe fn array_get(&mut self, a: Word, i: i64) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_array(a),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            !(r is Ok) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_array(a) ==> {
                &&& !(r matches Err(Fault::Heap(_)))
                &&& 0 <= i < old(self).array_len(a) ==> (r matches Ok(w) && {
                    &&& w == old(self).array_at(a, i as int)
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_array(a)
                    &&& final(self).array_len(a) == old(self).array_len(a)
                    &&& forall|j: int| #[trigger]
                        final(self).array_at(a, j) == old(self).array_at(a, j)
                    &&& final(self).evolved_by(*old(self), |b: usize| points(w, b), Set::empty())
                })
                &&& !(0 <= i < old(self).array_len(a)) ==> (r matches Err(Fault::Index(_)))
                    && *final(self) == *old(self)
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_array(a);
        let n = match self.length_of(a) {
            Ok(n) => n,
            Err(e) => return Err(Fault::Heap(e)),
        };
        if !(0 <= i && (i as u64) < (n as u64)) {
            return Err(Fault::Index(Box::new(IndexFault::Array { index: i, len: n as i64 })));
        }
        let w = match unsafe { self.heap.scanned(a, i as usize) } {
            Ok(w) => w,
            Err(e) => return Err(Fault::Heap(e)),
        };
        proof {
            if c {
                s0.heap.lemma_readable_scanned(a, i as int);
            }
        }
        if let Err(e) = unsafe { self.heap.dup(w) } {
            proof {
                lemma_settled_twice(s0.heap);
                if M::CHECKED {
                    self.lemma_words_kept(s0);
                }
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_elem_dupped(s0, w, a);
            }
        }
        Ok(w)
    }

    /// 配列から読んだ `w` を `dup` した後の、モデルが合っている道。
    proof fn lemma_elem_dupped(&self, s0: Ctx<M>, w: Word, a: Word)
        requires
            s0.held_ok(),
            array_ok(s0.heap, a),
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
            self.is_array(a),
            self.array_len(a) == s0.array_len(a),
            forall|j: int| #[trigger] self.array_at(a, j) == s0.array_at(a, j),
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
        lemma_readable_kept(self.heap, s0.heap, a);
        lemma_array_kept(self.heap, s0.heap, d, none, a);
    }
}

/// 新しい配列 `t`。`hs` になかった、長さ `n` の tag 0 の `Data` の物体で、呼ぶ側だけがその参照を持つ。
spec fn fresh_array<M: Mode>(hs: Heap<M>, h: Heap<M>, t: Word, n: int, lin: bool) -> bool {
    let tb = t.bits_spec();
    &&& t.is_ptr_spec()
    &&& h.word_in(t)
    &&& !live(hs, tb)
    &&& h.core.dom().contains(tb)
    &&& h.core.state(tb) == BlockState::Live
    &&& h.kind_of(t) == Kind::Data
    &&& h.tag_of(t) == 0
    &&& h.lin_of(t) == lin
    &&& h.count_of(t) == n
    &&& h.scan_of(t) == if n >= 255 {
        255u8
    } else {
        n as u8
    }
    &&& h.owned(t)
    &&& h.rc_value(t) > 0
    &&& h.core.settled_view().ext(tb) == 1
}

/// 新しい配列 `t` を埋める途中のヒープ。`hs` で生きていた物体の外の参照の数は `d` だけ変わり、`t` の要素の [0, i)
/// は `v` の語、[i, n) はまだ `()` である。
spec fn filling<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    t: Word,
    n: int,
    lin: bool,
    i: int,
    v: spec_fn(int) -> Word,
    d: spec_fn(usize) -> int,
) -> bool {
    &&& h.consistent()
    &&& h.settled()
    &&& moved(h, hs, d, Set::empty())
    &&& no_free(h, hs)
    &&& exact(h, hs, d)
    &&& fresh_array(hs, h, t, n, lin)
    &&& 0 <= i <= n
    &&& forall|j: int| 0 <= j < i ==> #[trigger] h.scanned_word(t, j) == v(j)
    &&& forall|j: int| i <= j < n ==> (#[trigger] h.scanned_word(t, j)).bits_spec() == 1
}

/// 埋め終えた新しい配列 `t`。要素は `v` の語である。
spec fn built<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    t: Word,
    n: int,
    lin: bool,
    v: spec_fn(int) -> Word,
    d: spec_fn(usize) -> int,
) -> bool {
    &&& h.consistent()
    &&& h.settled()
    &&& moved(h, hs, d, Set::empty())
    &&& fresh_array(hs, h, t, n, lin)
    &&& forall|j: int| 0 <= j < n ==> #[trigger] h.scanned_word(t, j) == v(j)
}

/// 写す元の配列 `a`。変化の後も同じ要素で生きている。
spec fn source_ok<M: Mode>(hs: Heap<M>, a: Word, d: spec_fn(usize) -> int, to: int) -> bool {
    let v = hs.core.settled_view();
    &&& a.is_ptr_spec()
    &&& hs.readable(a)
    &&& to <= hs.count_of(a)
    &&& v.rc_of(a.bits_spec()) == 0 || v.ext(a.bits_spec()) + d(a.bits_spec()) >= 1
}

/// 走査する語の始まりが `first` の物体の、走査する語と物理的な語の対応。
proof fn lemma_array_slots<M: Mode>(h: Heap<M>, t: Word, first: int)
    requires
        first == if h.scan_of(t) == 255 {
            1int
        } else {
            0int
        },
    ensures
        forall|j: int| #[trigger] h.scanned_word(t, j) == h.word_at(t, first + j),
        forall|j: int| #[trigger] h.scanned_at(t, j) <==> first <= j < first + h.count_of(t),
        first == 1 ==> h.count_of(t) == h.word_at(t, 0).bits_spec(),
        first == 0 ==> h.count_of(t) == h.scan_of(t),
{
}

/// `set_field` は書いた語のほかの語と `meta` を変えない。
proof fn lemma_slot_written<M: Mode>(h2: Heap<M>, h1: Heap<M>, t: Word, p: int, w: Word)
    requires
        h2.field_set(h1, t, p, w),
    ensures
        h2.word_at(t, p) == w,
        forall|j: int| j != p ==> #[trigger] h2.word_at(t, j) == h1.word_at(t, j),
        h2.meta_of(t) == h1.meta_of(t),
        h2.scan_of(t) == h1.scan_of(t),
        h2.kind_of(t) == h1.kind_of(t),
        h2.tag_of(t) == h1.tag_of(t),
        h2.lin_of(t) == h1.lin_of(t),
{
    let tb = t.bits_spec();
    assert(h2.core.slot_changed(h1.core, tb, p));
    assert forall|j: int| j != p implies #[trigger] h2.word_at(t, j) == h1.word_at(t, j) by {
        assert(h2.core.perm(tb).slots[j] == h1.core.perm(tb).slots[j]);
    }
}

/// `alloc_data` で作ったばかりの配列は埋め始めのヒープである。
proof fn lemma_fill_start<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    t: Word,
    n: int,
    lin: bool,
    v: spec_fn(int) -> Word,
)
    requires
        hs.consistent(),
        hs.settleable(),
        h.made(hs, t),
        h.kind_of(t) == Kind::Data,
        h.count_of(t) == n,
        h.tag_of(t) == 0,
        h.scan_of(t) == if n >= 255 {
            255u8
        } else {
            n as u8
        },
        h.lin_of(t) == lin,
        h.owned(t),
        forall|j: int| 0 <= j < n ==> (#[trigger] h.scanned_word(t, j)).bits_spec() == 1,
    ensures
        filling(hs, h, t, n, lin, 0, v, |b: usize| 0int),
{
    lemma_alloc_moved(h, hs, t);
}

/// 埋める途中のヒープで、`hs` で読めて `dup` できた語は今も読めて `dup` できる。
proof fn lemma_fill_word<M: Mode>(hs: Heap<M>, h: Heap<M>, d: spec_fn(usize) -> int, w: Word)
    requires
        h.consistent(),
        h.settled(),
        hs.consistent(),
        hs.settleable(),
        moved(h, hs, d, Set::empty()),
        no_free(h, hs),
        hs.readable(w),
        dupable(hs, w),
    ensures
        h.readable(w),
        dupable(h, w),
        h.dup_ok(w),
{
    lemma_readable_kept(h, hs, w);
    lemma_word_kept(h, hs, d, Set::empty(), w);
}

/// 写す元の配列は埋める途中のヒープでも同じ要素で読める。
proof fn lemma_source_kept<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    d: spec_fn(usize) -> int,
    a: Word,
    to: int,
)
    requires
        moved(h, hs, d, Set::empty()),
        source_ok(hs, a, d, to),
    ensures
        h.readable(a),
        h.count_of(a) == hs.count_of(a),
        forall|j: int| #[trigger] h.scanned_word(a, j) == hs.scanned_word(a, j),
{
    let b = a.bits_spec();
    assert(live(hs, b));
    assert(live(h, b));
    assert(same_head(h, hs, b));
    assert forall|j: int| #[trigger] h.scanned_word(a, j) == hs.scanned_word(a, j) by {}
}

/// 埋める途中の配列の i 番目の欄には、`set_field` で書ける。
proof fn lemma_fill_ready<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    t: Word,
    n: int,
    lin: bool,
    i: int,
    v: spec_fn(int) -> Word,
    d: spec_fn(usize) -> int,
    first: int,
)
    requires
        filling(hs, h, t, n, lin, i, v, d),
        i < n,
        first == if n >= 255 {
            1int
        } else {
            0int
        },
    ensures
        h.writable(t),
        h.owned(t),
        h.scanned_at(t, first + i),
        h.word_at(t, first + i).bits_spec() == 1,
{
    lemma_array_slots(h, t, first);
    assert(h.scanned_word(t, i).bits_spec() == 1);
}

/// 埋める途中のヒープで `w` を `dup` した後、`w` の参照を呼ぶ側が持ち、配列には書ける。
proof fn lemma_fill_dupped<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    h1: Heap<M>,
    t: Word,
    n: int,
    lin: bool,
    i: int,
    v: spec_fn(int) -> Word,
    d: spec_fn(usize) -> int,
    w: Word,
    first: int,
)
    requires
        filling(hs, h, t, n, lin, i, v, d),
        i < n,
        first == if n >= 255 {
            1int
        } else {
            0int
        },
        hs.consistent(),
        hs.settleable(),
        hs.readable(w),
        h.readable(w),
        dupable(h, w),
        h1.bumped(h, w),
        h1.settled(),
        h1.consistent(),
    ensures
        h1.writable(t),
        h1.owned(t),
        h1.scanned_at(t, first + i),
        h1.holds(w),
        h1.word_in(t),
{
    lemma_fill_ready(hs, h, t, n, lin, i, v, d, first);
    lemma_dup_moved(h1, h, w);
    lemma_readable_kept(h, hs, w);
    lemma_ext_nonneg_word(h, w);
    lemma_arg_moved(h1, h, |b: usize| points(w, b), Set::empty(), w);
    let tb = t.bits_spec();
    assert(live(h, tb));
    assert(live(h1, tb));
    assert(same_head(h1, h, tb));
}

/// `w` を `dup` して i 番目の欄に書いた後も、埋める途中のヒープである。
proof fn lemma_fill_step<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    h1: Heap<M>,
    h2: Heap<M>,
    t: Word,
    n: int,
    lin: bool,
    i: int,
    v: spec_fn(int) -> Word,
    d: spec_fn(usize) -> int,
    w: Word,
    first: int,
)
    requires
        filling(hs, h, t, n, lin, i, v, d),
        i < n,
        w == v(i),
        first == if n >= 255 {
            1int
        } else {
            0int
        },
        hs.readable(w),
        h1.bumped(h, w),
        h1.settled(),
        h1.consistent(),
        h2.field_set(h1, t, first + i, w),
        h2.consistent(),
    ensures
        filling(hs, h2, t, n, lin, i + 1, v, d),
{
    let tb = t.bits_spec();
    let none = Set::<usize>::empty();
    let pw = |b: usize| points(w, b);
    let dp = |b: usize| d(b) + points(w, b);
    lemma_fill_ready(hs, h, t, n, lin, i, v, d, first);
    lemma_dup_moved(h1, h, w);
    lemma_moved_chain(h1, h, hs, d, pw, dp, none, none, none);
    assert(live(h, tb));
    assert(live(h1, tb));
    assert(same_head(h1, h, tb));
    assert(h1.core.perm(tb).slots == h.core.perm(tb).slots);
    assert(h1.core.dom().contains(tb));
    lemma_set_unit_moved(h2, h1, t, first + i, w);
    lemma_moved_chain(h2, h1, hs, dp, |b: usize| -points(w, b), d, none, none.insert(tb), none);
    assert(live(h2, tb));
    assert(same_head(h2, h1, tb));
    // `w` は `hs` で読めて、`t` は `hs` になかったので、`w` は `t` を指さない
    assert(points(w, tb) == 0);
    lemma_slot_written(h2, h1, t, first + i, w);
    lemma_array_slots(h, t, first);
    lemma_array_slots(h1, t, first);
    lemma_array_slots(h2, t, first);
    if first == 1 {
        assert(h2.word_at(t, 0) == h1.word_at(t, 0));
    }
    assert(h2.count_of(t) == n);
    assert forall|j: int| 0 <= j < i + 1 implies #[trigger] h2.scanned_word(t, j) == v(j) by {
        if j != i {
            assert(h2.word_at(t, first + j) == h1.word_at(t, first + j));
            assert(h.scanned_word(t, j) == v(j));
        }
    }
    assert forall|j: int| i + 1 <= j < n implies (#[trigger] h2.scanned_word(t, j)).bits_spec()
        == 1 by {
        assert(h2.word_at(t, first + j) == h1.word_at(t, first + j));
        assert(h.scanned_word(t, j).bits_spec() == 1);
    }
    assert(h2.word_in(t));
}

/// `w` を `dup` せずに i 番目の欄に書いた後も、埋める途中のヒープである。`w` の外の参照は配列に移る。
proof fn lemma_fill_put<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    h2: Heap<M>,
    t: Word,
    n: int,
    lin: bool,
    i: int,
    v: spec_fn(int) -> Word,
    d: spec_fn(usize) -> int,
    e: spec_fn(usize) -> int,
    w: Word,
    first: int,
)
    requires
        filling(hs, h, t, n, lin, i, v, d),
        i < n,
        w == v(i),
        first == if n >= 255 {
            1int
        } else {
            0int
        },
        hs.readable(w),
        h2.field_set(h, t, first + i, w),
        h2.consistent(),
        forall|b: usize| live(hs, b) ==> #[trigger] e(b) == d(b) - points(w, b),
    ensures
        filling(hs, h2, t, n, lin, i + 1, v, e),
{
    let tb = t.bits_spec();
    let none = Set::<usize>::empty();
    lemma_fill_ready(hs, h, t, n, lin, i, v, d, first);
    lemma_set_unit_moved(h2, h, t, first + i, w);
    lemma_moved_chain(h2, h, hs, d, |b: usize| -points(w, b), e, none, none.insert(tb), none);
    assert(live(h, tb));
    assert(live(h2, tb));
    assert(same_head(h2, h, tb));
    assert(points(w, tb) == 0);
    lemma_slot_written(h2, h, t, first + i, w);
    lemma_array_slots(h, t, first);
    lemma_array_slots(h2, t, first);
    if first == 1 {
        assert(h2.word_at(t, 0) == h.word_at(t, 0));
    }
    assert(h2.count_of(t) == n);
    assert forall|j: int| 0 <= j < i + 1 implies #[trigger] h2.scanned_word(t, j) == v(j) by {
        if j != i {
            assert(h2.word_at(t, first + j) == h.word_at(t, first + j));
            assert(h.scanned_word(t, j) == v(j));
        }
    }
    assert forall|j: int| i + 1 <= j < n implies (#[trigger] h2.scanned_word(t, j)).bits_spec()
        == 1 by {
        assert(h2.word_at(t, first + j) == h.word_at(t, first + j));
        assert(h.scanned_word(t, j).bits_spec() == 1);
    }
    assert(h2.word_in(t));
}

/// 埋めた配列のヒープで、元の物体 `w` の参照を1つ手放した後も、配列は同じ要素で生きている。
proof fn lemma_built_released<M: Mode>(
    hs: Heap<M>,
    h: Heap<M>,
    h2: Heap<M>,
    t: Word,
    n: int,
    lin: bool,
    v: spec_fn(int) -> Word,
    d: spec_fn(usize) -> int,
    e: spec_fn(usize) -> int,
    w: Word,
)
    requires
        filling(hs, h, t, n, lin, n, v, d),
        hs.readable(w),
        h2.consistent(),
        h2.settled(),
        h2.evolved(h, w, -1),
        h2.kept_live(h),
        forall|b: usize| live(hs, b) ==> #[trigger] e(b) == d(b) - points(w, b),
    ensures
        built(hs, h2, t, n, lin, v, e),
{
    let tb = t.bits_spec();
    let none = Set::<usize>::empty();
    lemma_decref_moved(h2, h, w);
    lemma_moved_chain(h2, h, hs, d, |b: usize| -points(w, b), e, none, none, none);
    assert(live(h, tb));
    assert(points(w, tb) == 0);
    assert(live(h2, tb));
    assert(same_head(h2, h, tb));
    assert(h2.core.perm(tb).slots == h.core.perm(tb).slots);
    assert forall|j: int| 0 <= j < n implies #[trigger] h2.scanned_word(t, j) == v(j) by {
        assert(h.scanned_word(t, j) == v(j));
    }
    assert(h2.core.dom().contains(tb));
    assert(h2.word_in(t));
}

/// 新しい物体 `t` の要素 [from, to) に、`src` の同じ位置の要素を、`src` がなければ `x` を、`dup` して入れる。
/// `first` は走査する語の始まり (数の語の分のずれ) である。
#[expect(clippy::too_many_arguments, reason = "half of them are ghost arguments of the proof")]
fn fill<M: Mode>(
    heap: &mut Heap<M>,
    t: Word,
    first: usize,
    from: usize,
    to: usize,
    src: Option<Word>,
    x: Word,
    Ghost(hs): Ghost<Heap<M>>,
    Ghost(n): Ghost<int>,
    Ghost(lin): Ghost<bool>,
    Ghost(v): Ghost<spec_fn(int) -> Word>,
    Ghost(d): Ghost<spec_fn(usize) -> int>,
    Ghost(c): Ghost<bool>,
) -> (r: Result<(), HeapError>)
    requires
        old(heap).wf(),
        old(heap).settled(),
        !M::CHECKED ==> c,
        t.is_ptr_spec(),
        old(heap).word_in(t),
        src matches Some(a) ==> a.is_ptr_spec() && old(heap).word_in(a),
        old(heap).word_in(x),
        from <= to,
        first == if n >= 255 {
            1usize
        } else {
            0usize
        },
        c ==> {
            &&& hs.consistent()
            &&& hs.settleable()
            &&& filling(hs, *old(heap), t, n, lin, from as int, v, d)
            &&& to <= n
            &&& forall|j: int|
                from <= j < to ==> #[trigger] v(j) == match src {
                    Some(a) => hs.scanned_word(a, j),
                    None => x,
                }
            &&& forall|j: int| from <= j < to ==> hs.readable(#[trigger] v(j)) && dupable(hs, v(j))
            &&& src matches Some(a) ==> source_ok(hs, a, d, to as int)
        },
    ensures
        final(heap).wf(),
        final(heap).settled(),
        final(heap).word_in(t),
        src matches Some(a) ==> final(heap).word_in(a),
        final(heap).word_in(x),
        M::CHECKED ==> final(heap).words_kept(*old(heap)),
        c ==> r is Ok && filling(hs, *final(heap), t, n, lin, to as int, v, d),
{
    let ghost h0 = *heap;
    let mut i = from;
    while i < to
        invariant
            heap.wf(),
            heap.settled(),
            h0 == *old(heap),
            !M::CHECKED ==> c,
            t.is_ptr_spec(),
            heap.word_in(t),
            src matches Some(a) ==> a.is_ptr_spec() && heap.word_in(a),
            heap.word_in(x),
            from <= i <= to,
            first == if n >= 255 {
                1usize
            } else {
                0usize
            },
            M::CHECKED ==> heap.words_kept(h0),
            c ==> {
                &&& hs.consistent()
                &&& hs.settleable()
                &&& filling(hs, *heap, t, n, lin, i as int, v, d)
                &&& to <= n
                &&& forall|j: int|
                    from <= j < to ==> #[trigger] v(j) == match src {
                        Some(a) => hs.scanned_word(a, j),
                        None => x,
                    }
                &&& forall|j: int|
                    from <= j < to ==> hs.readable(#[trigger] v(j)) && dupable(hs, v(j))
                &&& src matches Some(a) ==> source_ok(hs, a, d, to as int)
            },
        decreases to - i,
    {
        let ghost h = *heap;
        proof {
            if c {
                if let Some(a) = src {
                    lemma_source_kept(hs, h, d, a, to as int);
                }
                assert(v(i as int) == match src {
                    Some(a) => hs.scanned_word(a, i as int),
                    None => x,
                });
            }
        }
        let w = match src {
            Some(a) => (unsafe { heap.scanned(a, i) })?,
            None => x,
        };
        proof {
            if c {
                assert(hs.readable(v(i as int)));
                lemma_fill_word(hs, h, d, w);
            }
        }
        if let Err(e) = unsafe { heap.dup(w) } {
            proof {
                if M::CHECKED {
                    lemma_words_trans(*heap, h, h0);
                }
            }
            return Err(e);
        }
        let ghost h1 = *heap;
        proof {
            if M::CHECKED {
                lemma_words_trans(h1, h, h0);
            }
            if c {
                lemma_fill_dupped(hs, h, h1, t, n, lin, i as int, v, d, w, first as int);
            }
        }
        (unsafe { heap.set_field(t, i + first, w) })?;
        proof {
            if M::CHECKED {
                lemma_words_trans(*heap, h1, h0);
            }
            if c {
                lemma_fill_step(hs, h, h1, *heap, t, n, lin, i as int, v, d, w, first as int);
            }
        }
        i += 1;
    }
    Ok(())
}

/// `make` で作れる長さの上限。これより長い配列のブロックは、確保器が作れる語の数 (2^32 未満) を超えうる。
const MAX_LEN: usize = 0xFFFF_0000;

impl<M: Mode> Ctx<M> {
    /// 配列の要素に入れられる値。`dup` できる。
    pub closed spec fn storable(&self, x: Word) -> bool {
        dupable(self.heap, x)
    }

    /// 呼ぶ側が持つ引数 `ws[i]` は、読めて、`decref` できる。
    proof fn lemma_arg_ready(&self, ws: Seq<Word>, i: int)
        requires
            self.held_ok(),
            self.holds_args(ws),
            0 <= i < ws.len(),
        ensures
            self.heap.readable(ws[i]),
            self.heap.owned(ws[i]),
            self.heap.decref_ok(ws[i]),
            ws[i].is_ptr_spec() && self.heap.core.settled_view().rc_of(ws[i].bits_spec()) != 0
                ==> self.heap.core.settled_view().ext(ws[i].bits_spec()) >= points_in(
                ws,
                ws[i].bits_spec(),
            ),
    {
        self.lemma_arg_counted(ws, i);
        lemma_points_at(ws, i, ws[i].bits_spec());
        lemma_moved_refl(self.heap);
        lemma_arg_moved(self.heap, self.heap, |b: usize| 0int, Set::empty(), ws[i]);
    }

    /// `old` から `moved(d)` で変えたヒープで、`Ctx` の語は持たれたままで、`evv` も同じリストである。`d` は、引数
    /// `ws` の参照を手放す分を超えて減らさない。
    proof fn lemma_kept_ok(&self, old: Ctx<M>, ws: Seq<Word>, d: spec_fn(usize) -> int)
        requires
            old.held_ok(),
            old.holds_args(ws),
            self.heap.consistent(),
            self.heap.settleable(),
            self.evv == old.evv,
            self.yielding == old.yielding,
            self.max_evv == old.max_evv,
            moved(self.heap, old.heap, d, Set::empty()),
            forall|b: usize| #[trigger] d(b) >= -points_in(ws, b),
        ensures
            self.held_ok(),
            self.evolved_by(old, d, Set::empty()),
    {
        let none = Set::<usize>::empty();
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies old.need(ws, b) + d(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            assert(d(b) >= -points_in(ws, b));
        }
        self.lemma_holds_moved(old, ws, seq![], d, none);
        self.lemma_evv_kept(old, d, none);
    }

    /// 埋め終えた新しい配列 `t` を返す道。`Ctx` の語は持たれたままで、呼ぶ側が `t` の参照を持ち、`t` は要素が `v`
    /// の配列である。
    proof fn lemma_made(
        &self,
        old: Ctx<M>,
        ws: Seq<Word>,
        t: Word,
        n: int,
        lin: bool,
        v: spec_fn(int) -> Word,
        d: spec_fn(usize) -> int,
    )
        requires
            old.held_ok(),
            old.holds_args(ws),
            built(old.heap, self.heap, t, n, lin, v, d),
            self.evv == old.evv,
            self.yielding == old.yielding,
            self.max_evv == old.max_evv,
            forall|b: usize| #[trigger] d(b) >= -points_in(ws, b),
            forall|j: int|
                0 <= j < n ==> old.heap.readable(#[trigger] v(j)) && dupable(old.heap, v(j)),
        ensures
            self.held_ok(),
            self.holds(t),
            self.is_array(t),
            self.array_len(t) == n,
            self.array_lin(t) == lin,
            forall|j: int| 0 <= j < n ==> #[trigger] self.array_at(t, j) == v(j),
            self.evolved_by(old, d, Set::empty()),
    {
        let h = self.heap;
        self.lemma_kept_ok(old, ws, d);
        assert forall|j: int| 0 <= j < h.count_of(t) implies dupable(
            h,
            #[trigger] h.scanned_word(t, j),
        ) by {
            assert(h.scanned_word(t, j) == v(j));
            h.lemma_readable_scanned(t, j);
            lemma_word_kept(h, old.heap, d, Set::empty(), v(j));
        }
        assert(array_ok(h, t));
        self.lemma_fresh_holds(old, t);
    }

    /// 引数 `x` の参照を手放して、誤り `f` を返す。
    unsafe fn refuse(&mut self, x: Word, f: Fault) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(x),
            !M::CHECKED ==> old(self).holds(x),
            !(f matches Fault::Heap(_)),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r == Err::<Word, Fault>(f) || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r == Err::<Word, Fault>(f),
            old(self).held_ok() && old(self).holds(x) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e))
                &&& !(r matches Err(Fault::Heap(_))) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |b: usize| -points(x, b), Set::empty())
                }
            },
    {
        let ghost s0 = *self;
        let ghost ws = seq![x];
        let ghost c = s0.held_ok() && s0.holds(x);
        proof {
            assert(ws[0] == x);
            if c {
                s0.lemma_arg_ready(ws, 0);
                assert forall|b: usize| #[trigger] -points(x, b) >= -points_in(ws, b) by {
                    lemma_points_in(ws, b);
                }
            }
        }
        if let Err(e) = unsafe { self.heap.decref(x) } {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if c {
                lemma_decref_moved(self.heap, s0.heap, x);
                self.lemma_kept_ok(s0, ws, |b: usize| -points(x, b));
            }
        }
        Err(f)
    }

    /// `Array.make` と `MutArray.make`。`x` の参照を受け取り、どの要素も `x` の、長さ `n` の新しい配列を返す。
    /// `MutArray` なら `lin` を真にする。`n` が負なら、`x` を手放して `Fault::Index` を返す。`n` が 0 なら `x` を手放す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`x` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。速いモードでは、`x` は `dup` できる (検査モードは `dup` が確かめる)。
    pub unsafe fn array_make(&mut self, n: i64, x: Word, lin: bool) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(x),
            !M::CHECKED ==> old(self).holds(x) && old(self).storable(x),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || r == Err::<Word, Fault>(
                Fault::Stopped(Stop::MemoryLimit),
            ) || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).holds(x) && old(self).storable(x) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e))
                &&& !(r matches Err(Fault::Heap(_))) ==> {
                    &&& n < 0 ==> (r matches Err(Fault::Index(_)))
                    &&& n >= 0 && !(r matches Err(Fault::Stopped(_))) ==> (r matches Ok(w) && {
                        &&& final(self).holds(w)
                        &&& final(self).is_array(w)
                        &&& final(self).array_len(w) == n
                        &&& final(self).array_lin(w) == lin
                        &&& forall|j: int| 0 <= j < n ==> #[trigger] final(self).array_at(w, j) == x
                    })
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |b: usize| -points(x, b), Set::empty())
                }
            },
    {
        let ghost s0 = *self;
        let ghost hs = self.heap;
        let ghost ws = seq![x];
        let ghost c = s0.held_ok() && s0.holds(x) && dupable(s0.heap, x);
        let ghost v = |j: int| x;
        let ghost z = |b: usize| 0int;
        let ghost dx = |b: usize| -points(x, b);
        proof {
            assert(ws[0] == x);
            if c {
                s0.lemma_arg_ready(ws, 0);
                assert forall|b: usize| #[trigger] dx(b) >= -points_in(ws, b) by {
                    lemma_points_in(ws, b);
                }
            }
        }
        if n < 0 {
            return unsafe { self.refuse(x, Fault::Index(Box::new(IndexFault::Length { len: n }))) };
        }
        let k = n as usize;
        // 確保器が作れない大きさと、確保量の上限を超える大きさは、確保を試す前に止める。確保器に任せると、プロセスが
        // panic で落ちるか、上限を報告する前に OS に止められるためである (docs/spec/object-model.md の「配列」)
        if k > MAX_LEN || over_budget(&self.control, (k + 4) * 8) {
            return unsafe { self.refuse(x, Fault::Stopped(Stop::MemoryLimit)) };
        }
        let first: usize = if k >= 255 {
            1
        } else {
            0
        };
        // 確保より前に確かめる。`x` が `lin` なら、`dup` が `LinDup` を返すためである
        let checked = if M::CHECKED && k > 1 {
            self.heap.dup_check(x)
        } else {
            Ok(())
        };
        if let Err(e) = checked {
            return Err(Fault::Heap(e));
        }
        let t = self.heap.alloc_data(0, k, 0, lin);
        let ghost h1 = self.heap;
        proof {
            if M::CHECKED {
                assert(h1.words_kept(hs));
            }
            if c {
                lemma_fill_start(hs, h1, t, k as int, lin, v);
                assert(filling(hs, h1, t, k as int, lin, 0, v, z));
                assert(hs.readable(x));
            }
        }
        if k == 0 {
            proof {
                if c {
                    lemma_arg_moved(h1, hs, z, Set::empty(), x);
                }
            }
            if let Err(e) = unsafe { self.heap.decref(x) } {
                proof {
                    lemma_words_trans(self.heap, h1, hs);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, h1, hs);
                    self.lemma_words_kept(s0);
                }
                if c {
                    lemma_built_released(hs, h1, self.heap, t, 0, lin, v, z, dx, x);
                    self.lemma_made(s0, ws, t, 0, lin, v, dx);
                }
            }
            return Ok(t);
        }
        let last = k - 1;
        let r = fill(
            &mut self.heap,
            t,
            first,
            0,
            last,
            None,
            x,
            Ghost(hs),
            Ghost(k as int),
            Ghost(lin),
            Ghost(v),
            Ghost(z),
            Ghost(c),
        );
        let ghost h2 = self.heap;
        if let Err(e) = r {
            proof {
                lemma_words_trans(h2, h1, hs);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if c {
                lemma_fill_ready(hs, h2, t, k as int, lin, last as int, v, z, first as int);
                lemma_readable_kept(h2, hs, x);
                lemma_arg_moved(h2, hs, z, Set::empty(), x);
            }
        }
        // 最後の要素には、受け取った `x` の参照をそのまま入れる
        if let Err(e) = unsafe { self.heap.set_field(t, last + first, x) } {
            proof {
                lemma_words_trans(h2, h1, hs);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if M::CHECKED {
                lemma_words_trans(h2, h1, hs);
                lemma_words_trans(self.heap, h2, hs);
                self.lemma_words_kept(s0);
            }
            if c {
                lemma_fill_put(
                    hs,
                    h2,
                    self.heap,
                    t,
                    k as int,
                    lin,
                    last as int,
                    v,
                    z,
                    dx,
                    x,
                    first as int,
                );
                self.lemma_made(s0, ws, t, k as int, lin, v, dx);
            }
        }
        Ok(t)
    }
}

/// rc が 1 の読める物体は、このヒープのブロックで、数えた参照は 1 つである。呼ぶ側が外の参照を持てば、ほかの物体
/// から辺で指されない。
proof fn lemma_unique<M: Mode>(h: Heap<M>, w: Word)
    requires
        h.consistent(),
        h.settleable(),
        w.is_ptr_spec(),
        h.readable(w),
        h.rc_value(w) == 1,
    ensures
        h.core.dom().contains(w.bits_spec()),
        live(h, w.bits_spec()),
        h.core.settled_view().rc_of(w.bits_spec()) == 1,
        h.core.settled_view().ext(w.bits_spec()) <= 1,
        h.core.settled_view().ext(w.bits_spec()) >= 1 ==> h.core.settled_view().indeg(w.bits_spec())
            == 0,
{
    let b = w.bits_spec();
    let v = h.core.settled_view();
    h.core.lemma_settled_view();
    if !h.core.dom().contains(b) {
        h.core.lemma_region_at(b);
    }
    reveal(Core::refs_wf);
    assert(v.dom().contains(b));
    assert(v.counted_wf(b));
}

impl<M: Mode> Ctx<M> {
    /// 範囲の外の `set` の道。`a` と `x` を手放した後も、`Ctx` の語は持たれたままである。
    proof fn lemma_set_dropped(&self, s0: Ctx<M>, h1: Heap<M>, a: Word, x: Word)
        requires
            s0.held_ok(),
            s0.holds_args(seq![a, x]),
            h1.consistent(),
            h1.settled(),
            h1.evolved(s0.heap, a, -1),
            h1.kept_live(s0.heap),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.evolved(h1, x, -1),
            self.heap.kept_live(h1),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.evolved_by(s0, |b: usize| -points(a, b) - points(x, b), Set::empty()),
    {
        let ws = seq![a, x];
        let none = Set::<usize>::empty();
        let d = |b: usize| -points(a, b) - points(x, b);
        lemma_decref_moved(h1, s0.heap, a);
        lemma_decref_moved(self.heap, h1, x);
        lemma_moved_chain(
            self.heap,
            h1,
            s0.heap,
            |b: usize| -points(a, b),
            |b: usize| -points(x, b),
            d,
            none,
            none,
            none,
        );
        assert forall|b: usize| #[trigger] d(b) >= -points_in(ws, b) by {
            lemma_points_in(ws, b);
        }
        self.lemma_kept_ok(s0, ws, d);
    }

    /// 範囲の外の `set` で `a` を手放した後、`x` もまだ手放せる。
    proof fn lemma_set_drop_mid(s0: Ctx<M>, h1: Heap<M>, a: Word, x: Word)
        requires
            s0.held_ok(),
            s0.holds_args(seq![a, x]),
            h1.consistent(),
            h1.settled(),
            h1.evolved(s0.heap, a, -1),
            h1.kept_live(s0.heap),
        ensures
            h1.decref_ok(x),
    {
        let ws = seq![a, x];
        s0.lemma_arg_counted(ws, 1);
        lemma_points_in(ws, x.bits_spec());
        lemma_decref_moved(h1, s0.heap, a);
        lemma_arg_moved(h1, s0.heap, |b: usize| -points(a, b), Set::empty(), x);
    }

    /// その場の `set` で、`settle` の後の `h1` に `x` を書ける。
    proof fn lemma_set_ready(s0: Ctx<M>, h1: Heap<M>, a: Word, idx: int, p: int, x: Word)
        requires
            s0.held_ok(),
            array_ok(s0.heap, a),
            s0.holds_args(seq![a, x]),
            dupable(s0.heap, x),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.settled(),
            s0.heap.rc_value(a) == 1,
            0 <= idx < s0.heap.count_of(a),
            p == idx + if s0.heap.scan_of(a) == 255 {
                1int
            } else {
                0int
            },
        ensures
            h1.writable(a),
            h1.owned(a),
            h1.holds(x),
            h1.scanned_at(a, p),
            h1.core.dom().contains(a.bits_spec()),
            h1.core.state(a.bits_spec()) == BlockState::Live,
            h1.word_at(a, p) == s0.heap.scanned_word(a, idx),
            points(x, a.bits_spec()) == 0,
    {
        let ws = seq![a, x];
        let h0 = s0.heap;
        let ab = a.bits_spec();
        lemma_settle_moved(h1, h0);
        lemma_unique(h0, a);
        s0.lemma_arg_counted(ws, 0);
        s0.lemma_arg_counted(ws, 1);
        lemma_points_in(ws, ab);
        lemma_points_in(ws, x.bits_spec());
        assert(points(x, ab) == 0);
        lemma_arg_moved(h1, h0, |b: usize| 0int, Set::empty(), x);
        assert(live(h0, ab) && live(h1, ab));
        assert(same_head(h1, h0, ab));
        assert(h1.core.perm(ab).slots == h0.core.perm(ab).slots);
        let first = if h0.scan_of(a) == 255 {
            1int
        } else {
            0int
        };
        lemma_array_slots(h0, a, first);
        lemma_array_slots(h1, a, first);
    }

    /// その場の `set` で `x` を書いた後、前の要素 `old` を手放せる。
    proof fn lemma_set_old_ready(
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        a: Word,
        idx: int,
        p: int,
        x: Word,
        old: Word,
    )
        requires
            s0.held_ok(),
            array_ok(s0.heap, a),
            s0.holds_args(seq![a, x]),
            dupable(s0.heap, x),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.settled(),
            s0.heap.rc_value(a) == 1,
            0 <= idx < s0.heap.count_of(a),
            p == idx + if s0.heap.scan_of(a) == 255 {
                1int
            } else {
                0int
            },
            old == h1.word_at(a, p),
            h2.field_set(h1, a, p, x),
            h2.consistent(),
        ensures
            h2.decref_ok(old),
    {
        let ws = seq![a, x];
        let h0 = s0.heap;
        let none = Set::<usize>::empty();
        Self::lemma_set_ready(s0, h1, a, idx, p, x);
        lemma_settle_moved(h1, h0);
        h0.lemma_readable_scanned(a, idx);
        assert(old == h0.scanned_word(a, idx));
        lemma_readable_kept(h1, h0, old);
        lemma_word_kept(h1, h0, |b: usize| 0int, none, old);
        lemma_set_moved(h2, h1, a, p, x);
        lemma_ext_nonneg_word(h1, old);
        s0.lemma_arg_counted(ws, 1);
        lemma_points_in(ws, x.bits_spec());
        lemma_arg_moved(h1, h0, |b: usize| 0int, none, x);
        if old.is_ptr_spec() && h1.core.settled_view().rc_of(old.bits_spec()) != 0 {
            assert(live(h1, old.bits_spec()));
            if points(x, old.bits_spec()) > 0 {
                assert(h1.core.settled_view().ext(old.bits_spec()) >= 1);
            }
        }
        lemma_arg_moved(
            h2,
            h1,
            |b: usize| points(h1.word_at(a, p), b) - points(x, b),
            none.insert(a.bits_spec()),
            old,
        );
    }

    /// その場の `set` の、モデルが合っている道。
    proof fn lemma_set_in_place(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        a: Word,
        idx: int,
        p: int,
        x: Word,
        old: Word,
    )
        requires
            s0.held_ok(),
            array_ok(s0.heap, a),
            s0.holds_args(seq![a, x]),
            dupable(s0.heap, x),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.settled(),
            s0.heap.rc_value(a) == 1,
            0 <= idx < s0.heap.count_of(a),
            p == idx + if s0.heap.scan_of(a) == 255 {
                1int
            } else {
                0int
            },
            old == h1.word_at(a, p),
            h2.field_set(h1, a, p, x),
            h2.consistent(),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.evolved(h2, old, -1),
            self.heap.kept_live(h2),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(a),
            self.is_array(a),
            self.array_len(a) == s0.array_len(a),
            self.array_lin(a) == s0.array_lin(a),
            self.array_at(a, idx) == x,
            forall|j: int|
                0 <= j < s0.array_len(a) && j != idx ==> #[trigger] self.array_at(a, j)
                    == s0.array_at(a, j),
            self.evolved_by(
                s0,
                |b: usize| points(a, b) - points(a, b) - points(x, b),
                Set::empty().insert(a.bits_spec()),
            ),
    {
        let ws = seq![a, x];
        let h0 = s0.heap;
        let h3 = self.heap;
        let ab = a.bits_spec();
        let none = Set::<usize>::empty();
        let xa = none.insert(ab);
        let z = |b: usize| 0int;
        let d2 = |b: usize| points(old, b) - points(x, b);
        let dx = |b: usize| -points(x, b);
        Self::lemma_set_ready(s0, h1, a, idx, p, x);
        lemma_settle_moved(h1, h0);
        lemma_set_moved(h2, h1, a, p, x);
        lemma_moved_chain(h2, h1, h0, z, d2, d2, none, xa, xa);
        lemma_decref_moved(h3, h2, old);
        lemma_moved_chain(h3, h2, h0, d2, |b: usize| -points(old, b), dx, xa, none, xa);
        // `a` の形と要素
        lemma_unique(h0, a);
        s0.lemma_arg_counted(ws, 0);
        lemma_points_in(ws, ab);
        assert(live(h0, ab));
        assert(live(h3, ab));
        assert(same_head(h3, h0, ab));
        assert(live(h1, ab) && live(h2, ab));
        assert(same_head(h1, h0, ab));
        assert(h1.core.perm(ab).slots == h0.core.perm(ab).slots);
        assert(h3.core.perm(ab).slots == h2.core.perm(ab).slots);
        let first = if h0.scan_of(a) == 255 {
            1int
        } else {
            0int
        };
        lemma_slot_written(h2, h1, a, p, x);
        lemma_array_slots(h0, a, first);
        lemma_array_slots(h1, a, first);
        lemma_array_slots(h2, a, first);
        lemma_array_slots(h3, a, first);
        if first == 1 {
            assert(h2.word_at(a, 0) == h1.word_at(a, 0));
        }
        assert(h3.count_of(a) == h0.count_of(a));
        assert(h3.scanned_word(a, idx) == x);
        assert forall|j: int|
            0 <= j < h0.count_of(a) && j != idx implies #[trigger] h3.scanned_word(a, j)
            == h0.scanned_word(a, j) by {
            assert(h2.word_at(a, first + j) == h1.word_at(a, first + j));
        }
        s0.lemma_arg_counted(ws, 1);
        lemma_points_in(ws, x.bits_spec());
        lemma_readable_kept(h1, h0, x);
        assert forall|j: int| 0 <= j < h3.count_of(a) implies dupable(
            h3,
            #[trigger] h3.scanned_word(a, j),
        ) by {
            h3.lemma_readable_scanned(a, j);
            if j == idx {
                lemma_word_kept(h3, h0, dx, xa, x);
            } else {
                h0.lemma_readable_scanned(a, j);
                lemma_word_kept(h3, h0, dx, xa, h0.scanned_word(a, j));
            }
        }
        assert(array_ok(h3, a));
        // `Ctx` の語と `evv`
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b) + dx(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![], dx, xa);
        assert forall|b: usize| #[trigger] self.need(seq![a], b) > 0 implies s0.need(ws, b) + dx(b)
            >= self.need(seq![a], b) by {
            lemma_points_in(seq![a], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![a], dx, xa);
        assert forall|n: Word| #[trigger] node_ok(h0, n) implies !xa.contains(n.bits_spec()) by {
            if n.bits_spec() == ab {
                assert(h0.tag_of(n) == h0.tag_of(a));
            }
        }
        assert forall|e: Word|
            #![trigger ev_ok(h0, e)]
            ev_ok(h0, e) && h3.readable(e) && xa.contains(e.bits_spec()) implies ev_ok(h3, e)
            && h3.tag_of(e) == h0.tag_of(e) by {
            assert(h0.tag_of(e) == h0.tag_of(a));
        }
        assert forall|t: Word|
            #![trigger table_ok(h0, t)]
            table_ok(h0, t) && h3.readable(t) && xa.contains(t.bits_spec()) implies table_ok(
            h3,
            t,
        ) by {
            assert(h3.count_of(t) == h3.count_of(a));
            assert forall|j: int| 0 <= j < h3.count_of(t) implies dupable(
                h3,
                #[trigger] h3.scanned_word(t, j),
            ) by {
                assert(h3.scanned_word(t, j) == h3.scanned_word(a, j));
            }
        }
        lemma_listed_kept(h3, h0, dx, xa, self.evv, self.max_evv);
        lemma_moved_weaken(
            h3,
            h0,
            dx,
            |b: usize| points(a, b) - points(a, b) - points(x, b),
            xa,
            xa,
        );
    }

    /// 共有の配列を写した `set` の道。
    proof fn lemma_set_copied(
        &self,
        s0: Ctx<M>,
        t: Word,
        n: int,
        lin: bool,
        a: Word,
        idx: int,
        x: Word,
        v: spec_fn(int) -> Word,
        d: spec_fn(usize) -> int,
    )
        requires
            s0.held_ok(),
            array_ok(s0.heap, a),
            s0.holds_args(seq![a, x]),
            dupable(s0.heap, x),
            n == s0.heap.count_of(a),
            0 <= idx < n,
            v == (|j: int|
                if j == idx {
                    x
                } else {
                    s0.heap.scanned_word(a, j)
                }),
            d == (|b: usize| -points(x, b) - points(a, b)),
            built(s0.heap, self.heap, t, n, lin, v, d),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(t),
            self.is_array(t),
            self.array_len(t) == n,
            self.array_lin(t) == lin,
            self.array_at(t, idx) == x,
            forall|j: int|
                0 <= j < n && j != idx ==> #[trigger] self.array_at(t, j) == s0.array_at(a, j),
            self.evolved_by(
                s0,
                |b: usize| points(t, b) - points(a, b) - points(x, b),
                Set::empty().insert(a.bits_spec()),
            ),
    {
        let ws = seq![a, x];
        assert forall|b: usize| #[trigger] d(b) >= -points_in(ws, b) by {
            lemma_points_in(ws, b);
        }
        s0.lemma_arg_counted(ws, 1);
        lemma_points_in(ws, x.bits_spec());
        assert forall|j: int| 0 <= j < n implies s0.heap.readable(#[trigger] v(j)) && dupable(
            s0.heap,
            v(j),
        ) by {
            if j != idx {
                s0.heap.lemma_readable_scanned(a, j);
            }
        }
        self.lemma_made(s0, ws, t, n, lin, v, d);
        assert forall|j: int| 0 <= j < n && j != idx implies #[trigger] self.array_at(t, j)
            == s0.array_at(a, j) by {
            assert(self.array_at(t, j) == v(j));
        }
        assert(self.array_at(t, idx) == v(idx));
        lemma_moved_weaken(
            self.heap,
            s0.heap,
            d,
            |b: usize| points(t, b) - points(a, b) - points(x, b),
            Set::empty(),
            Set::empty().insert(a.bits_spec()),
        );
    }

    /// `Array.set` と `MutArray.write`。`a` と `x` の参照を受け取り、位置 `i` を `x` にした配列を返す。`a` が一意なら
    /// その場で書き換えて `a` を返し、前の要素を手放す。共有なら、要素を `dup` して写した新しい配列を返し、`a` を
    /// 手放す。写しは `a` の `lin` ビットを引き継ぐ。範囲の外なら、`a` と `x` を手放して `Fault::Index` を返す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`a` と `x` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がそれぞれ
    /// の参照を1つずつ持つ。速いモードでは、`a` は配列で、`x` は `dup` できる (検査モードは形を確かめ、違えば
    /// `WrongKind` を返す)。
    pub unsafe fn array_set(&mut self, a: Word, i: i64, x: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(x),
            !M::CHECKED ==> old(self).is_array(a) && old(self).holds_args(seq![a, x]) && old(
                self,
            ).storable(x),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_array(a) && old(self).holds_args(seq![a, x]) && old(
                self,
            ).storable(x) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e))
                &&& 0 <= i < old(self).array_len(a) && !(r matches Err(Fault::Heap(_))) ==> (
                r matches Ok(w) && {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_array(w)
                    &&& final(self).array_len(w) == old(self).array_len(a)
                    &&& final(self).array_lin(w) == old(self).array_lin(a)
                    &&& final(self).array_at(w, i as int) == x
                    &&& forall|j: int|
                        0 <= j < old(self).array_len(a) && j != i
                            ==> #[trigger] final(self).array_at(w, j) == old(self).array_at(a, j)
                    &&& final(self).evolved_by(
                        *old(self),
                        |b: usize| points(w, b) - points(a, b) - points(x, b),
                        Set::empty().insert(a.bits_spec()),
                    )
                })
                &&& !(0 <= i < old(self).array_len(a)) && !(r matches Err(Fault::Heap(_))) ==> {
                    &&& r matches Err(Fault::Index(_))
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(
                        *old(self),
                        |b: usize| -points(a, b) - points(x, b),
                        Set::empty(),
                    )
                }
            },
    {
        let ghost s0 = *self;
        let ghost hs = self.heap;
        let ghost ws = seq![a, x];
        let ghost c = s0.held_ok() && s0.is_array(a) && s0.holds_args(ws) && dupable(s0.heap, x);
        proof {
            assert(ws[0] == a && ws[1] == x);
            if c {
                s0.lemma_arg_ready(ws, 0);
                s0.lemma_arg_ready(ws, 1);
            }
        }
        let n = match self.length_of(a) {
            Ok(n) => n,
            Err(e) => return Err(Fault::Heap(e)),
        };
        if !(0 <= i && (i as u64) < (n as u64)) {
            if let Err(e) = unsafe { self.heap.decref(a) } {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            let ghost h1 = self.heap;
            proof {
                if c {
                    Self::lemma_set_drop_mid(s0, h1, a, x);
                }
            }
            if let Err(e) = unsafe { self.heap.decref(x) } {
                proof {
                    lemma_words_trans(self.heap, h1, hs);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, h1, hs);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_set_dropped(s0, h1, a, x);
                }
            }
            return Err(Fault::Index(Box::new(IndexFault::Array { index: i, len: n as i64 })));
        }
        let idx = i as usize;
        let m = match unsafe { self.heap.meta(a) } {
            Ok(m) => m,
            Err(e) => return Err(Fault::Heap(e)),
        };
        let unique = match unsafe { self.heap.is_unique(a) } {
            Ok(u) => u,
            Err(e) => return Err(Fault::Heap(e)),
        };
        proof {
            m.lemma_fields();
        }
        if unique {
            let first: usize = if m.scan() == Meta::EXTENDED_SCAN {
                1
            } else {
                0
            };
            self.heap.settle();
            let ghost h1 = self.heap;
            proof {
                lemma_settled_twice(hs);
                if c {
                    Self::lemma_set_ready(s0, h1, a, idx as int, (idx + first) as int, x);
                }
            }
            let old = match unsafe { self.heap.field(a, idx + first) } {
                Ok(w) => w,
                Err(e) => {
                    proof {
                        self.lemma_words_kept(s0);
                    }
                    return Err(Fault::Heap(e));
                },
            };
            if let Err(e) = unsafe { self.heap.set_field(a, idx + first, x) } {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            let ghost h2 = self.heap;
            proof {
                if c {
                    Self::lemma_set_old_ready(
                        s0,
                        h1,
                        h2,
                        a,
                        idx as int,
                        (idx + first) as int,
                        x,
                        old,
                    );
                }
            }
            if let Err(e) = unsafe { self.heap.decref(old) } {
                proof {
                    lemma_words_trans(h2, h1, hs);
                    lemma_words_trans(self.heap, h2, hs);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            proof {
                if M::CHECKED {
                    lemma_words_trans(h2, h1, hs);
                    lemma_words_trans(self.heap, h2, hs);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_set_in_place(
                        s0,
                        h1,
                        h2,
                        a,
                        idx as int,
                        (idx + first) as int,
                        x,
                        old,
                    );
                }
            }
            return Ok(a);
        }
        // 共有の配列は写す。`x` の参照は写しの位置 `idx` に移す

        let first: usize = if n >= 255 {
            1
        } else {
            0
        };
        let ghost v = |j: int|
            if j == idx {
                x
            } else {
                hs.scanned_word(a, j)
            };
        let ghost z = |b: usize| 0int;
        let ghost dx = |b: usize| -points(x, b);
        let ghost dxa = |b: usize| -points(x, b) - points(a, b);
        let lin = m.lin();
        let t = self.heap.alloc_data(0, n, 0, lin);
        let ghost h1 = self.heap;
        proof {
            if c {
                lemma_fill_start(hs, h1, t, n as int, lin, v);
                assert forall|j: int| 0 <= j < n && j != idx implies hs.readable(#[trigger] v(j))
                    && dupable(hs, v(j)) by {
                    hs.lemma_readable_scanned(a, j);
                }
                s0.lemma_arg_counted(ws, 0);
                s0.lemma_arg_counted(ws, 1);
                lemma_points_in(ws, a.bits_spec());
                lemma_points_in(ws, x.bits_spec());
            }
        }
        let r = fill(
            &mut self.heap,
            t,
            first,
            0,
            idx,
            Some(a),
            x,
            Ghost(hs),
            Ghost(n as int),
            Ghost(lin),
            Ghost(v),
            Ghost(z),
            Ghost(c),
        );
        let ghost h2 = self.heap;
        if let Err(e) = r {
            proof {
                lemma_words_trans(h2, h1, hs);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if c {
                lemma_fill_ready(hs, h2, t, n as int, lin, idx as int, v, z, first as int);
                lemma_readable_kept(h2, hs, x);
                lemma_arg_moved(h2, hs, z, Set::empty(), x);
            }
        }
        if let Err(e) = unsafe { self.heap.set_field(t, idx + first, x) } {
            proof {
                lemma_words_trans(h2, h1, hs);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let ghost h3 = self.heap;
        proof {
            if M::CHECKED {
                lemma_words_trans(h2, h1, hs);
                lemma_words_trans(h3, h2, hs);
            }
            assert(h2.word_in(a));
            assert(h3.core.word_ok(a) == h2.core.word_ok(a));
            if c {
                lemma_fill_put(hs, h2, h3, t, n as int, lin, idx as int, v, z, dx, x, first as int);
            }
        }
        let r = fill(
            &mut self.heap,
            t,
            first,
            idx + 1,
            n,
            Some(a),
            x,
            Ghost(hs),
            Ghost(n as int),
            Ghost(lin),
            Ghost(v),
            Ghost(dx),
            Ghost(c),
        );
        let ghost h4 = self.heap;
        if let Err(e) = r {
            proof {
                lemma_words_trans(h2, h1, hs);
                lemma_words_trans(h3, h2, hs);
                lemma_words_trans(h4, h3, hs);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if c {
                lemma_arg_moved(h4, hs, dx, Set::empty(), a);
            }
        }
        if let Err(e) = unsafe { self.heap.decref(a) } {
            proof {
                lemma_words_trans(h2, h1, hs);
                lemma_words_trans(h3, h2, hs);
                lemma_words_trans(h4, h3, hs);
                lemma_words_trans(self.heap, h4, hs);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if M::CHECKED {
                lemma_words_trans(h2, h1, hs);
                lemma_words_trans(h3, h2, hs);
                lemma_words_trans(h4, h3, hs);
                lemma_words_trans(self.heap, h4, hs);
                self.lemma_words_kept(s0);
            }
            if c {
                lemma_built_released(hs, h4, self.heap, t, n as int, lin, v, dx, dxa, a);
                self.lemma_set_copied(s0, t, n as int, lin, a, idx as int, x, v, dxa);
            }
        }
        Ok(t)
    }
}

/// `moved` は、どのブロックについても `moved_lin` である。
proof fn lemma_moved_to_lin<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    d: spec_fn(usize) -> int,
    e: spec_fn(usize) -> int,
    x: Set<usize>,
    a: usize,
)
    requires
        moved(h1, h0, d, x),
        forall|b: usize| live(h0, b) ==> #[trigger] d(b) == e(b),
    ensures
        moved_lin(h1, h0, e, x, a),
{
    lemma_moved_weaken(h1, h0, d, e, x, x);
    assert forall|b: usize| live(h0, b) && live(h1, b) implies #[trigger] same_head_lin(
        h1,
        h0,
        b,
    ) by {
        assert(same_head(h1, h0, b));
    }
}

/// `set_lin` の後のヒープは `a` の `lin` ビットのほかは前と同じに読める。
proof fn lemma_relinked_same<M: Mode>(h1: Heap<M>, h0: Heap<M>, aw: Word, lin: bool)
    requires
        h1.relinked(h0, aw, lin),
    ensures
        h1.settled() == h0.settled(),
        forall|w: Word| #[trigger] h1.readable(w) == h0.readable(w),
        forall|w: Word| #[trigger] h1.word_in(w) == h0.word_in(w),
        forall|w: Word| #[trigger] h1.owned(w) == h0.owned(w),
        forall|w: Word| #[trigger] h1.rc_value(w) == h0.rc_value(w),
        forall|w: Word| #[trigger] h1.count_of(w) == h0.count_of(w),
        forall|w: Word| #[trigger] h1.tag_of(w) == h0.tag_of(w),
        forall|w: Word| #[trigger] h1.kind_of(w) == h0.kind_of(w),
        forall|w: Word| #[trigger] h1.scan_of(w) == h0.scan_of(w),
        forall|w: Word, i: int| #[trigger] h1.scanned_word(w, i) == h0.scanned_word(w, i),
        forall|w: Word| w.bits_spec() != aw.bits_spec() ==> #[trigger] h1.lin_of(w) == h0.lin_of(w),
        h1.lin_of(aw) == lin,
        forall|b: usize| #[trigger] live(h1, b) == live(h0, b),
        forall|b: usize| #[trigger] h1.core.block(b) == h0.core.block(b),
        forall|b: usize| #[trigger] h1.core.settled_view().ext(b) == h0.core.settled_view().ext(b),
        forall|b: usize| #[trigger]
            h1.core.settled_view().rc_of(b) == h0.core.settled_view().rc_of(b),
        forall|b: usize| b != aw.bits_spec() ==> #[trigger] h1.core.perm(b) == h0.core.perm(b),
        h1.core.perm(aw.bits_spec()).slots == h0.core.perm(aw.bits_spec()).slots,
        h1.core.perm(aw.bits_spec()).rc == h0.core.perm(aw.bits_spec()).rc,
        h1.core.model == h0.core.model,
{
    let ab = aw.bits_spec();
    assert forall|b: usize| b != ab implies #[trigger] h1.core.perm(b) == h0.core.perm(b) by {
        if h0.core.dom().contains(b) {
            assert(h1.core.perms@[b] == h0.core.perms@[b]);
        }
    }
    assert(h1.core.settled_view() == h1.core);
    assert(h0.core.settled_view() == h0.core);
    assert forall|w: Word| #[trigger] h1.count_of(w) == h0.count_of(w) by {
        if w.bits_spec() != ab {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word, i: int| #[trigger] h1.scanned_word(w, i) == h0.scanned_word(w, i) by {
        if w.bits_spec() != ab {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.tag_of(w) == h0.tag_of(w) by {
        if w.bits_spec() != ab {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.kind_of(w) == h0.kind_of(w) by {
        if w.bits_spec() != ab {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.scan_of(w) == h0.scan_of(w) by {
        if w.bits_spec() != ab {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.rc_value(w) == h0.rc_value(w) by {
        if w.bits_spec() != ab {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.owned(w) == h0.owned(w) by {
        assert(h1.rc_value(w) == h0.rc_value(w));
    }
    assert forall|w: Word| w.bits_spec() != ab implies #[trigger] h1.lin_of(w) == h0.lin_of(w) by {
        assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
    }
    assert forall|b: usize| #[trigger]
        h1.core.settled_view().rc_of(b) == h0.core.settled_view().rc_of(b) by {
        if b != ab {
            assert(h1.core.perm(b) == h0.core.perm(b));
        }
    }
}

/// 一意な物体 `a` を指す辺がなければ、生きている物体の走査する語は `a` を指さない。
proof fn lemma_no_edge<M: Mode>(h: Heap<M>, ab: usize, w: Word, i: int)
    requires
        h.consistent(),
        h.settled(),
        h.core.dom().contains(ab),
        h.core.indeg(ab) == 0,
        w.is_ptr_spec(),
        h.readable(w),
        0 <= i < h.count_of(w),
    ensures
        !(h.scanned_word(w, i).is_ptr_spec() && h.scanned_word(w, i).bits_spec() == ab),
{
    let b = w.bits_spec();
    let c = h.core;
    let p = c.perm(b);
    let j = p_first(p) + i;
    let u = h.scanned_word(w, i);
    assert(c.scanned(b, j));
    if !c.dom().contains(b) {
        c.lemma_region_at(b);
        assert(p_scanned(p, j));
    } else {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        assert(c.block_wf(b));
        assert(c.state(b) == BlockState::Live);
        assert(p_scanned(p, j));
        assert(c.slot_word(p, b, j));
        if u.is_ptr_spec() {
            assert(c.model@.edges.dom().contains((b, j)));
            assert(c.edge_wf((b, j)));
            lemma_indeg_pos(c.model@.edges, (b, j));
        }
    }
}

/// `set_lin` の後も、持たれていた語は持たれたままである。`a` を指す語も、`a` の rc が正なので手放せる。
proof fn lemma_held_word_relinked<M: Mode>(h1: Heap<M>, h0: Heap<M>, aw: Word, lin: bool, w: Word)
    requires
        h1.relinked(h0, aw, lin),
        h0.rc_value(aw) == 1,
        held_word(h0, w),
    ensures
        held_word(h1, w),
{
    lemma_relinked_same(h1, h0, aw, lin);
    if w.is_ptr_spec() && w.bits_spec() == aw.bits_spec() {
        assert(h1.rc_value(w) == h1.rc_value(aw));
    }
}

/// `set_lin` の後も、`a` を通らない `evv` のリストは同じ列のリストである。
proof fn lemma_path_relinked<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    aw: Word,
    lin: bool,
    w: Word,
    f: nat,
)
    requires
        h1.relinked(h0, aw, lin),
        h0.consistent(),
        h0.settled(),
        h0.core.dom().contains(aw.bits_spec()),
        h0.core.indeg(aw.bits_spec()) == 0,
        h0.tag_of(aw) == 0,
        path(h0, w, f) is Some,
        h0.readable(w),
    ensures
        path(h1, w, f) == path(h0, w, f),
    decreases f,
{
    let ab = aw.bits_spec();
    lemma_relinked_same(h1, h0, aw, lin);
    if w.bits_spec() != 1 && f > 0 {
        let e = h0.scanned_word(w, 0);
        let n = h0.scanned_word(w, 1);
        assert(node_ok(h0, w));
        assert(ev_ok(h0, e));
        assert(w.bits_spec() != ab) by {
            if w.bits_spec() == ab {
                assert(h0.tag_of(w) == h0.tag_of(aw));
            }
        }
        assert(e.bits_spec() != ab) by {
            if e.bits_spec() == ab {
                assert(h0.tag_of(e) == h0.tag_of(aw));
            }
        }
        h0.lemma_readable_scanned(w, 0);
        h0.lemma_readable_scanned(w, 1);
        h0.lemma_readable_scanned(e, 0);
        let t = h0.scanned_word(e, 0);
        lemma_no_edge(h0, ab, e, 0);
        lemma_no_edge(h0, ab, e, 1);
        lemma_no_edge(h0, ab, e, 2);
        assert(node_ok(h1, w));
        assert forall|i: int| 0 <= i < h1.count_of(t) implies dupable(
            h1,
            #[trigger] h1.scanned_word(t, i),
        ) by {
            lemma_no_edge(h0, ab, t, i);
            assert(dupable(h0, h0.scanned_word(t, i)));
        }
        assert(table_ok(h1, t));
        assert(ev_ok(h1, e));
        lemma_path_relinked(h1, h0, aw, lin, n, (f - 1) as nat);
    }
}

impl<M: Mode> Ctx<M> {
    /// `freeze` と `thaw` で `settle` した後の文脈は前の文脈と同じものを持つ。
    proof fn lemma_relin_settled(&self, s0: Ctx<M>, a: Word)
        requires
            s0.held_ok(),
            array_ok(s0.heap, a),
            s0.holds(a),
            self.heap == s0.heap.settled_heap(),
            self.heap.consistent(),
            self.heap.settled(),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(a),
            array_ok(self.heap, a),
            self.array_len(a) == s0.array_len(a),
            forall|j: int| #[trigger] self.array_at(a, j) == s0.array_at(a, j),
            self.heap.rc_value(a) == s0.heap.rc_value(a),
            moved(self.heap, s0.heap, |b: usize| 0int, Set::empty()),
            no_free(self.heap, s0.heap),
            exact(self.heap, s0.heap, |b: usize| 0int),
    {
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        lemma_settle_moved(self.heap, s0.heap);
        self.lemma_kept_ok(s0, seq![], z);
        assert forall|b: usize| #[trigger] self.need(seq![a], b) > 0 implies s0.need(seq![a], b)
            + z(b) >= self.need(seq![a], b) by {}
        self.lemma_holds_moved(s0, seq![a], seq![a], z, none);
        lemma_readable_kept(self.heap, s0.heap, a);
        lemma_array_kept(self.heap, s0.heap, z, none, a);
    }

    /// その場の `freeze` と `thaw` の、モデルが合っている道。`a` は一意で、ほかの物体から辺で指されないので、`lin`
    /// ビットを変えても `Ctx` の語と `evv` のリストはそのままである。
    proof fn lemma_relinked_ok(&self, s1: Ctx<M>, a: Word, lin: bool)
        requires
            s1.held_ok(),
            s1.holds(a),
            array_ok(s1.heap, a),
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
            self.is_array(a),
            self.array_len(a) == s1.array_len(a),
            forall|j: int| #[trigger] self.array_at(a, j) == s1.array_at(a, j),
            self.array_lin(a) == lin,
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
        // 配列の形
        assert forall|i: int| 0 <= i < h1.count_of(a) implies dupable(
            h1,
            #[trigger] h1.scanned_word(a, i),
        ) by {
            lemma_no_edge(h0, ab, a, i);
            assert(dupable(h0, h0.scanned_word(a, i)));
        }
        assert(array_ok(h1, a));
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

    /// その場の `freeze` と `thaw` の、`settle` の前の文脈からの変化。
    proof fn lemma_relinked_from(&self, s0: Ctx<M>, s1: Ctx<M>, a: Word)
        requires
            moved(s1.heap, s0.heap, |b: usize| 0int, Set::empty()),
            no_free(s1.heap, s0.heap),
            exact(s1.heap, s0.heap, |b: usize| 0int),
            moved_lin(self.heap, s1.heap, |b: usize| 0int, Set::empty(), a.bits_spec()),
            no_free(self.heap, s1.heap),
            forall|b: usize|
                #![trigger live(s1.heap, b)]
                live(s1.heap, b) ==> self.heap.core.settled_view().ext(b)
                    == s1.heap.core.settled_view().ext(b),
        ensures
            self.evolved_lin(s0, |b: usize| points(a, b) - points(a, b), Set::empty(), a),
    {
        let h0 = s0.heap;
        let h1 = s1.heap;
        let h2 = self.heap;
        let ab = a.bits_spec();
        let v0 = h0.core.settled_view();
        assert forall|b: usize|
            #![trigger live(h2, b), live(h0, b)]
            live(h0, b) && live(h2, b) implies {
            &&& b != ab ==> same_head(h2, h0, b)
            &&& same_head_lin(h2, h0, b)
            &&& h2.core.perm(b).slots == h0.core.perm(b).slots
        } by {
            assert(live(h1, b));
            assert(same_head(h1, h0, b));
            assert(same_head_lin(h2, h1, b));
        }
        assert forall|b: usize|
            #![trigger live(h0, b)]
            live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + (points(a, b) - points(a, b))
                >= 1) implies {
            &&& live(h2, b)
            &&& h2.core.block(b) == h0.core.block(b)
            &&& v0.rc_of(b) != 0 ==> h2.core.settled_view().ext(b) == v0.ext(b) + (points(a, b)
                - points(a, b))
        } by {
            assert(live(h1, b));
            assert(same_head(h1, h0, b));
            assert(live(h2, b));
            assert(h2.core.block(b) == h1.core.block(b));
        }
    }

    /// `freeze` と `thaw` の核。`a` の参照を受け取り、`lin` ビットを `lin` にした同じ要素の配列を返す。`a` が一意なら、
    /// 写さずにその場で `lin` ビットを変えて `a` を返す。共有なら、要素を `dup` して写し、`a` を手放す。ただし検査
    /// モードでは、共有の `MutArray` (`freeze` の `a`) は `NotUnique` で断る。`MutArray` はいつも一意だからである。
    unsafe fn relin(&mut self, a: Word, lin: bool) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_array(a) && old(self).holds(a),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_array(a) && old(self).holds(a) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e)) || (
                M::CHECKED && !lin && e == HeapError::NotUnique)
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_array(w)
                    &&& final(self).array_len(w) == old(self).array_len(a)
                    &&& final(self).array_lin(w) == lin
                    &&& forall|j: int|
                        0 <= j < old(self).array_len(a) ==> #[trigger] final(self).array_at(w, j)
                            == old(self).array_at(a, j)
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
        let ghost hs = self.heap;
        let ghost ws = seq![a];
        let ghost c = s0.held_ok() && s0.is_array(a) && s0.holds(a);
        proof {
            assert(ws[0] == a);
            if c {
                s0.lemma_arg_ready(ws, 0);
            }
        }
        let n = match self.length_of(a) {
            Ok(n) => n,
            Err(e) => return Err(Fault::Heap(e)),
        };
        let unique = match unsafe { self.heap.is_unique(a) } {
            Ok(u) => u,
            Err(e) => return Err(Fault::Heap(e)),
        };
        if unique {
            self.heap.settle();
            let ghost s1 = *self;
            proof {
                lemma_settled_twice(hs);
                if c {
                    s1.lemma_relin_settled(s0, a);
                }
            }
            if let Err(e) = unsafe { self.heap.set_lin(a, lin) } {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            proof {
                if M::CHECKED {
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_relinked_ok(s1, a, lin);
                    self.lemma_relinked_from(s0, s1, a);
                }
            }
            return Ok(a);
        }
        if M::CHECKED && !lin {
            return Err(Fault::Heap(HeapError::NotUnique));
        }
        let first: usize = if n >= 255 {
            1
        } else {
            0
        };
        let ghost v = |j: int| hs.scanned_word(a, j);
        let ghost z = |b: usize| 0int;
        let ghost da = |b: usize| -points(a, b);
        let t = self.heap.alloc_data(0, n, 0, lin);
        let ghost h1 = self.heap;
        proof {
            if c {
                lemma_fill_start(hs, h1, t, n as int, lin, v);
                assert forall|j: int| 0 <= j < n implies hs.readable(#[trigger] v(j)) && dupable(
                    hs,
                    v(j),
                ) by {
                    hs.lemma_readable_scanned(a, j);
                }
                s0.lemma_arg_counted(ws, 0);
                lemma_points_in(ws, a.bits_spec());
            }
        }
        let r = fill(
            &mut self.heap,
            t,
            first,
            0,
            n,
            Some(a),
            a,
            Ghost(hs),
            Ghost(n as int),
            Ghost(lin),
            Ghost(v),
            Ghost(z),
            Ghost(c),
        );
        let ghost h2 = self.heap;
        if let Err(e) = r {
            proof {
                lemma_words_trans(h2, h1, hs);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if c {
                lemma_arg_moved(h2, hs, z, Set::empty(), a);
            }
        }
        if let Err(e) = unsafe { self.heap.decref(a) } {
            proof {
                lemma_words_trans(h2, h1, hs);
                lemma_words_trans(self.heap, h2, hs);
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if M::CHECKED {
                lemma_words_trans(h2, h1, hs);
                lemma_words_trans(self.heap, h2, hs);
                self.lemma_words_kept(s0);
            }
            if c {
                lemma_built_released(hs, h2, self.heap, t, n as int, lin, v, z, da, a);
                assert forall|b: usize| #[trigger] da(b) >= -points_in(ws, b) by {
                    lemma_points_in(ws, b);
                }
                self.lemma_made(s0, ws, t, n as int, lin, v, da);
                lemma_moved_to_lin(
                    self.heap,
                    hs,
                    da,
                    |b: usize| points(t, b) - points(a, b),
                    Set::empty(),
                    a.bits_spec(),
                );
            }
        }
        Ok(t)
    }

    /// `MutArray.freeze`。`m` の参照を受け取り、写さずに `lin` ビットを下ろして `Array` として返す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`m` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ
    /// 持つ。速いモードでは、`m` は配列である (検査モードは形を確かめ、違えば `WrongKind` を、一意でなければ
    /// `NotUnique` を返す)。速いモードで共有の `m` を受けたときは、写して返す。
    pub unsafe fn array_freeze(&mut self, m: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(m),
            !M::CHECKED ==> old(self).is_array(m) && old(self).holds(m),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_array(m) && old(self).holds(m) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e)) || e
                    == HeapError::NotUnique
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_array(w)
                    &&& final(self).array_len(w) == old(self).array_len(m)
                    &&& !final(self).array_lin(w)
                    &&& forall|j: int|
                        0 <= j < old(self).array_len(m) ==> #[trigger] final(self).array_at(w, j)
                            == old(self).array_at(m, j)
                    &&& final(self).evolved_lin(
                        *old(self),
                        |b: usize| points(w, b) - points(m, b),
                        Set::empty(),
                        m,
                    )
                }
            },
    {
        unsafe { self.relin(m, false) }
    }

    /// `Array.thaw`。`a` の参照を受け取り、`lin` ビットを立てて `MutArray` として返す。一意ならその場で立て、共有なら
    /// 写した物体に立てる。
    ///
    /// # Safety
    /// `array_freeze` と同じ (`m` が `a`)。検査モードも、共有の `a` は写す。
    pub unsafe fn array_thaw(&mut self, a: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            !M::CHECKED ==> old(self).is_array(a) && old(self).holds(a),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_array(a) && old(self).holds(a) ==> {
                &&& r matches Err(Fault::Heap(e)) ==> chain_err(Err::<(), HeapError>(e))
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).is_array(w)
                    &&& final(self).array_len(w) == old(self).array_len(a)
                    &&& final(self).array_lin(w)
                    &&& forall|j: int|
                        0 <= j < old(self).array_len(a) ==> #[trigger] final(self).array_at(w, j)
                            == old(self).array_at(a, j)
                    &&& final(self).evolved_lin(
                        *old(self),
                        |b: usize| points(w, b) - points(a, b),
                        Set::empty(),
                        a,
                    )
                }
            },
    {
        unsafe { self.relin(a, true) }
    }

    /// `MutArray.empty`。長さ 0 の新しい配列を返す。`lin` が真なら `MutArray` である。
    pub fn array_empty(&mut self, lin: bool) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok,
            old(self).held_ok() ==> (r matches Ok(w) && {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).is_array(w)
                &&& final(self).array_len(w) == 0
                &&& final(self).array_lin(w) == lin
                &&& final(self).evolved_by(*old(self), |b: usize| 0int, Set::empty())
            }),
    {
        let ghost s0 = *self;
        let ghost hs = self.heap;
        let t = self.heap.alloc_data(0, 0, 0, lin);
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                let v = |j: int| t;
                lemma_fill_start(hs, self.heap, t, 0, lin, v);
                self.lemma_made(s0, seq![], t, 0, lin, v, |b: usize| 0int);
            }
        }
        Ok(t)
    }
}

} // verus!

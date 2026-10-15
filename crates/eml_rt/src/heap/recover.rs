//! 根から届く物体を新しいヒープに写す (docs/spec/object-model.md の「回復」)。
//!
//! 古いヒープは読むだけで、写し先は今ある確保と書き込みの操作だけで作る。そのため、写し先の不変条件は各操作の約束から
//! 出る。証明するのは、古いヒープを安全に読むことと、写し先の各物体の外の参照の数が語のスタックと写し終えた根の中の
//! 出現の数に等しいこと (速いモード) である。

use vstd::hash_map::HashMapWithView;
use vstd::hash_set::HashSetWithView;
use vstd::prelude::*;

use super::Core;
use super::Heap;
#[cfg(verus_only)]
use super::inv::{lemma_filled_init, p_count, p_filled, p_first, p_init, p_kind, p_scanned};
#[cfg(verus_only)]
use super::model::{BlockState, lemma_indeg_insert};
use crate::error::HeapError;
use crate::meta::{Header, Kind};
use crate::mode::Mode;
use crate::word::Word;

verus! {

broadcast use vstd::std_specs::hash::group_hash_axioms;

/// 写している途中の `Data` の物体。走査する語は、`next` 番目まで写し終えて語のスタックに積んである。
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    /// 古いヒープのブロック。
    h: *mut Header,
    tag: u16,
    /// 走査する語の始まりと数、ペイロードの語の数。
    first: usize,
    count: usize,
    cap: usize,
    next: usize,
}

/// `w` は物体 `b` を指す。
pub(crate) open spec fn at(w: Word, b: usize) -> int {
    if w.is_ptr_spec() && w.bits_spec() == b {
        1
    } else {
        0
    }
}

/// `ws` のうち物体 `b` を指す語の数。
pub(crate) open spec fn count_in(ws: Seq<Word>, b: usize) -> int
    decreases ws.len(),
{
    if ws.len() == 0 {
        0
    } else {
        count_in(ws.drop_last(), b) + at(ws.last(), b)
    }
}

/// 根の表のうち物体 `b` を指す語の数。
pub(crate) open spec fn count_roots(rs: Seq<Option<Word>>, b: usize) -> int
    decreases rs.len(),
{
    if rs.len() == 0 {
        0
    } else {
        count_roots(rs.drop_last(), b) + match rs.last() {
            Some(w) => at(w, b),
            None => 0,
        }
    }
}

proof fn lemma_count_push(ws: Seq<Word>, w: Word)
    ensures
        forall|b: usize| #[trigger] count_in(ws.push(w), b) == count_in(ws, b) + at(w, b),
{
    assert forall|b: usize| #[trigger] count_in(ws.push(w), b) == count_in(ws, b) + at(w, b) by {
        assert(ws.push(w).drop_last() =~= ws);
    }
}

proof fn lemma_count_pop(ws: Seq<Word>)
    requires
        ws.len() > 0,
    ensures
        forall|b: usize| #[trigger]
            count_in(ws, b) == count_in(ws.drop_last(), b) + at(ws.last(), b),
{
}

proof fn lemma_count_nonneg(ws: Seq<Word>, b: usize)
    ensures
        count_in(ws, b) >= 0,
    decreases ws.len(),
{
    if ws.len() > 0 {
        lemma_count_nonneg(ws.drop_last(), b);
    }
}

proof fn lemma_roots_nonneg(rs: Seq<Option<Word>>, b: usize)
    ensures
        count_roots(rs, b) >= 0,
    decreases rs.len(),
{
    if rs.len() > 0 {
        lemma_roots_nonneg(rs.drop_last(), b);
    }
}

proof fn lemma_roots_push(rs: Seq<Option<Word>>, r: Option<Word>)
    ensures
        forall|b: usize| #[trigger]
            count_roots(rs.push(r), b) == count_roots(rs, b) + match r {
                Some(w) => at(w, b),
                None => 0,
            },
{
    assert forall|b: usize| #[trigger]
        count_roots(rs.push(r), b) == count_roots(rs, b) + match r {
            Some(w) => at(w, b),
            None => 0,
        } by {
        assert(rs.push(r).drop_last() =~= rs);
    }
}

/// ブロックの番地は、ポインタの語のビット列である。
proof fn lemma_block_bits(b: usize)
    requires
        b % 8 == 0,
        b >= 8,
    ensures
        crate::word::is_ptr_spec(b),
{
    assert(b & 1 == 0) by (bit_vector)
        requires
            b % 8 == 0,
    ;
}

/// どの語も `b` を指さなければ、`b` を指す語の数は 0 である。
proof fn lemma_count_absent(ws: Seq<Word>, b: usize)
    requires
        forall|i: int| 0 <= i < ws.len() ==> at(#[trigger] ws[i], b) == 0,
    ensures
        count_in(ws, b) == 0,
    decreases ws.len(),
{
    if ws.len() > 0 {
        assert(at(ws[ws.len() - 1], b) == 0);
        lemma_count_absent(ws.drop_last(), b);
    }
}

/// どの根も `b` を指さなければ、`b` を指す根の数は 0 である。
proof fn lemma_roots_absent(rs: Seq<Option<Word>>, b: usize)
    requires
        forall|i: int| 0 <= i < rs.len() ==> (#[trigger] rs[i] matches Some(w) ==> at(w, b) == 0),
    ensures
        count_roots(rs, b) == 0,
    decreases rs.len(),
{
    if rs.len() > 0 {
        assert(rs[rs.len() - 1] matches Some(w) ==> at(w, b) == 0);
        lemma_roots_absent(rs.drop_last(), b);
    }
}

impl Core {
    /// 切り出したブロックは、どれも付けた塊のブロックでない。ページの頭の権限は1つしかないからである。
    proof fn lemma_carved_apart(tracked &mut self)
        requires
            old(self).pages.wf(),
            old(self).region_wf(),
        ensures
            *final(self) == *old(self),
            forall|h: usize| #[trigger]
                final(self).pages.carved_block(h) ==> !final(self).region_has(h),
    {
        if self.env@.region is Some {
            let tracked r = self.env.borrow().tracked_region();
            self.pages.lemma_pages_apart(r, self.checked());
            assert forall|h: usize| #[trigger] self.pages.carved_block(h) implies !self.region_has(
                h,
            ) by {
                self.pages.lemma_carved_page(h);
                if r.has(h) {
                    r.lemma_block(h, self.checked());
                }
            }
        }
    }
}

impl Core {
    /// `old` のブロックがどれも同じブロックのまま `self` にあり、付けた塊が同じなら、`old` で使える語は `self` でも
    /// 使える。
    proof fn lemma_words_grown(&self, old: Core)
        requires
            self.env == old.env,
            forall|b: usize| #[trigger]
                old.dom().contains(b) ==> self.dom().contains(b) && self.block(b) == old.block(b),
            forall|h: usize|
                self.dom().contains(h) && !old.dom().contains(h) ==> !#[trigger] self.region_has(h),
        ensures
            forall|w: Word| old.word_ok(w) ==> #[trigger] self.word_ok(w),
    {
        assert forall|w: Word| old.word_ok(w) implies #[trigger] self.word_ok(w) by {
            if w.is_ptr_spec() {
                let h = w.bits_spec();
                if !old.dom().contains(h) {
                    assert(old.region_has(h));
                    assert(self.region_has(h));
                    assert(!self.dom().contains(h));
                }
            }
        }
    }
}

impl<M: Mode> Heap<M> {
    /// 2つのヒープが同じ塊の `Arc` を持てば、付けた塊の権限も同じである。
    proof fn lemma_same_region_tied(&self, other: Heap<M>, source: Heap<M>)
        requires
            self.wf(),
            other.wf(),
            self.region == other.region,
            other.same_region(source),
        ensures
            self.same_region(source),
    {
    }

    /// 写し先のどのブロックも語のスタックと根に現れなければ、その外の参照の数は 0 と数えてよい。
    proof fn lemma_absent_from(&self, words: Seq<Word>, out: Seq<Option<Word>>, b: usize)
        requires
            !self.core.has(b),
            forall|i: int| 0 <= i < words.len() ==> self.core.word_ok(#[trigger] words[i]),
            forall|j: int|
                0 <= j < out.len() ==> (#[trigger] out[j] matches Some(w) ==> self.core.word_ok(w)),
        ensures
            count_in(words, b) == 0,
            count_roots(out, b) == 0,
    {
        assert forall|i: int| 0 <= i < words.len() implies at(#[trigger] words[i], b) == 0 by {
            assert(self.core.word_ok(words[i]));
        }
        lemma_count_absent(words, b);
        assert forall|j: int| 0 <= j < out.len() implies (#[trigger] out[j] matches Some(w) ==> at(
            w,
            b,
        ) == 0) by {
            if out[j] is Some {
                assert(self.core.word_ok(out[j]->0));
            }
        }
        lemma_roots_absent(out, b);
    }

    /// 確保の後も写し先の状態は保たれ、新しい物体 `nw` を語のスタックに積み、写し元のブロック `key` の写しにできる。
    proof fn lemma_target_made(
        &self,
        before: Heap<M>,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
        nw: Word,
        key: usize,
    )
        requires
            before.target_ok(source, words, out, done),
            self.made(before, nw),
            M::CHECKED ==> self.words_kept(before),
            !M::CHECKED ==> !self.lin_of(nw) && self.owned(nw),
        ensures
            self.target_ok(source, words.push(nw), out, done.insert(key, Some(nw))),
    {
        let ws = words.push(nw);
        let d2 = done.insert(key, Some(nw));
        self.lemma_same_region_tied(before, source);
        let n = nw.bits_spec();
        if !M::CHECKED {
            let a = self.core;
            let b0 = before.core;
            assert(a.frame(b0));
            assert(b0.settled_view() == b0);
            // 新しいブロックは付けた塊のブロックでない
            assert(!a.region_has(n));
            assert(!b0.has(n));
            assert forall|h: usize|
                a.dom().contains(h) && !b0.dom().contains(h) implies !#[trigger] a.region_has(
                h,
            ) by {
                assert(h == n);
            }
            a.lemma_words_grown(b0);
            assert forall|i: int| 0 <= i < words.len() implies b0.word_ok(#[trigger] words[i]) by {
                assert(before.usable(words[i]));
            }
            assert forall|j: int| 0 <= j < out.len() implies (#[trigger] out[j] matches Some(w)
                ==> b0.word_ok(w)) by {
                if out[j] is Some {
                    assert(before.usable(out[j]->0));
                }
            }
            before.lemma_absent_from(words, out, n);
            lemma_count_push(words, nw);
            assert forall|b: usize| a.dom().contains(b) && a.rc_of(b) != 0 implies #[trigger] a.ext(
                b,
            ) == count_in(ws, b) + count_roots(out, b) + if None::<usize> == Some(b) {
                1int
            } else {
                0int
            } by {
                if b == n {
                } else {
                    assert(b0.dom().contains(b));
                    assert(a.model@.edges == b0.model@.edges);
                    assert(a.indeg(b) == b0.indeg(b));
                    assert(a.ext(b) == b0.ext(b));
                }
            }
            assert(self.accounted(ws, out, None));
            assert forall|i: int| 0 <= i < ws.len() implies #[trigger] self.usable(ws[i]) by {
                if i < words.len() {
                    assert(ws[i] == words[i]);
                    assert(before.usable(words[i]));
                    if words[i].is_ptr_spec() {
                        assert(b0.has(words[i].bits_spec()));
                    }
                }
            }
            assert forall|j: int| 0 <= j < out.len() implies (#[trigger] out[j] matches Some(w)
                ==> self.usable(w)) by {
                if out[j] is Some {
                    let w = out[j]->0;
                    assert(before.usable(w));
                    if w.is_ptr_spec() {
                        assert(b0.has(w.bits_spec()));
                    }
                }
            }
            assert forall|k: usize| #[trigger] d2.contains_key(k) implies (d2[k] matches Some(w)
                ==> self.dupable(w)) by {
                if k == key {
                    assert(a.state(n) == BlockState::Live);
                    assert(a.settled_state(n) == BlockState::Live);
                    assert(self.readable(nw));
                    assert(self.usable(nw));
                    assert(self.dupable(nw));
                } else if done[k] is Some {
                    let w = done[k]->0;
                    assert(before.dupable(w));
                    if w.is_ptr_spec() {
                        let h = w.bits_spec();
                        assert(b0.has(h));
                        if b0.dom().contains(h) {
                            assert(a.perm(h) == b0.perm(h));
                            assert(self.lin_of(w) == before.lin_of(w));
                            assert(a.model@.owner[h] == b0.model@.owner[h]);
                            assert(self.worker == before.worker);
                            assert(a.rc_of(h) == b0.rc_of(h));
                            assert(before.owned(w));
                        } else {
                            assert(!a.dom().contains(h));
                            a.lemma_region_at(h);
                        }
                        assert(self.usable(w));
                        assert(self.owned(w));
                    }
                    assert(self.dupable(w));
                }
            }
        } else {
            assert forall|i: int| 0 <= i < ws.len() implies #[trigger] self.usable(ws[i]) by {
                if i < words.len() {
                    assert(ws[i] == words[i]);
                    assert(before.usable(words[i]));
                }
            }
            assert forall|j: int| 0 <= j < out.len() implies (#[trigger] out[j] matches Some(w)
                ==> self.usable(w)) by {
                if out[j] is Some {
                    assert(before.usable(out[j]->0));
                }
            }
            assert forall|k: usize| #[trigger] d2.contains_key(k) implies (d2[k] matches Some(w)
                ==> self.dupable(w)) by {
                if k == key {
                    assert(self.core.word_ok(nw));
                    assert(self.word_in(nw));
                } else if done[k] is Some {
                    assert(before.dupable(done[k]->0));
                    assert(before.word_in(done[k]->0));
                }
            }
        }
    }

    /// 写した物体 `nw` を `dup` した後も写し先の状態は保たれ、`nw` を語のスタックに積める。
    proof fn lemma_target_dup(
        &self,
        before: Heap<M>,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
        nw: Word,
    )
        requires
            before.target_ok(source, words, out, done),
            before.dupable(nw),
            self.wf(),
            self.settled(),
            self.values_moved(before),
            M::CHECKED ==> self.words_kept(before),
            !M::CHECKED ==> self.consistent() && self.bumped(before, nw),
        ensures
            self.target_ok(source, words.push(nw), out, done),
    {
        let ws = words.push(nw);
        self.lemma_same_region_tied(before, source);
        lemma_count_push(words, nw);
        if !M::CHECKED {
            let a = self.core;
            let b0 = before.core;
            assert(b0.settled_view() == b0);
            assert(a.bumped(b0, nw.bits_spec()));
            assert forall|w: Word| b0.word_ok(w) implies #[trigger] a.word_ok(w) by {
                if w.is_ptr_spec() && b0.dom().contains(w.bits_spec()) {
                    assert(a.perms@[w.bits_spec()] == b0.perms@[w.bits_spec()] || w.bits_spec()
                        == nw.bits_spec());
                }
            }
            assert forall|b: usize| a.dom().contains(b) && a.rc_of(b) != 0 implies #[trigger] a.ext(
                b,
            ) == count_in(ws, b) + count_roots(out, b) + if None::<usize> == Some(b) {
                1int
            } else {
                0int
            } by {
                assert(a.indeg(b) == b0.indeg(b));
                assert(b0.dom().contains(b));
                if b != nw.bits_spec() {
                    assert(a.perms@[b] == b0.perms@[b]);
                }
                assert(b0.rc_of(b) != 0);
                assert(b0.ext(b) == count_in(words, b) + count_roots(out, b));
                if nw.is_ptr_spec() && b == nw.bits_spec() {
                    assert(a.model@.refs[b] == b0.model@.refs[b] + 1);
                    assert(at(nw, b) == 1);
                } else {
                    if b == nw.bits_spec() {
                        b0.lemma_at(b);
                        lemma_block_bits(b);
                    }
                    assert(a.model@.refs[b] == b0.model@.refs[b]);
                    assert(at(nw, b) == 0);
                }
            }
            assert(self.accounted(ws, out, None));
            assert forall|w: Word| before.usable(w) implies #[trigger] self.usable(w) by {
                assert(a.word_ok(w));
                if w.is_ptr_spec() {
                    let h = w.bits_spec();
                    if !b0.dom().contains(h) {
                        b0.lemma_region_at(h);
                    }
                }
            }
            assert forall|w: Word| before.dupable(w) implies #[trigger] self.dupable(w) by {
                assert(self.usable(w));
                if w.is_ptr_spec() {
                    let h = w.bits_spec();
                    if !b0.dom().contains(h) {
                        b0.lemma_region_at(h);
                        a.lemma_region_at(h);
                    }
                }
            }
        }
        assert forall|i: int| 0 <= i < ws.len() implies #[trigger] self.usable(ws[i]) by {
            if i < words.len() {
                assert(ws[i] == words[i]);
                assert(before.usable(words[i]));
            } else {
                assert(before.usable(nw));
            }
        }
        assert forall|j: int| 0 <= j < out.len() implies (#[trigger] out[j] matches Some(w)
            ==> self.usable(w)) by {
            if out[j] is Some {
                assert(before.usable(out[j]->0));
            }
        }
        assert forall|k: usize| #[trigger] done.contains_key(k) implies (done[k] matches Some(w)
            ==> self.dupable(w)) by {
            if done[k] is Some {
                assert(before.dupable(done[k]->0));
            }
        }
    }

    /// 数えない語 (即値と rc 0 の物体) を語のスタックに積んでも、写し先の状態は保たれる。
    proof fn lemma_target_push(
        &self,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
        w: Word,
    )
        requires
            self.target_ok(source, words, out, done),
            self.usable(w),
            !M::CHECKED && w.is_ptr_spec() ==> self.core.rc_of(w.bits_spec()) == 0,
        ensures
            self.target_ok(source, words.push(w), out, done),
    {
        let ws = words.push(w);
        lemma_count_push(words, w);
        if !M::CHECKED {
            assert forall|b: usize|
                self.core.dom().contains(b) && self.core.rc_of(b)
                    != 0 implies #[trigger] self.core.ext(b) == count_in(ws, b) + count_roots(
                out,
                b,
            ) + if None::<usize> == Some(b) {
                1int
            } else {
                0int
            } by {
                assert(at(w, b) == 0);
            }
        }
        assert forall|i: int| 0 <= i < ws.len() implies #[trigger] self.usable(ws[i]) by {
            if i < words.len() {
                assert(ws[i] == words[i]);
            }
        }
    }

    /// 写し元の語 `w` の写しを語のスタックに積むか (すぐ写せるとき)、`w` の物体を写している途中の物体として積む
    /// (まだ写していない `Data` のとき)。写せない物体は `Unrecoverable` にする
    /// (docs/spec/object-model.md の「種類」)。誤りのときも写し先の
    /// 不変条件は保ち、呼ぶ側は写し先を捨てる。
    #[expect(clippy::too_many_arguments, reason = "the parts of one copy")]
    fn resolve(
        &self,
        new: &mut Heap<M>,
        set: &HashSetWithView<usize>,
        done: &mut HashMapWithView<usize, Option<Word>>,
        frames: &mut Vec<Frame>,
        words: &mut Vec<Word>,
        w: Word,
        Ghost(out): Ghost<Seq<Option<Word>>>,
    ) -> (r: Result<(), HeapError>)
        requires
            self.source_ok(set@, old(done)@, old(frames)@),
            old(new).target_ok(*self, old(words)@, out, old(done)@),
            self.source_word(w),
        ensures
            final(new).wf(),
            final(new).settled(),
            final(new).worker == self.worker,
            r is Ok ==> {
                &&& self.source_ok(set@, final(done)@, final(frames)@)
                &&& final(new).target_ok(*self, final(words)@, out, final(done)@)
                &&& old(done)@.dom().subset_of(final(done)@.dom())
                &&& ({
                    ||| final(frames)@ == old(frames)@ && final(words)@.len() == old(words)@.len()
                        + 1 && final(words)@.drop_last() == old(words)@
                    ||| final(frames)@.drop_last() == old(frames)@ && final(words)@ == old(words)@
                        && final(done)@.dom().len() > old(done)@.dom().len()
                })
            },
    {
        let ghost ws0 = words@;
        let ghost d0 = done@;
        let ghost n0 = *new;
        if !w.is_ptr() {
            words.push(w);
            proof {
                assert(new.usable(w));
                new.lemma_target_push(*self, ws0, out, d0, w);
                assert(words@.drop_last() =~= ws0);
            }
            return Ok(());
        }
        let a = w.bits();
        if !set.contains(&a) {
            // 切り出したブロックでなければ、付けた塊の物体である。写し先も同じ塊を付けているので、そのまま使う
            proof {
                assert(!self.core.pages.carved_block(a));
                if self.core.dom().contains(a) {
                    self.core.pages.lemma_in_use_carved(a);
                }
                assert(self.core.region_has(a));
                assert(!new.core.dom().contains(a)) by {
                    if new.core.dom().contains(a) {
                        assert(new.core.pages.in_use().contains(a));
                    }
                }
                assert(new.core.word_ok(w));
                new.core.lemma_region_at(a);
                assert(new.usable(w));
            }
            words.push(w);
            proof {
                new.lemma_target_push(*self, ws0, out, d0, w);
                assert(words@.drop_last() =~= ws0);
            }
            return Ok(());
        }
        let h = self.header_of(w);
        proof {
            assert(self.core.pages.carved_block(a));
            assert(!self.core.region_has(a));
            assert(self.core.dom().contains(a));
            if M::CHECKED {
                self.core.lemma_safe_at(a);
            } else {
                self.core.lemma_at(a);
            }
        }
        if M::CHECKED {
            // 生存の状態でない箱 (空けた箱、token の箱、空ける途中の箱) は写さない
            if self.core.prefix_state_get(h) & 3 != 1 {
                return Err(HeapError::Unrecoverable);
            }
        }
        proof {
            reveal(Core::building_wf);
            if M::CHECKED {
                assert(self.core.state(a) == BlockState::Live);
            } else {
                assert(self.core.state(a) == BlockState::Live);
            }
        }
        let rc = self.core.rc_get(h);
        if rc == 0 {
            if a == self.dummy.bits() {
                let d = new.dummy();
                proof {
                    assert(new.core.word_ok(d));
                    if !M::CHECKED {
                        assert(new.dummy_wf());
                    }
                    assert(new.usable(d));
                }
                words.push(d);
                proof {
                    new.lemma_target_push(*self, ws0, out, d0, d);
                    assert(words@.drop_last() =~= ws0);
                }
                return Ok(());
            }
            return Err(HeapError::Unrecoverable);
        }
        if rc < 0 {
            return Err(HeapError::Unrecoverable);
        }
        let m = self.core.meta_get(h);
        if m.lin() {
            return Err(HeapError::Unrecoverable);
        }
        match done.get(&a) {
            Some(Some(copied)) => {
                let nw = *copied;
                proof {
                    assert(n0.dupable(nw));
                }
                match unsafe { new.dup(nw) } {
                    Ok(()) => {},
                    Err(e) => return Err(e),
                }
                proof {
                    new.lemma_target_dup(n0, *self, ws0, out, d0, nw);
                }
                words.push(nw);
                proof {
                    assert(words@ =~= ws0.push(nw));
                    assert(words@.drop_last() =~= ws0);
                }
                return Ok(());
            },
            Some(None) => {
                // 写している途中の物体にまた着いたので、輪がある
                return Err(HeapError::Unrecoverable);
            },
            None => {},
        }
        match m.kind() {
            Kind::Data => {
                let (first, count) = self.core.scan_get(h);
                let cap = self.core.cap_get(h);
                done.insert(a, None);
                let f = Frame { h, tag: m.tag(), first, count, cap, next: 0 };
                frames.push(f);
                proof {
                    assert(done@.dom() =~= d0.dom().insert(a));
                    assert(frames@.drop_last() =~= old(frames)@);
                    assert(done@.dom().len() > d0.dom().len()) by {
                        vstd::set_lib::lemma_len_subset(d0.dom(), set@);
                    }
                    assert(self.frame_ok(f, done@));
                    assert forall|i: int| 0 <= i < frames@.len() implies #[trigger] self.frame_ok(
                        frames@[i],
                        done@,
                    ) by {
                        if i < old(frames)@.len() {
                            assert(frames@[i] == old(frames)@[i]);
                            assert(self.frame_ok(old(frames)@[i], d0));
                            assert(frames@[i].h@.addr != a);
                        }
                    }
                    assert forall|i: int, j: int|
                        0 <= i < frames@.len() && 0 <= j < frames@.len() && i != j implies (
                    #[trigger] frames@[i]).h@.addr != (#[trigger] frames@[j]).h@.addr by {
                        if i < old(frames)@.len() {
                            assert(self.frame_ok(old(frames)@[i], d0));
                        }
                        if j < old(frames)@.len() {
                            assert(self.frame_ok(old(frames)@[j], d0));
                        }
                    }
                    assert forall|k: usize| #[trigger] done@.contains_key(k) implies (
                    done@[k] matches Some(v) ==> new.dupable(v)) by {
                        if k != a {
                            assert(d0.contains_key(k));
                        }
                    }
                }
                Ok(())
            },
            Kind::String => {
                if M::CHECKED && self.core.prefix_first_get(h) != self.worker {
                    return Err(HeapError::Unrecoverable);
                }
                proof {
                    assert(self.core.kind(a) == Kind::String);
                    if M::CHECKED {
                        assert(self.decref_passes(a));
                    } else {
                        self.lemma_text_spec(w);
                    }
                }
                let text = self.string_text(w);
                let nw = new.alloc_string(text);
                proof {
                    new.lemma_target_made(n0, *self, ws0, out, d0, nw, a);
                    self.lemma_source_insert(set@, d0, old(frames)@, a, Some(nw));
                }
                done.insert(a, Some(nw));
                words.push(nw);
                proof {
                    assert(words@.drop_last() =~= ws0);
                }
                Ok(())
            },
            Kind::BigInt => {
                let x = self.core.slot_get(h, 0).bits() as i64;
                let nw = new.alloc_int(x);
                proof {
                    if nw.is_imm_spec() {
                        assert(new.usable(nw));
                        n0.lemma_target_push(*self, ws0, out, d0, nw);
                    } else {
                        new.lemma_target_made(n0, *self, ws0, out, d0, nw, a);
                    }
                    self.lemma_source_insert(set@, d0, old(frames)@, a, Some(nw));
                }
                done.insert(a, Some(nw));
                words.push(nw);
                proof {
                    assert(words@.drop_last() =~= ws0);
                }
                Ok(())
            },
            _ => Err(HeapError::Unrecoverable),
        }
    }

    /// 写している途中の物体の写し `nw` を作っている写し先の状態。`nw` の外の参照は1つで、まだ語のスタックにない。
    /// `k` 番より前の走査する語は、まだ `()` である。
    pub(crate) closed spec fn build_ok(
        &self,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
        nw: Word,
        first: int,
        count: int,
        raw: int,
        k: int,
    ) -> bool {
        let n = nw.bits_spec();
        let p = self.core.perm(n);
        &&& self.wf()
        &&& self.settled()
        &&& self.worker == source.worker
        &&& self.same_region(source)
        &&& !M::CHECKED ==> {
            &&& self.consistent()
            &&& self.accounted(words, out, Some(n))
            &&& count_in(words, n) == 0
            &&& count_roots(out, n) == 0
        }
        &&& forall|i: int| 0 <= i < words.len() ==> #[trigger] self.usable(words[i])
        &&& forall|j: int|
            0 <= j < out.len() ==> (#[trigger] out[j] matches Some(w) ==> self.usable(w))
        &&& forall|key: usize| #[trigger]
            done.contains_key(key) ==> (done[key] matches Some(w) ==> self.dupable(w))
        &&& nw.is_ptr_spec()
        &&& self.core.word_ok(nw)
        &&& self.core.dom().contains(n)
        &&& self.core.state(n) == BlockState::Live
        &&& self.core.rc_of(n) > 0
        &&& !self.lin_of(nw)
        &&& self.owned(nw)
        &&& self.core.kind(n) == Kind::Data
        &&& p_first(p) == first
        &&& p_count(p) == count
        &&& forall|i: int|
            first + count <= i < first + count + raw ==> #[trigger] self.raw_at(nw, i)
        &&& 0 <= k <= count
        &&& forall|j: int| 0 <= j < k ==> (#[trigger] self.scanned_word(nw, j)).bits_spec() == 1
    }

    /// 確保の後も、前に使えた語は使え、`dup` できた物体は `dup` できる。
    proof fn lemma_made_words(&self, before: Heap<M>, nw: Word)
        requires
            before.wf(),
            before.settled(),
            self.made(before, nw),
            M::CHECKED ==> self.words_kept(before),
        ensures
            forall|w: Word| before.usable(w) ==> #[trigger] self.usable(w),
            forall|w: Word| before.dupable(w) ==> #[trigger] self.dupable(w),
    {
        if !M::CHECKED {
            let a = self.core;
            let b0 = before.core;
            let n = nw.bits_spec();
            assert(b0.settled_view() == b0);
            assert(!a.region_has(n));
            assert forall|h: usize|
                a.dom().contains(h) && !b0.dom().contains(h) implies !#[trigger] a.region_has(
                h,
            ) by {
                assert(h == n);
            }
            a.lemma_words_grown(b0);
            assert forall|w: Word| before.usable(w) implies #[trigger] self.usable(w) by {
                if w.is_ptr_spec() {
                    assert(b0.has(w.bits_spec()));
                }
            }
            assert forall|w: Word| before.dupable(w) implies #[trigger] self.dupable(w) by {
                assert(self.usable(w));
                if w.is_ptr_spec() {
                    let h = w.bits_spec();
                    if b0.dom().contains(h) {
                        assert(a.perm(h) == b0.perm(h));
                        assert(a.model@.owner[h] == b0.model@.owner[h]);
                        assert(a.rc_of(h) == b0.rc_of(h));
                    } else {
                        assert(!a.dom().contains(h));
                        a.lemma_region_at(h);
                    }
                }
            }
        } else {
            assert forall|w: Word| before.usable(w) implies #[trigger] self.usable(w) by {
                assert(before.word_in(w));
            }
            assert forall|w: Word| before.dupable(w) implies #[trigger] self.dupable(w) by {
                assert(before.word_in(w));
            }
        }
    }

    /// `alloc_data` の後、写しを作り始められる。
    proof fn lemma_build_start(
        &self,
        before: Heap<M>,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
        nw: Word,
        first: int,
        count: int,
        raw: int,
    )
        requires
            before.target_ok(source, words, out, done),
            self.made(before, nw),
            M::CHECKED ==> self.words_kept(before),
            self.kind_of(nw) == Kind::Data,
            self.count_of(nw) == count,
            !self.lin_of(nw),
            self.owned(nw),
            p_first(self.core.perm(nw.bits_spec())) == first,
            forall|i: int|
                first + count <= i < first + count + raw ==> #[trigger] self.raw_at(nw, i),
            forall|j: int|
                0 <= j < count ==> (#[trigger] self.scanned_word(nw, j)).bits_spec() == 1,
        ensures
            self.build_ok(source, words, out, done, nw, first, count, raw, count),
    {
        // `nw` を積んだ状態の補題を借りて、語のスタックに積む前の状態を出す
        self.lemma_target_made(before, source, words, out, done, nw, 0);
        let n = nw.bits_spec();
        if !M::CHECKED {
            let a = self.core;
            let b0 = before.core;
            assert(!b0.has(n));
            assert forall|i: int| 0 <= i < words.len() implies b0.word_ok(#[trigger] words[i]) by {
                assert(before.usable(words[i]));
            }
            assert forall|j: int| 0 <= j < out.len() implies (#[trigger] out[j] matches Some(w)
                ==> b0.word_ok(w)) by {
                if out[j] is Some {
                    assert(before.usable(out[j]->0));
                }
            }
            before.lemma_absent_from(words, out, n);
            lemma_count_push(words, nw);
            assert forall|b: usize| a.dom().contains(b) && a.rc_of(b) != 0 implies #[trigger] a.ext(
                b,
            ) == count_in(words, b) + count_roots(out, b) + if Some(n) == Some(b) {
                1int
            } else {
                0int
            } by {
                assert(a.ext(b) == count_in(words.push(nw), b) + count_roots(out, b));
            }
        }
        assert forall|i: int| 0 <= i < words.len() implies #[trigger] self.usable(words[i]) by {
            assert(words.push(nw)[i] == words[i]);
        }
        self.lemma_made_words(before, nw);
        assert forall|key: usize| #[trigger] done.contains_key(key) implies (done[key] matches Some(
            w,
        ) ==> self.dupable(w)) by {
            if done[key] is Some {
                assert(before.dupable(done[key]->0));
            }
        }
    }

    /// 写しの生の語を書いた後も、写しを作る状態は保たれる。
    proof fn lemma_build_raw(
        &self,
        before: Heap<M>,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
        nw: Word,
        first: int,
        count: int,
        raw: int,
        k: int,
        i: int,
        v: u64,
    )
        requires
            before.build_ok(source, words, out, done, nw, first, count, raw, k),
            first + count <= i,
            self.raw_set(before, nw, i, v),
            self.wf(),
            !M::CHECKED ==> self.consistent(),
            M::CHECKED ==> self.words_kept(before),
        ensures
            self.build_ok(source, words, out, done, nw, first, count, raw, k),
    {
        let n = nw.bits_spec();
        let a = self.core;
        let b0 = before.core;
        self.lemma_same_region_tied(before, source);
        assert(a.perm(n).meta == b0.perm(n).meta);
        assert(a.cap(n) == b0.cap(n));
        assert(forall|w: Word| #[trigger] a.word_ok(w) == b0.word_ok(w));
        assert forall|w: Word| before.usable(w) implies #[trigger] self.usable(w) by {
            if !M::CHECKED && w.is_ptr_spec() {
                let h = w.bits_spec();
                if h != n {
                    if b0.dom().contains(h) {
                        assert(a.perms@[h] == b0.perms@[h]);
                    }
                }
            }
        }
        assert forall|w: Word| before.dupable(w) implies #[trigger] self.dupable(w) by {
            assert(self.usable(w));
            if !M::CHECKED && w.is_ptr_spec() {
                let h = w.bits_spec();
                if h != n && b0.dom().contains(h) {
                    assert(a.perms@[h] == b0.perms@[h]);
                } else if !b0.dom().contains(h) {
                    b0.lemma_region_at(h);
                    a.lemma_region_at(h);
                }
            }
        }
        if !M::CHECKED {
            assert forall|b: usize| a.dom().contains(b) && a.rc_of(b) != 0 implies #[trigger] a.ext(
                b,
            ) == count_in(words, b) + count_roots(out, b) + if Some(n) == Some(b) {
                1int
            } else {
                0int
            } by {
                if b != n {
                    assert(a.perms@[b] == b0.perms@[b]);
                }
                assert(a.model == b0.model);
                assert(b0.dom().contains(b));
                assert(b0.rc_of(b) != 0);
                assert(b0.ext(b) == count_in(words, b) + count_roots(out, b) + if Some(n) == Some(
                    b,
                ) {
                    1int
                } else {
                    0int
                });
            }
        }
        assert forall|j: int| 0 <= j < words.len() implies #[trigger] self.usable(words[j]) by {
            assert(before.usable(words[j]));
        }
        assert forall|j: int| 0 <= j < out.len() implies (#[trigger] out[j] matches Some(w)
            ==> self.usable(w)) by {
            if out[j] is Some {
                assert(before.usable(out[j]->0));
            }
        }
        assert forall|key: usize| #[trigger] done.contains_key(key) implies (done[key] matches Some(
            w,
        ) ==> self.dupable(w)) by {
            if done[key] is Some {
                assert(before.dupable(done[key]->0));
            }
        }
        assert forall|j: int| 0 <= j < k implies (#[trigger] self.scanned_word(nw, j)).bits_spec()
            == 1 by {
            assert(before.scanned_word(nw, j).bits_spec() == 1);
            assert(first + j != i);
        }
        assert forall|j: int|
            first + count <= j < first + count + raw implies #[trigger] self.raw_at(nw, j) by {
            assert(before.raw_at(nw, j));
        }
    }

    /// 語のスタックの一番上の語を写しの `k - 1` 番目の走査する語に移した後も、写しを作る状態は保たれる。
    proof fn lemma_build_field(
        &self,
        before: Heap<M>,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
        nw: Word,
        first: int,
        count: int,
        raw: int,
        k: int,
    )
        requires
            before.build_ok(source, words, out, done, nw, first, count, raw, k),
            k > 0,
            words.len() > 0,
            self.field_set(before, nw, first + k - 1, words.last()),
            self.wf(),
            !M::CHECKED ==> self.consistent(),
            M::CHECKED ==> self.words_kept(before),
        ensures
            self.build_ok(source, words.drop_last(), out, done, nw, first, count, raw, k - 1),
    {
        let n = nw.bits_spec();
        let a = self.core;
        let b0 = before.core;
        let v = words.last();
        let i = first + k - 1;
        let ws = words.drop_last();
        self.lemma_same_region_tied(before, source);
        assert(a.perm(n).meta == b0.perm(n).meta);
        assert(a.cap(n) == b0.cap(n));
        assert(forall|w: Word| #[trigger] a.word_ok(w) == b0.word_ok(w));
        assert forall|w: Word| before.usable(w) implies #[trigger] self.usable(w) by {
            if !M::CHECKED && w.is_ptr_spec() {
                let h = w.bits_spec();
                if h != n {
                    if b0.dom().contains(h) {
                        assert(a.perms@[h] == b0.perms@[h]);
                    }
                }
            }
        }
        assert forall|w: Word| before.dupable(w) implies #[trigger] self.dupable(w) by {
            assert(self.usable(w));
            if !M::CHECKED && w.is_ptr_spec() {
                let h = w.bits_spec();
                if h != n && b0.dom().contains(h) {
                    assert(a.perms@[h] == b0.perms@[h]);
                } else if !b0.dom().contains(h) {
                    b0.lemma_region_at(h);
                    a.lemma_region_at(h);
                }
            }
        }
        lemma_count_pop(words);
        lemma_count_nonneg(ws, n);
        if !M::CHECKED {
            // `nw` の i 語目はまだ `()` なので、そこから出る辺はない
            assert(before.scanned_word(nw, k - 1).bits_spec() == 1);
            crate::word::lemma_unit_bits();
            reveal(Core::edges_wf);
            if b0.model@.edges.dom().contains((n, i)) {
                assert(b0.edge_wf((n, i)));
            }
            assert(!b0.model@.edges.dom().contains((n, i)));
            let e0 = b0.model@.edges;
            assert(e0.remove((n, i)) =~= e0);
            if v.is_ptr_spec() {
                lemma_indeg_insert(e0, (n, i), v.bits_spec());
            }
            assert forall|b: usize| a.dom().contains(b) && a.rc_of(b) != 0 implies #[trigger] a.ext(
                b,
            ) == count_in(ws, b) + count_roots(out, b) + if Some(n) == Some(b) {
                1int
            } else {
                0int
            } by {
                if b != n {
                    assert(a.perms@[b] == b0.perms@[b]);
                }
                assert(b0.ext(b) == count_in(words, b) + count_roots(out, b) + if Some(n) == Some(
                    b,
                ) {
                    1int
                } else {
                    0int
                });
                if v.is_ptr_spec() && b == v.bits_spec() {
                    assert(a.indeg(b) == b0.indeg(b) + 1);
                } else {
                    assert(a.indeg(b) == b0.indeg(b));
                }
            }
        }
        assert forall|j: int| 0 <= j < ws.len() implies #[trigger] self.usable(ws[j]) by {
            assert(ws[j] == words[j]);
            assert(before.usable(words[j]));
        }
        assert forall|j: int| 0 <= j < out.len() implies (#[trigger] out[j] matches Some(w)
            ==> self.usable(w)) by {
            if out[j] is Some {
                assert(before.usable(out[j]->0));
            }
        }
        assert forall|key: usize| #[trigger] done.contains_key(key) implies (done[key] matches Some(
            w,
        ) ==> self.dupable(w)) by {
            if done[key] is Some {
                assert(before.dupable(done[key]->0));
            }
        }
        assert forall|j: int| 0 <= j < k - 1 implies (#[trigger] self.scanned_word(
            nw,
            j,
        )).bits_spec() == 1 by {
            assert(before.scanned_word(nw, j).bits_spec() == 1);
            assert(first + j != i);
        }
        assert forall|j: int|
            first + count <= j < first + count + raw implies #[trigger] self.raw_at(nw, j) by {
            assert(before.raw_at(nw, j));
        }
    }

    /// 写しを作り終えたら、写しを語のスタックに積み、写し元のブロック `key` の写しにできる。
    proof fn lemma_build_end(
        &self,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
        nw: Word,
        first: int,
        count: int,
        raw: int,
        key: usize,
    )
        requires
            self.build_ok(source, words, out, done, nw, first, count, raw, 0),
        ensures
            self.target_ok(source, words.push(nw), out, done.insert(key, Some(nw))),
    {
        let n = nw.bits_spec();
        let ws = words.push(nw);
        let d2 = done.insert(key, Some(nw));
        lemma_count_push(words, nw);
        assert(self.core.settled_state(n) == BlockState::Live);
        assert(self.usable(nw));
        assert(self.dupable(nw));
        if !M::CHECKED {
            assert forall|b: usize|
                self.core.dom().contains(b) && self.core.rc_of(b)
                    != 0 implies #[trigger] self.core.ext(b) == count_in(ws, b) + count_roots(
                out,
                b,
            ) + if None::<usize> == Some(b) {
                1int
            } else {
                0int
            } by {}
        }
        assert forall|i: int| 0 <= i < ws.len() implies #[trigger] self.usable(ws[i]) by {
            if i < words.len() {
                assert(ws[i] == words[i]);
            }
        }
        assert forall|k: usize| #[trigger] d2.contains_key(k) implies (d2[k] matches Some(w)
            ==> self.dupable(w)) by {
            if k != key {
                assert(done.contains_key(k));
            }
        }
    }

    /// 語のスタックのただ1つの語を、写し終えた根に移す。
    proof fn lemma_target_move(
        &self,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
    )
        requires
            self.target_ok(source, words, out, done),
            words.len() == 1,
        ensures
            self.target_ok(source, Seq::<Word>::empty(), out.push(Some(words[0])), done),
    {
        let w = words[0];
        let o2 = out.push(Some(w));
        lemma_roots_push(out, Some(w));
        assert(words.drop_last() =~= Seq::<Word>::empty());
        if !M::CHECKED {
            assert forall|b: usize|
                self.core.dom().contains(b) && self.core.rc_of(b)
                    != 0 implies #[trigger] self.core.ext(b) == count_in(Seq::<Word>::empty(), b)
                + count_roots(o2, b) + if None::<usize> == Some(b) {
                1int
            } else {
                0int
            } by {
                assert(count_in(words, b) == count_in(words.drop_last(), b) + at(words.last(), b));
            }
        }
        assert forall|j: int| 0 <= j < o2.len() implies (#[trigger] o2[j] matches Some(v)
            ==> self.usable(v)) by {
            if j < out.len() {
                assert(o2[j] == out[j]);
            } else {
                assert(self.usable(words[0]));
            }
        }
    }

    /// 写している途中の物体 `f` の写しを作る。走査する語の写しは語のスタックの上の `f.count` 個で、後ろからフィールドに
    /// 移す。作った写しは語のスタックに積み、写した表に入れる。
    #[expect(clippy::too_many_arguments, reason = "the parts of one copy")]
    fn build(
        &self,
        new: &mut Heap<M>,
        done: &mut HashMapWithView<usize, Option<Word>>,
        words: &mut Vec<Word>,
        f: Frame,
        Ghost(set): Ghost<Set<usize>>,
        Ghost(frames): Ghost<Seq<Frame>>,
        Ghost(out): Ghost<Seq<Option<Word>>>,
    ) -> (r: Result<(), HeapError>)
        requires
            self.source_ok(set, old(done)@, frames),
            self.frame_ok(f, old(done)@),
            forall|i: int| 0 <= i < frames.len() ==> #[trigger] frames[i].h@.addr != f.h@.addr,
            old(new).target_ok(*self, old(words)@, out, old(done)@),
        ensures
            final(new).wf(),
            final(new).settled(),
            final(new).worker == self.worker,
            r is Ok ==> {
                &&& self.source_ok(set, final(done)@, frames)
                &&& final(new).target_ok(*self, final(words)@, out, final(done)@)
                &&& final(done)@.dom() == old(done)@.dom()
            },
    {
        let ghost n0 = *new;
        let ghost ws0 = words@;
        let ghost d0 = done@;
        let ghost h = f.h@.addr;
        proof {
            if M::CHECKED {
                self.core.lemma_safe_at(h);
            } else {
                self.core.lemma_at(h);
            }
        }
        let raw = f.cap - f.first - f.count;
        let nw = new.alloc_data(f.tag, f.count, raw, false);
        let nfirst: usize = if f.count >= 255 {
            1
        } else {
            0
        };
        proof {
            assert(p_first(new.core.perm(nw.bits_spec())) == nfirst);
            new.lemma_build_start(
                n0,
                *self,
                ws0,
                out,
                d0,
                nw,
                nfirst as int,
                f.count as int,
                raw as int,
            );
        }
        let mut j: usize = 0;
        while j < raw
            invariant
                new.build_ok(
                    *self,
                    ws0,
                    out,
                    d0,
                    nw,
                    nfirst as int,
                    f.count as int,
                    raw as int,
                    f.count as int,
                ),
                self.source_ok(set, d0, frames),
                self.frame_ok(f, d0),
                raw == f.cap - f.first - f.count,
                nfirst <= 1,
                0 <= j <= raw,
                words@ == ws0,
                done@ == d0,
                h == f.h@.addr,
                p_filled(self.core.perm(h), self.core.cap(h)),
            decreases raw - j,
        {
            proof {
                assert(self.frame_ok(f, d0));
                assert(h == f.h@.addr);
                assert(f.cap == self.core.cap(f.h@.addr));
                assert(f.first + f.count + j < f.cap);
                lemma_filled_init(
                    self.core.perm(h),
                    self.core.cap(h),
                    (f.first + f.count + j) as int,
                );
            }
            let x = self.core.slot_get(f.h, f.first + f.count + j);
            let ghost before = *new;
            match unsafe { new.set_raw(nw, nfirst + f.count + j, x.bits() as u64) } {
                Ok(()) => {},
                Err(e) => return Err(e),
            }
            proof {
                new.lemma_build_raw(
                    before,
                    *self,
                    ws0,
                    out,
                    d0,
                    nw,
                    nfirst as int,
                    f.count as int,
                    raw as int,
                    f.count as int,
                    (nfirst + f.count + j) as int,
                    x.bits_spec() as u64,
                );
            }
            j += 1;
        }
        let mut k: usize = f.count;
        while k > 0
            invariant
                new.build_ok(
                    *self,
                    words@,
                    out,
                    d0,
                    nw,
                    nfirst as int,
                    f.count as int,
                    raw as int,
                    k as int,
                ),
                self.source_ok(set, d0, frames),
                self.frame_ok(f, d0),
                nfirst <= 1,
                0 <= k <= f.count,
                done@ == d0,
                h == f.h@.addr,
            decreases k,
        {
            let ghost wsb = words@;
            let v = match words.pop() {
                Some(v) => v,
                None => return Err(HeapError::Unrecoverable),
            };
            let ghost before = *new;
            proof {
                assert(wsb.last() == v);
                assert(words@ =~= wsb.drop_last());
                assert(before.usable(v)) by {
                    assert(before.usable(wsb[wsb.len() - 1]));
                }
                if !M::CHECKED {
                    lemma_count_pop(wsb);
                    lemma_count_nonneg(wsb.drop_last(), v.bits_spec());
                    lemma_roots_absent(Seq::<Option<Word>>::empty(), 0);
                    assert(before.core.settled_view() == before.core);
                    if v.is_ptr_spec() && before.core.rc_of(v.bits_spec()) != 0 {
                        assert(before.core.dom().contains(v.bits_spec())) by {
                            if !before.core.dom().contains(v.bits_spec()) {
                                before.core.lemma_region_at(v.bits_spec());
                            }
                        }
                        lemma_count_nonneg(Seq::<Word>::empty(), 0);
                        assert(count_in(wsb, v.bits_spec()) >= 1);
                        assert(before.core.ext(v.bits_spec()) >= 1) by {
                            assert(count_roots(out, v.bits_spec()) >= 0) by {
                                lemma_roots_nonneg(out, v.bits_spec());
                            }
                        }
                    }
                    assert(before.holds(v));
                }
            }
            match unsafe { new.set_field(nw, nfirst + k - 1, v) } {
                Ok(()) => {},
                Err(e) => return Err(e),
            }
            proof {
                new.lemma_build_field(
                    before,
                    *self,
                    wsb,
                    out,
                    d0,
                    nw,
                    nfirst as int,
                    f.count as int,
                    raw as int,
                    k as int,
                );
            }
            k -= 1;
        }
        let ghost wsf = words@;
        done.insert(f.h.addr(), Some(nw));
        words.push(nw);
        proof {
            new.lemma_build_end(
                *self,
                wsf,
                out,
                d0,
                nw,
                nfirst as int,
                f.count as int,
                raw as int,
                h,
            );
            assert(done@.dom() =~= d0.dom());
            assert forall|i: int| 0 <= i < frames.len() implies #[trigger] self.frame_ok(
                frames[i],
                done@,
            ) by {
                assert(self.frame_ok(frames[i], d0));
                assert(frames[i].h@.addr != h);
            }
        }
        Ok(())
    }

    /// 写している途中の物体がなくなるまで、一番上の物体の次の走査する語を写すか、写し終えた物体の写しを作る。
    fn copy_frames(
        &self,
        new: &mut Heap<M>,
        set: &HashSetWithView<usize>,
        done: &mut HashMapWithView<usize, Option<Word>>,
        frames: &mut Vec<Frame>,
        words: &mut Vec<Word>,
        Ghost(out): Ghost<Seq<Option<Word>>>,
    ) -> (r: Result<(), HeapError>)
        requires
            self.source_ok(set@, old(done)@, old(frames)@),
            old(new).target_ok(*self, old(words)@, out, old(done)@),
        ensures
            final(new).wf(),
            final(new).settled(),
            final(new).worker == self.worker,
            r is Ok ==> {
                &&& final(frames)@.len() == 0
                &&& self.source_ok(set@, final(done)@, final(frames)@)
                &&& final(new).target_ok(*self, final(words)@, out, final(done)@)
            },
    {
        while !frames.is_empty()
            invariant
                self.source_ok(set@, done@, frames@),
                new.target_ok(*self, words@, out, done@),
            decreases
                    set@.len() - done@.dom().len(),
                    frames@.len(),
                    if frames@.len() > 0 {
                        frames@.last().count - frames@.last().next
                    } else {
                        0
                    },
        {
            proof {
                vstd::set_lib::lemma_len_subset(done@.dom(), set@);
            }
            let top = frames.len() - 1;
            let f = frames[top];
            proof {
                assert(self.frame_ok(f, done@));
            }
            if f.next < f.count {
                let ghost hf = f.h@.addr;
                let ghost i = (f.first + f.next) as int;
                proof {
                    assert(self.core.scanned(hf, i));
                    if M::CHECKED {
                        self.core.lemma_safe_at(hf);
                        assert(self.core.safe_block_wf(hf));
                        assert(self.core.safe_slot(self.core.perm(hf), i));
                    } else {
                        self.core.lemma_live_slot(hf, i);
                    }
                }
                let w = self.core.slot_get(f.h, f.first + f.next);
                let nf = Frame {
                    h: f.h,
                    tag: f.tag,
                    first: f.first,
                    count: f.count,
                    cap: f.cap,
                    next: f.next + 1,
                };
                let ghost before = frames@;
                frames.set(top, nf);
                proof {
                    assert forall|j: int| 0 <= j < frames@.len() implies #[trigger] self.frame_ok(
                        frames@[j],
                        done@,
                    ) by {
                        if j != top {
                            assert(frames@[j] == before[j]);
                        }
                    }
                    assert forall|a: int, b: int|
                        0 <= a < frames@.len() && 0 <= b < frames@.len() && a != b implies (
                    #[trigger] frames@[a]).h@.addr != (#[trigger] frames@[b]).h@.addr by {
                        assert(before[a].h == frames@[a].h);
                        assert(before[b].h == frames@[b].h);
                    }
                    assert(self.source_word(w));
                }
                let ghost f1 = frames@;
                let ghost d1 = done@.dom();
                match self.resolve(new, set, done, frames, words, w, Ghost(out)) {
                    Ok(()) => {},
                    Err(e) => return Err(e),
                }
                proof {
                    vstd::set_lib::lemma_len_subset(d1, done@.dom());
                    vstd::set_lib::lemma_len_subset(done@.dom(), set@);
                }
            } else {
                let ghost before = frames@;
                let f = match frames.pop() {
                    Some(f) => f,
                    None => return Err(HeapError::Unrecoverable),
                };
                proof {
                    assert(frames@ =~= before.drop_last());
                    assert forall|j: int| 0 <= j < frames@.len() implies #[trigger] self.frame_ok(
                        frames@[j],
                        done@,
                    ) by {
                        assert(frames@[j] == before[j]);
                    }
                    assert forall|j: int|
                        0 <= j < frames@.len() implies #[trigger] frames@[j].h@.addr
                        != f.h@.addr by {
                        assert(frames@[j] == before[j]);
                        assert(f == before[top as int]);
                    }
                }
                match self.build(new, done, words, f, Ghost(set@), Ghost(frames@), Ghost(out)) {
                    Ok(()) => {},
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    /// 根 `roots` から届く物体を `new` に写し、写した根を返す。誤りのときも `new` は不変条件を保ち、呼ぶ側が捨てる。
    pub(crate) fn copy_roots(&mut self, new: &mut Heap<M>, roots: &[Option<Word>]) -> (r: Result<
        Vec<Option<Word>>,
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).settled(),
            !M::CHECKED ==> old(self).consistent(),
            old(new).wf(),
            old(new).settled(),
            old(new).unused(),
            !M::CHECKED ==> old(new).consistent(),
            old(new).worker_spec() == old(self).worker_spec(),
            old(new).same_region(*old(self)),
            forall|j: int|
                0 <= j < roots@.len() ==> (#[trigger] roots@[j] matches Some(w) ==> old(
                    self,
                ).source_word(w)),
        ensures
            *final(self) == *old(self),
            final(new).wf(),
            final(new).settled(),
            final(new).worker_spec() == old(new).worker_spec(),
            r matches Ok(out) ==> {
                &&& out@.len() == roots@.len()
                &&& forall|j: int|
                    0 <= j < out@.len() ==> (#[trigger] out@[j] is Some) == (roots@[j] is Some)
                &&& final(new).target_ok(*old(self), Seq::<Word>::empty(), out@, Map::empty())
                &&& !M::CHECKED ==> final(new).consistent() && final(new).roots_counted(out@)
                &&& forall|j: int|
                    0 <= j < out@.len() ==> (#[trigger] out@[j] matches Some(w)
                        ==> final(new).word_in(w) && (!M::CHECKED ==> final(new).readable(w)))
            },
    {
        proof {
            self.core.lemma_carved_apart();
        }
        let blocks = self.core.pages.blocks();
        let mut set: HashSetWithView<usize> = HashSetWithView::new();
        let mut i: usize = 0;
        while i < blocks.len()
            invariant
                self.wf(),
                0 <= i <= blocks@.len(),
                forall|h: usize| #[trigger]
                    set@.contains(h) <==> exists|k: int| 0 <= k < i && blocks@[k]@.addr == h,
            decreases blocks.len() - i,
        {
            let a = blocks[i].addr();
            set.insert(a);
            proof {
                assert forall|h: usize| #[trigger]
                    set@.contains(h) <==> exists|k: int|
                        0 <= k < i + 1 && blocks@[k]@.addr == h by {
                    if h == a {
                        assert(blocks@[i as int]@.addr == h);
                    }
                }
            }
            i += 1;
        }
        proof {
            assert forall|h: usize| #[trigger]
                set@.contains(h) == self.core.pages.carved_block(h) by {
                if self.core.pages.carved_block(h) {
                    let k = choose|k: int| 0 <= k < blocks@.len() && blocks@[k]@.addr == h;
                    assert(blocks@[k]@.addr == h);
                }
            }
        }
        let mut done: HashMapWithView<usize, Option<Word>> = HashMapWithView::new();
        let mut frames: Vec<Frame> = Vec::new();
        let mut words: Vec<Word> = Vec::new();
        let mut out: Vec<Option<Word>> = Vec::new();
        proof {
            assert(done@.dom() =~= Set::<usize>::empty());
            if !M::CHECKED {
                assert forall|b: usize|
                    new.core.dom().contains(b) && new.core.rc_of(b)
                        != 0 implies #[trigger] new.core.ext(b) == count_in(words@, b)
                    + count_roots(out@, b) + if None::<usize> == Some(b) {
                    1int
                } else {
                    0int
                } by {}
            }
            assert(new.target_ok(*self, words@, out@, done@));
            assert(self.source_ok(set@, done@, frames@));
        }
        let mut j: usize = 0;
        while j < roots.len()
            invariant
                self.wf(),
                *self == *old(self),
                self.source_ok(set@, done@, frames@),
                frames@.len() == 0,
                words@.len() == 0,
                new.target_ok(*self, words@, out@, done@),
                old(new).worker_spec() == self.worker_spec(),
                0 <= j <= roots@.len(),
                out@.len() == j,
                forall|k: int| 0 <= k < j ==> (#[trigger] out@[k] is Some) == (roots@[k] is Some),
                forall|k: int|
                    0 <= k < roots@.len() ==> (#[trigger] roots@[k] matches Some(w)
                        ==> self.source_word(w)),
            decreases roots.len() - j,
        {
            match roots[j] {
                None => {
                    let ghost o0 = out@;
                    out.push(None);
                    proof {
                        lemma_roots_push(o0, None);
                        if !M::CHECKED {
                            assert forall|b: usize|
                                new.core.dom().contains(b) && new.core.rc_of(b)
                                    != 0 implies #[trigger] new.core.ext(b) == count_in(words@, b)
                                + count_roots(out@, b) + if None::<usize> == Some(b) {
                                1int
                            } else {
                                0int
                            } by {}
                        }
                        assert forall|k: int| 0 <= k < out@.len() implies (
                        #[trigger] out@[k] matches Some(v) ==> new.usable(v)) by {
                            if k < o0.len() {
                                assert(out@[k] == o0[k]);
                            }
                        }
                    }
                },
                Some(w) => {
                    proof {
                        assert(self.source_word(w));
                    }
                    match self.resolve(
                        new,
                        &set,
                        &mut done,
                        &mut frames,
                        &mut words,
                        w,
                        Ghost(out@),
                    ) {
                        Ok(()) => {},
                        Err(e) => {
                            proof {
                                assert(new.worker_spec() == old(new).worker_spec());
                            }
                            return Err(e);
                        },
                    }
                    match self.copy_frames(
                        new,
                        &set,
                        &mut done,
                        &mut frames,
                        &mut words,
                        Ghost(out@),
                    ) {
                        Ok(()) => {},
                        Err(e) => {
                            proof {
                                assert(new.worker_spec() == old(new).worker_spec());
                            }
                            return Err(e);
                        },
                    }
                    if words.len() != 1 {
                        return Err(HeapError::Unrecoverable);
                    }
                    let ghost ws = words@;
                    let ghost o0 = out@;
                    let nw = words[0];
                    words.clear();
                    out.push(Some(nw));
                    proof {
                        new.lemma_target_move(*self, ws, o0, done@);
                        assert(words@ =~= Seq::<Word>::empty());
                        assert(out@ =~= o0.push(Some(ws[0])));
                    }
                },
            }
            j += 1;
        }
        proof {
            assert(new.target_ok(*self, Seq::<Word>::empty(), out@, done@));
            assert forall|k: usize| #[trigger]
                Map::<usize, Option<Word>>::empty().contains_key(k) implies (Map::<
                usize,
                Option<Word>,
            >::empty()[k] matches Some(v) ==> new.dupable(v)) by {}
            assert forall|k: int| 0 <= k < out@.len() implies (#[trigger] out@[k] matches Some(w)
                ==> new.word_in(w) && (!M::CHECKED ==> new.readable(w))) by {
                if out@[k] is Some {
                    assert(new.usable(out@[k]->0));
                }
            }
        }
        Ok(out)
    }

    /// 根の表 `roots` の語は、それぞれ写し先の参照を1つ持つ。rc が 0 でないブロックの外の参照の数は、根の中の出現の
    /// 数に等しい。
    pub closed spec fn roots_counted(&self, roots: Seq<Option<Word>>) -> bool {
        self.accounted(Seq::<Word>::empty(), roots, None)
    }

    /// 写し先の参照の勘定。rc が 0 でないブロックの外の参照の数は、語のスタックと写し終えた根の中の出現の数 (と、
    /// 作っている途中の物体 `extra` の1つ) に等しい。
    pub(crate) closed spec fn accounted(
        &self,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        extra: Option<usize>,
    ) -> bool {
        forall|b: usize|
            #![trigger self.core.ext(b)]
            self.core.dom().contains(b) && self.core.rc_of(b) != 0 ==> self.core.ext(b) == count_in(
                words,
                b,
            ) + count_roots(out, b) + if extra == Some(b) {
                1int
            } else {
                0int
            }
    }

    /// 写し先の語 `w` は使える。速いモードでは読める。
    pub(crate) closed spec fn usable(&self, w: Word) -> bool {
        &&& self.word_in(w)
        &&& !M::CHECKED ==> self.readable(w)
    }

    /// 写し先の物体 `w` を、2回目からの使い手のために `dup` できる。`lin` でなく、持ち主が今の worker である。
    pub(crate) closed spec fn dupable(&self, w: Word) -> bool {
        &&& self.usable(w)
        &&& !M::CHECKED ==> (w.is_ptr_spec() ==> !self.lin_of(w)) && self.owned(w)
    }

    /// 写し先 `self` の状態。`old` は写し始めの写し先である。
    pub(crate) closed spec fn target_ok(
        &self,
        source: Heap<M>,
        words: Seq<Word>,
        out: Seq<Option<Word>>,
        done: Map<usize, Option<Word>>,
    ) -> bool {
        &&& self.wf()
        &&& self.settled()
        &&& self.worker == source.worker
        &&& self.same_region(source)
        &&& !M::CHECKED ==> self.consistent() && self.accounted(words, out, None)
        &&& forall|i: int| 0 <= i < words.len() ==> #[trigger] self.usable(words[i])
        &&& forall|j: int|
            0 <= j < out.len() ==> (#[trigger] out[j] matches Some(w) ==> self.usable(w))
        &&& forall|k: usize| #[trigger]
            done.contains_key(k) ==> (done[k] matches Some(w) ==> self.dupable(w))
    }

    /// 写し元 `self` の状態。`set` は切り出したブロックの番地の集合で、`frames` の物体はどれも写している途中である。
    pub(crate) closed spec fn source_ok(
        &self,
        set: Set<usize>,
        done: Map<usize, Option<Word>>,
        frames: Seq<Frame>,
    ) -> bool {
        &&& self.wf()
        &&& self.settled()
        &&& !M::CHECKED ==> self.consistent()
        &&& forall|h: usize| #[trigger] set.contains(h) == self.core.pages.carved_block(h)
        &&& done.dom().subset_of(set)
        &&& forall|h: usize| #[trigger] self.core.pages.carved_block(h) ==> !self.core.region_has(h)
        &&& forall|i: int| 0 <= i < frames.len() ==> #[trigger] self.frame_ok(frames[i], done)
        &&& forall|i: int, j: int|
            0 <= i < frames.len() && 0 <= j < frames.len() && i != j ==> (
            #[trigger] frames[i]).h@.addr != (#[trigger] frames[j]).h@.addr
    }

    pub(crate) closed spec fn frame_ok(&self, f: Frame, done: Map<usize, Option<Word>>) -> bool {
        let h = f.h@.addr;
        let p = self.core.perm(h);
        &&& self.core.dom().contains(h)
        &&& f.h == self.core.block(h)
        &&& self.core.state(h) == BlockState::Live
        &&& p_kind(p) == Kind::Data
        &&& f.first == p_first(p)
        &&& f.count == p_count(p)
        &&& f.cap == self.core.cap(h)
        &&& f.first + f.count <= f.cap
        &&& f.next <= f.count
        &&& done.contains_key(h) && done[h] is None
    }

    /// まだ写していないブロック `a` を写した表 (`done`) に入れても、写し元の状態は保たれる。
    proof fn lemma_source_insert(
        &self,
        set: Set<usize>,
        done: Map<usize, Option<Word>>,
        frames: Seq<Frame>,
        a: usize,
        v: Option<Word>,
    )
        requires
            self.source_ok(set, done, frames),
            set.contains(a),
            !done.contains_key(a),
        ensures
            self.source_ok(set, done.insert(a, v), frames),
    {
        assert forall|i: int| 0 <= i < frames.len() implies #[trigger] self.frame_ok(
            frames[i],
            done.insert(a, v),
        ) by {
            assert(self.frame_ok(frames[i], done));
        }
    }

    /// 写し元の語 `w` を読める。
    pub(crate) open spec fn source_word(&self, w: Word) -> bool {
        &&& self.word_in(w)
        &&& !M::CHECKED ==> self.readable(w)
    }
}

} // verus!

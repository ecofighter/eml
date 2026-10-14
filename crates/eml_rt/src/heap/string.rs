//! 文字列の物体 (docs/spec/object-model.md の「文字列」)。種類 `String` の物体は、ここの操作だけが作り、書く。
//! 長さの語は生の 0 語目で、バイト列はその後ろにあり、ghost のバイト列 `text` に等しい。バイト列を書くのは、作るときと
//! `string_push` だけである。

use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;
#[cfg(verus_only)]
use vstd::utf8::{encode_utf8_valid_utf8, valid_utf8, valid_utf8_concat};

use super::*;
#[cfg(verus_only)]
use crate::heap::inv::string_wf;
#[cfg(verus_only)]
use crate::meta::{field_ptr, slot_offset};
use crate::text::{text, text_ptr, write_text};

verus! {

/// 文字列の後ろに写すバイト列。`Str` は呼ぶ側の `&str`、`Text` はこのヒープの文字列の物体 (ヘッダと長さ) である。
/// `string_concat` の右辺はヒープの権限から借りて読むので、`&mut self` を取る操作に `&str` として渡せない。そのため、
/// 右辺の権限は写す直前に `Core::write_tail` の中で借りる。
enum Tail<'a> {
    Str(&'a str),
    Text(*mut Header, usize),
}

impl Core {
    spec fn tail_bytes(&self, t: Tail) -> Seq<u8> {
        match t {
            Tail::Str(s) => s.spec_bytes(),
            Tail::Text(r, _) => self.perm(r@.addr).text(),
        }
    }

    /// `t` を読める。`Text` なら、このヒープの文字列のブロックで、長さはバイト列の長さに等しい。
    spec fn tail_ok(&self, t: Tail) -> bool {
        match t {
            Tail::Str(_) => true,
            Tail::Text(r, n) => {
                &&& self.perms@.contains_key(r@.addr)
                &&& self.perm(r@.addr).holds_text(r, n as int)
                &&& n == self.perm(r@.addr).text().len()
                &&& valid_utf8(self.perm(r@.addr).text())
            },
        }
    }

    spec fn opt_bytes(&self, t: Option<Tail>) -> Seq<u8> {
        match t {
            Some(t) => self.tail_bytes(t),
            None => Seq::empty(),
        }
    }

    spec fn opt_ok(&self, t: Option<Tail>) -> bool {
        match t {
            Some(t) => self.tail_ok(t),
            None => true,
        }
    }

    proof fn lemma_tail_utf8(&self, t: Tail)
        requires
            self.tail_ok(t),
        ensures
            valid_utf8(self.tail_bytes(t)),
    {
        if let Tail::Str(s) = t {
            encode_utf8_valid_utf8(s@);
        }
    }

    fn tail_len(&self, t: &Tail) -> (r: usize)
        requires
            self.tail_ok(*t),
        ensures
            r == self.tail_bytes(*t).len(),
    {
        match t {
            Tail::Str(s) => {
                // vstd の `str::len` の仕様は長さが usize に収まることを言わないので、スライスの長さで読む
                #[allow(clippy::needless_as_bytes)]
                let n = s.as_bytes().len();
                n
            },
            Tail::Text(_, n) => *n,
        }
    }

    /// `h` のペイロードの `at` バイト目から `t` を写す。`write_text` と同じ約束で、`Text` ならその物体の権限をここで借りる。
    fn write_tail(
        &self,
        h: *mut Header,
        cap: usize,
        at: usize,
        t: &Tail,
        Tracked(perm): Tracked<&mut BlockPerm>,
    )
        requires
            forall|i: int| #[trigger] old(perm).slots.contains_key(i) <==> 0 <= i < cap,
            cap >= 1,
            old(perm).holds_text(h, 8 * (cap - 1)),
            at == old(perm).text().len(),
            at + self.tail_bytes(*t).len() <= 8 * (cap - 1),
            self.tail_ok(*t),
        ensures
            final(perm).text() == old(perm).text() + self.tail_bytes(*t),
            final(perm).rc == old(perm).rc,
            final(perm).meta == old(perm).meta,
            final(perm).prefix == old(perm).prefix,
            final(perm).slots.dom() == old(perm).slots.dom(),
            final(perm).slots[0] == old(perm).slots[0],
            forall|i: int|
                0 <= i < cap ==> (#[trigger] final(perm).slots[i]).ptr() == old(
                    perm,
                ).slots[i].ptr(),
    {
        match t {
            Tail::Str(s) => write_text(h, cap, at, s, Tracked(perm)),
            Tail::Text(r, n) => {
                let tracked p = self.perms.borrow().tracked_borrow(r@.addr);
                write_text(h, cap, at, text(*r, *n, Tracked(p)), Tracked(perm));
            },
        }
    }

    /// `old` から、文字列 `h` の長さの語とバイト列だけが変わった (`string_push` のその場の追記)。
    pub(crate) open spec fn text_changed(&self, old: Core, h: usize) -> bool {
        &&& self.pages == old.pages
        &&& self.checked == old.checked
        &&& self.model == old.model
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& self.perm(h).rc == old.perm(h).rc
        &&& self.perm(h).meta == old.perm(h).meta
        &&& self.perm(h).prefix == old.perm(h).prefix
        &&& self.perm(h).slots.dom() == old.perm(h).slots.dom()
        &&& forall|j: int|
            self.perm(h).slots.contains_key(j) ==> (#[trigger] self.perm(h).slots[j]).ptr()
                == old.perm(h).slots[j].ptr()
        &&& string_wf(self.perm(h), self.cap(h))
    }

    /// その場の追記の後も、不変条件は保たれる。文字列は走査する語を持たないので、辺と数は変わらない。
    proof fn lemma_text_set(&self, s0: Core, h: usize)
        requires
            s0.full(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            s0.kind(h) == Kind::String,
            self.text_changed(s0, h),
        ensures
            self.full(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        let p = self.perm(h);
        let q = s0.perm(h);
        assert(s0.block_wf(h));
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            if b == h {
                assert forall|j: int| 0 <= j < self.cap(h) implies (#[trigger] p.slots[j]).ptr()
                    == field_ptr::<Word>(self.block(h), slot_offset(j)) by {
                    assert(q.slots.contains_key(j));
                }
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                    p,
                    h,
                    i,
                ) by {}
            } else {
                assert(self.perm(b) == s0.perm(b));
                let pb = self.perm(b);
                if s0.state(b) == BlockState::Live {
                    assert forall|j: int| #[trigger] p_scanned(pb, j) implies self.slot_word(
                        pb,
                        b,
                        j,
                    ) by {
                        assert(s0.slot_word(pb, b, j));
                    }
                }
                if s0.state(b) == BlockState::Building {
                    assert forall|j: int| #[trigger]
                        self.model@.written[b].contains(j) implies p_scanned(pb, j)
                        && self.slot_word(pb, b, j) by {
                        assert(s0.slot_word(pb, b, j));
                    }
                }
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.edges.dom().contains(e) implies self.edge_wf(
            e,
        ) by {
            assert(s0.edge_wf(e));
            // 文字列は走査する語を持たないので、辺の出どころにならない
            assert(e.0 != h);
            assert(self.perms@[e.0] == s0.perms@[e.0]);
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 検査モードで、その場の追記の後も安全の条件は保たれる。
    proof fn lemma_safe_text_set(&self, s0: Core, h: usize)
        requires
            s0.safe_wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            self.text_changed(s0, h),
        ensures
            self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        let p = self.perm(h);
        let q = s0.perm(h);
        assert(s0.safe_block_wf(h));
        assert forall|j: int| 0 <= j < self.cap(h) implies (#[trigger] p.slots[j]).ptr()
            == field_ptr::<Word>(self.block(h), slot_offset(j)) by {
            assert(q.slots.contains_key(j));
        }
        assert(self.safe_block_wf(h));
        // 生きている物体は「残した」語を持たない
        assert forall|j: int| #[trigger]
            self.model@.kept.contains((h, j)) implies self.safe_kept_wf((h, j)) by {
            assert(s0.safe_kept_wf((h, j)));
        }
        if s0.registered(h) {
            s0.lemma_registered(h);
        }
        self.lemma_safe_step(s0, h);
    }
}

impl<M: Mode> Heap<M> {
    pub closed spec fn is_string(&self, w: Word) -> bool {
        self.core.kind(w.bits_spec()) == Kind::String
    }

    /// 文字列のバイト列。
    pub closed spec fn text_of(&self, w: Word) -> Seq<u8> {
        self.core.perm(w.bits_spec()).text()
    }

    /// `alloc_string_immortal` の後の状態。`w` は rc 0 の生きている物体で、検査モードの外に出ている参照の数は 0 である。
    pub closed spec fn made_immortal(&self, old: Heap<M>, w: Word) -> bool {
        let h = w.bits_spec();
        &&& self.wf()
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.model@.building is None
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& !old.core.dom().contains(h)
        &&& self.core.dom() == old.core.dom().insert(h)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.rc_of(h) == 0
        &&& M::CHECKED ==> p_owner(self.core.perm(h)) == 0
        &&& old.consistent() && old.settleable() ==> {
            &&& self.consistent()
            &&& self.core.frame(old.core)
        }
    }

    /// `s` の UTF-8 のバイト列を持つ文字列を作る。
    pub fn alloc_string(&mut self, s: &str) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == s.spec_bytes(),
    {
        self.make_string(s, 1)
    }

    /// `alloc_string` と同じで、rc 0 の不死の物体にする。`CodeBase` の文字列の定数が使う。
    pub fn alloc_string_immortal(&mut self, s: &str) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made_immortal(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == s.spec_bytes(),
    {
        self.make_string(s, 0)
    }

    /// `s` を写した文字列を、rc `rc` の生きている物体として作る。
    fn make_string(&mut self, s: &str, rc: i32) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            rc == 0 || rc == 1,
        ensures
            rc == 1 ==> final(self).made(*old(self), w),
            rc == 0 ==> final(self).made_immortal(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == s.spec_bytes(),
    {
        let t = Tail::Str(s);
        let bytes = self.core.tail_len(&t);
        let w = self.make_text(bytes, t, None, rc);
        proof {
            assert(s.spec_bytes() + Seq::<u8>::empty() =~= s.spec_bytes());
        }
        w
    }

    /// 容量が `room` バイト以上の文字列を rc `rc` の生きている物体として作り、`head` と `tail` のバイト列を順に写す。
    /// rc 0 なら不死の物体で、検査モードの外に出ている参照の数は 0 から始まる (`alloc_header` が書く)。
    fn make_text(&mut self, room: usize, head: Tail, tail: Option<Tail>, rc: i32) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            rc == 0 || rc == 1,
            old(self).core.tail_ok(head),
            old(self).core.opt_ok(tail),
            old(self).core.tail_bytes(head).len() + old(self).core.opt_bytes(tail).len() <= room,
        ensures
            rc == 1 ==> final(self).made(*old(self), w),
            rc == 0 ==> final(self).made_immortal(*old(self), w),
            final(self).same_tokens(*old(self)),
            final(self).is_string(w),
            final(self).text_of(w) == old(self).core.tail_bytes(head) + old(self).core.opt_bytes(
                tail,
            ),
            forall|b: usize| #[trigger]
                old(self).core.dom().contains(b) ==> {
                    &&& final(self).core.dom().contains(b)
                    &&& final(self).core.perms@[b] == old(self).core.perms@[b]
                    &&& final(self).core.state(b) == old(self).core.settled_state(b)
                    &&& final(self).core.block(b) == old(self).core.block(b)
                    &&& final(self).core.cap(b) == old(self).core.cap(b)
                },
            forall|x: Word| old(self).core.word_ok(x) ==> #[trigger] final(self).core.word_ok(x),
    {
        // 長さの1語と、バイト列を語に切り上げた数。`room / 8` は usize の 1/8 以下なので、足しても桁あふれしない
        let payload_words = 1 + room / 8 + if room.is_multiple_of(8) {
            0
        } else {
            1
        };
        self.settle();
        let ghost s1 = *self;
        let meta = Meta::new(0, 0, Kind::String, false);
        let (h, Tracked(perm)) = self.alloc_header(payload_words, rc, meta);
        let tracked mut perm = perm;
        proof {
            self.core.pages.lemma_block_bounds(h@.addr);
        }
        let cap = self.core.pages.cap_of(h);
        proof {
            assert(8 * (cap - 1) >= room) by (nonlinear_arith)
                requires
                    cap >= payload_words,
                    payload_words == 1 + room / 8 + if room % 8 == 0 {
                        0int
                    } else {
                        1int
                    },
            ;
            assert(Seq::<u8>::empty() + s1.core.tail_bytes(head) =~= s1.core.tail_bytes(head));
        }
        self.core.write_tail(h, cap, 0, &head, Tracked(&mut perm));
        let mut len = self.core.tail_len(&head);
        if let Some(t) = &tail {
            self.core.write_tail(h, cap, len, t, Tracked(&mut perm));
            len += self.core.tail_len(t);
        }
        let tracked mut slot = perm.slots.tracked_remove(0);
        ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(len));
        proof {
            perm.slots.tracked_insert(0, slot);
        }
        let ghost mid = self.core;
        proof {
            check::lemma_alive_code();
            s1.core.lemma_tail_utf8(head);
            if let Some(t) = tail {
                s1.core.lemma_tail_utf8(t);
                valid_utf8_concat(s1.core.tail_bytes(head), s1.core.tail_bytes(t));
            } else {
                assert(s1.core.tail_bytes(head) + Seq::<u8>::empty() =~= s1.core.tail_bytes(head));
            }
            assert(perm.text() =~= s1.core.tail_bytes(head) + s1.core.opt_bytes(tail));
            let c = self.core;
            let a = h@.addr;
            assert(string_wf(perm, c.cap(a)));
            assert(c.perm_wf(perm, a, BlockState::Live, Set::empty(), self.worker));
        }
        self.core.publish(Ghost(h), Tracked(perm), Ghost(BlockState::Live), Ghost(self.worker));
        let w = Word::from_ptr(h);
        proof {
            if old(self).consistent() && old(self).settleable() {
                self.lemma_dummy_kept(s1);
                self.core.lemma_frame_chain(mid, s1.core, old(self).core);
            } else {
                self.lemma_published_safe(s1);
            }
            crate::word::lemma_aligned_is_ptr(h@.addr);
        }
        w
    }

    /// # Safety
    /// `w` はこのヒープの生きている文字列の物体を指す。返す位置を読めるのは、物体が生きていて、`string_push` か
    /// `string_concat` でその物体を書き換えていない間だけである。
    pub unsafe fn string(&self, w: Word) -> (r: Result<*const str, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && self.is_string(w),
        ensures
            self.consistent() && self.settleable() && self.readable(w) && self.is_string(w)
                ==> r is Ok,
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        if M::CHECKED && !matches!(self.core.meta_get(h).kind(), Kind::String) {
            return Err(HeapError::WrongKind);
        }
        proof {
            if M::CHECKED {
                self.core.lemma_safe_at(h@.addr);
            } else {
                self.core.lemma_live_block(h@.addr);
            }
        }
        let len = self.core.slot_get(h, 0).bits();
        proof {
            self.core.lemma_shape(h@.addr);
            let cap = self.core.cap(h@.addr);
            assert((len + 7) / 8 <= cap - 1) by (nonlinear_arith)
                requires
                    len <= 8 * (cap - 1),
                    cap >= 1,
            ;
        }
        let tracked p = self.core.perms.borrow().tracked_borrow(h@.addr);
        Ok(text_ptr(h, len, Tracked(p)))
    }

    /// `string_push` の前提がみな成り立ち、モデルが合っている。`decref` の前提に、left が生きている文字列であることを
    /// 足す。
    pub closed spec fn push_ok(&self, left: Word) -> bool {
        &&& self.settleable()
        &&& self.decref_ok(left)
        &&& self.readable(left)
        &&& self.is_string(left)
    }

    /// `string_concat` の前提がみな成り立ち、モデルが合っている。両辺が `decref` の前提を満たす文字列で、同じ物体なら
    /// 呼ぶ側がその数えた参照を2つ持つ。
    pub closed spec fn concat_ok(&self, left: Word, right: Word) -> bool {
        &&& self.push_ok(left)
        &&& self.readable(right)
        &&& self.is_string(right)
        &&& self.decref_ok(right)
        &&& left.bits_spec() == right.bits_spec() && self.rc_value(left) != 0
            ==> self.core.settled_view().ext(left.bits_spec()) >= 2
    }

    /// `string_push` の後の状態。left の参照を1つ手放し、`w` の参照を1つ作った。`w == left` ならその場で追記したので、
    /// left の長さの語とバイト列のほかは変わらない。そうでなければ `w` は新しい物体で、left は `decref` の後と同じである。
    pub closed spec fn pushed(&self, old: Heap<M>, left: Word, w: Word) -> bool {
        let v = old.core.settled_view();
        let h = w.bits_spec();
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.rc_of(h) == 1
        &&& self.core.ext(h) == 1
        &&& if w == left {
            self.core.text_changed(v, h)
        } else {
            &&& !old.core.dom().contains(h)
            &&& self.core.evolved(v, left.bits_spec(), -1)
        }
    }

    /// `string_concat` の後の状態。`string_push` の後の状態 `mid` から、right の参照を1つ手放した。
    pub closed spec fn concatenated(&self, old: Heap<M>, left: Word, right: Word, w: Word) -> bool {
        let h = w.bits_spec();
        &&& exists|mid: Heap<M>| #[trigger] mid.pushed(old, left, w) && self.evolved(mid, right, -1)
        &&& self.core.word_ok(w)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.rc_of(h) == 1
        &&& self.core.ext(h) == 1
    }

    /// 文字列を受けて手放す操作の検査。状態を変える前に呼ぶ。速いモードでは何も確かめない。
    fn checked_string(&self, h: *mut Header) -> (r: Result<(), HeapError>)
        requires
            self.core.sound(),
            self.core.checked@ == M::CHECKED,
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
        ensures
            !M::CHECKED ==> r is Ok,
            M::CHECKED ==> (r is Ok <==> self.core.kind(h@.addr) == Kind::String
                && self.decref_passes(h@.addr)),
            r is Err ==> chain_err(r) || r == Err::<(), HeapError>(HeapError::UseAfterFree) && !(
            self.core.state(h@.addr) == BlockState::Live || self.core.state(h@.addr)
                == BlockState::Building) || r == Err::<(), HeapError>(HeapError::WrongKind)
                && self.core.kind(h@.addr) != Kind::String,
    {
        if M::CHECKED {
            proof {
                self.core.lemma_code_state(h@.addr);
            }
        }
        self.checked_live(h)?;
        if M::CHECKED && !matches!(self.core.meta_get(h).kind(), Kind::String) {
            return Err(HeapError::WrongKind);
        }
        self.checked_decref(h)?;
        Ok(())
    }

    /// 文字列 `h` の参照を1つ手放す。文字列は子を持たないので、最後の参照なら解放の連鎖を通さずに箱だけを空ける。
    /// 検査モードの検査は、呼ぶ側が状態を変える前に済ませてある。
    fn release_text(&mut self, h: *mut Header, Ghost(w): Ghost<Word>, Ghost(c): Ghost<bool>)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            w.is_ptr_spec() && w.bits_spec() == h@.addr,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.kind(h@.addr) == Kind::String,
            !M::CHECKED ==> c,
            M::CHECKED ==> old(self).decref_passes(h@.addr),
            c ==> old(self).consistent() && old(self).core.state(h@.addr) == BlockState::Live && (
            old(self).core.rc_of(h@.addr) != 0 ==> old(self).core.ext(h@.addr) >= 1),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).core.model@.building is None,
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
            c ==> final(self).consistent() && final(self).evolved(*old(self), w, -1),
            forall|b: usize| #[trigger]
                old(self).core.dom().contains(b) && b != h@.addr ==> {
                    &&& final(self).core.dom().contains(b)
                    &&& final(self).core.perms@[b] == old(self).core.perms@[b]
                    &&& final(self).core.state(b) == old(self).core.state(b)
                    &&& final(self).core.block(b) == old(self).core.block(b)
                    &&& final(self).core.cap(b) == old(self).core.cap(b)
                },
            forall|x: Word|
                old(self).core.word_ok(x) && (x.is_ptr_spec() ==> x.bits_spec() != h@.addr)
                    ==> #[trigger] final(self).core.word_ok(x),
            M::CHECKED ==> final(self).core.dom() == old(self).core.dom() && final(self).core.pages
                == old(self).core.pages,
            M::CHECKED && old(self).decref_twice_passes(h@.addr) ==> final(self).decref_passes(
                h@.addr,
            ) && final(self).core.kind(h@.addr) == Kind::String,
    {
        let ghost s0 = self.core;
        proof {
            s0.lemma_frame_refl();
        }
        let last = match self.release_ref(h, Ghost(None), Ghost(c)) {
            Ok(last) => last,
            Err(_) => {
                // 速いモードは誤りを返さず、検査モードの検査は呼ぶ側が済ませてある
                proof {
                    assert(false);
                }
                return;
            },
        };
        let ghost s1 = self.core;
        if last {
            proof {
                if c {
                    s1.lemma_at(h@.addr);
                    reveal(Core::edges_wf);
                    assert forall|j: int|
                        !(#[trigger] s1.model@.edges.dom().contains((h@.addr, j))) by {
                        if s1.model@.edges.dom().contains((h@.addr, j)) {
                            assert(s1.edge_wf((h@.addr, j)));
                        }
                    }
                }
            }
            self.free_box(h, Ghost(c));
            proof {
                if c {
                    self.core.lemma_boxed_evolved(s1, h@.addr);
                    self.lemma_after_chain(*old(self), s0, s1, h@.addr, w);
                }
                if M::CHECKED {
                    self.lemma_safe_frame(*old(self));
                }
            }
        } else {
            proof {
                if c {
                    self.lemma_after_step(*old(self), s0, h@.addr, w, -1);
                }
                if M::CHECKED {
                    self.lemma_safe_frame(*old(self));
                }
            }
        }
    }

    /// `string_push` と `string_concat` の本体。検査はみな済んでいる。left の参照を1つ受け取り、`t` を後ろに足した
    /// 文字列の参照を返す。left が一意で容量が足りればその場で書き、足りなければ容量を倍にして作り直す。一意でなければ
    /// 和の長さで写す。どちらの写しも left を手放す前に済ませる。速いモードでは、大きい物体のページを箱の解放がすぐに
    /// 返すためである。
    // 速いモードは rc 0 の物体のページの頭を読まないので、容量は一意な left でだけ読む (docs/spec/object-model.md の「文字列」)。
    fn push_unchecked(&mut self, left: Word, t: Tail, Ghost(c): Ghost<bool>) -> (w: Word)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            left.is_ptr_spec(),
            old(self).core.word_ok(left),
            old(self).core.state(left.bits_spec()) == BlockState::Live,
            old(self).core.kind(left.bits_spec()) == Kind::String,
            old(self).core.tail_ok(t),
            t matches Tail::Text(r, _) && r@.addr == left.bits_spec() ==> old(self).core.rc_of(
                left.bits_spec(),
            ) != 1,
            !M::CHECKED ==> c,
            M::CHECKED ==> old(self).decref_passes(left.bits_spec()),
            c ==> old(self).consistent() && old(self).core.holds(left),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).core.model@.building is None,
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
            w.is_ptr_spec(),
            final(self).core.word_ok(w),
            final(self).is_string(w),
            final(self).text_of(w) == old(self).text_of(left) + old(self).core.tail_bytes(t),
            w != left ==> !old(self).core.dom().contains(w.bits_spec()),
            w == left ==> old(self).core.rc_of(left.bits_spec()) == 1,
            c ==> final(self).consistent() && final(self).pushed(*old(self), left, w),
            forall|b: usize| #[trigger]
                old(self).core.dom().contains(b) && b != left.bits_spec() ==> {
                    &&& final(self).core.dom().contains(b)
                    &&& final(self).core.perms@[b] == old(self).core.perms@[b]
                    &&& final(self).core.state(b) == old(self).core.state(b)
                    &&& final(self).core.block(b) == old(self).core.block(b)
                },
            M::CHECKED && old(self).decref_twice_passes(left.bits_spec()) ==> {
                &&& final(self).core.dom().contains(left.bits_spec())
                &&& final(self).core.block(left.bits_spec()) == old(self).core.block(
                    left.bits_spec(),
                )
                &&& final(self).core.kind(left.bits_spec()) == Kind::String
                &&& final(self).decref_passes(left.bits_spec())
            },
    {
        let ghost s0 = *self;
        let h = self.header_of(left);
        proof {
            s0.core.lemma_shape(h@.addr);
        }
        let len = self.core.slot_get(h, 0).bits();
        let need = match len.checked_add(self.core.tail_len(&t)) {
            Some(n) => n,
            None => too_large(),
        };
        let unique = self.core.rc_get(h) == 1;
        let cap = if unique {
            self.core.cap_get(h)
        } else {
            0
        };
        let room = if unique {
            8 * (cap - 1)
        } else {
            0
        };
        if unique && need <= room {
            let tracked mut perm = self.core.perms.borrow_mut().tracked_remove(h@.addr);
            proof {
                assert((8 * (cap - 1) + 7) / 8 == cap - 1);
            }
            self.core.write_tail(h, cap, len, &t, Tracked(&mut perm));
            let tracked mut slot = perm.slots.tracked_remove(0);
            ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(need));
            proof {
                perm.slots.tracked_insert(0, slot);
                self.core.perms.borrow_mut().tracked_insert(h@.addr, perm);
                let a = h@.addr;
                let q = s0.core.perm(a);
                s0.core.lemma_tail_utf8(t);
                valid_utf8_concat(q.text(), s0.core.tail_bytes(t));
                assert(self.core.perms@.dom() =~= s0.core.perms@.dom());
                assert(self.core.perm(a).slots.dom() =~= q.slots.dom());
                assert(self.core.text_changed(s0.core, a));
                if c {
                    self.core.lemma_text_set(s0.core, a);
                    reveal(Core::refs_wf);
                    assert(s0.core.counted_wf(a));
                    assert(self.core.perms@[s0.dummy.bits_spec()]
                        == s0.core.perms@[s0.dummy.bits_spec()]);
                }
                if M::CHECKED {
                    self.core.lemma_safe_text_set(s0.core, a);
                }
            }
            return left;
        }
        let want = if unique {
            let doubled = match room.checked_mul(2) {
                Some(d) => d,
                None => too_large(),
            };
            if need > doubled {
                need
            } else {
                doubled
            }
        } else {
            need
        };
        proof {
            let cap = s0.core.cap(h@.addr);
            assert((len + 7) / 8 <= cap - 1) by (nonlinear_arith)
                requires
                    len <= 8 * (cap - 1),
                    cap >= 1,
            ;
        }
        let w = self.make_text(want, Tail::Text(h, len), Some(t), 1);
        let ghost mid = *self;
        self.release_text(h, Ghost(left), Ghost(c));
        proof {
            let a = h@.addr;
            let b = w.bits_spec();
            assert(mid.core.perms@[b] == self.core.perms@[b]);
            if c {
                self.core.lemma_frame_evolved(mid.core, s0.core, a, -1);
            }
        }
        w
    }

    /// left の後ろに s を足した文字列を返す。left の参照を1つ受け取り、結果の参照を1つ返す。どの誤りも最初の確保より
    /// 前に返すので、`Err` のときヒープは変わらない (docs/spec/object-model.md の「文字列」)。
    ///
    /// # Safety
    /// `left` はこのヒープの生きている文字列の物体を指し、呼ぶ側がその参照を1つ持つ (`decref` と同じ前提)。`s` は left
    /// 自身の中身を指さない。一意な left を作り直すとこの呼び出しの中で left の箱を空け、大きい物体ならページも返すが、
    /// 引数の `&str` が指すメモリを呼び出しの間に返すと未定義の動作になるためである。自分自身との連結は `dup` と
    /// `string_concat(x, x)` で行う。`s` がほかの文字列の中身を指すときは、その物体はこの呼び出しの間生きている。
    pub unsafe fn string_push(&mut self, left: Word, s: &str) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            left.is_ptr_spec(),
            old(self).word_in(left),
            !M::CHECKED ==> old(self).push_ok(left),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> final(self).is_string(w) && final(self).text_of(w) == old(
                self,
            ).text_of(left) + s.spec_bytes(),
            old(self).push_ok(left) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).push_ok(left) ==> final(self).consistent()
                && final(self).pushed(*old(self), left, w)),
    {
        let ghost c = self.push_ok(left);
        self.settle();
        let h = self.header_of(left);
        proof {
            self.lemma_checks_pass(left);
            reveal(Core::building_wf);
            if c {
                self.core.lemma_readable(left);
            }
        }
        self.checked_string(h)?;
        let ghost s1 = *self;
        let w = self.push_unchecked(left, Tail::Str(s), Ghost(c));
        proof {
            assert(old(self).core.settled_view() == s1.core);
        }
        Ok(w)
    }

    /// `++`。両辺の参照を受け取り、結果の参照を返す。`string_push(left, string(right))` の後に right を手放す。どの誤りも
    /// 最初の確保より前に返す (docs/spec/object-model.md の「文字列」)。
    ///
    /// # Safety
    /// `left` と `right` はこのヒープの生きている文字列の物体を指し、呼ぶ側がそれぞれの参照を1つずつ持つ (同じ物体なら
    /// 2つ持つ)。
    pub unsafe fn string_concat(&mut self, left: Word, right: Word) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            left.is_ptr_spec(),
            right.is_ptr_spec(),
            old(self).word_in(left),
            old(self).word_in(right),
            !M::CHECKED ==> old(self).concat_ok(left, right),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> final(self).is_string(w) && final(self).text_of(w) == old(
                self,
            ).text_of(left) + old(self).text_of(right),
            old(self).concat_ok(left, right) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).concat_ok(left, right) ==> final(self).consistent()
                && final(self).concatenated(*old(self), left, right, w)),
    {
        let ghost c = self.concat_ok(left, right);
        self.settle();
        let hl = self.header_of(left);
        let hr = self.header_of(right);
        proof {
            self.lemma_checks_pass(left);
            self.lemma_checks_pass(right);
            reveal(Core::building_wf);
            if c {
                self.core.lemma_readable(left);
                self.core.lemma_readable(right);
            }
        }
        self.checked_string(hl)?;
        self.checked_string(hr)?;
        let same = left.bits() == right.bits();
        proof {
            if c && same {
                reveal(Core::refs_wf);
                self.core.lemma_at(hl@.addr);
                assert(self.core.counted_wf(hl@.addr));
            }
        }
        if same {
            self.checked_decref_twice(hl)?;
        }
        proof {
            self.core.lemma_shape(hr@.addr);
        }
        let len = self.core.slot_get(hr, 0).bits();
        proof {
            let cap = self.core.cap(hr@.addr);
            assert((len + 7) / 8 <= cap - 1) by (nonlinear_arith)
                requires
                    len <= 8 * (cap - 1),
                    cap >= 1,
            ;
        }
        let ghost s1 = *self;
        let w = self.push_unchecked(left, Tail::Text(hr, len), Ghost(c));
        let ghost mid = *self;
        proof {
            assert(old(self).core.settled_view() == s1.core);
        }
        self.release_text(hr, Ghost(right), Ghost(c));
        proof {
            let b = w.bits_spec();
            assert(mid.core.perms@[b] == self.core.perms@[b]);
            if c {
                assert(mid.pushed(*old(self), left, w));
            }
        }
        Ok(w)
    }
}

} // verus!

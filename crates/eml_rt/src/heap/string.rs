//! 文字列とバイト列の物体 (docs/spec/object-model.md の「バイト列と文字列」)。種類 `String` と `Bytes` の物体は、ここの
//! 操作だけが作り、書く。長さの語は生の 0 語目で、バイト列はその後ろにあり、ghost のバイト列 `text` に等しい。バイト列を
//! 書くのは、作るときと後ろに足すとき (`string_push` などと、`String.show` の `append_str` と `append_part`) だけである。

use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;
#[cfg(verus_only)]
use vstd::utf8::{
    encode_utf8_valid_utf8, is_char_boundary, is_char_boundary_iff_not_is_continuation_byte,
    is_char_boundary_start_end_of_seq, valid_utf8, valid_utf8_concat, valid_utf8_split,
};

use super::*;
#[cfg(verus_only)]
use crate::heap::inv::body_wf;
#[cfg(verus_only)]
use crate::meta::{field_ptr, slot_offset};
use crate::text::{bytes, bytes_ptr, text, text_ptr, write_byte, write_bytes, write_text};

verus! {

/// 文字列かバイト列の後ろに写すバイト列。`Str` は呼ぶ側の `&str`、`Bytes` は呼ぶ側の `&[u8]`、`Text(h, len, from, to)`
/// はこのヒープか付けた塊の文字列の物体 (ヘッダと長さ) のバイトの範囲 `[from, to)`、`Body(h, len, from, to)` は文字列
/// かバイト列の物体の同じ範囲である。`Body` は UTF-8 の境目を求めないので、バイト列にだけ写す。`string_concat` の右辺は
/// ヒープの権限から借りて読むので、`&mut self` を取る操作に `&str` として渡せない。そのため、右辺の権限は写す直前に
/// `Core::write_tail` の中で借りる。
enum Tail<'a> {
    Str(&'a str),
    Bytes(&'a [u8]),
    Text(*mut Header, usize, usize, usize),
    Body(*mut Header, usize, usize, usize),
}

/// `a` は文字列かバイト列の種類 `b` に等しい。
#[inline]
pub(super) fn same_body_kind(a: Kind, b: Kind) -> (r: bool)
    requires
        b.has_body(),
    ensures
        r == (a == b),
{
    matches!((a, b), (Kind::String, Kind::String) | (Kind::Bytes, Kind::Bytes))
}

/// 種類 `kind` の物体に `t` を写せる。文字列には、UTF-8 を約束しない `Bytes` と `Body` を写さない。
spec fn fits(kind: Kind, t: Tail) -> bool {
    kind == Kind::String ==> !(t is Bytes) && !(t is Body)
}

/// 妥当な UTF-8 の `to` より前の部分でも、`to` より前の文字の境目 `from` は文字の境目である。
proof fn lemma_boundary_in_prefix(bytes: Seq<u8>, from: int, to: int)
    requires
        valid_utf8(bytes),
        0 <= from <= to <= bytes.len(),
        is_char_boundary(bytes, from),
        is_char_boundary(bytes, to),
    ensures
        valid_utf8(bytes.subrange(0, to)),
        is_char_boundary(bytes.subrange(0, to), from),
        valid_utf8(bytes.subrange(from, to)),
{
    valid_utf8_split(bytes, to);
    let head = bytes.subrange(0, to);
    assert(head =~= bytes[..to]);
    if from == to {
        is_char_boundary_start_end_of_seq(head);
    } else {
        is_char_boundary_iff_not_is_continuation_byte(bytes, from);
        is_char_boundary_iff_not_is_continuation_byte(head, from);
    }
    valid_utf8_split(head, from);
    assert(head[from..] =~= bytes.subrange(from, to));
}

impl Core {
    spec fn tail_bytes(&self, t: Tail) -> Seq<u8> {
        match t {
            Tail::Str(s) => s.spec_bytes(),
            Tail::Bytes(b) => b@,
            Tail::Text(r, _, from, to) => self.perm(r@.addr).text().subrange(
                from as int,
                to as int,
            ),
            Tail::Body(r, _, from, to) => self.perm(r@.addr).text().subrange(
                from as int,
                to as int,
            ),
        }
    }

    /// `t` を読める。`Text` なら、このヒープか付けた塊の文字列のブロックで、長さはバイト列の長さに等しく、範囲の両端は
    /// 文字の境目である。
    spec fn tail_ok(&self, t: Tail) -> bool {
        match t {
            Tail::Str(_) => true,
            Tail::Bytes(_) => true,
            Tail::Text(r, n, from, to) => {
                &&& self.text_ok(r, n)
                &&& from <= to <= n
                &&& is_char_boundary(self.perm(r@.addr).text(), from as int)
                &&& is_char_boundary(self.perm(r@.addr).text(), to as int)
            },
            Tail::Body(r, n, from, to) => {
                &&& self.body_ok(r, n)
                &&& from <= to <= n
            },
        }
    }

    /// `r` はこのヒープか付けた塊の文字列かバイト列のブロックで、長さ `n` はバイト列の長さに等しい。
    spec fn body_ok(&self, r: *mut Header, n: usize) -> bool {
        &&& self.has(r@.addr)
        &&& self.region_wf()
        &&& self.perm(r@.addr).holds_body(r, n as int)
        &&& n == self.perm(r@.addr).text().len()
    }

    /// `body_ok` で、バイト列は妥当な UTF-8 である。
    spec fn text_ok(&self, r: *mut Header, n: usize) -> bool {
        &&& self.body_ok(r, n)
        &&& valid_utf8(self.perm(r@.addr).text())
    }

    /// 文字列の全体を写す `Text`。
    proof fn lemma_whole(&self, r: *mut Header, n: usize)
        requires
            self.text_ok(r, n),
        ensures
            self.tail_ok(Tail::Text(r, n, 0, n)),
            self.tail_bytes(Tail::Text(r, n, 0, n)) == self.perm(r@.addr).text(),
    {
        let t = self.perm(r@.addr).text();
        is_char_boundary_start_end_of_seq(t);
        assert(t.subrange(0, n as int) =~= t);
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

    /// `Bytes` と `Body` のほかの `Tail` は妥当な UTF-8 である。
    proof fn lemma_tail_utf8(&self, t: Tail)
        requires
            self.tail_ok(t),
            fits(Kind::String, t),
        ensures
            valid_utf8(self.tail_bytes(t)),
    {
        match t {
            Tail::Str(s) => encode_utf8_valid_utf8(s@),
            Tail::Bytes(_) => {},
            Tail::Text(r, _, from, to) => {
                lemma_boundary_in_prefix(self.perm(r@.addr).text(), from as int, to as int)
            },
            Tail::Body(_, _, _, _) => {},
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
            Tail::Bytes(b) => b.len(),
            Tail::Text(_, _, from, to) => *to - *from,
            Tail::Body(_, _, from, to) => *to - *from,
        }
    }

    /// `h` のペイロードの `at` バイト目から `t` を写す。`write_text` と同じ約束で、`Text` ならその物体の権限をここで借りる。
    // 範囲を切り出す `Text` の道を足すと、Rust がほかのクレートに自動で展開する大きさを超え、計測の `append` の命令の
    // 数が2.2倍になった。`string_push` の追記の道に展開させるために付ける
    #[inline]
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
            old(perm).holds_body(h, 8 * (cap - 1)),
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
            Tail::Bytes(b) => write_bytes(h, cap, at, b, Tracked(perm)),
            Tail::Text(r, n, from, to) => {
                write_text(h, cap, at, self.text_part(*r, *n, *from, *to), Tracked(perm))
            },
            Tail::Body(r, n, from, to) => {
                let tracked p = self.tracked_perm(r@.addr);
                let b = bytes(*r, *n, Tracked(p));
                write_bytes(h, cap, at, vstd::slice::slice_subrange(b, *from, *to), Tracked(perm))
            },
        }
    }

    /// 文字列 `r` のバイトの範囲 `[from, to)` を、その物体の権限から借りる。範囲は vstd の `str::split_at` で切り出すので、
    /// T10 の関数は文字列の全体を借りるだけでよい。
    fn text_part(&self, r: *mut Header, n: usize, from: usize, to: usize) -> (s: &str)
        requires
            self.tail_ok(Tail::Text(r, n, from, to)),
        ensures
            s.spec_bytes() == self.tail_bytes(Tail::Text(r, n, from, to)),
    {
        let tracked p = self.tracked_perm(r@.addr);
        proof {
            lemma_boundary_in_prefix(p.text(), from as int, to as int);
        }
        let (head, _) = text(r, n, Tracked(p)).split_at(to);
        proof {
            assert(head.spec_bytes() =~= p.text().subrange(0, to as int));
        }
        let (_, part) = head.split_at(from);
        proof {
            assert(part.spec_bytes() =~= p.text().subrange(from as int, to as int));
        }
        part
    }

    /// `old` から、文字列 `h` の長さの語とバイト列だけが変わった (`string_push` のその場の追記)。
    pub(crate) open spec fn text_changed(&self, old: Core, h: usize) -> bool {
        &&& self.pages == old.pages
        &&& self.env == old.env
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
        &&& body_wf(self.perm(h), self.cap(h))
    }

    /// `old` と `self` の両方で生きているブロックは、`meta`、rc の符号、持ち主を変えず、`h` のほかは語とバイト列も
    /// 変えない。文字列に足す操作は新しい物体を作り、`h` を書き換えるので、`kept_live` の代わりにこれを約束する。
    pub(crate) open spec fn kept_both(&self, old: Core, h: usize) -> bool {
        forall|b: usize|
            #![trigger self.settled_state(b), old.settled_state(b)]
            self.has(b) && self.settled_state(b) == BlockState::Live && old.has(b)
                && old.settled_state(b) == BlockState::Live ==> {
                &&& self.perm(b).meta == old.perm(b).meta
                &&& (self.rc_of(b) > 0) == (old.rc_of(b) > 0)
                &&& (self.rc_of(b) < 0) == (old.rc_of(b) < 0)
                &&& self.rc_of(b) > 0 ==> self.model@.owner[b] == old.model@.owner[b]
                &&& b != h ==> self.perm(b).slots == old.perm(b).slots && self.perm(b).text
                    == old.perm(b).text
            }
    }

    /// その場の追記の後も、不変条件は保たれる。文字列は走査する語を持たないので、辺と数は変わらない。
    proof fn lemma_text_set(&self, s0: Core, h: usize)
        requires
            s0.full(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            s0.kind(h).has_body(),
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
            s0.kind(h).has_body(),
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
    pub open spec fn is_string(&self, w: Word) -> bool {
        self.kind_of(w) == Kind::String
    }

    pub open spec fn is_bytes(&self, w: Word) -> bool {
        self.kind_of(w) == Kind::Bytes
    }

    /// 文字列かバイト列のバイト列。
    pub closed spec fn text_of(&self, w: Word) -> Seq<u8> {
        self.core.perm(w.bits_spec()).text()
    }

    /// 文字列かバイト列 `w` に書けるバイト数。
    pub closed spec fn capacity(&self, w: Word) -> int {
        8 * (self.core.cap(w.bits_spec()) - 1)
    }

    /// 文字列の spec の中身。`Ctx` の文字列の extern の証明が、物体の権限と結び付けるのに使う。
    pub(super) proof fn lemma_text_spec(&self, w: Word)
        ensures
            self.kind_of(w) == self.core.kind(w.bits_spec()),
            self.is_string(w) == (self.core.kind(w.bits_spec()) == Kind::String),
            self.is_bytes(w) == (self.core.kind(w.bits_spec()) == Kind::Bytes),
            self.text_of(w) == self.core.perm(w.bits_spec()).text(),
            self.capacity(w) == 8 * (self.core.cap(w.bits_spec()) - 1),
    {
    }

    /// `alloc_string_immortal` か `alloc_bytes_immortal` の後の状態。`w` は rc 0 の生きている物体で、検査モードの外に
    /// 出ている参照の数は 0 である。
    pub closed spec fn made_immortal(&self, old: Heap<M>, w: Word) -> bool {
        let h = w.bits_spec();
        &&& self.wf()
        &&& self.dummy == old.dummy
        &&& self.region == old.region
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
            final(self).owned(w),
            !final(self).lin_of(w),
            final(self).grew(*old(self), w),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        self.make_string(s, 1)
    }

    /// 容量が `room` バイト以上の空の文字列を作る。結果の長さを先に数えられる extern (`String.show`) が、追記の
    /// たびの作り直しを避けるために使う。
    pub fn alloc_string_room(&mut self, room: usize) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == Seq::<u8>::empty(),
            final(self).owned(w),
            !final(self).lin_of(w),
            final(self).capacity(w) >= room,
            final(self).grew(*old(self), w),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let empty = "";
        proof {
            reveal_strlit("");
            assert(empty.spec_bytes() =~= Seq::<u8>::empty());
        }
        let w = self.make_text(room, Tail::Str(empty), None, 1, Kind::String);
        proof {
            assert(Seq::<u8>::empty() + Seq::<u8>::empty() =~= Seq::<u8>::empty());
        }
        w
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
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        self.make_string(s, 0)
    }

    /// `b` を写したバイト列を作る。
    pub fn alloc_bytes(&mut self, b: &[u8]) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).is_bytes(w),
            final(self).text_of(w) == b@,
            final(self).owned(w),
            !final(self).lin_of(w),
            final(self).grew(*old(self), w),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        self.make_bytes(b, 1)
    }

    /// 容量が `room` バイト以上の空のバイト列を作る。`alloc_string_room` のバイト列の版である。
    pub fn alloc_bytes_room(&mut self, room: usize) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).is_bytes(w),
            final(self).text_of(w) == Seq::<u8>::empty(),
            final(self).owned(w),
            !final(self).lin_of(w),
            final(self).capacity(w) >= room,
            final(self).grew(*old(self), w),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let empty: &[u8] = &[];
        proof {
            assert(empty@ =~= Seq::<u8>::empty());
        }
        let w = self.make_text(room, Tail::Bytes(empty), None, 1, Kind::Bytes);
        proof {
            assert(Seq::<u8>::empty() + Seq::<u8>::empty() =~= Seq::<u8>::empty());
        }
        w
    }

    /// `alloc_bytes` と同じで、rc 0 の不死の物体にする。
    pub fn alloc_bytes_immortal(&mut self, b: &[u8]) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made_immortal(*old(self), w),
            final(self).is_bytes(w),
            final(self).text_of(w) == b@,
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        self.make_bytes(b, 0)
    }

    /// `b` を写したバイト列を、rc `rc` の生きている物体として作る。
    fn make_bytes(&mut self, b: &[u8], rc: i32) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            rc == 0 || rc == 1,
        ensures
            rc == 1 ==> final(self).made(*old(self), w),
            rc == 0 ==> final(self).made_immortal(*old(self), w),
            final(self).is_bytes(w),
            final(self).text_of(w) == b@,
            final(self).core.model@.owner[w.bits_spec()] == final(self).worker,
            !final(self).core.lin(w.bits_spec()),
            forall|x: Word| old(self).core.word_ok(x) ==> #[trigger] final(self).core.word_ok(x),
            final(self).grew(*old(self), w),
    {
        let w = self.make_text(b.len(), Tail::Bytes(b), None, rc, Kind::Bytes);
        proof {
            assert(b@ + Seq::<u8>::empty() =~= b@);
        }
        w
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
            final(self).core.model@.owner[w.bits_spec()] == final(self).worker,
            !final(self).core.lin(w.bits_spec()),
            forall|x: Word| old(self).core.word_ok(x) ==> #[trigger] final(self).core.word_ok(x),
            final(self).grew(*old(self), w),
    {
        let t = Tail::Str(s);
        let bytes = self.core.tail_len(&t);
        let w = self.make_text(bytes, t, None, rc, Kind::String);
        proof {
            assert(s.spec_bytes() + Seq::<u8>::empty() =~= s.spec_bytes());
        }
        w
    }

    /// 容量が `room` バイト以上の種類 `kind` (`String` か `Bytes`) の物体を rc `rc` の生きている物体として作り、`head` と
    /// `tail` のバイト列を順に写す。rc 0 なら不死の物体で、検査モードの外に出ている参照の数は 0 から始まる
    /// (`alloc_header` が書く)。文字列には `Tail::Bytes` と `Tail::Body` を写さないので、中身は妥当な UTF-8 になる。
    fn make_text(&mut self, room: usize, head: Tail, tail: Option<Tail>, rc: i32, kind: Kind) -> (w:
        Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            rc == 0 || rc == 1,
            kind.has_body(),
            fits(kind, head),
            tail matches Some(t) ==> fits(kind, t),
            old(self).core.tail_ok(head),
            old(self).core.opt_ok(tail),
            old(self).core.tail_bytes(head).len() + old(self).core.opt_bytes(tail).len() <= room,
        ensures
            rc == 1 ==> final(self).made(*old(self), w),
            rc == 0 ==> final(self).made_immortal(*old(self), w),
            final(self).same_tokens(*old(self)),
            final(self).core.env == old(self).core.env,
            final(self).core.kind(w.bits_spec()) == kind,
            final(self).text_of(w) == old(self).core.tail_bytes(head) + old(self).core.opt_bytes(
                tail,
            ),
            final(self).core.model@.owner[w.bits_spec()] == final(self).worker,
            !final(self).core.lin(w.bits_spec()),
            final(self).capacity(w) >= room,
            final(self).grew(*old(self), w),
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
        let meta = Meta::new(0, 0, kind, false);
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
            if tail is None {
                assert(s1.core.tail_bytes(head) + Seq::<u8>::empty() =~= s1.core.tail_bytes(head));
            }
            if kind == Kind::String {
                s1.core.lemma_tail_utf8(head);
                if let Some(t) = tail {
                    s1.core.lemma_tail_utf8(t);
                    valid_utf8_concat(s1.core.tail_bytes(head), s1.core.tail_bytes(t));
                }
            }
            assert(perm.text() =~= s1.core.tail_bytes(head) + s1.core.opt_bytes(tail));
            let c = self.core;
            let a = h@.addr;
            assert(body_wf(perm, c.cap(a)));
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
            assert forall|b: usize|
                #![trigger old(self).core.has(b)]
                old(self).core.has(b) && old(self).core.settled_state(b)
                    == BlockState::Live implies self.grown_block(*old(self), b) by {
                assert(s1.core.has(b));
                assert(b != h@.addr);
            }
        }
        w
    }

    /// # Safety
    /// `w` はこのヒープか付けた塊の生きている文字列の物体を指す。返す位置を読めるのは、物体が生きていて、`string_push`
    /// か `string_concat` でその物体を書き換えていない間だけである。
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
                self.core.lemma_shape(h@.addr);
                if self.core.dom().contains(h@.addr) {
                    self.core.lemma_safe_at(h@.addr);
                }
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
        let tracked p = self.core.tracked_perm(h@.addr);
        Ok(text_ptr(h, len, Tracked(p)))
    }

    /// # Safety
    /// `w` はこのヒープか付けた塊の生きているバイト列の物体を指す。返す位置を読めるのは、物体が生きていて、その物体を
    /// 書き換えていない間だけである。
    pub unsafe fn bytes(&self, w: Word) -> (r: Result<*const [u8], HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && self.is_bytes(w),
        ensures
            self.consistent() && self.settleable() && self.readable(w) && self.is_bytes(w)
                ==> r is Ok,
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        if M::CHECKED && !matches!(self.core.meta_get(h).kind(), Kind::Bytes) {
            return Err(HeapError::WrongKind);
        }
        proof {
            if M::CHECKED {
                self.core.lemma_shape(h@.addr);
                if self.core.dom().contains(h@.addr) {
                    self.core.lemma_safe_at(h@.addr);
                }
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
        let tracked p = self.core.tracked_perm(h@.addr);
        Ok(bytes_ptr(h, len, Tracked(p)))
    }

    /// `string_push` と `bytes_extend` の前提がみな成り立ち、モデルが合っている。`decref` の前提に、left が生きている
    /// 種類 `kind` (`String` か `Bytes`) の物体であることを足す。
    pub closed spec fn push_ok(&self, left: Word, kind: Kind) -> bool {
        &&& kind.has_body()
        &&& self.settleable()
        &&& self.decref_ok(left)
        &&& self.readable(left)
        &&& self.kind_of(left) == kind
    }

    /// `string_concat` とバイト列の連結の前提がみな成り立ち、モデルが合っている。両辺が `decref` の前提を満たす種類
    /// `kind` の物体で、同じ物体なら呼ぶ側がその数えた参照を2つ持つ。
    pub closed spec fn concat_ok(&self, left: Word, right: Word, kind: Kind) -> bool {
        &&& self.push_ok(left, kind)
        &&& self.readable(right)
        &&& self.kind_of(right) == kind
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
        &&& self.region == old.region
        &&& self.worker == old.worker
        &&& self.core.model@.building is None
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& self.core.dom().contains(h)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.rc_of(h) == 1
        &&& self.core.ext(h) == 1
        &&& self.owned(w)
        &&& self.core.kept_both(v, left.bits_spec())
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
        &&& exists|mid: Heap<M>| #[trigger]
            mid.pushed(old, left, w) && mid.consistent() && self.evolved(mid, right, -1)
                && self.kept_live(mid)
        &&& self.core.model@.building is None
        &&& self.core.word_ok(w)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.rc_of(h) == 1
        &&& self.core.ext(h) == 1
    }

    /// `pushed` の中身。`Ctx` の証明が RC のモデルの変化に直すのに使う。
    pub(super) proof fn lemma_pushed(&self, old: Heap<M>, left: Word, w: Word)
        requires
            self.pushed(old, left, w),
        ensures
            ({
                let v = old.core.settled_view();
                let h = w.bits_spec();
                &&& self.dummy == old.dummy
                &&& self.region == old.region
                &&& self.worker == old.worker
                &&& self.core.model@.building is None
                &&& w.is_ptr_spec()
                &&& self.core.word_ok(w)
                &&& self.core.dom().contains(h)
                &&& self.core.state(h) == BlockState::Live
                &&& self.core.rc_of(h) == 1
                &&& self.core.ext(h) == 1
                &&& self.owned(w)
                &&& self.core.kept_both(v, left.bits_spec())
                &&& if w == left {
                    self.core.text_changed(v, h)
                } else {
                    &&& !old.core.dom().contains(h)
                    &&& self.core.evolved(v, left.bits_spec(), -1)
                }
            }),
    {
    }

    /// `concatenated` の中身。`string_push` の後の状態を返す。
    pub(super) proof fn lemma_concatenated(
        &self,
        old: Heap<M>,
        left: Word,
        right: Word,
        w: Word,
    ) -> (mid: Heap<M>)
        requires
            self.concatenated(old, left, right, w),
        ensures
            mid.pushed(old, left, w),
            mid.consistent(),
            self.evolved(mid, right, -1),
            self.kept_live(mid),
            self.core.model@.building is None,
    {
        choose|mid: Heap<M>| #[trigger]
            mid.pushed(old, left, w) && mid.consistent() && self.evolved(mid, right, -1)
                && self.kept_live(mid)
    }

    /// 種類 `kind` の物体を受けて手放す操作と、借りる操作の検査。状態を変える前に呼ぶ。速いモードでは何も確かめない。
    pub(super) fn checked_body(&self, h: *mut Header, kind: Kind) -> (r: Result<(), HeapError>)
        requires
            self.core.sound(),
            self.core.checked() == M::CHECKED,
            self.core.has(h@.addr),
            h == self.core.block(h@.addr),
            kind.has_body(),
        ensures
            !M::CHECKED ==> r is Ok,
            M::CHECKED ==> (r is Ok <==> self.core.kind(h@.addr) == kind && self.decref_passes(
                h@.addr,
            )),
            r is Err ==> chain_err(r) || r == Err::<(), HeapError>(HeapError::UseAfterFree) && !(
            self.core.state(h@.addr) == BlockState::Live || self.core.state(h@.addr)
                == BlockState::Building) || r == Err::<(), HeapError>(HeapError::WrongKind)
                && self.core.kind(h@.addr) != kind,
    {
        if M::CHECKED {
            proof {
                self.core.lemma_code_state(h@.addr);
            }
        }
        self.checked_live(h)?;
        if M::CHECKED && !same_body_kind(self.core.meta_get(h).kind(), kind) {
            return Err(HeapError::WrongKind);
        }
        self.checked_decref(h)?;
        Ok(())
    }

    /// 検査モードで、種類 `kind` の物体 `a` と `b` を受けて手放す操作の検査が通った。両方がその種類で `decref` の検査
    /// を通り、同じ物体なら2回続けた `decref` の検査も通る。
    pub(super) open spec fn pair_passes(&self, a: Word, b: Word, kind: Kind) -> bool {
        &&& self.core.kind(a.bits_spec()) == kind
        &&& self.core.kind(b.bits_spec()) == kind
        &&& self.decref_passes(a.bits_spec())
        &&& self.decref_passes(b.bits_spec())
        &&& a.bits_spec() == b.bits_spec() ==> self.decref_twice_passes(a.bits_spec())
    }

    /// 種類 `kind` の物体 `a` と `b` を受けて手放す操作 (`string_concat` など) の検査。状態を変える前に呼ぶ。速い
    /// モードでは何も確かめない。
    pub(super) fn checked_pair(&self, a: Word, b: Word, kind: Kind) -> (r: Result<(), HeapError>)
        requires
            self.wf(),
            self.core.model@.building is None,
            a.is_ptr_spec(),
            b.is_ptr_spec(),
            self.word_in(a),
            self.word_in(b),
            kind.has_body(),
        ensures
            !M::CHECKED ==> r is Ok,
            self.concat_ok(a, b, kind) ==> r is Ok || chain_err(r),
            M::CHECKED && r is Ok ==> self.pair_passes(a, b, kind),
    {
        let ghost c = self.concat_ok(a, b, kind);
        let ha = self.header_of(a);
        let hb = self.header_of(b);
        proof {
            self.lemma_checks_pass(a);
            self.lemma_checks_pass(b);
            reveal(Core::building_wf);
            if c {
                self.core.lemma_readable(a);
                self.core.lemma_readable(b);
            }
        }
        self.checked_body(ha, kind)?;
        self.checked_body(hb, kind)?;
        let same = a.bits() == b.bits();
        proof {
            if c && same && self.core.dom().contains(ha@.addr) {
                reveal(Core::refs_wf);
                self.core.lemma_at(ha@.addr);
                assert(self.core.counted_wf(ha@.addr));
            }
            if !self.core.dom().contains(ha@.addr) {
                self.core.lemma_region_at(ha@.addr);
            }
        }
        if same {
            self.checked_decref_twice(ha)?;
        }
        Ok(())
    }

    /// 種類 `kind` の物体 `w` を読める。検査モードでは、呼ぶ側が検査 (`checked_body` か `checked_pair`) で生きている
    /// その種類の物体であることを確かめてある。速いモードでは、モデルで読めるその種類の物体である。
    pub(super) open spec fn readable_as(&self, w: Word, kind: Kind) -> bool {
        &&& w.is_ptr_spec()
        &&& self.word_in(w)
        &&& M::CHECKED ==> self.core.kind(w.bits_spec()) == kind && self.decref_passes(
            w.bits_spec(),
        )
        &&& !M::CHECKED ==> self.settleable() && self.readable(w) && self.core.kind(w.bits_spec())
            == kind
    }

    /// 文字列 `w` を読める。
    pub(super) open spec fn text_readable(&self, w: Word) -> bool {
        self.readable_as(w, Kind::String)
    }

    /// バイト列 `w` を読める。
    pub(super) open spec fn bytes_readable(&self, w: Word) -> bool {
        self.readable_as(w, Kind::Bytes)
    }

    /// 文字列 `w` のバイト列を借りる。
    pub(super) fn string_text(&self, w: Word) -> (s: &str)
        requires
            self.wf(),
            self.text_readable(w),
        ensures
            s.spec_bytes() == self.text_of(w),
    {
        let b = self.body_of(w);
        let s = unsafe { str::from_utf8_unchecked(b) };
        proof {
            assert(s.spec_bytes() =~= b@);
        }
        s
    }

    /// 文字列かバイト列 `w` の中身を `&[u8]` として借りる。文字列なら中身は妥当な UTF-8 である。
    pub(super) fn body_of(&self, w: Word) -> (b: &[u8])
        requires
            self.wf(),
            self.text_readable(w) || self.bytes_readable(w),
        ensures
            b@ == self.text_of(w),
            self.core.kind(w.bits_spec()) == Kind::String ==> valid_utf8(b@),
    {
        let h = self.header_of(w);
        proof {
            if M::CHECKED {
                self.core.lemma_shape(h@.addr);
                if self.core.dom().contains(h@.addr) {
                    self.core.lemma_safe_at(h@.addr);
                }
            } else {
                self.core.lemma_readable(w);
                reveal(Core::building_wf);
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
        let tracked p = self.core.tracked_perm(h@.addr);
        bytes(h, len, Tracked(p))
    }

    /// `w` は、ほかに参照のない生きている文字列かバイト列で、その場で書き換えられる。
    pub(super) open spec fn appendable(&self, w: Word) -> bool {
        let h = w.bits_spec();
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& self.core.dom().contains(h)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.kind(h).has_body()
        &&& self.core.rc_of(h) == 1
    }

    /// `old` の書き換えられる物体 `w` を、その場で書き換えた後の状態。`w` の長さの語とバイト列のほかは変わらず、
    /// ほかの文字列とバイト列は同じバイト列のまま読める。
    pub(super) open spec fn appended(&self, old: Heap<M>, w: Word) -> bool {
        &&& self.wf()
        &&& self.same_tokens(old)
        &&& self.dummy == old.dummy
        &&& self.region == old.region
        &&& self.worker == old.worker
        &&& self.core.text_changed(old.core, w.bits_spec())
        &&& self.appendable(w)
        &&& self.kind_of(w) == old.kind_of(w)
        &&& self.capacity(w) == old.capacity(w)
        &&& old.consistent() ==> self.consistent()
        &&& forall|x: Word| #[trigger] self.core.word_ok(x) == old.core.word_ok(x)
        &&& forall|x: Word|
            x.bits_spec() != w.bits_spec() ==> (old.text_readable(x) ==> self.text_readable(x)) && (
            old.bytes_readable(x) ==> self.bytes_readable(x)) && #[trigger] self.text_of(x)
                == old.text_of(x)
    }

    /// 書き足せる文字列 `w` の後ろに `s` を写す。容量は足りている。`String.show` がエスケープと囲みの `"` を書く。
    pub(super) fn append_str(&mut self, w: Word, s: &str)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).appendable(w),
            old(self).text_of(w).len() + s.spec_bytes().len() <= old(self).capacity(w),
        ensures
            final(self).appended(*old(self), w),
            final(self).text_of(w) == old(self).text_of(w) + s.spec_bytes(),
    {
        self.append_tail(w, Tail::Str(s));
    }

    /// 書き足せる文字列 `w` の後ろに、別の文字列 `src` のバイトの範囲 `[from, to)` を写す。容量は足りている。
    /// `String.show` が、エスケープしないバイトの区間をまとめて写す。
    pub(super) fn append_part(&mut self, w: Word, src: Word, from: usize, to: usize)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).appendable(w),
            old(self).text_readable(src),
            src.bits_spec() != w.bits_spec(),
            from <= to <= old(self).text_of(src).len(),
            is_char_boundary(old(self).text_of(src), from as int),
            is_char_boundary(old(self).text_of(src), to as int),
            old(self).text_of(w).len() + (to - from) <= old(self).capacity(w),
        ensures
            final(self).appended(*old(self), w),
            final(self).text_of(w) == old(self).text_of(w) + old(self).text_of(src).subrange(
                from as int,
                to as int,
            ),
    {
        let h = self.header_of(src);
        proof {
            self.core.lemma_shape(h@.addr);
            if M::CHECKED {
                if self.core.dom().contains(h@.addr) {
                    self.core.lemma_safe_at(h@.addr);
                }
            } else {
                self.core.lemma_readable(src);
                reveal(Core::building_wf);
                self.core.lemma_live_block(h@.addr);
            }
        }
        let n = self.core.slot_get(h, 0).bits();
        proof {
            let c = self.core.cap(h@.addr);
            assert((n + 7) / 8 <= c - 1) by (nonlinear_arith)
                requires
                    n <= 8 * (c - 1),
                    c >= 1,
            ;
        }
        self.append_tail(w, Tail::Text(h, n, from, to));
    }

    /// 書き換えられるバイト列 `w` の後ろに `b` を写す。容量は足りている。`Bytes.make` が中身を埋める。
    pub(super) fn append_bytes(&mut self, w: Word, b: &[u8])
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).appendable(w),
            old(self).is_bytes(w),
            old(self).text_of(w).len() + b@.len() <= old(self).capacity(w),
        ensures
            final(self).appended(*old(self), w),
            final(self).text_of(w) == old(self).text_of(w) + b@,
    {
        self.append_tail(w, Tail::Bytes(b));
    }

    /// 書き換えられるバイト列 `w` の後ろに、別のバイト列か文字列 `src` のバイトの範囲 `[from, to)` を写す。容量は足りて
    /// いる。`Bytes.slice` と `Bytes.of_string` が使う。
    pub(super) fn append_body(&mut self, w: Word, src: Word, from: usize, to: usize)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).appendable(w),
            old(self).is_bytes(w),
            old(self).bytes_readable(src) || old(self).text_readable(src),
            src.bits_spec() != w.bits_spec(),
            from <= to <= old(self).text_of(src).len(),
            old(self).text_of(w).len() + (to - from) <= old(self).capacity(w),
        ensures
            final(self).appended(*old(self), w),
            final(self).text_of(w) == old(self).text_of(w) + old(self).text_of(src).subrange(
                from as int,
                to as int,
            ),
    {
        let h = self.header_of(src);
        proof {
            self.core.lemma_shape(h@.addr);
            if M::CHECKED {
                if self.core.dom().contains(h@.addr) {
                    self.core.lemma_safe_at(h@.addr);
                }
            } else {
                self.core.lemma_readable(src);
                reveal(Core::building_wf);
                self.core.lemma_live_block(h@.addr);
            }
        }
        let n = self.core.slot_get(h, 0).bits();
        proof {
            let c = self.core.cap(h@.addr);
            assert((n + 7) / 8 <= c - 1) by (nonlinear_arith)
                requires
                    n <= 8 * (c - 1),
                    c >= 1,
            ;
        }
        self.append_tail(w, Tail::Body(h, n, from, to));
    }

    /// 書き足せる文字列 `w` の後ろに、妥当な UTF-8 のバイト列 `src` の全体を写す。容量は足りている。`String.decode` が使う。
    pub(super) fn append_utf8(&mut self, w: Word, src: Word)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).appendable(w),
            old(self).is_string(w),
            old(self).bytes_readable(src),
            valid_utf8(old(self).text_of(src)),
            src.bits_spec() != w.bits_spec(),
            old(self).text_of(w).len() + old(self).text_of(src).len() <= old(self).capacity(w),
        ensures
            final(self).appended(*old(self), w),
            final(self).text_of(w) == old(self).text_of(w) + old(self).text_of(src),
    {
        let h = self.header_of(src);
        proof {
            self.core.lemma_shape(h@.addr);
            if M::CHECKED {
                if self.core.dom().contains(h@.addr) {
                    self.core.lemma_safe_at(h@.addr);
                }
            } else {
                self.core.lemma_readable(src);
                reveal(Core::building_wf);
                self.core.lemma_live_block(h@.addr);
            }
        }
        let n = self.core.slot_get(h, 0).bits();
        proof {
            let c = self.core.cap(h@.addr);
            assert((n + 7) / 8 <= c - 1) by (nonlinear_arith)
                requires
                    n <= 8 * (c - 1),
                    c >= 1,
            ;
            self.core.lemma_whole(h, n);
        }
        self.append_tail(w, Tail::Text(h, n, 0, n));
    }

    /// `append_str`、`append_part`、`append_bytes`、`append_body` の本体。
    fn append_tail(&mut self, w: Word, t: Tail)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).appendable(w),
            old(self).core.tail_ok(t),
            fits(old(self).core.kind(w.bits_spec()), t),
            t matches Tail::Text(r, _, _, _) ==> r@.addr != w.bits_spec(),
            t matches Tail::Body(r, _, _, _) ==> r@.addr != w.bits_spec(),
            old(self).text_of(w).len() + old(self).core.tail_bytes(t).len() <= old(self).capacity(
                w,
            ),
        ensures
            final(self).appended(*old(self), w),
            final(self).text_of(w) == old(self).text_of(w) + old(self).core.tail_bytes(t),
    {
        let h = self.header_of(w);
        proof {
            self.core.lemma_shape(h@.addr);
            if M::CHECKED {
                self.core.lemma_safe_at(h@.addr);
            } else {
                reveal(Core::building_wf);
                self.core.lemma_live_block(h@.addr);
            }
        }
        let len = self.core.slot_get(h, 0).bits();
        let cap = self.core.cap_get(h);
        let ghost s0 = *self;
        self.write_in_place(h, len, cap, &t);
        proof {
            assert forall|x: Word| x.bits_spec() != w.bits_spec() implies (s0.text_readable(x)
                ==> self.text_readable(x)) && (s0.bytes_readable(x) ==> self.bytes_readable(x))
                && #[trigger] self.text_of(x) == s0.text_of(x) by {
                if s0.core.dom().contains(x.bits_spec()) {
                    assert(self.core.perms@[x.bits_spec()] == s0.core.perms@[x.bits_spec()]);
                }
            }
        }
    }

    /// 種類 `kind` の物体を2つ受けて手放す操作の前提を、`decref` の前提から言う。
    pub(super) proof fn lemma_concat_ok(&self, a: Word, b: Word, kind: Kind)
        requires
            kind.has_body(),
            self.settleable(),
            self.readable(a),
            self.readable(b),
            self.kind_of(a) == kind,
            self.kind_of(b) == kind,
            self.decref_ok(a),
            self.decref_ok(b),
            a.bits_spec() == b.bits_spec() && self.rc_value(a) != 0
                ==> self.core.settled_view().ext(a.bits_spec()) >= 2,
        ensures
            self.concat_ok(a, b, kind),
    {
    }

    /// 種類 `kind` の物体 `a`、`b` の順に参照を1つずつ手放す (同じ物体なら2つ手放す)。検査モードの検査は、呼ぶ側が
    /// 状態を変える前に `checked_pair` で済ませてある。`a` を手放した後の状態を返す。
    pub(super) fn release_pair(
        &mut self,
        a: Word,
        b: Word,
        Ghost(kind): Ghost<Kind>,
        Ghost(c): Ghost<bool>,
    ) -> (mid: Ghost<Heap<M>>)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            a.is_ptr_spec(),
            b.is_ptr_spec(),
            old(self).word_in(a),
            old(self).word_in(b),
            kind.has_body(),
            M::CHECKED ==> old(self).pair_passes(a, b, kind),
            !M::CHECKED ==> c,
            c ==> old(self).consistent() && old(self).concat_ok(a, b, kind),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).core.model@.building is None,
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
            final(self).worker == old(self).worker,
            M::CHECKED ==> final(self).words_kept(*old(self)),
            c ==> {
                &&& mid@.consistent()
                &&& mid@.core.model@.building is None
                &&& mid@.evolved(*old(self), a, -1)
                &&& mid@.kept_live(*old(self))
                &&& final(self).consistent()
                &&& final(self).evolved(mid@, b, -1)
                &&& final(self).kept_live(mid@)
            },
    {
        let ha = self.header_of(a);
        let hb = self.header_of(b);
        let ghost s0 = *self;
        proof {
            if c {
                self.lemma_checks_pass(a);
                self.core.lemma_readable(a);
                if s0.core.dom().contains(ha@.addr) {
                    reveal(Core::refs_wf);
                    s0.core.lemma_at(ha@.addr);
                    assert(s0.core.counted_wf(ha@.addr));
                }
            }
            if !s0.core.dom().contains(ha@.addr) {
                s0.core.lemma_region_at(ha@.addr);
            }
            if !s0.core.dom().contains(hb@.addr) {
                s0.core.lemma_region_at(hb@.addr);
            }
        }
        self.release_text(ha, Ghost(a), Ghost(c));
        let ghost mid = *self;
        proof {
            if !mid.core.dom().contains(hb@.addr) {
                s0.core.lemma_apart(hb@.addr);
            }
        }
        self.release_text(hb, Ghost(b), Ghost(c));
        Ghost(mid)
    }

    /// 文字列かバイト列 `h` の参照を1つ手放す。子を持たないので、最後の参照なら解放の連鎖を通さずに箱だけを空ける。
    /// 検査モードの検査は、呼ぶ側が状態を変える前に済ませてある。
    pub(super) fn release_text(
        &mut self,
        h: *mut Header,
        Ghost(w): Ghost<Word>,
        Ghost(c): Ghost<bool>,
    )
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            w.is_ptr_spec() && w.bits_spec() == h@.addr,
            old(self).core.has(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.kind(h@.addr).has_body(),
            !M::CHECKED ==> c,
            M::CHECKED ==> old(self).decref_passes(h@.addr),
            c ==> old(self).consistent() && old(self).core.state(h@.addr) == BlockState::Live && (
            old(self).core.rc_of(h@.addr) != 0 ==> old(self).core.ext(h@.addr) >= 1),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).core.env == old(self).core.env,
            final(self).core.model@.building is None,
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
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
            ) && final(self).core.kind(h@.addr) == old(self).core.kind(h@.addr),
            final(self).core.kept_live(old(self).core),
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
                // 最後の参照になる物体は rc が 0 でないので、塊の物体でない
                if !s0.dom().contains(h@.addr) {
                    s0.lemma_region_at(h@.addr);
                }
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
                self.core.lemma_kept_live_trans(s1, s0);
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

    /// 一意な文字列かバイト列 `h` の後ろに `t` を写す。容量は足りている。長さの語とバイト列のほかは変えない。
    #[inline]
    fn write_in_place(&mut self, h: *mut Header, len: usize, cap: usize, t: &Tail)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Live,
            old(self).core.kind(h@.addr).has_body(),
            len == old(self).core.perm(h@.addr).text().len(),
            cap == old(self).core.cap(h@.addr),
            len + old(self).core.tail_bytes(*t).len() <= 8 * (cap - 1),
            old(self).core.tail_ok(*t),
            fits(old(self).core.kind(h@.addr), *t),
            t matches Tail::Text(r, _, _, _) ==> r@.addr != h@.addr,
            t matches Tail::Body(r, _, _, _) ==> r@.addr != h@.addr,
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
            final(self).worker == old(self).worker,
            final(self).core.text_changed(old(self).core, h@.addr),
            final(self).core.perm(h@.addr).text() == old(self).core.perm(h@.addr).text() + old(
                self,
            ).core.tail_bytes(*t),
            old(self).consistent() ==> final(self).consistent(),
            forall|x: Word| #[trigger] final(self).core.word_ok(x) == old(self).core.word_ok(x),
    {
        let ghost s0 = *self;
        proof {
            s0.core.lemma_shape(h@.addr);
        }
        let need = len + self.core.tail_len(t);
        let tracked mut perm = self.core.perms.borrow_mut().tracked_remove(h@.addr);
        proof {
            assert((8 * (cap - 1) + 7) / 8 == cap - 1);
        }
        self.core.write_tail(h, cap, len, t, Tracked(&mut perm));
        let tracked mut slot = perm.slots.tracked_remove(0);
        ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(need));
        proof {
            perm.slots.tracked_insert(0, slot);
            self.core.perms.borrow_mut().tracked_insert(h@.addr, perm);
            let a = h@.addr;
            let q = s0.core.perm(a);
            if s0.core.kind(a) == Kind::String {
                s0.core.lemma_tail_utf8(*t);
                valid_utf8_concat(q.text(), s0.core.tail_bytes(*t));
            }
            assert(self.core.perms@.dom() =~= s0.core.perms@.dom());
            assert(self.core.perm(a).slots.dom() =~= q.slots.dom());
            assert(self.core.text_changed(s0.core, a));
            if s0.consistent() {
                self.core.lemma_text_set(s0.core, a);
                assert(self.core.perms@[s0.dummy.bits_spec()]
                    == s0.core.perms@[s0.dummy.bits_spec()]);
            }
            if M::CHECKED {
                self.core.lemma_safe_text_set(s0.core, a);
            }
        }
    }

    /// `string_push`、`string_concat` とバイト列の伸ばす操作の本体。検査はみな済んでいる。種類 `kind` の left の参照を
    /// 1つ受け取り、`t` を後ろに足した同じ種類の物体の参照を返す。left が一意で容量が足りればその場で書き、足りなければ容量を倍にして作り直す。一意でなければ
    /// 和の長さで写す。どちらの写しも left を手放す前に済ませる。速いモードでは、大きい物体のページを箱の解放がすぐに
    /// 返すためである。速いモードは rc 0 の物体のページの頭を読まないので、容量は一意な left でだけ読む
    /// (docs/spec/object-model.md の「バイト列と文字列」)。
    #[inline]
    fn push_unchecked(&mut self, left: Word, t: Tail, kind: Kind, Ghost(c): Ghost<bool>) -> (w:
        Word)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            left.is_ptr_spec(),
            old(self).core.word_ok(left),
            old(self).core.state(left.bits_spec()) == BlockState::Live,
            kind.has_body(),
            old(self).core.kind(left.bits_spec()) == kind,
            old(self).core.tail_ok(t),
            fits(kind, t),
            t matches Tail::Text(r, _, _, _) && r@.addr == left.bits_spec() ==> old(
                self,
            ).core.rc_of(left.bits_spec()) != 1,
            t matches Tail::Body(r, _, _, _) && r@.addr == left.bits_spec() ==> old(
                self,
            ).core.rc_of(left.bits_spec()) != 1,
            !M::CHECKED ==> c,
            M::CHECKED ==> old(self).decref_passes(left.bits_spec()),
            c ==> old(self).consistent() && old(self).core.holds(left) && old(self).owned(left),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).core.env == old(self).core.env,
            final(self).core.model@.building is None,
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
            final(self).worker == old(self).worker,
            w.is_ptr_spec(),
            final(self).core.word_ok(w),
            final(self).core.dom().contains(w.bits_spec()),
            final(self).core.kind(w.bits_spec()) == kind,
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
            M::CHECKED ==> forall|x: Word|
                old(self).core.word_ok(x) ==> #[trigger] final(self).core.word_ok(x),
            M::CHECKED && old(self).decref_twice_passes(left.bits_spec()) ==> {
                &&& final(self).core.has(left.bits_spec())
                &&& final(self).core.block(left.bits_spec()) == old(self).core.block(
                    left.bits_spec(),
                )
                &&& final(self).core.kind(left.bits_spec()) == kind
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
            proof {
                // 一意な物体 (rc 1) は、塊の物体でない
                if !s0.core.dom().contains(h@.addr) {
                    s0.core.lemma_region_at(h@.addr);
                }
            }
            self.write_in_place(h, len, cap, &t);
            proof {
                if c {
                    reveal(Core::refs_wf);
                    assert(s0.core.counted_wf(h@.addr));
                    assert forall|b: usize|
                        #![trigger self.core.settled_state(b), s0.core.settled_state(b)]
                        self.core.has(b) && s0.core.has(b) && b != h@.addr implies self.core.perm(b)
                        == s0.core.perm(b) by {
                        if s0.core.dom().contains(b) {
                            assert(self.core.perms@[b] == s0.core.perms@[b]);
                        }
                    }
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
            if kind == Kind::String {
                s0.core.lemma_whole(h, len);
            }
        }
        // 文字列の left は UTF-8 の約束のある `Text` で写す。`kind` は呼ぶ側の定数なので、展開されると分岐は消える
        let head = if same_body_kind(kind, Kind::String) {
            Tail::Text(h, len, 0, len)
        } else {
            Tail::Body(h, len, 0, len)
        };
        let w = self.make_text(want, head, Some(t), 1, kind);
        let ghost mid = *self;
        proof {
            // 塊の left は、新しい物体のブロックと重ならない
            if !s0.core.dom().contains(h@.addr) {
                s0.core.lemma_region_at(h@.addr);
                assert(mid.core.region_has(h@.addr));
                assert(!mid.core.pages.in_use().contains(h@.addr));
            }
        }
        self.release_text(h, Ghost(left), Ghost(c));
        proof {
            let a = h@.addr;
            let b = w.bits_spec();
            assert(mid.core.perms@[b] == self.core.perms@[b]);
            if c {
                self.core.lemma_frame_evolved(mid.core, s0.core, a, -1);
                // 両方で生きている物体は、作り直しの前 (`mid`) でも生きていて、`mid` から中身を変えない
                assert forall|x: usize|
                    #![trigger self.core.settled_state(x), s0.core.settled_state(x)]
                    self.core.has(x) && self.core.settled_state(x) == BlockState::Live
                        && s0.core.has(x) && s0.core.settled_state(x)
                        == BlockState::Live implies self.core.kept_block(s0.core, x) by {
                    assert(mid.core.has(x) && mid.core.settled_state(x) == BlockState::Live);
                    assert(self.core.kept_block(mid.core, x));
                    if s0.core.dom().contains(x) {
                        assert(mid.core.perms@[x] == s0.core.perms@[x]);
                    } else {
                        s0.core.lemma_apart(x);
                        s0.core.lemma_region_at(x);
                        mid.core.lemma_apart(x);
                    }
                }
            }
        }
        w
    }

    /// left の後ろに s を足した文字列を返す。left の参照を1つ受け取り、結果の参照を1つ返す。どの誤りも最初の確保より
    /// 前に返すので、`Err` のときヒープは変わらない (docs/spec/object-model.md の「バイト列と文字列」)。
    ///
    /// # Safety
    /// `left` はこのヒープか付けた塊の生きている文字列の物体を指し、呼ぶ側がその参照を1つ持つ (`decref` と同じ前提)。
    /// `s` は left 自身の中身を指さない。一意な left を作り直すとこの呼び出しの中で left の箱を空け、大きい物体なら
    /// ページも返すが、引数の `&str` が指すメモリを呼び出しの間に返すと未定義の動作になるためである。自分自身との連結
    /// は `dup` と `string_concat(x, x)` で行う。`s` がほかの文字列の中身を指すときは、その物体はこの呼び出しの間
    /// 生きている。
    #[inline]
    pub unsafe fn string_push(&mut self, left: Word, s: &str) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            left.is_ptr_spec(),
            old(self).word_in(left),
            !M::CHECKED ==> old(self).push_ok(left, Kind::String),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> final(self).is_string(w) && final(self).text_of(w) == old(
                self,
            ).text_of(left) + s.spec_bytes(),
            old(self).push_ok(left, Kind::String) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).push_ok(left, Kind::String) ==> final(self).consistent()
                && final(self).pushed(*old(self), left, w)),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let ghost c = self.push_ok(left, Kind::String);
        self.settle();
        let h = self.header_of(left);
        proof {
            self.lemma_checks_pass(left);
            reveal(Core::building_wf);
            if c {
                self.core.lemma_readable(left);
            }
        }
        self.checked_body(h, Kind::String)?;
        let ghost s1 = *self;
        let w = self.push_unchecked(left, Tail::Str(s), Kind::String, Ghost(c));
        proof {
            assert(old(self).core.settled_view() == s1.core);
        }
        Ok(w)
    }

    /// `++`。両辺の参照を受け取り、結果の参照を返す。`string_push(left, string(right))` の後に right を手放す。どの誤りも
    /// 最初の確保より前に返す (docs/spec/object-model.md の「バイト列と文字列」)。
    ///
    /// # Safety
    /// `left` と `right` はこのヒープか付けた塊の生きている文字列の物体を指し、呼ぶ側がそれぞれの参照を1つずつ持つ
    /// (同じ物体なら2つ持つ)。
    pub unsafe fn string_concat(&mut self, left: Word, right: Word) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            left.is_ptr_spec(),
            right.is_ptr_spec(),
            old(self).word_in(left),
            old(self).word_in(right),
            !M::CHECKED ==> old(self).concat_ok(left, right, Kind::String),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            final(self).settled(),
            r matches Ok(w) ==> w.is_ptr_spec() && final(self).is_string(w) && final(self).text_of(
                w,
            ) == old(self).text_of(left) + old(self).text_of(right),
            old(self).concat_ok(left, right, Kind::String) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).concat_ok(left, right, Kind::String)
                ==> final(self).consistent() && final(self).concatenated(
                *old(self),
                left,
                right,
                w,
            )),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        unsafe { self.concat(left, right, Kind::String) }
    }

    /// 種類 `kind` (`String` か `Bytes`) の2つの物体の連結。`string_concat` の本体で、`Bytes.append` も使う。
    ///
    /// # Safety
    /// `string_concat` と同じで、両辺は種類 `kind` の物体を指す。
    pub(super) unsafe fn concat(&mut self, left: Word, right: Word, kind: Kind) -> (r: Result<
        Word,
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            left.is_ptr_spec(),
            right.is_ptr_spec(),
            old(self).word_in(left),
            old(self).word_in(right),
            kind.has_body(),
            !M::CHECKED ==> old(self).concat_ok(left, right, kind),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            final(self).settled(),
            r matches Ok(w) ==> w.is_ptr_spec() && final(self).kind_of(w) == kind
                && final(self).text_of(w) == old(self).text_of(left) + old(self).text_of(right),
            old(self).concat_ok(left, right, kind) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).concat_ok(left, right, kind) ==> final(self).consistent()
                && final(self).concatenated(*old(self), left, right, w)),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let ghost c = self.concat_ok(left, right, kind);
        self.settle();
        let hr = self.header_of(right);
        self.checked_pair(left, right, kind)?;
        proof {
            let a = left.bits_spec();
            reveal(Core::building_wf);
            if c {
                self.lemma_checks_pass(left);
                self.lemma_checks_pass(right);
                self.core.lemma_readable(left);
                self.core.lemma_readable(right);
                // 同じ物体を2つ持つなら rc は 1 でないので、その場で自分自身を写すことはない
                if a == right.bits_spec() && self.core.dom().contains(a) {
                    reveal(Core::refs_wf);
                    self.core.lemma_at(a);
                    assert(self.core.counted_wf(a));
                }
            }
            if !self.core.dom().contains(a) {
                self.core.lemma_region_at(a);
            }
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
            if kind == Kind::String {
                self.core.lemma_whole(hr, len);
            } else {
                let t = self.core.perm(hr@.addr).text();
                assert(t.subrange(0, len as int) =~= t);
            }
        }
        let right_tail = if same_body_kind(kind, Kind::String) {
            Tail::Text(hr, len, 0, len)
        } else {
            Tail::Body(hr, len, 0, len)
        };
        let ghost s1 = *self;
        let w = self.push_unchecked(left, right_tail, kind, Ghost(c));
        let ghost mid = *self;
        proof {
            assert(old(self).core.settled_view() == s1.core);
            // 塊の right は、新しい物体のブロックと重ならない
            if !s1.core.dom().contains(hr@.addr) {
                s1.core.lemma_region_at(hr@.addr);
                assert(mid.core.region_has(hr@.addr));
                assert(!mid.core.pages.in_use().contains(hr@.addr));
            }
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

    /// 一意なバイト列 `h` を、前の `n` バイトに切り詰める。長さの語だけを書き、バイト列の語は書かない。
    #[inline]
    fn truncate_in_place(&mut self, h: *mut Header, n: usize)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Live,
            old(self).core.kind(h@.addr) == Kind::Bytes,
            n <= old(self).core.perm(h@.addr).text().len(),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
            final(self).worker == old(self).worker,
            final(self).core.text_changed(old(self).core, h@.addr),
            final(self).core.perm(h@.addr).text() == old(self).core.perm(h@.addr).text().subrange(
                0,
                n as int,
            ),
            old(self).consistent() ==> final(self).consistent(),
            forall|x: Word| #[trigger] final(self).core.word_ok(x) == old(self).core.word_ok(x),
    {
        let ghost s0 = *self;
        proof {
            s0.core.lemma_shape(h@.addr);
        }
        let tracked mut perm = self.core.perms.borrow_mut().tracked_remove(h@.addr);
        proof {
            perm.truncate_text(n as int);
        }
        let tracked mut slot = perm.slots.tracked_remove(0);
        ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(n));
        proof {
            perm.slots.tracked_insert(0, slot);
            self.core.perms.borrow_mut().tracked_insert(h@.addr, perm);
            let a = h@.addr;
            let q = s0.core.perm(a);
            assert(self.core.perms@.dom() =~= s0.core.perms@.dom());
            assert(self.core.perm(a).slots.dom() =~= q.slots.dom());
            assert(self.core.text_changed(s0.core, a));
            if s0.consistent() {
                self.core.lemma_text_set(s0.core, a);
                assert(self.core.perms@[s0.dummy.bits_spec()]
                    == s0.core.perms@[s0.dummy.bits_spec()]);
            }
            if M::CHECKED {
                self.core.lemma_safe_text_set(s0.core, a);
            }
        }
    }

    /// 一意なバイト列 `h` の `i` バイト目を `b` に書き換える。長さの語とほかのバイトは変えない。
    #[inline]
    fn write_byte_in_place(&mut self, h: *mut Header, i: usize, b: u8)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Live,
            old(self).core.kind(h@.addr) == Kind::Bytes,
            i < old(self).core.perm(h@.addr).text().len(),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
            final(self).worker == old(self).worker,
            final(self).core.text_changed(old(self).core, h@.addr),
            final(self).core.perm(h@.addr).text() == old(self).core.perm(h@.addr).text().update(
                i as int,
                b,
            ),
            old(self).consistent() ==> final(self).consistent(),
            forall|x: Word| #[trigger] final(self).core.word_ok(x) == old(self).core.word_ok(x),
    {
        let ghost s0 = *self;
        proof {
            s0.core.lemma_shape(h@.addr);
        }
        let cap = self.core.cap_get(h);
        let tracked mut perm = self.core.perms.borrow_mut().tracked_remove(h@.addr);
        proof {
            assert((8 * (cap - 1) + 7) / 8 == cap - 1);
        }
        write_byte(h, cap, i, b, Tracked(&mut perm));
        proof {
            self.core.perms.borrow_mut().tracked_insert(h@.addr, perm);
            let a = h@.addr;
            let q = s0.core.perm(a);
            assert(self.core.perms@.dom() =~= s0.core.perms@.dom());
            assert(self.core.perm(a).slots.dom() =~= q.slots.dom());
            assert(self.core.text_changed(s0.core, a));
            if s0.consistent() {
                self.core.lemma_text_set(s0.core, a);
                assert(self.core.perms@[s0.dummy.bits_spec()]
                    == s0.core.perms@[s0.dummy.bits_spec()]);
            }
            if M::CHECKED {
                self.core.lemma_safe_text_set(s0.core, a);
            }
        }
    }

    /// `set_body_kind` の後の状態。物体 `h` の `meta` の種類だけが `kind` になり、ほかは変わらない。
    pub(super) open spec fn rekinded(&self, old: Heap<M>, h: usize, kind: Kind) -> bool {
        &&& self.dummy == old.dummy
        &&& self.region == old.region
        &&& self.worker == old.worker
        &&& self.tokens == old.tokens
        &&& self.building == old.building
        &&& old.core.dom().contains(h)
        &&& old.core.state(h) == BlockState::Live
        &&& old.core.model@.building is None
        &&& old.core.kind(h).has_body()
        &&& self.core.meta_changed(old.core, h)
        &&& bits_wf(self.core.meta_bits(h))
        &&& bits_lin(self.core.meta_bits(h)) == bits_lin(old.core.meta_bits(h))
        &&& bits_tag(self.core.meta_bits(h)) == bits_tag(old.core.meta_bits(h))
        &&& bits_scan(self.core.meta_bits(h)) == bits_scan(old.core.meta_bits(h))
        &&& bits_kind(self.core.meta_bits(h)) == kind
    }

    /// 文字列かバイト列 `h` の種類を `kind` (`String` か `Bytes`) に書き換える。`meta` だけを書き、長さの語とバイト列は
    /// 変えない。`Bytes.of_string` と `String.decode` が、一意な引数を写さずに結果にするのに使う。一意な物体はほかの
    /// 物体から辺で指されず、ほかのスレッドも読めないので、種類を変えても誰の前提も崩れない
    /// (docs/spec/object-model.md の「変換」)。
    pub(super) fn set_body_kind(&mut self, h: *mut Header, kind: Kind)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Live,
            old(self).core.kind(h@.addr).has_body(),
            kind.has_body(),
            kind == Kind::String ==> valid_utf8(old(self).core.perm(h@.addr).text()),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).rekinded(*old(self), h@.addr, kind),
            final(self).core.kind(h@.addr) == kind,
            old(self).consistent() ==> final(self).consistent(),
            forall|x: Word| #[trigger] final(self).core.word_ok(x) == old(self).core.word_ok(x),
    {
        let ghost s0 = *self;
        proof {
            s0.core.lemma_shape(h@.addr);
        }
        let m = self.core.meta_get(h);
        let n = Meta::new(m.tag(), m.scan(), kind, m.lin());
        self.core.meta_put(h, n);
        proof {
            let a = h@.addr;
            m.lemma_fields();
            n.lemma_fields();
            self.core.lemma_meta_set(s0.core, a);
            if s0.consistent() {
                assert(self.core.perms@[s0.dummy.bits_spec()]
                    == s0.core.perms@[s0.dummy.bits_spec()]);
                assert(self.core.quiet());
                assert(self.dummy_wf());
            }
        }
    }

    /// バイト列 left の前の `keep` バイトを持つ、一意なバイト列を返す。left の参照を1つ受け取り、結果の参照を返す。
    /// left が一意ならその場で長さの語だけを書き、そうでなければ `keep` バイトを写した新しい物体を作ってから left を
    /// 手放す (docs/spec/object-model.md の「バイト列の操作」)。検査はみな済んでいる。
    fn own_unchecked(&mut self, left: Word, keep: usize, Ghost(c): Ghost<bool>) -> (w: Word)
        requires
            old(self).wf(),
            old(self).core.model@.building is None,
            left.is_ptr_spec(),
            old(self).core.word_ok(left),
            old(self).core.state(left.bits_spec()) == BlockState::Live,
            old(self).core.kind(left.bits_spec()) == Kind::Bytes,
            keep <= old(self).text_of(left).len(),
            !M::CHECKED ==> c,
            M::CHECKED ==> old(self).decref_passes(left.bits_spec()),
            c ==> old(self).consistent() && old(self).core.holds(left) && old(self).owned(left),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            final(self).core.env == old(self).core.env,
            final(self).core.model@.building is None,
            final(self).dummy == old(self).dummy,
            final(self).region == old(self).region,
            final(self).worker == old(self).worker,
            final(self).appendable(w),
            final(self).core.kind(w.bits_spec()) == Kind::Bytes,
            final(self).text_of(w) == old(self).text_of(left).subrange(0, keep as int),
            w != left ==> !old(self).core.dom().contains(w.bits_spec()),
            c ==> final(self).consistent() && final(self).pushed(*old(self), left, w),
            M::CHECKED ==> forall|x: Word|
                old(self).core.word_ok(x) ==> #[trigger] final(self).core.word_ok(x),
    {
        let ghost s0 = *self;
        let h = self.header_of(left);
        proof {
            s0.core.lemma_shape(h@.addr);
        }
        let len = self.core.slot_get(h, 0).bits();
        if self.core.rc_get(h) == 1 {
            proof {
                // 一意な物体 (rc 1) は、塊の物体でない
                if !s0.core.dom().contains(h@.addr) {
                    s0.core.lemma_region_at(h@.addr);
                }
            }
            self.truncate_in_place(h, keep);
            proof {
                if c {
                    reveal(Core::refs_wf);
                    assert(s0.core.counted_wf(h@.addr));
                    assert forall|b: usize|
                        #![trigger self.core.settled_state(b), s0.core.settled_state(b)]
                        self.core.has(b) && s0.core.has(b) && b != h@.addr implies self.core.perm(b)
                        == s0.core.perm(b) by {
                        if s0.core.dom().contains(b) {
                            assert(self.core.perms@[b] == s0.core.perms@[b]);
                        }
                    }
                }
            }
            return left;
        }
        proof {
            let cap = s0.core.cap(h@.addr);
            assert((len + 7) / 8 <= cap - 1) by (nonlinear_arith)
                requires
                    len <= 8 * (cap - 1),
                    cap >= 1,
            ;
        }
        let w = self.make_text(keep, Tail::Body(h, len, 0, keep), None, 1, Kind::Bytes);
        let ghost mid = *self;
        proof {
            assert(s0.core.tail_bytes(Tail::Body(h, len, 0, keep)) + Seq::<u8>::empty()
                =~= s0.text_of(left).subrange(0, keep as int));
            // 塊の left は、新しい物体のブロックと重ならない
            if !s0.core.dom().contains(h@.addr) {
                s0.core.lemma_region_at(h@.addr);
                assert(mid.core.region_has(h@.addr));
                assert(!mid.core.pages.in_use().contains(h@.addr));
            }
        }
        self.release_text(h, Ghost(left), Ghost(c));
        proof {
            let a = h@.addr;
            let b = w.bits_spec();
            assert(mid.core.perms@[b] == self.core.perms@[b]);
            if c {
                self.core.lemma_frame_evolved(mid.core, s0.core, a, -1);
                // 両方で生きている物体は、作り直しの前 (`mid`) でも生きていて、`mid` から中身を変えない
                assert forall|x: usize|
                    #![trigger self.core.settled_state(x), s0.core.settled_state(x)]
                    self.core.has(x) && self.core.settled_state(x) == BlockState::Live
                        && s0.core.has(x) && s0.core.settled_state(x)
                        == BlockState::Live implies self.core.kept_block(s0.core, x) by {
                    assert(mid.core.has(x) && mid.core.settled_state(x) == BlockState::Live);
                    assert(self.core.kept_block(mid.core, x));
                    if s0.core.dom().contains(x) {
                        assert(mid.core.perms@[x] == s0.core.perms@[x]);
                    } else {
                        s0.core.lemma_apart(x);
                        s0.core.lemma_region_at(x);
                        mid.core.lemma_apart(x);
                    }
                }
            }
        }
        w
    }

    /// `pushed` の後に、結果の物体 `w` のその場の書き換えだけを続けても、`pushed` は保たれる。
    proof fn lemma_pushed_text(&self, mid: Heap<M>, old: Heap<M>, left: Word, w: Word)
        requires
            mid.pushed(old, left, w),
            mid.wf(),
            self.core.text_changed(mid.core, w.bits_spec()),
            self.dummy == mid.dummy,
            self.region == mid.region,
            self.worker == mid.worker,
        ensures
            self.pushed(old, left, w),
    {
        let v = old.core.settled_view();
        let h = w.bits_spec();
        assert(self.core.word_ok(w) == mid.core.word_ok(w));
        assert(mid.core.kept_both(v, left.bits_spec()));
        if w != left {
            assert forall|b: usize|
                #![trigger v.dom().contains(b)]
                v.dom().contains(b) && v.settled_state(b) == BlockState::Live && (v.rc_of(b) == 0
                    || v.ext(b) + (if b == left.bits_spec() {
                    -1int
                } else {
                    0
                }) >= 1) implies {
                &&& self.core.dom().contains(b)
                &&& self.core.state(b) == BlockState::Live
                &&& self.core.perm(b).meta == v.perm(b).meta
                &&& self.core.perm(b).slots == v.perm(b).slots
                &&& self.core.perm(b).text == v.perm(b).text
                &&& self.core.block(b) == v.block(b)
                &&& self.core.cap(b) == v.cap(b)
                &&& (v.rc_of(b) == 0) == (self.core.rc_of(b) == 0)
                &&& v.rc_of(b) != 0 ==> self.core.ext(b) == v.ext(b) + if b == left.bits_spec() {
                    -1int
                } else {
                    0
                }
                &&& self.core.model@.owner[b] == v.model@.owner[b]
            } by {
                assert(mid.core.dom().contains(b));
                assert(b != h);
                assert(self.core.perms@[b] == mid.core.perms@[b]);
            }
        }
        assert forall|b: usize|
            #![trigger self.core.settled_state(b), v.settled_state(b)]
            self.core.has(b) && self.core.settled_state(b) == BlockState::Live && v.has(b)
                && v.settled_state(b) == BlockState::Live implies {
            &&& self.core.perm(b).meta == v.perm(b).meta
            &&& (self.core.rc_of(b) > 0) == (v.rc_of(b) > 0)
            &&& (self.core.rc_of(b) < 0) == (v.rc_of(b) < 0)
            &&& self.core.rc_of(b) > 0 ==> self.core.model@.owner[b] == v.model@.owner[b]
            &&& b != left.bits_spec() ==> self.core.perm(b).slots == v.perm(b).slots
                && self.core.perm(b).text == v.perm(b).text
        } by {
            assert(mid.core.has(b) && mid.core.settled_state(b) == BlockState::Live);
            assert(self.core.model == mid.core.model);
            if mid.core.dom().contains(b) {
                if b != h {
                    assert(self.core.perms@[b] == mid.core.perms@[b]);
                }
            }
            if b == h && w != left && !v.dom().contains(b) {
                // 新しく作った `w` は `old` で生きていなかったので、この枝は起きない
                mid.core.lemma_apart(b);
            }
        }
    }

    /// バイト列の後ろに `src` を足したバイト列を返す。`string_push` のバイト列の版で、約束も同じである。
    ///
    /// # Safety
    /// `left` はこのヒープか付けた塊の生きているバイト列の物体を指し、呼ぶ側がその参照を1つ持つ。`src` は left 自身の
    /// 中身を指さない (`string_push` と同じ理由)。
    pub unsafe fn bytes_extend(&mut self, left: Word, src: &[u8]) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            left.is_ptr_spec(),
            old(self).word_in(left),
            !M::CHECKED ==> old(self).push_ok(left, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> w.is_ptr_spec() && final(self).is_bytes(w) && final(self).text_of(w)
                == old(self).text_of(left) + src@ && final(self).word_in(w),
            old(self).push_ok(left, Kind::Bytes) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).push_ok(left, Kind::Bytes) ==> final(self).consistent()
                && final(self).pushed(*old(self), left, w) && final(self).others_kept(
                *old(self),
                left,
            )),
            final(self).settled(),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let ghost c = self.push_ok(left, Kind::Bytes);
        self.settle();
        let h = self.header_of(left);
        proof {
            self.lemma_checks_pass(left);
            reveal(Core::building_wf);
            if c {
                self.core.lemma_readable(left);
            }
        }
        self.checked_body(h, Kind::Bytes)?;
        let ghost s1 = *self;
        let w = self.push_unchecked(left, Tail::Bytes(src), Kind::Bytes, Ghost(c));
        proof {
            assert(old(self).core.settled_view() == s1.core);
            if c {
                assert forall|b: usize|
                    #![trigger old(self).core.has(b)]
                    old(self).core.has(b) && old(self).core.settled_state(b) == BlockState::Live
                        && b != left.bits_spec() implies self.core.has(b)
                    && self.core.settled_state(b) == BlockState::Live && self.core.block(b) == old(
                    self,
                ).core.block(b) by {
                    assert(s1.core.has(b));
                    if s1.core.dom().contains(b) {
                        assert(self.core.dom().contains(b));
                    } else {
                        s1.core.lemma_region_at(b);
                    }
                }
            }
        }
        Ok(w)
    }

    /// `Bytes.append_slice`。`dst` の後ろに、`src` のバイトの範囲 `[from, to)` を足したバイト列を返す。`dst` の参照を
    /// 受け取り、`src` は借りる。`dst` が一意ならその場で足す。同じ物体を借りて消費するときは、呼ぶ側がその参照を2つ
    /// 持つので一意でなく、写す道に進む。検査モードは、同じ物体で rc が 1 なら `NotUnique` を返す
    /// (docs/spec/lir.md の「extern の借用」)。どの誤りも最初の確保より前に返す。
    ///
    /// # Safety
    /// `dst` と `src` はこのヒープか付けた塊の生きているバイト列の物体を指し、呼ぶ側がそれぞれの参照を1つずつ持つ
    /// (同じ物体なら2つ持つ)。
    pub(super) unsafe fn bytes_append_part(
        &mut self,
        dst: Word,
        src: Word,
        from: usize,
        to: usize,
    ) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            dst.is_ptr_spec(),
            src.is_ptr_spec(),
            old(self).word_in(dst),
            old(self).word_in(src),
            from <= to <= old(self).text_of(src).len(),
            !M::CHECKED ==> old(self).concat_ok(dst, src, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> w.is_ptr_spec() && final(self).is_bytes(w) && final(self).text_of(w)
                == old(self).text_of(dst) + old(self).text_of(src).subrange(from as int, to as int),
            old(self).concat_ok(dst, src, Kind::Bytes) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).concat_ok(dst, src, Kind::Bytes)
                ==> final(self).consistent() && final(self).pushed(*old(self), dst, w)),
            final(self).settled(),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let ghost c = self.concat_ok(dst, src, Kind::Bytes);
        self.settle();
        let hs = self.header_of(src);
        self.checked_part(dst, src)?;
        proof {
            let a = dst.bits_spec();
            reveal(Core::building_wf);
            if c {
                self.lemma_checks_pass(dst);
                self.core.lemma_readable(dst);
                self.lemma_checks_pass(src);
                self.core.lemma_readable(src);
                // 同じ物体を2つ持つなら rc は 1 でないので、その場で自分自身を写すことはない
                if a == src.bits_spec() && self.core.dom().contains(a) {
                    reveal(Core::refs_wf);
                    self.core.lemma_at(a);
                    assert(self.core.counted_wf(a));
                }
            }
            if !self.core.dom().contains(a) {
                self.core.lemma_region_at(a);
            }
            self.core.lemma_shape(hs@.addr);
        }
        let n = self.core.slot_get(hs, 0).bits();
        proof {
            let cap = self.core.cap(hs@.addr);
            assert((n + 7) / 8 <= cap - 1) by (nonlinear_arith)
                requires
                    n <= 8 * (cap - 1),
                    cap >= 1,
            ;
        }
        let ghost s1 = *self;
        let w = self.push_unchecked(dst, Tail::Body(hs, n, from, to), Kind::Bytes, Ghost(c));
        proof {
            assert(old(self).core.settled_view() == s1.core);
        }
        Ok(w)
    }

    /// `bytes_append_part` の検査。`checked_pair` に、同じ物体を借りて消費するなら一意でないことを足す
    /// (docs/spec/lir.md の「extern の借用」)。状態を変える前に呼ぶ。速いモードでは何も確かめない。
    pub(super) fn checked_part(&self, dst: Word, src: Word) -> (r: Result<(), HeapError>)
        requires
            self.wf(),
            self.core.model@.building is None,
            dst.is_ptr_spec(),
            src.is_ptr_spec(),
            self.word_in(dst),
            self.word_in(src),
        ensures
            !M::CHECKED ==> r is Ok,
            self.concat_ok(dst, src, Kind::Bytes) ==> r is Ok || chain_err(r),
            M::CHECKED && r is Ok ==> self.pair_passes(dst, src, Kind::Bytes),
            M::CHECKED && r is Ok && dst.bits_spec() == src.bits_spec() ==> self.core.rc_of(
                dst.bits_spec(),
            ) != 1,
    {
        let ghost c = self.concat_ok(dst, src, Kind::Bytes);
        let hd = self.header_of(dst);
        proof {
            let a = dst.bits_spec();
            self.lemma_checks_pass(dst);
            reveal(Core::building_wf);
            if c {
                self.core.lemma_readable(dst);
                // 同じ物体を2つ持つなら rc は 1 でない
                if a == src.bits_spec() && self.core.dom().contains(a) {
                    reveal(Core::refs_wf);
                    self.core.lemma_at(a);
                    assert(self.core.counted_wf(a));
                }
            }
            if !self.core.dom().contains(a) {
                self.core.lemma_region_at(a);
            }
        }
        if M::CHECKED && dst.bits() == src.bits() {
            self.checked_body(hd, Kind::Bytes)?;
            if self.core.rc_get(hd) == 1 {
                return Err(HeapError::NotUnique);
            }
        }
        self.checked_pair(dst, src, Kind::Bytes)
    }

    /// 種類 `kind` の物体を1つ受けて手放す操作の前提を、`decref` の前提から言う。
    pub(super) proof fn lemma_push_ok(&self, w: Word, kind: Kind)
        requires
            kind.has_body(),
            self.settleable(),
            self.readable(w),
            self.kind_of(w) == kind,
            self.decref_ok(w),
        ensures
            self.push_ok(w, kind),
    {
    }

    /// `Bytes.truncate`。バイト列 left の前の `n` バイトを返す。left の参照を受け取り、結果の参照を返す。一意なら長さの
    /// 語だけを書き、容量は保つ (docs/spec/object-model.md の「バイト列の操作」)。どの誤りも最初の確保より前に返す。
    ///
    /// # Safety
    /// `bytes_extend` と同じ。
    pub(super) unsafe fn bytes_truncate(&mut self, left: Word, n: usize) -> (r: Result<
        Word,
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            left.is_ptr_spec(),
            old(self).word_in(left),
            n <= old(self).text_of(left).len(),
            !M::CHECKED ==> old(self).push_ok(left, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> w.is_ptr_spec() && final(self).is_bytes(w) && final(self).text_of(w)
                == old(self).text_of(left).subrange(0, n as int),
            old(self).push_ok(left, Kind::Bytes) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).push_ok(left, Kind::Bytes) ==> final(self).consistent()
                && final(self).pushed(*old(self), left, w)),
            final(self).settled(),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let ghost c = self.push_ok(left, Kind::Bytes);
        self.settle();
        let h = self.header_of(left);
        proof {
            self.lemma_checks_pass(left);
            reveal(Core::building_wf);
            if c {
                self.core.lemma_readable(left);
            }
        }
        self.checked_body(h, Kind::Bytes)?;
        let ghost s1 = *self;
        let w = self.own_unchecked(left, n, Ghost(c));
        proof {
            assert(old(self).core.settled_view() == s1.core);
        }
        Ok(w)
    }

    /// `Bytes.set`。バイト列 left の `i` バイト目を `b` にしたバイト列を返す。left の参照を受け取り、結果の参照を返す。
    /// 一意ならその場で書き、そうでなければ写してから書く。どの誤りも最初の確保より前に返す。
    ///
    /// # Safety
    /// `bytes_extend` と同じ。
    pub(super) unsafe fn bytes_set(&mut self, left: Word, i: usize, b: u8) -> (r: Result<
        Word,
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            left.is_ptr_spec(),
            old(self).word_in(left),
            i < old(self).text_of(left).len(),
            !M::CHECKED ==> old(self).push_ok(left, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> w.is_ptr_spec() && final(self).is_bytes(w) && final(self).text_of(w)
                == old(self).text_of(left).update(i as int, b),
            old(self).push_ok(left, Kind::Bytes) ==> r is Ok || chain_err(r),
            r matches Ok(w) ==> (old(self).push_ok(left, Kind::Bytes) ==> final(self).consistent()
                && final(self).pushed(*old(self), left, w)),
            final(self).settled(),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let ghost c = self.push_ok(left, Kind::Bytes);
        self.settle();
        let h = self.header_of(left);
        proof {
            self.lemma_checks_pass(left);
            reveal(Core::building_wf);
            if c {
                self.core.lemma_readable(left);
            }
        }
        self.checked_body(h, Kind::Bytes)?;
        let ghost s1 = *self;
        proof {
            self.core.lemma_shape(h@.addr);
        }
        let len = self.core.slot_get(h, 0).bits();
        let w = self.own_unchecked(left, len, Ghost(c));
        let ghost mid = *self;
        let hw = self.header_of(w);
        proof {
            assert(s1.text_of(left).subrange(0, len as int) =~= s1.text_of(left));
            assert(old(self).core.settled_view() == s1.core);
            if c {
                assert(mid.pushed(*old(self), left, w));
            }
        }
        self.write_byte_in_place(hw, i, b);
        proof {
            if c {
                self.lemma_pushed_text(mid, *old(self), left, w);
            }
            if M::CHECKED {
                assert forall|x: Word| s1.core.word_ok(x) implies #[trigger] self.core.word_ok(
                    x,
                ) by {
                    assert(mid.core.word_ok(x));
                }
            }
        }
        Ok(w)
    }
}

} // verus!

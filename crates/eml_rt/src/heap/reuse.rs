//! reuse の部品 (docs/spec/object-model.md の「reuse」と「検査モードの token」)。
//! token の箱は中身を消さない。一意の道で、同じ位置の同じフィールドを書かずに済ませる特殊化が、古いフィールドに頼る
//! ためである。

use vstd::prelude::*;

#[cfg(verus_only)]
use super::Core;
#[cfg(verus_only)]
use super::chain_err;
use super::check::reuse_in_place_check;
#[cfg(verus_only)]
use super::handover::{handover, kept_set};
#[cfg(verus_only)]
use super::inv::{p_count, p_first};
#[cfg(verus_only)]
use super::model::{BlockState, Model, Pos};
#[cfg(verus_only)]
use super::states::pos_set;
use super::{Heap, object_words};
use crate::error::HeapError;
use crate::meta::{Header, Meta};
#[cfg(verus_only)]
use crate::meta::{Kind, bits_kind, bits_scan};
use crate::mode::Mode;
use crate::word::Word;
#[cfg(verus_only)]
use crate::word::imm_value_spec;

verus! {

/// reuse の token。どちらのモードでも1語で、VM のレジスタと後のネイティブの値の大きさをモードで変えない。速いモードでは
/// 箱を指す語、検査モードでは token の表の添字の即値、空の token は `Word::EMPTY` である。
#[must_use]
#[derive(Debug, PartialEq, Eq)]
pub struct Token(Word);

impl Token {
    pub exec const EMPTY: Token
        ensures
            Self::EMPTY.word_spec().bits_spec() == 0,
    {
        Token(Word::EMPTY)
    }

    pub closed spec fn word_spec(&self) -> Word {
        self.0
    }

    #[inline]
    pub fn is_empty(&self) -> (r: bool)
        ensures
            r == (self.word_spec().bits_spec() == 0),
    {
        self.0.is_empty()
    }

    /// VM のレジスタに置くための語。
    #[inline]
    pub fn into_word(self) -> (w: Word)
        ensures
            w == self.word_spec(),
    {
        self.0
    }

    /// # Safety
    /// `w` は同じヒープの `into_word` が返した語である。
    #[inline]
    pub unsafe fn from_word(w: Word) -> (t: Token)
        ensures
            t.word_spec() == w,
    {
        Token(w)
    }
}

impl<M: Mode> Heap<M> {
    /// token が指す箱の番地。空の token なら `None`。検査モードでは token の表から引く。
    pub closed spec fn token_box(&self, t: Token) -> Option<usize> {
        let bits = t.word_spec().bits_spec();
        if bits == 0 {
            None
        } else if M::CHECKED {
            let i = imm_value_spec(bits) as int;
            if 0 <= i < self.tokens@.len() && self.tokens@[i] is Some {
                Some((self.tokens@[i]->0).0@.addr)
            } else {
                None
            }
        } else {
            Some(bits)
        }
    }

    /// token は空か、まだ使っていない token の箱を指す。
    pub closed spec fn token_ok(&self, t: Token) -> bool {
        match self.token_box(t) {
            None => t.word_spec().bits_spec() == 0,
            Some(b) => {
                &&& self.core.dom().contains(b)
                &&& self.core.state(b) == BlockState::Token
                &&& !M::CHECKED ==> t.word_spec().is_ptr_spec() && self.core.word_ok(t.word_spec())
                &&& M::CHECKED ==> t.word_spec().is_imm_spec()
            },
        }
    }

    /// token が空でなければ、その箱は `payload_words` 語のペイロードを持てる。
    pub closed spec fn token_fits(&self, t: Token, payload_words: usize) -> bool {
        self.token_box(t) matches Some(b) ==> self.core.cap(b) >= payload_words
    }

    /// `w` の箱は token の箱である。
    pub closed spec fn is_token_box(&self, w: Word) -> bool {
        self.core.dom().contains(w.bits_spec()) && self.core.state(w.bits_spec())
            == BlockState::Token
    }

    /// 構築中の物体を生きている物体にしても、token は使える。
    proof fn lemma_token_settled(&self, old: Heap<M>, t: Token)
        requires
            old.core.full(),
            old.token_ok(t),
            self.core == old.core.settled_view(),
            self.tokens == old.tokens,
        ensures
            self.token_ok(t),
    {
        reveal(Core::building_wf);
    }

    /// 生きている物体はどれも、同じ中身と数で生きている。
    pub closed spec fn kept_live(&self, old: Heap<M>) -> bool {
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.evolved(old.core.settled_view(), 0, 0)
    }

    /// `w` の物体を token の箱 `t` にした後の状態。子の参照は、残すフィールドとしてみな呼ぶ側に渡る。
    pub closed spec fn tokened(&self, old: Heap<M>, w: Word, t: Token) -> bool {
        let v = old.core.settled_view();
        let h = w.bits_spec();
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.token_box(t) == Some(h)
        &&& self.core.state(h) == BlockState::Token
        &&& self.core.evolved_by(v, handover(v.out_edges(h), h))
    }

    /// token の箱 `h` を構築中の物体 `w` にした後の状態。`meta` があればヘッダに書いた。前の物体の「残した」語のうち、
    /// 新しい `meta` の走査する語の位置にあるものは「残した」のまま残る。
    pub closed spec fn rebuilt(&self, old: Heap<M>, w: Word, meta: Option<Meta>) -> bool {
        let v = old.core.settled_view();
        let h = w.bits_spec();
        &&& self.wf()
        &&& self.consistent()
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& v.dom().contains(h)
        &&& v.state(h) == BlockState::Token
        &&& self.core.dom() == v.dom()
        &&& self.core.state(h) == BlockState::Building
        &&& self.core.model@.building == Some(h)
        &&& self.core.model@.written[h] == Set::<int>::empty()
        &&& self.core.meta_bits(h) == match meta {
            Some(m) => m.bits_spec(),
            None => v.meta_bits(h),
        }
        &&& self.core.rc_of(h) == 1
        &&& self.core.ext(h) == 1
        &&& self.core.perm(h).slots == v.perm(h).slots
        &&& self.core.perm(h).text == v.perm(h).text
        &&& self.core.model@.kept == v.model@.kept.filter(
            |e: Pos| e.0 != h || self.core.scanned(h, e.1),
        )
        &&& self.core.model@.edges == v.model@.edges
        &&& forall|b: usize| #[trigger]
            v.dom().contains(b) && b != h ==> {
                &&& self.core.perm(b) == v.perm(b)
                &&& self.core.state(b) == v.state(b)
                &&& self.core.block(b) == v.block(b)
                &&& self.core.cap(b) == v.cap(b)
                &&& self.core.model@.refs[b] == v.model@.refs[b]
                &&& self.core.model@.owner[b] == v.model@.owner[b]
            }
    }

    /// `into_token` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn into_token_ok(&self, w: Word) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.holds(w)
        &&& self.kind_of(w) == Kind::Data
        &&& self.rc_value(w) == 1
        &&& self.owned(w)
    }

    /// 一意な物体の箱を、フィールドに触れずに token にする。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側がその唯一の参照を持ち (rc が 1)、子の参照はもう処理してある。
    /// 種類は token にできる (`Data`)。持ち主は今の worker である。
    pub unsafe fn into_token(&mut self, w: Word) -> (r: Result<Token, HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            w.is_ptr_spec(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).into_token_ok(w),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(t) ==> final(self).token_box(t) == Some(w.bits_spec())
                && final(self).is_token_box(w),
            old(self).into_token_ok(w) ==> r is Ok,
            r matches Ok(t) ==> (old(self).into_token_ok(w) ==> final(self).consistent()
                && final(self).tokened(*old(self), w, t)),
    {
        let ghost c = self.into_token_ok(w);
        self.settle();
        let h = self.header_of(w);
        proof {
            if c {
                self.core.lemma_readable(w);
                self.lemma_checks_pass(w);
            }
        }
        if M::CHECKED {
            self.checked_live(h)?;
            let rc = self.core.rc_get(h);
            self.checked_owner(h, rc)?;
            if !self.core.meta_get(h).kind().tokenable() {
                return Err(HeapError::NotTokenable);
            }
            if rc != 1 {
                return Err(HeapError::NotUnique);
            }
        }
        let ghost s0 = self.core;
        let ghost a = h@.addr;
        let ghost first = p_first(s0.perm(a));
        let ghost n = p_count(s0.perm(a));
        let ghost ps = s0.model@.edges.dom().filter(|e: Pos| e.0 == a);
        proof {
            s0.lemma_shape(a);
            if M::CHECKED {
                reveal(Core::building_wf);
            }
            if c {
                s0.lemma_last_ref(a);
                assert(s0.model@.edges.restrict(ps) == s0.out_edges(a));
            }
        }
        self.core.begin_free(h, Ghost(ps), M::CHECKED, Ghost(c));
        proof {
            if c {
                self.core.lemma_begin_evolved(s0, a, ps);
            }
            vstd::set_lib::lemma_int_range(first, first + n);
        }
        let ghost s1 = self.core;
        self.core.finish_token(
            h,
            Ghost(vstd::set_lib::set_int_range(first, first + n)),
            M::CHECKED,
            Ghost(c),
        );
        proof {
            if c {
                self.core.lemma_after_token(s0, s1, a);
                self.core.lemma_by_trans(s1, s0, handover(s0.out_edges(a), a), 0);
                self.lemma_dummy_by(*old(self), handover(s0.out_edges(a), a));
            }
            if M::CHECKED {
                self.lemma_safe_frame(*old(self));
            }
        }
        if M::CHECKED {
            return Ok(self.checked_push_token(h));
        }
        Ok(Token(w))
    }

    /// token の箱を取り出す。検査モードでは、表の項目、箱の状態、世代の番号を確かめ、項目を空ける。
    fn take_token(&mut self, token: Token) -> (r: Result<Option<*mut Header>, HeapError>)
        requires
            old(self).wf(),
            !M::CHECKED ==> old(self).token_ok(token),
        ensures
            r is Err ==> *final(self) == *old(self),
            !M::CHECKED ==> *final(self) == *old(self),
            final(self).taken(*old(self)),
            !M::CHECKED ==> r is Ok,
            old(self).token_ok(token) ==> r is Ok,
            token.word_spec().bits_spec() == 0 ==> r == Ok::<Option<*mut Header>, HeapError>(None),
            token.word_spec().bits_spec() != 0 ==> (r is Ok ==> r matches Ok(Some(_))),
            r matches Ok(Some(h)) ==> {
                &&& final(self).core.dom().contains(h@.addr)
                &&& h == final(self).core.block(h@.addr)
                &&& final(self).core.state(h@.addr) == BlockState::Token
                &&& old(self).token_box(token) == Some(h@.addr)
                &&& M::CHECKED ==> !final(self).core.registered(h@.addr)
            },
    {
        if token.is_empty() {
            proof {
                self.lemma_taken_refl();
            }
            return Ok(None);
        }
        if !M::CHECKED {
            let h = self.header_of(token.0);
            proof {
                self.lemma_taken_refl();
            }
            return Ok(Some(h));
        }
        self.checked_take_token(token)
    }

    /// `old` から、token の表の項目を空けたほかは変わらない。
    pub closed spec fn taken(&self, old: Heap<M>) -> bool {
        &&& self.core.pages == old.core.pages
        &&& self.core.perms == old.core.perms
        &&& self.core.checked == old.core.checked
        &&& self.core.model@ == (Model { tokens: self.core.model@.tokens, ..old.core.model@ })
        &&& self.tokens@ == self.core.model@.tokens
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.wf()
        &&& old.consistent() ==> self.consistent()
    }

    proof fn lemma_taken_refl(&self)
        requires
            self.wf(),
        ensures
            self.taken(*self),
    {
    }

    /// 検査モードの token の取り出し。token の表を引き、前置きの状態と世代の番号を項目と照らし、項目を空ける。表の
    /// 項目があればその箱は token の状態で世代の番号も等しいので (`Core::safe_tokens_wf`)、照らし合わせは
    /// 断らない。断るのは、空けた項目と、表の外の添字である。
    fn checked_take_token(&mut self, token: Token) -> (r: Result<Option<*mut Header>, HeapError>)
        requires
            M::CHECKED,
            old(self).wf(),
            token.word_spec().bits_spec() != 0,
        ensures
            r is Err ==> *final(self) == *old(self),
            final(self).taken(*old(self)),
            old(self).token_ok(token) ==> r is Ok,
            r matches Ok(o) ==> o matches Some(h) && {
                &&& final(self).core.dom().contains(h@.addr)
                &&& h == final(self).core.block(h@.addr)
                &&& final(self).core.state(h@.addr) == BlockState::Token
                &&& old(self).token_box(token) == Some(h@.addr)
                &&& !final(self).core.registered(h@.addr)
            },
    {
        proof {
            self.lemma_taken_refl();
        }
        // 即値でない語は表の添字でないので、`imm_value` の前に断る
        if !token.0.is_imm() {
            return Err(HeapError::StaleToken);
        }
        let v = token.0.imm_value();
        if v < 0 {
            return Err(HeapError::StaleToken);
        }
        let index = v as usize;
        assert(index as int == v as int);
        if index >= self.tokens.len() {
            return Err(HeapError::StaleToken);
        }
        let (h, generation) = match self.tokens[index] {
            Some(e) => e,
            None => return Err(HeapError::StaleToken),
        };
        let ghost s0 = self.core;
        proof {
            reveal(Core::safe_tokens_wf);
            assert(s0.model@.tokens[index as int] == self.tokens@[index as int]);
            assert(s0.token_entry((h, generation)));
            s0.lemma_code_state(h@.addr);
        }
        let bits = self.core.prefix_state_get(h);
        if bits & 3 != 2 || bits >> 2u32 != generation {
            return Err(HeapError::StaleToken);
        }
        self.tokens.set(index, None);
        proof {
            self.core.model = Ghost(
                Model { tokens: s0.model@.tokens.update(index as int, None), ..s0.model@ },
            );
            self.core.lemma_token_taken(s0, index as int);
            self.core.lemma_tokens_frame(s0);
            assert(self.tokens@ =~= self.core.model@.tokens);
        }
        Ok(Some(h))
    }

    /// 検査モードで token の箱を token の表に積む。前置きは `finish_token` がもう token にしてある。
    fn checked_push_token(&mut self, h: *mut Header) -> (t: Token)
        requires
            M::CHECKED,
            old(self).wf(),
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Token,
            !old(self).core.registered(h@.addr),
        ensures
            final(self).wf(),
            old(self).consistent() ==> final(self).consistent(),
            final(self).core.pages == old(self).core.pages,
            final(self).core.perms == old(self).core.perms,
            final(self).core.checked == old(self).core.checked,
            final(self).core.model@ == (Model {
                tokens: final(self).core.model@.tokens,
                ..old(self).core.model@
            }),
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
            final(self).token_box(t) == Some(h@.addr),
    {
        let ghost s0 = self.core;
        let bits = self.core.prefix_state_get(h);
        let generation = bits >> 2u32;
        // 添字は使い回さない。使った項目を空のまま残して、古い token を確実に断るためである
        let t = Token(Word::imm_index(self.tokens.len()));
        self.tokens.push(Some((h, generation)));
        proof {
            self.core.model = Ghost(
                Model { tokens: s0.model@.tokens.push(Some((h, generation))), ..s0.model@ },
            );
            self.core.lemma_token_pushed(s0, h, generation);
            self.core.lemma_tokens_frame(s0);
            assert(self.tokens@ =~= self.core.model@.tokens);
        }
        t
    }

    /// `con_at` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn con_at_ok(&self, token: Token, payload_words: usize) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.token_ok(token)
        &&& self.token_fits(token, payload_words)
    }

    /// token が空でなければ、その箱に `meta` を書いて返す。空なら確保する。箱を使ったかも返す。フィールドは呼ぶ側が書く。
    /// 検査モードの級の検査 (`WrongClass`) は、モデルの外の確保器の級を見るので、モデルが合っていても返りうる。
    ///
    /// # Safety
    /// `token` はこのヒープの token で、まだ使っていない。空でなければ、その箱は `payload_words` の物体のこのモードでの
    /// 級にある。同じ物理的な語の数の物体から作った token なら、どちらのモードでも満たす。`meta` の種類は `Foreign`、
    /// `String`、`Free` のどれでもない。呼ぶ側は、ほかのヒープの操作の前に、走査する語がみな `meta` に合う値を持つようにする。
    /// 箱を使い回したときは、残したフィールドを書き直さなくてよい。
    #[inline]
    pub unsafe fn con_at(&mut self, token: Token, payload_words: usize, meta: Meta) -> (r: Result<
        (Word, bool),
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            meta.kind_spec() != Kind::Foreign && meta.kind_spec() != Kind::String
                && meta.kind_spec() != Kind::Free,
            meta.scan_spec() != 255,
            meta.scan_spec() <= payload_words,
            !M::CHECKED ==> old(self).con_at_ok(token, payload_words),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            r matches Ok((w, _)) ==> w.is_ptr_spec() && final(self).word_in(w),
            old(self).con_at_ok(token, payload_words) ==> r is Ok || r == Err::<
                (Word, bool),
                HeapError,
            >(HeapError::WrongClass),
            old(self).con_at_ok(token, payload_words) ==> (r matches Ok((w, reused)) ==> {
                &&& reused ==> final(self).rebuilt(*old(self), w, Some(meta))
                &&& !reused ==> final(self).built(*old(self), w, meta, 1, payload_words)
            }),
    {
        let ghost c = self.con_at_ok(token, payload_words);
        let ghost h0 = *self;
        self.settle();
        proof {
            if c {
                self.lemma_token_settled(h0, token);
            }
        }
        let ghost s0 = *self;
        let h = match self.take_token(token)? {
            None => {
                let w = unsafe { self.alloc(payload_words, meta) };
                return Ok((w, false));
            },
            Some(h) => h,
        };
        let ghost s1 = *self;
        if M::CHECKED {
            proof {
                self.core.lemma_shape(h@.addr);
            }
            if !self.core.pages.fits(h, object_words(payload_words)) {
                return Err(HeapError::WrongClass);
            }
        }
        proof {
            self.core.lemma_shape(h@.addr);
            if c {
                self.core.lemma_tokens_frame(s0.core);
            }
        }
        self.core.token_build(h, Some(meta), self.worker, M::CHECKED, Ghost(c));
        let w = Word::from_ptr(h);
        proof {
            crate::word::lemma_aligned_is_ptr(h@.addr);
            if c {
                self.lemma_rebuilt(s0, h, w, Some(meta));
            }
            if M::CHECKED {
                self.lemma_safe_frame(s1);
            }
        }
        Ok((w, true))
    }

    /// `reuse_in_place` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn reuse_ok(&self, token: Token) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.token_ok(token)
    }

    /// 同じコンストラクタの reuse。ヘッダに何も書かずに箱を返す。`meta` は検査モードが照らすためだけに渡す。
    ///
    /// 空の token を受けると、速いモードでも `StaleToken` を返す。
    ///
    /// # Safety
    /// `token` はこのヒープの空でない token で、まだ使っていない。`meta` の tag、scan、種類は箱と同じで、箱が `lin`
    /// ビットを持つなら `meta` も持つ。
    pub unsafe fn reuse_in_place(&mut self, token: Token, meta: Meta) -> (r: Result<
        Word,
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).reuse_ok(token),
        ensures
            final(self).wf(),
            token.word_spec().bits_spec() == 0 ==> r == Err::<Word, HeapError>(
                HeapError::StaleToken,
            ),
            !M::CHECKED && token.word_spec().bits_spec() != 0 ==> r is Ok,
            r matches Ok(w) ==> w.is_ptr_spec() && final(self).word_in(w),
            old(self).reuse_ok(token) ==> (r matches Ok(w) ==> final(self).rebuilt(
                *old(self),
                w,
                None,
            )),
    {
        let ghost c = self.reuse_ok(token);
        let ghost h0 = *self;
        self.settle();
        proof {
            if c {
                self.lemma_token_settled(h0, token);
            }
        }
        let ghost s0 = *self;
        let h = match self.take_token(token)? {
            None => return Err(HeapError::StaleToken),
            Some(h) => h,
        };
        let ghost s1 = *self;
        proof {
            self.core.lemma_shape(h@.addr);
            if c {
                self.core.lemma_tokens_frame(s0.core);
            }
        }
        if M::CHECKED {
            reuse_in_place_check(self.core.meta_get(h), meta)?;
        }
        self.core.token_build(h, None, self.worker, M::CHECKED, Ghost(c));
        let w = Word::from_ptr(h);
        proof {
            crate::word::lemma_aligned_is_ptr(h@.addr);
            if c {
                self.lemma_rebuilt(s0, h, w, None);
            }
            if M::CHECKED {
                self.lemma_safe_frame(s1);
            }
        }
        Ok(w)
    }

    /// `con_at` と `reuse_in_place` の後の状態。
    proof fn lemma_rebuilt(&self, s0: Heap<M>, h: *mut Header, w: Word, meta: Option<Meta>)
        requires
            self.tokens@ == self.core.model@.tokens,
            s0.wf(),
            s0.consistent(),
            s0.core.model@.building is None,
            s0.core.dom().contains(h@.addr),
            h == s0.core.block(h@.addr),
            s0.core.state(h@.addr) == BlockState::Token,
            self.dummy == s0.dummy,
            self.worker == s0.worker,
            self.core.full(),
            M::CHECKED ==> self.core.safe_wf(),
            self.core.checked == s0.core.checked,
            self.core.pages == s0.core.pages,
            self.core.dom() == s0.core.dom(),
            forall|b: usize| b != h@.addr ==> #[trigger] self.core.perms@[b] == s0.core.perms@[b],
            self.core.perm(h@.addr).rc == s0.core.perm(h@.addr).rc,
            self.core.perm(h@.addr).slots == s0.core.perm(h@.addr).slots,
            self.core.perm(h@.addr).text == s0.core.perm(h@.addr).text,
            self.core.meta_bits(h@.addr) == match meta {
                Some(m) => m.bits_spec(),
                None => s0.core.meta_bits(h@.addr),
            },
            self.core.state(h@.addr) == BlockState::Building,
            forall|b: usize| b != h@.addr ==> #[trigger] self.core.state(b) == s0.core.state(b),
            self.core.model@.building == Some(h@.addr),
            self.core.model@.written[h@.addr] == Set::<int>::empty(),
            self.core.model@.edges == s0.core.model@.edges,
            self.core.model@.kept == s0.core.model@.kept.filter(
                |e: Pos| e.0 != h@.addr || self.core.scanned(h@.addr, e.1),
            ),
            self.core.model@.refs == s0.core.model@.refs.insert(h@.addr, 1),
            forall|b: usize|
                b != h@.addr ==> #[trigger] self.core.model@.owner[b] == s0.core.model@.owner[b],
            self.core.rc_of(h@.addr) == 1,
            self.core.indeg(h@.addr) == 0,
            w.bits_spec() == h@.addr,
            w.ptr_spec()@.provenance == h@.provenance,
            w.ptr_spec()@.addr == h@.addr,
        ensures
            self.rebuilt(s0, w, meta),
    {
        reveal(Core::building_wf);
        self.core.lemma_at(h@.addr);
        crate::word::lemma_aligned_is_ptr(h@.addr);
        // 構築中の物体のほかに空ける途中の物体はなく、ダミーの物体は変わらない
        assert(s0.dummy.bits_spec() != h@.addr);
        assert forall|b: usize| #[trigger] self.core.dom().contains(b) implies self.core.state(b)
            != BlockState::Dying by {
            assert(s0.core.dom().contains(b));
        }
        assert(self.core.perms@[s0.dummy.bits_spec()] == s0.core.perms@[s0.dummy.bits_spec()]);
    }

    /// `drop_token` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn drop_token_ok(&self, token: Token) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.token_ok(token)
    }

    /// 使わなかった token の箱を空ける。
    ///
    /// # Safety
    /// `token` はこのヒープの token で、まだ使っていない。
    pub unsafe fn drop_token(&mut self, token: Token) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).drop_token_ok(token),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            old(self).drop_token_ok(token) ==> r is Ok && final(self).consistent()
                && final(self).kept_live(*old(self)),
    {
        let ghost c = self.drop_token_ok(token);
        let ghost h0 = *self;
        self.settle();
        proof {
            if c {
                self.lemma_token_settled(h0, token);
            }
        }
        let ghost mid = *self;
        if let Some(h) = self.take_token(token)? {
            let ghost s1 = self.core;
            let ghost t1 = *self;
            proof {
                s1.lemma_shape(h@.addr);
            }
            self.core.token_free(h, M::CHECKED, Ghost(c));
            let ghost s2 = self.core;
            self.free_box(h, Ghost(c));
            proof {
                if c {
                    self.core.lemma_token_dropped(s1, s2, h@.addr);
                }
                if M::CHECKED {
                    self.lemma_safe_frame(t1);
                }
            }
        } else {
            proof {
                if c {
                    self.core.lemma_evolved_self(0);
                }
            }
        }
        proof {
            if c {
                self.core.lemma_frame_evolved(mid.core, old(self).core, 0, 0);
                self.lemma_dummy_evolved(*old(self), 0, 0);
            }
        }
        Ok(())
    }

    /// 一意なら、残さない子を手放して箱を token にする。そうでなければ `release` と同じにして、空の token を返す。
    ///
    /// # Safety
    /// `release` と同じ。
    #[inline]
    pub unsafe fn release_reuse(&mut self, w: Word, tag: u16, keep: &[bool]) -> (r: Result<
        Token,
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            w.is_ptr_spec(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).release_ok(w, tag, keep@),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(t) ==> (t.word_spec().bits_spec() != 0 ==> final(self).token_box(t)
                == Some(w.bits_spec()) && final(self).is_token_box(w)),
            old(self).release_ok(w, tag, keep@) ==> r is Ok || chain_err(r),
            old(self).release_ok(w, tag, keep@) && r is Ok ==> final(self).consistent()
                && final(self).handed_over(*old(self), w, keep@),
    {
        let ghost c = self.release_ok(w, tag, keep@);
        let unique = unsafe { self.is_unique(w) };
        if unique? {
            self.settle();
            let h = self.header_of(w);
            proof {
                if c {
                    self.core.lemma_readable(w);
                    self.lemma_checks_pass(w);
                    if M::CHECKED {
                        self.core.lemma_code_state(h@.addr);
                    }
                }
            }
            self.release_check(h, tag, keep.len())?;
            proof {
                if M::CHECKED {
                    reveal(Core::building_wf);
                }
                if c {
                    self.core.lemma_last_ref(h@.addr);
                    assert(self.core.perm(h@.addr) == old(self).core.perm(h@.addr));
                    assert(self.core.count(h@.addr) == old(self).core.count(h@.addr));
                }
            }
            self.release_children(h, keep, true, Ghost(c))?;
            if M::CHECKED {
                return Ok(self.checked_push_token(h));
            }
            return Ok(Token(w));
        }
        let r = unsafe { self.release(w, tag, keep) };
        r?;
        Ok(Token::EMPTY)
    }
}

} // verus!

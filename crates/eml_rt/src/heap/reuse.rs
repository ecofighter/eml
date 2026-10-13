//! reuse の部品 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「reuse」と「検査モードの token」)。
//! token の箱は中身を消さない。一意の道で、同じ位置の同じフィールドを書かずに済ませる特殊化が、古いフィールドに頼る
//! ためである。

use std::ptr::NonNull;

use super::check::{self, State};
use super::{Heap, object_words};
use crate::error::HeapError;
use crate::meta::{self, Header, Kind, Meta};
use crate::mode::Mode;
use crate::sync::Ordering;
use crate::word::Word;

/// reuse の token。どちらのモードでも1語で、VM のレジスタと後のネイティブの値の大きさをモードで変えない。速いモードでは
/// 箱を指す語、検査モードでは token の表の添字の即値、空の token は `Word::EMPTY` である。
#[must_use]
#[derive(Debug, PartialEq, Eq)]
pub struct Token(Word);

impl Token {
    pub const EMPTY: Token = Token(Word::EMPTY);

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// VM のレジスタに置くための語。
    pub fn into_word(self) -> Word {
        self.0
    }

    /// # Safety
    /// `w` は同じヒープの `into_word` が返した語である。
    pub unsafe fn from_word(w: Word) -> Token {
        Token(w)
    }
}

impl<M: Mode> Heap<M> {
    /// 一意な物体の箱を、フィールドに触れずに token にする。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側がその唯一の参照を持ち、子の参照はもう処理してある。
    pub unsafe fn into_token(&mut self, w: Word) -> Result<Token, HeapError> {
        if !M::CHECKED {
            return Ok(Token(w));
        }
        let h = w.header();
        unsafe { self.check_live(h)? };
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        unsafe { self.check_owner(h, rc)? };
        if !unsafe { meta::meta(h) }.kind().tokenable() {
            return Err(HeapError::NotTokenable);
        }
        if rc != 1 {
            return Err(HeapError::NotUnique);
        }
        let prefix = unsafe { check::prefix(h) };
        prefix.set_state(State::Token, true);
        // 添字は使い回さない。使った項目を空のまま残して、古い token を確実に断るためである
        self.tokens.push(Some((h, prefix.generation())));
        Ok(Token(Word::imm((self.tokens.len() - 1) as i64)))
    }

    /// token の箱を取り出す。検査モードでは、表の項目、箱の状態、世代の番号を確かめ、項目を空ける。
    unsafe fn take_token(&mut self, token: Token) -> Result<Option<NonNull<Header>>, HeapError> {
        if token.is_empty() {
            return Ok(None);
        }
        if !M::CHECKED {
            return Ok(Some(token.0.header()));
        }
        let index = usize::try_from(token.0.imm_value()).map_err(|_| HeapError::StaleToken)?;
        let (h, generation) = self
            .tokens
            .get_mut(index)
            .and_then(Option::take)
            .ok_or(HeapError::StaleToken)?;
        let prefix = unsafe { check::prefix(h) };
        if prefix.state() != State::Token || prefix.generation() != generation {
            return Err(HeapError::StaleToken);
        }
        Ok(Some(h))
    }

    /// token が空でなければ、その箱に `meta` を書いて返す。空なら確保する。箱を使ったかも返す。フィールドは呼ぶ側が書く。
    ///
    /// # Safety
    /// `token` はこのヒープの token で、まだ使っていない。`meta` の種類は `Foreign` でも `Free` でもない。呼ぶ側は、
    /// ほかのヒープの操作の前に、走査する語がみな `meta` に合う値を持つようにする。箱を使い回したときは、残した
    /// フィールドを書き直さなくてよい。
    pub unsafe fn con_at(
        &mut self,
        token: Token,
        payload_words: usize,
        meta: Meta,
    ) -> Result<(Word, bool), HeapError> {
        debug_assert!(!matches!(meta.kind(), Kind::Foreign | Kind::Free));
        let Some(h) = (unsafe { self.take_token(token)? }) else {
            return Ok((unsafe { self.alloc(payload_words, meta) }, false));
        };
        if M::CHECKED {
            if !unsafe { self.pages.fits(h, object_words(payload_words)) } {
                return Err(HeapError::WrongClass);
            }
            unsafe { check::prefix(h) }.set_state(State::Alive, false);
        }
        unsafe { meta::set_meta(h, meta) };
        Ok((Word::from_header(h), true))
    }

    /// 同じコンストラクタの reuse。ヘッダに何も書かずに箱を返す。`meta` は検査モードが照らすためだけに渡す。
    ///
    /// # Safety
    /// `token` はこのヒープの空でない token で、まだ使っていない。
    pub unsafe fn reuse_in_place(&mut self, token: Token, meta: Meta) -> Result<Word, HeapError> {
        let h = unsafe { self.take_token(token)? }.ok_or(HeapError::StaleToken)?;
        if M::CHECKED {
            check::reuse_in_place_check(unsafe { meta::meta(h) }, meta)?;
            unsafe { check::prefix(h) }.set_state(State::Alive, false);
        }
        Ok(Word::from_header(h))
    }

    /// 使わなかった token の箱を空ける。
    ///
    /// # Safety
    /// `token` はこのヒープの token で、まだ使っていない。
    pub unsafe fn drop_token(&mut self, token: Token) -> Result<(), HeapError> {
        if let Some(h) = unsafe { self.take_token(token)? } {
            unsafe { self.free_box(h) };
        }
        Ok(())
    }

    /// 一意なら、残さない子を手放して箱を token にする。そうでなければ `release` と同じにして、空の token を返す。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側がその参照を1つ持っている。
    pub unsafe fn release_reuse(
        &mut self,
        w: Word,
        tag: u16,
        keep: &[bool],
    ) -> Result<Token, HeapError> {
        if unsafe { self.is_unique(w)? } {
            let h = w.header();
            unsafe {
                self.check_release(h, tag, keep.len())?;
                self.release_children(h, keep)?;
                self.into_token(w)
            }
        } else {
            unsafe { self.release(w, tag, keep)? };
            Ok(Token::EMPTY)
        }
    }
}

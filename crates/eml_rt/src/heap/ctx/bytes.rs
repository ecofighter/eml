//! バイト列の extern (docs/spec/object-model.md の「extern」)。`Std.Bytes` の行の結果を、ghost のバイト列に対して
//! 証明する。バイト列の物体を読み書きする部品はヒープの `string` にあり、文字列と同じ道を種類を引数にして使う。

#[cfg(verus_only)]
use vstd::utf8::valid_utf8;

#[cfg(verus_only)]
use super::super::Core;
use super::positions::{find_byte, find_bytes};
#[cfg(verus_only)]
use super::positions::{found_byte, found_from};
#[cfg(verus_only)]
use super::proof::*;
#[cfg(verus_only)]
use super::strings::{lemma_text_trans, lex};
use super::*;
use crate::alloc::over_budget;
use crate::externs::{FALSE, Fault, IndexFault, TRUE};

verus! {

/// `Bytes.make` が作れる最も長いバイト列。ペイロードの語の数が `u32` に収まる長さで、配列の要素の数の上限と同じ
/// 語の数である (docs/spec/object-model.md の「配列」)。
const MAX_BYTES: usize = 0x7_FFF8_0000;

/// `b` が妥当な UTF-8 なら `None` を、そうでなければ確かめが止まった位置 (`Utf8Error::valid_up_to`) を返す。
/// `core::str::from_utf8` の結果が vstd の `valid_utf8` に一致することと、止まった位置が長さより前であることは信頼する
/// (信頼の一覧の T5)。vstd は検査付きの `from_utf8` に約束を持たないためである。位置は誤りの文言にだけ使う。
#[verifier::external_body]
#[inline]
pub(crate) fn utf8_valid(b: &[u8]) -> (r: Option<usize>)
    ensures
        r is None <==> valid_utf8(b@),
        r matches Some(at) ==> at < b@.len(),
{
    core::str::from_utf8(b).err().map(|e| e.valid_up_to())
}

/// `n` 個の `b` の列。
pub open spec fn repeated(n: nat, b: u8) -> Seq<u8> {
    Seq::new(n, |_i: int| b)
}

impl<M: Mode> Ctx<M> {
    /// `Bytes.length`。`s` を借りて、バイトの数を返す。呼ぶ側の参照の数は変わらない。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`s` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ
    /// 持つ。速いモードでは、`s` はバイト列である (検査モードは確かめ、違えば `WrongKind` を返す)。
    pub unsafe fn bytes_length(&mut self, s: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_bytes(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(old(self).text_of(s).len() as i64)
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_bytes(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_body(h1, s, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let len = self.heap.body_of(s).len() as i64;
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_lent(s0, h1);
            }
        }
        Ok(len)
    }

    /// `Bytes.get`。`s` を借りて、位置 `i` のバイトを返す。範囲の外なら `Fault::Index` を返す。呼ぶ側の参照の数は
    /// 変わらない。
    ///
    /// # Safety
    /// `bytes_length` と同じ。
    pub unsafe fn bytes_get(&mut self, i: i64, s: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_bytes(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) && !(r matches Err(
                Fault::Heap(_),
            )) ==> {
                &&& 0 <= i < old(self).text_of(s).len() ==> r == Ok::<i64, Fault>(
                    old(self).text_of(s)[i as int] as i64,
                )
                &&& !(0 <= i < old(self).text_of(s).len()) ==> (r matches Err(Fault::Index(_)))
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_bytes(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_body(h1, s, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let r = {
            let bytes = self.heap.body_of(s);
            let len = bytes.len();
            if 0 <= i && (i as u64) < (len as u64) {
                Ok(bytes[i as usize] as i64)
            } else {
                Err(Fault::Index(Box::new(IndexFault::BytesIndex { index: i, len: len as i64 })))
            }
        };
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_lent(s0, h1);
            }
        }
        r
    }

    /// `s` を借りて、中身が妥当な UTF-8 かの `Bool` のタグを返す。確かめは T5 の `utf8_valid` がする。呼ぶ側の参照の
    /// 数は変わらない。
    ///
    /// # Safety
    /// `bytes_length` と同じ。
    pub unsafe fn bytes_valid_utf8(&mut self, s: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_bytes(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(
                    if valid_utf8(old(self).text_of(s)) {
                        TRUE
                    } else {
                        FALSE
                    },
                )
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_bytes(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_body(h1, s, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let valid = utf8_valid(self.heap.body_of(s)).is_none();
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_lent(s0, h1);
            }
        }
        Ok(
            if valid {
                TRUE
            } else {
                FALSE
            },
        )
    }

    /// `Eq Bytes` の `==`。両辺を借りる。呼ぶ側の参照の数は変わらない。
    ///
    /// # Safety
    /// `string_eq` と同じで、速いモードでは両辺がバイト列である。
    pub unsafe fn bytes_eq(&mut self, a: Word, b: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).lends(a, b, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).lends(a, b, Kind::Bytes) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends(a, b, Kind::Bytes) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(
                    if old(self).text_of(a) == old(self).text_of(b) {
                        TRUE
                    } else {
                        FALSE
                    },
                )
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        self.compare(a, b, false, Kind::Bytes)
    }

    /// `Eq Bytes` の `!=`。`bytes_eq` と同じ約束で、結果を逆にする。
    ///
    /// # Safety
    /// `bytes_eq` と同じ。
    pub unsafe fn bytes_ne(&mut self, a: Word, b: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).lends(a, b, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).lends(a, b, Kind::Bytes) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends(a, b, Kind::Bytes) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(
                    if old(self).text_of(a) == old(self).text_of(b) {
                        FALSE
                    } else {
                        TRUE
                    },
                )
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let eq = self.compare(a, b, false, Kind::Bytes)?;
        Ok(TRUE - eq)
    }

    /// `Ord Bytes` の `compare`。`Ordering` のタグを返す。順序はバイトの辞書式の順序 `lex` で、`String` と同じである。
    ///
    /// # Safety
    /// `bytes_eq` と同じ。
    pub unsafe fn bytes_compare(&mut self, a: Word, b: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).lends(a, b, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).lends(a, b, Kind::Bytes) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends(a, b, Kind::Bytes) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(lex(old(self).text_of(a), old(self).text_of(b)))
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        self.compare(a, b, true, Kind::Bytes)
    }

    /// `Bytes.slice`。`s` を借りて、位置 `start` から `end` の手前までの部分を写した新しいバイト列を返す。範囲が
    /// `0 <= start <= end <= len` でなければ、何も作らずに `Fault::Index` を返す。呼ぶ側の参照の数は、どちらでも
    /// 変わらない。
    ///
    /// # Safety
    /// `bytes_length` と同じ。
    pub unsafe fn bytes_slice(&mut self, start: i64, end: i64, s: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_bytes(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> {
                let text = old(self).text_of(s);
                &&& 0 <= start <= end <= text.len()
                &&& final(self).is_bytes(w)
                &&& final(self).text_of(w) == text.subrange(start as int, end as int)
            },
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) ==> {
                let text = old(self).text_of(s);
                &&& 0 <= start <= end <= text.len() ==> !(r matches Err(Fault::Index(_)))
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
                }
                &&& r matches Err(Fault::Index(_)) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
                }
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_bytes(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_body(h1, s, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let ghost text = h1.text_of(s);
        // 範囲は確保より前に確かめる。誤りなら何も作らない
        let len = self.heap.body_of(s).len();
        if !(0 <= start && start <= end && (end as u64) <= (len as u64)) {
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, h1, s0.heap);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_lent(s0, h1);
                }
            }
            return Err(
                Fault::Index(Box::new(IndexFault::BytesSlice { start, end, len: len as i64 })),
            );
        }
        let (from, to) = (start as usize, end as usize);
        let w = self.heap.alloc_bytes_room(to - from);
        let ghost h2 = self.heap;
        proof {
            h2.lemma_text_spec(w);
            self.lemma_room_made(h1, s, w, Kind::Bytes);
        }
        self.heap.append_body(w, s, from, to);
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                (to - from) as u64,
            );
        }
        let ghost h3 = self.heap;
        proof {
            assert(h3.text_of(w) =~= text.subrange(start as int, end as int));
            h3.lemma_text_spec(s);
            h3.lemma_text_spec(w);
            if c {
                s0.lemma_text_shown(h1, h2, h3, s, w, Kind::Bytes);
            }
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                lemma_words_trans(h2, h1, s0.heap);
                assert(h3.words_kept(h2));
                lemma_words_trans(h3, h2, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_shown_lent(s0, h1, h2, h3, s, w, Kind::Bytes);
            }
        }
        Ok(w)
    }

    /// `Bytes.empty`。長さ 0 の新しいバイト列を返す。確保量の上限は `bytes_make` と同じ道で確かめる。
    pub fn bytes_empty(&mut self) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || r == Err::<Word, Fault>(Fault::Stopped(Stop::MemoryLimit)),
            r is Err ==> *final(self) == *old(self),
            r matches Ok(w) ==> final(self).is_bytes(w) && final(self).text_of(w).len() == 0,
            r matches Ok(w) ==> (old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            }),
    {
        unsafe { self.bytes_make(0, 0) }
    }

    /// `Bytes.find_from`。`needle` と `s` を借りて、`from` 以上で `needle` が始まる最小の位置を返し、なければ -1 を返す。
    /// `from` が `0 <= from <= len` でなければ `Fault::Index` を返す。
    ///
    /// # Safety
    /// `bytes_eq` と同じ (`a` が `needle`、`b` が `s`)。
    pub unsafe fn bytes_find_from(&mut self, needle: Word, from: i64, s: Word) -> (r: Result<
        i64,
        Fault,
    >)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(needle),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).lends(needle, s, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).lends(needle, s, Kind::Bytes) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends(needle, s, Kind::Bytes) && !(r matches Err(
                Fault::Heap(_),
            )) ==> {
                let text = old(self).text_of(s);
                &&& 0 <= from <= text.len() ==> (r matches Ok(v) && found_from(
                    text,
                    old(self).text_of(needle),
                    from as int,
                    v,
                ))
                &&& !(0 <= from <= text.len()) ==> (r matches Err(Fault::Index(_)))
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.lends(needle, s, Kind::Bytes);
        if M::CHECKED && !(needle.is_ptr() && s.is_ptr()) {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let ha = self.heap.header_of(needle);
        let hb = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(needle);
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(needle);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_lent(h1, needle, s, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(ha, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        if let Err(e) = self.heap.checked_body(hb, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let r = {
            let n = self.heap.body_of(needle);
            let t = self.heap.body_of(s);
            let len = t.len();
            if 0 <= from && (from as u64) <= (len as u64) {
                // ヒープのバイト列は `isize::MAX` より短いので、ここには来ない
                if (len as u64) > (i64::MAX as u64) {
                    crate::alloc::too_large();
                }
                Ok(find_bytes(t, n, from as usize))
            } else {
                Err(Fault::Index(Box::new(IndexFault::BytesIndex { index: from, len: len as i64 })))
            }
        };
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_lent(s0, h1);
            }
        }
        r
    }

    /// `Bytes.find_byte_from`。`s` を借りて、`from` 以上で `b` がある最小の位置を返し、なければ -1 を返す。探すのは
    /// T13 の `find_byte` である。`from` が `0 <= from <= len` でなければ `Fault::Index` を返す。
    ///
    /// # Safety
    /// `bytes_length` と同じ。
    pub unsafe fn bytes_find_byte(&mut self, b: i64, from: i64, s: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            0 <= b < 256,
            !M::CHECKED ==> old(self).is_bytes(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_bytes(s) && old(self).holds(s) && !(r matches Err(
                Fault::Heap(_),
            )) ==> {
                let text = old(self).text_of(s);
                &&& 0 <= from <= text.len() ==> (r matches Ok(v) && found_byte(
                    text,
                    b as u8,
                    from as int,
                    v,
                ))
                &&& !(0 <= from <= text.len()) ==> (r matches Err(Fault::Index(_)))
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_bytes(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_body(h1, s, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let r = {
            let t = self.heap.body_of(s);
            let len = t.len();
            if 0 <= from && (from as u64) <= (len as u64) {
                // ヒープのバイト列は `isize::MAX` より短いので、ここには来ない
                if (len as u64) > (i64::MAX as u64) {
                    crate::alloc::too_large();
                }
                match find_byte(t, from as usize, b as u8) {
                    Some(i) => Ok(i as i64),
                    None => Ok(-1),
                }
            } else {
                Err(Fault::Index(Box::new(IndexFault::BytesIndex { index: from, len: len as i64 })))
            }
        };
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_lent(s0, h1);
            }
        }
        r
    }

    /// `Bytes.of_string`。`s` の参照を受け取り、同じバイトのバイト列の参照を返す。`s` が一意なら、その場で種類だけを
    /// `Bytes` に書き換えて `s` を返す。そうでなければ中身を写した新しいバイト列を返し、`s` の参照を手放す。写した
    /// バイトだけを `string_bytes_copied` に数える (docs/spec/object-model.md の「変換」)。
    ///
    /// # Safety
    /// `bytes_push` と同じ (`s` の参照は受け取る)。速いモードでは、`s` は文字列である。
    pub unsafe fn bytes_of_string(&mut self, s: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> final(self).is_bytes(w) && final(self).text_of(w) == old(
                self,
            ).text_of(s),
            r matches Ok(w) ==> (old(self).held_ok() && old(self).is_string(s) && old(self).holds(s)
                ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& w != s ==> final(self).evolved_by(
                    *old(self),
                    |x: usize| -points(s, x),
                    Set::empty(),
                )
                &&& w == s ==> final(self).evolved_kind(
                    *old(self),
                    |x: usize| 0int,
                    Set::empty(),
                    s,
                )
            }),
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_string(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let ghost s1 = *self;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_body(h1, s, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        if self.heap.core.rc_get(hs) == 1 {
            proof {
                lemma_unique_body(h1, s, Kind::String);
            }
            self.heap.set_body_kind(hs, Kind::Bytes);
            proof {
                self.heap.lemma_text_spec(s);
                h1.lemma_text_spec(s);
                if M::CHECKED {
                    assert(self.heap.words_kept(h1));
                    lemma_words_trans(self.heap, h1, s0.heap);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_rekinded_ok(s0, s1, s, Kind::Bytes);
                }
            }
            return Ok(s);
        }
        let ghost text = h1.text_of(s);
        let len = self.heap.body_of(s).len();
        let w = self.heap.alloc_bytes_room(len);
        let ghost h2 = self.heap;
        proof {
            h2.lemma_text_spec(w);
            self.lemma_room_made(h1, s, w, Kind::String);
        }
        self.heap.append_body(w, s, 0, len);
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                len as u64,
            );
        }
        let ghost h3 = self.heap;
        proof {
            assert(h3.text_of(w) =~= text.subrange(0, len as int));
            assert(text.subrange(0, len as int) =~= text);
            h3.lemma_text_spec(s);
            h3.lemma_text_spec(w);
            if c {
                s0.lemma_text_shown(h1, h2, h3, s, w, Kind::String);
            }
            if !self.heap.core.dom().contains(hs@.addr) {
                self.heap.core.lemma_region_at(hs@.addr);
            }
        }
        self.heap.release_text(hs, Ghost(s), Ghost(c));
        proof {
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                lemma_words_trans(h2, h1, s0.heap);
                assert(h3.words_kept(h2));
                assert(self.heap.words_kept(h3));
                lemma_words_trans(h3, h2, s0.heap);
                lemma_words_trans(self.heap, h3, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_converted_ok(s0, h1, h2, h3, s, w, Kind::String);
            }
        }
        Ok(w)
    }

    /// `String.decode`。`b` の参照を受け取り、中身が妥当な UTF-8 なら、同じバイトの文字列の参照を返す。`b` が一意なら、
    /// その場で種類だけを `String` に書き換えて `b` を返す。そうでなければ写した新しい文字列を返し、`b` の参照を
    /// 手放す。妥当でなければ `b` の参照を手放し、最初の妥当でない位置を持つ `Fault::Utf8` を返す。確かめは T5 の
    /// `utf8_valid` がし、写したバイトだけを `string_bytes_copied` に数える (docs/spec/object-model.md の「変換」)。
    ///
    /// # Safety
    /// `bytes_push` と同じ (`b` の参照は受け取る)。
    pub unsafe fn string_decode(&mut self, b: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).is_bytes(b) && old(self).holds(b),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Utf8 { .. })) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_bytes(b) && old(self).holds(b) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> {
                &&& valid_utf8(old(self).text_of(b))
                &&& final(self).is_string(w)
                &&& final(self).text_of(w) == old(self).text_of(b)
            },
            r matches Err(Fault::Utf8 { .. }) ==> !valid_utf8(old(self).text_of(b)),
            old(self).held_ok() && old(self).is_bytes(b) && old(self).holds(b) ==> {
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& w != b ==> final(self).evolved_by(
                        *old(self),
                        |x: usize| -points(b, x),
                        Set::empty(),
                    )
                    &&& w == b ==> final(self).evolved_kind(
                        *old(self),
                        |x: usize| 0int,
                        Set::empty(),
                        b,
                    )
                }
                &&& r matches Err(Fault::Utf8 { .. }) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |x: usize| -points(b, x), Set::empty())
                }
                &&& valid_utf8(old(self).text_of(b)) ==> !(r matches Err(Fault::Utf8 { .. }))
            },
    {
        let ghost s0 = *self;
        let ghost ws = seq![b];
        let ghost c = s0.held_ok() && s0.is_bytes(b) && s0.holds(b);
        if M::CHECKED && !b.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let ghost s1 = *self;
        let hb = self.heap.header_of(b);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(b);
            s0.heap.lemma_text_spec(b);
            if c {
                s0.lemma_settled_body(h1, b, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(hb, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let ghost text = h1.text_of(b);
        if let Some(at) = utf8_valid(self.heap.body_of(b)) {
            proof {
                if !self.heap.core.dom().contains(hb@.addr) {
                    self.heap.core.lemma_region_at(hb@.addr);
                }
            }
            self.heap.release_text(hb, Ghost(b), Ghost(c));
            proof {
                if M::CHECKED {
                    assert(self.heap.words_kept(h1));
                    lemma_words_trans(self.heap, h1, s0.heap);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_dropped_ok(s0, h1, ws, b);
                }
            }
            return Err(Fault::Utf8 { at: at as i64 });
        }
        if self.heap.core.rc_get(hb) == 1 {
            proof {
                lemma_unique_body(h1, b, Kind::Bytes);
                h1.lemma_text_spec(b);
            }
            self.heap.set_body_kind(hb, Kind::String);
            proof {
                self.heap.lemma_text_spec(b);
                if M::CHECKED {
                    assert(self.heap.words_kept(h1));
                    lemma_words_trans(self.heap, h1, s0.heap);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_rekinded_ok(s0, s1, b, Kind::String);
                }
            }
            return Ok(b);
        }
        let len = self.heap.body_of(b).len();
        let w = self.heap.alloc_string_room(len);
        let ghost h2 = self.heap;
        proof {
            h2.lemma_text_spec(w);
            self.lemma_room_made(h1, b, w, Kind::Bytes);
        }
        self.heap.append_utf8(w, b);
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                len as u64,
            );
        }
        let ghost h3 = self.heap;
        proof {
            assert(h3.text_of(w) =~= text);
            h3.lemma_text_spec(b);
            h3.lemma_text_spec(w);
            if c {
                s0.lemma_text_shown(h1, h2, h3, b, w, Kind::Bytes);
            }
            if !self.heap.core.dom().contains(hb@.addr) {
                self.heap.core.lemma_region_at(hb@.addr);
            }
        }
        self.heap.release_text(hb, Ghost(b), Ghost(c));
        proof {
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                lemma_words_trans(h2, h1, s0.heap);
                assert(h3.words_kept(h2));
                assert(self.heap.words_kept(h3));
                lemma_words_trans(h3, h2, s0.heap);
                lemma_words_trans(self.heap, h3, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_converted_ok(s0, h1, h2, h3, b, w, Kind::Bytes);
            }
        }
        Ok(w)
    }

    /// `Bytes.make`。どのバイトも `b` の、長さ `n` の新しいバイト列を返す。`n` が負なら `Fault::Index` を返す。
    /// 確保量の上限を超える長さは、確保を試す前に `Stopped(MemoryLimit)` で止める (`Array.make` と同じ)。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`b` は `Byte` の語で、0 から 255 である (docs/spec/lir.md の「値の表現」)。
    pub unsafe fn bytes_make(&mut self, n: i64, b: i64) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            0 <= b < 256,
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
            r matches Ok(w) ==> final(self).is_bytes(w) && final(self).text_of(w) == repeated(
                n as nat,
                b as u8,
            ),
            r matches Ok(w) ==> (old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            }),
    {
        let ghost s0 = *self;
        if n < 0 {
            return Err(Fault::Index(Box::new(IndexFault::BytesLength { len: n })));
        }
        let k = n as usize;
        // 確保器が作れない長さと確保量の上限を超える長さは、確保を試す前に止める。確保器に任せると、プロセスが panic で
        // 落ちるか、上限を報告する前に OS に止められるためである (docs/spec/object-model.md の「配列」)
        if k > MAX_BYTES || over_budget(&self.control, k + 16) {
            return Err(Fault::Stopped(Stop::MemoryLimit));
        }
        let byte = b as u8;
        let mut chunk = [0u8;64];
        let mut j: usize = 0;
        while j < 64
            invariant
                j <= 64,
                forall|i: int| 0 <= i < j ==> chunk@[i] == byte,
            decreases 64 - j,
        {
            chunk[j] = byte;
            j += 1;
        }
        let w = self.heap.alloc_bytes_room(k);
        let ghost h2 = self.heap;
        proof {
            h2.lemma_text_spec(w);
            assert(h2.text_of(w) =~= repeated(0, byte));
        }
        let mut done: usize = 0;
        while done < k
            invariant
                self.heap.wf(),
                self.heap.core.model@.building is None,
                self.heap.same_tokens(h2),
                self.heap.dummy == h2.dummy,
                self.heap.region == h2.region,
                self.heap.worker == h2.worker,
                self.heap == h2 || self.heap.core.text_changed(h2.core, w.bits_spec()),
                self.heap.appendable(w),
                self.heap.is_bytes(w),
                self.heap.capacity(w) >= k,
                done <= k,
                self.heap.text_of(w) == repeated(done as nat, byte),
                h2.consistent() ==> self.heap.consistent(),
                forall|x: Word| #[trigger] self.heap.core.word_ok(x) == h2.core.word_ok(x),
                forall|i: int| 0 <= i < 64 ==> chunk@[i] == byte,
                self.evv == s0.evv,
                self.yielding == s0.yielding,
                self.max_evv == s0.max_evv,
            decreases k - done,
        {
            let m = if k - done < 64 {
                k - done
            } else {
                64
            };
            let ghost before = self.heap;
            let part = vstd::slice::slice_subrange(chunk.as_slice(), 0, m);
            proof {
                self.heap.lemma_text_spec(w);
            }
            self.heap.append_bytes(w, part);
            proof {
                if before != h2 {
                    lemma_text_trans(self.heap.core, before.core, h2.core, w.bits_spec());
                }
                assert(self.heap.text_of(w) =~= repeated((done + m) as nat, byte));
            }
            done += m;
        }
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(k as u64);
        }
        proof {
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                assert(self.heap.words_kept(s0.heap));
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                if self.heap == h2 {
                    self.lemma_made_ok(s0, w);
                } else {
                    self.lemma_made_filled(s0, h2, w);
                }
            }
        }
        Ok(w)
    }

    /// `Bytes.push`。`w` の参照を受け取り、後ろに `b` を足したバイト列の参照を返す。`w` が一意で容量が足りれば、その場で
    /// 足す (`Heap::bytes_extend`)。
    ///
    /// # Safety
    /// `bytes_length` と同じ (`w` の参照は受け取る)。`b` は `Byte` の語で、0 から 255 である。
    pub unsafe fn bytes_push(&mut self, b: i64, w: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(w),
            0 <= b < 256,
            !M::CHECKED ==> old(self).is_bytes(w) && old(self).holds(w),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_bytes(w) && old(self).holds(w) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(x) ==> final(self).is_bytes(x) && final(self).text_of(x) == old(
                self,
            ).text_of(w).push(b as u8),
            r matches Ok(x) ==> (old(self).held_ok() && old(self).is_bytes(w) && old(self).holds(w)
                ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(x)
                &&& final(self).evolved_by(
                    *old(self),
                    |y: usize| points(x, y) - points(w, y),
                    Set::empty().insert(w.bits_spec()),
                )
            }),
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_bytes(w) && s0.holds(w);
        if M::CHECKED && !w.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(w);
            s0.heap.lemma_text_spec(w);
            if c {
                s0.lemma_settled_body(h1, w, Kind::Bytes);
                s0.lemma_settled_push_ok(h1, w, Kind::Bytes);
            }
        }
        let copied = if M::CHECKED {
            // 写すバイトを数えるために、一意かと長さを先に読む。読む前に検査モードの検査を済ませる
            // (docs/spec/object-model.md の「仕事の回数」)
            let hw = self.heap.header_of(w);
            if let Err(e) = self.heap.checked_body(hw, Kind::Bytes) {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            if self.heap.core.rc_get(hw) == 1 {
                1
            } else {
                (self.heap.body_of(w).len() as u64).wrapping_add(1)
            }
        } else {
            0
        };
        let one = [b as u8];
        let x = match unsafe { self.heap.bytes_extend(w, one.as_slice()) } {
            Ok(x) => x,
            Err(e) => {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(copied);
        }
        proof {
            assert(one@ =~= seq![b as u8]);
            self.heap.lemma_text_spec(x);
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_pushed_ok(s0, h1, seq![w], w, x, Kind::Bytes);
            }
        }
        Ok(x)
    }

    /// `Bytes.append`。両辺の参照を受け取り、`a` の後ろに `b` を足したバイト列の参照を返す。`a` がほかに参照のない
    /// バイト列で容量が足りれば、その場で足す (`++` と同じ道)。
    ///
    /// # Safety
    /// `bytes_eq` と同じ (両辺の参照は受け取る)。
    pub unsafe fn bytes_append(&mut self, a: Word, b: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).holds_pair(a, b, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).holds_pair(a, b, Kind::Bytes) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> final(self).is_bytes(w) && final(self).text_of(w) == old(
                self,
            ).text_of(a) + old(self).text_of(b),
            r matches Ok(w) ==> (old(self).held_ok() && old(self).holds_pair(a, b, Kind::Bytes)
                ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(
                    *old(self),
                    |x: usize| points(w, x) - points(a, x) - points(b, x),
                    Set::empty().insert(a.bits_spec()),
                )
            }),
    {
        unsafe { self.append_bodies(a, b, Kind::Bytes) }
    }

    /// `Bytes.append_slice`。`dst` の参照を受け取り、`src` を借りて、`dst` の後ろに `src` の位置 `from` から `to` の
    /// 手前までを足したバイト列の参照を返す。範囲が `0 <= from <= to <= len` でなければ、`dst` を手放して
    /// `Fault::Index` を返す。`src` と `dst` が同じ物体なら、呼ぶ側はその参照を2つ持つ (docs/spec/lir.md の
    /// 「extern の借用」)。
    ///
    /// # Safety
    /// `bytes_eq` と同じ。`dst` と `src` が同じ物体なら、呼ぶ側はその参照を2つ持つ (検査モードは、rc が 1 なら
    /// `NotUnique` を返す)。
    pub unsafe fn bytes_append_part(&mut self, from: i64, to: i64, src: Word, dst: Word) -> (r:
        Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(src),
            old(self).heap_spec().word_in(dst),
            !M::CHECKED ==> old(self).holds_pair(dst, src, Kind::Bytes),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).holds_pair(dst, src, Kind::Bytes) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> {
                let text = old(self).text_of(src);
                &&& 0 <= from <= to <= text.len()
                &&& final(self).is_bytes(w)
                &&& final(self).text_of(w) == old(self).text_of(dst) + text.subrange(
                    from as int,
                    to as int,
                )
            },
            old(self).held_ok() && old(self).holds_pair(dst, src, Kind::Bytes) ==> {
                let text = old(self).text_of(src);
                &&& 0 <= from <= to <= text.len() ==> !(r matches Err(Fault::Index(_)))
                &&& r matches Ok(w) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(w)
                    &&& final(self).evolved_by(
                        *old(self),
                        |x: usize| points(w, x) - points(dst, x),
                        Set::empty().insert(dst.bits_spec()),
                    )
                }
                &&& r matches Err(Fault::Index(_)) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |x: usize| -points(dst, x), Set::empty())
                }
            },
    {
        let ghost s0 = *self;
        let ghost ws = seq![dst, src];
        let ghost c = s0.held_ok() && s0.holds_pair(dst, src, Kind::Bytes);
        if M::CHECKED && !(src.is_ptr() && dst.is_ptr()) {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hd = self.heap.header_of(dst);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(src);
            h1.lemma_text_spec(dst);
            s0.heap.lemma_text_spec(src);
            s0.heap.lemma_text_spec(dst);
            if c {
                s0.lemma_settled_strings(h1, dst, src, Kind::Bytes);
                s0.lemma_arg_counted(ws, 0);
                lemma_points_in(ws, dst.bits_spec());
                lemma_settle_moved(h1, s0.heap);
                lemma_arg_moved(h1, s0.heap, |x: usize| 0int, Set::empty(), dst);
            }
        }
        if let Err(e) = self.heap.checked_part(dst, src) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let len = self.heap.body_of(src).len();
        if !(0 <= from && from <= to && (to as u64) <= (len as u64)) {
            proof {
                if c {
                    lemma_view_live(h1, dst.bits_spec());
                }
                if !self.heap.core.dom().contains(hd@.addr) {
                    self.heap.core.lemma_region_at(hd@.addr);
                }
            }
            self.heap.release_text(hd, Ghost(dst), Ghost(c));
            proof {
                if M::CHECKED {
                    assert(self.heap.words_kept(h1));
                    lemma_words_trans(self.heap, h1, s0.heap);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_dropped_ok(s0, h1, ws, dst);
                }
            }
            return Err(
                Fault::Index(
                    Box::new(IndexFault::BytesSlice { start: from, end: to, len: len as i64 }),
                ),
            );
        }
        let copied = if M::CHECKED {
            // 一意な dst には足す範囲だけを、そうでなければ dst の長さも数える
            // (docs/spec/object-model.md の「仕事の回数」)
            let part = (to - from) as u64;
            if self.heap.core.rc_get(hd) == 1 {
                part
            } else {
                (self.heap.body_of(dst).len() as u64).wrapping_add(part)
            }
        } else {
            0
        };
        let w = match unsafe { self.heap.bytes_append_part(dst, src, from as usize, to as usize) } {
            Ok(w) => w,
            Err(e) => {
                proof {
                    lemma_settled_twice(s0.heap);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(copied);
        }
        proof {
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_pushed_ok(s0, h1, ws, dst, w, Kind::Bytes);
            }
        }
        Ok(w)
    }

    /// `Bytes.truncate`。`w` の参照を受け取り、前の `n` バイトのバイト列の参照を返す。`w` が一意ならその場で長さの語
    /// だけを書く。`0 <= n <= len` でなければ、`w` を手放して `Fault::Index` を返す。
    ///
    /// # Safety
    /// `bytes_push` と同じ。
    pub unsafe fn bytes_truncate(&mut self, n: i64, w: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(w),
            !M::CHECKED ==> old(self).is_bytes(w) && old(self).holds(w),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_bytes(w) && old(self).holds(w) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(x) ==> {
                &&& 0 <= n <= old(self).text_of(w).len()
                &&& final(self).is_bytes(x)
                &&& final(self).text_of(x) == old(self).text_of(w).subrange(0, n as int)
            },
            old(self).held_ok() && old(self).is_bytes(w) && old(self).holds(w) ==> {
                &&& 0 <= n <= old(self).text_of(w).len() ==> !(r matches Err(Fault::Index(_)))
                &&& r matches Ok(x) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(x)
                    &&& final(self).evolved_by(
                        *old(self),
                        |y: usize| points(x, y) - points(w, y),
                        Set::empty().insert(w.bits_spec()),
                    )
                }
                &&& r matches Err(Fault::Index(_)) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |y: usize| -points(w, y), Set::empty())
                }
            },
    {
        let ghost s0 = *self;
        let ghost ws = seq![w];
        let ghost c = s0.held_ok() && s0.is_bytes(w) && s0.holds(w);
        if M::CHECKED && !w.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hw = self.heap.header_of(w);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(w);
            s0.heap.lemma_text_spec(w);
            if c {
                s0.lemma_settled_body(h1, w, Kind::Bytes);
                s0.lemma_settled_push_ok(h1, w, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(hw, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let len = self.heap.body_of(w).len();
        if !(0 <= n && (n as u64) <= (len as u64)) {
            proof {
                if !self.heap.core.dom().contains(hw@.addr) {
                    self.heap.core.lemma_region_at(hw@.addr);
                }
            }
            self.heap.release_text(hw, Ghost(w), Ghost(c));
            proof {
                if M::CHECKED {
                    assert(self.heap.words_kept(h1));
                    lemma_words_trans(self.heap, h1, s0.heap);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_dropped_ok(s0, h1, ws, w);
                }
            }
            return Err(
                Fault::Index(
                    Box::new(IndexFault::BytesSlice { start: 0, end: n, len: len as i64 }),
                ),
            );
        }
        let copied = if M::CHECKED && self.heap.core.rc_get(hw) != 1 {
            // 一意ならその場で長さの語だけを書くので数えず、そうでなければ結果の長さを数える
            // (docs/spec/object-model.md の「仕事の回数」)
            n as u64
        } else {
            0
        };
        let x = match unsafe { self.heap.bytes_truncate(w, n as usize) } {
            Ok(x) => x,
            Err(e) => {
                proof {
                    lemma_settled_twice(s0.heap);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(copied);
        }
        proof {
            self.heap.lemma_text_spec(x);
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_pushed_ok(s0, h1, ws, w, x, Kind::Bytes);
            }
        }
        Ok(x)
    }

    /// `Bytes.set`。`w` の参照を受け取り、位置 `i` のバイトを `b` にしたバイト列の参照を返す。`w` が一意ならその場で
    /// 書き、そうでなければ写してから書く。範囲の外なら、`w` を手放して `Fault::Index` を返す。
    ///
    /// # Safety
    /// `bytes_push` と同じ。
    pub unsafe fn bytes_set(&mut self, i: i64, b: i64, w: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(w),
            0 <= b < 256,
            !M::CHECKED ==> old(self).is_bytes(w) && old(self).holds(w),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_bytes(w) && old(self).holds(w) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(x) ==> {
                &&& 0 <= i < old(self).text_of(w).len()
                &&& final(self).is_bytes(x)
                &&& final(self).text_of(x) == old(self).text_of(w).update(i as int, b as u8)
            },
            old(self).held_ok() && old(self).is_bytes(w) && old(self).holds(w) ==> {
                &&& 0 <= i < old(self).text_of(w).len() ==> !(r matches Err(Fault::Index(_)))
                &&& r matches Ok(x) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(x)
                    &&& final(self).evolved_by(
                        *old(self),
                        |y: usize| points(x, y) - points(w, y),
                        Set::empty().insert(w.bits_spec()),
                    )
                }
                &&& r matches Err(Fault::Index(_)) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |y: usize| -points(w, y), Set::empty())
                }
            },
    {
        let ghost s0 = *self;
        let ghost ws = seq![w];
        let ghost c = s0.held_ok() && s0.is_bytes(w) && s0.holds(w);
        if M::CHECKED && !w.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hw = self.heap.header_of(w);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(w);
            s0.heap.lemma_text_spec(w);
            if c {
                s0.lemma_settled_body(h1, w, Kind::Bytes);
                s0.lemma_settled_push_ok(h1, w, Kind::Bytes);
            }
        }
        if let Err(e) = self.heap.checked_body(hw, Kind::Bytes) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let len = self.heap.body_of(w).len();
        if !(0 <= i && (i as u64) < (len as u64)) {
            proof {
                if !self.heap.core.dom().contains(hw@.addr) {
                    self.heap.core.lemma_region_at(hw@.addr);
                }
            }
            self.heap.release_text(hw, Ghost(w), Ghost(c));
            proof {
                if M::CHECKED {
                    assert(self.heap.words_kept(h1));
                    lemma_words_trans(self.heap, h1, s0.heap);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_dropped_ok(s0, h1, ws, w);
                }
            }
            return Err(
                Fault::Index(Box::new(IndexFault::BytesIndex { index: i, len: len as i64 })),
            );
        }
        let copied = if M::CHECKED && self.heap.core.rc_get(hw) != 1 {
            // 一意ならその場で1バイトを書くので数えず、そうでなければ写した長さを数える
            // (docs/spec/object-model.md の「仕事の回数」)
            len as u64
        } else {
            0
        };
        let x = match unsafe { self.heap.bytes_set(w, i as usize, b as u8) } {
            Ok(x) => x,
            Err(e) => {
                proof {
                    lemma_settled_twice(s0.heap);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(copied);
        }
        proof {
            self.heap.lemma_text_spec(x);
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_pushed_ok(s0, h1, ws, w, x, Kind::Bytes);
            }
        }
        Ok(x)
    }
}

/// `checked_body` を通った rc 1 の文字列かバイト列は、このヒープの生きている物体である。塊の物体は rc 0 なので、
/// rc 1 の物体は塊にない。
proof fn lemma_unique_body<M: Mode>(h: Heap<M>, w: Word, kind: Kind)
    requires
        h.wf(),
        h.core.model@.building is None,
        h.readable_as(w, kind),
        h.core.rc_of(w.bits_spec()) == 1,
    ensures
        h.core.dom().contains(w.bits_spec()),
        h.core.state(w.bits_spec()) == BlockState::Live,
        h.core.kind(w.bits_spec()) == kind,
{
    let b = w.bits_spec();
    h.core.lemma_shape(b);
    if M::CHECKED {
        h.core.lemma_code_state(b);
    } else {
        h.core.lemma_readable(w);
    }
    reveal(Core::building_wf);
    if !h.core.dom().contains(b) {
        h.core.lemma_region_at(b);
    }
}

/// `set_body_kind` の後のヒープは、`a` の種類のほかは前と同じに読める。`lin` ビット、rc、持ち主、語は変わらない。
proof fn lemma_rekinded_same<M: Mode>(h1: Heap<M>, h0: Heap<M>, a: usize, kind: Kind)
    requires
        h1.rekinded(h0, a, kind),
    ensures
        h0.core.kind(a).has_body(),
        forall|w: Word| #[trigger] h1.readable(w) == h0.readable(w),
        forall|w: Word| #[trigger] h1.owned(w) == h0.owned(w),
        forall|w: Word| #[trigger] h1.rc_value(w) == h0.rc_value(w),
        forall|w: Word| #[trigger] h1.count_of(w) == h0.count_of(w),
        forall|w: Word| #[trigger] h1.tag_of(w) == h0.tag_of(w),
        forall|w: Word| #[trigger] h1.scan_of(w) == h0.scan_of(w),
        forall|w: Word| #[trigger] h1.lin_of(w) == h0.lin_of(w),
        forall|w: Word, i: int| #[trigger] h1.scanned_word(w, i) == h0.scanned_word(w, i),
        forall|w: Word| w.bits_spec() != a ==> #[trigger] h1.kind_of(w) == h0.kind_of(w),
        h1.core.kind(a) == kind,
        forall|b: usize| #[trigger] live(h1, b) == live(h0, b),
        forall|b: usize| #[trigger] h1.core.block(b) == h0.core.block(b),
        forall|b: usize| #[trigger] h1.core.settled_view().ext(b) == h0.core.settled_view().ext(b),
        forall|b: usize| #[trigger]
            h1.core.settled_view().rc_of(b) == h0.core.settled_view().rc_of(b),
        forall|b: usize| b != a ==> #[trigger] h1.core.perm(b) == h0.core.perm(b),
        h1.core.perm(a).slots == h0.core.perm(a).slots,
        h1.core.perm(a).rc == h0.core.perm(a).rc,
        h1.core.perm(a).text == h0.core.perm(a).text,
        h1.core.model == h0.core.model,
        h1.core.env == h0.core.env,
{
    assert forall|b: usize| b != a implies #[trigger] h1.core.perm(b) == h0.core.perm(b) by {
        if h0.core.dom().contains(b) {
            assert(h1.core.perms@[b] == h0.core.perms@[b]);
        }
    }
    assert(h1.core.settled_view() == h1.core);
    assert(h0.core.settled_view() == h0.core);
    assert forall|w: Word| #[trigger] h1.count_of(w) == h0.count_of(w) by {
        if w.bits_spec() != a {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word, i: int| #[trigger] h1.scanned_word(w, i) == h0.scanned_word(w, i) by {
        if w.bits_spec() != a {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.tag_of(w) == h0.tag_of(w) by {
        if w.bits_spec() != a {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| w.bits_spec() != a implies #[trigger] h1.kind_of(w) == h0.kind_of(w) by {
        assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
    }
    assert forall|w: Word| #[trigger] h1.scan_of(w) == h0.scan_of(w) by {
        if w.bits_spec() != a {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.lin_of(w) == h0.lin_of(w) by {
        if w.bits_spec() != a {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.rc_value(w) == h0.rc_value(w) by {
        if w.bits_spec() != a {
            assert(h1.core.perm(w.bits_spec()) == h0.core.perm(w.bits_spec()));
        }
    }
    assert forall|w: Word| #[trigger] h1.owned(w) == h0.owned(w) by {
        assert(h1.rc_value(w) == h0.rc_value(w));
    }
    assert forall|b: usize| #[trigger]
        h1.core.settled_view().rc_of(b) == h0.core.settled_view().rc_of(b) by {
        if b != a {
            assert(h1.core.perm(b) == h0.core.perm(b));
        }
    }
}

/// `set_body_kind` の後も、`evv` のリストは同じ列のリストである。節点も evidence も入口の表も `Data` で、書き換えた
/// 物体は前も後も文字列かバイト列なので、どの物体が `Data` かは変わらない。
proof fn lemma_path_rekinded<M: Mode>(
    h1: Heap<M>,
    h0: Heap<M>,
    a: usize,
    kind: Kind,
    w: Word,
    f: nat,
)
    requires
        h1.rekinded(h0, a, kind),
        kind.has_body(),
    ensures
        path(h1, w, f) == path(h0, w, f),
    decreases f,
{
    lemma_rekinded_same(h1, h0, a, kind);
    assert forall|v: Word| #[trigger]
        h1.kind_of(v) == Kind::Data <==> h0.kind_of(v) == Kind::Data by {
        if v.bits_spec() == a {
            assert(h0.kind_of(v) == h0.core.kind(a));
        }
    }
    if w.bits_spec() != 1 && f > 0 {
        let e = h0.scanned_word(w, 0);
        let t = h0.scanned_word(e, 0);
        assert(node_ok(h1, w) == node_ok(h0, w));
        if table_ok(h0, t) {
            assert(h1.kind_of(t) == Kind::Data);
            assert forall|i: int| 0 <= i < h1.count_of(t) implies dupable(
                h1,
                #[trigger] h1.scanned_word(t, i),
            ) by {
                assert(dupable(h0, h0.scanned_word(t, i)));
            }
        }
        if table_ok(h1, t) {
            assert(h0.kind_of(t) == Kind::Data);
            assert forall|i: int| 0 <= i < h0.count_of(t) implies dupable(
                h0,
                #[trigger] h0.scanned_word(t, i),
            ) by {
                assert(dupable(h1, h1.scanned_word(t, i)));
            }
        }
        assert(ev_ok(h1, e) == ev_ok(h0, e));
        lemma_path_rekinded(h1, h0, a, kind, h0.scanned_word(w, 1), (f - 1) as nat);
    }
}

impl<M: Mode> Ctx<M> {
    /// 一意な引数の種類をその場で書き換えた `of_string` と `decode` の、モデルが合っている道。`s1` は `settle` の後の
    /// 文脈で、その後は引数 `a` の種類だけが変わった。参照の数も `lin` ビットも変わらないので、`Ctx` の語と `a` は
    /// 持たれたままで、`evv` のリストもそのままである。
    proof fn lemma_rekinded_ok(&self, s0: Ctx<M>, s1: Ctx<M>, a: Word, kind: Kind)
        requires
            s0.held_ok(),
            s0.holds(a),
            a.is_ptr_spec(),
            s1.heap == s0.heap.settled_heap(),
            s1.heap.consistent(),
            s1.evv == s0.evv,
            s1.yielding == s0.yielding,
            s1.max_evv == s0.max_evv,
            self.heap.rekinded(s1.heap, a.bits_spec(), kind),
            kind.has_body(),
            self.heap.consistent(),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(a),
            self.evolved_kind(s0, |b: usize| 0int, Set::empty(), a),
    {
        let h0 = s0.heap;
        let h1 = s1.heap;
        let h2 = self.heap;
        let ab = a.bits_spec();
        let z = |b: usize| 0int;
        let none = Set::<usize>::empty();
        // `settle` の後の文脈
        lemma_settle_moved(h1, h0);
        assert forall|b: usize| #[trigger] s1.need(seq![], b) > 0 implies s0.need(seq![], b) + z(b)
            >= s1.need(seq![], b) by {
            lemma_points_in(seq![], b);
        }
        s1.lemma_holds_moved(s0, seq![], seq![], z, none);
        s1.lemma_evv_kept(s0, z, none);
        assert forall|b: usize| #[trigger] s1.need(seq![a], b) > 0 implies s0.need(seq![a], b) + z(
            b,
        ) >= s1.need(seq![a], b) by {}
        s1.lemma_holds_moved(s0, seq![a], seq![a], z, none);
        // 種類の書き換え
        lemma_rekinded_same(h2, h1, ab, kind);
        assert(h2.core.settleable());
        assert(held_word(h2, self.evv));
        assert(held_word(h2, self.yielding.target));
        assert(held_word(h2, self.yielding.clause));
        assert(held_word(h2, self.yielding.cont));
        assert(held_word(h1, seq![a][0]));
        assert(held_word(h2, a));
        assert(self.holds_args(seq![])) by {
            assert forall|b: usize| #[trigger]
                self.need(seq![], b) > 0 && h2.core.settled_view().rc_of(b)
                    != 0 implies h2.core.settled_view().ext(b) >= self.need(seq![], b) by {
                assert(s1.need(seq![], b) == self.need(seq![], b));
            }
        }
        assert(self.holds_args(seq![a])) by {
            let ws = seq![a];
            assert forall|i: int| 0 <= i < ws.len() implies held_word(h2, #[trigger] ws[i]) by {}
            assert forall|b: usize| #[trigger]
                self.need(ws, b) > 0 && h2.core.settled_view().rc_of(b)
                    != 0 implies h2.core.settled_view().ext(b) >= self.need(ws, b) by {
                assert(s1.need(ws, b) == self.need(ws, b));
            }
        }
        let w = self.evv;
        let f = choose|f: nat| #[trigger] path(h1, w, f) is Some;
        lemma_seq_of(h1, w, f);
        lemma_path_rekinded(h2, h1, ab, kind, w, f);
        lemma_seq_of(h2, w, f);
        assert(listed(h2, w, self.max_evv));
        // `settle` と種類の書き換えをつなぐ
        let v0 = h0.core.settled_view();
        assert forall|b: usize|
            #![trigger live(h2, b), live(h0, b)]
            live(h0, b) && live(h2, b) implies {
            &&& b != ab ==> same_head(h2, h0, b)
            &&& same_head_kind(h2, h0, b)
            &&& h2.core.perm(b).slots == h0.core.perm(b).slots
        } by {
            assert(live(h1, b));
            assert(same_head(h1, h0, b));
            if b != ab {
                assert(h2.core.perm(b) == h1.core.perm(b));
            }
        }
        assert forall|b: usize|
            #![trigger live(h0, b)]
            live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + z(b) >= 1) implies {
            &&& live(h2, b)
            &&& h2.core.block(b) == h0.core.block(b)
            &&& v0.rc_of(b) != 0 ==> h2.core.settled_view().ext(b) == v0.ext(b) + z(b)
        } by {
            assert(live(h1, b));
        }
        assert(moved_kind(h2, h0, z, none, ab));
    }
}

} // verus!

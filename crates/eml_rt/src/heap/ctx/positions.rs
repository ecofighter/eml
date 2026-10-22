//! バイトの位置で読む文字列の核の extern (docs/spec/object-model.md の「extern」)。`String` の `byte_length`、
//! `byte_at`、`slice`、`find_from` の結果を、文字列のバイト列に対して証明する。分割や検索などの便利な関数は、これらの上に
//! 標準ライブラリが eml で書く (docs/spec/stdlib.md)。

#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;
#[cfg(verus_only)]
use vstd::utf8::{is_char_boundary, valid_utf8};

#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::alloc::too_large;
use crate::externs::{Fault, IndexFault};

verus! {

/// `needle` が `text` の位置 `i` から始まる。
pub open spec fn matches_at(text: Seq<u8>, needle: Seq<u8>, i: int) -> bool {
    0 <= i && i + needle.len() <= text.len() && text.subrange(i, i + needle.len()) == needle
}

/// `find_from` の結果。`from` 以上で `needle` が始まる最小の位置で、なければ -1 である。
pub open spec fn found_from(text: Seq<u8>, needle: Seq<u8>, from: int, r: i64) -> bool {
    if r >= 0 {
        &&& from <= r
        &&& matches_at(text, needle, r as int)
        &&& forall|j: int| from <= j < r ==> !matches_at(text, needle, j)
    } else {
        &&& r == -1
        &&& forall|j: int| from <= j ==> !matches_at(text, needle, j)
    }
}

/// `text[i..]` が `needle` で始まるか。
fn starts_at(text: &[u8], needle: &[u8], i: usize) -> (r: bool)
    ensures
        r == matches_at(text@, needle@, i as int),
{
    if i > text.len() || needle.len() > text.len() - i {
        return false;
    }
    let mut k: usize = 0;
    while k < needle.len()
        invariant
            text@.len() <= usize::MAX,
            i + needle@.len() <= text@.len(),
            k <= needle@.len(),
            forall|j: int| 0 <= j < k ==> text@[i + j] == needle@[j],
        decreases needle@.len() - k,
    {
        if text[i + k] != needle[k] {
            proof {
                let sub = text@.subrange(i as int, i + needle@.len());
                assert(sub[k as int] != needle@[k as int]);
            }
            return false;
        }
        k += 1;
    }
    proof {
        assert(text@.subrange(i as int, i + needle@.len()) =~= needle@);
    }
    true
}

/// `find_byte` の結果。`from` 以上で `b` がある最小の位置で、なければ -1 である。
pub open spec fn found_byte(text: Seq<u8>, b: u8, from: int, r: i64) -> bool {
    if r >= 0 {
        &&& from <= r < text.len()
        &&& text[r as int] == b
        &&& forall|j: int| from <= j < r ==> text[j] != b
    } else {
        &&& r == -1
        &&& forall|j: int| from <= j < text.len() ==> text[j] != b
    }
}

/// `hay` の `from` 以上で `b` がある最小の位置。8 バイトずつ1語に読み、`b` に等しいバイトがあるかを語の演算で
/// 確かめる (SWAR)。結果が最初の位置であることは、Miri の上で素朴なループと比べるテストで確かめ、信頼する
/// (信頼の一覧の T13)。語の演算の証明は `bit_vector` の大きな証明になるためである。
#[verifier::external_body]
#[inline]
pub(crate) fn find_byte(hay: &[u8], from: usize, b: u8) -> (r: Option<usize>)
    requires
        from <= hay@.len(),
    ensures
        r matches Some(i) ==> {
            &&& from <= i < hay@.len()
            &&& hay@[i as int] == b
            &&& forall|j: int| from <= j < i ==> hay@[j] != b
        },
        r is None ==> forall|j: int| from <= j < hay@.len() ==> hay@[j] != b,
{
    let ones: u64 = 0x0101_0101_0101_0101;
    let highs: u64 = 0x8080_8080_8080_8080;
    let pattern = ones * b as u64;
    let (words, rest) = hay[from..].as_chunks::<8>();
    let mut at = from;
    for word in words {
        // `x` では `b` に等しいバイトが 0 になり、`zeros` は 0 のバイトの最上位の bit に立つ。引き算の借りで、0 の
        // バイトより上のバイトにも bit が立ちうる。借りは上にしか伝わらないので、一番下の bit は最初の 0 のバイトの
        // ものである
        let x = u64::from_le_bytes(*word) ^ pattern;
        let zeros = x.wrapping_sub(ones) & !x & highs;
        if zeros != 0 {
            return Some(at + (zeros.trailing_zeros() / 8) as usize);
        }
        at += 8;
    }
    let mut k = 0;
    while k < rest.len() {
        if rest[k] == b {
            return Some(at + k);
        }
        k += 1;
    }
    None
}

/// `from` 以上で `needle` が始まる最小の位置。T13 の `find_byte` で `needle` の先頭のバイトを探し、見つけた位置
/// ごとに残りを比べる。なければ -1 を返す。
pub(crate) fn find_bytes(text: &[u8], needle: &[u8], from: usize) -> (r: i64)
    requires
        from <= text@.len(),
        text@.len() <= i64::MAX,
    ensures
        found_from(text@, needle@, from as int, r),
{
    if needle.is_empty() {
        proof {
            assert(text@.subrange(from as int, from as int) =~= needle@);
        }
        return from as i64;
    }
    if needle.len() > text.len() {
        return -1;
    }
    let last = text.len() - needle.len();
    let first = needle[0];
    let mut i = from;
    while i <= last
        invariant
            from <= i,
            0 < needle@.len(),
            first == needle@[0],
            last + needle@.len() == text@.len(),
            text@.len() <= i64::MAX,
            forall|j: int| from <= j < i ==> !matches_at(text@, needle@, j),
        decreases last + 1 - i,
    {
        let found = find_byte(text, i, first);
        let end = match found {
            Some(p) => p,
            None => text.len(),
        };
        proof {
            assert forall|j: int| i <= j < end implies !matches_at(text@, needle@, j) by {
                if matches_at(text@, needle@, j) {
                    assert(text@.subrange(j, j + needle@.len())[0] == text@[j]);
                }
            }
        }
        if end > last {
            return -1;
        }
        if starts_at(text, needle, end) {
            return end as i64;
        }
        i = end + 1;
    }
    -1
}

impl<M: Mode> Ctx<M> {
    /// `String.byte_length`。`s` を借りて、バイトの数を返す。呼ぶ側の参照の数は変わらない。
    ///
    /// # Safety
    /// `copy_string` と同じ。
    pub unsafe fn string_byte_length(&mut self, s: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(old(self).text_of(s).len() as i64)
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_string(s) && s0.holds(s);
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
                s0.lemma_settled_body(h1, s, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let bytes = self.heap.string_text(s).as_bytes();
        let len = bytes.len() as i64;
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

    /// `String.byte_at`。`s` を借りて、位置 `i` のバイトを返す。範囲の外なら `Fault::Index` を返す。呼ぶ側の参照の数は
    /// 変わらない。
    ///
    /// # Safety
    /// `copy_string` と同じ。
    pub unsafe fn string_byte_at(&mut self, i: i64, s: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) && !(r matches Err(
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
        let ghost c = s0.held_ok() && s0.is_string(s) && s0.holds(s);
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
                s0.lemma_settled_body(h1, s, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let r = {
            let bytes = self.heap.string_text(s).as_bytes();
            let len = bytes.len();
            if 0 <= i && (i as u64) < (len as u64) {
                Ok(bytes[i as usize] as i64)
            } else {
                Err(Fault::Index(Box::new(IndexFault::String { index: i, len: len as i64 })))
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

    /// `String.slice`。`s` を借りて、バイトの位置 `start` から `end` の手前までの部分の新しい文字列を返す。
    /// 範囲が `0 <= start <= end <= len` でないか、端が文字の境界にないときは、何も作らずに `Fault::Index` を返す。
    /// 呼ぶ側の参照の数は、どちらでも変わらない。
    ///
    /// # Safety
    /// `copy_string` と同じ。
    pub unsafe fn string_slice(&mut self, start: i64, end: i64, s: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> {
                let text = old(self).text_of(s);
                &&& 0 <= start <= end <= text.len()
                &&& is_char_boundary(text, start as int)
                &&& is_char_boundary(text, end as int)
                &&& final(self).is_string(w)
                &&& final(self).text_of(w) == text.subrange(start as int, end as int)
            },
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> {
                let text = old(self).text_of(s);
                &&& (0 <= start <= end <= text.len() && is_char_boundary(text, start as int)
                    && is_char_boundary(text, end as int)) ==> !(r matches Err(Fault::Index(_)))
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
        let ghost c = s0.held_ok() && s0.is_string(s) && s0.holds(s);
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
                s0.lemma_settled_body(h1, s, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let ghost text = h1.text_of(s);
        // 範囲と境界を、確保より前に確かめる。誤りなら何も作らない
        let fault = {
            let t = self.heap.string_text(s);
            let bytes = t.as_bytes();
            let len = bytes.len();
            if !(0 <= start && start <= end && (end as u64) <= (len as u64)) {
                Some(IndexFault::Slice { start, end, len: len as i64 })
            } else if !(t.is_char_boundary(start as usize) && t.is_char_boundary(end as usize)) {
                Some(IndexFault::Boundary { start, end })
            } else {
                None
            }
        };
        if let Some(fault) = fault {
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, h1, s0.heap);
                    self.lemma_words_kept(s0);
                }
                if c {
                    self.lemma_lent(s0, h1);
                }
            }
            return Err(Fault::Index(Box::new(fault)));
        }
        let (from, to) = (start as usize, end as usize);
        let w = self.heap.alloc_string_room(to - from);
        let ghost h2 = self.heap;
        proof {
            h2.lemma_text_spec(w);
            self.lemma_room_made(h1, s, w, Kind::String);
        }
        self.heap.append_part(w, s, from, to);
        let ghost h3 = self.heap;
        proof {
            assert(h3.text_of(w) =~= text.subrange(start as int, end as int));
            h3.lemma_text_spec(s);
            h3.lemma_text_spec(w);
            if c {
                s0.lemma_text_shown(h1, h2, h3, s, w, Kind::String);
            }
        }
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                (to - from) as u64,
            );
        }
        proof {
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                lemma_words_trans(h2, h1, s0.heap);
                assert(h3.words_kept(h2));
                lemma_words_trans(h3, h2, s0.heap);
                lemma_words_trans(self.heap, h3, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_shown_lent(s0, h1, h2, h3, s, w, Kind::String);
            }
        }
        Ok(w)
    }

    /// `String.find_from`。`needle` と `s` を借りて、`from` 以上で
    /// `needle` が始まる最小の位置を返し、なければ -1 を返す。`from` が `0 <= from <= len` でなければ `Fault::Index` を
    /// 返す。
    ///
    /// # Safety
    /// `string_eq` と同じ (`a` が `needle`、`b` が `s`)。
    pub unsafe fn string_find_from(&mut self, needle: Word, from: i64, s: Word) -> (r: Result<
        i64,
        Fault,
    >)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(needle),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).lends_strings(needle, s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Index(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).lends_strings(needle, s) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends_strings(needle, s) && !(r matches Err(
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
        let ghost c = s0.held_ok() && s0.lends_strings(needle, s);
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
                s0.lemma_settled_lent(h1, needle, s, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(ha, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        if let Err(e) = self.heap.checked_body(hb, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let r = {
            let n = self.heap.string_text(needle).as_bytes();
            let t = self.heap.string_text(s).as_bytes();
            let len = t.len();
            if 0 <= from && (from as u64) <= (len as u64) {
                // ヒープの文字列は `isize::MAX` より短いので、ここには来ない
                if (len as u64) > (i64::MAX as u64) {
                    too_large();
                }
                Ok(find_bytes(t, n, from as usize))
            } else {
                Err(Fault::Index(Box::new(IndexFault::String { index: from, len: len as i64 })))
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
}

} // verus!

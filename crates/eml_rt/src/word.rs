//! 8 バイトの語 (docs/spec/object-model.md の「語」)。下位ビットが 1 なら即値、0 なら
//! ヘッダを指すポインタである。`u64` でなくポインタ型で持つのは、strict provenance の下でポインタの出所を失わないためである。

// Verus の外のビルドでは、`global` の宣言が中括弧で囲んだ値に展開され、`unused_braces` の警告が出る
#![allow(unused_braces)]

use std::fmt;
use std::ptr;

use crate::meta::Header;
#[cfg(verus_only)]
use vstd::arithmetic::power2::{lemma2_to64_rest, pow2};
use vstd::prelude::*;

// 語を 8 バイトにするので、64 ビットの環境だけを対象にする (docs/spec/object-model.md の「受け入れる制約」)。Verus の
// 証明の `global size_of usize == 8` も、この前提に頼る
const _: () = assert!(usize::BITS == 64);

verus! {

global size_of usize == 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Word(*mut ());

/// vstd は `without_provenance_mut` に約束を持たない。`with_addr` は const fn でないので、`Word::imm` を const fn の
/// まま書くためにこれを使う (信頼の一覧の T5)。
pub assume_specification<T>[ core::ptr::without_provenance_mut::<T> ](addr: usize) -> (p: *mut T)
    ensures
        p@.addr == addr,
        p@.provenance == vstd::raw_ptr::Provenance::null(),
        p@.metadata == (),
;

/// 即値に収まらない `Word::imm` を止める。`Word::imm` は const fn なので、panic の関数も const fn にする (信頼の一覧の T5)。
#[verifier::external_body]
#[cold]
const fn not_an_immediate() -> ! {
    panic!("an integer that does not fit in 63 bits")
}

impl Word {
    // `-(1 << 62)` と `(1 << 62) - 1` を 16 進で書くのは、Verus が定数の式のシフトを桁あふれのおそれと報告するためである
    pub const MIN_INT: i64 = -0x4000_0000_0000_0000;

    pub const MAX_INT: i64 = 0x3FFF_FFFF_FFFF_FFFF;

    /// 語のビット列。
    pub closed spec fn bits_spec(self) -> usize {
        self.0@.addr
    }

    pub open spec fn is_imm_spec(self) -> bool {
        is_imm_spec(self.bits_spec())
    }

    pub open spec fn is_ptr_spec(self) -> bool {
        is_ptr_spec(self.bits_spec())
    }

    /// `()`。値 0 の即値で、語の値は 1 である。生の `unit` の値もこの語の値にする。
    pub exec const UNIT: Word
        ensures
            Self::UNIT.bits_spec() == 1,
    {
        assert(((0i64 << 1) | 1) as usize == 1) by (bit_vector);
        Word::imm(0)
    }

    /// 空の token と、まだ値を入れていない位置。即値でもポインタでもない。
    pub exec const EMPTY: Word
        ensures
            Self::EMPTY.bits_spec() == 0,
    {
        Word(ptr::without_provenance_mut(0))
    }

    /// 即値 `(n << 1) | 1`。n は 63 ビットに収まらなければならない。
    #[inline]
    pub const fn imm(n: i64) -> (r: Word)
        ensures
            fits_imm_spec(n),
            r.bits_spec() == imm_spec(n),
    {
        if !fits_imm(n) {
            not_an_immediate();
        }
        Word(ptr::without_provenance_mut(imm_bits(n)))
    }

    /// 添字 `i` の即値。63 ビットに収まらなければ止める。
    #[inline]
    pub(crate) fn imm_index(i: usize) -> (r: Word)
        ensures
            i <= Word::MAX_INT,
            r.bits_spec() == imm_spec(i as i64),
            imm_value_spec(r.bits_spec()) == i,
            r.bits_spec() != 0,
    {
        if i > Word::MAX_INT as usize {
            not_an_immediate();
        }
        let r = Word::imm(i as i64);
        proof {
            lemma_imm_round_trip(i as i64);
            let bits = r.bits_spec();
            assert(bits & 1 == 1 ==> bits != 0) by (bit_vector);
        }
        r
    }

    /// 63 ビットに収まる `Int` を即値にする。収まらなければ `None` で、呼ぶ側が `BigInt` の物体にする。
    #[inline]
    pub fn int(n: i64) -> (r: Option<Word>)
        ensures
            r is Some <==> fits_imm_spec(n),
            r matches Some(w) ==> w.bits_spec() == imm_spec(n),
    {
        if fits_imm(n) {
            Some(Word::imm(n))
        } else {
            None
        }
    }

    #[inline]
    pub fn is_imm(self) -> (r: bool)
        ensures
            r == self.is_imm_spec(),
    {
        is_imm_bits(self.0.addr())
    }

    #[inline]
    pub fn is_ptr(self) -> (r: bool)
        ensures
            r == self.is_ptr_spec(),
    {
        is_ptr_bits(self.0.addr())
    }

    #[inline]
    pub fn is_empty(self) -> (r: bool)
        ensures
            r == (self.bits_spec() == 0),
    {
        self.0.addr() == 0
    }

    /// 即値の値。算術右シフトで1ビットずらす。
    #[inline]
    pub fn imm_value(self) -> (r: i64)
        requires
            self.is_imm_spec(),
        ensures
            r == imm_value_spec(self.bits_spec()),
    {
        imm_value_bits(self.0.addr())
    }

    /// 整数のビット列を入れた語。数の語、`BigInt` の値、生の語をペイロードに書くときに使う。provenance は持たない。
    #[inline]
    pub(crate) const fn from_bits(bits: usize) -> (w: Word)
        ensures
            w.bits_spec() == bits,
            w.ptr_spec()@.provenance == vstd::raw_ptr::Provenance::null(),
    {
        Word(ptr::without_provenance_mut(bits))
    }

    /// 語のポインタ。provenance を含む。
    pub(crate) closed spec fn ptr_spec(self) -> *mut () {
        self.0
    }

    /// ヘッダを指す語。provenance を保つ。
    #[inline]
    pub(crate) fn from_ptr(h: *mut Header) -> (w: Word)
        ensures
            w.ptr_spec()@.addr == h@.addr,
            w.ptr_spec()@.provenance == h@.provenance,
            w.bits_spec() == h@.addr,
    {
        Word(h as *mut ())
    }

    /// 語の指すヘッダ。provenance を保つ。
    #[inline]
    pub(crate) fn as_ptr(self) -> (h: *mut Header)
        ensures
            h@.addr == self.ptr_spec()@.addr,
            h@.provenance == self.ptr_spec()@.provenance,
            h@.addr == self.bits_spec(),
    {
        self.0 as *mut Header
    }

    /// 語のビット列。VM が生のスロットに置くときに使う。
    #[inline]
    pub fn bits(self) -> (r: usize)
        ensures
            r == self.bits_spec(),
    {
        self.0.addr()
    }
}

// ビット列の計算を整数の関数に分け、`Word` のメソッドはそれを呼ぶ。仕様は値の言葉で書き、性質は補題に書く
pub open spec fn fits_imm_spec(n: i64) -> bool {
    Word::MIN_INT <= n <= Word::MAX_INT
}

pub open spec fn imm_spec(n: i64) -> usize {
    ((n << 1) | 1) as usize
}

pub open spec fn is_imm_spec(bits: usize) -> bool {
    bits & 1 == 1
}

pub open spec fn is_ptr_spec(bits: usize) -> bool {
    bits & 1 == 0 && bits != 0
}

pub open spec fn imm_value_spec(bits: usize) -> i64 {
    (bits as i64) >> 1
}

/// 63 ビットに収まるか。
#[inline]
pub(crate) const fn fits_imm(n: i64) -> (r: bool)
    ensures
        r == fits_imm_spec(n),
{
    Word::MIN_INT <= n && n <= Word::MAX_INT
}

/// 即値のビット列 `(n << 1) | 1`。
#[inline]
pub(crate) const fn imm_bits(n: i64) -> (r: usize)
    ensures
        r == imm_spec(n),
{
    ((n << 1) | 1) as usize
}

#[inline]
pub(crate) const fn is_imm_bits(bits: usize) -> (r: bool)
    ensures
        r == is_imm_spec(bits),
{
    bits & 1 == 1
}

/// ポインタの語のビット列か。空の語 (0) はポインタでない。
#[inline]
pub(crate) const fn is_ptr_bits(bits: usize) -> (r: bool)
    ensures
        r == is_ptr_spec(bits),
{
    bits & 1 == 0 && bits != 0
}

/// 即値の値。算術右シフトで1ビットずらす。
#[inline]
pub(crate) const fn imm_value_bits(bits: usize) -> (r: i64)
    ensures
        r == imm_value_spec(bits),
{
    (bits as i64) >> 1
}

// 補題
/// 2つの定数が 63 ビットの符号付き整数の範囲 (-2^62 から 2^62 - 1) の両端であることを示す。16 進で書いた値の
/// 書き誤りを、ここで見つけるためである
proof fn lemma_int_bounds()
    ensures
        Word::MIN_INT == -pow2(62),
        Word::MAX_INT == pow2(62) - 1,
{
    lemma2_to64_rest();
}

/// 63 ビットに収まる整数は、即値のビット列にして取り出すと元に戻り、ポインタの語と見分けられる
pub proof fn lemma_imm_round_trip(n: i64)
    requires
        fits_imm_spec(n),
    ensures
        is_imm_spec(imm_spec(n)),
        !is_ptr_spec(imm_spec(n)),
        imm_value_spec(imm_spec(n)) == n,
{
    let bits = imm_spec(n);
    assert(bits & 1 == 1) by (bit_vector)
        requires
            bits == ((n << 1) | 1) as usize,
    ;
    assert((bits as i64) >> 1 == n) by (bit_vector)
        requires
            bits == ((n << 1) | 1) as usize,
            -0x4000_0000_0000_0000i64 <= n <= 0x3FFF_FFFF_FFFF_FFFFi64,
    ;
}

/// `()` の語の値 1 は即値で、ポインタではない
pub proof fn lemma_unit_bits()
    ensures
        is_imm_spec(1),
        !is_ptr_spec(1),
{
    assert(1usize & 1 == 1) by (bit_vector);
}

/// 語に揃った 0 でない番地は、ポインタの語であり、即値ではない
pub proof fn lemma_aligned_is_ptr(bits: usize)
    requires
        bits % 8 == 0,
        bits != 0,
    ensures
        is_ptr_spec(bits),
        !is_imm_spec(bits),
{
    assert(bits & 1 == 0) by (bit_vector)
        requires
            bits % 8 == 0,
    ;
}

/// 即値とポインタの語の判定は重ならず、空の語 (0) はどちらでもない
pub proof fn lemma_imm_and_ptr_are_disjoint(bits: usize)
    ensures
        !(is_imm_spec(bits) && is_ptr_spec(bits)),
        !is_imm_spec(0),
        !is_ptr_spec(0),
{
    assert(!(bits & 1 == 1 && bits & 1 == 0)) by (bit_vector);
    assert(0usize & 1 == 0) by (bit_vector);
}

/// 番地と provenance が同じ語は、同じ語である
pub(crate) proof fn lemma_same_ptr(a: Word, b: Word)
    requires
        a.bits_spec() == b.bits_spec(),
        a.ptr_spec()@.provenance == b.ptr_spec()@.provenance,
    ensures
        a == b,
{
    assert(a.ptr_spec()@ == b.ptr_spec()@);
    assert(a.ptr_spec() == b.ptr_spec());
}

} // verus!
// 語は値で、スレッドをまたいでよいかは型でなく `share` の規約が守る
unsafe impl Send for Word {}
unsafe impl Sync for Word {}

impl fmt::Debug for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_imm() {
            write!(f, "Word::imm({})", self.imm_value())
        } else if self.is_empty() {
            f.write_str("Word::EMPTY")
        } else {
            write!(f, "Word({:#x})", self.0.addr())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediates_round_trip_at_the_63_bit_bounds() {
        for n in [0, 1, -1, Word::MIN_INT, Word::MAX_INT] {
            let w = Word::int(n).unwrap();
            assert!(w.is_imm());
            assert!(!w.is_ptr());
            assert_eq!(w.imm_value(), n);
        }
        assert_eq!(Word::int(Word::MAX_INT + 1), None);
        assert_eq!(Word::int(Word::MIN_INT - 1), None);
        assert_eq!(Word::int(i64::MIN), None);
    }

    #[test]
    fn unit_is_the_immediate_zero_whose_bits_are_one() {
        assert_eq!(Word::UNIT, Word::imm(0));
        assert_eq!(Word::UNIT.bits(), 1);
    }

    #[test]
    fn the_empty_word_is_neither_an_immediate_nor_a_pointer() {
        assert!(Word::EMPTY.is_empty());
        assert!(!Word::EMPTY.is_imm());
        assert!(!Word::EMPTY.is_ptr());
    }
}

//! 8 バイトの語 (docs/spec/object-model.md の「語」)。下位ビットが 1 なら即値、0 なら
//! ヘッダを指すポインタである。`u64` でなくポインタ型で持つのは、strict provenance の下でポインタの出所を失わないためである。

use std::fmt;
use std::ptr::{self, NonNull};

use crate::meta::Header;

// 語を 8 バイトにするので、64 ビットの環境だけを対象にする (docs/spec/object-model.md の「受け入れる制約」)。Verus の
// 写しの `global size_of usize == 8` も、この前提に頼る
const _: () = assert!(usize::BITS == 64);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Word(*mut ());

// 語は値で、スレッドをまたいでよいかは型でなく `share` の規約が守る
unsafe impl Send for Word {}
unsafe impl Sync for Word {}

impl Word {
    // `-(1 << 62)` と `(1 << 62) - 1` を 16 進で書くのは、Verus がシフトを桁あふれのおそれと報告し、写し
    // (`verus/word.rs`) と同じ文字にそろえるためである
    pub const MIN_INT: i64 = -0x4000_0000_0000_0000;
    pub const MAX_INT: i64 = 0x3FFF_FFFF_FFFF_FFFF;
    /// `()`。値 0 の即値で、語の値は 1 である。生の `unit` の値もこの語の値にする。
    pub const UNIT: Word = Word::imm(0);
    /// 空の token と、まだ値を入れていない位置。即値でもポインタでもない。
    pub const EMPTY: Word = Word(ptr::null_mut());

    /// 即値 `(n << 1) | 1`。n は 63 ビットに収まらなければならない。
    #[inline]
    pub const fn imm(n: i64) -> Word {
        assert!(fits_imm(n));
        Word(ptr::without_provenance_mut(imm_bits(n)))
    }

    /// 63 ビットに収まる `Int` を即値にする。収まらなければ `None` で、呼ぶ側が `BigInt` の物体にする。
    #[inline]
    pub fn int(n: i64) -> Option<Word> {
        if fits_imm(n) {
            Some(Word::imm(n))
        } else {
            None
        }
    }

    #[inline]
    pub fn is_imm(self) -> bool {
        is_imm_bits(self.0.addr())
    }

    #[inline]
    pub fn is_ptr(self) -> bool {
        is_ptr_bits(self.0.addr())
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0.is_null()
    }

    /// 即値の値。算術右シフトで1ビットずらす。
    #[inline]
    pub fn imm_value(self) -> i64 {
        debug_assert!(self.is_imm());
        imm_value_bits(self.0.addr())
    }

    /// 語のビット列。VM が生のスロットに置くときに使う。
    #[inline]
    pub fn bits(self) -> usize {
        self.0.addr()
    }

    #[inline]
    pub(crate) fn from_header(h: NonNull<Header>) -> Word {
        Word(h.as_ptr().cast())
    }

    #[inline]
    pub(crate) fn header(self) -> NonNull<Header> {
        debug_assert!(self.is_ptr());
        // ポインタの語は確保したヘッダから作るので、null にならない
        unsafe { NonNull::new_unchecked(self.0.cast()) }
    }
}

// ビット列の計算は、ポインタ型を扱えない Verus の写し (`verus/word.rs`) で証明するため、整数の関数に分ける
// (docs/implementation/testing.md の「Verus」)。

/// 63 ビットに収まるか。
#[inline]
pub(crate) const fn fits_imm(n: i64) -> bool {
    Word::MIN_INT <= n && n <= Word::MAX_INT
}

/// 即値のビット列 `(n << 1) | 1`。
#[inline]
pub(crate) const fn imm_bits(n: i64) -> usize {
    ((n << 1) | 1) as usize
}

#[inline]
pub(crate) const fn is_imm_bits(bits: usize) -> bool {
    bits & 1 == 1
}

/// ポインタの語のビット列か。空の語 (0) はポインタでない。
#[inline]
pub(crate) const fn is_ptr_bits(bits: usize) -> bool {
    bits & 1 == 0 && bits != 0
}

/// 即値の値。算術右シフトで1ビットずらす。
#[inline]
pub(crate) const fn imm_value_bits(bits: usize) -> i64 {
    (bits as i64) >> 1
}

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

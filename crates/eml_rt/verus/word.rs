//! `word.rs` の即値のビット列の計算の写しと証明 (spec の「Verus」)。

use vstd::prelude::*;

verus! {

global size_of usize == 8;

/// 本体の `Word` はポインタ型なので、写しは2つの定数だけを持つ
pub struct Word;

impl Word {
    pub const MIN_INT: i64 = -0x4000_0000_0000_0000;
    pub const MAX_INT: i64 = 0x3FFF_FFFF_FFFF_FFFF;
}

// 仕様。本体の関数がそれぞれこの値を返すことを示し、性質は値の言葉で補題に書く

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

pub(crate) const fn fits_imm(n: i64) -> (r: bool)
    ensures
        r == (Word::MIN_INT <= n && n <= Word::MAX_INT),
{
    Word::MIN_INT <= n && n <= Word::MAX_INT
}

pub(crate) const fn imm_bits(n: i64) -> (r: usize)
    ensures
        r == imm_spec(n),
{
    ((n << 1) | 1) as usize
}

pub(crate) const fn is_imm_bits(bits: usize) -> (r: bool)
    ensures
        r == is_imm_spec(bits),
{
    bits & 1 == 1
}

pub(crate) const fn is_ptr_bits(bits: usize) -> (r: bool)
    ensures
        r == is_ptr_spec(bits),
{
    bits & 1 == 0 && bits != 0
}

pub(crate) const fn imm_value_bits(bits: usize) -> (r: i64)
    ensures
        r == imm_value_spec(bits),
{
    (bits as i64) >> 1
}

// 補題

/// 63 ビットに収まる整数は、即値のビット列にして取り出すと元に戻り、ポインタの語と見分けられる
proof fn lemma_imm_round_trip(n: i64)
    requires
        Word::MIN_INT <= n <= Word::MAX_INT,
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

/// 即値とポインタの語の判定は重ならず、空の語 (0) はどちらでもない
proof fn lemma_imm_and_ptr_are_disjoint(bits: usize)
    ensures
        !(is_imm_spec(bits) && is_ptr_spec(bits)),
        !is_imm_spec(0),
        !is_ptr_spec(0),
{
    assert(!(bits & 1 == 1 && bits & 1 == 0)) by (bit_vector);
    assert(0usize & 1 == 0) by (bit_vector);
}

} // verus!

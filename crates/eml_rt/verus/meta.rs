//! `meta.rs` の `Kind` と `Meta` の写しと証明 (docs/implementation/testing.md の「Verus」)。

use vstd::prelude::*;

verus! {

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Data = 0,
    BigInt = 1,
    Foreign = 2,
    Free = 15,
}

/// 種類のビットが表す種類。0、1、2、15 のほかは `Meta` の正しさで除く
pub open spec fn kind_of(bits: u32) -> Kind {
    if bits == 0 {
        Kind::Data
    } else if bits == 1 {
        Kind::BigInt
    } else if bits == 2 {
        Kind::Foreign
    } else {
        Kind::Free
    }
}

pub open spec fn known_kind(bits: u32) -> bool {
    bits == 0 || bits == 1 || bits == 2 || bits == 15
}

impl Kind {
    fn from_bits(bits: u32) -> (r: Kind)
        requires
            known_kind(bits),
        ensures
            r == kind_of(bits),
    {
        match bits {
            0 => Kind::Data,
            1 => Kind::BigInt,
            2 => Kind::Foreign,
            15 => Kind::Free,
            _ => unreachable!("an unknown object kind {bits}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Meta(u32);

impl Meta {
    const LIN: u32 = 1 << 31;
    const KIND_SHIFT: u32 = 24;
    const KIND_MASK: u32 = 0xF;

    pub const fn new(tag: u16, scan: u8, kind: Kind, lin: bool) -> (r: Meta)
        ensures
            r.wf(),
            r.tag_spec() == tag,
            r.scan_spec() == scan,
            r.kind_spec() == kind,
            r.lin_spec() == lin,
    {
        proof {
            lemma_kind_bits(kind);
            lemma_consts();
        }
        let lin = if lin { Meta::LIN } else { 0 };
        proof {
            lemma_pack(tag, scan, kind as u32, lin);
        }
        Meta(tag as u32 | ((scan as u32) << 16) | ((kind as u32) << Meta::KIND_SHIFT) | lin)
    }

    pub const fn tag(self) -> (r: u16)
        ensures
            r == self.tag_spec(),
    {
        self.0 as u16
    }

    pub const fn scan(self) -> (r: u8)
        ensures
            r == self.scan_spec(),
    {
        (self.0 >> 16) as u8
    }

    pub fn kind(self) -> (r: Kind)
        requires
            self.wf(),
        ensures
            r == self.kind_spec(),
    {
        Kind::from_bits((self.0 >> Meta::KIND_SHIFT) & Meta::KIND_MASK)
    }

    pub const fn lin(self) -> (r: bool)
        ensures
            r == self.lin_spec(),
    {
        proof {
            lemma_consts();
        }
        self.0 & Meta::LIN != 0
    }

    pub const fn with_lin(self, lin: bool) -> (r: Meta)
        ensures
            self.wf() ==> r.wf(),
            r.lin_spec() == lin,
            r.tag_spec() == self.tag_spec(),
            r.scan_spec() == self.scan_spec(),
            r.kind_bits() == self.kind_bits(),
            r.kind_spec() == self.kind_spec(),
    {
        proof {
            lemma_consts();
            lemma_with_lin(self.0, if lin { Meta::LIN } else { 0 });
        }
        Meta((self.0 & !Meta::LIN) | if lin { Meta::LIN } else { 0 })
    }

    pub const fn same_shape(self, other: Meta) -> (r: bool)
        ensures
            self.wf() && other.wf() ==> (r <==> same_fields(self, other)),
    {
        proof {
            lemma_consts();
            if self.wf() && other.wf() {
                lemma_same_shape(self.0, other.0);
            }
        }
        (self.0 ^ other.0) & !Meta::LIN == 0
    }
}

// 仕様。ビットの位置は、定数を使わずに docs/spec/object-model.md の「ヘッダ」の並びのとおりに書く

impl Meta {
    pub closed spec fn tag_spec(self) -> u16 {
        self.0 as u16
    }

    pub closed spec fn scan_spec(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub closed spec fn kind_bits(self) -> u32 {
        (self.0 >> 24) & 0xF
    }

    pub closed spec fn kind_spec(self) -> Kind {
        kind_of(self.kind_bits())
    }

    pub closed spec fn lin_spec(self) -> bool {
        self.0 & 0x8000_0000 != 0
    }

    /// 正しい `Meta`。種類のビットは知っている種類で、`info` の予約の3ビットは 0 である。予約のビットを 0 に保つので、
    /// `same_shape` は tag と scan と種類だけを比べることになる
    pub closed spec fn wf(self) -> bool {
        known_kind(self.kind_bits()) && self.0 & 0x7000_0000 == 0
    }
}

pub open spec fn same_fields(a: Meta, b: Meta) -> bool {
    a.tag_spec() == b.tag_spec() && a.scan_spec() == b.scan_spec() && a.kind_spec() == b.kind_spec()
}

// 補題

/// 本体の定数を、仕様が使うビットの位置に結び付ける
proof fn lemma_consts()
    ensures
        Meta::LIN == 0x8000_0000,
        Meta::KIND_SHIFT == 24,
        Meta::KIND_MASK == 0xF,
{
    assert(1u32 << 31 == 0x8000_0000) by (bit_vector);
}

/// `kind as u32` は宣言の判別子なので、4ビットに収まる知っている種類である
proof fn lemma_kind_bits(kind: Kind)
    ensures
        known_kind(kind as u32),
        kind_of(kind as u32) == kind,
{
}

/// 詰めた語から、tag、scan、種類、`lin` ビットがそれぞれ元のとおりに取り出せ、予約のビットは 0 である
proof fn lemma_pack(tag: u16, scan: u8, k: u32, l: u32)
    by (bit_vector)
    requires
        k < 16,
        l == 0 || l == 0x8000_0000,
    ensures
        (tag as u32 | ((scan as u32) << 16) | (k << 24) | l) as u16 == tag,
        ((tag as u32 | ((scan as u32) << 16) | (k << 24) | l) >> 16) as u8 == scan,
        ((tag as u32 | ((scan as u32) << 16) | (k << 24) | l) >> 24) & 0xF == k,
        (tag as u32 | ((scan as u32) << 16) | (k << 24) | l) & 0x8000_0000 != 0 <==> l != 0,
        (tag as u32 | ((scan as u32) << 16) | (k << 24) | l) & 0x7000_0000 == 0,
{
}

/// `lin` ビットを差し替えても、ほかのビットは変わらない
proof fn lemma_with_lin(m: u32, l: u32)
    by (bit_vector)
    requires
        l == 0 || l == 0x8000_0000,
    ensures
        ((m & !0x8000_0000u32) | l) as u16 == m as u16,
        (((m & !0x8000_0000u32) | l) >> 16) as u8 == (m >> 16) as u8,
        (((m & !0x8000_0000u32) | l) >> 24) & 0xF == (m >> 24) & 0xF,
        ((m & !0x8000_0000u32) | l) & 0x8000_0000 != 0 <==> l != 0,
        ((m & !0x8000_0000u32) | l) & 0x7000_0000 == m & 0x7000_0000,
{
}

/// 予約のビットが 0 なら、`lin` ビットを除いて同じことは、tag と scan と種類のビットが同じことである
proof fn lemma_same_shape(a: u32, b: u32)
    by (bit_vector)
    requires
        a & 0x7000_0000 == 0,
        b & 0x7000_0000 == 0,
    ensures
        (a ^ b) & !0x8000_0000u32 == 0 <==> (a as u16 == b as u16 && (a >> 16) as u8 == (b >> 16)
            as u8 && (a >> 24) & 0xF == (b >> 24) & 0xF),
{
}

} // verus!

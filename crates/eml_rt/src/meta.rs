//! 物体のヘッダと種類 (docs/spec/object-model.md の「ヘッダ」と「種類」)。

use std::fmt;
use std::ptr::NonNull;

use crate::word::Word;
use std::sync::atomic::AtomicI32;
use vstd::prelude::*;

verus! {

/// 物体の種類。`info` の下位4ビットに入る。種類ごとの欄 (名前、走査の始まり、token、破棄処理) は、型ごとの表を
/// 持たずにここの `match` で決める。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Data = 0,
    BigInt = 1,
    Foreign = 2,
    /// 確保器が空いたブロックに付ける。生きている物体はこの種類を持たない。
    Free = 15,
}

/// 種類のビットが表す種類。0、1、2、15 のほかは `Meta` の型の不変条件で除く。
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
    #[inline]
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

    /// リークの報告の種類の名前。
    pub fn name(self) -> &'static str {
        match self {
            Kind::Data => "Data",
            Kind::BigInt => "BigInt",
            Kind::Foreign => "Foreign",
            Kind::Free => "Free",
        }
    }

    /// 走査する語 (`scan` が 255 なら数の語) がペイロードの何語目から始まるか。種類を足すときに決め忘れないように、
    /// 種類ごとに書く。
    #[inline]
    pub(crate) fn scan_start(self) -> (r: usize)
        ensures
            r == 0,
    {
        match self {
            Kind::Data | Kind::BigInt | Kind::Foreign | Kind::Free => 0,
        }
    }

    #[inline]
    pub(crate) fn tokenable(self) -> (r: bool)
        ensures
            r == (self == Kind::Data),
    {
        matches!(self, Kind::Data)
    }

    #[inline]
    pub(crate) fn has_finalizer(self) -> (r: bool)
        ensures
            r == (self == Kind::Foreign),
    {
        matches!(self, Kind::Foreign)
    }
}

/// ヘッダの `meta`。tag (16 ビット) | scan (8 ビット) | info (8 ビット。最上位が `lin` ビット、下位4ビットが種類)。
/// 型の不変条件 `wf` (種類のビットが知っている種類で、予約の3ビットが 0) を持つ。公開の `kind` が利用者の作った値を
/// 受けても、`Kind::from_bits` の `unreachable!` に届かないことを示すためである。`Meta` は `Copy` で `&mut` のメソッドを
/// 持たないので、型の不変条件の `no_unwind` の制約に当たらない。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Meta(u32);

impl Meta {
    /// `scan` のこの値は、走査する語の数がペイロードの生の1語にあることを表す。
    pub const EXTENDED_SCAN: u8 = 255;

    const LIN: u32 = 1 << 31;

    const KIND_SHIFT: u32 = 24;

    const KIND_MASK: u32 = 0xF;

    #[inline]
    pub const fn new(tag: u16, scan: u8, kind: Kind, lin: bool) -> (r: Meta)
        ensures
            r.tag_spec() == tag,
            r.scan_spec() == scan,
            r.kind_spec() == kind,
            r.lin_spec() == lin,
    {
        proof {
            lemma_kind_bits(kind);
            lemma_consts();
        }
        let lin = if lin {
            Meta::LIN
        } else {
            0
        };
        proof {
            lemma_pack(tag, scan, kind as u32, lin);
        }
        Meta(tag as u32 | ((scan as u32) << 16) | ((kind as u32) << Meta::KIND_SHIFT) | lin)
    }

    #[inline]
    pub const fn tag(self) -> (r: u16)
        ensures
            r == self.tag_spec(),
    {
        self.0 as u16
    }

    #[inline]
    pub const fn scan(self) -> (r: u8)
        ensures
            r == self.scan_spec(),
    {
        (self.0 >> 16) as u8
    }

    #[inline]
    pub fn kind(self) -> (r: Kind)
        ensures
            r == self.kind_spec(),
    {
        proof {
            use_type_invariant(self);
        }
        Kind::from_bits((self.0 >> Meta::KIND_SHIFT) & Meta::KIND_MASK)
    }

    #[inline]
    pub const fn lin(self) -> (r: bool)
        ensures
            r == self.lin_spec(),
    {
        proof {
            lemma_consts();
        }
        self.0 & Meta::LIN != 0
    }

    #[inline]
    pub const fn with_lin(self, lin: bool) -> (r: Meta)
        ensures
            r.lin_spec() == lin,
            r.tag_spec() == self.tag_spec(),
            r.scan_spec() == self.scan_spec(),
            r.kind_bits() == self.kind_bits(),
            r.kind_spec() == self.kind_spec(),
    {
        proof {
            use_type_invariant(self);
            lemma_consts();
            lemma_with_lin(
                self.0,
                if lin {
                    Meta::LIN
                } else {
                    0
                },
            );
        }
        Meta(
            (self.0 & !Meta::LIN) | if lin {
                Meta::LIN
            } else {
                0
            },
        )
    }

    /// `lin` ビットを除いて同じか。
    #[inline]
    pub const fn same_shape(self, other: Meta) -> (r: bool)
        ensures
            r <==> same_fields(self, other),
    {
        proof {
            use_type_invariant(self);
            use_type_invariant(other);
            lemma_consts();
            lemma_same_shape(self.0, other.0);
        }
        (self.0 ^ other.0) & !Meta::LIN == 0
    }
}

// 仕様。ビットの位置は、定数を使わずに docs/spec/object-model.md の「ヘッダ」の並びのとおりに書く
impl Meta {
    pub closed spec fn bits_spec(self) -> u32 {
        self.0
    }

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
    #[verifier::type_invariant]
    closed spec fn wf(self) -> bool {
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
        (a ^ b) & !0x8000_0000u32 == 0 <==> (a as u16 == b as u16 && (a >> 16) as u8 == (b
            >> 16) as u8 && (a >> 24) & 0xF == (b >> 24) & 0xF),
{
}

} // verus!
impl fmt::Debug for Meta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Meta")
            .field("tag", &self.tag())
            .field("scan", &self.scan())
            .field("kind", &self.kind())
            .field("lin", &self.lin())
            .finish()
    }
}

verus! {

/// 物体のヘッダ。ポインタはここを指す。`&Header` は作らず、`rc` は共有の参照で、`meta` は生のポインタで読み書きする。
/// `meta` を書くのは、ほかのスレッドがその物体を読めないときだけである。
// ヘッダの読み書きはまだ証明していないので、Verus には中身を見せない (信頼の一覧のテストの「未証明」)
#[verifier::external_body]
#[repr(C)]
pub(crate) struct Header {
    rc: AtomicI32,
    meta: u32,
}

} // verus!
/// ヘッダの語の数。8 と書かずに大きさから出し、ヘッダの形を変えたときに数え直す所を1か所にする。
pub(crate) const HEADER_WORDS: usize = size_of::<Header>().div_ceil(8);

/// # Safety
/// `h` は確保したブロックのヘッダの位置を指す。
#[inline]
pub(crate) unsafe fn init(h: NonNull<Header>, rc: i32, meta: Meta) {
    unsafe {
        h.as_ptr().write(Header {
            rc: AtomicI32::new(rc),
            meta: meta.0,
        })
    }
}

/// # Safety
/// `h` は初期化したヘッダを指す。
#[inline]
pub(crate) unsafe fn rc<'a>(h: NonNull<Header>) -> &'a AtomicI32 {
    unsafe { &(*h.as_ptr()).rc }
}

/// # Safety
/// `h` は初期化したヘッダを指す。
#[inline]
pub(crate) unsafe fn meta(h: NonNull<Header>) -> Meta {
    unsafe { Meta((&raw const (*h.as_ptr()).meta).read()) }
}

/// # Safety
/// `h` は初期化したヘッダを指し、ほかのスレッドはその物体を読めない。
#[inline]
pub(crate) unsafe fn set_meta(h: NonNull<Header>, meta: Meta) {
    unsafe { (&raw mut (*h.as_ptr()).meta).write(meta.0) }
}

/// ペイロードの物理的な i 語目の位置。
///
/// # Safety
/// `h` はペイロードが i + 1 語以上あるブロックのヘッダを指す。
#[inline]
pub(crate) unsafe fn slot(h: NonNull<Header>, i: usize) -> *mut Word {
    unsafe { h.as_ptr().cast::<Word>().add(HEADER_WORDS + i) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_keeps_each_field() {
        let m = Meta::new(0xBEEF, 7, Kind::Foreign, true);
        assert_eq!(m.tag(), 0xBEEF);
        assert_eq!(m.scan(), 7);
        assert_eq!(m.kind(), Kind::Foreign);
        assert!(m.lin());
        let plain = m.with_lin(false);
        assert!(!plain.lin());
        assert_eq!(
            (plain.tag(), plain.scan(), plain.kind()),
            (0xBEEF, 7, Kind::Foreign)
        );
    }

    #[test]
    fn same_shape_ignores_only_the_lin_bit() {
        let m = Meta::new(3, 2, Kind::Data, false);
        assert!(m.same_shape(m.with_lin(true)));
        assert!(!m.same_shape(Meta::new(4, 2, Kind::Data, false)));
        assert!(!m.same_shape(Meta::new(3, 1, Kind::Data, false)));
        assert!(!m.same_shape(Meta::new(3, 2, Kind::BigInt, false)));
    }

    #[test]
    fn only_data_becomes_a_token_and_only_foreign_has_a_finalizer() {
        for kind in [Kind::Data, Kind::BigInt, Kind::Foreign, Kind::Free] {
            assert_eq!(kind.tokenable(), kind == Kind::Data);
            assert_eq!(kind.has_finalizer(), kind == Kind::Foreign);
            assert_eq!(kind.scan_start(), 0);
        }
    }

    #[test]
    fn the_header_is_one_word() {
        assert_eq!(HEADER_WORDS, 1);
    }
}

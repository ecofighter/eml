//! 物体のヘッダと種類 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「ヘッダ」と「種類」)。

use std::fmt;
use std::ptr::NonNull;

use crate::sync::AtomicI32;
use crate::word::Word;

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

impl Kind {
    fn from_bits(bits: u32) -> Kind {
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
    pub(crate) fn scan_start(self) -> usize {
        match self {
            Kind::Data | Kind::BigInt | Kind::Foreign | Kind::Free => 0,
        }
    }

    pub(crate) fn tokenable(self) -> bool {
        matches!(self, Kind::Data)
    }

    pub(crate) fn has_finalizer(self) -> bool {
        matches!(self, Kind::Foreign)
    }
}

/// ヘッダの `meta`。tag (16 ビット) | scan (8 ビット) | info (8 ビット。最上位が `lin` ビット、下位4ビットが種類)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Meta(u32);

impl Meta {
    /// `scan` のこの値は、走査する語の数がペイロードの生の1語にあることを表す。
    pub const EXTENDED_SCAN: u8 = 255;
    const LIN: u32 = 1 << 31;
    const KIND_SHIFT: u32 = 24;
    const KIND_MASK: u32 = 0xF;

    pub const fn new(tag: u16, scan: u8, kind: Kind, lin: bool) -> Meta {
        let lin = if lin { Meta::LIN } else { 0 };
        Meta(tag as u32 | ((scan as u32) << 16) | ((kind as u32) << Meta::KIND_SHIFT) | lin)
    }

    pub const fn tag(self) -> u16 {
        self.0 as u16
    }

    pub const fn scan(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub fn kind(self) -> Kind {
        Kind::from_bits((self.0 >> Meta::KIND_SHIFT) & Meta::KIND_MASK)
    }

    pub const fn lin(self) -> bool {
        self.0 & Meta::LIN != 0
    }

    pub const fn with_lin(self, lin: bool) -> Meta {
        Meta((self.0 & !Meta::LIN) | if lin { Meta::LIN } else { 0 })
    }

    /// `lin` ビットを除いて同じか。
    pub const fn same_shape(self, other: Meta) -> bool {
        (self.0 ^ other.0) & !Meta::LIN == 0
    }
}

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

/// 物体のヘッダ。ポインタはここを指す。`&Header` は作らず、`rc` は共有の参照で、`meta` は生のポインタで読み書きする。
/// `meta` を書くのは、ほかのスレッドがその物体を読めないときだけである。
#[repr(C)]
pub(crate) struct Header {
    rc: AtomicI32,
    meta: u32,
}

/// ヘッダの語の数。loom の型に差し替えたビルドではヘッダが大きくなるので、8 と書かずに大きさから出す。
pub(crate) const HEADER_WORDS: usize = size_of::<Header>().div_ceil(8);

/// # Safety
/// `h` は確保したブロックのヘッダの位置を指す。
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
pub(crate) unsafe fn rc<'a>(h: NonNull<Header>) -> &'a AtomicI32 {
    unsafe { &(*h.as_ptr()).rc }
}

/// # Safety
/// `h` は初期化したヘッダを指す。
pub(crate) unsafe fn meta(h: NonNull<Header>) -> Meta {
    unsafe { Meta((&raw const (*h.as_ptr()).meta).read()) }
}

/// # Safety
/// `h` は初期化したヘッダを指し、ほかのスレッドはその物体を読めない。
pub(crate) unsafe fn set_meta(h: NonNull<Header>, meta: Meta) {
    unsafe { (&raw mut (*h.as_ptr()).meta).write(meta.0) }
}

/// ペイロードの物理的な i 語目の位置。
///
/// # Safety
/// `h` はペイロードが i + 1 語以上あるブロックのヘッダを指す。
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

//! 物体のヘッダと種類 (docs/spec/object-model.md の「ヘッダ」と「種類」)。

use std::fmt;

use crate::word::Word;
use vstd::prelude::*;
use vstd::raw_ptr::PointsTo;
#[cfg(verus_only)]
use vstd::raw_ptr::PointsToRaw;
#[cfg(verus_only)]
use vstd::raw_ptr::{PtrData, ptr_mut_from_data};

verus! {

/// 物体の種類。`info` の下位4ビットに入る。種類ごとの欄 (名前、走査の始まり、token、ペイロードが不透明か) は、型ごとの
/// 表を持たずにここの `match` で決める。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Data = 0,
    BigInt = 1,
    Foreign = 2,
    String = 3,
    /// `Float` の箱。値の語 (`f64` のビット列) を1つ持つ。`Int` の即値のような空きのビットがないので、`box` はいつも
    /// 確保する (docs/spec/object-model.md の「種類」)。
    Float = 4,
    /// バイト列。`String` と同じ形 (長さの語とバイト列) で、UTF-8 の不変条件だけを持たない
    /// (docs/spec/object-model.md の「バイト列と文字列」)。
    Bytes = 5,
    /// 確保器が空いたブロックに付ける。生きている物体はこの種類を持たない。
    Free = 15,
}

/// 種類のビットが表す種類。0、1、2、3、4、5、15 のほかは `Meta` の型の不変条件で除く。
pub open spec fn kind_of(bits: u32) -> Kind {
    if bits == 0 {
        Kind::Data
    } else if bits == 1 {
        Kind::BigInt
    } else if bits == 2 {
        Kind::Foreign
    } else if bits == 3 {
        Kind::String
    } else if bits == 4 {
        Kind::Float
    } else if bits == 5 {
        Kind::Bytes
    } else {
        Kind::Free
    }
}

pub open spec fn known_kind(bits: u32) -> bool {
    bits == 0 || bits == 1 || bits == 2 || bits == 3 || bits == 4 || bits == 5 || bits == 15
}

// 種類ごとの欄。破棄処理の欄だけは、箱を空けるたびに読むので、この `match` でなく `Meta::has_finalizer` が持つ
// (docs/spec/object-model.md の「種類」)
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
            3 => Kind::String,
            4 => Kind::Float,
            5 => Kind::Bytes,
            15 => Kind::Free,
            _ => unreachable!("an unknown object kind {bits}"),
        }
    }

    /// 値の語を1つ持つ箱の種類。生きている箱は 0 語目に値を書いてある (docs/spec/object-model.md の「種類」)。
    pub open spec fn boxes_value(self) -> bool {
        self == Kind::BigInt || self == Kind::Float
    }

    /// 長さの語とバイト列の形を持つ種類 (`String` と `Bytes`)。この仲間は ghost のバイト列と `holds_body` を共有し、
    /// 種類で違うのは追加の不変条件 (`String` の UTF-8) だけである (docs/spec/object-model.md の「バイト列と文字列」)。
    pub open spec fn has_body(self) -> bool {
        self == Kind::String || self == Kind::Bytes
    }

    /// リークの報告の種類の名前。
    pub fn name(self) -> &'static str {
        match self {
            Kind::Data => "Data",
            Kind::BigInt => "BigInt",
            Kind::Foreign => "Foreign",
            Kind::String => "String",
            Kind::Float => "Float",
            Kind::Bytes => "Bytes",
            Kind::Free => "Free",
        }
    }

    #[inline]
    pub(crate) fn tokenable(self) -> (r: bool)
        ensures
            r == (self == Kind::Data),
    {
        matches!(self, Kind::Data)
    }

    /// ペイロードが核の操作から見て不透明か。`raw` と `set_raw` はこの種類を断る。`Foreign` は Rust の値を、`String` と
    /// `Bytes` は途中までしか書いていない語を持つので、語として読み書きすると未初期化の読み込みか不変条件の破れになる
    /// (docs/spec/object-model.md の「種類」)。
    #[inline]
    pub(crate) fn opaque(self) -> (r: bool)
        ensures
            r == (self == Kind::Foreign || self.has_body()),
    {
        matches!(self, Kind::Foreign | Kind::String | Kind::Bytes)
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

    /// 種類が破棄処理を持つ (`Foreign` である) か。箱を空けるたびに読むので、`Kind::from_bits` を通さずに種類のビットを
    /// 比べる。`from_bits` の腕を1つ増やしただけで、解放の多い計測の命令の数が数 % 増えたためである
    /// (docs/implementation/benchmarks.md の「破棄処理の判定」)。
    #[inline]
    pub(crate) fn has_finalizer(self) -> (r: bool)
        ensures
            r == (self.kind_spec() == Kind::Foreign),
    {
        proof {
            lemma_consts();
        }
        (self.0 >> Meta::KIND_SHIFT) & Meta::KIND_MASK == Kind::Foreign as u32
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

    /// ヘッダに書く値。
    // ヒープを権限の上で書き直すまで、まだ使わない
    #[inline]
    pub(crate) const fn bits(self) -> (r: u32)
        ensures
            r == self.bits_spec(),
            bits_wf(r),
            bits_kind(r) == self.kind_spec(),
            bits_scan(r) == self.scan_spec(),
            bits_tag(r) == self.tag_spec(),
            bits_lin(r) == self.lin_spec(),
    {
        proof {
            use_type_invariant(self);
        }
        self.0
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

impl Meta {
    /// `meta` の値の欄は、それぞれの欄の spec と同じである。ヒープの述語 (値の言葉) と `Meta` のメソッドの約束
    /// (欄の spec の言葉) をつなぐ。
    pub(crate) proof fn lemma_fields(self)
        ensures
            bits_tag(self.bits_spec()) == self.tag_spec(),
            bits_scan(self.bits_spec()) == self.scan_spec(),
            bits_kind(self.bits_spec()) == self.kind_spec(),
            bits_lin(self.bits_spec()) == self.lin_spec(),
    {
    }
}

/// ヘッダの `meta` の欄の値 (整数) の仕様。ヒープの不変条件は、メモリの値をこの言葉で書く。
pub(crate) open spec fn bits_wf(bits: u32) -> bool {
    known_kind((bits >> 24) & 0xF) && bits & 0x7000_0000 == 0
}

pub(crate) open spec fn bits_kind(bits: u32) -> Kind {
    kind_of((bits >> 24) & 0xF)
}

pub(crate) open spec fn bits_scan(bits: u32) -> u8 {
    (bits >> 16) as u8
}

pub(crate) open spec fn bits_tag(bits: u32) -> u16 {
    bits as u16
}

pub(crate) open spec fn bits_lin(bits: u32) -> bool {
    bits & 0x8000_0000 != 0
}

impl Meta {
    /// ヘッダから読んだ値を `Meta` に戻す。値が正しい形であることは、ヒープの不変条件が持つ。
    #[inline]
    pub(crate) fn from_bits(bits: u32) -> (r: Meta)
        requires
            bits_wf(bits),
        ensures
            r.bits_spec() == bits,
            r.kind_spec() == bits_kind(bits),
            r.scan_spec() == bits_scan(bits),
            r.lin_spec() == bits_lin(bits),
            r.tag_spec() == bits_tag(bits),
    {
        Meta(bits)
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

/// 物体のヘッダ。ポインタはここを指す。`&Header` は作らない。`rc` は、共有の物体ではほかのスレッドが atomic に書き換える
/// ので、ヘッダ全体への参照は rc の atomic の読み書きと両立しないためである。欄は整数で持ち、rc の読み書きは
/// `crate::cell` の関数が `AtomicI32::from_ptr` を通して行う (docs/spec/object-model.md の「`unsafe` の核」)。`meta` を書くのは、
/// ほかのスレッドがその物体を読めないときだけである。
#[repr(C)]
pub(crate) struct Header {
    pub(crate) rc: i32,
    pub(crate) meta: u32,
}

global layout Header is size == 8, align == 4;

// 欄の揃えは環境によるので、vstd は約束しない。対象にする 64 ビットの環境の値を宣言する (信頼の一覧の T6)
global layout u32 is size == 4, align == 4;

global layout i32 is size == 4, align == 4;

global layout Word is size == 8, align == 8;

broadcast use {vstd::layout::layout_of_primitives, vstd::raw_ptr::group_raw_ptr_axioms};

/// ヘッダの語の数。
pub(crate) const HEADER_WORDS: usize = 1;

/// `h` と同じ provenance で、番地を `off` バイトずらしたポインタ。
pub(crate) open spec fn field_ptr<T>(h: *mut Header, off: int) -> *mut T {
    ptr_mut_from_data::<T>(
        PtrData { addr: (h@.addr + off) as usize, provenance: h@.provenance, metadata: () },
    )
}

/// ペイロードの i 語目の番地のずれ。
pub(crate) open spec fn slot_offset(i: int) -> int {
    8 * (HEADER_WORDS + i)
}

#[inline]
pub(crate) fn rc_ptr(h: *mut Header) -> (p: *mut i32)
    ensures
        p == field_ptr::<i32>(h, 0),
{
    h as *mut i32
}

#[inline]
pub(crate) fn meta_ptr(h: *mut Header) -> (p: *mut u32)
    requires
        h@.addr + 4 <= usize::MAX,
    ensures
        p == field_ptr::<u32>(h, 4),
{
    (h as *mut u32).with_addr(h.addr() + 4)
}

/// ペイロードの物理的な i 語目の位置。
#[inline]
pub(crate) fn slot_ptr(h: *mut Header, i: usize) -> (p: *mut Word)
    requires
        h@.addr + slot_offset(i as int) <= usize::MAX,
    ensures
        p == field_ptr::<Word>(h, slot_offset(i as int)),
{
    (h as *mut Word).with_addr(h.addr() + 8 * (HEADER_WORDS + i))
}

/// 検査モードの前置きの2つの欄 (所有者 ID か不死の物体の数、状態と世代の番号) の位置。ヘッダの直前の1語にある。
#[inline]
pub(crate) fn prefix_first_ptr(h: *mut Header) -> (p: *mut u32)
    requires
        h@.addr >= 8,
    ensures
        p == field_ptr::<u32>(h, -8),
{
    (h as *mut u32).with_addr(h.addr() - 8)
}

#[inline]
pub(crate) fn prefix_state_ptr(h: *mut Header) -> (p: *mut u32)
    requires
        h@.addr >= 8,
    ensures
        p == field_ptr::<u32>(h, -4),
{
    (h as *mut u32).with_addr(h.addr() - 4)
}

/// 検査モードの前置きの権限。
// 仕様だけで使う型。Verus の外のビルドでも型は残るので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) tracked struct PrefixPerm {
    pub(crate) tracked first: PointsTo<u32>,
    pub(crate) tracked state: PointsTo<u32>,
}

/// 1つのブロックの権限。ヘッダの2つの欄、ペイロードの各語、検査モードなら前置きを持つ。ペイロードの語はどれも `Word`
/// の権限にし、整数もビット列として `Word` に入れる (docs/spec/object-model.md の「`unsafe` の核」)。
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) tracked struct BlockPerm {
    pub(crate) tracked rc: PointsTo<i32>,
    pub(crate) tracked meta: PointsTo<u32>,
    pub(crate) tracked slots: Map<int, PointsTo<Word>>,
    pub(crate) tracked prefix: Option<PrefixPerm>,
    /// `String` と `Bytes` の物体のバイト列 (`text()`)。ほかの種類では意味を持たない。
    pub(crate) ghost text: Text,
}

/// `String` と `Bytes` の物体のバイト列。メモリのバイト列はこれに等しい (信頼の一覧の T10)。証明が好きな列を書けると
/// T10 の約束が崩れるので、欄を隠して、この module の外では作れないようにする。列を変えるのは
/// `BlockPerm::clear_text`、`BlockPerm::truncate_text`、`crate::text` の `write_bytes` と `write_byte` だけである。
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) ghost struct Text {
    bytes: Seq<u8>,
}

impl Text {
    pub(crate) closed spec fn view(self) -> Seq<u8> {
        self.bytes
    }
}

/// 語に揃った番地から、欄と語の揃えを導く。
proof fn lemma_align(x: int, k: int)
    requires
        x % 8 == 0,
        k >= 0,
    ensures
        (x + 8 * k) % 8 == 0,
        (x + 4) % 4 == 0,
        x % 4 == 0,
        (x - 8) % 4 == 0,
        (x - 4) % 4 == 0,
        align_of::<u32>() == 4,
        align_of::<i32>() == 4,
        align_of::<Word>() == 8,
        size_of::<u32>() == 4,
        size_of::<i32>() == 4,
        size_of::<Word>() == 8,
{
    assert((x + 8 * k) % 8 == 0) by (nonlinear_arith)
        requires
            x % 8 == 0,
            k >= 0,
    ;
    assert((x + 4) % 4 == 0 && x % 4 == 0 && (x - 8) % 4 == 0 && (x - 4) % 4 == 0)
        by (nonlinear_arith)
        requires
            x % 8 == 0,
    ;
}

/// ブロックの始まりの番地 (検査モードでは前置きの分だけ前)。
pub(crate) open spec fn block_start(h: *mut Header, prefixed: bool) -> int {
    h@.addr - if prefixed {
        8int
    } else {
        0int
    }
}

/// ブロックのバイト数。
pub(crate) open spec fn block_size(cap: nat, prefixed: bool) -> int {
    (if prefixed {
        8int
    } else {
        0int
    }) + 8 * (HEADER_WORDS + cap)
}

impl BlockPerm {
    /// 書いていないメモリの権限を、ブロックの欄ごとの権限に分ける。ブロックを切り出すときに使う。
    pub(crate) proof fn from_raw(
        tracked raw: PointsToRaw,
        h: *mut Header,
        cap: nat,
        prefixed: bool,
    ) -> (tracked r: BlockPerm)
        requires
            raw.is_range(block_start(h, prefixed), block_size(cap, prefixed)),
            raw.provenance() == h@.provenance,
            h@.addr % 8 == 0,
            block_start(h, prefixed) > 0,
            block_start(h, prefixed) + block_size(cap, prefixed) <= usize::MAX + 1,
        ensures
            r.wf(h, cap, prefixed),
            r.rc.is_uninit(),
            r.meta.is_uninit(),
            forall|i: int| 0 <= i < cap ==> (#[trigger] r.slots[i]).is_uninit(),
    {
        let start = block_start(h, prefixed);
        lemma_align(h@.addr as int, 0);
        let tracked mut rest = raw;
        let tracked mut prefix = Option::None;
        if prefixed {
            let tracked (first, rest1) = rest.split(Set::range(start, start + 4));
            let tracked (state, rest2) = rest1.split(Set::range(start + 4, start + 8));
            assert(rest2.dom() =~= Set::range(h@.addr as int, start + block_size(cap, prefixed)));
            rest = rest2;
            prefix =
            Option::Some(
                PrefixPerm {
                    first: first.into_typed::<u32>(start as usize),
                    state: state.into_typed::<u32>((start + 4) as usize),
                },
            );
        } else {
            assert(rest.dom() =~= Set::range(h@.addr as int, start + block_size(cap, prefixed)));
        }
        let hs = h@.addr as int;
        let tracked (rc_raw, rest3) = rest.split(Set::range(hs, hs + 4));
        let tracked (meta_raw, rest4) = rest3.split(Set::range(hs + 4, hs + 8));
        assert(rest4.dom() =~= Set::range(hs + 8, hs + 8 + 8 * cap));
        let tracked rc = rc_raw.into_typed::<i32>(h@.addr);
        let tracked meta = meta_raw.into_typed::<u32>((hs + 4) as usize);
        let tracked slots = Self::slots_from_raw(rest4, h, 0, cap);
        BlockPerm { rc, meta, slots, prefix, text: Text { bytes: Seq::empty() } }
    }

    /// ペイロードの `from` 語目から `cap` 語目の手前までの権限を作る。
    proof fn slots_from_raw(
        tracked raw: PointsToRaw,
        h: *mut Header,
        from: nat,
        cap: nat,
    ) -> (tracked r: Map<int, PointsTo<Word>>)
        requires
            from <= cap,
            raw.is_range(h@.addr + slot_offset(from as int), 8 * (cap - from)),
            raw.provenance() == h@.provenance,
            h@.addr % 8 == 0,
            h@.addr + slot_offset(cap as int) <= usize::MAX + 1,
        ensures
            forall|i: int| #[trigger] r.contains_key(i) <==> from <= i < cap,
            forall|i: int|
                from <= i < cap ==> (#[trigger] r[i]).ptr() == field_ptr::<Word>(h, slot_offset(i))
                    && r[i].is_uninit(),
        decreases cap - from,
    {
        if from == cap {
            Map::tracked_empty()
        } else {
            let a = h@.addr + slot_offset(from as int);
            lemma_align(h@.addr as int, HEADER_WORDS + from);
            let tracked (one, rest) = raw.split(Set::range(a, a + 8));
            assert(rest.dom() =~= Set::range(a + 8, a + 8 + 8 * (cap - from - 1)));
            let tracked word = one.into_typed::<Word>(a as usize);
            let tracked mut r = Self::slots_from_raw(rest, h, from + 1, cap);
            r.tracked_insert(from as int, word);
            r
        }
    }

    /// ブロックの権限を、書いていないメモリの権限に戻す。ページを OS に返すときに使う。
    pub(crate) proof fn into_raw(
        tracked self,
        h: *mut Header,
        cap: nat,
        prefixed: bool,
    ) -> (tracked r: PointsToRaw)
        requires
            self.wf(h, cap, prefixed),
            h@.addr % 8 == 0,
            block_start(h, prefixed) >= 0,
            block_start(h, prefixed) + block_size(cap, prefixed) <= usize::MAX + 1,
        ensures
            r.is_range(block_start(h, prefixed), block_size(cap, prefixed)),
            r.provenance() == h@.provenance,
    {
        let tracked BlockPerm { rc: mut rc, meta: mut meta, slots, prefix, .. } = self;
        rc.leak_contents();
        meta.leak_contents();
        let hs = h@.addr as int;
        let tracked raw = rc.into_raw().join(meta.into_raw());
        let tracked words = Self::slots_into_raw(slots, h, 0, cap);
        let tracked raw = raw.join(words);
        assert(raw.dom() =~= Set::range(hs, hs + 8 + 8 * cap));
        if prefixed {
            let tracked PrefixPerm { first: mut first, state: mut state } = prefix.tracked_unwrap();
            first.leak_contents();
            state.leak_contents();
            let tracked front = first.into_raw().join(state.into_raw());
            let tracked raw = front.join(raw);
            assert(raw.dom() =~= Set::range(
                block_start(h, prefixed),
                block_start(h, prefixed) + block_size(cap, prefixed),
            ));
            raw
        } else {
            raw
        }
    }

    proof fn slots_into_raw(
        tracked slots: Map<int, PointsTo<Word>>,
        h: *mut Header,
        from: nat,
        cap: nat,
    ) -> (tracked r: PointsToRaw)
        requires
            from <= cap,
            forall|i: int| from <= i < cap ==> #[trigger] slots.contains_key(i),
            forall|i: int|
                from <= i < cap ==> (#[trigger] slots[i]).ptr() == field_ptr::<Word>(
                    h,
                    slot_offset(i),
                ),
            h@.addr % 8 == 0,
            h@.addr + slot_offset(cap as int) <= usize::MAX + 1,
        ensures
            r.is_range(h@.addr + slot_offset(from as int), 8 * (cap - from)),
            r.provenance() == h@.provenance,
        decreases cap - from,
    {
        let a = h@.addr + slot_offset(from as int);
        if from == cap {
            let tracked r = PointsToRaw::empty(h@.provenance);
            assert(r.dom() =~= Set::range(a, a));
            r
        } else {
            let tracked mut slots = slots;
            let tracked mut word = slots.tracked_remove(from as int);
            lemma_align(h@.addr as int, 0);
            word.leak_contents();
            let tracked rest = Self::slots_into_raw(slots, h, from + 1, cap);
            assert(word.ptr()@.addr == a);
            let tracked one = word.into_raw();
            assert(one.dom() =~= Set::range(a, a + 8));
            let tracked r = one.join(rest);
            assert(r.dom() =~= Set::range(a, a + 8 * (cap - from)));
            r
        }
    }
}

impl BlockPerm {
    pub(crate) open spec fn text(self) -> Seq<u8> {
        self.text@
    }

    /// ghost のバイト列を空にする。確保したブロックの権限に前の物体のバイト列を残さないために、確保のたびに呼ぶ。
    /// 空の列はメモリのどのバイトとも対応しないので、T10 の約束は保たれる。
    pub(crate) proof fn clear_text(tracked &mut self)
        ensures
            final(self).text() == Seq::<u8>::empty(),
            final(self).rc == old(self).rc,
            final(self).meta == old(self).meta,
            final(self).slots == old(self).slots,
            final(self).prefix == old(self).prefix,
    {
        self.text = Text { bytes: Seq::empty() };
    }

    /// ghost のバイト列を前の `n` バイトに切り詰める。`Bytes.truncate` がその場で長さの語だけを書くときに使う。
    /// メモリの前の `n` バイトは列のその部分に等しいままなので、T10 の約束は保たれる。
    pub(crate) proof fn truncate_text(tracked &mut self, n: int)
        requires
            0 <= n <= old(self).text().len(),
        ensures
            final(self).text() == old(self).text().subrange(0, n),
            final(self).rc == old(self).rc,
            final(self).meta == old(self).meta,
            final(self).slots == old(self).slots,
            final(self).prefix == old(self).prefix,
    {
        self.text = Text { bytes: self.text.bytes.subrange(0, n) };
    }

    /// `String` か `Bytes` のブロック `h` の権限で、長さの語と、その後ろの `n` バイトを覆う語の権限を持つ。T10 の関数は、
    /// 権限とポインタがこの関係にあるときだけ、メモリのバイト列が ghost のバイト列に等しいことを約束する。
    pub(crate) open spec fn holds_body(self, h: *mut Header, n: int) -> bool {
        &&& self.meta.ptr() == field_ptr::<u32>(h, 4)
        &&& self.meta.is_init()
        &&& bits_kind(self.meta.value()).has_body()
        &&& forall|i: int|
            0 <= i <= (n + 7) / 8 ==> #[trigger] self.slots.contains_key(i) && self.slots[i].ptr()
                == field_ptr::<Word>(h, slot_offset(i))
    }

    /// `h` のヘッダを持ち、ペイロードが `cap` 語のブロックの権限である。`prefixed` なら前置きも持つ。
    pub(crate) open spec fn wf(self, h: *mut Header, cap: nat, prefixed: bool) -> bool {
        &&& self.rc.ptr() == field_ptr::<i32>(h, 0)
        &&& self.meta.ptr() == field_ptr::<u32>(h, 4)
        &&& forall|i: int| #[trigger] self.slots.contains_key(i) <==> 0 <= i < cap
        &&& forall|i: int|
            0 <= i < cap ==> (#[trigger] self.slots[i]).ptr() == field_ptr::<Word>(
                h,
                slot_offset(i),
            )
        &&& (self.prefix is Some) == prefixed
        &&& prefixed ==> self.prefix->0.first.ptr() == field_ptr::<u32>(h, -8)
            && self.prefix->0.state.ptr() == field_ptr::<u32>(h, -4)
    }
}

} // verus!

const _: () = assert!(HEADER_WORDS * 8 == size_of::<Header>());

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
        for kind in [
            Kind::Data,
            Kind::BigInt,
            Kind::Foreign,
            Kind::String,
            Kind::Float,
            Kind::Bytes,
            Kind::Free,
        ] {
            assert_eq!(kind.tokenable(), kind == Kind::Data);
            assert_eq!(
                Meta::new(0, 0, kind, false).has_finalizer(),
                kind == Kind::Foreign
            );
        }
    }

    #[test]
    fn the_header_is_one_word() {
        assert_eq!(HEADER_WORDS, 1);
    }
}

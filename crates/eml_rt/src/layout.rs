//! `eml_rt`自身が読む物体の語の位置、つまりヘッダの形と`Kind::Data`の数の語を定める唯一の所
//! (docs/spec/object-model.md の「語の位置」)。`eml_rt`の復号、アロケータ、ポインタの関数、ヒープの述語はここを通り、
//! コード生成器はすべての値をここから取る。実行の関数はどれもspecの対を持ち、`ensures`がそのspecなので、コード生成器の
//! 使う値がヒープ自身の頼る規則と同じであることをVerusが証明する。
//!
//! `scan`の印`Meta::EXTENDED_SCAN`は、設計として`INLINE_SCAN_LIMIT`と同じ数である。上限より小さい`scan`のバイトは走査する
//! 語の数そのものなので、収まらない最小の数が印になる。復号 (`scan_extended`、`scan_first`) が意味を持つのは`Kind::Data`
//! だけである。配列かどうかを表す述語はない。タグが0で走査する語が`INLINE_SCAN_LIMIT`個以上の`Data`は、配列と同じ`meta`を
//! 持つためである。
use vstd::prelude::*;

use crate::meta::{Kind, Meta};

verus! {

/// 1語のバイト数。
pub const WORD_BYTES: usize = 8;

/// ヘッダの語の数。
pub const HEADER_WORDS: usize = 1;

/// ヘッダの中の`rc`のバイトのずれ。
pub const RC_BYTE: usize = 0;

/// ヘッダの中の`meta`のバイトのずれ。
pub const META_BYTE: usize = 4;

/// 数の語があるとき、走査する語の数を持つペイロードの語。
pub const COUNT_WORD: usize = 0;

/// 数の語があるとき (配列では常に) の、走査する最初の語。数の語のすぐ後の語である。
pub const EXTENDED_FIRST: usize = COUNT_WORD + 1;

/// 配列でない`Data`が数の語を持つ、走査する語の最小の数。`scan`の印の値でもある。
pub const INLINE_SCAN_LIMIT: usize = 255;

/// 物体のポインタから、ペイロードのi語目までのバイトのずれ。
pub open spec fn slot_offset_spec(i: int) -> int {
    WORD_BYTES * (HEADER_WORDS + i)
}

/// 走査する語が`scanned`個の`Data` (`array`なら配列) が数の語を持つか。
pub open spec fn data_extended_spec(scanned: nat, array: bool) -> bool {
    array || scanned >= INLINE_SCAN_LIMIT
}

/// その形に書く`scan`のバイト。
pub open spec fn data_scan_spec(scanned: nat, array: bool) -> u8 {
    if data_extended_spec(scanned, array) {
        INLINE_SCAN_LIMIT as u8
    } else {
        scanned as u8
    }
}

/// 論理的な0番目の走査する語の、ペイロードの中の位置。
pub open spec fn data_first_spec(scanned: nat, array: bool) -> int {
    if data_extended_spec(scanned, array) {
        EXTENDED_FIRST as int
    } else {
        0
    }
}

/// `scan`のバイトから、`Data`が数の語を持つかを読む。
pub open spec fn scan_extended_spec(scan: u8) -> bool {
    scan as int == INLINE_SCAN_LIMIT
}

/// `scan`のバイトから、`Data`の走査する最初の語の位置を読む。
pub open spec fn scan_first_spec(scan: u8) -> int {
    if scan_extended_spec(scan) {
        EXTENDED_FIRST as int
    } else {
        0
    }
}

/// `scan`のバイトを復号すると、符号化で選んだものに戻る。
pub proof fn lemma_data_scan(scanned: nat, array: bool)
    ensures
        scan_extended_spec(data_scan_spec(scanned, array)) == data_extended_spec(scanned, array),
        scan_first_spec(data_scan_spec(scanned, array)) == data_first_spec(scanned, array),
        !data_extended_spec(scanned, array) ==> data_scan_spec(scanned, array) as nat == scanned,
{
}

#[inline]
pub const fn data_extended(scanned: usize, array: bool) -> (r: bool)
    ensures
        r == data_extended_spec(scanned as nat, array),
{
    array || scanned >= INLINE_SCAN_LIMIT
}

#[inline]
pub const fn data_scan(scanned: usize, array: bool) -> (r: u8)
    ensures
        r == data_scan_spec(scanned as nat, array),
{
    if data_extended(scanned, array) {
        Meta::EXTENDED_SCAN
    } else {
        scanned as u8
    }
}

#[inline]
pub const fn data_first(scanned: usize, array: bool) -> (r: usize)
    ensures
        r == data_first_spec(scanned as nat, array),
{
    if data_extended(scanned, array) {
        EXTENDED_FIRST
    } else {
        0
    }
}

/// 0番目の生の語の、ペイロードの中の位置。
#[inline]
pub const fn data_raw_first(scanned: usize, array: bool) -> (r: usize)
    requires
        scanned < usize::MAX,
    ensures
        r == data_first_spec(scanned as nat, array) + scanned,
{
    data_first(scanned, array) + scanned
}

/// 数の語を含めた、その形のペイロードの語の数。
pub open spec fn data_words_spec(scanned: nat, raw: nat, array: bool) -> int {
    data_first_spec(scanned, array) + scanned + raw
}

/// `data_words_spec`の語の数。桁があふれたら`None`を返す。
#[inline]
pub const fn data_words(scanned: usize, raw: usize, array: bool) -> (r: Option<usize>)
    ensures
        r matches Some(n) ==> n == data_words_spec(scanned as nat, raw as nat, array),
        r is None ==> data_words_spec(scanned as nat, raw as nat, array) > usize::MAX,
{
    match data_first(scanned, array).checked_add(scanned) {
        Some(n) => n.checked_add(raw),
        None => None,
    }
}

/// 数の語を持たない`Data`の`meta`。前提は実行時に確かめないので、Verusの外から呼ぶ側が自分で守る。
#[inline]
pub const fn data_meta(tag: u16, scanned: usize, lin: bool) -> (r: Meta)
    requires
        !data_extended_spec(scanned as nat, false),
    ensures
        r.tag_spec() == tag,
        r.scan_spec() == data_scan_spec(scanned as nat, false),
        r.scan_spec() == scanned,
        r.kind_spec() == Kind::Data,
        r.lin_spec() == lin,
{
    Meta::new(tag, data_scan(scanned, false), Kind::Data, lin)
}

#[inline]
pub const fn scan_extended(scan: u8) -> (r: bool)
    ensures
        r == scan_extended_spec(scan),
{
    scan == Meta::EXTENDED_SCAN
}

#[inline]
pub const fn scan_first(scan: u8) -> (r: usize)
    ensures
        r == scan_first_spec(scan),
{
    if scan_extended(scan) {
        EXTENDED_FIRST
    } else {
        0
    }
}

#[inline]
pub const fn slot_byte_offset(i: usize) -> (r: usize)
    requires
        slot_offset_spec(i as int) <= usize::MAX,
    ensures
        r == slot_offset_spec(i as int),
{
    WORD_BYTES * (HEADER_WORDS + i)
}

} // verus!

// `Meta::EXTENDED_SCAN`は上限を`as u8`で作るので、上限が`u8`に収まらなくなったときの切り捨てをここで止める。
const _: () = assert!(Meta::EXTENDED_SCAN as usize == INLINE_SCAN_LIMIT);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_and_word_values_are_pinned() {
        assert_eq!(INLINE_SCAN_LIMIT, 255);
        assert_eq!((WORD_BYTES, HEADER_WORDS, RC_BYTE, META_BYTE), (8, 1, 0, 4));
        assert_eq!((COUNT_WORD, EXTENDED_FIRST), (0, 1));
        assert_eq!(slot_byte_offset(0), 8);
        assert_eq!(slot_byte_offset(1), 16);
    }

    #[test]
    fn data_shapes_have_a_count_word_from_255_scanned_words_or_as_arrays() {
        // (scanned, array) から (extended, scan, first, raw first) へ
        let cases = [
            ((0, false), (false, 0, 0, 0)),
            ((254, false), (false, 254, 0, 254)),
            ((255, false), (true, 255, 1, 256)),
            ((300, false), (true, 255, 1, 301)),
            ((0, true), (true, 255, 1, 1)),
            ((254, true), (true, 255, 1, 255)),
            ((255, true), (true, 255, 1, 256)),
            ((300, true), (true, 255, 1, 301)),
        ];
        for ((scanned, array), expected) in cases {
            let got = (
                data_extended(scanned, array),
                data_scan(scanned, array),
                data_first(scanned, array),
                data_raw_first(scanned, array),
            );
            assert_eq!(got, expected, "scanned = {scanned}, array = {array}");
            let scan = data_scan(scanned, array);
            assert_eq!(
                (scan_extended(scan), scan_first(scan)),
                (expected.0, expected.2)
            );
        }
        assert_eq!(data_words(254, 2, false), Some(256));
        assert_eq!(data_words(255, 2, false), Some(258));
        assert_eq!(data_words(0, 0, true), Some(1));
        assert_eq!(data_words(usize::MAX, 0, true), None);
        let meta = data_meta(7, 254, true);
        assert_eq!(
            (meta.tag(), meta.scan(), meta.kind(), meta.lin()),
            (7, 254, Kind::Data, true)
        );
    }

    /// `scan`の255の規則の綴り。`255` (`255u8`なども) と`EXTENDED_SCAN`を数える。
    fn count_word_spellings(text: &str) -> usize {
        let s = crate::trust::prepare(text);
        (0..s.len())
            .filter(|&i| {
                (crate::trust::word_at(&s, i, "255")
                    && !s.get(i + 3).is_some_and(|c| c.is_ascii_digit()))
                    || crate::trust::word_at(&s, i, "EXTENDED_SCAN")
            })
            .count()
    }

    #[test]
    fn only_layout_spells_the_count_word_rule() {
        for (file, text) in crate::trust::FILES {
            let allowed = match *file {
                "layout.rs" | "externs.rs" => continue,
                "meta.rs" => 1,
                _ => 0,
            };
            assert_eq!(
                count_word_spellings(text),
                allowed,
                "{file} spells the count word rule"
            );
        }
    }

    #[test]
    fn count_word_spellings_are_found() {
        let text = "a == 255\nb != 255u8\nMeta::EXTENDED_SCAN\n// 255\nc == 2550\nx0255\n";
        assert_eq!(count_word_spellings(text), 3);
    }
}

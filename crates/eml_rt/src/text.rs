//! 文字列とバイト列の中身 (docs/spec/object-model.md の「バイト列と文字列」)。Verus の権限は語の単位で、語の中のバイトの
//! 位置を扱わないので、`&[u8]` で読み書きする4つの関数 (`write_bytes`、`write_byte`、`bytes`、`bytes_ptr`) は信頼の
//! 一覧の T10 である。どれも `String` か `Bytes` のブロックの権限を受け、長さの語 (ペイロードの 0 語目) の後ろの容量の
//! 中だけを読み書きする。メモリのバイト列は、いつも権限の ghost のバイト列 `text` に等しい。この約束は、権限が渡した
//! ポインタの `String` か `Bytes` のブロックのものであるとき (`BlockPerm::holds_body`) に限る。ghost のバイト列を
//! 変えるのは、ここの `write_bytes` と `write_byte`、確保のたびに列を空にする `BlockPerm::clear_text`、列を前の部分に
//! 縮める `BlockPerm::truncate_text` だけである。
//! `str` で読み書きする `write_text`、`text`、`text_ptr` は、vstd の `str::as_bytes` と `str::from_utf8_unchecked` の
//! 約束を使って、この4つの上に検証して書く。

use crate::meta::{BlockPerm, Header, slot_ptr};
use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;
#[cfg(verus_only)]
use vstd::utf8::valid_utf8;

verus! {

/// `h` のペイロードの `at` バイト目 (長さの語の後ろから数える) から `b` を写す。`at` は今の ghost のバイト列の
/// 長さで、写した後の ghost のバイト列は連結になる。長さの語と、ほかの欄は書かない。
#[verifier::external_body]
#[inline]
pub(crate) fn write_bytes(
    h: *mut Header,
    cap: usize,
    at: usize,
    b: &[u8],
    Tracked(perm): Tracked<&mut BlockPerm>,
)
    requires
        forall|i: int| #[trigger] old(perm).slots.contains_key(i) <==> 0 <= i < cap,
        cap >= 1,
        old(perm).holds_body(h, 8 * (cap - 1)),
        at == old(perm).text().len(),
        at + b@.len() <= 8 * (cap - 1),
    ensures
        final(perm).text() == old(perm).text() + b@,
        final(perm).rc == old(perm).rc,
        final(perm).meta == old(perm).meta,
        final(perm).prefix == old(perm).prefix,
        final(perm).slots.dom() == old(perm).slots.dom(),
        final(perm).slots[0] == old(perm).slots[0],
        forall|i: int|
            0 <= i < cap ==> (#[trigger] final(perm).slots[i]).ptr() == old(perm).slots[i].ptr(),
{
    debug_assert!(at + b.len() <= 8 * (cap - 1));
    // 生のポインタだけで写す。`copy_nonoverlapping` は、b が写す先と重ならないことを前提にする。このヒープの文字列から
    // 写すとき (`string_concat` の右辺、`String.show` の写す元、作り直しの left) は、写す先がいつも別のブロック
    // である。呼ぶ側の `&str` は、`string_push` の Safety が left 自身の中身を指さないことを求める
    // (docs/spec/object-model.md の「バイト列と文字列」)
    unsafe {
        std::ptr::copy_nonoverlapping(b.as_ptr(), slot_ptr(h, 1).cast::<u8>().add(at), b.len());
    }
}

/// `h` のペイロードの `i` バイト目を `b` に書き換える。`i` は今の ghost のバイト列の中の位置で、長さは変わらない。
/// 長さの語と、ほかの欄は書かない。
#[verifier::external_body]
#[inline]
pub(crate) fn write_byte(
    h: *mut Header,
    cap: usize,
    i: usize,
    b: u8,
    Tracked(perm): Tracked<&mut BlockPerm>,
)
    requires
        forall|k: int| #[trigger] old(perm).slots.contains_key(k) <==> 0 <= k < cap,
        cap >= 1,
        old(perm).holds_body(h, 8 * (cap - 1)),
        old(perm).text().len() <= 8 * (cap - 1),
        i < old(perm).text().len(),
    ensures
        final(perm).text() == old(perm).text().update(i as int, b),
        final(perm).rc == old(perm).rc,
        final(perm).meta == old(perm).meta,
        final(perm).prefix == old(perm).prefix,
        final(perm).slots.dom() == old(perm).slots.dom(),
        final(perm).slots[0] == old(perm).slots[0],
        forall|k: int|
            0 <= k < cap ==> (#[trigger] final(perm).slots[k]).ptr() == old(perm).slots[k].ptr(),
{
    debug_assert!(i < 8 * (cap - 1));
    unsafe {
        slot_ptr(h, 1).cast::<u8>().add(i).write(b);
    }
}

/// `h` の中身を `&[u8]` として借りる。借りている間、権限 (と同じブロック) は書き換えられない。
#[verifier::external_body]
#[inline]
pub(crate) fn bytes(h: *mut Header, len: usize, Tracked(perm): Tracked<&BlockPerm>) -> (r: &[u8])
    requires
        perm.holds_body(h, len as int),
        len == perm.text().len(),
    ensures
        r@ == perm.text(),
{
    unsafe { std::slice::from_raw_parts(slot_ptr(h, 1).cast::<u8>().cast_const(), len) }
}

/// `h` の中身の位置。指す先を読めるのは、物体が生きていて、書き換えていない間だけである。
#[verifier::external_body]
#[inline]
pub(crate) fn bytes_ptr(
    h: *mut Header,
    len: usize,
    Tracked(perm): Tracked<&BlockPerm>,
) -> *const [u8]
    requires
        perm.holds_body(h, len as int),
        len == perm.text().len(),
{
    std::ptr::slice_from_raw_parts(slot_ptr(h, 1).cast::<u8>().cast_const(), len)
}

/// `h` のペイロードの `at` バイト目から `s` のバイト列を写す。約束は `write_bytes` と同じである。
#[inline]
pub(crate) fn write_text(
    h: *mut Header,
    cap: usize,
    at: usize,
    s: &str,
    Tracked(perm): Tracked<&mut BlockPerm>,
)
    requires
        forall|i: int| #[trigger] old(perm).slots.contains_key(i) <==> 0 <= i < cap,
        cap >= 1,
        old(perm).holds_body(h, 8 * (cap - 1)),
        at == old(perm).text().len(),
        at + s.spec_bytes().len() <= 8 * (cap - 1),
    ensures
        final(perm).text() == old(perm).text() + s.spec_bytes(),
        final(perm).rc == old(perm).rc,
        final(perm).meta == old(perm).meta,
        final(perm).prefix == old(perm).prefix,
        final(perm).slots.dom() == old(perm).slots.dom(),
        final(perm).slots[0] == old(perm).slots[0],
        forall|i: int|
            0 <= i < cap ==> (#[trigger] final(perm).slots[i]).ptr() == old(perm).slots[i].ptr(),
{
    write_bytes(h, cap, at, s.as_bytes(), Tracked(perm));
}

/// `h` の中身を `str` として借りる。借りている間、権限 (と同じブロック) は書き換えられない。
#[inline]
pub(crate) fn text(h: *mut Header, len: usize, Tracked(perm): Tracked<&BlockPerm>) -> (r: &str)
    requires
        perm.holds_body(h, len as int),
        len == perm.text().len(),
        valid_utf8(perm.text()),
    ensures
        r.spec_bytes() == perm.text(),
{
    let b = bytes(h, len, Tracked(perm));
    let r = unsafe { str::from_utf8_unchecked(b) };
    proof {
        assert(r.spec_bytes() =~= perm.text());
    }
    r
}

/// `h` の中身の位置。公開の `string` が返す。指す先を読めるのは、物体が生きていて、`string_push` か `string_concat` で
/// 書き換えていない間だけである (docs/spec/object-model.md の「バイト列と文字列」)。
#[inline]
pub(crate) fn text_ptr(h: *mut Header, len: usize, Tracked(perm): Tracked<&BlockPerm>) -> *const str
    requires
        perm.holds_body(h, len as int),
        len == perm.text().len(),
        valid_utf8(perm.text()),
{
    bytes_ptr(h, len, Tracked(perm)) as *const str
}

} // verus!

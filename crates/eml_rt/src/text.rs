//! 文字列のバイト列 (docs/spec/object-model.md の「文字列」)。Verus の権限は語の単位で、語の中のバイトの位置を
//! 扱わないので、ここの関数は信頼の一覧の T10 である。どれも `String` のブロックの権限を受け、長さの語 (ペイロードの
//! 0 語目) の後ろの容量の中だけを読み書きする。メモリのバイト列は、いつも権限の ghost のバイト列 `text` に等しい。
//! この約束は、権限が渡したポインタの `String` のブロックのものであるとき (`BlockPerm::holds_text`) に限る。ghost の
//! バイト列を変えるのは、ここの `write_text` と、確保のたびに列を空にする `BlockPerm::clear_text` だけである。

use crate::meta::{BlockPerm, Header, slot_ptr};
use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;
#[cfg(verus_only)]
use vstd::utf8::valid_utf8;

verus! {

/// `h` のペイロードの `at` バイト目 (長さの語の後ろから数える) から `s` のバイトを写す。`at` は今の ghost のバイト列の
/// 長さで、写した後の ghost のバイト列は連結になる。長さの語と、ほかの欄は書かない。
#[verifier::external_body]
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
        old(perm).holds_text(h, 8 * (cap - 1)),
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
    debug_assert!(at + s.len() <= 8 * (cap - 1));
    // 生のポインタだけで写す。`copy_nonoverlapping` は、s が写す先と重ならないことを前提にする。このヒープの文字列から
    // 写すとき (`string_concat` の右辺と作り直しの left) は、写す先がいつも別のブロックである。呼ぶ側の `&str` は、
    // `string_push` の Safety が left 自身の中身を指さないことを求める (docs/spec/object-model.md の「文字列」)
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), slot_ptr(h, 1).cast::<u8>().add(at), s.len());
    }
}

/// `h` の中身を `str` として借りる。借りている間、権限 (と同じブロック) は書き換えられない。
#[verifier::external_body]
#[inline]
pub(crate) fn text(h: *mut Header, len: usize, Tracked(perm): Tracked<&BlockPerm>) -> (r: &str)
    requires
        perm.holds_text(h, len as int),
        len == perm.text().len(),
        valid_utf8(perm.text()),
    ensures
        r.spec_bytes() == perm.text(),
{
    unsafe {
        std::str::from_utf8_unchecked(
            std::slice::from_raw_parts(slot_ptr(h, 1).cast::<u8>().cast_const(), len),
        )
    }
}

/// `h` の中身の位置。公開の `string` が返す。指す先を読めるのは、物体が生きていて、`string_push` か `string_concat` で
/// 書き換えていない間だけである (docs/spec/object-model.md の「文字列」)。
#[verifier::external_body]
#[inline]
pub(crate) fn text_ptr(h: *mut Header, len: usize, Tracked(perm): Tracked<&BlockPerm>) -> *const str
    requires
        perm.holds_text(h, len as int),
        len == perm.text().len(),
        valid_utf8(perm.text()),
{
    std::ptr::slice_from_raw_parts(slot_ptr(h, 1).cast::<u8>().cast_const(), len) as *const str
}

} // verus!

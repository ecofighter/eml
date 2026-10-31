//! ほかのヒープのページに触れる関数 (docs/spec/object-model.md の「`unsafe` の核」の T14)。ブロックがどのヒープの
//! ものかは、ページの頭の `sink` で見分ける。頭のその部分はページを公開した後に書かないので、どのスレッドが読んでも
//! 同じ値になる (docs/spec/object-model.md の「ページ」)。

use std::sync::atomic::AtomicUsize;

use vstd::prelude::*;
use vstd::raw_ptr::PointsTo;

use crate::alloc::{PageHeader, page_of};
#[cfg(verus_only)]
use crate::alloc::{header_sink, page_base};
use crate::meta::Header;

verus! {

/// ヒープごとに1つ持つ、別のスレッドからの解放を受ける先。ページの頭の `sink` はこれを指す。
// どの欄も読み書きする操作がないので、使わないことの警告を止める
#[allow(dead_code)]
pub(crate) struct RemoteSink {
    /// 受けてまだ取り込んでいない解放の数。
    count: AtomicUsize,
    /// 別のスレッドが解放した大きい物体のページの列の先頭。
    large: AtomicUsize,
}

impl RemoteSink {
    pub(crate) fn new() -> RemoteSink {
        RemoteSink { count: AtomicUsize::new(0), large: AtomicUsize::new(0) }
    }
}

/// `sink` の番地。Verus は参照を生のポインタに変える式を扱わないので、この関数を信頼する (T14)。参照は null でない。
#[verifier::external_body]
#[inline]
pub(crate) fn sink_ptr(sink: &RemoteSink) -> (r: *const RemoteSink)
    ensures
        r@.addr != 0,
{
    sink as *const RemoteSink
}

/// ブロック `h` のページの頭の `sink` を読む (T14)。`hdr` はそのページの頭の権限で、読む値はその権限の値の `sink` と
/// 同じである。
#[verifier::external_body]
#[inline]
pub(crate) fn page_sink_of(h: *mut Header, Tracked(hdr): Tracked<&PointsTo<PageHeader>>) -> (r:
    *const RemoteSink)
    requires
        hdr.ptr()@.addr == page_base(h@.addr),
        hdr.is_init(),
    ensures
        r@.addr == header_sink(hdr.value())@.addr,
{
    unsafe { (*page_of(h)).sink }
}

} // verus!

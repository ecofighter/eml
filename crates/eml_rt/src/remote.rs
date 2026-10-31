//! ほかのヒープのページに触れる関数 (docs/spec/object-model.md の「`unsafe` の核」の T14 と「別のスレッドからの解放」)。
//! ブロックがどのヒープのものかは、ページの頭の `sink` で見分ける。頭のその部分はページを公開した後に書かないので、
//! どのスレッドが読んでも同じ値になる (docs/spec/object-model.md の「ページ」)。
//!
//! ほかのスレッドが書く語 (ページの遠隔の空きリストの語、空けたブロックの `meta` とペイロードの先頭の語、
//! `RemoteSink` の欄) は、持ち主の権限の値には映らない。持ち主はその値を権限を通して読まず、ここの関数で atomic に
//! 読む。

use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use vstd::prelude::*;
use vstd::raw_ptr::PointsTo;

use crate::alloc::{FIRST_BLOCK, FREE_META, LARGE, PageHeader, REMOTE_AT, page_of};
#[cfg(verus_only)]
use crate::alloc::{header_sink, page_base};
use crate::control::Control;
use crate::heap::Core;
use crate::meta::{BlockPerm, HEADER_WORDS, Header};

verus! {

/// ヒープごとに1つ持つ、別のスレッドからの解放を受ける先。ページの頭の `sink` はこれを指す。
pub(crate) struct RemoteSink {
    /// 受けてまだ取り込んでいない解放の数。取り込む時期を決める目安で、止まった時点では正確になる。
    count: AtomicUsize,
    /// 別のスレッドが解放した大きい物体のページの列の先頭。ページの遠隔の空きリストの語でつなぐ。大きい物体のページは
    /// どれも別の確保なので、番地でなくポインタで持つ。
    large: AtomicPtr<u8>,
    /// 検査モードで、解放を積んでいる途中のスレッドの数。
    pushing: AtomicU32,
}

/// 新しい `RemoteSink` を作り、その番地を返す。`Pages` を落とすときに `Box` に戻して手放す。`Box` のまま持つと、
/// ヒープを動かすたびに `Box` の一意の借用が付け直され、ほかのスレッドが同じ番地を通して書く間に重なるので、
/// 生のポインタで持つ (T14)。
#[verifier::external_body]
pub(crate) fn sink_new() -> (r: *const RemoteSink)
    ensures
        r@.addr != 0,
{
    let sink = RemoteSink {
        count: AtomicUsize::new(0),
        large: AtomicPtr::new(std::ptr::null_mut()),
        pushing: AtomicU32::new(0),
    };
    Box::into_raw(Box::new(sink))
}

/// ブロック `h` のページの頭の `sink` を読む (T14)。`h` は、生きているどれかのヒープか凍結した塊のページのブロックで
/// ある。読む値は、ページを作ったときに書いた値 (塊のページでは凍結したときに書いた null) で、rc が 0 でないブロックの
/// ページでは null でない。`hdr` にそのページの頭の権限を渡せば、読む値はその権限の値の `sink` と同じである。
#[verifier::external_body]
#[inline]
pub(crate) fn page_sink_of(
    h: *mut Header,
    Tracked(hdr): Tracked<Option<&PointsTo<PageHeader>>>,
) -> (r: *const RemoteSink)
    requires
        hdr matches Some(p) ==> p.ptr()@.addr == page_base(h@.addr) && p.is_init(),
    ensures
        hdr matches Some(p) ==> r@.addr == header_sink(p.value())@.addr,
{
    unsafe { (*page_of(h)).sink }
}

/// 検査モードで、ほかのヒープ `sink` に返す解放を始める (T14)。`pushing` を上げてから、空けるブロックの前置きを
/// 書き、破棄処理を呼び、積む。持ち主のページを歩く検査が、その間を積んでいる途中と見るためである。下ろすのは
/// `remote_push` である。
#[verifier::external_body]
#[inline]
pub(crate) fn sink_enter(sink: *const RemoteSink) {
    let sink = unsafe { &*sink };
    sink.pushing.fetch_add(1, Ordering::Relaxed);
}

/// ほかのヒープ `sink` のページのブロック `h` を空け、そのヒープに返す (T14)。`h` は最後の参照を手放した物体の箱で、
/// 子はもう手放してある。大きい物体のブロックは `remote_large_push` に渡す。小さいブロックはそのページの遠隔の
/// 空きリストに積む。速いモードでは `meta` を種類 `Free` にする。リストのつなぎはペイロードの先頭の語に書く。持ち主の
/// 級、ページの一覧、数、`Control` のバイト数には触れない。検査モードでは、呼ぶ側が `sink_enter` で上げた `pushing` を、
/// 積み終えてから Release で下ろす。`perm` は手放す。
#[verifier::external_body]
pub(crate) fn remote_push(
    h: *mut Header,
    sink: *const RemoteSink,
    Tracked(perm): Tracked<BlockPerm>,
    checked: bool,
    control: &Control,
) {
    if unsafe { (*page_of(h)).class } == LARGE {
        remote_large_push(h, sink, checked, control);
        return;
    }
    let sink = unsafe { &*sink };
    if !checked {
        unsafe { h.cast::<u32>().add(1).write(FREE_META) };
    }
    let remote = unsafe { AtomicPtr::from_ptr(remote_word(page_of(h)).cast::<*mut Header>()) };
    let link = unsafe { h.cast::<*mut Header>().add(HEADER_WORDS) };
    let mut old = remote.load(Ordering::Relaxed);
    loop {
        unsafe { link.write(old) };
        // Release で、空けたブロックへのこのスレッドの書き込みを、取り込む持ち主の Acquire に渡す
        match remote.compare_exchange_weak(old, h, Ordering::Release, Ordering::Relaxed) {
            Ok(_) => break,
            Err(now) => old = now,
        }
    }
    sink.count.fetch_add(1, Ordering::Relaxed);
    if checked {
        sink.pushing.fetch_sub(1, Ordering::Release);
    }
}

/// ほかのヒープ `sink` の大きい物体のブロック `h` を空け、そのページを持ち主の大きい物体の列に積む (T14)。ページは
/// 返さず、持ち主が次に取り込むときに返す。速いモードでは、ページのバイト数をここで `control` から引く。持ち主の
/// 取り込みを待つと、上限の判定がスレッドの順番に左右されるためである。検査モードの大きい物体のページは隔離して
/// 返さないので、引かない。`remote_push` だけが呼ぶ。
#[verifier::external_body]
pub(crate) fn remote_large_push(
    h: *mut Header,
    sink: *const RemoteSink,
    checked: bool,
    control: &Control,
) {
    let sink = unsafe { &*sink };
    let page = page_of(h);
    if !checked {
        unsafe { h.cast::<u32>().add(1).write(FREE_META) };
        control.refund(unsafe { (*page).bytes });
    }
    let link = unsafe { AtomicPtr::from_ptr(remote_word(page).cast::<*mut u8>()) };
    let mut old = sink.large.load(Ordering::Relaxed);
    loop {
        link.store(old, Ordering::Relaxed);
        match sink.large.compare_exchange_weak(
            old,
            page.cast::<u8>(),
            Ordering::Release,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(now) => old = now,
        }
    }
    sink.count.fetch_add(1, Ordering::Relaxed);
    if checked {
        sink.pushing.fetch_sub(1, Ordering::Release);
    }
}

/// 持ち主のページ `page` の遠隔の空きリストを空にし、積まれていたブロックのヘッダを `out` の後ろに足す (T14)。
/// 呼ぶのは持ち主のスレッドだけである。`core` はこのヒープの今の状態で、`held` は呼ぶ側が持つ物体の番地である。
///
/// 約束: 足したブロックは互いに違い、`out` にもとからあったブロックとも違い、このヒープのブロックで、最後の参照で
/// 空けたものである。足したブロックを含む集合 `r` で、次を満たすもの (`Core::freed_set`) がある。持ち主から見て空けた
/// ブロックの集合で、生きているか空けた状態にあり、生きているものは rc が 0 でなく、`held` と重ならず、辺でそこへ
/// 届くブロックはみな `r` にあり、残した語はそこを指さない。逐次の証明はこの集合を持ち主のモデルから外す。
/// 持ち主の言う「生きている」が古いのは、そのブロックを持ち主が持たないからで、持たないことは、生きている物体を
/// 1つのスレッドからだけ届かせる規則 (docs/spec/object-model.md の「`share`」) による。
#[verifier::external_body]
pub(crate) fn remote_take(
    page: *mut PageHeader,
    out: &mut Vec<*mut Header>,
    Ghost(core): Ghost<Core>,
    Ghost(held): Ghost<Set<usize>>,
)
    ensures
        taken(old(out)@, final(out)@, core, held),
{
    let remote = unsafe { AtomicPtr::from_ptr(remote_word(page).cast::<*mut Header>()) };
    if !remote.load(Ordering::Relaxed).is_null() {
        // Acquire で、積んだスレッドの書き込みを受ける
        let mut next = remote.swap(std::ptr::null_mut(), Ordering::Acquire);
        while !next.is_null() {
            out.push(next);
            next = unsafe { next.cast::<*mut Header>().add(HEADER_WORDS).read() };
        }
    }
}

/// 持ち主の大きい物体の列を空にし、積まれていたページのブロックのヘッダを `out` の後ろに足す (T14)。`prefix_words`
/// はこのヒープの前置きの語の数で、ブロックはページの先頭から `FIRST_BLOCK` と前置きの後にある。約束は
/// `remote_take` と同じで、足したブロックは大きい物体のブロックである。
#[verifier::external_body]
pub(crate) fn remote_large_take(
    sink: *const RemoteSink,
    prefix_words: usize,
    out: &mut Vec<*mut Header>,
    Ghost(core): Ghost<Core>,
    Ghost(held): Ghost<Set<usize>>,
)
    ensures
        taken(old(out)@, final(out)@, core, held),
        forall|i: int|
            old(out)@.len() <= i < final(out)@.len() ==> core.pages.large(
                #[trigger] final(out)@[i]@.addr,
            ),
{
    let sink = unsafe { &*sink };
    if !sink.large.load(Ordering::Relaxed).is_null() {
        let mut next = sink.large.swap(std::ptr::null_mut(), Ordering::Acquire);
        while !next.is_null() {
            out.push(next.cast::<Header>().wrapping_byte_add(FIRST_BLOCK + prefix_words * 8));
            next =
            unsafe { AtomicPtr::from_ptr(remote_word(next.cast()).cast::<*mut u8>()) }.load(
                Ordering::Relaxed,
            );
        }
    }
}

/// 受けてまだ取り込んでいない解放の数 (T14)。目安で、ほかのスレッドが同時に足しうる。
#[verifier::external_body]
#[inline]
pub(crate) fn sink_count(sink: *const RemoteSink) -> usize {
    let sink = unsafe { &*sink };
    sink.count.load(Ordering::Relaxed)
}

/// 取り込みを始める前に、受けた解放の数を 0 に戻す (T14)。
#[verifier::external_body]
#[inline]
pub(crate) fn sink_count_take(sink: *const RemoteSink) {
    let sink = unsafe { &*sink };
    sink.count.swap(0, Ordering::Relaxed);
}

/// 大きい物体の列が空でないか (T14)。
#[verifier::external_body]
#[inline]
pub(crate) fn sink_large_pending(sink: *const RemoteSink) -> bool {
    let sink = unsafe { &*sink };
    !sink.large.load(Ordering::Relaxed).is_null()
}

/// 検査モードで、解放を積んでいる途中のスレッドがあるか (T14)。止まった時点で読み、積んでいる途中を見つける。
#[verifier::external_body]
#[inline]
pub(crate) fn sink_pushing(sink: *const RemoteSink) -> bool {
    let sink = unsafe { &*sink };
    sink.pushing.load(Ordering::Acquire) != 0
}

/// 受けて取り込んでいない解放か、積んでいる途中の解放があるか (T14)。取り込みを終えたヒープでは、3つの欄はみな 0
/// である。
#[verifier::external_body]
#[inline]
pub(crate) fn sink_pending(sink: *const RemoteSink) -> bool {
    let sink = unsafe { &*sink };
    sink.count.load(Ordering::Relaxed) != 0 || !sink.large.load(Ordering::Relaxed).is_null()
        || sink.pushing.load(Ordering::Acquire) != 0
}

/// `out` の `old` の後ろに足したブロックは互いに違い、`old` のブロックとも違い、`core` のこのヒープのブロックである。
/// 足したブロックを含み `Core::freed_set` を満たす集合がある。
pub(crate) open spec fn taken(
    old: Seq<*mut Header>,
    new: Seq<*mut Header>,
    core: Core,
    held: Set<usize>,
) -> bool {
    &&& new.len() >= old.len()
    &&& new.subrange(0, old.len() as int) == old
    &&& forall|i: int, j: int|
        #![trigger new[i], new[j]]
        0 <= i < j < new.len() && old.len() <= j ==> new[i]@.addr != new[j]@.addr
    &&& forall|i: int|
        old.len() <= i < new.len() ==> core.dom().contains(#[trigger] new[i]@.addr) && new[i]
            == core.block(new[i]@.addr)
    &&& exists|r: Set<usize>|
        #![trigger core.freed_set(r, held)]
        core.freed_set(r, held) && forall|i: int|
            old.len() <= i < new.len() ==> r.contains(#[trigger] new[i]@.addr)
}

} // verus!

/// ページ `page` の遠隔の空きリストの語の位置。
fn remote_word(page: *mut PageHeader) -> *mut usize {
    page.cast::<usize>().wrapping_byte_add(REMOTE_AT)
}

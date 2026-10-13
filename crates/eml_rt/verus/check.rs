//! `heap/check.rs` の `release` と reuse の判定の写しと証明 (spec の「Verus」)。

use vstd::prelude::*;

use crate::meta::{Kind, Meta, same_fields};

verus! {

/// 本体の `HeapError` のうち、2つの判定が返す誤りだけを持つ
pub enum HeapError {
    WrongMeta,
    LinBitKept,
    WrongLayout,
}

/// `release` が名指す形。データの物体で、tag と走査する語の数が名指しのとおりである
pub open spec fn named_layout(meta: Meta, scanned: usize, tag: u16, keep_len: usize) -> bool {
    meta.kind_spec() == Kind::Data && meta.tag_spec() == tag && scanned == keep_len
}

/// 箱が持つ `lin` ビットを、新しい `meta` が落とす
pub open spec fn drops_lin(boxed: Meta, new: Meta) -> bool {
    boxed.lin_spec() && !new.lin_spec()
}

pub open spec fn layout_result(meta: Meta, scanned: usize, tag: u16, keep_len: usize) -> Result<
    (),
    HeapError,
> {
    if named_layout(meta, scanned, tag, keep_len) {
        Ok(())
    } else {
        Err(HeapError::WrongLayout)
    }
}

pub open spec fn reuse_result(boxed: Meta, new: Meta) -> Result<(), HeapError> {
    if !same_fields(boxed, new) {
        Err(HeapError::WrongMeta)
    } else if drops_lin(boxed, new) {
        Err(HeapError::LinBitKept)
    } else {
        Ok(())
    }
}

pub(crate) fn layout_check(
    meta: Meta,
    scanned: usize,
    tag: u16,
    keep_len: usize,
) -> (r: Result<(), HeapError>)
    requires
        meta.wf(),
    ensures
        r == layout_result(meta, scanned, tag, keep_len),
        r is Ok <==> named_layout(meta, scanned, tag, keep_len),
{
    if matches!(meta.kind(), Kind::Data) && meta.tag() == tag && scanned == keep_len {
        Ok(())
    } else {
        Err(HeapError::WrongLayout)
    }
}

pub(crate) fn reuse_in_place_check(boxed: Meta, new: Meta) -> (r: Result<(), HeapError>)
    requires
        boxed.wf(),
        new.wf(),
    ensures
        r == reuse_result(boxed, new),
        r is Ok <==> same_fields(boxed, new) && !drops_lin(boxed, new),
{
    if !boxed.same_shape(new) {
        return Err(HeapError::WrongMeta);
    }
    if boxed.lin() && !new.lin() {
        return Err(HeapError::LinBitKept);
    }
    Ok(())
}

} // verus!

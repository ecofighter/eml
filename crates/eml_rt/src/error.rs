//! 検査モードが報告する誤り (docs/spec/object-model.md の「報告する誤り」)。速いモードが返すのは、`foreign` の
//! `WrongKind`、空の token を受けた `reuse_in_place` の `StaleToken`、根の読み書きの `RootMisuse`、回復の
//! `Unrecoverable` だけである (docs/spec/object-model.md の「核の操作」)。

use std::fmt;
use vstd::prelude::*;

verus! {

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapError {
    UseAfterFree,
    ImmortalOverRelease,
    NotImmortal,
    LinDup,
    LinShared,
    LinReachedTwice,
    WrongOwner { owner: u32, worker: u32 },
    NotTokenable,
    NotUnique,
    NotWritable,
    NotLast,
    StaleToken,
    WrongClass,
    WrongMeta,
    LinBitKept,
    FinalizerSkipped,
    WrongLayout,
    WrongKind,
    OutOfBounds,
    SharedReachesLocal,
    ImmortalReachesMortal,
    YieldMisuse,
    BadEvidence,
    NoHandler,
    RootMisuse,
    Unrecoverable,
}

} // verus!
impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::UseAfterFree => f.write_str("use of a freed object"),
            HeapError::ImmortalOverRelease => {
                f.write_str("an immortal object is released more often than it was acquired")
            }
            HeapError::NotImmortal => f.write_str("an acquire of an object that is not immortal"),
            HeapError::LinDup => f.write_str("a Lin object is shared"),
            HeapError::LinShared => f.write_str("a Lin object is on the shared side"),
            HeapError::LinReachedTwice => f.write_str("share reaches the same Lin object twice"),
            HeapError::WrongOwner { owner, worker } => {
                write!(f, "an object of worker {owner} is used by worker {worker}")
            }
            HeapError::NotTokenable => {
                f.write_str("an object of this kind cannot become a reuse token")
            }
            HeapError::NotUnique => f.write_str("an object that must be unique is shared"),
            HeapError::NotWritable => f.write_str(
                "set_field or set_raw writes an object that is neither local nor being built",
            ),
            HeapError::NotLast => f.write_str("free_block on an object that is still referenced"),
            HeapError::StaleToken => f.write_str("a reuse token is used twice or is stale"),
            HeapError::WrongClass => f.write_str("a reuse token has another size class"),
            HeapError::WrongMeta => f.write_str("reuse_in_place names another constructor"),
            HeapError::LinBitKept => {
                f.write_str("reuse_in_place keeps a lin bit the new object does not have")
            }
            HeapError::FinalizerSkipped => f.write_str("free_block on an object with a finalizer"),
            HeapError::WrongLayout => {
                f.write_str("an object does not have the tag and number of fields a release names")
            }
            HeapError::WrongKind => f.write_str("an object is not of the expected kind"),
            HeapError::OutOfBounds => f.write_str("a field index is past the end of the object"),
            HeapError::SharedReachesLocal => f.write_str("a shared object reaches a local object"),
            HeapError::ImmortalReachesMortal => {
                f.write_str("an immortal object reaches a mortal object")
            }
            HeapError::YieldMisuse => {
                f.write_str("an evidence-passing primitive breaks the yield state")
            }
            HeapError::BadEvidence => f.write_str(
                "an evidence-passing primitive reads something that is not an evidence, an evv node or an entry table",
            ),
            HeapError::NoHandler => f.write_str("no evidence of the effect is in the evv"),
            HeapError::RootMisuse => f.write_str("a session root is read while empty or set twice"),
            HeapError::Unrecoverable => f.write_str(
                "a session root reaches an object that recovery cannot copy",
            ),
        }
    }
}

impl std::error::Error for HeapError {}

//! 検査モードが報告する誤り (docs/spec/object-model.md の「報告する誤り」)。速いモードが返すのは、`foreign` の
//! `WrongKind` と、空の token を受けた `reuse_in_place` の `StaleToken` だけである (docs/spec/object-model.md の「核の操作」)。

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::UseAfterFree => f.write_str("use of a freed object"),
            HeapError::ImmortalOverRelease => {
                f.write_str("an immortal object is released more often than it was acquired")
            }
            HeapError::NotImmortal => f.write_str("an acquire of an object that is not immortal"),
            // `eml_runtime` の `File` の検査と同じ文言にする
            HeapError::LinDup => f.write_str("a Lin object is shared"),
            HeapError::LinShared => f.write_str("a Lin object is on the shared side"),
            HeapError::LinReachedTwice => f.write_str("share reaches the same Lin object twice"),
            HeapError::WrongOwner { owner, worker } => {
                write!(f, "an object of worker {owner} is used by worker {worker}")
            }
            HeapError::NotTokenable => {
                f.write_str("an object of this kind cannot become a reuse token")
            }
            HeapError::NotUnique => {
                f.write_str("a reuse token is made from an object that is not unique")
            }
            HeapError::NotLast => f.write_str("free_block on an object that is still referenced"),
            HeapError::StaleToken => f.write_str("a reuse token is used twice or is stale"),
            HeapError::WrongClass => f.write_str("a reuse token has another size class"),
            HeapError::WrongMeta => f.write_str("reuse_in_place names another constructor"),
            HeapError::LinBitKept => {
                f.write_str("reuse_in_place keeps a lin bit the new object does not have")
            }
            HeapError::FinalizerSkipped => f.write_str("free_block on an object with a finalizer"),
            // `eml_runtime` の `release_fields` と同じ文言にする
            HeapError::WrongLayout => {
                f.write_str("an object does not have the tag and number of fields a release names")
            }
            HeapError::WrongKind => f.write_str("an object is not of the expected kind"),
            HeapError::OutOfBounds => f.write_str("a field index is past the end of the object"),
            HeapError::SharedReachesLocal => f.write_str("a shared object reaches a local object"),
            HeapError::ImmortalReachesMortal => {
                f.write_str("an immortal object reaches a mortal object")
            }
        }
    }
}

impl std::error::Error for HeapError {}

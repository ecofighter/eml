//! ISAとABIごとのstubを選ぶ (docs/spec/native.md の「対象」)。

#[cfg(all(target_arch = "aarch64", not(windows)))]
#[path = "aarch64.rs"]
mod imp;
#[cfg(not(all(target_arch = "aarch64", not(windows))))]
#[path = "unsupported.rs"]
mod imp;

pub(crate) use imp::longjmp;
pub use imp::{JmpBuf, SUPPORTED, eml_enter};

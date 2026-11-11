//! ISAとABIごとのstubを選ぶ (docs/spec/native.md の「対象」)。

#[cfg(all(target_arch = "aarch64", not(windows)))]
#[path = "aarch64.rs"]
mod imp;
#[cfg(all(target_arch = "x86_64", not(windows)))]
#[path = "x86_64_sysv.rs"]
mod imp;
#[cfg(not(any(
    all(target_arch = "aarch64", not(windows)),
    all(target_arch = "x86_64", not(windows))
)))]
#[path = "unsupported.rs"]
mod imp;

pub(crate) use imp::longjmp;
pub use imp::{JmpBuf, SUPPORTED, eml_enter};

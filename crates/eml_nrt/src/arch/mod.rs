//! ISAとABIごとのstubを選ぶ (docs/spec/native.md の「対象」)。

#[cfg(all(target_arch = "aarch64", not(windows)))]
#[path = "aarch64.rs"]
mod imp;
#[cfg(all(target_arch = "x86_64", not(windows)))]
#[path = "x86_64_sysv.rs"]
mod imp;
#[cfg(all(target_arch = "x86_64", windows))]
#[path = "x86_64_win64.rs"]
mod imp;
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[path = "unsupported.rs"]
mod imp;
#[cfg(all(target_arch = "aarch64", windows))]
#[path = "unsupported.rs"]
mod imp;

pub use imp::{JmpBuf, SUPPORTED, eml_enter};
pub(crate) use imp::{longjmp, stack_pointer};

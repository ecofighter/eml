//! atomic とページの確保の層 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「`unsafe` の核」)。
//! 後で `cfg(loom)` のときに loom の型へ差し替えるので、核はこの module から atomic と確保の関数を使う。

pub(crate) use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
pub(crate) use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

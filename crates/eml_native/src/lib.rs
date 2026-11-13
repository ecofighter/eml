//! ネイティブのコード生成器である。LIR から Cranelift で機械語を作り、JIT のホストで動かす。Cranelift のフラグを作る
//! 唯一の関数、シグネチャ、thunk もここにある (docs/spec/native.md の「`eml_enter`」)。C の関数をつなぐ道は
//! `eml_clink` の仕事で、ここには入らない (docs/implementation/architecture.md の「crate の依存関係」)。

#![deny(unsafe_code)]

pub mod abi;
mod jit;
mod lower;
mod mem;
pub mod thunk;
#[cfg(test)]
mod trust;
mod unsupported;

pub use eml_nrt::image::Options;
/// この機械でネイティブのコードを走らせられるとき、つまり対象にエントリのstubがあるときに真である
/// (docs/spec/native.mdの「対象」)。
pub const SUPPORTED: bool = eml_nrt::SUPPORTED;
pub use jit::{Arg, LoadError, Loaded, NativeMachine, Symbol, execute};
pub use lower::rows;
pub use unsupported::{Form, Unsupported, unsupported};

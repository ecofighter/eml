//! 利用者の C の関数をつなぐ (docs/spec/vm.md の「C の関数の呼び出し」)。`CSignature` ごとの橋渡しの関数を
//! Cranelift で作り、記号を宣言したモジュールの `link` から解決して、`eml_cabi::Linked` を返す。呼び手はプログラムを
//! 読み込む前につなぎ、結果の値だけを VM に渡す。

#![deny(unsafe_code)]

mod bridge;
mod error;
mod library;
mod linker;
#[cfg(test)]
mod trust;

pub use bridge::Bridges;
pub use error::LinkError;
pub use library::{candidates, running_os};
pub use linker::Linker;

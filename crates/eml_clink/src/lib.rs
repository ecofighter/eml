//! 利用者のCの関数をつなぐ (docs/spec/vm.mdの「C の関数の呼び出し」)。`CSignature`ごとの橋渡しの関数を
//! Craneliftで作り、記号を宣言したモジュールの`link`から解決して、`eml_cabi::Linked`を返す。呼び手はプログラムを
//! 読み込む前につなぎ、結果の値だけをVMに渡す。ネイティブのコード生成器 (`eml_native`) とは別のcrateで、
//! Craneliftのフラグも自分で作る (docs/implementation/architecture.mdの「`eml_clink`の内部」)。

#![deny(unsafe_code)]

mod bridge;
mod error;
mod library;
mod linker;
mod signature;
#[cfg(test)]
mod trust;

pub use bridge::Bridges;
pub use eml_cabi::Linked;
pub use error::LinkError;
pub use library::{candidates, running_os};
pub use linker::Linker;
pub use signature::c_signature;

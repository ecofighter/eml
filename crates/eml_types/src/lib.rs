//! 型、row、Kind の検査 (docs/spec/types.md)。

#[allow(dead_code)] // Task 7 の検査器が使う
mod kind;
#[allow(dead_code)] // Task 7 の検査器が使う
mod table;
mod ty;

use eml_diagnostics::Diagnostic;
use eml_hir::Module;

pub use ty::{Effect, Linearity, Multiplicity, Type};

#[derive(Debug, Default)]
pub struct TypedModule {}

pub fn check(_module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    (TypedModule::default(), Vec::new())
}

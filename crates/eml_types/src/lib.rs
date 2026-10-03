//! Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査。
//!
//! 最初の段階では仮実装で、空の `TypedModule` を返す。中身は後の段階で TDD で実装する。

use eml_diagnostics::Diagnostic;
use eml_hir::Module;

/// 型の情報を別テーブルに持つ HIR。
#[derive(Debug, Default)]
pub struct TypedModule {}

pub fn check(_module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    (TypedModule::default(), Vec::new())
}

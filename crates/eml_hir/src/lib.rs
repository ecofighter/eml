//! CST → HIR の変換と名前解決。
//!
//! 最初の段階では仮実装で、空の `Module` を返す。中身は後の段階で TDD で実装する。

use eml_diagnostics::{Diagnostic, FileId};
use eml_syntax::ast;

/// 1つのソースファイルの HIR。
#[derive(Debug, Default)]
pub struct Module {}

pub fn lower(_file: FileId, _source: &ast::SourceFile) -> (Module, Vec<Diagnostic>) {
    (Module::default(), Vec::new())
}

use eml_diagnostics::{Diagnostic, FileId};
use eml_syntax::ast;

#[derive(Debug, Default)]
pub struct Module {}

pub fn lower(_file: FileId, _source: &ast::SourceFile) -> (Module, Vec<Diagnostic>) {
    (Module::default(), Vec::new())
}

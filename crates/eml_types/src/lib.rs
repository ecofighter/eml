use eml_diagnostics::Diagnostic;
use eml_hir::Module;

#[derive(Debug, Default)]
pub struct TypedModule {}

pub fn check(_module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    (TypedModule::default(), Vec::new())
}

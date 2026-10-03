use eml_diagnostics::Diagnostic;
use eml_types::TypedModule;

/// 複数のスレッドが同じプログラムを実行できるように、実行時は `Arc<Program>` で読み取り専用で共有する。
#[derive(Debug, Default)]
pub struct Program {}

pub fn lower(_module: &TypedModule) -> (Program, Vec<Diagnostic>) {
    (Program::default(), Vec::new())
}

//! UI テストからプロセス内で呼べるように、CLI の中身をバイナリではなく lib に置く。

use std::sync::Arc;

use eml_core_ir::Program;
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, has_errors};

pub use eml_interp::RunConfig;
pub use eml_runtime::OutputSink;

pub fn check(files: &SourceFiles, file: FileId) -> Vec<Diagnostic> {
    analyze(files, file).1
}

/// 呼び出し側が実行の前に診断を表示できるように、検査と実行を別の関数にする (docs/implementation/architecture.md)。
#[derive(Debug)]
pub struct Compiled {
    /// 警告を含む。
    pub diagnostics: Vec<Diagnostic>,
    /// エラーがあれば `None`。
    pub program: Option<Arc<Program>>,
}

pub fn compile(files: &SourceFiles, file: FileId) -> Compiled {
    let (analysis, mut diagnostics) = analyze(files, file);
    // `main` がないことは実行するときだけ誤りにする。モジュール (S2) は `main` を持たないため (docs/spec/types.md)
    if analysis.main.is_none() {
        diagnostics.push(eml_types::missing_main(file));
    }
    let program = (!has_errors(&diagnostics)).then(|| Arc::new(analysis.program));
    Compiled {
        diagnostics,
        program,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    Completed,
    RuntimeError(String),
}

pub fn execute(program: Arc<Program>, config: &RunConfig, stdout: OutputSink) -> RunResult {
    match eml_interp::run(program, config, &stdout) {
        Ok(()) => RunResult::Completed,
        Err(error) => RunResult::RuntimeError(error.to_string()),
    }
}

struct Analysis {
    program: Program,
    main: Option<eml_hir::FunctionId>,
}

/// エラーがあっても止めずに全段階を実行する。1回の実行で、独立した複数のエラーを報告するため。
fn analyze(files: &SourceFiles, file: FileId) -> (Analysis, Vec<Diagnostic>) {
    let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    let main = typed.main;
    let (program, stage) = eml_core_ir::lower(&typed);
    diagnostics.extend(stage);
    (Analysis { program, main }, diagnostics)
}

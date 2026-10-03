//! UI テストからプロセス内で呼べるように、CLI の中身をバイナリではなく lib に置く。

use std::sync::Arc;

use eml_core_ir::Program;
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, has_errors};

pub use eml_interp::RunConfig;
pub use eml_runtime::OutputSink;

pub fn check(files: &SourceFiles, file: FileId) -> Vec<Diagnostic> {
    analyze(files, file).1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    NotRun,
    Completed,
    RuntimeError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// 実行した場合も、警告を表示できるように検査の診断を返す。
    pub diagnostics: Vec<Diagnostic>,
    pub result: RunResult,
}

pub fn run(
    files: &SourceFiles,
    file: FileId,
    config: &RunConfig,
    stdout: OutputSink,
) -> RunOutcome {
    let (program, diagnostics) = analyze(files, file);
    if has_errors(&diagnostics) {
        return RunOutcome {
            diagnostics,
            result: RunResult::NotRun,
        };
    }
    let result = match eml_interp::run(Arc::new(program), config, &stdout) {
        Ok(()) => RunResult::Completed,
        Err(error) => RunResult::RuntimeError(error.to_string()),
    };
    RunOutcome {
        diagnostics,
        result,
    }
}

/// エラーがあっても止めずに全段階を実行する。1回の実行で、独立した複数のエラーを報告するため。
fn analyze(files: &SourceFiles, file: FileId) -> (Program, Vec<Diagnostic>) {
    let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    let (program, stage) = eml_core_ir::lower(&typed);
    diagnostics.extend(stage);
    (program, diagnostics)
}

//! `eml check` / `eml run` の中身。各段階をつなぐだけで、テストからプロセス内で呼べる API を公開する。

use std::sync::Arc;

use eml_core_ir::Program;
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, has_errors};

pub use eml_interp::RunConfig;
pub use eml_runtime::OutputSink;

/// ファイルを検査し、すべての段階の診断を返す。
pub fn check(files: &SourceFiles, file: FileId) -> Vec<Diagnostic> {
    analyze(files, file).1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    /// 診断のエラーがあったので実行しなかった。
    NotRun,
    Completed,
    RuntimeError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// 検査で出た診断 (警告を含む)。
    pub diagnostics: Vec<Diagnostic>,
    pub result: RunResult,
}

/// ファイルを検査し、エラーがなければ実行する。プログラムの出力は `stdout` に書く。
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

/// 各段階を順につなぐ。エラーがあっても止めず、すべての段階の診断を集める。
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

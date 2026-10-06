//! UI テストからプロセス内で呼べるように、CLI の中身をバイナリではなく lib に置く。

use std::sync::Arc;

use eml_core_ir::Program;
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, has_errors, sort_diagnostics};

pub use eml_interp::{RunConfig, RuntimeError};
pub use eml_runtime::{Captured, OutputSink};

/// 呼び出し側が実行の前に診断を表示できるように、検査と実行を別の関数にする (docs/implementation/architecture.md)。
#[derive(Debug)]
pub struct Compiled {
    /// 警告を含む。
    pub diagnostics: Vec<Diagnostic>,
    /// エラーがあれば `None`。
    pub program: Option<Arc<Program>>,
}

/// 1回の検査や実行で読むソースの集まり。Prelude を最初に登録し、入口のファイルと一緒に変換する
/// (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 1.4)。import をたどるローダは S2 で足す。
pub struct Session {
    files: SourceFiles,
    prelude: FileId,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    pub fn new() -> Session {
        let mut files = SourceFiles::new();
        let prelude = files.add(eml_hir::PRELUDE_PATH, eml_hir::PRELUDE_SOURCE);
        Session { files, prelude }
    }

    pub fn add_file(&mut self, path: impl Into<String>, text: impl Into<String>) -> FileId {
        self.files.add(path, text)
    }

    /// 診断の表示に使う。
    pub fn files(&self) -> &SourceFiles {
        &self.files
    }

    /// Prelude の番号。Prelude の範囲を指す診断を確かめるのに使う。
    pub fn prelude(&self) -> FileId {
        self.prelude
    }

    pub fn check(&self, entry: FileId) -> Vec<Diagnostic> {
        let mut diagnostics = self.front(entry).2;
        sort_diagnostics(&mut diagnostics);
        diagnostics
    }

    pub fn compile(&self, entry: FileId) -> Compiled {
        let (program, typed, mut diagnostics) = self.front(entry);
        // `main` がないことは実行するときだけ誤りにする。モジュール (S2) は `main` を持たないため (docs/spec/types.md)
        if typed.main.is_none() {
            diagnostics.push(eml_types::missing_main(entry));
        }
        sort_diagnostics(&mut diagnostics);
        // Core IR は誤りのないプログラムだけを受け取る (docs/implementation/architecture.md)
        let program =
            (!has_errors(&diagnostics)).then(|| Arc::new(eml_core_ir::lower(&program, &typed)));
        Compiled {
            diagnostics,
            program,
        }
    }

    /// エラーがあっても止めずに、検査の段階をすべて実行する。1回の実行で、独立した複数のエラーを報告するため。
    fn front(&self, entry: FileId) -> (eml_hir::Program, eml_types::TypedModule, Vec<Diagnostic>) {
        let (parse, mut diagnostics) = eml_syntax::parse(entry, self.files.text(entry));
        let prelude = eml_hir::parse_prelude(self.prelude);
        let (program, stage) = eml_hir::lower((self.prelude, &prelude), (entry, &parse.tree()));
        diagnostics.extend(stage);
        let (typed, stage) = eml_types::check(&program);
        diagnostics.extend(stage);
        (program, typed, diagnostics)
    }
}

pub fn execute(
    program: Arc<Program>,
    config: &RunConfig,
    stdout: OutputSink,
) -> Result<(), RuntimeError> {
    eml_interp::run(program, config, &stdout)
}

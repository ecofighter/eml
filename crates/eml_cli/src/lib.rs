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
/// (docs/implementation/architecture.md の「CLI と lib API」)。import をたどるローダは M2 で足す。
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
        // `main` がないことは実行するときだけ誤りにする。モジュール (M2) は `main` を持たないため (docs/spec/types.md)
        let main = program.main();
        if main.is_none() {
            diagnostics.push(eml_types::missing_main(entry));
        }
        sort_diagnostics(&mut diagnostics);
        // Core IR は誤りのないプログラムだけを受け取る (docs/implementation/architecture.md)
        let program = match main {
            Some(main) if !has_errors(&diagnostics) => {
                Some(Arc::new(eml_core_ir::lower(&program, &typed, main)))
            }
            _ => None,
        };
        Compiled {
            diagnostics,
            program,
        }
    }

    /// エラーがあっても止めずに、検査の段階をすべて実行する。1回の実行で、独立した複数のエラーを報告するため。
    fn front(&self, entry: FileId) -> (eml_hir::Program, eml_types::TypedProgram, Vec<Diagnostic>) {
        // 読み込みの段は Prelude と入口をこの `Session` と同じ順に登録するので、診断の `FileId` は `self.files` を指す
        let (loaded, mut diagnostics) =
            eml_hir::load(self.files.path(entry), self.files.text(entry), &NoModules);
        debug_assert_eq!(loaded.entry, entry);
        let (def_map, stage) = eml_hir::def_map(&loaded.modules);
        diagnostics.extend(stage);
        let (program, stage) = eml_hir::lower(&def_map, &loaded.modules);
        diagnostics.extend(stage);
        let (typed, stage) = eml_types::check(&program);
        diagnostics.extend(stage);
        (program, typed, diagnostics)
    }
}

/// `Session` はファイルシステムから依存先を読まないので、どの import も E1026 になる。
struct NoModules;

impl eml_hir::ModuleSource for NoModules {
    fn read(&self, _: &eml_hir::ModulePath) -> Result<String, eml_hir::ReadError> {
        Err(eml_hir::ReadError::NotFound)
    }
}

pub fn execute(
    program: Arc<Program>,
    config: &RunConfig,
    stdout: OutputSink,
) -> Result<(), RuntimeError> {
    eml_interp::run(program, config, &stdout)
}

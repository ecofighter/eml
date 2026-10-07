//! UI テストからプロセス内で呼べるように、CLI の中身をバイナリではなく lib に置く。

mod fs_provider;

use std::sync::Arc;

use eml_core_ir::Program;
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, has_errors, sort_diagnostics};

pub use eml_hir::{ModulePath, ModuleSource, ReadError};
pub use eml_interp::{RunConfig, RuntimeError};
pub use eml_runtime::{Captured, OutputSink};
pub use fs_provider::FsProvider;

/// 呼び出し側が実行の前に診断を表示できるように、検査と実行を別の関数にする (docs/implementation/architecture.md)。
#[derive(Debug)]
pub struct Compiled {
    /// 警告を含む。
    pub diagnostics: Vec<Diagnostic>,
    /// エラーがあれば `None`。
    pub program: Option<Arc<Program>>,
}

/// 1回の検査や実行で読むソースの集まり。読み込みの段が Prelude、入口、import でたどった依存先を登録する
/// (docs/implementation/architecture.md の「CLI と lib API」)。`eml_cli` は段階をつなぐだけで、診断を自分では作らない。
pub struct Session {
    loaded: eml_hir::Loaded,
    /// 構文解析、`ItemTree`、読み込みの段の診断。
    load_diagnostics: Vec<Diagnostic>,
}

impl Session {
    /// ファイルはここで読み終える。`check` と `compile` は同じ読み込みの結果を使う。
    pub fn load(entry_path: &str, entry_text: &str, source: &dyn ModuleSource) -> Session {
        let (loaded, load_diagnostics) = eml_hir::load(entry_path, entry_text, source);
        Session {
            loaded,
            load_diagnostics,
        }
    }

    /// 診断の表示に使う。
    pub fn files(&self) -> &SourceFiles {
        &self.loaded.files
    }

    /// Prelude の番号。Prelude の範囲を指す診断を確かめるのに使う。
    pub fn prelude(&self) -> FileId {
        self.loaded.prelude
    }

    /// 入口のファイルの番号。`main` がないことの診断はここを指す。
    pub fn entry(&self) -> FileId {
        self.loaded.entry
    }

    /// 読み込んだモジュールの名前。番号の順で、Prelude、入口 (`Main`)、Prelude を除く標準ライブラリ、import でたどった
    /// 依存先の順に並ぶ。
    pub fn module_names(&self) -> impl Iterator<Item = &str> {
        self.loaded
            .modules
            .iter()
            .map(|module| module.name.as_str())
    }

    /// 出どころがユーザーのモジュールの名前。入口 (`Main`) と、import でたどった依存先である。標準ライブラリはいつも
    /// 読み込むので、UI テストの harness は 1ファイルのテストが import していないことをこれで確かめる。
    pub fn user_module_names(&self) -> impl Iterator<Item = &str> {
        self.loaded
            .modules
            .iter()
            .filter(|module| module.origin == eml_hir::ModuleOrigin::User)
            .map(|module| module.name.as_str())
    }

    pub fn check(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.front().2;
        sort_diagnostics(&mut diagnostics);
        diagnostics
    }

    pub fn compile(&self) -> Compiled {
        let (program, typed, mut diagnostics) = self.front();
        // `main` がないことは実行するときだけ誤りにする。`main` を持たないファイルも検査できるようにするため (docs/spec/types.md)
        let main = program.main();
        if main.is_none() {
            diagnostics.push(eml_types::missing_main(self.loaded.entry));
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
    fn front(&self) -> (eml_hir::Program, eml_types::TypedProgram, Vec<Diagnostic>) {
        let mut diagnostics = self.load_diagnostics.clone();
        let (def_map, stage) = eml_hir::def_map(&self.loaded.modules);
        diagnostics.extend(stage);
        let (program, stage) = eml_hir::lower(&def_map, &self.loaded.modules);
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

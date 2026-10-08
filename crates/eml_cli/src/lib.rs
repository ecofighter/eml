//! UI テストからプロセス内で呼べるように、CLI の中身をバイナリではなく lib に置く。
//!
//! パイプラインを組むのは `Session` だけで、CLI も `eml_test_support` もこれを通す。CLI とテストが同じ順で段階をつなぎ、
//! 同じ診断を集めるためである。段階は feature (`types` < `core` < `run`) で選ぶ。`eml_test_support` がこの feature で
//! 段階を選ぶので、各 crate のテストは自分より下流の crate を組み立てない (docs/implementation/testing.md)。

mod fs_provider;

#[cfg(feature = "core")]
use std::sync::Arc;

#[cfg(feature = "core")]
use eml_core_ir::{Pass, Program};
#[cfg(feature = "core")]
use eml_diagnostics::has_errors;
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, sort_diagnostics};

pub use eml_hir::{ModulePath, ModuleSource, ReadError};
#[cfg(feature = "run")]
pub use eml_interp::{RunConfig, RuntimeError};
#[cfg(feature = "run")]
pub use eml_runtime::{Captured, OutputSink};
pub use fs_provider::FsProvider;

/// `DefMap` と、読み込みの段と def_map の段の診断。
pub struct DefMapped {
    pub def_map: eml_hir::DefMap,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Lowered {
    pub program: eml_hir::Program,
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "types")]
pub struct Checked {
    pub program: eml_hir::Program,
    pub typed: eml_types::TypedProgram,
    pub diagnostics: Vec<Diagnostic>,
}

/// 呼び出し側が実行の前に診断を表示できるように、検査と実行を別の関数にする (docs/implementation/architecture.md)。
#[cfg(feature = "core")]
#[derive(Debug)]
pub struct Compiled {
    /// 警告を含む。
    pub diagnostics: Vec<Diagnostic>,
    /// エラーがあれば `None`。
    pub program: Option<Arc<Program>>,
}

/// 1回の検査や実行で読むソースの集まり。読み込みの段が Prelude、入口、import でたどった依存先を登録する
/// (docs/implementation/architecture.md の「CLI と lib API」)。`eml_cli` は段階をつなぐだけで、診断を自分では作らない。
///
/// 段階のメソッドは、エラーがあっても止めずに、読み込みの結果から呼ばれた段階までをすべて実行する。1回の実行で、
/// 独立した複数のエラーを報告するため。途中の結果は持たない。段階の結果の診断は、読み込みの段からその段階までの
/// すべての診断を、表示と同じ順 (`sort_diagnostics`) に並べたものである。
pub struct Session {
    loaded: eml_hir::Loaded,
    /// 構文解析、`ItemTree`、読み込みの段の診断。
    load_diagnostics: Vec<Diagnostic>,
}

impl Session {
    /// ファイルはここで読み終える。段階のメソッドはどれも同じ読み込みの結果を使う。
    pub fn load(entry_path: &str, entry_text: &str, source: &dyn ModuleSource) -> Session {
        Session::new(eml_hir::load(entry_path, entry_text, source))
    }

    /// 標準ライブラリを `(ファイル名, 本文)` の並びに差し替えて読む。標準ライブラリの中の item の扱いを確かめるテストの
    /// ための口である。並びの条件は `eml_hir::load_with_std` と同じである。
    pub fn load_with_std(
        std: &[(&str, &str)],
        entry_path: &str,
        entry_text: &str,
        source: &dyn ModuleSource,
    ) -> Session {
        Session::new(eml_hir::load_with_std(std, entry_path, entry_text, source))
    }

    fn new((loaded, load_diagnostics): (eml_hir::Loaded, Vec<Diagnostic>)) -> Session {
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

    pub fn def_map(&self) -> DefMapped {
        let mut diagnostics = self.load_diagnostics.clone();
        let (def_map, stage) = eml_hir::def_map(&self.loaded.modules);
        diagnostics.extend(stage);
        sort_diagnostics(&mut diagnostics);
        DefMapped {
            def_map,
            diagnostics,
        }
    }

    pub fn lower(&self) -> Lowered {
        let DefMapped {
            def_map,
            mut diagnostics,
        } = self.def_map();
        let (program, stage) = eml_hir::lower(&def_map, &self.loaded.modules);
        diagnostics.extend(stage);
        sort_diagnostics(&mut diagnostics);
        Lowered {
            program,
            diagnostics,
        }
    }

    #[cfg(feature = "types")]
    pub fn check(&self) -> Checked {
        let Lowered {
            program,
            mut diagnostics,
        } = self.lower();
        let (typed, stage) = eml_types::check(&program, self.files());
        diagnostics.extend(stage);
        sort_diagnostics(&mut diagnostics);
        Checked {
            program,
            typed,
            diagnostics,
        }
    }

    #[cfg(feature = "core")]
    pub fn compile(&self) -> Compiled {
        self.compile_with(eml_core_ir::lower)
    }

    /// `last` のパスの直後で Core IR を止める。確かめたいパスの直後の IR を見るテストのための口である
    /// (`eml_core_ir::lower_until`)。
    #[cfg(feature = "core")]
    pub fn compile_until(&self, last: Pass) -> Compiled {
        self.compile_with(|hir, typed, main| eml_core_ir::lower_until(hir, typed, main, last))
    }

    /// パスの順番は `eml_core_ir` だけが知るので、Core IR を作る関数を受け取る。
    #[cfg(feature = "core")]
    fn compile_with(
        &self,
        lower: impl FnOnce(&eml_hir::Program, &eml_types::TypedProgram, eml_hir::FunctionId) -> Program,
    ) -> Compiled {
        let Checked {
            program,
            typed,
            mut diagnostics,
        } = self.check();
        // `main` がないことは実行するときだけ誤りにする。`main` を持たないファイルも検査できるようにするため (docs/spec/types.md)
        let main = program.main();
        if main.is_none() {
            diagnostics.push(eml_types::missing_main(self.loaded.entry));
        }
        sort_diagnostics(&mut diagnostics);
        // Core IR は誤りのないプログラムだけを受け取る (docs/implementation/architecture.md)
        let program = match main {
            Some(main) if !has_errors(&diagnostics) => {
                Some(Arc::new(lower(&program, &typed, main)))
            }
            _ => None,
        };
        Compiled {
            diagnostics,
            program,
        }
    }
}

#[cfg(feature = "run")]
pub fn execute(
    program: Arc<Program>,
    config: &RunConfig,
    stdout: OutputSink,
) -> Result<(), RuntimeError> {
    eml_interp::run(program, config, &stdout)
}

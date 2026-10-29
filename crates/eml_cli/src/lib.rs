//! UI テストからプロセス内で呼べるように、CLI の中身をバイナリではなく lib に置く。
//!
//! パイプラインを組むのは `Session` だけで、CLI も `eml_test_support` もこれを通す。CLI とテストが同じ順で段階をつなぎ、
//! 同じ診断を集めるためである。段階は feature (`types` < `core` < `run`) で選ぶ。`eml_test_support` がこの feature で
//! 段階を選ぶので、各 crate のテストは自分より下流の crate を組み立てない (docs/implementation/testing.md)。

mod fs_provider;
pub mod lines;
pub mod pretty;
#[cfg(feature = "run")]
mod repl;

#[cfg(feature = "core")]
use eml_diagnostics::has_errors;
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, sort_diagnostics};

pub use eml_hir::{MemorySource, ModulePath, ModuleSource, ReadError};
pub use eml_syntax::{InputStatus, input_status};
#[cfg(feature = "run")]
pub use eml_vm::{
    Capture, CaptureHost, Clock, Control, Doorbell, Finished, HostRef, Limits, RunConfig, RunStats,
    RuntimeError, Sched, Signal, SourceLocation, Stop, VirtualSignal, VmFault, Written,
    capture_host, execute, script_host, stdio_host,
};
pub use fs_provider::FsProvider;
#[cfg(feature = "run")]
pub use repl::{Closed, Evaluated, Outcome, Repl, Typed};

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
pub struct Compiled<P> {
    /// 警告を含む。
    pub diagnostics: Vec<Diagnostic>,
    /// エラーがあれば `None`。
    pub program: Option<P>,
}

/// 1回の検査や実行で読むソースの集まり。読み込みの段が Prelude、エントリ、import でたどった依存先を登録する
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
    /// ためのインターフェースである。並びの条件は `eml_hir::load_with_std` と同じである。
    pub fn load_with_std(
        std: &[(&str, &str)],
        entry_path: &str,
        entry_text: &str,
        source: &dyn ModuleSource,
    ) -> Session {
        Session::new(eml_hir::load_with_std(std, entry_path, entry_text, source))
    }

    /// REPL の受け付けた入力と今の入力を、入力ごとのモジュールとして読む
    /// (docs/spec/modules.md の「REPL のセッション」)。`entry_file` は `:load` のファイル名で、なければ空である。
    /// 返す `ImportCache` は、この読み込みで新しく読んだモジュールで、`Repl` が入力を受け付けたときにだけ足す。
    pub fn load_session(
        inputs: &[eml_hir::SessionInput],
        file: Option<&str>,
        source: &dyn ModuleSource,
        cache: &eml_hir::ImportCache,
    ) -> (Session, eml_hir::ImportCache) {
        let (loaded, diagnostics, read) =
            eml_hir::load_session(eml_hir::STD, inputs, file, source, cache);
        (Session::new((loaded, diagnostics)), read)
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

    /// 診断を表示するファイルを、セッションを手放して返す。`Repl` は入力ごとにセッションを作り直すので、その入力の診断と
    /// 一緒に渡す。
    pub fn into_files(self) -> SourceFiles {
        self.loaded.files
    }

    /// Prelude の番号。Prelude の範囲を指す診断を確かめるのに使う。
    pub fn prelude(&self) -> FileId {
        self.loaded.prelude
    }

    /// エントリのファイルの番号。`main` がないことの診断はここを指す。
    pub fn entry(&self) -> FileId {
        self.loaded.entry
    }

    /// 読み込んだモジュールの名前。番号の順で、Prelude、エントリ (`Main`) の後に、import を幅優先でたどって見つけた順の
    /// モジュールが並ぶ。
    pub fn module_names(&self) -> impl Iterator<Item = &str> {
        self.loaded
            .modules
            .iter()
            .map(|module| module.name.as_str())
    }

    /// 出どころがユーザーのモジュールの名前。エントリ (`Main`) と、import でたどった依存先である。標準ライブラリはいつも
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
        let (def_map, stage) = eml_hir::def_map(&self.loaded);
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

    /// reuse の後の LIR を作る。
    #[cfg(feature = "core")]
    pub fn compile(&self) -> Compiled<eml_lir::Program> {
        self.compile_with(eml_core_ir::lower)
    }

    /// 検査が通れば、`lower` で IR を作る。パスの順番は `eml_core_ir` と `eml_lir` だけが知るので、IR を作る関数を
    /// 受け取る。テストは `eml_core_ir::lower_until` や `core_until` を渡して、確かめたいパスの直後で止める。
    #[cfg(feature = "core")]
    pub fn compile_with<P>(&self, lower: impl FnOnce(eml_core_ir::Input) -> P) -> Compiled<P> {
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
            Some(entry) if !has_errors(&diagnostics) => Some(lower(eml_core_ir::Input {
                hir: &program,
                typed: &typed,
                entry,
                files: self.files(),
            })),
            _ => None,
        };
        Compiled {
            diagnostics,
            program,
        }
    }
}

/// `eml run` の既定の上限。深さは 2^27 語 (1 GiB 分のスタック) で、OCaml 5 のスタックの既定の上限と同じ量である。
/// 確保量は物理メモリの 80% で、機械を使い切る前に分かる誤りで止める。物理メモリを読めない OS では上限を置かない。
/// `evv` はヒープの物体なので、確保量の上限で抑えられる (docs/spec/object-model.md の「実行の制御」)。
#[cfg(feature = "run")]
pub fn default_limits() -> Limits {
    Limits {
        max_depth: 1 << 27,
        max_bytes: physical_memory().map_or(usize::MAX, |bytes| bytes / 5 * 4),
        ..Limits::default()
    }
}

/// `eml repl` の上限。確保量は物理メモリの 40% で、深さは `default_limits()` と同じである。回復は写す間に古いヒープと
/// 写しを同時に持つので、合わせて `eml run` と同じ 80% に収める (docs/spec/repl.md の「コマンドライン」)。
#[cfg(feature = "run")]
pub fn repl_limits() -> Limits {
    Limits {
        max_bytes: physical_memory().map_or(usize::MAX, |bytes| bytes / 5 * 2),
        ..default_limits()
    }
}

/// `eml_rt` は vstd だけに依存したままにするため、OS に問い合わせるのはここである。`src/` の `unsafe` の呼び出しは、
/// この関数と、bin の `interrupt` モジュールと `terminal.rs` の `stdout_width` の中だけに置く。
#[cfg(all(feature = "run", unix))]
fn physical_memory() -> Option<usize> {
    // sysconf は名前のほかに前提を持たず、読めなければ -1 を返す
    let (pages, page_size) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    usize::try_from(pages)
        .ok()?
        .checked_mul(usize::try_from(page_size).ok()?)
        .filter(|&bytes| bytes > 0)
}

#[cfg(all(feature = "run", windows))]
fn physical_memory() -> Option<usize> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // MEMORYSTATUSEX は全部が整数なので、零で埋めてよい。呼ぶ前に dwLength を入れる決まりで、失敗すれば 0 を返す
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return None;
    }
    usize::try_from(status.ullTotalPhys)
        .ok()
        .filter(|&bytes| bytes > 0)
}

#[cfg(all(feature = "run", not(any(unix, windows))))]
fn physical_memory() -> Option<usize> {
    None
}

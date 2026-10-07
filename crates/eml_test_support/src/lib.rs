//! テストのためにパイプラインを組む処理 (診断のないことを確かめるものを含む)、診断を文字列にする処理 (docs/implementation/testing.md)。
//!
//! この crate は、テストする crate の型をそのまま使う。そのため、使ってよいのは各 crate の `tests/` にある結合テスト
//! からだけである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる。
//!
//! パイプラインは `eml_cli::Session` で組み、ここでは包むだけにする。CLI と同じ順で段階をつなぎ、同じ診断を集める
//! ため。段階は feature で選ぶ (`hir` < `types` < `core` < `run`)。各 crate は自分の段階までを有効にし、下流の crate に
//! テストを依存させない。

use std::fmt::Write;
#[cfg(feature = "core")]
use std::sync::Arc;

#[cfg(feature = "hir")]
use eml_cli::Session;
#[cfg(feature = "core")]
use eml_core_ir::{Pass, Program};
use eml_diagnostics::{Diagnostic, FileId, Label, LineCol, SourceFiles, sort_diagnostics};
#[cfg(feature = "run")]
use eml_interp::{RunConfig, RuntimeError};
#[cfg(feature = "run")]
use eml_runtime::OutputSink;

pub struct Parsed {
    pub files: SourceFiles,
    pub file: FileId,
    pub parse: eml_syntax::Parse,
    pub diagnostics: Vec<Diagnostic>,
}

/// `SourceFiles` は Clone できないので、ファイルは `Session` ごと持ち、メソッドで出す。
#[cfg(feature = "hir")]
pub struct Lowered {
    session: Session,
    pub program: eml_hir::Program,
    /// 読み込み、構文、HIR の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "hir")]
impl Lowered {
    pub fn files(&self) -> &SourceFiles {
        self.session.files()
    }

    pub fn file(&self) -> FileId {
        self.session.entry()
    }
}

#[cfg(feature = "types")]
pub struct Checked {
    session: Session,
    pub program: eml_hir::Program,
    pub typed: eml_types::TypedProgram,
    /// 読み込み、構文、HIR、型の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "types")]
impl Checked {
    pub fn files(&self) -> &SourceFiles {
        self.session.files()
    }

    pub fn file(&self) -> FileId {
        self.session.entry()
    }
}

/// UI テスト以外のテストで登録する、入口のファイルの表示のパス。
pub const ENTRY_PATH: &str = "test.em";

/// メモリ上の (根からの相対パス, 本文)。複数のファイルのテストでも、CLI と同じ読み込みの段を通すため。
pub struct MemorySource<'a>(pub &'a [(&'a str, &'a str)]);

#[cfg(feature = "hir")]
impl eml_hir::ModuleSource for MemorySource<'_> {
    fn read(&self, path: &eml_hir::ModulePath) -> Result<String, eml_hir::ReadError> {
        let wanted = path.file_path();
        self.0
            .iter()
            .find(|(file, _)| *file == wanted)
            .map(|(_, text)| text.to_string())
            .ok_or(eml_hir::ReadError::NotFound)
    }
}

pub fn source(text: &str) -> (SourceFiles, FileId) {
    let mut files = SourceFiles::new();
    let file = files.add(ENTRY_PATH, text);
    (files, file)
}

/// 構文の段だけを通す。どのテストでも lossless を確かめるため、木が元のテキストに戻ることもここで確認する。構文解析
/// するのは `SourceFiles` に保存したテキスト (先頭の BOM を除いたもの) である (docs/spec/lexical.md)。
pub fn parse(text: &str) -> Parsed {
    let (files, file) = source(text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
    assert_eq!(
        parse.syntax().text().to_string(),
        files.text(file),
        "tree must be lossless"
    );
    sort_diagnostics(&mut diagnostics);
    Parsed {
        files,
        file,
        parse,
        diagnostics,
    }
}

/// `modules` は根からの相対パス (`"Report/Csv.em"`) と本文の組である。
#[cfg(feature = "hir")]
fn load(entry: &str, modules: &[(&str, &str)]) -> Session {
    Session::load(ENTRY_PATH, entry, &MemorySource(modules))
}

/// 標準ライブラリを `(ファイル名, 本文)` の並びに差し替える。標準ライブラリの中の item の扱いを確かめるテストのため。
/// 並びは `Prelude.em` と、本物の `Fs.em` (または同じ extern の宣言を持つもの) を含める。extern の索引が両方を引くので、
/// 足りないと panic する。
#[cfg(feature = "hir")]
fn load_with_std(std: &[(&str, &str)], entry: &str) -> Session {
    Session::load_with_std(std, ENTRY_PATH, entry, &MemorySource(&[]))
}

#[cfg(feature = "hir")]
pub fn lower(text: &str) -> Lowered {
    lower_files(text, &[])
}

/// 診断には読み込みの段のものも入る。
#[cfg(feature = "hir")]
pub fn lower_files(entry: &str, modules: &[(&str, &str)]) -> Lowered {
    lower_session(load(entry, modules))
}

#[cfg(feature = "hir")]
pub fn lower_with_std(std: &[(&str, &str)], entry: &str) -> Lowered {
    lower_session(load_with_std(std, entry))
}

#[cfg(feature = "hir")]
fn lower_session(session: Session) -> Lowered {
    let eml_cli::Lowered {
        program,
        diagnostics,
    } = session.lower();
    Lowered {
        session,
        program,
        diagnostics,
    }
}

#[cfg(feature = "hir")]
pub fn def_map(text: &str) -> (eml_hir::DefMap, Vec<String>) {
    def_map_files(text, &[])
}

/// 診断は、読み込みの段と def_map の段のものである。
#[cfg(feature = "hir")]
pub fn def_map_files(entry: &str, modules: &[(&str, &str)]) -> (eml_hir::DefMap, Vec<String>) {
    let session = load(entry, modules);
    let eml_cli::DefMapped {
        def_map,
        diagnostics,
    } = session.def_map();
    (def_map, short(session.files(), &diagnostics))
}

/// 前提として診断のないソースを使うテストのため。条件を緩めないよう、警告も1件として数える。
pub fn parse_clean(text: &str) -> Parsed {
    let parsed = parse(text);
    assert_clean(&parsed.files, &parsed.diagnostics);
    parsed
}

#[cfg(feature = "hir")]
pub fn lower_clean(text: &str) -> Lowered {
    let lowered = lower(text);
    assert_clean(lowered.files(), &lowered.diagnostics);
    lowered
}

fn assert_clean(files: &SourceFiles, diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{}",
        short_text(files, diagnostics)
    );
}

#[cfg(feature = "types")]
pub fn check(text: &str) -> Checked {
    check_files(text, &[])
}

#[cfg(feature = "types")]
pub fn check_files(entry: &str, modules: &[(&str, &str)]) -> Checked {
    check_session(load(entry, modules))
}

/// 標準ライブラリを差し替えて型検査をする。並びの条件は `lower_with_std` と同じである。
#[cfg(feature = "types")]
pub fn check_with_std(std: &[(&str, &str)], entry: &str) -> Checked {
    check_session(load_with_std(std, entry))
}

#[cfg(feature = "types")]
fn check_session(session: Session) -> Checked {
    let eml_cli::Checked {
        program,
        typed,
        diagnostics,
    } = session.check();
    Checked {
        session,
        program,
        typed,
        diagnostics,
    }
}

#[cfg(feature = "core")]
pub fn core(text: &str) -> Arc<Program> {
    core_files(text, &[])
}

#[cfg(feature = "core")]
pub fn core_files(entry: &str, modules: &[(&str, &str)]) -> Arc<Program> {
    compiled(load(entry, modules).compile())
}

/// 確かめたいパスの直後の Core IR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn core_until(text: &str, last: Pass) -> Arc<Program> {
    core_until_files(text, &[], last)
}

#[cfg(feature = "core")]
pub fn core_until_files(entry: &str, modules: &[(&str, &str)], last: Pass) -> Arc<Program> {
    compiled(load(entry, modules).compile_until(last))
}

/// `eml run` と同じく、エラーがあれば Core IR を作らない。`main` がないこともエラーである。
#[cfg(feature = "core")]
fn compiled(compiled: eml_cli::Compiled) -> Arc<Program> {
    compiled
        .program
        .unwrap_or_else(|| panic!("{:#?}", compiled.diagnostics))
}

/// 実行のテストでは、つねに `debug_heap` を有効にする (docs/implementation/testing.md)。
#[cfg(feature = "run")]
pub fn run(text: &str) -> (String, Result<(), RuntimeError>) {
    run_files(text, &[])
}

#[cfg(feature = "run")]
pub fn run_files(entry: &str, modules: &[(&str, &str)]) -> (String, Result<(), RuntimeError>) {
    run_program(core_files(entry, modules), true)
}

/// 手で書いた Core IR を実行する。
#[cfg(feature = "run")]
pub fn execute(program: Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    run_program(Arc::new(program), debug_heap)
}

/// 実行の設定と出力の受け口は、ここだけで組み立てる。
#[cfg(feature = "run")]
fn run_program(program: Arc<Program>, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(debug_heap);
    let result = eml_cli::execute(program, &config, sink);
    (captured.contents(), result)
}

/// 1件を `E0001 1:2 message` の1行にする。期待値を読みやすくするため、位置はバイトではなく行と列にする。
pub fn short(files: &SourceFiles, diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| format!("{} {} {}", d.code, position(files, &d.primary), d.message))
        .collect()
}

/// `full` と同じく文字列にして、段階の表示の後ろにそのまま足せるようにする。
pub fn short_text(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    short(files, diagnostics)
        .into_iter()
        .map(|line| line + "\n")
        .collect()
}

/// 診断の形式は段階のテストごとに違うので、区切り方だけをここでそろえる。
pub fn with_diagnostics(mut dump: String, diagnostics: &str) -> String {
    if !diagnostics.is_empty() {
        dump.push_str("---\n");
        dump.push_str(diagnostics);
    }
    dump
}

/// fix のある診断ごとに、先頭の行に続けて編集を `開始..終了 "置き換える文字列"` の形で並べる。fix のない診断は出さない。
pub fn fixes(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in diagnostics {
        let Some(fix) = &d.fix else { continue };
        writeln!(
            out,
            "{} {} {}",
            d.code,
            position(files, &d.primary),
            fix.title
        )
        .unwrap();
        for edit in &fix.edits {
            let start = files.line_col(edit.file, edit.range.start());
            let end = files.line_col(edit.file, edit.range.end());
            writeln!(out, "  {start}..{end} {:?}", edit.replacement).unwrap();
        }
    }
    out
}

/// 1件を、先頭の行に続けてラベル、note、help を字下げした行にする。
pub fn full(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    report(diagnostics, |label| position(files, label).to_string())
}

/// 位置の前にファイルの表示のパスを付けた `full`。複数のモジュールのテストで、診断がどのファイルを指すかを読み分けるため。
pub fn full_with_paths(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    report(diagnostics, |label| located(files, label))
}

/// `test.em 1:2` の形の位置。
pub fn located(files: &SourceFiles, label: &Label) -> String {
    format!("{} {}", files.path(label.file), position(files, label))
}

fn report(diagnostics: &[Diagnostic], at: impl Fn(&Label) -> String) -> String {
    let mut out = String::new();
    for d in diagnostics {
        let primary = at(&d.primary);
        writeln!(out, "{} {primary} {}", d.code, d.message).unwrap();
        writeln!(out, "  {primary} {}", d.primary.message).unwrap();
        for label in &d.secondary {
            writeln!(out, "  {} {}", at(label), label.message).unwrap();
        }
        for note in &d.notes {
            writeln!(out, "  note: {note}").unwrap();
        }
        for help in &d.help {
            writeln!(out, "  help: {help}").unwrap();
        }
    }
    out
}

fn position(files: &SourceFiles, label: &Label) -> LineCol {
    files.line_col(label.file, label.range.start())
}

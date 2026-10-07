//! テストのためにパイプラインを組む処理 (診断のないことを確かめるものを含む)、診断を文字列にする処理 (docs/implementation/testing.md)。
//!
//! この crate は、テストする crate の型をそのまま使う。そのため、使ってよいのは各 crate の `tests/` にある結合テスト
//! からだけである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる。
//!
//! 段階は feature で選ぶ (`hir` < `types` < `core` < `run`)。各 crate は自分の段階までを有効にし、下流の crate に
//! テストを依存させない。

use std::fmt::Write;
#[cfg(feature = "run")]
use std::sync::Arc;

#[cfg(feature = "core")]
use eml_core_ir::{Pass, Program};
#[cfg(feature = "core")]
use eml_diagnostics::has_errors;
use eml_diagnostics::{Diagnostic, FileId, Label, LineCol, SourceFiles, sort_diagnostics};
#[cfg(feature = "run")]
use eml_interp::{RunConfig, RuntimeError};
#[cfg(feature = "run")]
use eml_runtime::OutputSink;

pub struct Parsed {
    pub files: SourceFiles,
    /// `hir` の feature のときに登録する Prelude の番号。
    pub prelude: Option<FileId>,
    pub file: FileId,
    pub parse: eml_syntax::Parse,
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "hir")]
pub struct Lowered {
    pub files: SourceFiles,
    pub file: FileId,
    pub program: eml_hir::Program,
    /// 構文と HIR の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "types")]
pub struct Checked {
    pub files: SourceFiles,
    pub file: FileId,
    pub program: eml_hir::Program,
    pub typed: eml_types::TypedProgram,
    /// 構文、HIR、型の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
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
    let (files, _, file) = source_with_prelude(text);
    (files, file)
}

/// 読み込みの段 (`eml_hir::load`) と同じく、Prelude を先に登録する。診断はファイルの番号の順に並ぶので、Prelude の範囲を
/// 指す診断が出たときに、テストと CLI で並びをそろえるため。
fn source_with_prelude(text: &str) -> (SourceFiles, Option<FileId>, FileId) {
    let mut files = SourceFiles::new();
    #[cfg(feature = "hir")]
    let prelude = Some(files.add(eml_hir::PRELUDE_PATH, eml_hir::PRELUDE_SOURCE));
    #[cfg(not(feature = "hir"))]
    let prelude = None;
    let file = files.add(ENTRY_PATH, text);
    (files, prelude, file)
}

/// どのテストでも lossless を確かめるため、木が元のテキストに戻ることもここで確認する。構文解析するのは
/// `SourceFiles` に保存したテキスト (先頭の BOM を除いたもの) である (docs/spec/lexical.md)。
pub fn parse(text: &str) -> Parsed {
    let (files, prelude, file) = source_with_prelude(text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
    assert_eq!(
        parse.syntax().text().to_string(),
        files.text(file),
        "tree must be lossless"
    );
    sort_diagnostics(&mut diagnostics);
    Parsed {
        files,
        prelude,
        file,
        parse,
        diagnostics,
    }
}

#[cfg(feature = "hir")]
pub fn lower(text: &str) -> Lowered {
    lower_files(text, &[])
}

/// `modules` は根からの相対パス (`"Report/Csv.em"`) と本文の組である。診断には読み込みの段のものも入る。
#[cfg(feature = "hir")]
pub fn lower_files(entry: &str, modules: &[(&str, &str)]) -> Lowered {
    lower_loaded(eml_hir::load(ENTRY_PATH, entry, &MemorySource(modules)))
}

/// Prelude に定義を足したプログラムを変換する。Prelude の中の item の扱いを確かめるテストのため。
#[cfg(feature = "hir")]
pub fn lower_with_prelude(prelude: &str, entry: &str) -> Lowered {
    lower_loaded(eml_hir::load_with_prelude(
        prelude,
        ENTRY_PATH,
        entry,
        &MemorySource(&[]),
    ))
}

#[cfg(feature = "hir")]
fn lower_loaded((loaded, mut diagnostics): (eml_hir::Loaded, Vec<Diagnostic>)) -> Lowered {
    let (def_map, stage) = eml_hir::def_map(&loaded.modules);
    diagnostics.extend(stage);
    let (program, stage) = eml_hir::lower(&def_map, &loaded.modules);
    diagnostics.extend(stage);
    sort_diagnostics(&mut diagnostics);
    Lowered {
        files: loaded.files,
        file: loaded.entry,
        program,
        diagnostics,
    }
}

#[cfg(feature = "hir")]
pub fn def_map(text: &str) -> (eml_hir::DefMap, Vec<String>) {
    def_map_files(text, &[])
}

/// 読み込みの段の後に `DefMap` を作り、入口の `ItemTree` の診断と `def_map` の診断を返す。構文解析と読み込みの段の
/// 診断は入れない。
#[cfg(feature = "hir")]
pub fn def_map_files(entry: &str, modules: &[(&str, &str)]) -> (eml_hir::DefMap, Vec<String>) {
    let (loaded, _) = eml_hir::load(ENTRY_PATH, entry, &MemorySource(modules));
    // 読み込みの段は構文解析と `ItemTree` の診断を混ぜて返すので、入口の `ItemTree` の診断だけを作り直す
    let (parse, _) = eml_syntax::parse(loaded.entry, loaded.files.text(loaded.entry));
    let (_, mut diagnostics) = eml_hir::item_tree(loaded.entry, &parse.tree());
    let (def_map, stage) = eml_hir::def_map(&loaded.modules);
    diagnostics.extend(stage);
    sort_diagnostics(&mut diagnostics);
    (def_map, short(&loaded.files, &diagnostics))
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
    assert_clean(&lowered.files, &lowered.diagnostics);
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
    let Lowered {
        files,
        file,
        program,
        mut diagnostics,
    } = lower_files(entry, modules);
    let (typed, stage) = eml_types::check(&program);
    diagnostics.extend(stage);
    sort_diagnostics(&mut diagnostics);
    Checked {
        files,
        file,
        program,
        typed,
        diagnostics,
    }
}

#[cfg(feature = "core")]
pub fn core(text: &str) -> Program {
    core_files(text, &[])
}

#[cfg(feature = "core")]
pub fn core_files(entry: &str, modules: &[(&str, &str)]) -> Program {
    let checked = check_without_errors(entry, modules);
    eml_core_ir::lower(&checked.program, &checked.typed, main_function(&checked))
}

/// 確かめたいパスの直後の Core IR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn core_until(text: &str, last: Pass) -> Program {
    core_until_files(text, &[], last)
}

#[cfg(feature = "core")]
pub fn core_until_files(entry: &str, modules: &[(&str, &str)], last: Pass) -> Program {
    let checked = check_without_errors(entry, modules);
    eml_core_ir::lower_until(
        &checked.program,
        &checked.typed,
        main_function(&checked),
        last,
    )
}

/// `eml run` と同じく、入口のモジュールの `main` から実行する。
#[cfg(feature = "core")]
fn main_function(checked: &Checked) -> eml_hir::FunctionId {
    checked
        .program
        .main()
        .expect("a program lowered to Core IR has `main`")
}

/// Core IR は診断のエラーがないプログラムだけを受け取る (docs/implementation/architecture.md)。
#[cfg(feature = "core")]
fn check_without_errors(entry: &str, modules: &[(&str, &str)]) -> Checked {
    let checked = check_files(entry, modules);
    assert!(
        !has_errors(&checked.diagnostics),
        "{:#?}",
        checked.diagnostics
    );
    checked
}

/// 実行のテストでは、つねに `debug_heap` を有効にする (docs/implementation/testing.md)。
#[cfg(feature = "run")]
pub fn run(text: &str) -> (String, Result<(), RuntimeError>) {
    run_files(text, &[])
}

#[cfg(feature = "run")]
pub fn run_files(entry: &str, modules: &[(&str, &str)]) -> (String, Result<(), RuntimeError>) {
    execute(core_files(entry, modules), true)
}

#[cfg(feature = "run")]
pub fn execute(program: Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(debug_heap);
    let result = eml_interp::run(Arc::new(program), &config, &sink);
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
    let mut out = String::new();
    for d in diagnostics {
        let at = position(files, &d.primary);
        writeln!(out, "{} {at} {}", d.code, d.message).unwrap();
        writeln!(out, "  {at} {}", d.primary.message).unwrap();
        for label in &d.secondary {
            writeln!(out, "  {} {}", position(files, label), label.message).unwrap();
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

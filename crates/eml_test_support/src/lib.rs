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
    pub file: FileId,
    pub parse: eml_syntax::Parse,
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "hir")]
pub struct Lowered {
    pub files: SourceFiles,
    pub file: FileId,
    pub module: eml_hir::Module,
    /// 構文と HIR の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "types")]
pub struct Checked {
    pub files: SourceFiles,
    pub file: FileId,
    pub module: eml_hir::Module,
    pub typed: eml_types::TypedModule,
    /// 構文、HIR、型の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

pub fn source(text: &str) -> (SourceFiles, FileId) {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    (files, file)
}

/// どのテストでも lossless を確かめるため、木が元のテキストに戻ることもここで確認する。構文解析するのは
/// `SourceFiles` に保存したテキスト (先頭の BOM を除いたもの) である (docs/spec/lexical.md)。
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

#[cfg(feature = "hir")]
pub fn lower(text: &str) -> Lowered {
    let Parsed {
        files,
        file,
        parse,
        mut diagnostics,
    } = parse(text);
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    sort_diagnostics(&mut diagnostics);
    Lowered {
        files,
        file,
        module,
        diagnostics,
    }
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
    let Lowered {
        files,
        file,
        module,
        mut diagnostics,
    } = lower(text);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    sort_diagnostics(&mut diagnostics);
    Checked {
        files,
        file,
        module,
        typed,
        diagnostics,
    }
}

#[cfg(feature = "core")]
pub fn core(text: &str) -> Program {
    let checked = check_without_errors(text);
    eml_core_ir::lower(&checked.module, &checked.typed)
}

/// 確かめたいパスの直後の Core IR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn core_until(text: &str, last: Pass) -> Program {
    let checked = check_without_errors(text);
    eml_core_ir::lower_until(&checked.module, &checked.typed, last)
}

/// Core IR は診断のエラーがないプログラムだけを受け取る (docs/implementation/architecture.md)。
#[cfg(feature = "core")]
fn check_without_errors(text: &str) -> Checked {
    let checked = check(text);
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
    execute(core(text), true)
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

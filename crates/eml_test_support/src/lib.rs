//! テストのためにパイプラインを組む処理と、診断を文字列にする処理 (docs/implementation/testing.md)。
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
use eml_core_ir::Program;
#[cfg(feature = "core")]
use eml_diagnostics::has_errors;
use eml_diagnostics::{Diagnostic, FileId, Label, LineCol, SourceFiles};
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
    /// 構文と HIR の診断を、各段階が返した順に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "types")]
pub struct Checked {
    pub files: SourceFiles,
    pub file: FileId,
    pub module: eml_hir::Module,
    pub typed: eml_types::TypedModule,
    /// 構文、HIR、型の診断を、各段階が返した順に並べたもの。
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
    let (parse, diagnostics) = eml_syntax::parse(file, files.text(file));
    assert_eq!(
        parse.syntax().text().to_string(),
        files.text(file),
        "tree must be lossless"
    );
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
    Lowered {
        files,
        file,
        module,
        diagnostics,
    }
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
    Checked {
        files,
        file,
        module,
        typed,
        diagnostics,
    }
}

/// Core IR は診断のエラーがないプログラムだけを受け取る (docs/implementation/architecture.md)。
#[cfg(feature = "core")]
pub fn core(text: &str) -> Program {
    let checked = check(text);
    assert!(
        !has_errors(&checked.diagnostics),
        "{:#?}",
        checked.diagnostics
    );
    eml_core_ir::lower(&checked.module, &checked.typed)
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

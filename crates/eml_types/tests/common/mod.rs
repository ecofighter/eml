use std::fmt::Write;

use eml_diagnostics::{SourceFiles, TextRange};

/// 推論結果と、構文・HIR・型の診断。診断はラベル、note、help まで表示する。
pub fn check_text(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, text);
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    let mut out = eml_types::dump(&module, &typed);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for d in &diagnostics {
            let at = position(text, d.primary.range);
            writeln!(out, "{} {at} {}", d.code, d.message).unwrap();
            writeln!(out, "  {at} {}", d.primary.message).unwrap();
            for label in &d.secondary {
                writeln!(out, "  {} {}", position(text, label.range), label.message).unwrap();
            }
            for note in &d.notes {
                writeln!(out, "  note: {note}").unwrap();
            }
            for help in &d.help {
                writeln!(out, "  help: {help}").unwrap();
            }
        }
    }
    out
}

fn position(text: &str, range: TextRange) -> String {
    let offset = u32::from(range.start()) as usize;
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("{line}:{column}")
}

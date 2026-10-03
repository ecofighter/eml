use eml_diagnostics::{Diagnostic, SourceFiles};

/// HIR の表示と、構文と HIR の診断を位置の順に並べたもの。
pub fn lower_text(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, text);
    let (module, hir_diagnostics) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(hir_diagnostics);
    diagnostics.sort_by_key(|d| d.primary.range.start());
    let mut out = eml_hir::pretty(&module);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for diagnostic in &diagnostics {
            out.push_str(&format_diagnostic(text, diagnostic));
            out.push('\n');
        }
    }
    out
}

fn format_diagnostic(text: &str, diagnostic: &Diagnostic) -> String {
    let offset = u32::from(diagnostic.primary.range.start()) as usize;
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("{} {line}:{column} {}", diagnostic.code, diagnostic.message)
}

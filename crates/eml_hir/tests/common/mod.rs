use eml_test_support::{lower, short};

/// HIR の表示と、構文と HIR の診断を位置の順に並べたもの。
pub fn lower_text(text: &str) -> String {
    let mut lowered = lower(text);
    lowered.diagnostics.sort_by_key(|d| d.primary.range.start());
    let mut out = eml_hir::pretty(&lowered.module);
    let diagnostics = short(&lowered.files, &lowered.diagnostics);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for line in diagnostics {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

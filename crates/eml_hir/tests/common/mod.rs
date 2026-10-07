use eml_test_support::{lower, short, short_text, with_diagnostics};

/// HIR の表示と、構文と HIR の診断を表示と同じ順に並べたもの。
pub fn lower_text(text: &str) -> String {
    let lowered = lower(text);
    with_diagnostics(
        eml_hir::pretty(&lowered.program),
        &short_text(&lowered.files, &lowered.diagnostics),
    )
}

/// 構文と HIR の診断を、表示と同じ順に並べたもの。
pub fn diagnostics(text: &str) -> Vec<String> {
    let lowered = lower(text);
    short(&lowered.files, &lowered.diagnostics)
}

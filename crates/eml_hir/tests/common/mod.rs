#![allow(dead_code)]

use eml_test_support::{Lowered, lower, short, short_text, with_diagnostics};

/// HIR の表示と、構文と HIR の診断を位置の順に並べたもの。
pub fn lower_text(text: &str) -> String {
    let lowered = lower_sorted(text);
    with_diagnostics(
        eml_hir::pretty(&lowered.module),
        &short_text(&lowered.files, &lowered.diagnostics),
    )
}

/// 構文と HIR の診断を、位置の順に並べたもの。
pub fn diagnostics(text: &str) -> Vec<String> {
    let lowered = lower_sorted(text);
    short(&lowered.files, &lowered.diagnostics)
}

/// 段階が返した順ではなく位置の順にして、期待値をソースと見比べやすくする。
fn lower_sorted(text: &str) -> Lowered {
    let mut lowered = lower(text);
    lowered.diagnostics.sort_by_key(|d| d.primary.range.start());
    lowered
}

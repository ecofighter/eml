use eml_test_support::{check, full};

/// 推論結果と、構文・HIR・型の診断。診断はラベル、note、help まで表示する。
pub fn check_text(text: &str) -> String {
    let checked = check(text);
    let mut out = eml_types::dump(&checked.module, &checked.typed);
    if !checked.diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&full(&checked.files, &checked.diagnostics));
    }
    out
}

use eml_test_support::{check, full, with_diagnostics};

/// 推論結果と、構文・HIR・型の診断。診断はラベル、note、help まで表示する。
pub fn check_text(text: &str) -> String {
    let checked = check(text);
    with_diagnostics(
        eml_types::dump(&checked.program, &checked.typed),
        &full(checked.files(), &checked.diagnostics),
    )
}

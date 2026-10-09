use eml_test_support::{check, full, with_diagnostics};

/// 推論結果と、構文・HIR・型の診断。診断はラベル、note、help まで表示する。
pub fn check_text(text: &str) -> String {
    let checked = check(text);
    with_diagnostics(
        eml_types::dump(&checked.program, &checked.typed),
        &full(checked.files(), &checked.diagnostics),
    )
}

/// 関数のスキームに残った Kind の制約の行。なければ空にする。
pub fn kinds(text: &str, function: &str) -> String {
    let checked = check(text);
    let dump = eml_types::dump(&checked.program, &checked.typed);
    let head = format!("{function} : ");
    let mut lines = dump.lines().skip_while(|line| !line.starts_with(&head));
    lines.next();
    lines
        .next()
        .filter(|line| line.starts_with("  kinds: "))
        .unwrap_or("")
        .to_string()
}

use eml_test_support::{lower, lower_files, short, short_text, with_diagnostics};

/// HIR の表示と、構文と HIR の診断を表示と同じ順に並べたもの。
pub fn lower_text(text: &str) -> String {
    lower_files_text(text, &[])
}

/// 入口と、根からの相対パスで置いたモジュールを変換する。表示は `lower_text` と同じ形である。
pub fn lower_files_text(entry: &str, modules: &[(&str, &str)]) -> String {
    let lowered = lower_files(entry, modules);
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

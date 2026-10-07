use eml_test_support::{
    full_with_paths, located, lower, lower_files, short, short_text, with_diagnostics,
};

/// HIR の表示と、構文と HIR の診断を表示と同じ順に並べたもの。
pub fn lower_text(text: &str) -> String {
    lower_files_text(text, &[])
}

/// 入口と、根からの相対パスで置いたモジュールを変換する。表示は `lower_text` と同じ形である。
pub fn lower_files_text(entry: &str, modules: &[(&str, &str)]) -> String {
    let lowered = lower_files(entry, modules);
    with_diagnostics(
        eml_hir::pretty(&lowered.program),
        &short_text(lowered.files(), &lowered.diagnostics),
    )
}

/// 構文と HIR の診断を、表示と同じ順に並べたもの。
pub fn diagnostics(text: &str) -> Vec<String> {
    let lowered = lower(text);
    short(lowered.files(), &lowered.diagnostics)
}

/// 複数のモジュールのプログラムの診断を、ラベル、note、help まで、位置に表示のパスを付けて表示する。
pub fn module_report(entry: &str, modules: &[(&str, &str)]) -> String {
    let lowered = lower_files(entry, modules);
    full_with_paths(lowered.files(), &lowered.diagnostics)
}

/// 診断の番号と位置だけを並べる。文言を別のテストで確かめる診断 (構文の誤りと読み込みの段の誤り) を含むテストに使う。
pub fn module_codes(entry: &str, modules: &[(&str, &str)]) -> Vec<String> {
    let lowered = lower_files(entry, modules);
    lowered
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code, located(lowered.files(), &d.primary)))
        .collect()
}

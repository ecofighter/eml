use eml_core_ir::Pass;

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にして表示する。
pub fn core_text(text: &str, last: Pass) -> String {
    eml_core_ir::pretty(&eml_test_support::core_until(text, last))
}

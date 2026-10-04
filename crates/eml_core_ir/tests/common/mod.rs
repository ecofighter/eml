/// 誤りのないプログラムを Core IR にして表示する。
pub fn core_text(text: &str) -> String {
    eml_core_ir::pretty(&eml_test_support::core(text))
}

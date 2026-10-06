use eml_core_ir::Pass;

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にして表示する。
pub fn core_text(text: &str, last: Pass) -> String {
    let shown = eml_core_ir::pretty(&eml_test_support::core_until(text, last));
    let parsed = eml_core_ir::parse(&shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(
        eml_core_ir::pretty(&parsed),
        shown,
        "the printed Core IR must read back"
    );
    shown
}

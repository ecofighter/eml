use eml_core_ir::Pass;

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にして表示する。読み直したプログラムにも verifier を
/// かけるのは、`pretty` と `parse` が同じ誤りをして、違うプログラムで表示だけがそろうことを見つけるためである。
pub fn core_text(text: &str, last: Pass) -> String {
    core_text_files(text, &[], last)
}

/// `modules` は根からの相対パスと本文の組である (`eml_test_support::core_until_files`)。
pub fn core_text_files(entry: &str, modules: &[(&str, &str)], last: Pass) -> String {
    let shown = eml_core_ir::pretty(&eml_test_support::core_until_files(entry, modules, last));
    let parsed = eml_core_ir::parse(&shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(
        eml_core_ir::pretty(&parsed),
        shown,
        "the printed Core IR must read back"
    );
    let verified = match last {
        Pass::Translate | Pass::Simplify => eml_core_ir::verify_scopes(&parsed),
        Pass::Perceus => eml_core_ir::verify(&parsed),
    };
    if let Err(error) = verified {
        panic!("the Core IR read back must verify: {error}\n{shown}");
    }
    shown
}

use eml_core_ir::{
    Pass, Program, parse, pretty, pretty_with_positions, verify, verify_scopes, verify_translated,
};

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にして表示する。
pub fn core_text(text: &str, last: Pass) -> String {
    core_text_files(text, &[], last)
}

/// `modules` は根からの相対パスと本文の組である (`eml_test_support::core_until_files`)。
pub fn core_text_files(entry: &str, modules: &[(&str, &str)], last: Pass) -> String {
    let shown = pretty(&eml_test_support::core_until_files(entry, modules, last));
    read_back(&shown, last, pretty);
    shown
}

/// extern の呼び出しの位置も表示する。
pub fn core_text_with_positions(text: &str) -> String {
    let shown = pretty_with_positions(&eml_test_support::core_until(text, Pass::Translate));
    read_back(&shown, Pass::Translate, pretty_with_positions);
    shown
}

/// 表示した IR を読み直し、同じ表示に戻ることと、読み直した IR が同じ段の verifier を通ることを確かめる。`pretty` と
/// `parse` が同じ誤りをして、違うプログラムで表示だけがそろうことを見つけるためである。
fn read_back(shown: &str, last: Pass, show: fn(&Program) -> String) {
    let parsed = parse(shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(show(&parsed), shown, "the printed Core IR must read back");
    let verified = match last {
        Pass::Translate => verify_translated(&parsed),
        Pass::Boxing | Pass::Contract => verify_scopes(&parsed),
        Pass::Perceus => verify(&parsed),
    };
    if let Err(error) = verified {
        panic!("the Core IR read back must verify: {error}\n{shown}");
    }
}

/// 名前で選んだ関数の表示。ほかの関数 (`entry$main` など) を期待値から外すため。内部の関数は `internal fn` の行から
/// 始まる。
pub fn function(shown: &str, name: &str) -> String {
    let header = shown
        .find(&format!("fn {name}("))
        .or_else(|| shown.find(&format!("fn {name:?}(")))
        .unwrap_or_else(|| panic!("no `{name}` in\n{shown}"));
    let start = shown[..header].rfind('\n').map_or(0, |newline| newline + 1);
    let end = shown[start..]
        .find("\n}\n")
        .expect("a function ends with `}`")
        + start
        + 3;
    shown[start..end].to_string()
}

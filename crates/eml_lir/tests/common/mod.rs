use eml_lir::{Pass, parse, passes_until, pretty, verify, verify_flattened, verify_scopes};

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

/// 手で書いた LIR を box の挿入から `last` まで通した表示。構造体はまだソースから作られないので、開くパスの後の形は
/// この道で作る。表示を読み直すと同じ表示に戻り、読み直したものが `last` の後の段の verifier を通ることも確かめる。
pub fn passes_text(text: &str, last: Pass) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}\n{text}"));
    passes_until(&mut program, last);
    let shown = pretty(&program);
    let parsed = parse(&shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(pretty(&parsed), shown, "the printed LIR must read back");
    let verified = match last {
        Pass::Boxing => verify_scopes(&parsed),
        Pass::Flatten | Pass::Contract => verify_flattened(&parsed),
        Pass::Perceus | Pass::Reuse => verify(&parsed),
    };
    if let Err(error) = verified {
        panic!("the LIR read back must verify: {error}\n{shown}");
    }
    shown
}

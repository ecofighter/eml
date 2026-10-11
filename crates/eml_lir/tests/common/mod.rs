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

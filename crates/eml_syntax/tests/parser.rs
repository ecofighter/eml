use eml_diagnostics::{SourceFiles, render};
use eml_syntax::{debug_tree, parse};

/// どのテストでも lossless を確かめるため、木が元のテキストに戻ることもここで確認する。
fn dump(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    assert_eq!(
        parse.syntax().text().to_string(),
        text,
        "tree must be lossless"
    );
    let mut out = debug_tree(&parse.syntax());
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&render(&diagnostics, &files));
    }
    out
}

#[test]
fn empty_file() {
    insta::assert_snapshot!(dump(""), @"SOURCE_FILE@0..0");
}

#[test]
fn trivia_only_file_has_no_errors() {
    insta::assert_snapshot!(dump("-- only a comment\n\n"), @r#"
    SOURCE_FILE@0..19
      COMMENT@0..17 "-- only a comment"
      WHITESPACE@17..19 "\n\n"
    "#);
}

#[test]
fn stray_tokens_are_one_error_until_the_next_item() {
    insta::assert_snapshot!(dump("1 2 3"), @r#"
    SOURCE_FILE@0..5
      ERROR@0..5
        INT@0..1 "1"
        WHITESPACE@1..2 " "
        INT@2..3 "2"
        WHITESPACE@3..4 " "
        INT@4..5 "3"
    ---
    [E0003] Error: expected an item
       ╭─[ test.em:1:1 ]
       │
     1 │ 1 2 3
       │ ┬  
       │ ╰── not the start of an item
    ───╯
    "#);
}

#[test]
fn lexer_errors_are_not_reported_twice() {
    let text = "€ x";
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (_, diagnostics) = parse(file, text);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    // `€` は字句解析の E0001 だけ。続く `x` は項目ではないので E0003 を1件出す。
    assert_eq!(codes, ["E0001", "E0003"]);
    assert_eq!(u32::from(diagnostics[1].primary.range.start()), 4);
}

#[test]
fn recovery_resumes_at_the_next_item() {
    // 1つの項目の中のエラーは、次の行の項目の解析に影響しない。
    let text = "a : Int -> )\nb : Int\nc : Int";
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    assert_eq!(codes, ["E0011"]);
    assert_eq!(u32::from(diagnostics[0].primary.range.start()), 11);
    let kinds: Vec<String> = parse
        .syntax()
        .children()
        .map(|node| format!("{:?}", node.kind()))
        .collect();
    assert_eq!(kinds, ["SIGNATURE", "ERROR", "SIGNATURE", "SIGNATURE"]);
}

/// パースが panic せずに終わり、木が元のテキストに戻ることだけを確かめる。
fn assert_parses_losslessly(text: &str) {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, _) = parse(file, text);
    assert_eq!(parse.syntax().text(), text, "tree must be lossless");
}

#[test]
fn long_unclosed_paren_lookahead_does_not_hit_the_step_limit() {
    // 先読み走査 (`apat_len`) は、閉じ括弧を探して EOF まで読む。
    assert_parses_losslessly(&("(".to_string() + &"a ".repeat(600_000)));
}

#[test]
fn long_use_lookahead_does_not_hit_the_step_limit() {
    // 先読み走査 (`has_left_arrow`) は、`<-` を探して EOF まで読む。
    assert_parses_losslessly(&("f =\n  use g (".to_string() + &"a ".repeat(600_000)));
}

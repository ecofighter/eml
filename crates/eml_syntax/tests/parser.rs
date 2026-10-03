use eml_diagnostics::{SourceFiles, render};
use eml_syntax::{debug_tree, parse};

/// パースした木と診断を表示する。木は必ず元のテキストに戻ることも確認する。
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
    insta::assert_snapshot!(dump("// only a comment\n\n"), @r#"
    SOURCE_FILE@0..19
      COMMENT@0..17 "// only a comment"
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
    [E0003] Error: expected an item (`fn`, `type`, or `effect`)
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
    let text = "$ x";
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (_, diagnostics) = parse(file, text);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    // `$` は字句解析の E0001 だけ。続く `x` は項目ではないので E0003 を1件出す。
    assert_eq!(codes, ["E0001", "E0003"]);
    assert_eq!(u32::from(diagnostics[1].primary.range.start()), 2);
}

#[test]
fn recovery_resumes_at_item_keywords() {
    // 項目の文法は後の段階で実装する。今は項目ごとに「まだ対応していない」を1件ずつ出し、
    // 項目の間のエラーとは独立に報告できることを確認する。
    let text = "fn main () : Unit { } ? type T { } effect E { }";
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    assert_eq!(codes, ["E0004", "E0001", "E0004", "E0004"]);
    let kinds: Vec<String> = parse
        .syntax()
        .children()
        .map(|node| format!("{:?}", node.kind()))
        .collect();
    assert_eq!(kinds, ["ERROR", "ERROR", "ERROR"]);
}

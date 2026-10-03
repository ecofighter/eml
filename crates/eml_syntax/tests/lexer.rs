use eml_diagnostics::{SourceFiles, render};
use eml_syntax::lex;

/// トークン列を1行1トークンで表示する。
fn dump(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (tokens, diagnostics) = lex(file, text);
    let mut out = String::new();
    for token in &tokens {
        out.push_str(&format!(
            "{:?}@{:?} {:?}\n",
            token.kind, token.range, &text[token.range]
        ));
    }
    let joined: String = tokens.iter().map(|token| &text[token.range]).collect();
    assert_eq!(joined, text, "tokens must cover the whole text");
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&render(&diagnostics, &files));
    }
    out
}

#[test]
fn keywords_and_identifiers() {
    insta::assert_snapshot!(dump("fn fnord let _ _x x1 Some IO"), @r#"
    FN_KW@0..2 "fn"
    WHITESPACE@2..3 " "
    LIDENT@3..8 "fnord"
    WHITESPACE@8..9 " "
    LET_KW@9..12 "let"
    WHITESPACE@12..13 " "
    UNDERSCORE@13..14 "_"
    WHITESPACE@14..15 " "
    LIDENT@15..17 "_x"
    WHITESPACE@17..18 " "
    LIDENT@18..20 "x1"
    WHITESPACE@20..21 " "
    UIDENT@21..25 "Some"
    WHITESPACE@25..26 " "
    UIDENT@26..28 "IO"
    "#);
}

#[test]
fn all_keywords() {
    let text = "fn let if then else match type effect handle resume drop return never once multi true false";
    let kinds: Vec<String> = dump(text)
        .lines()
        .filter(|line| !line.starts_with("WHITESPACE"))
        .map(|line| line.split('@').next().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        [
            "FN_KW",
            "LET_KW",
            "IF_KW",
            "THEN_KW",
            "ELSE_KW",
            "MATCH_KW",
            "TYPE_KW",
            "EFFECT_KW",
            "HANDLE_KW",
            "RESUME_KW",
            "DROP_KW",
            "RETURN_KW",
            "NEVER_KW",
            "ONCE_KW",
            "MULTI_KW",
            "TRUE_KW",
            "FALSE_KW",
        ]
    );
}

#[test]
fn operators_use_longest_match() {
    let text = "( ) { } , ; : = -> => | < > + - * / % ++ == != <= >= && || ! <<= --";
    let kinds: Vec<String> = dump(text)
        .lines()
        .filter(|line| !line.starts_with("WHITESPACE"))
        .map(|line| line.split('@').next().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        [
            "L_PAREN",
            "R_PAREN",
            "L_BRACE",
            "R_BRACE",
            "COMMA",
            "SEMICOLON",
            "COLON",
            "EQ",
            "THIN_ARROW",
            "FAT_ARROW",
            "PIPE",
            "LT",
            "GT",
            "PLUS",
            "MINUS",
            "STAR",
            "SLASH",
            "PERCENT",
            "PLUS2",
            "EQ2",
            "NEQ",
            "LTEQ",
            "GTEQ",
            "AMP2",
            "PIPE2",
            "BANG",
            "LT",
            "LTEQ",
            "MINUS",
            "MINUS",
        ]
    );
}

#[test]
fn literals_and_comments() {
    insta::assert_snapshot!(dump("42 \"a\\n\\\"b\" // note\n-1"), @r#"
    INT@0..2 "42"
    WHITESPACE@2..3 " "
    STRING@3..11 "\"a\\n\\\"b\""
    WHITESPACE@11..12 " "
    COMMENT@12..19 "// note"
    WHITESPACE@19..20 "\n"
    MINUS@20..21 "-"
    INT@21..22 "1"
    "#);
}

#[test]
fn unexpected_characters_are_merged_into_one_error() {
    insta::assert_snapshot!(dump("a $@ b é"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..2 " "
    ERROR_TOKEN@2..4 "$@"
    WHITESPACE@4..5 " "
    LIDENT@5..6 "b"
    WHITESPACE@6..7 " "
    ERROR_TOKEN@7..9 "é"
    ---
    [E0001] Error: unexpected character `$@`
       ╭─[ test.em:1:3 ]
       │
     1 │ a $@ b é
       │   ─┬  
       │    ╰── not valid in eml source
    ───╯
    [E0001] Error: unexpected character `é`
       ╭─[ test.em:1:8 ]
       │
     1 │ a $@ b é
       │        ┬  
       │        ╰── not valid in eml source
    ───╯
    "#);
}

#[test]
fn unterminated_string_becomes_string_with_error() {
    insta::assert_snapshot!(dump("\"abc\nx"), @r#"
    STRING@0..4 "\"abc"
    WHITESPACE@4..5 "\n"
    LIDENT@5..6 "x"
    ---
    [E0002] Error: unterminated string literal
       ╭─[ test.em:1:1 ]
       │
     1 │ "abc
       │ ──┬─  
       │   ╰─── missing closing `"`
    ───╯
    "#);
}

#[test]
fn crlf_line_endings_are_whitespace() {
    insta::assert_snapshot!(dump("a\r\nb // c\r\n"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..3 "\r\n"
    LIDENT@3..4 "b"
    WHITESPACE@4..5 " "
    COMMENT@5..9 "// c"
    WHITESPACE@9..11 "\r\n"
    "#);
}

#[test]
fn byte_order_mark_is_whitespace() {
    insta::assert_snapshot!(dump("\u{feff}fn"), @r#"
    WHITESPACE@0..3 "\u{feff}"
    FN_KW@3..5 "fn"
    "#);
}

#[test]
fn comment_at_end_of_file_without_newline() {
    insta::assert_snapshot!(dump("x // end"), @r#"
    LIDENT@0..1 "x"
    WHITESPACE@1..2 " "
    COMMENT@2..8 "// end"
    "#);
}

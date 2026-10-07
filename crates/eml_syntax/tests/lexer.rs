use eml_diagnostics::render;
use eml_syntax::lex;
use eml_test_support::{source, with_diagnostics};

fn dump(text: &str) -> String {
    let (files, file) = source(text);
    let (tokens, mut diagnostics) = lex(file, text);
    eml_diagnostics::sort_diagnostics(&mut diagnostics);
    let mut out = String::new();
    for token in &tokens {
        out.push_str(&format!(
            "{:?}@{:?} {:?}\n",
            token.kind, token.range, &text[token.range]
        ));
    }
    let joined: String = tokens.iter().map(|token| &text[token.range]).collect();
    assert_eq!(joined, text, "tokens must cover the whole text");
    with_diagnostics(out, &render(&diagnostics, &files))
}

fn kinds(text: &str) -> Vec<String> {
    let (_, file) = source(text);
    let (tokens, _) = lex(file, text);
    let joined: String = tokens.iter().map(|token| &text[token.range]).collect();
    assert_eq!(joined, text, "tokens must cover the whole text");
    tokens
        .iter()
        .filter(|token| !token.kind.is_trivia())
        .map(|token| format!("{:?}", token.kind))
        .collect()
}

fn diags(text: &str) -> Vec<String> {
    let (_, file) = source(text);
    let (_, mut diagnostics) = lex(file, text);
    eml_diagnostics::sort_diagnostics(&mut diagnostics);
    diagnostics
        .iter()
        .map(|d| format!("{}@{:?} {}", d.code, d.primary.range, d.message))
        .collect()
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
fn resume_is_an_ordinary_name() {
    assert_eq!(kinds("resume"), ["LIDENT"]);
}

#[test]
fn all_keywords() {
    let text = "data type effect where pub extern import as infixl infixr infix \
                let in if then else match with handle from drop return \
                never once multi use fn forall class instance";
    assert_eq!(
        kinds(text),
        [
            "DATA_KW",
            "TYPE_KW",
            "EFFECT_KW",
            "WHERE_KW",
            "PUB_KW",
            "EXTERN_KW",
            "IMPORT_KW",
            "AS_KW",
            "INFIXL_KW",
            "INFIXR_KW",
            "INFIX_KW",
            "LET_KW",
            "IN_KW",
            "IF_KW",
            "THEN_KW",
            "ELSE_KW",
            "MATCH_KW",
            "WITH_KW",
            "HANDLE_KW",
            "FROM_KW",
            "DROP_KW",
            "RETURN_KW",
            "NEVER_KW",
            "ONCE_KW",
            "MULTI_KW",
            "USE_KW",
            "FN_KW",
            "FORALL_KW",
            "CLASS_KW",
            "INSTANCE_KW",
        ]
    );
}

#[test]
fn true_and_false_are_identifiers() {
    assert_eq!(kinds("true false True"), ["LIDENT", "LIDENT", "UIDENT"]);
}

#[test]
fn identifiers_may_contain_primes() {
    assert_eq!(kinds("x' go'' A'b"), ["LIDENT", "LIDENT", "UIDENT"]);
}

#[test]
fn operators_and_reserved_symbols() {
    let text = "( ) [ ] { } , ; = | : . -> <- .. - :: |> <> == => !$@ -->";
    assert_eq!(
        kinds(text),
        [
            "L_PAREN",
            "R_PAREN",
            "L_BRACK",
            "R_BRACK",
            "L_BRACE",
            "R_BRACE",
            "COMMA",
            "SEMICOLON",
            "EQ",
            "PIPE",
            "COLON",
            "DOT",
            "THIN_ARROW",
            "LEFT_ARROW",
            "DOT2",
            "MINUS",
            "CONOP",
            "OP",
            "OP",
            "OP",
            "OP",
            "OP",
            "OP",
        ]
    );
}

#[test]
fn literals_and_comments() {
    insta::assert_snapshot!(dump("42 \"a\\n\\\"b\" -- note\n-1"), @r#"
    INT@0..2 "42"
    WHITESPACE@2..3 " "
    STRING@3..11 "\"a\\n\\\"b\""
    WHITESPACE@11..12 " "
    COMMENT@12..19 "-- note"
    WHITESPACE@19..20 "\n"
    MINUS@20..21 "-"
    INT@21..22 "1"
    "#);
}

#[test]
fn dashes_start_a_comment_unless_an_operator_character_follows() {
    insta::assert_snapshot!(dump("x -- note\n---- banner\n-- | doc\ny"), @r#"
    LIDENT@0..1 "x"
    WHITESPACE@1..2 " "
    COMMENT@2..9 "-- note"
    WHITESPACE@9..10 "\n"
    COMMENT@10..21 "---- banner"
    WHITESPACE@21..22 "\n"
    COMMENT@22..30 "-- | doc"
    WHITESPACE@30..31 "\n"
    LIDENT@31..32 "y"
    "#);
    assert_eq!(
        kinds("a --> b --| c"),
        ["LIDENT", "OP", "LIDENT", "OP", "LIDENT"]
    );
}

#[test]
fn block_comments_nest() {
    insta::assert_snapshot!(dump("a {- x {- y -} z -} b"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..2 " "
    BLOCK_COMMENT@2..19 "{- x {- y -} z -}"
    WHITESPACE@19..20 " "
    LIDENT@20..21 "b"
    "#);
}

#[test]
fn unterminated_block_comment_is_an_error() {
    assert_eq!(
        diags("a {- x {- y -}"),
        ["E0005@2..4 unterminated block comment"]
    );
    assert_eq!(kinds("a {- x {- y -}"), ["LIDENT"]);
}

#[test]
fn shebang_is_trivia_only_at_the_start_of_the_file() {
    insta::assert_snapshot!(dump("#!/usr/bin/env eml run\nx"), @r##"
    SHEBANG@0..22 "#!/usr/bin/env eml run"
    WHITESPACE@22..23 "\n"
    LIDENT@23..24 "x"
    "##);
    assert_eq!(kinds("x\n#!y"), ["LIDENT", "ERROR_TOKEN", "OP", "LIDENT"]);
}

#[test]
fn number_forms() {
    assert_eq!(
        kinds("123 1_000 0xff 0o17 0b1010 1.5 1e9 2.5e-3"),
        ["INT", "INT", "INT", "INT", "INT", "FLOAT", "FLOAT", "FLOAT"]
    );
}

#[test]
fn malformed_numbers_are_errors() {
    assert_eq!(
        diags("0xZZ 12ab 0x"),
        [
            "E0007@0..4 invalid number literal `0xZZ`",
            "E0007@5..9 invalid number literal `12ab`",
            "E0007@10..12 invalid number literal `0x`",
        ]
    );
    assert_eq!(kinds("0xZZ 12ab 0x"), ["INT", "INT", "INT"]);
}

#[test]
fn character_literals() {
    assert_eq!(kinds(r"'a' '\n' x'"), ["CHAR", "CHAR", "LIDENT"]);
}

#[test]
fn float_right_after_a_dot_is_split_into_field_indices() {
    insta::assert_snapshot!(dump("t.0.1"), @r#"
    LIDENT@0..1 "t"
    DOT@1..2 "."
    INT@2..3 "0"
    DOT@3..4 "."
    INT@4..5 "1"
    "#);
    assert_eq!(kinds("x . 0.1"), ["LIDENT", "DOT", "FLOAT"]);
    assert_eq!(kinds("1.5"), ["FLOAT"]);
}

#[test]
fn valid_escapes_have_no_errors() {
    assert!(diags(r#""\n\t\r\\\"\0\u{1F600}""#).is_empty());
}

#[test]
fn invalid_escapes_are_errors() {
    assert_eq!(
        diags(r#""a\qb""#),
        ["E0008@2..4 unknown escape sequence `\\q`"]
    );
    assert_eq!(
        diags(r#""\u{110000}""#),
        ["E0008@1..11 invalid unicode escape `\\u{110000}`"]
    );
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
fn backslash_at_end_of_line_gives_only_the_unterminated_error() {
    assert_eq!(
        diags("\"abc\\\nx"),
        ["E0002@0..5 unterminated string literal"]
    );
    assert_eq!(kinds("\"abc\\\nx"), ["STRING", "LIDENT"]);
}

#[test]
fn unterminated_string_does_not_swallow_carriage_return() {
    assert_eq!(
        diags("\"abc\r\nx"),
        ["E0002@0..4 unterminated string literal"]
    );
    assert_eq!(kinds("\"abc\r\nx"), ["STRING", "LIDENT"]);
}

#[test]
fn interpolation_is_skipped_with_not_yet_supported() {
    let text = r#""a\{f "x"} b" c"#;
    assert_eq!(
        diags(text),
        ["E0004@2..10 string interpolation is not supported yet"]
    );
    assert_eq!(kinds(text), ["STRING", "LIDENT"]);
}

#[test]
fn later_stage_literals_are_single_tokens() {
    insta::assert_snapshot!(dump("\"\"\"\n  a\n  \"\"\" r\"x\" r#\"y\"z\"# `ls -l`"), @r##"
    MULTILINE_STRING@0..13 "\"\"\"\n  a\n  \"\"\""
    WHITESPACE@13..14 " "
    RAW_STRING@14..18 "r\"x\""
    WHITESPACE@18..19 " "
    RAW_STRING@19..27 "r#\"y\"z\"#"
    WHITESPACE@27..28 " "
    COMMAND@28..35 "`ls -l`"
    "##);
}

#[test]
fn unterminated_later_stage_literals_are_errors() {
    assert_eq!(diags("`ls"), ["E0002@0..3 unterminated command literal"]);
    assert_eq!(diags("r#\"abc"), ["E0002@0..3 unterminated raw string"]);
    assert_eq!(
        diags("\"\"\"abc"),
        ["E0002@0..3 unterminated multi-line string"]
    );
}

#[test]
fn unexpected_characters_are_merged_into_one_error() {
    insta::assert_snapshot!(dump("a €€ b é"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..2 " "
    ERROR_TOKEN@2..8 "€€"
    WHITESPACE@8..9 " "
    LIDENT@9..10 "b"
    WHITESPACE@10..11 " "
    ERROR_TOKEN@11..13 "é"
    ---
    [E0001] Error: unexpected character `€€`
       ╭─[ test.em:1:3 ]
       │
     1 │ a €€ b é
       │   ─┬  
       │    ╰── not valid in eml source
    ───╯
    [E0001] Error: unexpected character `é`
       ╭─[ test.em:1:8 ]
       │
     1 │ a €€ b é
       │        ┬  
       │        ╰── not valid in eml source
    ───╯
    "#);
}

#[test]
fn unexpected_character_messages_escape_and_truncate() {
    assert_eq!(
        diags("a\u{1}b"),
        ["E0001@1..2 unexpected character `\\u{1}`"]
    );
    assert_eq!(
        diags(&"€".repeat(20)),
        ["E0001@0..60 unexpected character `€€€€€€€€€€€€€€€€…`"]
    );
}

#[test]
fn backslash_outside_strings_is_unexpected() {
    assert_eq!(diags("a \\ b"), ["E0001@2..3 unexpected character `\\`"]);
}

#[test]
fn crlf_line_endings_are_whitespace() {
    insta::assert_snapshot!(dump("a\r\nb -- c\r\n"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..3 "\r\n"
    LIDENT@3..4 "b"
    WHITESPACE@4..5 " "
    COMMENT@5..9 "-- c"
    WHITESPACE@9..11 "\r\n"
    "#);
}

#[test]
fn byte_order_mark_in_the_middle_is_an_unexpected_character() {
    // 先頭の BOM は読み込み時に除く。lexer に届いた U+FEFF は、どこにあっても認識できない文字である
    // (docs/spec/lexical.md)。
    // 見えない文字なので、メッセージではエスケープして名指しする。
    assert_eq!(
        diags("fn\u{feff}"),
        ["E0001@2..5 unexpected character `\\u{feff}`"]
    );
}

#[test]
fn comment_at_end_of_file_without_newline() {
    insta::assert_snapshot!(dump("x -- end"), @r#"
    LIDENT@0..1 "x"
    WHITESPACE@1..2 " "
    COMMENT@2..8 "-- end"
    "#);
}

#[test]
fn unicode_escape_does_not_run_past_the_string() {
    let text = r#""\u{zz" f {x}"#;
    assert_eq!(diags(text), ["E0008@1..3 invalid unicode escape `\\u`"]);
    assert_eq!(
        kinds(text),
        ["STRING", "LIDENT", "L_BRACE", "LIDENT", "R_BRACE"]
    );
}

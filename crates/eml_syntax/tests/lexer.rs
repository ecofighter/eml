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
                never once multi use fn forall class instance deriving";
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
            "DERIVING_KW",
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
            "FAT_ARROW",
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
    STRING_START@3..4 "\""
    STRING_TEXT@4..5 "a"
    ESCAPE@5..7 "\\n"
    ESCAPE@7..9 "\\\""
    STRING_TEXT@9..10 "b"
    STRING_END@10..11 "\""
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
    STRING_START@0..1 "\""
    STRING_TEXT@1..4 "abc"
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
    assert_eq!(
        kinds("\"abc\\\nx"),
        ["STRING_START", "STRING_TEXT", "ESCAPE", "LIDENT"]
    );
}

#[test]
fn unterminated_string_does_not_swallow_carriage_return() {
    assert_eq!(
        diags("\"abc\r\nx"),
        ["E0002@0..4 unterminated string literal"]
    );
    assert_eq!(
        kinds("\"abc\r\nx"),
        ["STRING_START", "STRING_TEXT", "LIDENT"]
    );
}

#[test]
fn raw_strings_are_single_tokens() {
    insta::assert_snapshot!(dump("r\"x\" r#\"y\"z\"#"), @r##"
    RAW_STRING@0..4 "r\"x\""
    WHITESPACE@4..5 " "
    RAW_STRING@5..13 "r#\"y\"z\"#"
    "##);
}

#[test]
fn unterminated_raw_and_command_literals_are_errors() {
    assert_eq!(diags("`ls"), ["E0002@0..3 unterminated command literal"]);
    assert_eq!(diags("r#\"abc"), ["E0002@0..3 unterminated raw string"]);
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
        [
            "STRING_START",
            "ESCAPE",
            "STRING_TEXT",
            "STRING_END",
            "LIDENT",
            "L_BRACE",
            "LIDENT",
            "R_BRACE"
        ]
    );
}

#[test]
fn a_string_is_split_into_tokens() {
    assert_eq!(
        kinds(r#""a\{x}b""#),
        [
            "STRING_START",
            "STRING_TEXT",
            "INTERP_START",
            "LIDENT",
            "INTERP_END",
            "STRING_TEXT",
            "STRING_END"
        ]
    );
    assert_eq!(kinds(r#""""#), ["STRING_START", "STRING_END"]);
    assert_eq!(
        kinds(r#""\n\u{41}\q""#),
        ["STRING_START", "ESCAPE", "ESCAPE", "ESCAPE", "STRING_END"]
    );
    assert_eq!(
        diags(r#""\n\u{41}\q""#),
        ["E0008@9..11 unknown escape sequence `\\q`"]
    );
}

#[test]
fn holes_nest_strings_and_count_braces() {
    assert_eq!(
        kinds(r#""a \{f "b \{x}"} c""#),
        [
            "STRING_START",
            "STRING_TEXT",
            "INTERP_START",
            "LIDENT",
            "STRING_START",
            "STRING_TEXT",
            "INTERP_START",
            "LIDENT",
            "INTERP_END",
            "STRING_END",
            "INTERP_END",
            "STRING_TEXT",
            "STRING_END",
        ]
    );
    assert_eq!(
        kinds(r#""\{ {a} }""#),
        [
            "STRING_START",
            "INTERP_START",
            "L_BRACE",
            "LIDENT",
            "R_BRACE",
            "INTERP_END",
            "STRING_END"
        ]
    );
    // 穴の外の `}` は、いつも普通の閉じ括弧である
    assert_eq!(kinds("}"), ["R_BRACE"]);
}

#[test]
fn braces_in_comments_and_strings_do_not_close_a_hole() {
    assert_eq!(
        kinds(r#""\{f "}" {- } -} x}""#),
        [
            "STRING_START",
            "INTERP_START",
            "LIDENT",
            "STRING_START",
            "STRING_TEXT",
            "STRING_END",
            "LIDENT",
            "INTERP_END",
            "STRING_END",
        ]
    );
}

#[test]
fn an_unclosed_hole_ends_at_the_newline_with_an_empty_interp_end() {
    let text = "\"a \\{x\ny";
    assert_eq!(
        diags(text),
        ["E0002@3..5 unterminated string interpolation"]
    );
    insta::assert_snapshot!(dump(text), @r#"
    STRING_START@0..1 "\""
    STRING_TEXT@1..3 "a "
    INTERP_START@3..5 "\\{"
    LIDENT@5..6 "x"
    INTERP_END@6..6 ""
    WHITESPACE@6..7 "\n"
    LIDENT@7..8 "y"
    ---
    [E0002] Error: unterminated string interpolation
       ╭─[ test.em:1:4 ]
       │
     1 │ "a \{x
       │    ─┬  
       │     ╰── missing closing `}`
       │ 
       │ Note: an interpolation must be closed on the same line
    ───╯
    "#);
}

#[test]
fn only_the_innermost_unclosed_layer_is_reported() {
    // 閉じていないのは内側の文字列、穴、外側の文字列の3層だが、報告は内側の文字列の1件だけである
    assert_eq!(
        diags("\"a \\{f \"b"),
        ["E0002@7..9 unterminated string literal"]
    );
    assert_eq!(
        diags("\"a \\{f \"b\nc"),
        ["E0002@7..9 unterminated string literal"]
    );
}

/// `diags` に、secondary のラベルと note を足したもの。
fn details(text: &str) -> Vec<String> {
    let (_, file) = source(text);
    let (_, mut diagnostics) = lex(file, text);
    eml_diagnostics::sort_diagnostics(&mut diagnostics);
    diagnostics
        .iter()
        .map(|d| {
            let mut line = format!("{}@{:?} {}", d.code, d.primary.range, d.message);
            for label in &d.secondary {
                line.push_str(&format!(" | {:?} {}", label.range, label.message));
            }
            for note in &d.notes {
                line.push_str(&format!(" | note: {note}"));
            }
            line
        })
        .collect()
}

#[test]
fn a_string_opened_in_a_hole_points_at_the_hole() {
    // 穴の中の `"` は新しい文字列を開くので、`}` を書き忘れると内側の文字列が閉じていないことになる。報告は内側の
    // 文字列の1件のままで、外側の穴の `\{` を secondary で示す
    let hint = "this interpolation is not closed; a `\"` inside `\\{…}` starts a new string";
    assert_eq!(
        details("x = \"count: \\{n\"\ny = 1"),
        [format!(
            "E0002@15..16 unterminated string literal | 12..14 {hint}"
        )]
    );
    assert_eq!(
        details("\"a \\{f \"b"),
        [format!(
            "E0002@7..9 unterminated string literal | 3..5 {hint}"
        )]
    );
    assert_eq!(
        details("\"a \\{f `ls"),
        [
            "E0002@7..10 unterminated command literal | 3..5 this interpolation is not closed; a \
          backtick inside `\\{…}` starts a new command literal"
        ]
    );
    // 穴の外の文字列と、内側の穴が一番内側の層のときは、secondary を付けない
    assert_eq!(
        details("\"a \\{f \"b \\{x\n"),
        [
            "E0002@10..12 unterminated string interpolation | note: an interpolation must be closed \
          on the same line"
        ]
    );
    assert_eq!(details("\"ab"), ["E0002@0..3 unterminated string literal"]);
}

#[test]
fn a_hole_closed_by_a_newline_has_a_note() {
    assert_eq!(
        details("\"a \\{x\ny"),
        [
            "E0002@3..5 unterminated string interpolation | note: an interpolation must be closed on \
          the same line"
        ]
    );
    // ファイルの終わりで閉じた穴には付けない。改行を足しても閉じないため
    assert_eq!(
        details("\"a \\{x"),
        ["E0002@3..5 unterminated string interpolation"]
    );
    // 複数行の文字列の中の穴も、改行で閉じる
    assert_eq!(
        details("\"\"\"\n  a \\{x\n  \"\"\""),
        [
            "E0002@8..10 unterminated string interpolation | note: an interpolation must be closed \
          on the same line"
        ]
    );
}

#[test]
fn a_block_comment_in_a_hole_stops_at_the_newline() {
    let found = diags("\"\\{x {- a\n-} }\"");
    assert_eq!(found[0], "E0005@5..7 unterminated block comment");
}

#[test]
fn a_unicode_escape_does_not_swallow_a_hole() {
    assert_eq!(
        kinds(r#""\u{4\{x}}""#),
        [
            "STRING_START",
            "ESCAPE",
            "STRING_TEXT",
            "INTERP_START",
            "LIDENT",
            "INTERP_END",
            "STRING_TEXT",
            "STRING_END"
        ]
    );
    assert_eq!(
        diags(r#""\u{4\{x}}""#),
        ["E0008@1..3 invalid unicode escape `\\u`"]
    );
}

#[test]
fn a_backslash_at_the_end_of_a_line_or_file_is_a_one_byte_escape() {
    assert_eq!(kinds("\"abc\\"), ["STRING_START", "STRING_TEXT", "ESCAPE"]);
    assert_eq!(diags("\"abc\\"), ["E0002@0..5 unterminated string literal"]);
    assert_eq!(
        kinds("\"abc\\\nx"),
        ["STRING_START", "STRING_TEXT", "ESCAPE", "LIDENT"]
    );
}

#[test]
fn command_literals_are_split_into_tokens() {
    assert_eq!(
        kinds("`ls \\{..xs} -l`"),
        [
            "CMD_START",
            "CMD_TEXT",
            "INTERP_START",
            "DOT2",
            "LIDENT",
            "INTERP_END",
            "CMD_TEXT",
            "CMD_END"
        ]
    );
    // コマンドリテラルの中の `"` は本文である
    assert_eq!(kinds("`a\"b`"), ["CMD_START", "CMD_TEXT", "CMD_END"]);
    assert_eq!(
        kinds("`a \\` b`"),
        ["CMD_START", "CMD_TEXT", "ESCAPE", "CMD_TEXT", "CMD_END"]
    );
    assert_eq!(diags("`ls"), ["E0002@0..3 unterminated command literal"]);
}

#[test]
fn multibyte_text_around_holes() {
    assert_eq!(
        kinds("\"é\\{x}ü\""),
        [
            "STRING_START",
            "STRING_TEXT",
            "INTERP_START",
            "LIDENT",
            "INTERP_END",
            "STRING_TEXT",
            "STRING_END"
        ]
    );
    assert_eq!(
        diags("\"é\\{x"),
        ["E0002@3..5 unterminated string interpolation"]
    );
}

#[test]
fn a_multiline_string_is_split_into_tokens() {
    let text = "\"\"\"\n  a\\{x}\n  \"\"\"";
    assert_eq!(
        kinds(text),
        [
            "STRING_START",
            "STRING_TEXT",
            "INTERP_START",
            "LIDENT",
            "INTERP_END",
            "STRING_TEXT",
            "STRING_END"
        ]
    );
    assert_eq!(diags(text), Vec::<String>::new());
}

#[test]
fn multiline_string_layout_errors_stay_inside_the_string() {
    // 開きと閉じが同じ行
    assert_eq!(
        diags("x = (\"\"\"m\"\"\", 1)"),
        ["E0014@5..12 a multi-line string must start on a new line after `\"\"\"`"]
    );
    // 開きの行の文字
    assert_eq!(
        diags("\"\"\" ab\n  \"\"\""),
        ["E0014@4..6 unexpected text after the opening `\"\"\"`"]
    );
    // 閉じの前の文字。字下げの検査はしない
    assert_eq!(
        diags("\"\"\"\nx\n  ab\"\"\""),
        ["E0014@8..10 unexpected text before the closing `\"\"\"`"]
    );
    // 字下げの足りない行ごとに1件
    assert_eq!(
        diags("\"\"\"\n a\nb\n    \"\"\""),
        [
            "E0014@4..6 line is indented less than the closing `\"\"\"`",
            "E0014@7..8 line is indented less than the closing `\"\"\"`",
        ]
    );
    // 閉じていなければ E0002 と開きの行の検査だけ
    assert_eq!(
        diags("\"\"\"abc"),
        [
            "E0002@0..3 unterminated multi-line string",
            "E0014@3..6 unexpected text after the opening `\"\"\"`",
        ]
    );
}

#[test]
fn short_indent_before_a_multibyte_character() {
    assert_eq!(
        diags("\"\"\"\n é\n  \"\"\""),
        ["E0014@4..7 line is indented less than the closing `\"\"\"`"]
    );
}

#[test]
fn a_backslash_newline_in_a_multiline_string_is_an_invalid_escape() {
    assert_eq!(
        diags("\"\"\"\n  a\\\n  \"\"\""),
        ["E0008@7..8 invalid escape `\\` at the end of a line"]
    );
}

#[test]
fn a_multiline_string_in_a_hole_is_reported_once() {
    assert_eq!(
        diags("\"\\{\"\"\"\nx"),
        ["E0014@3..6 multi-line strings are not allowed inside an interpolation"]
    );
}

#[test]
fn a_raw_string_in_a_hole_stops_at_the_newline() {
    assert_eq!(
        diags("\"\\{r\"a\nb\"}\""),
        ["E0002@3..5 unterminated raw string"]
    );
}

#[test]
fn a_unicode_escape_does_not_run_past_a_command_literal() {
    // `}` を探す範囲は、閉じのバッククォートの手前までである
    let text = "`a \\u{` b}";
    assert_eq!(
        kinds(text),
        [
            "CMD_START",
            "CMD_TEXT",
            "ESCAPE",
            "CMD_TEXT",
            "CMD_END",
            "LIDENT",
            "R_BRACE"
        ]
    );
    assert_eq!(diags(text), ["E0008@3..5 invalid unicode escape `\\u`"]);
}

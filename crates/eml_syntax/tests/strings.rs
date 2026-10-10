//! 文字列、補間、コマンドリテラルの CST と値 (docs/spec/lexical.md の「文字列」)。

use crate::common::{diagnostics, shape};
use eml_syntax::ast::{Expr, StringLit, StringPart};
use rowan::ast::AstNode;

/// `x = <literal>` の右辺の文字列の部分。穴は、その式のテキストで表す。
fn parts(literal: &str) -> Option<Vec<String>> {
    let parsed = eml_test_support::parse(&format!("x = {literal}"));
    let string = parsed
        .parse
        .syntax()
        .descendants()
        .find_map(StringLit::cast)
        .expect("a string");
    string.parts().map(|parts| {
        parts
            .into_iter()
            .map(|part| match part {
                StringPart::Text(text) => format!("text {text:?}"),
                StringPart::Hole(expr) => format!(
                    "hole {}",
                    expr.map_or("<missing>".to_string(), |e: Expr| e
                        .syntax()
                        .text()
                        .to_string())
                ),
            })
            .collect()
    })
}

#[test]
fn a_string_with_a_hole_is_a_string_lit_with_an_interp() {
    insta::assert_snapshot!(shape("x = \"a\\{y}b\""), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "x"
        EQ "="
        STRING_LIT
          STRING_START "\""
          STRING_TEXT "a"
          INTERP
            INTERP_START "\\{"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "y"
            INTERP_END "}"
          STRING_TEXT "b"
          STRING_END "\""
    "#);
}

#[test]
fn string_parts_decode_escapes_and_keep_holes() {
    assert_eq!(parts(r#""""#), Some(vec![]));
    assert_eq!(
        parts(r#""a\n\u{41}""#),
        Some(vec![r#"text "a\nA""#.to_string()])
    );
    assert_eq!(
        parts(r#""a\{f x}b\{y}""#),
        Some(vec![
            r#"text "a""#.to_string(),
            "hole f x".to_string(),
            r#"text "b""#.to_string(),
            "hole y".to_string(),
        ])
    );
    // 閉じていない文字列と不正なエスケープは、lexer が報告済みなので値を持たない
    assert_eq!(parts(r#""abc"#), None);
    assert_eq!(parts(r#""\q""#), None);
}

#[test]
fn a_command_literal_is_a_command_lit() {
    insta::assert_snapshot!(shape("x = `ls \\{..xs}`"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "x"
        EQ "="
        COMMAND_LIT
          CMD_START "`"
          CMD_TEXT "ls "
          INTERP
            INTERP_START "\\{"
            DOT2 ".."
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "xs"
            INTERP_END "}"
          CMD_END "`"
    "#);
    assert_eq!(diagnostics("x = `ls \\{..xs}`"), Vec::<String>::new());
}

#[test]
fn an_empty_hole_and_a_spread_in_a_string_are_errors() {
    assert_eq!(
        diagnostics("x = \"\\{}\""),
        ["E0011 1:8 expected an expression"]
    );
    assert_eq!(
        diagnostics("x = \"\\{..xs}\""),
        ["E0011 1:8 expected an expression"]
    );
}

#[test]
fn extra_tokens_in_a_hole_are_skipped_to_its_end() {
    assert_eq!(
        diagnostics("x = \"\\{a b) c}\" ++ d"),
        ["E0011 1:11 expected `}`"]
    );
}

#[test]
fn an_unclosed_hole_does_not_read_the_next_line() {
    // 幅 0 の `INTERP_END` で穴が閉じるので、次の行は次の等式として読める。パーサは誤りを重ねない
    assert_eq!(
        diagnostics("x = \"a \\{y\nz = 1"),
        ["E0002 1:8 unterminated string interpolation"]
    );
}

#[test]
fn an_unclosed_hole_with_an_unfinished_expression_reports_only_the_hole() {
    // 幅 0 の `INTERP_END` は lexer が報告済みなので、穴の中の入れ子の構文もそこで誤りを重ねない
    let holes = ["1 +", "if x then", "[1,", "let y = 1 in", "(x"];
    let found: Vec<_> = holes
        .iter()
        .map(|hole| (*hole, diagnostics(&format!("x = \"\\{{{hole}\nz = 1"))))
        .collect();
    let only_the_hole: Vec<_> = holes
        .iter()
        .map(|hole| {
            let expected = vec!["E0002 1:6 unterminated string interpolation".to_string()];
            (*hole, expected)
        })
        .collect();
    assert_eq!(found, only_the_hole);
}

#[test]
fn interpolation_in_a_pattern_is_an_error() {
    assert_eq!(
        diagnostics("f : String -> Int\nf \"a\\{x}\" = 1\nf _ = 0"),
        ["E0011 2:5 string interpolation is not allowed in a pattern"]
    );
}

#[test]
fn string_patterns_can_be_operator_equation_operands() {
    // 演算子の等式の先読み (`apat_len`) が、文字列のパターンを1つの apat として飛ばす
    assert_eq!(
        diagnostics("(<+>) : String -> String -> String\n\"a\" <+> b = b\na <+> b = a"),
        Vec::<String>::new()
    );
}

#[test]
fn multiline_parts_strip_the_indentation_of_the_closing_line() {
    let text = "\"\"\"   \n    Hello, \\{name}!\n      indented\n\n  \n    \"\"\"";
    assert_eq!(
        parts(text),
        Some(vec![
            r#"text "Hello, ""#.to_string(),
            "hole name".to_string(),
            r#"text "!\n  indented\n\n""#.to_string(),
        ])
    );
}

#[test]
fn a_multiline_string_closed_on_the_next_line_is_empty() {
    assert_eq!(parts("\"\"\"\n\"\"\""), Some(vec![]));
}

#[test]
fn crlf_multiline_strings_have_lf_values() {
    assert_eq!(
        parts("\"\"\"\r\n  a\r\n  b\\{x}\r\n  \"\"\""),
        Some(vec![r#"text "a\nb""#.to_string(), "hole x".to_string()])
    );
}

#[test]
fn a_multiline_string_with_a_layout_error_has_no_value() {
    assert_eq!(parts("\"\"\"\nab\n  \"\"\""), None);
}

#[test]
fn a_mismatched_bracket_in_a_hole_does_not_escape_it() {
    // `(` の閉じがないまま穴が閉じる。穴の外の括弧とブロックは影響を受けない
    assert_eq!(
        diagnostics("f x =\n  g (\"\\{(x}\", 1)\n  h"),
        ["E0011 2:11 expected `)`"]
    );
    // 穴の中の対応のない `)` は、穴の外の `(` を閉じない。閉じてしまうと、次の行の `)` がラムダの本体のブロックを
    // 閉じられず、`y)` に誤りが重なる
    assert_eq!(
        diagnostics("f x =\n  g (\"\\{x)}\", fn y ->\n    y)\n  h"),
        ["E0011 2:10 expected `}`"]
    );
}

#[test]
fn a_hole_inside_a_block_keeps_the_block_open() {
    assert_eq!(
        diagnostics("f : Int -> String\nf x =\n  let s = \"\\{x}\"\n  s"),
        Vec::<String>::new()
    );
}

#[test]
fn an_unexpected_string_is_named_as_a_string() {
    assert_eq!(
        diagnostics("data T = | A \"x\""),
        ["E0011 1:14 unexpected string"]
    );
}

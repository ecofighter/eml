mod common;

use common::diagnostics;
use eml_syntax::ast::{Literal, LiteralValue};
use rowan::ast::AstNode;

/// `x = <literal>` を構文解析し、右辺のリテラルの値を返す。
fn value(literal: &str) -> Option<LiteralValue> {
    let parsed = eml_test_support::parse(&format!("x = {literal}"));
    parsed
        .parse
        .syntax()
        .descendants()
        .find_map(Literal::cast)
        .expect("a literal")
        .value()
}

fn int_value(literal: &str) -> Option<i64> {
    match value(literal) {
        Some(LiteralValue::Int(n)) => Some(n),
        None => None,
        other => panic!("not an integer: {other:?}"),
    }
}

fn decode_string(literal: &str) -> Option<String> {
    match value(literal) {
        Some(LiteralValue::String(s)) => Some(s),
        None => None,
        other => panic!("not a string: {other:?}"),
    }
}

#[test]
fn integer_values() {
    assert_eq!(int_value("1_000"), Some(1000));
    assert_eq!(int_value("0xff"), Some(255));
    assert_eq!(int_value("0o17"), Some(15));
    assert_eq!(int_value("0b1010"), Some(10));
    assert_eq!(int_value("9223372036854775807"), Some(i64::MAX));
    assert_eq!(int_value("9223372036854775808"), None);
}

#[test]
fn too_large_integer_literals_are_reported() {
    assert_eq!(
        diagnostics("x = 9223372036854775808"),
        ["E0007 1:5 integer literal `9223372036854775808` is too large"]
    );
    assert!(diagnostics("x = 0x7fff_ffff_ffff_ffff").is_empty());
}

#[test]
fn string_values() {
    assert_eq!(decode_string(r#""a\nb""#).as_deref(), Some("a\nb"));
    assert_eq!(
        decode_string(r#""\t\r\\\"\0""#).as_deref(),
        Some("\t\r\\\"\0")
    );
    assert_eq!(decode_string(r#""\u{1F600}!""#).as_deref(), Some("😀!"));
    assert_eq!(decode_string(r#""""#).as_deref(), Some(""));
}

#[test]
fn strings_reported_by_the_lexer_have_no_value() {
    assert_eq!(decode_string(r#""\{x}""#), None);
    assert_eq!(decode_string(r#""\q""#), None);
    assert_eq!(decode_string(r#""abc"#), None);
    assert_eq!(decode_string(r#""abc\""#), None);
    assert_eq!(decode_string(r#"""#), None);
}

#[test]
fn unsupported_literals_have_no_value() {
    // 浮動小数と文字はパーサが E0004 を報告済みで、値は持たない。
    assert_eq!(value("1.5"), None);
    assert_eq!(value("'a'"), None);
}

#[test]
fn unicode_escapes_the_lexer_rejects_have_no_value() {
    // lexer の検査と値の取り出しは同じ規則を使う (docs/spec/lexical.md)。1〜6桁の16進数で、Unicode のスカラー値
    // でなければ、lexer が E0008 を報告し、値は持たない。
    for literal in [
        r#""\u{+41}""#,
        r#""\u{0000041}""#,
        r#""\u{}""#,
        r#""\u{110000}""#,
        r#""\u{D800}""#,
    ] {
        assert_eq!(decode_string(literal), None, "{literal}");
        assert!(
            diagnostics(&format!("x = {literal}"))
                .iter()
                .any(|d| d.starts_with("E0008 ")),
            "{literal}"
        );
    }
}

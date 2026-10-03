mod common;

use common::diagnostics;
use eml_syntax::{decode_string, int_value};

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

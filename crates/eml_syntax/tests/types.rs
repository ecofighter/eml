//! 型と row の構文。

mod common;

use common::{diagnostics, helps, lines, shape};

#[test]
fn signature_with_function_type() {
    insta::assert_snapshot!(shape("len : List a -> Int"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "len"
        COLON ":"
        FN_TYPE
          APP_TYPE
            PATH
              NAME_REF
                UIDENT "List"
            VAR_TYPE
              LIDENT "a"
          THIN_ARROW "->"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
    "#);
}

#[test]
fn qualified_type_names() {
    insta::assert_snapshot!(shape("x : Option.Option Int"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "x"
        COLON ":"
        APP_TYPE
          PATH
            NAME_REF
              UIDENT "Option"
            DOT "."
            NAME_REF
              UIDENT "Option"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
    "#);
}

#[test]
fn effect_row_with_effects_and_tail() {
    insta::assert_snapshot!(shape("f : a -> <IO, State s | e> b"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          VAR_TYPE
            LIDENT "a"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            EFFECT
              PATH
                NAME_REF
                  UIDENT "IO"
            COMMA ","
            EFFECT
              PATH
                NAME_REF
                  UIDENT "State"
              VAR_TYPE
                LIDENT "s"
            PIPE "|"
            LIDENT "e"
            R_ANGLE ">"
          VAR_TYPE
            LIDENT "b"
    "#);
}

#[test]
fn empty_row_is_split_from_one_operator_token() {
    insta::assert_snapshot!(shape("g : Unit -> <> Unit"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "g"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Unit"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            R_ANGLE ">"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Unit"
    "#);
}

#[test]
fn row_variable_alone() {
    insta::assert_snapshot!(shape("h : Unit -> <e> Unit"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "h"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Unit"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            LIDENT "e"
            R_ANGLE ">"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Unit"
    "#);
}

#[test]
fn arrows_at_the_end_of_lines_continue_the_type() {
    insta::assert_snapshot!(shape(&lines(&["f : Int ->", "  Int ->", "    Int"])), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
            THIN_ARROW "->"
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
    "#);
}

#[test]
fn tuple_and_parenthesized_types() {
    insta::assert_snapshot!(shape("p : (Int, (String -> Int))"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "p"
        COLON ":"
        TUPLE_TYPE
          L_PAREN "("
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
          COMMA ","
          PAREN_TYPE
            L_PAREN "("
            FN_TYPE
              PATH_TYPE
                PATH
                  NAME_REF
                    UIDENT "String"
              THIN_ARROW "->"
              PATH_TYPE
                PATH
                  NAME_REF
                    UIDENT "Int"
            R_PAREN ")"
          R_PAREN ")"
    "#);
}

#[test]
fn dot_with_spaces_in_a_qualified_name_is_an_error() {
    insta::assert_snapshot!(shape("f : Foo . Bar"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        PATH_TYPE
          PATH
            NAME_REF
              UIDENT "Foo"
            DOT "."
            NAME_REF
              UIDENT "Bar"
    ---
    E0010 1:9 unexpected whitespace around `.`
    "#);
}

#[test]
fn virtual_tokens_are_described_without_articles() {
    let text = lines(&["f : Int ->", "    Int", "    Int"]);
    assert_eq!(diagnostics(&text), ["E0011 2:8 unexpected line break"]);
}

#[test]
fn unclosed_row_is_an_error() {
    assert_eq!(
        diagnostics("f : Int -> <IO Int"),
        ["E0011 1:19 expected `>`"]
    );
}

#[test]
fn row_with_something_other_than_an_effect_is_an_error() {
    assert_eq!(
        diagnostics("f : Int -> <1> Int"),
        ["E0011 1:13 expected an effect"]
    );
}

#[test]
fn aligned_signature_lines_are_one_error() {
    insta::assert_snapshot!(shape(&lines(&["f : Int ->", "  Int ->", "  Int"])), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
            THIN_ARROW "->"
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
    ---
    E0009 2:7 expected an indented block after `->`
    "#);
}

#[test]
fn many_aligned_signature_lines_are_still_one_error() {
    assert_eq!(
        diagnostics(&lines(&["f : A ->", "  B ->", "  C ->", "  D"])),
        ["E0009 2:5 expected an indented block after `->`"]
    );
}

#[test]
fn arrow_at_the_end_of_a_line_suggests_a_leading_arrow() {
    assert_eq!(
        helps(&lines(&["f : Int ->", "  Int ->", "  Int"])),
        [
            "indent the next line more, or in a type that spans lines, put `->` at the start of the next line"
        ]
    );
}

#[test]
fn arrows_ending_consecutive_top_level_signatures_are_each_an_error() {
    // 揃えたシグネチャの続きの行ではないので、2件とも報告する。
    assert_eq!(
        diagnostics(&lines(&["f : A ->", "g : B ->", "h : C"])),
        [
            "E0009 1:7 expected an indented block after `->`",
            "E0009 2:7 expected an indented block after `->`"
        ]
    );
}

#[test]
fn arrows_ending_consecutive_operation_signatures_are_each_an_error() {
    assert_eq!(
        diagnostics(&lines(&[
            "effect E where",
            "  op : A ->",
            "  op2 : B ->",
            "  op3 : C"
        ])),
        [
            "E0009 2:10 expected an indented block after `->`",
            "E0009 3:11 expected an indented block after `->`"
        ]
    );
}

#[test]
fn row_written_right_after_the_arrow() {
    insta::assert_snapshot!(shape("f : Int -><IO> Int"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            EFFECT
              PATH
                NAME_REF
                  UIDENT "IO"
            R_ANGLE ">"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
    "#);
    assert!(diagnostics("f : Int -><> Int").is_empty());
}

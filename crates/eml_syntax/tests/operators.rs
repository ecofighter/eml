//! 演算子の列、前置の `-`、セクション、被演算子の欠けの構文。

mod common;

use common::{diagnostics, shape};

#[test]
fn operator_sequence_is_flat_with_prefix_minus() {
    insta::assert_snapshot!(shape("x = -a + b * c"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "x"
        EQ "="
        OP_SEQ
          MINUS "-"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "a"
          OP "+"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "b"
          OP "*"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "c"
    "#);
}

#[test]
fn minus_after_a_function_is_subtraction() {
    insta::assert_snapshot!(shape("y = f -1 :: xs"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "y"
        EQ "="
        OP_SEQ
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "f"
          MINUS "-"
          LITERAL
            INT "1"
          CONOP "::"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "xs"
    "#);
}

#[test]
fn sections() {
    insta::assert_snapshot!(shape("s = ((+), (+ 1), (1 +), (.name), (- 1), (-))"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "s"
        EQ "="
        TUPLE_EXPR
          L_PAREN "("
          OP_REF
            L_PAREN "("
            OP "+"
            R_PAREN ")"
          COMMA ","
          RIGHT_SECTION
            L_PAREN "("
            OP "+"
            LITERAL
              INT "1"
            R_PAREN ")"
          COMMA ","
          LEFT_SECTION
            L_PAREN "("
            LITERAL
              INT "1"
            OP "+"
            R_PAREN ")"
          COMMA ","
          FIELD_SECTION
            L_PAREN "("
            DOT "."
            LIDENT "name"
            R_PAREN ")"
          COMMA ","
          PAREN_EXPR
            L_PAREN "("
            OP_SEQ
              MINUS "-"
              LITERAL
                INT "1"
            R_PAREN ")"
          COMMA ","
          OP_REF
            L_PAREN "("
            MINUS "-"
            R_PAREN ")"
          R_PAREN ")"
    "#);
}

#[test]
fn missing_operand_does_not_affect_the_next_item() {
    insta::assert_snapshot!(shape("a = 1 +\nb = 2"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "a"
        EQ "="
        OP_SEQ
          LITERAL
            INT "1"
          OP "+"
      EQUATION
        NAME
          LIDENT "b"
        EQ "="
        LITERAL
          INT "2"
    ---
    E0011 1:8 expected an expression
    "#);
}

#[test]
fn missing_operand_at_the_end_of_the_file_points_after_the_operator() {
    assert_eq!(diagnostics("a = 1 +"), ["E0011 1:8 expected an expression"]);
    assert_eq!(
        diagnostics("a = 1 +\n"),
        ["E0011 1:8 expected an expression"]
    );
}

#[test]
fn missing_operand_before_a_comment_points_after_the_operator() {
    assert_eq!(
        diagnostics("a = 1 + -- c\nb = 2"),
        ["E0011 1:8 expected an expression"]
    );
}

#[test]
fn section_ending_with_an_operator_is_one_error() {
    // 被演算子の欠けたセクションは E0011 を1件だけ出し、次の項目を壊さない。
    insta::assert_snapshot!(shape("s = (+ a +)\nt = 1"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "s"
        EQ "="
        RIGHT_SECTION
          L_PAREN "("
          OP "+"
          OP_SEQ
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "a"
            OP "+"
          R_PAREN ")"
      EQUATION
        NAME
          LIDENT "t"
        EQ "="
        LITERAL
          INT "1"
    ---
    E0011 1:11 expected an expression
    "#);
}

#[test]
fn section_of_two_operators_is_one_error() {
    insta::assert_snapshot!(shape("s = (+ *)\nt = 1"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "s"
        EQ "="
        RIGHT_SECTION
          L_PAREN "("
          OP "+"
          ERROR
            OP "*"
          R_PAREN ")"
      EQUATION
        NAME
          LIDENT "t"
        EQ "="
        LITERAL
          INT "1"
    ---
    E0011 1:8 expected an expression
    "#);
}

#[test]
fn sections_with_operator_sequences() {
    insta::assert_snapshot!(shape("s = ((+ a * b), (a * b +), (+ -1))"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "s"
        EQ "="
        TUPLE_EXPR
          L_PAREN "("
          RIGHT_SECTION
            L_PAREN "("
            OP "+"
            OP_SEQ
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "a"
              OP "*"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "b"
            R_PAREN ")"
          COMMA ","
          LEFT_SECTION
            L_PAREN "("
            OP_SEQ
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "a"
              OP "*"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "b"
            OP "+"
            R_PAREN ")"
          COMMA ","
          RIGHT_SECTION
            L_PAREN "("
            OP "+"
            OP_SEQ
              MINUS "-"
              LITERAL
                INT "1"
            R_PAREN ")"
          R_PAREN ")"
    "#);
}

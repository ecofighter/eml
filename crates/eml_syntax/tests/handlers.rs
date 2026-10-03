mod common;

use common::{diagnostics, lines, shape};

#[test]
fn handler_with_operation_and_return_clauses() {
    let text = lines(&[
        "try action =",
        "  handle action () with",
        "    | fail _ -> None",
        "    | return x -> Some x",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "try"
        BIND_PAT
          LIDENT "action"
        EQ "="
        BLOCK
          EXPR_STMT
            HANDLE_EXPR
              HANDLE_KW "handle"
              APP_EXPR
                PATH_EXPR
                  LIDENT "action"
                UNIT_EXPR
                  L_PAREN "("
                  R_PAREN ")"
              WITH_KW "with"
              OP_CLAUSE
                PIPE "|"
                LIDENT "fail"
                WILDCARD_PAT
                  UNDERSCORE "_"
                THIN_ARROW "->"
                PATH_EXPR
                  UIDENT "None"
              RETURN_CLAUSE
                PIPE "|"
                RETURN_KW "return"
                BIND_PAT
                  LIDENT "x"
                THIN_ARROW "->"
                APP_EXPR
                  PATH_EXPR
                    UIDENT "Some"
                  PATH_EXPR
                    LIDENT "x"
    "#);
}

#[test]
fn parameterized_handler() {
    let text = lines(&[
        "run_state init action =",
        "  handle action () from init with",
        "    | get () k st -> resume k st st",
        "    | put st2 k _ -> resume k () st2",
        "    | return x st -> (x, st)",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "run_state"
        BIND_PAT
          LIDENT "init"
        BIND_PAT
          LIDENT "action"
        EQ "="
        BLOCK
          EXPR_STMT
            HANDLE_EXPR
              HANDLE_KW "handle"
              APP_EXPR
                PATH_EXPR
                  LIDENT "action"
                UNIT_EXPR
                  L_PAREN "("
                  R_PAREN ")"
              FROM_KW "from"
              PATH_EXPR
                LIDENT "init"
              WITH_KW "with"
              OP_CLAUSE
                PIPE "|"
                LIDENT "get"
                UNIT_PAT
                  L_PAREN "("
                  R_PAREN ")"
                BIND_PAT
                  LIDENT "k"
                BIND_PAT
                  LIDENT "st"
                THIN_ARROW "->"
                RESUME_EXPR
                  RESUME_KW "resume"
                  PATH_EXPR
                    LIDENT "k"
                  PATH_EXPR
                    LIDENT "st"
                  PATH_EXPR
                    LIDENT "st"
              OP_CLAUSE
                PIPE "|"
                LIDENT "put"
                BIND_PAT
                  LIDENT "st2"
                BIND_PAT
                  LIDENT "k"
                WILDCARD_PAT
                  UNDERSCORE "_"
                THIN_ARROW "->"
                RESUME_EXPR
                  RESUME_KW "resume"
                  PATH_EXPR
                    LIDENT "k"
                  UNIT_EXPR
                    L_PAREN "("
                    R_PAREN ")"
                  PATH_EXPR
                    LIDENT "st2"
              RETURN_CLAUSE
                PIPE "|"
                RETURN_KW "return"
                BIND_PAT
                  LIDENT "x"
                BIND_PAT
                  LIDENT "st"
                THIN_ARROW "->"
                TUPLE_EXPR
                  L_PAREN "("
                  PATH_EXPR
                    LIDENT "x"
                  COMMA ","
                  PATH_EXPR
                    LIDENT "st"
                  R_PAREN ")"
    "#);
}

#[test]
fn handler_on_one_line_with_drop() {
    insta::assert_snapshot!(shape("h = handle f () with | ask key k -> drop k"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "h"
        EQ "="
        HANDLE_EXPR
          HANDLE_KW "handle"
          APP_EXPR
            PATH_EXPR
              LIDENT "f"
            UNIT_EXPR
              L_PAREN "("
              R_PAREN ")"
          WITH_KW "with"
          OP_CLAUSE
            PIPE "|"
            LIDENT "ask"
            BIND_PAT
              LIDENT "key"
            BIND_PAT
              LIDENT "k"
            THIN_ARROW "->"
            DROP_EXPR
              DROP_KW "drop"
              PATH_EXPR
                LIDENT "k"
    "#);
}

#[test]
fn resume_is_an_operand() {
    insta::assert_snapshot!(shape("y = resume k 1 + 2"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "y"
        EQ "="
        OP_SEQ
          RESUME_EXPR
            RESUME_KW "resume"
            PATH_EXPR
              LIDENT "k"
            LITERAL
              INT "1"
          OP "+"
          LITERAL
            INT "2"
    "#);
}

#[test]
fn malformed_clauses_are_errors() {
    assert_eq!(
        diagnostics("h = handle f () with | return -> 1"),
        ["E0011 1:31 expected a pattern"]
    );
    assert_eq!(
        diagnostics("h = handle f () with | 1 -> 2"),
        ["E0011 1:24 expected a lowercase name"]
    );
}

#[test]
fn drop_needs_an_argument() {
    assert_eq!(
        diagnostics("h = drop"),
        ["E0011 1:9 expected an expression"]
    );
}

#[test]
fn handle_must_be_parenthesized_as_an_argument() {
    assert_eq!(
        diagnostics("f = g handle x with | return y -> y"),
        ["E0012 1:7 `handle` expression must be parenthesized here"]
    );
}

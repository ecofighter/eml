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
        NAME
          LIDENT "try"
        BIND_PAT
          NAME
            LIDENT "action"
        EQ "="
        BLOCK
          EXPR_STMT
            HANDLE_EXPR
              HANDLE_KW "handle"
              APP_EXPR
                PATH_EXPR
                  PATH
                    NAME_REF
                      LIDENT "action"
                UNIT_EXPR
                  L_PAREN "("
                  R_PAREN ")"
              WITH_KW "with"
              OP_CLAUSE
                PIPE "|"
                NAME_REF
                  LIDENT "fail"
                WILDCARD_PAT
                  UNDERSCORE "_"
                THIN_ARROW "->"
                PATH_EXPR
                  PATH
                    NAME_REF
                      UIDENT "None"
              RETURN_CLAUSE
                PIPE "|"
                RETURN_KW "return"
                BIND_PAT
                  NAME
                    LIDENT "x"
                THIN_ARROW "->"
                APP_EXPR
                  PATH_EXPR
                    PATH
                      NAME_REF
                        UIDENT "Some"
                  PATH_EXPR
                    PATH
                      NAME_REF
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
        NAME
          LIDENT "run_state"
        BIND_PAT
          NAME
            LIDENT "init"
        BIND_PAT
          NAME
            LIDENT "action"
        EQ "="
        BLOCK
          EXPR_STMT
            HANDLE_EXPR
              HANDLE_KW "handle"
              APP_EXPR
                PATH_EXPR
                  PATH
                    NAME_REF
                      LIDENT "action"
                UNIT_EXPR
                  L_PAREN "("
                  R_PAREN ")"
              FROM_KW "from"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "init"
              WITH_KW "with"
              OP_CLAUSE
                PIPE "|"
                NAME_REF
                  LIDENT "get"
                UNIT_PAT
                  L_PAREN "("
                  R_PAREN ")"
                BIND_PAT
                  NAME
                    LIDENT "k"
                BIND_PAT
                  NAME
                    LIDENT "st"
                THIN_ARROW "->"
                RESUME_EXPR
                  RESUME_KW "resume"
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "k"
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "st"
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "st"
              OP_CLAUSE
                PIPE "|"
                NAME_REF
                  LIDENT "put"
                BIND_PAT
                  NAME
                    LIDENT "st2"
                BIND_PAT
                  NAME
                    LIDENT "k"
                WILDCARD_PAT
                  UNDERSCORE "_"
                THIN_ARROW "->"
                RESUME_EXPR
                  RESUME_KW "resume"
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "k"
                  UNIT_EXPR
                    L_PAREN "("
                    R_PAREN ")"
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "st2"
              RETURN_CLAUSE
                PIPE "|"
                RETURN_KW "return"
                BIND_PAT
                  NAME
                    LIDENT "x"
                BIND_PAT
                  NAME
                    LIDENT "st"
                THIN_ARROW "->"
                TUPLE_EXPR
                  L_PAREN "("
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "x"
                  COMMA ","
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "st"
                  R_PAREN ")"
    "#);
}

#[test]
fn handler_on_one_line_with_drop() {
    insta::assert_snapshot!(shape("h = handle f () with | ask key k -> drop k"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "h"
        EQ "="
        HANDLE_EXPR
          HANDLE_KW "handle"
          APP_EXPR
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "f"
            UNIT_EXPR
              L_PAREN "("
              R_PAREN ")"
          WITH_KW "with"
          OP_CLAUSE
            PIPE "|"
            NAME_REF
              LIDENT "ask"
            BIND_PAT
              NAME
                LIDENT "key"
            BIND_PAT
              NAME
                LIDENT "k"
            THIN_ARROW "->"
            DROP_EXPR
              DROP_KW "drop"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "k"
    "#);
}

#[test]
fn resume_is_an_operand() {
    insta::assert_snapshot!(shape("y = resume k 1 + 2"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "y"
        EQ "="
        OP_SEQ
          RESUME_EXPR
            RESUME_KW "resume"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "k"
            LITERAL
              INT "1"
          OP "+"
          LITERAL
            INT "2"
    "#);
}

#[test]
fn resume_and_drop_must_be_parenthesized_as_arguments() {
    assert_eq!(
        diagnostics("f = g resume k 1"),
        ["E0012 1:7 `resume` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("f = g drop k"),
        ["E0012 1:7 `drop` expression must be parenthesized here"]
    );
}

#[test]
fn the_first_argument_of_resume_and_drop_follows_the_layers() {
    assert_eq!(
        diagnostics("f = drop if c then a else b"),
        ["E0012 1:10 `if` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("f = resume fn x -> x"),
        ["E0012 1:12 `fn` expression must be parenthesized here"]
    );
}

#[test]
fn resume_as_an_argument_is_read_as_an_operand() {
    insta::assert_snapshot!(shape("f = g resume k 1 + 2"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        EQ "="
        OP_SEQ
          APP_EXPR
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "g"
            RESUME_EXPR
              RESUME_KW "resume"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "k"
              LITERAL
                INT "1"
          OP "+"
          LITERAL
            INT "2"
    ---
    E0012 1:7 `resume` expression must be parenthesized here
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

#[test]
fn from_needs_an_initial_state() {
    assert_eq!(
        diagnostics("f = handle g () from with | return x -> x"),
        ["E0011 1:22 expected the initial state"]
    );
}

#[test]
fn clauses_must_start_with_a_pipe() {
    assert_eq!(
        diagnostics("f = handle g () with x"),
        ["E0011 1:22 expected a clause starting with `|`"]
    );
}

#[test]
fn clauses_at_the_column_of_handle_are_read_as_clauses() {
    assert_eq!(
        diagnostics(&lines(&["f =", "  handle g () with", "  | return x -> x"])),
        ["E0009 2:15 expected an indented block after `with`"]
    );
}

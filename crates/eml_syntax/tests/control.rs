use crate::common::{diagnostics, helps, lines, shape};

#[test]
fn if_with_else_on_separate_lines() {
    let text = lines(&["f c =", "  if c then", "    a", "  else", "    b"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "c"
        EQ "="
        BLOCK
          EXPR_STMT
            IF_EXPR
              IF_KW "if"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "c"
              THEN_KW "then"
              BLOCK
                EXPR_STMT
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "a"
              ELSE_KW "else"
              BLOCK
                EXPR_STMT
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "b"
    "#);
}

#[test]
fn if_without_else() {
    insta::assert_snapshot!(shape("f c = if c then g x"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "c"
        EQ "="
        IF_EXPR
          IF_KW "if"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "c"
          THEN_KW "then"
          APP_EXPR
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "g"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "x"
    "#);
}

#[test]
fn else_if_chain_on_one_line() {
    insta::assert_snapshot!(shape("f = if a then x else if b then y else z"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        EQ "="
        IF_EXPR
          IF_KW "if"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "a"
          THEN_KW "then"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "x"
          ELSE_KW "else"
          IF_EXPR
            IF_KW "if"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "b"
            THEN_KW "then"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "y"
            ELSE_KW "else"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "z"
    "#);
}

#[test]
fn else_on_the_next_line_after_a_one_line_then() {
    let text = lines(&["main () =", "  if s then a", "  else b"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "main"
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        BLOCK
          EXPR_STMT
            IF_EXPR
              IF_KW "if"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "s"
              THEN_KW "then"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "a"
              ELSE_KW "else"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "b"
    "#);
}

#[test]
fn missing_then_is_an_error() {
    assert_eq!(diagnostics("f = if c 1"), ["E0011 1:11 expected `then`"]);
}

#[test]
fn match_with_indented_arms() {
    let text = lines(&[
        "f b =",
        "  match b with",
        "    | True -> 1",
        "    | False ->",
        "        g x",
        "        0",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "b"
        EQ "="
        BLOCK
          EXPR_STMT
            MATCH_EXPR
              MATCH_KW "match"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "b"
              WITH_KW "with"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  PATH
                    NAME_REF
                      UIDENT "True"
                THIN_ARROW "->"
                LITERAL
                  INT "1"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  PATH
                    NAME_REF
                      UIDENT "False"
                THIN_ARROW "->"
                BLOCK
                  EXPR_STMT
                    APP_EXPR
                      PATH_EXPR
                        PATH
                          NAME_REF
                            LIDENT "g"
                      PATH_EXPR
                        PATH
                          NAME_REF
                            LIDENT "x"
                  EXPR_STMT
                    LITERAL
                      INT "0"
    "#);
}

#[test]
fn match_on_one_line() {
    insta::assert_snapshot!(shape("g b = match b with | True -> 1 | False -> 0"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "g"
        BIND_PAT
          NAME
            LIDENT "b"
        EQ "="
        MATCH_EXPR
          MATCH_KW "match"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "b"
          WITH_KW "with"
          MATCH_ARM
            PIPE "|"
            CON_PAT
              PATH
                NAME_REF
                  UIDENT "True"
            THIN_ARROW "->"
            LITERAL
              INT "1"
          MATCH_ARM
            PIPE "|"
            CON_PAT
              PATH
                NAME_REF
                  UIDENT "False"
            THIN_ARROW "->"
            LITERAL
              INT "0"
    "#);
}

#[test]
fn later_arms_on_one_line_belong_to_the_inner_match() {
    insta::assert_snapshot!(shape("h = match a with | X -> match b with | Y -> 1 | Z -> 2"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "h"
        EQ "="
        MATCH_EXPR
          MATCH_KW "match"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "a"
          WITH_KW "with"
          MATCH_ARM
            PIPE "|"
            CON_PAT
              PATH
                NAME_REF
                  UIDENT "X"
            THIN_ARROW "->"
            MATCH_EXPR
              MATCH_KW "match"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "b"
              WITH_KW "with"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  PATH
                    NAME_REF
                      UIDENT "Y"
                THIN_ARROW "->"
                LITERAL
                  INT "1"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  PATH
                    NAME_REF
                      UIDENT "Z"
                THIN_ARROW "->"
                LITERAL
                  INT "2"
    "#);
}

#[test]
fn arms_ending_with_an_arrow_are_each_an_error() {
    assert_eq!(
        diagnostics(&lines(&[
            "f x =",
            "  match x with",
            "    | A ->",
            "    | B ->",
            "    | C -> 1"
        ])),
        [
            "E0009 3:9 expected an indented block after `->`",
            "E0009 4:9 expected an indented block after `->`"
        ]
    );
}

#[test]
fn arms_at_the_column_of_match_are_read_as_arms() {
    let text = lines(&[
        "f b =",
        "  match b with",
        "  | True -> 1",
        "  | False -> 0",
        "  g b",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "b"
        EQ "="
        BLOCK
          EXPR_STMT
            MATCH_EXPR
              MATCH_KW "match"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "b"
              WITH_KW "with"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  PATH
                    NAME_REF
                      UIDENT "True"
                THIN_ARROW "->"
                LITERAL
                  INT "1"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  PATH
                    NAME_REF
                      UIDENT "False"
                THIN_ARROW "->"
                LITERAL
                  INT "0"
          EXPR_STMT
            APP_EXPR
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "g"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "b"
    ---
    E0009 2:11 expected an indented block after `with`
    "#);
}

#[test]
fn arms_at_the_column_of_match_get_a_help() {
    assert_eq!(
        helps(&lines(&["f b =", "  match b with", "  | True -> 1"])),
        ["indent the `|` arms more than the line with `with`"]
    );
}

#[test]
fn arm_without_pipe_is_an_error() {
    assert_eq!(
        diagnostics("f b = match b with True -> 1"),
        ["E0011 1:20 expected an arm starting with `|`"]
    );
    assert_eq!(
        diagnostics(&lines(&["f b =", "  match b with", "    True -> 1"])),
        ["E0011 3:5 expected `|` before the arm"]
    );
}

#[test]
fn control_expressions_must_be_parenthesized_as_arguments_and_operands() {
    assert_eq!(
        diagnostics("f = g match x with | A -> 1"),
        ["E0012 1:7 `match` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("x = 1 + if c then 2 else 3"),
        ["E0012 1:9 `if` expression must be parenthesized here"]
    );
}

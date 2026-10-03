mod common;

use common::{diagnostics, lines, shape};

#[test]
fn if_with_else_on_separate_lines() {
    let text = lines(&["f c =", "  if c then", "    a", "  else", "    b"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "c"
        EQ "="
        BLOCK
          EXPR_STMT
            IF_EXPR
              IF_KW "if"
              PATH_EXPR
                LIDENT "c"
              THEN_KW "then"
              BLOCK
                EXPR_STMT
                  PATH_EXPR
                    LIDENT "a"
              ELSE_KW "else"
              BLOCK
                EXPR_STMT
                  PATH_EXPR
                    LIDENT "b"
    "#);
}

#[test]
fn if_without_else() {
    insta::assert_snapshot!(shape("f c = if c then g x"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "c"
        EQ "="
        IF_EXPR
          IF_KW "if"
          PATH_EXPR
            LIDENT "c"
          THEN_KW "then"
          APP_EXPR
            PATH_EXPR
              LIDENT "g"
            PATH_EXPR
              LIDENT "x"
    "#);
}

#[test]
fn else_if_chain_on_one_line() {
    insta::assert_snapshot!(shape("f = if a then x else if b then y else z"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        EQ "="
        IF_EXPR
          IF_KW "if"
          PATH_EXPR
            LIDENT "a"
          THEN_KW "then"
          PATH_EXPR
            LIDENT "x"
          ELSE_KW "else"
          IF_EXPR
            IF_KW "if"
            PATH_EXPR
              LIDENT "b"
            THEN_KW "then"
            PATH_EXPR
              LIDENT "y"
            ELSE_KW "else"
            PATH_EXPR
              LIDENT "z"
    "#);
}

#[test]
fn else_on_the_next_line_after_a_one_line_then() {
    let text = lines(&["main () =", "  if s then a", "  else b"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
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
                LIDENT "s"
              THEN_KW "then"
              PATH_EXPR
                LIDENT "a"
              ELSE_KW "else"
              PATH_EXPR
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
        LIDENT "f"
        BIND_PAT
          LIDENT "b"
        EQ "="
        BLOCK
          EXPR_STMT
            MATCH_EXPR
              MATCH_KW "match"
              PATH_EXPR
                LIDENT "b"
              WITH_KW "with"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  UIDENT "True"
                THIN_ARROW "->"
                LITERAL
                  INT "1"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  UIDENT "False"
                THIN_ARROW "->"
                BLOCK
                  EXPR_STMT
                    APP_EXPR
                      PATH_EXPR
                        LIDENT "g"
                      PATH_EXPR
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
        LIDENT "g"
        BIND_PAT
          LIDENT "b"
        EQ "="
        MATCH_EXPR
          MATCH_KW "match"
          PATH_EXPR
            LIDENT "b"
          WITH_KW "with"
          MATCH_ARM
            PIPE "|"
            CON_PAT
              UIDENT "True"
            THIN_ARROW "->"
            LITERAL
              INT "1"
          MATCH_ARM
            PIPE "|"
            CON_PAT
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
        LIDENT "h"
        EQ "="
        MATCH_EXPR
          MATCH_KW "match"
          PATH_EXPR
            LIDENT "a"
          WITH_KW "with"
          MATCH_ARM
            PIPE "|"
            CON_PAT
              UIDENT "X"
            THIN_ARROW "->"
            MATCH_EXPR
              MATCH_KW "match"
              PATH_EXPR
                LIDENT "b"
              WITH_KW "with"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  UIDENT "Y"
                THIN_ARROW "->"
                LITERAL
                  INT "1"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  UIDENT "Z"
                THIN_ARROW "->"
                LITERAL
                  INT "2"
    "#);
}

#[test]
fn arms_at_the_column_of_match_need_indentation() {
    assert_eq!(
        diagnostics(&lines(&["f b =", "  match b with", "  | True -> 1"])),
        [
            "E0009 2:11 expected an indented block after `with`",
            "E0011 3:3 expected a statement",
        ]
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
fn trailing_lambda_with_a_block_body() {
    let text = lines(&["f = each items fn item ->", "  println item"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        EQ "="
        APP_EXPR
          PATH_EXPR
            LIDENT "each"
          PATH_EXPR
            LIDENT "items"
          LAMBDA_EXPR
            FN_KW "fn"
            BIND_PAT
              LIDENT "item"
            THIN_ARROW "->"
            BLOCK
              EXPR_STMT
                APP_EXPR
                  PATH_EXPR
                    LIDENT "println"
                  PATH_EXPR
                    LIDENT "item"
    "#);
}

#[test]
fn lambda_parameters() {
    insta::assert_snapshot!(shape("g = map (fn (x : Int) (a, b) -> x) xs"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "g"
        EQ "="
        APP_EXPR
          PATH_EXPR
            LIDENT "map"
          PAREN_EXPR
            L_PAREN "("
            LAMBDA_EXPR
              FN_KW "fn"
              ANNOT_PAT
                L_PAREN "("
                BIND_PAT
                  LIDENT "x"
                COLON ":"
                PATH_TYPE
                  UIDENT "Int"
                R_PAREN ")"
              TUPLE_PAT
                L_PAREN "("
                BIND_PAT
                  LIDENT "a"
                COMMA ","
                BIND_PAT
                  LIDENT "b"
                R_PAREN ")"
              THIN_ARROW "->"
              PATH_EXPR
                LIDENT "x"
            R_PAREN ")"
          PATH_EXPR
            LIDENT "xs"
    "#);
}

#[test]
fn lambda_as_an_operand() {
    insta::assert_snapshot!(shape("h = xs |> each fn l -> println l"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "h"
        EQ "="
        OP_SEQ
          PATH_EXPR
            LIDENT "xs"
          OP "|>"
          APP_EXPR
            PATH_EXPR
              LIDENT "each"
            LAMBDA_EXPR
              FN_KW "fn"
              BIND_PAT
                LIDENT "l"
              THIN_ARROW "->"
              APP_EXPR
                PATH_EXPR
                  LIDENT "println"
                PATH_EXPR
                  LIDENT "l"
    "#);
}

#[test]
fn lambda_needs_a_parameter() {
    assert_eq!(
        diagnostics("f = fn -> 1"),
        ["E0011 1:8 expected a parameter"]
    );
}

#[test]
fn let_in_inside_parentheses() {
    insta::assert_snapshot!(shape("f = (let x = 1 in x)"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        EQ "="
        PAREN_EXPR
          L_PAREN "("
          LET_EXPR
            LET_KW "let"
            BIND_PAT
              LIDENT "x"
            EQ "="
            LITERAL
              INT "1"
            IN_KW "in"
            PATH_EXPR
              LIDENT "x"
          R_PAREN ")"
    "#);
}

#[test]
fn use_statements() {
    let text = lines(&[
        "main () =",
        "  use with_env",
        "  use tmp <- with_temp_dir",
        "  build tmp",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "main"
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        BLOCK
          USE_STMT
            USE_KW "use"
            PATH_EXPR
              LIDENT "with_env"
          USE_STMT
            USE_KW "use"
            BIND_PAT
              LIDENT "tmp"
            LEFT_ARROW "<-"
            PATH_EXPR
              LIDENT "with_temp_dir"
          EXPR_STMT
            APP_EXPR
              PATH_EXPR
                LIDENT "build"
              PATH_EXPR
                LIDENT "tmp"
    "#);
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

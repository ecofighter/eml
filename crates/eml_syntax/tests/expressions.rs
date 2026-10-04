mod common;

use common::{diagnostics, lines, shape};

#[test]
fn equation_with_a_constructor_pattern() {
    insta::assert_snapshot!(shape("len Nil = 0"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "len"
        CON_PAT
          UIDENT "Nil"
        EQ "="
        LITERAL
          INT "0"
    "#);
}

#[test]
fn parameter_patterns() {
    insta::assert_snapshot!(shape("f (Some (x, _)) (y :: rest) 0 -1 \"s\" () = x"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        PAREN_PAT
          L_PAREN "("
          CON_PAT
            UIDENT "Some"
            TUPLE_PAT
              L_PAREN "("
              BIND_PAT
                LIDENT "x"
              COMMA ","
              WILDCARD_PAT
                UNDERSCORE "_"
              R_PAREN ")"
          R_PAREN ")"
        PAREN_PAT
          L_PAREN "("
          INFIX_CON_PAT
            BIND_PAT
              LIDENT "y"
            CONOP "::"
            BIND_PAT
              LIDENT "rest"
          R_PAREN ")"
        LITERAL_PAT
          INT "0"
        LITERAL_PAT
          MINUS "-"
          INT "1"
        LITERAL_PAT
          STRING "\"s\""
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        PATH_EXPR
          LIDENT "x"
    "#);
}

#[test]
fn block_body_with_let_and_expression_statements() {
    let text = lines(&[
        "main () =",
        "  let x = 1",
        "  let n : Int = x",
        "  println x",
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
          LET_STMT
            LET_KW "let"
            BIND_PAT
              LIDENT "x"
            EQ "="
            LITERAL
              INT "1"
          LET_STMT
            LET_KW "let"
            BIND_PAT
              LIDENT "n"
            COLON ":"
            PATH_TYPE
              UIDENT "Int"
            EQ "="
            PATH_EXPR
              LIDENT "x"
          EXPR_STMT
            APP_EXPR
              PATH_EXPR
                LIDENT "println"
              PATH_EXPR
                LIDENT "x"
    "#);
}

#[test]
fn application_field_access_and_qualified_names() {
    insta::assert_snapshot!(shape("y = String.split_once \" \" line.text t.0.1"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "y"
        EQ "="
        APP_EXPR
          PATH_EXPR
            UIDENT "String"
            DOT "."
            LIDENT "split_once"
          LITERAL
            STRING "\" \""
          FIELD_EXPR
            PATH_EXPR
              LIDENT "line"
            DOT "."
            LIDENT "text"
          FIELD_EXPR
            FIELD_EXPR
              PATH_EXPR
                LIDENT "t"
              DOT "."
              INT "0"
            DOT "."
            INT "1"
    "#);
}

#[test]
fn annotation_and_unit() {
    insta::assert_snapshot!(shape("a = ((x : Int), ())"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "a"
        EQ "="
        TUPLE_EXPR
          L_PAREN "("
          ANNOT_EXPR
            L_PAREN "("
            PATH_EXPR
              LIDENT "x"
            COLON ":"
            PATH_TYPE
              UIDENT "Int"
            R_PAREN ")"
          COMMA ","
          UNIT_EXPR
            L_PAREN "("
            R_PAREN ")"
          R_PAREN ")"
    "#);
}

#[test]
fn deeper_lines_continue_the_expression() {
    let text = lines(&["t =", "  lines s", "    |> map f", "    |> sum"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "t"
        EQ "="
        BLOCK
          EXPR_STMT
            OP_SEQ
              APP_EXPR
                PATH_EXPR
                  LIDENT "lines"
                PATH_EXPR
                  LIDENT "s"
              OP "|>"
              APP_EXPR
                PATH_EXPR
                  LIDENT "map"
                PATH_EXPR
                  LIDENT "f"
              OP "|>"
              PATH_EXPR
                LIDENT "sum"
    "#);
}

#[test]
fn dot_with_spaces_is_an_error() {
    insta::assert_snapshot!(shape("x = a . b"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "x"
        EQ "="
        FIELD_EXPR
          PATH_EXPR
            LIDENT "a"
          DOT "."
          LIDENT "b"
    ---
    E0010 1:7 unexpected whitespace around `.`
    "#);
}

#[test]
fn operator_definition() {
    insta::assert_snapshot!(shape("dir </> name = join dir name"), @r#"
    SOURCE_FILE
      EQUATION
        BIND_PAT
          LIDENT "dir"
        OP "</>"
        BIND_PAT
          LIDENT "name"
        EQ "="
        APP_EXPR
          PATH_EXPR
            LIDENT "join"
          PATH_EXPR
            LIDENT "dir"
          PATH_EXPR
            LIDENT "name"
    "#);
}

#[test]
fn top_level_pattern_bindings_are_errors() {
    insta::assert_snapshot!(shape("(a, b) = p"), @r#"
    SOURCE_FILE
      ERROR
        TUPLE_PAT
          L_PAREN "("
          BIND_PAT
            LIDENT "a"
          COMMA ","
          BIND_PAT
            LIDENT "b"
          R_PAREN ")"
        EQ "="
        LIDENT "p"
    ---
    E0011 1:8 top-level pattern bindings are not allowed
    "#);
    assert_eq!(
        diagnostics("x :: rest = xs"),
        ["E0011 1:3 top-level pattern bindings are not allowed"]
    );
}

#[test]
fn later_stage_literals_are_not_supported_yet() {
    assert_eq!(
        diagnostics("x = (1.5, 'c', [1], r\"raw\", \"\"\"m\"\"\", `ls`)"),
        [
            "E0004 1:6 floating-point literals are not supported yet",
            "E0004 1:11 character literals are not supported yet",
            "E0004 1:16 lists are not supported yet",
            "E0004 1:21 raw strings are not supported yet",
            "E0004 1:29 multi-line strings are not supported yet",
            "E0004 1:38 command literals are not supported yet",
        ]
    );
}

#[test]
fn let_in_as_a_statement() {
    let text = lines(&["f x =", "  let y = x in y"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "x"
        EQ "="
        BLOCK
          EXPR_STMT
            LET_EXPR
              LET_KW "let"
              BIND_PAT
                LIDENT "y"
              EQ "="
              PATH_EXPR
                LIDENT "x"
              IN_KW "in"
              PATH_EXPR
                LIDENT "y"
    "#);
}

#[test]
fn body_on_an_unindented_line_gets_an_empty_block() {
    insta::assert_snapshot!(shape("f x =\ng = 1"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "x"
        EQ "="
        BLOCK
      EQUATION
        LIDENT "g"
        EQ "="
        LITERAL
          INT "1"
    ---
    E0009 1:5 expected an indented block after `=`
    "#);
}

#[test]
fn lambdas_ending_consecutive_lines_are_each_an_error() {
    assert_eq!(
        diagnostics(&lines(&[
            "f =",
            "  let g = fn a ->",
            "  let h = fn b ->",
            "  g"
        ])),
        [
            "E0009 2:16 expected an indented block after `->`",
            "E0009 3:16 expected an indented block after `->`"
        ]
    );
}

#[test]
fn line_at_column_0_after_a_missing_block_closes_the_bracket() {
    // 規則 2 の例外 (E0009 を出した行) は列 0 の行には当てない。h と k の欠けた右辺も報告する。
    assert_eq!(
        diagnostics(&lines(&["f = g (fn x ->", "h = 1 +", "k = 2 +"])),
        [
            "E0009 1:13 expected an indented block after `->`",
            "E0011 1:15 expected `)`",
            "E0011 2:8 expected an expression",
            "E0011 3:8 expected an expression"
        ]
    );
}

#[test]
fn unclosed_paren_is_reported() {
    assert_eq!(diagnostics("a = f (1\nb = 2"), ["E0011 1:9 expected `)`"]);
}

#[test]
fn leftover_tokens_in_a_statement_are_skipped_to_the_next_line() {
    assert_eq!(
        diagnostics(&lines(&["f =", "  g x)", "  h"])),
        ["E0011 2:6 unexpected `)`"]
    );
}

#[test]
fn error_token_in_an_expression_is_reported_once() {
    assert_eq!(
        diagnostics("z = f € x"),
        ["E0001 1:7 unexpected character `€`"]
    );
}

#[test]
fn nested_unclosed_brackets_are_reported_once() {
    assert_eq!(
        diagnostics("f = g (h (a\nb = 1"),
        ["E0011 1:12 expected `)`"]
    );
}

#[test]
fn mismatched_closing_bracket_closes_the_innermost_bracket() {
    insta::assert_snapshot!(shape("x = (a]\ny = 1"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "x"
        EQ "="
        PAREN_EXPR
          L_PAREN "("
          PATH_EXPR
            LIDENT "a"
          R_BRACK "]"
      EQUATION
        LIDENT "y"
        EQ "="
        LITERAL
          INT "1"
    ---
    E0011 1:7 expected `)`
    "#);
}

#[test]
fn mismatched_closing_bracket_ends_an_unsupported_list() {
    assert_eq!(
        diagnostics("x = [a)\ny = 1"),
        ["E0004 1:5 lists are not supported yet"]
    );
}

#[test]
fn semicolon_inside_brackets_is_one_error() {
    assert_eq!(
        diagnostics("h = (a; b)\nk = 1"),
        ["E0011 1:7 unexpected `;` inside brackets"]
    );
}

#[test]
fn implicitly_closed_bracket_in_an_unsupported_list_does_not_swallow_the_file() {
    assert_eq!(
        diagnostics("x = [a (b\ny = 1\nz = 2 +"),
        [
            "E0004 1:5 lists are not supported yet",
            "E0011 3:8 expected an expression"
        ]
    );
}

#[test]
fn implicitly_closed_bracket_after_a_semicolon_does_not_swallow_the_file() {
    assert_eq!(
        diagnostics("h = (a; (b\ny = 1\nz = 2 +"),
        [
            "E0011 1:7 unexpected `;` inside brackets",
            "E0011 3:8 expected an expression"
        ]
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

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
fn operator_sequence_is_flat_with_prefix_minus() {
    insta::assert_snapshot!(shape("x = -a + b * c"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "x"
        EQ "="
        OP_SEQ
          MINUS "-"
          PATH_EXPR
            LIDENT "a"
          OP "+"
          PATH_EXPR
            LIDENT "b"
          OP "*"
          PATH_EXPR
            LIDENT "c"
    "#);
}

#[test]
fn minus_after_a_function_is_subtraction() {
    insta::assert_snapshot!(shape("y = f -1 :: xs"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "y"
        EQ "="
        OP_SEQ
          PATH_EXPR
            LIDENT "f"
          MINUS "-"
          LITERAL
            INT "1"
          CONOP "::"
          PATH_EXPR
            LIDENT "xs"
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
fn sections() {
    insta::assert_snapshot!(shape("s = ((+), (+ 1), (1 +), (.name), (- 1), (-))"), @r#"
    SOURCE_FILE
      EQUATION
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
fn missing_operand_does_not_affect_the_next_item() {
    insta::assert_snapshot!(shape("a = 1 +\nb = 2"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "a"
        EQ "="
        OP_SEQ
          LITERAL
            INT "1"
          OP "+"
      EQUATION
        LIDENT "b"
        EQ "="
        LITERAL
          INT "2"
    ---
    E0011 1:8 expected an expression
    "#);
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

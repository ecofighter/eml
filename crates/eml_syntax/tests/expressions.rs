use crate::common::{diagnostics, lines, shape};

#[test]
fn equation_with_a_constructor_pattern() {
    insta::assert_snapshot!(shape("len Nil = 0"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "len"
        CON_PAT
          PATH
            NAME_REF
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
        NAME
          LIDENT "f"
        PAREN_PAT
          L_PAREN "("
          CON_PAT
            PATH
              NAME_REF
                UIDENT "Some"
            TUPLE_PAT
              L_PAREN "("
              BIND_PAT
                NAME
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
              NAME
                LIDENT "y"
            CONOP "::"
            BIND_PAT
              NAME
                LIDENT "rest"
          R_PAREN ")"
        LITERAL_PAT
          INT "0"
        LITERAL_PAT
          MINUS "-"
          INT "1"
        LITERAL_PAT
          STRING_LIT
            STRING_START "\""
            STRING_TEXT "s"
            STRING_END "\""
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        PATH_EXPR
          PATH
            NAME_REF
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
        NAME
          LIDENT "main"
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        BLOCK
          LET_STMT
            LET_KW "let"
            BIND_PAT
              NAME
                LIDENT "x"
            EQ "="
            LITERAL
              INT "1"
          LET_STMT
            LET_KW "let"
            BIND_PAT
              NAME
                LIDENT "n"
            COLON ":"
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
            EQ "="
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "x"
          EXPR_STMT
            APP_EXPR
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "println"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "x"
    "#);
}

#[test]
fn application_field_access_and_qualified_names() {
    insta::assert_snapshot!(shape("y = String.split_once \" \" line.text t.0.1"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "y"
        EQ "="
        APP_EXPR
          PATH_EXPR
            PATH
              NAME_REF
                UIDENT "String"
              DOT "."
              NAME_REF
                LIDENT "split_once"
          STRING_LIT
            STRING_START "\""
            STRING_TEXT " "
            STRING_END "\""
          FIELD_EXPR
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "line"
            DOT "."
            LIDENT "text"
          FIELD_EXPR
            FIELD_EXPR
              PATH_EXPR
                PATH
                  NAME_REF
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
        NAME
          LIDENT "a"
        EQ "="
        TUPLE_EXPR
          L_PAREN "("
          ANNOT_EXPR
            L_PAREN "("
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "x"
            COLON ":"
            PATH_TYPE
              PATH
                NAME_REF
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
        NAME
          LIDENT "t"
        EQ "="
        BLOCK
          EXPR_STMT
            OP_SEQ
              APP_EXPR
                PATH_EXPR
                  PATH
                    NAME_REF
                      LIDENT "lines"
                PATH_EXPR
                  PATH
                    NAME_REF
                      LIDENT "s"
              OP "|>"
              APP_EXPR
                PATH_EXPR
                  PATH
                    NAME_REF
                      LIDENT "map"
                PATH_EXPR
                  PATH
                    NAME_REF
                      LIDENT "f"
              OP "|>"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "sum"
    "#);
}

#[test]
fn dot_with_spaces_is_an_error() {
    insta::assert_snapshot!(shape("x = a . b"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "x"
        EQ "="
        FIELD_EXPR
          PATH_EXPR
            PATH
              NAME_REF
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
          NAME
            LIDENT "dir"
        NAME
          OP "</>"
        BIND_PAT
          NAME
            LIDENT "name"
        EQ "="
        APP_EXPR
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "join"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "dir"
          PATH_EXPR
            PATH
              NAME_REF
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
            NAME
              LIDENT "a"
          COMMA ","
          BIND_PAT
            NAME
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
fn later_stage_literals_are_parsed() {
    // 浮動小数、文字、コマンドリテラルの E0004 は HIR が出す
    // (docs/implementation/status.md の「未対応の構文と E0004」)
    assert_eq!(
        diagnostics("x = (1.5, 'c', [1], `ls`)"),
        Vec::<String>::new()
    );
}

#[test]
fn let_in_as_a_statement() {
    let text = lines(&["f x =", "  let y = x in y"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "x"
        EQ "="
        BLOCK
          EXPR_STMT
            LET_EXPR
              LET_KW "let"
              BIND_PAT
                NAME
                  LIDENT "y"
              EQ "="
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "x"
              IN_KW "in"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "y"
    "#);
}

#[test]
fn body_on_an_unindented_line_gets_an_empty_block() {
    insta::assert_snapshot!(shape("f x =\ng = 1"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "x"
        EQ "="
        BLOCK
      EQUATION
        NAME
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
        NAME
          LIDENT "x"
        EQ "="
        PAREN_EXPR
          L_PAREN "("
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "a"
          R_BRACK "]"
      EQUATION
        NAME
          LIDENT "y"
        EQ "="
        LITERAL
          INT "1"
    ---
    E0011 1:7 expected `)`
    "#);
}

#[test]
fn mismatched_closing_bracket_ends_a_list() {
    assert_eq!(diagnostics("x = [a)\ny = 1"), ["E0011 1:7 expected `]`"]);
}

#[test]
fn semicolon_inside_brackets_is_one_error() {
    assert_eq!(
        diagnostics("h = (a; b)\nk = 1"),
        ["E0011 1:7 unexpected `;` inside brackets"]
    );
}

#[test]
fn implicitly_closed_bracket_in_a_list_does_not_swallow_the_file() {
    assert_eq!(
        diagnostics("x = [a (b\ny = 1\nz = 2 +"),
        [
            "E0011 1:10 expected `)`",
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
fn lambda_in_parentheses_with_a_block_body() {
    let text = lines(&["f = each items (fn item ->", "  println item)"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        EQ "="
        APP_EXPR
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "each"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "items"
          PAREN_EXPR
            L_PAREN "("
            LAMBDA_EXPR
              FN_KW "fn"
              BIND_PAT
                NAME
                  LIDENT "item"
              THIN_ARROW "->"
              BLOCK
                EXPR_STMT
                  APP_EXPR
                    PATH_EXPR
                      PATH
                        NAME_REF
                          LIDENT "println"
                    PATH_EXPR
                      PATH
                        NAME_REF
                          LIDENT "item"
            R_PAREN ")"
    "#);
}

#[test]
fn lambda_parameters() {
    insta::assert_snapshot!(shape("g = map (fn (x : Int) (a, b) -> x) xs"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "g"
        EQ "="
        APP_EXPR
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "map"
          PAREN_EXPR
            L_PAREN "("
            LAMBDA_EXPR
              FN_KW "fn"
              ANNOT_PAT
                L_PAREN "("
                BIND_PAT
                  NAME
                    LIDENT "x"
                COLON ":"
                PATH_TYPE
                  PATH
                    NAME_REF
                      UIDENT "Int"
                R_PAREN ")"
              TUPLE_PAT
                L_PAREN "("
                BIND_PAT
                  NAME
                    LIDENT "a"
                COMMA ","
                BIND_PAT
                  NAME
                    LIDENT "b"
                R_PAREN ")"
              THIN_ARROW "->"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "x"
            R_PAREN ")"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "xs"
    "#);
}

#[test]
fn lambda_must_be_parenthesized_as_an_argument_or_an_operand() {
    assert_eq!(
        diagnostics("f = g fn x -> x"),
        ["E0012 1:7 `fn` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("h = xs |> each fn l -> println l"),
        ["E0012 1:16 `fn` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("f = 1 + fn x -> x"),
        ["E0012 1:9 `fn` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("f = - fn x -> x"),
        ["E0012 1:7 `fn` expression must be parenthesized here"]
    );
}

#[test]
fn old_trailing_lambda_reports_one_error() {
    let text = lines(&["f = each items fn item ->", "  println item"]);
    assert_eq!(
        diagnostics(&text),
        ["E0012 1:16 `fn` expression must be parenthesized here"]
    );
}

#[test]
fn lambda_in_a_tuple_and_an_annotation() {
    assert_eq!(
        diagnostics("f = ((fn x -> x, 1), (fn y -> y : Int -> Int))"),
        Vec::<String>::new()
    );
}

#[test]
fn lambda_closed_on_its_own_line() {
    let text = lines(&["f =", "  each items (fn item ->", "    println item", "  )"]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn lambdas_in_if_branches() {
    assert_eq!(
        diagnostics("f c = if c then fn x -> x else fn y -> y"),
        Vec::<String>::new()
    );
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
        NAME
          LIDENT "f"
        EQ "="
        PAREN_EXPR
          L_PAREN "("
          LET_EXPR
            LET_KW "let"
            BIND_PAT
              NAME
                LIDENT "x"
            EQ "="
            LITERAL
              INT "1"
            IN_KW "in"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "x"
          R_PAREN ")"
    "#);
}

#[test]
fn let_in_with_a_block_on_the_right() {
    let text = lines(&["f x =", "  let y =", "      x + 1", "    in y"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "x"
        EQ "="
        BLOCK
          EXPR_STMT
            LET_EXPR
              LET_KW "let"
              BIND_PAT
                NAME
                  LIDENT "y"
              EQ "="
              BLOCK
                EXPR_STMT
                  OP_SEQ
                    PATH_EXPR
                      PATH
                        NAME_REF
                          LIDENT "x"
                    OP "+"
                    LITERAL
                      INT "1"
              IN_KW "in"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "y"
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
        NAME
          LIDENT "main"
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        BLOCK
          USE_STMT
            USE_KW "use"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "with_env"
          USE_STMT
            USE_KW "use"
            BIND_PAT
              NAME
                LIDENT "tmp"
            LEFT_ARROW "<-"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "with_temp_dir"
          EXPR_STMT
            APP_EXPR
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "build"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "tmp"
    "#);
}

#[test]
fn list_expressions() {
    insta::assert_snapshot!(shape("x = [1, f y, []]"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "x"
        EQ "="
        LIST_EXPR
          L_BRACK "["
          LITERAL
            INT "1"
          COMMA ","
          APP_EXPR
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "f"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "y"
          COMMA ","
          LIST_EXPR
            L_BRACK "["
            R_BRACK "]"
          R_BRACK "]"
    "#);
}

#[test]
fn a_list_may_end_with_a_comma_and_span_lines() {
    let text = lines(&[
        "main () =",
        "  let xs = [",
        "    1,",
        "    2,",
        "  ]",
        "  xs",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
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
          LET_STMT
            LET_KW "let"
            BIND_PAT
              NAME
                LIDENT "xs"
            EQ "="
            LIST_EXPR
              L_BRACK "["
              LITERAL
                INT "1"
              COMMA ","
              LITERAL
                INT "2"
              COMMA ","
              R_BRACK "]"
          EXPR_STMT
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "xs"
    "#);
}

// 括弧の中で `->` が開いたブロックは、閉じ括弧で閉じる。その前に行末の `,` があれば、そこで閉じる
// (docs/spec/layout.md の規則 4)。このテストは、最後の要素が開いたブロックを閉じ括弧で閉じる場合である。
#[test]
fn a_list_element_can_open_a_block() {
    let text = lines(&[
        "main () =",
        "  let fs = [fn x -> x, fn x ->",
        "    x + 1]",
        "  fs",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn an_element_after_a_line_final_comma_may_start_at_the_column_of_the_block() {
    let text = lines(&["f a b =", "  g (fn x ->", "      a,", "      b)"]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn a_trailing_comma_before_a_deeper_closing_bracket_closes_the_block() {
    let text = lines(&["f =", "  [fn x ->", "      x + 1,", "        ]"]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn list_patterns() {
    insta::assert_snapshot!(shape("f [a, _] [] = a"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        LIST_PAT
          L_BRACK "["
          BIND_PAT
            NAME
              LIDENT "a"
          COMMA ","
          WILDCARD_PAT
            UNDERSCORE "_"
          R_BRACK "]"
        LIST_PAT
          L_BRACK "["
          R_BRACK "]"
        EQ "="
        PATH_EXPR
          PATH
            NAME_REF
              LIDENT "a"
    "#);
}

#[test]
fn a_list_pattern_may_end_with_a_comma() {
    assert_eq!(diagnostics("f [a,] = a"), Vec::<String>::new());
}

#[test]
fn unclosed_list_pattern_does_not_swallow_the_next_item() {
    assert_eq!(
        diagnostics("f [a\ng = 1 +"),
        ["E0011 1:5 expected `]`", "E0011 2:8 expected an expression"]
    );
}

#[test]
fn a_comma_closes_a_block_opened_inside_brackets() {
    let text = lines(&[
        "f n =",
        "  g (fn x ->",
        "      x + 1,",
        "    n)",
        "",
        "h n =",
        "  [fn x ->",
        "    x,",
        "   fn y -> y]",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

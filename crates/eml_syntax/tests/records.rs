//! レコードの構文 (docs/spec/records.md の「構文」)。

use crate::common::{diagnostics, lines, shape};

#[test]
fn a_record_alternative_has_field_declarations() {
    insta::assert_snapshot!(shape("data P =\n  | P { name : String, age : Int }\n  | Q Int"), @r#"
    SOURCE_FILE
      DATA_ITEM
        DATA_KW "data"
        NAME
          UIDENT "P"
        EQ "="
        ALT
          PIPE "|"
          NAME
            UIDENT "P"
          RECORD_FIELDS
            L_BRACE "{"
            FIELD_DECL
              NAME
                LIDENT "name"
              COLON ":"
              PATH_TYPE
                PATH
                  NAME_REF
                    UIDENT "String"
            COMMA ","
            FIELD_DECL
              NAME
                LIDENT "age"
              COLON ":"
              PATH_TYPE
                PATH
                  NAME_REF
                    UIDENT "Int"
            R_BRACE "}"
        ALT
          PIPE "|"
          NAME
            UIDENT "Q"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
    "#);
}

#[test]
fn construction_update_and_patterns() {
    let text = lines(&[
        "f p = match p with",
        "  | P { name, age = a } -> P { name, age = a + 1 }",
        "  | q -> { q | age = 0 }",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "p"
        EQ "="
        MATCH_EXPR
          MATCH_KW "match"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "p"
          WITH_KW "with"
          MATCH_ARM
            PIPE "|"
            RECORD_PAT
              PATH
                NAME_REF
                  UIDENT "P"
              L_BRACE "{"
              FIELD_PAT
                NAME_REF
                  LIDENT "name"
              COMMA ","
              FIELD_PAT
                NAME_REF
                  LIDENT "age"
                EQ "="
                BIND_PAT
                  NAME
                    LIDENT "a"
              R_BRACE "}"
            THIN_ARROW "->"
            RECORD_EXPR
              PATH
                NAME_REF
                  UIDENT "P"
              L_BRACE "{"
              FIELD
                NAME_REF
                  LIDENT "name"
              COMMA ","
              FIELD
                NAME_REF
                  LIDENT "age"
                EQ "="
                OP_SEQ
                  PATH_EXPR
                    PATH
                      NAME_REF
                        LIDENT "a"
                  OP "+"
                  LITERAL
                    INT "1"
              R_BRACE "}"
          MATCH_ARM
            PIPE "|"
            BIND_PAT
              NAME
                LIDENT "q"
            THIN_ARROW "->"
            UPDATE_EXPR
              L_BRACE "{"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "q"
              PIPE "|"
              FIELD
                NAME_REF
                  LIDENT "age"
                EQ "="
                LITERAL
                  INT "0"
              R_BRACE "}"
    "#);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn a_constructor_followed_by_an_update_reads_the_update_as_an_argument() {
    // `Some { p | age = 1 }` は `Some` に更新を渡す式である。`Some P { … }` は `Some (P { … })` と読む
    insta::assert_snapshot!(shape("x = Some { p | age = 1 }\ny = Some P { a = 1 }\nz = M.P { a }"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "x"
        EQ "="
        APP_EXPR
          PATH_EXPR
            PATH
              NAME_REF
                UIDENT "Some"
          UPDATE_EXPR
            L_BRACE "{"
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "p"
            PIPE "|"
            FIELD
              NAME_REF
                LIDENT "age"
              EQ "="
              LITERAL
                INT "1"
            R_BRACE "}"
      EQUATION
        NAME
          LIDENT "y"
        EQ "="
        APP_EXPR
          PATH_EXPR
            PATH
              NAME_REF
                UIDENT "Some"
          RECORD_EXPR
            PATH
              NAME_REF
                UIDENT "P"
            L_BRACE "{"
            FIELD
              NAME_REF
                LIDENT "a"
              EQ "="
              LITERAL
                INT "1"
            R_BRACE "}"
      EQUATION
        NAME
          LIDENT "z"
        EQ "="
        RECORD_EXPR
          PATH
            NAME_REF
              UIDENT "M"
            DOT "."
            NAME_REF
              UIDENT "P"
          L_BRACE "{"
          FIELD
            NAME_REF
              LIDENT "a"
          R_BRACE "}"
    "#);
}

#[test]
fn an_update_base_needs_parentheses_around_expression_forms() {
    assert_eq!(
        diagnostics("x = { match o with | A -> p | age = 1 }")[0],
        "E0012 1:7 `match` expression must be parenthesized here"
    );
    assert_eq!(diagnostics("x = { p | }"), ["E0011 1:11 expected a field"]);
}

#[test]
fn anonymous_records_are_syntax_errors_and_are_skipped() {
    assert_eq!(
        diagnostics("t : { name : String }\nt = 1"),
        ["E0011 1:5 expected a type"]
    );
    assert_eq!(diagnostics("x = { name = 1 }"), ["E0011 1:12 expected `|`"]);
    assert_eq!(diagnostics("f { a } = a"), ["E0011 1:3 expected a pattern"]);
}

#[test]
fn field_indices_are_plain_decimals() {
    assert_eq!(diagnostics("x = (t.0, t.0.1, (.1))"), Vec::<String>::new());
    assert_eq!(
        diagnostics("x = t.01"),
        ["E0011 1:7 expected a field index"]
    );
    assert_eq!(
        diagnostics("x = t.0x1"),
        ["E0011 1:7 expected a field index"]
    );
}

#[test]
fn multi_line_records_parse_in_every_position() {
    let text = lines(&[
        "f u =",
        "  let p = P {",
        "    name =",
        "      long_name u,",
        "    on_save = fn e ->",
        "      log e,",
        "    age = 3,",
        "  }",
        "  let q = match p with",
        "    | P { name = \"a\" } -> P {",
        "        name,",
        "        age = 1,",
        "      }",
        "    | _ ->",
        "      P {",
        "        name = \"b\",",
        "      }",
        "  g P {",
        "      name = \"a\",",
        "    } (match p with",
        "      | P { name } -> name)",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn record_patterns_in_operator_equations_and_top_level_bindings() {
    assert_eq!(diagnostics("P { a } <+> q = a"), Vec::<String>::new());
    assert_eq!(
        diagnostics("P { a } = x")[0],
        "E0011 1:9 top-level pattern bindings are not allowed"
    );
}

#[test]
fn a_brace_counts_as_one_nesting_level() {
    // `(` と同じく1段に数え、フィールドの数は数えない
    let fields: Vec<String> = (0..300).map(|i| format!("f{i} = 0")).collect();
    assert_eq!(
        diagnostics(&format!("x = P {{ {} }}", fields.join(", "))),
        Vec::<String>::new()
    );
}

#[test]
fn an_update_bar_at_the_statement_column_leaves_the_brace_unclosed() {
    // 文と同じ列の行は括弧を閉じるので (docs/spec/layout.md の規則 2)、閉じ括弧のない `(` と同じ形の誤りになる
    let text = lines(&["f =", "  let q = { p", "  | age = 1 }", "  q"]);
    assert_eq!(
        diagnostics(&text),
        ["E0011 2:14 expected `|`", "E0011 3:3 expected a statement"]
    );
}

#[test]
fn trailing_commas_are_allowed_in_declarations_patterns_and_updates() {
    assert_eq!(
        diagnostics("data P =\n  | P { a : Int, }"),
        Vec::<String>::new()
    );
    assert_eq!(
        diagnostics("f p = match p with\n  | P { a, } -> a"),
        Vec::<String>::new()
    );
    assert_eq!(diagnostics("x = { p | a = 1, }"), Vec::<String>::new());
}

#[test]
fn a_comma_or_an_equals_sign_after_the_brace_is_read_as_a_record() {
    for text in ["x = C { , }", "x = P { = 1, a = 2 }"] {
        assert_eq!(diagnostics(text), ["E0011 1:9 expected a field"], "{text}");
        let tree = shape(text);
        assert!(tree.contains("RECORD_EXPR"), "{tree}");
        assert!(!tree.contains("UPDATE_EXPR"), "{tree}");
    }
}

#[test]
fn records_may_be_written_with_leading_commas() {
    let text = lines(&[
        "data P =",
        "  | P",
        "    { name : String",
        "    , age : Int",
        "    }",
        "",
        "p =",
        "  P",
        "    { name = \"a\"",
        "    , age = 3",
        "    }",
        "",
        "q =",
        "  { p",
        "    | name = fn x ->",
        "        x",
        "    , age = 4",
        "    }",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

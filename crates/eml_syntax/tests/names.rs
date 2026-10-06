mod common;

use common::shape;

#[test]
fn qualified_and_plain_names_are_paths_of_name_refs() {
    insta::assert_snapshot!(shape("f = Csv.parse x Foo.Bar.Baz y"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        EQ "="
        APP_EXPR
          PATH_EXPR
            PATH
              NAME_REF
                UIDENT "Csv"
              DOT "."
              NAME_REF
                LIDENT "parse"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "x"
          PATH_EXPR
            PATH
              NAME_REF
                UIDENT "Foo"
              DOT "."
              NAME_REF
                UIDENT "Bar"
              DOT "."
              NAME_REF
                UIDENT "Baz"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "y"
    "#);
}

#[test]
fn types_patterns_and_effects_hold_paths() {
    insta::assert_snapshot!(shape("f : M.T Int -> <M.E Int, IO> Unit\nf (M.C x) = x"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          APP_TYPE
            PATH
              NAME_REF
                UIDENT "M"
              DOT "."
              NAME_REF
                UIDENT "T"
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
                  UIDENT "M"
                DOT "."
                NAME_REF
                  UIDENT "E"
              PATH_TYPE
                PATH
                  NAME_REF
                    UIDENT "Int"
            COMMA ","
            EFFECT
              PATH
                NAME_REF
                  UIDENT "IO"
            R_ANGLE ">"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Unit"
      EQUATION
        LIDENT "f"
        PAREN_PAT
          L_PAREN "("
          CON_PAT
            PATH
              NAME_REF
                UIDENT "M"
              DOT "."
              NAME_REF
                UIDENT "C"
            BIND_PAT
              LIDENT "x"
          R_PAREN ")"
        EQ "="
        PATH_EXPR
          PATH
            NAME_REF
              LIDENT "x"
    "#);
}

#[test]
fn a_clause_names_its_operation_with_a_name_ref() {
    insta::assert_snapshot!(shape("f = handle g () with\n  | get () k -> resume k 1"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        EQ "="
        HANDLE_EXPR
          HANDLE_KW "handle"
          APP_EXPR
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "g"
            UNIT_EXPR
              L_PAREN "("
              R_PAREN ")"
          WITH_KW "with"
          OP_CLAUSE
            PIPE "|"
            NAME_REF
              LIDENT "get"
            UNIT_PAT
              L_PAREN "("
              R_PAREN ")"
            BIND_PAT
              LIDENT "k"
            THIN_ARROW "->"
            RESUME_EXPR
              RESUME_KW "resume"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "k"
              LITERAL
                INT "1"
    "#);
}

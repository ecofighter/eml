mod common;

use common::shape;

#[test]
fn qualified_and_plain_names_are_paths_of_name_refs() {
    insta::assert_snapshot!(shape("f = Csv.parse x Foo.Bar.Baz y"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
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
        NAME
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
        NAME
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
              NAME
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
        NAME
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
              NAME
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

#[test]
fn definitions_hold_names() {
    let text = "infixl 6 <+>\n(<+>) : Int -> Int -> Int\na <+> b = a\ndata P a = | P a | a :* a\neffect E s where\n  get : Unit -> s\nf x = x";
    insta::assert_snapshot!(shape(text), @r#"
    SOURCE_FILE
      FIXITY_ITEM
        INFIXL_KW "infixl"
        INT "6"
        NAME
          OP "<+>"
      SIGNATURE
        NAME
          L_PAREN "("
          OP "<+>"
          R_PAREN ")"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
            THIN_ARROW "->"
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
      EQUATION
        BIND_PAT
          NAME
            LIDENT "a"
        NAME
          OP "<+>"
        BIND_PAT
          NAME
            LIDENT "b"
        EQ "="
        PATH_EXPR
          PATH
            NAME_REF
              LIDENT "a"
      DATA_ITEM
        DATA_KW "data"
        NAME
          UIDENT "P"
        NAME
          LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          NAME
            UIDENT "P"
          VAR_TYPE
            LIDENT "a"
        ALT
          PIPE "|"
          VAR_TYPE
            LIDENT "a"
          NAME
            CONOP ":*"
          VAR_TYPE
            LIDENT "a"
      EFFECT_ITEM
        EFFECT_KW "effect"
        NAME
          UIDENT "E"
        NAME
          LIDENT "s"
        WHERE_KW "where"
        OP_DECL
          NAME
            LIDENT "get"
          COLON ":"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Unit"
            THIN_ARROW "->"
            VAR_TYPE
              LIDENT "s"
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "x"
        EQ "="
        PATH_EXPR
          PATH
            NAME_REF
              LIDENT "x"
    "#);
}

#[test]
fn minus_can_be_defined_as_an_operator() {
    insta::assert_snapshot!(shape("(-) : Int -> Int -> Int\na - b = a"), @r#"
    SOURCE_FILE
      SIGNATURE
        NAME
          L_PAREN "("
          MINUS "-"
          R_PAREN ")"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
            THIN_ARROW "->"
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
      EQUATION
        BIND_PAT
          NAME
            LIDENT "a"
        NAME
          MINUS "-"
        BIND_PAT
          NAME
            LIDENT "b"
        EQ "="
        PATH_EXPR
          PATH
            NAME_REF
              LIDENT "a"
    "#);
}

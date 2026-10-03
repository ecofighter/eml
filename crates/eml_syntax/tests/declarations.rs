mod common;

use common::{diagnostics, helps, lines, shape};

#[test]
fn signature_with_function_type() {
    insta::assert_snapshot!(shape("len : List a -> Int"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "len"
        COLON ":"
        FN_TYPE
          APP_TYPE
            UIDENT "List"
            VAR_TYPE
              LIDENT "a"
          THIN_ARROW "->"
          PATH_TYPE
            UIDENT "Int"
    "#);
}

#[test]
fn qualified_type_names() {
    insta::assert_snapshot!(shape("x : Option.Option Int"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "x"
        COLON ":"
        APP_TYPE
          UIDENT "Option"
          DOT "."
          UIDENT "Option"
          PATH_TYPE
            UIDENT "Int"
    "#);
}

#[test]
fn effect_row_with_effects_and_tail() {
    insta::assert_snapshot!(shape("f : a -> <IO, State s | e> b"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          VAR_TYPE
            LIDENT "a"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            EFFECT
              UIDENT "IO"
            COMMA ","
            EFFECT
              UIDENT "State"
              VAR_TYPE
                LIDENT "s"
            PIPE "|"
            LIDENT "e"
            R_ANGLE ">"
          VAR_TYPE
            LIDENT "b"
    "#);
}

#[test]
fn empty_row_is_split_from_one_operator_token() {
    insta::assert_snapshot!(shape("g : Unit -> <> Unit"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "g"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Unit"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            R_ANGLE ">"
          PATH_TYPE
            UIDENT "Unit"
    "#);
}

#[test]
fn row_variable_alone() {
    insta::assert_snapshot!(shape("h : Unit -> <e> Unit"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "h"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Unit"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            LIDENT "e"
            R_ANGLE ">"
          PATH_TYPE
            UIDENT "Unit"
    "#);
}

#[test]
fn operator_signature() {
    insta::assert_snapshot!(shape("(</>) : Path -> String -> Path"), @r#"
    SOURCE_FILE
      SIGNATURE
        L_PAREN "("
        OP "</>"
        R_PAREN ")"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Path"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              UIDENT "String"
            THIN_ARROW "->"
            PATH_TYPE
              UIDENT "Path"
    "#);
}

#[test]
fn arrows_at_the_end_of_lines_continue_the_type() {
    insta::assert_snapshot!(shape(&lines(&["f : Int ->", "  Int ->", "    Int"])), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Int"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              UIDENT "Int"
            THIN_ARROW "->"
            PATH_TYPE
              UIDENT "Int"
    "#);
}

#[test]
fn tuple_and_parenthesized_types() {
    insta::assert_snapshot!(shape("p : (Int, (String -> Int))"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "p"
        COLON ":"
        TUPLE_TYPE
          L_PAREN "("
          PATH_TYPE
            UIDENT "Int"
          COMMA ","
          PAREN_TYPE
            L_PAREN "("
            FN_TYPE
              PATH_TYPE
                UIDENT "String"
              THIN_ARROW "->"
              PATH_TYPE
                UIDENT "Int"
            R_PAREN ")"
          R_PAREN ")"
    "#);
}

#[test]
fn data_declarations() {
    let text = lines(&[
        "data Option a =",
        "  | None",
        "  | Some a",
        "data Color = | Red | Green",
        "data List a =",
        "  | Nil",
        "  | a :: List a",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      DATA_ITEM
        DATA_KW "data"
        UIDENT "Option"
        LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          UIDENT "None"
        ALT
          PIPE "|"
          UIDENT "Some"
          VAR_TYPE
            LIDENT "a"
      DATA_ITEM
        DATA_KW "data"
        UIDENT "Color"
        EQ "="
        ALT
          PIPE "|"
          UIDENT "Red"
        ALT
          PIPE "|"
          UIDENT "Green"
      DATA_ITEM
        DATA_KW "data"
        UIDENT "List"
        LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          UIDENT "Nil"
        ALT
          PIPE "|"
          VAR_TYPE
            LIDENT "a"
          CONOP "::"
          APP_TYPE
            UIDENT "List"
            VAR_TYPE
              LIDENT "a"
    "#);
}

#[test]
fn constructor_without_leading_pipe_is_an_error() {
    insta::assert_snapshot!(shape("data T = A | B"), @r#"
    SOURCE_FILE
      DATA_ITEM
        DATA_KW "data"
        UIDENT "T"
        EQ "="
        ALT
          UIDENT "A"
        ALT
          PIPE "|"
          UIDENT "B"
    ---
    E0011 1:10 expected `|` before the constructor
    "#);
}

#[test]
fn effect_declaration() {
    let text = lines(&[
        "effect State s where",
        "  get : Unit -> s",
        "  never fail : String -> a",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EFFECT_ITEM
        EFFECT_KW "effect"
        UIDENT "State"
        LIDENT "s"
        WHERE_KW "where"
        OP_DECL
          LIDENT "get"
          COLON ":"
          FN_TYPE
            PATH_TYPE
              UIDENT "Unit"
            THIN_ARROW "->"
            VAR_TYPE
              LIDENT "s"
        OP_DECL
          NEVER_KW "never"
          LIDENT "fail"
          COLON ":"
          FN_TYPE
            PATH_TYPE
              UIDENT "String"
            THIN_ARROW "->"
            VAR_TYPE
              LIDENT "a"
    "#);
}

#[test]
fn effect_operations_must_be_on_indented_lines() {
    insta::assert_snapshot!(shape("effect E where op : A"), @r#"
    SOURCE_FILE
      EFFECT_ITEM
        EFFECT_KW "effect"
        UIDENT "E"
        WHERE_KW "where"
      ERROR
        LIDENT "op"
        COLON ":"
        UIDENT "A"
    ---
    E0011 1:16 expected the operations on indented lines after `where`
    "#);
}

#[test]
fn fixity_declarations() {
    let text = lines(&["infixr 5 </>, ++", "infixl 6 -", "infix 4 ::"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      FIXITY_ITEM
        INFIXR_KW "infixr"
        INT "5"
        OP "</>"
        COMMA ","
        OP "++"
      FIXITY_ITEM
        INFIXL_KW "infixl"
        INT "6"
        MINUS "-"
      FIXITY_ITEM
        INFIX_KW "infix"
        INT "4"
        CONOP "::"
    "#);
}

#[test]
fn precedence_out_of_range_is_an_error() {
    assert_eq!(
        diagnostics("infixl 10 +"),
        ["E0011 1:8 precedence must be an integer from 0 to 9"]
    );
}

#[test]
fn pub_and_type_are_parsed_but_not_supported_yet() {
    insta::assert_snapshot!(shape("pub type Person = (String, Int)"), @r#"
    SOURCE_FILE
      TYPE_ITEM
        PUB_KW "pub"
        TYPE_KW "type"
        UIDENT "Person"
        EQ "="
        TUPLE_TYPE
          L_PAREN "("
          PATH_TYPE
            UIDENT "String"
          COMMA ","
          PATH_TYPE
            UIDENT "Int"
          R_PAREN ")"
    ---
    E0004 1:1 `pub` is not supported yet
    E0004 1:5 `type` declarations are not supported yet
    "#);
}

#[test]
fn import_and_records_are_skipped_as_not_supported_yet() {
    insta::assert_snapshot!(shape("import Report.Csv\nt : { name : String }"), @r#"
    SOURCE_FILE
      ERROR
        IMPORT_KW "import"
        UIDENT "Report"
        DOT "."
        UIDENT "Csv"
      SIGNATURE
        LIDENT "t"
        COLON ":"
        ERROR
          L_BRACE "{"
          LIDENT "name"
          COLON ":"
          UIDENT "String"
          R_BRACE "}"
    ---
    E0004 1:1 `import` is not supported yet
    E0004 2:5 records are not supported yet
    "#);
}

#[test]
fn errors_in_one_declaration_do_not_affect_the_next() {
    let text = lines(&["x : Int ->", "y : Int", "z : ) Int", "w : Int"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "x"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Int"
          THIN_ARROW "->"
      SIGNATURE
        LIDENT "y"
        COLON ":"
        PATH_TYPE
          UIDENT "Int"
      SIGNATURE
        LIDENT "z"
        COLON ":"
      ERROR
        R_PAREN ")"
        UIDENT "Int"
      SIGNATURE
        LIDENT "w"
        COLON ":"
        PATH_TYPE
          UIDENT "Int"
    ---
    E0009 1:9 expected an indented block after `->`
    E0011 3:5 expected a type
    "#);
}

#[test]
fn dot_with_spaces_in_a_qualified_name_is_an_error() {
    insta::assert_snapshot!(shape("f : Foo . Bar"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        PATH_TYPE
          UIDENT "Foo"
          DOT "."
          UIDENT "Bar"
    ---
    E0010 1:9 unexpected whitespace around `.`
    "#);
}

#[test]
fn reserved_keywords_are_errors() {
    assert_eq!(
        diagnostics("class Foo"),
        ["E0011 1:1 `class` is reserved for future use"]
    );
}

#[test]
fn lone_lowercase_name_is_not_an_item() {
    assert_eq!(diagnostics("foo"), ["E0003 1:1 expected an item"]);
}

#[test]
fn stray_unterminated_string_reports_both_problems() {
    assert_eq!(
        diagnostics("\"abc"),
        [
            "E0002 1:1 unterminated string literal",
            "E0003 1:1 expected an item"
        ]
    );
}

#[test]
fn virtual_tokens_are_described_without_articles() {
    let text = lines(&["f : Int ->", "    Int", "    Int"]);
    assert_eq!(diagnostics(&text), ["E0011 2:8 unexpected line break"]);
}

#[test]
fn unclosed_row_is_an_error() {
    assert_eq!(
        diagnostics("f : Int -> <IO Int"),
        ["E0011 1:19 expected `>`"]
    );
}

#[test]
fn row_with_something_other_than_an_effect_is_an_error() {
    assert_eq!(
        diagnostics("f : Int -> <1> Int"),
        ["E0011 1:13 expected an effect"]
    );
}

#[test]
fn constructor_must_have_a_name() {
    assert_eq!(
        diagnostics("data T = | 1"),
        ["E0011 1:12 expected a constructor"]
    );
    assert_eq!(
        diagnostics("data T = | a b"),
        ["E0011 1:14 expected a constructor name or an infix constructor"]
    );
}

#[test]
fn data_needs_constructors() {
    assert_eq!(
        diagnostics("data T = 1"),
        ["E0011 1:10 expected a constructor starting with `|`"]
    );
}

#[test]
fn fixity_needs_a_precedence() {
    assert_eq!(
        diagnostics("infixl +"),
        ["E0011 1:8 expected a precedence from 0 to 9"]
    );
}

#[test]
fn effect_body_lines_must_be_operations() {
    assert_eq!(
        diagnostics(&lines(&["effect E where", "  1"])),
        ["E0011 2:3 expected an operation signature"]
    );
}

#[test]
fn pub_without_an_item_is_an_error() {
    assert_eq!(
        diagnostics("pub"),
        [
            "E0004 1:1 `pub` is not supported yet",
            "E0003 1:4 expected an item"
        ]
    );
}

#[test]
fn aligned_signature_lines_are_one_error() {
    insta::assert_snapshot!(shape(&lines(&["f : Int ->", "  Int ->", "  Int"])), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Int"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              UIDENT "Int"
            THIN_ARROW "->"
            PATH_TYPE
              UIDENT "Int"
    ---
    E0009 2:7 expected an indented block after `->`
    "#);
}

#[test]
fn many_aligned_signature_lines_are_still_one_error() {
    assert_eq!(
        diagnostics(&lines(&["f : A ->", "  B ->", "  C ->", "  D"])),
        ["E0009 2:5 expected an indented block after `->`"]
    );
}

#[test]
fn arrow_at_the_end_of_a_line_suggests_a_leading_arrow() {
    assert_eq!(
        helps(&lines(&["f : Int ->", "  Int ->", "  Int"])),
        ["in a type that spans lines, put `->` at the start of the next line"]
    );
}

use crate::common::{diagnostics, item_kinds, lines, shape};

#[test]
fn operator_signature() {
    insta::assert_snapshot!(shape("(</>) : Path -> String -> Path"), @r#"
    SOURCE_FILE
      SIGNATURE
        NAME
          L_PAREN "("
          OP "</>"
          R_PAREN ")"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Path"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "String"
            THIN_ARROW "->"
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Path"
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
        NAME
          UIDENT "Option"
        NAME
          LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          NAME
            UIDENT "None"
        ALT
          PIPE "|"
          NAME
            UIDENT "Some"
          VAR_TYPE
            LIDENT "a"
      DATA_ITEM
        DATA_KW "data"
        NAME
          UIDENT "Color"
        EQ "="
        ALT
          PIPE "|"
          NAME
            UIDENT "Red"
        ALT
          PIPE "|"
          NAME
            UIDENT "Green"
      DATA_ITEM
        DATA_KW "data"
        NAME
          UIDENT "List"
        NAME
          LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          NAME
            UIDENT "Nil"
        ALT
          PIPE "|"
          VAR_TYPE
            LIDENT "a"
          NAME
            CONOP "::"
          APP_TYPE
            PATH
              NAME_REF
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
        NAME
          UIDENT "T"
        EQ "="
        ALT
          NAME
            UIDENT "A"
        ALT
          PIPE "|"
          NAME
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
        NAME
          UIDENT "State"
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
        OP_DECL
          NEVER_KW "never"
          NAME
            LIDENT "fail"
          COLON ":"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
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
        NAME
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
        NAME
          OP "</>"
        COMMA ","
        NAME
          OP "++"
      FIXITY_ITEM
        INFIXL_KW "infixl"
        INT "6"
        NAME
          MINUS "-"
      FIXITY_ITEM
        INFIX_KW "infix"
        INT "4"
        NAME
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
fn pub_and_type_are_parsed() {
    insta::assert_snapshot!(shape("pub type Person = (String, Int)"), @r#"
    SOURCE_FILE
      TYPE_ITEM
        PUB_KW "pub"
        TYPE_KW "type"
        NAME
          UIDENT "Person"
        EQ "="
        TUPLE_TYPE
          L_PAREN "("
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "String"
          COMMA ","
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
          R_PAREN ")"
    "#);
}

#[test]
fn import_is_parsed_and_record_types_are_errors() {
    insta::assert_snapshot!(shape("import Report.Csv\nt : { name : String }"), @r#"
    SOURCE_FILE
      IMPORT_ITEM
        IMPORT_KW "import"
        PATH
          NAME_REF
            UIDENT "Report"
          DOT "."
          NAME_REF
            UIDENT "Csv"
      SIGNATURE
        NAME
          LIDENT "t"
        COLON ":"
        ERROR
          L_BRACE "{"
          LIDENT "name"
          COLON ":"
          UIDENT "String"
          R_BRACE "}"
    ---
    E0011 2:5 expected a type
    "#);
}

#[test]
fn errors_in_one_declaration_do_not_affect_the_next() {
    let text = lines(&["x : Int ->", "y : Int", "w : Int"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      SIGNATURE
        NAME
          LIDENT "x"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
          THIN_ARROW "->"
      SIGNATURE
        NAME
          LIDENT "y"
        COLON ":"
        PATH_TYPE
          PATH
            NAME_REF
              UIDENT "Int"
      SIGNATURE
        NAME
          LIDENT "w"
        COLON ":"
        PATH_TYPE
          PATH
            NAME_REF
              UIDENT "Int"
    ---
    E0009 1:9 expected an indented block after `->`
    "#);
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
fn data_missing_its_equals_sign_still_parses_the_constructors() {
    // `=` を書き忘れた形。選択肢を読むので、HIR はコンストラクタを引ける
    assert_eq!(diagnostics("data T | A | B"), ["E0011 1:8 expected `=`"]);
    assert_eq!(
        diagnostics(&lines(&["data T", "  | A", "  | B"])),
        ["E0011 2:3 expected `=`"]
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
fn pub_on_an_equation_is_an_error() {
    // `pub` は宣言に付ける (docs/spec/grammar.md の `item`)。等式の関数はシグネチャで公開する
    assert_eq!(
        diagnostics(&lines(&[
            "f : Int",
            "pub f = 1",
            "(<+>) : Int -> Int -> Int",
            "pub a <+> b = a"
        ])),
        [
            "E0011 2:1 `pub` cannot be written on an equation",
            "E0011 4:1 `pub` cannot be written on an equation",
        ]
    );
}

#[test]
fn pub_on_an_import_is_an_error() {
    assert_eq!(
        diagnostics("pub import M"),
        ["E0011 1:1 `pub` cannot be written on an import"]
    );
}

#[test]
fn pub_without_an_item_is_an_error() {
    assert_eq!(diagnostics("pub"), ["E0003 1:4 expected an item"]);
}

#[test]
fn infix_constructor_with_type_applications() {
    insta::assert_snapshot!(shape(&lines(&["data L a =", "  | Nil", "  | List a :: L a"])), @r#"
    SOURCE_FILE
      DATA_ITEM
        DATA_KW "data"
        NAME
          UIDENT "L"
        NAME
          LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          NAME
            UIDENT "Nil"
        ALT
          PIPE "|"
          APP_TYPE
            PATH
              NAME_REF
                UIDENT "List"
            VAR_TYPE
              LIDENT "a"
          NAME
            CONOP "::"
          APP_TYPE
            PATH
              NAME_REF
                UIDENT "L"
            VAR_TYPE
              LIDENT "a"
    "#);
}

#[test]
fn data_without_constructors_is_parsed() {
    insta::assert_snapshot!(shape("data Int\npub data File"), @r#"
    SOURCE_FILE
      DATA_ITEM
        DATA_KW "data"
        NAME
          UIDENT "Int"
      DATA_ITEM
        PUB_KW "pub"
        DATA_KW "data"
        NAME
          UIDENT "File"
    "#);
}

#[test]
fn extern_declarations() {
    insta::assert_snapshot!(shape(&lines(&[
        "pub extern println : String -> <IO> Unit",
        "pub extern data Int",
        "pub extern effect IO",
    ])), @r#"
    SOURCE_FILE
      SIGNATURE
        PUB_KW "pub"
        EXTERN_KW "extern"
        NAME
          LIDENT "println"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "String"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            EFFECT
              PATH
                NAME_REF
                  UIDENT "IO"
            R_ANGLE ">"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Unit"
      DATA_ITEM
        PUB_KW "pub"
        EXTERN_KW "extern"
        DATA_KW "data"
        NAME
          UIDENT "Int"
      EFFECT_ITEM
        PUB_KW "pub"
        EXTERN_KW "extern"
        EFFECT_KW "effect"
        NAME
          UIDENT "IO"
    "#);
}

#[test]
fn extern_on_other_items_is_one_error_each() {
    let report: Vec<String> = [
        "extern type T = Int",
        "extern infixl 6 +",
        "extern data T = | A",
        "extern effect E where\n  op : Unit -> Unit",
        "extern data T a",
        "extern effect E a",
        "extern pub f : Int",
        "extern f x = x",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"extern type T = Int\" => [\"E0011 1:1 `extern` cannot be written on this item\"]",
        "\"extern infixl 6 +\" => [\"E0011 1:1 `extern` cannot be written on this item\"]",
        "\"extern data T = | A\" => [\"E0011 1:15 `extern data` cannot have parameters, `=` or `where`\"]",
        "\"extern effect E where\\n  op : Unit -> Unit\" => [\"E0011 1:17 `extern effect` cannot have parameters, `=` or `where`\"]",
        "\"extern data T a\" => [\"E0011 1:15 `extern data` cannot have parameters, `=` or `where`\"]",
        "\"extern effect E a\" => [\"E0011 1:17 `extern effect` cannot have parameters, `=` or `where`\"]",
        "\"extern pub f : Int\" => [\"E0011 1:1 `extern` cannot be written before `pub`\"]",
        "\"extern f x = x\" => [\"E0011 1:1 `extern` cannot be written on this item\"]",
    ]
    "#);
}

#[test]
fn misplaced_extern_is_one_error_at_extern() {
    let report: Vec<String> = [
        "extern",
        "extern\nf : Int",
        "extern data T U",
        "extern pub type T",
        "extern pub type T = Int",
        "pub extern f x = x",
        "pub extern import M",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"extern\" => [\"E0003 1:7 expected an item\"]",
        "\"extern\\nf : Int\" => [\"E0003 1:7 expected an item\"]",
        "\"extern data T U\" => [\"E0011 1:15 unexpected `U`\"]",
        "\"extern pub type T\" => [\"E0011 1:1 `extern` cannot be written before `pub`\", \"E0011 1:18 expected `=`\"]",
        "\"extern pub type T = Int\" => [\"E0011 1:1 `extern` cannot be written before `pub`\"]",
        "\"pub extern f x = x\" => [\"E0011 1:5 `extern` cannot be written on this item\"]",
        "\"pub extern import M\" => [\"E0011 1:5 `extern` cannot be written on this item\"]",
    ]
    "#);
    // 項目のない `extern` の後も、次の項目から読み直す
    assert_eq!(item_kinds("extern\nf : Int"), ["ERROR", "SIGNATURE"]);
}

#[test]
fn parameters_of_extern_data_are_still_names() {
    insta::assert_snapshot!(shape("extern data T a"), @r#"
    SOURCE_FILE
      DATA_ITEM
        EXTERN_KW "extern"
        DATA_KW "data"
        NAME
          UIDENT "T"
        NAME
          LIDENT "a"
    ---
    E0011 1:15 `extern data` cannot have parameters, `=` or `where`
    "#);
}

#[test]
fn extern_equation_is_still_an_equation() {
    assert_eq!(item_kinds("extern f x = x"), ["EQUATION"]);
}

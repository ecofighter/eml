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
fn an_operator_starting_with_an_arrow_and_an_angle_cannot_be_declared() {
    assert_eq!(
        diagnostics("infixl 5 -><")[0],
        "E0011 1:10 expected an operator"
    );
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
        "pub extern effect IO where\n  never io_failed : Int -> a",
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
        WHERE_KW "where"
        OP_DECL
          NEVER_KW "never"
          NAME
            LIDENT "io_failed"
          COLON ":"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
            THIN_ARROW "->"
            VAR_TYPE
              LIDENT "a"
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
        "\"extern data T = | A\" => [\"E0011 1:15 `extern data` cannot have `:`, `=` or `where`\"]",
        "\"extern effect E where\\n  op : Unit -> Unit\" => []",
        "\"extern data T a\" => []",
        "\"extern effect E a\" => [\"E0011 1:17 `extern effect` cannot have parameters or `=`\"]",
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
fn parameters_of_extern_data_are_names() {
    insta::assert_snapshot!(shape("extern data T a"), @r#"
    SOURCE_FILE
      DATA_ITEM
        EXTERN_KW "extern"
        DATA_KW "data"
        NAME
          UIDENT "T"
        NAME
          LIDENT "a"
    "#);
}

#[test]
fn extern_equation_is_still_an_equation() {
    assert_eq!(item_kinds("extern f x = x"), ["EQUATION"]);
}

#[test]
fn deriving_may_be_written_with_leading_commas() {
    let text = lines(&[
        "data Color =",
        "  | Red",
        "  | Green",
        "  deriving",
        "    ( Eq",
        "    , Show",
        "    )",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn data_can_declare_row_parameters_and_a_kind() {
    insta::assert_snapshot!(shape("data Iter <e> a : Lin =\n  | Done\n  | More a (once Unit -> <e> Iter <e> a)"), @r#"
    SOURCE_FILE
      DATA_ITEM
        DATA_KW "data"
        NAME
          UIDENT "Iter"
        ROW_PARAM
          L_ANGLE "<"
          NAME
            LIDENT "e"
          R_ANGLE ">"
        NAME
          LIDENT "a"
        KIND_ANNOT
          COLON ":"
          NAME_REF
            UIDENT "Lin"
        EQ "="
        ALT
          PIPE "|"
          NAME
            UIDENT "Done"
        ALT
          PIPE "|"
          NAME
            UIDENT "More"
          VAR_TYPE
            LIDENT "a"
          PAREN_TYPE
            L_PAREN "("
            FN_TYPE
              ONCE_KW "once"
              PATH_TYPE
                PATH
                  NAME_REF
                    UIDENT "Unit"
              THIN_ARROW "->"
              EFFECT_ROW
                L_ANGLE "<"
                LIDENT "e"
                R_ANGLE ">"
              APP_TYPE
                PATH
                  NAME_REF
                    UIDENT "Iter"
                ROW_ARG
                  EFFECT_ROW
                    L_ANGLE "<"
                    LIDENT "e"
                    R_ANGLE ">"
                VAR_TYPE
                  LIDENT "a"
            R_PAREN ")"
    "#);
}

#[test]
fn a_row_parameter_is_one_name() {
    assert_eq!(
        diagnostics("data T <IO> = | T"),
        ["E0011 1:9 expected a row variable"]
    );
    assert_eq!(
        diagnostics("data T <e | f> = | T"),
        ["E0011 1:11 expected a row variable"]
    );
}

#[test]
fn extern_data_cannot_declare_a_kind() {
    assert_eq!(
        diagnostics("extern data File : Lin"),
        ["E0011 1:18 `extern data` cannot have `:`, `=` or `where`"]
    );
}

#[test]
fn abstract_data_and_effect() {
    insta::assert_snapshot!(shape(&lines(&[
        "pub abstract data Handle a = | Handle Int",
        "pub abstract effect Ping where\n  ping : Unit -> Int",
    ])), @r#"
    SOURCE_FILE
      DATA_ITEM
        PUB_KW "pub"
        ABSTRACT_KW "abstract"
        DATA_KW "data"
        NAME
          UIDENT "Handle"
        NAME
          LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          NAME
            UIDENT "Handle"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Int"
      EFFECT_ITEM
        PUB_KW "pub"
        ABSTRACT_KW "abstract"
        EFFECT_KW "effect"
        NAME
          UIDENT "Ping"
        WHERE_KW "where"
        OP_DECL
          NAME
            LIDENT "ping"
          COLON ":"
          FN_TYPE
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Unit"
            THIN_ARROW "->"
            PATH_TYPE
              PATH
                NAME_REF
                  UIDENT "Int"
    "#);
}

#[test]
fn misplaced_abstract_is_one_error_at_abstract() {
    let report: Vec<String> = [
        "abstract data T = | T",
        "pub abstract f : Int -> Int",
        "pub abstract extern data U",
        "abstract pub data V = | V",
        "pub abstract type W = Int",
        "pub abstract f x = x",
        "pub abstract import M",
        "pub abstract class C a",
        "abstract",
        "class C a where\n  abstract f : a -> Int",
        "instance C Int where\n  abstract f x = x",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"abstract data T = | T\" => [\"E0011 1:1 `abstract` is only allowed on `pub data` and `pub effect`\"]",
        "\"pub abstract f : Int -> Int\" => [\"E0011 1:5 `abstract` is only allowed on `pub data` and `pub effect`\"]",
        "\"pub abstract extern data U\" => [\"E0011 1:5 `abstract` is only allowed on `pub data` and `pub effect`\"]",
        "\"abstract pub data V = | V\" => [\"E0011 1:1 `abstract` cannot be written before `pub`\"]",
        "\"pub abstract type W = Int\" => [\"E0011 1:5 `abstract` is only allowed on `pub data` and `pub effect`\"]",
        "\"pub abstract f x = x\" => [\"E0011 1:1 `pub` cannot be written on an equation\", \"E0011 1:5 `abstract` is only allowed on `pub data` and `pub effect`\"]",
        "\"pub abstract import M\" => [\"E0011 1:1 `pub` cannot be written on an import\", \"E0011 1:5 `abstract` is only allowed on `pub data` and `pub effect`\"]",
        "\"pub abstract class C a\" => [\"E0011 1:5 `abstract` is only allowed on `pub data` and `pub effect`\"]",
        "\"abstract\" => [\"E0003 1:9 expected an item\"]",
        "\"class C a where\\n  abstract f : a -> Int\" => [\"E0011 2:3 `abstract` is only allowed on `pub data` and `pub effect`\"]",
        "\"instance C Int where\\n  abstract f x = x\" => [\"E0011 2:3 `abstract` is only allowed on `pub data` and `pub effect`\"]",
    ]
    "#);
    // `abstract pub` は `pub abstract` と読み直す
    assert_eq!(item_kinds("abstract pub data V = | V"), ["DATA_ITEM"]);
}

#[test]
fn extern_c_signature_with_attributes() {
    insta::assert_snapshot!(shape("pub extern \"C\" pure symbol \"cos\" cosine : Float -> Float"), @r#"
    SOURCE_FILE
      SIGNATURE
        PUB_KW "pub"
        EXTERN_KW "extern"
        ABI
          STRING_LIT
            STRING_START "\""
            STRING_TEXT "C"
            STRING_END "\""
        C_ATTR
          LIDENT "pure"
        C_ATTR
          LIDENT "symbol"
          STRING_LIT
            STRING_START "\""
            STRING_TEXT "cos"
            STRING_END "\""
        NAME
          LIDENT "cosine"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Float"
          THIN_ARROW "->"
          PATH_TYPE
            PATH
              NAME_REF
                UIDENT "Float"
    "#);
}

#[test]
fn extern_c_data_with_drop() {
    insta::assert_snapshot!(shape("extern \"C\" data File drop \"fclose\"\nextern \"C\" data Handle"), @r#"
    SOURCE_FILE
      DATA_ITEM
        EXTERN_KW "extern"
        ABI
          STRING_LIT
            STRING_START "\""
            STRING_TEXT "C"
            STRING_END "\""
        DATA_KW "data"
        NAME
          UIDENT "File"
        C_DROP
          DROP_KW "drop"
          STRING_LIT
            STRING_START "\""
            STRING_TEXT "fclose"
            STRING_END "\""
      DATA_ITEM
        EXTERN_KW "extern"
        ABI
          STRING_LIT
            STRING_START "\""
            STRING_TEXT "C"
            STRING_END "\""
        DATA_KW "data"
        NAME
          UIDENT "Handle"
    "#);
    // `extern "C"` の後に書けるのは署名と型引数のない `data` だけで、ほかの形は E0011 である
    let report: Vec<String> = [
        "extern \"C\" data T a drop \"f\"",
        "extern \"C\" effect E where\n  op : Unit -> Unit",
        "extern \"C\" symbol \"e\" effect E where\n  op : Unit -> Unit",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"extern \\\"C\\\" data T a drop \\\"f\\\"\" => [\"E0011 1:19 `extern \\\"C\\\" data` cannot have type parameters\"]",
        "\"extern \\\"C\\\" effect E where\\n  op : Unit -> Unit\" => [\"E0011 1:1 `extern` cannot be written on this item\"]",
        "\"extern \\\"C\\\" symbol \\\"e\\\" effect E where\\n  op : Unit -> Unit\" => [\"E0011 1:1 `extern` cannot be written on this item\"]",
    ]
    "#);
}

#[test]
fn link_items_named_framework_and_path() {
    let text = lines(&[
        "link on linux \"libm.so.6\"",
        "link on macos framework \"Accelerate\"",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      LINK_ITEM
        LIDENT "link"
        LIDENT "on"
        LIDENT "linux"
        STRING_LIT
          STRING_START "\""
          STRING_TEXT "libm.so.6"
          STRING_END "\""
      LINK_ITEM
        LIDENT "link"
        LIDENT "on"
        LIDENT "macos"
        LIDENT "framework"
        STRING_LIT
          STRING_START "\""
          STRING_TEXT "Accelerate"
          STRING_END "\""
    "#);
}

#[test]
fn a_link_shaped_line_is_a_link_item() {
    // `link on <ident>` の後が文字列か `framework` のときだけ `link` の item で、ほかは等式のまま読む
    assert_eq!(item_kinds("link on linux \"m\""), ["LINK_ITEM"]);
    // `link` の item は宣言に数えないので、後ろに import を書ける
    assert_eq!(
        diagnostics("link on linux \"m\"\nimport M"),
        Vec::<String>::new()
    );
    assert_eq!(item_kinds("link on os = 1"), ["EQUATION"]);
    assert_eq!(item_kinds("link on os x = 1"), ["EQUATION"]);
    assert_eq!(item_kinds("link x = 1"), ["EQUATION"]);
    // `link` の item は最上位にだけあるので、クラスと instance の中では等式のままである
    let members = lines(&[
        "class C a where",
        "  link : a -> a",
        "  link on linux \"a\" = on",
        "instance C Int where",
        "  link on linux \"a\" = on",
    ]);
    assert_eq!(diagnostics(&members), Vec::<String>::new());
    let report: Vec<String> = ["link on os \"a\" = 1", "link on os framework = 1"]
        .iter()
        .map(|text| format!("{text:?} => {:?} {:?}", item_kinds(text), diagnostics(text)))
        .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"link on os \\\"a\\\" = 1\" => [\"LINK_ITEM\"] [\"E0018 1:16 a `link` item is `link on <os> \\\"<library>\\\"` or `link on macos framework \\\"<name>\\\"`\"]",
        "\"link on os framework = 1\" => [\"LINK_ITEM\"] [\"E0018 1:22 a `link` item is `link on <os> \\\"<library>\\\"` or `link on macos framework \\\"<name>\\\"`\"]",
    ]
    "#);
}

#[test]
fn pure_is_still_a_variable_name() {
    let text = lines(&[
        "pure : Int -> Int",
        "pure x = x",
        "symbol = pure 1",
        "blocking = symbol",
        "extern pure : Int",
        "extern \"C\" pure pure : Float -> Float",
    ]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
    assert_eq!(
        item_kinds(&text),
        [
            "SIGNATURE",
            "EQUATION",
            "EQUATION",
            "EQUATION",
            "SIGNATURE",
            "SIGNATURE"
        ]
    );
}

#[test]
fn duplicate_attribute_is_e0018() {
    let report: Vec<String> = [
        "extern \"C\" pure pure f : Int -> Int",
        "extern \"C\" symbol \"a\" symbol \"b\" f : Int -> Int",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"extern \\\"C\\\" pure pure f : Int -> Int\" => [\"E0018 1:17 `pure` is written twice\"]",
        "\"extern \\\"C\\\" symbol \\\"a\\\" symbol \\\"b\\\" f : Int -> Int\" => [\"E0018 1:23 `symbol` is written twice\"]",
    ]
    "#);
}

#[test]
fn pure_with_blocking_is_e0018() {
    let report: Vec<String> = [
        "extern \"C\" pure blocking f : Int -> Int",
        "extern \"C\" blocking symbol \"g\" pure f : Int -> Int",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"extern \\\"C\\\" pure blocking f : Int -> Int\" => [\"E0018 1:17 `pure` and `blocking` cannot both be written\"]",
        "\"extern \\\"C\\\" blocking symbol \\\"g\\\" pure f : Int -> Int\" => [\"E0018 1:32 `pure` and `blocking` cannot both be written\"]",
    ]
    "#);
}

#[test]
fn symbol_without_a_string_is_e0018() {
    let report: Vec<String> = [
        "extern \"C\" symbol f : Int -> Int",
        "extern \"C\" data T drop",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"extern \\\"C\\\" symbol f : Int -> Int\" => [\"E0018 1:19 `symbol` needs a string\"]",
        "\"extern \\\"C\\\" data T drop\" => [\"E0018 1:23 `drop` needs a string\"]",
    ]
    "#);
}

#[test]
fn interpolated_c_name_is_e0018() {
    let report: Vec<String> = [
        "extern \"C\" symbol \"f\\{x}\" f : Int -> Int",
        "extern \"C\" data T drop \"\\{x}\"",
        "link on linux \"lib\\{x}.so\"",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"extern \\\"C\\\" symbol \\\"f\\\\{x}\\\" f : Int -> Int\" => [\"E0018 1:21 a C name is a plain string\"]",
        "\"extern \\\"C\\\" data T drop \\\"\\\\{x}\\\"\" => [\"E0018 1:25 a C name is a plain string\"]",
        "\"link on linux \\\"lib\\\\{x}.so\\\"\" => [\"E0018 1:19 a C name is a plain string\"]",
    ]
    "#);
}

#[test]
fn malformed_link_is_e0018() {
    let report: Vec<String> = [
        "link on macos framework",
        "link on linux \"a\" \"b\"",
        "pub link on linux \"a\"",
        "extern link on linux \"a\"",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"link on macos framework\" => [\"E0018 1:24 a `link` item is `link on <os> \\\"<library>\\\"` or `link on macos framework \\\"<name>\\\"`\"]",
        "\"link on linux \\\"a\\\" \\\"b\\\"\" => [\"E0018 1:19 a `link` item is `link on <os> \\\"<library>\\\"` or `link on macos framework \\\"<name>\\\"`\"]",
        "\"pub link on linux \\\"a\\\"\" => [\"E0011 1:1 `pub` cannot be written on a `link` item\"]",
        "\"extern link on linux \\\"a\\\"\" => [\"E0011 1:1 `extern` cannot be written on this item\"]",
    ]
    "#);
}

#[test]
fn attributes_without_an_abi_are_e0018() {
    let report: Vec<String> = [
        "extern pure f : Int -> Int",
        "extern data T drop \"f\"",
        "extern \"C\" pure data T",
    ]
    .iter()
    .map(|text| format!("{text:?} => {:?}", diagnostics(text)))
    .collect();
    insta::assert_debug_snapshot!(report, @r#"
    [
        "\"extern pure f : Int -> Int\" => [\"E0018 1:8 attributes need `extern \\\"C\\\"`\"]",
        "\"extern data T drop \\\"f\\\"\" => [\"E0018 1:15 attributes need `extern \\\"C\\\"`\"]",
        "\"extern \\\"C\\\" pure data T\" => [\"E0018 1:12 `pure` is only allowed on an `extern \\\"C\\\"` function\"]",
    ]
    "#);
}

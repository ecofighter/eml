use crate::common::{diagnostics, item_kinds};
use eml_diagnostics::render;
use eml_syntax::debug_tree;
use eml_test_support::{parse, with_diagnostics};

/// lossless の確認は `eml_test_support::parse` が行う。
fn dump(text: &str) -> String {
    let parsed = parse(text);
    with_diagnostics(
        debug_tree(&parsed.parse.syntax()),
        &render(&parsed.diagnostics, &parsed.files),
    )
}

#[test]
fn empty_file() {
    insta::assert_snapshot!(dump(""), @"SOURCE_FILE@0..0");
}

#[test]
fn trivia_only_file_has_no_errors() {
    insta::assert_snapshot!(dump("-- only a comment\n\n"), @r#"
    SOURCE_FILE@0..19
      COMMENT@0..17 "-- only a comment"
      WHITESPACE@17..19 "\n\n"
    "#);
}

#[test]
fn stray_tokens_are_one_error_until_the_next_item() {
    insta::assert_snapshot!(dump("1 2 3"), @r#"
    SOURCE_FILE@0..5
      ERROR@0..5
        INT@0..1 "1"
        WHITESPACE@1..2 " "
        INT@2..3 "2"
        WHITESPACE@3..4 " "
        INT@4..5 "3"
    ---
    [E0003] Error: expected an item
       ╭─[ test.em:1:1 ]
       │
     1 │ 1 2 3
       │ ┬  
       │ ╰── not the start of an item
    ───╯
    "#);
}

#[test]
fn lexer_errors_are_not_reported_twice() {
    let diagnostics = parse("€ x").diagnostics;
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    // `€` は字句解析の E0001 だけ。続く `x` は項目ではないので E0003 を1件出す。
    assert_eq!(codes, ["E0001", "E0003"]);
    assert_eq!(u32::from(diagnostics[1].primary.range.start()), 4);
}

#[test]
fn recovery_resumes_at_the_next_item() {
    // 1つの項目の中のエラーは、次の行の項目の解析に影響しない。
    let text = "a : Int -> )\nb : Int\nc : Int";
    let diagnostics = parse(text).diagnostics;
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    assert_eq!(codes, ["E0011"]);
    assert_eq!(u32::from(diagnostics[0].primary.range.start()), 11);
    assert_eq!(
        item_kinds(text),
        ["SIGNATURE", "ERROR", "SIGNATURE", "SIGNATURE"]
    );
}

/// パースが panic せずに終わり、木が元のテキストに戻ることだけを確かめる。
fn assert_parses_losslessly(text: &str) {
    parse(text);
}

#[test]
fn long_unclosed_paren_lookahead_does_not_hit_the_step_limit() {
    // 先読み走査 (`apat_len`) は、閉じ括弧を探して EOF まで読む。
    assert_parses_losslessly(&("(".to_string() + &"a ".repeat(600_000)));
}

#[test]
fn long_use_lookahead_does_not_hit_the_step_limit() {
    // 先読み走査 (`has_left_arrow`) は、`<-` を探して EOF まで読む。
    assert_parses_losslessly(&("f =\n  use g (".to_string() + &"a ".repeat(600_000)));
}

#[test]
fn recovered_empty_block_stays_on_its_line() {
    let tree = debug_tree(&parse("f =\ng = 1").parse.syntax());
    assert!(tree.contains("\n  EQUATION@0..3\n"), "{tree}");
    assert!(tree.contains("\n    BLOCK@3..3\n"), "{tree}");
}

#[test]
fn shebang_after_a_byte_order_mark_is_trivia() {
    // BOM は読み込み時に除くので (docs/spec/lexical.md)、その後の `#!` はファイルの先頭の shebang である。
    assert!(parse("\u{feff}#!x\ny = 1").diagnostics.is_empty());
    assert_eq!(item_kinds("\u{feff}#!x\ny = 1"), ["EQUATION"]);
}

#[test]
fn reserved_keywords_are_errors() {
    assert_eq!(
        diagnostics("forall a"),
        ["E0011 1:1 `forall` is reserved for future use"]
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
fn annotated_patterns_can_appear_in_any_pattern() {
    let text = "f (Some (x : Int)) = x\nk (x : Int) = x\ng = match y with\n  | ((a : Int), b) -> a\nh = let (z : Int) = 1 in z";
    let parsed = eml_test_support::parse_clean(text);
    insta::assert_snapshot!(eml_syntax::debug_tree(&parsed.parse.syntax()), @r#"
    SOURCE_FILE@0..106
      EQUATION@0..22
        NAME@0..1
          LIDENT@0..1 "f"
        WHITESPACE@1..2 " "
        PAREN_PAT@2..18
          L_PAREN@2..3 "("
          CON_PAT@3..17
            PATH@3..7
              NAME_REF@3..7
                UIDENT@3..7 "Some"
            WHITESPACE@7..8 " "
            ANNOT_PAT@8..17
              L_PAREN@8..9 "("
              BIND_PAT@9..10
                NAME@9..10
                  LIDENT@9..10 "x"
              WHITESPACE@10..11 " "
              COLON@11..12 ":"
              WHITESPACE@12..13 " "
              PATH_TYPE@13..16
                PATH@13..16
                  NAME_REF@13..16
                    UIDENT@13..16 "Int"
              R_PAREN@16..17 ")"
          R_PAREN@17..18 ")"
        WHITESPACE@18..19 " "
        EQ@19..20 "="
        WHITESPACE@20..21 " "
        PATH_EXPR@21..22
          PATH@21..22
            NAME_REF@21..22
              LIDENT@21..22 "x"
      WHITESPACE@22..23 "\n"
      EQUATION@23..38
        NAME@23..24
          LIDENT@23..24 "k"
        WHITESPACE@24..25 " "
        ANNOT_PAT@25..34
          L_PAREN@25..26 "("
          BIND_PAT@26..27
            NAME@26..27
              LIDENT@26..27 "x"
          WHITESPACE@27..28 " "
          COLON@28..29 ":"
          WHITESPACE@29..30 " "
          PATH_TYPE@30..33
            PATH@30..33
              NAME_REF@30..33
                UIDENT@30..33 "Int"
          R_PAREN@33..34 ")"
        WHITESPACE@34..35 " "
        EQ@35..36 "="
        WHITESPACE@36..37 " "
        PATH_EXPR@37..38
          PATH@37..38
            NAME_REF@37..38
              LIDENT@37..38 "x"
      WHITESPACE@38..39 "\n"
      EQUATION@39..79
        NAME@39..40
          LIDENT@39..40 "g"
        WHITESPACE@40..41 " "
        EQ@41..42 "="
        WHITESPACE@42..43 " "
        MATCH_EXPR@43..79
          MATCH_KW@43..48 "match"
          WHITESPACE@48..49 " "
          PATH_EXPR@49..50
            PATH@49..50
              NAME_REF@49..50
                LIDENT@49..50 "y"
          WHITESPACE@50..51 " "
          WITH_KW@51..55 "with"
          WHITESPACE@55..58 "\n  "
          MATCH_ARM@58..79
            PIPE@58..59 "|"
            WHITESPACE@59..60 " "
            TUPLE_PAT@60..74
              L_PAREN@60..61 "("
              ANNOT_PAT@61..70
                L_PAREN@61..62 "("
                BIND_PAT@62..63
                  NAME@62..63
                    LIDENT@62..63 "a"
                WHITESPACE@63..64 " "
                COLON@64..65 ":"
                WHITESPACE@65..66 " "
                PATH_TYPE@66..69
                  PATH@66..69
                    NAME_REF@66..69
                      UIDENT@66..69 "Int"
                R_PAREN@69..70 ")"
              COMMA@70..71 ","
              WHITESPACE@71..72 " "
              BIND_PAT@72..73
                NAME@72..73
                  LIDENT@72..73 "b"
              R_PAREN@73..74 ")"
            WHITESPACE@74..75 " "
            THIN_ARROW@75..77 "->"
            WHITESPACE@77..78 " "
            PATH_EXPR@78..79
              PATH@78..79
                NAME_REF@78..79
                  LIDENT@78..79 "a"
      WHITESPACE@79..80 "\n"
      EQUATION@80..106
        NAME@80..81
          LIDENT@80..81 "h"
        WHITESPACE@81..82 " "
        EQ@82..83 "="
        WHITESPACE@83..84 " "
        LET_EXPR@84..106
          LET_KW@84..87 "let"
          WHITESPACE@87..88 " "
          ANNOT_PAT@88..97
            L_PAREN@88..89 "("
            BIND_PAT@89..90
              NAME@89..90
                LIDENT@89..90 "z"
            WHITESPACE@90..91 " "
            COLON@91..92 ":"
            WHITESPACE@92..93 " "
            PATH_TYPE@93..96
              PATH@93..96
                NAME_REF@93..96
                  UIDENT@93..96 "Int"
            R_PAREN@96..97 ")"
          WHITESPACE@97..98 " "
          EQ@98..99 "="
          WHITESPACE@99..100 " "
          LITERAL@100..101
            INT@100..101 "1"
          WHITESPACE@101..102 " "
          IN_KW@102..104 "in"
          WHITESPACE@104..105 " "
          PATH_EXPR@105..106
            PATH@105..106
              NAME_REF@105..106
                LIDENT@105..106 "z"
    "#);
}

#[test]
fn a_class_has_a_context_a_variable_and_members() {
    let text = "class Eq a => Ord a where\n  compare : a -> a -> Ordering\n  (<) : a -> a -> Bool\n  x < y = lt x y";
    assert!(
        parse(text).diagnostics.is_empty(),
        "{:?}",
        parse(text).diagnostics
    );
    insta::assert_snapshot!(dump(text), @r#"
    SOURCE_FILE@0..96
      CLASS_ITEM@0..96
        CLASS_KW@0..5 "class"
        WHITESPACE@5..6 " "
        CONTEXT@6..13
          CONSTRAINT@6..10
            APP_TYPE@6..10
              PATH@6..8
                NAME_REF@6..8
                  UIDENT@6..8 "Eq"
              WHITESPACE@8..9 " "
              VAR_TYPE@9..10
                LIDENT@9..10 "a"
          WHITESPACE@10..11 " "
          FAT_ARROW@11..13 "=>"
        WHITESPACE@13..14 " "
        NAME@14..17
          UIDENT@14..17 "Ord"
        WHITESPACE@17..18 " "
        NAME@18..19
          LIDENT@18..19 "a"
        WHITESPACE@19..20 " "
        WHERE_KW@20..25 "where"
        WHITESPACE@25..28 "\n  "
        SIGNATURE@28..56
          NAME@28..35
            LIDENT@28..35 "compare"
          WHITESPACE@35..36 " "
          COLON@36..37 ":"
          WHITESPACE@37..38 " "
          FN_TYPE@38..56
            VAR_TYPE@38..39
              LIDENT@38..39 "a"
            WHITESPACE@39..40 " "
            THIN_ARROW@40..42 "->"
            WHITESPACE@42..43 " "
            FN_TYPE@43..56
              VAR_TYPE@43..44
                LIDENT@43..44 "a"
              WHITESPACE@44..45 " "
              THIN_ARROW@45..47 "->"
              WHITESPACE@47..48 " "
              PATH_TYPE@48..56
                PATH@48..56
                  NAME_REF@48..56
                    UIDENT@48..56 "Ordering"
        WHITESPACE@56..59 "\n  "
        SIGNATURE@59..79
          NAME@59..62
            L_PAREN@59..60 "("
            OP@60..61 "<"
            R_PAREN@61..62 ")"
          WHITESPACE@62..63 " "
          COLON@63..64 ":"
          WHITESPACE@64..65 " "
          FN_TYPE@65..79
            VAR_TYPE@65..66
              LIDENT@65..66 "a"
            WHITESPACE@66..67 " "
            THIN_ARROW@67..69 "->"
            WHITESPACE@69..70 " "
            FN_TYPE@70..79
              VAR_TYPE@70..71
                LIDENT@70..71 "a"
              WHITESPACE@71..72 " "
              THIN_ARROW@72..74 "->"
              WHITESPACE@74..75 " "
              PATH_TYPE@75..79
                PATH@75..79
                  NAME_REF@75..79
                    UIDENT@75..79 "Bool"
        WHITESPACE@79..82 "\n  "
        EQUATION@82..96
          BIND_PAT@82..83
            NAME@82..83
              LIDENT@82..83 "x"
          WHITESPACE@83..84 " "
          NAME@84..85
            OP@84..85 "<"
          WHITESPACE@85..86 " "
          BIND_PAT@86..87
            NAME@86..87
              LIDENT@86..87 "y"
          WHITESPACE@87..88 " "
          EQ@88..89 "="
          WHITESPACE@89..90 " "
          APP_EXPR@90..96
            PATH_EXPR@90..92
              PATH@90..92
                NAME_REF@90..92
                  LIDENT@90..92 "lt"
            WHITESPACE@92..93 " "
            PATH_EXPR@93..94
              PATH@93..94
                NAME_REF@93..94
                  LIDENT@93..94 "x"
            WHITESPACE@94..95 " "
            PATH_EXPR@95..96
              PATH@95..96
                NAME_REF@95..96
                  LIDENT@95..96 "y"
    "#);
}

#[test]
fn an_instance_has_a_context_a_class_a_head_and_members() {
    let text = "instance Show a => Show (Option a) where\n  show x = describe x\n  extern (==)";
    assert!(
        parse(text).diagnostics.is_empty(),
        "{:?}",
        parse(text).diagnostics
    );
    insta::assert_snapshot!(dump(text), @r#"
    SOURCE_FILE@0..76
      INSTANCE_ITEM@0..76
        INSTANCE_KW@0..8 "instance"
        WHITESPACE@8..9 " "
        CONTEXT@9..18
          CONSTRAINT@9..15
            APP_TYPE@9..15
              PATH@9..13
                NAME_REF@9..13
                  UIDENT@9..13 "Show"
              WHITESPACE@13..14 " "
              VAR_TYPE@14..15
                LIDENT@14..15 "a"
          WHITESPACE@15..16 " "
          FAT_ARROW@16..18 "=>"
        WHITESPACE@18..19 " "
        PATH@19..23
          NAME_REF@19..23
            UIDENT@19..23 "Show"
        WHITESPACE@23..24 " "
        PAREN_TYPE@24..34
          L_PAREN@24..25 "("
          APP_TYPE@25..33
            PATH@25..31
              NAME_REF@25..31
                UIDENT@25..31 "Option"
            WHITESPACE@31..32 " "
            VAR_TYPE@32..33
              LIDENT@32..33 "a"
          R_PAREN@33..34 ")"
        WHITESPACE@34..35 " "
        WHERE_KW@35..40 "where"
        WHITESPACE@40..43 "\n  "
        EQUATION@43..62
          NAME@43..47
            LIDENT@43..47 "show"
          WHITESPACE@47..48 " "
          BIND_PAT@48..49
            NAME@48..49
              LIDENT@48..49 "x"
          WHITESPACE@49..50 " "
          EQ@50..51 "="
          WHITESPACE@51..52 " "
          APP_EXPR@52..62
            PATH_EXPR@52..60
              PATH@52..60
                NAME_REF@52..60
                  LIDENT@52..60 "describe"
            WHITESPACE@60..61 " "
            PATH_EXPR@61..62
              PATH@61..62
                NAME_REF@61..62
                  LIDENT@61..62 "x"
        WHITESPACE@62..65 "\n  "
        EXTERN_METHOD@65..76
          EXTERN_KW@65..71 "extern"
          WHITESPACE@71..72 " "
          NAME@72..76
            L_PAREN@72..73 "("
            OP@73..75 "=="
            R_PAREN@75..76 ")"
    "#);
}

#[test]
fn a_signature_can_start_with_a_context() {
    for text in [
        "f : Eq a => a -> Bool",
        "f : (Eq a, Show b) => a -> b -> String",
        "f : (Eq a) => a -> Bool",
        "f : Eq a\n  => a -> Bool",
    ] {
        assert!(parse(text).diagnostics.is_empty(), "{text}");
        assert_eq!(item_kinds(text), ["SIGNATURE"], "{text}");
    }
    insta::assert_snapshot!(dump("f : (Eq a, Show b) => a -> b"), @r#"
    SOURCE_FILE@0..28
      SIGNATURE@0..28
        NAME@0..1
          LIDENT@0..1 "f"
        WHITESPACE@1..2 " "
        COLON@2..3 ":"
        WHITESPACE@3..4 " "
        CONTEXT@4..21
          L_PAREN@4..5 "("
          CONSTRAINT@5..9
            APP_TYPE@5..9
              PATH@5..7
                NAME_REF@5..7
                  UIDENT@5..7 "Eq"
              WHITESPACE@7..8 " "
              VAR_TYPE@8..9
                LIDENT@8..9 "a"
          COMMA@9..10 ","
          WHITESPACE@10..11 " "
          CONSTRAINT@11..17
            APP_TYPE@11..17
              PATH@11..15
                NAME_REF@11..15
                  UIDENT@11..15 "Show"
              WHITESPACE@15..16 " "
              VAR_TYPE@16..17
                LIDENT@16..17 "b"
          R_PAREN@17..18 ")"
          WHITESPACE@18..19 " "
          FAT_ARROW@19..21 "=>"
        WHITESPACE@21..22 " "
        FN_TYPE@22..28
          VAR_TYPE@22..23
            LIDENT@22..23 "a"
          WHITESPACE@23..24 " "
          THIN_ARROW@24..26 "->"
          WHITESPACE@26..27 " "
          VAR_TYPE@27..28
            LIDENT@27..28 "b"
    "#);
}

#[test]
fn deriving_follows_the_last_constructor_in_every_layout() {
    for text in [
        "data C = | A | B deriving (Eq, Show)",
        "data C = | A | B deriving Eq",
        "data C =\n  | A\n  | B\n  deriving (Eq, Show)",
        "data C =\n  | A\n  | B\n      deriving Eq",
        "data C =\n    | A\n    | B\n  deriving Eq",
    ] {
        assert!(
            parse(text).diagnostics.is_empty(),
            "{text}: {:?}",
            parse(text).diagnostics
        );
        assert_eq!(item_kinds(text), ["DATA_ITEM"], "{text}");
    }
    insta::assert_snapshot!(dump("data C =\n  | A\n  deriving (Eq, Prelude.Show)"), @r#"
    SOURCE_FILE@0..44
      DATA_ITEM@0..44
        DATA_KW@0..4 "data"
        WHITESPACE@4..5 " "
        NAME@5..6
          UIDENT@5..6 "C"
        WHITESPACE@6..7 " "
        EQ@7..8 "="
        WHITESPACE@8..11 "\n  "
        ALT@11..14
          PIPE@11..12 "|"
          WHITESPACE@12..13 " "
          NAME@13..14
            UIDENT@13..14 "A"
        WHITESPACE@14..17 "\n  "
        DERIVING@17..44
          DERIVING_KW@17..25 "deriving"
          WHITESPACE@25..26 " "
          L_PAREN@26..27 "("
          PATH@27..29
            NAME_REF@27..29
              UIDENT@27..29 "Eq"
          COMMA@29..30 ","
          WHITESPACE@30..31 " "
          PATH@31..43
            NAME_REF@31..38
              UIDENT@31..38 "Prelude"
            DOT@38..39 "."
            NAME_REF@39..43
              UIDENT@39..43 "Show"
          R_PAREN@43..44 ")"
    "#);
}

#[test]
fn a_constructor_after_deriving_is_an_error() {
    assert_eq!(
        diagnostics("data C =\n  | A\n  deriving Eq\n  | B"),
        ["E0011 4:3 a constructor cannot follow `deriving`"]
    );
}

#[test]
fn an_instance_cannot_be_public_or_have_signatures() {
    assert_eq!(
        diagnostics("pub instance Eq C"),
        ["E0011 1:1 `pub` cannot be written on an instance"]
    );
    assert_eq!(
        diagnostics("instance Eq C where\n  (==) : C -> C -> Bool\n  a == b = True"),
        ["E0011 2:3 an instance cannot have signatures"]
    );
    assert_eq!(
        diagnostics("class Eq a where\n  pub (==) : a -> a -> Bool"),
        ["E0011 2:3 `pub` cannot be written on a class member"]
    );
    assert_eq!(
        diagnostics("instance Eq C where\n  pub a == b = True\n  pub extern show"),
        [
            "E0011 2:3 `pub` cannot be written on an instance member",
            "E0011 3:3 `pub` cannot be written on an instance member",
        ]
    );
}

#[test]
fn a_fat_arrow_outside_a_context_is_an_error() {
    // `=>` は予約の記号なので、演算子の定義にも式にも書けない
    assert_eq!(diagnostics("x => y = x"), ["E0003 1:1 expected an item"]);
    assert_eq!(
        diagnostics("f : a -> (Eq a => a)"),
        ["E0011 1:16 expected `)`"]
    );
}

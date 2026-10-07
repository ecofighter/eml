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

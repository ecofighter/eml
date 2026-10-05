mod common;

use common::{diagnostics, item_kinds};
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
    let text = "f (Some (x : Int)) = x\ng = match y with\n  | ((a : Int), b) -> a\nh = let (z : Int) = 1 in z";
    let parsed = eml_test_support::parse_clean(text);
    let tree = eml_syntax::debug_tree(&parsed.parse.syntax());
    assert_eq!(tree.matches("ANNOT_PAT").count(), 3, "{tree}");
}

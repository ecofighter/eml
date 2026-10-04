use super::*;
use crate::debug_tree;
use crate::layout::layout;
use crate::lexer::lex;
use crate::sink::build_tree;
use crate::{SyntaxKind::*, SyntaxNode};
use eml_diagnostics::SourceFiles;

/// 本物の文法から独立して仕組みを試すため、小さな文法を引数で渡す。
fn run(text: &str, grammar: impl FnOnce(&mut Parser)) -> (String, Vec<Diagnostic>) {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (tokens, _) = lex(file, text);
    let (input, _) = layout(file, text, &tokens);
    let mut p = Parser::new(file, text, input);
    grammar(&mut p);
    let (events, diagnostics) = p.finish();
    let tree = SyntaxNode::new_root(build_tree(text, &tokens, events));
    assert_eq!(tree.text().to_string(), text, "tree must be lossless");
    (debug_tree(&tree), diagnostics)
}

#[test]
fn trivia_inside_root_goes_to_the_enclosing_node() {
    let (tree, _) = run(" a -- c\n b ", |p| {
        let root = p.start();
        let inner = p.start();
        p.bump_any();
        inner.complete(p, ERROR);
        p.bump_any();
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..11
      WHITESPACE@0..1 " "
      ERROR@1..2
        LIDENT@1..2 "a"
      WHITESPACE@2..3 " "
      COMMENT@3..7 "-- c"
      WHITESPACE@7..9 "\n "
      LIDENT@9..10 "b"
      WHITESPACE@10..11 " "
    "#);
}

#[test]
fn precede_wraps_a_completed_node() {
    let (tree, _) = run("a b", |p| {
        let root = p.start();
        let first = p.start();
        p.bump_any();
        let first = first.complete(p, ERROR);
        let outer = first.precede(p);
        p.bump_any();
        outer.complete(p, ERROR);
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..3
      ERROR@0..3
        ERROR@0..1
          LIDENT@0..1 "a"
        WHITESPACE@1..2 " "
        LIDENT@2..3 "b"
    "#);
}

#[test]
fn abandoned_marker_leaves_no_node() {
    let (tree, _) = run("a", |p| {
        let root = p.start();
        let unused = p.start();
        unused.abandon(p);
        p.bump_any();
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..1
      LIDENT@0..1 "a"
    "#);
}

#[test]
fn lookahead_skips_trivia_and_reports_eof() {
    run("a  -- c\n b", |p| {
        let root = p.start();
        assert_eq!(p.current(), LIDENT);
        assert_eq!(p.nth(1), LIDENT);
        assert_eq!(p.nth(2), EOF);
        assert!(!p.eat(UIDENT));
        assert!(p.eat(LIDENT));
        p.bump(LIDENT);
        assert!(p.at_eof());
        root.complete(p, SOURCE_FILE);
    });
}

#[test]
fn error_at_eof_points_after_the_last_token() {
    let (_, diagnostics) = run("a ", |p| {
        let root = p.start();
        p.bump_any();
        p.error(ErrorCode(9999), "expected more", "here");
        root.complete(p, SOURCE_FILE);
    });
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].primary.range,
        TextRange::empty(TextSize::new(1))
    );
}

#[test]
fn error_at_a_virtual_token_points_after_the_last_token() {
    let (_, diagnostics) = run("a =\n  b", |p| {
        let root = p.start();
        p.bump(LIDENT);
        p.bump(EQ);
        assert_eq!(p.current(), LAYOUT_OPEN);
        p.error(ErrorCode(9999), "expected more", "here");
        while !p.at_eof() {
            p.bump_any();
        }
        root.complete(p, SOURCE_FILE);
    });
    assert_eq!(
        diagnostics[0].primary.range,
        TextRange::empty(TextSize::new(3))
    );
}

#[test]
fn error_in_an_empty_file_points_at_the_start() {
    let (_, diagnostics) = run("", |p| {
        let root = p.start();
        p.error(ErrorCode(9999), "expected more", "here");
        root.complete(p, SOURCE_FILE);
    });
    assert_eq!(
        diagnostics[0].primary.range,
        TextRange::empty(TextSize::new(0))
    );
}

#[test]
fn virtual_tokens_make_no_tree_tokens() {
    let (tree, _) = run("f =\n  a", |p| {
        let root = p.start();
        let mut virtuals = 0;
        while !p.at_eof() {
            if p.current().is_virtual() {
                virtuals += 1;
            }
            p.bump_any();
        }
        assert_eq!(virtuals, 2);
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..7
      LIDENT@0..1 "f"
      WHITESPACE@1..2 " "
      EQ@2..3 "="
      WHITESPACE@3..6 "\n  "
      LIDENT@6..7 "a"
    "#);
}

#[test]
fn semicolon_is_a_separator() {
    run("a; b", |p| {
        let root = p.start();
        p.bump(LIDENT);
        assert!(p.at_sep());
        p.bump_any();
        p.bump(LIDENT);
        root.complete(p, SOURCE_FILE);
    });
}

#[test]
fn split_first_char_divides_an_operator_token() {
    let (tree, _) = run("<>->", |p| {
        let root = p.start();
        p.split_first_char(L_ANGLE);
        assert_eq!(p.current(), OP);
        assert_eq!(p.current_text(), ">->");
        p.split_first_char(R_ANGLE);
        assert_eq!(p.current(), THIN_ARROW);
        p.bump(THIN_ARROW);
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..4
      L_ANGLE@0..1 "<"
      R_ANGLE@1..2 ">"
      THIN_ARROW@2..4 "->"
    "#);
}

#[test]
fn split_prefix_divides_an_arrow_and_a_row() {
    let (tree, _) = run("-><", |p| {
        let root = p.start();
        p.split_prefix(THIN_ARROW, 2);
        assert_eq!(p.current(), OP);
        assert_eq!(p.current_text(), "<");
        p.bump_remap(L_ANGLE);
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..3
      THIN_ARROW@0..2 "->"
      L_ANGLE@2..3 "<"
    "#);
}

#[test]
fn bump_remap_changes_the_tree_kind() {
    let (tree, _) = run("<", |p| {
        let root = p.start();
        p.bump_remap(L_ANGLE);
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..1
      L_ANGLE@0..1 "<"
    "#);
}

#[test]
fn touching_tokens() {
    run("a.b c", |p| {
        let root = p.start();
        assert!(!p.touches_prev());
        assert!(p.touches_next());
        p.bump(LIDENT);
        assert!(p.touches_prev());
        assert!(p.touches_next());
        p.bump(DOT);
        assert!(!p.touches_next());
        p.bump(LIDENT);
        p.bump(LIDENT);
        root.complete(p, SOURCE_FILE);
    });
}

#[test]
fn precede_can_be_chained() {
    let (tree, _) = run("a b c", |p| {
        let root = p.start();
        let first = p.start();
        p.bump_any();
        let first = first.complete(p, ERROR);
        let second = first.precede(p);
        p.bump_any();
        let second = second.complete(p, ERROR);
        let third = second.precede(p);
        p.bump_any();
        third.complete(p, ERROR);
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..5
      ERROR@0..5
        ERROR@0..3
          ERROR@0..1
            LIDENT@0..1 "a"
          WHITESPACE@1..2 " "
          LIDENT@2..3 "b"
        WHITESPACE@3..4 " "
        LIDENT@4..5 "c"
    "#);
}

#[test]
fn abandoning_a_preceded_marker_keeps_the_child() {
    let (tree, _) = run("a b", |p| {
        let root = p.start();
        let first = p.start();
        p.bump_any();
        let first = first.complete(p, ERROR);
        let outer = first.precede(p);
        outer.abandon(p);
        let next = p.start();
        p.bump_any();
        next.complete(p, ERROR);
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..3
      ERROR@0..1
        LIDENT@0..1 "a"
      WHITESPACE@1..2 " "
      ERROR@2..3
        LIDENT@2..3 "b"
    "#);
}

#[test]
fn abandoning_a_marker_that_is_not_the_last_event() {
    let (tree, _) = run("a b", |p| {
        let root = p.start();
        let outer = p.start();
        let inner = p.start();
        p.bump_any();
        inner.complete(p, ERROR);
        outer.abandon(p);
        p.bump_any();
        root.complete(p, SOURCE_FILE);
    });
    insta::assert_snapshot!(tree, @r#"
    SOURCE_FILE@0..3
      ERROR@0..1
        LIDENT@0..1 "a"
      WHITESPACE@1..2 " "
      LIDENT@2..3 "b"
    "#);
}

#[test]
#[should_panic(expected = "marker must be completed or abandoned")]
fn forgotten_marker_panics() {
    run("a", |p| {
        let _forgotten = p.start();
    });
}

#[test]
#[should_panic(expected = "the parser seems stuck")]
fn parser_without_progress_panics() {
    run("a", |p| {
        loop {
            p.current();
        }
    });
}

#[test]
fn errors_at_the_same_position_are_reported_once() {
    let (_, diagnostics) = run("a", |p| {
        let root = p.start();
        p.error(ErrorCode(9999), "first", "here");
        p.error(ErrorCode(9998), "second", "here");
        p.bump_any();
        root.complete(p, SOURCE_FILE);
    });
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "first");
}

#[test]
fn no_error_is_reported_at_an_error_token() {
    let (_, diagnostics) = run("€", |p| {
        let root = p.start();
        p.error(ErrorCode(9999), "expected something", "here");
        p.bump_any();
        root.complete(p, SOURCE_FILE);
    });
    assert!(diagnostics.is_empty());
}

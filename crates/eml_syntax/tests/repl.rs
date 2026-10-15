//! REPL の入力の読み分けと、入力の終わりの判定 (docs/spec/grammar.md の「REPL の入力」と、docs/spec/layout.md の
//! 「REPL の入力の終わり」)。

use eml_syntax::SyntaxKind::{self, REPL_EXPR, REPL_ITEMS, REPL_LET};
use eml_syntax::{InputStatus, input_status, parse_input};
use eml_test_support::{short, source};

/// 根の種類と、構文の診断。木が元のテキストに戻ることもここで確かめる。
fn input(text: &str) -> (SyntaxKind, Vec<String>) {
    let (files, file) = source(text);
    let (parse, diagnostics) = parse_input(file, files.text(file));
    assert_eq!(
        parse.syntax().text().to_string(),
        text,
        "tree must be lossless"
    );
    (parse.syntax().kind(), short(&files, &diagnostics))
}

fn kind(text: &str) -> SyntaxKind {
    let (kind, diagnostics) = input(text);
    assert!(diagnostics.is_empty(), "{text:?}: {diagnostics:?}");
    kind
}

#[test]
fn declarations_and_signatures_are_items() {
    for text in [
        "data Color = | Red | Green",
        "data Color =\n  | Red\n  | Green",
        "effect Ask where\n  ask : Unit -> Int",
        "import Std.Fs",
        "infixl 6 <+>",
        "class Size a where\n  size : a -> Int",
        "instance Show Color where\n  show c = \"c\"",
        "f : Int -> Int",
        "(<+>) : Int -> Int -> Int",
        "pub f : Int -> Int",
    ] {
        assert_eq!(kind(text), REPL_ITEMS, "{text:?}");
    }
}

#[test]
fn equations_with_a_top_level_eq_are_items() {
    for text in [
        "f x = x + 1",
        "f : Int -> Int; f x = x + 1",
        "f : Int -> Int\nf 0 = 1\nf n = n",
        "f x =\n  x + 1",
        "a <+> b = a",
        "x = 3",
    ] {
        assert_eq!(kind(text), REPL_ITEMS, "{text:?}");
    }
}

#[test]
fn let_bindings_are_let_inputs() {
    for text in ["let x = 1", "let x : Int = 1", "let f =\n  fn y -> y"] {
        assert_eq!(kind(text), REPL_LET, "{text:?}");
    }
}

#[test]
fn everything_else_is_an_expression() {
    for text in [
        "f x",
        "1 + 2",
        "x",
        "f x == 3",
        "(+) 1 2",
        "let x = 1 in x + 1",
        "run (fn () ->\n    let y = 1\n    y)",
        // 括弧の中の `=` は、フィールドの `=` である
        "{ p | name = \"a\" }",
        "Person { name = \"a\" }",
        "\"a \\{b}\"",
        "match x with\n  | 1 -> 2\n  | _ -> 3",
        "Just 3",
        // 先頭の空行と字下げは、入力の種類を変えない
        "\n\nf x",
        "  1 + 2",
    ] {
        assert_eq!(kind(text), REPL_EXPR, "{text:?}");
    }
}

/// `->` でブロックが開いた後の `=` は、等式の `=` ではない。括弧のない `fn` は式の誤りとして報告され、等式の誤りには
/// ならない。
#[test]
fn an_eq_after_a_block_opens_is_not_an_equation() {
    assert_eq!(
        input("run <| fn () ->\n  let y = 1\n  y"),
        (
            REPL_EXPR,
            vec!["E0012 1:8 `fn` expression must be parenthesized here".to_string()]
        )
    );
}

#[test]
fn trivia_only_input_is_empty_items() {
    for text in ["", "  ", "-- only a comment", "\n\n"] {
        let (files, file) = source(text);
        let (parse, diagnostics) = parse_input(file, files.text(file));
        assert_eq!(parse.syntax().kind(), REPL_ITEMS);
        assert_eq!(parse.syntax().children().count(), 0, "{text:?}");
        assert!(diagnostics.is_empty());
    }
}

#[test]
fn a_let_or_expression_input_holds_one_item() {
    assert_eq!(
        input("1; 2"),
        (
            REPL_EXPR,
            vec!["E0011 1:4 an input holds only one `let` or expression".to_string()]
        )
    );
    assert_eq!(
        input("let x = 1\nlet y = 2"),
        (
            REPL_LET,
            vec!["E0011 2:1 an input holds only one `let` or expression".to_string()]
        )
    );
    // 末尾の `;` の後に何もなければ、2つ目の項目はない
    assert_eq!(kind("f x;"), REPL_EXPR);
}

#[test]
fn a_session_let_binds_one_name() {
    assert_eq!(
        input("let (a, b) = (1, 2)"),
        (
            REPL_LET,
            vec!["E0011 1:5 a `let` input binds one name".to_string()]
        )
    );
    // `in` が続けば式なので、パターンで束縛してよい
    assert_eq!(kind("let (a, b) = (1, 2) in a"), REPL_EXPR);
}

#[test]
fn the_let_input_tree_has_the_let_statement() {
    let (files, file) = source("let x : Int = 1");
    let (parse, _) = parse_input(file, files.text(file));
    let eml_syntax::ast::ReplInput::ReplLet(input) = parse.repl_input() else {
        panic!("expected a let input");
    };
    let stmt = input.stmt().expect("the let statement");
    assert!(stmt.ty().is_some() && stmt.body().is_some());
}

#[test]
fn the_expression_input_tree_has_the_expression() {
    let (files, file) = source("f x");
    let (parse, _) = parse_input(file, files.text(file));
    let eml_syntax::ast::ReplInput::ReplExpr(input) = parse.repl_input() else {
        panic!("expected an expression input");
    };
    assert_eq!(u32::from(input.expr().unwrap().range().end()), 3);
}

#[test]
fn the_items_input_tree_lists_the_items() {
    let (files, file) = source("f : Int -> Int; f x = x");
    let (parse, _) = parse_input(file, files.text(file));
    let eml_syntax::ast::ReplInput::ReplItems(input) = parse.repl_input() else {
        panic!("expected items");
    };
    assert_eq!(input.items().count(), 2);
}

/// 演算子のシグネチャは `(+)` の後に `:` が要る。要らないと、`(+) 1 2` がシグネチャとして読まれる。
#[test]
fn an_operator_signature_needs_the_colon() {
    assert_eq!(kind("(+) 1 2"), REPL_EXPR);
    assert_eq!(kind("(+) : Int -> Int -> Int"), REPL_ITEMS);
}

fn complete(text: &str) {
    assert_eq!(input_status(text), InputStatus::Complete, "{text:?}");
}

fn incomplete(text: &str) {
    assert_eq!(input_status(text), InputStatus::Incomplete, "{text:?}");
}

#[test]
fn one_line_inputs_are_complete() {
    for text in [
        "",
        "-- a comment",
        "f x",
        "1 + 2",
        "let x = 1",
        "import Std.Fs",
        "data Color = | Red | Green",
        "\n\nf x",
        // 閉じた括弧の中の字下げした行は、ブロックではない
        "(1,\n  2)",
        "let s = \"\"\"\n  a\n  \"\"\"",
    ] {
        complete(text);
    }
}

/// 規則 1。括弧と row、行をまたげる文字列が閉じていなければ、空行が来ても続く。
#[test]
fn open_brackets_rows_and_multiline_strings_continue() {
    for text in [
        "(1,",
        "[1,\n",
        "(1,\n\n",
        "{ p | name =",
        "f : Int -> <IO",
        "let s = \"\"\"\n  abc",
        "let s = \"\"\"\n  abc\n\n",
        "let s = r\"abc",
        "let s = r#\"a\nb\n\n",
    ] {
        incomplete(text);
    }
}

/// 1行で閉じる文字列と補間の穴は行をまたげないので、閉じていなくても入力を続けない。続けても閉じることがないため。
#[test]
fn single_line_strings_and_holes_do_not_continue() {
    complete("\"abc");
    complete("\"a \\{x");
}

/// 規則 2。
#[test]
fn a_block_opening_last_token_continues() {
    for text in [
        "f x =",
        "main () =\n",
        "if c then",
        "if c then 1 else",
        "handle e with",
        "fn x ->",
        "class Size a where",
    ] {
        incomplete(text);
    }
}

/// 規則 3。規則 3 のブロックの中の最後の行と、シグネチャか等式で終わる item の並びは、空行で終わる。
#[test]
fn indented_lines_and_definitions_end_at_a_blank_line() {
    for (text, ended) in [
        ("main () =\n  println 1", "main () =\n  println 1\n"),
        ("data Color =\n  | Red", "data Color =\n  | Red\n"),
        ("f : Int -> Int", "f : Int -> Int\n"),
        ("f : Int -> Int\nf 0 = 1", "f : Int -> Int\nf 0 = 1\n"),
        (
            "f : Int -> Int; f x = x + 1",
            "f : Int -> Int; f x = x + 1\n",
        ),
        ("f x = x", "f x = x\n   "),
        (
            "class Size a where\n  size : a -> Int",
            "class Size a where\n  size : a -> Int\n",
        ),
    ] {
        incomplete(text);
        complete(ended);
    }
}

/// 複数行の入力は、最後の行より前のどこで切っても `Incomplete` である。トランスクリプトの harness と同じ確かめである。
#[test]
fn every_prefix_of_a_multi_line_input_is_incomplete() {
    let lines = ["f : Int -> Int", "f 0 = 1", "f n = n * 2", ""];
    for end in 1..lines.len() {
        incomplete(&lines[..end].join("\n"));
    }
    complete(&lines.join("\n"));
}

/// 端末やエディタが行を `\r\n` で送っても、入力の終わりと種類の判定は `\n` と同じである。
#[test]
fn crlf_lines_end_inputs_like_lf() {
    incomplete("f : Int -> Int\r\nf 0 = 1");
    complete("f : Int -> Int\r\nf 0 = 1\r\n");
    incomplete("main () =\r\n  println 1");
    complete("main () =\r\n  println 1\r\n");
    assert_eq!(kind("f x = x\r\n"), REPL_ITEMS);
    assert_eq!(kind("f x\r\n"), REPL_EXPR);
}

//! タプルの式・型・パターンと、リテラルのパターンの変換。

use crate::common::{diagnostics, lower_text};
use eml_hir::{Body, ExprKind, LocalId, Program, Stmt};

#[test]
fn tuples_in_expressions_types_and_patterns() {
    let text = "swap : (Int, String) -> (String, Int)\nswap (n, s) = (s, n)\n\nnested : Int -> ((Int, Int), String)\nnested x =\n  let (a, b) = (x, x)\n  let f = fn (p, q) -> (q, p)\n  match f (a, b) with\n    | (c, d) -> ((c, d), \"x\")";
    insta::assert_snapshot!(lower_text(text), @r#"
    swap : (Int, String) -> (String, Int)
    swap (n#0, s#1) = (s#1, n#0)
    nested : Int -> ((Int, Int), String)
    nested x#0 = {
      let (a#1, b#2) = (x#0, x#0)
      let f#5 = (fn (p#3, q#4) -> (q#4, p#3))
      (match (f#5 (a#1, b#2)) with | (c#6, d#7) -> ((c#6, d#7), "x"))
    }
    "#);
}

#[test]
fn literal_patterns_include_negative_numbers_and_strings() {
    let text = "data Option a = | None | Some a\n\ndescribe : Int -> String\ndescribe n = match n with\n  | 0 -> \"zero\"\n  | -1 -> \"minus one\"\n  | _ -> \"other\"\n\ngreet : String -> Int\ngreet s = match s with\n  | \"hi\" -> 1\n  | \"\" -> 0\n  | other -> 2\n\nnested : Option Int -> Int\nnested o = match o with | Some -1 -> 1 | _ -> 0";
    insta::assert_snapshot!(lower_text(text), @r#"
    data Option a
      | None
      | Some a
    describe : Int -> String
    describe n#0 = (match n#0 with | 0 -> "zero" | -1 -> "minus one" | _ -> "other")
    greet : String -> Int
    greet s#0 = (match s#0 with | "hi" -> 1 | "" -> 0 | other#1 -> 2)
    nested : Main.Option Int -> Int
    nested o#0 = (match o#0 with | Some -1 -> 1 | _ -> 0)
    "#);
}

#[test]
fn a_literal_too_large_for_int_becomes_a_missing_pattern() {
    // `-` の後の数が `Int` に収まらなければ、式と同じく字句の段階の E0007 だけを出す
    let text = "f : Int -> Int\nf n = match n with | -9223372036854775808 -> 0 | _ -> 1";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f n#0 = (match n#0 with | <missing> -> 0 | _ -> 1)
    ---
    E0007 2:23 integer literal `9223372036854775808` is too large
    ");
}

#[test]
fn a_name_is_bound_once_per_tuple_pattern() {
    // タプルの要素は外側のパターンと同じ組なので、要素どうしの重複も E1017 になる
    let text = "f : (Int, Int) -> Int\nf (x, x) = x\n\ng : (Int, Int) -> Int\ng p =\n  let (y, (z, y)) = (p, p)\n  z";
    assert_eq!(
        diagnostics(text),
        [
            "E1017 2:7 `x` is bound more than once",
            "E1017 6:15 `y` is bound more than once",
        ]
    );
}

fn body<'m>(program: &'m Program, name: &str) -> &'m Body {
    program
        .functions()
        .find(|(_, function)| function.name == name)
        .and_then(|(id, _)| program.body(id))
        .expect("the body")
}

#[test]
fn walkers_see_through_tuples() {
    let text = "f : Int -> Int -> (Int, Int)\nf a b =\n  let ((x, _), y) = ((a, a), b)\n  (fn u -> (x, u)) y";
    let module = eml_test_support::lower_clean(text).program;
    let body = body(&module, "f");
    let names = |locals: Vec<LocalId>| -> Vec<String> {
        locals
            .iter()
            .map(|&local| body.locals[local].name.clone())
            .collect()
    };
    let (_, expr) = body
        .exprs
        .iter()
        .find(|(_, expr)| matches!(expr.kind, ExprKind::Lambda(_)))
        .expect("the lambda");
    let ExprKind::Lambda(closure) = &expr.kind else {
        unreachable!();
    };
    // タプルの中の `x` を捕まえ、引数の `u` は捕まえない
    assert_eq!(names(body.closure_captures(closure)), ["x"]);
    let ExprKind::Block { stmts, .. } = &body.exprs[body.root].kind else {
        panic!("expected a block body");
    };
    let Stmt::Let { pat, .. } = &stmts[0] else {
        panic!("expected a `let`");
    };
    assert_eq!(names(body.pat_bindings(*pat)), ["x", "y"]);
    for (id, expr) in body.exprs.iter() {
        if let ExprKind::Tuple(elements) = &expr.kind {
            let mut children = Vec::new();
            body.walk_child_exprs(id, |child| children.push(child));
            assert_eq!(&children, elements);
        }
    }
}

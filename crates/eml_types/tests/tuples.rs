//! タプル、リテラルのパターン、`==` の比べ方の型検査 (docs/spec/records.md、docs/spec/declarations.md の標準の
//! 演算子の表)。

mod common;

use common::check_text;
use eml_hir::{ExprKind, Res};
use eml_types::Equality;

/// 関数 `name` の本体で決まった `==` / `!=` の比べ方を、ソースの順に並べる。
fn decided(text: &str, name: &str) -> Vec<(&'static str, Equality)> {
    let checked = eml_test_support::check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        eml_test_support::short_text(&checked.files, &checked.diagnostics)
    );
    let (id, function) = checked
        .module
        .functions
        .iter()
        .find(|(_, function)| function.name == name)
        .unwrap();
    let body = function.body.as_ref().unwrap();
    let mut found: Vec<(u32, &'static str, Equality)> = checked.typed.bodies[id]
        .equalities
        .iter()
        .map(|(expr, &equality)| {
            let ExprKind::Path(Res::Builtin(builtin)) = &body.exprs[expr].kind else {
                panic!("equalities are keyed by the operator");
            };
            (
                u32::from(body.exprs[expr].range.start()),
                builtin.name(),
                equality,
            )
        })
        .collect();
    found.sort_by_key(|&(start, _, _)| start);
    found
        .into_iter()
        .map(|(_, name, equality)| (name, equality))
        .collect()
}

#[test]
fn tuples_are_closed_records_shown_with_parentheses() {
    // `both` は `x` を2回使うので、タプルの Kind (要素の Kind の join) を通して `a` に `Unr` の制約が付く
    let text = "swap : (Int, String) -> (String, Int)\nswap p = match p with\n  | (n, s) -> (s, n)\n\nnested : ((Int, Bool), String) -> Int\nnested t =\n  let ((n, _), _) = t\n  n\n\nboth : a -> (a, a)\nboth x = (x, x)";
    insta::assert_snapshot!(check_text(text), @r"
    swap : (Int, String) -> (String, Int)
      p#0 : (Int, String)
      n#1 : Int
      s#2 : String
    nested : ((Int, Bool), String) -> Int
      t#0 : ((Int, Bool), String)
      n#1 : Int
    both : a -> (a, a)
      kinds: a <= Unr
      x#0 : a
    ");
}

#[test]
fn a_tuple_pattern_with_another_number_of_elements_is_a_mismatch() {
    let text = "first : (Int, Int) -> Int\nfirst p = match p with\n  | (a, b, c) -> a";
    insta::assert_snapshot!(check_text(text), @r"
    first : (Int, Int) -> Int
      p#0 : (Int, Int)
      a#1 : {error}
      b#2 : {error}
      c#3 : {error}
    ---
    E2001 3:5 mismatched types
      3:5 expected `(Int, Int)`, found `(_, _, _)`
      note: this pattern matches a tuple of 3 elements
    ");
}

#[test]
fn a_literal_pattern_of_another_type_is_a_mismatch() {
    let text = "name : Int -> String\nname n = match n with\n  | \"zero\" -> \"zero\"\n  | -1 -> \"minus one\"\n  | _ -> \"other\"";
    insta::assert_snapshot!(check_text(text), @r#"
    name : Int -> String
      n#0 : Int
    ---
    E2001 3:5 mismatched types
      3:5 expected `Int`, found `String`
      note: a literal pattern matches only values of the type of the literal
    "#);
}

#[test]
fn the_operand_type_decides_how_equality_compares() {
    let text =
        "same : Int -> String -> Bool -> Bool\nsame n s b = n == 1 && s != \"x\" && b == True";
    assert_eq!(
        decided(text, "same"),
        [
            ("==", Equality::Int),
            ("!=", Equality::String),
            ("==", Equality::Bool),
        ]
    );
}

#[test]
fn an_operand_type_decided_after_the_operator_is_used() {
    // ラムダの引数の型は、`eq` を呼んだ後で `String` に決まる。比べ方は本体の検査が終わってから決める
    let text =
        "later : Unit -> Bool\nlater () =\n  let eq = fn x -> fn y -> x == y\n  eq \"a\" \"b\"";
    assert_eq!(decided(text, "later"), [("==", Equality::String)]);
}

#[test]
fn only_int_string_and_bool_can_be_compared() {
    let text = "data Color =\n  | Red\n  | Green\n\ncolors : Color -> Color -> Bool\ncolors a b = a == b\n\npairs : (Int, Int) -> Bool\npairs p = p != (1, 2)\n\nfuns : (Int -> Int) -> Bool\nfuns f = f == (fn n -> n)\n\npoly : a -> a -> Bool\npoly x y = x == y";
    insta::assert_snapshot!(check_text(text), @r"
    colors : Color -> Color -> Bool
      a#0 : Color
      b#1 : Color
    pairs : (Int, Int) -> Bool
      p#0 : (Int, Int)
    funs : (Int -> Int) -> Bool
      f#0 : Int -> Int
      n#1 : Int
    poly : a -> a -> Bool
      x#0 : a
      y#1 : a
    ---
    E2006 6:16 values of type `Color` cannot be compared with `==`
      6:16 `==` cannot compare `Color`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    E2006 9:13 values of type `(Int, Int)` cannot be compared with `!=`
      9:13 `!=` cannot compare `(Int, Int)`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    E2006 12:12 values of type `Int -> Int` cannot be compared with `==`
      12:12 `==` cannot compare `Int -> Int`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    E2006 15:14 values of type `a` cannot be compared with `==`
      15:14 `==` cannot compare `a`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    ");
}

#[test]
fn undecided_operands_are_reported_and_errors_are_not() {
    // `same` の引数の型はどこでも決まらない。`missing` の型は報告済みの誤りの跡 (`Error`) なので、E2006 を重ねない
    let text = "undecided : Unit -> Bool\nundecided () =\n  let same = fn x -> fn y -> x == y\n  True\n\nbroken : Int -> Bool\nbroken n = missing == n";
    insta::assert_snapshot!(check_text(text), @r"
    undecided : Unit -> Bool
      x#0 : _
      y#1 : _
      same#2 : _ -> <_> _ -> <_> Bool
    broken : Int -> Bool
      n#0 : Int
    ---
    E2006 3:32 values of type `_` cannot be compared with `==`
      3:32 `==` cannot compare `_`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    E1001 7:12 cannot find value `missing`
      7:12 not found in this scope
    ");
}

#[test]
fn an_undecided_operand_is_not_reported_when_the_body_has_another_error() {
    // `g` の引数の型は `1 + g` の誤りが直れば決まる。E2006 は連鎖なので出さない
    let text = "broken : Unit -> Int\nbroken () =\n  let g = fn x -> x == x\n  1 + g";
    insta::assert_snapshot!(check_text(text), @"
    broken : Unit -> Int
      x#0 : _
      g#1 : _ -> <_> Bool
    ---
    E2001 4:7 mismatched types
      4:7 expected `Int`, found `_ -> <_> Bool`
      4:5 argument 2 of `+`
    ");
}

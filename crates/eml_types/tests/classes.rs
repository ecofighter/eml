//! 型クラスの制約の解決 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「型検査」)。

use crate::common::check_text;
use eml_test_support::{check, short};

const SAME: &str = "class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n  differ x y = not (same x y)\n\n";
const COLOR: &str = "data Color =\n  | Red\n  | Green\n\n";

fn lines(text: &str) -> Vec<String> {
    let checked = check(text);
    short(checked.files(), &checked.diagnostics)
}

#[test]
fn methods_and_constrained_functions_check() {
    let text = format!(
        "{SAME}{COLOR}instance Same Color where\n  same Red Red = True\n  same Green Green = True\n  same _ _ = False\n\nboth : Same a => a -> a -> a -> Bool\nboth x y z = same x y && same y z\n\nmain : Unit -> <IO> Unit\nmain () = if both Red Red Green then println \"y\" else println \"n\""
    );
    insta::assert_snapshot!(check_text(&text), @"
    same : a -> a -> Bool
    differ : a -> a -> Bool
    both : a -> a -> a -> Bool
      kinds: a <= Unr
      x#0 : a
      y#1 : a
      z#2 : a
    main : Unit -> <IO> Unit
    differ : a -> a -> Bool
      x#0 : a
      y#1 : a
    Same Color.same : Color -> Color -> Bool
      $0#0 : Color
      $1#1 : Color
    ");
}

#[test]
fn a_missing_instance_is_reported_at_the_reference() {
    let text = format!("{SAME}{COLOR}f : Color -> Bool\nf c = same c c");
    assert_eq!(
        lines(&text),
        ["E2006 11:7 no instance of `Same` for `Color`"]
    );
}

#[test]
fn a_type_variable_needs_the_constraint_in_the_signature() {
    let text = format!("{SAME}f : a -> Bool\nf x = same x x");
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 7:7 no instance of `Same` for `a`
      7:7 `same` requires `Same a`
      help: add `Same a =>` to the signature of `f`
    ");
}

#[test]
fn a_superclass_is_given_with_its_subclass() {
    let text = format!(
        "{SAME}class Same a => Order a where\n  less : a -> a -> Bool\n\nboth : Order a => a -> a -> Bool\nboth x y = less x y || same x y"
    );
    assert_eq!(lines(&text), Vec::<String>::new());
}

#[test]
fn an_instance_context_is_needed_where_the_instance_is_used() {
    let text = format!(
        "{SAME}data Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same (Box x) (Box y) = same x y\n\nf : Bool\nf = same (Box (fn x -> x + 1)) (Box (fn x -> x))"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 12:5 no instance of `Same` for `Int -> <_> Int`
      12:5 `same` requires `Same (Box (Int -> <_> Int))`
      note: needed for `Same (Box (Int -> <_> Int))`
    ");
}

#[test]
fn an_undecided_type_is_ambiguous_unless_the_body_has_another_error() {
    let text = format!("{SAME}f : Int -> Int\nf x =\n  let s = same\n  x");
    assert_eq!(
        lines(&text),
        ["E2009 8:11 cannot decide which instance of `Same` `same` uses"]
    );
    let text = format!("{SAME}f : Int -> Int\nf x =\n  let s = same\n  x + \"1\"");
    let found = lines(&text);
    assert!(
        found.iter().all(|line| !line.starts_with("E2009")),
        "{found:?}"
    );
}

#[test]
fn a_missing_superclass_instance_is_reported_at_the_instance() {
    let text = format!(
        "{SAME}class Same a => Order a where\n  less : a -> a -> Bool\n\n{COLOR}instance Order Color where\n  less _ _ = False"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 13:16 no instance of `Same` for `Color`
      13:16 `Order` requires `Same`, its superclass
    ");
}

#[test]
fn instance_and_default_bodies_are_checked_at_their_types() {
    let text = format!(
        "class Size a where\n  size : a -> Int\n  twice : a -> Int\n  twice x = size x ++ \"\"\n\n{COLOR}instance Size Color where\n  size _ = \"one\""
    );
    insta::assert_snapshot!(lines(&text).join("\n"), @"
    E2001 4:13 mismatched types
    E2001 4:13 mismatched types
    E2001 11:12 mismatched types
    ");
}

#[test]
fn method_own_variables_can_carry_constraints() {
    let text = format!(
        "{SAME}class Pick a where\n  pick : Same b => a -> b -> b -> b\n\n{COLOR}instance Pick Color where\n  pick _ x y = if same x y then x else y"
    );
    assert_eq!(lines(&text), Vec::<String>::new());
}

#[test]
fn a_head_variable_and_a_method_variable_with_the_same_name_stay_apart() {
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> (b -> Int -> b) -> b\n\ninstance Fold (Box b) where\n  fold (Box _) acc step = step acc 1";
    assert_eq!(lines(text), Vec::<String>::new());
    // 頭の `b` と、付け替えたメソッドの `b1` を取り違えると、型が合わずに E2001 になる
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> (b -> Int -> b) -> b\n\ninstance Fold (Box b) where\n  fold (Box item) _ _ = item";
    assert!(lines(text)[0].starts_with("E2001"), "{:?}", lines(text));
}

#[test]
fn a_clause_variable_has_no_instance() {
    let text = format!(
        "{SAME}effect Pick where\n  pick : a -> a\n\nrun : Int -> Int\nrun v =\n  handle pick v with\n    | pick x k -> if same x x then k x else k x"
    );
    assert!(lines(&text)[0].starts_with("E2006"), "{:?}", lines(&text));
}

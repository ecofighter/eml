//! 網羅性と到達可能性の検査 (docs/spec/exhaustiveness.md)。

use eml_diagnostics::{Severity, has_errors};
use eml_test_support::{check, full};

/// 型検査までの診断を、ラベルと note まで表示する。
fn diagnostics(text: &str) -> String {
    let checked = check(text);
    full(&checked.files, &checked.diagnostics)
}

#[test]
fn a_match_that_misses_a_constructor() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o = match o with\n  | Some n -> n";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `None`
    ");
}

#[test]
fn a_bool_match_that_misses_true() {
    let text = "f : Bool -> Int\nf b = match b with\n  | False -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 2:7 `match` does not cover every value
      2:7 no arm matches some values
      note: not covered: `True`
    ");
}

#[test]
fn a_nested_pattern_reports_the_missing_inner_constructor() {
    let text = "data Option a = | None | Some a\n\nf : Option (Option Int) -> Int\nf o = match o with\n  | Some (Some n) -> n\n  | None -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `Some None`
    ");
}

#[test]
fn a_recursive_type_reports_a_nested_example() {
    let text = "data List a = | Nil | Cons a (List a)\n\nf : List Int -> Int\nf xs = match xs with\n  | Nil -> 0\n  | Cons x Nil -> x";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:8 `match` does not cover every value
      4:8 no arm matches some values
      note: not covered: `Cons _ (Cons _ _)`
    ");
}

#[test]
fn more_than_three_examples_are_cut_short() {
    let text = "data Color = | Red | Green | Blue | Cyan | Magenta\n\nname : Color -> Int\nname c = match c with\n  | Red -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:10 `match` does not cover every value
      4:10 no arm matches some values
      note: not covered: `Green`, `Blue`, `Cyan`, and more
    ");
}

#[test]
fn an_arm_after_a_wildcard_is_unreachable() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o = match o with\n  | _ -> 0\n  | Some n -> n";
    let checked = check(text);
    insta::assert_snapshot!(full(&checked.files, &checked.diagnostics), @r"
    E4004 6:5 unreachable `match` arm
      6:5 the arms above already match every value of this pattern
    ");
    assert_eq!(checked.diagnostics[0].severity, Severity::Warning);
    assert!(!has_errors(&checked.diagnostics));
}

#[test]
fn a_repeated_arm_is_unreachable() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o = match o with\n  | None -> 0\n  | Some n -> n\n  | None -> 1";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4004 7:5 unreachable `match` arm
      7:5 the arms above already match every value of this pattern
    ");
}

#[test]
fn single_constructor_patterns_are_irrefutable() {
    let text = "data Box a = | Box a\n\nunbox : Box Int -> Int\nunbox (Box n) =\n  let Box m = Box n\n  let f = fn (Box k) -> k\n  f (Box m)";
    assert_eq!(diagnostics(text), "");
}

#[test]
fn refutable_let_and_lambda_patterns() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o =\n  let Some n = o\n  let g = fn (Some k) -> k\n  g (Some n)";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4003 5:7 this pattern does not match every value
      5:7 `let` needs a pattern that matches every value
      note: not covered: `None`
    E4003 6:15 this pattern does not match every value
      6:15 a parameter needs a pattern that matches every value
      note: not covered: `None`
    ");
}

#[test]
fn refutable_clause_parameters() {
    // 節は持ち上げる関数で、引数のパターンもラムダの引数と同じ経路で分解するので、同じ E4003 で調べる
    let text = "data Option a = | None | Some a\n\neffect Ask where\n  ask : Option Int -> Int\n\nf : Unit -> Int\nf () =\n  handle Some (ask (Some 1)) with\n    | ask (Some q) k -> resume k q\n    | return (Some r) -> r";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4003 9:12 this pattern does not match every value
      9:12 a parameter needs a pattern that matches every value
      note: not covered: `None`
    E4003 10:15 this pattern does not match every value
      10:15 a parameter needs a pattern that matches every value
      note: not covered: `None`
    ");
}

#[test]
fn a_refutable_equation_points_at_the_signature() {
    let text =
        "data Option a = | None | Some a\n\nget : Option Int -> Int -> Int\nget (Some n) m = n + m";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4002 3:1 the equation of `get` does not cover every argument
      3:1 `get` is not defined for some arguments
      4:1 this equation does not match every argument
      note: not covered: `get None _`
    ");
}

#[test]
fn a_scrutinee_with_an_error_type_is_not_checked() {
    let text = "data Option a = | None | Some a\n\nf : Int -> Int\nf x = match missing with\n  | Some n -> n";
    insta::assert_snapshot!(diagnostics(text), @r"
    E1001 4:13 cannot find value `missing`
      4:13 not found in this scope
    ");
}

#[test]
fn constructors_of_two_types_in_one_column_add_no_exhaustiveness_errors() {
    // `Nil` の型の誤りは E2001 で報告済みなので、混ざった列から E4xxx を連鎖させない
    let text = "data Option a = | None | Some a\ndata List a = | Nil | Cons a (List a)\n\nf : Option Int -> Int\nf o = match o with\n  | Nil -> 0\n  | None -> 1";
    let checked = check(text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert!(codes.contains(&"E2001".to_string()), "{codes:?}");
    assert!(
        codes.iter().all(|code| !code.starts_with("E4")),
        "{codes:?}"
    );
}

#[test]
fn a_match_without_arms_is_left_to_the_syntax_error() {
    let text = "f : Bool -> Int\nf b = match b with";
    insta::assert_snapshot!(diagnostics(text), @"
    E0009 2:15 expected an indented block after `with`
      2:15 the next line must be indented more than the enclosing block
      help: indent the `|` arms more than the line with `with`
    ");
}

#[test]
fn a_tuple_match_reports_a_missing_tuple() {
    let text = "data Option a = | None | Some a\n\nf : (Option Int, Bool) -> Int\nf p = match p with\n  | (Some n, True) -> n\n  | (None, _) -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `(Some _, False)`
    ");
}

#[test]
fn a_tuple_inside_a_constructor_reports_the_whole_example() {
    let text = "data Option a = | None | Some a\n\nf : Option (Int, Int) -> Int\nf o = match o with\n  | Some (0, y) -> y\n  | None -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `Some (_, _)`
    ");
}

#[test]
fn int_literals_alone_never_cover_every_value() {
    let text = "f : Int -> String\nf n = match n with\n  | 0 -> \"zero\"\n  | 1 -> \"one\"";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 2:7 `match` does not cover every value
      2:7 no arm matches some values
      note: not covered: `_`
    ");
}

#[test]
fn string_literals_alone_never_cover_every_value() {
    let text = "f : String -> Int\nf s = match s with\n  | \"a\" -> 1\n  | \"b\" -> 2";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 2:7 `match` does not cover every value
      2:7 no arm matches some values
      note: not covered: `_`
    ");
}

#[test]
fn a_repeated_literal_is_unreachable() {
    let text = "f : Int -> Int\nf n = match n with\n  | 0 -> 1\n  | 0 -> 2\n  | _ -> 3";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4004 4:5 unreachable `match` arm
      4:5 the arms above already match every value of this pattern
    ");
}

#[test]
fn negative_and_positive_literals_are_different_values() {
    // `-1` と `1` は別の値なので、2つ目の `-1` だけが到達しない
    let text =
        "f : Int -> Int\nf n = match n with\n  | -1 -> 0\n  | 1 -> 1\n  | -1 -> 2\n  | _ -> 3";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4004 5:5 unreachable `match` arm
      5:5 the arms above already match every value of this pattern
    ");
}

#[test]
fn literals_with_a_wildcard_cover_every_value() {
    let text = "f : String -> Int\nf s = match s with\n  | \"yes\" -> 1\n  | _ -> 0";
    assert_eq!(diagnostics(text), "");
}

#[test]
fn tuple_patterns_are_irrefutable() {
    let text = "swap : (Int, String) -> (String, Int)\nswap p =\n  let (a, b) = p\n  (b, a)\n\nfirst : (Int, Int) -> Int\nfirst (x, _) = x\n\npick : (Int, Int) -> Int\npick p = (fn (a, b) -> a + b) p";
    assert_eq!(diagnostics(text), "");
}

#[test]
fn a_literal_inside_a_tuple_makes_the_let_refutable() {
    let text = "f : (Int, Int) -> Int\nf p =\n  let (0, y) = p\n  y";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4003 3:7 this pattern does not match every value
      3:7 `let` needs a pattern that matches every value
      note: not covered: `(_, _)`
    ");
}

#[test]
fn a_literal_inside_a_tuple_parameter_makes_the_equation_refutable() {
    let text = "f : (Int, Bool) -> Int\nf (0, b) = 1";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4002 1:1 the equation of `f` does not cover every argument
      1:1 `f` is not defined for some arguments
      2:1 this equation does not match every argument
      note: not covered: `f (_, _)`
    ");
}

#[test]
fn a_witness_spans_a_literal_column_and_a_data_column() {
    let text = "data Option a = | None | Some a\n\nf : (Int, Option Int) -> Int\nf p = match p with\n  | (0, Some _) -> 1\n  | (_, None) -> 2";
    insta::assert_snapshot!(diagnostics(text), @"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `(_, Some _)`
    ");
}

#[test]
fn several_equations_that_miss_an_argument() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Option Int -> Int\nf (Some x) _ = x\nf None (Some y) = y";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4002 3:1 the equations of `f` do not cover every argument
      3:1 `f` is not defined for some arguments
      4:1 an equation of `f`
      5:1 an equation of `f`
      note: not covered: `f None None`
    ");
}

#[test]
fn an_equation_after_a_catch_all_is_unreachable() {
    let text = "g : Int -> Int\ng _ = 0\ng 1 = 1";
    let checked = check(text);
    assert!(!has_errors(&checked.diagnostics));
    insta::assert_snapshot!(full(&checked.files, &checked.diagnostics), @r"
    E4005 3:3 unreachable equation
      3:3 the equations above already match these arguments
    ");
    assert_eq!(checked.diagnostics[0].severity, Severity::Warning);
}

#[test]
fn a_value_defined_twice_has_an_unreachable_equation() {
    let text = "pi : Int\npi = 3\npi = 4";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4005 3:1 unreachable equation
      3:1 the equations above already match these arguments
    ");
}

#[test]
fn equations_with_a_type_error_add_no_exhaustiveness_errors() {
    let text = "f : Int -> Int\nf \"a\" = 1\nf 2 = 2";
    let text_out = diagnostics(text);
    assert!(!text_out.contains("E4002"), "{text_out}");
}

#[test]
fn a_dropped_equation_adds_no_exhaustiveness_errors() {
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x = x";
    let text_out = diagnostics(text);
    assert!(text_out.contains("E1020"), "{text_out}");
    assert!(
        !text_out.contains("E4001") && !text_out.contains("E4002"),
        "{text_out}"
    );
    assert!(!text_out.contains("E4005"), "{text_out}");
}

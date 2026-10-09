//! タプル、リテラルのパターン、`==` と `!=` が求める `Eq` の制約の型検査 (docs/spec/records.md、
//! docs/spec/types.md の「制約の解決」)。

use crate::common::check_text;

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
fn equality_needs_an_eq_instance_of_the_operand_type() {
    let text = "data Color =\n  | Red\n  | Green\n\ncolors : Color -> Color -> Bool\ncolors a b = a == b\n\npairs : (Int, Int) -> Bool\npairs p = p != (1, 2)\n\nfuns : (Int -> Int) -> Bool\nfuns f = f == (fn n -> n)\n\npoly : a -> a -> Bool\npoly x y = x == y";
    insta::assert_snapshot!(check_text(text), @"
    colors : Color -> Color -> Bool
      a#0 : Color
      b#1 : Color
    pairs : (Int, Int) -> Bool
      p#0 : (Int, Int)
    funs : (Int -> Int) -> Bool
      kinds: (Int -> Int) <= Unr
      f#0 : Int -> Int
      n#1 : Int
    poly : a -> a -> Bool
      kinds: a <= Unr
      x#0 : a
      y#1 : a
    ---
    E2006 6:16 no instance of `Eq` for `Color`
      6:16 `==` requires `Eq Color`
    E2006 12:12 no instance of `Eq` for `Int -> Int`
      12:12 `==` requires `Eq (Int -> Int)`
    E2006 15:14 no instance of `Eq` for `a`
      15:14 `==` requires `Eq a`
      help: add `Eq a =>` to the signature of `poly`
    ");
}

#[test]
fn undecided_operands_are_reported_and_errors_are_not() {
    // `same` の引数の型はどこでも決まらない。`missing` の型は報告済みの誤りの跡 (`Error`) なので、E2009 を重ねない
    let text = "undecided : Unit -> Bool\nundecided () =\n  let same = fn x -> fn y -> x == y\n  True\n\nbroken : Int -> Bool\nbroken n = missing == n";
    insta::assert_snapshot!(check_text(text), @"
    undecided : Unit -> Bool
      x#0 : _
      y#1 : _
      same#2 : _ -> <_> _ -> <_> Bool
    broken : Int -> Bool
      n#0 : Int
    ---
    E2009 3:32 cannot decide which instance of `Eq` to use for `==`
      3:32 the type here is never decided
      help: add a type annotation
    E1001 7:12 cannot find value `missing`
      7:12 not found in this scope
    ");
}

#[test]
fn an_undecided_operand_is_not_reported_when_the_body_has_another_error() {
    // `g` の引数の型は `1 + g` の誤りが直れば決まる。E2009 は連鎖なので出さない
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

#[test]
fn two_undecided_equalities_in_one_body_are_both_reported() {
    // 本体に別の誤りがあるかは制約を解く前に1回だけ決めるので、先の E2009 が後の E2009 を抑えない
    let text = "both : Unit -> Bool\nboth () =\n  let same = fn x -> fn y -> x == y\n  let differ = fn x -> fn y -> x != y\n  True";
    insta::assert_snapshot!(check_text(text), @"
    both : Unit -> Bool
      x#0 : _
      y#1 : _
      same#2 : _ -> <_> _ -> <_> Bool
      x#3 : _
      y#4 : _
      differ#5 : _ -> <_> _ -> <_> Bool
    ---
    E2009 3:32 cannot decide which instance of `Eq` to use for `==`
      3:32 the type here is never decided
      help: add a type annotation
    E2009 4:34 cannot decide which instance of `Eq` to use for `!=`
      4:34 the type here is never decided
      help: add a type annotation
    ");
}

#[test]
fn an_undecided_instance_suppresses_the_linearity_diagnostics() {
    // E2009 は線形性の検査より前に決まり、本体の誤りに数えるので、`f` を使わないことの E3003 を出さない
    // (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
    let text = "leak : Fs.File -> Bool\nleak f =\n  let same = fn x -> fn y -> x == y\n  True";
    insta::assert_snapshot!(check_text(text), @"
    leak : File -> Bool
      f#0 : File
      x#1 : _
      y#2 : _
      same#3 : _ -> <_> _ -> <_> Bool
    ---
    E2009 3:32 cannot decide which instance of `Eq` to use for `==`
      3:32 the type here is never decided
      help: add a type annotation
    ");
}

mod common;

use common::lower_text;

#[test]
fn precedence_and_left_associativity() {
    insta::assert_snapshot!(lower_text("f : Int -> Int\nf x = 1 + 2 * x - 3"), @r"
    f : Int -> Int
    f x#0 = (- (+ 1 (* 2 x#0)) 3)
    ");
}

#[test]
fn right_associativity() {
    insta::assert_snapshot!(lower_text("s : String\ns = \"a\" ++ \"b\" ++ \"c\""), @r#"
    s : String
    s = (++ "a" (++ "b" "c"))
    "#);
}

#[test]
fn prefix_minus_has_precedence_six() {
    let text = "n : Int\nn = - 2 * 3\nm : Int\nm = - 2 + 3\nk : Bool\nk = 1 == -2";
    insta::assert_snapshot!(lower_text(text), @r"
    n : Int
    n = (negate (* 2 3))
    m : Int
    m = (+ (negate 2) 3)
    k : Bool
    k = (== 1 (negate 2))
    ");
}

#[test]
fn and_and_or_become_if() {
    insta::assert_snapshot!(lower_text("b : Bool\nb = True && False || True"), @r"
    b : Bool
    b = (if (if True False False) True True)
    ");
}

#[test]
fn pipes_become_applications() {
    let text = "f : Int -> Int\nf x = x\ng : Int -> Int -> Int\ng a b = a\np : Int\np = 1 |> f |> g 2\nq : Int\nq = g 1 <| f 2";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = x#0
    g : Int -> Int -> Int
    g a#0 b#1 = a#0
    p : Int
    p = (@g 2 (@f 1))
    q : Int
    q = (@g 1 (@f 2))
    ");
}

#[test]
fn non_associative_operators_cannot_be_chained() {
    insta::assert_snapshot!(lower_text("b : Bool\nb = 1 < 2 < 3"), @r"
    b : Bool
    b = <missing>
    ---
    E1006 2:11 `<` and `<` cannot be combined without parentheses
    ");
}

#[test]
fn prefix_minus_after_a_tighter_operator_needs_parentheses() {
    insta::assert_snapshot!(lower_text("n : Int\nn = 2 * -3"), @r"
    n : Int
    n = (* 2 (negate 3))
    ---
    E1006 2:9 a prefix `-` cannot appear here without parentheses
    ");
}

#[test]
fn unknown_and_unsupported_operators_and_missing_operands() {
    let text = "x : Int\nx = 1 <+> 2\ny : Int\ny = 1 :: 2\nz : Int\nz = 1 +";
    insta::assert_snapshot!(lower_text(text), @r"
    x : Int
    x = (<missing> 1 2)
    y : Int
    y = (<missing> 1 2)
    z : Int
    z = (+ 1 <missing>)
    ---
    E1001 2:7 cannot find operator `<+>`
    E0004 4:7 lists are not supported yet
    E0011 6:8 expected an expression
    ");
}

#[test]
fn mixed_associativity_is_rejected_in_both_orders() {
    let text = "x : Int\nx = 1 <+> 2 >> 3\ny : Int\ny = 1 >> 2 <+> 3";
    insta::assert_snapshot!(lower_text(text), @r"
    x : Int
    x = <missing>
    y : Int
    y = (>> 1 <missing>)
    ---
    E1001 2:7 cannot find operator `<+>`
    E1006 2:13 `<+>` and `>>` cannot be combined without parentheses
    E1006 4:12 `>>` and `<+>` cannot be combined without parentheses
    ");
}

#[test]
fn composition_operators_are_builtin_calls() {
    insta::assert_snapshot!(lower_text("h : Bool -> Bool\nh = not >> not << not"), @r"
    h : Bool -> Bool
    h = (>> not (<< not not))
    ");
}

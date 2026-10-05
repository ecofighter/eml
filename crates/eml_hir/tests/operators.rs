mod common;

use common::{diagnostics, lower_text};

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
    p = (@g 2 |>(@f |>1))
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

#[test]
fn a_declared_fixity_regroups_a_constructor_operator() {
    // 宣言がなければ `infixl 9` で `(a :+ b) :+ c` になるところを、`infixr` で右に組む
    let text = "infixr 5 :+\ndata P = | E | Int :+ P\n\np : P\np = 1 :+ 2 :+ E";
    insta::assert_snapshot!(lower_text(text), @r"
    data P
      | E
      | Int :+ P
    p : P
    p = (:+ 1 (:+ 2 E))
    ");
}

#[test]
fn a_fixity_declared_after_its_use_still_applies() {
    let text = "data P = | E | Int :+ P\n\np : P\np = 1 :+ 2 :+ E\n\ninfixr 5 :+";
    insta::assert_snapshot!(lower_text(text), @r"
    data P
      | E
      | Int :+ P
    p : P
    p = (:+ 1 (:+ 2 E))
    ");
}

#[test]
fn fixity_declarations_need_one_definition_in_this_module() {
    let text = "infixr 5 :+\ninfixl 6 :+\ninfixl 6 +\ninfix 4 <=>\ndata P = | E | Int :+ P\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1021 2:10 `:+` has more than one fixity declaration",
            "E1022 3:10 `+` is not defined in this module",
            "E1022 4:9 `<=>` is not defined in this module",
        ]
    );
}

#[test]
fn constructor_patterns_with_conflicting_fixities_need_parentheses() {
    let text = "infixl 5 :+\ninfixr 5 :-\ndata P = | E | P :+ Int | Int :- P\n\nf : P -> Int\nf p = match p with | a :+ b :- c -> 0 | _ -> 1";
    assert_eq!(
        diagnostics(text),
        vec!["E1006 6:29 `:+` and `:-` cannot be combined without parentheses"]
    );
}

#[test]
fn user_operators_are_functions_with_their_own_fixity() {
    let text =
        "infixr 5 <+>\n(<+>) : Int -> Int -> Int\na <+> b = a + b\n\nf : Int\nf = 1 <+> 2 <+> 3";
    insta::assert_snapshot!(lower_text(text), @r"
    <+> : Int -> Int -> Int
    <+> a#0 b#1 = (+ a#0 b#1)
    f : Int
    f = (@<+> 1 (@<+> 2 3))
    ");
}

#[test]
fn a_user_operator_without_a_fixity_is_infixl_9() {
    // ユーザーの `+` は Prelude の `+` を隠し、宣言がないので `infixl 9` になる。`*` (7) より強く結合する
    let text = "(+) : Int -> Int -> Int\na + b = a - b\n\nf : Int\nf = 1 * 2 + 3";
    insta::assert_snapshot!(lower_text(text), @r"
    + : Int -> Int -> Int
    + a#0 b#1 = (- a#0 b#1)
    f : Int
    f = (* 1 (@+ 2 3))
    ");
}

#[test]
fn a_user_definition_hides_a_desugared_operator() {
    // ユーザーが `&&` を定義すると、短絡の `if` ではなく普通の呼び出しになる
    let text = "(&&) : Bool -> Bool -> Bool\na && b = b\n\nf : Bool\nf = True && False";
    insta::assert_snapshot!(lower_text(text), @r"
    && : Bool -> Bool -> Bool
    && a#0 b#1 = b#1
    f : Bool
    f = (@&& True False)
    ");
}

#[test]
fn operator_references_and_sections_become_lambdas() {
    let text = "a : Int -> Int -> Int\na = (+)\nb : Int -> Int\nb = (+ 1)\nc : Int -> Int\nc = (10 -)\nd : Bool -> Bool -> Bool\nd = (&&)\ne : Int\ne = (- 1)";
    insta::assert_snapshot!(lower_text(text), @"
    a : Int -> Int -> Int
    a = (fn $a#0 $b#1 -> (+ $a#0 $b#1))
    b : Int -> Int
    b = (fn $x#0 -> (+ $x#0 1))
    c : Int -> Int
    c = (fn $x#0 -> (- 10 $x#0))
    d : Bool -> Bool -> Bool
    d = (fn $a#0 $b#1 -> (if $a#0 $b#1 False))
    e : Int
    e = (negate 1)
    ");
}

#[test]
fn a_section_operand_may_be_an_operator_sequence_that_binds_tighter() {
    let text = "a : Int -> Int\na = (+ 2 * 3)\nb : Int -> Int\nb = (2 * 3 +)\nc : Int -> Int\nc = (1 + 2 +)\nd : String -> String\nd = (++ \"a\" ++ \"b\")";
    insta::assert_snapshot!(lower_text(text), @r#"
    a : Int -> Int
    a = (fn $x#0 -> (+ $x#0 (* 2 3)))
    b : Int -> Int
    b = (fn $x#0 -> (+ (* 2 3) $x#0))
    c : Int -> Int
    c = (fn $x#0 -> (+ (+ 1 2) $x#0))
    d : String -> String
    d = (fn $x#0 -> (++ $x#0 (++ "a" "b")))
    "#);
}

#[test]
fn a_prefix_minus_in_a_section_operand_follows_the_operator_sequence_rules() {
    let text =
        "a : Int -> Int\na = (+ -1)\nb : Int -> Int\nb = (* - 2)\nc : Int -> Int\nc = (- 2 *)";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1006 2:8 a prefix `-` cannot appear here without parentheses",
            "E1006 4:8 a prefix `-` cannot appear here without parentheses",
            "E1023 6:5 the section of `*` needs parentheses around its operand",
        ]
    );
    insta::assert_snapshot!(lower_text("d : Int -> Int\nd = (- 2 +)"), @"
    d : Int -> Int
    d = (fn $x#0 -> (+ (negate 2) $x#0))
    ");
}

#[test]
fn an_invalid_section_still_reports_errors_in_its_operand() {
    let text = "a : Int -> Int\na = (* undefined_name + 2)";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1023 2:5 the section of `*` needs parentheses around its operand",
            "E1001 2:8 cannot find value `undefined_name`",
        ]
    );
}

#[test]
fn a_section_operand_that_binds_looser_needs_parentheses() {
    let text = "a : Int -> Int\na = (* 1 + 2)\nb : Int -> Int\nb = (+ 1 + 2)\nc : Bool -> Bool\nc = (== 1 == 2)";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1023 2:5 the section of `*` needs parentheses around its operand",
            "E1023 4:5 the section of `+` needs parentheses around its operand",
            "E1023 6:5 the section of `==` needs parentheses around its operand",
        ]
    );
}

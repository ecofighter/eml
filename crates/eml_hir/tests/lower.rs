mod common;

use common::lower_text;

#[test]
fn a_signature_and_an_equation_become_a_function() {
    insta::assert_snapshot!(lower_text("f : Int -> <IO> Unit\nf x = println (show_int x)"), @r"
    f : Int -> <IO> Unit
    f x#0 = (println (show_int x#0))
    ");
}

#[test]
fn later_lets_shadow_earlier_names() {
    let text = "f : Int -> Int\nf x =\n  let y = x\n  let x = y\n  x";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let y#1 = x#0
      let x#2 = y#1
      x#2
    }
    ");
}

#[test]
fn functions_resolve_in_any_order_and_shadow_builtins() {
    let text =
        "a : Int -> Int\na n = b n\n\nb : Int -> Int\nb n = not n\n\nnot : Int -> Int\nnot n = n";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int -> Int
    a n#0 = (@b n#0)
    b : Int -> Int
    b n#0 = (@not n#0)
    not : Int -> Int
    not n#0 = n#0
    ");
}

#[test]
fn if_without_else_and_annotations() {
    let text = "f : Bool -> Unit\nf b =\n  if b then println \"yes\"\n  (() : Unit)";
    insta::assert_snapshot!(lower_text(text), @r#"
    f : Bool -> Unit
    f b#0 = {
      (if b#0 (println "yes"))
      (() : Unit)
    }
    "#);
}

#[test]
fn unit_and_wildcard_parameters_and_literals() {
    let text = "g : Unit -> Bool -> String\ng () _ = if True then \"a\\n\" else \"b\"";
    insta::assert_snapshot!(lower_text(text), @r#"
    g : Unit -> Bool -> String
    g () _ = (if True "a\n" "b")
    "#);
}

#[test]
fn undefined_names_are_reported() {
    insta::assert_snapshot!(lower_text("f : Int -> Strin\nf x = g y Foo"), @r"
    f : Int -> <error>
    f x#0 = (<missing> <missing> <missing>)
    ---
    E1002 1:12 cannot find type `Strin`
    E1001 2:7 cannot find value `g`
    E1001 2:9 cannot find value `y`
    E1001 2:11 cannot find constructor `Foo`
    ");
}

#[test]
fn signatures_and_equations_are_paired_by_name() {
    let text = "a : Int\nb = 1\nc : Int\nc = 2\nc : Int\nd : Int\nd = 3\nd = 4";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int
    a = <no equation>
    b : <no signature>
    b = 1
    c : Int
    c = 2
    d : Int
    d = 3
    ---
    E1005 1:1 `a` has a signature but no equation
    E1004 2:1 `b` has no type signature
    E1003 5:1 `c` is defined more than once
    E0004 8:1 defining a function with several equations is not supported yet
    ");
}

#[test]
fn constructs_of_later_stages_are_not_yet_supported() {
    let text =
        "data Color = | Red\nf : Int -> Int\nf x =\n  let g = fn y -> y\n  match x with | _ -> x";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let g#1 = <missing>
      <missing>
    }
    ---
    E0004 1:1 `data` declarations are not supported yet
    E0004 4:11 lambdas are not supported yet
    E0004 5:3 `match` is not supported yet
    ");
}

#[test]
fn rows_and_types_of_later_stages() {
    let text = "f : Int -> <e> Int\nf x = x\ng : a -> <State> Int\ng x = 1";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> <error> Int
    f x#0 = x#0
    g : <error> -> <error> Int
    g x#0 = 1
    ---
    E0004 1:13 row variables are not supported yet
    E0004 3:5 type variables are not supported yet
    E1002 3:11 cannot find effect `State`
    ");
}

#[test]
fn operator_definitions_and_qualified_names() {
    let text = "(<+>) : Int\na <+> b = a\nf : Int -> Int\nf (x) = List.length x";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = (<missing> x#0)
    ---
    E0004 1:2 defining operators is not supported yet
    E0004 2:3 defining operators is not supported yet
    E0004 4:9 qualified names are not supported yet
    ");
}

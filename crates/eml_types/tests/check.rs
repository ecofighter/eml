mod common;

use common::check_text;

#[test]
fn signatures_and_local_types() {
    let text = "add : Int -> Int -> Int\nadd a b =\n  let sum = a + b\n  sum\n\nmain : Unit -> <IO> Unit\nmain () =\n  let message = \"sum: \" ++ show_int (add 1 2)\n  println message";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
      sum#2 : Int
    main : Unit -> <IO> Unit
      message#0 : String
    ");
}

#[test]
fn argument_mismatch_points_at_the_callee() {
    let text = "f : Int -> Int\nf x = x\n\ng : Unit -> Int\ng () = f \"one\"";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
      x#0 : Int
    g : Unit -> Int
    ---
    E2001 5:10 mismatched types
      5:10 expected `Int`, found `String`
      5:8 argument 1 of `f`
    ");
}

#[test]
fn statements_annotations_conditions_and_signatures() {
    let text = "h : Bool -> Int\nh b =\n  1\n  let n : Int = \"x\"\n  if n then 1 else \"two\"";
    insta::assert_snapshot!(check_text(text), @r#"
    h : Bool -> Int
      b#0 : Bool
      n#1 : Int
    ---
    E2001 3:3 mismatched types
      3:3 expected `Unit`, found `Int`
      note: a statement that is not the last one in a block must have type `Unit`
    E2001 4:17 mismatched types
      4:17 expected `Int`, found `String`
      4:11 expected because of this annotation
    E2001 5:6 mismatched types
      5:6 expected `Bool`, found `Int`
      note: the condition of `if` must have type `Bool`
    E2001 5:20 mismatched types
      5:20 expected `Int`, found `String`
      1:5 expected because of the signature of `h`
    "#);
}

#[test]
fn if_without_else_must_be_unit() {
    insta::assert_snapshot!(check_text("k : Bool -> Int\nk b = if b then 1"), @r"
    k : Bool -> Int
      b#0 : Bool
    ---
    E2001 2:17 mismatched types
      2:17 expected `Unit`, found `Int`
      note: an `if` without `else` must have type `Unit`
    E2001 2:7 mismatched types
      2:7 expected `Int`, found `Unit`
      1:5 expected because of the signature of `k`
    ");
}

#[test]
fn effects_must_be_in_the_signature() {
    let text = "greet : String -> Unit\ngreet name = println name\n\nshout : String -> <IO> Unit\nshout name = println (name ++ \"!\")";
    insta::assert_snapshot!(check_text(text), @r"
    greet : String -> Unit
      name#0 : String
    shout : String -> <IO> Unit
      name#0 : String
    ---
    E2002 2:14 `println` performs `IO`, which the signature of `greet` does not allow
      2:14 this call performs `IO`
      1:9 the row of this signature does not include it
      help: add `IO` to the row of the signature of `greet`, as in `-> <IO> ...`
    ");
}

#[test]
fn main_type_and_parameter_count() {
    let text = "main : Int -> Int\nmain n = n\n\ntwo : Int -> Int\ntwo a b = a";
    insta::assert_snapshot!(check_text(text), @r"
    main : Int -> Int
      n#0 : Int
    two : Int -> Int
      a#0 : Int
      b#1 : {error}
    ---
    E2004 1:8 `main` must have type `Unit -> <IO> Unit`
      1:8 found `Int -> Int`
    E2001 5:7 `two` has 2 parameters but its signature has 1 arrow
      5:7 this parameter has no arrow in the signature
      4:7 the signature
    ");
}

#[test]
fn function_values_and_partial_application() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\npartial : Unit -> Int\npartial () =\n  let f = add 1\n  let g = add\n  0\n\ncurried : Int -> Int -> Int\ncurried a = a";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
    partial : Unit -> Int
      f#0 : Int -> <_> Int
      g#1 : Int -> <_> Int -> <_> Int
    curried : Int -> Int -> Int
      a#0 : Int
    ---
    E2001 11:13 mismatched types
      11:13 expected `Int -> Int`, found `Int`
      10:11 expected because of the signature of `curried`
    ");
}

#[test]
fn name_errors_do_not_cascade() {
    let text = "f : Int -> Int\nf x = g (x + undefined_thing) 1\nb : Bool\nb = 1 < 2 < 3";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
      x#0 : Int
    b : Bool
    ---
    E1001 2:7 cannot find value `g`
      2:7 not found in this scope
    E1001 2:14 cannot find value `undefined_thing`
      2:14 not found in this scope
    E1006 4:11 `<` and `<` cannot be combined without parentheses
      4:11 use parentheses to group the operators
    ");
}

#[test]
fn too_many_arguments_and_non_functions() {
    let text = "one : Int -> Int\none x = x\n\nbad : Unit -> Int\nbad () = one 1 2 + True 3";
    insta::assert_snapshot!(check_text(text), @r"
    one : Int -> Int
      x#0 : Int
    bad : Unit -> Int
    ---
    E2001 5:16 `one` takes 1 argument but 2 were given
      5:16 unexpected argument
    E2001 5:25 `True` is not a function
      5:25 unexpected argument
    ");
}

#[test]
fn main_with_an_erroneous_row_is_not_reported_again() {
    insta::assert_snapshot!(check_text("main : Unit -> <Console> Unit\nmain () = ()"), @r"
        main : Unit -> <_> Unit
        ---
        E1002 1:17 cannot find effect `Console`
          1:17 not found in this scope
        ");
}

#[test]
fn function_typed_parameters_can_be_called_and_passed() {
    let text = "apply : (Int -> Int) -> Int -> Int\napply f x = f x\n\npass : (Int -> Int) -> Int\npass f = apply f 1";
    insta::assert_snapshot!(check_text(text), @"
    apply : (Int -> Int) -> Int -> Int
      f#0 : Int -> Int
      x#1 : Int
    pass : (Int -> Int) -> Int
      f#0 : Int -> Int
    ");
}

#[test]
fn a_call_reports_a_missing_effect_once() {
    let text =
        "f : Int -> <IO> Int -> <IO> Unit\nf a b = println \"x\"\n\ng : Int -> Unit\ng n = f 1 2";
    insta::assert_snapshot!(check_text(text), @"
    f : Int -> <IO> Int -> <IO> Unit
      a#0 : Int
      b#1 : Int
    g : Int -> Unit
      n#0 : Int
    ---
    E2002 5:7 `f` performs `IO`, which the signature of `g` does not allow
      5:7 this call performs `IO`
      4:5 the row of this signature does not include it
      help: add `IO` to the row of the signature of `g`, as in `-> <IO> ...`
    ");
}

#[test]
fn a_value_cannot_perform_effects() {
    let text = "v : Unit\nv = println \"x\"";
    insta::assert_snapshot!(check_text(text), @"
    v : Unit
    ---
    E2002 2:5 `println` performs `IO`, which the signature of `v` does not allow
      2:5 this call performs `IO`
      1:5 the row of this signature does not include it
      help: `v` takes no parameters, so it cannot perform `IO`; make it a function taking `()`, as in `v : Unit -> <IO> ...` with `v () = ...`
    ");
}

#[test]
fn polymorphic_functions_are_instantiated_at_each_use() {
    let text = "id : a -> a\nid x = x\n\nuse_both : Unit -> String\nuse_both () =\n  let n = id 1\n  let s = id \"s\"\n  s";
    insta::assert_snapshot!(check_text(text), @r"
    id : a -> a
      x#0 : a
    use_both : Unit -> String
      n#0 : Int
      s#1 : String
    ");
}

#[test]
fn rigid_type_variables_do_not_unify_with_other_types() {
    let text = "f : a -> b\nf x = x\n\ng : a -> Int\ng x = x";
    insta::assert_snapshot!(check_text(text), @r"
    f : a -> b
      x#0 : a
    g : a -> Int
      x#0 : a
    ---
    E2001 2:7 mismatched types
      2:7 expected `b`, found `a`
      1:5 expected because of the signature of `f`
    E2001 5:7 mismatched types
      5:7 expected `Int`, found `a`
      4:5 expected because of the signature of `g`
    ");
}

#[test]
fn annotations_in_the_body_refer_to_the_signature() {
    let text = "f : a -> a\nf x =\n  let y : a = x\n  y\n\ng : a -> Int\ng x = (x : Int)";
    insta::assert_snapshot!(check_text(text), @r"
    f : a -> a
      x#0 : a
      y#1 : a
    g : a -> Int
      x#0 : a
    ---
    E2001 7:8 mismatched types
      7:8 expected `Int`, found `a`
      7:12 expected because of this annotation
    ");
}

#[test]
fn row_variables_pass_effects_through() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nshout : String -> <IO> Unit\nshout s = apply println s\n\nquiet : String -> Unit\nquiet s = apply println s";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (a -> <e> b) -> a -> <e> b
      f#0 : a -> <e> b
      x#1 : a
    shout : String -> <IO> Unit
      s#0 : String
    quiet : String -> Unit
      s#0 : String
    ---
    E2002 8:11 `apply` performs `IO`, which the signature of `quiet` does not allow
      8:11 this call performs `IO`
      7:9 the row of this signature does not include it
      help: add `IO` to the row of the signature of `quiet`, as in `-> <IO> ...`
    ");
}

#[test]
fn a_rigid_row_variable_must_be_in_the_ambient_row() {
    let text = "run : (Unit -> <e> Unit) -> Unit\nrun f = f ()";
    insta::assert_snapshot!(check_text(text), @r"
    run : (Unit -> <e> Unit) -> Unit
      f#0 : Unit -> <e> Unit
    ---
    E2002 2:9 `f` performs `e`, which the signature of `run` does not allow
      2:9 this call performs `e`
      1:7 the row of this signature does not include it
      help: add `e` to the row of the signature of `run`, as in `-> <e> ...`
    ");
}

#[test]
fn equations_may_return_functions_and_calls_may_pass_more_arguments() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\nadder : Int -> Int -> Int\nadder x = add x\n\ninc : Int -> Int\ninc = adder 1\n\nthree : Unit -> Int\nthree () = adder 1 2 + inc 1";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
    adder : Int -> Int -> Int
      x#0 : Int
    inc : Int -> Int
    three : Unit -> Int
    ");
}

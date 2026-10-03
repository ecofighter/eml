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
fn function_values_are_not_yet_supported() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\npartial : Unit -> Int\npartial () =\n  let f = add 1\n  let g = add\n  0\n\ncurried : Int -> Int -> Int\ncurried a = a";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
    partial : Unit -> Int
      f#0 : {error}
      g#1 : {error}
    curried : Int -> Int -> Int
      a#0 : Int
    ---
    E0004 6:11 partial application is not supported yet
      6:11 this is implemented in a later stage
    E0004 7:11 using a function as a value is not supported yet
      7:11 this is implemented in a later stage
    E0004 11:1 an equation with fewer parameters than arrows in its signature is not supported yet
      11:1 this is implemented in a later stage
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
        main : Unit -> Unit
        ---
        E1002 1:17 cannot find effect `Console`
          1:17 not found in this scope
        ");
}

#[test]
fn function_typed_parameters_cannot_be_called_or_passed() {
    let text = "apply : (Int -> Int) -> Int -> Int\napply f x = f x\n\npass : (Int -> Int) -> Int\npass f = apply f 1";
    insta::assert_snapshot!(check_text(text), @"
    apply : (Int -> Int) -> Int -> Int
      f#0 : Int -> Int
      x#1 : Int
    pass : (Int -> Int) -> Int
      f#0 : Int -> Int
    ---
    E0004 2:13 calling a function value is not supported yet
      2:13 this is implemented in a later stage
    E0004 5:16 using a function as a value is not supported yet
      5:16 this is implemented in a later stage
    ");
}

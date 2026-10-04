mod common;

use common::check_text;

#[test]
fn operations_have_the_row_of_their_effect() {
    let text = "effect Log where\n  log : String -> String -> Unit\n  never fail : String -> a\n\nwarn : String -> <Log> Unit\nwarn m = log \"warn\" m\n\nstop : Unit -> <Log> Int\nstop () = fail \"no\"";
    insta::assert_snapshot!(check_text(text), @r"
    log : String -> String -> <Log> Unit
    fail : String -> <Log> a
    warn : String -> <Log> Unit
      m#0 : String
    stop : Unit -> <Log> Int
    ");
}

#[test]
fn an_unhandled_operation_is_not_in_the_row() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (ask ()))";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Ask> Int
    main : Unit -> <IO> Unit
    ---
    E2002 5:30 `ask` performs `Ask`, which the signature of `main` does not allow
      5:30 this call performs `Ask`
      4:8 the row of this signature does not include it
      help: add `Ask` to the row of the signature of `main`, as in `-> <Ask> ...`
    ");
}

#[test]
fn operations_are_values_and_can_be_partially_applied() {
    let text = "effect Log where\n  log : String -> String -> Unit\n\neach : (String -> <e> Unit) -> <e> Unit\neach f = f \"a\"\n\nrun : Unit -> <Log> Unit\nrun () =\n  let info = log \"info\"\n  each info\n  each (log \"warn\")";
    insta::assert_snapshot!(check_text(text), @r"
    log : String -> String -> <Log> Unit
    each : (String -> <e> Unit) -> <e> Unit
      f#0 : String -> <e> Unit
    run : Unit -> <Log> Unit
      info#0 : String -> <Log> Unit
    ");
}

#[test]
fn a_handler_removes_its_effect_and_types_the_continuation() {
    let text = "effect Ask where\n  ask : String -> Int\n\nrun : Unit -> <Ask, IO> Int\nrun () =\n  let n = ask \"n\"\n  println \"asked\"\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let total =\n    handle run () with\n      | ask key k -> resume k 41\n      | return x -> show_int x\n  println total";
    insta::assert_snapshot!(check_text(text), @r"
    ask : String -> <Ask> Int
    run : Unit -> <Ask, IO> Int
      n#0 : Int
    main : Unit -> <IO> Unit
      key#0 : String
      k#1 : Cont Int String <IO>
      x#2 : Int
      total#3 : String
    ");
}

#[test]
fn type_variables_of_an_operation_are_rigid_in_its_clause() {
    let text = "effect Pick where\n  pick : a -> a -> a\n\nfirst : Unit -> Int\nfirst () =\n  handle pick 1 2 with\n    | pick x y k -> resume k x\n\nwrong : Unit -> Int\nwrong () =\n  handle pick 1 2 with\n    | pick x y k -> resume k 0";
    insta::assert_snapshot!(check_text(text), @r"
    pick : a -> a -> <Pick> a
    first : Unit -> Int
      x#0 : a
      y#1 : a
      k#2 : Cont a Int <>
    wrong : Unit -> Int
      x#0 : a
      y#1 : a
      k#2 : Cont a Int <>
    ---
    E2001 12:30 mismatched types
      12:30 expected `a`, found `Int`
      note: `resume` passes this value as the result of the operation
    ");
}

#[test]
fn resume_needs_a_continuation_and_drop_takes_any_value() {
    let text = "f : Int -> Int\nf n =\n  drop n\n  resume n 1";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
      n#0 : Int
    ---
    E2001 4:10 mismatched types
      4:10 expected `Cont _ _ <_>`, found `Int`
      note: the first argument of `resume` must be a continuation
    ");
}

#[test]
fn the_body_of_a_handler_may_perform_only_the_handled_effect_and_the_outer_row() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nnoisy : Unit -> <IO> Int\nnoisy () =\n  println \"x\"\n  1\n\nf : Unit -> Int\nf () =\n  handle noisy () with\n    | ask () k -> resume k 1";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Ask> Int
    noisy : Unit -> <IO> Int
    f : Unit -> Int
      k#0 : Cont Int Int <>
    ---
    E2002 11:10 `noisy` performs `IO`, which the signature of `f` does not allow
      11:10 this call performs `IO`
      9:5 the row of this signature does not include it
      help: add `IO` to the row of the signature of `f`, as in `-> <IO> ...`
    ");
}

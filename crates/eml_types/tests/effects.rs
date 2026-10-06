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
    insta::assert_snapshot!(check_text(text), @"
    pick : a -> a -> <Pick> a
      kinds: a <= Unr
    first : Unit -> Int
      x#0 : a
      y#1 : a
      k#2 : Cont a Int <>
      $r#3 : Int
    wrong : Unit -> Int
      x#0 : a
      y#1 : a
      k#2 : Cont a Int <>
      $r#3 : Int
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
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    noisy : Unit -> <IO> Int
    f : Unit -> Int
      k#0 : Cont Int Int <>
      $r#1 : Int
    ---
    E2002 11:10 `noisy` performs `IO`, which the signature of `f` does not allow
      11:10 this call performs `IO`
      9:5 the row of this signature does not include it
      help: add `IO` to the row of the signature of `f`, as in `-> <IO> ...`
    ");
}

#[test]
fn a_continuation_of_a_once_operation_must_be_used_exactly_once() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : Unit -> Int\ntwice () =\n  handle ask () with\n    | ask () k -> resume k 1 + resume k 2\n\nunused : Unit -> Int\nunused () =\n  handle ask () with\n    | ask () k -> 0\n\ndiscarded : Unit -> Int\ndiscarded () =\n  handle ask () with\n    | ask () _ -> 0\n\ncaptured : Unit -> <Ask> Int\ncaptured () =\n  handle ask () with\n    | ask () k ->\n        handle ask () with\n          | ask () inner -> resume k (resume inner 1)";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    twice : Unit -> Int
      k#0 : Cont Int Int <>
      $r#1 : Int
    unused : Unit -> Int
      k#0 : Cont Int Int <>
      $r#1 : Int
    discarded : Unit -> Int
      $r#0 : Int
    captured : Unit -> <Ask> Int
      k#0 : Cont Int Int <Ask>
      inner#1 : Cont Int Int <Ask>
      $r#2 : Int
      $r#3 : Int
    ---
    E3002 7:39 `k` must be used exactly once, but it is used more than once
      7:39 used again here
      7:26 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    E3005 12:5 the continuation `k` of a `once` operation must be resumed or dropped
      12:5 this clause
      12:14 `k` is bound here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: call `resume k v` or `drop k` on every path
    E3004 17:14 a linear value cannot be discarded with `_`
      17:14 this pattern discards it
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: bind it to a name and pass the name to `drop`
    E3001 22:14 `k` must be used exactly once, but an operation clause captures it
      22:14 `k` is bound here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: an operation clause runs each time its operation is performed
    ");
}

#[test]
fn a_closure_capturing_a_continuation_cannot_be_used_twice() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : (Int -> Int) -> Int\ntwice f = f (f 0)\n\ng : Unit -> Int\ng () =\n  handle ask () with\n    | ask () k -> twice (fn n -> resume k n)";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    twice : (Int -> Int) -> Int
      kinds: (Int -> Int) <= Unr
      f#0 : Int -> Int
    g : Unit -> Int
      k#0 : Cont Int Int <>
      n#1 : Int
      $r#2 : Int
    ---
    E3001 10:19 a linear value is passed to `twice`, which may use it more than once or not at all
      10:19 `twice` is used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_body_with_a_reported_error_does_not_report_linear_values() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  handle ask () with\n    | ask () k -> resume k";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    f : Unit -> Int
      k#0 : Cont Int Int <>
      $r#1 : Int
    ---
    E1011 7:19 `resume` takes a continuation, a value, and an optional state, but 1 argument was given
      7:19 this `resume`
    ");
}

#[test]
fn a_continuation_cannot_pass_through_a_polymorphic_operation_parameter() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\neffect Sink where\n  sink : a -> Unit\n\neffect Pair where\n  pair : a -> (a -> a -> b) -> b\n\nsunk : Unit -> <Sink> Int\nsunk () =\n  handle ask () with\n    | ask () k ->\n        sink k\n        0\n\npaired : Unit -> <Pair> Int\npaired () =\n  handle ask () with\n    | ask () k ->\n        let f = pair 1 (fn x y -> fn u -> resume k (x + y))\n        f ()";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    sink : a -> <Sink> Unit
      kinds: a <= Unr
    pair : a -> (a -> a -> b) -> <Pair> b
      kinds: a <= Unr, (a -> a -> b) <= Unr, (a -> b) <= Unr, b <= Unr
    sunk : Unit -> <Sink> Int
      k#0 : Cont Int Int <Sink>
      $r#1 : Int
    paired : Unit -> <Pair> Int
      k#0 : Cont Int Int <Pair>
      x#1 : Int
      y#2 : Int
      u#3 : Unit
      f#4 : Unit -> <Pair> Int
      $r#5 : Int
    ---
    E3001 14:9 a linear value is passed to `sink`, which may use it more than once or not at all
      14:9 `sink` is used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    E3001 21:17 a linear value is passed to `pair`, which may use it more than once or not at all
      21:17 `pair` is used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_clause_dropped_by_an_error_does_not_cause_linear_misuse() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\neffect Log where\n  log : String -> Unit\n\nf : Unit -> Int\nf () =\n  handle ask () with\n    | ask () k ->\n        handle 1 with\n          | log m k2 -> resume k2 ()\n          | return x -> x\n          | return y -> resume k y";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    log : String -> <Log> Unit
    f : Unit -> Int
      k#0 : Cont Int Int <>
      m#1 : String
      k2#2 : Cont Unit Int <>
      x#3 : Int
      $r#5 : Int
    ---
    E1014 14:11 this handler has more than one `return` clause
      14:11 another `return` clause
      13:11 the first `return` clause
    ");
}

#[test]
fn type_parameters_of_an_effect_are_not_fixed_to_unr() {
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\neffect Store s where\n  store : a -> s -> Unit\n\ncounter : Unit -> <State Int> Int\ncounter () =\n  put (get () + 1)\n  get ()";
    insta::assert_snapshot!(check_text(text), @r"
    get : Unit -> <State s> s
    put : s -> <State s> Unit
    store : a -> s -> <Store s> Unit
      kinds: a <= Unr
    counter : Unit -> <State Int> Int
    ");
}

#[test]
fn type_arguments_of_a_performed_effect_must_match_the_row() {
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\nwrong : Unit -> <State Int> Unit\nwrong () = put \"text\"";
    insta::assert_snapshot!(check_text(text), @r"
    get : Unit -> <State s> s
    put : s -> <State s> Unit
    wrong : Unit -> <State Int> Unit
    ---
    E2001 6:12 `put` performs `State String`, but the row allows `State Int`
      6:12 this call performs `State String`
      note: the type arguments of an effect must match those in the row
    ");
}

#[test]
fn a_handler_takes_the_type_arguments_of_its_effect_from_the_body() {
    let text = "effect Reader r where\n  ask : Unit -> r\n\ngreeting : Unit -> <Reader String> String\ngreeting () = ask ()\n\nrun : Unit -> String\nrun () =\n  handle greeting () with\n    | ask () k -> resume k \"x\"\n\nwrong : Unit -> String\nwrong () =\n  handle greeting () with\n    | ask () k -> resume k 1";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Reader r> r
    greeting : Unit -> <Reader String> String
    run : Unit -> String
      k#0 : Cont String String <>
      $r#1 : String
    wrong : Unit -> String
      k#0 : Cont String String <>
      $r#1 : String
    ---
    E2001 15:28 mismatched types
      15:28 expected `String`, found `Int`
      note: `resume` passes this value as the result of the operation
    ");
}

#[test]
fn a_function_type_with_other_effect_arguments_is_a_mismatch() {
    // `f` を `drop` するのは、使わない引数の `Unr` の制約を `kinds:` の行に出さないため
    let text = "effect Reader r where\n  ask : Unit -> r\n\nrun : (Unit -> <Reader String> Int) -> Int\nrun f =\n  drop f\n  0\n\nnumber : Unit -> <Reader Int> Int\nnumber () = ask ()\n\napply_it : Unit -> Int\napply_it () = run number";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Reader r> r
    run : (Unit -> <Reader String> Int) -> Int
      f#0 : Unit -> <Reader String> Int
    number : Unit -> <Reader Int> Int
    apply_it : Unit -> Int
    ---
    E2001 13:19 mismatched types
      13:19 expected `Unit -> <Reader String> Int`, found `Unit -> <Reader Int | _> Int`
      13:15 argument 1 of `run`
    ");
}

#[test]
fn a_continuation_of_a_multi_operation_may_be_resumed_twice() {
    let text = "effect Choice where\n  multi choose : Unit -> Bool\n\nboth : Unit -> Int\nboth () =\n  handle (if choose () then 1 else 2) with\n    | choose () k -> resume k True + resume k False";
    insta::assert_snapshot!(check_text(text), @"
    choose : Unit -> <Choice> Bool
    both : Unit -> Int
      k#0 : Cont Bool Int <>
      $r#1 : Int
    ");
}

#[test]
fn the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\neffect Choice where\n  multi choose : Unit -> Bool\n\ncaptured : Unit -> Int\ncaptured () =\n  handle ask () with\n    | ask () k ->\n        handle (if choose () then 1 else 2) with\n          | choose () c -> resume c True + resume c False\n          | return n -> resume k n";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    choose : Unit -> <Choice> Bool
    captured : Unit -> Int
      k#0 : Cont Int Int <>
      c#1 : Cont Bool Int <>
      n#2 : Int
      $r#3 : Int
    ---
    E3006 11:9 `k` must be used exactly once, but it is kept alive across a call that may resume more than once
      11:9 this handle may perform `choose`, a `multi` operation
      13:11 the `return` clause captures `k`
      5:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: do not capture `k` in the `return` clause
    ");
}

#[test]
fn the_return_clause_of_a_multi_handler_may_capture_an_unrestricted_value() {
    let text = "effect Choice where\n  multi choose : Unit -> Bool\n\ncaptured : Int -> Int\ncaptured base =\n  handle (if choose () then 1 else 2) with\n    | choose () c -> resume c True + resume c False\n    | return n -> n + base";
    insta::assert_snapshot!(check_text(text), @r"
    choose : Unit -> <Choice> Bool
    captured : Int -> Int
      base#0 : Int
      c#1 : Cont Bool Int <>
      n#2 : Int
    ");
}

#[test]
fn continuations_of_handlers_with_a_state_carry_the_state() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  handle ask () from \"s\" with\n    | ask () k st -> resume k 1 st\n    | return x st -> x\n\ng : Unit -> Int\ng () =\n  handle ask () from () with\n    | ask () k st -> resume k 1 st\n    | return x st -> x";
    let dump = check_text(text);
    assert!(
        dump.contains("k#") && dump.contains(": Cont Int Int <> from String"),
        "{dump}"
    );
    assert!(dump.contains(": Cont Int Int <> from Unit"), "{dump}");
    assert!(!dump.contains("E2"), "{dump}");
}

#[test]
fn a_continuation_with_a_state_is_resumed_through_a_lambda_and_more_than_once() {
    let text = "effect Choose where\n  multi choose : Unit -> Bool\n\nf : Unit -> Int\nf () =\n  handle (if choose () then 1 else 2) from 0 with\n    | choose () k st ->\n        let again = fn c -> resume c True (st + 1)\n        again k + resume k False (st + 2)\n    | return x st -> x + st";
    let checked = eml_test_support::check(text);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
}

#[test]
fn resume_must_match_the_state_of_its_continuation() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmissing : Unit -> Int\nmissing () =\n  handle ask () from 0 with\n    | ask () k st -> resume k st\n    | return x st -> x\n\nextra : Unit -> Int\nextra () =\n  handle ask () with\n    | ask () k -> resume k 1 2";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    missing : Unit -> Int
      k#0 : Cont Int Int <> from Int
      st#1 : Int
      x#2 : Int
      st#3 : Int
    extra : Unit -> Int
      k#0 : Cont Int Int <>
      $r#1 : Int
    ---
    E2007 7:22 this continuation comes from a handler with a state, so `resume` needs the next state
      7:22 the next state is missing
      help: pass the next state as the third argument: `resume k v st`
    E2007 13:19 this continuation comes from a handler without a state, so `resume` takes no state
      13:19 the state argument is not expected
      help: remove the third argument
    ");
}

#[test]
fn a_continuation_passed_to_a_lambda_is_checked_where_the_states_meet() {
    // ラムダの中の2引数の `resume` が欄を「状態なし」にし、状態のある `k` を渡したところで食い違う
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  handle ask () from 0 with\n    | ask () k st ->\n        let again = fn c -> resume c 1\n        again k\n    | return x _ -> x";
    let checked = eml_test_support::check(text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E2007"]);
}

#[test]
fn a_handler_of_an_unknown_effect_still_checks_its_initial_state() {
    let text = "f : Unit -> Int\nf () =\n  handle 1 from (1 + \"a\") with\n    | nope () k st -> resume k 1 st\n    | return x st -> x";
    let checked = eml_test_support::check(text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert!(codes.contains(&"E1001".to_string()), "{codes:?}");
    assert!(codes.contains(&"E2001".to_string()), "{codes:?}");
}

fn fix_text(text: &str) -> String {
    let checked = eml_test_support::check(text);
    eml_test_support::fixes(&checked.files, &checked.diagnostics)
}

const ASK: &str = "effect Ask where\n  ask : Unit -> Int\n\n";

#[test]
fn the_state_argument_is_removed_from_a_continuation_without_a_state() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () with\n    | ask () k -> resume k 1 2"
    );
    insta::assert_snapshot!(fix_text(&text), @r#"
    E2007 7:19 remove the state argument
      7:29..7:31 ""
    "#);
}

#[test]
fn the_current_state_is_passed_when_the_clause_binds_it_to_a_name() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () from 0 with\n    | ask () k (st : Int) -> resume k st\n    | return x _ -> x"
    );
    insta::assert_snapshot!(fix_text(&text), @r#"
    E2007 7:30 pass the current state `st`
      7:41..7:41 " st"
    "#);
}

#[test]
fn no_state_is_passed_when_the_state_is_a_pattern() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () from (0, 0) with\n    | ask () k (a, b) -> resume k a\n    | return x _ -> x"
    );
    assert_eq!(fix_text(&text), "");
}

#[test]
fn no_state_is_passed_when_its_name_is_shadowed() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () from 0 with\n    | ask () k st ->\n        let st = 5\n        resume k st\n    | return x _ -> x"
    );
    assert_eq!(fix_text(&text), "");
}

#[test]
fn no_state_is_passed_for_a_continuation_outside_its_clause() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () from 0 with\n    | ask () k st ->\n        let j = k\n        resume j st\n    | return x _ -> x"
    );
    let fixes = fix_text(&text);
    assert!(!fixes.contains("pass the current state"), "{fixes}");
}

#[test]
fn the_state_of_the_clause_that_binds_k_is_passed_in_nested_handlers() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () from 0 with\n    | ask () k outer ->\n        handle ask () from 1 with\n          | ask () j inner -> resume k inner\n          | return y _ -> y\n    | return x _ -> x"
    );
    insta::assert_snapshot!(fix_text(&text), @r#"
    E2007 9:31 pass the current state `outer`
      9:45..9:45 " outer"
    "#);
}

#[test]
fn the_state_argument_is_removed_up_to_its_closing_paren() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () with\n    | ask () k -> resume k 1 (2)"
    );
    insta::assert_snapshot!(fix_text(&text), @r#"
    E2007 7:19 remove the state argument
      7:29..7:33 ""
    "#);
}

#[test]
fn the_state_argument_is_removed_after_the_closing_paren_of_the_value() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () with\n    | ask () k -> resume k (1) 2"
    );
    insta::assert_snapshot!(fix_text(&text), @r#"
    E2007 7:19 remove the state argument
      7:31..7:33 ""
    "#);
}

#[test]
fn the_current_state_is_passed_after_the_closing_paren_of_the_value() {
    let text = format!(
        "{ASK}f : Unit -> Int\nf () =\n  handle ask () from 0 with\n    | ask () k st -> resume k (st + 1)\n    | return x _ -> x"
    );
    insta::assert_snapshot!(fix_text(&text), @r#"
    E2007 7:22 pass the current state `st`
      7:39..7:39 " st"
    "#);
}

#[test]
fn operations_of_a_duplicate_effect_do_not_cascade() {
    // 今は重複したエフェクトの `y` が使え、`f` の row にない `E` として E2002 が連鎖する
    let text = "effect E where\n  x : Unit -> Int\neffect E where\n  y : Unit -> Int\n\nf : Unit -> Int\nf () = y ()";
    let checked = eml_test_support::check(text);
    assert_eq!(
        eml_test_support::short(&checked.files, &checked.diagnostics),
        ["E1003 3:8 `E` is defined more than once"]
    );
}

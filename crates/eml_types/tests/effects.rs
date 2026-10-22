use crate::common::{check_full, check_short, check_text};

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
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () = println (show (ask ()))";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    main : Unit -> <IO> Unit
    ---
    E2002 5:26 `ask` performs `Ask`, which the signature of `main` does not allow
      5:26 this call performs `Ask`
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
    let text = "effect Ask where\n  ask : String -> Int\n\nrun : Unit -> <Ask, IO, Panic> Int\nrun () =\n  let n = ask \"n\"\n  println \"asked\"\n  n + 1\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let total =\n    handle run () with\n      | ctl ask key k -> k 41\n      | return x -> show x\n  println total";
    insta::assert_snapshot!(check_text(text), @"
    ask : String -> <Ask> Int
    run : Unit -> <Ask, IO, Panic> Int
      n#0 : Int
    main : Unit -> <IO, Panic> Unit
      key#0 : String
      k#1 : once Int -> <IO, Panic> String
      x#2 : Int
      total#3 : String
    ");
}

/// 節の `k` は普通の関数型である。状態のある handler では2引数で、row は最後の矢印に付く (docs/spec/effects.md)。
#[test]
fn a_continuation_is_a_function() {
    let stateless = "effect Ask where\n  ask : Unit -> Int\n\nrun : Unit -> <IO> String\nrun () =\n  handle ask () with\n    | ctl ask () k -> k 1\n    | return x -> show x";
    let stateful = "effect Tick where\n  tick : Unit -> Int\n\nrun : Unit -> <Panic> Int\nrun () =\n  handle tick () from \"s\" with\n    | ctl tick () k st -> k 1 st\n    | return x _ -> x";
    insta::assert_snapshot!(check_text(stateless), @"
    ask : Unit -> <Ask> Int
    run : Unit -> <IO> String
      k#0 : once Int -> <IO> String
      x#1 : Int
    ");
    insta::assert_snapshot!(check_text(stateful), @"
    tick : Unit -> <Tick> Int
    run : Unit -> <Panic> Int
      k#0 : once Int -> once String -> <Panic> Int
      st#1 : String
      x#2 : Int
    ");
}

/// 状態のある `multi` の `k` を部分適用したクロージャは、`Lin` の値を捕まえると1回しか呼べない (docs/spec/effects.md)。
#[test]
fn a_partial_continuation_capturing_a_linear_value_is_linear() {
    let text = "effect Pick where\n  multi pick : Unit -> Fs.File\n\nrun : Unit -> <IO> Unit\nrun () =\n  handle pick () from () with\n    | ctl pick () k st ->\n        let g = k (Fs.open \"a\")\n        g st\n        g st\n    | return f _ -> Fs.close f";
    insta::assert_snapshot!(check_text(text), @"
    pick : Unit -> <Pick> File
    run : Unit -> <IO> Unit
      k#0 : File -> once Unit -> <IO> Unit
      st#1 : Unit
      g#2 : once Unit -> <IO> Unit
      f#3 : File
    ---
    E3002 10:9 `g` must be used exactly once, but it is used more than once
      10:9 used again here
      9:9 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

/// 状態のある `once` の操作の `k v` は、状態を受ける関数も `Lin` になるので2回呼べない。`m₂` が `m` を受けることの確認
/// (docs/spec/effects.md)。
#[test]
fn a_partial_continuation_of_a_once_operation_with_a_state_is_linear() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nrun : Unit -> <Panic> Int\nrun () =\n  handle ask () from 0 with\n    | ctl ask () k st ->\n        let g = k 1\n        let a = g st\n        let b = g st\n        a + b\n    | return x _ -> x";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    run : Unit -> <Panic> Int
      k#0 : once Int -> once Int -> <Panic> Int
      st#1 : Int
      g#2 : once Int -> <Panic> Int
      a#3 : Int
      b#4 : Int
      x#5 : Int
    ---
    E3002 10:17 `g` must be used exactly once, but it is used more than once
      10:17 used again here
      9:17 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn type_variables_of_an_operation_are_rigid_in_its_clause() {
    let text = "effect Pick where\n  pick : a -> a -> a\n\nfirst : Unit -> Int\nfirst () =\n  handle pick 1 2 with\n    | ctl pick x y k -> k x\n\nwrong : Unit -> Int\nwrong () =\n  handle pick 1 2 with\n    | ctl pick x y k -> k 0";
    insta::assert_snapshot!(check_text(text), @"
    pick : a -> a -> <Pick> a
      kinds: a <= Unr
    first : Unit -> Int
      x#0 : a
      y#1 : a
      k#2 : once a -> Int
      $r#3 : Int
    wrong : Unit -> Int
      x#0 : a
      y#1 : a
      k#2 : once a -> Int
      $r#3 : Int
    ---
    E2001 12:27 mismatched types
      12:27 expected `a`, found `Int`
      12:25 argument 1 of `k`
    ");
}

#[test]
fn drop_takes_any_value_and_a_non_function_cannot_be_called() {
    let text = "f : Int -> Int\nf n =\n  drop n\n  n 1";
    insta::assert_snapshot!(check_text(text), @"
    f : Int -> Int
      n#0 : Int
    ---
    E2001 4:5 `n` is not a function
      4:5 unexpected argument
    ");
}

#[test]
fn the_body_of_a_handler_may_perform_only_the_handled_effect_and_the_outer_row() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nnoisy : Unit -> <IO> Int\nnoisy () =\n  println \"x\"\n  1\n\nf : Unit -> Int\nf () =\n  handle noisy () with\n    | ctl ask () k -> k 1";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    noisy : Unit -> <IO> Int
    f : Unit -> Int
      k#0 : once Int -> Int
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
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : Unit -> <Panic> Int\ntwice () =\n  handle ask () with\n    | ctl ask () k -> k 1 + k 2\n\nunused : Unit -> Int\nunused () =\n  handle ask () with\n    | ctl ask () k -> 0\n\ndiscarded : Unit -> Int\ndiscarded () =\n  handle ask () with\n    | ctl ask () _ -> 0\n\ncaptured : Unit -> <Ask> Int\ncaptured () =\n  handle ask () with\n    | ctl ask () k ->\n        handle ask () with\n          | ctl ask () inner -> k (inner 1)";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    twice : Unit -> <Panic> Int
      k#0 : once Int -> <Panic> Int
      $r#1 : Int
    unused : Unit -> Int
      k#0 : once Int -> Int
      $r#1 : Int
    discarded : Unit -> Int
      $r#0 : Int
    captured : Unit -> <Ask> Int
      k#0 : once Int -> <Ask> Int
      inner#1 : once Int -> <Ask> Int
      $r#2 : Int
      $r#3 : Int
    ---
    E3002 7:29 `k` must be used exactly once, but it is used more than once
      7:29 used again here
      7:23 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    E3005 12:5 the continuation `k` of a `once` operation must be called or dropped
      12:5 this clause
      12:18 `k` is bound here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: call `k` or `drop k` on every path
    E3004 17:18 a linear value cannot be discarded with `_`
      17:18 this pattern discards it
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: bind it to a name and pass the name to `drop`
    E3001 22:18 `k` must be used exactly once, but an operation clause captures it
      22:18 `k` is bound here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: an operation clause runs each time its operation is performed
    ");
}

#[test]
fn a_closure_capturing_a_continuation_cannot_be_used_twice() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : (Int -> Int) -> Int\ntwice f = f (f 0)\n\ng : Unit -> Int\ng () =\n  handle ask () with\n    | ctl ask () k -> twice (fn n -> k n)";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    twice : (Int -> Int) -> Int
      kinds: (Int -> Int) <= Unr
      f#0 : Int -> Int
    g : Unit -> Int
      k#0 : once Int -> Int
      n#1 : Int
      $r#2 : Int
    ---
    E3001 10:23 a linear value is passed to `twice`, which may use it more than once or not at all
      10:23 `twice` is used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_body_with_a_reported_error_does_not_report_linear_values() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  handle ask () with\n    | ctl ask () k -> k";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    f : Unit -> Int
      k#0 : once Int -> Int
      $r#1 : Int
    ---
    E2001 7:23 mismatched types
      7:23 expected `Int`, found `once Int -> Int`
      note: each clause of a handler must have the type of the whole `handle` expression
    ");
}

#[test]
fn a_continuation_cannot_pass_through_a_polymorphic_operation_parameter() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\neffect Sink where\n  sink : a -> Unit\n\neffect Pair where\n  pair : a -> (a -> a -> b) -> b\n\nsunk : Unit -> <Sink> Int\nsunk () =\n  handle ask () with\n    | ctl ask () k ->\n        sink k\n        0\n\npaired : Unit -> <Pair, Panic> Int\npaired () =\n  handle ask () with\n    | ctl ask () k ->\n        let f = pair 1 (fn x y -> fn u -> k (x + y))\n        f ()";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    sink : a -> <Sink> Unit
      kinds: a <= Unr
    pair : a -> (a -> a -> b) -> <Pair> b
      kinds: a <= Unr, (a -> a -> b) <= Unr, (a -> b) <= Unr, b <= Unr
    sunk : Unit -> <Sink> Int
      k#0 : once Int -> <Sink> Int
      $r#1 : Int
    paired : Unit -> <Pair, Panic> Int
      k#0 : once Int -> <Pair, Panic> Int
      x#1 : Int
      y#2 : Int
      u#3 : Unit
      f#4 : once Unit -> <Panic, Pair> Int
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
    let text = "effect Ask where\n  ask : Unit -> Int\n\neffect Log where\n  log : String -> Unit\n\nf : Unit -> Int\nf () =\n  handle ask () with\n    | ctl ask () k ->\n        handle 1 with\n          | ctl log m k2 -> k2 ()\n          | return x -> x\n          | return y -> k y";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    log : String -> <Log> Unit
    f : Unit -> Int
      k#0 : once Int -> Int
      m#1 : String
      k2#2 : once Unit -> Int
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
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\neffect Store s where\n  store : a -> s -> Unit\n\ncounter : Unit -> <State Int, Panic> Int\ncounter () =\n  put (get () + 1)\n  get ()";
    insta::assert_snapshot!(check_text(text), @r"
    get : Unit -> <State s> s
    put : s -> <State s> Unit
    store : a -> s -> <Store s> Unit
      kinds: a <= Unr
    counter : Unit -> <State Int, Panic> Int
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
    let text = "effect Reader r where\n  ask : Unit -> r\n\ngreeting : Unit -> <Reader String> String\ngreeting () = ask ()\n\nrun : Unit -> String\nrun () =\n  handle greeting () with\n    | ctl ask () k -> k \"x\"\n\nwrong : Unit -> String\nwrong () =\n  handle greeting () with\n    | ctl ask () k -> k 1";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Reader r> r
    greeting : Unit -> <Reader String> String
    run : Unit -> String
      k#0 : once String -> String
      $r#1 : String
    wrong : Unit -> String
      k#0 : once String -> String
      $r#1 : String
    ---
    E2001 15:25 mismatched types
      15:25 expected `String`, found `Int`
      15:23 argument 1 of `k`
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
fn a_continuation_of_a_multi_operation_may_be_called_twice() {
    let text = "effect Choice where\n  multi choose : Unit -> Bool\n\nboth : Unit -> <Panic> Int\nboth () =\n  handle (if choose () then 1 else 2) with\n    | ctl choose () k -> k True + k False";
    insta::assert_snapshot!(check_text(text), @"
    choose : Unit -> <Choice> Bool
    both : Unit -> <Panic> Int
      k#0 : Bool -> <Panic> Int
      $r#1 : Int
    ");
}

#[test]
fn the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\neffect Choice where\n  multi choose : Unit -> Bool\n\ncaptured : Unit -> <Panic> Int\ncaptured () =\n  handle ask () with\n    | ctl ask () k ->\n        handle (if choose () then 1 else 2) with\n          | ctl choose () c -> c True + c False\n          | return n -> k n";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    choose : Unit -> <Choice> Bool
    captured : Unit -> <Panic> Int
      k#0 : once Int -> <Panic> Int
      c#1 : Bool -> <Panic> Int
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

/// `IO` は2回以上は再開しないエフェクトなので、`multi` の操作の節の中でも、`IO` を起こす呼び出しをまたいで線形の値を
/// 持ち越せる (docs/spec/types.md の「Kind」)。このテストは `Once` と `Never` を見分けない。多重度が `Once` であることは
/// `table` の単体テスト `an_extern_effect_is_performed_at_most_once` が確かめる。
#[test]
fn a_linear_value_may_be_kept_across_an_io_call_in_a_multi_clause() {
    let text = "effect Choice where\n  multi choose : Unit -> Bool\n\nsay : String -> <IO> Unit\nsay s = println s\n\nboth : Unit -> <IO, Panic> Int\nboth () =\n  handle (if choose () then 1 else 2) with\n    | ctl choose () k ->\n        let file = Fs.open \"a\"\n        say \"x\"\n        Fs.close file\n        k True + k False";
    insta::assert_snapshot!(check_text(text), @"
    choose : Unit -> <Choice> Bool
    say : String -> <IO> Unit
      s#0 : String
    both : Unit -> <IO, Panic> Int
      k#0 : Bool -> <IO, Panic> Int
      file#1 : File
      $r#2 : Int
    ");
}

#[test]
fn the_return_clause_of_a_multi_handler_may_capture_an_unrestricted_value() {
    let text = "effect Choice where\n  multi choose : Unit -> Bool\n\ncaptured : Int -> <Panic> Int\ncaptured base =\n  handle (if choose () then 1 else 2) with\n    | ctl choose () c -> c True + c False\n    | return n -> n + base";
    insta::assert_snapshot!(check_text(text), @"
    choose : Unit -> <Choice> Bool
    captured : Int -> <Panic> Int
      base#0 : Int
      c#1 : Bool -> <Panic> Int
      n#2 : Int
    ");
}

#[test]
fn continuations_of_handlers_with_a_state_carry_the_state() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  handle ask () from \"s\" with\n    | ctl ask () k st -> k 1 st\n    | return x st -> x\n\ng : Unit -> Int\ng () =\n  handle ask () from () with\n    | ctl ask () k st -> k 1 st\n    | return x st -> x";
    let dump = check_text(text);
    assert!(
        dump.contains("k#") && dump.contains(": once Int -> once String -> Int"),
        "{dump}"
    );
    assert!(dump.contains(": once Int -> once Unit -> Int"), "{dump}");
    assert!(!dump.contains("E2"), "{dump}");
}

#[test]
fn a_continuation_with_a_state_is_called_through_a_lambda_and_more_than_once() {
    let text = "effect Choose where\n  multi choose : Unit -> Bool\n\nf : Unit -> <Panic> Int\nf () =\n  handle (if choose () then 1 else 2) from 0 with\n    | ctl choose () k st ->\n        let again = fn c -> c True (st + 1)\n        again k + k False (st + 2)\n    | return x st -> x + st";
    let checked = eml_test_support::check(text);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
}

#[test]
fn the_arity_of_a_continuation_follows_the_state_of_its_handler() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmissing : Unit -> Int\nmissing () =\n  handle ask () from 0 with\n    | ctl ask () k st -> k st\n    | return x st -> x\n\nextra : Unit -> Int\nextra () =\n  handle ask () with\n    | ctl ask () k -> k 1 2";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    missing : Unit -> Int
      k#0 : once Int -> once Int -> Int
      st#1 : Int
      x#2 : Int
      st#3 : Int
    extra : Unit -> Int
      k#0 : once Int -> Int
      $r#1 : Int
    ---
    E2001 7:26 mismatched types
      7:26 expected `Int`, found `once Int -> Int`
      note: each clause of a handler must have the type of the whole `handle` expression
    E2001 13:27 `k` takes 1 argument but 2 were given
      13:27 unexpected argument
    ");
}

#[test]
fn a_continuation_with_a_state_called_without_it_in_a_lambda_is_a_mismatch() {
    // ラムダの中の `c 1` は `c` に引数を1つだけ渡す。状態のある `k` を渡すと、`again k` が部分適用の関数になり、
    // 節の型と食い違う
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  handle ask () from 0 with\n    | ctl ask () k st ->\n        let again = fn c -> c 1\n        again k\n    | return x _ -> x";
    insta::assert_snapshot!(check_full(text), @"
    E2001 9:9 mismatched types
      9:9 expected `Int`, found `once Int -> Int`
      note: each clause of a handler must have the type of the whole `handle` expression
    ");
}

#[test]
fn a_handler_of_an_unknown_effect_still_checks_its_initial_state() {
    let text = "f : Unit -> <Panic> Int\nf () =\n  handle 1 from (1 + \"a\") with\n    | ctl nope () k st -> k 1 st\n    | return x st -> x";
    let checked = eml_test_support::check(text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert!(codes.contains(&"E1001".to_string()), "{codes:?}");
    assert!(codes.contains(&"E2001".to_string()), "{codes:?}");
}

#[test]
fn operations_of_a_duplicate_effect_do_not_cascade() {
    // 重複したエフェクトの操作 `y` は使えない印を持つ。使えると、`f` の row にない `E` として E2002 が連鎖する
    let text = "effect E where\n  x : Unit -> Int\neffect E where\n  y : Unit -> Int\n\nf : Unit -> Int\nf () = y ()";
    let checked = eml_test_support::check(text);
    assert_eq!(
        eml_test_support::short(checked.files(), &checked.diagnostics),
        ["E1003 3:8 `E` is defined more than once"]
    );
}

/// 線形性だけが食い違う E2001 は、`Lin` の矢印に `once` を付けて書くので、note を足さなくても食い違いが読める
/// (docs/spec/types.md の「Kind の書き出し」)。
#[test]
fn a_linear_arrow_is_displayed_as_once() {
    let text = "data Box =\n  | Box (Int -> Int)\n  | Done\n\neffect Ask where\n  ask : Unit -> Int\n\npick : Bool -> Box -> Int\npick c b =\n  match b with\n    | Box g ->\n        handle ask () with\n          | ctl ask () k ->\n              match Box k with\n                | Box h -> h 1\n                | Done -> 0\n          | return x -> x\n    | Done -> 0";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    pick : Bool -> Box -> Int
      c#0 : Bool
      b#1 : Box
      g#2 : Int -> Int
      k#3 : once Int -> Int
      h#4 : Int -> Int
      x#5 : Int
    ---
    E2001 14:25 mismatched types
      14:25 expected `Int -> Int`, found `once Int -> Int`
      14:21 argument 1 of `Box`
    ");
}

/// 線形性の食い違いが大きな型の中にあっても、`once` は入れ子の矢印に付く。
#[test]
fn a_linear_arrow_nested_in_a_tuple_is_displayed_as_once() {
    let text = "data Box =\n  | Box (Int -> Int)\n  | Done\n\ndata Pair =\n  | Pair (Int -> Int, Int)\n\neffect Ask where\n  ask : Unit -> Int\n\npick : Bool -> Box -> Int\npick c b =\n  match b with\n    | Box g ->\n        handle ask () with\n          | ctl ask () k ->\n              match Pair (k, 1) with\n                | Pair (f, n) -> f n\n          | return x -> x\n    | Done -> 0";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    pick : Bool -> Box -> Int
      c#0 : Bool
      b#1 : Box
      g#2 : Int -> Int
      k#3 : once Int -> Int
      f#4 : Int -> Int
      n#5 : Int
      x#6 : Int
    ---
    E2001 17:26 mismatched types
      17:26 expected `(Int -> Int, Int)`, found `(once Int -> Int, Int)`
      17:21 argument 1 of `Pair`
    ");
}

#[test]
fn once_arrows_in_argument_position_are_parenthesized() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nkeep : ((once Int -> Int) -> Int) -> Int\nkeep g = handle ask () with\n  | ctl ask () k -> g k\n  | return x -> x";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    keep : ((once Int -> Int) -> Int) -> Int
      kinds: ((once Int -> Int) -> Int) <= Unr
      g#0 : (once Int -> Int) -> Int
      k#1 : once Int -> Int
      x#2 : Int
    ");
}

// 操作の引数の型に現れない操作ごとの型変数は、操作を呼ぶ側が `Lin` の型で具体化できるので、節ではその型の値を線形に
// 扱う (docs/spec/effects.md の「handler の意味」)
const BOX: &str = "data Box a =\n  | Box (a -> (a, a))\n\ndata Sink a =\n  | Sink (a -> Unit)\n\n";

#[test]
fn a_clause_cannot_copy_a_value_of_a_result_only_operation_variable() {
    let text = format!(
        "{BOX}effect Mk where\n  mk : Unit -> Box a\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match mk () with\n    | Box _ -> ()) with\n    | ctl mk () k -> k (Box (fn x -> (x, x)))"
    );
    insta::assert_snapshot!(check_text(&text), @"
    mk : Unit -> <Mk> Box a
    main : Unit -> <IO> Unit
      k#0 : once Box a -> <IO> Unit
      x#1 : a
      $r#2 : Unit
    ---
    E3002 14:42 `x` must be used exactly once, but it is used more than once
      14:42 used again here
      14:39 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_clause_cannot_discard_a_value_of_a_result_only_operation_variable() {
    let text = format!(
        "{BOX}effect Mk where\n  sink : Unit -> Sink a\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match sink () with\n    | Sink _ -> ()) with\n    | ctl sink () k -> k (Sink (fn _ -> ()))"
    );
    insta::assert_snapshot!(check_text(&text), @"
    sink : Unit -> <Mk> Sink a
    main : Unit -> <IO> Unit
      k#0 : once Sink a -> <IO> Unit
      $r#1 : Unit
    ---
    E3004 14:36 a linear value cannot be discarded with `_`
      14:36 this pattern discards it
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: bind it to a name and pass the name to `drop`
    ");
}

#[test]
fn a_clause_may_copy_a_value_of_an_operation_variable_in_a_parameter() {
    let text = "effect Put where\n  put : a -> Unit\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle put 1 with\n    | ctl put x k ->\n        let p = (x, x)\n        k ()";
    insta::assert_snapshot!(check_text(text), @"
    put : a -> <Put> Unit
      kinds: a <= Unr
    main : Unit -> <IO> Unit
      x#0 : a
      k#1 : once Unit -> <IO> Unit
      p#2 : (a, a)
      $r#3 : Unit
    ");
}

// 操作の型の矢印は、呼ぶ側から節へ渡る位置 (引数と、結果の中で節が受け取る位置) では宣言で `Unr` に固定し、節が
// 作って渡す位置では節の中で `Unr` にする。操作ごとの row 変数は、節の中では `multi` の操作を含みうる
// (docs/spec/effects.md の「handler の意味」)
const REF: &str = "data Ref a =\n  | Ref (a -> Unit) a\n\n";

#[test]
fn a_linear_closure_cannot_enter_a_contravariant_argument_of_data_in_the_result() {
    let text = "data Sink a =\n  | Sink (a -> <IO> Unit)\n\neffect Mk where\n  mk : Unit -> Sink (Unit -> <IO> Unit)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let h = Fs.open \"a\"\n  handle (match mk () with\n    | Sink put -> put (fn () -> Fs.close h)) with\n    | ctl mk () k ->\n        k (Sink (fn f ->\n          f ()\n          f ()))";
    insta::assert_snapshot!(check_text(text), @"
    mk : Unit -> <Mk> Sink (Unit -> <IO> Unit)
    main : Unit -> <IO> Unit
      h#0 : File
      put#1 : (Unit -> <IO> Unit) -> <IO> Unit
      k#2 : once Sink (Unit -> <IO> Unit) -> <IO> Unit
      f#3 : Unit -> <IO> Unit
      $r#4 : Unit
    ---
    E3001 11:24 a lambda that captures a linear value is used where it may be called any number of times
      11:24 this lambda
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn the_performer_may_call_a_function_the_clause_gives_more_than_once() {
    let text = "effect Mk where\n  mk : Unit -> (Unit -> <IO> Unit, Int)\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match mk () with\n    | (g, _) ->\n        g ()\n        g ()) with\n    | ctl mk () k -> k (fn () -> println \"g\", 1)";
    insta::assert_snapshot!(check_text(text), @"
    mk : Unit -> <Mk> (Unit -> <IO> Unit, Int)
    main : Unit -> <IO> Unit
      g#0 : Unit -> <IO> Unit
      k#1 : once (Unit -> <IO> Unit, Int) -> <IO> Unit
      $r#2 : Unit
    ");
}

#[test]
fn a_clause_cannot_give_a_linear_closure_through_two_contravariant_data_arguments() {
    let text = "data Sink a =\n  | Sink (a -> <IO> Unit)\n\neffect Mk where\n  mk : Unit -> (Sink (Sink (Unit -> <IO> Unit)), Int)\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match mk () with\n    | (Sink s, _) -> s (Sink (fn g -> g ()))) with\n    | ctl mk () k -> k (Sink (fn sk ->\n        let f = Fs.open \"a\"\n        match sk with\n          | Sink h -> h (fn () -> Fs.close f)), 0)";
    insta::assert_snapshot!(check_text(text), @"
    mk : Unit -> <Mk> (Sink (Sink (Unit -> <IO> Unit)), Int)
    main : Unit -> <IO> Unit
      s#0 : Sink (Unit -> <IO> Unit) -> <IO> Unit
      g#1 : Unit -> <IO> Unit
      k#2 : once (Sink (Sink (once Unit -> <IO> Unit)), Int) -> <IO> Unit
      sk#3 : Sink (once Unit -> <IO> Unit)
      f#4 : File
      h#5 : (once Unit -> <IO> Unit) -> <IO> Unit
      $r#6 : Unit
    ---
    E3001 11:5 a linear value is used where the type of `mk` requires a value that may be used any number of times
      11:5 this clause handles `mk`
      14:26 this lambda captures a linear value
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: the code that performs `mk` may call the functions in its result any number of times
    ");
}

#[test]
fn a_twice_flipped_arrow_is_produced_by_the_clause() {
    // `inner` は、呼ぶ側の関数の引数の引数なので、向きが2回反転して節が作って渡す位置にある
    let text = "effect Mk where\n  mk : Unit -> (((Unit -> <IO> Unit) -> <IO> Unit) -> <IO> Unit, Int)\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match mk () with\n    | (run, _) -> run (fn inner ->\n        inner ()\n        inner ())) with\n    | ctl mk () k ->\n        let h = Fs.open \"a\"\n        k (fn cb -> cb (fn () -> Fs.close h), 0)";
    insta::assert_snapshot!(check_text(text), @"
    mk : Unit -> <Mk> (((Unit -> <IO> Unit) -> <IO> Unit) -> <IO> Unit, Int)
    main : Unit -> <IO> Unit
      run#0 : ((Unit -> <IO> Unit) -> <IO> Unit) -> <IO> Unit
      inner#1 : Unit -> <IO> Unit
      k#2 : once (once ((once Unit -> <IO> Unit) -> <IO> Unit) -> <IO> Unit, Int) -> <IO> Unit
      h#3 : File
      cb#4 : (once Unit -> <IO> Unit) -> <IO> Unit
      $r#5 : Unit
    ---
    E3001 10:5 a linear value is used where the type of `mk` requires a value that may be used any number of times
      10:5 this clause handles `mk`
      12:12 this lambda captures a linear value
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: the code that performs `mk` may call the functions in its result any number of times
    E3001 10:5 a linear value is used where the type of `mk` requires a value that may be used any number of times
      10:5 this clause handles `mk`
      12:25 this lambda captures a linear value
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: the code that performs `mk` may call the functions in its result any number of times
    ");
}

#[test]
fn a_linear_closure_cannot_enter_an_arrow_in_a_row_label_of_the_result() {
    // ラベルの型引数の矢印は両方の向きとして `Unr` に固定するので、`mk` の節が渡す `twice` はそれを2回呼べ、`ask`
    // の答えに線形なクロージャは渡せない
    let text = "effect Ask a where\n  ask : Unit -> a\n\neffect Mk where\n  mk : Unit -> (Unit -> <Ask (Unit -> <IO> Unit), IO> Unit, Int)\n\ntwice : Unit -> <Ask (Unit -> <IO> Unit), IO> Unit\ntwice () =\n  let f = ask ()\n  f ()\n  f ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match mk () with\n    | (g, _) ->\n        handle g () with\n          | ctl ask () k ->\n              let h = Fs.open \"a\"\n              k (fn () -> Fs.close h)) with\n    | ctl mk () k -> k (twice, 0)";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask a> a
    mk : Unit -> <Mk> (Unit -> <Ask (Unit -> <IO> Unit), IO> Unit, Int)
    twice : Unit -> <Ask (Unit -> <IO> Unit), IO> Unit
      kinds: (Unit -> <IO> Unit) <= Unr
      f#0 : Unit -> <IO> Unit
    main : Unit -> <IO> Unit
      g#0 : Unit -> <Ask (Unit -> <IO> Unit), IO> Unit
      k#1 : once (Unit -> <IO> Unit) -> <Mk, IO> Unit
      h#2 : File
      $r#3 : Unit
      k#4 : once (Unit -> <Ask (Unit -> <IO> Unit), IO> Unit, Int) -> <IO> Unit
      $r#5 : Unit
    ---
    E3001 20:18 a lambda that captures a linear value is used where it may be called any number of times
      20:18 this lambda
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_clause_cannot_keep_a_file_across_a_function_with_an_operation_row_variable() {
    let text = "effect Mk where\n  mk : Unit -> ((Unit -> <IO | e> Int) -> <IO | e> Int, Int)\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match mk () with\n    | (run, _) -> println (show (run (fn () -> 1)))) with\n    | ctl mk () k ->\n        k (fn g ->\n          let h = Fs.open \"a\"\n          let x = g ()\n          Fs.close h\n          x, 0)";
    insta::assert_snapshot!(check_text(text), @"
    mk : Unit -> <Mk> ((Unit -> <IO | e> Int) -> <IO | e> Int, Int)
    main : Unit -> <IO> Unit
      run#0 : (Unit -> <IO> Int) -> <IO> Int
      k#1 : once ((Unit -> <IO | e> Int) -> <IO | e> Int, Int) -> <IO> Unit
      g#2 : Unit -> <IO | e> Int
      h#3 : File
      x#4 : Int
      $r#5 : Unit
    ---
    E3006 11:19 `h` must be used exactly once, but it is kept alive across a call that may resume more than once
      11:19 this call may perform `multi` operations
      10:15 `h` is bound here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `h` before this call
    ");
}

#[test]
fn a_clause_cannot_resume_with_a_linear_closure_where_the_result_has_an_arrow() {
    let text = "effect Mk where\n  mk : Unit -> (Unit -> <IO> Unit, Int)\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match mk () with\n    | (g, _) -> g ()) with\n    | ctl mk () k ->\n        let h = Fs.open \"a\"\n        k (fn () -> Fs.close h, 1)";
    insta::assert_snapshot!(check_text(text), @"
    mk : Unit -> <Mk> (Unit -> <IO> Unit, Int)
    main : Unit -> <IO> Unit
      g#0 : Unit -> <IO> Unit
      k#1 : once (once Unit -> <IO> Unit, Int) -> <IO> Unit
      h#2 : File
      $r#3 : Unit
    ---
    E3001 8:5 a linear value is used where the type of `mk` requires a value that may be used any number of times
      8:5 this clause handles `mk`
      10:12 this lambda captures a linear value
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: the code that performs `mk` may call the functions in its result any number of times
    ");
}

#[test]
fn a_clause_may_use_a_parameter_arrow_as_a_linear_one() {
    let text = "effect Op where\n  op : (Unit -> <IO> Unit) -> Unit\n\nsame : a -> a -> (a, a)\nsame x y = (x, y)\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle op (fn () -> ()) with\n    | ctl op x k ->\n        let h = Fs.open \"a\"\n        let (p, q) = same x (fn () -> Fs.close h)\n        p ()\n        q ()\n        k ()";
    insta::assert_snapshot!(check_text(text), @"
    op : (Unit -> <IO> Unit) -> <Op> Unit
      kinds: (Unit -> <IO> Unit) <= Unr
    same : a -> a -> (a, a)
      x#0 : a
      y#1 : a
    main : Unit -> <IO> Unit
      x#0 : Unit -> <IO> Unit
      k#1 : once Unit -> <IO> Unit
      h#2 : File
      p#3 : once Unit -> <IO> Unit
      q#4 : once Unit -> <IO> Unit
      $r#5 : Unit
    ");
}

#[test]
fn a_clause_may_call_a_closure_it_receives_through_the_result_more_than_once() {
    let text = "effect Mk where\n  mk : Unit -> ((Unit -> <IO> Unit) -> <IO> Unit, Int)\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle (match mk () with\n    | (run, _) -> run (fn () -> ())) with\n    | ctl mk () k ->\n        let run = fn g ->\n          g ()\n          g ()\n        k (run, 1)";
    insta::assert_snapshot!(check_text(text), @"
    mk : Unit -> <Mk> ((Unit -> <IO> Unit) -> <IO> Unit, Int)
    main : Unit -> <IO> Unit
      run#0 : (Unit -> <IO> Unit) -> <IO> Unit
      k#1 : once ((Unit -> <IO> Unit) -> <IO> Unit, Int) -> <IO> Unit
      g#2 : Unit -> <IO> Unit
      run#3 : (Unit -> <IO> Unit) -> <IO> Unit
      $r#4 : Unit
    ");
}

#[test]
fn a_linear_closure_cannot_be_passed_to_a_function_from_the_result() {
    let text = "effect Mk where\n  mk : Unit -> ((Unit -> <IO> Unit) -> <IO> Unit, Int)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let h = Fs.open \"a\"\n  handle (match mk () with\n    | (run, _) -> run (fn () -> Fs.close h)) with\n    | ctl mk () k -> k (fn g -> g (), 1)";
    insta::assert_snapshot!(check_text(text), @"
    mk : Unit -> <Mk> ((Unit -> <IO> Unit) -> <IO> Unit, Int)
    main : Unit -> <IO> Unit
      h#0 : File
      run#1 : (Unit -> <IO> Unit) -> <IO> Unit
      k#2 : once ((Unit -> <IO> Unit) -> <IO> Unit, Int) -> <IO> Unit
      g#3 : Unit -> <IO> Unit
      $r#4 : Unit
    ---
    E3001 8:24 a lambda that captures a linear value is used where it may be called any number of times
      8:24 this lambda
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_linear_closure_cannot_enter_an_arrow_in_both_directions_of_the_result() {
    let text = format!(
        "{REF}effect Mk where\n  mk : Unit -> Ref (Unit -> <IO> Unit)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let h = Fs.open \"a\"\n  handle (match mk () with\n    | Ref put v ->\n        put (fn () -> Fs.close h)\n        v ()) with\n    | ctl mk () k -> k (Ref (fn _ -> ()) (fn () -> ()))"
    );
    insta::assert_snapshot!(check_text(&text), @"
    mk : Unit -> <Mk> Ref (Unit -> <IO> Unit)
    main : Unit -> <IO> Unit
      h#0 : File
      put#1 : (Unit -> <IO> Unit) -> Unit
      v#2 : Unit -> <IO> Unit
      k#3 : once Ref (Unit -> <IO> Unit) -> <IO> Unit
      $r#4 : Unit
    ---
    E3001 12:14 a lambda that captures a linear value is used where it may be called any number of times
      12:14 this lambda
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

/// 純粋なローカル関数の row は包含を解くときに `<>` に閉じるので、`multi` の操作を起こす関数の中で呼んでも、その呼び出しは
/// 再開しない (docs/spec/types.md の「呼び出しの包含を解く」)。
#[test]
fn a_pure_local_call_inside_multi_keeps_a_file() {
    let text = "effect Choice where\n  multi choose : Unit -> Bool\n\nheld : Unit -> <Choice, IO, Panic> Unit\nheld () =\n  let f = Fs.open \"a\"\n  let count n = if n == 0 then 0 else count (n - 1)\n  let x = count 3\n  Fs.close f\n  if choose () then println (show x) else ()";
    assert_eq!(check_short(text), Vec::<String>::new());
}

#[test]
fn a_linear_local_function_held_across_multi_is_e3006() {
    let text = "effect Choice where\n  multi choose : Unit -> Bool\n\nheld : Unit -> <Choice, IO, Panic> Unit\nheld () =\n  let f = Fs.open \"a\"\n  let close () = Fs.close f\n  let b = choose ()\n  close ()";
    insta::assert_snapshot!(check_full(text), @"
    E3006 8:11 `close` must be used exactly once, but it is kept alive across a call that may resume more than once
      8:11 this call may perform `choose`, a `multi` operation
      7:7 `close` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `close` before this call
    ");
}

/// `fn` の節の本体の型の誤りは、隠れた `k` の引数ではなく、操作の結果の型との誤りとして本体を指す
/// (docs/spec/expressions.md の「handler」)。
#[test]
fn a_fn_clause_body_mismatch_names_the_operation_result() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  handle ask () with\n    | fn ask () -> \"x\"";
    insta::assert_snapshot!(check_full(text), @"
    E2001 7:20 mismatched types
      7:20 expected `Int`, found `String`
      note: the `fn` clause for `ask` must return the result type of `ask`
    ");
}

/// パラメータ付き handler の `fn` の節の本体は、操作の結果と次の状態の組を返す (docs/spec/expressions.md の
/// 「パラメータ付き handler」)。
#[test]
fn a_stateful_fn_clause_body_mismatch_names_the_pair() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : s -> Int\nf s0 =\n  handle ask () from s0 with\n    | fn ask () st -> st\n    | return x _ -> x";
    insta::assert_snapshot!(check_full(text), @"
    E2001 7:23 mismatched types
      7:23 expected `(Int, s)`, found `s`
      note: the `fn` clause for `ask` must return a pair of the result of `ask` and the next state
    ");
}

/// 組の中の関数が起こすエフェクトは、固定点で、その関数を呼ぶ組のどの関数の row にも入る (docs/spec/types.md の
/// 「呼び出しの包含を解く」)。
#[test]
fn a_group_effect_reaches_every_member() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nrun : Int -> <Ask, Panic> Int\nrun n =\n  let f x = if x == 0 then 0 else g (x - 1)\n  and g y = ask () + f y\n  f n";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    run : Int -> <Ask, Panic> Int
      n#0 : Int
      f#1 : Int -> <Panic, Ask> Int
      g#2 : Int -> <Ask, Panic> Int
      x#3 : Int
      y#4 : Int
    ");
}

/// 組の関数は関数ごとに row を持つので、`f` の中で張った handler が処理する `Ask` は `f` の row に出ない。
#[test]
fn a_handler_inside_a_group_keeps_its_effect_inside() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nrun : Int -> <Panic> Int\nrun m =\n  let f n = handle g n with\n    | fn ask () -> 1\n  and g n = if n == 0 then ask () else f (n - 1)\n  f m";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    run : Int -> <Panic> Int
      m#0 : Int
      f#1 : Int -> <Panic> Int
      g#2 : Int -> <Ask, Panic> Int
      n#3 : Int
      $k#4 : once Int -> <Panic> Int
      $r#5 : Int
      $r#6 : Int
      n#7 : Int
    ");
}

/// `h` の `Ask` は、`g` に届いてから `f` に届く。固定点は呼び出しを2回以上たどる。
#[test]
fn a_three_member_fixpoint() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nrun : Int -> <Ask, Panic> Int\nrun n =\n  let f x = if x == 0 then 0 else g (x - 1)\n  and g x = h x\n  and h x = ask () + f x\n  f n";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    run : Int -> <Ask, Panic> Int
      n#0 : Int
      f#1 : Int -> <Panic, Ask> Int
      g#2 : Int -> <Ask, Panic> Int
      h#3 : Int -> <Ask, Panic> Int
      x#4 : Int
      x#5 : Int
      x#6 : Int
    ");
}

/// `f` の row には、`g` の `Ask` を足し終えてから `h` の rigid な `e` を末尾に束縛するので、呼ぶ順序によらず通る
/// (docs/spec/types.md の「推論」)。
#[test]
fn a_rigid_ambient_takes_the_labels_of_a_group_first() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nrun : (Unit -> <e> Int) -> <Ask, Panic | e> Int\nrun h =\n  let f n = h () + g n\n  and g n = ask () + n\n  f 1";
    insta::assert_snapshot!(check_full(text), @"");
}

/// 自分を呼ぶローカル関数 `walk` の本体の中の組が `walk` を呼ぶ。`walk` の `Ask` は、`walk` を直接呼ぶ `even` だけでなく、
/// `even` を呼ぶ `odd` にも届く。`odd` の row を内側の組で先に閉じると、`walk` の `Ask` が入らない。
#[test]
fn a_nested_group_calls_the_enclosing_recursive_function() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nrun : Int -> <Ask, Panic> Int\nrun m =\n  let walk n =\n    let even k = if k == 0 then walk (n - 1) else odd (k - 1)\n    and odd k = if k == 0 then 0 else even (k - 1)\n    if n == 0 then ask () else even n\n  walk m";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    run : Int -> <Ask, Panic> Int
      m#0 : Int
      walk#1 : Int -> <Ask, Panic> Int
      n#2 : Int
      even#3 : Int -> <Panic, Ask> Int
      odd#4 : Int -> <Panic, Ask> Int
      k#5 : Int
      k#6 : Int
    ");
}

/// `run_ask g` で `g` の row が `<Ask | ρ>` (`ρ` は `f` の row) になる。`f` の中の `g ()` の位置で足りない `Ask` を `f` の
/// row に足すと、`g` の row も伸びる。有限の row の解はないので、固定点は足すのをやめ、包含が E2002 を報告する。
#[test]
fn a_group_member_sharing_its_tail_with_the_caller_stops_the_fixpoint() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nrun_ask : (Unit -> <Ask | e> Int) -> <e> Int\nrun_ask action = handle action () with\n  | fn ask () -> 1\n\nrun : Unit -> <Panic> Int\nrun () =\n  let f n = run_ask g + g ()\n  and g () = ask () + f 0\n  f 1";
    insta::assert_snapshot!(check_full(text), @"
    E2002 12:3 `f` performs `Ask`, which the signature of `run` does not allow
      12:3 this call performs `Ask`
      8:7 the row of this signature does not include it
      help: add `Ask` to the row of the signature of `run`, as in `-> <Ask> ...`
    ");
}

/// `wrap` の外に出る `Ask` が、`g` を通って `f` に戻るたびに1つずつ増える。足すラベルの数は始めたときの数で抑えるので、
/// 固定点は止まり、包含が E2002 を報告する。
#[test]
fn a_growing_cycle_of_calls_stops_the_fixpoint() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nwrap : (Unit -> <e> Int) -> <Ask, Panic | e> Int\nwrap action = ask () + action ()\n\nrun : Int -> <Ask, Panic> Int\nrun m =\n  let f n = wrap (fn () -> g n)\n  and g n = f n\n  f m";
    insta::assert_snapshot!(check_full(text), @"
    E2002 9:13 `wrap` performs `Ask`, which this lambda does not allow
      9:13 this call performs `Ask`
    E2002 11:3 `f` performs `Ask`, which the signature of `run` does not allow
      11:3 this call performs `Ask`
      7:7 the row of this signature does not include it
      help: add `Ask` to the row of the signature of `run`, as in `-> <Ask> ...`
    ");
}

/// Prelude の `for` は本体のエフェクトを通し、添字の足し算の `Panic` を足す
/// (docs/spec/stdlib.md の「`for` と `while`」)。
#[test]
fn for_passes_the_body_effects_and_adds_panic() {
    let text = "show_all : Int -> <IO, Panic> Unit\nshow_all n = for 0 n (fn i -> println (show i))\n\nno_panic : Int -> <IO> Unit\nno_panic n = for 0 n (fn i -> println (show i))";
    insta::assert_snapshot!(check_short(text).join("\n"), @"E2002 5:14 `for` performs `Panic`, which the signature of `no_panic` does not allow");
}

/// `while` は条件と本体のエフェクトだけを通す。
#[test]
fn while_passes_the_condition_and_body_effects() {
    let text = "effect Ask where\n  ask : Unit -> Bool\n\nloud : Unit -> <Ask, IO> Unit\nloud () = while (fn () -> ask ()) (fn () -> println \"again\")\n\nspin : Unit -> Unit\nspin () = while (fn () -> False) (fn () -> ())";
    assert_eq!(check_short(text), Vec::<String>::new());
}

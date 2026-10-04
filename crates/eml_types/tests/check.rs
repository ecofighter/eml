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
        main : Unit -> <{error}> Unit
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

#[test]
fn lambdas_are_checked_against_the_expected_type_or_inferred() {
    let text = "apply : (Int -> Int) -> Int\napply f = f 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = apply (fn x -> x + 1)\n  let id = fn y -> y\n  let s = id \"s\"\n  let k = fn (z : Int) _ -> z\n  println (show_int (k n s))";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (Int -> Int) -> Int
      f#0 : Int -> Int
    main : Unit -> <IO> Unit
      x#0 : Int
      n#1 : Int
      y#2 : String
      id#3 : String -> <IO> String
      s#4 : String
      z#5 : Int
      k#6 : Int -> <IO> String -> <IO> Int
    ");
}

#[test]
fn a_lambda_cannot_perform_effects_its_expected_type_does_not_allow() {
    let text = "each : (String -> Unit) -> Unit\neach f = f \"a\"\n\nmain : Unit -> <IO> Unit\nmain () = each (fn s -> println s)";
    insta::assert_snapshot!(check_text(text), @r"
    each : (String -> Unit) -> Unit
      f#0 : String -> Unit
    main : Unit -> <IO> Unit
      s#0 : String
    ---
    E2002 5:25 `println` performs `IO`, which this lambda does not allow
      5:25 this call performs `IO`
      5:11 argument 1 of `each` does not allow it
    ");
}

#[test]
fn annotated_lambda_parameters_must_match_and_arities_must_agree() {
    let text = "apply : (Int -> Int) -> Int\napply f = f 1\n\nbad : Unit -> Int\nbad () = apply (fn (x : String) -> 1) + apply (fn a b -> a)";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (Int -> Int) -> Int
      f#0 : Int -> Int
    bad : Unit -> Int
      x#0 : String
      a#1 : Int
      b#2 : {error}
    ---
    E2001 5:20 mismatched types
      5:20 expected `Int`, found `String`
      note: an annotated lambda parameter must have the parameter type the lambda is expected to have
    E2001 5:53 this lambda has 2 parameters but its expected type `Int -> Int` has 1 arrow
      5:53 this parameter has no arrow in the expected type
    ");
}

#[test]
fn an_arity_mismatch_in_a_lambda_does_not_cascade() {
    let text = "apply : (Int -> Int) -> Int\napply f = f 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = apply (fn a b -> println \"x\")\n  ()";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (Int -> Int) -> Int
      f#0 : Int -> Int
    main : Unit -> <IO> Unit
      a#0 : Int
      b#1 : {error}
      n#2 : Int
    ---
    E2001 6:23 this lambda has 2 parameters but its expected type `Int -> Int` has 1 arrow
      6:23 this parameter has no arrow in the expected type
    ");
}

#[test]
fn kinds_follow_from_how_values_are_used() {
    let text = "twice : (a -> a) -> a -> a\ntwice f x = f (f x)\n\nid : a -> a\nid x = x\n\nboth : a -> (a -> a -> b) -> b\nboth x g = g x x\n\npick : c -> c -> c\npick p q = p\n\ncall : d -> d\ncall y = both y pick";
    insta::assert_snapshot!(check_text(text), @r"
    twice : (a -> a) -> a -> a
      kinds: (a -> a) <= Unr
      f#0 : a -> a
      x#1 : a
    id : a -> a
      x#0 : a
    both : a -> (a -> a -> b) -> b
      kinds: a <= Unr
      x#0 : a
      g#1 : a -> a -> b
    pick : c -> c -> c
      kinds: c <= Unr
      p#0 : c
      q#1 : c
    call : d -> d
      kinds: d <= Unr
      y#0 : d
    ");
}

#[test]
fn kinds_are_shared_within_a_strongly_connected_component() {
    let text = "ping : a -> Int -> a\nping x n = if n == 0 then x else pong (first x x) (n - 1)\n\npong : b -> Int -> b\npong y n = ping y n\n\nfirst : c -> c -> c\nfirst u v = u";
    insta::assert_snapshot!(check_text(text), @r"
    ping : a -> Int -> a
      kinds: a <= Unr
      x#0 : a
      n#1 : Int
    pong : b -> Int -> b
      kinds: b <= Unr
      y#0 : b
      n#1 : Int
    first : c -> c -> c
      kinds: c <= Unr
      u#0 : c
      v#1 : c
    ");
}

#[test]
fn captured_values_count_inside_the_lambda_body() {
    let text = "dupper : a -> Unit -> (a -> a -> a) -> a\ndupper x = fn () g -> g x x\n\nkeeper : a -> Unit -> a\nkeeper x = fn () -> x";
    insta::assert_snapshot!(check_text(text), @r"
    dupper : a -> Unit -> (a -> a -> a) -> a
      kinds: a <= Unr
      x#0 : a
      g#1 : a -> a -> a
    keeper : a -> Unit -> a
      x#0 : a
    ");
}

#[test]
fn an_undefined_effect_row_is_fresh_at_each_call() {
    let text = "run : (Unit -> <Console> Unit) -> <Console> Unit\nrun f = f ()\n\nloud : Unit -> <IO> Unit\nloud () = run (fn () -> println \"x\")\n\nquiet : Unit -> Unit\nquiet () = run (fn () -> ())";
    insta::assert_snapshot!(check_text(text), @"
    run : (Unit -> <{error}> Unit) -> <{error}> Unit
      f#0 : Unit -> <{error}> Unit
    loud : Unit -> <IO> Unit
    quiet : Unit -> Unit
    ---
    E1002 1:17 cannot find effect `Console`
      1:17 not found in this scope
    E1002 1:36 cannot find effect `Console`
      1:36 not found in this scope
    ");
}

#[test]
fn a_callee_that_is_not_a_name_is_described_without_quotes() {
    let text = "inc : Int -> Int\ninc n = n + 1\n\neach : (String -> Unit) -> Unit\neach f = f \"a\"\n\nbad : Bool -> Int\nbad b = (if b then 1 else 2) 3 + (if b then inc else inc) \"x\" + (if b then inc else inc) 1 2\n\nloud : Bool -> Unit\nloud b =\n  (if b then println else println) \"x\"\n  (if b then each else each) (fn s -> println s)";
    insta::assert_snapshot!(check_text(text), @"
    inc : Int -> Int
      n#0 : Int
    each : (String -> Unit) -> Unit
      f#0 : String -> Unit
    bad : Bool -> Int
      b#0 : Bool
    loud : Bool -> Unit
      b#0 : Bool
      s#1 : String
    ---
    E2001 8:30 this expression is not a function
      8:30 unexpected argument
    E2001 8:59 mismatched types
      8:59 expected `Int`, found `String`
      8:35 argument 1 of this expression
    E2001 8:92 this expression takes 1 argument but 2 were given
      8:92 unexpected argument
    E2002 12:3 this expression performs `IO`, which the signature of `loud` does not allow
      12:3 this call performs `IO`
      10:8 the row of this signature does not include it
      help: add `IO` to the row of the signature of `loud`, as in `-> <IO> ...`
    E2002 13:39 `println` performs `IO`, which this lambda does not allow
      13:39 this call performs `IO`
      13:4 argument 1 of this expression does not allow it
    ");
}

#[test]
fn an_undefined_effect_row_does_not_pass_the_body_effects_to_callers() {
    // 未定義のエフェクトの row はどのエフェクトも受け入れるが、本体のエフェクトを取り込んで呼び出し側に伝えない
    // (docs/spec/types.md の「エラーの扱い」)
    let text = "g : Unit -> <Console> Unit\ng () = println \"x\"\n\nh : Unit -> Unit\nh () = g ()";
    insta::assert_snapshot!(check_text(text), @"
    g : Unit -> <{error}> Unit
    h : Unit -> Unit
    ---
    E1002 1:14 cannot find effect `Console`
      1:14 not found in this scope
    ");
}

#[test]
fn a_missing_effect_points_at_the_arrow_of_the_body() {
    // 本体のエフェクトは、引数の数だけ矢印をたどった最後の矢印の row に入るので、その矢印を指す
    // (docs/spec/diagnostics.md の E2002)
    let text = "f : Int -> Int -> Unit\nf a b = println \"x\"";
    insta::assert_snapshot!(check_text(text), @"
    f : Int -> Int -> Unit
      a#0 : Int
      b#1 : Int
    ---
    E2002 2:9 `println` performs `IO`, which the signature of `f` does not allow
      2:9 this call performs `IO`
      1:12 the row of this signature does not include it
      help: add `IO` to the row of the signature of `f`, as in `-> <IO> ...`
    ");
}

#[test]
fn an_undefined_effect_does_not_hide_an_unrelated_missing_effect() {
    // `l` の row は `bad` の引数と単一化して末尾 `Error` になるが、`l` が起こす `IO` は既知のエフェクトなので、純粋な `p`
    // の中で呼べば E2002 になる (docs/spec/types.md の「エラーの扱い」は `Error` が関わる制約だけを黙らせる)
    let text = "bad : (Unit -> <Console> Unit) -> Unit\nbad f = ()\n\np : Unit -> Unit\np () =\n  let l = fn () -> println \"x\"\n  bad l\n  l ()";
    let out = check_text(text);
    assert!(
        out.contains("E1002 1:17 cannot find effect `Console`"),
        "{out}"
    );
    assert!(out.contains("E2002 8:3 `l` performs `IO`"), "{out}");
}

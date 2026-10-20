//! `var` の型検査 (docs/spec/types.md の「`var` の変数」)。変数を使う所は合成のエフェクトの `perform` と同じく row に
//! ラベルを求め、推論の後で変数を `Unr` の変数と配列の変数に分ける。

use eml_hir::ExprKind;
use eml_test_support::{check, short_text};

use crate::common::{check_full, check_short, check_text};

/// 入口のモジュールの関数ごとに、`var` の変数の型と分類、添字のノードの種類を出す。診断があれば先に出す。
fn vars(text: &str) -> String {
    let checked = check(text);
    let program = &checked.program;
    let mut out = short_text(checked.files(), &checked.diagnostics);
    for (id, function) in program.functions() {
        if program.modules[id.module].name != "Main" {
            continue;
        }
        let (Some(body), Some(types)) = (program.body(id), checked.typed.bodies.get(id)) else {
            continue;
        };
        let file = program.file(id.module);
        for (var, data) in body.vars.iter() {
            let Some(var_type) = types.vars.get(var) else {
                out.push_str(&format!("{}: var {} untyped\n", function.name, data.name));
                continue;
            };
            out.push_str(&format!(
                "{}: var {} : {} ({:?})\n",
                function.name,
                data.name,
                checked.typed.types.display(var_type.ty, &program.names),
                var_type.class,
            ));
        }
        for (expr, data) in body.exprs.iter() {
            if !matches!(data.kind, ExprKind::Index { .. }) {
                continue;
            }
            let at = checked.files().line_col(file, data.range.start());
            out.push_str(&format!(
                "{}: index {at} {:?}\n",
                function.name,
                types.indexes.get(expr)
            ));
        }
    }
    out
}

#[test]
fn a_scalar_var_types() {
    let text = "\
main : Unit -> <IO, Panic> Unit
main () =
  var n = 0
  n := n + 1
  println \"\\{n}\"
";
    insta::assert_snapshot!(vars(text), @"main: var n : Int (Unr)");
}

#[test]
fn a_mutarray_var_types_with_reads_writes_and_an_extraction() {
    let text = "\
main : Unit -> <IO, Panic> Unit
main () =
  var a = MutArray.make 3 0
  a[0] := 1
  let v = a[1]
  println \"\\{v}\"
  println \"\\{Array.length (MutArray.freeze a)}\"
";
    insta::assert_snapshot!(vars(text), @"
    main: var a : MutArray Int (Array)
    main: index 5:11 Some(ArrayVar)
    ");
}

#[test]
fn a_use_adds_the_var_label_to_a_lambda() {
    let text = "\
main : Unit -> <IO> Unit
main () =
  var x = 1
  let g = fn () -> x
  println \"\\{g ()}\"
";
    insta::assert_snapshot!(check_text(text), @"
    main : Unit -> <IO> Unit
      g#0 : Unit -> <var x> Int
    ");
}

#[test]
fn an_index_needs_panic() {
    let text = "\
f : Unit -> Int
f () =
  var a = Array.from_list [1, 2]
  a[0]
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E2002 4:3 `a` performs `Panic`, which the signature of `f` does not allow",
    ]
    "#);
}

#[test]
fn an_array_var_index_reads_the_array() {
    let text = "\
f : Unit -> <Panic> Int
f () =
  var a = Array.make 3 0
  a[1]
";
    insta::assert_snapshot!(vars(text), @"
    f: var a : Array Int (Unr)
    f: index 4:3 Some(Array)
    ");
}

#[test]
fn an_expression_index_reads_an_array() {
    let text = "\
f : Array Int -> <Panic> Int
f xs = xs[0]
";
    insta::assert_snapshot!(vars(text), @"f: index 2:8 Some(Array)");
}

#[test]
fn a_lin_element_is_e3001() {
    let text = "\
main : Unit -> <IO, Panic> Unit
main () =
  var a = MutArray.empty ()
  a[0] := Fs.open \"x\"
  drop a
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E3001 4:3 a linear value is used where an unrestricted value is expected",
    ]
    "#);
}

#[test]
fn a_self_referencing_var_has_no_infinite_type() {
    let text = "\
main : Unit -> <IO, Panic> Unit
main () =
  var fs = MutArray.make 2 (fn () -> 0)
  fs[0] := (fn () -> fs[1] ())
  drop (MutArray.freeze fs)
";
    insta::assert_snapshot!(vars(text), @"
    main: var fs : MutArray (Unit -> <var fs, Panic> Int) (Array)
    main: index 4:22 Some(ArrayVar)
    ");
}

#[test]
fn a_var_of_a_signature_type_variable_constrains_the_scheme() {
    let text = "\
keep : a -> a
keep x =
  var y = x
  y

main : Unit -> <IO> Unit
main () =
  let f = keep (Fs.open \"x\")
  Fs.close f
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E3001 8:11 a linear value is passed to `keep`, which may use it more than once or not at all",
    ]
    "#);
}

#[test]
fn a_linear_var_is_e3007() {
    let text = "\
main : Unit -> <IO> Unit
main () =
  var f = Fs.open \"x\"
  ()
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E3007 3:3 the variable `f` has the linear type `File`",
    ]
    "#);
}

#[test]
fn a_closure_escaping_as_the_value_is_e2019_once() {
    let text = "\
main : Unit -> <IO> Unit
main () =
  let g =
    var x = 1
    fn () -> x
  println \"\\{g ()}\"
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E2019 5:5 a function that uses the variable `x` can escape its scope",
    ]
    "#);
}

#[test]
fn an_inner_function_may_carry_the_label() {
    let text = "\
main : Unit -> <IO, Panic> Unit
main () =
  var n = 0
  let bump () = n := n + 1
  bump ()
  bump ()
  println \"\\{n}\"
";
    insta::assert_debug_snapshot!(check_short(text), @"[]");
}

#[test]
fn a_shadowed_outer_local_is_reported_once() {
    let text = "\
main : Unit -> <IO> Unit
main () =
  var f = fn () -> 0
  let g = f
  let g = g
  var x = 1
  f := fn () -> x
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E2019 3:3 a function that uses the variable `x` can escape its scope",
    ]
    "#);
}

#[test]
fn an_outer_local_receiving_a_closure_is_e2019() {
    let text = "\
apply : (Unit -> <e> Int) -> <e> Int
apply f = f ()

main : Unit -> <IO> Unit
main () =
  let run = fn f -> apply f
  var x = 1
  println \"\\{run (fn () -> x)}\"
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E2019 6:7 a function that uses the variable `x` can escape its scope",
    ]
    "#);
}

#[test]
fn a_closed_lambda_inside_the_scope_is_e2002() {
    let text = "\
call : (Unit -> Int) -> Int
call f = f ()

main : Unit -> <IO> Unit
main () =
  var x = 1
  println \"\\{call (fn () -> x)}\"
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E2002 7:29 `x` performs `var x`, which this lambda does not allow",
    ]
    "#);
}

#[test]
fn an_int_var_cannot_be_indexed() {
    let text = "\
f : Unit -> <Panic> Int
f () =
  var n = 1
  n[0]
";
    insta::assert_snapshot!(check_full(text), @"
    E2001 4:3 mismatched types
      4:3 expected `MutArray _`, found `Int`
      3:3 the type of `n` comes from here
      note: only a `var` variable that holds an `Array` or a `MutArray` can be indexed
    ");
}

#[test]
fn an_expression_index_needs_an_array() {
    let text = "\
f : List Int -> <Panic> Int
f xs = xs[0]
";
    insta::assert_snapshot!(check_full(text), @"
    E2001 2:8 mismatched types
      2:8 expected `Array _`, found `List Int`
      note: only an `Array` value can be indexed; keep a `MutArray` in a `var` variable to index it
    ");
}

#[test]
fn an_escaped_call_still_reports_its_other_missing_effects() {
    let text = "\
main : Unit -> <IO> Unit
main () =
  let g =
    var x = 1
    fn () -> x + 1
  println \"\\{g ()}\"
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E2019 5:5 a function that uses the variable `x` can escape its scope",
        "E2002 6:14 `g` performs `Panic`, which the signature of `main` does not allow",
    ]
    "#);
}

#[test]
fn taking_on_both_branches_is_accepted() {
    let text = "\
f : Bool -> <Panic> Array Int
f c =
  var a = MutArray.make 3 0
  a[0] := 1
  if c then MutArray.freeze a else MutArray.freeze a
";
    insta::assert_debug_snapshot!(check_short(text), @"[]");
}

#[test]
fn a_diverging_branch_need_not_take() {
    let text = "\
f : Bool -> <IO, Panic> Array Int
f c =
  var a = MutArray.make 3 0
  if c then exit 1 else MutArray.freeze a
";
    insta::assert_debug_snapshot!(check_short(text), @"[]");
}

#[test]
fn a_last_expression_that_diverges_need_not_take() {
    let text = "\
f : Unit -> <IO, Panic> Unit
f () =
  var a = MutArray.make 3 0
  a[0] := 1
  exit 2
";
    insta::assert_debug_snapshot!(check_short(text), @"[]");
}

#[test]
fn every_match_arm_takes() {
    let text = "\
f : Int -> <Panic> Array Int
f n =
  var a = MutArray.make 3 0
  match n with
    | 0 -> MutArray.freeze a
    | _ -> Array.from_list [n, Array.length (MutArray.freeze a)]
";
    insta::assert_debug_snapshot!(check_short(text), @"[]");
}

#[test]
fn a_branch_that_does_not_take_is_e3003() {
    let text = "\
f : Bool -> <Panic> Unit
f c =
  var a = MutArray.make 3 0
  if c then drop a else ()
";
    insta::assert_snapshot!(check_full(text), @"
    E3003 3:3 the array variable `a` is not taken on some paths
      3:3 `a` is declared here
      4:25 this branch does not take `a`
      note: taking an array variable hands over its array, so every path of the last expression takes it exactly once
      help: end the block with an expression that takes `a`, such as `MutArray.freeze a` or `drop a`
    ");
}

#[test]
fn an_omitted_else_does_not_take() {
    let text = "\
f : Bool -> <Panic> Unit
f c =
  var a = MutArray.make 3 0
  if c then drop a
";
    insta::assert_snapshot!(check_full(text), @"
    E3003 3:3 the array variable `a` is not taken on some paths
      3:3 `a` is declared here
      4:3 the omitted `else` does not take `a`
      note: taking an array variable hands over its array, so every path of the last expression takes it exactly once
      help: end the block with an expression that takes `a`, such as `MutArray.freeze a` or `drop a`
    ");
}

#[test]
fn a_take_in_a_handle_body_is_e3008() {
    let text = "\
effect Ask where
  ask : Unit -> Int

f : Unit -> <Panic> Array Int
f () =
  var a = MutArray.make 3 0
  handle MutArray.freeze a with
    | ctl ask () k -> k 1
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E3008 7:26 the array variable `a` cannot be taken inside a function",
    ]
    "#);
}

#[test]
fn a_take_after_a_use_is_e3008() {
    let text = "\
run : (Unit -> <e> a) -> <e> a
run f = f ()

f : Unit -> <Panic> Array Int
f () =
  var a = MutArray.make 3 0
  use run
  MutArray.freeze a
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E3008 8:19 the array variable `a` cannot be taken inside a function",
    ]
    "#);
}

#[test]
fn a_type_error_suppresses_the_take_rules() {
    let text = "\
f : Unit -> <Panic> Unit
f () =
  var a = MutArray.make 3 0
  let s : String = 1
  ()
";
    insta::assert_debug_snapshot!(check_short(text), @r#"
    [
        "E2001 4:20 mismatched types",
    ]
    "#);
}

#[test]
fn an_unr_var_may_be_read_anywhere() {
    let text = "\
f : Unit -> <Panic> (Int, Int)
f () =
  var n = 1
  let g () = n
  (n, g ())
";
    insta::assert_debug_snapshot!(check_short(text), @"[]");
}

#[test]
fn a_value_kept_across_a_var_scope_is_e3006() {
    let text = "\
effect Choose where
  multi choose : Unit -> Bool

g : Unit -> <Choose, IO, Panic> Unit
g () =
  let f = Fs.open \"x\"
  let r =
    var n = 0
    if choose () then n := 1 else ()
    n
  Fs.close f
  println (show r)
";
    insta::assert_snapshot!(check_full(text), @"
    E3006 8:5 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      8:5 the scope of this `var` may perform `choose`, a `multi` operation
      6:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before the scope of this `var`
    ");
}

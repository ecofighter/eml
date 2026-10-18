//! `optimize` パスの骨組み (docs/spec/core-ir.md)。

use eml_core_ir::{Pass, optimize, parse, pretty};

use crate::common::{core_text, function};

#[test]
fn optimize_keeps_a_first_order_program() {
    let text = "main : Unit -> <IO> Unit\nmain () = println (show (1 + 2))";
    assert_eq!(
        core_text(text, Pass::Optimize),
        core_text(text, Pass::Translate)
    );
}

#[test]
fn unreachable_functions_are_dropped() {
    let mut program = parse(
        "fn unused(a.0: int) -> int {
  return a.0
}
fn helper(a.0: int) -> int {
  return a.0
}
fn entry$main() -> int {
  let r.0: int = call helper(1)
  return r.0
}
",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    optimize(&mut program);
    let shown = pretty(&program);
    assert!(!shown.contains("unused"), "{shown}");
    assert!(shown.contains("call helper(1)"), "{shown}");
    assert_eq!(program.functions.len(), 2);
    assert_eq!(program.function(program.entry).name, "entry$main");
    eml_core_ir::verify_translated(&program).unwrap_or_else(|error| panic!("{error}\n{shown}"));
}

#[test]
fn a_known_lambda_is_called_directly() {
    let text =
        "main : Unit -> <IO> Unit\nmain () =\n  let f = fn x -> x + 1\n  println (show (f 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.1: int = call main$lambda0(2)
      let t.2: obj = extern "Prelude.Show Int.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    "#);
}

#[test]
fn a_partial_application_is_called_directly() {
    let text = "add : Int -> Int -> Int\nadd x y = x + y\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = add 1\n  println (show (g 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.2: int = call add(1, 2)
      let t.3: obj = extern "Prelude.Show Int.show"(t.2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
    }
    "#);
}

#[test]
fn an_under_saturated_known_apply_builds_a_closure() {
    let text = "add3 : Int -> Int -> Int -> Int\nadd3 x y z = x + y + z\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = add3 1\n  let h = g 2\n  println (show (h 3))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.3: int = call add3(1, 2, 3)
      let t.4: obj = extern "Prelude.Show Int.show"(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    "#);
}

#[test]
fn yield_as_a_value_becomes_a_perform() {
    let text = "import Yield (Yield(..))\n\nproduce : Unit -> <Yield Int> Unit\nproduce () =\n  let y = yield\n  y 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (Yield.collect produce))";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("call op$"), "{shown}");
    insta::assert_snapshot!(function(&shown, "produce"), @"
    yields fn produce(p.0: unit) -> unit {
      let t.1: unit = perform Std.Yield.Yield.yield(1)
      return t.1
    }
    ");
}

#[test]
fn a_masked_apply_of_an_operation_value_stays() {
    let text = "\
effect Yield a where
  yield : a -> Unit

render : Unit -> <Yield String | e> Unit
render () =
  handle (let y : Int -> <Yield Int | e> Unit = yield in y 1) with
    | ctl yield n k ->
      yield (show n)
      k ()

main : Unit -> <IO> Unit
main () =
  handle render () with
    | ctl yield s k ->
      println s
      k ()
";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "render$handle0"), @"
    internal yields fn render$handle0(p.0: unit) -> unit {
      let t.1: unit = mask [Yield@1] yields apply &op$yield(1)
      return t.1
    }
    ");
}

#[test]
fn a_constructor_value_becomes_a_con() {
    let text = "data Pair = | Pair Int Int\n\nfirst : Pair -> Int\nfirst (Pair a _) = a\n\nmain : Unit -> <IO> Unit\nmain () =\n  let p = Pair 1\n  println (show (first (p 2)))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.2: obj = con Pair #0(1, 2)
      let t.3: int = call first(t.2)
      let t.4: obj = extern "Prelude.Show Int.show"(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    "#);
}

#[test]
fn a_caller_without_yield_points_stops_yielding() {
    let text = "import State (State(..))\n\nf : Int -> <State Int> Int\nf n =\n  let g = fn x -> x + n\n  g 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let (r, _) = State.run 0 (fn () -> f 2)\n  println (show r)";
    let yields = |pass| {
        let program = eml_test_support::core_until(text, pass);
        let f = program
            .functions
            .iter()
            .find(|function| function.name == "f")
            .unwrap_or_else(|| panic!("no `f` in\n{}", pretty(&program)));
        f.yields
    };
    assert!(yields(Pass::Translate));
    assert!(!yields(Pass::Optimize));
}

#[test]
fn an_over_saturated_known_apply_calls_then_applies_the_rest() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = fn x -> fn y -> x + y\n  println (show (f 1 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.4: tobj = call main$lambda1(1)
      let t.1: int = apply t.4(2)
      let t.2: obj = extern "Prelude.Show Int.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    "#);
}

/// 書き換えで使われなくなった `closure` は残さない。値として参照されなくなった `add` に、box の挿入が `add$boxed` を
/// 作らないためである。
#[test]
fn a_closure_whose_applies_are_all_rewritten_is_dropped() {
    let text = "add : Int -> Int -> Int\nadd x y = x + y\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = add 1\n  println (show (g 2))";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("closure add("), "{shown}");
    let lir = eml_test_support::lir(text);
    assert!(
        lir.functions
            .iter()
            .all(|function| function.name != "add$boxed"),
        "{}",
        eml_lir::pretty(&lir)
    );
}

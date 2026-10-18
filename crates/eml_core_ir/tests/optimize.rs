//! `optimize` パスの骨組み (docs/spec/core-ir.md)。

use eml_core_ir::{Call, Pass, Rhs, Stmt, optimize, parse, pretty};
use eml_lir::Atom;

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

/// `pass` の直後の関数の名前。
fn names(text: &str, pass: Pass) -> Vec<String> {
    eml_test_support::core_until(text, pass)
        .functions
        .into_iter()
        .map(|function| function.name)
        .collect()
}

#[test]
fn map_is_copied_for_a_known_lambda() {
    let text =
        "main : Unit -> <IO> Unit\nmain () = println (show (List.map (fn x -> x * 2) [1, 2, 3]))";
    let shown = core_text(text, Pass::Optimize);
    let copy = function(&shown, "Std.List.map@[Int, Int]{0=main$lambda0}");
    let go = function(&shown, "Std.List.map@[Int, Int]$let$go{0=main$lambda0}");
    assert!(!go.contains("apply"), "{go}");
    insta::assert_snapshot!(format!("{}\n\n{copy}\n{go}", names(text, Pass::Optimize).join("\n")), @r#"
    Prelude.show_rest@[Int]
    Prelude.Show List.show@[Int]
    main
    Std.List.reverse@[Int]
    Std.List.reverse_onto@[Int]
    main$lambda0
    entry$main
    Std.List.map@[Int, Int]{0=main$lambda0}
    Std.List.map@[Int, Int]$let$go{0=main$lambda0}

    fn "Std.List.map@[Int, Int]{0=main$lambda0}"(xs.1: tobj) -> tobj {
      let t.2: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0}"(#0, xs.1)
      return t.2
    }

    fn "Std.List.map@[Int, Int]$let$go{0=main$lambda0}"(acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: int, rest.4: tobj) -> b2 }
    b1:
      let t.5: tobj = call "Std.List.reverse@[Int]"(acc.1)
      return t.5
    b2:
      let t.6: int = call main$lambda0(x.3)
      let d.7: tobj = con Prelude.List #1(t.6, acc.1)
      let t.8: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0}"(d.7, rest.4)
      return t.8
    }
    "#);
}

#[test]
fn a_recursive_call_reuses_the_copy() {
    let text = include_str!("../../../bench/tree.em");
    let folds: Vec<String> = names(text, Pass::Optimize)
        .into_iter()
        .filter(|name| name.starts_with("fold@"))
        .collect();
    assert_eq!(folds, ["fold@[Int, Int]{0=main$lambda0}"]);
}

#[test]
fn captures_become_parameters() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let k = 3\n  println (show (List.map (fn x -> x * k) [1, 2, 3]))";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("closure"), "{shown}");
    insta::assert_snapshot!(format!(
        "{}\n{}\n{}",
        function(&shown, "main"),
        function(&shown, "Std.List.map@[Int, Int]{0=main$lambda0/1}"),
        function(&shown, "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}")
    ), @r#"
    fn main(p.0: unit) -> unit {
      let d.2: tobj = con Prelude.List #1(3, #0)
      let d.3: tobj = con Prelude.List #1(2, d.2)
      let d.4: tobj = con Prelude.List #1(1, d.3)
      let t.5: tobj = call "Std.List.map@[Int, Int]{0=main$lambda0/1}"(3, d.4)
      let t.6: obj = call "Prelude.Show List.show@[Int]"(t.5)
      let t.7: unit = extern Prelude.println(t.6)
      return t.7
    }

    fn "Std.List.map@[Int, Int]{0=main$lambda0/1}"(k.3: int, xs.1: tobj) -> tobj {
      let t.2: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}"(k.3, #0, xs.1)
      return t.2
    }

    fn "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}"(k.9: int, acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: int, rest.4: tobj) -> b2 }
    b1:
      let t.5: tobj = call "Std.List.reverse@[Int]"(acc.1)
      return t.5
    b2:
      let t.6: int = call main$lambda0(k.9, x.3)
      let d.7: tobj = con Prelude.List #1(t.6, acc.1)
      let t.8: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}"(k.9, d.7, rest.4)
      return t.8
    }
    "#);
}

#[test]
fn a_closure_captured_by_a_closure_is_specialized() {
    let text = "main : Unit -> <IO> Unit\nmain () = println (show (Iter.to_list (Iter.map (fn x -> x + 1) (Iter.from_list [1, 2, 3]))))";
    let shown = core_text(text, Pass::Optimize);
    insta::assert_snapshot!(function(&shown, "Std.Iter.map@[Int, Int]{0=main$lambda0}"), @r#"
    fn "Std.Iter.map@[Int, Int]{0=main$lambda0}"(it.1: obj lin) -> obj lin {
      let c.2: tobj lin = closure "Std.Iter.map@[Int, Int]$lambda0{0=main$lambda0}"(it.1)
      let d.3: obj lin = con Std.Iter.Iter #0(c.2)
      return d.3
    }
    "#);
}

#[test]
fn generate_reaches_the_handle_body() {
    let text = "\
import Yield (Yield(..))

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  yield n
  yield (n + 1)

main : Unit -> <IO> Unit
main () = println (show (Iter.to_list (Iter.generate (nums 3))))
";
    let program = eml_test_support::core_until(text, Pass::Optimize);
    let bodies: Vec<_> = program
        .functions
        .iter()
        .filter(|function| {
            function
                .name
                .starts_with("Std.Iter.generate@[Int]$handle0{")
        })
        .collect();
    assert_eq!(bodies.len(), 1, "{}", pretty(&program));
    let shown = pretty(&program);
    let body = function(&shown, &bodies[0].name);
    assert!(body.contains("call nums("), "{body}");
    assert!(!body.contains("apply"), "{body}");
}

#[test]
fn a_closure_only_stored_in_data_is_not_specialized() {
    let text = "\
wrap : (Int -> Int) -> Option (Int -> Int)
wrap f = Some f

main : Unit -> <IO> Unit
main () =
  match wrap (fn x -> x + 1) with
    | Some g -> println (show (g 1))
    | None -> ()
";
    assert!(
        names(text, Pass::Optimize)
            .iter()
            .all(|name| !name.starts_with("wrap{")),
    );
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "wrap"), @"
    fn wrap(f.0: tobj poly) -> tobj poly {
      let d.1: tobj poly = con Prelude.Option #1(f.0)
      return d.1
    }
    ");
}

/// 複製の名前とコードは鍵だけで決まり、ほかの関数の有無や作った順によらない (docs/spec/vm.md の「追記」)。
#[test]
fn a_copy_does_not_depend_on_the_rest_of_the_program() {
    let a =
        "main : Unit -> <IO> Unit\nmain () =\n  println (show (List.map (fn x -> x + 1) [1, 2]))";
    let b = "other : List Int -> List Int\nother ys = List.map (fn y -> y * 2) ys\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (show (List.map (fn x -> x + 1) [1, 2]))\n  println (show (other [3]))";
    for name in [
        "Std.List.map@[Int, Int]{0=main$lambda0}",
        "Std.List.map@[Int, Int]$let$go{0=main$lambda0}",
    ] {
        assert_eq!(
            function(&core_text(a, Pass::Optimize), name),
            function(&core_text(b, Pass::Optimize), name)
        );
    }
}

/// 節の関数は、ちょうど1つの handle に属する (`crates/eml_core_ir/src/evidence/handlers.rs` の `entries`)。
#[test]
fn two_handler_copies_keep_their_own_clauses() {
    let text = "\
import Yield (Yield(..))

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  yield n
  yield (n + 1)

other : Int -> Unit -> <Yield Int> Unit
other n () = yield (n * 10)

main : Unit -> <IO> Unit
main () =
  Yield.for_each (nums 3) (fn x -> println \"\\{x}\")
  Yield.for_each (other 3) (fn x -> println \"\\{x}\")
";
    let program = eml_test_support::core_until(text, Pass::Optimize);
    let mut clauses = Vec::new();
    for function in &program.functions {
        let closures: std::collections::HashMap<_, _> = function
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter_map(|stmt| match stmt {
                Stmt::Let {
                    var,
                    rhs: Rhs::MakeClosure(target, _),
                } => Some((*var, *target)),
                _ => None,
            })
            .collect();
        for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
            if let Stmt::Let {
                rhs:
                    Rhs::Call {
                        call:
                            Call::Handle {
                                clauses: atoms,
                                ret,
                                ..
                            },
                        ..
                    },
                ..
            } = stmt
            {
                for atom in atoms.iter().chain([ret]) {
                    clauses.push(match *atom {
                        Atom::Fn(target) => target,
                        Atom::Var(var) => closures[&var],
                        _ => panic!("a clause is a function value"),
                    });
                }
            }
        }
    }
    assert_eq!(clauses.len(), 4, "{}", pretty(&program));
    let distinct: std::collections::HashSet<_> = clauses.iter().collect();
    assert_eq!(distinct.len(), 4, "{}", pretty(&program));
    eml_test_support::core_until(text, Pass::Evidence);
}

/// 同じ関数の閉包でも、捕獲の数が違えば鍵が違う。名前にも捕獲の数を書き、同じ名前の関数が2つにならないようにする
/// (docs/spec/vm.md の「追記」)。`core_text` は表示を読み直すので、名前が重なれば失敗する。
#[test]
fn closures_with_different_capture_counts_get_different_copies() {
    let text = "\
apply_to : (Int -> Int) -> Int -> Int
apply_to f x = f x

add3 : Int -> Int -> Int -> Int
add3 a b c = a + b + c

pick : (Int -> Int -> Int) -> Int
pick g = g 1 2

main : Unit -> <IO> Unit
main () =
  println (show (apply_to (add3 1 2) 3))
  println (show (pick (add3 1)))
";
    let shown = core_text(text, Pass::Optimize);
    let copies: Vec<String> = names(text, Pass::Optimize)
        .into_iter()
        .filter(|name| name.contains("{0=add3"))
        .collect();
    assert_eq!(copies, ["apply_to{0=add3/2}", "pick{0=add3/1}"], "{shown}");

    // 多相再帰の一様な instance は2つの閉包で同じなので、捕獲の数だけが鍵を分ける
    let text = "\
data Nested a =
  | Flat a
  | Nest (Nested (List a))

map_nested : (a -> b) -> Nested a -> Nested b
map_nested f n = match n with
  | Flat x -> Flat (f x)
  | Nest inner -> Nest (map_nested (List.map f) inner)

add3 : Int -> Int -> Int -> Int
add3 a b c = a + b + c

main : Unit -> <IO> Unit
main () =
  let r1 = map_nested (add3 1 2) (Flat 10)
  let r2 = map_nested (add3 1) (Flat 10)
  match r1 with
    | Flat x -> println (show x)
    | Nest _ -> println \"nest\"
  match r2 with
    | Flat g -> println (show (g 5))
    | Nest _ -> println \"nest\"
";
    let shown = core_text(text, Pass::Optimize);
    for name in [
        "map_nested@[_:poly, _:poly]{0=add3/2}",
        "map_nested@[_:poly, _:poly]{0=add3/1}",
    ] {
        function(&shown, name);
    }
}

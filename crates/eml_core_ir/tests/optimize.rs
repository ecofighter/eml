//! `optimize` パスの骨組み (docs/spec/core-ir.md)。

use eml_core_ir::{Call, Pass, Rhs, Stmt, optimize, parse, pretty};
use eml_lir::Atom;

use crate::common::{core_text, function};

/// 1階のプログラムは、小さな `main` が入口に展開されるほかは変わらない。
#[test]
fn optimize_keeps_a_first_order_program() {
    let text = "main : Unit -> <IO> Unit\nmain () = println (show (1 + 2))";
    let body = |shown: &str, name: &str| {
        let code = function(shown, name);
        code.lines().skip(1).collect::<Vec<_>>().join("\n")
    };
    assert_eq!(
        body(&core_text(text, Pass::Optimize), "entry$main"),
        body(&core_text(text, Pass::Translate), "main")
    );
}

#[test]
fn unreachable_functions_are_dropped() {
    let mut program = parse(
        "fn unused(a.0: int) -> int {
  return a.0
}
fn helper(a.0: int) -> int {
  let r.1: int = call helper(a.0)
  return r.1
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
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: int = extern Prelude.+(2, 1)
      let t.2: obj = extern "Prelude.Show Int.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    "#);
}

#[test]
fn a_partial_application_is_called_directly() {
    let text = "add : Int -> Int -> Int\nadd x y = x + y\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = add 1\n  println (show (g 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: int = extern Prelude.+(1, 2)
      let t.2: obj = extern "Prelude.Show Int.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    "#);
}

#[test]
fn an_under_saturated_known_apply_builds_a_closure() {
    let text = "add3 : Int -> Int -> Int -> Int\nadd3 x y z = x + y + z\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = add3 1\n  let h = g 2\n  println (show (h 3))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: int = extern Prelude.+(1, 2)
      let t.2: int = extern Prelude.+(t.1, 3)
      let t.3: obj = extern "Prelude.Show Int.show"(t.2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
    }
    "#);
}

#[test]
fn yield_as_a_value_becomes_a_perform() {
    let text = format!(
        "import Yield (Yield(..))\n\nproduce : Unit -> <Yield Int> Unit\nproduce () =\n  let y = yield\n  y 1\n{HIDE}\nmain : Unit -> <IO> Unit\nmain () =\n  match hide 0 produce with\n    | Box p -> println (show (Yield.collect p))"
    );
    let shown = core_text(&text, Pass::Optimize);
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
    // `con` を `hide` に渡し、既知のコンストラクタの規則で解けないようにする
    let text = "data Pair = | Pair Int Int\n\nfirst : Pair -> Int\nfirst (Pair a _) = a\n{HIDE}\nmain : Unit -> <IO> Unit\nmain () =\n  let p = Pair 1\n  match hide 0 (p 2) with\n    | Box q -> println (show (first q))";
    let text = text.replace("{HIDE}", HIDE);
    let text = text.as_str();
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: obj = con Pair #0(1, 2)
      let t.2: obj = call "hide@[Pair]"(0, t.1)
      unpack t.2 Box #0(q.3: obj)
      unpack q.3 Pair #0(a.4: int, x.5: int)
      let t.6: obj = extern "Prelude.Show Int.show"(a.4)
      let t.7: unit = extern Prelude.println(t.6)
      return t.7
    }
    "#);
}

#[test]
fn a_caller_without_yield_points_stops_yielding() {
    // `f` は自分を呼ぶので展開されず、印を付け直した関数として残る
    let text = "import State (State(..))\n\nf : Int -> <State Int> Int\nf n =\n  let g = fn x -> x + n\n  if n == 0 then g 1 else f (n - 1)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let (r, _) = State.run 0 (fn () -> f 2)\n  println (show r)";
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
    // `adder` は自分を呼ぶので展開されず、呼び出しが返す閉包は呼ぶ側から見えない
    let text = "\
adder : Int -> Int -> (Int -> Int)
adder n x = if n <= 0 then (fn y -> x + y) else adder (n - 1) x

main : Unit -> <IO> Unit
main () =
  let f = adder 0
  println (show (f 1 2))
";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: tobj poly = call adder(0, 1)
      let t.2: int = apply t.1(2)
      let t.3: obj = extern "Prelude.Show Int.show"(t.2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
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

/// 値をデータに入れて取り出す。取り出した値は既知の閉包でないので、handle の本体に届いても handler への特殊化の
/// 対象にならない。S13e-1 の規則だけを見るテストが使う。`hide` は自分を呼ぶので展開されず、取り出した値は呼ぶ側から見えない。
const HIDE: &str = "
data Box a = | Box a

hide : Int -> a -> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x
";

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
    let go = function(&shown, "Std.List.map@[Int, Int]$let$go{0=main$lambda0}");
    assert!(!go.contains("apply"), "{go}");
    insta::assert_snapshot!(format!("{}\n\n{go}", names(text, Pass::Optimize).join("\n")), @r#"
    Prelude.show_rest@[Int]
    Std.List.reverse_onto@[Int]
    entry$main
    Std.List.map@[Int, Int]$let$go{0=main$lambda0}

    fn "Std.List.map@[Int, Int]$let$go{0=main$lambda0}"(acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: int, rest.4: tobj) -> b2 }
    b1:
      let t.9: tobj = call "Std.List.reverse_onto@[Int]"(#0, acc.1)
      return t.9
    b2:
      let t.10: int = extern Prelude.*(x.3, 2)
      let d.7: tobj = con Prelude.List #1(t.10, acc.1)
      let t.8: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0}"(d.7, rest.4)
      return t.8
    }
    "#);
}

#[test]
fn each_yield_is_specialized() {
    let text = format!(
        "\
import Yield (Yield(..))

emit : List String -> Unit -> <Yield String> Unit
emit lines () = List.each yield lines
{HIDE}
main : Unit -> <IO> Unit
main () =
  match hide 0 (emit [\"a\"]) with
    | Box p -> println (show (Yield.collect p))
"
    );
    let shown = core_text(&text, Pass::Optimize);
    let copy = shown
        .lines()
        .find(|line| line.contains("fn \"Std.List.each@") && line.contains("{0="))
        .unwrap_or_else(|| panic!("{shown}"));
    let name = copy.split('"').nth(1).unwrap();
    let body = function(&shown, name);
    assert!(body.contains("perform Std.Yield.Yield.yield("), "{body}");
    assert!(!body.contains("apply"), "{body}");
}

#[test]
fn a_wrapper_shaped_lambda_is_specialized() {
    let text =
        "main : Unit -> <IO> Unit\nmain () = println (show (List.map (fn x -> Some x) [1, 2, 3]))";
    let shown = core_text(text, Pass::Optimize);
    let go = shown
        .lines()
        .find(|line| line.contains("fn \"Std.List.map@") && line.contains("$let$go"))
        .unwrap_or_else(|| panic!("{shown}"));
    let body = function(&shown, go.split('"').nth(1).unwrap());
    assert!(body.contains("con Prelude.Option #1"), "{body}");
    assert!(!body.contains("apply"), "{body}");
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
        "{}\n{}",
        function(&shown, "entry$main"),
        function(&shown, "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}")
    ), @r#"
    fn entry$main() -> unit {
      let d.1: tobj = con Prelude.List #1(3, #0)
      let d.2: tobj = con Prelude.List #1(2, d.1)
      let d.3: tobj = con Prelude.List #1(1, d.2)
      let t.4: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}"(3, #0, d.3)
      switch t.4 Prelude.List { #0 -> b1, #1(x.5: int, rest.6: tobj) -> b2 }
    b1:
      let s.7: obj = const "[]"
      jump b3(s.7)
    b2:
      let s.8: obj = const "["
      let t.9: obj = extern "Prelude.Show Int.show"(x.5)
      let t.10: obj = extern Prelude.++(s.8, t.9)
      let t.11: obj = call "Prelude.show_rest@[Int]"(t.10, rest.6)
      jump b3(t.11)
    b3(t.12: obj):
      let t.13: unit = extern Prelude.println(t.12)
      return t.13
    }

    fn "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}"(k.9: int, acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: int, rest.4: tobj) -> b2 }
    b1:
      let t.10: tobj = call "Std.List.reverse_onto@[Int]"(#0, acc.1)
      return t.10
    b2:
      let t.11: int = extern Prelude.*(x.3, k.9)
      let d.7: tobj = con Prelude.List #1(t.11, acc.1)
      let t.8: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}"(k.9, d.7, rest.4)
      return t.8
    }
    "#);
}

#[test]
fn a_closure_captured_by_a_closure_is_specialized() {
    let text = format!(
        "{HIDE}\nmain : Unit -> <IO> Unit\nmain () =\n  let Box it = hide 0 (Iter.from_list [1, 2, 3])\n  let Box mapped = hide 0 (Iter.map (fn x -> x + 1) it)\n  println (show (Iter.to_list mapped))"
    );
    let shown = core_text(&text, Pass::Optimize);
    insta::assert_snapshot!(function(&shown, "Std.Iter.map@[Int, Int]$lambda0{0=main$lambda0}"), @r#"
    internal fn "Std.Iter.map@[Int, Int]$lambda0{0=main$lambda0}"(it.1: obj lin, p.2: unit) -> tobj lin {
      unpack it.1 Std.Iter.Iter #0(pull.12: tobj lin)
      let t.13: tobj lin = apply pull.12(())
      switch t.13 Prelude.Option { #0 -> b1, #1(x.4: obj lin) -> b2 }
    b1:
      let u.7: unit = drop t.13
      return #0
    b2:
      unpack x.4 (,) #0(x.5: int, rest.6: obj lin)
      let t.14: int = extern Prelude.+(x.5, 1)
      let c.15: tobj lin = closure "Std.Iter.map@[Int, Int]$lambda0{0=main$lambda0}"(rest.6)
      let d.16: obj lin = con Std.Iter.Iter #0(c.15)
      let d.10: obj lin = con (,) #0(t.14, d.16)
      let d.11: tobj lin = con Prelude.Option #1(d.10)
      return d.11
    }
    "#);
}

/// `emit` の末尾でない呼び出しが、S13e-2 の handler への特殊化を止め、handle の本体を残す。
#[test]
fn generate_reaches_the_handle_body() {
    let text = "\
import Yield (Yield(..))

emit : Int -> <Yield Int> Unit
emit x = yield x

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  emit n
  emit (n + 1)

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
    // `nums` と `emit` は小さいので、本体に展開される
    assert!(body.contains("perform Std.Yield.Yield.yield("), "{body}");
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
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: int = extern Prelude.+(1, 1)
      let t.2: obj = extern "Prelude.Show Int.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    "#);
}

/// 複製の名前とコードは鍵だけで決まり、ほかの関数の有無や作った順によらない (docs/spec/vm.md の「追記」)。
#[test]
fn a_copy_does_not_depend_on_the_rest_of_the_program() {
    let a =
        "main : Unit -> <IO> Unit\nmain () =\n  println (show (List.map (fn x -> x + 1) [1, 2]))";
    let b = "other : List Int -> List Int\nother ys = List.map (fn y -> y * 2) ys\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (show (List.map (fn x -> x + 1) [1, 2]))\n  println (show (other [3]))";
    let name = "Std.List.map@[Int, Int]$let$go{0=main$lambda0}";
    assert_eq!(
        function(&core_text(a, Pass::Optimize), name),
        function(&core_text(b, Pass::Optimize), name)
    );
}

/// 節の関数は、ちょうど1つの handle に属する (`crates/eml_core_ir/src/evidence/handlers.rs` の `entries`)。
#[test]
fn two_handler_copies_keep_their_own_clauses() {
    let text = format!(
        "\
import Yield (Yield(..))

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  yield n
  yield (n + 1)

other : Int -> Unit -> <Yield Int> Unit
other n () = yield (n * 10)
{HIDE}
main : Unit -> <IO> Unit
main () =
  match hide 0 (nums 3) with
    | Box p -> Yield.for_each p (fn x -> println \"\\{{x}}\")
  match hide 0 (other 3) with
    | Box q -> Yield.for_each q (fn x -> println \"\\{{x}}\")
"
    );
    let program = eml_test_support::core_until(&text, Pass::Optimize);
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
    eml_test_support::core_until(&text, Pass::Evidence);
}

/// translate の関数の中で既知の閉包を捕まえた節の複製も、持ち主の handle が値として使うだけなので `internal` にする。
/// そうしないと box の挿入が `$boxed` を挟み、yield するたびに `box` と `unbox` が増える
/// (docs/spec/core-ir.md の「handle を含む関数の複製」)。
#[test]
fn a_clause_copy_in_a_translated_function_is_internal() {
    let text = format!(
        "\
import Yield (Yield(..))

nums : Unit -> <Yield Int> Unit
nums () =
  yield 1
  yield 2
{HIDE}
main : Unit -> <IO> Unit
main () =
  let f = fn x -> x + 1
  match hide 0 nums with
    | Box p ->
      handle p () with
        | fn yield x -> println (show (f x))
"
    );
    let program = eml_test_support::core_until(&text, Pass::Optimize);
    let copy = program
        .functions
        .iter()
        .find(|function| function.name == "main$handle0$yield{0=main$lambda0}")
        .unwrap_or_else(|| panic!("{}", pretty(&program)));
    assert!(copy.internal, "{}", pretty(&program));
    let lir = eml_test_support::lir(&text);
    assert!(
        lir.functions
            .iter()
            .all(|function| !function.name.ends_with("}$boxed")),
        "{}",
        eml_lir::pretty(&lir)
    );
}

/// 同じ関数の閉包でも、捕獲の数が違えば鍵が違う。名前にも捕獲の数を書き、同じ名前の関数が2つにならないようにする
/// (docs/spec/vm.md の「追記」)。`core_text` は表示を読み直すので、名前が重なれば失敗する。
#[test]
fn closures_with_different_capture_counts_get_different_copies() {
    let text = "\
apply_to : (Int -> Int) -> Int -> Int
apply_to f x = if x > 9 then apply_to f (x - 1) else f x

add3 : Int -> Int -> Int -> Int
add3 a b c = a + b + c

pick : (Int -> Int -> Int) -> Int -> Int
pick g n = if n > 0 then pick g (n - 1) else g 1 2

main : Unit -> <IO> Unit
main () =
  println (show (apply_to (add3 1 2) 3))
  println (show (pick (add3 1) 0))
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

/// handler の複製の関数 (名前に `{H=` を含むもの) を、表示の順に並べる。
fn handler_copies(shown: &str) -> String {
    shown
        .lines()
        .filter(|line| line.contains("fn \"") && line.contains("{H="))
        .map(|line| function(shown, line.split('"').nth(1).unwrap()))
        .collect::<Vec<_>>()
        .join("\n")
}

const NUMS: &str = "\
import Yield (Yield(..))

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1
";

/// すぐに再開する節の handler は、経路の関数の複製に節を展開して消える
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「S13e-2 handler への特殊化」)。
#[test]
fn push_fold_has_no_handle_left() {
    let shown = core_text(include_str!("../../../bench/push_fold.em"), Pass::Optimize);
    assert!(
        shown
            .lines()
            .all(|line| !line.contains("handle ") && !line.contains("perform Std.Yield")),
        "{shown}"
    );
    // 畳み込みのラムダは直接の呼び出しになり、そのあと展開される
    let copies = handler_copies(&shown);
    assert!(copies.contains("extern Prelude.+("), "{shown}");
    assert!(!copies.contains("apply"), "{shown}");
}

#[test]
fn for_each_is_specialized() {
    let text = format!(
        "{NUMS}\nmain : Unit -> <IO> Unit\nmain () = Yield.for_each (nums 3) (fn x -> println \"\\{{x}}\")\n"
    );
    let shown = core_text(&text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    insta::assert_snapshot!(format!("{}\n{}", function(&shown, "entry$main"), handler_copies(&shown)), @r#"
    fn entry$main() -> unit {
      let t.1: unit = call "nums$let$go{H=Std.Yield.for_each@#0450afda50afc4ec}"(3, 1)
      return t.1
    }

    fn "nums$let$go{H=Std.Yield.for_each@#0450afda50afc4ec}"(n.0: int, i.1: int) -> unit {
      let t.2: enum = extern "Prelude.Ord Int.>"(i.1, n.0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.10: obj = extern "Prelude.Show Int.show"(i.1)
      let t.11: unit = extern Prelude.println(t.10)
      let t.9: unit = call "nums$let$go{H=Std.Yield.for_each@#0450afda50afc4ec}$h0"(n.0, i.1, t.11)
      return t.9
    b2:
      return ()
    }

    fn "nums$let$go{H=Std.Yield.for_each@#0450afda50afc4ec}$h0"(n.0: int, i.1: int, t.2: unit) -> unit {
      let t.3: int = extern Prelude.+(i.1, 1)
      let t.4: unit = call "nums$let$go{H=Std.Yield.for_each@#0450afda50afc4ec}"(n.0, t.3)
      return t.4
    }
    "#);
}

/// 状態つきの handler では、複製が状態を最後の引数で受け、節の展開が次の状態を継続の関数に渡す。
#[test]
fn a_stateful_handler_threads_its_state() {
    let text = "\
import Std.State (State(..))

count : Int -> <State Int> Unit
count n =
  if n == 0 then ()
  else
    put (get () + 1)
    count (n - 1)

main : Unit -> <IO> Unit
main () =
  let (_, st) = State.run 0 (fn () -> count 5)
  println (show st)
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    insta::assert_snapshot!(handler_copies(&shown), @r#"
    fn "count{H=Std.State.run@#c7a76c0c3d933166}"(n.0: int, st.7: int) -> obj {
      let t.1: enum = extern "Prelude.Eq Int.=="(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.11: obj = call "count{H=Std.State.run@#c7a76c0c3d933166}$h0"(n.0, st.7, st.7)
      return t.11
    b2:
      let d.12: obj = con (,) #0((), st.7)
      return d.12
    }

    fn "count{H=Std.State.run@#c7a76c0c3d933166}$h1"(n.0: int, t.1: unit, st.3: int) -> obj {
      let t.2: int = extern Prelude.-(n.0, 1)
      let t.4: obj = call "count{H=Std.State.run@#c7a76c0c3d933166}"(t.2, st.3)
      return t.4
    }

    fn "count{H=Std.State.run@#c7a76c0c3d933166}$h0"(n.0: int, t.1: int, st.3: int) -> obj {
      let t.2: int = extern Prelude.+(t.1, 1)
      let t.4: obj = call "count{H=Std.State.run@#c7a76c0c3d933166}$h1"(n.0, (), t.2)
      return t.4
    }
    "#);
}

/// `Yield.for_each` の複製の handle が残ることを確かめる。
fn keeps_the_handle(text: &str) {
    let shown = core_text(text, Pass::Optimize);
    assert!(shown.contains("handle "), "{shown}");
    assert!(!shown.contains("{H="), "{shown}");
}

#[test]
fn a_non_tail_yielding_frame_keeps_the_handle() {
    keeps_the_handle(
        "\
import Yield (Yield(..))

emit2 : Int -> <Yield Int> Unit
emit2 i =
  yield i
  yield (i + 1)

nums : Unit -> <Yield Int> Unit
nums () =
  emit2 1
  emit2 3

main : Unit -> <IO> Unit
main () = Yield.for_each nums (fn x -> println \"\\{x}\")
",
    );
}

#[test]
fn a_masked_call_keeps_the_handle() {
    keeps_the_handle(
        "\
import Yield (Yield(..))

gen : (Unit -> <e> Unit) -> Unit -> <Yield Int | e> Unit
gen cb () =
  yield 1
  cb ()

main : Unit -> <IO> Unit
main () = Yield.for_each (gen (fn () -> println \"cb\")) (fn x -> println \"\\{x}\")
",
    );
}

/// 経路に handle があれば特殊化しない。内側の handle は特殊化してよいので、外側の `Yield.for_each` の handle だけを見る。
#[test]
fn a_nested_handle_keeps_the_handle() {
    let text = "\
import Yield (Yield(..))

nums : Unit -> <Yield Int> Unit
nums () =
  yield 1
  handle (yield 2) with
    | fn yield x -> yield (x + 1)

main : Unit -> <IO> Unit
main () = Yield.for_each nums (fn x -> println \"\\{x}\")
";
    let shown = core_text(text, Pass::Optimize);
    let owner = shown
        .lines()
        .find(|line| line.contains("fn \"Std.Yield.for_each@"))
        .unwrap_or_else(|| panic!("{shown}"));
    let owner = function(&shown, owner.split('"').nth(1).unwrap());
    assert!(owner.contains("handle "), "{shown}");
}

#[test]
fn an_unknown_producer_keeps_the_handle() {
    keeps_the_handle(&format!(
        "{NUMS}
data Box a = | Box a

hide : Int -> a -> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

main : Unit -> <IO> Unit
main () =
  match hide 0 (nums 3) with
    | Box p -> Yield.for_each p (fn x -> println \"\\{{x}}\")
"
    ));
}

/// すべての操作が `never` のエフェクトは、中断が巻き戻しで安いので特殊化しない。
#[test]
fn a_never_only_effect_is_not_targeted() {
    let text = "\
import Fail (Fail(..))

check : Int -> <Fail String> Int
check n = if n > 0 then n else fail \"negative\"

main : Unit -> <IO> Unit
main () =
  match Fail.to_result (fn () -> check 3) with
    | Ok n -> println (show n)
    | Err e -> println e
";
    let shown = core_text(text, Pass::Optimize);
    assert!(shown.contains("handle "), "{shown}");
    assert!(!shown.contains("{H="), "{shown}");
}

/// handler の複製の名前とコードは、handle を含む関数と経路の関数のコードだけで決まる (docs/spec/vm.md の「追記」)。
#[test]
fn handler_copies_do_not_depend_on_the_rest_of_the_program() {
    let a = format!(
        "{NUMS}\nmain : Unit -> <IO> Unit\nmain () = println (show (Yield.fold (fn acc x -> acc + x) 0 (nums 3)))\n"
    );
    let b = format!(
        "{NUMS}
other : Int -> Unit -> <Yield Int> Unit
other n () =
  yield n
  yield (n * 2)

main : Unit -> <IO> Unit
main () =
  println (show (Yield.fold (fn acc x -> acc + x) 0 (nums 3)))
  println (show (Yield.fold (fn acc x -> acc * x) 1 (other 4)))
"
    );
    let a = core_text(&a, Pass::Optimize);
    let b = core_text(&b, Pass::Optimize);
    let copies = handler_copies(&a);
    assert!(!copies.is_empty(), "{a}");
    for copy in copies.split("\n\n") {
        assert!(b.contains(copy.trim()), "{copy}\n---\n{b}");
    }
}

/// 定数の操作の引数を節の本体に展開すると、その定数で分ける `switch` ができる。合う case へのジャンプにして、読み直した
/// IR が変換の段の verifier を通ることを確かめる (`core_text` が読み直す)。`T` の `A` のように、フィールドを持つ
/// コンストラクタと混ざる引数のないコンストラクタも `tobj` の定数として届き、飛ばした case のフィールドも写す。
#[test]
fn a_constant_operation_argument_folds_the_clause_switch() {
    let text = "\
import Yield (Yield(..))

data Level = | Info | Warn

effect Say where
  say : Level -> Unit

flags : Unit -> <Yield Bool> Int
flags () =
  yield True
  yield False
  41

levels : Unit -> <Say> Unit
levels () =
  say Info
  say Warn

data T = | A | B Int

effect Pick where
  pick : T -> Unit

picks : Unit -> <Pick> Unit
picks () =
  pick A
  pick (B 3)

main : Unit -> <IO> Unit
main () =
  handle picks () with
    | fn pick t -> match t with
      | A -> println \"a\"
      | B n -> println \"b\\{n}\"
  let r = handle flags () from 0 with
    | fn yield b st -> if b then ((), st + 10) else ((), st + 1)
    | return x st -> x * 100 + st
  println (show r)
  handle levels () with
    | fn say l -> match l with
      | Info -> println \"info\"
      | Warn -> println \"warn\"
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    for line in shown
        .lines()
        .filter(|line| line.trim_start().starts_with("switch "))
    {
        let scrutinee = line.trim_start()["switch ".len()..]
            .split(' ')
            .next()
            .unwrap();
        assert!(!scrutinee.starts_with('#'), "{line}\n{shown}");
    }
}

/// `k` を値として使う節は、継続の関数の閉包を `k` として受ける写し `<g>{k}` を末尾で呼ぶ形になり、`Iter.generate` で
/// 引く形から yield が消える (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「書き換え」)。`Iter`
/// は `hide` を通して `Iter.fold` に届けるので、融合 (S13e-3) で継続の関数が消えない。
#[test]
fn iter_generate_has_no_yield_left() {
    let text = format!(
        "{NUMS}{HIDE}
main : Unit -> <IO> Unit
main () =
  let Box it = hide 0 (Iter.generate (nums 100000))
  println \"\\{{Iter.fold (fn acc x -> acc + x) 0 it}}\"
"
    );
    let shown = core_text(&text, Pass::Optimize);
    assert!(
        shown
            .lines()
            .all(|line| !line.contains("handle ") && !line.contains("perform Std.Yield")),
        "{shown}"
    );
    let continuation = shown
        .lines()
        .find(|line| line.contains(" fn \"") && line.contains("{H=") && line.contains("}$h0\"("))
        .unwrap_or_else(|| panic!("{shown}"));
    assert!(continuation.starts_with("internal "), "{continuation}");
    // 節の写し `<g>{k}` は小さいので、それを呼ぶ handler の複製に展開される
    let copies = handler_copies(&shown);
    assert!(copies.contains("con Std.Iter.Iter #0(k"), "{copies}");
    assert!(!shown.contains("closure cont$"), "{shown}");
}

/// 継続の関数は、`perform` の後で生きている変数を捕獲し、再開の値を最後に受ける。
#[test]
fn a_continuation_captures_its_live_variables() {
    let text = format!(
        "{NUMS}{HIDE}\nmain : Unit -> <IO> Unit\nmain () =\n  let Box it = hide 0 (Iter.generate (nums 3))\n  println (show (Iter.to_list it))\n"
    );
    let shown = core_text(&text, Pass::Optimize);
    let name = shown
        .lines()
        .find(|line| line.contains("fn \"nums$let$go{H=") && line.contains("}$h0\"("))
        .unwrap_or_else(|| panic!("{shown}"));
    let name = name.split('"').nth(1).unwrap().to_string();
    insta::assert_snapshot!(function(&shown, &name), @r#"
    internal fn "nums$let$go{H=Std.Iter.generate@#8c179e1f604536f1}$h0"(n.0: int, i.1: int, t.2: unit) -> tobj lin {
      let t.3: int = extern Prelude.+(i.1, 1)
      let t.5: enum = extern "Prelude.Ord Int.>"(t.3, n.0)
      switch t.5 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let k.6: tobj lin = closure "nums$let$go{H=Std.Iter.generate@#8c179e1f604536f1}$h0"(n.0, t.3)
      let d.7: obj lin = con Std.Iter.Iter #0(k.6)
      let d.8: obj lin = con (,) #0(t.3, d.7)
      let d.9: tobj lin = con Prelude.Option #1(d.8)
      return d.9
    b2:
      return #0
    }
    "#);
}

const WALK: &str = "\
effect Walk where
  step : Int -> Unit
  never stop : Unit -> a
";

/// `never` の操作だけを起こす関数は、`yields` の付け直しで印を失う。経路の関数がそれを末尾でなく呼ぶと、H に届く
/// `perform` を印で見つけられないので、handle を残す。
#[test]
fn a_helper_that_performs_a_never_operation_keeps_the_handle() {
    let text = format!(
        "{WALK}
check : Int -> <Walk> Int
check i = if i == 3 then stop () else i

walk : Int -> <Walk> Unit
walk i =
  let j = check i
  step j
  walk (i + 1)

main : Unit -> <IO> Unit
main () =
  handle walk 1 with
    | fn step n -> println (show n)
    | ctl stop () -> println \"stopped\"
"
    );
    let shown = core_text(&text, Pass::Optimize);
    assert!(shown.contains("handle "), "{shown}");
    assert!(!shown.contains("{H="), "{shown}");
}

/// `never` の操作の `perform` は、節の関数の末尾呼び出しになり、その後ろは消える。
#[test]
fn a_never_operation_calls_its_clause() {
    let text = format!(
        "{WALK}
walk : Int -> <Walk> Unit
walk i =
  if i == 3 then stop ()
  else
    step i
    walk (i + 1)

main : Unit -> <IO> Unit
main () =
  handle walk 1 with
    | fn step n -> println (show n)
    | ctl stop () -> println \"stopped\"
"
    );
    let shown = core_text(&text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    assert!(!shown.contains("perform "), "{shown}");
}

/// 条件つきの `yield` の後の合流のブロックは、継続の関数に写さず、合流のブロックの関数 `f{H}$hjJ` を呼ぶ。写すと、
/// 合流のブロックがその前のどの継続の関数にも入り、コードが `yield` の数の2乗で育つ。EP の `f$jJ` と同じ形で、
/// 大きさは数に比例する (docs/spec/core-ir.md の「handler の複製」)。
#[test]
fn continuations_across_join_blocks_stay_linear_in_size() {
    let size = |count: usize| {
        let lines: String = (1..=count)
            .map(|i| format!("  if n > {i} then yield {i} else ()\n"))
            .collect();
        let text = format!(
            "import Yield (Yield(..))\n\ngen : Int -> Unit -> <Yield Int> Unit\ngen n () =\n{lines}  ()\n\nmain : Unit -> <IO> Unit\nmain () = println (show (Yield.fold (fn a b -> a + b) 0 (gen 7)))\n"
        );
        let shown = core_text(&text, Pass::Optimize);
        assert!(!shown.contains("handle "), "{shown}");
        shown.lines().count()
    };
    let (small, large) = (size(50), size(100));
    assert!(
        large <= small * 5 / 2,
        "{small} lines for 50, {large} for 100"
    );
}

/// 既知のラムダは、`map` の複製のループに展開される。ループの中に乗算の extern が入り、ラムダの呼び出しは残らない
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「インライン化」)。
#[test]
fn a_known_lambda_is_inlined_into_its_copy() {
    let text =
        "main : Unit -> <IO> Unit\nmain () = println (show (List.map (fn x -> x * 2) [1, 2, 3]))";
    let shown = core_text(text, Pass::Optimize);
    let go = function(&shown, "Std.List.map@[Int, Int]$let$go{0=main$lambda0}");
    assert!(!go.contains("main$lambda0("), "{go}");
    insta::assert_snapshot!(go, @r#"
    fn "Std.List.map@[Int, Int]$let$go{0=main$lambda0}"(acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.3: int, rest.4: tobj) -> b2 }
    b1:
      let t.9: tobj = call "Std.List.reverse_onto@[Int]"(#0, acc.1)
      return t.9
    b2:
      let t.10: int = extern Prelude.*(x.3, 2)
      let d.7: tobj = con Prelude.List #1(t.10, acc.1)
      let t.8: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0}"(d.7, rest.4)
      return t.8
    }
    "#);
}

/// 名前が `name` で始まる関数の直接の呼び出しが残っているか。
fn calls(shown: &str, name: &str) -> bool {
    shown.lines().any(|line| {
        line.split_once("= call ")
            .is_some_and(|(_, callee)| callee.trim_start_matches('"').starts_with(name))
    })
}

/// Prelude の合成子は、インライン化で展開される (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の
/// 「インライン化」)。
#[test]
fn pipe_operators_are_inlined() {
    let text = "\
inc : Int -> Int
inc x = x + 1

double : Int -> Int
double x = x * 2

main : Unit -> <IO> Unit
main () =
  println (show ([1, 2, 3] |> List.length))
  println (show ((inc >> double) 4))
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!calls(&shown, "Prelude.|>@"), "{shown}");
    assert!(!calls(&shown, "Prelude.>>@[Int, Int, Int]{"), "{shown}");
}

/// 自分を呼ぶ関数は、インライン化しない。
#[test]
fn a_recursive_function_is_not_inlined() {
    let text = "\
count : Int -> Int -> Int
count acc n = if n == 0 then acc else count (acc + 1) (n - 1)

main : Unit -> <IO> Unit
main () = println (show (count 0 3))
";
    let shown = core_text(text, Pass::Optimize);
    assert!(calls(&shown, "count("), "{shown}");
}

/// handle を含む関数は、インライン化しない。EP は、節の関数がちょうど1つの handle に属することを前提にする
/// (`crates/eml_core_ir/src/evidence/handlers.rs` の `entries`)。
#[test]
fn a_function_with_a_handle_is_not_inlined() {
    let text = "\
effect Ask where
  ask : Unit -> Int

answer : (Unit -> <Ask> Int) -> Int
answer f =
  handle f () + 1 with
    | ctl ask () k -> k 41
{HIDE}
main : Unit -> <IO> Unit
main () =
  match hide 0 (fn () -> ask ()) with
    | Box f -> println (show (answer f))
";
    let text = text.replace("{HIDE}", HIDE);
    let shown = core_text(&text, Pass::Optimize);
    assert!(shown.contains("handle "), "{shown}");
    assert!(calls(&shown, "answer("), "{shown}");
}

/// インライン化した関数のコードは、その関数と呼び出し先の凍らせたコードだけで決まり、ほかの関数の有無によらない
/// (docs/spec/vm.md の「追記」)。
#[test]
fn inlining_does_not_depend_on_the_rest_of_the_program() {
    let a = "\
inc : Int -> Int
inc x = x + 1

main : Unit -> <IO> Unit
main () =
  println (show (List.map (fn x -> inc x) [1, 2]))
";
    let b = "\
inc : Int -> Int
inc x = x + 1

twice : Int -> Int
twice x = inc (inc x)

other : List Int -> List Int
other ys = List.map (fn y -> twice y) ys

main : Unit -> <IO> Unit
main () =
  println (show (List.map (fn x -> inc x) [1, 2]))
  println (show (other [3]))
";
    let name = "Std.List.map@[Int, Int]$let$go{0=main$lambda0}";
    let a = function(&core_text(a, Pass::Optimize), name);
    assert!(!calls(&a, "main$lambda0") && !calls(&a, "inc("), "{a}");
    assert_eq!(a, function(&core_text(b, Pass::Optimize), name));
}

/// 展開は呼び出し先の書き換えた後のコードを写すので、大きさが上限を超えた関数は展開されず、コードは呼び出しの
/// 連なりの深さに比例して育つ。展開した本体の中の呼び出しを呼ぶ側でもう1度展開すると、深さの指数で育つ
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「止まること」)。
#[test]
fn nested_inlining_grows_linearly_with_the_call_depth() {
    let size = |depth: usize| {
        let mut text = String::from("f0 : Int -> Int\nf0 x = x + 1\n\n");
        for i in 1..=depth {
            let calls: Vec<String> = (0..6).map(|k| format!("f{} (x + {k})", i - 1)).collect();
            text.push_str(&format!(
                "f{i} : Int -> Int\nf{i} x = {}\n\n",
                calls.join(" + ")
            ));
        }
        text.push_str(&format!(
            "main : Unit -> <IO> Unit\nmain () = println (display (f{depth} 1))"
        ));
        core_text(&text, Pass::Optimize).lines().count()
    };
    let (shallow, deep) = (size(8), size(16));
    assert!(
        deep <= shallow * 5 / 2,
        "{shallow} lines at depth 8, {deep} at depth 16"
    );
}

/// 同じ関数の中で作った `con` への `match` は、その場で解ける
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「既知のコンストラクタ」)。
#[test]
fn matching_a_local_con_resolves() {
    let text = "\
main : Unit -> <IO> Unit
main () =
  let n = match Some 3 with
    | Some x -> x
    | None -> 0
  println (show n)
";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: obj = extern "Prelude.Show Int.show"(3)
      let t.2: unit = extern Prelude.println(t.1)
      return t.2
    }
    "#);
    // translate は `Some 3` を決定木の中で開くので、`fuse` の規則を見るには、`con` を展開した関数から受ける
    let inlined = "\
some : Int -> Option Int
some x = Some x

main : Unit -> <IO> Unit
main () =
  let n = match some 3 with
    | Some x -> x
    | None -> 0
  println (show n)
";
    let main = function(&core_text(inlined, Pass::Optimize), "entry$main");
    assert!(
        !main.contains("switch ") && !main.contains("con "),
        "{main}"
    );
}

/// 展開した関数が返す `con` は、合流のブロックの引数を通って `match` に届く。`jump` の側で `switch` を決める
/// (docs/future/roadmap.md の「処理系」の項目「jump threading とインライン化」)。
#[test]
fn a_con_returned_by_an_inlined_function_resolves() {
    let text = "\
pick : Bool -> Option Int
pick b = if b then Some 1 else None

main : Unit -> <IO> Unit
main () =
  let c = List.length (Env.args ()) > 0
  match pick c with
    | Some x -> println (show x)
    | None -> println \"none\"
";
    let shown = core_text(text, Pass::Optimize);
    let main = function(&shown, "entry$main");
    assert!(!main.contains("con Prelude.Option"), "{main}");
    insta::assert_snapshot!(main, @r#"
    fn entry$main() -> unit {
      let t.1: int = extern Std.Env.arg_count(())
      let t.2: tobj = call Std.Env.collect_args(t.1, #0)
      let t.3: int = call "Std.List.length@[String]$let$go"(0, t.2)
      let t.4: enum = extern "Prelude.Ord Int.>"(t.3, 0)
      switch t.4 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.5: obj = const "none"
      let t.6: unit = extern Prelude.println(s.5)
      return t.6
    b2:
      let t.7: obj = extern "Prelude.Show Int.show"(1)
      let t.8: unit = extern Prelude.println(t.7)
      return t.8
    }
    "#);
}

/// ほかでも使う `con` は開かない。開くと引数の所有権が2か所に移る。translate は `Unr` の `con` を使う所が
/// いくつあっても決定木の中で開くので、`con` は展開した関数から受ける。
#[test]
fn a_con_with_two_uses_is_not_opened() {
    let text = "\
some : Int -> Option Int
some x = Some x

main : Unit -> <IO> Unit
main () =
  let it = some 3
  let n = List.length [it]
  match it with
    | Some x -> println (show (x + n))
    | None -> ()
";
    let shown = core_text(text, Pass::Optimize);
    let main = function(&shown, "entry$main");
    assert!(main.contains("switch "), "{main}");
}

/// `con` のフィールドの既知の閉包は、`con` を解いた後で既知の閉包になり、`apply` は直接の呼び出しになる。
#[test]
fn next_of_a_known_iter_calls_its_closure() {
    let text = "\
main : Unit -> <IO> Unit
main () =
  match Iter.next (Iter.from_list [1, 2]) with
    | Some (x, rest) ->
      drop rest
      println (show x)
    | None -> ()
";
    let shown = core_text(text, Pass::Optimize);
    let main = function(&shown, "entry$main");
    assert!(!main.contains("apply "), "{main}");
}

/// 展開した関数の `if` の両方の枝が作る組は、合流のブロックの `unpack` を `jump` の側で飛ばして、組を作らずに渡す
/// (docs/future/roadmap.md の「処理系」の項目「jump threading とインライン化」)。translate は同じ関数の中の
/// `let (a, b) = if …` を自分で組を作らない形にするので、組は展開した関数から受ける。
#[test]
fn a_tuple_from_both_arms_of_an_if_is_threaded() {
    let text = "\
pair : Bool -> (Int, Int)
pair c = if c then (1, 2) else (3, 4)

main : Unit -> <IO> Unit
main () =
  let c = List.length (Env.args ()) > 0
  let (a, b) = pair c
  println (show (a - b))
";
    let main = function(&core_text(text, Pass::Optimize), "entry$main");
    assert!(
        !main.contains("con (,)") && !main.contains("unpack "),
        "{main}"
    );
}

/// 自分を直接呼ぶ関数 (融合した後のループ) の表示。
fn loops(text: &str) -> Vec<String> {
    let program = eml_test_support::core_until(text, Pass::Optimize);
    let shown = pretty(&program);
    program
        .functions
        .iter()
        .enumerate()
        .filter(|(index, function)| {
            function
                .blocks
                .iter()
                .flat_map(|block| &block.stmts)
                .any(|stmt| {
                    matches!(stmt, Stmt::Let {
                        var: _,
                        rhs: Rhs::Call { call: Call::Direct(target, _), mask: _ },
                    } if target.0 as usize == *index)
                })
        })
        .map(|(_, function)| self::function(&shown, &function.name))
        .collect()
}

/// 融合したループは、1要素ごとに `Iter` も `Option` も閉包も作らず、`apply` もしない
/// (docs/spec/core-ir.md の「最適化」)。
fn assert_fused(text: &str) {
    let loops = loops(text);
    assert!(
        loops
            .iter()
            .any(|code| code.contains("fn \"Std.Iter.fold@")),
        "{}",
        core_text(text, Pass::Optimize)
    );
    for code in &loops {
        for allocation in [
            "con Std.Iter.Iter",
            "con Prelude.Option",
            "closure ",
            "apply ",
        ] {
            assert!(!code.contains(allocation), "{allocation} in\n{code}");
        }
    }
}

/// `Iter.fold` は、`Iter.generate` が作る既知の `Iter` の形について複製され、要素ごとの確保のないループになる
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「`con` の値の形についての特殊化」)。
#[test]
fn iter_fold_over_generate_is_fused() {
    assert_fused(include_str!("../../../bench/iter_generate.em"));
}

/// `Iter.filter` と `Iter.map` を重ねた形では、`Iter.map` の段は `Iter.fold` のループに入り、ループは `Iter` も
/// `Option` も閉包も作らない。`Iter.generate` は `Iter.filter` が要素を飛ばすループに入り、そのループは `apply` を
/// しない。そのループは再帰するので展開されず、残した要素ごとに `Some` を作って返す。これをなくす継続についての
/// 特殊化は後に回した (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「`con` の値の形についての
/// 特殊化」)。
#[test]
fn iter_pipeline_fuses_map_into_the_fold_and_generate_into_the_filter() {
    let text = include_str!("../../../bench/iter_pipeline.em");
    let loops = loops(text);
    let named = |prefix: &str| {
        loops
            .iter()
            .find(|code| code.contains(&format!("fn \"{prefix}")))
            .unwrap_or_else(|| panic!("no {prefix} loop in\n{}", core_text(text, Pass::Optimize)))
    };
    let fold = named("Std.Iter.fold@");
    assert!(fold.contains("extern Prelude.*("), "{fold}");
    for allocation in ["con Std.Iter.Iter", "con Prelude.Option", "closure "] {
        assert!(!fold.contains(allocation), "{allocation} in\n{fold}");
    }
    let filter = named("Std.Iter.filter@");
    assert!(!filter.contains("apply "), "{filter}");
    assert!(filter.contains("con Prelude.Option"), "{filter}");
}

/// `File` を持つ生産者も融合する。`Iter.fold` だけの形と `Iter.map` を重ねた形のループは、`File` を引数に回し、
/// `Iter` を作らず `apply` もしない。`tests/ui/run/std/fused_iter_file_take.em` は、このループで `File` がちょうど
/// 1回閉じることを確かめる。
#[test]
fn a_fold_over_a_file_producer_is_fused() {
    let text = "\
import Yield (Yield(..))

numbers : Int -> Unit -> <Yield Int, IO> Unit
numbers n () =
  let go file i =
    if i > n then Fs.close file
    else
      yield i
      go file (i + 1)
  go (Fs.open \"numbers.txt\") 1

main : Unit -> <IO> Unit
main () =
  println (show (Iter.fold (fn a b -> a + b) 0 (Iter.generate (numbers 5))))
  println (show (Iter.fold (fn a b -> a + b) 0 (Iter.map (fn x -> x * 2) (Iter.generate (numbers 5)))))
";
    let folds: Vec<String> = loops(text)
        .into_iter()
        .filter(|code| code.contains("fn \"Std.Iter.fold@"))
        .collect();
    assert_eq!(folds.len(), 2, "{}", core_text(text, Pass::Optimize));
    for code in &folds {
        assert!(code.contains("obj lin"), "the File is a parameter:\n{code}");
        for allocation in ["con Std.Iter.Iter", "closure ", "apply "] {
            assert!(!code.contains(allocation), "{allocation} in\n{code}");
        }
    }
}

/// 書き換えで生まれた呼び出しの展開は、関数ごとに `MAX_INLINE_ROUNDS` 回で止まる。自分を引数に受けて呼ぶ閉包は、
/// 展開するたびに既知の閉包への `apply` をまた持ち込むので、上限がないと融合の段が止まらない
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「止まること」)。実行の結果は UI テストの
/// `tests/ui/run/functions/self_applying_closure.em` が確かめる。
#[test]
fn a_self_applying_closure_stops_inlining() {
    let text = "\
data Fix = | Fix (Fix -> Int -> Int)

fix_apply : Fix -> Int -> Int
fix_apply (Fix f) x = f (Fix f) x

main : Unit -> <IO> Unit
main () =
  let lam = fn self n -> if n == 0 then 0 else fix_apply self (n - 1) + 2
  println (show (fix_apply (Fix lam) 10))
";
    let shown = core_text(text, Pass::Optimize);
    assert!(shown.contains("fn entry$main("), "{shown}");
}

/// 融合した複製の名前とコードは鍵だけで決まり、ほかの関数の有無によらない (docs/spec/vm.md の「追記」)。
#[test]
fn fusion_does_not_depend_on_the_rest_of_the_program() {
    let a = format!(
        "{NUMS}
main : Unit -> <IO> Unit
main () = println (show (Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (nums 10))))
"
    );
    let b = format!(
        "{NUMS}
evens : Int -> Unit -> <Yield Int> Unit
evens n () =
  let go i =
    if i > n then ()
    else
      yield (i * 2)
      go (i + 1)
  go 1

main : Unit -> <IO> Unit
main () =
  println (show (Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (nums 10))))
  println (show (Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (evens 10))))
"
    );
    let shown = core_text(&a, Pass::Optimize);
    let fused = loops(&a);
    let [code] = &fused[..] else {
        panic!("{shown}");
    };
    let name = code.split('"').nth(1).unwrap_or_else(|| panic!("{code}"));
    assert!(name.starts_with("Std.Iter.fold@"), "{code}");
    assert_eq!(*code, function(&core_text(&b, Pass::Optimize), name));
}

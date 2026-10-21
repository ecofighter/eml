//! `optimize` パスの骨組み (docs/spec/core-ir.md)。

use eml_core_ir::{Call, Pass, Rhs, Stmt, optimize, parse, pretty};
use eml_lir::Atom;

use crate::common::{core_text, function};

/// 1階のプログラムは、小さな `main` が入口に展開されるほかは変わらない。
#[test]
fn optimize_keeps_a_first_order_program() {
    let text = "main : Unit -> <IO, Panic> Unit\nmain () = println (show (1 + 2))";
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
    let text = "main : Unit -> <IO, Panic> Unit\nmain () =\n  let f = fn x -> x + 1\n  println (show (f 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: int = extern "Prelude.Num Int.+"(2, 1)
      let t.2: obj = extern "Prelude.Show Int.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    "#);
}

#[test]
fn a_partial_application_is_called_directly() {
    let text = "add : Int -> Int -> <Panic> Int\nadd x y = x + y\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let g = add 1\n  println (show (g 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: int = extern "Prelude.Num Int.+"(1, 2)
      let t.2: obj = extern "Prelude.Show Int.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    "#);
}

#[test]
fn an_under_saturated_known_apply_builds_a_closure() {
    let text = "add3 : Int -> Int -> Int -> <Panic> Int\nadd3 x y z = x + y + z\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let g = add3 1\n  let h = g 2\n  println (show (h 3))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: int = extern "Prelude.Num Int.+"(1, 2)
      let t.2: int = extern "Prelude.Num Int.+"(t.1, 3)
      let t.3: obj = extern "Prelude.Show Int.show"(t.2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
    }
    "#);
}

#[test]
fn yield_as_a_value_becomes_a_perform() {
    let text = format!(
        "import Yield (Yield(..))\n\nproduce : Unit -> <Yield Int> Unit\nproduce () =\n  let y = yield\n  y 1\n{HIDE}\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  match hide 0 produce with\n    | Box p -> println (show (Yield.collect p))"
    );
    let shown = core_text(&text, Pass::Optimize);
    // 型引数を持つ包む関数の名前は引用符で囲んで表示される (`call "$op$Std.Yield.Yield.yield@[Int]"(..)`)
    assert!(
        !shown.contains("call $op$") && !shown.contains("call \"$op$"),
        "{shown}"
    );
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
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "render$handle0"), @r#"
    internal yields uniform from 0 fn render$handle0(p.0: unit) -> unit {
      let t.1: unit = mask [Yield@1] yields apply &"$op$yield@[Int]"(1)
      return t.1
    }
    "#);
}

#[test]
fn a_constructor_value_becomes_a_con() {
    // `con` を `hide` に渡し、既知のコンストラクタの規則で解けないようにする
    let text = "data Pair = | Pair Int Int\n\nfirst : Pair -> Int\nfirst (Pair a _) = a\n{HIDE}\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let p = Pair 1\n  match hide 0 (p 2) with\n    | Box q -> println (show (first q))";
    let text = text.replace("{HIDE}", HIDE);
    let text = text.as_str();
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: Pair{int, int} = con Pair #0(1, 2)
      let t.2: Box{Pair{int, int}} = call "hide@[Pair]"(0, t.1)
      unpack t.2 Box #0(q.3: Pair{int, int})
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
    let text = "import State (State(..))\n\nf : Int -> <State Int, Panic> Int\nf n =\n  let g = fn x -> x + n\n  if n == 0 then g 1 else f (n - 1)\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let (r, _) = State.run 0 (fn () -> f 2)\n  println (show r)";
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
adder : Int -> Int -> <Panic> (Int -> <Panic> Int)
adder n x = if n <= 0 then (fn y -> x + y) else adder (n - 1) x

main : Unit -> <IO, Panic> Unit
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

/// 書き換えで使われなくなった `closure` は残さない。
#[test]
fn a_closure_whose_applies_are_all_rewritten_is_dropped() {
    let text = "add : Int -> Int -> <Panic> Int\nadd x y = x + y\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let g = add 1\n  println (show (g 2))";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("closure add("), "{shown}");
}

/// 値をデータに入れて取り出す。取り出した値は既知の閉包でないので、handle の本体に届いても handler への特殊化の
/// 対象にならない。既知の閉包の規則だけを見るテストが使う。`hide` は自分を呼ぶので展開されず、取り出した値は呼ぶ側から見えない。
const HIDE: &str = "
data Box a = | Box a

hide : Int -> a -> <Panic> Box a
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
    let text = "main : Unit -> <IO, Panic> Unit\nmain () = println (show (List.map (fn x -> x * 2) [1, 2, 3]))";
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
      let t.10: int = extern "Prelude.Num Int.*"(x.3, 2)
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
main : Unit -> <IO, Panic> Unit
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
    let text = "main : Unit -> <IO, Panic> Unit\nmain () =\n  let k = 3\n  println (show (List.map (fn x -> x * k) [1, 2, 3]))";
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
      let t.11: int = extern "Prelude.Num Int.*"(x.3, k.9)
      let d.7: tobj = con Prelude.List #1(t.11, acc.1)
      let t.8: tobj = call "Std.List.map@[Int, Int]$let$go{0=main$lambda0/1}"(k.9, d.7, rest.4)
      return t.8
    }
    "#);
}

#[test]
fn a_closure_captured_by_a_closure_is_specialized() {
    let text = format!(
        "{HIDE}\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box it = hide 0 (Iter.from_list [1, 2, 3])\n  let Box mapped = hide 0 (Iter.map (fn x -> x + 1) it)\n  println (show (Iter.to_list mapped))"
    );
    let shown = core_text(&text, Pass::Optimize);
    insta::assert_snapshot!(function(&shown, "Std.Iter.map@[Int, Int]$lambda0{0=main$lambda0}"), @r#"
    internal uniform from 1 fn "Std.Iter.map@[Int, Int]$lambda0{0=main$lambda0}"(it.1: obj lin, p.2: unit) -> tobj lin {
      unpack it.1 Std.Iter.Iter #0(pull.12: tobj lin)
      let t.13: tobj lin = apply pull.12(())
      switch t.13 Prelude.Option { #0 -> b1, #1(x.4: (,){int, obj lin} lin) -> b2 }
    b1:
      let u.7: unit = drop t.13
      return #0
    b2:
      unpack x.4 (,) #0(x.5: int, rest.6: obj lin)
      let t.14: int = extern "Prelude.Num Int.+"(x.5, 1)
      let c.15: tobj lin = closure "Std.Iter.map@[Int, Int]$lambda0{0=main$lambda0}"(rest.6)
      let d.16: obj lin = con Std.Iter.Iter #0(c.15)
      let d.10: (,){int, obj lin} lin = con (,) #0(t.14, d.16)
      let d.11: tobj lin = con Prelude.Option #1(d.10)
      return d.11
    }
    "#);
}

/// `emit` の末尾でない呼び出しが、handler への特殊化を止め、handle の本体を残す。
#[test]
fn generate_reaches_the_handle_body() {
    let text = "\
import Yield (Yield(..))

emit : Int -> <Yield Int> Unit
emit x = yield x

nums : Int -> Unit -> <Yield Int, Panic> Unit
nums n () =
  emit n
  emit (n + 1)

main : Unit -> <IO, Panic> Unit
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

/// データに入れるだけの引数は特殊化しない (docs/spec/core-ir.md の「複製」)。`wrap` は自分を呼ぶので展開されず、
/// 呼び出しのまま残る。自分を呼ぶ所に `f` を渡すと、直接の呼び出しに渡す位置として特殊化するので、別の閉包を渡す。
#[test]
fn a_closure_only_stored_in_data_is_not_specialized() {
    let text = "\
wrap : Int -> (Int -> <Panic> Int) -> <Panic> Option (Int -> <Panic> Int)
wrap n f = if n <= 0 then Some f else wrap (n - 1) (fn x -> x)

main : Unit -> <IO, Panic> Unit
main () =
  match wrap 0 (fn x -> x + 1) with
    | Some g -> println (show (g 1))
    | None -> ()
";
    let names = names(text, Pass::Optimize);
    assert!(names.iter().any(|name| name == "wrap"), "{names:?}");
    assert!(
        names.iter().all(|name| !name.starts_with("wrap{")),
        "{names:?}"
    );
    insta::assert_snapshot!(function(&core_text(text, Pass::Optimize), "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.1: tobj = call wrap(0, &main$lambda0)
      switch t.1 Prelude.Option { #0 -> b1, #1(g.2: tobj) -> b2 }
    b1:
      return ()
    b2:
      let t.3: int = apply g.2(1)
      let t.4: obj = extern "Prelude.Show Int.show"(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    "#);
}

/// 複製の名前とコードは鍵だけで決まり、ほかの関数の有無や作った順によらない (docs/spec/vm.md の「追記」)。
#[test]
fn a_copy_does_not_depend_on_the_rest_of_the_program() {
    let a = "main : Unit -> <IO, Panic> Unit\nmain () =\n  println (show (List.map (fn x -> x + 1) [1, 2]))";
    let b = "other : List Int -> <Panic> List Int\nother ys = List.map (fn y -> y * 2) ys\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  println (show (List.map (fn x -> x + 1) [1, 2]))\n  println (show (other [3]))";
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

nums : Int -> Unit -> <Yield Int, Panic> Unit
nums n () =
  yield n
  yield (n + 1)

other : Int -> Unit -> <Yield Int, Panic> Unit
other n () = yield (n * 10)
{HIDE}
main : Unit -> <IO, Panic> Unit
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
/// 複製は複製元の節の `internal` の印を写す (docs/spec/core-ir.md の「鍵と名前」「handle を含む関数の複製」)。
#[test]
fn a_clause_copy_in_a_translated_function_is_internal() {
    let text = format!(
        "\
import Yield (Yield(..))

nums : Unit -> <Yield Int, Panic> Unit
nums () =
  yield 1
  yield 2
{HIDE}
main : Unit -> <IO, Panic> Unit
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
}

/// 同じ関数の閉包でも、捕獲の数が違えば鍵が違う。名前にも捕獲の数を書き、同じ名前の関数が2つにならないようにする
/// (docs/spec/vm.md の「追記」)。`core_text` は表示を読み直すので、名前が重なれば失敗する。
#[test]
fn closures_with_different_capture_counts_get_different_copies() {
    let text = "\
apply_to : (Int -> <Panic> Int) -> Int -> <Panic> Int
apply_to f x = if x > 9 then apply_to f (x - 1) else f x

add3 : Int -> Int -> Int -> <Panic> Int
add3 a b c = a + b + c

pick : (Int -> Int -> <Panic> Int) -> Int -> <Panic> Int
pick g n = if n > 0 then pick g (n - 1) else g 1 2

main : Unit -> <IO, Panic> Unit
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

map_nested : (a -> <Panic> b) -> Nested a -> <Panic> Nested b
map_nested f n = match n with
  | Flat x -> Flat (f x)
  | Nest inner -> Nest (map_nested (List.map f) inner)

add3 : Int -> Int -> Int -> <Panic> Int
add3 a b c = a + b + c

main : Unit -> <IO, Panic> Unit
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

nums : Int -> Unit -> <Yield Int, Panic> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1
";

/// すぐに再開する節の handler は、経路の関数の複製に節を展開して消える
/// (docs/spec/core-ir.md の「handler への特殊化」)。
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
    assert!(copies.contains("extern \"Prelude.Num Int.+\"("), "{shown}");
    assert!(!copies.contains("apply"), "{shown}");
}

#[test]
fn for_each_is_specialized() {
    let text = format!(
        "{NUMS}\nmain : Unit -> <IO, Panic> Unit\nmain () = Yield.for_each (nums 3) (fn x -> println \"\\{{x}}\")\n"
    );
    let shown = core_text(&text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    insta::assert_snapshot!(format!("{}\n{}", function(&shown, "entry$main"), handler_copies(&shown)), @r#"
    fn entry$main() -> unit {
      let t.1: unit = call "nums$let$go{H=Std.Yield.for_each@#0450afda50afc4ec}"(3, 1)
      return ()
    }

    fn "nums$let$go{H=Std.Yield.for_each@#0450afda50afc4ec}"(n.0: int, i.1: int) -> unit {
      let t.2: enum = extern "Prelude.Ord Int.>"(i.1, n.0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.8: obj = extern "Prelude.Show Int.show"(i.1)
      let t.9: unit = extern Prelude.println(t.8)
      let t.4: int = extern "Prelude.Num Int.+"(i.1, 1)
      let t.7: unit = call "nums$let$go{H=Std.Yield.for_each@#0450afda50afc4ec}"(n.0, t.4)
      return t.7
    b2:
      return ()
    }
    "#);
}

/// 状態つきの handler では、複製が状態を最後の引数で受ける。`count` は `Unit` を返すので、複製は次の状態だけを返す
/// (docs/spec/core-ir.md の「handler の複製」)。展開した節は次の状態をブロックの引数で後ろへ渡すが、ここではそのブロックに
/// 入る辺が1本なので、融合がブロックをつないで引数は残らない。状態を受けるブロックの引数は、2本の辺が入る
/// `a_conditional_perform_passes_the_state_to_the_join` が固定する。
#[test]
fn a_stateful_handler_threads_its_state() {
    let text = "\
import Std.State (State(..))

count : Int -> <State Int, Panic> Unit
count n =
  if n == 0 then ()
  else
    put (get () + 1)
    count (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let (_, st) = State.run 0 (fn () -> count 5)
  println (show st)
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    insta::assert_snapshot!(handler_copies(&shown), @r#"
    fn "count{H=Std.State.run@#c7a76c0c3d933166}"(n.0: int, st.7: int) -> int {
      let t.1: enum = extern "Prelude.Eq Int.=="(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern "Prelude.Num Int.+"(st.7, 1)
      let t.5: int = extern "Prelude.Num Int.-"(n.0, 1)
      let t.10: int = call "count{H=Std.State.run@#c7a76c0c3d933166}"(t.5, t.3)
      return t.10
    b2:
      return st.7
    }
    "#);
}

/// 末尾でない `put` のループ (`bench/state_loop.em` と同じプログラム) も、状態を渡す形で特殊化される。`handle` が
/// 消え、`loop` の複製が状態を引数で受けることを、スナップショットで固定する。
#[test]
fn a_non_tail_put_is_specialized() {
    let text = include_str!("../../../bench/state_loop.em");
    let shown = core_text(text, Pass::Optimize);
    assert!(
        shown
            .lines()
            .all(|line| !line.contains("handle ") && !line.contains("perform Std.State")),
        "{shown}"
    );
    // `bump` の複製は小さいので、融合が `loop` の複製に展開し、`bump` の `get` と `put` は状態の足し算に
    // なる
    insta::assert_snapshot!(handler_copies(&shown), @r#"
    fn "loop{H=Std.State.run@#c7a76c0c3d933166}"(i.0: int, n.1: int, st.6: int) -> int {
      let t.2: enum = extern "Prelude.Ord Int.>"(i.0, n.1)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.9: int = extern "Prelude.Num Int.+"(st.6, i.0)
      let t.4: int = extern "Prelude.Num Int.+"(i.0, 1)
      let t.8: int = call "loop{H=Std.State.run@#c7a76c0c3d933166}"(t.4, n.1, t.9)
      return t.8
    b2:
      return st.6
    }
    "#);
}

/// `if` の片方の腕だけが `put` する。合流のブロックは状態を引数で受け、2つの腕はそれぞれの今の状態を渡す。
#[test]
fn a_conditional_perform_passes_the_state_to_the_join() {
    let text = "\
import Std.State (State(..))

tally : List Int -> <State Int, Panic> Unit
tally xs = match xs with
  | Nil -> ()
  | x :: rest ->
    if x % 2 == 0 then put (get () + x) else ()
    tally rest

main : Unit -> <IO, Panic> Unit
main () = println (show (State.exec 0 (fn () -> tally [1, 2, 3, 4, 5, 6])))
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    insta::assert_snapshot!(handler_copies(&shown), @r#"
    fn "tally{H=Std.State.run@#c7a76c0c3d933166}"(xs.0: tobj, st.10: int) -> int {
      switch xs.0 Prelude.List { #0 -> b1, #1(x.1: int, rest.2: tobj) -> b2 }
    b1:
      return st.10
    b2:
      let t.3: int = extern "Prelude.Div Int.%"(x.1, 2)
      let t.4: enum = extern "Prelude.Eq Int.=="(t.3, 0)
      switch t.4 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      jump b5((), st.10)
    b4:
      let t.6: int = extern "Prelude.Num Int.+"(st.10, x.1)
      jump b5((), t.6)
    b5(t.8: unit, st.13: int):
      let t.14: int = call "tally{H=Std.State.run@#c7a76c0c3d933166}"(rest.2, st.13)
      return t.14
    }
    "#);
}

/// 結果が `Unit` でない経路の関数の複製は、`return` で結果と次の状態の組を作って返す。
#[test]
fn a_pair_result_is_built_at_return() {
    let text = "\
import Std.State (State(..))

count : Int -> <State Int, Panic> Int
count n = if n == 0 then 0 else
  put (get () + 1)
  count (n - 1)

main : Unit -> <IO, Panic> Unit
main () = println (show (State.exec 0 (fn () -> count 5)))
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    insta::assert_snapshot!(handler_copies(&shown), @r#"
    fn "count{H=Std.State.run@#6d0e8cc9f0bb1f6e}"(n.0: int, st.7: int) -> (,){int, int} {
      let t.1: enum = extern "Prelude.Eq Int.=="(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern "Prelude.Num Int.+"(st.7, 1)
      let t.5: int = extern "Prelude.Num Int.-"(n.0, 1)
      let t.10: (,){int, int} = call "count{H=Std.State.run@#6d0e8cc9f0bb1f6e}"(t.5, t.3)
      return t.10
    b2:
      let d.11: (,){int, int} = con (,) #0(0, st.7)
      return d.11
    }
    "#);
}

/// `Yield.for_each` の複製の handle が残ることを確かめる。
fn keeps_the_handle(text: &str) {
    let shown = core_text(text, Pass::Optimize);
    assert!(shown.contains("handle "), "{shown}");
    assert!(!shown.contains("{H="), "{shown}");
}

const EMIT2: &str = "\
import Yield (Yield(..))

emit2 : Int -> <Yield Int, Panic> Unit
emit2 i =
  yield i
  yield (i + 1)

nums : Unit -> <Yield Int, Panic> Unit
nums () =
  emit2 1
  emit2 3
";

/// `k` を値として使う節の handler (`Iter.generate`) は末尾の経路の形のままなので、末尾でない `yields` の呼び出しが
/// 経路にあれば handle を残す。
#[test]
fn a_non_tail_yielding_frame_keeps_the_handle() {
    keeps_the_handle(&format!(
        "{EMIT2}\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Iter.to_list (Iter.generate nums)))\n"
    ));
}

/// すぐに再開する節だけの handler は、末尾でない `yields` の呼び出しをたどって特殊化する。
#[test]
fn a_non_tail_yielding_frame_is_specialized_when_clauses_resume() {
    let text = format!(
        "{EMIT2}\nmain : Unit -> <IO, Panic> Unit\nmain () = Yield.for_each nums (fn x -> println \"\\{{x}}\")\n"
    );
    let shown = core_text(&text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
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

/// 経路に特殊化しない handle が残れば、外側の handle も特殊化しない。`try_panic` の handle は `never` の操作だけの
/// エフェクトなので特殊化されず、外側の `Yield.for_each` の handle が残る。
#[test]
fn a_nested_handle_keeps_the_handle() {
    let text = "\
import Yield (Yield(..))

nums : Unit -> <Yield Int, Panic> Unit
nums () =
  yield 1
  let _ = try_panic (fn () -> yield 2)
  yield 3

main : Unit -> <IO, Panic> Unit
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

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

main : Unit -> <IO, Panic> Unit
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
        "{NUMS}\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Yield.fold (fn acc x -> acc + x) 0 (nums 3)))\n"
    );
    let b = format!(
        "{NUMS}
other : Int -> Unit -> <Yield Int, Panic> Unit
other n () =
  yield n
  yield (n * 2)

main : Unit -> <IO, Panic> Unit
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

/// `bench/state_nested.em` のプログラム。状態つきの2つの handler (`Std.State` と `Tick`) が重なる。
const STATE_NESTED: &str = include_str!("../../../bench/state_nested.em");

/// 後の回の handler の複製の名前か。内側の複製 `f{H=…}` の複製の名前は `f{H=…}{H=…}` で、64字を超えると内側の
/// 複製の名前にハッシュを付けた `f{H=…}@#<16 hex>` になる (docs/spec/core-ir.md の「handler の複製」)。ハッシュのない
/// 名前は `{H=` を2つ以上持つものを取り、既知の閉包の複製の最初の回の複製 `f{0=…}{H=…}` は取らない。ハッシュの名前は、
/// ハッシュの前の部分に `{H=` を持つものを取る。ただし、名前の長い、既知の閉包の複製の最初の回の複製 `f{0=…}{H=…}@#<16 hex>`
/// も同じ形なので、これだけは見分けられない。
fn nested_copy(line: &str) -> bool {
    let Some(name) = line.split('"').nth(1).filter(|_| line.contains("fn \"")) else {
        return false;
    };
    let (base, hashed) = match name.rsplit_once("@#") {
        Some((base, hash)) if hash.len() == 16 => (base, true),
        _ => (name, false),
    };
    base.matches("{H=").count() >= if hashed { 1 } else { 2 }
}

/// 入れ子の handler は内側から特殊化する (docs/spec/core-ir.md の「handler への特殊化」)。内側の `Tick` の handle が
/// 複製の呼び出しに変わると、外側の `State.run` の経路は内側の複製を通ってたどれるようになり、外側の handler は
/// 内側の複製の複製を作る。
#[test]
fn nested_handlers_are_specialized_inside_out() {
    let shown = core_text(STATE_NESTED, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    assert!(shown.lines().any(nested_copy), "{shown}");
    let all = handler_copies(&shown);
    let nested: Vec<&str> = all
        .split("\n\n")
        .filter(|copy| copy.lines().next().is_some_and(nested_copy))
        .map(str::trim)
        .collect();
    insta::assert_snapshot!(nested.join("\n\n"), @r#"
    fn "loop{H=count@#3090723f4081c669}@#41424a60520c7b81"(i.0: int, n.1: int, c.6: int, st.9: int) -> (,){int, int} {
      let t.2: enum = extern "Prelude.Ord Int.>"(i.0, n.1)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.14: int = extern "Prelude.Num Int.+"(c.6, 1)
      let t.15: int = extern "Prelude.Num Int.+"(st.9, i.0)
      let t.4: int = extern "Prelude.Num Int.+"(i.0, 1)
      let t.12: (,){int, int} = call "loop{H=count@#3090723f4081c669}@#41424a60520c7b81"(t.4, n.1, t.14, t.15)
      return t.12
    b2:
      let d.13: (,){int, int} = con (,) #0(c.6, st.9)
      return d.13
    }
    "#);
}

/// 後の回の複製も、その回の初めのコードから作るので、名前でコードが決まる (docs/spec/vm.md の「追記」)。
#[test]
fn nested_handler_copies_do_not_depend_on_the_rest_of_the_program() {
    let main = "main : Unit -> <IO, Panic> Unit
main () =
  let (ticks, total) = State.run 0 counted
  println \"\\{total} \\{ticks}\"
";
    let other = "double : Int -> <State Int, Panic> Unit
double n =
  if n <= 0 then ()
  else
    put (get () * 2)
    double (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let (ticks, total) = State.run 0 counted
  println \"\\{total} \\{ticks}\"
  println (show (State.exec 1 (fn () -> double 10)))
";
    assert!(STATE_NESTED.ends_with(main));
    let a = core_text(STATE_NESTED, Pass::Optimize);
    let b = core_text(&STATE_NESTED.replace(main, other), Pass::Optimize);
    let all = handler_copies(&a);
    let copies: Vec<&str> = all
        .split("\n\n")
        .filter(|copy| copy.lines().next().is_some_and(nested_copy))
        .map(str::trim)
        .collect();
    assert!(!copies.is_empty(), "{a}");
    for copy in copies {
        assert!(b.contains(copy), "{copy}\n---\n{b}");
    }
}

/// 状態を渡す形を確かめる UI テストのプログラムが、その形で特殊化されることを確かめる。`lin_state` の handle は
/// 消え、`under_generator` では `Std.State` の handle が消えて、外側の `Iter.generate` の handle だけが残る。`panic` では
/// `Std.State` の handle が消えて、`try_panic` の handle だけが残る。
#[test]
fn passing_ui_programs_are_specialized() {
    let lin_state = core_text(
        include_str!("../../../tests/ui/run/effects/state_passing_lin_state.em"),
        Pass::Optimize,
    );
    assert!(!lin_state.contains("handle "), "{lin_state}");
    let generator = core_text(
        include_str!("../../../tests/ui/run/effects/state_passing_under_generator.em"),
        Pass::Optimize,
    );
    let owners: Vec<&str> = generator
        .lines()
        .filter(|line| line.contains("fn \""))
        .filter_map(|line| line.split('"').nth(1))
        .filter(|name| function(&generator, name).contains("handle "))
        .collect();
    assert!(
        owners.len() == 1 && owners[0].starts_with("Std.Iter.generate@"),
        "{owners:?}\n{generator}"
    );
    assert!(generator.contains("fn \"sums{H="), "{generator}");
    let panic = core_text(
        include_str!("../../../tests/ui/run/effects/state_passing_panic.em"),
        Pass::Optimize,
    );
    let handles: Vec<&str> = panic
        .lines()
        .filter(|line| line.contains("fn \""))
        .filter_map(|line| line.split('"').nth(1))
        .filter(|name| function(&panic, name).contains("handle "))
        .collect();
    assert!(
        handles.len() == 1 && handles[0].starts_with("Prelude.try_panic@"),
        "{handles:?}\n{panic}"
    );
    assert!(panic.contains("{H=Std.State."), "{panic}");
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

main : Unit -> <IO, Panic> Unit
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
/// 引く形から yield が消える (docs/spec/core-ir.md の「handler の複製」)。`Iter`
/// は `hide` を通して `Iter.fold` に届けるので、融合で継続の関数が消えない。
#[test]
fn iter_generate_has_no_yield_left() {
    let text = format!(
        "{NUMS}{HIDE}
main : Unit -> <IO, Panic> Unit
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
    // 型ごとの `$cont$` の名前は型引数を持つので、いつも引用符で囲んで表示される (`closure "$cont$@[..]"(..)`)
    assert!(
        !shown.contains("closure $cont$") && !shown.contains("closure \"$cont$"),
        "{shown}"
    );
}

/// 継続の関数は、`perform` の後で生きている変数を捕獲し、再開の値を最後に受ける。
#[test]
fn a_continuation_captures_its_live_variables() {
    let text = format!(
        "{NUMS}{HIDE}\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box it = hide 0 (Iter.generate (nums 3))\n  println (show (Iter.to_list it))\n"
    );
    let shown = core_text(&text, Pass::Optimize);
    let name = shown
        .lines()
        .find(|line| line.contains("fn \"nums$let$go{H=") && line.contains("}$h0\"("))
        .unwrap_or_else(|| panic!("{shown}"));
    let name = name.split('"').nth(1).unwrap().to_string();
    insta::assert_snapshot!(function(&shown, &name), @r#"
    internal uniform from 2 fn "nums$let$go{H=Std.Iter.generate@#8c179e1f604536f1}$h0"(n.0: int, i.1: int, t.2: unit) -> tobj lin {
      let t.3: int = extern "Prelude.Num Int.+"(i.1, 1)
      let t.5: enum = extern "Prelude.Ord Int.>"(t.3, n.0)
      switch t.5 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let k.6: tobj lin = closure "nums$let$go{H=Std.Iter.generate@#8c179e1f604536f1}$h0"(n.0, t.3)
      let d.7: obj lin = con Std.Iter.Iter #0(k.6)
      let d.8: (,){int, obj lin} lin = con (,) #0(t.3, d.7)
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

walk : Int -> <Walk, Panic> Unit
walk i =
  let j = check i
  step j
  walk (i + 1)

main : Unit -> <IO, Panic> Unit
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
walk : Int -> <Walk, Panic> Unit
walk i =
  if i == 3 then stop ()
  else
    step i
    walk (i + 1)

main : Unit -> <IO, Panic> Unit
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
            "import Yield (Yield(..))\n\ngen : Int -> Unit -> <Yield Int> Unit\ngen n () =\n{lines}  ()\n\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Yield.fold (fn a b -> a + b) 0 (gen 7)))\n"
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
/// (docs/spec/core-ir.md の「インライン化」)。
#[test]
fn a_known_lambda_is_inlined_into_its_copy() {
    let text = "main : Unit -> <IO, Panic> Unit\nmain () = println (show (List.map (fn x -> x * 2) [1, 2, 3]))";
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
      let t.10: int = extern "Prelude.Num Int.*"(x.3, 2)
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

/// Prelude の合成子は、インライン化で展開される (docs/spec/core-ir.md の「インライン化」)。
#[test]
fn pipe_operators_are_inlined() {
    let text = "\
inc : Int -> <Panic> Int
inc x = x + 1

double : Int -> <Panic> Int
double x = x * 2

main : Unit -> <IO, Panic> Unit
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
count : Int -> Int -> <Panic> Int
count acc n = if n == 0 then acc else count (acc + 1) (n - 1)

main : Unit -> <IO, Panic> Unit
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

answer : (Unit -> <Ask> Int) -> <Panic> Int
answer f =
  handle f () + 1 with
    | ctl ask () k -> k 41
{HIDE}
main : Unit -> <IO, Panic> Unit
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
inc : Int -> <Panic> Int
inc x = x + 1

main : Unit -> <IO, Panic> Unit
main () =
  println (show (List.map (fn x -> inc x) [1, 2]))
";
    let b = "\
inc : Int -> <Panic> Int
inc x = x + 1

twice : Int -> <Panic> Int
twice x = inc (inc x)

other : List Int -> <Panic> List Int
other ys = List.map (fn y -> twice y) ys

main : Unit -> <IO, Panic> Unit
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
/// (docs/spec/core-ir.md の「融合が止まること」)。
#[test]
fn nested_inlining_grows_linearly_with_the_call_depth() {
    let size = |depth: usize| {
        let mut text = String::from("f0 : Int -> <Panic> Int\nf0 x = x + 1\n\n");
        for i in 1..=depth {
            let calls: Vec<String> = (0..6).map(|k| format!("f{} (x + {k})", i - 1)).collect();
            text.push_str(&format!(
                "f{i} : Int -> <Panic> Int\nf{i} x = {}\n\n",
                calls.join(" + ")
            ));
        }
        text.push_str(&format!(
            "main : Unit -> <IO, Panic> Unit\nmain () = println (display (f{depth} 1))"
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
/// (docs/spec/core-ir.md の「既知のコンストラクタ」)。
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

main : Unit -> <IO, Panic> Unit
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
/// (docs/spec/core-ir.md の「既知のコンストラクタ」)。
#[test]
fn a_con_returned_by_an_inlined_function_resolves() {
    let text = "\
pick : Bool -> Option Int
pick b = if b then Some 1 else None

main : Unit -> <IO, Panic> Unit
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

main : Unit -> <IO, Panic> Unit
main () =
  let it = some 3
  let n = List.length [it]
  match it with
    | Some x -> println (show (x + n))
    | None -> ()
";
    let shown = core_text(text, Pass::Optimize);
    let main = function(&shown, "entry$main");
    let some = main
        .lines()
        .find(|line| line.contains("= con Prelude.Option #1("))
        .and_then(|line| line.trim().strip_prefix("let "))
        .and_then(|line| line.split(':').next())
        .unwrap_or_else(|| panic!("the con stays:\n{main}"));
    assert!(
        main.contains(&format!("switch {some} Prelude.Option")),
        "{main}"
    );
}

/// 同じ関数の中の `con` への `switch` も、`switch` のほかの行き先で `con` を使っていてよい。translate は
/// フィールドを束縛しない葉で `Unr` でない値を `drop` するので、`None` の葉が `Option Fs.File` を捨てる。
/// `merge_jumps` が合流のブロックをつないで、`jump` の側で決めるはずだった `switch` が `con` と同じブロックに来ても、
/// jump threading と同じ規則で解ける (docs/spec/core-ir.md の「既知のコンストラクタ」)。
#[test]
fn a_local_switch_resolves_when_another_case_drops_the_con() {
    let text = "\
maybe_file : Bool -> Fs.File -> <IO> Option Fs.File
maybe_file c f =
  if c then Some f
  else
    Fs.close f
    None

close_opt : Option Fs.File -> <IO> Int
close_opt o = match o with
  | None -> 0
  | Some f ->
    Fs.close f
    1

main : Unit -> <IO> Unit
main () = println (show (close_opt (maybe_file True (Fs.open \"in.txt\"))))
";
    let main = function(&core_text(text, Pass::Optimize), "entry$main");
    assert!(
        !main.contains("con Prelude.Option") && !main.contains("switch "),
        "{main}"
    );
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
/// (docs/spec/core-ir.md の「既知のコンストラクタ」)。translate は同じ関数の中の
/// `let (a, b) = if …` を自分で組を作らない形にするので、組は展開した関数から受ける。
#[test]
fn a_tuple_from_both_arms_of_an_if_is_threaded() {
    let text = "\
pair : Bool -> (Int, Int)
pair c = if c then (1, 2) else (3, 4)

main : Unit -> <IO, Panic> Unit
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
/// (docs/spec/core-ir.md の「`con` の値の形についての特殊化」)。
#[test]
fn iter_fold_over_generate_is_fused() {
    assert_fused(include_str!("../../../bench/iter_generate.em"));
}

/// `Iter.filter` と `Iter.map` を重ねた形では、`Iter.map` の段は `Iter.fold` のループに入り、ループは `Iter` も
/// `Option` も閉包も作らない。`Iter.generate` は `Iter.filter` が要素を飛ばすループに入り、そのループは `apply` を
/// しない。そのループは再帰するので展開されず、残した要素ごとに `Some` を作って返す。これをなくす継続についての
/// 特殊化は後に回した (docs/spec/core-ir.md の「`con` の値の形についての特殊化」)。
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
    assert!(fold.contains("extern \"Prelude.Num Int.*\"("), "{fold}");
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

numbers : Int -> Unit -> <Yield Int, IO, Panic> Unit
numbers n () =
  let go file i =
    if i > n then Fs.close file
    else
      yield i
      go file (i + 1)
  go (Fs.open \"numbers.txt\") 1

main : Unit -> <IO, Panic> Unit
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
/// (docs/spec/core-ir.md の「融合が止まること」)。実行の結果は UI テストの
/// `tests/ui/run/functions/self_applying_closure.em` が確かめる。
#[test]
fn a_self_applying_closure_stops_inlining() {
    let text = "\
data Fix = | Fix (Fix -> Int -> <Panic> Int)

fix_apply : Fix -> Int -> <Panic> Int
fix_apply (Fix f) x = f (Fix f) x

main : Unit -> <IO, Panic> Unit
main () =
  let lam = fn self n -> if n == 0 then 0 else fix_apply self (n - 1) + 2
  println (show (fix_apply (Fix lam) 10))
";
    let shown = core_text(text, Pass::Optimize);
    assert!(shown.contains("fn entry$main("), "{shown}");
}

/// 自分を引数に受けて呼ぶ閉包で、本体が `g (Fix g)` を `calls` 回呼ぶもの。`name` の関数が閉包を作って呼ぶ。
fn self_applying(name: &str, calls: usize) -> String {
    let applies: Vec<String> = (1..=calls)
        .map(|k| format!("g (Fix g) (n - {k})"))
        .collect();
    format!(
        "data Fix = | Fix (Fix -> Int -> <Panic> Int)

{name} : Int -> <Panic> Int
{name} m =
  let lam = fn self n -> match self with
    | Fix g -> if n <= 0 then 1 else {}
  lam (Fix lam) m
",
        applies.join(" + ")
    )
}

/// 書き換えで生まれた呼び出しの展開は、関数ごとの大きさの予算 (`GROWTH_FACTOR`) で止まる。展開した本体が既知の
/// 閉包への `apply` を k 個持ち込むと、回ごとに k 倍に育つためである。予算がなく回数の上限
/// (`MAX_INLINE_ROUNDS`) だけで止めると、呼び出しが2つで6,163行、3つで157,491行、4つで1,747,659行になった
/// (docs/spec/core-ir.md の「融合が止まること」)。予算で止まると、呼び出しの数によらず数百行に収まる。
#[test]
fn inlining_fusion_created_calls_stays_within_a_budget() {
    for calls in [2, 3, 4] {
        let text = format!(
            "{}\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (spin 5))\n",
            self_applying("spin", calls)
        );
        let started = std::time::Instant::now();
        let lines = core_text(&text, Pass::Optimize).lines().count();
        let elapsed = started.elapsed();
        assert!(lines <= 1000, "{lines} lines with {calls} calls per body");
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "{elapsed:?} with {calls} calls per body"
        );
    }
}

/// 予算は関数ごとに決まる。予算を使い切る関数があっても、同じプログラムのほかの関数の融合は変わらない。
/// プログラム全体の予算にすると、名前が同じ関数のコードがプログラムの残りに依って変わる (docs/spec/vm.md の
/// 「追記」)。
#[test]
fn the_growth_budget_binds_per_function() {
    let text = format!(
        "{NUMS}\n{}
main : Unit -> <IO, Panic> Unit
main () =
  println (show (spin 5))
  println (show (Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (nums 10))))
",
        self_applying("spin", 4)
    );
    let shown = core_text(&text, Pass::Optimize);
    let spin = function(&shown, "spin");
    // 予算を使い切った所で、書き換えで生まれた呼び出しが展開されずに残る
    assert!(spin.contains("call spin$lambda0("), "{spin}");
    assert!(spin.lines().count() <= 500, "{spin}");
    assert_fused(&text);
}

/// 融合した複製の名前とコードは鍵だけで決まり、ほかの関数の有無によらない (docs/spec/vm.md の「追記」)。
#[test]
fn fusion_does_not_depend_on_the_rest_of_the_program() {
    let a = format!(
        "{NUMS}
main : Unit -> <IO, Panic> Unit
main () = println (show (Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (nums 10))))
"
    );
    let b = format!(
        "{NUMS}
evens : Int -> Unit -> <Yield Int, Panic> Unit
evens n () =
  let go i =
    if i > n then ()
    else
      yield (i * 2)
      go (i + 1)
  go 1

main : Unit -> <IO, Panic> Unit
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

/// `Iter` を作り合う2つの関数 `ping` と `pong` (2つの関数の輪) と、`Iter` を引く側の関数。どちらも相手の `Iter` を
/// `Iter.map` で包んで返すので、`Iter.map` の閉包の複製の鍵に、輪の相手の閉包の複製を持つ `Iter` の形が入る。
const CYCLES: &str = "
add : Int -> Int -> <Panic> Int
add a b = a + b

double : Int -> <Panic> Int
double x = x * 2

ping : (Int -> <e> Int) -> Iter <e> Int -> Iter <e> Int
ping f it =
  Iter (fn () ->
    match Iter.next it with
      | None -> None
      | Some (x, rest) -> Some (f x, Iter.map f (pong f rest)))

pong : (Int -> <e> Int) -> Iter <e> Int -> Iter <e> Int
pong f it =
  Iter (fn () ->
    match Iter.next it with
      | None -> None
      | Some (x, rest) -> Some (f (f x), Iter.map f (ping f rest)))
";

/// 2つのプログラムに共通する名前の関数は、同じコードになる。その名前を返す。プログラムごとに違う `main` と入口の
/// 関数は比べない。
fn shared_functions(a: &str, b: &str) -> Vec<String> {
    let (a_shown, b_shown) = (core_text(a, Pass::Optimize), core_text(b, Pass::Optimize));
    let b_names = names(b, Pass::Optimize);
    let shared: Vec<String> = names(a, Pass::Optimize)
        .into_iter()
        .filter(|name| {
            !name.starts_with("entry$") && !name.starts_with("main") && b_names.contains(name)
        })
        .collect();
    for name in &shared {
        assert_eq!(function(&a_shown, name), function(&b_shown, name), "{name}");
    }
    shared
}

/// 自分の閉包を値として参照する複製 (`Iter.map` の写し) と、互いを参照する複製の輪 (`ping` と `pong` の閉包の写し)
/// は、引く側が違っても、輪にどちらの端から入っても、同じ名前なら同じコードになる (docs/spec/core-ir.md の
/// 「融合が止まること」と docs/spec/vm.md の「追記」)。複製を作る順は入り方で変わるので、書き換えている複製を含む組の
/// 複製を読まない規則 (`settled`) がこれを守る。この規則を外して、書き換え終えた複製をいつも読んでよいことにすると、
/// 輪の `Iter.map` の写しのコードが入り方で変わり、このテストは失敗する。
#[test]
fn copies_in_cycles_do_not_depend_on_the_entry_into_the_cycle() {
    let a = format!(
        "import Iter (Iter(..))
{NUMS}{CYCLES}
main : Unit -> <IO, Panic> Unit
main () =
  println (show (Iter.to_list (Iter.take 3 (Iter.map double (Iter.generate (nums 10))))))
  println (show (Iter.fold add 0 (Iter.take 3 (ping double (Iter.generate (nums 10))))))
"
    );
    let b = format!(
        "import Iter (Iter(..))
{NUMS}{CYCLES}
main : Unit -> <IO, Panic> Unit
main () =
  println (show (Iter.to_list (Iter.zip (Iter.map double (Iter.generate (nums 10))) (Iter.generate (nums 3)))))
  println (show (Iter.fold add 0 (Iter.take 3 (Iter.map double (Iter.generate (nums 10))))))
  println (show (Iter.fold add 0 (Iter.take 3 (pong double (Iter.generate (nums 10))))))
"
    );
    let shared = shared_functions(&a, &b);
    // 共通する名前の中に、互いの閉包を作る2つの `Iter.map` の写し (複製の輪) がある
    let shown = core_text(&a, Pass::Optimize);
    let maps: Vec<&String> = shared
        .iter()
        .filter(|name| name.starts_with("Std.Iter.map@"))
        .collect();
    let refers =
        |from: &str, to: &str| function(&shown, from).contains(&format!("closure {to:?}("));
    assert!(
        maps.iter()
            .any(|x| maps.iter().any(|y| x != y && refers(x, y) && refers(y, x))),
        "no shared cycle of Iter.map copies in {shared:#?}"
    );
}

/// IR のテキストを読み、`optimize` の後の表示を返す。変換の段の verifier を通ることも確かめる。
fn optimized(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    optimize(&mut program);
    let shown = pretty(&program);
    eml_core_ir::verify_translated(&program).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    shown
}

/// 状態を渡す形では、展開した節の末尾の `resume k(v, s)` を `perform` の後ろへのジャンプにする。ジャンプの実引数には
/// box の挿入が変換を入れないので、v の Repr が `perform` の結果と互換でなければ handle を残す
/// (docs/spec/core-ir.md の「handler への特殊化」)。
#[test]
fn a_resume_value_of_another_repr_keeps_the_handle() {
    let shown = optimized(
        "\
effect Ask { ask(unit) -> tobj }
internal yields fn body(p.0: unit) -> int {
  let t.1: int = perform Ask.ask(())
  return t.1
}
internal fn ask(c.0: tobj, p.1: unit, k.2: tobj, s.3: unit) -> tobj {
  let t.4: tobj = resume k.2(c.0, ())
  return t.4
}
internal fn ret(x.0: tobj, s.1: unit) -> tobj {
  return x.0
}
fn entry$main() -> tobj {
  let s.0: obj = const \"x\"
  let c.1: tobj = closure ask(s.0)
  let t.2: tobj = handle Ask((), &body) { ask: c.1 } return &ret
  return t.2
}
",
    );
    assert!(shown.contains("handle "), "{shown}");
    assert!(!shown.contains("{H="), "{shown}");
}

/// `unit` を返す f が、組を返す経路の関数 g を末尾で呼ぶ。複製の結果の形が違うので、f の複製は g の複製を末尾でなく
/// 呼んで組を開き、次の状態を返す。
#[test]
fn a_tail_call_across_result_shapes_is_adapted() {
    let shown = optimized(
        "\
effect Count { tick(unit) -> unit }
internal yields fn body(p.0: unit) -> tobj {
  let t.1: unit = call f(3)
  return t.1
}
yields fn f(n.0: int) -> unit {
  switch n.0 { 0 -> b1, _ -> b2 }
b1:
  let t.1: tobj = call g(3)
  return t.1
b2:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  let t.3: unit = call f(m.2)
  return t.3
}
yields fn g(n.0: int) -> tobj {
  let t.1: unit = perform Count.tick(())
  switch n.0 { 0 -> b1, _ -> b2 }
b1:
  return ()
b2:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  let t.3: tobj = call g(m.2)
  return t.3
}
internal fn tick(p.0: unit, k.1: tobj, s.2: int) -> tobj {
  let n.3: int = extern \"Prelude.Num Int.+\"(s.2, 1)
  let t.4: tobj = resume k.1((), n.3)
  return t.4
}
internal fn ret(x.0: tobj, s.1: int) -> int {
  return s.1
}
fn entry$main() -> int {
  let t.0: int = handle Count(0, &body) { tick: &tick } return &ret
  return t.0
}
",
    );
    insta::assert_snapshot!(function(&shown, "\"f{H=ret}\""), @r#"
    fn "f{H=ret}"(n.0: int, s.4: int) -> int {
      switch n.0 { 0 -> b1, _ -> b2 }
    b1:
      let t.5: (,){tobj, int} = call "g{H=ret}"(3, s.4)
      unpack t.5 (,) #0(t.1: tobj, s.6: int)
      return s.6
    b2:
      let m.2: int = extern "Prelude.Num Int.-"(n.0, 1)
      let t.7: int = call "f{H=ret}"(m.2, s.4)
      return t.7
    }
    "#);
}

/// H に触れない `yields` の関数は経路に入れず、呼び出しをそのまま残す (docs/spec/core-ir.md の「handler への
/// 特殊化」)。`noise` は外側の `Std.State` だけを起こすので、`Tick` の複製を作らない。
#[test]
fn a_callee_that_does_not_touch_the_handler_is_not_copied() {
    let text = "\
import Std.State (State(..))

effect Tick where
  tick : Unit -> Unit

noise : Int -> <State Int, Panic> Unit
noise n =
  if n == 0 then ()
  else
    put (get () + n)
    noise (n - 1)

loop : Int -> <State Int, Tick, Panic> Unit
loop i =
  if i == 0 then ()
  else
    noise 3
    tick ()
    loop (i - 1)

count : (Unit -> <Tick | e> a) -> <Panic | e> (a, Int)
count action =
  handle action () from 0 with
    | fn tick () c -> ((), c + 1)
    | return x c -> (x, c)

main : Unit -> <IO, Panic> Unit
main () =
  let ((_, ticks), total) = State.run 0 (fn () -> count (fn () -> loop 10))
  println \"\\{total} \\{ticks}\"
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    let names: Vec<&str> = shown
        .lines()
        .filter(|line| line.contains("fn \""))
        .filter_map(|line| line.split('"').nth(1))
        .collect();
    // `Tick` の handler の `return` の節は `count` の複製の中にある
    let tick = |name: &str| name.contains("{H=count@");
    assert!(
        names
            .iter()
            .all(|name| !(name.starts_with("noise{") && tick(name))),
        "{shown}"
    );
    let looped = names
        .iter()
        .find(|name| name.starts_with("loop{") && tick(name))
        .unwrap_or_else(|| panic!("{shown}"));
    let code = function(&shown, looped);
    let callee = code
        .lines()
        .filter_map(|line| line.split("call \"").nth(1))
        .find(|callee| callee.starts_with("noise"))
        .unwrap_or_else(|| panic!("{code}"));
    assert!(!tick(callee), "{code}");
}

/// 状態つきの3つの handler を重ねたループ (`tests/ui/run/effects/state_passing_many_handlers.em` と同じプログラム)。
/// `step` は3つのエフェクトをどれも起こし、`loop` から末尾でなく呼ばれる。
const MANY_HANDLERS: &str =
    include_str!("../../../tests/ui/run/effects/state_passing_many_handlers.em");

/// 入れ子の handler の複製は、値の並びを1つのタプルで返す (docs/spec/core-ir.md の「handler の複製」)。`step` は
/// `Unit` を返すので、3回目の複製は3つの状態の `(,,)` を返し、組の中に組を入れない。
#[test]
fn nested_handlers_return_one_tuple() {
    let shown = core_text(MANY_HANDLERS, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    let copies: Vec<&str> = shown
        .lines()
        .filter(|line| nested_copy(line))
        .filter_map(|line| line.split('"').nth(1))
        .collect();
    let step = copies
        .iter()
        .find(|name| name.starts_with("step{"))
        .unwrap_or_else(|| panic!("{shown}"));
    let looped = copies
        .iter()
        .find(|name| name.starts_with("loop{"))
        .unwrap_or_else(|| panic!("{shown}"));
    let step = function(&shown, step);
    let looped = function(&shown, looped);
    for code in [&step, &looped] {
        assert!(!code.contains("con (,) "), "{shown}");
    }
    insta::assert_snapshot!(format!("{step}\n{looped}"), @r#"
    fn "step{H=run_c@#16721f776dc3b3e5}{H=run_b@#f19715733acc9a50}@#d06ed3ed0f858a82"(k.0: int, s.7: int, s.11: int, s.17: int) -> (,,){int, int, int} {
      let t.1: enum = extern "Prelude.Eq Int.=="(k.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.18: int = extern "Prelude.Num Int.+"(s.17, 1)
      let t.12: int = extern "Prelude.Num Int.+"(s.11, 1)
      let t.8: int = extern "Prelude.Num Int.+"(s.7, 1)
      let t.5: int = extern "Prelude.Num Int.-"(k.0, 1)
      let t.22: (,,){int, int, int} = call "step{H=run_c@#16721f776dc3b3e5}{H=run_b@#f19715733acc9a50}@#d06ed3ed0f858a82"(t.5, t.8, t.12, t.18)
      return t.22
    b2:
      let d.25: (,,){int, int, int} = con (,,) #0(s.7, s.11, s.17)
      return d.25
    }

    fn "loop{H=run_c@#16721f776dc3b3e5}{H=run_b@#f19715733acc9a50}@#e9f1b383dd022326"(i.0: int, s.5: int, s.8: int, s.13: int) -> (,,){int, int, int} {
      let t.1: enum = extern "Prelude.Eq Int.=="(i.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.14: (,,){int, int, int} = call "step{H=run_c@#16721f776dc3b3e5}{H=run_b@#f19715733acc9a50}@#d06ed3ed0f858a82"(1, s.5, s.8, s.13)
      unpack t.14 (,,) #0(t.15: int, t.16: int, s.17: int)
      let t.3: int = extern "Prelude.Num Int.-"(i.0, 1)
      let t.18: (,,){int, int, int} = call "loop{H=run_c@#16721f776dc3b3e5}{H=run_b@#f19715733acc9a50}@#e9f1b383dd022326"(t.3, t.15, t.16, s.17)
      return t.18
    b2:
      let d.21: (,,){int, int, int} = con (,,) #0(s.5, s.8, s.13)
      return d.21
    }
    "#);
}

/// 経路の関数の中の `try_panic` は handle を持つので、H に触れる関数として経路に入り、外側の状態つきの handler を
/// 特殊化しない (docs/spec/core-ir.md の「handler への特殊化」)。
#[test]
fn a_try_panic_on_the_path_still_keeps_the_handle() {
    let text = "\
effect Tick where
  tick : Unit -> Unit

guarded : Unit -> <Tick> Unit
guarded () =
  let _ = try_panic (fn () -> tick ())
  ()

loop : Int -> <Tick, Panic> Unit
loop i =
  if i == 0 then ()
  else
    guarded ()
    loop (i - 1)

count : (Unit -> <Tick | e> a) -> <Panic | e> (a, Int)
count action =
  handle action () from 0 with
    | fn tick () c -> ((), c + 1)
    | return x c -> (x, c)

main : Unit -> <IO, Panic> Unit
main () =
  let (_, ticks) = count (fn () -> loop 3)
  println (show ticks)
";
    let shown = core_text(text, Pass::Optimize);
    assert!(shown.contains("handle Tick("), "{shown}");
    assert!(!shown.contains("{H="), "{shown}");
}

/// `var` を手で脱糖したふるい。`Marks` は `MutArray Bool` を、`J` は `Int` を状態に持つ。節は `var` の脱糖と同じ形で、
/// 範囲は `length` と `panic` で確かめる。`for` と `while` の本体は既知の閉包として直接の呼び出しになって経路に入り、
/// 内側の `J` から順に、どちらの handle も消える。
#[test]
fn while_and_for_bodies_become_known_closures() {
    let text = "\
effect Marks where
  take : Unit -> MutArray Bool
  length : Unit -> Int
  read : Int -> Bool
  write : Int -> Bool -> Unit

effect J where
  get : Unit -> Int
  set : Int -> Unit

checked : Int -> <Marks, Panic> Unit
checked i =
  let n = length ()
  if i < 0 || i >= n then panic \"index out of range\" else ()

cross : Int -> Int -> <Marks, Panic> Unit
cross i n =
  handle while (fn () -> get () < n) (fn () ->
      let j = get ()
      checked j
      write j False
      set (j + i)) from i * i with
    | fn get () s -> (s, s)
    | fn set v _ -> ((), v)
    | return x _ -> x

count_primes : Array Bool -> Int -> Int -> Int -> <Panic> Int
count_primes marks i n acc =
  if i >= n then acc
  else count_primes marks (i + 1) n (if Array.get marks i then acc + 1 else acc)

sieve : Int -> <Marks, Panic> Int
sieve n =
  for 2 n (fn i ->
    if i * i < n then
      checked i
      if read i then cross i n else ()
    else ())
  count_primes (MutArray.freeze (take ())) 2 n 0

main : Unit -> <IO, Panic> Unit
main () =
  let n = 100
  let primes =
    handle sieve n from MutArray.make n True with
      | fn take () m -> (m, MutArray.empty ())
      | fn length () m -> MutArray.length m
      | fn read i m -> MutArray.read m i
      | fn write i v m -> ((), MutArray.write m i v)
      | return x m ->
        let _ = MutArray.freeze m
        x
  println (show primes)
";
    let shown = core_text(text, Pass::Optimize);
    assert!(
        shown
            .lines()
            .all(|line| !line.contains("handle ") && !line.contains("perform ")),
        "{shown}"
    );
}

/// `for` と `while` で書いた `var` のふるいは、変数の handler がすべて状態を渡す形に特殊化され、`handle` も
/// `perform` も残らない (docs/spec/core-ir.md の「`var` の変換」)。
#[test]
fn a_var_loop_is_specialized() {
    let shown = core_text(
        include_str!("../../../tests/ui/run/var/sieve.em"),
        Pass::Optimize,
    );
    assert!(!shown.contains("handle "), "{shown}");
    assert!(!shown.contains("perform "), "{shown}");
}

/// 入れ子の `var` は内側から特殊化する。内側の `j` の複製を作った後の回で、外側の `marks` の複製の経路はその複製を
/// 通る。`while` の複製は `j` と `marks` の両方の状態を受ける。名前は 64 文字を超えるので構造の hash になり、
/// 見出しの引数が入れ子を示す (docs/spec/core-ir.md の「入れ子の handler」)。
#[test]
fn nested_vars_are_specialized_inside_out() {
    let shown = core_text(
        include_str!("../../../tests/ui/run/var/sieve.em"),
        Pass::Optimize,
    );
    let copies: Vec<&str> = shown
        .lines()
        .filter(|line| line.starts_with("fn \""))
        .collect();
    insta::assert_snapshot!(copies.join("\n"), @r#"
    fn "Prelude.for<e>{2=count$lambda0/1}{H=count$var0$return}"(lo.0: int, hi.1: int, marks.7: obj, st.8: int) -> int {
    fn "Prelude.for<e>{2=sieve$lambda2/1}{H=sieve$var0$return}"(lo.0: int, hi.1: int, n.7: int, st.8: obj lin) -> obj lin {
    fn "Prelude.while<e>{0=sieve$lambda0/1, 1=sieve$lambda1/1}@#55b5caa901e2f263"(n.5: int, i.6: int, st.7: int, st.12: obj lin) -> (,){int, obj lin} lin {
    "#);
}

/// `unpack` と `jump` だけの合流のブロック M のフィールドを、M の先で使う。`resolve_known` が内側の `con` を開いて
/// ブロックを分けると、この形の M ができる (状態を渡す複製の組を開いて元の組を作り直す所がそうなる)。M を飛ばして
/// 先へ直接つなぐと `z.7` が定義されないので、M を分ける作り方をとる。1本目の `jump` が M を分け、同じ回の
/// 2本目の `jump` は分けた M を直接つなぐ (docs/spec/core-ir.md の「既知のコンストラクタ」)。
#[test]
fn threading_an_unpack_keeps_fields_used_past_the_join() {
    let shown = optimized(
        "layout Prelude.Bool { False, True }
layout Triple { T(int, int, int) }
layout Pair { P(int, int) }
fn entry$main(a.0: enum, n.1: int) -> int {
  switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let t.2: obj = con Triple #0(n.1, 2, 3)
  jump b3(t.2)
b2:
  let t.3: obj = con Triple #0(4, n.1, 6)
  jump b3(t.3)
b3(p.4: obj):
  unpack p.4 Triple #0(x.5: int, y.6: int, z.7: int)
  let q.8: obj = con Pair #0(x.5, y.6)
  unpack q.8 Pair #0(u.9: int, v.10: int)
  let s.11: int = extern \"Prelude.Num Int.+\"(u.9, z.7)
  let s.12: int = extern \"Prelude.Num Int.+\"(s.11, v.10)
  return s.12
}
",
    );
    insta::assert_snapshot!(shown, @r#"
    layout Prelude.Bool { False, True }
    layout Triple { T(int, int, int) }
    layout Pair { P(int, int) }
    fn entry$main(a.0: enum, n.1: int) -> int {
      switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(n.1, 2, 3)
    b2:
      jump b3(4, n.1, 6)
    b3(x.5: int, y.6: int, z.7: int):
      let s.11: int = extern "Prelude.Num Int.+"(x.5, z.7)
      let s.12: int = extern "Prelude.Num Int.+"(s.11, y.6)
      return s.12
    }
    "#);
}

/// `switch` で選ばれる行き先 C が `jump` だけになっても、C のフィールドを C の先で使うなら、C を飛ばさずに分ける。
#[test]
fn threading_a_switch_keeps_fields_used_past_the_case() {
    let shown = optimized(
        "layout Prelude.Bool { False, True }
layout Opt { None, Some(int, int, int) }
layout Pair { P(int, int) }
fn entry$main(a.0: enum, n.1: int) -> int {
  switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let t.2: obj = con Opt #1(n.1, 2, 3)
  jump b3(t.2)
b2:
  let t.3: obj = con Opt #1(4, n.1, 6)
  jump b3(t.3)
b3(p.4: obj):
  switch p.4 Opt { #0 -> b4, #1(x.5: int, y.6: int, z.7: int) -> b5 }
b4:
  return 0
b5:
  let q.8: obj = con Pair #0(x.5, y.6)
  unpack q.8 Pair #0(u.9: int, v.10: int)
  let s.11: int = extern \"Prelude.Num Int.+\"(u.9, z.7)
  let s.12: int = extern \"Prelude.Num Int.+\"(s.11, v.10)
  return s.12
}
",
    );
    insta::assert_snapshot!(shown, @r#"
    layout Prelude.Bool { False, True }
    layout Opt { None, Some(int, int, int) }
    layout Pair { P(int, int) }
    fn entry$main(a.0: enum, n.1: int) -> int {
      switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(n.1, 2, 3)
    b2:
      jump b3(4, n.1, 6)
    b3(x.5: int, y.6: int, z.7: int):
      let s.11: int = extern "Prelude.Num Int.+"(x.5, z.7)
      let s.12: int = extern "Prelude.Num Int.+"(s.11, y.6)
      return s.12
    }
    "#);
}

/// handler への特殊化が作る関数 (`f{H}` と、節の写しの複製 `<g>{k}{…}`) は `split` を持たない。直接呼ばれるか、
/// 特殊化した経路の中で型どおりに呼ばれるためである。複製は `k` を `apply` で呼ばず、継続の関数を直接呼ぶ
/// (docs/spec/core-ir.md の「節の写しの、継続についての複製」)。
#[test]
fn handler_specialization_functions_are_typed() {
    let program =
        eml_test_support::core_until(include_str!("../../../bench/state_loop.em"), Pass::Optimize);
    let shown = pretty(&program);
    let copies: Vec<&eml_core_ir::CoreFn> = program
        .functions
        .iter()
        .filter(|function| function.name.contains("{H="))
        .collect();
    assert!(!copies.is_empty(), "{shown}");
    assert!(
        copies.iter().all(|function| function.split.is_none()),
        "{shown}"
    );

    let program = eml_test_support::core_until(CLAUSE_WITH_K, Pass::Optimize);
    let shown = pretty(&program);
    let copy = program
        .functions
        .iter()
        .find(|function| function.name.contains("{k}{"))
        .unwrap_or_else(|| panic!("no `<g>{{k}}{{…}}` in\n{shown}"));
    assert_eq!(copy.split, None, "{shown}");
    let calls = |continued: bool| {
        copy.blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter(|stmt| match stmt {
                Stmt::Let {
                    rhs:
                        Rhs::Call {
                            call: Call::Apply { .. },
                            ..
                        },
                    ..
                } => !continued,
                Stmt::Let {
                    rhs:
                        Rhs::Call {
                            call: Call::Direct(target, _),
                            ..
                        },
                    ..
                } => continued && program.function(*target).name.ends_with("$h0"),
                _ => false,
            })
            .count()
    };
    assert_eq!(calls(false), 0, "{shown}");
    assert!(calls(true) > 0, "{shown}");
}

/// 節の写しの複製 `<g>{k}{…}` が展開されずに残るプログラム。`state_loop` の節はすぐに再開するので写しを作らない。この節は `k` を
/// 末尾でなく呼ぶので写しになり、本体が展開の上限より大きいので複製に展開されない。
const CLAUSE_WITH_K: &str = "\
effect Ask where
  ask : Unit -> Int

sum : Int -> Int -> <Ask, Panic> Int
sum acc n = if n <= 0 then acc else sum (acc + ask ()) (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle sum 0 3 with
    | ctl ask () k ->
      let v = k 10
      v * 1 + v * 2 + v * 3 + v * 4 + v * 5 + v * 6 + v * 7 + v * 8 + v * 9 + v * 10 + v * 11 + v * 12 + v * 13 + v * 14 + v * 15 + v * 16 + v * 17 + v * 18 + v * 19 + v * 20 + v * 21 + v * 22 + v * 23 + v * 24 + v * 25 + v * 26 + v * 27 + v * 28 + v * 29
  println (show r)
";

/// 節の写しは、`perform` の地点ごとに、そこで渡す継続の関数について複製される。複製は `k` を継続の関数の捕獲の
/// 引数に開き、`k` への `apply` を継続の関数の直接の呼び出しにする。本体に `handle` のない節では、閉包を受ける写し `<g>{k}` は残らない
/// (docs/spec/core-ir.md の「節の写しの、継続についての複製」)。
#[test]
fn a_clause_copy_is_copied_for_each_continuation() {
    let program = eml_test_support::core_until(TWO_ASKS, Pass::Optimize);
    let shown = pretty(&program);
    let copies: Vec<&eml_core_ir::CoreFn> = program
        .functions
        .iter()
        .filter(|function| function.name.contains("{k}"))
        .collect();
    let mut names: Vec<&str> = copies.iter().map(|copy| copy.name.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "main$handle0$ask{k}{1=sum{H=main$handle0$return}$h0/2}",
            "main$handle0$ask{k}{1=sum{H=main$handle0$return}$h1/3}",
        ],
        "{shown}"
    );
    let copy = function(&shown, names[1]);
    let head: Vec<&str> = copy.lines().take(2).collect();
    insta::assert_snapshot!(head.join("\n"), @r#"
    fn "main$handle0$ask{k}{1=sum{H=main$handle0$return}$h1/3}"(p.0: unit, acc.61: int, n.62: int, t.63: int, p.2: unit) -> int {
      let t.3: int = call "sum{H=main$handle0$return}$h1"(acc.61, n.62, t.63, 10)
    "#);
    assert!(!copy.contains("apply "), "{copy}");
}

/// 節の写しの複製が直接呼ぶだけの継続の関数は、値の参照が残らないので `split` を持たない。`k` を値として使う節
/// (`Iter.generate`) の継続の関数は、複製が閉包を作り直すので `split` を保つ
/// (docs/spec/core-ir.md の「直接だけ呼ばれる持ち上げた関数」)。
#[test]
fn a_continuation_called_only_by_its_clause_copy_is_typed() {
    let continuations = |text: &str| {
        let program = eml_test_support::core_until(text, Pass::Optimize);
        let shown = pretty(&program);
        let found: Vec<Option<u32>> = program
            .functions
            .iter()
            .filter(|function| function.name.contains("{H=") && function.name.contains("}$h"))
            .filter(|function| !function.name.contains("}$hj"))
            .map(|function| function.split)
            .collect();
        assert!(!found.is_empty(), "{shown}");
        found
    };
    assert!(continuations(TWO_ASKS).iter().all(Option::is_none));
    let generated = format!(
        "{NUMS}{HIDE}\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box it = hide 0 (Iter.generate (nums 3))\n  println (show (Iter.to_list it))\n"
    );
    assert!(continuations(&generated).iter().all(Option::is_some));
}

/// `perform` の地点が2つある経路の関数。節は `k` を末尾でなく呼ぶので写しになり、展開の上限より大きいので複製に
/// 展開されない。
const TWO_ASKS: &str = "\
effect Ask where
  ask : Unit -> Int

sum : Int -> Int -> <Ask, Panic> Int
sum acc n =
  if n <= 0 then acc
  else
    let a = ask ()
    let b = ask ()
    sum (acc + a * b) (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle sum 0 3 with
    | ctl ask () k ->
      let v = k 10
      v * 1 + v * 2 + v * 3 + v * 4 + v * 5 + v * 6 + v * 7 + v * 8 + v * 9 + v * 10 + v * 11 + v * 12 + v * 13 + v * 14 + v * 15 + v * 16 + v * 17 + v * 18 + v * 19 + v * 20 + v * 21 + v * 22 + v * 23 + v * 24 + v * 25 + v * 26 + v * 27 + v * 28 + v * 29
  println (show r)
";

/// `k` を捨てる節の複製は、閉包を作り直して捨てるのでなく、`unr` でない捕獲だけを捨てる。ここでは捕獲の `File` を
/// 捨て、複製と継続の関数は小さいので呼ぶ側に展開される (docs/spec/core-ir.md の「節の写しの、継続についての複製」)。
#[test]
fn a_clause_copy_drops_the_captures_instead_of_k() {
    let shown = core_text(DROPPED_K, Pass::Optimize);
    insta::assert_snapshot!(function(&shown, "entry$main"), @r#"
    fn entry$main() -> unit {
      let s.1: obj = const "input.txt"
      let t.2: obj lin = extern Std.Fs.open(s.1)
      let t.3: unit = drop t.2
      let s.4: obj = const "dropped"
      let t.5: unit = extern Prelude.println(s.4)
      return t.5
    }
    "#);
}

/// `once` の操作の節で、`File` を生きている変数に持つ経路の関数。節は `k` を捨てる。
const DROPPED_K: &str = "\
effect Ask where
  ask : Unit -> Int

read_after : Fs.File -> <Ask, IO> Unit
read_after f =
  let n = ask ()
  Fs.close f

main : Unit -> <IO> Unit
main () =
  let f = Fs.open \"input.txt\"
  handle read_after f with
    | ctl ask () k ->
        drop k
        println \"dropped\"
";

/// 本体に `handle` を持つ節の写しは、継続について複製しない。複製すると中の `handle` が写り、節の関数がちょうど1つの
/// `handle` に属するという EP の前提に反するためである (docs/spec/core-ir.md の「節の写しの、継続についての複製」)。
#[test]
fn a_clause_copy_with_a_handle_is_not_copied_for_its_continuation() {
    let program = eml_test_support::core_until(HANDLE_IN_CLAUSE, Pass::Optimize);
    let shown = pretty(&program);
    assert!(
        program
            .functions
            .iter()
            .all(|function| !function.name.contains("{k}{")),
        "{shown}"
    );
    assert!(shown.contains("{k}\"("), "{shown}");
}

/// 節の本体に `handle` がある handler。節の本体は展開の上限より大きいので、写しが残る。
const HANDLE_IN_CLAUSE: &str = "\
effect Ask where
  ask : Unit -> Int

effect Tell where
  tell : Int -> Unit

sum : Int -> Int -> <Ask, Panic> Int
sum acc n = if n <= 0 then acc else sum (acc + ask ()) (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle sum 0 3 with
    | ctl ask () k ->
      let v = k 10
      let w = handle quiet () with | fn tell x -> ()
      w + v * 1 + v * 2 + v * 3 + v * 4 + v * 5 + v * 6 + v * 7 + v * 8 + v * 9 + v * 10 + v * 11 + v * 12 + v * 13 + v * 14 + v * 15 + v * 16 + v * 17 + v * 18 + v * 19 + v * 20 + v * 21 + v * 22 + v * 23 + v * 24 + v * 25 + v * 26 + v * 27 + v * 28 + v * 29
  println (show r)

quiet : Unit -> <Tell> Int
quiet () =
  tell 1
  2
";

/// `con` の複製は、開いた位置より後ろの一様な引数だけを一様に保ち、結果は一様なまま写す。一様な引数の位置を開く `r` の
/// 複製は一様な引数が残らない。一様でない位置だけを開く `g` の複製は、末尾の一様な引数がそのまま残るので `split` を
/// 引数の増減だけずらす (docs/spec/core-ir.md の「`con` の値の形についての特殊化」)。
#[test]
fn a_con_copy_keeps_the_uniform_parameters_after_the_opened_ones() {
    let mut program = parse(
        "\
layout Box { Box(tobj, int) }
fn inc(x.0: int) -> int {
  return x.0
}
internal uniform from 0 fn r(x.0: int, s.1: tobj) -> int {
  unpack s.1 Box #0(f.2: tobj, n.5: int)
  let y.3: int = apply f.2(n.5)
  let z.4: int = call r(y.3, s.1)
  return z.4
}
internal uniform from 1 fn g(b.0: tobj, k.1: tobj, s.2: unit) -> int {
  unpack b.0 Box #0(f.3: tobj, n.6: int)
  let y.4: int = apply f.3(n.6)
  let z.5: int = call g(b.0, k.1, s.2)
  return z.5
}
fn entry$main(k.0: tobj) -> int {
  let b.1: tobj = con Box #0(&inc, 5)
  let t.2: int = call r(1, b.1)
  let c.3: tobj = con Box #0(&inc, 6)
  let t.4: int = call g(c.3, k.0, ())
  return t.4
}
",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    optimize(&mut program);
    let shown = pretty(&program);
    let uniform_tail = |prefix: &str| {
        let copy = program
            .functions
            .iter()
            .find(|function| function.name.starts_with(prefix))
            .unwrap_or_else(|| panic!("no `{prefix}` copy in\n{shown}"));
        copy.split.map(|at| copy.params().len() - at as usize)
    };
    assert_eq!(uniform_tail("r{"), Some(0), "{shown}");
    assert_eq!(uniform_tail("g{"), Some(2), "{shown}");
}

/// 状態を渡す形の複製が2つ以上の値を返すとき、結果は値の Repr と印から作るタプルの形の構造体である。末尾でない
/// 呼び出しは構造体を受けて `unpack` し、`return` の節の結果の組も構造体の `con` で作る
/// (docs/spec/core-ir.md の「handler の複製」)。
#[test]
fn passing_returns_a_tuple_struct() {
    let text = "\
import Std.State (State(..))

count : Int -> <State Int, Panic> Int
count n = if n == 0 then 0 else
  put (get () + 1)
  let r = count (n - 1)
  r + 2

main : Unit -> <IO, Panic> Unit
main () = println (show (State.exec 0 (fn () -> count 5)))
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    insta::assert_snapshot!(handler_copies(&shown), @r#"
    fn "count{H=Std.State.run@#6d0e8cc9f0bb1f6e}"(n.0: int, st.8: int) -> (,){int, int} {
      let t.1: enum = extern "Prelude.Eq Int.=="(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern "Prelude.Num Int.+"(st.8, 1)
      let t.5: int = extern "Prelude.Num Int.-"(n.0, 1)
      let t.11: (,){int, int} = call "count{H=Std.State.run@#6d0e8cc9f0bb1f6e}"(t.5, t.3)
      unpack t.11 (,) #0(t.6: int, st.12: int)
      let t.7: int = extern "Prelude.Num Int.+"(t.6, 2)
      let d.13: (,){int, int} = con (,) #0(t.7, st.12)
      return d.13
    b2:
      let d.14: (,){int, int} = con (,) #0(0, st.8)
      return d.14
    }
    "#);
}

/// 開いた後の語の数が 6 を超える並びは、構造体にせず今の `obj` のタプルで返す
/// (docs/spec/core-ir.md の「構造体にする型」)。
#[test]
fn a_returned_tuple_over_six_words_stays_obj() {
    let text = "\
import Std.State (State(..))

data Six =
  | Six Float Float Float Float Float Float

count : Int -> <State Six, Panic> Int
count n = if n == 0 then 0 else
  let Six a b c d e f = get ()
  put (Six (a + 1.0) b c d e f)
  count (n - 1) + 1

main : Unit -> <IO, Panic> Unit
main () =
  let Six a _ _ _ _ _ = State.exec (Six 0.0 0.0 0.0 0.0 0.0 0.0) (fn () -> count 5)
  println (show a)
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    let header = shown
        .lines()
        .find(|line| line.starts_with("fn \"count{H="))
        .unwrap_or_else(|| panic!("{shown}"));
    assert!(header.ends_with("-> obj {"), "{shown}");
}

/// 構造体の型の `var` の状態は、状態を渡す形の複製でも構造体のまま持ち回る。2つの `var` の状態を返す複製の組は、
/// 入れ子の構造体を葉に持つ構造体になる (docs/spec/core-ir.md の「handler の複製」の
/// optimize)。
#[test]
fn a_struct_var_state_stays_a_struct() {
    let text = "\
data Vec3 =
  | Vec3 Float Float Float

add : Vec3 -> Vec3 -> <Panic> Vec3
add (Vec3 a b c) (Vec3 x y z) = Vec3 (a + x) (b + y) (c + z)

walk : Int -> <Panic> Float
walk n =
  var p = Vec3 0.0 0.0 0.0
  var steps = 0
  for 0 n (fn i ->
    p <- add p (Vec3 1.0 2.0 3.0)
    steps <- steps + 1)
  let Vec3 x y z = p
  x + y + z + Float.of_int steps

main : Unit -> <IO, Panic> Unit
main () = println (show (walk 4))
";
    let shown = core_text(text, Pass::Optimize);
    assert!(!shown.contains("handle "), "{shown}");
    let headers: Vec<&str> = shown
        .lines()
        .filter(|line| line.starts_with("fn \"") && line.contains("{H="))
        .collect();
    insta::assert_snapshot!(headers.join("\n"), @r#"fn "Prelude.for<e>{2=walk$lambda0}{H=walk$var1$return}@#168ca81853eb6170"(lo.0: int, hi.1: int, st.7: int, st.10: Vec3{float, float, float}) -> (,){int, Vec3{float, float, float}} {"#);
}

/// 組を構造体で返す複製で、`return` の値が並びの Repr と互換なだけ (`tobj` の結果の `()`) のときは、値と状態を並びの
/// Repr の引数を持つブロックへジャンプで渡し、そこで構造体の `con` を作る。構造体の `con` の引数は正確な位置で、
/// ジャンプの実引数は命令なしで収まる互換の位置だからである
/// (docs/spec/core-ir.md の「handler の複製」)。
#[test]
fn a_compatible_return_value_reaches_the_tuple_through_a_block() {
    let shown = optimized(
        "\
effect Count { tick(unit) -> unit }
internal yields fn body(p.0: unit) -> tobj {
  let t.1: tobj = call g(3)
  return t.1
}
yields fn g(n.0: int) -> tobj {
  let t.1: unit = perform Count.tick(())
  switch n.0 { 0 -> b1, _ -> b2 }
b1:
  return ()
b2:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  let t.3: tobj = call g(m.2)
  return t.3
}
internal fn tick(p.0: unit, k.1: tobj, s.2: int) -> tobj {
  let n.3: int = extern \"Prelude.Num Int.+\"(s.2, 1)
  let t.4: tobj = resume k.1((), n.3)
  return t.4
}
internal fn ret(x.0: tobj, s.1: int) -> int {
  return s.1
}
fn entry$main() -> int {
  let t.0: int = handle Count(0, &body) { tick: &tick } return &ret
  return t.0
}
",
    );
    insta::assert_snapshot!(function(&shown, "\"g{H=ret}\""), @r#"
    fn "g{H=ret}"(n.0: int, s.4: int) -> (,){tobj, int} {
      let n.5: int = extern "Prelude.Num Int.+"(s.4, 1)
      switch n.0 { 0 -> b1, _ -> b2 }
    b1:
      jump b3((), n.5)
    b2:
      let m.2: int = extern "Prelude.Num Int.-"(n.0, 1)
      let t.10: (,){tobj, int} = call "g{H=ret}"(m.2, n.5)
      return t.10
    b3(t.7: tobj, s.8: int):
      let d.9: (,){tobj, int} = con (,) #0(t.7, s.8)
      return d.9
    }
    "#);
}

/// 既知の閉包をフィールドに持つ構造体の `con` を渡す呼び出しの複製は、値のフィールドを形の Repr (`int`) で受ける。
/// 複製の入口で作り直す構造体の `con` の引数は、形と比べる正確な位置だからである。配置の Repr (`tobj`) で受けると、
/// 作り直す `con` が verifier に断られる
/// (docs/spec/core-ir.md の「handler の複製」)。
#[test]
fn a_con_copy_takes_struct_fields_in_their_shape_reprs() {
    let text = "\
apply_n : (Int -> <Panic> Int, Int) -> Int -> <Panic> Int
apply_n (f, x) n = if n == 0 then x else apply_n (f, f x) (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let k = 3
  println (show (apply_n ((fn y -> y + k), 1) 10))
";
    let shown = core_text(text, Pass::Optimize);
    let header = shown
        .lines()
        .find(|line| line.starts_with("fn \"apply_n{"))
        .unwrap_or_else(|| panic!("{shown}"));
    insta::assert_snapshot!(header, @r#"fn "apply_n{0=(,) #0(main$lambda0/1, _)}"(k.9: int, p.11: int, n.1: int) -> int {"#);
}

/// 状態つきの handler の複製で、末尾の呼び出しの結果をそのまま返してよいのは、呼ぶ複製の並びが f の並びと同じときだけ
/// である。タプルの形は値の Repr で決まるので、大きさが同じ並びでも形が違う。`obj` を返す f が `tobj` を返す g を末尾で
/// 呼ぶと、f の複製は g の組を受け直して開き、f の並びの組を作り直して返す
/// (docs/spec/core-ir.md の「handler の複製」)。
#[test]
fn a_tail_call_to_a_copy_with_another_tuple_shape_is_received() {
    let shown = optimized(
        "\
effect Count { tick(unit) -> unit }
internal yields fn body(p.0: unit) -> tobj {
  let t.1: obj = call f(3)
  return t.1
}
yields fn f(n.0: int) -> obj {
  switch n.0 { 0 -> b1, _ -> b2 }
b1:
  let t.1: tobj = call g(3)
  return t.1
b2:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  let t.3: obj = call f(m.2)
  return t.3
}
yields fn g(n.0: int) -> tobj {
  let t.1: unit = perform Count.tick(())
  switch n.0 { 0 -> b1, _ -> b2 }
b1:
  let s.4: obj = const \"done\"
  return s.4
b2:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  let t.3: tobj = call g(m.2)
  return t.3
}
internal fn tick(p.0: unit, k.1: tobj, s.2: int) -> tobj {
  let n.3: int = extern \"Prelude.Num Int.+\"(s.2, 1)
  let t.4: tobj = resume k.1((), n.3)
  return t.4
}
internal fn ret(x.0: tobj, s.1: int) -> int {
  let u.2: unit = drop x.0
  return s.1
}
fn entry$main() -> int {
  let t.0: int = handle Count(0, &body) { tick: &tick } return &ret
  return t.0
}
",
    );
    insta::assert_snapshot!(function(&shown, "\"f{H=ret}\""), @r#"
    fn "f{H=ret}"(n.0: int, s.4: int) -> (,){obj, int} {
      switch n.0 { 0 -> b1, _ -> b2 }
    b1:
      let t.5: (,){tobj, int} = call "g{H=ret}"(3, s.4)
      unpack t.5 (,) #0(t.1: tobj, s.6: int)
      jump b3(t.1, s.6)
    b2:
      let m.2: int = extern "Prelude.Num Int.-"(n.0, 1)
      let t.10: (,){obj, int} = call "f{H=ret}"(m.2, s.4)
      return t.10
    b3(t.7: obj, s.8: int):
      let d.9: (,){obj, int} = con (,) #0(t.7, s.8)
      return d.9
    }
    "#);
}

/// `unr` の値を `lin` の受け手に渡す展開は通し、`lin` の値を `unr` の受け手に渡す展開は拒む
/// (docs/spec/lir.md の「Kind の印」)。結果の印だけを `poly` にした呼ばれる側で、受け手の印を変える。
#[test]
fn inlining_passes_unr_to_lin_but_not_lin_to_unr() {
    let callee = "fn g(a.0: obj) -> obj poly {\n  return a.0\n}\n";
    let passing = optimized(&format!(
        "{callee}fn entry$main(s.0: obj) -> obj lin {{\n  let r.1: obj lin = call g(s.0)\n  return r.1\n}}\n"
    ));
    assert!(!passing.contains("call g("), "{passing}");
    let callee = "fn g(a.0: obj lin) -> obj poly {\n  return a.0\n}\n";
    let rejected = optimized(&format!(
        "{callee}fn entry$main(s.0: obj lin) -> obj {{\n  let r.1: obj = call g(s.0)\n  return r.1\n}}\n"
    ));
    assert!(rejected.contains("call g("), "{rejected}");
}

/// 既知の `con` を開くとき、`unr` のフィールドを `lin` の受け手に渡すのは通し、逆は通さない。
#[test]
fn opening_a_known_con_passes_unr_to_lin_but_not_lin_to_unr() {
    let passing = optimized(
        "layout Box { B(obj) }
fn entry$main(a.0: obj) -> obj lin {
  let p.1: obj = con Box #0(a.0)
  unpack p.1 Box #0(x.2: obj lin)
  return x.2
}
",
    );
    assert!(!passing.contains("con Box"), "{passing}");
    let rejected = optimized(
        "layout Box { B(obj) }
fn entry$main(a.0: obj lin) -> obj {
  let p.1: obj lin = con Box #0(a.0)
  unpack p.1 Box #0(x.2: obj)
  return x.2
}
",
    );
    assert!(rejected.contains("con Box"), "{rejected}");
}

/// 既知の閉包を開いた複製は、開いた閉包の捕獲の数だけ引数が増えるが、後ろの一様な引数の数は変わらない
/// (docs/spec/core-ir.md の「鍵と名前」)。
#[test]
fn a_known_closure_copy_keeps_its_uniform_tail() {
    let mut program = parse(
        "\
internal fn h(a.0: int, b.1: int, x.2: int) -> int {
  let t.3: int = extern \"Prelude.Num Int.+\"(a.0, b.1)
  let t.4: int = extern \"Prelude.Num Int.+\"(t.3, x.2)
  return t.4
}
internal uniform from 1 fn r(f.0: tobj, s.1: tobj) -> tobj {
  let t.2: int = apply f.0(1)
  switch t.2 { 0 -> b1, _ -> b2 }
b1:
  return s.1
b2:
  let t.3: tobj = call r(f.0, s.1)
  return t.3
}
fn entry$main() -> tobj {
  let c.0: tobj = closure h(3, 4)
  let t.1: tobj = call r(c.0, ())
  return t.1
}
",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    optimize(&mut program);
    let shown = pretty(&program);
    let copy = program
        .functions
        .iter()
        .find(|function| function.name.starts_with("r{"))
        .unwrap_or_else(|| panic!("no `r` copy in\n{shown}"));
    assert_eq!(copy.params().len(), 3, "{shown}");
    assert_eq!(copy.split, Some(2), "{shown}");
}

/// 持ち上げた関数の ABI は、持ち主の一族のコードだけで決まる。`user` のラムダは `keep` の複製が値として持つだけで、
/// `user` の中では値にならないので型どおりになる。`user` を直接呼ぶプログラムと、値として使うプログラムで、ラムダの
/// `split` もコードも同じである。名前が同じならコードも同じという追記の規則を、この決め方も守る
/// (docs/spec/core-ir.md の「直接だけ呼ばれる持ち上げた関数」)。
#[test]
fn a_lifted_function_gets_the_same_abi_in_every_program() {
    let common = format!(
        "{HIDE}
keep : Int -> (Int -> <Panic> Int) -> <Panic> Box (Int -> <Panic> Int)
keep n f = if n <= 0 then Box f else keep (n - 1) f

user : Int -> <Panic> Box (Int -> <Panic> Int)
user n = keep 0 (fn x -> x + n)
"
    );
    let direct = format!(
        "{common}
main : Unit -> <IO, Panic> Unit
main () = match user 3 with
  | Box g -> println (show (g 4))
"
    );
    let value = format!(
        "{common}
main : Unit -> <IO, Panic> Unit
main () = match hide 0 user with
  | Box u -> match u 3 with
    | Box g -> println (show (g 4))
"
    );
    let lambda = |text: &str| {
        let program = eml_test_support::core_until(text, Pass::Optimize);
        let lambda = program
            .functions
            .iter()
            .find(|function| function.name == "user$lambda0")
            .unwrap_or_else(|| panic!("no `user$lambda0` in\n{}", pretty(&program)));
        (lambda.split, function(&pretty(&program), "user$lambda0"))
    };
    let (split, code) = lambda(&direct);
    assert_eq!(split, None, "{code}");
    assert_eq!(lambda(&value), (split, code));
}

/// 包む関数の名前は `$` で始まるので、利用者の関数 `con` は `$con$…` の持ち主にならず、`split` は外れない
/// (docs/spec/core-ir.md の「直接だけ呼ばれる持ち上げた関数」)。
#[test]
fn a_user_function_named_con_does_not_own_a_constructor_wrapper() {
    let program = eml_test_support::core_until(
        &format!(
            "{HIDE}
con : Int -> Int
con x = x

main : Unit -> <IO, Panic> Unit
main () = match hide 0 Some with
  | Box g -> println (show (g (con 4)))
"
        ),
        Pass::Optimize,
    );
    let wrapper = program
        .functions
        .iter()
        .find(|function| function.name == "$con$Prelude.Some@[Int]")
        .unwrap_or_else(|| panic!("no constructor wrapper in\n{}", pretty(&program)));
    assert_eq!(wrapper.split, Some(0));
}

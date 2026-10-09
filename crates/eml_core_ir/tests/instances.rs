//! 単相化の instance (docs/spec/core-ir.md の「変換の規則」)。どの (関数, 型引数) が instance になり、どう名付けるかを
//! 確かめる。

use crate::common::{core_text, function};
use eml_core_ir::Pass;

/// `Pass::Translate` までの Core IR の、内部の印のない関数のうち、`name@` で始まる名前を並べる。
fn instances_of(text: &str, name: &str) -> Vec<String> {
    let program = eml_test_support::core_until(text, Pass::Translate);
    let prefix = format!("{name}@");
    program
        .functions
        .iter()
        .filter(|function| !function.internal && function.name.starts_with(&prefix))
        .map(|function| function.name.clone())
        .collect()
}

#[test]
fn a_chain_used_at_two_types_has_two_instances_per_function() {
    let n = 20;
    let mut text = String::new();
    for i in 1..n {
        text.push_str(&format!("f{i} : a -> a\nf{i} x = f{} x\n\n", i + 1));
    }
    text.push_str(&format!("f{n} : a -> a\nf{n} x = x\n\n"));
    text.push_str("main : Unit -> <IO> Unit\nmain () = println (show_int (f1 1) ++ f1 \"s\")");
    for i in 1..=n {
        assert_eq!(
            instances_of(&text, &format!("f{i}")),
            [format!("f{i}@[Int]"), format!("f{i}@[String]")]
        );
    }
}

#[test]
fn one_function_used_at_k_types_has_k_instances() {
    let text = "id : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = id 1\n  let _ = id \"s\"\n  let _ = id True\n  let _ = id ()\n  let _ = id (1, \"s\")\n  ()";
    assert_eq!(
        instances_of(text, "id"),
        [
            "id@[Int]",
            "id@[String]",
            "id@[Bool]",
            "id@[Unit]",
            "id@[(Int, String)]"
        ]
    );
}

#[test]
fn polymorphic_recursion_is_uniform() {
    let text = "depth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else 1 + depth (n - 1) (x, x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (depth 3 1))";
    assert_eq!(instances_of(text, "depth"), ["depth@[_]"]);
}

#[test]
fn mutual_polymorphic_recursion_is_uniform() {
    let text = "f : Int -> a -> Int\nf n x = if n == 0 then 0 else g (n - 1) (x, x)\n\ng : Int -> b -> Int\ng n y = f n y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 3 1))";
    assert_eq!(instances_of(text, "f"), ["f@[_]"]);
    assert_eq!(instances_of(text, "g"), ["g@[_]"]);
}

#[test]
fn recursion_through_a_function_value_is_found() {
    // 自分を値として渡して呼ぶ形も、値の参照が具体化の表に入るので、大きくなる辺になる
    let text = "call : (Int -> b -> Int) -> Int -> b -> Int\ncall f n x = f n x\n\ndepth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else call depth (n - 1) (x, x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (depth 3 1))";
    assert_eq!(instances_of(text, "depth"), ["depth@[_]"]);
}

#[test]
fn only_the_growing_position_is_uniform() {
    let text = "walk : Int -> a -> b -> b\nwalk n x y = if n == 0 then y else walk (n - 1) (x, x) y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (walk 3 1 5))";
    assert_eq!(instances_of(text, "walk"), ["walk@[_, Int]"]);
    let shown = core_text(text, Pass::Translate);
    let header = function(&shown, "walk@[_, Int]");
    assert!(
        header.starts_with("fn \"walk@[_, Int]\"(n.0: int, x.1: tobj, y.2: int) -> int {"),
        "{header}"
    );
}

#[test]
fn a_generic_call_in_a_clause_at_the_clause_variable_is_uniform() {
    let text = "id : a -> a\nid x = x\n\neffect Pick where\n  pick : a -> a\n\nrun : Int -> Int\nrun v =\n  handle pick v with\n    | pick x k -> k (id x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (run 1))";
    assert_eq!(instances_of(text, "id"), ["id@[_]"]);
}

#[test]
fn a_clause_variable_that_escapes_through_the_handle_is_uniform() {
    let text = "\
effect Leak where
  put : a -> Unit
  never stop : Unit -> a

effect Abort where
  never abort : Unit -> a

id : a -> a
id x = x

pair : a -> (a, a)
pair x = (x, x)

body : Unit -> <Leak> b
body () =
  put 1
  stop ()

run : Unit -> <Abort> Unit
run () =
  let r = handle body () with
            | put x k ->
                drop k
                id x
            | stop u -> abort ()
  let p = pair r
  ()

main : Unit -> <IO> Unit
main () =
  handle run () with
    | abort u -> println \"aborted\"
  println \"done\"";
    assert_eq!(instances_of(text, "id"), ["id@[_]"]);
    assert_eq!(instances_of(text, "pair"), ["pair@[_]"]);
}

#[test]
fn a_long_shared_type_argument_gets_an_ordinal_name() {
    // `ident` の型引数は列の長さの指数の大きさで表示されるので、名前は順番の形になる
    let n = 64;
    let mut text = String::from(
        "ident : a -> a\nident x = x\n\nrun : Unit -> Int\nrun () =\n  let f0 = ident\n",
    );
    for i in 1..=n {
        text.push_str(&format!("  let f{i} = f{} ident\n", i - 1));
    }
    text.push_str(&format!(
        "  f{n} 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (run ()))"
    ));
    let names = instances_of(&text, "ident");
    assert_eq!(names.len(), n + 1);
    assert!(
        names
            .iter()
            .any(|name| name.starts_with("ident@") && !name.starts_with("ident@[")),
        "{names:?}"
    );
}

#[test]
fn monomorphic_programs_keep_their_function_order_and_names() {
    let text = "double : Int -> Int\ndouble n = n + n\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (double 2))";
    let program = eml_test_support::core_until(text, Pass::Translate);
    let names: Vec<&str> = program
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(names, ["double", "main", "entry$main"]);
}

#[test]
fn a_polymorphic_entry_is_uniform() {
    let checked = eml_test_support::check(
        "first : a -> a\nfirst x = x\n\nmain : Unit -> <IO> Unit\nmain () = ()",
    );
    let (entry, _) = checked
        .program
        .functions()
        .find(|(_, function)| function.name == "first")
        .unwrap();
    let program = eml_core_ir::lower_until(
        &checked.program,
        &checked.typed,
        entry,
        checked.files(),
        Pass::Translate,
    );
    let names: Vec<&str> = program
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(names, ["first@[_]", "entry$first"]);
}

const SMALL: usize = 2000;
const MAX_RATIO: f64 = 6.0;
/// 型の深さが大きさに比例する形を測るスレッドのスタック (crates/eml_types/tests/scaling.rs の `DEEP_STACK` と同じ)。
const DEEP_STACK: usize = 64 << 20;

/// 多相な関数の鎖を `Int` と `String` で使う形。
fn chain(n: usize) -> String {
    let mut text = String::new();
    for i in 1..n {
        text.push_str(&format!("f{i} : a -> a\nf{i} x = f{} x\n\n", i + 1));
    }
    text.push_str(&format!("f{n} : a -> a\nf{n} x = x\n\n"));
    text.push_str("main : Unit -> <IO> Unit\nmain () = println (show_int (f1 1) ++ f1 \"s\")");
    text
}

/// 部分を共有する型を作る `let` の列を、総称な関数の本体に置く形。`ident` の型引数は列の長さの指数の大きさで表示され、
/// `run` の型変数を含む。
fn shared_types(n: usize) -> String {
    let mut text =
        String::from("ident : a -> a\nident x = x\n\nrun : a -> a\nrun x =\n  let f0 = ident\n");
    for i in 1..=n {
        text.push_str(&format!("  let f{i} = f{} ident\n", i - 1));
    }
    text.push_str(&format!(
        "  f{n} x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (run 5))"
    ));
    text
}

/// Core IR の段階 (`eml_core_ir::lower`) だけの時間。3回測って最小を使う。
fn lower_time(text: &str) -> std::time::Duration {
    let checked = eml_test_support::check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "the generated program has errors"
    );
    let (entry, _) = checked
        .program
        .functions()
        .find(|(id, function)| {
            checked.program.modules[id.module].name == "Main" && function.name == "main"
        })
        .unwrap();
    (0..3)
        .map(|_| {
            let start = std::time::Instant::now();
            let _ = eml_core_ir::lower(&checked.program, &checked.typed, entry, checked.files());
            start.elapsed()
        })
        .min()
        .unwrap()
}

fn assert_linear(generate: fn(usize) -> String) {
    let small = lower_time(&generate(SMALL));
    let large = lower_time(&generate(SMALL * 4));
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= MAX_RATIO,
        "size {SMALL} took {small:?} and size {} took {large:?} (ratio {ratio:.1})",
        SMALL * 4
    );
}

fn assert_linear_deep(generate: fn(usize) -> String) {
    let measured = std::thread::Builder::new()
        .stack_size(DEEP_STACK)
        .spawn(move || assert_linear(generate))
        .unwrap()
        .join();
    if let Err(panic) = measured {
        std::panic::resume_unwind(panic);
    }
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn lowering_a_chain_of_polymorphic_functions_is_linear() {
    assert_linear(chain);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn lowering_shared_type_arguments_is_linear() {
    assert_linear_deep(shared_types);
}

//! 単相化の instance (docs/spec/core-ir.md の「変換の規則」)。どの (関数, 型引数) が instance になり、どう名付けるかを
//! 確かめる。

use crate::common::{core_text, function};
use eml_core_ir::Pass;

/// `Pass::Translate` までの Core IR の、内部の印のない関数のうち、`name@` で始まる名前を並べる。
fn instances_of(text: &str, name: &str) -> Vec<String> {
    instances_of_files(text, &[], name)
}

/// `instances_of` の、import するモジュールを持つ形。`modules` は根からの相対パスと本文の組である。
fn instances_of_files(text: &str, modules: &[(&str, &str)], name: &str) -> Vec<String> {
    let program = eml_test_support::core_until_files(text, modules, Pass::Translate);
    let prefix = format!("{name}@");
    program
        .functions
        .iter()
        .filter(|function| !function.internal && function.name.starts_with(&prefix))
        .map(|function| function.name.clone())
        .collect()
}

/// `name@#` に16桁の小文字の16進数が続き、`suffix` で終わる名前か。
fn is_hash_name(name: &str, base: &str, suffix: &str) -> bool {
    name.strip_prefix(&format!("{base}@#"))
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|hex| {
            hex.len() == 16
                && hex
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        })
}

#[test]
fn a_chain_used_at_two_types_has_two_instances_per_function() {
    let n = 20;
    let mut text = String::new();
    for i in 1..n {
        text.push_str(&format!("f{i} : a -> a\nf{i} x = f{} x\n\n", i + 1));
    }
    text.push_str(&format!("f{n} : a -> a\nf{n} x = x\n\n"));
    text.push_str("main : Unit -> <IO> Unit\nmain () = println (show (f1 1) ++ f1 \"s\")");
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
    let text = "depth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else 1 + depth (n - 1) (x, x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (depth 3 1))";
    assert_eq!(instances_of(text, "depth"), ["depth@[_]"]);
}

#[test]
fn mutual_polymorphic_recursion_is_uniform() {
    let text = "f : Int -> a -> Int\nf n x = if n == 0 then 0 else g (n - 1) (x, x)\n\ng : Int -> b -> Int\ng n y = f n y\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 3 1))";
    assert_eq!(instances_of(text, "f"), ["f@[_]"]);
    assert_eq!(instances_of(text, "g"), ["g@[_]"]);
}

#[test]
fn recursion_through_a_function_value_is_found() {
    // 自分を値として渡して呼ぶ形も、値の参照が具体化の表に入るので、大きくなる辺になる
    let text = "call : (Int -> b -> Int) -> Int -> b -> Int\ncall f n x = f n x\n\ndepth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else call depth (n - 1) (x, x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (depth 3 1))";
    assert_eq!(instances_of(text, "depth"), ["depth@[_]"]);
}

#[test]
fn only_the_growing_position_is_uniform() {
    let text = "walk : Int -> a -> b -> b\nwalk n x y = if n == 0 then y else walk (n - 1) (x, x) y\n\nmain : Unit -> <IO> Unit\nmain () = println (show (walk 3 1 5))";
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
    let text = "id : a -> a\nid x = x\n\neffect Pick where\n  pick : a -> a\n\nrun : Int -> Int\nrun v =\n  handle pick v with\n    | ctl pick x k -> k (id x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (run 1))";
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
            | ctl put x k ->
                drop k
                id x
            | ctl stop u -> abort ()
  let p = pair r
  ()

main : Unit -> <IO> Unit
main () =
  handle run () with
    | ctl abort u -> println \"aborted\"
  println \"done\"";
    assert_eq!(instances_of(text, "id"), ["id@[_]"]);
    assert_eq!(instances_of(text, "pair"), ["pair@[_]"]);
}

#[test]
fn a_long_shared_type_argument_gets_a_hash_name() {
    // `ident` の型引数は列の長さの指数の大きさで表示されるので、名前は型の構造のハッシュの形になる
    let n = 64;
    let mut text = String::from(
        "ident : a -> a\nident x = x\n\nrun : Unit -> Int\nrun () =\n  let f0 = ident\n",
    );
    for i in 1..=n {
        text.push_str(&format!("  let f{i} = f{} ident\n", i - 1));
    }
    text.push_str(&format!(
        "  f{n} 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show (run ()))"
    ));
    let names = instances_of(&text, "ident");
    assert_eq!(names.len(), n + 1);
    assert!(
        names.iter().any(|name| is_hash_name(name, "ident", "")),
        "{names:?}"
    );
}

#[test]
fn a_row_polymorphic_function_has_an_instance_per_row_bit() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : (Unit -> <e> Int) -> <e> Int\ntwice f = f () + f ()\n\npure : Unit -> Int\npure () = twice (fn () -> 1)\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (show (pure ()))\n  println (show (handle twice ask with\n    | ctl ask () k -> k 2))";
    let shown = core_text(text, Pass::Translate);
    // 純粋な instance は今の名前で印がなく、yield しうる instance は `<e>` を持つ
    insta::assert_snapshot!(function(&shown, "twice"), @"
    fn twice(f.0: tobj) -> int {
      let t.1: int = apply f.0(())
      let t.2: int = apply f.0(())
      let t.3: int = extern Prelude.+(t.1, t.2)
      return t.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "twice<e>"), @r#"
    yields fn "twice<e>"(f.0: tobj) -> int {
      let t.1: int = yields apply f.0(())
      let t.2: int = yields apply f.0(())
      let t.3: int = extern Prelude.+(t.1, t.2)
      return t.3
    }
    "#);
}

#[test]
fn only_the_set_row_variables_are_named_in_signature_order() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nboth : (Unit -> <e> Int) -> (Unit -> <f> Int) -> <f> Int\nboth a b = b ()\n\nrun : Unit -> <Ask> Int\nrun () = both (fn () -> 1) ask + both ask ask\n\nmain : Unit -> <IO> Unit\nmain () = println (show (handle run () with\n    | ctl ask () k -> k 2))";
    let program = eml_test_support::core_until(text, Pass::Translate);
    let names: Vec<&str> = program
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .filter(|name| name.starts_with("both"))
        .collect();
    // 1つ目の呼び出しは f のビットだけが立ち、2つ目は両方が立つ
    assert_eq!(names, ["both<f>", "both<e, f>"]);
}

#[test]
fn a_hash_name_keeps_its_row_bits() {
    // `tap` の型引数 (f1 の型) は列の長さの指数の大きさで表示されるので、名前はハッシュの形になり、その後ろに立った
    // ビットが付く
    let n = 64;
    let mut text = String::from(
        "effect Ask where\n  ask : Unit -> Int\n\nident : a -> a\nident x = x\n\ntap : a -> (Unit -> <e> Int) -> <e> a\ntap x g =\n  let _ = g ()\n  x\n\nrun : Unit -> <Ask> Int\nrun () =\n  let f0 = ident\n",
    );
    for i in 1..=n {
        text.push_str(&format!("  let f{i} = f{} ident\n", i - 1));
    }
    text.push_str(&format!(
        "  let _ = tap f1 ask\n  f{n} 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show (handle run () with\n    | ctl ask () k -> k 2))"
    ));
    assert_eq!(instances_of(&text, "tap"), ["tap@#25dc3bd8ddbc9f49<e>"]);
}

/// `tap` を長い型引数で使うプログラム。`short` なら、その前に `Int` でも使う。
fn tap_program(short: bool) -> String {
    let n = 64;
    let mut text = String::from(
        "effect Ask where\n  ask : Unit -> Int\n\nident : a -> a\nident x = x\n\ntap : a -> (Unit -> <e> Int) -> <e> a\ntap x g =\n  let _ = g ()\n  x\n\nrun : Unit -> <Ask> Int\nrun () =\n  let f0 = ident\n",
    );
    if short {
        text.push_str("  let _ = tap 1 ask\n");
    }
    for i in 1..=n {
        text.push_str(&format!("  let f{i} = f{} ident\n", i - 1));
    }
    text.push_str(&format!(
        "  let _ = tap f1 ask\n  f{n} 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show (handle run () with\n    | ctl ask () k -> k 2))"
    ));
    text
}

#[test]
fn a_hash_name_does_not_depend_on_the_other_instances() {
    // 順番の名前は、前に見つかった instance があると番号が変わった。名前は型引数だけで決まるので、同じ名前の関数が
    // いつも同じコードになる (docs/spec/core-ir.md の「変換の規則」)
    let alone = instances_of(&tap_program(false), "tap");
    let with_short = instances_of(&tap_program(true), "tap");
    assert_eq!(alone, ["tap@#25dc3bd8ddbc9f49<e>"]);
    assert_eq!(with_short, ["tap@[Int]<e>", "tap@#25dc3bd8ddbc9f49<e>"]);
}

#[test]
fn a_type_outside_the_prelude_and_the_entry_is_qualified_by_its_module() {
    // 名前の型は、診断の表示名と違って、名前が重ならなくてもモジュールで修飾する。後の入力が同じ名前を定義しても
    // 名前が変わらないようにするためである
    let text = "id : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = id (Fs.open \"x.txt\")\n  Fs.close f";
    assert_eq!(instances_of(text, "id"), ["id@[Std.Fs.File]"]);
    let modules = [("Shape.em", "pub data Shape = | Dot\n")];
    let text = "import Shape (Shape(..))\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = id Dot\n  ()";
    assert_eq!(
        instances_of_files(text, &modules, "id"),
        ["id@[Shape.Shape]"]
    );
}

#[test]
fn a_prelude_type_is_not_qualified_even_when_another_module_shares_its_name() {
    // 入口の `Bool` は Prelude の `Bool` と重なるので `Main.` で修飾する。Prelude の `Bool` は修飾しない
    let text = "data Bool = | No | Yes\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = id True\n  let _ = id No\n  ()";
    assert_eq!(instances_of(text, "id"), ["id@[Bool]", "id@[Main.Bool]"]);
    // import したモジュールが Prelude と同じ名前を定義しても、Prelude の型は修飾しない
    let modules = [("Opt.em", "pub data Option a = | Nope\n")];
    let text = "import Opt\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = id (Some 1)\n  let _ = id Opt.Nope\n  ()";
    assert_eq!(
        instances_of_files(text, &modules, "id"),
        ["id@[Option Int]", "id@[Opt.Option _]"]
    );
}

#[test]
fn a_method_reference_passes_its_row_bits_to_the_method_function() {
    // instance のメソッドの関数と既定のメソッドの関数は、メソッドのシグネチャと同じ順の row 変数を持つ。pure の中の
    // 呼び出しは純粋な instance に、run の中の呼び出しは `<e>` の instance に向かう
    let text = "effect Ask where\n  ask : Unit -> Int\n\nclass Runner a where\n  run_with : a -> (Unit -> <e> Int) -> <e> Int\n  run_default : a -> (Unit -> <e> Int) -> <e> Int\n  run_default x g = g ()\n\ninstance Runner Int where\n  run_with n g = g () + n\n\nrun : Unit -> <Ask> Int\nrun () = run_with 1 ask + run_default 1 ask\n\npure : Unit -> Int\npure () = run_with 2 (fn () -> 3) + run_default 2 (fn () -> 3)\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (show (pure ()))\n  println (show (handle run () with\n    | ctl ask () k -> k 2))";
    let program = eml_test_support::core_until(text, Pass::Translate);
    let mut names: Vec<(&str, bool)> = program
        .functions
        .iter()
        .filter(|function| function.name.contains("run_"))
        .map(|function| (function.name.as_str(), function.yields))
        .collect();
    names.sort();
    insta::assert_debug_snapshot!(names, @r#"
    [
        (
            "Runner Int.run_with",
            false,
        ),
        (
            "Runner Int.run_with<e>",
            true,
        ),
        (
            "run_default@[Int]",
            false,
        ),
        (
            "run_default@[Int]<e>",
            true,
        ),
    ]
    "#);
}

#[test]
fn a_clause_lambda_over_an_operation_row_variable_may_yield() {
    let text = "effect Run where\n  run : (Unit -> <e> Int) -> Int\n\nbody : Unit -> <Run> Int\nbody () = run (fn () -> 1)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle body () with\n      | ctl run f k ->\n        let g = fn () -> f () + 1\n        k 7\n  println (show r)";
    let shown = core_text(text, Pass::Translate);
    // g の row は操作の row 変数で、main の row 変数ではないので、g は yield しうる
    insta::assert_snapshot!(function(&shown, "main$lambda0"), @"
    internal yields fn main$lambda0(f.0: tobj, p.1: unit) -> int {
      let t.2: int = yields apply f.0(())
      let t.3: int = extern Prelude.+(t.2, 1)
      return t.3
    }
    ");
}

#[test]
fn an_apply_is_split_after_an_arrow_that_may_yield() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nadder : Int -> <Ask> (Int -> Int)\nadder x =\n  let n = ask ()\n  fn y -> x + y + n\n\nrun : (Int -> <Ask> (Int -> Int)) -> <Ask> Int\nrun h = h 1 2\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle run adder with\n      | ctl ask () k -> k 10\n  println (show r)";
    let shown = core_text(text, Pass::Translate);
    // h 1 で yield しうるので、1つ目の apply の後で分け、1つ目に印を付ける
    insta::assert_snapshot!(function(&shown, "run"), @"
    yields fn run(h.0: tobj poly) -> int {
      let t.1: tobj poly = yields apply h.0(1)
      let t.2: int = apply t.1(2)
      return t.2
    }
    ");
}

#[test]
fn monomorphic_programs_keep_their_function_order_and_names() {
    let text = "double : Int -> Int\ndouble n = n + n\n\nmain : Unit -> <IO> Unit\nmain () = println (show (double 2))";
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
    let program = eml_core_ir::core_until(
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
    assert_eq!(names, ["first@[_:poly]", "entry$first"]);
}

/// `name` の関数を入口にして、`Pass::Translate` までの Core IR を表示する。
fn core_text_with_entry(text: &str, name: &str) -> String {
    let checked = eml_test_support::check(text);
    let (entry, _) = checked
        .program
        .functions()
        .find(|(_, function)| function.name == name)
        .unwrap();
    eml_core_ir::pretty(&eml_core_ir::core_until(
        &checked.program,
        &checked.typed,
        entry,
        checked.files(),
        Pass::Translate,
    ))
}

#[test]
fn a_row_polymorphic_entry_may_yield() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ngo : Unit -> <e> Int\ngo () = 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    // 入口の row のビットはすべて立つので、`go<e>` と入口の関数が yield しうる
    insta::assert_snapshot!(core_text_with_entry(text, "go"), @r#"
    effect Prelude.IO { never io_failed(enum, obj) -> never }
    effect Ask { ask(unit) -> int }
    yields fn "go<e>"(p.0: unit) -> int {
      return 1
    }
    yields fn entry$go() -> int {
      let t.0: int = call "go<e>"(())
      return t.0
    }
    "#);
}

#[test]
fn a_point_free_entry_takes_its_mark_from_its_first_arrow() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nact : Unit -> <Ask> Int\nact = fn () -> ask ()\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    // 引数のない値の instance は印を持たないが、入口の関数は `()` を渡す矢印の row で yield しうる
    insta::assert_snapshot!(function(&core_text_with_entry(text, "act"), "entry$act"), @"
    yields fn entry$act() -> int {
      let f.0: tobj = call act()
      let t.1: int = yields apply f.0(())
      return t.1
    }
    ");
}

#[test]
fn a_point_free_entry_whose_first_arrow_has_its_own_row_variable_may_yield() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ngo : Unit -> <e> Int\ngo = fn () -> 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    // 最初の矢印の row は入口自身の row 変数なので、その印は入口の row のビット (すべて立つ) から決まる
    insta::assert_snapshot!(function(&core_text_with_entry(text, "go"), "entry$go"), @r#"
    yields fn entry$go() -> int {
      let f.0: tobj = call "go<e>"()
      let t.1: int = yields apply f.0(())
      return t.1
    }
    "#);
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
    text.push_str("main : Unit -> <IO> Unit\nmain () = println (show (f1 1) ++ f1 \"s\")");
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
        "  f{n} x\n\nmain : Unit -> <IO> Unit\nmain () = println (show (run 5))"
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

#[test]
fn instances_that_differ_only_in_linearity_have_different_names() {
    // `k` は `File` を捕まえるので矢印が `Lin` になる。`inc` の宣言の一番外の矢印は `Unr` で、参照の row は開いて
    // いるので、`main` の中で呼ぶと `<IO>` になる。2つの型引数は線形性だけが違い、名前は矢印の線形性を `once` で書くので、
    // 番号付きの名前に落ちない (docs/spec/core-ir.md の「変換の規則」)
    let text = "id : a -> a\nid x = x\n\ninc : Int -> Int\ninc x = x + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let h = Fs.open \"a.txt\"\n  let k = fn n ->\n    Fs.close h\n    n\n  let one = id k\n  let a = one 1\n  let i = id inc\n  println (show (i a))";
    assert_eq!(
        instances_of(text, "id"),
        ["id@[once Int -> <IO> Int]", "id@[Int -> Int]"]
    );
}

#[test]
fn a_function_called_with_lin_and_unr_closures_has_one_instance() {
    // instance の鍵は型引数だけで、呼ばれる側の矢印の変数は鍵に入らない。`apply` の `f` の矢印は instance の中で
    // `Poly` になる (docs/spec/types.md の「instance での評価」)
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let h = Fs.open \"a.txt\"\n  let a = apply (fn n ->\n    Fs.close h\n    n) 1\n  let b = apply (fn n -> n + 1) a\n  println (show b)";
    assert_eq!(instances_of(text, "apply"), ["apply@[Int, Int]"]);
}

/// spec の「instance での評価」の `f` の例。
const F_EXAMPLE: &str = "f : a -> (a -> <IO> Int) -> (Int -> <IO> Int) -> Int -> <IO> Int\nf x eat g n =\n  if n == 0 then g (eat x)\n  else\n    let k = fn y -> g (eat x + y)\n    f k (fn h -> h 1) (fn z -> z) (n - 1)\n\nshut_with : Fs.File -> Int -> <IO> Int\nshut_with file z =\n  Fs.close file\n  z\n\nmain : Unit -> <IO> Unit\nmain () =\n  let file = Fs.open \"a.txt\"\n  let n = f 1 (fn y -> y) (shut_with file) 3\n  println (show n)";

#[test]
fn a_type_variable_above_a_poly_arrow_makes_the_recursive_call_poly() {
    // spec の「instance での評価」の `f` の例。`k` の矢印は `μ_a` と同じ成分にあり、`f@[Int]` で `g` の矢印の
    // `Poly` が `μ_a` に伝わるので、再帰の呼び出しの型引数は `poly Int -> <IO> Int` になる
    let text = F_EXAMPLE;
    assert_eq!(
        instances_of(text, "f"),
        ["f@[Int]", "f@[poly Int -> <IO> Int]"]
    );
}

#[test]
fn a_closure_with_a_file_passed_as_a_type_variable_lowers() {
    // 上の `f` の例を LIR まで下ろす。debug ビルドでは、translate の参照、操作、捕獲の検査がこの途中で走るので、
    // 検査がこのプログラムを拒まないことも確かめる。`k` は `File` を捕まえうるので `unr` にならない
    // (docs/spec/core-ir.md の「食い違いがないことの検査」)
    let text = F_EXAMPLE;
    let program = eml_test_support::lir(text);
    let names: Vec<&str> = program
        .functions
        .iter()
        .filter(|function| !function.internal && function.name.starts_with("f@"))
        .map(|function| function.name.as_str())
        .collect();
    // optimize は `f` を既知のラムダごとに複製するので、2つの instance は複製に置き換わる。複製の連鎖は鍵の入れ子の
    // 深さで止まる (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「鍵」)
    assert_eq!(
        names,
        [
            "f@[Int]{1=main$lambda0, 2=shut_with}",
            "f@#a5c201dc663fca79",
            "f@#c525f19f0fa72adf",
            "f@#8125c0618c7d4a0f",
            "f@#57be9109987267a6",
            "f@[Int]$lambda0{1=main$lambda0, 2=shut_with}",
            "f@#4e5278e67ecf2e33",
            "f@[poly Int -> <IO> Int]$lambda1{0=f@#a5c201dc663fca79}",
            "f@[poly Int -> <IO> Int]$lambda1{0=f@#8125c0618c7d4a0f}",
            "f@[Int]$lambda1{0=f@[Int]$lambda0{1=main$lambda0, 2=shut_with}}",
            "f@#a5c201dc663fca79$boxed",
            "f@#8125c0618c7d4a0f$boxed",
            "f@[Int]$lambda0{1=main$lambda0, 2=shut_with}$boxed",
        ]
    );
}

#[test]
fn a_result_only_operation_variable_gives_one_uniform_key() {
    // `mk` の `a` は結果の型にだけ現れるので、`ids` の鍵は一様な位置の `_` が1つになる。名前の `Flexible` は Kind の印を
    // 付けて書く (docs/spec/types.md の「`Flexible` と `OpVar` の Kind」と、docs/spec/core-ir.md の「変換の規則」)。
    // `OpVar` が `Poly` の Kind を持つことは、eml_types の `kind_export::a_clause_in_main_keeps_its_operation_variable_kinds` が確かめる
    let text = "ids : List a -> List a\nids xs = xs\n\neffect Make where\n  mk : Unit -> List a\n\nbody : Unit -> <Make> Int\nbody () =\n  let xs = (mk () : List Int)\n  0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = handle body () with\n            | ctl mk () k -> k (ids [])\n  println (show n)";
    assert_eq!(instances_of(text, "ids"), ["ids@[_:poly]"]);
}

#[test]
fn instance_names_show_row_arguments() {
    let text = "data Iter <e> a =\n  | Done\n  | More a (once Unit -> <e> Iter <e> a)\n\nid : a -> a\nid x = x\n\nempty : Iter <IO> Int\nempty = Done\n\nmain : Unit -> <IO> Unit\nmain () = drop (id empty)";
    assert_eq!(instances_of(text, "id"), ["id@[Iter <IO> Int]"]);
}

#[test]
fn a_top_level_function_in_a_once_field_lowers() {
    // `Task worker` の `worker` の型は一番外側の矢印が `once` で、`worker` のシグネチャの `Unr` と食い違う。参照の一番
    // 外側の矢印は参照ごとの変数なので、translate の参照の検査はそこを比べない (docs/spec/types.md の「トップレベルの関数の参照」)
    let text = "data Task =\n  | Task (once Unit -> <IO> Unit)\n\nworker : Unit -> <IO> Unit\nworker () = println \"work\"\n\nget_worker : Unit -> (Unit -> <IO> Unit)\nget_worker () = worker\n\nmain : Unit -> <IO> Unit\nmain () =\n  match Task worker with\n    | Task f -> f ()\n  get_worker () ()";
    let program = eml_test_support::lir(text);
    let names: Vec<&str> = program
        .functions
        .iter()
        .filter(|function| !function.internal && !function.name.contains('$'))
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(names, ["worker", "get_worker", "main"]);
}

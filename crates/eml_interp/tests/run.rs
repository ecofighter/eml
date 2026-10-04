use std::sync::Arc;

use eml_core_ir::{Atom, CExpr, CExprId, CoreFn, FnIdx, Linearity, Program, Rhs, VarId, VarInfo};
use eml_diagnostics::{SourceFiles, has_errors};
use eml_interp::{RunConfig, RuntimeError};
use eml_runtime::OutputSink;

/// `debug_heap` を有効にして実行し、出力と結果を返す。
fn run(text: &str) -> (String, Result<(), RuntimeError>) {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, text);
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    assert!(!has_errors(&diagnostics), "{diagnostics:#?}");
    execute(eml_core_ir::lower(&module, &typed), true)
}

fn execute(program: Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(debug_heap);
    let result = eml_interp::run(Arc::new(program), &config, &sink);
    (captured.contents(), result)
}

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
}

#[test]
fn hello_world() {
    assert_eq!(
        run(&main_with("  println \"hi\"")),
        ("hi\n".to_string(), Ok(()))
    );
}

#[test]
fn arithmetic_truncates_toward_zero() {
    let body = "  println (show_int (1 + 2 * 3))\n  println (show_int ((-7) / 2))\n  println (show_int ((-7) % 2))\n  println (show_int (-2 * 3))";
    assert_eq!(
        run(&main_with(body)),
        ("7\n-3\n-1\n-6\n".to_string(), Ok(()))
    );
}

#[test]
fn recursion() {
    let text =
        "factorial : Int -> Int\nfactorial n = if n <= 1 then 1 else n * factorial (n - 1)\n\n"
            .to_string()
            + &main_with("  println (show_int (factorial 20))");
    assert_eq!(run(&text), ("2432902008176640000\n".to_string(), Ok(())));
}

#[test]
fn deep_recursion_does_not_overflow_the_stack() {
    let text = "count : Int -> Int\ncount n = if n == 0 then 0 else 1 + count (n - 1)\n\n"
        .to_string()
        + &main_with("  println (show_int (count 100000))");
    assert_eq!(run(&text), ("100000\n".to_string(), Ok(())));
}

#[test]
fn strings_are_freed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\npick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\n".to_string()
        + &main_with("  let s = \"x\"\n  let s = s ++ \"y\"\n  let _ = \"z\"\n  println (twice s)\n  println (show_int (ignore \"w\"))\n  println (pick True \"a\")\n  println (pick False \"b\")");
    assert_eq!(run(&text), ("xyxy\n1\naa\nnoneb\n".to_string(), Ok(())));
}

#[test]
fn and_and_or_short_circuit() {
    let text = "noisy : Bool -> <IO> Bool\nnoisy b =\n  println \"evaluated\"\n  b\n\nshow_bool : Bool -> String\nshow_bool b = if b then \"True\" else \"False\"\n\n".to_string()
        + &main_with("  println (show_bool (False && noisy True))\n  println (show_bool (True || noisy False))\n  println (show_bool (True && noisy False))");
    assert_eq!(
        run(&text),
        ("False\nTrue\nevaluated\nFalse\n".to_string(), Ok(()))
    );
}

#[test]
fn integer_overflow_is_a_runtime_error() {
    let (stdout, result) = run(&main_with(
        "  println \"before\"\n  println (show_int (9223372036854775807 + 1))",
    ));
    assert_eq!(stdout, "before\n");
    assert_eq!(
        result,
        Err(RuntimeError("integer overflow in `main`".to_string()))
    );
}

#[test]
fn division_by_zero_is_a_runtime_error() {
    let text = "divide : Int -> Int -> Int\ndivide a b = a / b\n\n".to_string()
        + &main_with("  println (show_int (divide 1 0))");
    assert_eq!(
        run(&text).1,
        Err(RuntimeError("division by zero in `divide`".to_string()))
    );
}

/// Perceus の挿入を経ない手書きの Core IR で、`debug_heap` がリークを見つけることを確かめる。
fn leaking_program() -> Program {
    let var = |name: &str, boxed| VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed,
    };
    Program {
        functions: vec![CoreFn {
            name: "main".to_string(),
            params: vec![VarId(0)],
            vars: vec![var("p", false), var("s", true)],
            body: CExprId(1),
            exprs: vec![
                CExpr::Return(Atom::Unit),
                CExpr::Let {
                    var: VarId(1),
                    rhs: Rhs::ConstString(0),
                    body: CExprId(0),
                },
            ],
        }],
        main: FnIdx(0),
        strings: vec!["leaked".to_string()],
    }
}

#[test]
fn debug_heap_reports_leaks() {
    assert_eq!(
        execute(leaking_program(), true).1,
        Err(RuntimeError(
            "memory leak: objects were not freed: 1 String".to_string()
        ))
    );
    assert_eq!(execute(leaking_program(), false).1, Ok(()));
}

#[test]
fn long_statement_sequence_does_not_overflow_the_stack() {
    // 逐次の文は入れ子ではないので、後段は長い `Let` の連鎖を再帰せずに処理しなければならない
    let mut body = "  let s = \"x\"\n".repeat(5000);
    body.push_str("  println s");
    assert_eq!(run(&main_with(&body)), ("x\n".to_string(), Ok(())));
}

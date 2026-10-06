//! ソースから実行して確かめるテストは UI テスト (`tests/ui/run/`) に置く。ここには、Core IR のテキストや生成した
//! ソースが要るものだけを置く (docs/implementation/testing.md)。

use std::fmt::Write;

use eml_core_ir::Program;
use eml_interp::RuntimeError;
use eml_test_support::{execute, run};

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
}

/// Perceus の挿入を経ない手書きの Core IR で、`debug_heap` がリークを見つけることを確かめる。
fn leaking_program() -> Program {
    eml_core_ir::parse(
        r#"
fn main() {
  let s1^ = const "leaked"
  return ()
}
"#,
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn debug_heap_reports_leaks() {
    assert_eq!(
        execute(leaking_program(), true).1,
        Err(RuntimeError::Leak(vec![("String".to_string(), 1)]))
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

#[test]
fn long_sequence_of_if_statements_does_not_overflow_the_stack() {
    // 文の `if` は join point になり、続きの文はその本体に入れ子になる。後段は、この入れ子を再帰せずに処理しなければならない
    let mut body = "  if True then println \"x\"\n".repeat(5000);
    body.push_str("  println \"done\"");
    let expected = format!("{}done\n", "x\n".repeat(5000));
    assert_eq!(run(&main_with(&body)), (expected, Ok(())));
}

#[test]
fn a_match_with_a_thousand_literals_runs_in_a_debug_build() {
    // 平らな `Switch` は、リテラルの数だけ入れ子を深くしない (docs/spec/core-ir.md)
    let mut text = String::from("pick : Int -> Int\npick n =\n  match n with\n");
    for i in 0..1000 {
        writeln!(text, "    | {i} -> {}", i + 1).unwrap();
    }
    text.push_str(
        "    | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick 999))\n",
    );
    let (out, result) = eml_test_support::run(&text);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "1000\n");
}

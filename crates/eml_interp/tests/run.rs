//! ソースから実行して確かめるテストは UI テスト (`tests/ui/run/`) に置く。ここには、Core IR のテキストや生成した
//! ソースが要るものだけを置く (docs/implementation/testing.md)。

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

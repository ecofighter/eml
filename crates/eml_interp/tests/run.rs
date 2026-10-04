//! ソースから実行して確かめるテストは UI テスト (`tests/ui/run/`) に置く。ここには、手書きの Core IR や生成した
//! ソースが要るものだけを置く (docs/implementation/testing.md)。

use eml_core_ir::{Atom, CExpr, CExprId, CoreFn, FnIdx, Linearity, Program, Rhs, VarId, VarInfo};
use eml_interp::RuntimeError;
use eml_test_support::{execute, run};

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
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

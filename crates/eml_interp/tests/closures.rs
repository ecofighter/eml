//! 手で組んだ Core IR で、クロージャの eval/apply を確かめる (docs/spec/core-ir.md)。

use std::sync::Arc;

use eml_core_ir::{
    Atom, CExpr, CExprId, CoreFn, FnIdx, IoOp, Linearity, PrimOp, Program, Rhs, VarId, VarInfo,
};
use eml_interp::{RunConfig, run};
use eml_runtime::OutputSink;

enum Step {
    Let(u32, Rhs),
    Dup(u32),
}

fn function(name: &str, params: u32, vars: &[(&str, bool)], steps: Vec<Step>, ret: Atom) -> CoreFn {
    let mut exprs = vec![CExpr::Return(ret)];
    let mut body = CExprId(0);
    for step in steps.into_iter().rev() {
        exprs.push(match step {
            Step::Let(var, rhs) => CExpr::Let {
                var: VarId(var),
                rhs,
                body,
            },
            Step::Dup(var) => CExpr::Dup {
                var: VarId(var),
                body,
            },
        });
        body = CExprId(exprs.len() as u32 - 1);
    }
    CoreFn {
        name: name.to_string(),
        params: (0..params).map(VarId).collect(),
        vars: vars
            .iter()
            .map(|&(name, boxed)| VarInfo {
                name: name.to_string(),
                linearity: Linearity::Unr,
                boxed,
            })
            .collect(),
        body,
        exprs,
    }
}

fn run_program(functions: Vec<CoreFn>, main: u32, strings: &[&str]) -> String {
    let program = Program {
        functions,
        main: FnIdx(main),
        strings: strings.iter().map(|s| s.to_string()).collect(),
    };
    let mut config = RunConfig::default();
    config.debug_heap = true;
    let (sink, buffer) = OutputSink::capture();
    run(Arc::new(program), &config, &sink).unwrap();
    String::from_utf8(buffer.lock().unwrap().clone()).unwrap()
}

fn var(n: u32) -> Atom {
    Atom::Var(VarId(n))
}

/// `first a b = a`。
fn first(boxed: bool) -> CoreFn {
    function("first", 2, &[("a", boxed), ("b", false)], vec![], var(0))
}

/// 値 `n` の変数を文字列にして出力する。
fn print_int(n: u32, show: u32, out: u32) -> Vec<Step> {
    vec![
        Step::Let(show, Rhs::Prim(PrimOp::ShowInt, vec![var(n)])),
        Step::Let(out, Rhs::Perform(IoOp::Println, vec![var(show)])),
    ]
}

#[test]
fn a_partial_application_waits_for_the_rest_of_the_arguments() {
    let mut steps = vec![
        Step::Let(1, Rhs::MakeClosure(FnIdx(0), vec![Atom::Int(10)])),
        Step::Let(2, Rhs::Apply(var(1), vec![Atom::Int(20)])),
    ];
    steps.extend(print_int(2, 3, 4));
    let vars = [
        ("p", false),
        ("c", true),
        ("r", false),
        ("s", true),
        ("t", false),
    ];
    let main = function("main", 1, &vars, steps, var(4));
    assert_eq!(run_program(vec![first(false), main], 1, &[]), "10\n");
}

#[test]
fn extra_arguments_are_applied_to_the_returned_function() {
    // make x = closure first(x)
    let make = function(
        "make",
        1,
        &[("x", false), ("c", true)],
        vec![Step::Let(1, Rhs::MakeClosure(FnIdx(0), vec![var(0)]))],
        var(1),
    );
    let mut steps = vec![
        Step::Let(1, Rhs::MakeClosure(FnIdx(1), vec![])),
        Step::Let(2, Rhs::Apply(var(1), vec![Atom::Int(5), Atom::Int(6)])),
    ];
    steps.extend(print_int(2, 3, 4));
    let vars = [
        ("p", false),
        ("m", true),
        ("r", false),
        ("s", true),
        ("t", false),
    ];
    let main = function("main", 1, &vars, steps, var(4));
    assert_eq!(run_program(vec![first(false), make, main], 2, &[]), "5\n");
}

#[test]
fn a_shared_closure_keeps_its_captured_values() {
    let steps = vec![
        Step::Let(1, Rhs::ConstString(0)),
        Step::Let(2, Rhs::MakeClosure(FnIdx(0), vec![var(1)])),
        Step::Dup(2),
        Step::Let(3, Rhs::Apply(var(2), vec![Atom::Int(1)])),
        Step::Let(4, Rhs::Perform(IoOp::Println, vec![var(3)])),
        Step::Let(5, Rhs::Apply(var(2), vec![Atom::Int(2)])),
        Step::Let(6, Rhs::Perform(IoOp::Println, vec![var(5)])),
    ];
    let vars = [
        ("p", false),
        ("s", true),
        ("c", true),
        ("r", true),
        ("t", false),
        ("r", true),
        ("t", false),
    ];
    let main = function("main", 1, &vars, steps, var(6));
    assert_eq!(run_program(vec![first(true), main], 1, &["a"]), "a\na\n");
}

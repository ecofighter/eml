//! 手で組んだ Core IR で、クロージャの eval/apply を確かめる (docs/spec/core-ir.md)。

use eml_core_ir::{
    Atom, CExpr, CExprId, Call, CoreFn, FnIdx, IoOp, Linearity, PrimOp, Program, Rhs, VarId,
    VarInfo,
};

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
        joins: Vec::new(),
    }
}

fn run_program(functions: Vec<CoreFn>, main: u32, strings: &[&str]) -> String {
    let program = Program {
        functions,
        entry: FnIdx(main),
        strings: strings.iter().map(|s| s.to_string()).collect(),
    };
    let (stdout, result) = eml_test_support::execute(program, true);
    result.unwrap();
    stdout
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
        Step::Let(1, Rhs::MakeClosure(FnIdx(0), vec![])),
        Step::Let(2, Rhs::call(Call::Apply(var(1), vec![Atom::Int(10)]))),
        Step::Let(3, Rhs::call(Call::Apply(var(2), vec![Atom::Int(20)]))),
    ];
    steps.extend(print_int(3, 4, 5));
    let vars = [
        ("p", false),
        ("c", true),
        ("d", true),
        ("r", false),
        ("s", true),
        ("t", false),
    ];
    let main = function("main", 0, &vars, steps, var(5));
    assert_eq!(run_program(vec![first(false), main], 1, &[]), "10\n");
}

#[test]
fn a_returned_function_can_still_wait_for_more_arguments() {
    let first3 = function(
        "first3",
        3,
        &[("a", false), ("b", false), ("c", false)],
        vec![],
        var(0),
    );
    // make x = closure first3(x)
    let make = function(
        "make",
        1,
        &[("x", false), ("c", true)],
        vec![Step::Let(1, Rhs::MakeClosure(FnIdx(0), vec![var(0)]))],
        var(1),
    );
    let mut steps = vec![
        Step::Let(1, Rhs::MakeClosure(FnIdx(1), vec![])),
        Step::Let(
            2,
            Rhs::call(Call::Apply(var(1), vec![Atom::Int(5), Atom::Int(6)])),
        ),
        Step::Let(3, Rhs::call(Call::Apply(var(2), vec![Atom::Int(7)]))),
    ];
    steps.extend(print_int(3, 4, 5));
    let vars = [
        ("p", false),
        ("m", true),
        ("r", true),
        ("s", false),
        ("t", true),
        ("u", false),
    ];
    let main = function("main", 0, &vars, steps, var(5));
    assert_eq!(run_program(vec![first3, make, main], 2, &[]), "5\n");
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
        Step::Let(
            2,
            Rhs::call(Call::Apply(var(1), vec![Atom::Int(5), Atom::Int(6)])),
        ),
    ];
    steps.extend(print_int(2, 3, 4));
    let vars = [
        ("p", false),
        ("m", true),
        ("r", false),
        ("s", true),
        ("t", false),
    ];
    let main = function("main", 0, &vars, steps, var(4));
    assert_eq!(run_program(vec![first(false), make, main], 2, &[]), "5\n");
}

#[test]
fn a_shared_closure_keeps_its_captured_values() {
    let steps = vec![
        Step::Let(1, Rhs::ConstString(0)),
        Step::Let(2, Rhs::MakeClosure(FnIdx(0), vec![var(1)])),
        Step::Dup(2),
        Step::Let(
            3,
            Rhs::Call {
                call: Call::Apply(var(2), vec![Atom::Int(1)]),
                saved: vec![VarId(2)],
            },
        ),
        Step::Let(4, Rhs::Perform(IoOp::Println, vec![var(3)])),
        Step::Let(5, Rhs::call(Call::Apply(var(2), vec![Atom::Int(2)]))),
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
    let main = function("main", 0, &vars, steps, var(6));
    assert_eq!(run_program(vec![first(true), main], 1, &["a"]), "a\na\n");
}

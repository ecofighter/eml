//! 呼び出しの評価の手順と、引数をまとめて渡す範囲 (docs/spec/expressions.md の「関数」)。

use eml_hir::{EvalStep, ExprKind, call_steps};
use eml_test_support::lower_clean;

const PRELUDE: &str = "f : Int -> Int -> Int\nf a b = a\n\ng : Unit -> Int\ng () = 1\n\nh : Int -> (Int -> Int)\nh a = fn b -> a + b\n\nw : Int -> Int -> Int\nw = fn a b -> a\n\n";

/// 関数 `t` の本体 (呼び出し) の手順を、評価する部分式のソースと、適用する矢印の番号で表す。
fn steps(t: &str) -> Vec<String> {
    let lowered = lower_clean(&format!("{PRELUDE}{t}"));
    let module = &lowered.module;
    let (_, function) = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "t")
        .unwrap();
    let body = function.body.as_ref().unwrap();
    assert!(matches!(body.exprs[body.root].kind, ExprKind::Call { .. }));
    let source = lowered.files.text(lowered.file);
    call_steps(module, body, body.root)
        .into_iter()
        .map(|step| match step {
            EvalStep::Eval(expr) => format!("eval {}", &source[body.exprs[expr].range]),
            EvalStep::Arrow(index) => format!("arrow {index}"),
        })
        .collect()
}

#[test]
fn arguments_up_to_the_arity_are_passed_together() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = f 1 (g ())"),
        ["eval f", "eval 1", "eval g ()", "arrow 0", "arrow 1"]
    );
}

#[test]
fn an_argument_beyond_the_arity_waits_for_the_call() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = h 1 (g ())"),
        ["eval h", "eval 1", "arrow 0", "eval g ()", "arrow 1"]
    );
}

#[test]
fn parentheses_do_not_change_the_order() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = (h 1) (g ())"),
        ["eval h", "eval 1", "arrow 0", "eval g ()", "arrow 1"]
    );
}

#[test]
fn a_value_beyond_the_arity_is_passed_together() {
    assert_eq!(
        steps("t : Int -> Int\nt x = h 1 x"),
        ["eval h", "eval 1", "eval x", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_function_value_is_applied_before_a_later_argument() {
    assert_eq!(
        steps("t : (Int -> Int -> Int) -> Int\nt k = k 1 (g ())"),
        ["eval k", "eval 1", "arrow 0", "eval g ()", "arrow 1"]
    );
}

#[test]
fn a_top_level_value_is_not_a_known_callee() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = w 1 (g ())"),
        ["eval w", "eval 1", "arrow 0", "eval g ()", "arrow 1"]
    );
}

#[test]
fn the_left_of_a_pipe_is_evaluated_first() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = g () |> f 1"),
        ["eval g ()", "eval f", "eval 1", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_nested_pipe_is_a_callee() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = 1 |> (2 |> f)"),
        ["eval 1", "eval 2 |> f", "arrow 0"]
    );
}

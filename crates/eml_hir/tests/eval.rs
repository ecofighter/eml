//! 呼び出しの評価の手順 (docs/spec/expressions.md の「関数適用」)。引数をすべて評価してから、矢印を順に適用する。

use eml_hir::{EvalStep, ExprKind, Res, call_steps};
use eml_test_support::lower_clean;

const PRELUDE: &str = "f : Int -> Int -> Int\nf a b = a\n\ng : Unit -> Int\ng () = 1\n\nh : Int -> (Int -> Int)\nh a = fn b -> a + b\n\nw : Int -> Int -> Int\nw = fn a b -> a\n\nv : Int\nv = g ()\n\neffect Combine where\n  combine : Int -> Int -> Int\n\n";

/// 呼び出しの手順を、評価する部分式のソースと、適用する矢印の番号で表す。
fn render(source: &str, body: &eml_hir::Body, steps: Vec<EvalStep>) -> Vec<String> {
    steps
        .into_iter()
        .map(|step| match step {
            EvalStep::Eval(expr) => format!("eval {}", &source[body.exprs[expr].range]),
            EvalStep::Arrow(index) => format!("arrow {index}"),
        })
        .collect()
}

/// 関数 `t` の本体 (呼び出し) の手順。
fn steps(t: &str) -> Vec<String> {
    let lowered = lower_clean(&format!("{PRELUDE}{t}"));
    let program = &lowered.program;
    let (id, _) = program
        .functions()
        .find(|(_, function)| function.name == "t")
        .unwrap();
    let body = program.body(id).unwrap();
    assert!(matches!(body.exprs[body.root].kind, ExprKind::Call { .. }));
    let source = lowered.files().text(lowered.file());
    render(source, body, call_steps(body, body.root))
}

#[test]
fn arguments_up_to_the_arity_are_passed_together() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = f 1 (g ())"),
        ["eval f", "eval 1", "eval g ()", "arrow 0", "arrow 1"]
    );
}

/// 引数の数を超える引数も、矢印を適用する前に評価する。値かどうかで手順は変わらない。
#[test]
fn an_argument_beyond_the_arity_is_evaluated_before_the_call() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = h 1 (g ())"),
        ["eval h", "eval 1", "eval g ()", "arrow 0", "arrow 1"]
    );
    assert_eq!(
        steps("t : Int -> Int\nt x = h 1 x"),
        ["eval h", "eval 1", "eval x", "arrow 0", "arrow 1"]
    );
}

#[test]
fn parentheses_do_not_change_the_order() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = (h 1) (g ())"),
        ["eval h", "eval 1", "eval g ()", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_function_value_takes_all_its_arguments_after_they_are_evaluated() {
    assert_eq!(
        steps("t : (Int -> Int -> Int) -> Int\nt k = k 1 (g ())"),
        ["eval k", "eval 1", "eval g ()", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_top_level_value_is_evaluated_like_any_callee() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = w 1 (g ())"),
        ["eval w", "eval 1", "eval g ()", "arrow 0", "arrow 1"]
    );
    assert_eq!(
        steps("t : Unit -> Int\nt () = h 1 v"),
        ["eval h", "eval 1", "eval v", "arrow 0", "arrow 1"]
    );
}

#[test]
fn arguments_of_an_operation_are_passed_together() {
    assert_eq!(
        steps("t : Unit -> <Combine> Int\nt () = combine 1 (g ())"),
        ["eval combine", "eval 1", "eval g ()", "arrow 0", "arrow 1"]
    );
}

#[test]
fn the_left_of_a_pipe_is_evaluated_first() {
    // `x |> f 1` は `(|>) x (f 1)` の呼び出しで、`x` を `f` と `1` より先に評価する
    assert_eq!(
        steps("t : Unit -> Int\nt () = g () |> f 1"),
        ["eval |>", "eval g ()", "eval f 1", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_nested_pipe_is_an_argument() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = 1 |> (2 |> f)"),
        ["eval |>", "eval 1", "eval 2 |> f", "arrow 0", "arrow 1"]
    );
}

/// 関数 `t` の本体のうち、節の `k` を呼ぶ呼び出しの手順。
fn continuation_steps(t: &str) -> Vec<String> {
    let lowered = lower_clean(&format!("{PRELUDE}{t}"));
    let program = &lowered.program;
    let (id, _) = program
        .functions()
        .find(|(_, function)| function.name == "t")
        .unwrap();
    let body = program.body(id).unwrap();
    let source = lowered.files().text(lowered.file());
    let (call, _) = body
        .exprs
        .iter()
        .find(|(_, expr)| match &expr.kind {
            ExprKind::Call { callee, args: _ } => matches!(
                body.exprs[*callee].kind,
                ExprKind::Path(Res::Local(local)) if body.continuations.contains_idx(local)
            ),
            _ => false,
        })
        .unwrap();
    render(source, body, call_steps(body, call))
}

/// 状態のある handler の `k st (st + 1)` は、引数をすべて評価してから1回で呼ぶ手順になる。
#[test]
fn a_continuation_is_called_with_all_its_arguments_at_once() {
    assert_eq!(
        continuation_steps(
            "effect Tick where\n  tick : Unit -> Int\n\nt : Unit -> Int\nt () =\n  handle tick () from 0 with\n    | tick () k st -> k st (st + 1)\n    | return x _ -> x"
        ),
        ["eval k", "eval st", "eval st + 1", "arrow 0", "arrow 1"]
    );
}

/// 型を明示した節の `(k : …)` も、継続の引数の数を記録する。
#[test]
fn an_annotated_clause_pattern_records_the_continuation_arity() {
    assert_eq!(
        continuation_steps(
            "effect Tick where\n  tick : Unit -> Int\n\nt : Unit -> Int\nt () =\n  handle tick () from 0 with\n    | tick () (k : Int -> Int -> Int) st -> k st (st + 1)\n    | return x _ -> x"
        ),
        ["eval k", "eval st", "eval st + 1", "arrow 0", "arrow 1"]
    );
}

/// 状態のない handler の `k` は引数の数が1なので、`k 1 (g ())` は `g ()` を評価してから、再開と余った引数の適用を順に行う。
#[test]
fn an_argument_beyond_the_continuation_arity_is_evaluated_before_the_resume() {
    assert_eq!(
        continuation_steps(
            "effect Ask where\n  ask : Unit -> Int\n\nt : Unit -> Int\nt () =\n  let f =\n    handle ask () with\n      | ask () k -> k 1 (g ())\n      | return x -> fn y -> x + y\n  f 2"
        ),
        ["eval k", "eval 1", "eval g ()", "arrow 0", "arrow 1"]
    );
}

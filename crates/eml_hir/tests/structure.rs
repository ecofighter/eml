//! HIR のデータ構造と走査関数のテスト。

use eml_hir::{Body, ExprId, ExprKind, Function, LineStart, LocalId, Module, PatKind, TypeRefKind};

/// 診断がないことを確かめて HIR を返す。
fn module(text: &str) -> Module {
    eml_test_support::lower_clean(text).module
}

fn function<'m>(module: &'m Module, name: &str) -> &'m Function {
    module
        .functions
        .iter()
        .map(|(_, function)| function)
        .find(|function| function.name == name)
        .expect("the function")
}

#[test]
fn signature_and_body_annotations_live_in_separate_arenas() {
    // 本体を書き換えても、シグネチャのアリーナは変わらない。本体の注釈の型変数は、シグネチャの表を指す
    let module = module("f : a -> a\nf x = (x : a)");
    let f = function(&module, "f");
    let signature = f.signature.as_ref().expect("a signature");
    assert_eq!(signature.generics.type_vars.len(), 1);
    let body = f.body.as_ref().expect("a body");
    assert_eq!(body.types.len(), 1);
    let (_, annotation) = body.types.iter().next().expect("an annotation");
    let TypeRefKind::Var(var) = annotation.kind else {
        panic!("a type variable");
    };
    assert_eq!(signature.generics.type_vars[var].name, "a");
}

fn body<'m>(module: &'m Module, name: &str) -> &'m Body {
    function(module, name).body.as_ref().expect("a body")
}

fn children(body: &Body, id: ExprId) -> Vec<ExprId> {
    let mut out = Vec::new();
    body.walk_child_exprs(id, |child| out.push(child));
    out
}

/// 引数の名前が `param` のラムダ。
fn lambda(body: &Body, param: &str) -> ExprId {
    body.exprs
        .iter()
        .find(|(_, expr)| match &expr.kind {
            ExprKind::Lambda(closure) => closure.params.iter().any(|&pat| {
                body.pat_bindings(pat)
                    .iter()
                    .any(|&local| body.locals[local].name == param)
            }),
            _ => false,
        })
        .map(|(id, _)| id)
        .expect("the lambda")
}

fn lambda_closure<'a>(body: &'a Body, param: &str) -> &'a eml_hir::Closure {
    let ExprKind::Lambda(closure) = &body.exprs[lambda(body, param)].kind else {
        unreachable!();
    };
    closure
}

fn names(body: &Body, locals: &[LocalId]) -> Vec<String> {
    locals
        .iter()
        .map(|&local| body.locals[local].name.clone())
        .collect()
}

#[test]
fn child_expressions_include_let_initializers_and_lambda_bodies() {
    let module = module("f : Int -> Int\nf x =\n  let y = x + 1\n  (fn z -> z) y");
    let body = body(&module, "f");
    let root = children(body, body.root);
    assert_eq!(root.len(), 2, "the let initializer and the tail");
    assert!(matches!(body.exprs[root[0]].kind, ExprKind::Call { .. }));
    let call = children(body, root[1]);
    let ExprKind::Lambda(eml_hir::Closure {
        body: lambda_body, ..
    }) = body.exprs[call[0]].kind
    else {
        panic!("the callee is the lambda");
    };
    assert_eq!(children(body, call[0]), [lambda_body]);
}

#[test]
fn bindings_look_inside_annotated_patterns() {
    let module = module("f : Int -> Int\nf x = (fn (y : Int) -> y) x");
    let body = body(&module, "f");
    let ExprKind::Lambda(closure) = &body.exprs[lambda(body, "y")].kind else {
        unreachable!();
    };
    let params = &closure.params;
    assert!(matches!(body.pats[params[0]].kind, PatKind::Annot { .. }));
    assert_eq!(names(body, &body.pat_bindings(params[0])), ["y"]);
}

#[test]
fn a_lambda_captures_what_its_nested_lambdas_capture() {
    // 入れ子のラムダが捕まえる変数は、内側のクロージャを作る外側のラムダも捕まえる (docs/spec/core-ir.md)
    let text = "f : Int -> Int -> Int\nf a b =\n  let g = fn x ->\n    let c = x + b\n    fn y -> a + c + y\n  g 1 2";
    let module = module(text);
    let body = body(&module, "f");
    assert_eq!(
        names(body, &body.closure_captures(lambda_closure(body, "x"))),
        ["a", "b"]
    );
    assert_eq!(
        names(body, &body.closure_captures(lambda_closure(body, "y"))),
        ["a", "c"]
    );
}

#[test]
fn prelude_signatures_without_equations_are_intrinsic_functions() {
    let module = module("f : Int\nf = 1");
    for name in ["println", "show_int", "negate", "+", "==", ">>", "&&", "|>"] {
        let function = function(&module, name);
        assert!(function.intrinsic, "{name}");
        assert!(function.signature.is_some(), "{name}");
        assert!(function.body.is_none(), "{name}");
    }
    assert!(!function(&module, "f").intrinsic);
    assert_eq!(function(&module, "+").arity(), Some(2));
    assert_eq!(function(&module, ">>").arity(), Some(3));
}

#[test]
fn internal_builtins_cannot_be_named() {
    let lowered = eml_test_support::lower("f : Int -> Int\nf x = negate x");
    let codes: Vec<String> = lowered
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E1001"]);
}

#[test]
fn a_block_records_where_its_last_line_starts() {
    // fix が最後の文の前に行を入れるので、その位置と字下げを持つ (docs/spec/diagnostics.md の「線形性の診断」)
    let module = module("f : Int -> Int\nf x =\n  let y = x\n  y");
    let body = function(&module, "f").body.as_ref().expect("a body");
    let ExprKind::Block { last_line, .. } = &body.exprs[body.root].kind else {
        panic!("the body is a block");
    };
    assert_eq!(
        *last_line,
        Some(LineStart {
            offset: 35.into(),
            indent: 2,
        })
    );
}

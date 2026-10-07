//! HIR のデータ構造と走査関数のテスト。

use eml_hir::{
    Body, ExprId, ExprKind, Function, FunctionId, LineStart, LocalId, PatKind, Program, TypeRefKind,
};

/// 診断がないことを確かめて HIR を返す。
fn module(text: &str) -> Program {
    eml_test_support::lower_clean(text).program
}

fn function_id(program: &Program, name: &str) -> FunctionId {
    program
        .functions()
        .find(|(_, function)| function.name == name)
        .map(|(id, _)| id)
        .expect("the function")
}

fn function<'m>(program: &'m Program, name: &str) -> &'m Function {
    &program[function_id(program, name)]
}

#[test]
fn signature_and_body_annotations_live_in_separate_arenas() {
    // 本体を書き換えても、シグネチャのアリーナは変わらない。本体の注釈の型変数は、シグネチャの表を指す
    let module = module("f : a -> a\nf x = (x : a)");
    let f = function(&module, "f");
    let signature = f.signature.as_ref().expect("a signature");
    assert_eq!(signature.generics.type_vars.len(), 1);
    let body = body(&module, "f");
    assert_eq!(body.types.len(), 1);
    let (_, annotation) = body.types.iter().next().expect("an annotation");
    let TypeRefKind::Var(var) = annotation.kind else {
        panic!("a type variable");
    };
    assert_eq!(signature.generics.type_vars[var].name, "a");
}

fn body<'m>(program: &'m Program, name: &str) -> &'m Body {
    program.body(function_id(program, name)).expect("a body")
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
    for name in ["show_int", "negate", "+", "=="] {
        let function = function(&module, name);
        assert!(function.intrinsic, "{name}");
        assert!(function.signature.is_some(), "{name}");
        assert!(module.body(function_id(&module, name)).is_none(), "{name}");
    }
    // eml で書いた Prelude の関数は intrinsic ではない (docs/spec/declarations.md の標準の演算子の表)
    for name in ["not", "&&", "||", ">>", "<<", "|>", "<|"] {
        assert!(!function(&module, name).intrinsic, "{name}");
        assert!(module.body(function_id(&module, name)).is_some(), "{name}");
    }
    assert!(!function(&module, "f").intrinsic);
    assert_eq!(module.arity(function_id(&module, "+")), Some(2));
    assert_eq!(module.arity(function_id(&module, ">>")), Some(2));
}

#[test]
fn prelude_functions_with_equations_are_not_intrinsic() {
    // Prelude の関数は本体を持てる (docs/spec/declarations.md の標準の演算子の表)。intrinsic は等式の
    // ないシグネチャだけである
    let prelude = format!(
        "{}\npub twice : Int -> Int\ntwice x = x + x\n",
        eml_hir::PRELUDE_SOURCE
    );
    let lowered = eml_test_support::lower_with_std(&[("Prelude.em", &prelude)], "");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let program = lowered.program;
    let twice = function_id(&program, "twice");
    assert!(!program[twice].intrinsic);
    assert!(program.body(twice).is_some());
    assert!(program[function_id(&program, "show_int")].intrinsic);
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
    // fix が最後の文の前に行を入れるので、その位置と字下げを持つ
    // (docs/implementation/diagnostics.md の「線形性の診断」)
    let module = module("f : Int -> Int\nf x =\n  let y = x\n  y");
    let body = body(&module, "f");
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

#[test]
fn the_prelude_and_the_entry_are_separate_modules() {
    let lowered = eml_test_support::lower_clean("f : Int\nf = 1");
    let program = &lowered.program;
    let prelude = &program.modules[program.prelude];
    let entry = &program.modules[program.entry];
    assert_eq!(prelude.name, "Prelude");
    assert_eq!(entry.name, "Main");
    assert_eq!(lowered.files.path(prelude.file), eml_hir::PRELUDE_PATH);
    assert_eq!(entry.file, lowered.file);
    // Prelude の item は Prelude のモジュールに、ユーザーの関数は入口のモジュールにある
    let (plus, _) = program.functions().find(|(_, f)| f.name == "+").unwrap();
    let (f, _) = program.functions().find(|(_, f)| f.name == "f").unwrap();
    assert_eq!(plus.module, program.prelude);
    assert_eq!(f.module, program.entry);
    assert_eq!(program.lang.bool.module, program.prelude);
    // 本体は入口のモジュールの表にある
    assert!(program.body(f).is_some());
    assert!(program.body(plus).is_none());
    assert_eq!(program.arity(plus), Some(2));
    // `f` は引数のない値である
    assert_eq!(program.arity(f), Some(0));
}

#[test]
fn imported_modules_follow_the_entry() {
    let lowered = eml_test_support::lower_files(
        "import Report.Csv\n\nf : Int\nf = 1",
        &[("Report/Csv.em", "pub g : Int\ng = 2")],
    );
    assert!(
        lowered.diagnostics.is_empty(),
        "{}",
        eml_test_support::short_text(&lowered.files, &lowered.diagnostics)
    );
    let program = &lowered.program;
    let names: Vec<&str> = program
        .modules
        .iter()
        .map(|(_, module)| module.name.as_str())
        .collect();
    assert_eq!(names, ["Prelude", "Main", "Report.Csv"]);
    let g = function_id(program, "g");
    assert_eq!(lowered.files.path(program.file(g.module)), "Report/Csv.em");
    assert!(program.body(g).is_some());
}

#[test]
fn main_is_the_entry_function_named_main() {
    let program = module("f : Int\nf = 1\n\nmain : Unit -> <IO> Unit\nmain () = ()");
    let main = program.main().expect("main");
    assert_eq!(main.module, program.entry);
    assert_eq!(main, function_id(&program, "main"));
}

#[test]
fn a_program_without_main_has_no_entry_function() {
    assert_eq!(module("f : Int\nf = 1").main(), None);
}

#[test]
fn an_operation_named_main_is_not_the_entry_function() {
    assert_eq!(module("effect E where\n  main : Unit -> Unit").main(), None);
}

#[test]
fn a_main_in_the_prelude_is_not_the_entry_function() {
    // `main` は入口のモジュールからだけ探す (docs/implementation/architecture.md の「CLI と lib API」)
    let prelude = format!(
        "{}\nmain : Unit -> <IO> Unit\nmain () = ()\n",
        eml_hir::PRELUDE_SOURCE
    );
    let lowered = eml_test_support::lower_with_std(&[("Prelude.em", &prelude)], "");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let program = lowered.program;
    assert!(
        program
            .functions()
            .any(|(_, function)| function.name == "main")
    );
    assert_eq!(program.main(), None);
}

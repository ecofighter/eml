//! HIR のデータ構造と走査関数のテスト。

use eml_hir::{Function, Module, TypeRefKind};

/// 診断のエラーがないことを確かめて HIR を返す。
fn module(text: &str) -> Module {
    let lowered = eml_test_support::lower(text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    lowered.module
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

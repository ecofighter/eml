//! テストで推論結果を確かめるための表示。

use std::fmt::Write;

use eml_hir::Program;

use crate::context::Context;
use crate::kind::Bound;
use crate::kind::problem::KindScheme;
use crate::shape::Shape;
use crate::ty::{KindConstraint, KindTerm, Linearity, Multiplicity, RowTerm};
use crate::{Decl, DeclType, TypedProgram};

/// 入口のモジュールだけを表示する。Prelude はどのプログラムにもあるので、テストの表示を Prelude に左右させないため。
pub fn dump(program: &Program, typed: &TypedProgram) -> String {
    // Kind の制約の表示に型の名前が要る。テストの表示にしか使わないので、`Context` を作り直す費用は問題にしない
    let context = Context::new(program);
    let mut out = String::new();
    for (id, operation) in program.operations() {
        if id.module != program.entry {
            continue;
        }
        if let Some(declared) = typed.decls.get(&Decl::Operation(id)) {
            writeln!(out, "{} : {}", operation.name, declared.ty).unwrap();
            write_kinds(&mut out, &context, declared);
        }
    }
    for (id, function) in program.functions() {
        if id.module != program.entry {
            continue;
        }
        if let Some(declared) = typed.decls.get(&Decl::Function(id)) {
            writeln!(out, "{} : {}", function.name, declared.ty).unwrap();
            write_kinds(&mut out, &context, declared);
        }
        let (Some(body), Some(types)) = (program.body(id), typed.bodies.get(id)) else {
            continue;
        };
        for (local, data) in body.locals.iter() {
            if let Some(ty) = types.locals.get(local) {
                writeln!(
                    out,
                    "  {}#{} : {ty}",
                    data.name,
                    u32::from(local.into_raw())
                )
                .unwrap();
            }
        }
    }
    out
}

fn write_kinds(out: &mut String, context: &Context, declared: &DeclType) {
    let constraints = kind_constraints(context, &declared.shape, &declared.kinds);
    if !constraints.is_empty() {
        let kinds: Vec<String> = constraints.iter().map(ToString::to_string).collect();
        writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
    }
}

/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(context: &Context, shape: &Shape, scheme: &KindScheme) -> Vec<KindConstraint> {
    let names = shape.kind_names(context);
    let rows = shape.row_names();
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).cloned().map(KindTerm::Of),
    };
    let row_term = |bound: Bound<Multiplicity>| match bound {
        Bound::Const(Multiplicity::Multi) => Some(RowTerm::Multi),
        Bound::Const(_) => None,
        Bound::Var(var) => rows.get(&var).cloned().map(RowTerm::Of),
    };
    let mut constraints: Vec<KindConstraint> = scheme
        .lin
        .iter()
        .filter(|(lower, upper)| {
            matches!(lower, Bound::Const(_)) != matches!(upper, Bound::Const(_))
        })
        .filter_map(|&(lower, upper)| {
            Some(KindConstraint::Linearity {
                lower: term(lower)?,
                upper: term(upper)?,
            })
        })
        .collect();
    for carry in &scheme.carries {
        let (Some(value), Some(row)) = (term(carry.lin), row_term(carry.mult)) else {
            continue;
        };
        let constraint = KindConstraint::Carry { value, row };
        if !constraints.contains(&constraint) {
            constraints.push(constraint);
        }
    }
    constraints
}

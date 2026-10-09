//! テストで推論結果を確かめるための表示。

use std::fmt::Write;

use eml_hir::{DisplayNames, ModuleId, ModuleOrigin, Program, ValueItem};

use crate::kind::Bound;
use crate::kind::problem::KindScheme;
use crate::shape::Shape;
use crate::store::TypeStore;
use crate::ty::{KindConstraint, KindTerm, Linearity, Multiplicity, RowTerm};
use crate::{DeclType, TypedProgram};

/// ユーザーのモジュールを、モジュールの番号の順に表示する。標準ライブラリのモジュールはどのプログラムにもあるので、テストの
/// 表示を標準ライブラリに左右させないため。モジュールが2つ以上なら、`eml_hir::pretty` と同じく各モジュールの前に `-- 名前`
/// の見出しを付ける。
pub fn dump(program: &Program, typed: &TypedProgram) -> String {
    let names = &program.names;
    // `Shape::kind_names` が表示のために作る型を登録するので、型検査の結果の表は変えずに複製する
    let mut types = typed.types.clone();
    let modules: Vec<ModuleId> = program
        .modules
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| program.origin(id) == ModuleOrigin::User)
        .collect();
    let mut out = String::new();
    for &module in &modules {
        if modules.len() > 1 {
            writeln!(out, "-- {}", program.modules[module].name).unwrap();
        }
        for (id, operation) in program.operations().filter(|(id, _)| id.module == module) {
            if let Some(declared) = typed.decls.get(&ValueItem::Operation(id)) {
                let ty = types.display(declared.ty, names).to_string();
                writeln!(out, "{} : {ty}", operation.name).unwrap();
                write_kinds(&mut out, &mut types, names, declared);
            }
        }
        for (id, function) in program.functions().filter(|(id, _)| id.module == module) {
            if let Some(declared) = typed.decls.get(&ValueItem::Function(id)) {
                let ty = types.display(declared.ty, names).to_string();
                writeln!(out, "{} : {ty}", function.name).unwrap();
                write_kinds(&mut out, &mut types, names, declared);
            }
            let (Some(body), Some(body_types)) = (program.body(id), typed.bodies.get(id)) else {
                continue;
            };
            for (local, data) in body.locals.iter() {
                if let Some(ty) = body_types.locals.get(local) {
                    writeln!(
                        out,
                        "  {}#{} : {}",
                        data.name,
                        u32::from(local.into_raw()),
                        types.display(*ty, names)
                    )
                    .unwrap();
                }
            }
        }
    }
    out
}

fn write_kinds(out: &mut String, types: &mut TypeStore, names: &DisplayNames, declared: &DeclType) {
    let constraints = kind_constraints(types, &declared.shape, &declared.kinds);
    if !constraints.is_empty() {
        let kinds: Vec<String> = constraints
            .iter()
            .map(|constraint| constraint.show(types, names))
            .collect();
        writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
    }
}

/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(
    types: &mut TypeStore,
    shape: &Shape,
    scheme: &KindScheme,
) -> Vec<KindConstraint> {
    let names = shape.kind_names(types);
    let rows = shape.row_names();
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).copied().map(KindTerm::Of),
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

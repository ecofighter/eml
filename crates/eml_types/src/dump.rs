//! テストで推論結果を確かめるための表示。

use std::fmt::Write;

use eml_hir::{DisplayNames, ModuleId, Program, ValueItem};

use crate::kind::Bound;
use crate::kind::problem::KindScheme;
use crate::shape::Shape;
use crate::store::{TypeId, TypeStore};
use crate::ty::{KindConstraint, KindTerm, Linearity, Multiplicity, RowTerm};
use crate::{DeclType, TypedProgram};

/// ユーザーのモジュールを、モジュールの番号の順に表示する。標準ライブラリのモジュールはどのプログラムにもあるので、テストの
/// 表示を標準ライブラリに左右させないため。モジュールが2つ以上なら、`eml_hir::pretty` と同じく各モジュールの前に `-- 名前`
/// の見出しを付ける。
pub fn dump(program: &Program, typed: &TypedProgram) -> String {
    write_dump(program, typed, false)
}

/// `dump` と同じ並びで、型に線形性を書く。矢印の `Lin` は `%1`、`Poly` は `%m`、宣言の Kind 変数 `Var(i)` は `%m{i}` と
/// 書き、`Flexible` と `OpVar` の Kind も名前の後に同じ印で書く。宣言の `joins` があれば、`joins: %m2 = %m0 | %m1` の
/// 行を足す。書き出した線形性を単体テストで読めるようにするためのもので、診断や `dump` の出力は変えない。
pub fn dump_linear(program: &Program, typed: &TypedProgram) -> String {
    write_dump(program, typed, true)
}

fn write_dump(program: &Program, typed: &TypedProgram, linear: bool) -> String {
    let names = &program.names;
    // `Shape::kind_names` が表示のために作る型を登録するので、型検査の結果の表は変えずに複製する
    let mut types = typed.types.clone();
    let modules: Vec<ModuleId> = program
        .modules
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| !program.origin(id).is_std())
        .collect();
    let mut out = String::new();
    for &module in &modules {
        if modules.len() > 1 {
            writeln!(out, "-- {}", program.modules[module].name).unwrap();
        }
        let declarations = program
            .operations()
            .filter(|(id, _)| id.module == module)
            .map(|(id, _)| ValueItem::Operation(id))
            .chain(
                program
                    .methods()
                    .filter(|(id, _)| id.module == module)
                    .map(|(id, _)| ValueItem::Method(id)),
            );
        for decl in declarations {
            write_decl(&mut out, &mut types, program, decl, typed, linear);
        }
        for (id, _) in program.functions().filter(|(id, _)| id.module == module) {
            write_decl(
                &mut out,
                &mut types,
                program,
                ValueItem::Function(id),
                typed,
                linear,
            );
            let (Some(body), Some(body_types)) = (program.body(id), typed.bodies.get(id)) else {
                continue;
            };
            for (local, data) in body.locals.iter() {
                if let Some(&ty) = body_types.locals.get(local) {
                    writeln!(
                        out,
                        "  {}#{} : {}",
                        data.name,
                        u32::from(local.into_raw()),
                        show(&types, ty, names, linear)
                    )
                    .unwrap();
                }
            }
        }
    }
    out
}

fn show(types: &TypeStore, ty: TypeId, names: &DisplayNames, linear: bool) -> String {
    if linear {
        types.display_kinds(ty, names).to_string()
    } else {
        types.display(ty, names).to_string()
    }
}

/// 宣言の型と、Kind の制約があればその行を書く。`linear` なら `joins` の行も書く。型のない宣言は書かない。
fn write_decl(
    out: &mut String,
    types: &mut TypeStore,
    program: &Program,
    decl: ValueItem,
    typed: &TypedProgram,
    linear: bool,
) {
    let Some(declared) = typed.decls.get(&decl) else {
        return;
    };
    let names = &program.names;
    let ty = show(types, declared.ty, names, linear);
    writeln!(out, "{} : {ty}", program.value_name(decl)).unwrap();
    write_kinds(out, types, names, declared);
    if linear && !declared.joins.is_empty() {
        let first = declared.shape.lin_vars;
        let joins: Vec<String> = declared
            .joins
            .iter()
            .enumerate()
            .map(|(k, set)| {
                let vars: Vec<String> = set.iter().map(|var| format!("%m{var}")).collect();
                format!("%m{} = {}", first + k, vars.join(" | "))
            })
            .collect();
        writeln!(out, "  joins: {}", joins.join(", ")).unwrap();
    }
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

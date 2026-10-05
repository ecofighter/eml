use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::builtin::{BUILTINS, Builtin};
use eml_hir::{FunctionId, Generics, Module, OperationId, RowRef, TypeRef, TypeRefId, TypeRefKind};
use la_arena::Arena;
use la_arena::ArenaMap;

use crate::kind::{Bound, KindVar};
use crate::scheme::{Rigids, Scheme, lower_operation, lower_signature};
use crate::table::{Row, Table, TyShape};
use crate::ty::{EffectLabel, KindConstraint, KindTerm, Linearity, Type};
use crate::{BodyTypes, TypedModule, codes, scc, usage};

mod body;
mod handle;
mod report;

use body::BodyCheck;
pub(crate) use body::BodyTyping;
use report::AmbientSource;

pub(crate) fn check_module(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    let mut table = Table::new(
        module.lang,
        &module.types,
        &module.effects,
        &module.operations,
    );
    // 組み込みの型は Prelude のシグネチャから、ユーザーの関数と同じ経路で作る。本体がないので、作ってすぐ多相化する
    let mut builtins: HashMap<Builtin, Scheme> = HashMap::new();
    for info in BUILTINS {
        let Some(signature) = module.builtins.get(&info.builtin) else {
            continue;
        };
        let rigids = Rigids::new(&mut table, &signature.generics);
        let ty = lower_signature(&mut table, signature, &rigids);
        table.closure_kinds(ty, info.arity, &[]);
        let mut scheme = Scheme::new(ty, &rigids);
        scheme.generalize(&table);
        builtins.insert(info.builtin, scheme);
    }
    // 操作は本体を持たない値なので、組み込みと同じく作ってすぐ多相化する
    let mut operations: ArenaMap<OperationId, Scheme> = ArenaMap::default();
    for (id, operation) in module.operations.iter() {
        let rigids = Rigids::new(&mut table, &operation.signature.generics);
        let ty = lower_operation(&mut table, operation, &rigids);
        table.closure_kinds(ty, operation.arity, &[]);
        // エフェクトの型引数は handle ごとに具体的な型で節を検査するので、`Unr` に固定しない
        let effect_kinds: Vec<KindVar> = rigids
            .effect_args(operation)
            .into_iter()
            .flat_map(|ty| table.kind_bounds(ty))
            .filter_map(|bound| match bound {
                Bound::Var(var) => Some(var),
                Bound::Const(_) => None,
            })
            .collect();
        // 結果の型は縛らない。`never fail : String -> a` をどの型としても使えるようにするため
        let mut spine = ty;
        for _ in 0..operation.arity {
            let TyShape::Fn { param, ret, .. } = table.shape(spine).clone() else {
                break;
            };
            table.unrestricted(param, &effect_kinds);
            spine = ret;
        }
        let mut scheme = Scheme::new(ty, &rigids);
        scheme.generalize(&table);
        operations.insert(id, scheme);
    }
    let mut diagnostics = Vec::new();
    let mut rigids = ArenaMap::default();
    let mut schemes: ArenaMap<FunctionId, Scheme> = ArenaMap::default();
    for (id, function) in module.functions.iter() {
        let no_generics = Generics::default();
        let generics = function
            .signature
            .as_ref()
            .map_or(&no_generics, |signature| &signature.generics);
        let function_rigids = Rigids::new(&mut table, generics);
        if let Some(signature) = &function.signature {
            let ty = lower_signature(&mut table, signature, &function_rigids);
            // 部分適用のクロージャは、それまでの引数を捕まえる (docs/spec/types.md の「関数型」)
            if let Some(body) = &function.body {
                table.closure_kinds(ty, body.params.len(), &[]);
            }
            schemes.insert(id, Scheme::new(ty, &function_rigids));
        }
        rigids.insert(id, function_rigids);
    }
    let main = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "main")
        .map(|(id, _)| id);
    if let Some(id) = main {
        check_main(module, &table, &schemes, id, &mut diagnostics);
    }
    let mut bodies = Vec::new();
    // 呼ばれる側の SCC から順に検査し、SCC ごとに Kind を多相化する (docs/spec/types.md の「推論」)
    for component in scc::components(module) {
        for &id in &component {
            let function = &module.functions[id];
            let (Some(scheme), Some(body)) = (schemes.get(id), &function.body) else {
                continue;
            };
            let signature = scheme.ty;
            let mut checker = BodyCheck {
                module,
                function,
                body,
                rigids: &rigids[id],
                schemes: &schemes,
                builtins: &builtins,
                operations: &operations,
                table: &mut table,
                diagnostics: &mut diagnostics,
                ambient: Row::pure(),
                ambient_source: AmbientSource::Signature,
                typing: BodyTyping::default(),
            };
            checker.check_function(signature);
            let typing = checker.typing;
            usage::constrain(body, &typing, &mut table);
            bodies.push((id, typing));
        }
        for &id in &component {
            if let Some(scheme) = schemes.get_mut(id) {
                scheme.generalize(&table);
            }
        }
    }
    // Kind の制約の違反は、線形な値の誤った使い方である (docs/spec/linearity.md)
    for origin in table.solve_kinds() {
        diagnostics.push(report::linear_misuse(module.file, &origin));
    }
    let mut typed = TypedModule {
        main,
        ..TypedModule::default()
    };
    for (id, scheme) in schemes.iter() {
        typed.signatures.insert(
            id,
            crate::Scheme {
                ty: table.export(scheme.ty),
                constraints: kind_constraints(&table, scheme),
            },
        );
    }
    for (&builtin, scheme) in &builtins {
        typed.builtins.insert(
            builtin,
            crate::Scheme {
                ty: table.export(scheme.ty),
                constraints: kind_constraints(&table, scheme),
            },
        );
    }
    for (id, scheme) in operations.iter() {
        typed.operations.insert(
            id,
            crate::Scheme {
                ty: table.export(scheme.ty),
                constraints: kind_constraints(&table, scheme),
            },
        );
    }
    for (id, typing) in bodies {
        let mut types = BodyTypes::default();
        for (expr, &ty) in typing.exprs.iter() {
            types.exprs.insert(expr, table.export(ty));
        }
        for (local, &ty) in typing.locals.iter() {
            types.locals.insert(local, table.export(ty));
        }
        for (pat, &ty) in typing.pats.iter() {
            types.pats.insert(pat, table.export(ty));
        }
        typed.bodies.insert(id, types);
    }
    (typed, diagnostics)
}

fn check_main(
    module: &Module,
    table: &Table,
    schemes: &ArenaMap<FunctionId, Scheme>,
    id: FunctionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &module.functions[id];
    let (Some(scheme), Some(signature)) = (schemes.get(id), &function.signature) else {
        return;
    };
    // 未定義のエフェクトや解決できなかった型変数・row 変数の跡から E2004 を連鎖させないため
    // (docs/spec/types.md の「エラーの扱い」)
    if has_error(&signature.types, signature.ty) {
        return;
    }
    let found = table.display(scheme.ty);
    let expected = Type::Fn {
        param: Box::new(Type::unit()),
        linearity: Linearity::Unr,
        effects: vec![EffectLabel {
            id: module.lang.io,
            name: module.effects[module.lang.io].name.clone(),
            args: Vec::new(),
        }],
        tail: None,
        ret: Box::new(Type::unit()),
    };
    if !found.contains_error() && found != expected {
        diagnostics.push(Diagnostic::error(
            codes::INVALID_MAIN_TYPE,
            "`main` must have type `Unit -> <IO> Unit`",
            Label::new(module.file, signature.range, format!("found `{found}`")),
        ));
    }
}

fn has_error(types: &Arena<TypeRef>, id: TypeRefId) -> bool {
    match &types[id].kind {
        TypeRefKind::Error => true,
        TypeRefKind::Con(_, args) => args.iter().any(|&arg| has_error(types, arg)),
        TypeRefKind::Var(_) => false,
        TypeRefKind::Fn { param, row, ret } => {
            let args_error = match row {
                RowRef::Closed { effects, .. } | RowRef::Open { effects, .. } => effects
                    .iter()
                    .any(|effect| effect.args.iter().any(|&arg| has_error(types, arg))),
                RowRef::Omitted | RowRef::Error => false,
            };
            matches!(row, RowRef::Error)
                || args_error
                || has_error(types, *param)
                || has_error(types, *ret)
        }
    }
}

/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(table: &Table, scheme: &Scheme) -> Vec<KindConstraint> {
    let names = table.kind_names(scheme.ty);
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).cloned().map(KindTerm::Of),
    };
    scheme
        .lin_constraints()
        .iter()
        .filter(|(lower, upper)| {
            matches!(lower, Bound::Const(_)) != matches!(upper, Bound::Const(_))
        })
        .filter_map(|&(lower, upper)| {
            Some(KindConstraint {
                lower: term(lower)?,
                upper: term(upper)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::report::count;

    #[test]
    fn counts_are_pluralized() {
        assert_eq!(count(1, "arrow"), "1 arrow");
        assert_eq!(count(2, "arrow"), "2 arrows");
    }
}

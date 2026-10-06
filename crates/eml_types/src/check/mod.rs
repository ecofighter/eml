use std::collections::{HashMap, HashSet};

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::builtin::{BUILTINS, Builtin};
use eml_hir::{
    ConstructorId, FunctionId, Generics, Module, OperationId, RowRef, TypeRef, TypeRefId,
    TypeRefKind,
};
use la_arena::{Arena, ArenaMap};

use crate::context::Context;
use crate::kind::problem::{Decl, KindProblem, KindScheme, OwnVars};
use crate::kind::solve::solve_scc;
use crate::kind::{Bound, KindOrigin, KindReason, KindVar, Provenance};
use crate::shape::{Own, Shape, constructor_shape, operation_shape, signature_shape};
use crate::table::{Row, Table, TyShape};
use crate::ty::{EffectLabel, KindConstraint, KindTerm, Linearity, Multiplicity, RowTerm, Type};
use crate::{BodyTypes, TypedModule, carry, codes, exhaustive, scc, usage};

mod body;
mod equality;
mod handle;
mod report;

use body::BodyCheck;
pub(crate) use body::{BodyTyping, CallRows};
use report::AmbientSource;

/// 段0の結果。宣言ごとの閉じた型の形である。
pub(crate) struct Signatures {
    pub functions: ArenaMap<FunctionId, Shape>,
    pub builtins: HashMap<Builtin, Shape>,
    pub operations: ArenaMap<OperationId, Shape>,
    pub constructors: ArenaMap<ConstructorId, Shape>,
}

impl Signatures {
    pub fn get(&self, decl: Decl) -> Option<&Shape> {
        match decl {
            Decl::Function(id) => self.functions.get(id),
            Decl::Builtin(builtin) => self.builtins.get(&builtin),
            Decl::Operation(id) => self.operations.get(id),
            Decl::Constructor(id) => self.constructors.get(id),
        }
    }
}

/// 段1の結果。
pub(crate) struct Checked {
    pub types: BodyTypes,
    pub problem: KindProblem,
}

pub(crate) fn check_module(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    let context = Context::new(
        module.lang,
        &module.types,
        &module.constructors,
        &module.effects,
        &module.operations,
    );
    let signatures = signatures(module, &context);
    let mut schemes = declaration_schemes(module, &context, &signatures);
    let mut diagnostics = Vec::new();
    let main = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "main")
        .map(|(id, _)| id);
    if let Some(id) = main {
        check_main(module, &context, &signatures, id, &mut diagnostics);
    }
    // 本体の検査は関数ごとに独立しているので、アリーナの順に回す。診断の順は表示する側が決める
    // (docs/spec/diagnostics.md の「診断の順」)
    let components = scc::components(module);
    let mut bodies = ArenaMap::default();
    let mut problems: ArenaMap<FunctionId, KindProblem> = ArenaMap::default();
    for (id, _) in module.functions.iter() {
        if let Some((checked, found)) = check_body(module, &context, &signatures, id) {
            diagnostics.extend(found);
            bodies.insert(id, checked.types);
            problems.insert(id, checked.problem);
        }
    }
    // 等式のない関数も参照されうるので、制約のないスキームを持たせる
    for (id, _) in signatures.functions.iter() {
        if problems.get(id).is_none() {
            schemes.insert(Decl::Function(id), KindScheme::default());
        }
    }
    // 呼ばれる側の SCC から順に解き、SCC ごとに Kind を多相化する (docs/spec/types.md の「推論」)
    let mut violated = Vec::new();
    for component in &components {
        let members: Vec<(Decl, &KindProblem)> = component
            .iter()
            .filter_map(|&id| Some((Decl::Function(id), problems.get(id)?)))
            .collect();
        let solution = solve_scc(&members, &schemes);
        for (&(decl, _), scheme) in members.iter().zip(solution.schemes) {
            schemes.insert(decl, scheme);
        }
        violated.extend(solution.violated);
    }
    diagnostics.extend(report_violations(module, violated));
    let typed = typed_module(&context, &signatures, &schemes, bodies, main);
    // 網羅性は型推論と使用回数のパスの後に、書き出した型の上で調べる (docs/spec/exhaustiveness.md の「検査パス」)
    diagnostics.extend(exhaustive::check(module, &typed));
    (typed, diagnostics)
}

/// 段0: すべての宣言のシグネチャを閉じた形にする。宣言ごとに独立している。
pub(crate) fn signatures(module: &Module, context: &Context) -> Signatures {
    let functions = module
        .functions
        .iter()
        .filter_map(|(id, function)| {
            let signature = function.signature.as_ref()?;
            Some((id, signature_shape(context, signature)))
        })
        .collect();
    let builtins = BUILTINS
        .iter()
        .filter_map(|info| {
            let signature = module.builtins.get(&info.builtin)?;
            Some((info.builtin, signature_shape(context, signature)))
        })
        .collect();
    let operations = module
        .operations
        .iter()
        .map(|(id, operation)| (id, operation_shape(context, operation)))
        .collect();
    let constructors = module
        .constructors
        .iter()
        .map(|(id, constructor)| {
            let def = &module.types[constructor.ty];
            (id, constructor_shape(context, def, constructor))
        })
        .collect();
    Signatures {
        functions,
        builtins,
        operations,
        constructors,
    }
}

/// 段1: 1つの関数の本体を、全宣言の型の形だけを見て検査する。呼び出し先の本体の検査の結果は要らない
/// (docs/spec/types.md の「推論」)。シグネチャと本体の両方がある関数だけを検査する。
pub(crate) fn check_body(
    module: &Module,
    context: &Context,
    signatures: &Signatures,
    id: FunctionId,
) -> Option<(Checked, Vec<Diagnostic>)> {
    let function = &module.functions[id];
    let (Some(signature), Some(body), Some(shape)) = (
        &function.signature,
        &function.body,
        signatures.functions.get(id),
    ) else {
        return None;
    };
    let mut table = Table::new(context);
    let own = shape.instantiate_rigid(&mut table, &signature.generics);
    // 部分適用のクロージャは、それまでの引数を捕まえる (docs/spec/types.md の「関数型」)
    table.closure_kinds(own.ty, body.params.len(), &[]);
    // ここから後の制約は、由来を付け忘れたら Unattributed になり、違反すれば段2が見つける
    table.set_kind_origin(Provenance::Unattributed(function.name_range));
    let mut diagnostics = Vec::new();
    let mut checker = BodyCheck {
        module,
        function,
        body,
        rigids: &own.rigids,
        signatures,
        table: &mut table,
        diagnostics: &mut diagnostics,
        ambient: Row::pure(),
        ambient_source: AmbientSource::Signature,
        comparisons: Vec::new(),
        typing: BodyTyping::default(),
        instances: Vec::new(),
    };
    checker.check_function(own.ty);
    checker.resolve_equalities();
    let typing = checker.typing;
    let instances = checker.instances;
    let reliable = usage::reliable(body, diagnostics.is_empty());
    usage::constrain(body, &typing, &mut table, reliable);
    carry::constrain(module, body, &typing, &mut table, reliable);
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
    types.equalities = typing.equalities;
    let own_vars = OwnVars {
        lin: own.lin,
        mult: own.mult,
    };
    let problem = table.into_problem(instances, own_vars);
    Some((Checked { types, problem }, diagnostics))
}

/// 本体のない宣言の Kind のスキーム。宣言から出る制約だけを持つ問題を、1つの宣言だけの SCC として解く。
fn declaration_schemes(
    module: &Module,
    context: &Context,
    signatures: &Signatures,
) -> HashMap<Decl, KindScheme> {
    let mut problems: Vec<(Decl, KindProblem)> = Vec::new();
    for info in BUILTINS {
        let (Some(shape), Some(signature)) = (
            signatures.builtins.get(&info.builtin),
            module.builtins.get(&info.builtin),
        ) else {
            continue;
        };
        let problem = declaration_problem(context, shape, &signature.generics, |table, own| {
            table.closure_kinds(own.ty, info.arity, &[]);
        });
        problems.push((Decl::Builtin(info.builtin), problem));
    }
    for (id, operation) in module.operations.iter() {
        let shape = &signatures.operations[id];
        let generics = &operation.signature.generics;
        let problem = declaration_problem(context, shape, generics, |table, own| {
            table.closure_kinds(own.ty, operation.arity, &[]);
            // エフェクトの型引数は handle ごとに具体的な型で節を検査するので、`Unr` に固定しない
            let effect_kinds: Vec<KindVar> = own
                .rigids
                .effect_args(operation)
                .into_iter()
                .flat_map(|ty| table.kind_bounds(ty))
                .filter_map(|bound| match bound {
                    Bound::Var(var) => Some(var),
                    Bound::Const(_) => None,
                })
                .collect();
            // 結果の型は縛らない。`never fail : String -> a` をどの型としても使えるようにするため
            let mut spine = own.ty;
            for _ in 0..operation.arity {
                let TyShape::Fn { param, ret, .. } = table.shape(spine).clone() else {
                    break;
                };
                table.unrestricted(param, &effect_kinds);
                spine = ret;
            }
        });
        problems.push((Decl::Operation(id), problem));
    }
    for (id, constructor) in module.constructors.iter() {
        let shape = &signatures.constructors[id];
        let generics = &module.types[constructor.ty].generics;
        let problem = declaration_problem(context, shape, generics, |table, own| {
            table.closure_kinds(own.ty, constructor.fields.len(), &[]);
        });
        problems.push((Decl::Constructor(id), problem));
    }
    let mut schemes = HashMap::new();
    for (decl, problem) in &problems {
        let solution = solve_scc(&[(*decl, problem)], &schemes);
        // 宣言の問題の制約はすべて宣言の由来 (`Provenance::Declaration`) で、それだけでは破れないので、報告する違反はない
        debug_assert!(solution.violated.is_empty());
        let scheme = solution.schemes.into_iter().next().unwrap_or_default();
        schemes.insert(*decl, scheme);
    }
    schemes
}

/// 宣言の形を使い捨ての表に置き、宣言から出る制約だけを足した Kind の問題。
fn declaration_problem(
    context: &Context,
    shape: &Shape,
    generics: &Generics,
    constrain: impl FnOnce(&mut Table<'_>, &Own),
) -> KindProblem {
    let mut table = Table::new(context);
    let own = shape.instantiate_rigid(&mut table, generics);
    constrain(&mut table, &own);
    let own_vars = OwnVars {
        lin: own.lin,
        mult: own.mult,
    };
    table.into_problem(Vec::new(), own_vars)
}

/// Kind の制約の違反は、線形な値の誤った使い方である (docs/spec/linearity.md)。位置の順に並べ、同じ範囲の由来は `KindReason::order_key` の順に並べる。
/// 同じ値の持ち越しの違反は、呼び出しの位置が最も前のものだけを報告する (docs/spec/diagnostics.md の E3006)。
fn report_violations(module: &Module, mut origins: Vec<KindOrigin>) -> Vec<Diagnostic> {
    origins.sort_by_cached_key(|origin| {
        (
            origin.range.start(),
            origin.range.end(),
            origin.reason.order_key(),
        )
    });
    origins.dedup();
    let mut carried = HashSet::new();
    let mut out = Vec::new();
    for origin in origins {
        if let KindReason::CarriedAcross { value, .. } = &origin.reason
            && !carried.insert(value.key())
        {
            continue;
        }
        out.push(report::linear_misuse(module, &origin));
    }
    out
}

fn typed_module(
    context: &Context,
    signatures: &Signatures,
    schemes: &HashMap<Decl, KindScheme>,
    bodies: ArenaMap<FunctionId, BodyTypes>,
    main: Option<FunctionId>,
) -> TypedModule {
    let empty = KindScheme::default();
    let export = |decl: Decl, shape: &Shape| crate::Scheme {
        ty: shape.export(context),
        constraints: kind_constraints(context, shape, schemes.get(&decl).unwrap_or(&empty)),
    };
    TypedModule {
        signatures: signatures
            .functions
            .iter()
            .map(|(id, shape)| (id, export(Decl::Function(id), shape)))
            .collect(),
        bodies,
        main,
        builtins: signatures
            .builtins
            .iter()
            .map(|(&builtin, shape)| (builtin, export(Decl::Builtin(builtin), shape)))
            .collect(),
        operations: signatures
            .operations
            .iter()
            .map(|(id, shape)| (id, export(Decl::Operation(id), shape)))
            .collect(),
        constructors: signatures
            .constructors
            .iter()
            .map(|(id, shape)| (id, export(Decl::Constructor(id), shape)))
            .collect(),
    }
}

fn check_main(
    module: &Module,
    context: &Context,
    signatures: &Signatures,
    id: FunctionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &module.functions[id];
    let (Some(shape), Some(signature)) = (signatures.functions.get(id), &function.signature) else {
        return;
    };
    // 未定義のエフェクトや解決できなかった型変数・row 変数の跡から E2004 を連鎖させないため
    // (docs/spec/types.md の「エラーの扱い」)
    if has_error(&signature.types, signature.ty) {
        return;
    }
    let found = shape.export(context);
    let expected = Type::Fn {
        param: Box::new(Type::unit()),
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

fn has_error(types: &Arena<TypeRef>, id: TypeRefId) -> bool {
    match &types[id].kind {
        TypeRefKind::Error => true,
        TypeRefKind::Con(_, args) => args.iter().any(|&arg| has_error(types, arg)),
        TypeRefKind::Tuple(elements) => elements.iter().any(|&element| has_error(types, element)),
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

#[cfg(test)]
mod tests {
    use super::report::count;

    #[test]
    fn counts_are_pluralized() {
        assert_eq!(count(1, "arrow"), "1 arrow");
        assert_eq!(count(2, "arrow"), "2 arrows");
    }
}

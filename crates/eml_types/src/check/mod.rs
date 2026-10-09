use std::collections::{HashMap, HashSet};

use eml_diagnostics::{Diagnostic, Label, SourceFiles};
use eml_hir::{
    Constructor, Function, FunctionId, FunctionKind, Generics, ItemMap, Operation, Program, RowRef,
    TypeRef, TypeRefId, TypeRefKind, ValueItem,
};
use la_arena::Arena;

use crate::context::Context;
use crate::kind::problem::{KindProblem, KindScheme, OwnVars};
use crate::kind::solve::solve_scc;
use crate::kind::{Bound, KindOrigin, KindReason, KindVar, Provenance, Span};
use crate::shape::{Own, Shape, constructor_shape, operation_shape, signature_shape};
use crate::store::{EffectLabel, TypeKind, TypeStore};
use crate::table::{Exporter, Row, Table, TyShape};
use crate::{
    BodyTypes, DeclType, Instantiation, TypedProgram, carry, codes, exhaustive, scc, usage,
};

mod body;
mod equality;
mod handle;
mod report;

use body::BodyCheck;
pub(crate) use body::{BodyTyping, CallRows};
use report::AmbientSource;

/// 段0の結果。宣言ごとの閉じた型の形である。
pub(crate) struct Signatures {
    pub functions: ItemMap<Function, Shape>,
    pub operations: ItemMap<Operation, Shape>,
    pub constructors: ItemMap<Constructor, Shape>,
}

impl Signatures {
    pub fn get(&self, decl: ValueItem) -> Option<&Shape> {
        match decl {
            ValueItem::Function(id) => self.functions.get(id),
            ValueItem::Operation(id) => self.operations.get(id),
            ValueItem::Constructor(id) => self.constructors.get(id),
            // 型検査はまだクラスを扱えない。E0004 を報告済みなので、参照は誤りの型にして連鎖させない
            ValueItem::Method(_) => None,
        }
    }
}

/// 段1の結果。
pub(crate) struct Checked {
    pub types: BodyTypes,
    pub problem: KindProblem,
}

pub(crate) fn check_module(
    program: &Program,
    files: &SourceFiles,
) -> (TypedProgram, Vec<Diagnostic>) {
    let mut diagnostics = classes_not_supported(program);
    let context = Context::new(program);
    let signatures = signatures(program, &context);
    let mut schemes = declaration_schemes(program, &context, &signatures);
    // 宣言と本体の型を1つの表に登録する。表は追記だけするので、本体を独立に検査する順は結果の意味を変えない
    // (docs/implementation/architecture.md の「`eml_types` の内部」)
    let mut types = TypeStore::new(program);
    let main = program.main();
    if let Some(id) = main {
        check_main(program, &signatures, id, &mut types, &mut diagnostics);
    }
    // 本体の検査は関数ごとに独立しているので、アリーナの順に回す。診断の順は表示する側が決める
    // (docs/spec/diagnostics.md の「診断の順」)
    let components = scc::components(program);
    let mut bodies = ItemMap::default();
    let mut problems: ItemMap<Function, KindProblem> = ItemMap::default();
    for (id, _) in program.functions() {
        if let Some((checked, found)) = check_body(program, &context, &signatures, id, &mut types) {
            diagnostics.extend(found);
            bodies.insert(id, checked.types);
            problems.insert(id, checked.problem);
        }
    }
    // 等式のない関数も参照されうるので、制約のないスキームを持たせる。extern の関数のスキームは宣言から作ってあるので
    // 上書きしない
    for (id, _) in signatures.functions.iter() {
        if problems.get(id).is_none() {
            schemes.entry(ValueItem::Function(id)).or_default();
        }
    }
    // 呼ばれる側の SCC から順に解き、SCC ごとに Kind を多相化する (docs/spec/types.md の「推論」)
    let mut violated = Vec::new();
    for component in &components {
        let members: Vec<(ValueItem, &KindProblem)> = component
            .iter()
            .filter_map(|&id| Some((ValueItem::Function(id), problems.get(id)?)))
            .collect();
        let solution = solve_scc(&members, &schemes);
        for (&(decl, _), scheme) in members.iter().zip(solution.schemes) {
            schemes.insert(decl, scheme);
        }
        violated.extend(solution.violated);
    }
    diagnostics.extend(report_violations(program, files, &types, violated));
    let typed = typed_program(&signatures, schemes, bodies, types);
    // 網羅性は型推論と使用回数のパスの後に、書き出した型の上で調べる (docs/spec/exhaustiveness.md の「検査パス」)
    diagnostics.extend(exhaustive::check(program, &typed));
    (typed, diagnostics)
}

/// S5 の途中の仮の診断。型検査がクラスを扱えるようになったら外す。
fn classes_not_supported(program: &Program) -> Vec<Diagnostic> {
    let message = "type classes are not supported by the type checker yet";
    let classes = program
        .classes()
        .map(|(id, class)| (id.module, class.name_range));
    let instances = program
        .instances()
        .map(|(id, instance)| (id.module, instance.head_range));
    // 既定のメソッドと instance のメソッドの関数の制約は、クラスと instance の位置で報告済み
    let constraints = program
        .functions()
        .filter(|(_, function)| {
            matches!(
                function.kind,
                FunctionKind::Defined | FunctionKind::Extern(_)
            )
        })
        .filter_map(|(id, function)| {
            let constraint = function.signature.as_ref()?.constraints.first()?;
            Some((id.module, constraint.range))
        });
    classes
        .chain(instances)
        .chain(constraints)
        .map(|(module, range)| Diagnostic::not_yet_supported(program.file(module), range, message))
        .collect()
}

/// 段0: すべての宣言のシグネチャを閉じた形にする。宣言ごとに独立している。
pub(crate) fn signatures(program: &Program, context: &Context) -> Signatures {
    let functions = program
        .functions()
        .filter_map(|(id, function)| {
            let signature = function.signature.as_ref()?;
            Some((id, signature_shape(context, signature)))
        })
        .collect();
    let operations = program
        .operations()
        .map(|(id, operation)| (id, operation_shape(context, operation)))
        .collect();
    let constructors = program
        .constructors()
        .map(|(id, constructor)| {
            let def = &program[constructor.ty];
            (id, constructor_shape(context, def, constructor))
        })
        .collect();
    Signatures {
        functions,
        operations,
        constructors,
    }
}

/// 段1: 1つの関数の本体を、全宣言の型の形だけを見て検査する。呼び出し先の本体の検査の結果は要らない
/// (docs/spec/types.md の「推論」)。シグネチャと本体の両方がある関数だけを検査する。
pub(crate) fn check_body(
    program: &Program,
    context: &Context,
    signatures: &Signatures,
    id: FunctionId,
    types: &mut TypeStore,
) -> Option<(Checked, Vec<Diagnostic>)> {
    let function = &program[id];
    let (Some(signature), Some(body), Some(shape)) = (
        &function.signature,
        program.body(id),
        signatures.functions.get(id),
    ) else {
        return None;
    };
    let mut table = Table::new(context);
    let own = shape.instantiate_rigid(&mut table, &signature.generics);
    // 部分適用のクロージャは、それまでの引数を捕まえる (docs/spec/types.md の「関数型」)
    table.closure_kinds(own.ty, body.params.len(), &[]);
    let file = program.file(id.module);
    // ここから後の制約は、由来を付け忘れたら Unattributed になり、違反すれば段2が見つける
    table.set_kind_origin(Provenance::Unattributed(Span {
        file,
        range: function.name_range,
    }));
    let mut diagnostics = Vec::new();
    let mut checker = BodyCheck {
        program,
        file,
        function,
        body,
        rigids: &own.rigids,
        signatures,
        table: &mut table,
        types: &mut *types,
        diagnostics: &mut diagnostics,
        ambient: Row::pure(),
        ambient_source: AmbientSource::Signature,
        typing: BodyTyping::default(),
        instances: Vec::new(),
    };
    checker.check_function(own.ty);
    checker.check_comparisons();
    let typing = checker.typing;
    let instances = checker.instances;
    let reliable = usage::reliable(body, diagnostics.is_empty());
    usage::constrain(file, body, &typing, &mut table, types, reliable);
    carry::constrain(program, file, body, &typing, &mut table, reliable);
    // 式、局所変数、パターン、具体化の型は表の同じ節点を共有するので、1つの `Exporter` で書き出し、各節点を1回だけ
    // 書き出す
    let mut exporter = Exporter::new(&table, types);
    let mut body_types = BodyTypes::default();
    for (expr, &ty) in typing.exprs.iter() {
        body_types.exprs.insert(expr, exporter.export(ty));
    }
    for (local, &ty) in typing.locals.iter() {
        body_types.locals.insert(local, exporter.export(ty));
    }
    for (pat, &ty) in typing.pats.iter() {
        body_types.pats.insert(pat, exporter.export(ty));
    }
    // 本体全体の検査が終わってから `exprs` と一緒に書き出す。後の文の単一化で決まった型引数を含めるためで、
    // carry が足すのは Kind の制約だけである
    for (expr, (decl, args)) in typing.instantiations.iter() {
        let args = args.iter().map(|&arg| exporter.export(arg)).collect();
        body_types
            .instantiations
            .insert(expr, Instantiation { decl: *decl, args });
    }
    body_types.masks = typing.masks;
    let own_vars = OwnVars {
        lin: own.lin,
        mult: own.mult,
    };
    let problem = table.into_problem(instances, own_vars);
    Some((
        Checked {
            types: body_types,
            problem,
        },
        diagnostics,
    ))
}

/// 本体のない宣言の Kind のスキーム。宣言から出る制約だけを持つ問題を、1つの宣言だけの SCC として解く。
fn declaration_schemes(
    program: &Program,
    context: &Context,
    signatures: &Signatures,
) -> HashMap<ValueItem, KindScheme> {
    let mut problems: Vec<(ValueItem, KindProblem)> = Vec::new();
    // extern の関数は本体を持たないので、部分適用のクロージャの Kind だけを宣言から出す
    // (docs/spec/types.md の「関数型」)
    for (id, function) in program
        .functions()
        .filter(|(_, function)| matches!(function.kind, FunctionKind::Extern(_)))
    {
        let (Some(shape), Some(signature)) = (signatures.functions.get(id), &function.signature)
        else {
            continue;
        };
        let arity = signature.arity();
        let problem = declaration_problem(context, shape, &signature.generics, |table, own| {
            table.closure_kinds(own.ty, arity, &[]);
        });
        problems.push((ValueItem::Function(id), problem));
    }
    for (id, operation) in program.operations() {
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
        problems.push((ValueItem::Operation(id), problem));
    }
    for (id, constructor) in program.constructors() {
        let shape = &signatures.constructors[id];
        let generics = &program[constructor.ty].generics;
        let problem = declaration_problem(context, shape, generics, |table, own| {
            table.closure_kinds(own.ty, constructor.fields.len(), &[]);
        });
        problems.push((ValueItem::Constructor(id), problem));
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

/// Kind の制約の違反は、線形な値の誤った使い方である (docs/spec/linearity.md)。ファイルと位置の順に並べ、同じ範囲の由来は `KindReason::order_key` の順に並べる。
/// 同じ値の持ち越しの違反は、呼び出しの位置が最も前のものだけを報告する (docs/implementation/diagnostics.md の E3006)。
fn report_violations(
    program: &Program,
    files: &SourceFiles,
    types: &TypeStore,
    mut origins: Vec<KindOrigin>,
) -> Vec<Diagnostic> {
    origins.sort_by_cached_key(|origin| {
        (
            origin.span.file,
            origin.span.range.start(),
            origin.span.range.end(),
            origin.reason.order_key(types, &program.names),
        )
    });
    origins.dedup();
    let mut carried = HashSet::new();
    let mut out = Vec::new();
    for origin in origins {
        // 値の範囲は由来と同じファイルにある。別のファイルの値は、範囲が同じでも別の値である
        if let KindReason::CarriedAcross { value, .. } = &origin.reason
            && !carried.insert((origin.span.file, value.key()))
        {
            continue;
        }
        out.push(report::linear_misuse(program, files, types, &origin));
    }
    out
}

/// 段0の形と段2のスキームを、宣言ごとの結果にまとめる。この時点で、形を持つ宣言はすべてスキームを持つ。extern の
/// 関数、操作、コンストラクタは `declaration_schemes` が、本体に問題のない関数は `check_module` が (制約がなければ空の
/// スキームを)、解いた SCC の関数は SCC の解が入れるためである。
fn typed_program(
    signatures: &Signatures,
    mut schemes: HashMap<ValueItem, KindScheme>,
    bodies: ItemMap<Function, BodyTypes>,
    mut types: TypeStore,
) -> TypedProgram {
    let shapes = signatures
        .functions
        .iter()
        .map(|(id, shape)| (ValueItem::Function(id), shape))
        .chain(
            signatures
                .operations
                .iter()
                .map(|(id, shape)| (ValueItem::Operation(id), shape)),
        )
        .chain(
            signatures
                .constructors
                .iter()
                .map(|(id, shape)| (ValueItem::Constructor(id), shape)),
        );
    let decls = shapes
        .map(|(decl, shape)| {
            let declared = DeclType {
                ty: shape.export(&mut types),
                shape: shape.clone(),
                kinds: schemes
                    .remove(&decl)
                    .expect("every declaration has a Kind scheme"),
            };
            (decl, declared)
        })
        .collect();
    TypedProgram {
        types,
        decls,
        bodies,
    }
}

fn check_main(
    program: &Program,
    signatures: &Signatures,
    id: FunctionId,
    types: &mut TypeStore,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &program[id];
    let (Some(shape), Some(signature)) = (signatures.functions.get(id), &function.signature) else {
        return;
    };
    // 未定義のエフェクトや解決できなかった型変数・row 変数の跡から E2004 を連鎖させないため
    // (docs/spec/types.md の「エラーの扱い」)
    if has_error(&signature.types, signature.ty) {
        return;
    }
    let found = shape.export(types);
    let expected = types.intern(TypeKind::Fn {
        param: types.unit(),
        effects: vec![EffectLabel {
            id: program.io(),
            args: Vec::new(),
        }],
        tail: None,
        ret: types.unit(),
    });
    // 同じ形の型は同じ ID なので、ID を比べれば型を比べたことになる
    if !types.contains_error(found) && found != expected {
        diagnostics.push(Diagnostic::error(
            codes::INVALID_MAIN_TYPE,
            format!(
                "`main` must have type `{}`",
                types.display(expected, &program.names)
            ),
            Label::new(
                program.file(id.module),
                signature.range,
                format!("found `{}`", types.display(found, &program.names)),
            ),
        ));
    }
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
    use eml_diagnostics::TextRange;

    use super::report::count;
    use super::report_violations;
    use crate::kind::{CallKind, CarriedValue, KindOrigin, KindReason, Span};

    #[test]
    fn counts_are_pluralized() {
        assert_eq!(count(1, "arrow"), "1 arrow");
        assert_eq!(count(2, "arrow"), "2 arrows");
    }

    #[test]
    fn carried_values_in_different_files_are_reported_separately() {
        // 範囲が同じでも、ファイルが違えば別の値である
        let (program, files) = crate::test_program_with_files("");
        let prelude = program.file(program.prelude);
        let entry = program.file(program.entry);
        let range = TextRange::new(0.into(), 1.into());
        let origin = |file| KindOrigin {
            span: Span { file, range },
            reason: KindReason::CarriedAcross {
                value: CarriedValue::Temporary(range),
                multi: None,
                call: CallKind::Call,
            },
        };
        let types = crate::TypeStore::new(&program);
        let reported = report_violations(
            &program,
            &files,
            &types,
            vec![origin(entry), origin(prelude)],
        );
        let files: Vec<_> = reported.iter().map(|d| d.primary.file).collect();
        assert_eq!(files, vec![prelude, entry]);
    }
}

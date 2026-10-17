use std::collections::{HashMap, HashSet};

use eml_diagnostics::{Diagnostic, Label, SourceFiles};
use eml_hir::{
    Binding, ClassId, Constructor, ExprId, Function, FunctionId, FunctionKind, Generics,
    InstanceId, InstanceOrigin, ItemMap, LocalId, Method, Operation, Program, RowRef, Signature,
    TypeDefKind, TypeRef, TypeRefId, TypeRefKind, TypeVarId, ValueItem,
};
use la_arena::Arena;

use crate::context::Context;
use crate::kind::entail::unentailed;
use crate::kind::eval::scheme_facts;
use crate::kind::problem::{Instance, KindProblem, KindScheme, OwnVars};
use crate::kind::solve::{Solution, solve_scc};
use crate::kind::summary::BodyKinds;
use crate::kind::{Bound, KindOrigin, KindReason, KindVar, Provenance, Span};
use crate::shape::{Arrows, Own, Shape, constructor_shape, operation_shape, signature_shape};
use crate::store::{EffectLabel, Lin, TypeKind, TypeStore};
use crate::table::{Exporter, RigidVar, Row, Table, TyShape};
use crate::ty::Linearity;
use crate::{
    BodyTypes, DeclType, InputType, Instantiation, RowYield, TypedProgram, Uniform, Update, carry,
    codes, exhaustive, scc, uniform, usage,
};
use constraints::{Defaulting, Failure, show_constraint, solve};

mod body;
mod constraints;
mod field;
mod handle;
mod input;
mod query;
mod report;

use body::BodyCheck;
pub(crate) use body::{BodyTyping, CallRows};
pub(crate) use constraints::structural;
use report::AmbientSource;

/// 段0の結果。宣言ごとの閉じた型の形である。
pub(crate) struct Signatures {
    pub functions: ItemMap<Function, Shape>,
    pub operations: ItemMap<Operation, Shape>,
    pub constructors: ItemMap<Constructor, Shape>,
    pub methods: ItemMap<Method, Shape>,
    /// REPL の `let` の束縛の型。束縛を作った入力の関数を書き出したときに入る (`input::finish_input`)。
    pub bindings: ItemMap<Binding, Shape>,
}

impl Signatures {
    pub fn get(&self, decl: ValueItem) -> Option<&Shape> {
        match decl {
            ValueItem::Function(id) => self.functions.get(id),
            ValueItem::Operation(id) => self.operations.get(id),
            ValueItem::Constructor(id) => self.constructors.get(id),
            ValueItem::Method(id) => self.methods.get(id),
            ValueItem::Binding(id) => self.bindings.get(id),
        }
    }
}

/// 段1を終えた本体。Kind を解いた後に型を書き出すので、推論用の表を SCC の解が出るまで持つ
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。
pub(crate) struct PendingBody<'c> {
    table: Table<'c>,
    typing: BodyTyping,
    own: Own,
    instances: Vec<Instance>,
    /// 使った回数を正しく数えられる本体か (`usage::reliable`)。
    pub reliable: bool,
    /// `:type` の問い合わせの型の文字列。
    query: Option<String>,
    /// 戻らない呼び出しの前で暗黙に手放す変数 (`BodyTypes::implicit_drops`)。
    implicit_drops: HashMap<ExprId, Vec<LocalId>>,
}

impl PendingBody<'_> {
    /// Kind の制約を取り出す。表の形は書き出しで使うので残す。
    pub fn take_problem(&mut self) -> KindProblem {
        let own = OwnVars {
            lin: self.own.lin.clone(),
            mult: self.own.mult.clone(),
        };
        self.table
            .take_problem(std::mem::take(&mut self.instances), own)
    }
}

pub(crate) fn check_module(
    program: &Program,
    files: &SourceFiles,
) -> (TypedProgram, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let context = Context::new(program);
    // 宣言と本体の型を1つの表に登録する。表は追記だけするので、本体を独立に検査する順は結果の意味を変えない
    // (docs/implementation/architecture.md の「`eml_types` の内部」)
    let mut types = TypeStore::new(program);
    check_instances(program, &context, &mut types, &mut diagnostics);
    let mut signatures = signatures(program, &context);
    let mut schemes = declaration_schemes(program, &context, &signatures);
    let main = program.main();
    if let Some(id) = main {
        check_main(program, &signatures, id, &mut types, &mut diagnostics);
    }
    // 等式のない関数も参照されうるので、制約のないスキームを持たせる。extern の関数のスキームは宣言から作ってあるので
    // 上書きしない
    for (id, function) in program.functions() {
        if function.signature.is_some() && program.body(id).is_none() {
            schemes.entry(ValueItem::Function(id)).or_default();
        }
    }
    // 呼ばれる側の SCC から順に、検査、Kind の解決、型の書き出しをする。書き出す型に Kind の解を入れるため、
    // 表は SCC の解が出るまで持つ。診断の順は表示する側が決める (docs/spec/diagnostics.md の「診断の順」)
    let mut bodies = ItemMap::default();
    let mut joins = HashMap::new();
    let mut checked = HashSet::new();
    let mut unreliable = HashSet::new();
    let mut violated = Vec::new();
    let mut inputs = ItemMap::default();
    let mut query = None;
    for component in &scc::components(program) {
        let mut pending: Vec<(FunctionId, PendingBody<'_>)> = Vec::new();
        for &id in component {
            let result = match program[id].kind {
                FunctionKind::Input(kind) => Some(input::check_input(
                    program,
                    &context,
                    &signatures,
                    id,
                    kind,
                    &mut types,
                )),
                _ => check_body(program, &context, &signatures, id, &mut types),
            };
            if let Some((body, found)) = result {
                diagnostics.extend(found);
                checked.insert(id);
                if !body.reliable {
                    unreliable.insert(id);
                }
                pending.push((id, body));
            }
        }
        let problems: Vec<(ValueItem, KindProblem)> = pending
            .iter_mut()
            .map(|(id, body)| (ValueItem::Function(*id), body.take_problem()))
            .collect();
        let members: Vec<(ValueItem, &KindProblem)> = problems
            .iter()
            .map(|(decl, problem)| (*decl, problem))
            .collect();
        let Solution {
            schemes: solved,
            violated: found,
            lin,
        } = solve_scc(&members, &schemes);
        for (&(decl, _), scheme) in members.iter().zip(solved) {
            schemes.insert(decl, scheme);
        }
        violated.extend(found);
        for (member, (id, mut body)) in pending.into_iter().enumerate() {
            // 宣言自身の変数の畳み込みは、この SCC の解が残した新しいスキームで決める
            let scheme = &schemes[&ValueItem::Function(id)];
            // 入力の関数は宣言の型を持たず、宣言自身の Kind 変数もない
            let lin_vars = signatures
                .functions
                .get(id)
                .map_or(0, |shape| shape.lin_vars);
            let kinds = BodyKinds::new(&lin, member, scheme, lin_vars);
            if let Some(shown) = body.query.take() {
                query = Some(shown);
            }
            let (body_types, body_joins) = export_body(body, kinds, &mut types);
            if let FunctionKind::Input(kind) = program[id].kind
                && let Some(input) = input::finish_input(
                    program,
                    id,
                    kind,
                    &body_types,
                    &mut types,
                    &mut signatures,
                    &mut schemes,
                    &mut diagnostics,
                )
            {
                inputs.insert(id, input);
            }
            bodies.insert(id, body_types);
            joins.insert(id, body_joins);
        }
    }
    diagnostics.extend(check_method_kinds(
        program,
        &context,
        &signatures,
        &checked,
        &unreliable,
        &schemes,
    ));
    diagnostics.extend(report_violations(program, files, &types, violated));
    let mut typed = typed_program(&signatures, schemes, joins, bodies, inputs, types);
    typed.query = query;
    let (uniform, found) = uniform::uniform(program, &typed.types, &typed.decls, &typed.bodies);
    typed.uniform = uniform;
    diagnostics.extend(found);
    // 網羅性は型推論と使用回数のパスの後に、書き出した型の上で調べる (docs/spec/exhaustiveness.md の「検査パス」)
    diagnostics.extend(exhaustive::check(program, &typed));
    (typed, diagnostics)
}

/// instance ごとに、頭が `Unr` であることと、クラスの直接の上位クラスの instance が頭の型にあり、その文脈が
/// この instance の文脈から導けることを確かめる。導出した instance は、フィールドの制約が解けることも確かめる
/// (docs/spec/types.md の「`Unr` のクラス」、「instance と既定のメソッドの検査」と
/// 「導出した instance の検査」)。上位クラスの instance の頭の型変数は、同じ `data` の型引数なので、番号でこの instance の
/// 型変数に対応する。
fn check_instances(
    program: &Program,
    context: &Context,
    types: &mut TypeStore,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let names = &program.names;
    for (id, instance) in program.instances() {
        let class = instance.class;
        let vars: Vec<&str> = instance
            .generics
            .type_vars
            .values()
            .map(|var| var.name.as_str())
            .collect();
        let head = instance_head(program, id);
        // 頭の型変数を `Unr` とすれば、頭が `Lin` になるのは定数の `Lin` を含むときだけである
        let linear = context.data_kinds[instance.head].lin;
        if linear {
            diagnostics.push(
                Diagnostic::error(
                    codes::LINEAR_INSTANCE_HEAD,
                    format!(
                        "`{head}` is linear, so it cannot have an instance of `{}`",
                        names.class(class)
                    ),
                    Label::new(program.file(id.module), instance.head_range, "a linear type"),
                )
                .with_note(
                    "the methods of a class may copy or drop their arguments, which a linear value forbids",
                ),
            );
        }
        // `Lin` のフィールドには instance がないので、E2010 に E2006 を重ねない
        if let InstanceOrigin::Derived(_) = instance.origin
            && !linear
        {
            diagnostics.extend(check_derived_fields(program, context, types, id));
        }
        let given: Vec<(ClassId, TypeVarId)> = instance
            .context
            .iter()
            .flat_map(|constraint| {
                std::iter::once(constraint.class)
                    .chain(program.superclasses(constraint.class))
                    .map(|class| (class, constraint.var))
            })
            .collect();
        let at = |message: String, label: String| {
            Diagnostic::error(
                codes::NO_INSTANCE,
                message,
                Label::new(program.file(id.module), instance.head_range, label),
            )
        };
        for &superclass in &program[class].superclasses {
            let Some(found) = program.instance(superclass, instance.head) else {
                diagnostics.push(at(
                    format!("no instance of `{}` for `{head}`", names.class(superclass)),
                    format!(
                        "`{}` requires `{}`, its superclass",
                        names.class(class),
                        names.class(superclass)
                    ),
                ));
                continue;
            };
            for constraint in &program[found].context {
                if given.contains(&(constraint.class, constraint.var)) {
                    continue;
                }
                let needed = names.class(constraint.class);
                let var = vars[u32::from(constraint.var.into_raw()) as usize];
                let wanted = format!("{needed} {var}");
                diagnostics.push(
                    at(
                        format!("no instance of `{needed}` for `{var}`"),
                        format!(
                            "the instance of `{}` for `{head}` requires `{wanted}`",
                            names.class(superclass)
                        ),
                    )
                    // 導出した instance の文脈は `deriving` が決めるので、書き足す場所がない
                    // (docs/spec/declarations.md の「`deriving`」)
                    .with_help(match instance.origin {
                        InstanceOrigin::Written => {
                            format!("add `{wanted}` to the context of this instance")
                        }
                        InstanceOrigin::Derived(_) => format!(
                            "a derived instance cannot have `{wanted}` in its context; write the instance by hand"
                        ),
                    }),
                );
            }
        }
    }
}

/// 導出した instance の各フィールドの型 `F` について、文脈を与えられた制約として `C F` を解く。解けなければ
/// `deriving` のクラス名を指して E2006 にする。フィールドの型は、`data` の型引数を rigid 変数として使い捨ての表に
/// 下ろす。同じ位置に誤りを重ねないよう、報告は instance ごとに最初のフィールドの1つだけにする。
fn check_derived_fields(
    program: &Program,
    context: &Context,
    types: &mut TypeStore,
    id: InstanceId,
) -> Option<Diagnostic> {
    let instance = &program[id];
    let def = &program[instance.head];
    let TypeDefKind::Data { constructors } = &def.kind else {
        return None;
    };
    for &constructor in constructors {
        let fields = program[constructor].fields.len();
        let mut table = Table::new(context);
        let own = constructor_shape(context, def, &program[constructor])
            .instantiate_rigid(&mut table, &def.generics);
        let givens: Vec<(ClassId, RigidVar)> = instance
            .context
            .iter()
            .flat_map(|constraint| {
                let var = own.rigids.vars()[u32::from(constraint.var.into_raw()) as usize];
                std::iter::once(constraint.class)
                    .chain(program.superclasses(constraint.class))
                    .map(move |class| (class, var))
            })
            .collect();
        let mut spine = own.ty;
        for _ in 0..fields {
            let TyShape::Fn { param, ret, .. } = table.shape(spine).clone() else {
                break;
            };
            spine = ret;
            let Err(Failure::NoInstance { root, leaf }) = solve(
                program,
                &table,
                (instance.class, param),
                &givens,
                false,
                Defaulting::None,
            ) else {
                continue;
            };
            let mut exporter = Exporter::diagnostic(&table, types);
            let (field, leaf_ty) = (exporter.export(root.1), exporter.export(leaf.1));
            if types.contains_error(field) {
                continue;
            }
            let names = &program.names;
            let class = names.class(instance.class);
            return Some(
                Diagnostic::error(
                    codes::NO_INSTANCE,
                    format!(
                        "no instance of `{}` for `{}`",
                        names.class(leaf.0),
                        types.display(leaf_ty, names)
                    ),
                    Label::new(
                        program.file(id.module),
                        instance.head_range,
                        format!(
                            "`deriving {class}` needs it for a field of `{}`",
                            names.constructor(constructor)
                        ),
                    ),
                )
                .with_note(format!(
                    "the field of type `{}` needs `{}`",
                    types.display(field, names),
                    show_constraint(program, types, root.0, field)
                )),
            );
        }
    }
    None
}

/// メソッドと、それを定義する関数の矢印の決め方。本体の検査の `instantiate_rigid` は形から作るので、既定のメソッドと
/// instance のメソッドの本体も同じ矢印で検査する
/// (docs/spec/types.md の「`Unr` のクラス」)。
const METHOD_ARROWS: Arrows = Arrows::Method { outermost: true };

/// instance の頭の型の表示 (`Box a`)。
pub(crate) fn instance_head(program: &Program, id: InstanceId) -> String {
    let instance = &program[id];
    std::iter::once(program.names.ty(instance.head))
        .chain(
            instance
                .generics
                .type_vars
                .values()
                .map(|var| var.name.as_str()),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

/// 段0: すべての宣言のシグネチャを閉じた形にする。宣言ごとに独立している。
pub(crate) fn signatures(program: &Program, context: &Context) -> Signatures {
    let functions = program
        .functions()
        .filter_map(|(id, function)| {
            let signature = function.signature.as_ref()?;
            let arrows = match function.kind {
                FunctionKind::DefaultMethod(_) | FunctionKind::InstanceMethod(..) => METHOD_ARROWS,
                FunctionKind::Defined | FunctionKind::Extern(_) | FunctionKind::Input(_) => {
                    Arrows::OutermostUnr
                }
            };
            Some((id, signature_shape(context, signature, arrows)))
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
    let methods = program
        .methods()
        .map(|(id, method)| {
            (
                id,
                signature_shape(context, &method.signature, METHOD_ARROWS),
            )
        })
        .collect();
    Signatures {
        functions,
        operations,
        constructors,
        methods,
        bindings: ItemMap::default(),
    }
}

/// 段1: 1つの関数の本体を、全宣言の型の形だけを見て検査する。呼び出し先の本体の検査の結果は要らない
/// (docs/spec/types.md の「推論」)。シグネチャと本体の両方がある関数だけを検査する。
pub(crate) fn check_body<'c>(
    program: &Program,
    context: &'c Context,
    signatures: &Signatures,
    id: FunctionId,
    types: &mut TypeStore,
) -> Option<(PendingBody<'c>, Vec<Diagnostic>)> {
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
    let previous = table.set_kind_origin(Provenance::Given);
    for var in unrestricted_vars(program, function, signature) {
        table.kind_at_most(own.rigids.ty(var), Bound::Const(Linearity::Unr));
    }
    table.set_kind_origin(previous);
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
        local_frames: Vec::new(),
        closed_locals: HashSet::new(),
        fn_clause_bodies: HashMap::new(),
    };
    checker.check_function(own.ty);
    checker.solve_constraints(Defaulting::None);
    let typing = checker.typing;
    let instances = checker.instances;
    // 頭が線形な instance (E2010) のメソッドは、頭の値を `Unr` とみなして書いた本体なので、線形性の診断を重ねない
    // (docs/spec/types.md の「`Unr` のクラス」)
    let linear_head = matches!(function.kind, FunctionKind::InstanceMethod(instance, _)
        if context.data_kinds[program[instance].head].lin);
    let reliable = usage::reliable(body, diagnostics.is_empty()) && !linear_head;
    let implicit_drops =
        usage::constrain(program, file, body, &typing, &mut table, types, reliable);
    carry::constrain(
        program,
        file,
        body,
        &typing,
        &mut table,
        reliable,
        &implicit_drops,
    );
    Some((
        PendingBody {
            table,
            typing,
            own,
            instances,
            reliable,
            query: None,
            implicit_drops,
        },
        diagnostics,
    ))
}

/// 段1の本体の型を、SCC の解から求めた Kind 変数の値 `kinds` を使って `TypeStore` に書き出し、本体の `joins` も返す。
/// row の分類は生きている表が要るので、ここでする。
pub(crate) fn export_body(
    pending: PendingBody<'_>,
    kinds: BodyKinds<'_>,
    types: &mut TypeStore,
) -> (BodyTypes, Vec<Vec<u32>>) {
    let PendingBody {
        table,
        typing,
        own,
        implicit_drops,
        ..
    } = pending;
    // 式、局所変数、パターン、具体化の型は表の同じ節点を共有するので、1つの `Exporter` で書き出し、各節点を1回だけ
    // 書き出す。`joins` の番号は初めて現れた順に振るので、この書き出しの順 (式、局所変数、パターン、具体化、更新、
    // 捨てたフィールド) が番号を決める (docs/spec/types.md の
    // 「宣言の Kind 変数の番号」)
    let mut exporter = Exporter::solved(&table, types, kinds);
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
    // row は型引数と同じく本体全体の単一化が終わってから分類する。rigid な row 変数がこの定義のものかを見分けられる
    // のは、表が生きている今だけである (docs/spec/types.md の
    // 「row の具体化の記録」)
    let own_rows = own.rigids.row_vars();
    let classify = |row: &Row| table.row_yield(row, &own_rows);
    // 本体全体の検査が終わってから `exprs` と一緒に書き出す。後の文の単一化で決まった型引数を含めるためで、
    // carry が足すのは Kind の制約だけである
    for (expr, (decl, args, rows)) in typing.instantiations.iter() {
        let args = args.iter().map(|&arg| exporter.export(arg)).collect();
        let rows = rows
            .iter()
            .map(|&var| table.row_var_yield(var, &own_rows))
            .collect();
        body_types.instantiations.insert(
            expr,
            Instantiation {
                decl: *decl,
                args,
                rows,
            },
        );
    }
    body_types.body_yield = typing.ambient.as_ref().map_or(RowYield::Never, classify);
    body_types.first_arrow_yield = match table.shape(own.ty) {
        TyShape::Fn { row, .. } => classify(row),
        _ => RowYield::Never,
    };
    for (expr, row) in typing.lambdas.iter() {
        body_types.lambda_yields.insert(expr, classify(row));
    }
    body_types.arrow_yields = typing
        .arrows
        .iter()
        .map(|(&key, row)| (key, classify(row)))
        .collect();
    body_types.masks = typing.masks;
    body_types.implicit_drops = implicit_drops;
    body_types.fields = typing.fields;
    for (expr, update) in typing.updates.iter() {
        let update = Update {
            ctor: update.ctor,
            written: update.written.clone(),
            fields: update
                .fields
                .iter()
                .map(|&ty| exporter.export(ty))
                .collect(),
        };
        body_types.updates.insert(expr, update);
    }
    for (expr, discarded) in typing.discarded.iter() {
        let discarded = discarded
            .iter()
            .map(|&(field, ty)| (field, exporter.export(ty)))
            .collect();
        body_types.discarded.insert(expr, discarded);
    }
    (body_types, exporter.into_joins())
}

/// `Unr` とみなすシグネチャの型変数。制約 `C a` は `a ≤ Unr` を意味し、instance の本体は頭の型変数を、既定のメソッドは
/// クラスの型変数を `Unr` とみなして検査する
/// (docs/spec/types.md の「`Unr` のクラス」)。既定のメソッドのクラスの型変数は
/// 制約 `C a` にも現れるが、instance のメソッドの頭の型変数とそろえて並べる。
fn unrestricted_vars(
    program: &Program,
    function: &Function,
    signature: &Signature,
) -> Vec<TypeVarId> {
    let class_side = match function.kind {
        FunctionKind::InstanceMethod(instance, _) => program[instance].generics.type_vars.len(),
        FunctionKind::DefaultMethod(_) => 1,
        FunctionKind::Defined | FunctionKind::Extern(_) | FunctionKind::Input(_) => 0,
    };
    let mut vars: Vec<TypeVarId> = signature
        .generics
        .type_vars
        .iter()
        .take(class_side)
        .map(|(var, _)| var)
        .collect();
    vars.extend(
        signature
            .constraints
            .iter()
            .map(|constraint| constraint.var),
    );
    vars
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
    // メソッドも本体を持たないので、extern の関数と同じく宣言だけから作る
    // (docs/spec/types.md の「instance と既定のメソッドの検査」)
    for (id, method) in program.methods() {
        let shape = &signatures.methods[id];
        let generics = &method.signature.generics;
        let arity = method.signature.arity();
        let problem = declaration_problem(context, shape, generics, |table, own| {
            table.closure_kinds(own.ty, arity, &[]);
            // クラスの型変数 (番号 0) とメソッド自身の制約の型変数は、制約 `C a` があるので `Unr` である
            let class_var = generics.type_vars.iter().map(|(var, _)| var).take(1);
            let constrained = method.signature.constraints.iter().map(|c| c.var);
            for var in class_var.chain(constrained) {
                table.kind_at_most(own.rigids.ty(var), Bound::Const(Linearity::Unr));
            }
        });
        problems.push((ValueItem::Method(id), problem));
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
            // 結果の型は縛らない。`never fail : String -> a` をどの型としても使えるようにするため。呼ぶ側から節へ渡る位置の
            // 矢印は、形がすでに `Unr` に固定している (`Arrows::OperationResult`)
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

/// instance のメソッドと既定のメソッドの Kind のスキームが、クラスの側のスキームから導けることを確かめる (E2011。
/// docs/spec/types.md の「instance と既定のメソッドの検査」)。クラスの側の
/// スキームは、関数の形に宣言から出る制約だけを足して作る。関数の形はクラスの型変数を頭の型に置き換えたメソッドの
/// シグネチャなので、前提の「頭の型に置き換えたこと」は形が受け持つ。関数のスキームと同じ形から作るので、2つの
/// スキームは同じ Kind 変数の番号を使う。
///
/// 使った回数を数えられない本体 (`unreliable`) は確かめない。その本体のスキームには、由来を記録しない使用回数の制約が
/// 残っていて、それをもとに E2011 を出すと誤りの連鎖になるためである (docs/spec/types.md の「エラーの扱い」)。
fn check_method_kinds(
    program: &Program,
    context: &Context,
    signatures: &Signatures,
    checked: &HashSet<FunctionId>,
    unreliable: &HashSet<FunctionId>,
    schemes: &HashMap<ValueItem, KindScheme>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, function) in program.functions() {
        let (instance, method) = match function.kind {
            FunctionKind::InstanceMethod(instance, method) => (Some(instance), method),
            FunctionKind::DefaultMethod(method) => (None, method),
            FunctionKind::Defined | FunctionKind::Extern(_) | FunctionKind::Input(_) => continue,
        };
        if unreliable.contains(&id) || !checked.contains(&id) {
            continue;
        }
        let decl = ValueItem::Function(id);
        let (Some(shape), Some(signature), Some(scheme)) = (
            signatures.functions.get(id),
            &function.signature,
            schemes.get(&decl),
        ) else {
            continue;
        };
        let arity = program[method].signature.arity();
        let unrestricted = unrestricted_vars(program, function, signature);
        let problem = declaration_problem(context, shape, &signature.generics, |table, own| {
            table.closure_kinds(own.ty, arity, &[]);
            for &var in &unrestricted {
                table.kind_at_most(own.rigids.ty(var), Bound::Const(Linearity::Unr));
            }
        });
        let solution = solve_scc(&[(decl, &problem)], schemes);
        debug_assert!(solution.violated.is_empty());
        let class_side = solution.schemes.into_iter().next().unwrap_or_default();
        if unentailed(&class_side, scheme).is_empty() {
            continue;
        }
        let name = &program[method].name;
        let message = match instance {
            Some(instance) => format!(
                "`{name}` in the instance for `{}` needs more than the signature of `{name}` allows",
                instance_head(program, instance)
            ),
            None => {
                format!("the default `{name}` needs more than the signature of `{name}` allows")
            }
        };
        diagnostics.push(
            Diagnostic::error(
                codes::METHOD_KIND_MISMATCH,
                message,
                Label::new(
                    program.file(id.module),
                    function.name_range,
                    "this definition",
                ),
            )
            .with_note(
                "the signature of a method leaves its own type variables free to be linear, and this definition copies, drops or keeps a value of such a type",
            ),
        );
    }
    diagnostics
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
/// 関数、メソッド、操作、コンストラクタは `declaration_schemes` が、本体に問題のない関数は `check_module` が
/// (制約がなければ空のスキームを)、解いた SCC の関数は SCC の解が入れるためである。`joins` は本体を書き出した関数の
/// ものである。
fn typed_program(
    signatures: &Signatures,
    mut schemes: HashMap<ValueItem, KindScheme>,
    mut joins: HashMap<FunctionId, Vec<Vec<u32>>>,
    bodies: ItemMap<Function, BodyTypes>,
    inputs: ItemMap<Function, InputType>,
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
        )
        .chain(
            signatures
                .methods
                .iter()
                .map(|(id, shape)| (ValueItem::Method(id), shape)),
        )
        .chain(
            signatures
                .bindings
                .iter()
                .map(|(id, shape)| (ValueItem::Binding(id), shape)),
        );
    let decls = shapes
        .map(|(decl, shape)| {
            let kinds = schemes
                .remove(&decl)
                .expect("every declaration has a Kind scheme");
            let joins = match decl {
                ValueItem::Function(id) => joins.remove(&id).unwrap_or_default(),
                ValueItem::Operation(_)
                | ValueItem::Constructor(_)
                | ValueItem::Method(_)
                | ValueItem::Binding(_) => Vec::new(),
            };
            let facts = scheme_facts(&kinds, shape.lin_vars);
            let declared = DeclType {
                ty: shape.export(&mut types, Some(&facts)),
                shape: shape.clone(),
                kinds,
                facts,
                joins,
            };
            (decl, declared)
        })
        .collect();
    TypedProgram {
        types,
        decls,
        bodies,
        uniform: Uniform::default(),
        inputs,
        query: None,
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
    let found = shape.export(types, None);
    let expected = types.intern(TypeKind::Fn {
        param: types.unit(),
        lin: Lin::Unr,
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
        TypeRefKind::Con(_, args, rows) => {
            args.iter().any(|&arg| has_error(types, arg))
                || rows.iter().any(|row| row_has_error(types, row))
        }
        TypeRefKind::Tuple(elements) => elements.iter().any(|&element| has_error(types, element)),
        TypeRefKind::Var(_) => false,
        TypeRefKind::Fn {
            param, row, ret, ..
        } => row_has_error(types, row) || has_error(types, *param) || has_error(types, *ret),
    }
}

fn row_has_error(types: &Arena<TypeRef>, row: &RowRef) -> bool {
    match row {
        RowRef::Closed { effects, .. } | RowRef::Open { effects, .. } => effects
            .iter()
            .any(|effect| effect.args.iter().any(|&arg| has_error(types, arg))),
        RowRef::Omitted => false,
        RowRef::Error => true,
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

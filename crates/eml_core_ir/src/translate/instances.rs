//! 単相化の instance の表 (docs/spec/core-ir.md の「変換の規則」)。本体を変換する前に、入口から届く (関数, 型引数,
//! row のビット) の組と生成する関数をすべて集め、番号の順を決める。変換はこの表を読むだけで、変換の途中で instance
//! を足さない。

use std::collections::{HashMap, HashSet, VecDeque};

use eml_extern::Extern;
use eml_hir::{
    CoreMethod, ExprId, ExprKind, FunctionId, FunctionKind, InputKind, InstanceOrigin, MethodId,
    MethodImpl, ModuleOrigin, Program as HirProgram, TypeDefKind, ValueItem,
};
use eml_types::{
    BodyTypes, InputDisplay, InstanceNode, Instantiation, Lin, Resolution, RowYield, Substitution,
    TypeId, TypeKind, TypeNames, TypeStore, TypedProgram, Update,
};
use la_arena::ArenaMap;

use super::names::{NameTable, args_hash};
use super::program::core_name;
use super::types::split_arrows;

/// `Instances::list` の位置。変換はこの順に関数の番号を予約する。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct InstanceId(pub(super) usize);

/// 定義された関数かメソッドへの参照の行き先。
#[derive(Clone, Copy, Debug)]
pub(super) enum Target {
    Function(InstanceId),
    /// `extern` で結んだ instance のメソッド。
    Extern(Extern),
}

pub(super) struct Instance {
    pub(super) name: String,
    pub(super) kind: InstanceKind,
}

pub(super) enum InstanceKind {
    /// HIR の関数の本体を変換する instance。
    Function(FunctionInstance),
    /// translate が Core IR を組む関数 (`derive.rs`)。
    Generated(GeneratedInstance),
}

pub(super) struct FunctionInstance {
    pub(super) function: FunctionId,
    /// シグネチャの型変数の順の型引数。代入と一様な位置を反映した鍵である。
    #[expect(
        dead_code,
        reason = "名前は `Found` の型引数から作るので、今は読む所がない。証拠を決める段で読む"
    )]
    pub(super) args: Vec<TypeId>,
    /// シグネチャの row 変数の順の、row が yield しうるかのビット。鍵の一部である。
    pub(super) bits: Vec<bool>,
    /// 宣言の型に instance の代入をかけた型。引数と `ret` の Repr を決める。
    pub(super) signature: TypeId,
    /// 本体の型に代入をかけた表。`None` なら型検査の表をそのまま使う。
    pub(super) types: Option<Box<BodyTypes>>,
    /// 本体の中の、定義された関数とメソッドへの参照の行き先。`extern` で結んだ instance のメソッドへの参照は
    /// `Target::Extern` で入る。extern の関数への参照は入らない。
    pub(super) targets: ArenaMap<ExprId, Target>,
}

pub(super) struct GeneratedInstance {
    pub(super) generated: Generated,
    /// 生成する関数が呼ぶ行き先。コンストラクタの順、フィールドの順に並ぶ (`Collector::generated_calls`)。
    pub(super) calls: Vec<Target>,
    /// `compare` が違うコンストラクタの組のタグを比べるために呼ぶ `tag$`。コンストラクタが2つ以上ある型の `compare`
    /// だけが持つ。
    pub(super) tag: Option<Target>,
    /// コンストラクタごとのフィールドの型。タプルは要素の型を持つ1つのコンストラクタで、`Unit` は空である。
    pub(super) fields: Vec<Vec<TypeId>>,
}

/// 生成する関数 (docs/spec/core-ir.md の「導出とタプルの生成器」)。
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Generated {
    pub(super) node: InstanceNode,
    pub(super) method: GeneratedMethod,
    /// 頭の型引数。タプルなら要素の型である。
    pub(super) args: Vec<TypeId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum GeneratedMethod {
    Eq,
    Compare,
    ShowPrec,
    Show,
    /// `compare` がタグを比べるための、タグの番号を返す補助の関数。
    Tag,
}

impl GeneratedMethod {
    pub(super) fn arity(self) -> usize {
        match self {
            GeneratedMethod::Eq | GeneratedMethod::Compare | GeneratedMethod::ShowPrec => 2,
            GeneratedMethod::Show | GeneratedMethod::Tag => 1,
        }
    }
}

impl From<CoreMethod> for GeneratedMethod {
    fn from(method: CoreMethod) -> GeneratedMethod {
        match method {
            CoreMethod::Eq => GeneratedMethod::Eq,
            CoreMethod::Compare => GeneratedMethod::Compare,
            CoreMethod::ShowPrec => GeneratedMethod::ShowPrec,
            CoreMethod::Show => GeneratedMethod::Show,
        }
    }
}

pub(super) struct Instances {
    pub(super) list: Vec<Instance>,
    pub(super) entry: InstanceId,
    /// 入口が `Show` を持つ型の式の入力なら、その型の `show` の行き先。入口の関数 `entry$` が結果の表示に使う。
    /// 入力の本体にない呼び出しなので、参照の行き先 (`targets`) とは別に持つ
    /// (docs/spec/core-ir.md の「REPL の入力」)。
    pub(super) show: Option<Target>,
    /// 型検査の型の表に、代入の結果を足した表。
    pub(super) store: TypeStore,
}

/// 見つけた順の instance。番号の順は、すべて見つけてから決める。
enum Found {
    Function(FoundFunction),
    Generated {
        generated: Generated,
        calls: Vec<FoundTarget>,
        tag: Option<FoundTarget>,
        fields: Vec<Vec<TypeId>>,
    },
}

struct FoundFunction {
    function: FunctionId,
    args: Vec<TypeId>,
    bits: Vec<bool>,
    signature: TypeId,
    types: Option<Box<BodyTypes>>,
    targets: ArenaMap<ExprId, FoundTarget>,
}

/// `Target` の、番号を付け直す前の形。関数の行き先は `found` の位置で持つ。
#[derive(Clone, Copy)]
enum FoundTarget {
    Function(usize),
    Extern(Extern),
}

impl FoundTarget {
    fn renumber(self, renumbered: &[usize]) -> Target {
        match self {
            FoundTarget::Function(index) => Target::Function(InstanceId(renumbered[index])),
            FoundTarget::Extern(row) => Target::Extern(row),
        }
    }
}

/// 入口から届く instance を集める途中の状態。
struct Collector<'a> {
    hir: &'a HirProgram,
    typed: &'a TypedProgram,
    store: TypeStore,
    found: Vec<Found>,
    keys: HashMap<(FunctionId, Vec<TypeId>, Vec<bool>), usize>,
    generated_keys: HashMap<Generated, usize>,
    queue: VecDeque<usize>,
}

impl Collector<'_> {
    /// (関数, 型引数, row のビット) の instance の位置。初めての組なら足して、本体をたどる列に入れる。`signature` は
    /// 宣言の型のまま入れ、本体をたどるときに代入をかけた型に置き換える。
    fn add(&mut self, function: FunctionId, args: Vec<TypeId>, bits: Vec<bool>) -> usize {
        let Collector {
            typed,
            found,
            keys,
            queue,
            ..
        } = self;
        *keys
            .entry((function, args.clone(), bits.clone()))
            .or_insert_with(|| {
                found.push(Found::Function(FoundFunction {
                    function,
                    args,
                    bits,
                    signature: typed.decls[&ValueItem::Function(function)].ty,
                    types: None,
                    targets: ArenaMap::default(),
                }));
                queue.push_back(found.len() - 1);
                found.len() - 1
            })
    }

    /// 型引数 `args` と row のビット `bits` の参照の行き先 `function` の instance。一様な位置の型引数は `Flexible`
    /// にする。row のビットには一様な位置を当てはめない (docs/spec/core-ir.md
    /// の「row のビットと instance の鍵」)。
    fn add_uniform(&mut self, function: FunctionId, args: &[TypeId], bits: Vec<bool>) -> usize {
        let key = args
            .iter()
            .enumerate()
            .map(|(position, &arg)| {
                if self.typed.uniform.function(function, position) {
                    let kind = self.typed.uniform_kind(function, position);
                    self.store.flexible(kind)
                } else {
                    arg
                }
            })
            .collect();
        self.add(function, key, bits)
    }

    /// 生成する関数の位置。鍵の型引数は、instance の節点の一様な位置で `Flexible` にする。初めての鍵なら足して、
    /// 呼ぶ行き先を解く列に入れる。
    fn add_generated(&mut self, generated: Generated) -> usize {
        let Generated { node, method, args } = generated;
        let args = args
            .iter()
            .enumerate()
            .map(|(position, &arg)| {
                if self.typed.uniform.instance(node, position) {
                    self.store.flexible(Lin::Poly)
                } else {
                    arg
                }
            })
            .collect();
        let key = Generated { node, method, args };
        let Collector {
            found,
            generated_keys,
            queue,
            ..
        } = self;
        *generated_keys.entry(key.clone()).or_insert_with(|| {
            found.push(Found::Generated {
                generated: key,
                calls: Vec::new(),
                tag: None,
                fields: Vec::new(),
            });
            queue.push_back(found.len() - 1);
            found.len() - 1
        })
    }

    /// メソッドの参照の行き先 (docs/spec/core-ir.md の「メソッドの解決」)。
    /// `args` は代入をかけた後の型引数で、先頭がクラスの型変数の型である。型検査が制約をすべて具体的な型で解いて
    /// いるので、ここで `Given` と `Missing` に出会うことはない。導出した instance とタプルの instance の中心の
    /// メソッドは生成する関数へ、ほかのメソッドはクラスの既定のメソッドへ向かう。`bits` はメソッドのシグネチャの row
    /// 変数の順のビットである。instance のメソッドの関数と既定のメソッドの関数は同じ順の row 変数を持つので、そのまま
    /// 渡す。生成する関数は row 変数を持たない。
    fn method_target(&mut self, method: MethodId, args: &[TypeId], bits: Vec<bool>) -> FoundTarget {
        let hir = self.hir;
        let class = hir[method].class;
        let core = hir.core_method(method);
        // 解決に `Flexible` が要る参照は、型検査が E2006、E2009、E2012 で止めている
        // (docs/spec/core-ir.md の「メソッドの解決」)。`resolve` は `Flexible` を `Missing` と答えるので、先に分けて止める
        assert!(
            !matches!(self.store.kind(args[0]), TypeKind::Flexible(_)),
            "internal error: the reference to `{}` resolves a constraint at a uniform (`Flexible`) type, which the type checker rejects",
            hir[method].name
        );
        match eml_types::resolve(hir, &self.store, class, args[0]) {
            Resolution::Instance {
                instance,
                args: head,
            } => match (hir[instance].origin, core) {
                (InstanceOrigin::Derived(_), Some(core)) => {
                    FoundTarget::Function(self.add_generated(Generated {
                        node: InstanceNode::Declared(instance),
                        method: core.into(),
                        args: head,
                    }))
                }
                _ => match hir[instance].method(method) {
                    Some(MethodImpl::Function(function)) => {
                        let key: Vec<TypeId> = head.iter().chain(&args[1..]).copied().collect();
                        FoundTarget::Function(self.add_uniform(function, &key, bits))
                    }
                    Some(MethodImpl::Extern(row)) => FoundTarget::Extern(row),
                    None => self.default_target(method, args, bits),
                },
            },
            Resolution::Tuple(elements) => match core {
                Some(core) => FoundTarget::Function(self.add_generated(Generated {
                    node: InstanceNode::Tuple(elements.len()),
                    method: core.into(),
                    args: elements,
                })),
                None => self.default_target(method, args, bits),
            },
            other @ (Resolution::Given | Resolution::Missing) => unreachable!(
                "the type checker resolves every method reference at a concrete type, found {other:?}"
            ),
        }
    }

    fn default_target(
        &mut self,
        method: MethodId,
        args: &[TypeId],
        bits: Vec<bool>,
    ) -> FoundTarget {
        let default = self.hir[method]
            .default
            .expect("HIR reports a missing method without a default (E1036)");
        FoundTarget::Function(self.add_uniform(default, args, bits))
    }

    /// HIR の関数の instance の本体をたどり、代入をかけた型と参照の行き先を記録する。
    fn function_body(&mut self, index: usize) {
        let (hir, typed) = (self.hir, self.typed);
        let Found::Function(found) = &self.found[index] else {
            unreachable!("called for a function instance")
        };
        let function = found.function;
        let bits = found.bits.clone();
        let body = typed
            .bodies
            .get(function)
            .expect("every reached body is type-checked");
        let names = type_vars(hir, function);
        debug_assert_eq!(
            names.len(),
            found.args.len(),
            "an instance key must supply one type argument per type variable of the signature"
        );
        debug_assert_eq!(
            row_vars(hir, function).len(),
            bits.len(),
            "an instance key must supply one row bit per row variable of the signature"
        );
        let kinds = typed.kind_values(ValueItem::Function(function), &found.args, &self.store);
        let mut subst =
            Substitution::new(names.iter().cloned().zip(found.args.iter().copied()), kinds);
        let decl = typed.decls[&ValueItem::Function(function)].ty;
        let signature = self.store.substitute(decl, &mut subst);
        let mut targets = ArenaMap::default();
        let mut instantiations = ArenaMap::default();
        for (expr, instantiation) in body.instantiations.iter() {
            // 節の型変数 (`OpVar`) は、型変数を持たない関数でも `Flexible` にそろえる
            let args: Vec<TypeId> = instantiation
                .args
                .iter()
                .map(|&arg| self.store.substitute(arg, &mut subst))
                .collect();
            let row_bits = || {
                instantiation
                    .rows
                    .iter()
                    .map(|&row| resolve(row, &bits))
                    .collect()
            };
            match instantiation.decl {
                ValueItem::Function(callee) if hir[callee].kind == FunctionKind::Defined => {
                    let target = self.add_uniform(callee, &args, row_bits());
                    targets.insert(expr, FoundTarget::Function(target));
                }
                ValueItem::Method(method) => {
                    targets.insert(expr, self.method_target(method, &args, row_bits()));
                }
                _ => {}
            }
            instantiations.insert(
                expr,
                Instantiation {
                    decl: instantiation.decl,
                    args,
                    rows: instantiation.rows.clone(),
                },
            );
        }
        // 型変数も Kind 変数も持たない関数の本体の型に現れる変数は、節の `OpVar` だけである。`OpVar` の Repr は
        // `tobj` で、その Kind は `linearity` が直接読めるので、表は写さずに型検査の表を使う
        // (docs/spec/core-ir.md の「変換の規則」)
        let copy = typed.needs_substitution(ValueItem::Function(function));
        // 写さずに使えるのは、型検査が宣言自身の変数を、宣言の型と同じく本体の型でも定数に畳むからである
        // (docs/spec/types.md の「書き出すときの規則」)
        debug_assert!(
            copy || !keeps_kind_vars(body, &self.store),
            "a body used without a copy keeps a Kind variable"
        );
        let types = copy.then(|| {
            let store = &mut self.store;
            let mut each = |ty: TypeId| store.substitute(ty, &mut subst);
            let types = BodyTypes {
                exprs: body
                    .exprs
                    .iter()
                    .map(|(expr, &ty)| (expr, each(ty)))
                    .collect(),
                locals: body
                    .locals
                    .iter()
                    .map(|(local, &ty)| (local, each(ty)))
                    .collect(),
                pats: body.pats.iter().map(|(pat, &ty)| (pat, each(ty))).collect(),
                instantiations,
                masks: body.masks.clone(),
                fields: body.fields.clone(),
                updates: body
                    .updates
                    .iter()
                    .map(|(expr, update)| {
                        let update = Update {
                            ctor: update.ctor,
                            written: update.written.clone(),
                            fields: update.fields.iter().map(|&ty| each(ty)).collect(),
                        };
                        (expr, update)
                    })
                    .collect(),
                discarded: body
                    .discarded
                    .iter()
                    .map(|(expr, discarded)| {
                        let discarded = discarded
                            .iter()
                            .map(|&(field, ty)| (field, each(ty)))
                            .collect();
                        (expr, discarded)
                    })
                    .collect(),
                body_yield: body.body_yield,
                first_arrow_yield: body.first_arrow_yield,
                lambda_yields: body.lambda_yields.clone(),
                arrow_yields: body.arrow_yields.clone(),
            };
            debug_assert!(
                types
                    .exprs
                    .values()
                    .chain(types.locals.values())
                    .chain(types.pats.values())
                    .chain(
                        types
                            .instantiations
                            .values()
                            .flat_map(|instantiation| &instantiation.args)
                    )
                    .all(|&ty| !self.store.contains_type_vars(ty)
                        && !self.store.contains_kind_vars(ty)),
                "an instance body keeps no type variable or Kind variable"
            );
            Box::new(types)
        });
        let Found::Function(found) = &mut self.found[index] else {
            unreachable!("checked above")
        };
        found.signature = signature;
        found.targets = targets;
        found.types = types;
    }

    /// 生成する関数のフィールドの型と、それが呼ぶ行き先を、コンストラクタの順、フィールドの順に解く
    /// (`Eq` は各フィールドの型の `==`、`Compare` は `compare`、`ShowPrec` は `show_prec`、`Show` は同じ型の
    /// `ShowPrec`)。コンストラクタが2つ以上ある型の `Compare` は、`Tag` も別に解く。
    fn generated_calls(&mut self, index: usize) {
        let hir = self.hir;
        let Found::Generated { generated, .. } = &self.found[index] else {
            unreachable!("called for a generated function")
        };
        let generated = generated.clone();
        let fields = self.field_types(&generated);
        let core = |core: CoreMethod| {
            let lang = &hir.lang;
            let class = match core {
                CoreMethod::Eq => lang.eq,
                CoreMethod::Compare => lang.ord,
                CoreMethod::ShowPrec | CoreMethod::Show => lang.show,
            };
            *hir[class]
                .methods
                .iter()
                .find(|&&method| hir.core_method(method) == Some(core))
                .expect("the Prelude classes declare their core methods")
        };
        let each_field = |collector: &mut Self, method: MethodId| -> Vec<FoundTarget> {
            fields
                .iter()
                .flatten()
                .map(|&field| collector.method_target(method, &[field], Vec::new()))
                .collect()
        };
        let mut tag = None;
        let calls = match generated.method {
            GeneratedMethod::Eq => each_field(self, core(CoreMethod::Eq)),
            GeneratedMethod::Compare => {
                let calls = each_field(self, core(CoreMethod::Compare));
                if fields.len() > 1 {
                    tag = Some(FoundTarget::Function(self.add_generated(Generated {
                        node: generated.node,
                        method: GeneratedMethod::Tag,
                        args: Vec::new(),
                    })));
                }
                calls
            }
            GeneratedMethod::ShowPrec => each_field(self, core(CoreMethod::ShowPrec)),
            GeneratedMethod::Show => {
                let show_prec = self.add_generated(Generated {
                    method: GeneratedMethod::ShowPrec,
                    ..generated.clone()
                });
                vec![FoundTarget::Function(show_prec)]
            }
            GeneratedMethod::Tag => Vec::new(),
        };
        let Found::Generated {
            calls: slot,
            tag: tag_slot,
            fields: field_slot,
            ..
        } = &mut self.found[index]
        else {
            unreachable!("checked above")
        };
        *slot = calls;
        *tag_slot = tag;
        *field_slot = fields;
    }

    /// コンストラクタごとのフィールドの型。コンストラクタの宣言の型に、`data` の型引数の名前から `args` への代入を
    /// かけて、矢印の引数として取り出す。型引数を鍵に持たない `Tag` は、足りない型引数を一様な型 (`Flexible(Poly)`) で埋める。
    /// フィールドの型は Repr にしか使わないためである。
    fn field_types(&mut self, generated: &Generated) -> Vec<Vec<TypeId>> {
        let hir = self.hir;
        let instance = match generated.node {
            InstanceNode::Tuple(0) => return Vec::new(),
            InstanceNode::Tuple(_) => return vec![generated.args.clone()],
            InstanceNode::Declared(instance) => instance,
        };
        let ty = hir[instance].head;
        let TypeDefKind::Data { constructors } = &hir[ty].kind else {
            unreachable!("only data types derive instances")
        };
        let names = hir[ty]
            .generics
            .type_vars
            .values()
            .map(|var| var.name.clone());
        let names: Vec<String> = names.collect();
        // `Tag` は型引数を鍵に持たない。フィールドの型は Repr にしか使わないので、足りない分は一様な型で埋める
        let mut args = generated.args.clone();
        let uniform = self.store.flexible(Lin::Poly);
        args.resize(names.len(), uniform);
        constructors
            .iter()
            .map(|&constructor| {
                let item = ValueItem::Constructor(constructor);
                let kinds = self.typed.kind_values(item, &args, &self.store);
                let mut subst =
                    Substitution::new(names.iter().cloned().zip(args.iter().copied()), kinds);
                let declared = self.typed.decls[&item].ty;
                let ty = self.store.substitute(declared, &mut subst);
                split_arrows(&self.store, ty, hir[constructor].fields.len()).0
            })
            .collect()
    }

    /// 関数の instance ごとに、参照の型と呼ばれる instance の型、`perform` の型と操作の宣言の型、節の型と操作の宣言の
    /// 型を比べ、`unr` と `lin` が食い違わないことを確かめる。この食い違いを最初に拒むのは S8d の V2 なので、それより
    /// 前に書き出しか評価が誤れば気づけない。そのため debug ビルドで確かめる
    /// (docs/spec/core-ir.md の「食い違いがないことの検査」)。
    fn check_linearity(&mut self) {
        let Collector {
            hir,
            typed,
            store,
            found: list,
            ..
        } = self;
        let (hir, typed) = (*hir, *typed);
        for found in list.iter() {
            let Found::Function(found) = found else {
                continue;
            };
            let function = found.function;
            let types = found.types.as_deref().unwrap_or_else(|| {
                typed
                    .bodies
                    .get(function)
                    .expect("every reached body is type-checked")
            });
            let instance = |store: &TypeStore| {
                format!(
                    "`{}` at `{}`",
                    hir[function].name,
                    store.display_linear(found.signature, &hir.names)
                )
            };
            for (expr, &target) in found.targets.iter() {
                // 生成器の instance と extern は比べる型を持たない。extern は S8d の Kind の列で確かめる
                let FoundTarget::Function(index) = target else {
                    continue;
                };
                let Found::Function(callee) = &list[index] else {
                    continue;
                };
                assert!(
                    store.lin_compatible(types.exprs[expr], callee.signature),
                    "internal error: in {}, a reference of type `{}` meets unr with lin in its callee `{}` at `{}`",
                    instance(store),
                    store.display_linear(types.exprs[expr], &hir.names),
                    hir[callee.function].name,
                    store.display_linear(callee.signature, &hir.names),
                );
            }
            for (expr, instantiation) in types.instantiations.iter() {
                let ValueItem::Operation(op) = instantiation.decl else {
                    continue;
                };
                let names = hir[op]
                    .signature
                    .generics
                    .type_vars
                    .iter()
                    .map(|(_, var)| var.name.clone());
                let kinds = typed.kind_values(instantiation.decl, &instantiation.args, store);
                let mut subst =
                    Substitution::new(names.zip(instantiation.args.iter().copied()), kinds);
                let declared = store.substitute(typed.decls[&instantiation.decl].ty, &mut subst);
                assert!(
                    store.lin_compatible(types.exprs[expr], declared),
                    "internal error: in {}, a reference of type `{}` meets unr with lin in operation `{}` at `{}`",
                    instance(store),
                    store.display_linear(types.exprs[expr], &hir.names),
                    hir[op].name,
                    store.display_linear(declared, &hir.names),
                );
            }
            let body = hir
                .body(function)
                .expect("every reached function has a body");
            for (_, expr) in body.exprs.iter() {
                let ExprKind::Handle { clauses, .. } = &expr.kind else {
                    continue;
                };
                for clause in clauses {
                    // 節の具体化は記録されないので、宣言の型を代入せずに比べる。`Rigid` はどの型とも合い、`Var` は
                    // `Poly` として比べる
                    let declared = typed.decls[&ValueItem::Operation(clause.op)].ty;
                    let (params, ret) = split_arrows(store, declared, clause.arity);
                    let mut pairs: Vec<(TypeId, TypeId)> = clause
                        .args()
                        .iter()
                        .map(|&pat| types.pats[pat])
                        .zip(params)
                        .collect();
                    // `k` は操作の結果を最初の引数で受ける
                    if let Some(k) = clause.k()
                        && let TypeKind::Fn { param, .. } = store.kind(types.pats[k])
                    {
                        pairs.push((*param, ret));
                    }
                    for (clause_ty, declared_ty) in pairs {
                        assert!(
                            store.lin_compatible(clause_ty, declared_ty),
                            "internal error: in {}, a clause of `{}` takes `{}`, which meets unr with lin in the declared `{}`",
                            instance(store),
                            hir[clause.op].name,
                            store.display_linear(clause_ty, &hir.names),
                            store.display_linear(declared_ty, &hir.names),
                        );
                    }
                }
            }
        }
    }
}

pub(super) fn collect(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId) -> Instances {
    // 制約を持つ関数の証拠は呼び出し側の型で決まるので、入口にはできない。入口を選ぶのは translate の呼び出し側で
    // ある (docs/spec/core-ir.md の「メソッドの解決」)
    assert!(
        hir[entry]
            .signature
            .as_ref()
            .is_none_or(|signature| signature.constraints.is_empty()),
        "the entry of translate has no constraints"
    );
    let mut collector = Collector {
        hir,
        typed,
        store: typed.types.clone(),
        found: Vec::new(),
        keys: HashMap::new(),
        generated_keys: HashMap::new(),
        queue: VecDeque::new(),
    };
    // 入口は呼び出し側が選ぶので、型変数を持つ関数なら、どの型でも動く一様な instance にする
    let entry_args = (0..type_vars(hir, entry).len())
        .map(|position| {
            let kind = typed.uniform_kind(entry, position);
            collector.store.flexible(kind)
        })
        .collect();
    // row のビットも呼び出し側が決めるので、安全側に倒してすべて立てる
    // (docs/spec/core-ir.md の「row のビットと instance の鍵」)
    let entry_bits = vec![true; row_vars(hir, entry).len()];
    let entry_index = collector.add(entry, entry_args, entry_bits);
    let show = match (hir[entry].kind, typed.inputs.get(entry)) {
        (FunctionKind::Input(InputKind::Expr), Some(input))
            if input.display == InputDisplay::Show =>
        {
            // `show` は row 変数を持たない
            Some(collector.method_target(hir.lang.show_method, &[input.ty], Vec::new()))
        }
        _ => None,
    };
    // 先に見つけた instance から順にたどる。番号の順と名前が変換の順によらないようにするため
    while let Some(index) = collector.queue.pop_front() {
        match collector.found[index] {
            Found::Function(_) => collector.function_body(index),
            Found::Generated { .. } => collector.generated_calls(index),
        }
    }
    // 呼ばれる instance の `signature` は、その本体をたどった後に代入をかけた型になるので、すべてたどり終えてから比べる
    if cfg!(debug_assertions) {
        collector.check_linearity();
    }
    order(hir, collector.found, entry_index, show, collector.store)
}

/// 本体の型の表のどれかが Kind 変数 (`Var`) を含むか。
fn keeps_kind_vars(body: &BodyTypes, store: &TypeStore) -> bool {
    body.exprs
        .values()
        .chain(body.locals.values())
        .chain(body.pats.values())
        .chain(
            body.instantiations
                .values()
                .flat_map(|instantiation| &instantiation.args),
        )
        .chain(body.updates.values().flat_map(|update| &update.fields))
        .chain(
            body.discarded
                .values()
                .flat_map(|discarded| discarded.iter().map(|(_, ty)| ty)),
        )
        .any(|&ty| store.contains_kind_vars(ty))
}

/// シグネチャの型変数の名前。並びは具体化の表の型引数の順と同じである。
fn type_vars(hir: &HirProgram, function: FunctionId) -> Vec<String> {
    hir[function]
        .signature
        .as_ref()
        .map(|signature| {
            signature
                .generics
                .type_vars
                .iter()
                .map(|(_, var)| var.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// シグネチャの row 変数の名前。並びは具体化の表の `rows` と、instance の row のビットの順と同じである。
fn row_vars(hir: &HirProgram, function: FunctionId) -> Vec<String> {
    hir[function]
        .signature
        .as_ref()
        .map(|signature| {
            signature
                .generics
                .row_vars
                .iter()
                .map(|(_, var)| var.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// 参照の row の分類を、参照を含む instance の row のビットで解く。立てば、その row で動く関数は yield しうる。
pub(super) fn resolve(row: RowYield, bits: &[bool]) -> bool {
    match row {
        RowYield::Never => false,
        RowYield::Always => true,
        RowYield::Var(i) => bits[i as usize],
    }
}

/// instance の名前の後ろに付ける、立ったビットの row 変数を `<e, f>` の形で並べた部分。ビットが立たなければ空である
/// (docs/spec/core-ir.md の「row のビットと instance の鍵」)。
fn row_suffix(names: &[String], bits: &[bool]) -> String {
    let set: Vec<&str> = names
        .iter()
        .zip(bits)
        .filter(|&(_, &bit)| bit)
        .map(|(name, _)| name.as_str())
        .collect();
    if set.is_empty() {
        String::new()
    } else {
        format!("<{}>", set.join(", "))
    }
}

/// `@[` と `]` の間の上限の文字数。部分を共有する型では表示が指数の長さになるので、超えたらハッシュの名前にする。
const NAME_LIMIT: usize = 64;

/// 型変数を持つ関数の instance の名前 (docs/spec/core-ir.md の「変換の規則」)。名前は型引数だけで決め、ほかの
/// instance の有無や見つけた順によらない。同じ名前の関数がいつも同じコードになるためである。`suffix` は row のビットの
/// 部分 (`row_suffix`) で、どちらの形の名前にも付ける。
fn instance_name(
    store: &TypeStore,
    names: &NameTable,
    base: &str,
    args: &[TypeId],
    suffix: &str,
) -> String {
    let shown: Option<Vec<String>> = args
        .iter()
        .map(|&arg| store.display_name_bounded(arg, names, NAME_LIMIT))
        .collect();
    let inner = shown
        .map(|shown| shown.join(", "))
        .filter(|inner| inner.chars().count() <= NAME_LIMIT);
    match inner {
        Some(inner) => format!("{base}@[{inner}]{suffix}"),
        None => format!("{base}@#{:016x}{suffix}", args_hash(store, names, args)),
    }
}

/// 生成する関数の、鍵を付ける前の名前 (docs/spec/core-ir.md の「メソッドの解決」)。
/// 手で書いた instance のメソッドと同じ形で、導出した instance は型を定義したモジュールで、タプルと `Unit` は
/// Prelude で修飾する。補助の `tag$` は `con$` と同じ修飾の規則に従う。
fn generated_base(hir: &HirProgram, names: &NameTable, generated: &Generated) -> String {
    let (class, method) = match generated.method {
        GeneratedMethod::Eq => (hir.lang.eq, "=="),
        GeneratedMethod::Compare => (hir.lang.ord, "compare"),
        GeneratedMethod::ShowPrec => (hir.lang.show, "show_prec"),
        GeneratedMethod::Show => (hir.lang.show, "show"),
        GeneratedMethod::Tag => {
            let InstanceNode::Declared(instance) = generated.node else {
                unreachable!("tuples have one constructor and no tags to compare")
            };
            let ty = hir[instance].head;
            return format!("tag${}", core_name(hir, ty.module, &hir[ty].name));
        }
    };
    let class = names.class(class);
    match generated.node {
        InstanceNode::Declared(instance) => {
            let ty = hir[instance].head;
            let name = format!("{class} {}.{method}", names.ty(ty));
            core_name(hir, ty.module, &name)
        }
        InstanceNode::Tuple(count) => {
            let ty = format!("({})", ",".repeat(count.saturating_sub(1)));
            core_name(hir, hir.prelude, &format!("{class} {ty}.{method}"))
        }
    }
}

/// HIR の関数の、鍵を付ける前の名前。セッションの instance のメソッドは、HIR の名前でなく、型とクラスの中段の名前で
/// 書く。HIR の名前は利用者向けの表示名で、型を定義し直すと古い instance のメソッドの名前が `Show Color#1.show` に
/// 変わり、新しい instance のメソッドが古い名前を受け継ぐためである
/// (docs/spec/core-ir.md の「REPL の入力」)。
fn function_base(hir: &HirProgram, names: &NameTable, function: FunctionId) -> String {
    let module = function.module;
    match hir[function].kind {
        FunctionKind::InstanceMethod(instance, method)
            if matches!(hir.modules[module].origin, ModuleOrigin::Session(_)) =>
        {
            let class = names.class(hir[instance].class);
            let name = format!(
                "{class} {}.{}",
                names.ty(hir[instance].head),
                hir[method].name
            );
            core_name(hir, module, &name)
        }
        _ => core_name(hir, module, &hir[function].name),
    }
}

/// HIR の関数の instance を HIR の関数の順に並べ、同じ関数の instance は見つけた順に並べる。生成する関数は、その後に
/// 見つけた順に並べる。`TypeId` の値の順には並べない。後の段階が `TypeId` の値に意味を持たせないためである。
fn order(
    hir: &HirProgram,
    found: Vec<Found>,
    entry: usize,
    show: Option<FoundTarget>,
    store: TypeStore,
) -> Instances {
    let position: HashMap<FunctionId, usize> = hir
        .functions()
        .enumerate()
        .map(|(position, (id, _))| (id, position))
        .collect();
    let mut sorted: Vec<usize> = (0..found.len()).collect();
    sorted.sort_by_key(|&index| match &found[index] {
        Found::Function(found) => (0, position[&found.function], index),
        Found::Generated { .. } => (1, 0, index),
    });
    let mut renumbered = vec![0; found.len()];
    for (new, &old) in sorted.iter().enumerate() {
        renumbered[old] = new;
    }
    let names = NameTable::new(hir);
    let mut found: Vec<Option<Found>> = found.into_iter().map(Some).collect();
    let list: Vec<Instance> = sorted
        .iter()
        .map(
            |&old| match found[old].take().expect("each instance is taken once") {
                Found::Function(found) => {
                    let base = function_base(hir, &names, found.function);
                    let suffix = row_suffix(&row_vars(hir, found.function), &found.bits);
                    let name = if type_vars(hir, found.function).is_empty() {
                        format!("{base}{suffix}")
                    } else {
                        instance_name(&store, &names, &base, &found.args, &suffix)
                    };
                    Instance {
                        name,
                        kind: InstanceKind::Function(FunctionInstance {
                            function: found.function,
                            args: found.args,
                            bits: found.bits,
                            signature: found.signature,
                            types: found.types,
                            targets: found
                                .targets
                                .iter()
                                .map(|(expr, &target)| (expr, target.renumber(&renumbered)))
                                .collect(),
                        }),
                    }
                }
                Found::Generated {
                    generated,
                    calls,
                    tag,
                    fields,
                } => {
                    let base = generated_base(hir, &names, &generated);
                    let name = if generated.args.is_empty() {
                        base
                    } else {
                        instance_name(&store, &names, &base, &generated.args, "")
                    };
                    Instance {
                        name,
                        kind: InstanceKind::Generated(GeneratedInstance {
                            generated,
                            calls: calls
                                .iter()
                                .map(|target| target.renumber(&renumbered))
                                .collect(),
                            tag: tag.map(|target| target.renumber(&renumbered)),
                            fields,
                        }),
                    }
                }
            },
        )
        .collect();
    // 名前は関数を照らす鍵なので、重なれば別の関数が同じコードとして扱われる (docs/spec/core-ir.md の「変換の規則」)
    debug_assert!(
        {
            let mut seen = HashSet::new();
            list.iter()
                .all(|instance| seen.insert(instance.name.as_str()))
        },
        "two instances have the same name"
    );
    Instances {
        list,
        entry: InstanceId(renumbered[entry]),
        show: show.map(|target| target.renumber(&renumbered)),
        store,
    }
}

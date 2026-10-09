//! 単相化の instance の表 (docs/spec/core-ir.md の「変換の規則」)。本体を変換する前に、入口から届く (関数, 型引数)
//! の組と生成する関数をすべて集め、番号の順を決める。変換はこの表を読むだけで、変換の途中で instance を足さない。

use std::collections::{HashMap, HashSet, VecDeque};

use eml_extern::Extern;
use eml_hir::{
    CoreMethod, ExprId, FunctionId, FunctionKind, InstanceOrigin, MethodId, MethodImpl,
    Program as HirProgram, TypeDefKind, ValueItem,
};
use eml_types::{
    BodyTypes, InstanceNode, Instantiation, Resolution, Substitution, TypeId, TypeStore,
    TypedProgram,
};
use la_arena::ArenaMap;

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
    /// 宣言の型に instance の代入をかけた型。引数と `ret` の Repr を決める。
    pub(super) signature: TypeId,
    /// 本体の型に代入をかけた表。`None` なら型検査の表をそのまま使う。
    pub(super) types: Option<BodyTypes>,
    /// 本体の中の、定義された関数とメソッドへの参照の行き先。extern の関数への参照は入らない。
    pub(super) targets: ArenaMap<ExprId, Target>,
}

pub(super) struct GeneratedInstance {
    pub(super) generated: Generated,
    /// 生成する関数が呼ぶ行き先。コンストラクタの順、フィールドの順に並ぶ (`Collector::generated_calls`)。
    pub(super) calls: Vec<Target>,
    /// コンストラクタごとのフィールドの型。タプルは要素の型を持つ1つのコンストラクタで、`Unit` は空である。
    pub(super) fields: Vec<Vec<TypeId>>,
}

/// 生成する関数 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「導出とタプルの生成器」)。
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
    /// 型検査の型の表に、代入の結果を足した表。
    pub(super) store: TypeStore,
}

/// 見つけた順の instance。番号の順は、すべて見つけてから決める。
enum Found {
    Function(FoundFunction),
    Generated {
        generated: Generated,
        calls: Vec<FoundTarget>,
        fields: Vec<Vec<TypeId>>,
    },
}

struct FoundFunction {
    function: FunctionId,
    args: Vec<TypeId>,
    signature: TypeId,
    types: Option<BodyTypes>,
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
    keys: HashMap<(FunctionId, Vec<TypeId>), usize>,
    generated_keys: HashMap<Generated, usize>,
    queue: VecDeque<usize>,
}

impl Collector<'_> {
    /// (関数, 鍵) の instance の位置。初めての組なら足して、本体をたどる列に入れる。`signature` は宣言の型のまま
    /// 入れ、本体をたどるときに代入をかけた型に置き換える。
    fn add(&mut self, function: FunctionId, args: Vec<TypeId>) -> usize {
        let Collector {
            typed,
            found,
            keys,
            queue,
            ..
        } = self;
        *keys.entry((function, args.clone())).or_insert_with(|| {
            found.push(Found::Function(FoundFunction {
                function,
                args,
                signature: typed.decls[&ValueItem::Function(function)].ty,
                types: None,
                targets: ArenaMap::default(),
            }));
            queue.push_back(found.len() - 1);
            found.len() - 1
        })
    }

    /// 型引数 `args` の参照の行き先 `function` の instance。一様な位置の型引数は `Flexible` にする。
    fn add_uniform(&mut self, function: FunctionId, args: &[TypeId]) -> usize {
        let key = args
            .iter()
            .enumerate()
            .map(|(position, &arg)| {
                if self.typed.uniform.function(function, position) {
                    self.store.flexible()
                } else {
                    arg
                }
            })
            .collect();
        self.add(function, key)
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
                    self.store.flexible()
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
                fields: Vec::new(),
            });
            queue.push_back(found.len() - 1);
            found.len() - 1
        })
    }

    /// メソッドの参照の行き先 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「メソッドへの参照の解決」)。
    /// `args` は代入をかけた後の型引数で、先頭がクラスの型変数の型である。型検査が制約をすべて具体的な型で解いて
    /// いるので、ここで `Given` と `Missing` に出会うことはない。導出した instance とタプルの instance の中心の
    /// メソッドは生成する関数へ、ほかのメソッドはクラスの既定のメソッドへ向かう。
    fn method_target(&mut self, method: MethodId, args: &[TypeId]) -> FoundTarget {
        let hir = self.hir;
        let class = hir[method].class;
        let core = hir.core_method(method);
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
                        FoundTarget::Function(self.add_uniform(function, &key))
                    }
                    Some(MethodImpl::Extern(row)) => FoundTarget::Extern(row),
                    None => self.default_target(method, args),
                },
            },
            Resolution::Tuple(elements) => match core {
                Some(core) => FoundTarget::Function(self.add_generated(Generated {
                    node: InstanceNode::Tuple(elements.len()),
                    method: core.into(),
                    args: elements,
                })),
                None => self.default_target(method, args),
            },
            other @ (Resolution::Given | Resolution::Missing) => unreachable!(
                "the type checker resolves every method reference at a concrete type, found {other:?}"
            ),
        }
    }

    fn default_target(&mut self, method: MethodId, args: &[TypeId]) -> FoundTarget {
        let default = self.hir[method]
            .default
            .expect("HIR reports a missing method without a default (E1036)");
        FoundTarget::Function(self.add_uniform(default, args))
    }

    /// HIR の関数の instance の本体をたどり、代入をかけた型と参照の行き先を記録する。
    fn function_body(&mut self, index: usize) {
        let (hir, typed) = (self.hir, self.typed);
        let Found::Function(found) = &self.found[index] else {
            unreachable!("called for a function instance")
        };
        let function = found.function;
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
        let mut subst = Substitution::new(names.iter().cloned().zip(found.args.iter().copied()));
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
            match instantiation.decl {
                ValueItem::Function(callee) if hir[callee].kind == FunctionKind::Defined => {
                    let target = self.add_uniform(callee, &args);
                    targets.insert(expr, FoundTarget::Function(target));
                }
                ValueItem::Method(method) => {
                    targets.insert(expr, self.method_target(method, &args));
                }
                _ => {}
            }
            instantiations.insert(
                expr,
                Instantiation {
                    decl: instantiation.decl,
                    args,
                },
            );
        }
        // 型変数を持たない関数の本体の型に現れる型変数は、節の `OpVar` だけである。`OpVar` の Repr は `tobj` なので、
        // 表は写さずに型検査の表を使う
        let types = (!names.is_empty()).then(|| {
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
                    .all(|&ty| !self.store.contains_type_vars(ty)),
                "an instance body keeps no type variable"
            );
            types
        });
        let Found::Function(found) = &mut self.found[index] else {
            unreachable!("checked above")
        };
        found.signature = signature;
        found.targets = targets;
        found.types = types;
    }

    /// 生成する関数のフィールドの型と、それが呼ぶ行き先を、コンストラクタの順、フィールドの順に解く
    /// (`Eq` は各フィールドの型の `==`、`Compare` は `compare` と最後に `Tag`、`ShowPrec` は `show_prec`、`Show` は
    /// 同じ型の `ShowPrec`)。
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
                .map(|&field| collector.method_target(method, &[field]))
                .collect()
        };
        let calls = match generated.method {
            GeneratedMethod::Eq => each_field(self, core(CoreMethod::Eq)),
            GeneratedMethod::Compare => {
                let mut calls = each_field(self, core(CoreMethod::Compare));
                if fields.len() > 1 {
                    let tag = self.add_generated(Generated {
                        node: generated.node,
                        method: GeneratedMethod::Tag,
                        args: Vec::new(),
                    });
                    calls.push(FoundTarget::Function(tag));
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
            fields: field_slot,
            ..
        } = &mut self.found[index]
        else {
            unreachable!("checked above")
        };
        *slot = calls;
        *field_slot = fields;
    }

    /// コンストラクタごとのフィールドの型。コンストラクタの宣言の型に、`data` の型引数の名前から `args` への代入を
    /// かけて、矢印の引数として取り出す。型引数を持たない `Tag` は宣言の型のままで、Repr が配置と同じになる。
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
        let mut subst = Substitution::new(names.zip(generated.args.iter().copied()));
        constructors
            .iter()
            .map(|&constructor| {
                let declared = self.typed.decls[&ValueItem::Constructor(constructor)].ty;
                let ty = self.store.substitute(declared, &mut subst);
                split_arrows(&self.store, ty, hir[constructor].fields.len()).0
            })
            .collect()
    }
}

pub(super) fn collect(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId) -> Instances {
    // 制約を持つ関数の証拠は呼び出し側の型で決まるので、入口にはできない。入口を選ぶのは translate の呼び出し側で
    // ある (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「一様な位置と制約付きの多相再帰」)
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
    let entry_args = vec![collector.store.flexible(); type_vars(hir, entry).len()];
    let entry_index = collector.add(entry, entry_args);
    // 先に見つけた instance から順にたどる。番号の順と名前が変換の順によらないようにするため
    while let Some(index) = collector.queue.pop_front() {
        match collector.found[index] {
            Found::Function(_) => collector.function_body(index),
            Found::Generated { .. } => collector.generated_calls(index),
        }
    }
    order(hir, collector.found, entry_index, collector.store)
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

/// `@[` と `]` の間の上限の文字数。部分を共有する型では表示が指数の長さになるので、超えたら順番の名前にする。
const NAME_LIMIT: usize = 64;

/// 型変数を持つ関数の instance の名前 (docs/spec/core-ir.md の「変換の規則」)。`ordinal` は、その関数の instance の
/// 中の順番 (1 から数える) である。
fn instance_name(
    store: &TypeStore,
    hir: &HirProgram,
    base: &str,
    args: &[TypeId],
    ordinal: usize,
    taken: &mut HashSet<String>,
) -> String {
    let shown: Option<Vec<String>> = args
        .iter()
        .map(|&arg| store.display_bounded(arg, &hir.names, NAME_LIMIT))
        .collect();
    let inner = shown
        .map(|shown| shown.join(", "))
        .filter(|inner| inner.chars().count() <= NAME_LIMIT);
    if let Some(inner) = inner {
        let name = format!("{base}@[{inner}]");
        // 異なる型が同じ表示になる (row の末尾の表示など) ときは、後の instance を順番の名前にする
        if taken.insert(name.clone()) {
            return name;
        }
    }
    let name = format!("{base}@{ordinal}");
    taken.insert(name.clone());
    name
}

/// 生成する関数の、鍵を付ける前の名前 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「名前」)。
/// 手で書いた instance のメソッドと同じ形で、導出した instance は型を定義したモジュールで、タプルと `Unit` は
/// Prelude で修飾する。補助の `tag$` は `con$` と同じ修飾の規則に従う。
fn generated_base(hir: &HirProgram, generated: &Generated) -> String {
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
    let class = hir.names.class(class);
    match generated.node {
        InstanceNode::Declared(instance) => {
            let ty = hir[instance].head;
            let name = format!("{class} {}.{method}", hir.names.ty(ty));
            core_name(hir, ty.module, &name)
        }
        InstanceNode::Tuple(count) => {
            let ty = format!("({})", ",".repeat(count.saturating_sub(1)));
            core_name(hir, hir.prelude, &format!("{class} {ty}.{method}"))
        }
    }
}

/// HIR の関数の instance を HIR の関数の順に並べ、同じ関数の instance は見つけた順に並べる。生成する関数は、その後に
/// 見つけた順に並べる。`TypeId` の値の順には並べない。後の段階が `TypeId` の値に意味を持たせないためである。
fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) -> Instances {
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
    let mut ordinals: HashMap<FunctionId, usize> = HashMap::new();
    let mut generated_ordinals: HashMap<(InstanceNode, GeneratedMethod), usize> = HashMap::new();
    let mut taken = HashSet::new();
    let mut found: Vec<Option<Found>> = found.into_iter().map(Some).collect();
    let list = sorted
        .iter()
        .map(
            |&old| match found[old].take().expect("each instance is taken once") {
                Found::Function(found) => {
                    let base = core_name(hir, found.function.module, &hir[found.function].name);
                    let ordinal = ordinals.entry(found.function).or_default();
                    *ordinal += 1;
                    let name = if type_vars(hir, found.function).is_empty() {
                        base
                    } else {
                        instance_name(&store, hir, &base, &found.args, *ordinal, &mut taken)
                    };
                    Instance {
                        name,
                        kind: InstanceKind::Function(FunctionInstance {
                            function: found.function,
                            args: found.args,
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
                    fields,
                } => {
                    let base = generated_base(hir, &generated);
                    let ordinal = generated_ordinals
                        .entry((generated.node, generated.method))
                        .or_default();
                    *ordinal += 1;
                    let name = if generated.args.is_empty() {
                        base
                    } else {
                        instance_name(&store, hir, &base, &generated.args, *ordinal, &mut taken)
                    };
                    Instance {
                        name,
                        kind: InstanceKind::Generated(GeneratedInstance {
                            generated,
                            calls: calls
                                .iter()
                                .map(|target| target.renumber(&renumbered))
                                .collect(),
                            fields,
                        }),
                    }
                }
            },
        )
        .collect();
    Instances {
        list,
        entry: InstanceId(renumbered[entry]),
        store,
    }
}

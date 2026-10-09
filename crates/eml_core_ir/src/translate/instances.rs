//! 単相化の instance の表 (docs/spec/core-ir.md の「変換の規則」)。本体を変換する前に、入口から届く (関数, 型引数)
//! の組をすべて集め、番号の順を決める。変換はこの表を読むだけで、変換の途中で instance を足さない。

use std::collections::{HashMap, HashSet, VecDeque};

use eml_extern::Extern;
use eml_hir::{
    ExprId, FunctionId, FunctionKind, MethodId, MethodImpl, Program as HirProgram, ValueItem,
};
use eml_types::{
    BodyTypes, Instantiation, Resolution, Substitution, TypeId, TypeStore, TypedProgram,
};
use la_arena::ArenaMap;

use super::program::core_name;

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
    pub(super) function: FunctionId,
    /// シグネチャの型変数の順の型引数。代入と一様な位置を反映した鍵である。
    #[expect(
        dead_code,
        reason = "名前は `Found` の型引数から作るので、今は読む所がない。S5 が証拠を決めるときに読む"
    )]
    pub(super) args: Vec<TypeId>,
    pub(super) name: String,
    /// 宣言の型に instance の代入をかけた型。引数と `ret` の Repr を決める。
    pub(super) signature: TypeId,
    /// 本体の型に代入をかけた表。`None` なら型検査の表をそのまま使う。
    pub(super) types: Option<BodyTypes>,
    /// 本体の中の、定義された関数とメソッドへの参照の行き先。extern の関数への参照は入らない。
    pub(super) targets: ArenaMap<ExprId, Target>,
}

pub(super) struct Instances {
    pub(super) list: Vec<Instance>,
    pub(super) entry: InstanceId,
    /// 型検査の型の表に、代入の結果を足した表。
    pub(super) store: TypeStore,
}

/// 見つけた順の instance。番号の順は、すべて見つけてから決める。
struct Found {
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

/// 入口から届く instance を集める途中の状態。
struct Collector<'a> {
    hir: &'a HirProgram,
    typed: &'a TypedProgram,
    store: TypeStore,
    found: Vec<Found>,
    keys: HashMap<(FunctionId, Vec<TypeId>), usize>,
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
            found.push(Found {
                function,
                args,
                signature: typed.decls[&ValueItem::Function(function)].ty,
                types: None,
                targets: ArenaMap::default(),
            });
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

    /// メソッドの参照の行き先 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「メソッドへの参照の解決」)。
    /// `args` は代入をかけた後の型引数で、先頭がクラスの型変数の型である。型検査が制約をすべて具体的な型で解いて
    /// いるので、ここで `Given` と `Missing` に出会うことはない。
    fn method_target(&mut self, method: MethodId, args: &[TypeId]) -> FoundTarget {
        let hir = self.hir;
        let class = hir[method].class;
        match eml_types::resolve(hir, &self.store, class, args[0]) {
            Resolution::Instance {
                instance,
                args: head,
            } => match hir[instance].method(method) {
                Some(MethodImpl::Function(function)) => {
                    let key: Vec<TypeId> = head.iter().chain(&args[1..]).copied().collect();
                    FoundTarget::Function(self.add_uniform(function, &key))
                }
                Some(MethodImpl::Extern(row)) => FoundTarget::Extern(row),
                None => {
                    let default = hir[method]
                        .default
                        .expect("HIR reports a missing method without a default (E1036)");
                    FoundTarget::Function(self.add_uniform(default, args))
                }
            },
            Resolution::Tuple(_) => {
                unreachable!("translate does not resolve tuple instances until S5 Task 9")
            }
            other @ (Resolution::Given | Resolution::Missing) => unreachable!(
                "the type checker resolves every method reference at a concrete type, found {other:?}"
            ),
        }
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
        queue: VecDeque::new(),
    };
    // 入口は呼び出し側が選ぶので、型変数を持つ関数なら、どの型でも動く一様な instance にする
    let entry_args = vec![collector.store.flexible(); type_vars(hir, entry).len()];
    let entry_index = collector.add(entry, entry_args);
    // 先に見つけた instance から順に本体をたどる。番号の順と名前が変換の順によらないようにするため
    while let Some(index) = collector.queue.pop_front() {
        let function = collector.found[index].function;
        let body = typed
            .bodies
            .get(function)
            .expect("every reached body is type-checked");
        let names = type_vars(hir, function);
        debug_assert_eq!(
            names.len(),
            collector.found[index].args.len(),
            "an instance key must supply one type argument per type variable of the signature"
        );
        let mut subst = Substitution::new(
            names
                .iter()
                .cloned()
                .zip(collector.found[index].args.iter().copied()),
        );
        let decl = typed.decls[&ValueItem::Function(function)].ty;
        collector.found[index].signature = collector.store.substitute(decl, &mut subst);
        let mut targets = ArenaMap::default();
        let mut instantiations = ArenaMap::default();
        for (expr, instantiation) in body.instantiations.iter() {
            // 節の型変数 (`OpVar`) は、型変数を持たない関数でも `Flexible` にそろえる
            let args: Vec<TypeId> = instantiation
                .args
                .iter()
                .map(|&arg| collector.store.substitute(arg, &mut subst))
                .collect();
            match instantiation.decl {
                ValueItem::Function(callee) if hir[callee].kind == FunctionKind::Defined => {
                    let target = collector.add_uniform(callee, &args);
                    targets.insert(expr, FoundTarget::Function(target));
                }
                ValueItem::Method(method) => {
                    targets.insert(expr, collector.method_target(method, &args));
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
        collector.found[index].targets = targets;
        // 型変数を持たない関数の本体の型に現れる型変数は、節の `OpVar` だけである。`OpVar` の Repr は `tobj` なので、
        // 表は写さずに型検査の表を使う
        if !names.is_empty() {
            let store = &mut collector.store;
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
                    .all(|&ty| !collector.store.contains_type_vars(ty)),
                "an instance body keeps no type variable"
            );
            collector.found[index].types = Some(types);
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

/// HIR の関数の順に並べ、同じ関数の instance は見つけた順に並べる。`TypeId` の値の順には並べない。後の段階が
/// `TypeId` の値に意味を持たせないためである。
fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) -> Instances {
    let position: HashMap<FunctionId, usize> = hir
        .functions()
        .enumerate()
        .map(|(position, (id, _))| (id, position))
        .collect();
    let mut sorted: Vec<usize> = (0..found.len()).collect();
    sorted.sort_by_key(|&index| (position[&found[index].function], index));
    let mut renumbered = vec![0; found.len()];
    for (new, &old) in sorted.iter().enumerate() {
        renumbered[old] = new;
    }
    let mut ordinals: HashMap<FunctionId, usize> = HashMap::new();
    let mut taken = HashSet::new();
    let mut found: Vec<Option<Found>> = found.into_iter().map(Some).collect();
    let list = sorted
        .iter()
        .map(|&old| {
            let found = found[old].take().expect("each instance is taken once");
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
                function: found.function,
                args: found.args,
                signature: found.signature,
                types: found.types,
                targets: found
                    .targets
                    .iter()
                    .map(|(expr, &target)| {
                        let target = match target {
                            FoundTarget::Function(index) => {
                                Target::Function(InstanceId(renumbered[index]))
                            }
                            FoundTarget::Extern(row) => Target::Extern(row),
                        };
                        (expr, target)
                    })
                    .collect(),
            }
        })
        .collect();
    Instances {
        list,
        entry: InstanceId(renumbered[entry]),
        store,
    }
}

//! 単相化の instance の表 (docs/spec/core-ir.md の「変換の規則」)。本体を変換する前に、入口から届く (関数, 型引数)
//! の組をすべて集め、番号の順を決める。変換はこの表を読むだけで、変換の途中で instance を足さない。

use std::collections::{HashMap, HashSet, VecDeque};

use eml_hir::{ExprId, FunctionId, FunctionKind, Program as HirProgram, ValueItem};
use eml_types::{
    BodyTypes, Instantiation, Substitution, TypeId, TypeKind, TypeStore, TypedProgram,
};
use la_arena::ArenaMap;

use super::program::core_name;

/// `Instances::list` の位置。変換はこの順に関数の番号を予約する。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct InstanceId(pub(super) usize);

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
    /// 本体の中の、定義された関数への参照の行き先。extern の関数への参照は入らない。
    pub(super) targets: ArenaMap<ExprId, InstanceId>,
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
    targets: ArenaMap<ExprId, usize>,
}

pub(super) fn collect(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId) -> Instances {
    let uniform = uniform_positions(hir, typed);
    let mut store = typed.types.clone();
    let mut found: Vec<Found> = Vec::new();
    let mut keys: HashMap<(FunctionId, Vec<TypeId>), usize> = HashMap::new();
    let mut queue = VecDeque::new();
    // `signature` は宣言の型のまま入れ、本体をたどるときに代入をかけた型に置き換える
    let mut add = |function: FunctionId,
                   args: Vec<TypeId>,
                   found: &mut Vec<Found>,
                   queue: &mut VecDeque<usize>| {
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
    };
    // 入口は呼び出し側が選ぶので、型変数を持つ関数なら、どの型でも動く一様な instance にする
    let entry_args = vec![store.flexible(); type_vars(hir, entry).len()];
    let entry_index = add(entry, entry_args, &mut found, &mut queue);
    // 先に見つけた instance から順に本体をたどる。番号の順と名前が変換の順によらないようにするため
    while let Some(index) = queue.pop_front() {
        let function = found[index].function;
        let body = typed
            .bodies
            .get(function)
            .expect("every reached body is type-checked");
        let names = type_vars(hir, function);
        debug_assert_eq!(
            names.len(),
            found[index].args.len(),
            "an instance key must supply one type argument per type variable of the signature"
        );
        let mut subst =
            Substitution::new(names.iter().cloned().zip(found[index].args.iter().copied()));
        let decl = typed.decls[&ValueItem::Function(function)].ty;
        found[index].signature = store.substitute(decl, &mut subst);
        let mut targets = ArenaMap::default();
        let mut instantiations = ArenaMap::default();
        for (expr, instantiation) in body.instantiations.iter() {
            // 節の型変数 (`OpVar`) は、型変数を持たない関数でも `Flexible` にそろえる
            let args: Vec<TypeId> = instantiation
                .args
                .iter()
                .map(|&arg| store.substitute(arg, &mut subst))
                .collect();
            if let ValueItem::Function(callee) = instantiation.decl
                && hir[callee].kind == FunctionKind::Defined
            {
                let key: Vec<TypeId> = args
                    .iter()
                    .enumerate()
                    .map(|(position, &arg)| {
                        if uniform.contains(&(callee, position)) {
                            store.flexible()
                        } else {
                            arg
                        }
                    })
                    .collect();
                let target = add(callee, key, &mut found, &mut queue);
                targets.insert(expr, target);
            }
            instantiations.insert(
                expr,
                Instantiation {
                    decl: instantiation.decl,
                    args,
                },
            );
        }
        found[index].targets = targets;
        // 型変数を持たない関数の本体の型に現れる型変数は、節の `OpVar` だけである。`OpVar` の Repr は `tobj` なので、
        // 表は写さずに型検査の表を使う
        if !names.is_empty() {
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
                    .all(|&ty| !store.contains_type_vars(ty)),
                "an instance body keeps no type variable"
            );
            found[index].types = Some(types);
        }
    }
    order(hir, found, entry_index, store)
}

/// 多相再帰で大きくなる型変数の位置 (docs/spec/core-ir.md の「変換の規則」)。節点は (関数, 型変数の番号) で、関数 f
/// の本体の参照 `g @[T0, …]` の Tj に f の型変数 i が現れるとき (f, i) から (g, j) へ辺を引く。Tj が i そのもの
/// でなければ大きくなる辺である。大きくなる辺の両端を含む強連結成分の節点を、一様な位置とする。
fn uniform_positions(hir: &HirProgram, typed: &TypedProgram) -> HashSet<(FunctionId, usize)> {
    let mut nodes: Vec<(FunctionId, usize)> = Vec::new();
    let mut node_of: HashMap<(FunctionId, usize), usize> = HashMap::new();
    for (id, function) in hir.functions() {
        if function.kind != FunctionKind::Defined {
            continue;
        }
        for position in 0..type_vars(hir, id).len() {
            node_of.insert((id, position), nodes.len());
            nodes.push((id, position));
        }
    }
    let mut edges: Vec<Vec<(usize, bool)>> = vec![Vec::new(); nodes.len()];
    for (f, function) in hir.functions() {
        if function.kind != FunctionKind::Defined {
            continue;
        }
        let names = type_vars(hir, f);
        if names.is_empty() {
            continue;
        }
        let Some(body) = typed.bodies.get(f) else {
            continue;
        };
        // 本体の中で共有された型を、各節点1回だけたどる
        let mut occurrences = HashMap::new();
        for (_, instantiation) in body.instantiations.iter() {
            let ValueItem::Function(g) = instantiation.decl else {
                continue;
            };
            if hir[g].kind != FunctionKind::Defined {
                continue;
            }
            for (j, &arg) in instantiation.args.iter().enumerate() {
                let occurs = vars_in(&typed.types, arg, &names, &mut occurrences);
                for (i, name) in names.iter().enumerate() {
                    if occurs[i] {
                        let grow =
                            !matches!(typed.types.kind(arg), TypeKind::Rigid(n) if n == name);
                        edges[node_of[&(f, i)]].push((node_of[&(g, j)], grow));
                    }
                }
            }
        }
    }
    let component = strongly_connected(&edges);
    let mut growing = HashSet::new();
    for (from, out) in edges.iter().enumerate() {
        for &(to, grow) in out {
            if grow && component[from] == component[to] {
                growing.insert(component[from]);
            }
        }
    }
    nodes
        .into_iter()
        .enumerate()
        .filter(|(node, _)| growing.contains(&component[*node]))
        .map(|(_, position)| position)
        .collect()
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

/// `ty` に現れる `names` の型変数 (番号ごとの真偽)。`substitute` がたどる位置 (型構成子の引数、関数型の引数と結果と
/// エフェクトの型引数、レコードとタプルの要素) を見る。結果を `memo` に覚え、共有された型は1回だけたどる。
fn vars_in(
    types: &TypeStore,
    ty: TypeId,
    names: &[String],
    memo: &mut HashMap<TypeId, Vec<bool>>,
) -> Vec<bool> {
    if !types.contains_type_vars(ty) {
        return vec![false; names.len()];
    }
    if let Some(found) = memo.get(&ty) {
        return found.clone();
    }
    let mut found = vec![false; names.len()];
    let mut merge = |child: Vec<bool>| {
        for (slot, child) in found.iter_mut().zip(child) {
            *slot |= child;
        }
    };
    match types.kind(ty) {
        TypeKind::Rigid(name) => {
            if let Some(position) = names.iter().position(|n| n == name) {
                merge({
                    let mut one = vec![false; names.len()];
                    one[position] = true;
                    one
                });
            }
        }
        TypeKind::Con { args, .. } => {
            for &arg in args {
                merge(vars_in(types, arg, names, memo));
            }
        }
        TypeKind::Record(fields) => {
            for &(_, field) in fields {
                merge(vars_in(types, field, names, memo));
            }
        }
        TypeKind::Fn {
            param,
            effects,
            ret,
            ..
        } => {
            merge(vars_in(types, *param, names, memo));
            for label in effects {
                for &arg in &label.args {
                    merge(vars_in(types, arg, names, memo));
                }
            }
            merge(vars_in(types, *ret, names, memo));
        }
        TypeKind::OpVar(_) | TypeKind::Flexible | TypeKind::Error => {}
    }
    memo.insert(ty, found.clone());
    found
}

/// 強連結成分の番号。Tarjan の方法を作業の列で行い、グラフの深さに比例して Rust のスタックを使わない。
fn strongly_connected(edges: &[Vec<(usize, bool)>]) -> Vec<usize> {
    const UNSEEN: usize = usize::MAX;
    let mut index = vec![UNSEEN; edges.len()];
    let mut low = vec![0; edges.len()];
    let mut on_stack = vec![false; edges.len()];
    let mut stack = Vec::new();
    let mut component = vec![UNSEEN; edges.len()];
    let (mut next_index, mut next_component) = (0, 0);
    for root in 0..edges.len() {
        if index[root] != UNSEEN {
            continue;
        }
        // (節点, 次に見る辺の位置)
        let mut work = vec![(root, 0)];
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(node, edge)) = work.last() {
            if let Some(&(to, _)) = edges[node].get(edge) {
                work.last_mut().expect("read above").1 += 1;
                if index[to] == UNSEEN {
                    index[to] = next_index;
                    low[to] = next_index;
                    next_index += 1;
                    stack.push(to);
                    on_stack[to] = true;
                    work.push((to, 0));
                } else if on_stack[to] {
                    low[node] = low[node].min(index[to]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == index[node] {
                loop {
                    let member = stack
                        .pop()
                        .expect("the root of a component is on the stack");
                    on_stack[member] = false;
                    component[member] = next_component;
                    if member == node {
                        break;
                    }
                }
                next_component += 1;
            }
        }
    }
    component
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
                    .map(|(expr, &target)| (expr, InstanceId(renumbered[target])))
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

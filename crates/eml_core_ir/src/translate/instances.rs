//! 単相化の instance の表 (docs/spec/core-ir.md の「変換の規則」)。本体を変換する前に、入口から届く (関数, 型引数)
//! の組をすべて集め、番号の順を決める。変換はこの表を読むだけで、変換の途中で instance を足さない。

use std::collections::{HashMap, VecDeque};

use eml_hir::{ExprId, FunctionId, FunctionKind, Program as HirProgram, ValueItem};
use eml_types::{BodyTypes, TypeId, TypeStore, TypedProgram};
use la_arena::ArenaMap;

use super::program::core_name;

/// `Instances::list` の位置。変換はこの順に関数の番号を予約する。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct InstanceId(pub(super) usize);

pub(super) struct Instance {
    pub(super) function: FunctionId,
    /// シグネチャの型変数の順の型引数。
    #[expect(dead_code, reason = "型引数を持つ鍵は次の変更で使う")]
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
    let store = typed.types.clone();
    let mut found: Vec<Found> = Vec::new();
    let mut keys: HashMap<(FunctionId, Vec<TypeId>), usize> = HashMap::new();
    let mut queue = VecDeque::new();
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
    let entry_index = add(entry, Vec::new(), &mut found, &mut queue);
    // 先に見つけた instance から順に本体をたどる。番号の順と名前が変換の順によらないようにするため
    while let Some(index) = queue.pop_front() {
        let function = found[index].function;
        let body = typed
            .bodies
            .get(function)
            .expect("every reached body is type-checked");
        let mut targets = ArenaMap::default();
        for (expr, instantiation) in body.instantiations.iter() {
            let ValueItem::Function(callee) = instantiation.decl else {
                continue;
            };
            if hir[callee].kind != FunctionKind::Defined {
                continue;
            }
            let target = add(callee, Vec::new(), &mut found, &mut queue);
            targets.insert(expr, target);
        }
        found[index].targets = targets;
    }
    order(hir, found, entry_index, store)
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
    let mut found: Vec<Option<Found>> = found.into_iter().map(Some).collect();
    let list = sorted
        .iter()
        .map(|&old| {
            let found = found[old].take().expect("each instance is taken once");
            Instance {
                name: core_name(hir, found.function.module, &hir[found.function].name),
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

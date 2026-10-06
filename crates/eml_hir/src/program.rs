//! プログラム全体の HIR と、プログラム全体で一意な item の ID (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の
//! 1.2 と 1.3)。

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Index, IndexMut};

use eml_diagnostics::FileId;
use la_arena::{Arena, ArenaMap, Idx, RawIdx};

use crate::hir::{
    Body, Constructor, EffectDef, Function, LangItems, Operation, Signature, TypeDef,
};

pub type ModuleId = Idx<Module>;

/// item の ID。モジュールと、モジュールの中の番号の組で、プログラム全体で一意である。
pub struct ItemId<T> {
    pub module: ModuleId,
    pub local: Idx<T>,
}

impl<T> ItemId<T> {
    pub fn new(module: ModuleId, local: Idx<T>) -> ItemId<T> {
        ItemId { module, local }
    }
}

// `derive` は `T` にも同じ trait を求めるので、手で書く
impl<T> Clone for ItemId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ItemId<T> {}

impl<T> PartialEq for ItemId<T> {
    fn eq(&self, other: &Self) -> bool {
        (self.module, self.local) == (other.module, other.local)
    }
}

impl<T> Eq for ItemId<T> {}

impl<T> Hash for ItemId<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.module, self.local).hash(state);
    }
}

impl<T> PartialOrd for ItemId<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for ItemId<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.module.into_raw(), self.local.into_raw())
            .cmp(&(other.module.into_raw(), other.local.into_raw()))
    }
}

impl<T> fmt::Debug for ItemId<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}",
            u32::from(self.module.into_raw()),
            u32::from(self.local.into_raw())
        )
    }
}

pub type FunctionId = ItemId<Function>;
pub type TypeDefId = ItemId<TypeDef>;
pub type ConstructorId = ItemId<Constructor>;
pub type EffectId = ItemId<EffectDef>;
pub type OperationId = ItemId<Operation>;

/// item の ID から値を引く表。モジュールごとに `ArenaMap` を持つ。`ArenaMap` と同じ使い方にして、下流の表の
/// 置き換えを型の名前だけで済ませるため。
pub struct ItemMap<T, V> {
    modules: Vec<ArenaMap<Idx<T>, V>>,
}

impl<T, V> Default for ItemMap<T, V> {
    fn default() -> Self {
        ItemMap {
            modules: Vec::new(),
        }
    }
}

impl<T, V> ItemMap<T, V> {
    pub fn insert(&mut self, id: ItemId<T>, value: V) {
        let module = module_index(id.module);
        if self.modules.len() <= module {
            self.modules.resize_with(module + 1, ArenaMap::default);
        }
        self.modules[module].insert(id.local, value);
    }

    pub fn get(&self, id: ItemId<T>) -> Option<&V> {
        self.modules.get(module_index(id.module))?.get(id.local)
    }

    pub fn get_mut(&mut self, id: ItemId<T>) -> Option<&mut V> {
        self.modules
            .get_mut(module_index(id.module))?
            .get_mut(id.local)
    }

    /// モジュールの順、モジュールの中の番号の順にたどる。
    pub fn iter(&self) -> impl Iterator<Item = (ItemId<T>, &V)> {
        self.modules.iter().enumerate().flat_map(|(module, map)| {
            let module = ModuleId::from_raw(RawIdx::from(module as u32));
            map.iter()
                .map(move |(local, value)| (ItemId::new(module, local), value))
        })
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }
}

fn module_index(module: ModuleId) -> usize {
    u32::from(module.into_raw()) as usize
}

impl<T, V> Index<ItemId<T>> for ItemMap<T, V> {
    type Output = V;

    fn index(&self, id: ItemId<T>) -> &V {
        self.get(id).expect("the id has a value in the map")
    }
}

impl<T, V> IndexMut<ItemId<T>> for ItemMap<T, V> {
    fn index_mut(&mut self, id: ItemId<T>) -> &mut V {
        self.get_mut(id).expect("the id has a value in the map")
    }
}

impl<T, V> FromIterator<(ItemId<T>, V)> for ItemMap<T, V> {
    fn from_iter<I: IntoIterator<Item = (ItemId<T>, V)>>(iter: I) -> Self {
        let mut map = ItemMap::default();
        for (id, value) in iter {
            map.insert(id, value);
        }
        map
    }
}

impl<T, V: fmt::Debug> fmt::Debug for ItemMap<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// プログラム全体の HIR。Prelude と入口のモジュールからなる。
#[derive(Debug)]
pub struct Program {
    pub modules: Arena<Module>,
    pub prelude: ModuleId,
    /// `eml check` と `eml run` に渡したファイルのモジュール。`main` はここから探す。
    pub entry: ModuleId,
    pub lang: LangItems,
}

/// HIR のノードは `SyntaxNodePtr` ではなく範囲を持つ。演算子の列を組み直した部分式のように、対応する構文ノードの
/// ない式があるため。
#[derive(Debug)]
pub struct Module {
    pub file: FileId,
    pub name: String,
    /// ほかのモジュールと型検査の段0が見るもの。
    pub items: Items,
    /// 関数の本体。等式のある関数だけを含む。本体を item から分けるのは、本体を書き換えても item が変わらない
    /// ようにするため。
    pub bodies: ArenaMap<Idx<Function>, Body>,
}

impl Module {
    pub fn new(file: FileId, name: &str) -> Module {
        Module {
            file,
            name: name.to_string(),
            items: Items::default(),
            bodies: ArenaMap::default(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Items {
    pub functions: Arena<Function>,
    /// 型の item。Prelude では組み込みの `Int`、`String`、`Unit`、`File` と `Bool`、ユーザーのモジュールでは `data` の
    /// 宣言。
    pub types: Arena<TypeDef>,
    /// `data` の宣言のコンストラクタ。値の名前空間に置くトップレベルの値である (docs/spec/modules.md の「名前空間」)。
    pub constructors: Arena<Constructor>,
    /// エフェクトの item。Prelude では組み込みの `IO`、ユーザーのモジュールでは `effect` の宣言。
    pub effects: Arena<EffectDef>,
    /// エフェクトの操作。値の名前空間に置くトップレベルの値である (docs/spec/modules.md の「名前空間」)。
    pub operations: Arena<Operation>,
}

impl Program {
    fn items<'a, T: 'a>(
        &'a self,
        arena: impl Fn(&'a Items) -> &'a Arena<T> + 'a,
    ) -> impl Iterator<Item = (ItemId<T>, &'a T)> + 'a {
        self.modules.iter().flat_map(move |(module, data)| {
            arena(&data.items)
                .iter()
                .map(move |(local, item)| (ItemId::new(module, local), item))
        })
    }

    /// Prelude、入口のモジュールの順に、モジュールの中の宣言の順でたどる。
    pub fn functions(&self) -> impl Iterator<Item = (FunctionId, &Function)> {
        self.items(|items| &items.functions)
    }

    pub fn types(&self) -> impl Iterator<Item = (TypeDefId, &TypeDef)> {
        self.items(|items| &items.types)
    }

    pub fn constructors(&self) -> impl Iterator<Item = (ConstructorId, &Constructor)> {
        self.items(|items| &items.constructors)
    }

    pub fn effects(&self) -> impl Iterator<Item = (EffectId, &EffectDef)> {
        self.items(|items| &items.effects)
    }

    pub fn operations(&self) -> impl Iterator<Item = (OperationId, &Operation)> {
        self.items(|items| &items.operations)
    }

    pub fn body(&self, id: FunctionId) -> Option<&Body> {
        self.modules[id.module].bodies.get(id.local)
    }

    /// 引数がそろうまで本体が動かない引数の数。intrinsic はシグネチャの一番外側の `->` の数、ほかは等式の引数の数
    /// である。本体のない関数 (E1005) は `None`。
    pub fn arity(&self, id: FunctionId) -> Option<usize> {
        let function = &self[id];
        if function.intrinsic {
            return function.signature.as_ref().map(Signature::arity);
        }
        self.body(id).map(|body| body.params.len())
    }

    pub fn file(&self, module: ModuleId) -> FileId {
        self.modules[module].file
    }

    /// `eml run` が実行を始める関数。入口のモジュールだけから探し、Prelude には置かない
    /// (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 1.4)。同じ名前の関数が重複したときは、最初の定義である。
    pub fn main(&self) -> Option<FunctionId> {
        self.modules[self.entry]
            .items
            .functions
            .iter()
            .find(|(_, function)| function.name == "main")
            .map(|(local, _)| ItemId::new(self.entry, local))
    }
}

macro_rules! program_index {
    ($($item:ident => $arena:ident,)*) => {$(
        impl Index<ItemId<$item>> for Program {
            type Output = $item;

            fn index(&self, id: ItemId<$item>) -> &$item {
                &self.modules[id.module].items.$arena[id.local]
            }
        }
    )*};
}

program_index! {
    Function => functions,
    TypeDef => types,
    Constructor => constructors,
    EffectDef => effects,
    Operation => operations,
}

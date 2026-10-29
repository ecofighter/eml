//! instance の名前と生成する関数の名前に使う、item の名前の表と型のハッシュ (docs/spec/core-ir.md の「変換の規則」)。
//! 名前は、同じ名前の関数がいつも同じコードになるように、型引数と item だけで決める。診断の表示名 (`DisplayNames`) は
//! 名前が重なるときだけ修飾するので、後の入力が同じ名前を定義すると表示が変わり、ここには使えない。

use eml_hir::{ClassId, EffectId, ModuleId, ModuleOrigin, Program as HirProgram, TypeDefId};
use rustc_hash::{FxHashMap, FxHashSet};

use super::program::versioned;
use crate::fnv::Fnv;
use eml_types::{EffectLabel, Lin, RowTail, TypeId, TypeKind, TypeNames, TypeStore};

/// 型、エフェクト、クラスの名前の表。Prelude の item は修飾しない。エントリのモジュールの item は修飾しないが、Prelude の
/// 型、エフェクト、クラスと同じ名前のものはモジュール名で修飾する。ほかのモジュールの item は、いつもモジュール名で
/// 修飾する。Prelude は固定なので、どの名前が重なるかは入力で変わらない。
pub(super) struct NameTable {
    types: FxHashMap<TypeDefId, String>,
    effects: FxHashMap<EffectId, String>,
    classes: FxHashMap<ClassId, String>,
    unit: String,
}

impl NameTable {
    pub(super) fn new(hir: &HirProgram) -> NameTable {
        // 型、エフェクト、クラスは同じ名前空間にあるので合わせて数える (docs/spec/modules.md の「名前空間」)
        let prelude: FxHashSet<&str> = hir
            .types()
            .map(|(id, def)| (id.module, def.name.as_str()))
            .chain(
                hir.effects()
                    .map(|(id, def)| (id.module, def.name.as_str())),
            )
            .chain(
                hir.classes()
                    .map(|(id, def)| (id.module, def.name.as_str())),
            )
            .filter(|&(module, _)| module == hir.prelude)
            .map(|(_, name)| name)
            .collect();
        let name = |module: ModuleId, name: &str| qualified(hir, &prelude, module, name);
        let unit = hir
            .types()
            .find(|(id, def)| id.module == hir.prelude && def.name == "Unit")
            .map(|(_, def)| def.name.clone())
            .expect("the Prelude declares `Unit`");
        NameTable {
            types: hir
                .types()
                .map(|(id, def)| (id, name(id.module, &def.name)))
                .collect(),
            effects: hir
                .effects()
                .map(|(id, def)| (id, name(id.module, &def.name)))
                .collect(),
            classes: hir
                .classes()
                .map(|(id, def)| (id, name(id.module, &def.name)))
                .collect(),
            unit,
        }
    }

    pub(super) fn class(&self, id: ClassId) -> &str {
        &self.classes[&id]
    }
}

impl TypeNames for NameTable {
    fn ty(&self, id: TypeDefId) -> &str {
        &self.types[&id]
    }

    fn effect(&self, id: EffectId) -> &str {
        &self.effects[&id]
    }

    fn unit(&self) -> &str {
        &self.unit
    }
}

/// REPL のセッションのモジュールは、エントリと同じく修飾せず、バージョンを付けた中段の名前で書く。Prelude の名前と重なれば
/// `Main.` で修飾する。セッションのモジュールの名前 (`<input N>`) は入力の番号を含むので、修飾には使わない
/// (docs/spec/core-ir.md の「REPL の入力」)。
fn qualified(hir: &HirProgram, prelude: &FxHashSet<&str>, module: ModuleId, name: &str) -> String {
    if module == hir.prelude {
        return name.to_string();
    }
    let session = matches!(hir.modules[module].origin, ModuleOrigin::Session(_));
    if module != hir.entry && !session {
        return format!("{}.{name}", hir.modules[module].name);
    }
    let local = versioned(hir, module, name);
    if prelude.contains(name) {
        format!("Main.{local}")
    } else {
        local
    }
}

/// 型引数の並びのハッシュ。名前の表示 (`TypeStore::display_name_bounded`) が上限を超えた instance の名前に使う。表示と
/// 同じ情報 (item の名前、矢印の線形性、row、`Flexible` と `OpVar` の Kind) を混ぜ、`TypeId` の値は混ぜない。部分を
/// 共有する型でも費用が表の大きさに比例するように、型ごとに1回だけ計算する。
pub(super) fn args_hash(store: &TypeStore, names: &NameTable, args: &[TypeId]) -> u64 {
    let mut hasher = Hasher {
        store,
        names,
        memo: FxHashMap::default(),
    };
    let mut state = Fnv::new();
    state.number(args.len() as u64);
    for &arg in args {
        let hash = hasher.hash(arg);
        state.number(hash);
    }
    state.0
}

struct Hasher<'a> {
    store: &'a TypeStore,
    names: &'a NameTable,
    memo: FxHashMap<TypeId, u64>,
}

impl Hasher<'_> {
    fn hash(&mut self, id: TypeId) -> u64 {
        if let Some(&hash) = self.memo.get(&id) {
            return hash;
        }
        let mut state = Fnv::new();
        match self.store.kind(id) {
            TypeKind::Con {
                id: def,
                args,
                rows,
            } => {
                state.number(0);
                state.text(self.names.ty(*def));
                self.children(&mut state, args);
                // row の型引数の数は型の名前で決まるので混ぜない
                for row in rows {
                    self.row(&mut state, &row.effects, &row.tail);
                }
            }
            TypeKind::Tuple(elements) => {
                state.number(1);
                self.children(&mut state, elements);
            }
            TypeKind::Fn {
                param,
                lin,
                effects,
                tail,
                ret,
            } => {
                state.number(2);
                let param = self.hash(*param);
                state.number(param);
                state.lin(*lin);
                self.row(&mut state, effects, tail);
                let ret = self.hash(*ret);
                state.number(ret);
            }
            TypeKind::Rigid(name) => {
                state.number(3);
                state.text(name);
            }
            TypeKind::OpVar(name, kind) => {
                state.number(4);
                state.text(name);
                state.lin(*kind);
            }
            TypeKind::Flexible(kind) => {
                state.number(5);
                state.lin(*kind);
            }
            TypeKind::Error => state.number(6),
        }
        self.memo.insert(id, state.0);
        state.0
    }

    fn row(&mut self, state: &mut Fnv, effects: &[EffectLabel], tail: &Option<RowTail>) {
        state.number(effects.len() as u64);
        for label in effects {
            state.text(self.names.effect(label.id));
            self.children(state, &label.args);
        }
        match tail {
            None => state.number(0),
            Some(RowTail::Rigid(name)) => {
                state.number(1);
                state.text(name);
            }
            Some(RowTail::Flexible) => state.number(2),
            Some(RowTail::Error) => state.number(3),
        }
    }

    fn children(&mut self, state: &mut Fnv, children: &[TypeId]) {
        state.number(children.len() as u64);
        for &child in children {
            let hash = self.hash(child);
            state.number(hash);
        }
    }
}

impl Fnv {
    fn lin(&mut self, lin: Lin) {
        match lin {
            Lin::Unr => self.number(0),
            Lin::Lin => self.number(1),
            Lin::Poly => self.number(2),
            Lin::Var(index) => {
                self.number(3);
                self.number(u64::from(index));
            }
        }
    }
}

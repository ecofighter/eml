//! モジュールごとのスコープ表 (docs/implementation/architecture.md の「`eml_hir` の内部」)。全モジュールの
//! `ItemTree` から、item の ID、名前の表、定義に付く fixity、lang item を作る。名前を解決しなくても決まるものだけを
//! 読むので、item の変換より先に作れる。

use std::collections::{HashMap, HashSet};

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use la_arena::{Idx, RawIdx};

use crate::codes;
use crate::hir::LangItems;
use crate::item_tree::{Fixity, ItemTree};
use crate::program::{
    ConstructorId, EffectId, FunctionId, ItemId, ModuleId, OperationId, TypeDefId,
};

/// 値の名前空間の定義を引いた結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueItem {
    Function(FunctionId),
    Operation(OperationId),
    Constructor(ConstructorId),
    /// 重複した `data` のコンストラクタか、重複した `effect` の操作。使った位置は診断を出さずに `Missing` にする。
    Unusable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeItem {
    Type(TypeDefId),
    Effect(EffectId),
}

/// 種類を指定して名前を引いた結果 (規則3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup<T> {
    Found(T),
    /// 使えない定義だけが見つかった。診断を出さずに `Missing` にする。
    Unusable,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Value {
    Function(FunctionId),
    Operation(OperationId),
    Constructor(ConstructorId),
}

impl Value {
    fn item(self) -> ValueItem {
        match self {
            Value::Function(id) => ValueItem::Function(id),
            Value::Operation(id) => ValueItem::Operation(id),
            Value::Constructor(id) => ValueItem::Constructor(id),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Definition<T> {
    item: T,
    range: TextRange,
    public: bool,
    /// 重複した宣言の部品は使えない (規則2)。重複の判定に加えず、同じ名前の使える定義がないときだけ引かれる。
    usable: bool,
}

#[derive(Debug)]
struct ModuleScope {
    name: String,
    /// 名前ごとの定義。ソースの位置の順で、使える定義の最初が名前の定義である (規則1)。
    values: HashMap<String, Vec<Definition<Value>>>,
    types: HashMap<String, Vec<Definition<TypeItem>>>,
    /// 型引数の数 (E1015)。重複した型引数を除いた数である。
    type_params: HashMap<TypeDefId, usize>,
    effect_params: HashMap<EffectId, usize>,
    /// 定義に付いた fixity、宣言が `pub` か、宣言の演算子の位置。
    fixities: HashMap<Value, (Fixity, bool, TextRange)>,
    functions: Vec<FunctionId>,
    type_ids: Vec<TypeDefId>,
    constructors: Vec<Vec<ConstructorId>>,
    effects: Vec<EffectId>,
    operations: Vec<Vec<OperationId>>,
}

/// プログラム全体の名前の表。
#[derive(Debug)]
pub struct DefMap {
    modules: Vec<ModuleScope>,
    prelude: ModuleId,
    entry: ModuleId,
    lang: LangItems,
}

/// `trees[0]` を Prelude、`trees[1]` を入口のモジュールとして読む。値と型の名前空間の重複 (E1003)、fixity の重複
/// (E1021)、このモジュールにない演算子の fixity (E1022) を出す。
pub fn def_map(trees: &[ItemTree]) -> (DefMap, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let prelude = module_id(0);
    let modules: Vec<ModuleScope> = trees
        .iter()
        .enumerate()
        .map(|(index, tree)| {
            let module = module_id(index);
            let mut scope = ModuleScope::new(module, module == prelude, tree);
            scope.check_duplicates(tree.file, &mut diagnostics);
            scope.attach_fixities(tree, &mut diagnostics);
            scope
        })
        .collect();
    let lang = lang_items(&modules[0]);
    (
        DefMap {
            modules,
            prelude,
            entry: module_id(1),
            lang,
        },
        diagnostics,
    )
}

/// `trees` の番号のモジュールの ID。`lower` もモジュールを同じ順に置く。
pub(crate) fn module_id(index: usize) -> ModuleId {
    ModuleId::from_raw(RawIdx::from(index as u32))
}

fn item_id<T>(module: ModuleId, local: usize) -> ItemId<T> {
    ItemId::new(module, Idx::from_raw(RawIdx::from(local as u32)))
}

impl ModuleScope {
    /// 局所の番号は `ItemTree` の順に振る。コンストラクタと操作は、宣言の順に通し番号に
    /// する。`lower` も同じ順にアリーナへ置く。
    fn new(module: ModuleId, is_prelude: bool, tree: &ItemTree) -> ModuleScope {
        let name = if is_prelude { "Prelude" } else { "Main" };
        let functions = (0..tree.functions.len())
            .map(|k| item_id(module, k))
            .collect();
        let type_ids: Vec<TypeDefId> = (0..tree.data.len()).map(|k| item_id(module, k)).collect();
        let mut next = 0;
        let constructors = tree
            .data
            .iter()
            .map(|data| {
                data.constructors
                    .iter()
                    .map(|_| {
                        next += 1;
                        item_id(module, next - 1)
                    })
                    .collect()
            })
            .collect();
        let effects: Vec<EffectId> = (0..tree.effects.len())
            .map(|k| item_id(module, k))
            .collect();
        let mut next = 0;
        let operations = tree
            .effects
            .iter()
            .map(|effect| {
                effect
                    .operations
                    .iter()
                    .map(|_| {
                        next += 1;
                        item_id(module, next - 1)
                    })
                    .collect()
            })
            .collect();
        let mut scope = ModuleScope {
            name: name.to_string(),
            values: HashMap::new(),
            types: HashMap::new(),
            type_params: HashMap::new(),
            effect_params: HashMap::new(),
            fixities: HashMap::new(),
            functions,
            type_ids,
            constructors,
            effects,
            operations,
        };
        scope.declare(tree);
        scope
    }

    fn declare(&mut self, tree: &ItemTree) {
        for (k, data) in tree.data.iter().enumerate() {
            let id = self.type_ids[k];
            push(
                &mut self.types,
                &data.name,
                TypeItem::Type(id),
                data.name_range,
                data.public,
                true,
            );
            self.type_params
                .insert(id, unique_params(data.syntax.params().map(|p| p.text())));
        }
        for (k, effect) in tree.effects.iter().enumerate() {
            let id = self.effects[k];
            push(
                &mut self.types,
                &effect.name,
                TypeItem::Effect(id),
                effect.name_range,
                effect.public,
                true,
            );
            self.effect_params
                .insert(id, unique_params(effect.syntax.params().map(|p| p.text())));
        }
        for names in self.types.values_mut() {
            names.sort_by_key(|definition| definition.range.start());
        }
        // 重複した宣言 (名前の2つ目以降の定義) の部品は使えない (規則2)
        let duplicates: HashSet<TypeItem> = self
            .types
            .values()
            .flat_map(|names| names.iter().skip(1).map(|definition| definition.item))
            .collect();
        for (k, function) in tree.functions.iter().enumerate() {
            let id = self.functions[k];
            push(
                &mut self.values,
                &function.name,
                Value::Function(id),
                function.first_range,
                function.public,
                true,
            );
        }
        for (k, data) in tree.data.iter().enumerate() {
            let usable = !duplicates.contains(&TypeItem::Type(self.type_ids[k]));
            for (j, constructor) in data.constructors.iter().enumerate() {
                let id = self.constructors[k][j];
                push(
                    &mut self.values,
                    &constructor.name,
                    Value::Constructor(id),
                    constructor.name_range,
                    data.public,
                    usable,
                );
            }
        }
        for (k, effect) in tree.effects.iter().enumerate() {
            let usable = !duplicates.contains(&TypeItem::Effect(self.effects[k]));
            for (j, operation) in effect.operations.iter().enumerate() {
                let id = self.operations[k][j];
                push(
                    &mut self.values,
                    &operation.name,
                    Value::Operation(id),
                    operation.name_range,
                    effect.public,
                    usable,
                );
            }
        }
        for names in self.values.values_mut() {
            names.sort_by_key(|definition| definition.range.start());
        }
    }

    /// 名前空間ごとに、使える定義の2つ目以降を E1003 にする (規則1)。
    fn check_duplicates(&self, file: FileId, diagnostics: &mut Vec<Diagnostic>) {
        let mut report = |name: &str, ranges: Vec<TextRange>| {
            if let Some((first, rest)) = ranges.split_first() {
                for again in rest {
                    diagnostics.push(duplicate(file, name, *first, *again));
                }
            }
        };
        for (name, definitions) in &self.types {
            report(
                name,
                definitions
                    .iter()
                    .map(|definition| definition.range)
                    .collect(),
            );
        }
        for (name, definitions) in &self.values {
            report(
                name,
                definitions
                    .iter()
                    .filter(|definition| definition.usable)
                    .map(|definition| definition.range)
                    .collect(),
            );
        }
    }

    /// fixity の宣言を、演算子が解決した先の定義に付ける。同じ定義への2回目の宣言は E1021、このモジュールで定義して
    /// いない演算子への宣言は E1022 にし、どちらも付けない (docs/spec/declarations.md の「fixity」)。
    fn attach_fixities(&mut self, tree: &ItemTree, diagnostics: &mut Vec<Diagnostic>) {
        let file = tree.file;
        for item in &tree.fixities {
            let Some(fixity) = item.fixity else {
                continue;
            };
            for (name, range) in &item.operators {
                let found = self.values.get(name).and_then(|definitions| {
                    definitions
                        .iter()
                        .find(|definition| definition.usable)
                        .or_else(|| definitions.first())
                });
                let Some(definition) = found else {
                    diagnostics.push(Diagnostic::error(
                        codes::FIXITY_WITHOUT_DEFINITION,
                        format!("`{name}` is not defined in this module"),
                        Label::new(
                            file,
                            *range,
                            "a fixity declaration needs a definition of its operator in the same module",
                        ),
                    ));
                    continue;
                };
                if let Some((_, _, first)) = self.fixities.get(&definition.item) {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::DUPLICATE_FIXITY,
                            format!("`{name}` has more than one fixity declaration"),
                            Label::new(file, *range, "declared again here"),
                        )
                        .with_secondary(Label::new(
                            file,
                            *first,
                            "first declared here",
                        )),
                    );
                    continue;
                }
                self.fixities
                    .insert(definition.item, (fixity, item.public, *range));
            }
        }
    }
}

fn push<T>(
    table: &mut HashMap<String, Vec<Definition<T>>>,
    name: &str,
    item: T,
    range: TextRange,
    public: bool,
    usable: bool,
) {
    table.entry(name.to_string()).or_default().push(Definition {
        item,
        range,
        public,
        usable,
    });
}

/// 重複した型引数 (E1003。宣言の変換が報告する) を除いた数。
fn unique_params(names: impl Iterator<Item = String>) -> usize {
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen.len()
}

/// 同じ名前空間の定義の重複 (docs/spec/modules.md の「名前空間」)。トップレベルの定義と、宣言の中の型引数に使う。
/// ソースで後に書いた方を primary にする。
pub(crate) fn duplicate(
    file: FileId,
    name: &str,
    first: TextRange,
    again: TextRange,
) -> Diagnostic {
    Diagnostic::error(
        codes::DUPLICATE_DEFINITION,
        format!("`{name}` is defined more than once"),
        Label::new(file, again, "defined again here"),
    )
    .with_secondary(Label::new(file, first, "first defined here"))
}

/// Prelude から、処理系が役割で引く item を名前で引く。`pub` によらない。Prelude は処理系と一緒に配るソースなので、
/// 見つからなければ panic する。
fn lang_items(prelude: &ModuleScope) -> LangItems {
    let ty = |name: &str| match prelude.types.get(name).and_then(|names| names.first()) {
        Some(Definition {
            item: TypeItem::Type(id),
            ..
        }) => *id,
        _ => unreachable!("the Prelude declares the type `{name}`"),
    };
    let effect = |name: &str| match prelude.types.get(name).and_then(|names| names.first()) {
        Some(Definition {
            item: TypeItem::Effect(id),
            ..
        }) => *id,
        _ => unreachable!("the Prelude declares the effect `{name}`"),
    };
    let value = |name: &str| match prelude.values.get(name).and_then(|names| names.first()) {
        Some(definition) => definition.item,
        None => unreachable!("the Prelude declares `{name}`"),
    };
    let constructor = |name: &str| match value(name) {
        Value::Constructor(id) => id,
        _ => unreachable!("`{name}` is a constructor of the Prelude"),
    };
    let function = |name: &str| match value(name) {
        Value::Function(id) => id,
        _ => unreachable!("`{name}` is a function of the Prelude"),
    };
    LangItems {
        int: ty("Int"),
        string: ty("String"),
        bool: ty("Bool"),
        unit: ty("Unit"),
        file: ty("File"),
        io: effect("IO"),
        true_ctor: constructor("True"),
        false_ctor: constructor("False"),
        negate: function("negate"),
        eq: function("=="),
        ne: function("!="),
        and: function("&&"),
        or: function("||"),
    }
}

impl DefMap {
    pub fn prelude(&self) -> ModuleId {
        self.prelude
    }

    pub fn entry(&self) -> ModuleId {
        self.entry
    }

    pub fn module_name(&self, module: ModuleId) -> &str {
        &self.scope(module).name
    }

    pub fn lang(&self) -> LangItems {
        self.lang
    }

    /// `module` の中から名前を引く口。
    pub fn resolver(&self, module: ModuleId) -> Resolver<'_> {
        Resolver {
            def_map: self,
            module,
        }
    }

    /// `ItemTree` の k 番目の関数の ID。`lower` が番号の一致を確かめるのに使う。
    pub fn function_id(&self, module: ModuleId, k: usize) -> FunctionId {
        self.scope(module).functions[k]
    }

    pub fn type_id(&self, module: ModuleId, k: usize) -> TypeDefId {
        self.scope(module).type_ids[k]
    }

    pub fn constructor_id(&self, module: ModuleId, data: usize, k: usize) -> ConstructorId {
        self.scope(module).constructors[data][k]
    }

    pub fn effect_id(&self, module: ModuleId, k: usize) -> EffectId {
        self.scope(module).effects[k]
    }

    pub fn operation_id(&self, module: ModuleId, effect: usize, k: usize) -> OperationId {
        self.scope(module).operations[effect][k]
    }

    fn scope(&self, module: ModuleId) -> &ModuleScope {
        &self.modules[u32::from(module.into_raw()) as usize]
    }
}

/// モジュールの中から名前を引く口。名前を引く順は「自分のモジュール → Prelude の `pub` の名前」である
/// (docs/spec/modules.md の「名前の解決」)。
#[derive(Clone, Copy)]
pub struct Resolver<'a> {
    def_map: &'a DefMap,
    module: ModuleId,
}

impl<'a> Resolver<'a> {
    pub fn module(&self) -> ModuleId {
        self.module
    }

    fn own(&self) -> &'a ModuleScope {
        self.def_map.scope(self.module)
    }

    /// 自分が Prelude でなければ Prelude。
    fn prelude(&self) -> Option<&'a ModuleScope> {
        (self.module != self.def_map.prelude).then(|| self.def_map.scope(self.def_map.prelude))
    }

    pub fn value(&self, name: &str) -> Option<ValueItem> {
        if let Some(definitions) = self.own().values.get(name) {
            if let Some(definition) = definitions.iter().find(|definition| definition.usable) {
                return Some(definition.item.item());
            }
            if !definitions.is_empty() {
                return Some(ValueItem::Unusable);
            }
        }
        self.prelude()?
            .values
            .get(name)?
            .iter()
            .find(|definition| definition.usable && definition.public)
            .map(|definition| definition.item.item())
    }

    /// パターンの先頭の名前。コンストラクタだけから引く (規則3)。
    pub fn constructor(&self, name: &str) -> Lookup<ConstructorId> {
        self.lookup(name, |value| match value {
            Value::Constructor(id) => Some(id),
            _ => None,
        })
    }

    /// handler の節の先頭の名前。操作だけから引く (規則3、docs/spec/modules.md の「名前の解決」)。
    pub fn operation(&self, name: &str) -> Lookup<OperationId> {
        self.lookup(name, |value| match value {
            Value::Operation(id) => Some(id),
            _ => None,
        })
    }

    fn lookup<T>(&self, name: &str, kind: impl Fn(Value) -> Option<T>) -> Lookup<T> {
        let mut unusable = false;
        for definition in self.own().values.get(name).into_iter().flatten() {
            let Some(found) = kind(definition.item) else {
                continue;
            };
            if definition.usable {
                return Lookup::Found(found);
            }
            unusable = true;
        }
        if unusable {
            return Lookup::Unusable;
        }
        let from_prelude = self.prelude().and_then(|prelude| {
            prelude
                .values
                .get(name)?
                .iter()
                .filter(|definition| definition.usable && definition.public)
                .find_map(|definition| kind(definition.item))
        });
        from_prelude.map_or(Lookup::NotFound, Lookup::Found)
    }

    pub fn type_item(&self, name: &str) -> Option<TypeItem> {
        if let Some(definition) = self.own().types.get(name).and_then(|names| names.first()) {
            return Some(definition.item);
        }
        self.prelude()?
            .types
            .get(name)?
            .iter()
            .find(|definition| definition.public)
            .map(|definition| definition.item)
    }

    pub fn type_params(&self, id: TypeDefId) -> usize {
        self.def_map
            .scope(id.module)
            .type_params
            .get(&id)
            .copied()
            .unwrap_or(0)
    }

    pub fn effect_params(&self, id: EffectId) -> usize {
        self.def_map
            .scope(id.module)
            .effect_params
            .get(&id)
            .copied()
            .unwrap_or(0)
    }

    /// 演算子の fixity。名前を解決した先の定義に付く。別のモジュールの定義の fixity は、宣言が `pub` のときだけ効く。
    /// 宣言がなければ `infixl 9` (docs/spec/declarations.md の「fixity」)。
    pub fn fixity(&self, op: &str) -> Fixity {
        let value = match self.value(op) {
            Some(ValueItem::Function(id)) => Value::Function(id),
            Some(ValueItem::Operation(id)) => Value::Operation(id),
            Some(ValueItem::Constructor(id)) => Value::Constructor(id),
            Some(ValueItem::Unusable) | None => return Fixity::DEFAULT,
        };
        let module = match value {
            Value::Function(id) => id.module,
            Value::Operation(id) => id.module,
            Value::Constructor(id) => id.module,
        };
        match self.def_map.scope(module).fixities.get(&value) {
            Some((fixity, public, _)) if module == self.module || *public => *fixity,
            _ => Fixity::DEFAULT,
        }
    }
}

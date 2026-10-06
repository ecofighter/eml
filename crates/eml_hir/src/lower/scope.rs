//! トップレベルの名前の解決。名前空間は、値 (関数、操作、コンストラクタ) と型 (型名とエフェクト名) の2つである
//! (docs/spec/modules.md の「名前空間」)。

use std::collections::HashMap;

use la_arena::Arena;

use eml_diagnostics::TextRange;

use crate::hir::{EffectDef, Generics, LangItems, TypeDef};
use crate::program::{
    ConstructorId, EffectId, FunctionId, ItemId, Items, Module, ModuleId, OperationId, TypeDefId,
};

/// 演算子の結合の向き (docs/spec/declarations.md の「fixity」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Assoc {
    Left,
    Right,
    None,
}

/// 演算子の優先順位と結合 (docs/spec/declarations.md の「fixity」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fixity {
    pub precedence: u8,
    pub assoc: Assoc,
}

impl Fixity {
    /// fixity の宣言がない演算子 (Haskell と同じ)。
    pub(super) const DEFAULT: Fixity = Fixity {
        precedence: 9,
        assoc: Assoc::Left,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueItem {
    Function(FunctionId),
    Operation(OperationId),
    Constructor(ConstructorId),
    /// 重複した `data` の型 (E1003) のコンストラクタ。使っても診断を足さない。
    Unusable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeItem {
    Type(TypeDefId),
    Effect(EffectId),
}

/// Prelude の `pub` の関数は名前解決の最も外側のスコープで、ユーザーの定義で隠せる。そのため、ユーザーの定義を
/// 先に引き、なければ Prelude の関数を引く。
#[derive(Debug, Default)]
pub(super) struct ItemScope {
    /// ユーザーが定義した値 (関数、操作、コンストラクタ)。
    values: HashMap<String, ValueItem>,
    types: HashMap<String, TypeItem>,
    /// エフェクトの型引数の個数。row のエフェクトの型引数の個数を確かめるのに使う (E1015)。
    effect_params: HashMap<EffectId, usize>,
    /// 型の型引数の個数。型の適用の型引数の個数を確かめるのに使う (E1015)。
    type_params: HashMap<TypeDefId, usize>,
    /// Prelude の `pub` の関数。ユーザーの定義が隠せるように、`values` の後で引く。
    prelude_functions: HashMap<String, FunctionId>,
    /// Prelude の演算子の fixity。
    prelude_fixities: HashMap<String, Fixity>,
    /// ユーザーが宣言した fixity と、宣言の演算子の位置。
    fixities: HashMap<String, (Fixity, TextRange)>,
}

impl ItemScope {
    pub(super) fn new() -> ItemScope {
        ItemScope::default()
    }

    /// 同じ名前のユーザーの値があれば、それを返す。重複の診断は呼び出し側が出す。
    pub(super) fn define_function(&mut self, name: &str, id: FunctionId) -> Option<ValueItem> {
        self.values
            .insert(name.to_string(), ValueItem::Function(id))
    }

    pub(super) fn define_operation(&mut self, name: &str, id: OperationId) -> Option<ValueItem> {
        self.values
            .insert(name.to_string(), ValueItem::Operation(id))
    }

    /// コンストラクタは大文字か `:` で始まる。関数と操作は小文字で始まるか、`:` で始まらない演算子である。パーサは `:` の演算子を関数として定義させないので、同じ名前のユーザーの値はない。
    pub(super) fn define_constructor(&mut self, name: &str, id: ConstructorId) {
        self.values
            .insert(name.to_string(), ValueItem::Constructor(id));
    }

    pub(super) fn define_unusable_constructor(&mut self, name: &str) {
        self.values
            .entry(name.to_string())
            .or_insert(ValueItem::Unusable);
    }

    pub(super) fn is_unusable(&self, name: &str) -> bool {
        matches!(self.values.get(name), Some(ValueItem::Unusable))
    }

    pub(super) fn define_type(&mut self, name: &str, id: TypeDefId, params: usize) {
        self.types.insert(name.to_string(), TypeItem::Type(id));
        self.type_params.insert(id, params);
    }

    pub(super) fn type_params(&self, id: TypeDefId) -> usize {
        self.type_params.get(&id).copied().unwrap_or(0)
    }

    /// パターンの先頭の名前は、コンストラクタだけから引く (docs/spec/modules.md の「名前の解決」)。
    pub(super) fn constructor(&self, name: &str) -> Option<ConstructorId> {
        match self.values.get(name) {
            Some(ValueItem::Constructor(id)) => Some(*id),
            _ => None,
        }
    }

    pub(super) fn define_effect(&mut self, name: &str, id: EffectId, params: usize) {
        self.types.insert(name.to_string(), TypeItem::Effect(id));
        self.effect_params.insert(id, params);
    }

    pub(super) fn effect_params(&self, id: EffectId) -> usize {
        self.effect_params.get(&id).copied().unwrap_or(0)
    }

    pub(super) fn define_prelude_function(&mut self, name: &str, id: FunctionId) {
        self.prelude_functions.insert(name.to_string(), id);
    }

    /// Prelude の `pub` の関数。ユーザーの定義に隠されていても引く。
    pub(super) fn prelude_function(&self, name: &str) -> Option<FunctionId> {
        self.prelude_functions.get(name).copied()
    }

    pub(super) fn value(&self, name: &str) -> Option<ValueItem> {
        self.values
            .get(name)
            .copied()
            .or_else(|| self.prelude_function(name).map(ValueItem::Function))
    }

    /// handler の節の先頭の名前は、エフェクトの操作だけから引く (docs/spec/modules.md の「名前の解決」)。
    pub(super) fn operation(&self, name: &str) -> Option<OperationId> {
        match self.values.get(name) {
            Some(ValueItem::Operation(id)) => Some(*id),
            _ => None,
        }
    }

    pub(super) fn declare_prelude_fixity(&mut self, op: &str, fixity: Fixity) {
        self.prelude_fixities.insert(op.to_string(), fixity);
    }

    /// 2回目の宣言なら、1回目の演算子の位置を返し、表を変えない。
    pub(super) fn declare_fixity(
        &mut self,
        op: &str,
        fixity: Fixity,
        range: TextRange,
    ) -> Result<(), TextRange> {
        if let Some((_, first)) = self.fixities.get(op) {
            return Err(*first);
        }
        self.fixities.insert(op.to_string(), (fixity, range));
        Ok(())
    }

    /// このモジュールで定義した値か。Prelude の `data` のコンストラクタも同じ表にあるが、演算子の名前のものはない。
    pub(super) fn defines_value(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// 演算子の fixity。fixity は名前が解決した先の定義に付く。ユーザーの定義は Prelude の演算子を隠すので、宣言の
    /// ないユーザーの演算子は `infixl 9` である (docs/spec/declarations.md の「fixity」)。
    pub(super) fn fixity(&self, op: &str) -> Fixity {
        if let Some((fixity, _)) = self.fixities.get(op) {
            return *fixity;
        }
        if self.values.contains_key(op) {
            return Fixity::DEFAULT;
        }
        self.prelude_fixities
            .get(op)
            .copied()
            .unwrap_or(Fixity::DEFAULT)
    }

    pub(super) fn type_item(&self, name: &str) -> Option<TypeItem> {
        self.types.get(name).copied()
    }
}

/// Prelude より前に作る組み込みの item。`Bool` は Prelude の `data` である。
pub(super) struct BuiltinItems {
    pub int: TypeDefId,
    pub string: TypeDefId,
    pub unit: TypeDefId,
    pub file: TypeDefId,
    pub io: EffectId,
}

/// 組み込みの型とエフェクトを item として登録する (docs/spec/declarations.md と docs/spec/effects.md)。`File` は組み込みの
/// 線形型である (docs/spec/effects.md)。
pub(super) fn builtin_items(
    module: ModuleId,
    items: &mut Items,
    scope: &mut ItemScope,
) -> BuiltinItems {
    let mut ty = |name: &str| {
        let id = ItemId::new(module, items.types.alloc(TypeDef::builtin(name)));
        scope.define_type(name, id, 0);
        id
    };
    let (int, string, unit, file) = (ty("Int"), ty("String"), ty("Unit"), ty("File"));
    let io = ItemId::new(
        module,
        items.effects.alloc(EffectDef {
            name: "IO".to_string(),
            generics: Generics::default(),
            operations: Vec::new(),
        }),
    );
    scope.define_effect("IO", io, 0);
    BuiltinItems {
        int,
        string,
        unit,
        file,
        io,
    }
}

/// Prelude を変換した直後、ユーザーの定義が同じ名前を上書きする前に呼ぶ。
pub(super) fn lang_items(
    builtin: BuiltinItems,
    scope: &ItemScope,
    modules: &Arena<Module>,
    prelude: &HashMap<String, FunctionId>,
) -> LangItems {
    let Some(TypeItem::Type(bool)) = scope.type_item("Bool") else {
        unreachable!("the Prelude declares `Bool`");
    };
    let constructor = |name: &str| match scope.constructor(name) {
        Some(id) => id,
        None => unreachable!("the Prelude declares `{name}`"),
    };
    let (false_ctor, true_ctor) = (constructor("False"), constructor("True"));
    // Core IR は `Bool` を、タグ 0 の `False` と 1 の `True` で表す (docs/spec/core-ir.md)
    let tag = |id: ConstructorId| modules[id.module].items.constructors[id.local].tag;
    assert_eq!((tag(false_ctor), tag(true_ctor)), (0, 1));
    let function = |name: &str| match prelude.get(name) {
        Some(&id) => id,
        None => unreachable!("the Prelude declares `{name}`"),
    };
    LangItems {
        int: builtin.int,
        string: builtin.string,
        bool,
        unit: builtin.unit,
        file: builtin.file,
        io: builtin.io,
        true_ctor,
        false_ctor,
        negate: function("negate"),
        eq: function("=="),
        ne: function("!="),
        and: function("&&"),
        or: function("||"),
        pipe: function("|>"),
        apply: function("<|"),
        io_operations: ["println", "open", "read_all", "close"].map(function),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::Function;

    #[test]
    fn user_functions_shadow_prelude_functions() {
        let mut functions: Arena<Function> = Arena::new();
        let module = ModuleId::from_raw(la_arena::RawIdx::from(0));
        let mut function = |name: &str| {
            ItemId::new(
                module,
                functions.alloc(Function {
                    name: name.to_string(),
                    name_range: Default::default(),
                    signature_name_range: None,
                    equation_ranges: Vec::new(),
                    signature: None,
                    intrinsic: false,
                }),
            )
        };
        let (prelude, user) = (function("not"), function("not"));
        let mut scope = ItemScope::new();
        scope.define_prelude_function("not", prelude);
        assert_eq!(scope.value("not"), Some(ValueItem::Function(prelude)));
        scope.define_function("not", user);
        assert_eq!(scope.value("not"), Some(ValueItem::Function(user)));
        assert_eq!(scope.prelude_function("not"), Some(prelude));
        assert_eq!(scope.value("nope"), None);
    }

    #[test]
    fn types_and_effects_share_the_type_namespace() {
        let mut items = Items::default();
        let mut scope = ItemScope::new();
        let module = ModuleId::from_raw(la_arena::RawIdx::from(0));
        let lang = builtin_items(module, &mut items, &mut scope);
        assert_eq!(scope.type_item("Int"), Some(TypeItem::Type(lang.int)));
        assert_eq!(scope.type_item("IO"), Some(TypeItem::Effect(lang.io)));
        assert_eq!(scope.type_item("Console"), None);
    }
}

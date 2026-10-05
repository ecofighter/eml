//! トップレベルの名前の解決。名前空間は、値 (関数、組み込みの値) と型 (型名とエフェクト名) の2つである
//! (docs/spec/modules.md の「名前空間」)。

use std::collections::HashMap;

use la_arena::Arena;

use crate::builtin::Builtin;
use crate::hir::{
    Constructor, ConstructorId, EffectDef, EffectId, FunctionId, Generics, LangItems, OperationId,
    TypeDef, TypeDefId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueItem {
    Function(FunctionId),
    Operation(OperationId),
    Constructor(ConstructorId),
    Builtin(Builtin),
    /// 重複した `data` の型 (E1003) のコンストラクタ。使っても診断を足さない。
    Unusable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeItem {
    Type(TypeDefId),
    Effect(EffectId),
}

/// 組み込みは名前解決の最も外側のスコープで、ユーザーの定義で隠せる。そのため、ユーザーの定義を先に引き、
/// なければ組み込みを引く。
#[derive(Debug, Default)]
pub(super) struct ItemScope {
    /// ユーザーが定義した値 (関数、操作、コンストラクタ)。
    values: HashMap<String, ValueItem>,
    types: HashMap<String, TypeItem>,
    /// エフェクトの型引数の個数。row のエフェクトの型引数の個数を確かめるのに使う (E1015)。
    effect_params: HashMap<EffectId, usize>,
    /// 型の型引数の個数。型の適用の型引数の個数を確かめるのに使う (E1015)。
    type_params: HashMap<TypeDefId, usize>,
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

    /// コンストラクタは大文字か `:` で始まり、関数と操作は小文字で始まるので、同じ名前のユーザーの値はない。
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

    pub(super) fn value(&self, name: &str) -> Option<ValueItem> {
        self.values
            .get(name)
            .copied()
            .or_else(|| Builtin::from_name(name).map(ValueItem::Builtin))
    }

    /// handler の節の先頭の名前は、エフェクトの操作だけから引く (docs/spec/modules.md の「名前の解決」)。
    pub(super) fn operation(&self, name: &str) -> Option<OperationId> {
        match self.values.get(name) {
            Some(ValueItem::Operation(id)) => Some(*id),
            _ => None,
        }
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
    pub io: EffectId,
}

/// 組み込みの型とエフェクトを item として登録する (docs/spec/declarations.md と docs/spec/effects.md)。
pub(super) fn builtin_items(
    types: &mut Arena<TypeDef>,
    effects: &mut Arena<EffectDef>,
    scope: &mut ItemScope,
) -> BuiltinItems {
    let mut ty = |name: &str| {
        let id = types.alloc(TypeDef::builtin(name));
        scope.define_type(name, id, 0);
        id
    };
    let (int, string, unit) = (ty("Int"), ty("String"), ty("Unit"));
    let io = effects.alloc(EffectDef {
        name: "IO".to_string(),
        generics: Generics::default(),
        operations: Vec::new(),
    });
    scope.define_effect("IO", io, 0);
    BuiltinItems {
        int,
        string,
        unit,
        io,
    }
}

/// Prelude を変換した直後、ユーザーの定義が同じ名前を上書きする前に呼ぶ。
pub(super) fn lang_items(
    builtin: BuiltinItems,
    scope: &ItemScope,
    constructors: &Arena<Constructor>,
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
    assert_eq!(
        (constructors[false_ctor].tag, constructors[true_ctor].tag),
        (0, 1)
    );
    LangItems {
        int: builtin.int,
        string: builtin.string,
        bool,
        unit: builtin.unit,
        io: builtin.io,
        true_ctor,
        false_ctor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::Function;

    #[test]
    fn user_functions_shadow_builtins() {
        let mut functions: Arena<Function> = Arena::new();
        let id = functions.alloc(Function {
            name: "not".to_string(),
            name_range: Default::default(),
            signature_name_range: None,
            signature: None,
            body: None,
        });
        let mut scope = ItemScope::new();
        assert_eq!(scope.value("not"), Some(ValueItem::Builtin(Builtin::Not)));
        scope.define_function("not", id);
        assert_eq!(scope.value("not"), Some(ValueItem::Function(id)));
        assert_eq!(scope.value("nope"), None);
    }

    #[test]
    fn types_and_effects_share_the_type_namespace() {
        let mut types = Arena::new();
        let mut effects = Arena::new();
        let mut scope = ItemScope::new();
        let lang = builtin_items(&mut types, &mut effects, &mut scope);
        assert_eq!(scope.type_item("Int"), Some(TypeItem::Type(lang.int)));
        assert_eq!(scope.type_item("IO"), Some(TypeItem::Effect(lang.io)));
        assert_eq!(scope.type_item("Console"), None);
    }
}

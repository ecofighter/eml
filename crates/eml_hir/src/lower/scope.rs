//! トップレベルの名前の解決。名前空間は、値 (関数、組み込みの値) と型 (型名とエフェクト名) の2つである
//! (docs/spec/modules.md の「名前空間」)。

use std::collections::HashMap;

use la_arena::Arena;

use crate::builtin::Builtin;
use crate::hir::{EffectDef, EffectId, FunctionId, LangItems, OperationId, TypeDef, TypeDefId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueItem {
    Function(FunctionId),
    Operation(OperationId),
    Builtin(Builtin),
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
    /// ユーザーが定義した値 (関数と操作)。
    values: HashMap<String, ValueItem>,
    types: HashMap<String, TypeItem>,
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

    pub(super) fn define_type(&mut self, name: &str, id: TypeDefId) {
        self.types.insert(name.to_string(), TypeItem::Type(id));
    }

    pub(super) fn define_effect(&mut self, name: &str, id: EffectId) {
        self.types.insert(name.to_string(), TypeItem::Effect(id));
    }

    pub(super) fn value(&self, name: &str) -> Option<ValueItem> {
        self.values
            .get(name)
            .copied()
            .or_else(|| Builtin::from_name(name).map(ValueItem::Builtin))
    }

    /// handler の節の先頭の名前は、エフェクトの操作だけから引く (docs/spec/modules.md の「名前の解決」)。
    #[expect(dead_code)]
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

/// 組み込みの型とエフェクトを item として登録し、lang item を返す (docs/spec/declarations.md と docs/spec/effects.md)。
pub(super) fn builtin_items(
    types: &mut Arena<TypeDef>,
    effects: &mut Arena<EffectDef>,
    scope: &mut ItemScope,
) -> LangItems {
    let mut ty = |name: &str| {
        let id = types.alloc(TypeDef {
            name: name.to_string(),
        });
        scope.define_type(name, id);
        id
    };
    let (int, string, bool, unit) = (ty("Int"), ty("String"), ty("Bool"), ty("Unit"));
    let io = effects.alloc(EffectDef {
        name: "IO".to_string(),
        operations: Vec::new(),
    });
    scope.define_effect("IO", io);
    LangItems {
        int,
        string,
        bool,
        unit,
        io,
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

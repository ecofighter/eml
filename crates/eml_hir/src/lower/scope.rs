//! トップレベルの名前の解決。名前空間は、値 (関数、組み込みの値) と型 (型名とエフェクト名) の2つである
//! (docs/spec/modules.md の「名前空間」)。

use std::collections::HashMap;

use crate::builtin::{Builtin, BuiltinType, builtin_effect};
use crate::hir::{EffectRef, FunctionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueItem {
    Function(FunctionId),
    Builtin(Builtin),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeItem {
    Builtin(BuiltinType),
    Effect(EffectRef),
}

/// 組み込みは名前解決の最も外側のスコープで、ユーザーの定義で隠せる。そのため、ユーザーの定義を先に引き、
/// なければ組み込みを引く。
#[derive(Debug, Default)]
pub(super) struct ItemScope {
    functions: HashMap<String, FunctionId>,
}

impl ItemScope {
    pub(super) fn new() -> ItemScope {
        ItemScope::default()
    }

    pub(super) fn define_function(&mut self, name: &str, id: FunctionId) {
        self.functions.insert(name.to_string(), id);
    }

    pub(super) fn value(&self, name: &str) -> Option<ValueItem> {
        self.functions
            .get(name)
            .map(|&id| ValueItem::Function(id))
            .or_else(|| Builtin::from_name(name).map(ValueItem::Builtin))
    }

    pub(super) fn type_item(&self, name: &str) -> Option<TypeItem> {
        BuiltinType::from_name(name)
            .map(TypeItem::Builtin)
            .or_else(|| builtin_effect(name).map(TypeItem::Effect))
    }
}

#[cfg(test)]
mod tests {
    use la_arena::Arena;

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
        let scope = ItemScope::new();
        assert_eq!(
            scope.type_item("Int"),
            Some(TypeItem::Builtin(BuiltinType::Int))
        );
        assert_eq!(scope.type_item("IO"), Some(TypeItem::Effect(EffectRef::Io)));
        assert_eq!(scope.type_item("Console"), None);
    }
}

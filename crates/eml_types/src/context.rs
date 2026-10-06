//! モジュール全体で1回だけ求める、型検査の前提。関数ごとの型の表はこれを借りるだけにして、表を作る費用を関数の
//! 大きさに比例させる (docs/implementation/architecture.md の「`eml_types` の内部」)。

use eml_hir::{
    Constructor, EffectDef, EffectId, LangItems, OpMultiplicity, Operation, OperationId, TypeDef,
    TypeDefId,
};
use la_arena::{Arena, ArenaMap};

use crate::data::{DataKind, data_kinds};
use crate::ty::Multiplicity;

pub(crate) struct Context {
    pub lang: LangItems,
    /// 型構成子ごとの、Kind の決まり方 (`crate::data`)。
    pub data_kinds: ArenaMap<TypeDefId, DataKind>,
    /// 名前を持つのは、`Module` を渡さずに型を書き出せるようにするため。
    pub type_names: ArenaMap<TypeDefId, String>,
    pub effect_names: ArenaMap<EffectId, String>,
    /// エフェクトがその row に入れる操作の上限。操作の多重度の最大である (docs/spec/types.md の「Kind」)。
    pub effect_multiplicities: ArenaMap<EffectId, Multiplicity>,
    pub operation_multiplicities: ArenaMap<OperationId, Multiplicity>,
}

impl Context {
    pub fn new(
        lang: LangItems,
        types: &Arena<TypeDef>,
        constructors: &Arena<Constructor>,
        effects: &Arena<EffectDef>,
        operations: &Arena<Operation>,
    ) -> Context {
        let operation_multiplicities = operations
            .iter()
            .map(|(id, operation)| (id, multiplicity(operation.multiplicity)))
            .collect();
        let effect_multiplicities = effects
            .iter()
            .map(|(id, effect)| {
                // 組み込みの `IO` は、実行時が必ず1回再開するので `Once` である (docs/spec/effects.md)
                let multiplicity = if id == lang.io {
                    Multiplicity::Once
                } else {
                    effect
                        .operations
                        .iter()
                        .map(|&op| multiplicity(operations[op].multiplicity))
                        .max()
                        .unwrap_or(Multiplicity::Never)
                };
                (id, multiplicity)
            })
            .collect();
        Context {
            lang,
            data_kinds: data_kinds(types, constructors, &lang),
            type_names: types
                .iter()
                .map(|(id, def)| (id, def.name.clone()))
                .collect(),
            effect_names: effects
                .iter()
                .map(|(id, def)| (id, def.name.clone()))
                .collect(),
            effect_multiplicities,
            operation_multiplicities,
        }
    }
}

fn multiplicity(multiplicity: OpMultiplicity) -> Multiplicity {
    match multiplicity {
        OpMultiplicity::Never => Multiplicity::Never,
        OpMultiplicity::Once => Multiplicity::Once,
        OpMultiplicity::Multi => Multiplicity::Multi,
    }
}

/// 組み込みの型だけを持つ前提。表と閉じた形の単体テストが、`Module` を組まずに表を作るのに使う。
#[cfg(test)]
pub(crate) fn test_context() -> Context {
    use eml_diagnostics::TextRange;
    use eml_hir::{FunctionId, Generics};
    use la_arena::RawIdx;

    let mut types = Arena::new();
    let mut effects = Arena::new();
    let mut constructors = Arena::new();
    let int = types.alloc(TypeDef::builtin("Int"));
    let string = types.alloc(TypeDef::builtin("String"));
    let bool = types.alloc(TypeDef::builtin("Bool"));
    let unit = types.alloc(TypeDef::builtin("Unit"));
    let file = types.alloc(TypeDef::builtin("File"));
    let mut constructor = |name: &str, tag| {
        constructors.alloc(Constructor {
            name: name.to_string(),
            range: TextRange::default(),
            ty: bool,
            tag,
            fields: Vec::new(),
        })
    };
    let function = |index: u32| FunctionId::from_raw(RawIdx::from(index));
    let false_ctor = constructor("False", 0);
    let true_ctor = constructor("True", 1);
    let lang = LangItems {
        int,
        string,
        bool,
        unit,
        file,
        io: effects.alloc(EffectDef {
            name: "IO".to_string(),
            generics: Generics::default(),
            operations: Vec::new(),
        }),
        true_ctor,
        false_ctor,
        // 関数の lang item は、型の表を作るときに読まない
        negate: function(0),
        eq: function(1),
        ne: function(2),
        and: function(3),
        or: function(4),
        pipe: function(5),
        apply: function(6),
        io_operations: [7, 8, 9, 10].map(function),
    };
    Context::new(lang, &types, &constructors, &effects, &Arena::new())
}

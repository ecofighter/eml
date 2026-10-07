//! プログラム全体で1回だけ求める、型検査の前提。関数ごとの型の表はこれを借りるだけにして、表を作る費用を関数の
//! 大きさに比例させる (docs/implementation/architecture.md の「`eml_types` の内部」)。

use std::collections::HashSet;

use eml_hir::{
    EffectDef, EffectId, EffectKind, ExternIndex, ItemMap, LangItems, OpMultiplicity, Operation,
    Program, TypeDef,
};

use crate::data::{DataKind, data_kinds};
use crate::ty::Multiplicity;

pub(crate) struct Context {
    pub lang: LangItems,
    pub externs: ExternIndex,
    /// extern のエフェクト。ユーザーのモジュールの extern (E1033) も含め、誤りを重ねない。
    extern_effects: HashSet<EffectId>,
    /// 型構成子ごとの、Kind の決まり方 (`crate::data`)。
    pub data_kinds: ItemMap<TypeDef, DataKind>,
    /// エフェクトがその row に入れる操作の上限。操作の多重度の最大で、extern のエフェクトは `Once` である
    /// (docs/spec/types.md の「Kind」)。
    pub effect_multiplicities: ItemMap<EffectDef, Multiplicity>,
    pub operation_multiplicities: ItemMap<Operation, Multiplicity>,
}

impl Context {
    /// プログラム全体の型、コンストラクタ、エフェクト、操作から表を作る。
    pub fn new(program: &Program) -> Context {
        let lang = program.lang;
        let operation_multiplicities = program
            .operations()
            .map(|(id, operation)| (id, multiplicity(operation.multiplicity)))
            .collect();
        let extern_effects: HashSet<EffectId> = program
            .effects()
            .filter(|(_, effect)| matches!(effect.kind, EffectKind::Extern(_)))
            .map(|(id, _)| id)
            .collect();
        let effect_multiplicities = program
            .effects()
            .map(|(id, effect)| {
                // extern のエフェクトは操作を持たないので、操作から求めると `Never` になってしまう。extern の関数は
                // その場で1回だけ戻るので `Once` である (docs/spec/types.md の「Kind」)
                let multiplicity = match effect.kind {
                    EffectKind::Extern(_) => Multiplicity::Once,
                    EffectKind::Defined => effect
                        .operations
                        .iter()
                        .map(|&op| multiplicity(program[op].multiplicity))
                        .max()
                        .unwrap_or(Multiplicity::Never),
                };
                (id, multiplicity)
            })
            .collect();
        Context {
            lang,
            externs: program.externs.clone(),
            extern_effects,
            data_kinds: data_kinds(program),
            effect_multiplicities,
            operation_multiplicities,
        }
    }

    /// extern のエフェクトは handle できないので、row の中で重なったら1つにまとめ、`mask` に入れない
    /// (docs/spec/types.md の「推論」)。
    pub fn is_extern_effect(&self, effect: EffectId) -> bool {
        self.extern_effects.contains(&effect)
    }
}

fn multiplicity(multiplicity: OpMultiplicity) -> Multiplicity {
    match multiplicity {
        OpMultiplicity::Never => Multiplicity::Never,
        OpMultiplicity::Once => Multiplicity::Once,
        OpMultiplicity::Multi => Multiplicity::Multi,
    }
}

/// Prelude だけのプログラムの前提。表と閉じた形の単体テストが使う。
#[cfg(test)]
pub(crate) fn test_context() -> Context {
    Context::new(&crate::test_program(""))
}

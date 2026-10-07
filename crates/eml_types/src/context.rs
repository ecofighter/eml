//! プログラム全体で1回だけ求める、型検査の前提。関数ごとの型の表はこれを借りるだけにして、表を作る費用を関数の
//! 大きさに比例させる (docs/implementation/architecture.md の「`eml_types` の内部」)。

use eml_hir::{
    EffectDef, ExternIndex, ItemMap, LangItems, OpMultiplicity, Operation, Program, TypeDef,
};

use crate::data::{DataKind, data_kinds};
use crate::ty::Multiplicity;

pub(crate) struct Context {
    pub lang: LangItems,
    pub externs: ExternIndex,
    /// 型構成子ごとの、Kind の決まり方 (`crate::data`)。
    pub data_kinds: ItemMap<TypeDef, DataKind>,
    /// エフェクトがその row に入れる操作の上限。操作の多重度の最大である (docs/spec/types.md の「Kind」)。
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
        let effect_multiplicities = program
            .effects()
            .map(|(id, effect)| {
                let multiplicity = effect
                    .operations
                    .iter()
                    .map(|&op| multiplicity(program[op].multiplicity))
                    .max()
                    .unwrap_or(Multiplicity::Never);
                (id, multiplicity)
            })
            .collect();
        Context {
            lang,
            externs: program.externs.clone(),
            data_kinds: data_kinds(program),
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

/// Prelude だけのプログラムの前提。表と閉じた形の単体テストが使う。
#[cfg(test)]
pub(crate) fn test_context() -> Context {
    Context::new(&crate::test_program(""))
}

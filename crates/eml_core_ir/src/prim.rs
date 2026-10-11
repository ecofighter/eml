//! evidence passing の基本操作 (docs/spec/core-ir.md の「基本操作」)。
//! 表は `eml_extern` に置かない。`eml_extern` の表は `std/` と1対1に結び付くが、基本操作はソースに現れないためである。

use crate::{Program, Repr};

/// 基本操作の種類と即値。エフェクトと操作は `Program.effects` の番号である。`EvSetClause` と `EvClause` は、
/// テキストの形で操作の名前をエフェクトで修飾するために、操作だけでなくエフェクトも持つ。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Prim {
    EvvGet,
    EvvSet,
    EvvPush,
    /// 飛ばすエフェクトの多重集合。
    EvvMask(Vec<u32>),
    EvNew(u32),
    EvSetClause {
        effect: u32,
        op: u32,
    },
    EvRenew,
    EvLookup(u32),
    EvClause {
        effect: u32,
        op: u32,
    },
    EvEvv,
    EvTakeState,
    EvSetState,
    Yielding,
    YieldStatus,
    YieldTo,
    YieldAbort,
    YieldContTake,
    YieldContSet,
    YieldFinish,
    Dummy,
}

/// 基本操作の名前と、引数と結果の Repr。引数と結果は、直接の呼び出しと同じく互換の位置である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimRow {
    /// テキストの形の名前。即値はエフェクトと操作の名前で書く (`ev_clause[State.get]`)。
    pub name: String,
    pub params: Vec<Repr>,
    pub ret: Repr,
}

impl Prim {
    pub fn row(&self, program: &Program) -> PrimRow {
        use Repr::{Int, Obj, TObj, Unit};
        let row = |name: String, params: &[Repr], ret| PrimRow {
            name,
            params: params.to_vec(),
            ret,
        };
        let plain = |name: &str| name.to_string();
        let effect = |name: &str, effect: u32| format!("{name}[{}]", effect_text(program, effect));
        let operation = |name: &str, effect: u32, op: u32| {
            format!("{name}[{}]", operation_text(program, effect, op))
        };
        match self {
            Prim::EvvGet => row(plain("evv_get"), &[], TObj),
            Prim::EvvSet => row(plain("evv_set"), &[TObj], Unit),
            Prim::EvvPush => row(plain("evv_push"), &[TObj, Obj], TObj),
            Prim::EvvMask(effects) => {
                let names: Vec<String> = effects
                    .iter()
                    .map(|&effect| effect_text(program, effect))
                    .collect();
                row(format!("evv_mask[{}]", names.join(", ")), &[TObj], TObj)
            }
            Prim::EvNew(e) => row(effect("ev_new", *e), &[TObj, TObj], Obj),
            Prim::EvSetClause { effect, op } => {
                row(operation("ev_set_clause", *effect, *op), &[Obj, TObj], Unit)
            }
            Prim::EvRenew => row(plain("ev_renew"), &[Obj, TObj], Obj),
            Prim::EvLookup(e) => row(effect("ev_lookup", *e), &[], Obj),
            Prim::EvClause { effect, op } => {
                row(operation("ev_clause", *effect, *op), &[Obj], TObj)
            }
            Prim::EvEvv => row(plain("ev_evv"), &[Obj], TObj),
            Prim::EvTakeState => row(plain("ev_take_state"), &[Obj], TObj),
            Prim::EvSetState => row(plain("ev_set_state"), &[Obj, TObj], Unit),
            Prim::Yielding => row(plain("yielding"), &[], Int),
            Prim::YieldStatus => row(plain("yield_status"), &[Obj], Int),
            Prim::YieldTo => row(plain("yield_to"), &[Obj, TObj, TObj], Unit),
            Prim::YieldAbort => row(plain("yield_abort"), &[Obj, TObj, TObj], Unit),
            Prim::YieldContTake => row(plain("yield_cont_take"), &[], TObj),
            Prim::YieldContSet => row(plain("yield_cont_set"), &[TObj], Unit),
            Prim::YieldFinish => row(plain("yield_finish"), &[], TObj),
            Prim::Dummy => row(plain("dummy"), &[], Obj),
        }
    }

    /// 基本操作はどれも実行の文脈か evidence を読み書きするので、縮約は消さない。
    pub fn pure(&self) -> bool {
        false
    }
}

/// エフェクトの名前。表にない番号は、`mask` と同じく `#N` で書く。誤りを含む IR も表示でき、verifier がその番号を
/// 報告できるようにするためである。
pub(crate) fn effect_text(program: &Program, effect: u32) -> String {
    program
        .effects
        .get(effect as usize)
        .map_or_else(|| format!("#{effect}"), |info| info.name.clone())
}

/// `E.op` の形。表にない操作は `E.#N` で書く。
pub(crate) fn operation_text(program: &Program, effect: u32, op: u32) -> String {
    let name = program
        .effects
        .get(effect as usize)
        .and_then(|info| info.operations.get(op as usize))
        .map_or_else(|| format!("#{op}"), |operation| operation.name.clone());
    format!("{}.{name}", effect_text(program, effect))
}

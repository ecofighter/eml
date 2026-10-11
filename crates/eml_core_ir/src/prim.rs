//! evidence passing の基本操作 (docs/spec/core-ir.md の「基本操作」)。
//! 表は `eml_extern` に置かない。`eml_extern` の表は `std/` と1対1に結び付くが、基本操作はソースに現れないためである。

use crate::Repr;

/// 基本操作の種類と即値。エフェクトと操作は `Program.effects` の番号である。`EvSetClause` と `EvClause` は、
/// テキストの形で操作の名前をエフェクトで修飾するために、操作だけでなくエフェクトも持つ。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Prim {
    EvvGet,
    EvvSet,
    EvvPush,
    /// 飛ばすエフェクトの多重集合。呼び出しの `mask` と同じく、空でなく番号の昇順に並ぶ。
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

/// 基本操作の引数と結果の Repr。引数と結果は、直接の呼び出しと同じく互換の位置である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimRow {
    pub params: &'static [Repr],
    pub ret: Repr,
}

/// 基本操作の即値。テキストの形は `[…]` に書き、verifier は番号が表にあるかを確かめる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Immediates<'a> {
    None,
    Effects(&'a [u32]),
    Effect(u32),
    Operation { effect: u32, op: u32 },
}

/// どの種類の基本操作も1つずつ持つ並び。即値は空か 0 である。名前は `Prim::name` にだけ書き、テキストを読む側は
/// この並びを名前で引く (`Prim::by_name`)。
const KINDS: [Prim; 20] = [
    Prim::EvvGet,
    Prim::EvvSet,
    Prim::EvvPush,
    Prim::EvvMask(Vec::new()),
    Prim::EvNew(0),
    Prim::EvSetClause { effect: 0, op: 0 },
    Prim::EvRenew,
    Prim::EvLookup(0),
    Prim::EvClause { effect: 0, op: 0 },
    Prim::EvEvv,
    Prim::EvTakeState,
    Prim::EvSetState,
    Prim::Yielding,
    Prim::YieldStatus,
    Prim::YieldTo,
    Prim::YieldAbort,
    Prim::YieldContTake,
    Prim::YieldContSet,
    Prim::YieldFinish,
    Prim::Dummy,
];

impl Prim {
    /// テキストの形で、即値の前に書く名前。
    pub fn name(&self) -> &'static str {
        match self {
            Prim::EvvGet => "evv_get",
            Prim::EvvSet => "evv_set",
            Prim::EvvPush => "evv_push",
            Prim::EvvMask(_) => "evv_mask",
            Prim::EvNew(_) => "ev_new",
            Prim::EvSetClause { .. } => "ev_set_clause",
            Prim::EvRenew => "ev_renew",
            Prim::EvLookup(_) => "ev_lookup",
            Prim::EvClause { .. } => "ev_clause",
            Prim::EvEvv => "ev_evv",
            Prim::EvTakeState => "ev_take_state",
            Prim::EvSetState => "ev_set_state",
            Prim::Yielding => "yielding",
            Prim::YieldStatus => "yield_status",
            Prim::YieldTo => "yield_to",
            Prim::YieldAbort => "yield_abort",
            Prim::YieldContTake => "yield_cont_take",
            Prim::YieldContSet => "yield_cont_set",
            Prim::YieldFinish => "yield_finish",
            Prim::Dummy => "dummy",
        }
    }

    /// 名前が `name` の種類の基本操作。即値は空か 0 で、読む側が埋める。
    pub(crate) fn by_name(name: &str) -> Option<Prim> {
        KINDS.into_iter().find(|kind| kind.name() == name)
    }

    pub(crate) fn immediates(&self) -> Immediates<'_> {
        match self {
            Prim::EvvMask(effects) => Immediates::Effects(effects),
            Prim::EvNew(effect) | Prim::EvLookup(effect) => Immediates::Effect(*effect),
            Prim::EvSetClause { effect, op } | Prim::EvClause { effect, op } => {
                Immediates::Operation {
                    effect: *effect,
                    op: *op,
                }
            }
            Prim::EvvGet
            | Prim::EvvSet
            | Prim::EvvPush
            | Prim::EvRenew
            | Prim::EvEvv
            | Prim::EvTakeState
            | Prim::EvSetState
            | Prim::Yielding
            | Prim::YieldStatus
            | Prim::YieldTo
            | Prim::YieldAbort
            | Prim::YieldContTake
            | Prim::YieldContSet
            | Prim::YieldFinish
            | Prim::Dummy => Immediates::None,
        }
    }

    pub fn row(&self) -> PrimRow {
        use Repr::{Int, Obj, TObj, Unit};
        let (params, ret): (&'static [Repr], Repr) = match self {
            Prim::EvvGet => (&[], TObj),
            Prim::EvvSet => (&[TObj], Unit),
            Prim::EvvPush => (&[TObj, Obj], TObj),
            Prim::EvvMask(_) => (&[TObj], TObj),
            Prim::EvNew(_) => (&[TObj, TObj], Obj),
            Prim::EvSetClause { .. } => (&[Obj, TObj], Unit),
            Prim::EvRenew => (&[Obj, TObj], Obj),
            Prim::EvLookup(_) => (&[], Obj),
            Prim::EvClause { .. } => (&[Obj], TObj),
            Prim::EvEvv => (&[Obj], TObj),
            Prim::EvTakeState => (&[Obj], TObj),
            Prim::EvSetState => (&[Obj, TObj], Unit),
            Prim::Yielding => (&[], Int),
            Prim::YieldStatus => (&[Obj], Int),
            Prim::YieldTo => (&[Obj, TObj, TObj], Unit),
            Prim::YieldAbort => (&[Obj, TObj, TObj], Unit),
            Prim::YieldContTake => (&[], TObj),
            Prim::YieldContSet => (&[TObj], Unit),
            Prim::YieldFinish => (&[], TObj),
            Prim::Dummy => (&[], Obj),
        };
        PrimRow { params, ret }
    }

    /// 基本操作はどれも実行の文脈か evidence を読み書きするので、縮約は消さない。
    pub fn pure(&self) -> bool {
        false
    }
}

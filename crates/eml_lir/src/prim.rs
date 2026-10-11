//! evidence passing の基本操作 (docs/spec/lir.md の「基本操作」)。
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
    /// 引数ごとの借用の印で、長さは `params` と同じである。借用の引数は読む使いで、所有を受け取らない。借用にするのは、
    /// 基本操作がその引数そのものを物体にも実行の文脈にも入れない引数である。中身の一部を基本操作が自分で `dup` して
    /// 共有する引数 (`evv_mask` のリストの残り、`ev_renew` の入口の表) も借用にする
    /// (docs/spec/lir.md の「基本操作」)。
    pub borrowed: &'static [bool],
    pub ret: Repr,
}

/// 基本操作の即値。テキストの形は `[…]` に書き、verifier は番号が表にあるかを確かめる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Immediates<'a> {
    None,
    Effects(&'a [u32]),
    Effect(u32),
    Operation { effect: u32, op: u32 },
}

/// 種類ごとの型、名前、見本 (即値を空か 0 にしたもの) の並びから、`KINDS` と `Prim::name` の網羅的な `match` を
/// 作る。並びから漏れた種類は `match` が網羅的でなくなるので、`KINDS` に入れ忘れることがない。
macro_rules! kinds {
    ($($kind:pat => $name:literal, $sample:expr;)*) => {
        /// どの種類の基本操作も1つずつ持つ並び。テキストを読む側は、この並びを名前で引く (`Prim::by_name`)。
        const KINDS: [Prim; [$($name),*].len()] = [$($sample),*];

        impl Prim {
            /// テキストの形で、即値の前に書く名前。
            pub fn name(&self) -> &'static str {
                match self {
                    $($kind => $name,)*
                }
            }
        }
    };
}

kinds! {
    Prim::EvvGet => "evv_get", Prim::EvvGet;
    Prim::EvvSet => "evv_set", Prim::EvvSet;
    Prim::EvvPush => "evv_push", Prim::EvvPush;
    Prim::EvvMask(_) => "evv_mask", Prim::EvvMask(Vec::new());
    Prim::EvNew(_) => "ev_new", Prim::EvNew(0);
    Prim::EvSetClause { .. } => "ev_set_clause", Prim::EvSetClause { effect: 0, op: 0 };
    Prim::EvRenew => "ev_renew", Prim::EvRenew;
    Prim::EvLookup(_) => "ev_lookup", Prim::EvLookup(0);
    Prim::EvClause { .. } => "ev_clause", Prim::EvClause { effect: 0, op: 0 };
    Prim::EvEvv => "ev_evv", Prim::EvEvv;
    Prim::EvTakeState => "ev_take_state", Prim::EvTakeState;
    Prim::EvSetState => "ev_set_state", Prim::EvSetState;
    Prim::Yielding => "yielding", Prim::Yielding;
    Prim::YieldStatus => "yield_status", Prim::YieldStatus;
    Prim::YieldTo => "yield_to", Prim::YieldTo;
    Prim::YieldAbort => "yield_abort", Prim::YieldAbort;
    Prim::YieldContTake => "yield_cont_take", Prim::YieldContTake;
    Prim::YieldContSet => "yield_cont_set", Prim::YieldContSet;
    Prim::YieldFinish => "yield_finish", Prim::YieldFinish;
    Prim::Dummy => "dummy", Prim::Dummy;
}

impl Prim {
    /// 名前が `name` の種類の基本操作。即値は空か 0 で、読む側が埋める。
    pub fn by_name(name: &str) -> Option<Prim> {
        KINDS.into_iter().find(|kind| kind.name() == name)
    }

    pub fn immediates(&self) -> Immediates<'_> {
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
        const NONE: &[bool] = &[];
        let (params, borrowed, ret): (&'static [Repr], &'static [bool], Repr) = match self {
            Prim::EvvGet => (&[], NONE, TObj),
            Prim::EvvSet => (&[TObj], &[false], Unit),
            Prim::EvvPush => (&[TObj, Obj], &[false, false], TObj),
            Prim::EvvMask(_) => (&[TObj], &[true], TObj),
            Prim::EvNew(_) => (&[TObj, TObj], &[false, false], Obj),
            Prim::EvSetClause { .. } => (&[Obj, TObj], &[true, false], Unit),
            Prim::EvRenew => (&[Obj, TObj], &[true, false], Obj),
            Prim::EvLookup(_) => (&[], NONE, Obj),
            Prim::EvClause { .. } => (&[Obj], &[true], TObj),
            Prim::EvEvv => (&[Obj], &[true], TObj),
            Prim::EvTakeState => (&[Obj], &[true], TObj),
            Prim::EvSetState => (&[Obj, TObj], &[true, false], Unit),
            Prim::Yielding => (&[], NONE, Int),
            Prim::YieldStatus => (&[Obj], &[true], Int),
            Prim::YieldTo => (&[Obj, TObj, TObj], &[false, false, false], Unit),
            Prim::YieldAbort => (&[Obj, TObj, TObj], &[false, false, false], Unit),
            Prim::YieldContTake => (&[], NONE, TObj),
            Prim::YieldContSet => (&[TObj], &[false], Unit),
            Prim::YieldFinish => (&[], NONE, TObj),
            Prim::Dummy => (&[], NONE, Obj),
        };
        PrimRow {
            params,
            borrowed,
            ret,
        }
    }

    /// 基本操作はどれも実行の文脈か evidence を読み書きするので、縮約は消さない。
    pub fn pure(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_prim() -> impl Iterator<Item = Prim> {
        KINDS.into_iter()
    }

    /// 見本が自分の行の種類でなければ、ほかの種類の名前が2回現れる。
    #[test]
    fn every_kind_has_its_own_name() {
        let mut names: Vec<&str> = every_prim().map(|prim| prim.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KINDS.len());
    }

    #[test]
    fn every_kind_marks_each_parameter() {
        for prim in every_prim() {
            let row = prim.row();
            assert_eq!(row.borrowed.len(), row.params.len(), "{}", prim.name());
        }
    }
}

//! evidence passing の基本操作と、REPL のセッションの根を読み書きする基本操作 (docs/spec/lir.md の「基本操作」)。
//! 表は `eml_extern` に置かない。`eml_extern` の表は `std/` と1対1に結び付くが、基本操作はソースに現れないためである。

use crate::{Mark, MaskItem, Repr};

/// 基本操作の種類と即値。エフェクトと操作は `Program.effects` の番号である。`EvSetClause` と `EvClause` は、
/// テキストの形で操作の名前をエフェクトで修飾するために、操作だけでなくエフェクトも持つ。`RootGet` と `RootSet` の
/// 即値は根の番号である。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Prim {
    EvvGet,
    EvvSet,
    EvvPush,
    /// 飛ばす項目。呼び出しの `mask` と同じく、空でなく、エフェクトの番号の真に昇順に並ぶ。
    EvvMask(Vec<MaskItem>),
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
    RootGet(u32),
    RootSet(u32),
    /// 不死の空の `MutArray`。`var` の配列の変数を取り出した後の状態である (docs/spec/lir.md の「基本操作」)。
    EmptyMutArray,
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
    /// 引数ごとの印で、長さは `params` と同じである。evidence と `evv` の位置は `unr`、状態、継続、節の本体の位置は
    /// `poly` である。evidence passing が自分の変数に付ける印と同じなので、`prim` の検査が evidence passing 自身を確かめる
    /// (docs/spec/lir.md の「基本操作」)。
    pub param_marks: &'static [Mark],
    pub ret_mark: Mark,
}

/// 基本操作の即値。テキストの形は `[…]` に書き、verifier は番号が表にあるかを確かめる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Immediates<'a> {
    None,
    Mask(&'a [MaskItem]),
    Effect(u32),
    Operation { effect: u32, op: u32 },
    Root(u32),
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
    Prim::RootGet(_) => "root_get", Prim::RootGet(0);
    Prim::RootSet(_) => "root_set", Prim::RootSet(0);
    Prim::EmptyMutArray => "empty_mut_array", Prim::EmptyMutArray;
}

impl Prim {
    /// 名前が `name` の種類の基本操作。即値は空か 0 で、読む側が埋める。
    pub fn by_name(name: &str) -> Option<Prim> {
        KINDS.into_iter().find(|kind| kind.name() == name)
    }

    pub fn immediates(&self) -> Immediates<'_> {
        match self {
            Prim::EvvMask(items) => Immediates::Mask(items),
            Prim::EvNew(effect) | Prim::EvLookup(effect) => Immediates::Effect(*effect),
            Prim::EvSetClause { effect, op } | Prim::EvClause { effect, op } => {
                Immediates::Operation {
                    effect: *effect,
                    op: *op,
                }
            }
            Prim::RootGet(root) | Prim::RootSet(root) => Immediates::Root(*root),
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
            | Prim::Dummy
            | Prim::EmptyMutArray => Immediates::None,
        }
    }

    pub fn row(&self) -> PrimRow {
        use Mark::{Lin, Poly, Unr};
        use Repr::{Int, Obj, TObj, Unit};
        const NONE: &[bool] = &[];
        const NO_MARKS: &[Mark] = &[];
        let (params, borrowed, ret, param_marks, ret_mark): (
            &'static [Repr],
            &'static [bool],
            Repr,
            &'static [Mark],
            Mark,
        ) = match self {
            Prim::EvvGet => (&[], NONE, TObj, NO_MARKS, Unr),
            Prim::EvvSet => (&[TObj], &[false], Unit, &[Unr], Unr),
            Prim::EvvPush => (&[TObj, Obj], &[false, false], TObj, &[Unr, Unr], Unr),
            Prim::EvvMask(_) => (&[TObj], &[true], TObj, &[Unr], Unr),
            Prim::EvNew(_) => (&[TObj, TObj], &[false, false], Obj, &[Poly, Unr], Unr),
            Prim::EvSetClause { .. } => (&[Obj, TObj], &[true, false], Unit, &[Unr, Unr], Unr),
            Prim::EvRenew => (&[Obj, TObj], &[true, false], Obj, &[Unr, Poly], Unr),
            Prim::EvLookup(_) => (&[], NONE, Obj, NO_MARKS, Unr),
            Prim::EvClause { .. } => (&[Obj], &[true], TObj, &[Unr], Unr),
            Prim::EvEvv => (&[Obj], &[true], TObj, &[Unr], Unr),
            Prim::EvTakeState => (&[Obj], &[true], TObj, &[Unr], Poly),
            Prim::EvSetState => (&[Obj, TObj], &[true, false], Unit, &[Unr, Poly], Unr),
            Prim::Yielding => (&[], NONE, Int, NO_MARKS, Unr),
            Prim::YieldStatus => (&[Obj], &[true], Int, &[Unr], Unr),
            Prim::YieldTo => (
                &[Obj, TObj, TObj],
                &[false, false, false],
                Unit,
                &[Unr, Poly, Poly],
                Unr,
            ),
            Prim::YieldAbort => (&[Obj, TObj], &[false, false], Unit, &[Unr, Poly], Unr),
            Prim::YieldContTake => (&[], NONE, TObj, NO_MARKS, Poly),
            Prim::YieldContSet => (&[TObj], &[false], Unit, &[Poly], Unr),
            Prim::YieldFinish => (&[], NONE, TObj, NO_MARKS, Poly),
            Prim::Dummy => (&[], NONE, Obj, NO_MARKS, Poly),
            // 根に置くのは `Unr` の値だけなので、どちらの位置も `unr` である
            Prim::RootGet(_) => (&[], NONE, TObj, NO_MARKS, Unr),
            Prim::RootSet(_) => (&[TObj], &[false], Unit, &[Unr], Unr),
            // 結果は `MutArray` の値なので `lin` である
            Prim::EmptyMutArray => (&[], NONE, Obj, NO_MARKS, Lin),
        };
        PrimRow {
            params,
            borrowed,
            ret,
            param_marks,
            ret_mark,
        }
    }

    /// `root_get` は根を読んで `dup` するだけで、`empty_mut_array` は定数を読むだけなので、結果が使われなければ
    /// 縮約が消してよい。ほかの基本操作は実行の文脈か evidence か根を書き換えるので消さない。
    pub fn pure(&self) -> bool {
        matches!(self, Prim::RootGet(_) | Prim::EmptyMutArray)
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

    #[test]
    fn every_kind_marks_each_parameter_like_its_repr() {
        for prim in every_prim() {
            let row = prim.row();
            assert_eq!(row.param_marks.len(), row.params.len(), "{}", prim.name());
        }
    }

    /// 状態、κ、節の本体の位置だけが `poly` で、ほかは `unr` である。根の位置も `unr` である。空の配列の結果だけが
    /// `lin` である (docs/spec/lir.md の「基本操作」)。
    #[test]
    fn the_marks_of_the_primitives_follow_the_spec_table() {
        use Mark::{Lin, Poly, Unr};
        let table: [(&str, &[Mark], Mark); 23] = [
            ("evv_get", &[], Unr),
            ("evv_set", &[Unr], Unr),
            ("evv_push", &[Unr, Unr], Unr),
            ("evv_mask", &[Unr], Unr),
            ("ev_new", &[Poly, Unr], Unr),
            ("ev_set_clause", &[Unr, Unr], Unr),
            ("ev_renew", &[Unr, Poly], Unr),
            ("ev_lookup", &[], Unr),
            ("ev_clause", &[Unr], Unr),
            ("ev_evv", &[Unr], Unr),
            ("ev_take_state", &[Unr], Poly),
            ("ev_set_state", &[Unr, Poly], Unr),
            ("yielding", &[], Unr),
            ("yield_status", &[Unr], Unr),
            ("yield_to", &[Unr, Poly, Poly], Unr),
            ("yield_abort", &[Unr, Poly], Unr),
            ("yield_cont_take", &[], Poly),
            ("yield_cont_set", &[Poly], Unr),
            ("yield_finish", &[], Poly),
            ("dummy", &[], Poly),
            ("root_get", &[], Unr),
            ("root_set", &[Unr], Unr),
            ("empty_mut_array", &[], Lin),
        ];
        assert_eq!(table.len(), KINDS.len());
        for (name, params, ret) in table {
            let row = Prim::by_name(name).expect("a primitive").row();
            assert_eq!(row.param_marks, params, "{name}");
            assert_eq!(row.ret_mark, ret, "{name}");
        }
    }
}

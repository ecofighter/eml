//! evidence passing の基本操作と、REPL のセッションの根を読み書きする基本操作 (docs/spec/lir.md の「基本操作」)。
//! 表は `eml_extern` に置かない。`eml_extern` の表は `std/` と1対1に結び付くが、基本操作はソースに現れないためである。

use crate::{Mark, MaskItem, Repr};

/// 基本操作の種類と即値。エフェクトは`Program.effects`の番号である。`RootGet`と`RootSet`の即値は根の番号である。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Prim {
    EvvGet,
    EvvSet,
    /// 借りたevidenceの節点をリストの先頭に足す。節点の参照はランタイムが自分で取る (docs/spec/lir.mdの「基本操作」)。
    EvvEnter,
    /// 飛ばす項目。呼び出しの `mask` と同じく、空でなく、エフェクトの番号の真に昇順に並ぶ。
    EvvMask(Vec<MaskItem>),
    EvRenew,
    /// 状態と外側のリストを取り出し終えた持ち主のevidenceを退かせる (docs/spec/lir.mdの「基本操作」)。
    EvRetire,
    EvEvv,
    EvTakeEvv,
    EvTakeState,
    /// `Ctx::ev_take_state`の持ち主の行。フレーム自身のevidenceの状態を取り出す。
    EvLastState,
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
    /// `par` の子を中断し、親で起こし直す閉包を渡す。戻らない (docs/spec/lir.md の「基本操作」)。
    ChildAbort,
}

/// 命令が自分の意味の一部として走らせる`Ctx`の操作で、それ自身は基本操作でない。`clause E.op`は`ev_lookup`、続いて
/// `ev_clause`を走らせ (docs/spec/lir.mdの「節の呼び出し」)、`evidence`は`ev_new`、続いてエントリごとに`ev_set_clause`を
/// 走らせる (docs/spec/lir.mdの「evidence」)。ネイティブのコードはそれぞれをエントリ
/// `eml_rt_<Step::name()>`で呼び、エントリは`Ctx`のメソッドと同じ引数を持つshimである。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    EvLookup,
    EvClause,
    EvNew,
    EvSetClause,
}

impl Step {
    pub const ALL: [Step; 4] = [
        Step::EvLookup,
        Step::EvClause,
        Step::EvNew,
        Step::EvSetClause,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Step::EvLookup => "ev_lookup",
            Step::EvClause => "ev_clause",
            Step::EvNew => "ev_new",
            Step::EvSetClause => "ev_set_clause",
        }
    }

    /// `ctx`の後にエントリが受けるオペランドの数。
    pub fn operands(self) -> usize {
        match self {
            Step::EvLookup => 1,
            Step::EvClause => 2,
            Step::EvNew => 4,
            Step::EvSetClause => 3,
        }
    }

    /// エントリが語を1つ返すか。
    pub fn returns(self) -> bool {
        match self {
            Step::EvLookup | Step::EvClause | Step::EvNew => true,
            Step::EvSetClause => false,
        }
    }
}

/// 基本操作の引数と結果の Repr。引数と結果は、直接の呼び出しと同じく互換の位置である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimRow {
    pub params: &'static [Repr],
    /// 引数ごとの借用の印で、長さは `params` と同じである。借用の引数は読む使いで、所有を受け取らない。借用にするのは、
    /// 基本操作がその引数そのものを物体にも実行の文脈にも入れない引数である。中身の一部を基本操作が自分で `dup` して
    /// 共有する引数 (`evv_mask` のリストの残り、`ev_renew` のエントリの表) も借用にする
    /// (docs/spec/lir.md の「基本操作」)。
    pub borrowed: &'static [bool],
    pub ret: Repr,
    /// 引数ごとの印で、長さは`params`と同じである。`ev`の位置は`lin`、ほかのevidenceと`evv`の位置は`unr`、状態、継続、
    /// 節の本体の位置は`poly`である。evidence passingが自分の変数に付ける印と同じなので、`prim`の検査がevidence passing自身を確かめる
    /// (docs/spec/lir.md の「基本操作」)。
    pub param_marks: &'static [Mark],
    pub ret_mark: Mark,
}

/// 基本操作の即値。テキストの形は `[…]` に書き、verifier は番号が表にあるかを確かめる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Immediates<'a> {
    None,
    Mask(&'a [MaskItem]),
    Root(u32),
}

/// 種類ごとの型、名前、見本 (即値を空か 0 にしたもの) の並びから、`Prim::KINDS` と `Prim::name` の網羅的な `match` を
/// 作る。並びから漏れた種類は `match` が網羅的でなくなるので、`Prim::KINDS` に入れ忘れることがない。
macro_rules! kinds {
    ($($kind:pat => $name:literal, $sample:expr;)*) => {
        impl Prim {
            /// どの種類の基本操作も1つずつ持つ並び。テキストを読む側は、この並びを名前で引く (`Prim::by_name`)。
            pub const KINDS: [Prim; [$($name),*].len()] = [$($sample),*];

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
    Prim::EvvEnter => "evv_enter", Prim::EvvEnter;
    Prim::EvvMask(_) => "evv_mask", Prim::EvvMask(Vec::new());
    Prim::EvRenew => "ev_renew", Prim::EvRenew;
    Prim::EvRetire => "ev_retire", Prim::EvRetire;
    Prim::EvEvv => "ev_evv", Prim::EvEvv;
    Prim::EvTakeEvv => "ev_take_evv", Prim::EvTakeEvv;
    Prim::EvTakeState => "ev_take_state", Prim::EvTakeState;
    Prim::EvLastState => "ev_last_state", Prim::EvLastState;
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
    Prim::ChildAbort => "child_abort", Prim::ChildAbort;
}

impl Prim {
    /// 名前が `name` の種類の基本操作。即値は空か 0 で、読む側が埋める。
    pub fn by_name(name: &str) -> Option<Prim> {
        Self::KINDS.into_iter().find(|kind| kind.name() == name)
    }

    pub fn immediates(&self) -> Immediates<'_> {
        match self {
            Prim::EvvMask(items) => Immediates::Mask(items),
            Prim::RootGet(root) | Prim::RootSet(root) => Immediates::Root(*root),
            Prim::EvvGet
            | Prim::EvvSet
            | Prim::EvvEnter
            | Prim::EvRenew
            | Prim::EvRetire
            | Prim::EvEvv
            | Prim::EvTakeEvv
            | Prim::EvTakeState
            | Prim::EvLastState
            | Prim::EvSetState
            | Prim::Yielding
            | Prim::YieldStatus
            | Prim::YieldTo
            | Prim::YieldAbort
            | Prim::YieldContTake
            | Prim::YieldContSet
            | Prim::YieldFinish
            | Prim::Dummy
            | Prim::EmptyMutArray
            | Prim::ChildAbort => Immediates::None,
        }
    }

    pub fn row(&self) -> PrimRow {
        use Mark::{Lin, Poly, Unr};
        use Repr::{Cev, Ev, Evv, Fev, Int, Obj, Rev, TObj, Unit};
        const NONE: &[bool] = &[];
        const NO_MARKS: &[Mark] = &[];
        let (params, borrowed, ret, param_marks, ret_mark): (
            &'static [Repr],
            &'static [bool],
            Repr,
            &'static [Mark],
            Mark,
        ) = match self {
            Prim::EvvGet => (&[], NONE, Evv, NO_MARKS, Unr),
            Prim::EvvSet => (&[Evv], &[false], Unit, &[Unr], Unr),
            Prim::EvvEnter => (&[Evv, Ev], &[false, true], Evv, &[Unr, Lin], Unr),
            Prim::EvvMask(_) => (&[Evv], &[true], Evv, &[Unr], Unr),
            Prim::EvRenew => (&[Rev, TObj], &[true, false], Ev, &[Unr, Poly], Lin),
            Prim::EvRetire => (&[Ev], &[false], Rev, &[Lin], Unr),
            Prim::EvEvv => (&[Cev], &[true], Evv, &[Unr], Unr),
            Prim::EvTakeEvv => (&[Ev], &[true], Evv, &[Lin], Unr),
            Prim::EvTakeState => (&[Cev], &[true], TObj, &[Unr], Poly),
            Prim::EvLastState => (&[Ev], &[true], TObj, &[Lin], Poly),
            Prim::EvSetState => (&[Cev, TObj], &[true, false], Unit, &[Unr, Poly], Unr),
            Prim::Yielding => (&[], NONE, Int, NO_MARKS, Unr),
            Prim::YieldStatus => (&[Ev], &[true], Int, &[Lin], Unr),
            Prim::YieldTo => (
                &[Cev, TObj, TObj],
                &[false, false, false],
                Unit,
                &[Unr, Poly, Poly],
                Unr,
            ),
            Prim::YieldAbort => (&[Cev, TObj], &[false, false], Unit, &[Unr, Poly], Unr),
            Prim::YieldContTake => (&[], NONE, TObj, NO_MARKS, Poly),
            Prim::YieldContSet => (&[TObj], &[false], Unit, &[Poly], Unr),
            Prim::YieldFinish => (&[], NONE, TObj, NO_MARKS, Poly),
            Prim::Dummy => (&[], NONE, Obj, NO_MARKS, Poly),
            // 根に置くのは `Unr` の値だけなので、どちらの位置も `unr` である
            Prim::RootGet(_) => (&[], NONE, TObj, NO_MARKS, Unr),
            Prim::RootSet(_) => (&[TObj], &[false], Unit, &[Unr], Unr),
            // 結果は `MutArray` の値なので `lin` である
            Prim::EmptyMutArray => (&[], NONE, Obj, NO_MARKS, Lin),
            // evidence は節のエントリが持ち続けるので借りる。操作の引数を捕まえた起こし直しの閉包は親に渡る
            Prim::ChildAbort => (&[Fev, TObj], &[true, false], Unit, &[Unr, Poly], Unr),
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
        Prim::KINDS.into_iter()
    }

    /// 見本が自分の行の種類でなければ、ほかの種類の名前が2回現れる。
    #[test]
    fn every_kind_has_its_own_name() {
        let mut names: Vec<&str> = every_prim().map(|prim| prim.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Prim::KINDS.len());
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

    /// 状態、κ、節の本体の位置だけが`poly`で、`ev`の位置と結果、空の配列の結果が`lin`で、ほかは`unr`である。根の
    /// 位置も`unr`である (docs/spec/lir.mdの「基本操作」)。
    #[test]
    fn the_marks_of_the_primitives_follow_the_spec_table() {
        use Mark::{Lin, Poly, Unr};
        let table: [(&str, &[Mark], Mark); 23] = [
            ("evv_get", &[], Unr),
            ("evv_set", &[Unr], Unr),
            ("evv_enter", &[Unr, Lin], Unr),
            ("evv_mask", &[Unr], Unr),
            ("ev_renew", &[Unr, Poly], Lin),
            ("ev_retire", &[Lin], Unr),
            ("ev_evv", &[Unr], Unr),
            ("ev_take_evv", &[Lin], Unr),
            ("ev_take_state", &[Unr], Poly),
            ("ev_last_state", &[Lin], Poly),
            ("ev_set_state", &[Unr, Poly], Unr),
            ("yielding", &[], Unr),
            ("yield_status", &[Lin], Unr),
            ("yield_to", &[Unr, Poly, Poly], Unr),
            ("yield_abort", &[Unr, Poly], Unr),
            ("yield_cont_take", &[], Poly),
            ("yield_cont_set", &[Poly], Unr),
            ("yield_finish", &[], Poly),
            ("dummy", &[], Poly),
            ("root_get", &[], Unr),
            ("root_set", &[Unr], Unr),
            ("empty_mut_array", &[], Lin),
            ("child_abort", &[Unr, Poly], Unr),
        ];
        assert_eq!(table.len(), Prim::KINDS.len());
        for (name, params, ret) in table {
            let row = Prim::by_name(name).expect("a primitive").row();
            assert_eq!(row.param_marks, params, "{name}");
            assert_eq!(row.ret_mark, ret, "{name}");
        }
    }
}

//! Kind の変数、`下限 ≤ 上限` の制約の両辺 (`Bound`)、束の値 (`Level`)、制約の由来、持ち越しの制約を置く
//! (docs/spec/types.md の「推論」)。線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の両方に使う。
//! 段1が集める問題とスキームは `problem` に、段2が SCC ごとに解く処理は `solve` にある。

use eml_diagnostics::{TextRange, TextSize};
use eml_hir::OperationId;

use crate::table::Row;
use crate::ty::{Linearity, Multiplicity};

pub(crate) mod problem;
pub(crate) mod solve;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KindVar(u32);

impl KindVar {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(index: usize) -> KindVar {
        KindVar(index as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Bound<T> {
    Const(T),
    Var(KindVar),
}

/// Kind の束の値。線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の2つがある (docs/spec/types.md の「Kind」)。
pub(crate) trait Level: Copy + Ord + std::hash::Hash + std::fmt::Debug {
    const BOTTOM: Self;
}

impl Level for Linearity {
    const BOTTOM: Self = Linearity::Unr;
}

impl Level for Multiplicity {
    const BOTTOM: Self = Multiplicity::Never;
}

/// Kind の制約の由来。制約が破れたときに E3001〜E3005 が指す場所と理由である (docs/spec/diagnostics.md の「線形性の診断」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KindOrigin {
    pub range: TextRange,
    pub reason: KindReason,
}

/// `drop x` の行を入れる位置と、その行の字下げ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DropFix {
    pub offset: TextSize,
    pub indent: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindReason {
    /// ある経路で2回以上使った変数。名前と、その経路で1回目と2回目に使った位置。
    UsedMoreThanOnce {
        name: String,
        first: TextRange,
        second: TextRange,
    },
    /// ある経路で使わなかった変数。`fix` は `drop x` の行を入れる先である。
    NotUsed {
        name: String,
        path: UnusedPath,
        fix: Option<DropFix>,
    },
    /// `once` の操作の節の `k` を、ある経路で `resume` も `drop` もしなかった。`clause` は節の範囲。
    ContinuationNotUsed { name: String, clause: TextRange },
    /// `_` で受けた値。
    Discarded,
    /// 操作の節が捕まえた変数。
    CapturedByClause(String),
    /// ラムダが捕まえた値。
    CapturedByLambda,
    /// トップレベルの関数、組み込み、操作のスキームから複写した制約。名前は参照した値の名前である。
    Passed(String),
    /// 型の単一化で出た制約。
    Unified,
    /// 呼び出しをまたいで持っている値。由来の範囲は呼び出しの範囲である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    /// `multi` は報告が指す `multi` の操作で、持ち越しのパスが決める。row を持たないのは、由来を型の表から切り離し、
    /// スキームに残せるようにするため。
    CarriedAcross {
        value: CarriedValue,
        multi: Option<OperationId>,
        call: CallKind,
    },
    /// スキームから複写した持ち越しの制約。由来の範囲は参照した位置である。`inner` は、呼んだ関数の中で値をまたがせている
    /// 位置の要約である (docs/spec/diagnostics.md の E3006)。
    CarriedThrough {
        name: String,
        inner: Option<CarriedInner>,
    },
}

impl KindReason {
    /// 同じ範囲の由来を並べる順。種類は宣言の順で、同じ種類は名前と位置で比べる。同じ値の持ち越しの違反から報告する1件を、
    /// 制約が並んだ順に左右されずに選ぶため (docs/spec/diagnostics.md の E3006)。
    pub fn order_key(&self) -> (u8, String, (u32, u32)) {
        let at = |range: TextRange| (u32::from(range.start()), u32::from(range.end()));
        let none = (0, 0);
        match self {
            KindReason::UsedMoreThanOnce { name, first, .. } => (0, name.clone(), at(*first)),
            KindReason::NotUsed { name, .. } => (1, name.clone(), none),
            KindReason::ContinuationNotUsed { name, clause } => (2, name.clone(), at(*clause)),
            KindReason::Discarded => (3, String::new(), none),
            KindReason::CapturedByClause(name) => (4, name.clone(), none),
            KindReason::CapturedByLambda => (5, String::new(), none),
            KindReason::Passed(name) => (6, name.clone(), none),
            KindReason::Unified => (7, String::new(), none),
            KindReason::CarriedAcross { value, .. } => (8, String::new(), at(value.key())),
            KindReason::CarriedThrough { name, inner } => (
                9,
                name.clone(),
                inner.as_ref().map_or(none, |inner| at(inner.range)),
            ),
        }
    }
}

/// 変数を使わなかった経路。E3003 の secondary が指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnusedPath {
    /// `if` の枝、または `match` の枝の本体。
    Branch(TextRange),
    /// `else` のない `if`。
    NoElse(TextRange),
    /// どの経路でも使わなかった。範囲はスコープの終わりの長さ0の範囲である。
    ScopeEnd(TextRange),
    /// どの経路でも使わないうちに、同じブロックの後の `let` で隠された。範囲は隠した束縛である。
    Shadowed(TextRange),
}

/// 呼び出しをまたいで持っている値。E3006 の secondary が指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CarriedValue {
    /// 局所変数。名前と束縛の範囲。
    Local { name: String, binding: TextRange },
    /// 評価済みで消費前の部分式の値。部分式の範囲。
    Temporary(TextRange),
    /// `return` の節が捕まえた変数。名前、束縛の範囲、節の範囲。
    ReturnCapture {
        name: String,
        binding: TextRange,
        clause: TextRange,
    },
}

impl CarriedValue {
    /// 同じ値を見分ける範囲。同じ値の違反は1件だけ報告する (docs/spec/diagnostics.md の E3006)。
    pub fn key(&self) -> TextRange {
        match self {
            CarriedValue::Local { binding, .. } | CarriedValue::ReturnCapture { binding, .. } => {
                *binding
            }
            CarriedValue::Temporary(range) => *range,
        }
    }
}

/// 値がまたぐもの。持ち越しのパスが多重度の成分を作るのに使う。操作の直接の呼び出しでは、その操作の多重度だけを見る。
#[derive(Debug, Clone)]
pub(crate) enum Across {
    Row(Row),
    Operation(OperationId),
}

/// 値がまたぐ式の種類。E3006 の primary の言い方を決める。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallKind {
    Call,
    /// `resume k v`。`k` が変数ならその名前。
    Resume {
        k: Option<String>,
    },
    Handle,
}

/// `CarriedThrough` が指す、呼んだ関数の中の持ち越しの1段分。報告は1段しかたどらないので入れ子にしない。入れ子にすると、
/// 呼び出しの段数だけ由来が深くなり、複写のたびにその深さの時間がかかる (docs/spec/diagnostics.md の E3006)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CarriedInner {
    pub range: TextRange,
    pub label: InnerLabel,
}

/// 報告の secondary の言い方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InnerLabel {
    /// 名前のある値を持ったまま呼んだ。
    Kept(String),
    /// さらに別の関数を通った。
    Through(String),
    /// 名前のない値を持ったまま呼んだ。
    Value,
}

impl CarriedInner {
    pub fn of(origin: &KindOrigin) -> CarriedInner {
        let label = match &origin.reason {
            KindReason::CarriedAcross {
                value: CarriedValue::Local { name, .. } | CarriedValue::ReturnCapture { name, .. },
                ..
            } => InnerLabel::Kept(name.clone()),
            KindReason::CarriedThrough { name, .. } => InnerLabel::Through(name.clone()),
            _ => InnerLabel::Value,
        };
        CarriedInner {
            range: origin.range,
            label,
        }
    }
}

/// 持ち越しの制約 (docs/spec/types.md の「推論」)。`lin` の解が `Lin` なら、`mult` の解は `Once` 以下でなければならない。
/// 線形性と多重度の束とは別に持つのは、スキームに残すときも由来を持つためである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Carry {
    pub lin: Bound<Linearity>,
    pub mult: Bound<Multiplicity>,
    pub origin: Option<KindOrigin>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_have_a_fixed_order() {
        let used = KindReason::UsedMoreThanOnce {
            name: "f".to_string(),
            first: TextRange::new(1.into(), 2.into()),
            second: TextRange::new(3.into(), 4.into()),
        };
        let unified = KindReason::Unified;
        assert!(used.order_key() < unified.order_key());
        let a = KindReason::Passed("a".to_string());
        let b = KindReason::Passed("b".to_string());
        assert!(a.order_key() < b.order_key());
    }
}

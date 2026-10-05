//! Kind の変数と `下限 ≤ 上限` の制約を集め、束の上で最小解を求める (docs/spec/types.md の「推論」)。
//! 線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の両方に使う。

use std::collections::{HashMap, HashSet};

use eml_diagnostics::{TextRange, TextSize};
use eml_hir::OperationId;

use crate::table::Row;
use crate::ty::{Linearity, Multiplicity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KindVar(u32);

impl KindVar {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bound<T> {
    Const(T),
    Var(KindVar),
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
    CarriedAcross {
        value: CarriedValue,
        across: Across,
        call: CallKind,
    },
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

/// 値がまたぐもの。row は報告するときに解く。操作の直接の呼び出しでは、その操作の多重度だけを見る。
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// 持ち越しの制約 (docs/spec/types.md の「推論」)。`lin` の解が `Lin` なら、`mult` の解は `Once` 以下でなければならない。
/// 線形性と多重度の束とは別に持つのは、スキームに残すときも由来を持つためである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Carry {
    pub lin: Bound<Linearity>,
    pub mult: Bound<Multiplicity>,
    pub origin: Option<KindOrigin>,
}

/// 破れた持ち越しの制約の番号。持ち越しの制約はどちらの束の最小解も動かさないので、解いた後に確かめればよい。
pub(crate) fn violated_carries(
    carries: &[Carry],
    lin: &[Linearity],
    mult: &[Multiplicity],
) -> Vec<usize> {
    let lin_value = |bound: Bound<Linearity>| match bound {
        Bound::Const(c) => c,
        Bound::Var(v) => lin[v.index()],
    };
    let mult_value = |bound: Bound<Multiplicity>| match bound {
        Bound::Const(c) => c,
        Bound::Var(v) => mult[v.index()],
    };
    carries
        .iter()
        .enumerate()
        .filter(|(_, carry)| {
            lin_value(carry.lin) == Linearity::Lin && mult_value(carry.mult) == Multiplicity::Multi
        })
        .map(|(index, _)| index)
        .collect()
}

#[derive(Debug)]
pub(crate) struct Lattice<T> {
    bottom: T,
    vars: usize,
    constraints: Vec<(Bound<T>, Bound<T>)>,
    /// 制約ごとの由来。`constraints` と同じ順に並ぶ。
    origins: Vec<Option<KindOrigin>>,
    /// これから作る制約に記録する由来。制約を作る側の引数を増やさずに済むよう、型の表が設定する。
    current: Option<KindOrigin>,
}

impl<T: Copy + Ord> Lattice<T> {
    pub fn new(bottom: T) -> Self {
        Lattice {
            bottom,
            vars: 0,
            constraints: Vec::new(),
            origins: Vec::new(),
            current: None,
        }
    }

    pub fn set_origin(&mut self, origin: Option<KindOrigin>) {
        self.current = origin;
    }

    pub fn origin(&self, index: usize) -> Option<&KindOrigin> {
        self.origins[index].as_ref()
    }

    pub fn fresh(&mut self) -> KindVar {
        self.vars += 1;
        KindVar(self.vars as u32 - 1)
    }

    pub fn require(&mut self, lower: Bound<T>, upper: Bound<T>) {
        self.constraints.push((lower, upper));
        self.origins.push(self.current.clone());
    }

    /// 最小解と、満たせなかった制約 (定数の上限を超えたもの) の番号を返す。束は有限で更新は単調なので止まる。
    pub fn solve(&self) -> (Vec<T>, Vec<usize>) {
        let mut values = vec![self.bottom; self.vars];
        let value = |values: &[T], bound: Bound<T>| match bound {
            Bound::Const(c) => c,
            Bound::Var(KindVar(v)) => values[v as usize],
        };
        loop {
            let mut changed = false;
            for &(lower, upper) in &self.constraints {
                if let Bound::Var(KindVar(v)) = upper {
                    let lower = value(&values, lower);
                    if lower > values[v as usize] {
                        values[v as usize] = lower;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let violated = self
            .constraints
            .iter()
            .enumerate()
            .filter(|(_, (lower, upper))| {
                matches!(upper, Bound::Const(c) if value(&values, *lower) > *c)
            })
            .map(|(index, _)| index)
            .collect();
        (values, violated)
    }

    pub fn value(&self, var: KindVar) -> T {
        self.solve().0[var.0 as usize]
    }

    /// `keep` の変数について、他の変数を経由した推移も含めて成り立つ制約を返す。多相化したスキームに残す制約で、
    /// 具体化のたびに複製する (docs/spec/types.md の「推論」)。下限が束の最小元なら何も言わないので省く。
    pub fn residual(&self, keep: &[KindVar]) -> Vec<(Bound<T>, Bound<T>)> {
        let kept: HashSet<KindVar> = keep.iter().copied().collect();
        let mut upward: HashMap<KindVar, Vec<Bound<T>>> = HashMap::new();
        let mut downward: HashMap<KindVar, Vec<Bound<T>>> = HashMap::new();
        for &(lower, upper) in &self.constraints {
            if let Bound::Var(v) = lower {
                upward.entry(v).or_default().push(upper);
            }
            if let Bound::Var(v) = upper {
                downward.entry(v).or_default().push(lower);
            }
        }
        let mut out = Vec::new();
        for &start in keep {
            let (vars, least_upper) = reach(&upward, &kept, start, |a, b| a.min(b));
            for var in vars {
                push_constraint(&mut out, (Bound::Var(start), Bound::Var(var)));
            }
            if let Some(c) = least_upper {
                push_constraint(&mut out, (Bound::Var(start), Bound::Const(c)));
            }
            let (_, greatest_lower) = reach(&downward, &kept, start, |a, b| a.max(b));
            if let Some(c) = greatest_lower.filter(|c| *c != self.bottom) {
                push_constraint(&mut out, (Bound::Const(c), Bound::Var(start)));
            }
        }
        out
    }

    /// スキームに残した制約を、具体化した新しい変数について足す。`map` にない変数はそのまま使う。
    pub fn copy_constraints(
        &mut self,
        constraints: &[(Bound<T>, Bound<T>)],
        map: &HashMap<KindVar, KindVar>,
    ) {
        let rename = |bound: Bound<T>| match bound {
            Bound::Var(v) => Bound::Var(map.get(&v).copied().unwrap_or(v)),
            constant => constant,
        };
        for &(lower, upper) in constraints {
            self.constraints.push((rename(lower), rename(upper)));
            self.origins.push(self.current.clone());
        }
    }
}

/// `start` から辺をたどり、出会った `keep` の変数 (その先へは進まない) と、出会った定数を `pick` でまとめた値を返す。
/// `keep` の変数の先は、その変数の残余の制約が受け持つ。
fn reach<T: Copy>(
    edges: &HashMap<KindVar, Vec<Bound<T>>>,
    keep: &HashSet<KindVar>,
    start: KindVar,
    pick: impl Fn(T, T) -> T,
) -> (Vec<KindVar>, Option<T>) {
    let mut vars = Vec::new();
    let mut constant: Option<T> = None;
    let mut seen = HashSet::from([start]);
    let mut work = vec![start];
    while let Some(var) = work.pop() {
        for &next in edges.get(&var).into_iter().flatten() {
            match next {
                Bound::Const(c) => constant = Some(constant.map_or(c, |old| pick(old, c))),
                Bound::Var(w) if !seen.insert(w) => {}
                Bound::Var(w) if keep.contains(&w) => vars.push(w),
                Bound::Var(w) => work.push(w),
            }
        }
    }
    (vars, constant)
}

fn push_constraint<T: PartialEq>(
    out: &mut Vec<(Bound<T>, Bound<T>)>,
    constraint: (Bound<T>, Bound<T>),
) {
    if !out.contains(&constraint) {
        out.push(constraint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconstrained_variables_take_the_bottom() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let v = lattice.fresh();
        assert_eq!(lattice.value(v), Linearity::Unr);
    }

    #[test]
    fn lower_bounds_propagate_through_chains() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let a = lattice.fresh();
        let b = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(b));
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(a));
        assert_eq!(
            lattice.solve(),
            (vec![Linearity::Lin, Linearity::Lin], vec![])
        );
    }

    #[test]
    fn an_upper_bound_below_the_solution_is_reported() {
        let mut lattice = Lattice::new(Multiplicity::Never);
        let a = lattice.fresh();
        lattice.require(Bound::Const(Multiplicity::Multi), Bound::Var(a));
        lattice.require(Bound::Var(a), Bound::Const(Multiplicity::Once));
        assert_eq!(lattice.solve().1, vec![1]);
    }

    #[test]
    fn residual_constraints_pass_through_internal_variables() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let a = lattice.fresh();
        let internal = lattice.fresh();
        let b = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(internal));
        lattice.require(Bound::Var(internal), Bound::Const(Linearity::Unr));
        lattice.require(Bound::Var(a), Bound::Var(b));
        assert_eq!(
            lattice.residual(&[a, b]),
            vec![
                (Bound::Var(a), Bound::Var(b)),
                (Bound::Var(a), Bound::Const(Linearity::Unr)),
            ]
        );
    }

    #[test]
    fn residual_lower_bounds_above_the_bottom_are_kept() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let internal = lattice.fresh();
        let c = lattice.fresh();
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(internal));
        lattice.require(Bound::Var(internal), Bound::Var(c));
        lattice.require(Bound::Const(Linearity::Unr), Bound::Var(c));
        assert_eq!(
            lattice.residual(&[c]),
            vec![(Bound::Const(Linearity::Lin), Bound::Var(c))]
        );
    }

    #[test]
    fn copied_constraints_use_the_new_variables() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let a = lattice.fresh();
        let copy = lattice.fresh();
        let map = std::collections::HashMap::from([(a, copy)]);
        lattice.copy_constraints(&[(Bound::Const(Linearity::Lin), Bound::Var(a))], &map);
        assert_eq!(lattice.value(copy), Linearity::Lin);
        assert_eq!(lattice.value(a), Linearity::Unr);
    }

    #[test]
    fn a_carry_breaks_only_when_the_value_is_linear_and_the_row_is_multi() {
        let carry = |lin, mult| Carry {
            lin,
            mult,
            origin: None,
        };
        let carries = [
            carry(Bound::Var(KindVar(0)), Bound::Var(KindVar(0))),
            carry(Bound::Const(Linearity::Lin), Bound::Var(KindVar(1))),
            carry(Bound::Var(KindVar(1)), Bound::Const(Multiplicity::Multi)),
        ];
        let lin = [Linearity::Lin, Linearity::Unr];
        let mult = [Multiplicity::Multi, Multiplicity::Once];
        assert_eq!(violated_carries(&carries, &lin, &mult), vec![0]);
    }
}

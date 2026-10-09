//! Kind のスキームの含意。instance のメソッドと既定のメソッドのスキームが、クラスのシグネチャから作ったスキームから
//! 導けるかを確かめる (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「instance と既定のメソッドの検査」)。
//! どちらのスキームも同じ `Shape` の Kind 変数の番号を使う。
//!
//! 導き方は健全だが完全ではない。前提の制約を辺としてたどった先の定数と、変数どうしの到達だけを見る。

use std::collections::{HashMap, HashSet};

use super::problem::KindScheme;
use super::{Bound, KindVar, Level};
use crate::ty::{Linearity, Multiplicity};

/// 前提から導けない制約。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unentailed {
    Linearity(Bound<Linearity>, Bound<Linearity>),
    Multiplicity(Bound<Multiplicity>, Bound<Multiplicity>),
    Carry(Bound<Linearity>, Bound<Multiplicity>),
}

/// `scheme` の制約のうち、`assumed` から導けないもの。
pub(crate) fn unentailed(assumed: &KindScheme, scheme: &KindScheme) -> Vec<Unentailed> {
    let lin = Lattice::new(&assumed.lin);
    let mult = Lattice::new(&assumed.mult);
    let mut out = Vec::new();
    for &(lower, upper) in &scheme.lin {
        if !lin.entails(lower, upper) {
            out.push(Unentailed::Linearity(lower, upper));
        }
    }
    for &(lower, upper) in &scheme.mult {
        if !mult.entails(lower, upper) {
            out.push(Unentailed::Multiplicity(lower, upper));
        }
    }
    let assumed_carries: HashSet<(Bound<Linearity>, Bound<Multiplicity>)> = assumed
        .carries
        .iter()
        .map(|carry| (carry.lin, carry.mult))
        .collect();
    for carry in &scheme.carries {
        // 値の側が必ず `Unr` か、row の側が必ず `Once` 以下なら、持ち越しの制約は破れない
        let entailed = lin.upper(carry.lin) == Linearity::Unr
            || mult.upper(carry.mult) <= Multiplicity::Once
            || assumed_carries.contains(&(carry.lin, carry.mult));
        if !entailed {
            out.push(Unentailed::Carry(carry.lin, carry.mult));
        }
    }
    out
}

/// 1つの束の前提。制約 `l ≤ u` を辺 `l → u` とし、変数に直接付いた定数の境界を持つ。
struct Lattice<T> {
    upward: HashMap<KindVar, Vec<KindVar>>,
    downward: HashMap<KindVar, Vec<KindVar>>,
    /// 変数に直接付いた定数の上限の最小と、下限の最大。
    upper: HashMap<KindVar, T>,
    lower: HashMap<KindVar, T>,
}

impl<T: Level> Lattice<T> {
    fn new(constraints: &[(Bound<T>, Bound<T>)]) -> Lattice<T> {
        let mut lattice = Lattice {
            upward: HashMap::new(),
            downward: HashMap::new(),
            upper: HashMap::new(),
            lower: HashMap::new(),
        };
        for &(lower, upper) in constraints {
            match (lower, upper) {
                (Bound::Var(a), Bound::Var(b)) => {
                    lattice.upward.entry(a).or_default().push(b);
                    lattice.downward.entry(b).or_default().push(a);
                }
                (Bound::Var(v), Bound::Const(c)) => {
                    let slot = lattice.upper.entry(v).or_insert(c);
                    *slot = (*slot).min(c);
                }
                (Bound::Const(c), Bound::Var(v)) => {
                    let slot = lattice.lower.entry(v).or_insert(c);
                    *slot = (*slot).max(c);
                }
                // 定数どうしの制約は前提として何も言わない
                (Bound::Const(_), Bound::Const(_)) => {}
            }
        }
        lattice
    }

    /// `lower ≤ upper` が前提から導けるか。
    fn entails(&self, lower: Bound<T>, upper: Bound<T>) -> bool {
        if self.upper(lower) <= self.lower(upper) {
            return true;
        }
        match (lower, upper) {
            (Bound::Var(a), Bound::Var(b)) => self.reachable(a, true).contains(&b),
            _ => false,
        }
    }

    /// 前提のもとでの値の上限。辺をたどって届く定数の上限の最小で、届かなければ束の最大元である。
    fn upper(&self, bound: Bound<T>) -> T {
        match bound {
            Bound::Const(c) => c,
            Bound::Var(v) => self
                .reachable(v, true)
                .iter()
                .filter_map(|var| self.upper.get(var).copied())
                .min()
                .unwrap_or(T::TOP),
        }
    }

    /// 前提のもとでの値の下限。辺を逆にたどって届く定数の下限の最大で、届かなければ束の最小元である。
    fn lower(&self, bound: Bound<T>) -> T {
        match bound {
            Bound::Const(c) => c,
            Bound::Var(v) => self
                .reachable(v, false)
                .iter()
                .filter_map(|var| self.lower.get(var).copied())
                .max()
                .unwrap_or(T::BOTTOM),
        }
    }

    /// `start` (自身を含む) から、`up` なら辺の向きに、そうでなければ逆向きに届く変数。作業の列でたどる。
    fn reachable(&self, start: KindVar, up: bool) -> HashSet<KindVar> {
        let edges = if up { &self.upward } else { &self.downward };
        let mut seen = HashSet::from([start]);
        let mut work = vec![start];
        while let Some(var) = work.pop() {
            for &next in edges.get(&var).into_iter().flatten() {
                if seen.insert(next) {
                    work.push(next);
                }
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::{Carry, Provenance};

    fn var<T>(index: usize) -> Bound<T> {
        Bound::Var(KindVar::from_index(index))
    }

    const UNR: Bound<Linearity> = Bound::Const(Linearity::Unr);
    const LIN: Bound<Linearity> = Bound::Const(Linearity::Lin);
    const ONCE: Bound<Multiplicity> = Bound::Const(Multiplicity::Once);

    fn lin(constraints: Vec<(Bound<Linearity>, Bound<Linearity>)>) -> KindScheme {
        KindScheme {
            lin: constraints,
            ..KindScheme::default()
        }
    }

    fn carry(lin: Bound<Linearity>, mult: Bound<Multiplicity>) -> Carry {
        Carry {
            lin,
            mult,
            origin: Provenance::Declaration,
        }
    }

    #[test]
    fn an_upper_bound_follows_the_edges_of_the_assumption() {
        let assumed = lin(vec![(var(0), var(1)), (var(1), UNR)]);
        assert_eq!(unentailed(&assumed, &lin(vec![(var(0), UNR)])), vec![]);
    }

    #[test]
    fn a_variable_bound_follows_reachability() {
        let assumed = lin(vec![(var(0), var(1)), (var(1), var(2))]);
        assert_eq!(unentailed(&assumed, &lin(vec![(var(0), var(2))])), vec![]);
        assert_eq!(
            unentailed(&assumed, &lin(vec![(var(2), var(0))])),
            vec![Unentailed::Linearity(var(2), var(0))]
        );
    }

    #[test]
    fn a_free_variable_entails_no_bound() {
        let scheme = lin(vec![(var(0), UNR), (LIN, var(1))]);
        assert_eq!(
            unentailed(&KindScheme::default(), &scheme),
            vec![
                Unentailed::Linearity(var(0), UNR),
                Unentailed::Linearity(LIN, var(1)),
            ]
        );
    }

    #[test]
    fn a_lower_bound_meets_an_upper_bound_through_constants() {
        // 前提が a ≤ Unr と Lin ≤ b を言えば、a ≤ b が導ける
        let assumed = lin(vec![(var(0), UNR), (LIN, var(1))]);
        assert_eq!(unentailed(&assumed, &lin(vec![(var(0), var(1))])), vec![]);
    }

    #[test]
    fn a_carry_is_entailed_by_an_unrestricted_value_a_bounded_row_or_the_same_pair() {
        let assumed = KindScheme {
            lin: vec![(var(0), UNR)],
            mult: vec![(var(0), ONCE)],
            carries: vec![carry(var(2), var(1))],
        };
        let scheme = KindScheme {
            carries: vec![
                carry(var(0), var(1)),
                carry(var(1), var(0)),
                carry(var(2), var(1)),
                carry(var(1), var(1)),
            ],
            ..KindScheme::default()
        };
        assert_eq!(
            unentailed(&assumed, &scheme),
            vec![Unentailed::Carry(var(1), var(1))]
        );
    }
}

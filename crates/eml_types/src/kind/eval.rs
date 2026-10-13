//! 宣言の Kind のスキームの上の評価。定数に畳める変数と、instance での値 `val` を求める
//! (docs/spec/types.md の「instance での評価」と「書き出すときの規則」)。
//! スキームの変数は `Shape` の番号である。

use super::Bound;
use super::problem::KindScheme;
use crate::store::Lin;
use crate::ty::Linearity;

/// スキームだけから決まる、宣言自身の変数の値。どの instance でも同じなので、定数に畳める。
#[derive(Debug)]
pub(crate) struct SchemeFacts {
    /// 辺を上にたどって定数の `Unr` の上限に届く。
    pub fixed_unr: Vec<bool>,
    /// 辺を下にたどって定数の `Lin` の下限に届く。
    pub forced_lin: Vec<bool>,
    /// 辺の下から上への向き。`values` が結びを上へ伝えるのに使う。
    upward: Vec<Vec<usize>>,
    /// 定数の `Lin` を直接の下限に持つ変数。
    lin_seeds: Vec<usize>,
}

/// 変数ごとにたどり直さず、定数を持つ変数から辺に沿って印を広げる。宣言ごとに変数と制約の数に比例する仕事で済む。
pub(crate) fn scheme_facts(scheme: &KindScheme, lin_vars: usize) -> SchemeFacts {
    let mut fixed_unr = vec![false; lin_vars];
    let mut forced_lin = vec![false; lin_vars];
    let mut upward = vec![Vec::new(); lin_vars];
    let mut downward = vec![Vec::new(); lin_vars];
    let mut lin_seeds = Vec::new();
    for &(lower, upper) in &scheme.lin {
        match (lower, upper) {
            (Bound::Var(lower), Bound::Var(upper)) => {
                upward[lower.index()].push(upper.index());
                downward[upper.index()].push(lower.index());
            }
            (Bound::Var(var), Bound::Const(Linearity::Unr)) => fixed_unr[var.index()] = true,
            (Bound::Const(Linearity::Lin), Bound::Var(var)) => {
                forced_lin[var.index()] = true;
                lin_seeds.push(var.index());
            }
            _ => {}
        }
    }
    // `Unr` の上限は下の変数へ、`Lin` の下限は上の変数へ伝わる
    spread(&mut fixed_unr, &downward);
    spread(&mut forced_lin, &upward);
    SchemeFacts {
        fixed_unr,
        forced_lin,
        upward,
        lin_seeds,
    }
}

fn spread(marks: &mut [bool], edges: &[Vec<usize>]) {
    let mut work: Vec<usize> = (0..marks.len()).filter(|&var| marks[var]).collect();
    while let Some(var) = work.pop() {
        for &next in &edges[var] {
            if !marks[next] {
                marks[next] = true;
                work.push(next);
            }
        }
    }
}

/// 宣言自身の変数の `val`。`base(v)` は、型変数の `μ` なら型引数の線形性、矢印の `m` なら `Poly` である。値は `Unr`、
/// `Poly`、`Lin` の向きに上がるだけなので、輪のある制約でもワークリストの固定点が止まる。
pub(crate) fn values(
    lin_vars: usize,
    base: impl Fn(usize) -> Lin,
    facts: &SchemeFacts,
) -> Vec<Lin> {
    let mut values: Vec<Lin> = (0..lin_vars)
        .map(|var| {
            if facts.fixed_unr[var] {
                Lin::Unr
            } else {
                base(var)
            }
        })
        .collect();
    for &var in &facts.lin_seeds {
        if !facts.fixed_unr[var] {
            values[var] = Lin::Lin;
        }
    }
    // 固定は値より先に決まる。固定した変数は、下にある変数の値を結ばない
    let mut work: Vec<usize> = (0..lin_vars).collect();
    while let Some(var) = work.pop() {
        for &next in &facts.upward[var] {
            if facts.fixed_unr[next] {
                continue;
            }
            let joined = values[next].join(values[var]);
            if joined != values[next] {
                values[next] = joined;
                work.push(next);
            }
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::KindVar;

    fn var(index: usize) -> Bound<Linearity> {
        Bound::Var(KindVar::from_index(index))
    }

    #[test]
    fn an_own_variable_below_a_constant_lin_is_lin_through_another_own_variable() {
        // 0 は定数の `Lin` の上にあり、1 は 0 の上、2 は 1 の上にある。0 から 2 へ辺を上にたどるので、`base` が
        // `Unr` でも 2 まで `Lin` になる。3 はどの辺とも関係ないので `base` のままである
        let scheme = KindScheme {
            lin: vec![
                (Bound::Const(Linearity::Lin), var(0)),
                (var(0), var(1)),
                (var(1), var(2)),
            ],
            ..KindScheme::default()
        };
        let facts = scheme_facts(&scheme, 4);
        assert_eq!(facts.forced_lin, [true, true, true, false]);
        assert_eq!(
            values(4, |_| Lin::Unr, &facts),
            [Lin::Lin, Lin::Lin, Lin::Lin, Lin::Unr]
        );
    }
}

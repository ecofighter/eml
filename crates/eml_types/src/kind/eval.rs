//! 宣言の Kind のスキームの上の評価。定数に畳める変数と、instance での値 `val` を求める
//! (docs/superpowers/specs/2026-10-12-s8c-kind-export-design.md の「instance での評価」と「書き出すときの規則」)。
//! スキームの変数は `Shape` の番号である。

use super::Bound;
use super::problem::KindScheme;
use crate::store::Lin;
use crate::ty::Linearity;

/// スキームだけから決まる、宣言自身の変数の値。どの instance でも同じなので、定数に畳める。
pub(crate) struct SchemeFacts {
    /// 辺を上にたどって定数の `Unr` の上限に届く。
    pub fixed_unr: Vec<bool>,
    /// 辺を下にたどって定数の `Lin` の下限に届く。
    pub forced_lin: Vec<bool>,
}

/// 変数ごとにたどり直さず、定数を持つ変数から辺に沿って印を広げる。宣言ごとに変数と制約の数に比例する仕事で済む。
pub(crate) fn scheme_facts(scheme: &KindScheme, lin_vars: usize) -> SchemeFacts {
    let mut fixed_unr = vec![false; lin_vars];
    let mut forced_lin = vec![false; lin_vars];
    let mut upward = vec![Vec::new(); lin_vars];
    let mut downward = vec![Vec::new(); lin_vars];
    for &(lower, upper) in &scheme.lin {
        match (lower, upper) {
            (Bound::Var(lower), Bound::Var(upper)) => {
                upward[lower.index()].push(upper.index());
                downward[upper.index()].push(lower.index());
            }
            (Bound::Var(var), Bound::Const(Linearity::Unr)) => fixed_unr[var.index()] = true,
            (Bound::Const(Linearity::Lin), Bound::Var(var)) => forced_lin[var.index()] = true,
            _ => {}
        }
    }
    // `Unr` の上限は下の変数へ、`Lin` の下限は上の変数へ伝わる
    spread(&mut fixed_unr, &downward);
    spread(&mut forced_lin, &upward);
    SchemeFacts {
        fixed_unr,
        forced_lin,
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
    scheme: &KindScheme,
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
    let mut upward = vec![Vec::new(); lin_vars];
    for &(lower, upper) in &scheme.lin {
        match (lower, upper) {
            (Bound::Var(lower), Bound::Var(upper)) => {
                upward[lower.index()].push(upper.index());
            }
            (Bound::Const(Linearity::Lin), Bound::Var(var)) if !facts.fixed_unr[var.index()] => {
                values[var.index()] = Lin::Lin;
            }
            _ => {}
        }
    }
    // 固定は値より先に決まる。固定した変数は、下にある変数の値を結ばない
    let mut work: Vec<usize> = (0..lin_vars).collect();
    while let Some(var) = work.pop() {
        for &next in &upward[var] {
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

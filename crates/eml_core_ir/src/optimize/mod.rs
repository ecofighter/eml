//! 最適化パス。translate の出力を EP の前に書き換える (docs/spec/core-ir.md)。既知の閉包への `apply` を直接の
//! 呼び出しにし、`yields` の印を付け直し、到達できない関数を落とす
//! (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「S13e-1 既知の閉包への特殊化」)。

mod known;
mod reach;
mod yields;

use crate::Program;

pub(crate) use reach::retain_reachable;
pub(crate) use yields::{kept_marks, recompute_yields};

pub fn optimize(program: &mut Program) {
    let callees: Vec<known::Callee> = program.functions.iter().map(known::Callee::of).collect();
    for function in &mut program.functions {
        known::rewrite(function, &callees);
    }
    let keep = kept_marks(program);
    recompute_yields(program, &keep);
    retain_reachable(program);
}

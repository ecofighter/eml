//! 最適化パス。translate の出力を EP の前に書き換える (docs/spec/core-ir.md)。関数ごとの作業リストで、既知の閉包への
//! `apply` を直接の呼び出しにし、既知の閉包を受ける関数と閉包を鍵ごとの複製にする。そのあと `yields` の印を付け直し、
//! 到達できない関数を落とす
//! (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「S13e-1 既知の閉包への特殊化」)。

mod handlers;
mod known;
mod reach;
mod specialize;
mod yields;

use crate::Program;

pub(crate) use reach::retain_reachable;
pub(crate) use yields::{kept_marks, recompute_yields};

pub fn optimize(program: &mut Program) {
    specialize::specialize(program);
    // handler への特殊化は、付け直す前の印も読む (`handlers::path`)
    let translated: Vec<bool> = program
        .functions
        .iter()
        .map(|function| function.yields)
        .collect();
    let keep = kept_marks(program);
    recompute_yields(program, &keep);
    handlers::handlers(program, &translated);
    let keep = kept_marks(program);
    recompute_yields(program, &keep);
    retain_reachable(program);
}

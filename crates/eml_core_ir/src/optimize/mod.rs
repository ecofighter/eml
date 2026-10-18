//! 最適化パス。translate の出力を EP の前に書き換える (docs/spec/core-ir.md)。いまは到達できない関数を落とすだけである。

mod reach;

use crate::Program;

pub(crate) use reach::retain_reachable;

/// 入口から到達できる関数だけを残す。
pub fn optimize(program: &mut Program) {
    retain_reachable(program);
}

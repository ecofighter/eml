//! 具体化した row が yield しうるかの分類 (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の
//! 「row の具体化の記録」)。

use crate::RowYield;
use crate::table::{Row, RowVar, Table, Tail};

/// 解いた row を分類する。`own` は、この定義の row 変数を `generics.row_vars` の順に並べたものである。
pub(super) fn row_yield(table: &Table, row: &Row, own: &[RowVar]) -> RowYield {
    let row = table.resolve_row(row);
    if row
        .labels
        .iter()
        .any(|label| !table.is_extern_effect(label.effect))
    {
        return RowYield::Always;
    }
    match row.tail {
        Tail::Closed | Tail::Error => RowYield::Never,
        // 最後まで決まらなかった row 変数には、どのエフェクトも求められていない。yield しうる値はそこを通らない
        Tail::Var(var) if !table.is_rigid_row(var) => RowYield::Never,
        Tail::Var(var) => match own.iter().position(|&o| o == var) {
            Some(index) => RowYield::Var(index as u32),
            // 操作のシグネチャの row 変数を、節の中で具体化したもの
            None => RowYield::Always,
        },
    }
}

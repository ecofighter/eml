//! 具体化した row が yield しうるかの分類 (docs/spec/types.md の
//! 「row の具体化の記録」)。

use super::*;
use crate::RowYield;

impl Table<'_> {
    /// 解いた row を分類する。`own` は、この定義の row 変数を `generics.row_vars` の順に並べたものである。
    pub fn row_yield(&self, row: &Row, own: &[RowVar]) -> RowYield {
        let row = self.resolve_row(row);
        // `IO` も、失敗すると `try_io` まで中断するので yield しうる (docs/spec/types.md の「row の具体化の記録」)
        if !row.labels.is_empty() {
            return RowYield::Always;
        }
        self.tail_yield(row.tail, own)
    }

    /// 参照の具体化が記録した、ラベルを持たない row 変数を分類する。
    pub fn row_var_yield(&self, var: RowVar, own: &[RowVar]) -> RowYield {
        match &self.row_vars[var.0 as usize].binding {
            Some(bound) => self.row_yield(bound, own),
            None => self.tail_yield(Tail::Var(var), own),
        }
    }

    /// 束縛のない末尾を分類する。
    fn tail_yield(&self, tail: Tail, own: &[RowVar]) -> RowYield {
        match tail {
            Tail::Closed | Tail::Error => RowYield::Never,
            // 最後まで決まらなかった row 変数には、どのエフェクトも求められていない。yield しうる値はそこを通らない
            Tail::Var(var) if !self.is_rigid_row(var) => RowYield::Never,
            Tail::Var(var) => match own.iter().position(|&o| o == var) {
                Some(index) => RowYield::Var(index as u32),
                // 操作のシグネチャの row 変数を、節の中で具体化したもの
                None => RowYield::Always,
            },
        }
    }
}

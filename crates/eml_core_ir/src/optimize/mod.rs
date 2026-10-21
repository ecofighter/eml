//! 最適化パス。translate の出力を EP の前に書き換える (docs/spec/core-ir.md の「最適化」)。まず関数ごとの作業リストで、
//! 既知の閉包への `apply` を直接の呼び出しにし、既知の閉包を受ける関数と閉包を鍵ごとの複製にする (「既知の閉包」)。
//! 次に、本体が既知の `handle` を handler ごとの複製の呼び出しにする (「handler への特殊化」)。最後に、小さな再帰しない
//! 関数の展開と既知のコンストラクタと `con` の値の形についての複製をまとめた融合の段 (`fuse`、「融合」) を置く。段ごとに
//! `yields` の印を付け直す。最後に、直接だけ呼ばれる、値として生まれた関数を型どおりにし、到達できない関数を落とす。

mod blocks;
mod direct;
mod fuse;
mod handlers;
mod known;
mod reach;
mod specialize;
mod yields;

use crate::Program;

pub(crate) use reach::retain_reachable;
pub(crate) use yields::{kept_marks, recompute_yields};

pub fn optimize(program: &mut Program) {
    let lifted = direct::lifted(program);
    specialize::specialize(program);
    // handler への特殊化は、付け直す前の印も読む (`handlers::path`)
    let mut translated: Vec<bool> = program
        .functions
        .iter()
        .map(|function| function.yields)
        .collect();
    let keep = kept_marks(program);
    recompute_yields(program, &keep);
    let continued = handlers::handlers(program, &mut translated);
    let keep = kept_marks(program);
    recompute_yields(program, &keep);
    fuse::fuse(program);
    let keep = kept_marks(program);
    recompute_yields(program, &keep);
    direct::type_direct_only(program, &lifted, &continued);
    retain_reachable(program);
}

//! 呼び出しのフレームに退避する変数を決めるパス (docs/spec/core-ir.md)。Perceus の後に、各呼び出しの後で使う変数を
//! 記録する。RC の対象でない変数も、`Jump` の先の join point の本体で使う変数も含む。インタプリタはこの変数だけを
//! フレームに退避するので、フレームはちょうど所有している参照だけを持つ。

use crate::liveness::liveness;
use crate::{CExpr, Program, Rhs};

pub(crate) fn record(program: &mut Program) {
    for function in &mut program.functions {
        let all = vec![true; function.vars.len()];
        let live = liveness(function, &all).exprs;
        for expr in &mut function.exprs {
            if let CExpr::Let {
                var,
                rhs: Rhs::Call { saved, .. },
                body,
            } = expr
            {
                let mut after = live[body.0 as usize].clone();
                after.remove(var);
                *saved = after.into_iter().collect();
            }
        }
    }
}

//! EP が使う、末尾の位置の呼び出しの判定と純粋な右辺の判定の Core IR の写し。LIR の box の挿入と縮約も同じ判定の
//! 写しを持ち、どちらも docs/spec/core-ir.md の「変換の規則」にある末尾の位置の呼び出しの1つの定義を実装する
//! (docs/superpowers/specs/2026-10-11-s8a-lir-design.md の「末尾の位置の判定の写し」)。LIR の純粋な右辺は `box` と
//! `unbox` を含むが、EP の出力にはその2つがないので、EP の出力の上では2つの写しが同じ答えを返す。

use eml_extern::Purity;

use crate::{Block, Call, Rhs, Stmt, Term};
use eml_lir::Atom;

/// 末尾の位置の呼び出しと、その文の位置。ブロックの終端が `return x` で、同じブロックに `let x = <呼び出し>` が
/// あり、その後の文がすべて純粋な `let` であるときの、その呼び出しである。後ろの純粋な `let` は `return` が使わない
/// ので縮約が消し、そこで末尾呼び出しになる (docs/spec/core-ir.md の「変換の規則」)。形で決めるので、`never` の操作の
/// 節の呼び出しも含む。EP は、この呼び出しを yield の地点にしない
/// (docs/spec/core-ir.md の「yield の地点」)。
pub(crate) fn tail_position_call(block: &Block) -> Option<(usize, &Call)> {
    let Term::Return(Atom::Var(returned)) = block.term else {
        return None;
    };
    for (index, stmt) in block.stmts.iter().enumerate().rev() {
        let Stmt::Let { var, rhs } = stmt else {
            return None;
        };
        if *var == returned {
            return match rhs {
                Rhs::Call { call, mask: _ } => Some((index, call)),
                _ => None,
            };
        }
        if !pure(rhs) {
            return None;
        }
    }
    None
}

/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。extern は表の行が `Pure` のものだけである。
/// `con` と `closure` が所有権を受け取る値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が
/// 早まるだけである。
pub(crate) fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::ConstString(_) | Rhs::Con { ctor: _, args: _ } | Rhs::MakeClosure(_, _) => true,
        Rhs::Extern {
            ext,
            args: _,
            at: _,
        } => ext.row().purity == Purity::Pure,
        Rhs::Prim(prim, _) => prim.pure(),
        Rhs::Call { call: _, mask: _ } | Rhs::Drop(_) => false,
    }
}

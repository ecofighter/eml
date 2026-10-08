//! 変数の生存 (docs/spec/core-ir.md の「パス」)。ブロックを後ろからたどる1回のループで、ブロックごとの入口の生存集合を
//! 求め、側の表として返す。IR には書かない。辺はすべて番号の大きいブロックへ向かうので、後ろからたどれば、行き先の
//! 入口の集合はいつも求め終わっている。

use std::collections::BTreeSet;

use crate::{Atom, BlockId, CoreFn, Stmt, Term, VarId};

/// ブロックごとの入口の生存集合。`BlockId` の番号で引き、各集合は昇順に並ぶ。入口は、ブロックの引数と case の
/// フィールドを束縛した後の位置なので、使う引数とフィールドも集合に入る。RC の対象に限らず、すべての変数を持つ。
/// Perceus が、呼び出しの後で生きている RC でない変数も `saved` に入れるためである。
pub fn live_in(function: &CoreFn) -> Vec<Vec<VarId>> {
    let mut live_in = vec![Vec::new(); function.blocks.len()];
    for (index, block) in function.blocks.iter().enumerate().rev() {
        let mut live = live_after_term(function, &block.term, &live_in);
        block.term.for_each_atom(|atom| insert_var(&mut live, atom));
        for stmt in block.stmts.iter().rev() {
            step_back(stmt, &mut live);
        }
        live_in[index] = live.into_iter().collect();
    }
    live_in
}

/// 終端の後で生きている変数。終端そのものが使う変数は入らない。`return` と `tail` の後には何も残らない。`jump` は
/// (行き先の入口 − 行き先の引数)、`switch` は各 case の (行き先の入口 − フィールド) と default の入口の和である。
pub(crate) fn live_after_term(
    function: &CoreFn,
    term: &Term,
    live_in: &[Vec<VarId>],
) -> BTreeSet<VarId> {
    let mut live = BTreeSet::new();
    let mut add = |target: BlockId, bound: &[VarId]| {
        live.extend(
            live_in[target.0 as usize]
                .iter()
                .copied()
                .filter(|var| !bound.contains(var)),
        );
    };
    match term {
        Term::Return(_) | Term::TailCall { call: _, mask: _ } => {}
        Term::Jump { target, args: _ } => add(*target, &function.block(*target).params),
        Term::Switch {
            scrutinee: _,
            cases,
            default,
        } => {
            for case in cases {
                add(case.target, &case.fields);
            }
            if let Some(default) = default {
                add(*default, &[]);
            }
        }
    }
    live
}

/// 文の前で生きている変数に更新する。`dup` と `decref` も変数を使うものとして数え、Perceus の後の IR にも使えるようにする。
pub(crate) fn step_back(stmt: &Stmt, live: &mut BTreeSet<VarId>) {
    for var in stmt.defs() {
        live.remove(var);
    }
    match stmt {
        Stmt::Dup(var) | Stmt::Decref(var) => {
            live.insert(*var);
        }
        Stmt::Let { var: _, rhs: _ }
        | Stmt::Unpack {
            value: _,
            tag: _,
            fields: _,
        } => stmt.for_each_atom(|atom| insert_var(live, atom)),
    }
}

pub(crate) fn insert_var(live: &mut BTreeSet<VarId>, atom: Atom) {
    if let Atom::Var(var) = atom {
        live.insert(var);
    }
}

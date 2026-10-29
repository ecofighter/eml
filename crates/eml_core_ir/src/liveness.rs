//! 変数の生存 (docs/spec/lir.md の「生存解析」)。ブロックを後ろからたどる1回のループで、ブロックごとのエントリの生存集合を
//! 求め、側の表として返す。IR には書かない。辺はすべて番号の大きいブロックへ向かうので、後ろからたどれば、行き先の
//! エントリの集合はいつも求め終わっている。

use std::collections::BTreeSet;

use crate::{Block, CoreFn, Stmt, Term};
use eml_lir::{Atom, BlockId, VarId};

/// ブロックごとのエントリの生存集合。`BlockId` の番号で引き、各集合は昇順に並ぶ。エントリは、ブロックの引数と case の
/// フィールドを束縛した後の位置なので、使う引数とフィールドも集合に入る。
pub(crate) fn live_in(function: &CoreFn) -> Vec<Vec<VarId>> {
    let mut live_in = vec![Vec::new(); function.blocks.len()];
    for (index, block) in function.blocks.iter().enumerate().rev() {
        let mut live = live_before_term(function, block, &live_in);
        for stmt in block.stmts.iter().rev() {
            step_back(stmt, &mut live);
        }
        live_in[index] = live.into_iter().collect();
    }
    live_in
}

/// 終端の後で生きている変数。終端そのものが使う変数は入らない。`return` の後には何も残らない。`jump` は
/// (行き先のエントリ − 行き先の引数)、`switch` は各 case の (行き先のエントリ − フィールド) と default のエントリの和である。
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
        Term::Return(_) => {}
        Term::Jump { target, args: _ } => add(*target, &function.block(*target).params),
        Term::Switch {
            scrutinee: _,
            layout: _,
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

/// ブロックの終端の前で生きている変数。終端が使う変数も入る。
pub(crate) fn live_before_term(
    function: &CoreFn,
    block: &Block,
    live_in: &[Vec<VarId>],
) -> BTreeSet<VarId> {
    let mut live = live_after_term(function, &block.term, live_in);
    block.term.for_each_atom(|atom| insert_var(&mut live, atom));
    live
}

/// 合流のブロック `join` のエントリで生きている変数から、`join` の引数と `exclude` を除いたもの。番号の昇順に並ぶ。
pub(crate) fn join_live<'a>(
    function: &'a CoreFn,
    live_in: &'a [Vec<VarId>],
    join: BlockId,
    exclude: Option<VarId>,
) -> impl Iterator<Item = VarId> + 'a {
    let params = &function.block(join).params;
    live_in[join.0 as usize]
        .iter()
        .copied()
        .filter(move |var| !params.contains(var) && Some(*var) != exclude)
}

/// 文の前で生きている変数に更新する。
pub(crate) fn step_back(stmt: &Stmt, live: &mut BTreeSet<VarId>) {
    for var in stmt.defs() {
        live.remove(var);
    }
    stmt.for_each_atom(|atom| insert_var(live, atom));
}

fn insert_var(live: &mut BTreeSet<VarId>, atom: Atom) {
    if let Atom::Var(var) = atom {
        live.insert(var);
    }
}

//! Perceus の `dup` / `decref` の挿入と、呼び出しの `saved` (docs/spec/core-ir.md の「パス」)。変数を使うことを所有権の
//! 移動として扱い、後でも使う変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は RC の対象 (`Repr::is_rc`)
//! の変数だけである。ブロックを前からたどり、その場で書き換える。

use std::collections::{BTreeSet, HashMap};

use crate::{Atom, Block, BlockId, CoreFn, Program, Rhs, Stmt, Term, VarId};

use crate::liveness::{insert_var, live_after_term, live_in, step_back};

/// 変換と contract の後に、プログラム全体に1回だけかける。入力は RC の命令を持たない。
pub fn perceus(program: &mut Program) {
    for function in &mut program.functions {
        insert_rc(function);
    }
}

fn insert_rc(function: &mut CoreFn) {
    let live_in = live_in(function);
    let rc: Vec<bool> = function.vars.iter().map(|var| var.repr.is_rc()).collect();
    // `switch` の行き先には、その `switch` の辺1本だけが入る (R3)。入口の所有は `switch` のブロックで決まるので、
    // 行き先に着くまでここに置く
    let mut switch_entries: HashMap<BlockId, Vec<VarId>> = HashMap::new();
    for index in 0..function.blocks.len() {
        let id = BlockId(index as u32);
        let after_term = live_after_term(function, &function.blocks[index].term, &live_in);
        let block = &mut function.blocks[index];
        // 入口と合流するブロックは、生きている変数と引数を1つずつ所有して始まる。使わない引数は先頭で捨てる
        let owned = switch_entries.remove(&id).unwrap_or_else(|| {
            let mut owned: BTreeSet<VarId> = live_in[index].iter().copied().collect();
            owned.extend(block.params.iter().copied());
            owned.into_iter().filter(|var| rc[var.0 as usize]).collect()
        });
        if let Term::Switch {
            scrutinee: _,
            cases,
            default,
        } = &block.term
        {
            // 行き先は、(`switch` の前の所有 − scrutinee) にフィールドを足したものを所有する。scrutinee を行き先でも
            // 使うなら、`switch` の前で複製した分を所有する。この集合は終端の後で生きている RC の対象と同じである
            let kept: Vec<VarId> = after_term
                .iter()
                .copied()
                .filter(|var| rc[var.0 as usize])
                .collect();
            for case in cases {
                let mut entry = kept.clone();
                entry.extend(case.fields.iter().copied().filter(|var| rc[var.0 as usize]));
                entry.sort_unstable();
                switch_entries.insert(case.target, entry);
            }
            if let Some(default) = default {
                switch_entries.insert(*default, kept);
            }
        }
        rewrite(block, &owned, after_term, &live_in[index], &rc);
    }
}

/// ブロックの文を書き換える。後ろからたどって各文の後で生きている変数を求め、複製と解放と `saved` を決めてから、
/// 前から並べ直す。終端の前に足す文は `stmts` の末尾に置くので、終端を「文と新しい終端」に置き換える書き換え
/// (S3b-2b の `TailCall` の降格) も、ここで終端を差し替えればその場でできる。
fn rewrite(
    block: &mut Block,
    owned: &[VarId],
    mut live: BTreeSet<VarId>,
    live_in: &[VarId],
    rc: &[bool],
) {
    let mut uses = Vec::new();
    block
        .term
        .for_each_atom(|atom| push_rc_var(&mut uses, atom, rc));
    let term_dups = dups(uses, &live);
    block.term.for_each_atom(|atom| insert_var(&mut live, atom));
    let mut plans = Vec::with_capacity(block.stmts.len());
    for stmt in block.stmts.iter_mut().rev() {
        debug_assert!(
            !matches!(stmt, Stmt::Dup(_) | Stmt::Decref(_)),
            "Perceus runs once on code without RC instructions"
        );
        // `live` はこの文の後で生きている変数である。呼び出しは、そのうち結果の変数以外を退避する
        if let Stmt::Let {
            var,
            rhs:
                Rhs::Call {
                    call: _,
                    mask: _,
                    saved,
                },
        } = stmt
        {
            *saved = live.iter().copied().filter(|v| v != var).collect();
        }
        let dead: Vec<VarId> = stmt
            .defs()
            .iter()
            .copied()
            .filter(|var| rc[var.0 as usize] && !live.contains(var))
            .collect();
        let mut uses = Vec::new();
        stmt.for_each_atom(|atom| push_rc_var(&mut uses, atom, rc));
        plans.push((dups(uses, &live), dead));
        step_back(stmt, &mut live);
    }
    debug_assert!(live.iter().eq(live_in.iter()), "the block's live-in set");

    let old = std::mem::take(&mut block.stmts);
    let stmts = &mut block.stmts;
    // 所有していて入口で死んでいる変数 (使わない引数、フィールド、ほかの行き先だけが使う変数) は先頭で捨てる
    stmts.extend(
        owned
            .iter()
            .filter(|var| live_in.binary_search(var).is_err())
            .map(|&var| Stmt::Decref(var)),
    );
    for (stmt, (dups, dead)) in old.into_iter().zip(plans.into_iter().rev()) {
        stmts.extend(dups.into_iter().map(Stmt::Dup));
        stmts.push(stmt);
        stmts.extend(dead.into_iter().map(Stmt::Decref));
    }
    // 終端が渡さない所有は残らない。生きている変数は、最後に使う位置で所有権ごと渡り、死んだ変数は先頭か定義の直後で
    // 捨ててあるためである
    stmts.extend(term_dups.into_iter().map(Stmt::Dup));
}

/// 使うたびに所有権を1つ渡すので、2回目以降の使用と、後でも生きている変数の分を複製する。`uses` は使う順の RC の
/// 対象の変数で、結果は変数の昇順に並ぶ。
fn dups(mut uses: Vec<VarId>, live_after: &BTreeSet<VarId>) -> Vec<VarId> {
    uses.sort_unstable();
    let mut dups = Vec::new();
    for (index, &var) in uses.iter().enumerate() {
        let last = uses.get(index + 1) != Some(&var);
        if !last || live_after.contains(&var) {
            dups.push(var);
        }
    }
    dups
}

fn push_rc_var(uses: &mut Vec<VarId>, atom: Atom, rc: &[bool]) {
    if let Atom::Var(var) = atom
        && rc[var.0 as usize]
    {
        uses.push(var);
    }
}

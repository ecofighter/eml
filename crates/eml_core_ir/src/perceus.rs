//! Perceus の `dup` / `decref` / `release` の挿入と、呼び出しの `saved` (docs/spec/core-ir.md の「パス」)。値を消費する
//! 使いを所有権の移動として扱い、後でも使う変数を複製し、使わなくなった変数をできるだけ早く捨てる。`switch` と
//! `unpack` は値を読むだけで、フィールドは値から借りて始まる。行き先の入口と `unpack` の直後で、生きているフィールドを
//! 所有にする。対象は RC の対象 (`Repr::is_rc`) の変数だけである。ブロックを前からたどり、その場で書き換える。

use std::collections::{BTreeSet, HashMap};

use crate::{Atom, Block, BlockId, CasePattern, CoreFn, Ctor, Program, Rhs, Stmt, Term, VarId};

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
    // `switch` の行き先には、その `switch` の辺1本だけが入る (R3)。入口の所有と、case が分解した値は `switch` の
    // ブロックで決まるので、行き先に着くまでここに置く
    let mut switch_entries: HashMap<BlockId, Vec<VarId>> = HashMap::new();
    let mut case_values: HashMap<BlockId, Destructured> = HashMap::new();
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
        let destructured = case_values.remove(&id);
        if let Term::Switch {
            scrutinee,
            layout,
            cases,
            default,
        } = &block.term
        {
            // `switch` は scrutinee を読むだけなので、どの行き先も `switch` の前の所有をそのまま引き継ぐ。それは終端の
            // 後で生きている RC の対象に scrutinee を足したものである。フィールドは借りて始まるので、所有に入らない
            let value = match *scrutinee {
                Atom::Var(var) if rc[var.0 as usize] => Some(var),
                _ => None,
            };
            let mut kept: BTreeSet<VarId> = after_term
                .iter()
                .copied()
                .filter(|var| rc[var.0 as usize])
                .collect();
            kept.extend(value);
            let kept: Vec<VarId> = kept.into_iter().collect();
            for case in cases {
                switch_entries.insert(case.target, kept.clone());
                let CasePattern::Tag(tag) = case.pattern else {
                    continue;
                };
                // タグの `switch` はいつも配置を持つ (docs/spec/core-ir.md の「データの配置」)。持たない IR を黙って飛ばすと
                // `release` が抜けるので、止める
                let layout = layout.expect("a switch with tag cases has a layout");
                if let Some(value) = value
                    && !case.fields.is_empty()
                {
                    case_values.insert(
                        case.target,
                        Destructured {
                            value,
                            ctor: Ctor { layout, tag },
                            fields: case.fields.clone(),
                        },
                    );
                }
            }
            if let Some(default) = default {
                switch_entries.insert(*default, kept);
            }
        }
        rewrite(
            block,
            &owned,
            destructured.as_ref(),
            after_term,
            &live_in[index],
            &rc,
        );
    }
}

/// ブロックの文を書き換える。後ろからたどって各文の後で生きている変数を求め、複製と解放と `saved` を決めてから、
/// 前から並べ直す。終端の前に足す文は `stmts` の末尾に置くので、終端を「文と新しい終端」に置き換える書き換え
/// (S3b-2c の `TailCall` の降格) も、ここで終端を差し替えればその場でできる。`destructured` は、このブロックが
/// case の行き先なら、その case が分解した値である。
fn rewrite(
    block: &mut Block,
    owned: &[VarId],
    destructured: Option<&Destructured>,
    mut live: BTreeSet<VarId>,
    live_in: &[VarId],
    rc: &[bool],
) {
    let mut uses = Vec::new();
    block
        .term
        .for_each_consumed(|atom| push_rc_var(&mut uses, atom, rc));
    let term_dups = dups(uses, &live);
    block.term.for_each_atom(|atom| insert_var(&mut live, atom));
    let mut plans = Vec::with_capacity(block.stmts.len());
    for stmt in block.stmts.iter_mut().rev() {
        debug_assert!(
            !matches!(stmt, Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. }),
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
        // 文の直後に足す文。`unpack` の後では借りたフィールドを所有にし、ほかの文では定義して使わない変数を捨てる
        let after: Vec<Stmt> = match stmt {
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => match own(*value, *ctor, fields, |var| live.contains(&var), rc) {
                Owning::Dups(vars) => vars.into_iter().map(Stmt::Dup).collect(),
                Owning::Release(release) => vec![release],
                Owning::Decref => vec![Stmt::Decref(*value)],
            },
            _ => stmt
                .defs()
                .iter()
                .copied()
                .filter(|var| rc[var.0 as usize] && !live.contains(var))
                .map(Stmt::Decref)
                .collect(),
        };
        let mut uses = Vec::new();
        stmt.for_each_consumed(|atom| push_rc_var(&mut uses, atom, rc));
        plans.push((dups(uses, &live), after));
        step_back(stmt, &mut live);
    }
    debug_assert!(live.iter().eq(live_in.iter()), "the block's live-in set");

    let old = std::mem::take(&mut block.stmts);
    let stmts = &mut block.stmts;
    let is_live = |var: VarId| live_in.binary_search(&var).is_ok();
    // 入口の順は、フィールドの複製、死んだ所有の `decref` (変数の番号の順)、`release` である。translate は束縛の順に
    // 番号を振るので、親が先に手放され、入れ子の値の参照が1つに戻って `release` が一意の側を通れる
    // (docs/spec/core-ir.md の「Perceus」)
    let mut release = None;
    if let Some(destructured) = destructured {
        match own(
            destructured.value,
            destructured.ctor,
            &destructured.fields,
            is_live,
            rc,
        ) {
            Owning::Dups(vars) => stmts.extend(vars.into_iter().map(Stmt::Dup)),
            Owning::Release(stmt) => release = Some((destructured.value, stmt)),
            Owning::Decref => {}
        }
    }
    let released = release.as_ref().map(|&(value, _)| value);
    // 所有していて入口で死んでいる変数 (使わない引数、ほかの行き先だけが使う変数、フィールドを使わない scrutinee) は
    // 先頭で捨てる
    stmts.extend(
        owned
            .iter()
            .filter(|&&var| !is_live(var) && Some(var) != released)
            .map(|&var| Stmt::Decref(var)),
    );
    stmts.extend(release.map(|(_, stmt)| stmt));
    for (stmt, (dups, after)) in old.into_iter().zip(plans.into_iter().rev()) {
        stmts.extend(dups.into_iter().map(Stmt::Dup));
        stmts.push(stmt);
        stmts.extend(after);
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

/// `case` が分解した値と、その値から借りて始まるフィールド。行き先のブロックを書き換える間も持つので、フィールドを
/// 写して持つ。
struct Destructured {
    value: VarId,
    ctor: Ctor,
    fields: Vec<VarId>,
}

/// 借りたフィールドのうち、生きているものを所有にする方法 (docs/spec/core-ir.md の「Perceus」)。
enum Owning {
    /// 値が生きているので、生きているフィールドを複製する。
    Dups(Vec<VarId>),
    /// 値が死ぬので、生きているフィールドを名前に書いた `release` で値を手放す。
    Release(Stmt),
    /// 値が死に、生きているフィールドもないので、値を `decref` で手放す。
    Decref,
}

fn own(
    value: VarId,
    ctor: Ctor,
    fields: &[VarId],
    is_live: impl Fn(VarId) -> bool,
    rc: &[bool],
) -> Owning {
    let live: Vec<bool> = fields
        .iter()
        .map(|&var| rc[var.0 as usize] && is_live(var))
        .collect();
    if is_live(value) {
        return Owning::Dups(
            fields
                .iter()
                .zip(&live)
                .filter(|&(_, &live)| live)
                .map(|(&var, _)| var)
                .collect(),
        );
    }
    if !live.contains(&true) {
        return Owning::Decref;
    }
    Owning::Release(Stmt::Release {
        value,
        ctor,
        fields: fields
            .iter()
            .zip(&live)
            .map(|(&var, &live)| live.then_some(var))
            .collect(),
    })
}

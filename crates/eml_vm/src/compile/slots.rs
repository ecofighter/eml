//! スロットの割り当て (docs/spec/vm.md の「スロットの割り当て」)。文の単位の生存から干渉グラフを作り、入口の引数を
//! スロット 0.. に固定してから、残りの変数を番号の順に、隣と違ういちばん小さい色で塗る。

use std::collections::BTreeSet;

use eml_lir::liveness::{live_after_term, live_in, step_back};
use eml_lir::{Atom, LirFn, Stmt, Term, VarId};

use crate::code::CompileError;
use crate::op::Slot;

pub(crate) struct Slots {
    /// 変数の番号ごとのスロット。
    pub(crate) of: Vec<Slot>,
    /// 変数が使うスロットの数。一時のスロットはこの後に置く。
    pub(crate) colors: u16,
}

pub(crate) fn allocate(function: &LirFn) -> Result<Slots, CompileError> {
    let graph = interference(function);
    let mut color: Vec<Option<usize>> = vec![None; function.vars.len()];
    for (index, param) in function.params().iter().enumerate() {
        color[param.0 as usize] = Some(index);
    }
    let mut colors = function.params().len();
    for var in 0..function.vars.len() {
        if color[var].is_some() {
            continue;
        }
        let taken: BTreeSet<usize> = graph[var].iter().filter_map(|&n| color[n]).collect();
        let free = (0..)
            .find(|c| !taken.contains(c))
            .expect("a free color exists");
        color[var] = Some(free);
        colors = colors.max(free + 1);
    }
    let too_many = || CompileError::TooManySlots {
        function: function.name.clone(),
    };
    let of = color
        .into_iter()
        .map(|c| Slot::try_from(c.expect("every variable has a color")).map_err(|_| too_many()))
        .collect::<Result<_, _>>()?;
    Ok(Slots {
        of,
        colors: u16::try_from(colors).map_err(|_| too_many())?,
    })
}

/// 変数ごとの隣。
fn interference(function: &LirFn) -> Vec<BTreeSet<usize>> {
    let mut graph = vec![BTreeSet::new(); function.vars.len()];
    let mut edge = |a: VarId, b: VarId| {
        if a != b {
            graph[a.0 as usize].insert(b.0 as usize);
            graph[b.0 as usize].insert(a.0 as usize);
        }
    };
    let live_in = live_in(function);
    for (index, block) in function.blocks.iter().enumerate() {
        // ブロックの引数は入口で同時に書くので、互いと、入口で生きている変数と干渉する
        for &param in &block.params {
            for &other in block.params.iter().chain(&live_in[index]) {
                edge(param, other);
            }
        }
        // case のフィールドは switch で書くので、互いと、行き先の入口で生きている変数と、scrutinee と干渉する。
        // scrutinee のスロットを上書きすると、残りのフィールドを読めなくなるためである
        if let Term::Switch {
            scrutinee, cases, ..
        } = &block.term
        {
            for case in cases {
                let target = &live_in[case.target.0 as usize];
                for &field in &case.fields {
                    for &other in case.fields.iter().chain(target) {
                        edge(field, other);
                    }
                    if let Atom::Var(var) = scrutinee {
                        edge(field, *var);
                    }
                }
            }
        }
        let mut live = live_after_term(function, &block.term, &live_in);
        block.term.for_each_atom(|atom| {
            if let Atom::Var(var) = atom {
                live.insert(var);
            }
        });
        for stmt in block.stmts.iter().rev() {
            // 定義は、その文の後で生きているすべての変数と干渉する。定義した変数が死んでいても同じである
            for def in stmt.defs() {
                for other in stmt.defs().chain(live.iter().copied()) {
                    edge(def, other);
                }
            }
            // unpack の値は、書くフィールドと干渉する (switch の scrutinee と同じ理由)
            if let Stmt::Unpack { value, fields, .. } = stmt {
                for &field in fields {
                    edge(field, *value);
                }
            }
            step_back(stmt, &mut live);
        }
    }
    graph
}

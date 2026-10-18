//! handler の複製の部品。関数を `perform` の地点で切って続きを継続の関数にすることと、節の本体を `perform` の
//! 地点に展開すること (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「書き換え」)。

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::liveness::{insert_var, step_back};
use crate::tail::tail_position_call;
use crate::{Block, Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, CasePattern, VarId, VarInfo};

use super::super::known::fresh;

/// `function` の `block` の `stmt` 番目の文 `let v = perform …` の後ろを、継続の関数に切り出す。返すのは、地点の後で
/// 生きている変数 (v と状態を除き、番号の昇順) と、継続の関数である。継続の関数の引数は、その変数、再開の値 v、
/// 状態つきの handler なら状態 `state` の順に並ぶ。本体は、ブロックの残りを入口にし、そこから届くブロックを元の順の
/// まま番号を振り直して続ける。届かないブロックは落とす。状態の変数も引数になるので、続きは再開で受けた次の状態を読む。
///
/// 生存は切り出す部分の上だけで求め、変数の表は本体に現れる変数だけに元の順のまま詰める。EP の継続の関数と同じく、
/// 関数全体の表を地点ごとに写すと、地点の数と変数の数の積の大きさになるためである (docs/spec/core-ir.md の
/// 「継続の関数」)。
pub(crate) fn cut(
    function: &CoreFn,
    block: BlockId,
    stmt: usize,
    state: Option<VarId>,
) -> (Vec<VarId>, CoreFn) {
    let source = function.block(block);
    let Stmt::Let { var: result, .. } = source.stmts[stmt] else {
        panic!("a cut point is a `let`");
    };
    let reached = reachable(&function.blocks, &source.term);
    let mut live_in: HashMap<BlockId, BTreeSet<VarId>> = HashMap::new();
    for &index in reached.iter().rev() {
        let block = function.block(index);
        let mut live = live_after(function, &block.term, &live_in);
        block.term.for_each_atom(|atom| insert_var(&mut live, atom));
        for stmt in block.stmts.iter().rev() {
            step_back(stmt, &mut live);
        }
        live_in.insert(index, live);
    }
    let mut live = live_after(function, &source.term, &live_in);
    source
        .term
        .for_each_atom(|atom| insert_var(&mut live, atom));
    for later in source.stmts[stmt + 1..].iter().rev() {
        step_back(later, &mut live);
    }
    live.remove(&result);
    if let Some(state) = state {
        live.remove(&state);
    }
    let live: Vec<VarId> = live.into_iter().collect();

    let mut params = live.clone();
    params.push(result);
    params.extend(state);
    let mut blocks = vec![Block {
        params,
        stmts: source.stmts[stmt + 1..].to_vec(),
        term: source.term.clone(),
    }];
    let renumber: HashMap<BlockId, BlockId> = reached
        .iter()
        .enumerate()
        .map(|(position, &index)| (index, BlockId(position as u32 + 1)))
        .collect();
    blocks.extend(reached.iter().map(|&index| function.block(index).clone()));
    for block in &mut blocks {
        block
            .term
            .for_each_successor_mut(|target| *target = renumber[target]);
    }
    let mut used = Vec::new();
    for block in &mut blocks {
        block.for_each_var_mut(|var| used.push(*var));
    }
    used.sort_unstable();
    used.dedup();
    for block in &mut blocks {
        block.for_each_var_mut(|var| {
            *var = VarId(used.binary_search(var).expect("a used variable") as u32);
        });
    }
    let cut = CoreFn {
        name: function.name.clone(),
        internal: true,
        yields: function.yields,
        clause: None,
        vars: used
            .iter()
            .map(|var| function.vars[var.0 as usize].clone())
            .collect(),
        ret: function.ret,
        ret_mark: function.ret_mark,
        blocks,
    };
    (live, cut)
}

/// 終端の後で生きている変数 (`crate::liveness::live_after_term` の、入口の生存集合を表で持つ形)。
fn live_after(
    function: &CoreFn,
    term: &Term,
    live_in: &HashMap<BlockId, BTreeSet<VarId>>,
) -> BTreeSet<VarId> {
    let mut live = BTreeSet::new();
    let mut add = |target: BlockId, bound: &[VarId]| {
        live.extend(
            live_in[&target]
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

/// 入口から届かないブロックを落とし、残ったブロックに元の順のまま番号を振り直す。`inline_clause` が切り離した
/// ブロックは辺が入らなくなるので、残すと構造の規則に反する (docs/spec/lir.md の「構造の規則」)。
pub(crate) fn prune(function: &mut CoreFn) {
    let mut kept = vec![BlockId::ENTRY];
    kept.extend(reachable(&function.blocks, &function.blocks[0].term));
    let renumber: HashMap<BlockId, BlockId> = kept
        .iter()
        .enumerate()
        .map(|(position, &index)| (index, BlockId(position as u32)))
        .collect();
    let mut blocks: Vec<Block> = kept
        .iter()
        .map(|&index| {
            std::mem::replace(
                &mut function.blocks[index.0 as usize],
                Block {
                    params: Vec::new(),
                    stmts: Vec::new(),
                    term: Term::Return(Atom::Unit),
                },
            )
        })
        .collect();
    for block in &mut blocks {
        block
            .term
            .for_each_successor_mut(|target| *target = renumber[target]);
    }
    function.blocks = blocks;
}

/// `term` の行き先から届くブロックを、番号の昇順に返す。届く部分だけをたどるので、関数の残りの大きさに依らない。
fn reachable(blocks: &[Block], term: &Term) -> Vec<BlockId> {
    let mut seen = HashSet::new();
    let mut pending: Vec<BlockId> = term.successors().collect();
    while let Some(block) = pending.pop() {
        if seen.insert(block) {
            pending.extend(blocks[block.0 as usize].term.successors());
        }
    }
    let mut reached: Vec<BlockId> = seen.into_iter().collect();
    reached.sort_unstable_by_key(|block| block.0);
    reached
}

/// `target` の `at` の文 (`perform`) の場所に、すぐに再開する節の関数 `clause` の本体を写して展開する。ブロックの
/// `perform` から後ろは捨て、節の入口のブロックの文を続ける。節のほかのブロックは後ろに足す。節の変数は新しい変数に
/// し、引数は置き換える。`args` は捕獲と操作の引数で、状態の引数は `state` に、末尾の `resume k(v, s)` は
/// `resume(v, s)` の右辺にする。`resume` の結果は `target` が返す値なので、その変数の Repr と印は `target` の戻り値に
/// そろえる。`resume` から `return` までの純粋な `let` は使われないので写さない。
///
/// 写した文は、どれも H 宛てとして書き換えない。節の中の `perform` は、もとの意味でも handler の外側で動くためである
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「意味が変わらない理由」)。
pub(crate) fn inline_clause(
    target: &mut CoreFn,
    at: (BlockId, usize),
    clause: &CoreFn,
    args: &[Atom],
    state: Atom,
    resume: impl Fn(Atom, Atom) -> Rhs,
) {
    let params = clause.params();
    let count = params.len();
    let k = params[count - 2];
    let mut substitution: HashMap<VarId, Atom> = params[..count - 2]
        .iter()
        .copied()
        .zip(args.iter().copied())
        .collect();
    substitution.insert(params[count - 1], state);
    let mut copy = Copy {
        vars: &mut target.vars,
        source: &clause.vars,
        substitution,
        k,
    };

    let base = target.blocks.len() as u32;
    let block_of = |block: BlockId| match block.0 {
        0 => at.0,
        index => BlockId(base + index - 1),
    };
    let returned = VarInfo {
        name: "t".to_string(),
        repr: target.ret,
        mark: target.ret_mark,
    };
    let mut copied = Vec::with_capacity(clause.blocks.len());
    for (index, block) in clause.blocks.iter().enumerate() {
        let params = if index == 0 {
            Vec::new()
        } else {
            block
                .params
                .iter()
                .map(|&param| copy.define(param))
                .collect()
        };
        let resumed = match block.term {
            Term::Return(_) => tail_position_call(block).map(|(at, _)| at),
            _ => None,
        };
        let mut stmts = Vec::with_capacity(block.stmts.len());
        let mut term = None;
        for (position, stmt) in block.stmts.iter().enumerate() {
            if resumed == Some(position) {
                let Stmt::Let {
                    var: _,
                    rhs:
                        Rhs::Call {
                            call: Call::Resume { k: _, arg, state },
                            mask: _,
                        },
                } = stmt
                else {
                    panic!("an immediately resuming clause ends in `resume`");
                };
                let var = fresh(copy.vars, returned.clone());
                let rhs = resume(copy.atom(*arg), copy.atom(*state));
                stmts.push(Stmt::Let { var, rhs });
                term = Some(Term::Return(Atom::Var(var)));
                break;
            }
            stmts.push(copy.stmt(stmt));
        }
        let term = term.unwrap_or_else(|| copy.term(&block.term, block_of));
        copied.push(Block {
            params,
            stmts,
            term,
        });
    }
    let mut copied = copied.into_iter();
    let entry = copied.next().expect("a function has an entry block");
    let host = &mut target.blocks[at.0.0 as usize];
    host.stmts.truncate(at.1);
    host.stmts.extend(entry.stmts);
    host.term = entry.term;
    target.blocks.extend(copied);
}

/// 節の本体を写すときの変数の対応。
struct Copy<'a> {
    vars: &'a mut Vec<VarInfo>,
    source: &'a [VarInfo],
    substitution: HashMap<VarId, Atom>,
    k: VarId,
}

impl Copy<'_> {
    fn define(&mut self, var: VarId) -> VarId {
        let copy = fresh(self.vars, self.source[var.0 as usize].clone());
        self.substitution.insert(var, Atom::Var(copy));
        copy
    }

    fn atom(&self, atom: Atom) -> Atom {
        match atom {
            Atom::Var(var) => {
                debug_assert_ne!(var, self.k, "`k` is used only by the final `resume`");
                self.substitution[&var]
            }
            atom => atom,
        }
    }

    fn var(&self, var: VarId) -> VarId {
        match self.atom(Atom::Var(var)) {
            Atom::Var(var) => var,
            atom => panic!("a variable is replaced by the constant {atom:?}"),
        }
    }

    fn stmt(&mut self, stmt: &Stmt) -> Stmt {
        match stmt {
            Stmt::Let { var, rhs } => {
                let mut rhs = rhs.clone();
                rhs.for_each_atom_mut(|atom| *atom = self.atom(*atom));
                Stmt::Let {
                    var: self.define(*var),
                    rhs,
                }
            }
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => Stmt::Unpack {
                value: self.var(*value),
                ctor: *ctor,
                fields: fields.iter().map(|&field| self.define(field)).collect(),
            },
        }
    }

    fn term(&mut self, term: &Term, block_of: impl Fn(BlockId) -> BlockId) -> Term {
        let mut term = term.clone();
        term.for_each_atom_mut(|atom| *atom = self.atom(*atom));
        term.for_each_successor_mut(|target| *target = block_of(*target));
        if let Some(target) = constant_target(&term) {
            return Term::Jump {
                target,
                args: Vec::new(),
            };
        }
        if let Term::Switch { cases, .. } = &mut term {
            for case in cases {
                case.fields = case
                    .fields
                    .iter()
                    .map(|&field| self.define(field))
                    .collect();
            }
        }
        term
    }
}

/// 引数を置き換えて定数になった `switch` の行き先。操作の引数が定数 (`True`、引数のないコンストラクタ、整数) なら、
/// 節の本体の `switch` の値も定数になる。定数で分ける `switch` は verifier が断るので (docs/spec/lir.md の
/// 「verifier」)、合う case か `default` へのジャンプにする。定数はフィールドを持たないので、合う case はフィールドを
/// 束縛しない。`switch` の行き先は引数を持たないので、ジャンプは値を渡さない。ほかの case のブロックには辺が入らなく
/// なり、複製の最後の `prune` が落とす。
fn constant_target(term: &Term) -> Option<BlockId> {
    let Term::Switch {
        scrutinee,
        layout: _,
        cases,
        default,
    } = term
    else {
        return None;
    };
    let pattern = match *scrutinee {
        Atom::Tag(tag) => CasePattern::Tag(tag),
        Atom::Int(value) => CasePattern::Int(value),
        Atom::Var(_) | Atom::Unit | Atom::Fn(_) => return None,
    };
    let matched = cases.iter().find(|case| case.pattern == pattern);
    debug_assert!(
        matched.is_none_or(|case| case.fields.is_empty()),
        "a constant has no fields"
    );
    Some(
        matched
            .map(|case| case.target)
            .or(*default)
            .expect("a switch on a constant has a matching case or a default"),
    )
}

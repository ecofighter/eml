//! handler の複製の部品 (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「書き換え」)。
//! `perform` の地点からの続きを継続の関数にし、合流のブロックからの続きを合流のブロックの関数にする (`region`)。
//! すぐに再開する節の本体を `perform` の地点に展開し (`inline_clause`)、引数を置き換えて定数になった `switch` を
//! 合う行き先へのジャンプにする (`blocks::constant_target`)。`k` を値として使う節は、`k` に継続の関数の閉包を受ける写し
//! `<g>{k}` にする (`clause_with_k`)。展開で辺の入らなくなったブロックは、最後に落とす (`blocks::prune`)。

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::tail::tail_position_call;
use crate::{Block, Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, FnIdx, Repr, VarId, VarInfo};

use super::super::blocks::constant_target;
use super::super::known::fresh;

/// ブロックごとの、合流のブロックか。合流のブロックは、入る辺が2本以上か、引数を持つブロックである。引数を持つ
/// ブロックには、引数を渡す `jump` で入る (docs/spec/lir.md の「構造の規則」の R3 と R4)。`switch` の行き先は入る辺が
/// 1本で引数を持たないので、合流のブロックにならない。
pub(crate) fn joins(blocks: &[Block]) -> Vec<bool> {
    let mut edges = vec![0usize; blocks.len()];
    for block in blocks {
        for target in block.term.successors() {
            edges[target.0 as usize] += 1;
        }
    }
    blocks
        .iter()
        .zip(edges)
        .map(|(block, edges)| edges > 1 || !block.params.is_empty())
        .collect()
}

/// `function` の `block` の `start` 番目の文からの続きを、1つの関数の本体にする。入口は、引数を `params` にした
/// ブロックの残りで、そこから合流のブロックを通らずに届くブロックを元の順のまま番号を振り直して続ける。合流の
/// ブロック J への `jump` は、`exit(J, 実引数)` が返す呼び出しの末尾呼び出しにして、J には進まない。届かない
/// ブロックは落とす。継続の関数 `f{H}$hN` と合流のブロックの関数 `f{H}$hjJ` は、どちらもこの形で作る。作る関数は
/// 内部の関数でない。`k` として渡す継続の関数だけを、呼ぶ側が内部の関数にする (docs/spec/lir.md の「一様な関数」)。
///
/// 合流のブロックを写さないのは、写すと、合流のブロックとその先が、そこへ届くどの継続の関数にも入るためである。
/// 条件つきの `perform` が並ぶと、コードが `perform` の数の2乗で育つ。合流のブロックでない行き先は、入る辺が
/// 1本なので、どの続きにも多くとも1回しか入らない。EP の継続の関数が合流のブロックで `f$jJ` を呼ぶのと同じ形で
/// ある (docs/spec/core-ir.md の「継続の関数」)。変数の表は本体に現れる変数だけに元の順のまま詰める。関数全体の表を
/// 地点ごとに写すと、地点の数と変数の数の積の大きさになるためである。
pub(crate) fn region(
    function: &CoreFn,
    (block, start): (BlockId, usize),
    params: Vec<VarId>,
    joins: &[bool],
    mut exit: impl FnMut(BlockId, &[Atom]) -> Call,
) -> CoreFn {
    let source = function.block(block);
    let follow = |term: &Term, pending: &mut Vec<BlockId>| {
        pending.extend(term.successors().filter(|target| !joins[target.0 as usize]));
    };
    let mut seen = BTreeSet::new();
    let mut pending = Vec::new();
    follow(&source.term, &mut pending);
    while let Some(target) = pending.pop() {
        if seen.insert(target) {
            follow(&function.block(target).term, &mut pending);
        }
    }
    let renumber: HashMap<BlockId, BlockId> = seen
        .iter()
        .enumerate()
        .map(|(position, &index)| (index, BlockId(position as u32 + 1)))
        .collect();
    let mut blocks = vec![Block {
        params,
        stmts: source.stmts[start..].to_vec(),
        term: source.term.clone(),
    }];
    blocks.extend(seen.iter().map(|&index| function.block(index).clone()));
    let mut added = Vec::new();
    for block in &mut blocks {
        match &block.term {
            Term::Jump { target, args } if joins[target.0 as usize] => {
                let call = exit(*target, args);
                let var = VarId((function.vars.len() + added.len()) as u32);
                added.push(VarInfo {
                    name: "t".to_string(),
                    repr: function.ret,
                    mark: function.ret_mark,
                });
                block.stmts.push(Stmt::Let {
                    var,
                    rhs: Rhs::Call {
                        call,
                        mask: Vec::new(),
                    },
                });
                block.term = Term::Return(Atom::Var(var));
            }
            _ => block
                .term
                .for_each_successor_mut(|target| *target = renumber[target]),
        }
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
    let info = |var: &VarId| {
        let index = var.0 as usize;
        function
            .vars
            .get(index)
            .unwrap_or_else(|| &added[index - function.vars.len()])
            .clone()
    };
    CoreFn {
        name: function.name.clone(),
        internal: false,
        yields: function.yields,
        clause: None,
        vars: used.iter().map(info).collect(),
        ret: function.ret,
        ret_mark: function.ret_mark,
        blocks,
    }
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
    // 捕獲の引数は複製の引数そのもので、展開した本体と、継続の関数に渡す生きている変数の両方に現れうる。2か所で
    // 使ってよいのは、操作の節の捕獲が `Unr` に限られるためである (E3001、docs/spec/linearity.md)。呼ぶ側が確かめる
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

    /// `unpack` の値は `obj` で、`obj` に収まる定数はないので (`Atom::fits`)、`performs_fit` を通った引数は変数である。
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
        // 飛ばした case のブロックも写すので、そのフィールドも先に定義する。ブロックは辺が入らなくなり、`prune` が落とす
        if let Term::Switch { cases, .. } = &mut term {
            for case in cases {
                case.fields = case
                    .fields
                    .iter()
                    .map(|&field| self.define(field))
                    .collect();
            }
        }
        match constant_target(&term) {
            Some(target) => Term::Jump {
                target,
                args: Vec::new(),
            },
            None => term,
        }
    }
}

/// `k` を値として受ける節の写し `<g>{k}`。`k` には、生の継続でなく、継続の関数の閉包が来る
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「書き換え」)。`k` の引数は位置をそのままに、
/// Repr を `tobj` にして印を保つ。`k` を包む `closure cont$(k)` と `closure cont$state(k)` (`resumes` が見分ける) は
/// 消して、使う所を `k` そのものにする。継続の関数の閉包は包む関数と同じ引数を受けるので、包む必要がないためである。
/// `resume k(v, s)` は、状態のない handler では `apply k(v)` に、状態つきでは `apply k(v, s)` にする。`mask` は
/// そのまま残す。EP が `resume` から作る `apply` と同じく、印は節の関数の `yields` の印にする (docs/spec/core-ir.md の
/// 「`apply` の印と分け方」)。`k` の出現がこの3つの形だけであることは、呼ぶ側が確かめてある。写しは handler の
/// 複製から直接呼ばれるために生まれるので、内部の関数でない (docs/spec/lir.md の「一様な関数」)。
pub(crate) fn clause_with_k(
    clause: &CoreFn,
    name: String,
    stateful: bool,
    resumes: impl Fn(FnIdx) -> bool,
) -> CoreFn {
    let mut copy = clause.clone();
    copy.name = name;
    copy.internal = false;
    let k = copy.params()[copy.params().len() - 2];
    copy.vars[k.0 as usize].repr = Repr::TObj;
    let mut wrapped = HashSet::new();
    for block in &mut copy.blocks {
        block.stmts.retain(|stmt| match stmt {
            Stmt::Let {
                var,
                rhs: Rhs::MakeClosure(function, captures),
            } if resumes(*function) && captures[..] == [Atom::Var(k)] => {
                wrapped.insert(*var);
                false
            }
            _ => true,
        });
    }
    for block in &mut copy.blocks {
        block.for_each_var_mut(|var| {
            if wrapped.contains(var) {
                *var = k;
            }
        });
        for stmt in &mut block.stmts {
            if let Stmt::Let {
                var: _,
                rhs: Rhs::Call { call, mask: _ },
            } = stmt
                && let Call::Resume {
                    k: callee,
                    arg,
                    state,
                } = *call
            {
                debug_assert_eq!(callee, Atom::Var(k), "a clause resumes only its own `k`");
                let args = if stateful {
                    vec![arg, state]
                } else {
                    vec![arg]
                };
                *call = Call::Apply {
                    callee,
                    args,
                    yields: clause.yields,
                    prompt: None,
                };
            }
        }
    }
    copy
}

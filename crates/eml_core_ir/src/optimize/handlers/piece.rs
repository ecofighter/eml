//! handler の複製の部品 (docs/spec/core-ir.md の「handler の複製」)。
//! `perform` の地点からの続きを継続の関数にし、合流のブロックからの続きを合流のブロックの関数にする (`region`)。
//! すぐに再開する節の本体を写して (`clause_blocks`) `perform` の地点に展開し (`inline_clause`)、引数を置き換えて
//! 定数になった `switch` を合う行き先へのジャンプにする (`blocks::constant_target`)。`k` を値として使う節は、`k` に継続の関数の閉包を受ける写し
//! `<g>{k}` にし (`clause_with_k`)、それを `perform` の地点の継続について複製する (`clause_with_continuation`)。展開で辺の入らなくなったブロックは、最後に落とす (`blocks::prune`)。

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::tail::tail_position_call;
use crate::{Block, Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, FnIdx, Mark, Repr, VarId, VarInfo, push_var};

use super::super::blocks::{Renamer, constant_target};
use super::super::known::{Callee, KnownClosure, apply_rewritable_to};

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
/// 内部の関数でなく、`split` を持たない。特殊化した経路の中で型どおりに呼ばれるためである (docs/spec/lir.md の
/// 「関数の ABI」)。`k` として渡す継続の関数だけは、呼ぶ側が内部の関数にして、生きている変数の数の `split` を付ける
/// (docs/spec/core-ir.md の「handler の複製」)。値の参照が残らなければ、`optimize` の最後にその `split` を外す
/// (docs/spec/core-ir.md の「直接だけ呼ばれる持ち上げた関数」)。
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
                    rhs: Rhs::call(call),
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
        split: None,
        yields: function.yields,
        clause: None,
        vars: used.iter().map(info).collect(),
        ret: function.ret,
        ret_mark: function.ret_mark,
        blocks,
    }
}

/// `target` の `at` の文 (`perform`) の場所に、すぐに再開する節の関数 `clause` の本体を写して展開する。ブロックの
/// `perform` から後ろは捨て、節の入口のブロックの文を続ける。節のほかのブロックは後ろに足す。`args` は捕獲と操作の
/// 引数で、状態の引数は `state` に、末尾の `resume k(v, s)` は `resume(v, s)` の右辺にする (`clause_blocks`)。
/// `resume` の結果は `target` が返す値なので、その変数の Repr と印は `target` の戻り値にそろえる。
pub(crate) fn inline_clause(
    target: &mut CoreFn,
    at: (BlockId, usize),
    clause: &CoreFn,
    args: &[Atom],
    state: Atom,
    resume: impl Fn(Atom, Atom) -> Rhs,
) {
    let returned = VarInfo {
        name: "t".to_string(),
        repr: target.ret,
        mark: target.ret_mark,
    };
    let base = target.blocks.len() as u32;
    let copied = clause_blocks(
        &mut target.vars,
        clause,
        args,
        state,
        base,
        |vars, value, state| {
            let var = push_var(vars, returned.clone());
            (
                vec![Stmt::Let {
                    var,
                    rhs: resume(value, state),
                }],
                Term::Return(Atom::Var(var)),
            )
        },
    );
    let mut copied = copied.into_iter();
    let entry = copied.next().expect("a function has an entry block");
    let host = &mut target.blocks[at.0.0 as usize];
    host.stmts.truncate(at.1);
    host.stmts.extend(entry.stmts);
    host.term = entry.term;
    target.blocks.extend(copied);
}

/// すぐに再開する節の関数 `clause` の本体のブロックを写す。節の変数は `vars` に足す新しい変数にし、引数は置き換える。
/// `args` は捕獲と操作の引数で、状態の引数は `state` にする。返すブロックの 0 番目は入口で、その文と終端は呼ぶ側が
/// 地点のブロックに続ける。節の i 番目 (i > 0) のブロックは `BlockId(base + i - 1)` に置く前提で行き先を振る。
/// 末尾の `resume k(v, s)` は、`resume(vars, v, s)` が返す文と終端にする。`resume` から `return` までの純粋な
/// `let` は使われないので写さない。
///
/// 写した文は、どれも H 宛てとして書き換えない。節の中の `perform` は、もとの意味でも handler の外側で動くためである
/// (docs/spec/core-ir.md の「handler の特殊化で意味が変わらない理由」)。
pub(crate) fn clause_blocks(
    vars: &mut Vec<VarInfo>,
    clause: &CoreFn,
    args: &[Atom],
    state: Atom,
    base: u32,
    mut resume: impl FnMut(&mut Vec<VarInfo>, Atom, Atom) -> (Vec<Stmt>, Term),
) -> Vec<Block> {
    let params = clause.params();
    let count = params.len();
    // 捕獲の引数は複製の引数そのもので、展開した本体と、続きに渡す値の両方に現れうる。2か所で使ってよいのは、操作の
    // 節の捕獲が `Unr` に限られるためである (E3001、docs/spec/linearity.md)。呼ぶ側が確かめる。`k` (`count - 2` 番目)
    // は置き換えないので、末尾の `resume` のほかで使えば `Renamer::atom` が止まる
    let mut copy = Renamer::new(vars, &clause.vars);
    for (&param, &arg) in params[..count - 2].iter().zip(args) {
        copy.substitute(param, arg);
    }
    copy.substitute(params[count - 1], state);
    // 入口のブロックには辺が入らないので、行き先になるのは 1 番目から後ろのブロックだけである
    let block_of = |block: BlockId| {
        debug_assert_ne!(block, BlockId::ENTRY, "no edge enters the entry block");
        BlockId(base + block.0 - 1)
    };
    let mut copied = Vec::with_capacity(clause.blocks.len());
    for (index, block) in clause.blocks.iter().enumerate() {
        let params = copy.params(index, block);
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
                            call:
                                Call::Resume {
                                    k: _, arg, state, ..
                                },
                            mask: _,
                        },
                } = stmt
                else {
                    panic!("an immediately resuming clause ends in `resume`");
                };
                let (value, state) = (copy.atom(*arg), copy.atom(*state));
                let (tail, end) = resume(&mut *copy.vars, value, state);
                stmts.extend(tail);
                term = Some(end);
                break;
            }
            stmts.push(copy.stmt(stmt));
        }
        let term = term.unwrap_or_else(|| {
            let term = copy.term(&block.term, block_of);
            // 飛ばした case のブロックは辺が入らなくなり、`prune` が落とす
            match constant_target(&term) {
                Some(target) => Term::Jump {
                    target,
                    args: Vec::new(),
                },
                None => term,
            }
        });
        copied.push(Block {
            params,
            stmts,
            term,
        });
    }
    copied
}

/// `k` を値として受ける節の写し `<g>{k}`。`k` には、生の継続でなく、継続の関数の閉包が来る
/// (docs/spec/core-ir.md の「handler の複製」)。`k` の引数は位置をそのままに、
/// Repr を `tobj` にして印を保つ。`k` を包む `closure $cont$(k)` と `closure $cont$state(k)` (`resumes` が見分ける) は
/// 消して、使う所を `k` そのものにする。継続の関数の閉包は包む関数と同じ引数を受けるので、包む必要がないためである。
/// `resume k(v, s)` は、状態のない handler では `apply k(v)` に、状態つきでは `apply k(v, s)` にする。`mask` は
/// そのまま残す。EP が `resume` から作る `apply` と同じく、印は節の関数の `yields` の印にする (docs/spec/core-ir.md の
/// 「`apply` の印と分け方」)。`k` の出現がこの3つの形だけであることは、呼ぶ側が確かめてある。写しは handler の
/// 複製から直接呼ばれるために生まれるので、内部の関数でなく、`split` も持たない。`k` への `apply` は EP の再開と同じ形
/// で、ほかの `apply` と同じく一様である (docs/spec/lir.md の「関数の ABI」)。
pub(crate) fn clause_with_k(
    clause: &CoreFn,
    name: String,
    stateful: bool,
    resumes: impl Fn(FnIdx) -> bool,
) -> CoreFn {
    let mut copy = clause.clone();
    copy.name = name;
    copy.internal = false;
    copy.split = None;
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

/// 節の写し `<g>{k}` の `k` の位置を、そこへ渡す継続の関数の閉包 `closure continuation(捕獲…)` について開いた複製
/// (docs/spec/core-ir.md の「節の写しの、継続についての複製」)。`k` の引数は捕獲ごとの新しい引数になる。その Repr
/// と印は継続の関数の捕獲の位置の引数から写し、印は `k` の印を超えない。既知の閉包の複製 (`Specializer::copy`) と
/// 同じ作り方である。`count` は捕獲の数である。
///
/// `k` への `apply` は、既知の閉包への `apply` と同じ規則 (`apply_rewritable_to`) で書き換えられるとき、
/// `call continuation(捕獲…, 引数…)` にする。`mask` はそのまま残す。`drop k` は、`unr` でない捕獲の `drop` にする。
/// 閉包を捨てると捕獲を手放すのと同じで、`unr` の捕獲は Perceus が手放す。それ以外の使う所では、その場で閉包を
/// 作り直す。`unr` でない `k` は経路ごとに1回しか使われないので、捕獲もそれぞれの経路で1回だけ使われる。
pub(crate) fn clause_with_continuation(
    with_k: &CoreFn,
    name: String,
    continuation: FnIdx,
    callee: &Callee,
    count: usize,
) -> CoreFn {
    let mut copy = with_k.clone();
    copy.name = name;
    let params = copy.params().to_vec();
    let at = params.len() - 2;
    let k = params[at];
    let info = copy.vars[k.0 as usize].clone();
    let captures: Vec<VarId> = callee.params[..count]
        .iter()
        .map(|param| {
            let mark = if param.mark.at_least(info.mark) {
                info.mark
            } else {
                param.mark
            };
            push_var(
                &mut copy.vars,
                VarInfo {
                    name: param.name.clone(),
                    repr: param.repr,
                    mark,
                },
            )
        })
        .collect();
    copy.blocks[0]
        .params
        .splice(at..=at, captures.iter().copied());
    let closure = KnownClosure {
        function: continuation,
        captures: captures.iter().copied().map(Atom::Var).collect(),
    };
    let released: Vec<VarId> = captures
        .iter()
        .copied()
        .filter(|&capture| copy.mark(capture) != Mark::Unr)
        .collect();
    let rebuild = |vars: &mut Vec<VarInfo>, out: &mut Vec<Stmt>| {
        if captures.is_empty() {
            return Atom::Fn(continuation);
        }
        let var = push_var(vars, info.clone());
        out.push(Stmt::Let {
            var,
            rhs: Rhs::MakeClosure(continuation, closure.captures.clone()),
        });
        Atom::Var(var)
    };
    // 捕獲がどれも `unr` の `drop k` は文ごと消え、その結果の `()` の変数は `()` に置き換える
    let mut dropped = HashSet::new();
    for block in 0..copy.blocks.len() {
        let stmts = std::mem::take(&mut copy.blocks[block].stmts);
        let mut out = Vec::with_capacity(stmts.len());
        for mut stmt in stmts {
            match &mut stmt {
                Stmt::Let {
                    var,
                    rhs:
                        Rhs::Call {
                            call:
                                Call::Apply {
                                    callee: Atom::Var(target),
                                    args,
                                    ..
                                },
                            mask,
                        },
                } if *target == k
                    && apply_rewritable_to(&copy.vars, &closure, callee, *var, args, mask) =>
                {
                    let mut given = closure.captures.clone();
                    given.extend(args.iter().copied());
                    out.push(Stmt::Let {
                        var: *var,
                        rhs: Rhs::Call {
                            call: Call::Direct(continuation, given),
                            mask: std::mem::take(mask),
                        },
                    });
                    continue;
                }
                Stmt::Let {
                    var,
                    rhs: Rhs::Drop(Atom::Var(target)),
                } if *target == k => {
                    let mut vars = released.iter().copied();
                    match vars.next() {
                        Some(first) => out.push(Stmt::Let {
                            var: *var,
                            rhs: Rhs::Drop(Atom::Var(first)),
                        }),
                        None => {
                            dropped.insert(*var);
                        }
                    }
                    for capture in vars {
                        let unit = push_var(
                            &mut copy.vars,
                            VarInfo {
                                name: "u".to_string(),
                                repr: Repr::Unit,
                                mark: Mark::Unr,
                            },
                        );
                        out.push(Stmt::Let {
                            var: unit,
                            rhs: Rhs::Drop(Atom::Var(capture)),
                        });
                    }
                    continue;
                }
                _ => {}
            }
            let mut uses = false;
            stmt.for_each_atom(|atom| uses |= atom == Atom::Var(k));
            if uses {
                let value = rebuild(&mut copy.vars, &mut out);
                if let Stmt::Let { var: _, rhs } = &mut stmt {
                    rhs.for_each_atom_mut(|atom| {
                        if *atom == Atom::Var(k) {
                            *atom = value;
                        }
                    });
                }
            }
            out.push(stmt);
        }
        let mut uses = false;
        copy.blocks[block]
            .term
            .for_each_atom(|atom| uses |= atom == Atom::Var(k));
        if uses {
            let value = rebuild(&mut copy.vars, &mut out);
            copy.blocks[block].term.for_each_atom_mut(|atom| {
                if *atom == Atom::Var(k) {
                    *atom = value;
                }
            });
        }
        copy.blocks[block].stmts = out;
    }
    if !dropped.is_empty() {
        for block in &mut copy.blocks {
            for stmt in &mut block.stmts {
                if let Stmt::Let { var: _, rhs } = stmt {
                    rhs.for_each_atom_mut(|atom| {
                        if matches!(atom, Atom::Var(var) if dropped.contains(var)) {
                            *atom = Atom::Unit;
                        }
                    });
                }
            }
            block.term.for_each_atom_mut(|atom| {
                if matches!(atom, Atom::Var(var) if dropped.contains(var)) {
                    *atom = Atom::Unit;
                }
            });
        }
    }
    copy
}

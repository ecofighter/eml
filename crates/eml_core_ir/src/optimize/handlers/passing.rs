//! 状態を渡す形の handler の複製 (docs/spec/core-ir.md の「handler の複製」)。どの節もすぐに再開する handler の
//! 経路の関数 f を、状態を変数で持ち回る1つの関数 `f{H}` に写す。H 宛ての `perform` には節の本体を展開し、その
//! `resume` を `perform` の後ろのブロックへのジャンプにする。経路の関数の呼び出しは、末尾かどうかによらず複製の
//! 呼び出しにし、次の状態を結果で受ける。継続の関数と合流のブロックの関数は作らない。

use std::collections::HashSet;

use super::super::blocks;
use super::super::known::fresh;
use super::{Copies, KnownFn, Shape, piece, targeted};
use crate::tail::tail_position_call;
use crate::{Block, Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::{
    Atom, BlockId, Ctor, FnIdx, Layout, LayoutCtor, LayoutId, Mark, Repr, TUPLE, VarId, VarInfo,
};

/// 複製 `f{H}` の結果の形。f と handler だけで決まるので、名前が同じ複製はいつも同じ形の結果を返す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Returned {
    /// 状態のない handler: f の結果をそのまま返す。
    Plain,
    /// 状態つきの handler で、f の結果が `unit` のとき: 次の状態だけを返す。`Unit` を返す関数が、呼ばれるたびに組を
    /// 確保しないためである。
    StateOnly,
    /// それ以外: f の結果と次の状態の組 `(,)` を返す。
    Pair,
}

pub(super) fn returned(function: &CoreFn, stateful: bool) -> Returned {
    if !stateful {
        Returned::Plain
    } else if function.ret == Repr::Unit {
        Returned::StateOnly
    } else {
        Returned::Pair
    }
}

/// 組の配置 `(,)`。translate がタプルに使う配置と同じ形で、なければ足す (`tuple_layout(2)`)。
pub(super) fn pair_layout(program: &mut Program) -> LayoutId {
    if let Some(index) = program
        .layouts
        .iter()
        .position(|layout| layout.name == "(,)")
    {
        return LayoutId(index as u32);
    }
    program.layouts.push(Layout {
        name: "(,)".to_string(),
        constructors: vec![LayoutCtor {
            name: "(,)".to_string(),
            fields: vec![Repr::TObj, Repr::TObj],
        }],
    });
    LayoutId(program.layouts.len() as u32 - 1)
}

/// `return` の節の状態の引数。複製はこの Repr と印で状態を持ち回る。
pub(super) fn state_info(ret: &CoreFn) -> VarInfo {
    ret.vars[ret.params()[ret.params().len() - 1].0 as usize].clone()
}

/// `shape` の形の複製の結果の Repr と印。`state` は状態つきの handler の状態の引数である。
pub(super) fn result_info(function: &CoreFn, shape: Returned, state: Option<&VarInfo>) -> VarInfo {
    let (repr, mark) = match (shape, state) {
        (Returned::Plain, _) => (function.ret, function.ret_mark),
        (Returned::StateOnly, Some(state)) => (state.repr, state.mark),
        (Returned::Pair, Some(state)) => (Repr::Obj, function.ret_mark.join(state.mark)),
        (_, None) => unreachable!("only a stateful handler threads a state"),
    };
    VarInfo {
        name: "t".to_string(),
        repr,
        mark,
    }
}

/// 展開した節の末尾の `resume k(v, s)` は、`perform` の後ろのブロックへのジャンプになる。ジャンプの実引数には box の
/// 挿入が変換を入れないので (docs/spec/lir.md の「位置の規則」)、v は `perform` の結果の変数と、s は状態の引数と、
/// Repr も印も互換でなければならない。v が操作の引数なら、置き換える `perform` の実引数で比べる。
pub(super) fn resumes_fit(
    program: &Program,
    path: &[FnIdx],
    effect: u32,
    clauses: &[KnownFn],
    state: Option<&VarInfo>,
) -> bool {
    path.iter().all(|&function| {
        let function = program.function(function);
        targeted(function, effect).all(|(block, stmt)| {
            let Stmt::Let {
                var: result,
                rhs:
                    Rhs::Call {
                        call: Call::Perform { op, args, .. },
                        mask: _,
                    },
            } = &function.blocks[block.0 as usize].stmts[stmt]
            else {
                unreachable!("a targeted statement is a perform");
            };
            let clause = &clauses[*op as usize];
            let source = program.function(clause.function);
            let params = source.params();
            let operation = &params[clause.captures.len()..params.len() - 2];
            let fits = |atom: Atom, expected: &VarInfo| {
                let (vars, atom) = match atom {
                    Atom::Var(var) => match operation.iter().position(|&param| param == var) {
                        Some(index) => (&function.vars, args[index]),
                        None => (&source.vars, atom),
                    },
                    atom => (&source.vars, atom),
                };
                atom.fits_compatible(vars, expected.repr)
                    && match atom {
                        Atom::Var(var) => vars[var.0 as usize].mark.compatible(expected.mark),
                        _ => true,
                    }
            };
            let result = &function.vars[result.0 as usize];
            source.blocks.iter().all(|block| {
                let Some((
                    _,
                    Call::Resume {
                        arg, state: next, ..
                    },
                )) = tail_position_call(block)
                else {
                    return true;
                };
                fits(*arg, result) && state.is_none_or(|state| fits(*next, state))
            })
        })
    })
}

/// 複製 `f{H}` を作る。引数は f の引数、操作の節の捕獲 (操作の順)、状態つきなら状態の順にする。`return` の節は
/// handle の地点で呼ぶので、その捕獲は受けない。受けると、末尾でない呼び出しのたびに渡し、`unr` でない捕獲を
/// 2か所で使うことになるためである。
///
/// f のブロックを順に写す。ブロックごとに、H 宛ての `perform` の数だけ、節の入口より後ろのブロックと、続きの
/// ブロック C を足すので、写す前に、元のブロックが新しい並びのどこから始まるかを決めておく。状態は今の値を表す
/// 変数で持ち回り、`jump` の行き先には状態の引数を足して今の状態を渡す。`switch` の行き先は入る辺が1本なので、
/// `switch` の地点の状態で始まる (docs/spec/lir.md の「構造の規則」の R3)。
pub(super) fn build(copies: &mut Copies, index: FnIdx, function: FnIdx, shape: &Shape) {
    let program = copies.program;
    let source = program.function(function);
    let ret = program.function(shape.ret());
    let state = shape.stateful.then(|| state_info(ret));
    let own = returned(source, shape.stateful);
    let result = result_info(source, own, state.as_ref());
    let mut code = CoreFn {
        name: copies.name(function, shape),
        internal: false,
        yields: source.yields,
        clause: None,
        vars: source.vars.clone(),
        ret: result.repr,
        ret_mark: result.mark,
        blocks: Vec::new(),
    };
    let (_, operations) = shape
        .clauses
        .split_last()
        .expect("a handler has a return clause");
    let captures: Vec<Vec<Atom>> = operations
        .iter()
        .map(|&(clause, count)| {
            let clause = program.function(clause);
            clause.params()[..count]
                .iter()
                .map(|&param| {
                    Atom::Var(fresh(&mut code.vars, clause.vars[param.0 as usize].clone()))
                })
                .collect()
        })
        .collect();
    let all_captures: Vec<Atom> = captures.iter().flatten().copied().collect();
    // 操作の節の捕獲は、展開したどの節の本体にも、経路の関数の複製のどの呼び出しにも渡す。何度も使ってよいのは、
    // 操作の節の捕獲が `Unr` に限られるためである (E3001、docs/spec/linearity.md)
    debug_assert!(
        all_captures
            .iter()
            .all(|atom| matches!(atom, Atom::Var(var) if code.mark(*var) == Mark::Unr)),
        "an operation clause captures only `unr` values"
    );
    let initial = state.clone().map(|info| fresh(&mut code.vars, info));

    let performs: Vec<(BlockId, usize)> = targeted(source, shape.effect).collect();
    let clause_of = |op: u32| program.function(operations[op as usize].0);
    let op_of = |(block, stmt): (BlockId, usize)| match &source.block(block).stmts[stmt] {
        Stmt::Let {
            var: _,
            rhs:
                Rhs::Call {
                    call: Call::Perform { op, .. },
                    mask: _,
                },
        } => *op,
        _ => unreachable!("a targeted statement is a perform"),
    };
    let mut starts = Vec::with_capacity(source.blocks.len());
    let mut next = 0;
    for block in 0..source.blocks.len() {
        starts.push(BlockId(next));
        next += 1;
        next += performs
            .iter()
            .filter(|at| at.0.0 as usize == block)
            .map(|&at| clause_of(op_of(at)).blocks.len() as u32)
            .sum::<u32>();
    }
    let mut jumped = vec![false; source.blocks.len()];
    for block in &source.blocks {
        if let Term::Jump { target, args: _ } = block.term {
            jumped[target.0 as usize] = true;
        }
    }

    let pair = copies.pair.map(|layout| Ctor { layout, tag: TUPLE });
    // `StateOnly` の複製の結果の値は `()` なので、受けた変数の使いを `()` にする。使う所は定義より後ろにある
    let mut units: HashSet<VarId> = HashSet::new();
    let mut entered: Vec<Option<VarId>> = vec![None; source.blocks.len()];
    let mut blocks: Vec<Block> = Vec::with_capacity(next as usize);
    for (number, original) in source.blocks.iter().enumerate() {
        debug_assert_eq!(BlockId(blocks.len() as u32), starts[number]);
        let mut params = original.params.clone();
        let mut current_state = if number == 0 {
            params.extend(all_captures.iter().map(|atom| match atom {
                Atom::Var(var) => *var,
                _ => unreachable!("captures are parameters"),
            }));
            params.extend(initial);
            initial
        } else if jumped[number] {
            let param = state.clone().map(|info| fresh(&mut code.vars, info));
            params.extend(param);
            param
        } else {
            entered[number]
        };
        debug_assert_eq!(current_state.is_some(), shape.stateful);
        let mut current = Block {
            params,
            stmts: Vec::new(),
            term: Term::Return(Atom::Unit),
        };
        let tail = tail_position_call(original).map(|(at, _)| at);
        let mut ended = false;
        for (at, stmt) in original.stmts.iter().enumerate() {
            let mut stmt = stmt.clone();
            if let Stmt::Let { var: _, rhs } = &mut stmt {
                rhs.for_each_atom_mut(|atom| unit(&units, atom));
            }
            let Stmt::Let {
                var,
                rhs: Rhs::Call { call, mask },
            } = &stmt
            else {
                current.stmts.push(stmt);
                continue;
            };
            let var = *var;
            match call {
                Call::Perform {
                    effect, op, args, ..
                } if *effect == shape.effect => {
                    let clause = clause_of(*op);
                    let mut clause_args = captures[*op as usize].clone();
                    clause_args.extend(args.iter().copied());
                    let here = blocks.len() as u32;
                    let rest = BlockId(here + clause.blocks.len() as u32);
                    let stateful = shape.stateful;
                    let copied = piece::clause_blocks(
                        &mut code.vars,
                        clause,
                        &clause_args,
                        current_state.map_or(Atom::Unit, Atom::Var),
                        here + 1,
                        |_, value, next| {
                            let mut args = vec![value];
                            if stateful {
                                args.push(next);
                            }
                            (Vec::new(), Term::Jump { target: rest, args })
                        },
                    );
                    let mut copied = copied.into_iter();
                    let entry = copied.next().expect("a function has an entry block");
                    current.stmts.extend(entry.stmts);
                    current.term = entry.term;
                    blocks.push(current);
                    blocks.extend(copied);
                    debug_assert_eq!(BlockId(blocks.len() as u32), rest);
                    current_state = state.clone().map(|info| fresh(&mut code.vars, info));
                    let mut params = vec![var];
                    params.extend(current_state);
                    current = Block {
                        params,
                        stmts: Vec::new(),
                        term: Term::Return(Atom::Unit),
                    };
                }
                Call::Direct(target, args) if program.function(*target).yields => {
                    debug_assert!(mask.is_empty(), "a path has no masked call");
                    let callee = returned(program.function(*target), shape.stateful);
                    let copy = copies.copy(*target, shape);
                    let mut args = args.clone();
                    args.extend(all_captures.iter().copied());
                    args.extend(current_state.map(Atom::Var));
                    let call = |args| Rhs::Call {
                        call: Call::Direct(copy, args),
                        mask: Vec::new(),
                    };
                    // 結果の形が違う末尾の呼び出しは、末尾でない呼び出しとして受け直し、下の `return` で f の形にする
                    if tail == Some(at) && callee == own {
                        let returned = fresh(&mut code.vars, result.clone());
                        current.stmts.push(Stmt::Let {
                            var: returned,
                            rhs: call(args),
                        });
                        current.term = Term::Return(Atom::Var(returned));
                        ended = true;
                        break;
                    }
                    match callee {
                        Returned::Plain => current.stmts.push(Stmt::Let {
                            var,
                            rhs: call(args),
                        }),
                        Returned::StateOnly => {
                            let next = fresh(&mut code.vars, state.clone().expect("a state"));
                            current.stmts.push(Stmt::Let {
                                var: next,
                                rhs: call(args),
                            });
                            units.insert(var);
                            current_state = Some(next);
                        }
                        Returned::Pair => {
                            let received = fresh(
                                &mut code.vars,
                                result_info(program.function(*target), callee, state.as_ref()),
                            );
                            let next = fresh(&mut code.vars, state.clone().expect("a state"));
                            current.stmts.push(Stmt::Let {
                                var: received,
                                rhs: call(args),
                            });
                            current.stmts.push(Stmt::Unpack {
                                value: received,
                                ctor: pair.expect("a stateful passing handler has the pair layout"),
                                fields: vec![var, next],
                            });
                            current_state = Some(next);
                        }
                    }
                }
                _ => current.stmts.push(stmt),
            }
        }
        if !ended {
            let mut term = original.term.clone();
            term.for_each_atom_mut(|atom| unit(&units, atom));
            match &mut term {
                Term::Return(value) => {
                    let value = *value;
                    term = finish(&mut code, &mut current, own, value, current_state, pair);
                }
                Term::Jump { target, args } => {
                    *target = starts[target.0 as usize];
                    args.extend(current_state.map(Atom::Var));
                }
                Term::Switch { .. } => term.for_each_successor_mut(|target| {
                    entered[target.0 as usize] = current_state;
                    *target = starts[target.0 as usize];
                }),
            }
            current.term = term;
        }
        blocks.push(current);
    }
    code.blocks = blocks;
    blocks::prune(&mut code);
    copies.place(index, code);
}

fn unit(units: &HashSet<VarId>, atom: &mut Atom) {
    if let Atom::Var(var) = atom
        && units.contains(var)
    {
        *atom = Atom::Unit;
    }
}

/// f の `return value` を、複製の結果の形 `own` の `return` にする。`StateOnly` で捨てる `unr` でない値は `drop` する。
/// `unr` でない値には Perceus が `decref` を出せないためである (docs/spec/lir.md の「Kind の印の検査」の V1)。組の
/// 印は、結果の印に値の印を結んだものにする。`return` は値と結果の印が互換であればよいので、値の印が結果の印を
/// 超えることがあり、組の作る所の検査はその値の印も求めるためである。
fn finish(
    code: &mut CoreFn,
    block: &mut Block,
    own: Returned,
    value: Atom,
    state: Option<VarId>,
    pair: Option<Ctor>,
) -> Term {
    let mark = |code: &CoreFn, atom: Atom| match atom {
        Atom::Var(var) => code.mark(var),
        _ => Mark::Unr,
    };
    match own {
        Returned::Plain => Term::Return(value),
        Returned::StateOnly => {
            if mark(code, value) != Mark::Unr {
                let dropped = fresh(
                    &mut code.vars,
                    VarInfo {
                        name: "u".to_string(),
                        repr: Repr::Unit,
                        mark: Mark::Unr,
                    },
                );
                block.stmts.push(Stmt::Let {
                    var: dropped,
                    rhs: Rhs::Drop(value),
                });
            }
            Term::Return(Atom::Var(state.expect("a state")))
        }
        Returned::Pair => {
            let state = state.expect("a state");
            let built_mark = code.ret_mark.join(mark(code, value));
            let built = fresh(
                &mut code.vars,
                VarInfo {
                    name: "d".to_string(),
                    repr: Repr::Obj,
                    mark: built_mark,
                },
            );
            block.stmts.push(Stmt::Let {
                var: built,
                rhs: Rhs::Con {
                    ctor: pair.expect("a stateful passing handler has the pair layout"),
                    args: vec![value, Atom::Var(state)],
                },
            });
            Term::Return(Atom::Var(built))
        }
    }
}

//! 状態を渡す形の handler の複製 (docs/spec/core-ir.md の「handler の複製」)。どの節もすぐに再開する handler の
//! 経路の関数 f を、状態を変数で持ち回る1つの関数 `f{H}` に写す。H 宛ての `perform` には節の本体を展開し、その
//! `resume` を `perform` の後ろのブロックへのジャンプにする。経路の関数の呼び出しは、末尾かどうかによらず複製の
//! 呼び出しにし、次の状態を結果の値の並び (`Returned`) で受ける。H に触れない関数の呼び出しはそのまま残す。継続の
//! 関数と合流のブロックの関数は作らない。

use std::collections::{HashMap, HashSet};

use super::super::blocks;
use super::super::known::fresh;
use super::{Copies, KnownFn, Shape, Table, piece, targeted};
use crate::tail::tail_position_call;
use crate::translate::tuple_struct;
use crate::{Block, Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::{
    Atom, BlockId, Ctor, FnIdx, Layout, LayoutCtor, LayoutId, Mark, Repr, Role, Structs, TUPLE,
    VarId, VarInfo,
};

/// 複製 `f{H}` が返す値の並び。複製の元の名前と handler で決まるので、名前が同じ複製はいつも同じ形の結果を返す
/// (docs/spec/core-ir.md の「handler の複製」)。元が基の関数なら、並びは f の結果 (Repr が `unit` でなければ) で、元が
/// 前の回の状態を渡す形の複製なら、その複製の並びである。状態つきの handler は、それに状態を1つ足す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Returned {
    pub(super) values: Vec<(Repr, Mark)>,
}

impl Returned {
    /// 並びを返す結果の Repr と印。値がなければ `()` を、1つならその値を返す。`Unit` を返す関数が、呼ばれるたびに
    /// 組を確保しないためである。2つ以上なら、その大きさのタプルを1つ作って返す。外側の handler ごとに組を重ねない
    /// ためである。タプルの Repr は `Tuples` が値の Repr と印から決める。
    pub(super) fn info(&self, tuples: &Tuples) -> VarInfo {
        let (repr, mark) = match self.values[..] {
            [] => (Repr::Unit, Mark::Unr),
            [value] => value,
            _ => (
                tuples.repr(&self.values),
                self.values
                    .iter()
                    .fold(Mark::Unr, |joined, &(_, mark)| joined.join(mark)),
            ),
        };
        VarInfo {
            name: "t".to_string(),
            repr,
            mark,
        }
    }
}

/// `function` を元にした複製の並び。`state` は状態つきの handler の状態の Repr と印で、`None` なら元の並びを返す。
/// 末尾の経路の形の複製は、`return` の節の結果を返すので、基の関数と同じに扱う。
pub(super) fn returned(
    table: &Table,
    function: FnIdx,
    program: &Program,
    state: Option<(Repr, Mark)>,
) -> Returned {
    let mut values = match table.returns.get(&function) {
        Some(copy) => copy.values.clone(),
        None => {
            let function = program.function(function);
            if function.ret == Repr::Unit {
                Vec::new()
            } else {
                vec![(function.ret, function.ret_mark)]
            }
        }
    };
    values.extend(state);
    Returned { values }
}

/// 状態を渡す形の複製が開くか返すタプル。回の初めに、複製が借りる前の `program` に配置と形を足しておく。
pub(super) struct Tuples {
    /// 大きさごとの配置。
    layouts: HashMap<usize, LayoutId>,
    /// 値の並びごとのタプルの Repr。値の Repr と印から作る形の構造体で、6 語を超える並びは `obj` である
    /// (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「Core IR」の optimize)。並びは複製の元の名前と
    /// handler で決まるので、名前が同じ複製はいつも同じ形を返す。
    reprs: HashMap<Vec<(Repr, Mark)>, Repr>,
}

impl Tuples {
    /// 値の並び `lists` のうち2つ以上の値を持つものの、配置と Repr を決める。
    pub(super) fn new(program: &mut Program, lists: Vec<Vec<(Repr, Mark)>>) -> Tuples {
        let mut tuples = Tuples {
            layouts: HashMap::new(),
            reprs: HashMap::new(),
        };
        for values in lists {
            if values.len() < 2 || tuples.reprs.contains_key(&values) {
                continue;
            }
            let layout = *tuples
                .layouts
                .entry(values.len())
                .or_insert_with(|| tuple_layout(program, values.len()));
            let repr = tuple_struct(&mut program.structs, layout, values.clone());
            tuples.reprs.insert(values, repr);
        }
        tuples
    }

    fn ctor(&self, size: usize) -> Ctor {
        Ctor {
            layout: *self
                .layouts
                .get(&size)
                .expect("the round adds the tuple layouts its copies use"),
            tag: TUPLE,
        }
    }

    fn repr(&self, values: &[(Repr, Mark)]) -> Repr {
        *self
            .reprs
            .get(values)
            .expect("the round adds the tuple reprs its copies use")
    }
}

/// `size` 個の要素のタプルの配置。translate がタプルに使う配置と同じ形 (フィールドはどれも `tobj`) で、配置の表に
/// なければ足す。
fn tuple_layout(program: &mut Program, size: usize) -> LayoutId {
    let name = format!("({})", ",".repeat(size - 1));
    if let Some(index) = program
        .layouts
        .iter()
        .position(|layout| layout.name == name)
    {
        return LayoutId(index as u32);
    }
    program.layouts.push(Layout {
        name: name.clone(),
        constructors: vec![LayoutCtor {
            name,
            fields: vec![Repr::TObj; size],
        }],
    });
    LayoutId(program.layouts.len() as u32 - 1)
}

/// `return` の節の状態の引数。複製はこの Repr と印で状態を持ち回る。
pub(super) fn state_info(ret: &CoreFn) -> VarInfo {
    ret.vars[ret.params()[ret.params().len() - 1].0 as usize].clone()
}

/// 複製の呼び出し `call` の結果を受ける文。`base` は呼ぶ複製の元の並びで、`state` は状態つきの handler の状態の
/// 引数である。返すのは、文と、元の関数の結果を束縛した変数と、次の状態である。元の結果は `value` が作る変数に
/// 束縛し、並びが空なら束縛しない (結果は `()` である)。並びが2つ以上なら、受けたタプルを開いてから元の結果の
/// タプルを作り直す。作り直したタプルは、そのすぐ後ろで前の回の複製のコードが開くので、融合の既知の
/// コンストラクタで消える。
pub(super) fn receive(
    vars: &mut Vec<VarInfo>,
    call: Rhs,
    base: &Returned,
    value: impl FnOnce(&mut Vec<VarInfo>) -> VarId,
    state: Option<&VarInfo>,
    tuples: &Tuples,
) -> (Vec<Stmt>, Option<VarId>, Option<VarId>) {
    let Some(state) = state else {
        let value = value(vars);
        return (
            vec![Stmt::Let {
                var: value,
                rhs: call,
            }],
            Some(value),
            None,
        );
    };
    let count = base.values.len();
    if count == 0 {
        let next = fresh(vars, state.clone());
        return (
            vec![Stmt::Let {
                var: next,
                rhs: call,
            }],
            None,
            Some(next),
        );
    }
    let mut values = base.values.clone();
    values.push((state.repr, state.mark));
    let received = fresh(vars, Returned { values }.info(tuples));
    let mut stmts = vec![Stmt::Let {
        var: received,
        rhs: call,
    }];
    if count == 1 {
        let value = value(vars);
        let next = fresh(vars, state.clone());
        stmts.push(Stmt::Unpack {
            value: received,
            ctor: tuples.ctor(2),
            fields: vec![value, next],
        });
        return (stmts, Some(value), Some(next));
    }
    let fields: Vec<VarId> = base
        .values
        .iter()
        .map(|&(repr, mark)| {
            fresh(
                vars,
                VarInfo {
                    name: "t".to_string(),
                    repr,
                    mark,
                },
            )
        })
        .collect();
    let next = fresh(vars, state.clone());
    let mut unpacked = fields.clone();
    unpacked.push(next);
    stmts.push(Stmt::Unpack {
        value: received,
        ctor: tuples.ctor(count + 1),
        fields: unpacked,
    });
    let value = value(vars);
    stmts.push(Stmt::Let {
        var: value,
        rhs: Rhs::Con {
            ctor: tuples.ctor(count),
            args: fields.into_iter().map(Atom::Var).collect(),
        },
    });
    (stmts, Some(value), Some(next))
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
                atom.fits_compatible(vars, &program.structs, expected.repr)
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
/// `switch` の地点の状態で始まる (docs/spec/lir.md の「構造の規則」の R3)。作った複製の並びは、後の回の複製のために
/// 表に残す。
pub(super) fn build(copies: &mut Copies, index: FnIdx, function: FnIdx, shape: &Shape) {
    let program = copies.program;
    let source = program.function(function);
    let ret = program.function(shape.ret());
    let state = shape.stateful.then(|| state_info(ret));
    let base = returned(copies.table, function, program, None);
    let own = returned(
        copies.table,
        function,
        program,
        state.as_ref().map(|state| (state.repr, state.mark)),
    );
    let result = own.info(copies.tuples);
    copies.table.returns.insert(index, own);
    let touching = copies.touching[&shape.effect].as_slice();
    let tuples = copies.tuples;
    let mut code = CoreFn {
        name: copies.name(function, shape),
        internal: false,
        role: Role::Typed,
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

    // 並びが空の複製の結果の値は `()` なので、受けた変数の使いを `()` にする。使う所は定義より後ろにある
    let mut units: HashSet<VarId> = HashSet::new();
    let mut entered: Vec<Option<VarId>> = vec![None; source.blocks.len()];
    let mut blocks: Vec<Block> = Vec::with_capacity(next as usize);
    let mut extra: Vec<Block> = Vec::new();
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
                // H に触れない関数は経路の外なので、呼び出しをそのまま残す (`path`)
                Call::Direct(target, args)
                    if program.function(*target).yields && touching[target.0 as usize] =>
                {
                    debug_assert!(mask.is_empty(), "a path has no masked call");
                    let callee = returned(copies.table, *target, program, None);
                    let copy = copies.copy(*target, shape);
                    let mut args = args.clone();
                    args.extend(all_captures.iter().copied());
                    args.extend(current_state.map(Atom::Var));
                    let call = |args| Rhs::Call {
                        call: Call::Direct(copy, args),
                        mask: Vec::new(),
                    };
                    // 状態のない handler の複製は元の結果をそのまま返す。状態つきでは、並びの大きさが違う末尾の
                    // 呼び出しを末尾でない呼び出しとして受け直し、下の `return` で f の並びにする
                    if tail == Some(at)
                        && (!shape.stateful || callee.values.len() == base.values.len())
                    {
                        let returned = fresh(&mut code.vars, result.clone());
                        current.stmts.push(Stmt::Let {
                            var: returned,
                            rhs: call(args),
                        });
                        current.term = Term::Return(Atom::Var(returned));
                        ended = true;
                        break;
                    }
                    let (stmts, value, next) = receive(
                        &mut code.vars,
                        call(args),
                        &callee,
                        |_| var,
                        state.as_ref(),
                        tuples,
                    );
                    current.stmts.extend(stmts);
                    if value.is_none() {
                        units.insert(var);
                    }
                    if next.is_some() {
                        current_state = next;
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
                    term = finish(
                        &mut code,
                        &mut current,
                        &base,
                        value,
                        current_state,
                        tuples,
                        &program.structs,
                        (&mut extra, next),
                    );
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
    blocks.append(&mut extra);
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

/// f の `return value` を、複製の結果の並びの `return` にする。`base` は f の並びである。並びが空なら今の状態を返し、
/// 捨てる `unr` でない値は `drop` する。`unr` でない値には Perceus が `decref` を出せないためである
/// (docs/spec/lir.md の「Kind の印の検査」の V1)。そうでなければ、f の並びと今の状態のタプルを返す。f の並びが2つ
/// 以上なら value はそのタプルなので、開いてから並べ直す。タプルの印は、結果の印に要素の印を結んだものにする。
/// `return` は値と結果の印が互換であればよいので、値の印が結果の印を超えることがあり、タプルを作る所の検査はその
/// 値の印も求めるためである。
///
/// 構造体の `con` の引数は正確な位置だが、`return` の value は f の結果の Repr と互換であればよい (`obj` の変数や
/// `tobj` の位置の `()`)。そのときは、value と状態を、並びの Repr の引数を持つブロックへのジャンプで渡し、そこで
/// タプルを作る。ジャンプの実引数は命令なしで収まる互換の位置なので、`return` と同じ値をそのまま渡せる
/// (docs/spec/lir.md の「位置の規則」)。足すブロックは `extra` の並びに積み、番号は `extra` の2つ目の値から振る。
#[allow(clippy::too_many_arguments)]
fn finish(
    code: &mut CoreFn,
    block: &mut Block,
    base: &Returned,
    value: Atom,
    state: Option<VarId>,
    tuples: &Tuples,
    structs: &Structs,
    extra: (&mut Vec<Block>, u32),
) -> Term {
    let Some(state) = state else {
        return Term::Return(value);
    };
    let mark = |code: &CoreFn, atom: Atom| match atom {
        Atom::Var(var) => code.mark(var),
        _ => Mark::Unr,
    };
    let args = match base.values.len() {
        0 => {
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
            return Term::Return(Atom::Var(state));
        }
        1 => {
            let (repr, _) = base.values[0];
            let exact = match value {
                Atom::Var(var) => structs.same(code.repr(var), repr),
                constant => constant.fits(&code.vars, repr),
            };
            if matches!(code.ret, Repr::Struct(_)) && !exact {
                let (extra, first) = extra;
                let target = BlockId(first + extra.len() as u32);
                let info = VarInfo {
                    name: "t".to_string(),
                    repr,
                    mark: mark(code, value),
                };
                let param = fresh(&mut code.vars, info);
                let state_info = code.vars[state.0 as usize].clone();
                let next = fresh(&mut code.vars, state_info);
                let mut built = Block {
                    params: vec![param, next],
                    stmts: Vec::new(),
                    term: Term::Return(Atom::Unit),
                };
                built.term = build_tuple(code, &mut built, vec![Atom::Var(param)], next, tuples);
                extra.push(built);
                return Term::Jump {
                    target,
                    args: vec![value, Atom::Var(state)],
                };
            }
            vec![value]
        }
        count => {
            let Atom::Var(returned) = value else {
                unreachable!("a copy returns its tuple in a variable");
            };
            let fields: Vec<VarId> = base
                .values
                .iter()
                .map(|&(repr, mark)| {
                    fresh(
                        &mut code.vars,
                        VarInfo {
                            name: "t".to_string(),
                            repr,
                            mark,
                        },
                    )
                })
                .collect();
            block.stmts.push(Stmt::Unpack {
                value: returned,
                ctor: tuples.ctor(count),
                fields: fields.clone(),
            });
            fields.into_iter().map(Atom::Var).collect()
        }
    };
    build_tuple(code, block, args, state, tuples)
}

/// f の並び `args` と今の状態のタプルを作って返す。
fn build_tuple(
    code: &mut CoreFn,
    block: &mut Block,
    mut args: Vec<Atom>,
    state: VarId,
    tuples: &Tuples,
) -> Term {
    args.push(Atom::Var(state));
    let built_mark = args.iter().fold(code.ret_mark, |joined, &arg| {
        joined.join(match arg {
            Atom::Var(var) => code.mark(var),
            _ => Mark::Unr,
        })
    });
    let built = fresh(
        &mut code.vars,
        VarInfo {
            name: "d".to_string(),
            repr: code.ret,
            mark: built_mark,
        },
    );
    block.stmts.push(Stmt::Let {
        var: built,
        rhs: Rhs::Con {
            ctor: tuples.ctor(args.len()),
            args,
        },
    });
    Term::Return(Atom::Var(built))
}

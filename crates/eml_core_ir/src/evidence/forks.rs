//! `par` の子の中断に使う関数 `fork$E.op` と `reperform$E.op` (docs/spec/core-ir.md の「`par` の子の中断」)。
//! プログラムが `par` を走らせうるときだけ、fork の関数を持つエフェクトの操作ごとに1組作る。どちらもプログラムの
//! どこからも参照されず、VM が名前で引く。中身は操作の表の行とタプルの配置の名前だけで決まるので、名前が同じなら
//! コードも同じである。

use crate::builder::FnBuilder;
use crate::{Call, Program, Rhs, Stmt, Term};
use eml_extern::{Extern, ExternEffect};
use eml_lir::{Atom, Ctor, FnIdx, Mark, OperationInfo, Prim, Repr, TUPLE, VarId, VarInfo};

use super::{call, dummy, named};

pub(super) fn forks(program: &mut Program) {
    if !may_run_par(program) {
        return;
    }
    let effects: Vec<u32> = eml_lir::fork_effects(&program.effects).collect();
    for effect in effects {
        for op in 0..program.effects[effect as usize].operations.len() as u32 {
            // `fork$` が `reperform$` を値として参照するので、`reperform$` の番号を先に決める
            let reperform = FnIdx(program.functions.len() as u32 + 1);
            let fork = fork(program, effect, op, reperform);
            program.functions.push(fork);
            let reperform = reperform_fn(program, effect, op);
            program.functions.push(reperform);
        }
    }
}

/// `eml_lir::Program::may_run_par` の Core IR の写し。`to_lir` は命令を 1 対 1 に写すので、2つは同じ答えになる。
fn may_run_par(program: &Program) -> bool {
    program
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.stmts)
        .any(|stmt| {
            matches!(
                stmt,
                Stmt::Let {
                    rhs: Rhs::Call {
                        call: Call::Par { .. },
                        ..
                    } | Rhs::Prim(Prim::RootGet(_), _),
                    ..
                }
            )
        })
}

/// `fork$E.op(ev, a_1, .., a_k)`。引数を1つの値に詰めて `child_abort` に渡す。`ev` は節の入口の引数として入口が
/// 持ち、`child_abort` は借りるだけである。`child_abort` は戻らないので、後のダミーには実行が届かない。
fn fork(program: &mut Program, effect: u32, op: u32, reperform: FnIdx) -> crate::CoreFn {
    let operation = eml_lir::operation(&program.effects, effect, op).clone();
    let mut b = FnBuilder::new();
    let ev = b.param(named("ev", Repr::Obj, Mark::Unr));
    let args: Vec<VarId> = operation
        .params
        .iter()
        .zip(&operation.param_marks)
        .map(|(&repr, &mark)| b.param(named("a", repr, mark)))
        .collect();
    let packed = match args[..] {
        [] => Atom::Unit,
        [arg] => Atom::Var(arg),
        _ => {
            let layout = program.tuple_layout(args.len());
            let mark = operation
                .param_marks
                .iter()
                .copied()
                .fold(Mark::Unr, Mark::join);
            Atom::Var(bind(
                &mut b,
                named("t", Repr::Obj, mark),
                Rhs::Con {
                    ctor: Ctor { layout, tag: TUPLE },
                    args: args.iter().map(|&arg| Atom::Var(arg)).collect(),
                },
            ))
        }
    };
    bind(
        &mut b,
        named("u", Repr::Unit, Prim::ChildAbort.row().ret_mark),
        Rhs::Prim(
            Prim::ChildAbort,
            vec![Atom::Var(ev), Atom::Fn(reperform), packed],
        ),
    );
    let value = dummy(Repr::Obj, |info, rhs| bind(&mut b, info, rhs));
    b.terminate(Term::Return(value));
    let mut function = b.finish(
        eml_lir::fork_name(&program.effects, effect, op),
        false,
        false,
        Repr::Obj,
        Mark::Poly,
    );
    function.clause = Some((effect, op));
    function
}

/// `reperform$E.op(t)`。詰めた値を開き、今の `evv` の最初の `E` の evidence で操作を起こす。実行系は呼ぶ前に、子が
/// 届いた handler が最初の evidence になるように `evv` を整える (docs/spec/core-ir.md の「`par` の子の中断」)。
/// 値として呼ばれるので、引数と結果は一様である。
fn reperform_fn(program: &mut Program, effect: u32, op: u32) -> crate::CoreFn {
    let operation = eml_lir::operation(&program.effects, effect, op).clone();
    let mut b = FnBuilder::new();
    let packed = b.param(named("t", packed_repr(&operation), Mark::Poly));
    let args: Vec<Atom> = match operation.params.len() {
        0 => Vec::new(),
        1 => vec![Atom::Var(packed)],
        size => {
            let fields: Vec<VarId> = operation
                .param_marks
                .iter()
                .map(|&mark| b.var(named("a", Repr::TObj, mark)))
                .collect();
            b.emit(Stmt::Unpack {
                value: packed,
                ctor: Ctor {
                    layout: program.tuple_layout(size),
                    tag: TUPLE,
                },
                fields: fields.clone(),
            });
            fields.into_iter().map(Atom::Var).collect()
        }
    };
    let result = named("r", Repr::TObj, Mark::Poly);
    let rhs = match ExternEffect::from_name(&program.effects[effect as usize].name) {
        // extern のエフェクトの evidence は `try_panic` だけが置き、ないときは VM が実行時エラーにする。`panic` は
        // その探し方を持つので、`Panic` は `panic` で起こし直す。位置は子の中にあり、親の側にはない
        Some(ExternEffect::Panic) => Rhs::Extern {
            ext: Extern::Panic,
            args,
            at: None,
        },
        Some(ExternEffect::Io) => unreachable!("`IO` is not never-only, so it has no fork entry"),
        None => {
            let evidence = prim(&mut b, "ev", Repr::Obj, Prim::EvLookup(effect), Vec::new());
            let entry = prim(
                &mut b,
                "c",
                Repr::TObj,
                Prim::EvClause { effect, op },
                vec![Atom::Var(evidence)],
            );
            let mut clause_args = vec![Atom::Var(evidence)];
            clause_args.extend(args);
            call(Call::Clause {
                effect,
                op,
                callee: Atom::Var(entry),
                args: clause_args,
            })
        }
    };
    let result = bind(&mut b, result, rhs);
    b.terminate(Term::Return(Atom::Var(result)));
    b.finish(
        eml_lir::reperform_name(&program.effects, effect, op),
        false,
        false,
        Repr::TObj,
        Mark::Poly,
    )
}

/// `child_abort` が運ぶ詰めた値の Repr。引数が1つならその一様な Repr で、`fork$` の引数がそのまま入る。
fn packed_repr(operation: &OperationInfo) -> Repr {
    match operation.params[..] {
        [] => Repr::Unit,
        [repr @ (Repr::Obj | Repr::TObj | Repr::Unit)] => repr,
        [_] => Repr::TObj,
        _ => Repr::Obj,
    }
}

fn bind(b: &mut FnBuilder, info: VarInfo, rhs: Rhs) -> VarId {
    let var = b.var(info);
    b.emit(Stmt::Let { var, rhs });
    var
}

fn prim(b: &mut FnBuilder, name: &str, repr: Repr, prim: Prim, args: Vec<Atom>) -> VarId {
    let mark = prim.row().ret_mark;
    bind(b, named(name, repr, mark), Rhs::Prim(prim, args))
}

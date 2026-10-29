//! `par` の子の中断に使う関数 `fork$E.op` と `reperform$E.op` (docs/spec/core-ir.md の「`par` の子の中断」)。
//! プログラムが `par` を走らせうるときだけ、fork の関数を持つエフェクトの操作ごとに1組作る。`fork$` はプログラムの
//! どこからも参照されず、VM が名前で引く。`reperform$` は、`fork$` が `child_abort` に渡す閉包として値で参照される。
//! 中身は操作の表の行だけで決まるので、名前が同じならコードも同じである。

use crate::builder::FnBuilder;
use crate::{Call, Program, Rhs, Stmt, Term};
use eml_extern::{Extern, ExternEffect};
use eml_lir::{Atom, FnIdx, Mark, Prim, Repr, VarId, VarInfo};

use super::{captured_mark, dummy};

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

/// `fork$E.op(ev, a_1, .., a_k)`。操作の引数を捕まえた `reperform$E.op` の閉包を `child_abort` に渡す。`ev` は節の
/// エントリの引数としてエントリが持ち、`child_abort` は借りるだけである。`child_abort` は戻らないので、後のダミーには実行が
/// 届かない。
fn fork(program: &Program, effect: u32, op: u32, reperform: FnIdx) -> crate::CoreFn {
    let operation = eml_lir::operation(&program.effects, effect, op).clone();
    let mut b = FnBuilder::new();
    let ev = b.param(VarInfo::new("ev", Repr::Obj, Mark::Unr));
    let args: Vec<Atom> = operation
        .params
        .iter()
        .zip(&operation.param_marks)
        .map(|(&repr, &mark)| Atom::Var(b.param(VarInfo::new("a", repr, mark))))
        .collect();
    let mark = captured_mark(|var| b.mark(var), &args);
    let clause = b.bind(
        VarInfo::new("c", Repr::TObj, mark),
        Rhs::MakeClosure(reperform, args),
    );
    b.bind(
        VarInfo::new("u", Repr::Unit, Prim::ChildAbort.row().ret_mark),
        Rhs::Prim(Prim::ChildAbort, vec![Atom::Var(ev), Atom::Var(clause)]),
    );
    let value = dummy(Repr::Obj, |info, rhs| b.bind(info, rhs));
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

/// `reperform$E.op(a_1, .., a_k, u)`。今の `evv` の最初の `E` の evidence で操作を起こす。実行系は呼ぶ前に、子が
/// 届いた handler が最初の evidence になるように `evv` を整える (docs/spec/core-ir.md の「`par` の子の中断」)。
/// 操作の引数は閉包の捕獲なので型どおりの Repr で受け、実行系が適用する `()` の `u` と結果だけが一様である。
fn reperform_fn(program: &Program, effect: u32, op: u32) -> crate::CoreFn {
    let operation = eml_lir::operation(&program.effects, effect, op).clone();
    let mut b = FnBuilder::new();
    let args: Vec<Atom> = operation
        .params
        .iter()
        .zip(&operation.param_marks)
        .map(|(&repr, &mark)| Atom::Var(b.param(VarInfo::new("a", repr, mark))))
        .collect();
    b.param(VarInfo::new("u", Repr::Unit, Mark::Unr));
    let result = VarInfo::new("r", Repr::TObj, Mark::Poly);
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
            Rhs::call(Call::Clause {
                effect,
                op,
                callee: Atom::Var(entry),
                args: clause_args,
            })
        }
    };
    let result = b.bind(result, rhs);
    b.terminate(Term::Return(Atom::Var(result)));
    let mut function = b.finish(
        eml_lir::reperform_name(&program.effects, effect, op),
        false,
        false,
        Repr::TObj,
        Mark::Poly,
    );
    function.split = Some(operation.params.len() as u32);
    function
}

fn prim(b: &mut FnBuilder, name: &str, repr: Repr, prim: Prim, args: Vec<Atom>) -> VarId {
    let mark = prim.row().ret_mark;
    b.bind(VarInfo::new(name, repr, mark), Rhs::Prim(prim, args))
}

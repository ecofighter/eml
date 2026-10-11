//! EP が作る補助の関数の中身 (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「EP が作る補助の
//! 関数」)。どれも `yields` の印を立て、`internal` にしない。多くの定義から直接呼ばれるためである。evidence を受ける
//! 引数は `obj`、ほかの引数と `ret` は `tobj` で、どれも一様である。中の `apply` はどれも印を立てる。補助の関数は
//! 作るときに最終の形にするので、yield の確認と遅い道もここで置き、後で分けない。

use crate::builder::FnBuilder;
use crate::prim::effect_text;
use crate::{
    Atom, BlockId, Call, Case, CasePattern, CoreFn, FnIdx, Prim, Program, Repr, Rhs, Stmt, Term,
    VarId,
};

use super::{call, direct, dummy, named, yield_switch};

/// `x` をそのまま返す。yield を始めるときの κ である。
pub(super) fn id() -> CoreFn {
    let mut b = FnBuilder::new();
    let x = b.param(named("x", Repr::TObj));
    b.terminate(Term::Return(Atom::Var(x)));
    b.finish("id$".to_string(), false, true, Repr::TObj)
}

/// `compose$(κ, c, x)`。κ を先に呼び、その結果で c を呼ぶ。κ が yield したら、取り出した κ' と c で作り直す。
pub(super) fn compose(compose: FnIdx) -> CoreFn {
    let mut b = FnBuilder::new();
    let k = b.param(named("k", Repr::TObj));
    let c = b.param(named("c", Repr::TObj));
    let x = b.param(named("x", Repr::TObj));
    let r = bind(&mut b, "r", Repr::TObj, apply(k, vec![Atom::Var(x)]));
    let (slow, fast) = check(&mut b);
    b.reopen(slow);
    let taken = take(&mut b);
    let rebuilt = bind(
        &mut b,
        "k",
        Repr::TObj,
        Rhs::MakeClosure(compose, vec![Atom::Var(taken), Atom::Var(c)]),
    );
    yield_dummy(&mut b, rebuilt);
    b.reopen(fast);
    let t = bind(&mut b, "t", Repr::TObj, apply(c, vec![Atom::Var(r)]));
    b.terminate(Term::Return(Atom::Var(t)));
    b.finish("compose$".to_string(), false, true, Repr::TObj)
}

/// `prompt$(ev, ret, x)`。`evv` を `ev` の外側に戻し、`yield_status(ev)` で4つに分ける
/// (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「handle と `prompt$`」)。0 と 2 と 3 の道で
/// `x` は使われず、Perceus が手放す。そのとき `x` はダミーなので、手放しても何も起こらない。
pub(super) fn prompt(resume: FnIdx, rewind: FnIdx) -> CoreFn {
    let mut b = FnBuilder::new();
    let ev = b.param(named("ev", Repr::Obj));
    let ret = b.param(named("ret", Repr::TObj));
    let x = b.param(named("x", Repr::TObj));
    let w = prim(&mut b, "w", Repr::TObj, Prim::EvEvv, vec![Atom::Var(ev)]);
    prim(&mut b, "u", Repr::Unit, Prim::EvvSet, vec![Atom::Var(w)]);
    let status = prim(
        &mut b,
        "s",
        Repr::Int,
        Prim::YieldStatus,
        vec![Atom::Var(ev)],
    );
    let targets: Vec<_> = (0..4).map(|_| b.new_block()).collect();
    b.terminate(Term::Switch {
        scrutinee: Atom::Var(status),
        layout: None,
        cases: (0..3)
            .map(|status| Case {
                pattern: CasePattern::Int(status),
                fields: Vec::new(),
                target: targets[status as usize],
            })
            .collect(),
        default: Some(targets[3]),
    });

    // yield していない
    b.reopen(targets[0]);
    let state = take_state(&mut b, ev);
    let r = bind(
        &mut b,
        "r",
        Repr::TObj,
        apply(ret, vec![Atom::Var(x), Atom::Var(state)]),
    );
    b.terminate(Term::Return(Atom::Var(r)));

    // `ev` 宛ての yield。節の本体に、再開すると `resume$` で handler をつなぎ直す継続を渡す
    b.reopen(targets[1]);
    let k = take(&mut b);
    let clause = prim(&mut b, "c", Repr::TObj, Prim::YieldFinish, Vec::new());
    let state = take_state(&mut b, ev);
    let resumption = bind(
        &mut b,
        "k",
        Repr::TObj,
        Rhs::MakeClosure(resume, vec![Atom::Var(ev), Atom::Var(ret), Atom::Var(k)]),
    );
    let r = bind(
        &mut b,
        "r",
        Repr::TObj,
        apply(clause, vec![Atom::Var(resumption), Atom::Var(state)]),
    );
    b.terminate(Term::Return(Atom::Var(r)));

    // `ev` 宛ての中断。再開しないので継続を捨てる
    b.reopen(targets[2]);
    let k = take(&mut b);
    bind(&mut b, "u", Repr::Unit, Rhs::Drop(Atom::Var(k)));
    let clause = prim(&mut b, "c", Repr::TObj, Prim::YieldFinish, Vec::new());
    let state = take_state(&mut b, ev);
    let r = bind(
        &mut b,
        "r",
        Repr::TObj,
        apply(clause, vec![Atom::Var(state)]),
    );
    b.terminate(Term::Return(Atom::Var(r)));

    // ほかの宛先。再開されたら `rewind$` が今の状態で handler をつなぎ直す
    b.reopen(targets[3]);
    let state = take_state(&mut b, ev);
    let k = take(&mut b);
    let rebuilt = bind(
        &mut b,
        "k",
        Repr::TObj,
        Rhs::MakeClosure(
            rewind,
            vec![
                Atom::Var(ev),
                Atom::Var(ret),
                Atom::Var(state),
                Atom::Var(k),
            ],
        ),
    );
    yield_dummy(&mut b, rebuilt);
    b.finish("prompt$".to_string(), false, true, Repr::TObj)
}

/// `resume$(ev, ret, κ, v, s)`。状態 `s` の新しい evidence を今の `evv` の先頭に足してから κ を再開し、その結果を
/// `prompt$` に渡す (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「再開と状態」)。
pub(super) fn resume(prompt: FnIdx) -> CoreFn {
    let mut b = FnBuilder::new();
    let ev = b.param(named("ev", Repr::Obj));
    let ret = b.param(named("ret", Repr::TObj));
    let k = b.param(named("k", Repr::TObj));
    let v = b.param(named("v", Repr::TObj));
    let s = b.param(named("s", Repr::TObj));
    let renewed = prim(
        &mut b,
        "e",
        Repr::Obj,
        Prim::EvRenew,
        vec![Atom::Var(ev), Atom::Var(s)],
    );
    let w = prim(&mut b, "w", Repr::TObj, Prim::EvvGet, Vec::new());
    let pushed = prim(
        &mut b,
        "w",
        Repr::TObj,
        Prim::EvvPush,
        vec![Atom::Var(w), Atom::Var(renewed)],
    );
    prim(
        &mut b,
        "u",
        Repr::Unit,
        Prim::EvvSet,
        vec![Atom::Var(pushed)],
    );
    let x = bind(&mut b, "x", Repr::TObj, apply(k, vec![Atom::Var(v)]));
    let r = bind(
        &mut b,
        "r",
        Repr::TObj,
        direct(
            prompt,
            vec![Atom::Var(renewed), Atom::Var(ret), Atom::Var(x)],
        ),
    );
    b.terminate(Term::Return(Atom::Var(r)));
    b.finish("resume$".to_string(), false, true, Repr::TObj)
}

/// `rewind$(ev, ret, s, κ, v)`。外側のフレームの yield で handler の外へ出たときに `prompt$` が取り出した状態 `s` で、
/// `resume$` を呼ぶ。
pub(super) fn rewind(resume: FnIdx) -> CoreFn {
    let mut b = FnBuilder::new();
    let ev = b.param(named("ev", Repr::Obj));
    let ret = b.param(named("ret", Repr::TObj));
    let s = b.param(named("s", Repr::TObj));
    let k = b.param(named("k", Repr::TObj));
    let v = b.param(named("v", Repr::TObj));
    let r = bind(
        &mut b,
        "r",
        Repr::TObj,
        direct(
            resume,
            [ev, ret, k, v, s].into_iter().map(Atom::Var).collect(),
        ),
    );
    b.terminate(Term::Return(Atom::Var(r)));
    b.finish("rewind$".to_string(), false, true, Repr::TObj)
}

/// `mask$[effects]` の名前。多重集合をエフェクトの名前で書く。
pub(super) fn mask_name(program: &Program, effects: &[u32]) -> String {
    let names: Vec<String> = effects
        .iter()
        .map(|&effect| effect_text(program, effect))
        .collect();
    format!("mask$[{}]", names.join(", "))
}

/// `mask$[effects](κ, v)`。再開されたときに `evv` から `effects` を飛ばしてから κ を呼び、戻ったら `evv` を戻す。
/// κ が yield したら、もう一度包む (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「`mask`」)。
pub(super) fn mask(mask: FnIdx, name: String, effects: &[u32]) -> CoreFn {
    let mut b = FnBuilder::new();
    let k = b.param(named("k", Repr::TObj));
    let v = b.param(named("v", Repr::TObj));
    let w = prim(&mut b, "w", Repr::TObj, Prim::EvvGet, Vec::new());
    let masked = prim(
        &mut b,
        "m",
        Repr::TObj,
        Prim::EvvMask(effects.to_vec()),
        vec![Atom::Var(w)],
    );
    prim(
        &mut b,
        "u",
        Repr::Unit,
        Prim::EvvSet,
        vec![Atom::Var(masked)],
    );
    let r = bind(&mut b, "r", Repr::TObj, apply(k, vec![Atom::Var(v)]));
    let (slow, fast) = check(&mut b);
    b.reopen(slow);
    let taken = take(&mut b);
    let rebuilt = bind(
        &mut b,
        "k",
        Repr::TObj,
        Rhs::MakeClosure(mask, vec![Atom::Var(taken)]),
    );
    yield_dummy(&mut b, rebuilt);
    b.reopen(fast);
    prim(&mut b, "u", Repr::Unit, Prim::EvvSet, vec![Atom::Var(w)]);
    b.terminate(Term::Return(Atom::Var(r)));
    b.finish(name, false, true, Repr::TObj)
}

fn bind(b: &mut FnBuilder, name: &str, repr: Repr, rhs: Rhs) -> VarId {
    let var = b.var(named(name, repr));
    b.emit(Stmt::Let { var, rhs });
    var
}

fn prim(b: &mut FnBuilder, name: &str, repr: Repr, prim: Prim, args: Vec<Atom>) -> VarId {
    bind(b, name, repr, Rhs::Prim(prim, args))
}

fn apply(callee: VarId, args: Vec<Atom>) -> Rhs {
    call(Call::Apply {
        callee: Atom::Var(callee),
        args,
        yields: true,
    })
}

fn take(b: &mut FnBuilder) -> VarId {
    prim(b, "k", Repr::TObj, Prim::YieldContTake, Vec::new())
}

fn take_state(b: &mut FnBuilder, ev: VarId) -> VarId {
    prim(b, "st", Repr::TObj, Prim::EvTakeState, vec![Atom::Var(ev)])
}

/// 直前の呼び出しが yield したかで分ける。遅い道と速い道のブロックを、この順に作って返す。
fn check(b: &mut FnBuilder) -> (BlockId, BlockId) {
    let yielding = prim(b, "y", Repr::Int, Prim::Yielding, Vec::new());
    let slow = b.new_block();
    let fast = b.new_block();
    b.terminate(yield_switch(yielding, fast, slow));
    (slow, fast)
}

/// 作り直した κ を戻し、ダミーを返して今のブロックを閉じる。
fn yield_dummy(b: &mut FnBuilder, k: VarId) {
    prim(b, "u", Repr::Unit, Prim::YieldContSet, vec![Atom::Var(k)]);
    let value = dummy(Repr::TObj, |info, rhs| {
        let var = b.var(info);
        b.emit(Stmt::Let { var, rhs });
        var
    });
    b.terminate(Term::Return(value));
}

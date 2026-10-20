//! EP が作る補助の関数の中身 (docs/spec/core-ir.md の「EP が作る補助の関数」)。どれも `yields` の印を立て、
//! `internal` にしない。多くの定義から直接呼ばれるためである。evidence を受ける引数は `obj`、ほかの引数と `ret` は
//! `tobj` で、どれも一様である。印は evidence の引数が `unr`、ほかの引数と `ret` が `poly` である。κ や状態は `File` を
//! 持ちうるためである (docs/spec/core-ir.md の「EP が足す変数の印」)。中の
//! `apply` はどれも印を立てる。補助の関数は作るときに最終の形にするので、yield の確認と遅い道もここで置き、後で分けない。

use crate::builder::FnBuilder;
use crate::{Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::syntax::mask_text;
use eml_lir::{Atom, BlockId, Case, CasePattern, FnIdx, Mark, MaskItem, Prim, Repr, Site, VarId};

use super::{call, captured_mark, direct, dummy, named, yield_switch};

/// `x` をそのまま返す。yield を始めるときの κ である。
pub(super) fn id() -> CoreFn {
    let mut b = FnBuilder::new();
    let x = b.param(named("x", Repr::TObj, Mark::Poly));
    b.terminate(Term::Return(Atom::Var(x)));
    b.finish("id$".to_string(), false, true, Repr::TObj, Mark::Poly)
}

/// `compose$(κ, c, x)`。κ を先に呼び、その結果で c を呼ぶ。κ が yield したら、取り出した κ' と c で作り直す。
pub(super) fn compose(compose: FnIdx) -> CoreFn {
    let mut b = FnBuilder::new();
    let k = b.param(named("k", Repr::TObj, Mark::Poly));
    let c = b.param(named("c", Repr::TObj, Mark::Poly));
    let x = b.param(named("x", Repr::TObj, Mark::Poly));
    let r = apply(&mut b, "r", k, vec![Atom::Var(x)]);
    let (slow, fast) = check(&mut b);
    b.reopen(slow);
    // 遅い道の `r` は κ が yield したときのダミーである
    drop(&mut b, r);
    let taken = take(&mut b);
    let rebuilt = closure(&mut b, compose, vec![Atom::Var(taken), Atom::Var(c)]);
    yield_dummy(&mut b, rebuilt);
    b.reopen(fast);
    let t = apply(&mut b, "t", c, vec![Atom::Var(r)]);
    b.terminate(Term::Return(Atom::Var(t)));
    b.finish("compose$".to_string(), false, true, Repr::TObj, Mark::Poly)
}

/// `prompt$(ev, ret, x)`。`evv` を `ev` の外側に戻し、`yield_status(ev)` で4つに分ける
/// (docs/spec/core-ir.md の「handle と `prompt$`」)。状態 1、2、3 の道では `x` はダミーで使われないので、道の先頭で
/// drop する。`x` は `poly` なので、Perceus の `decref` には任せない。ダミーの drop は何もしない。中断の道では `ret` も
/// 使われないので drop する。捕まえた `File` を閉じる本当の後始末はこれだけである
/// (docs/spec/core-ir.md の「EP の明示の `drop`」)。
pub(super) fn prompt(resume: FnIdx, rewind: FnIdx) -> CoreFn {
    let mut b = FnBuilder::new();
    let ev = b.param(named("ev", Repr::Obj, Mark::Unr));
    let ret = b.param(named("ret", Repr::TObj, Mark::Poly));
    let x = b.param(named("x", Repr::TObj, Mark::Poly));
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
    let r = apply(&mut b, "r", ret, vec![Atom::Var(x), Atom::Var(state)]);
    b.terminate(Term::Return(Atom::Var(r)));

    // `ev` 宛ての yield。節の本体に、再開すると `resume$` で handler をつなぎ直す継続を渡す
    b.reopen(targets[1]);
    drop(&mut b, x);
    let k = take(&mut b);
    let clause = prim(&mut b, "c", Repr::TObj, Prim::YieldFinish, Vec::new());
    let state = take_state(&mut b, ev);
    let resumption = closure(
        &mut b,
        resume,
        vec![Atom::Var(ev), Atom::Var(ret), Atom::Var(k)],
    );
    let r = apply(
        &mut b,
        "r",
        clause,
        vec![Atom::Var(resumption), Atom::Var(state)],
    );
    b.terminate(Term::Return(Atom::Var(r)));

    // `ev` 宛ての中断。巻き戻しで着地したので、κ はない
    // (docs/spec/core-ir.md の「handle と `prompt$`」)
    b.reopen(targets[2]);
    drop(&mut b, ret);
    drop(&mut b, x);
    let clause = prim(&mut b, "c", Repr::TObj, Prim::YieldFinish, Vec::new());
    let state = take_state(&mut b, ev);
    let r = apply(&mut b, "r", clause, vec![Atom::Var(state)]);
    b.terminate(Term::Return(Atom::Var(r)));

    // ほかの宛先。再開されたら `rewind$` が今の状態で handler をつなぎ直す
    b.reopen(targets[3]);
    drop(&mut b, x);
    let state = take_state(&mut b, ev);
    let k = take(&mut b);
    let rebuilt = closure(
        &mut b,
        rewind,
        vec![
            Atom::Var(ev),
            Atom::Var(ret),
            Atom::Var(state),
            Atom::Var(k),
        ],
    );
    yield_dummy(&mut b, rebuilt);
    b.finish("prompt$".to_string(), false, true, Repr::TObj, Mark::Poly)
}

/// `resume$(ev, ret, κ, v, s)`。状態 `s` の新しい evidence を今の `evv` の先頭に足してから κ を再開し、その結果を
/// `prompt$` に渡す (docs/spec/core-ir.md の「再開と状態」)。
pub(super) fn resume(prompt: FnIdx) -> CoreFn {
    let mut b = FnBuilder::new();
    let ev = b.param(named("ev", Repr::Obj, Mark::Unr));
    let ret = b.param(named("ret", Repr::TObj, Mark::Poly));
    let k = b.param(named("k", Repr::TObj, Mark::Poly));
    let v = b.param(named("v", Repr::TObj, Mark::Poly));
    let s = b.param(named("s", Repr::TObj, Mark::Poly));
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
    let x = apply_marked(&mut b, "x", k, vec![Atom::Var(v)], Some(renewed));
    let r = bind(
        &mut b,
        "r",
        Repr::TObj,
        Mark::Poly,
        direct(
            prompt,
            vec![Atom::Var(renewed), Atom::Var(ret), Atom::Var(x)],
        ),
    );
    b.terminate(Term::Return(Atom::Var(r)));
    b.finish("resume$".to_string(), false, true, Repr::TObj, Mark::Poly)
}

/// `rewind$(ev, ret, s, κ, v)`。外側のフレームの yield で handler の外へ出たときに `prompt$` が取り出した状態 `s` で、
/// `resume$` を呼ぶ。
pub(super) fn rewind(resume: FnIdx) -> CoreFn {
    let mut b = FnBuilder::new();
    let ev = b.param(named("ev", Repr::Obj, Mark::Unr));
    let ret = b.param(named("ret", Repr::TObj, Mark::Poly));
    let s = b.param(named("s", Repr::TObj, Mark::Poly));
    let k = b.param(named("k", Repr::TObj, Mark::Poly));
    let v = b.param(named("v", Repr::TObj, Mark::Poly));
    let r = bind(
        &mut b,
        "r",
        Repr::TObj,
        Mark::Poly,
        direct(
            resume,
            [ev, ret, k, v, s].into_iter().map(Atom::Var).collect(),
        ),
    );
    b.terminate(Term::Return(Atom::Var(r)));
    b.finish("rewind$".to_string(), false, true, Repr::TObj, Mark::Poly)
}

/// `mask$[items]` の名前。項目をテキストの形で書く。
pub(super) fn mask_name(program: &Program, items: &[MaskItem]) -> String {
    format!("mask$[{}]", mask_text(&program.effects, items))
}

/// `mask$[items](κ, v)`。再開されたときに `evv` から `items` の項目を飛ばしてから κ を呼び、戻ったら `evv` を戻す。
/// κ が yield したら、もう一度包む (docs/spec/core-ir.md の「`mask` の包み」)。
pub(super) fn mask(mask: FnIdx, name: String, items: &[MaskItem]) -> CoreFn {
    let mut b = FnBuilder::new();
    let k = b.param(named("k", Repr::TObj, Mark::Poly));
    let v = b.param(named("v", Repr::TObj, Mark::Poly));
    let w = prim(&mut b, "w", Repr::TObj, Prim::EvvGet, Vec::new());
    let masked = prim(
        &mut b,
        "m",
        Repr::TObj,
        Prim::EvvMask(items.to_vec()),
        vec![Atom::Var(w)],
    );
    prim(
        &mut b,
        "u",
        Repr::Unit,
        Prim::EvvSet,
        vec![Atom::Var(masked)],
    );
    let r = apply(&mut b, "r", k, vec![Atom::Var(v)]);
    let (slow, fast) = check(&mut b);
    b.reopen(slow);
    // 遅い道の `r` は κ が yield したときのダミーである
    drop(&mut b, r);
    let taken = take(&mut b);
    let rebuilt = closure(&mut b, mask, vec![Atom::Var(taken)]);
    yield_dummy(&mut b, rebuilt);
    b.reopen(fast);
    prim(&mut b, "u", Repr::Unit, Prim::EvvSet, vec![Atom::Var(w)]);
    b.terminate(Term::Return(Atom::Var(r)));
    b.finish(name, false, true, Repr::TObj, Mark::Poly)
}

fn bind(b: &mut FnBuilder, name: &str, repr: Repr, mark: Mark, rhs: Rhs) -> VarId {
    let var = b.var(named(name, repr, mark));
    b.emit(Stmt::Let { var, rhs });
    var
}

/// 結果の印は基本操作の表の印である。表は EP が自分の変数に付ける印と同じに作ってある。
fn prim(b: &mut FnBuilder, name: &str, repr: Repr, prim: Prim, args: Vec<Atom>) -> VarId {
    let mark = prim.row().ret_mark;
    bind(b, name, repr, mark, Rhs::Prim(prim, args))
}

/// κ を包むクロージャ。
fn closure(b: &mut FnBuilder, target: FnIdx, captures: Vec<Atom>) -> VarId {
    let mark = captured_mark(|var| b.mark(var), &captures);
    bind(b, "k", Repr::TObj, mark, Rhs::MakeClosure(target, captures))
}

fn drop(b: &mut FnBuilder, var: VarId) {
    bind(b, "u", Repr::Unit, Mark::Unr, Rhs::Drop(Atom::Var(var)));
}

/// 補助の関数の `apply` は、一様な位置の κ、節の本体、`return` の節を呼ぶ。結果は何でもありうるので `poly` である。
fn apply(b: &mut FnBuilder, name: &str, callee: VarId, args: Vec<Atom>) -> VarId {
    apply_marked(b, name, callee, args, None)
}

/// 続く `prompt$` の evidence を `prompt` に持つ `apply`。中断が巻き戻しで着地する先を示す。
fn apply_marked(
    b: &mut FnBuilder,
    name: &str,
    callee: VarId,
    args: Vec<Atom>,
    prompt: Option<VarId>,
) -> VarId {
    // 補助の関数の `apply` は EP の一様な場所である
    // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「`apply` の ABI」)
    let rhs = call(Call::Apply {
        callee: Atom::Var(callee),
        args,
        yields: true,
        prompt,
        site: Site::Uniform,
    });
    bind(b, name, Repr::TObj, Mark::Poly, rhs)
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

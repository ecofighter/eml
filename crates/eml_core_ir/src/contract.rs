//! box の挿入と Perceus の間の縮約のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消し、その後で
//! すべてのブロックに末尾呼び出しの規則を当てる。末尾呼び出しを作るのはこのパスだけである。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。

use eml_extern::Purity;

use crate::{Block, Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::{Atom, EffectInfo, OpRet, Repr, operation};

pub fn contract(program: &mut Program) {
    let rets: Vec<Repr> = program
        .functions
        .iter()
        .map(|function| function.ret)
        .collect();
    for function in &mut program.functions {
        remove_dead_lets(function);
        for block in &mut function.blocks {
            tail_call(block, function.ret, &rets, &program.effects);
        }
        // 1回のパスで不動点に達することを確かめる (docs/spec/core-ir.md の「縮約」)。確かめるパスも IR を書き換えうるので、
        // その副作用を `debug_assert!` の式に隠さない
        if cfg!(debug_assertions) {
            let removed = remove_dead_lets(function);
            assert!(
                !removed,
                "one backward pass removes every dead binding in `{}`",
                function.name
            );
        }
    }
}

/// `let x = <呼び出し>` の後の終端が `return x` で、呼び出しの結果が関数の `ret` と互換なら、その2つを末尾呼び出しに
/// する。互換は推移的でないので、x を通してつながっていた2つの位置が、直接つないでも互換かを確かめる。結果は、直接の
/// 呼び出しなら呼ばれる関数の `ret`、節の呼び出しなら操作の `ret`、関数値の適用なら `tobj` である。`never` の操作の
/// 節の呼び出しは戻らないので、どの `ret` とも互換とする (docs/spec/core-ir.md の「縮約」)。`saved` は Perceus が
/// 決めるので、この時点では空である。`mask` も、EP が `evv_mask` の包みに置き換えたので空である。
fn tail_call(block: &mut Block, ret: Repr, rets: &[Repr], effects: &[EffectInfo]) {
    let Term::Return(Atom::Var(returned)) = block.term else {
        return;
    };
    let Some(Stmt::Let {
        var,
        rhs: Rhs::Call {
            call,
            mask,
            saved: _,
        },
    }) = block.stmts.last()
    else {
        return;
    };
    if *var != returned {
        return;
    }
    debug_assert!(mask.is_empty(), "a mask reaches contract");
    let compatible = match call {
        Call::Direct(target, _) => rets[target.0 as usize].compatible(ret),
        Call::Apply { .. } => Repr::TObj.compatible(ret),
        Call::Clause {
            effect,
            op,
            callee: _,
            args: _,
        } => match operation(effects, *effect, *op).ret {
            OpRet::Repr(repr) => repr.compatible(ret),
            OpRet::Never => true,
        },
        Call::Handle { .. } | Call::Perform { .. } | Call::Resume { .. } => {
            unreachable!("an effect instruction reaches contract")
        }
    };
    if !compatible {
        return;
    }
    let Some(Stmt::Let {
        var: _,
        rhs: Rhs::Call {
            call,
            mask: _,
            saved: _,
        },
    }) = block.stmts.pop()
    else {
        unreachable!("the last statement was checked above");
    };
    block.term = Term::TailCall(call);
}

/// 使われない純粋な `let` を消し、1つでも消したかを返す。定義は使う位置を支配し、辺は番号の大きい
/// ブロックへ向かうので、ブロックを後ろから、文を後ろから見れば、`let` を見る時点でその変数の使用はすべて見終わって
/// いる。関数全体の使用の数を消すたびに減らせば、1回のパスで、消した `let` だけが使っていた束縛も消える。
fn remove_dead_lets(function: &mut CoreFn) -> bool {
    let mut uses = vec![0usize; function.vars.len()];
    let mut count = |atom: Atom| {
        if let Atom::Var(var) = atom {
            uses[var.0 as usize] += 1;
        }
    };
    for block in &function.blocks {
        for stmt in &block.stmts {
            stmt.for_each_atom(&mut count);
        }
        block.term.for_each_atom(&mut count);
    }
    let mut removed = false;
    for block in function.blocks.iter_mut().rev() {
        let mut keep = vec![true; block.stmts.len()];
        for (position, stmt) in block.stmts.iter().enumerate().rev() {
            if let Stmt::Let { var, rhs } = stmt
                && uses[var.0 as usize] == 0
                && pure(rhs)
            {
                keep[position] = false;
                rhs.for_each_atom(|atom| {
                    if let Atom::Var(used) = atom {
                        uses[used.0 as usize] -= 1;
                    }
                });
            }
        }
        if keep.contains(&false) {
            let mut keep = keep.into_iter();
            block.stmts.retain(|_| keep.next() == Some(true));
            removed = true;
        }
    }
    removed
}

/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。extern は表の行が `Pure` のものだけである。
/// `con` と `closure` が所有権を受け取る値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が
/// 早まるだけである。`box` を消すと確保が1つ減るだけで、`unbox` は値を読むだけなので、どちらも評価の順を変えない。
pub(crate) fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::ConstString(_)
        | Rhs::Con { ctor: _, args: _ }
        | Rhs::MakeClosure(_, _)
        | Rhs::Box(_)
        | Rhs::Unbox(_) => true,
        Rhs::Extern {
            ext,
            args: _,
            at: _,
        } => ext.row().purity == Purity::Pure,
        Rhs::Prim(prim, _) => prim.pure(),
        Rhs::Call {
            call: _,
            mask: _,
            saved: _,
        }
        | Rhs::Drop(_) => false,
    }
}

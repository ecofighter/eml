//! translate と Perceus の間の縮約のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消し、消した
//! ブロックの末尾にだけ末尾呼び出しの規則をもう一度当てる。RC の命令がまだないので、所有権を扱わずに書き換えられる。

use eml_extern::Purity;

use crate::{Atom, Block, CoreFn, Program, Rhs, Stmt, Term};

pub fn contract(program: &mut Program) {
    for function in &mut program.functions {
        for index in remove_dead_lets(function) {
            tail_call(&mut function.blocks[index]);
        }
        // 1回のパスで不動点に達することを確かめる (docs/spec/core-ir.md の「縮約」)。確かめるパスも IR を書き換えうるので、
        // その副作用を `debug_assert!` の式に隠さない
        if cfg!(debug_assertions) {
            let left = remove_dead_lets(function);
            assert!(
                left.is_empty(),
                "one backward pass removes every dead binding in `{}`",
                function.name
            );
        }
    }
}

/// `let x = <呼び出し>` の後の終端が `return x` なら、その2つを末尾呼び出しにする。translate の `finish` と contract が
/// 使う。`saved` は Perceus が決めるので、この時点では空である。`mask` は末尾かどうかと独立なので、そのまま運ぶ。
pub fn tail_call(block: &mut Block) {
    let Term::Return(Atom::Var(returned)) = block.term else {
        return;
    };
    let Some(Stmt::Let {
        var,
        rhs: Rhs::Call {
            call: _,
            mask: _,
            saved: _,
        },
    }) = block.stmts.last()
    else {
        return;
    };
    if *var != returned {
        return;
    }
    let Some(Stmt::Let {
        var: _,
        rhs: Rhs::Call {
            call,
            mask,
            saved: _,
        },
    }) = block.stmts.pop()
    else {
        unreachable!("the last statement was checked above");
    };
    block.term = Term::TailCall { call, mask };
}

/// 使われない純粋な `let` を消し、文を消したブロックの番号を返す。定義は使う位置を支配し、辺は番号の大きい
/// ブロックへ向かうので、ブロックを後ろから、文を後ろから見れば、`let` を見る時点でその変数の使用はすべて見終わって
/// いる。関数全体の使用の数を消すたびに減らせば、1回のパスで、消した `let` だけが使っていた束縛も消える。
fn remove_dead_lets(function: &mut CoreFn) -> Vec<usize> {
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
    let mut changed = Vec::new();
    for (index, block) in function.blocks.iter_mut().enumerate().rev() {
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
            changed.push(index);
        }
    }
    changed
}

/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。extern は表の行が `Pure` のものだけである。
/// `con` と `closure` が所有権を受け取る値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が
/// 早まるだけである。
fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::ConstString(_) | Rhs::Con { ctor: _, args: _ } | Rhs::MakeClosure(_, _) => true,
        Rhs::Extern {
            ext,
            args: _,
            at: _,
        } => ext.row().purity == Purity::Pure,
        Rhs::Call {
            call: _,
            mask: _,
            saved: _,
        }
        | Rhs::Drop(_) => false,
    }
}

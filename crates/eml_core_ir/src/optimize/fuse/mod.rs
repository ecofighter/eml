//! 融合の段。handler への特殊化の後のプログラムを凍らせて元 (`Base`) にし、関数ごとに規則を当てられなくなるまで
//! 当てる。規則は、小さな再帰しない関数のインライン化、定数で分ける `switch` の畳み込み、1本の `jump` だけが入る
//! ブロックのつなぎ、使われない純粋な `let` の削除である
//! (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「S13e-3 インライン化と融合」)。
//!
//! 展開する本体はいつも元のコードから写す。元のコードは名前で決まるので、関数を書き換える順によらず、同じ名前の関数は
//! 同じコードになる (docs/spec/vm.md の「追記」)。

mod inline;

use crate::tail::pure;
use crate::{Call, CoreFn, Program, Rhs, Stmt};
use eml_lir::Atom;

use super::blocks;

/// `fuse` に入った時点のプログラム。展開するかどうかの判断と、展開する本体は、どれもここから読む。
struct Base {
    functions: Vec<CoreFn>,
    /// 直接の呼び出しの関係で、2つ以上の関数の SCC に入るか、自分を呼ぶ関数。
    recursive: Vec<bool>,
    /// 関数の文の数。ブロックの終端も1つの文と数える。文を持たないブロックが多い関数 (定数を返す枝の多い `match`)
    /// も、展開すれば終端の数だけコードが増えるためである。
    size: Vec<usize>,
}

impl Base {
    fn new(program: &Program) -> Base {
        Base {
            recursive: recursive(&program.functions),
            size: program
                .functions
                .iter()
                .map(|function| {
                    function
                        .blocks
                        .iter()
                        .map(|block| block.stmts.len() + 1)
                        .sum()
                })
                .collect(),
            functions: program.functions.clone(),
        }
    }
}

pub(crate) fn fuse(program: &mut Program) {
    let base = Base::new(program);
    let mut index = 0;
    while index < program.functions.len() {
        fuse_function(&base, &mut program.functions[index]);
        index += 1;
    }
}

/// 関数1つに、どの規則も当たらなくなるまで規則を当てる。どの規則も関数を小さくするか、展開の深さを1つ進めるので止まる。
fn fuse_function(base: &Base, function: &mut CoreFn) {
    let mut depths = Vec::new();
    loop {
        let mut changed = inline::inline_calls(base, function, &mut depths);
        if blocks::fold_constant_switches(function) {
            blocks::prune(function);
            changed = true;
        }
        changed |= blocks::merge_jumps(function);
        changed |= drop_dead_pure_lets(function);
        if !changed {
            break;
        }
    }
}

/// 使われない純粋な `let` を消し、1つでも消したかを返す。展開した本体が返す値を呼ぶ側が捨てる所で、値を作るだけの
/// 文が残るためである。純粋の規則は EP と同じ `crate::tail::pure` で、消しても評価の順と実行時エラーは変わらない。
/// 定義は使う位置を支配し、辺は番号の大きいブロックへ向かうので、ブロックを後ろから、文を後ろから見れば、`let` を
/// 見る時点でその変数の使いはすべて見終わっている。消すたびに使いの数を減らせば、1回で、消した `let` だけが使って
/// いた束縛も消える (LIR の縮約の `remove_dead_lets` と同じ形)。
fn drop_dead_pure_lets(function: &mut CoreFn) -> bool {
    let mut uses = vec![0usize; function.vars.len()];
    for block in &function.blocks {
        let mut count = |atom: Atom| {
            if let Atom::Var(var) = atom {
                uses[var.0 as usize] += 1;
            }
        };
        block
            .stmts
            .iter()
            .for_each(|stmt| stmt.for_each_atom(&mut count));
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

/// 直接の呼び出しの関係で再帰する関数。Tarjan の方法で SCC を求める。プログラムの大きさに比例する手間で済み、
/// 深い呼び出しの連なりでもスタックを溢れさせないように、再帰を使わずに書く。
fn recursive(functions: &[CoreFn]) -> Vec<bool> {
    let count = functions.len();
    let callees: Vec<Vec<usize>> = functions
        .iter()
        .map(|function| {
            let mut callees: Vec<usize> = function
                .blocks
                .iter()
                .flat_map(|block| &block.stmts)
                .filter_map(|stmt| match stmt {
                    Stmt::Let {
                        var: _,
                        rhs:
                            Rhs::Call {
                                call: Call::Direct(target, _),
                                mask: _,
                            },
                    } => Some(target.0 as usize),
                    _ => None,
                })
                .collect();
            callees.sort_unstable();
            callees.dedup();
            callees
        })
        .collect();
    const UNSEEN: usize = usize::MAX;
    let mut order = vec![UNSEEN; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut recursive = vec![false; count];
    for root in 0..count {
        if order[root] != UNSEEN {
            continue;
        }
        let mut frames = vec![(root, 0usize)];
        order[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(frame) = frames.last_mut() {
            let (function, position) = *frame;
            if let Some(&callee) = callees[function].get(position) {
                frame.1 += 1;
                if order[callee] == UNSEEN {
                    order[callee] = next;
                    low[callee] = next;
                    next += 1;
                    stack.push(callee);
                    on_stack[callee] = true;
                    frames.push((callee, 0));
                } else if on_stack[callee] {
                    low[function] = low[function].min(order[callee]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(caller, _)) = frames.last() {
                low[caller] = low[caller].min(low[function]);
            }
            if low[function] == order[function] {
                let mut members = Vec::new();
                loop {
                    let member = stack.pop().expect("an SCC member is on the stack");
                    on_stack[member] = false;
                    members.push(member);
                    if member == function {
                        break;
                    }
                }
                if members.len() > 1 || callees[function].contains(&function) {
                    for member in members {
                        recursive[member] = true;
                    }
                }
            }
        }
    }
    recursive
}

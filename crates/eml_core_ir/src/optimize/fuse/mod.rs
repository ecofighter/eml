//! 融合の段。handler への特殊化の後のプログラムの直接の呼び出しの関係を凍らせ (`Base`)、呼ばれる関数から先に、
//! 関数を1つずつ1回だけ書き換える。規則は、小さな再帰しない関数のインライン化、定数で分ける `switch` の畳み込み、
//! 1本の `jump` だけが入るブロックのつなぎ、使われない純粋な `let` の削除である
//! (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「S13e-3 インライン化と融合」)。
//!
//! 展開する本体は、呼び出し先の書き換えた後のコードから写す。呼び出し先のコードはそのときにはもう決まっていて、名前で
//! 決まる。展開した本体の中の呼び出しは、呼び出し先を書き換えたときに展開するかどうかを決めてあるので、呼ぶ側では
//! 展開し直さない。どの関数のコードも、その関数と呼び出し先のコードだけで決まり、プログラムのほかの関数によらない
//! (docs/spec/vm.md の「追記」)。展開を重ねても、展開した本体の大きさは `INLINE_LIMIT` を超えないので、コードは
//! 呼び出しの連なりの深さの積で育たない。

mod inline;
mod known_con;

use crate::tail::pure;
use crate::{Call, CoreFn, Program, Rhs, Stmt};
use eml_lir::Atom;

use super::blocks;
use super::known::Callee;

/// `fuse` に入った時点の直接の呼び出しの関係。
struct Base {
    /// 2つ以上の関数の SCC に入るか、自分を呼ぶ関数。展開しない。
    recursive: Vec<bool>,
    /// 書き換える順。どの関数も、SCC の外の呼び出し先より後に来る。
    order: Vec<usize>,
}

pub(crate) fn fuse(program: &mut Program) {
    let (recursive, order) = call_graph(&program.functions);
    let base = Base { recursive, order };
    // 呼ぶ相手の引数の数、Repr、印、包む命令は、融合で変わらない
    let callees: Vec<Callee> = program.functions.iter().map(Callee::of).collect();
    let mut inlinable = vec![false; program.functions.len()];
    for &index in &base.order {
        let mut function = std::mem::replace(&mut program.functions[index], placeholder());
        fuse_function(&mut function, &program.functions, &inlinable, &callees);
        inlinable[index] = !base.recursive[index] && inline::inlinable(&function);
        program.functions[index] = function;
    }
}

/// 書き換えている間だけ関数の表に置く空の関数。書き換えている関数は再帰しないので、自分を展開しようとはしない。
fn placeholder() -> CoreFn {
    CoreFn {
        name: String::new(),
        internal: false,
        yields: false,
        clause: None,
        vars: Vec::new(),
        ret: eml_lir::Repr::Unit,
        ret_mark: eml_lir::Mark::Unr,
        blocks: Vec::new(),
    }
}

/// 関数1つを書き換える。展開を1回の走査で済ませ、そのあと片付けと既知のコンストラクタの規則を、どれも当たらなく
/// なるまで繰り返す。展開した本体は読み直さない。どの規則も、`switch` や `unpack` を通る経路か `apply` か文を
/// 減らし、ほかの規則が減らしたものを増やさないので、繰り返しは止まる。
fn fuse_function(
    function: &mut CoreFn,
    functions: &[CoreFn],
    inlinable: &[bool],
    callees: &[Callee],
) {
    inline::inline_calls(function, functions, inlinable);
    loop {
        let mut changed = blocks::fold_constant_switches(function);
        blocks::prune(function);
        // つなげたブロックの引数を定数で置き換えると、つなげなかったブロックの `switch` も定数で分けることがある
        if blocks::merge_jumps(function) {
            changed = true;
            blocks::fold_constant_switches(function);
            blocks::prune(function);
        }
        changed |= known_con::resolve_known(function);
        changed |= known_con::thread_jumps(function);
        blocks::prune(function);
        changed |= known_con::call_known_closures(function, callees);
        changed |= drop_dead_pure_lets(function);
        if !changed {
            break;
        }
    }
}

/// 使われない純粋な `let` を消す。展開した本体が返す値を呼ぶ側が捨てる所で、値を作るだけの
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
    let mut dropped = false;
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
            dropped = true;
            let mut keep = keep.into_iter();
            block.stmts.retain(|_| keep.next() == Some(true));
        }
    }
    dropped
}

/// 直接の呼び出しの関係で再帰する関数と、呼ばれる関数が先に来る関数の順。Tarjan の方法で SCC を求める。Tarjan の
/// 方法は SCC を、そこから届く SCC より後に見つけるので、見つけた順に並べれば呼ばれる関数が先に来る。プログラムの
/// 大きさに比例する手間で済み、深い呼び出しの連なりでもスタックを溢れさせないように、再帰を使わずに書く。
fn call_graph(functions: &[CoreFn]) -> (Vec<bool>, Vec<usize>) {
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
    let mut visit = vec![UNSEEN; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut recursive = vec![false; count];
    let mut order = Vec::with_capacity(count);
    for root in 0..count {
        if visit[root] != UNSEEN {
            continue;
        }
        let mut frames = vec![(root, 0usize)];
        visit[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(frame) = frames.last_mut() {
            let (function, position) = *frame;
            if let Some(&callee) = callees[function].get(position) {
                frame.1 += 1;
                if visit[callee] == UNSEEN {
                    visit[callee] = next;
                    low[callee] = next;
                    next += 1;
                    stack.push(callee);
                    on_stack[callee] = true;
                    frames.push((callee, 0));
                } else if on_stack[callee] {
                    low[function] = low[function].min(visit[callee]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(caller, _)) = frames.last() {
                low[caller] = low[caller].min(low[function]);
            }
            if low[function] == visit[function] {
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
                    for &member in &members {
                        recursive[member] = true;
                    }
                }
                order.extend(members);
            }
        }
    }
    (recursive, order)
}

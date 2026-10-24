//! トップレベルの関数の呼び出しグラフの強連結成分 (docs/spec/types.md の「推論」)。Kind の制約は呼び出しを通じて
//! 伝わるので、呼ばれる側の SCC から順に検査する。

use std::collections::{HashMap, HashSet};

use eml_hir::{ExprKind, FunctionId, Program, Res, ValueItem};

/// SCC を、呼ばれる側が先になる順に返す。
pub(crate) fn components(program: &Program) -> Vec<Vec<FunctionId>> {
    let ids: Vec<FunctionId> = program.functions().map(|(id, _)| id).collect();
    let position: HashMap<FunctionId, usize> =
        ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let edges: Vec<Vec<usize>> = ids
        .iter()
        .map(|&id| {
            callees(program, id)
                .into_iter()
                .map(|callee| position[&callee])
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    tarjan(
        ids.len(),
        |v, edge| edges[v].get(edge).copied(),
        |members| out.push(members.iter().rev().map(|&w| ids[w]).collect()),
    );
    out
}

/// Tarjan の方法で強連結成分を求め、見つけた順 (行き先の成分が先) に `component` へ渡す。節点が多くても Rust の
/// スタックを使わないよう、明示的なスタックでたどる。`succ(v, i)` は `v` の `i` 番目の辺の行き先で、なければ
/// `None` である。成分の節点はスタックから外した順に並ぶ。この順は SCC の検査の順と Kind 変数の番号を決めるので、
/// 呼ぶ側は並べ替えずに使う。
pub(crate) fn tarjan(
    n: usize,
    succ: impl Fn(usize, usize) -> Option<usize>,
    mut component: impl FnMut(&[usize]),
) {
    let unvisited = usize::MAX;
    let mut index = vec![unvisited; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut members = Vec::new();
    let mut next = 0;
    for root in 0..n {
        if index[root] != unvisited {
            continue;
        }
        // (節点, 次に見る辺の位置)
        let mut work = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(v, edge)) = work.last() {
            if let Some(w) = succ(v, edge) {
                work.last_mut().expect("read above").1 += 1;
                if index[w] == unvisited {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                members.clear();
                loop {
                    let w = stack
                        .pop()
                        .expect("the root of a component is on the stack");
                    on_stack[w] = false;
                    members.push(w);
                    if w == v {
                        break;
                    }
                }
                component(&members);
            }
        }
    }
}

/// 本体で参照するトップレベルの関数。呼び出しと値としての参照の両方を数える。`let` の束縛の参照は、束縛を作った入力の
/// 関数への参照として数える。
fn callees(program: &Program, id: FunctionId) -> Vec<FunctionId> {
    let Some(body) = program.body(id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (_, expr) in body.exprs.iter() {
        // `let` の束縛の型は、束縛を作った入力の関数を書き出して決まるので、その入力の関数を先に検査する
        // (docs/spec/types.md の「REPL の入力」)
        let callee = match expr.kind {
            ExprKind::Path(Res::Item(ValueItem::Function(callee))) => callee,
            ExprKind::Path(Res::Item(ValueItem::Binding(binding))) => program[binding].input,
            _ => continue,
        };
        // 並びの順が深さ優先の順を決めるので、集合は重複を除くだけに使う
        if seen.insert(callee) {
            out.push(callee);
        }
    }
    out
}

#[cfg(test)]
mod tests {

    use super::*;

    fn names(text: &str) -> Vec<Vec<String>> {
        let program = crate::test_program(text);
        // Prelude の extern の関数は、それぞれが1つだけの SCC になる。ここではソースの関数だけを見る
        components(&program)
            .into_iter()
            .filter(|component| component.iter().all(|id| id.module == program.entry))
            .map(|component| {
                component
                    .into_iter()
                    .map(|id| program[id].name.clone())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn callees_come_before_callers_and_cycles_stay_together() {
        let text = "a : Int -> Int\na n = b n + c n\n\nb : Int -> Int\nb n = c n\n\nc : Int -> Int\nc n = if n == 0 then 0 else d n\n\nd : Int -> Int\nd n = c (n - 1)";
        assert_eq!(names(text), vec![vec!["c", "d"], vec!["b"], vec!["a"]]);
    }
}

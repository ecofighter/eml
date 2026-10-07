//! トップレベルの関数の呼び出しグラフの強連結成分 (docs/spec/types.md の「推論」)。Kind の制約は呼び出しを通じて
//! 伝わるので、呼ばれる側の SCC から順に検査する。

use std::collections::HashMap;

use eml_hir::{ExprKind, FunctionId, Program, Res, ValueItem};

/// SCC を、呼ばれる側が先になる順に返す。Tarjan の方法で、関数の数が多くても Rust のスタックを使わないように、
/// 明示的なスタックでたどる。
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
    let unvisited = usize::MAX;
    let mut index = vec![unvisited; ids.len()];
    let mut low = vec![0; ids.len()];
    let mut on_stack = vec![false; ids.len()];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut out = Vec::new();
    for root in 0..ids.len() {
        if index[root] != unvisited {
            continue;
        }
        let mut work = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(v, edge)) = work.last() {
            if let Some(&w) = edges[v].get(edge) {
                work.last_mut().unwrap().1 += 1;
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
                let mut component = Vec::new();
                loop {
                    let w = stack.pop().unwrap();
                    on_stack[w] = false;
                    component.push(ids[w]);
                    if w == v {
                        break;
                    }
                }
                component.reverse();
                out.push(component);
            }
        }
    }
    out
}

/// 本体で参照するトップレベルの関数。呼び出しと値としての参照の両方を数える。
fn callees(program: &Program, id: FunctionId) -> Vec<FunctionId> {
    let Some(body) = program.body(id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (_, expr) in body.exprs.iter() {
        if let ExprKind::Path(Res::Item(ValueItem::Function(callee))) = expr.kind
            && !out.contains(&callee)
        {
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

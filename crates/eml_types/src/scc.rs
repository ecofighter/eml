//! トップレベルの関数の呼び出しグラフの強連結成分 (docs/spec/types.md の「推論」)。Kind の制約は呼び出しを通じて
//! 伝わるので、呼ばれる側の SCC から順に検査する。

use eml_hir::{ExprKind, FunctionId, Program, Res, ValueItem};
use rustc_hash::{FxHashMap, FxHashSet};

/// SCC を、呼ばれる側が先になる順に返す。
pub(crate) fn components(program: &Program) -> Vec<Vec<FunctionId>> {
    let ids: Vec<FunctionId> = program.functions().map(|(id, _)| id).collect();
    let position: FxHashMap<FunctionId, usize> =
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
    crate::kind::graph::components(&edges)
        .into_iter()
        .map(|members| members.into_iter().rev().map(|w| ids[w]).collect())
        .collect()
}

/// 本体で参照するトップレベルの関数。呼び出しと値としての参照の両方を数える。`let` の束縛の参照は、束縛を作った入力の
/// 関数への参照として数える。
fn callees(program: &Program, id: FunctionId) -> Vec<FunctionId> {
    let Some(body) = program.body(id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = FxHashSet::default();
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

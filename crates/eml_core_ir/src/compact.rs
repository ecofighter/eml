//! パスの間でアリーナを組み直す (docs/spec/core-ir.md のパスの表)。根から前順 (式、子の順) に式を並べ直し、
//! たどれない式を捨てる。パスは木から外れた式をアリーナに残してよいので、次のパスと verifier が、アリーナの全体が
//! 1本の木だと見なせるようにここで揃える。

use crate::{CExpr, CExprId, CoreFn, JoinId};

/// 式を前順に並べ直し、残った join point に元の番号の順で 0 から番号を振り直して索引を作り直す。
/// 長い連鎖で再帰しないように、作業の列でたどる。
pub(crate) fn compact(function: &mut CoreFn) {
    let mut new_ids: Vec<Option<CExprId>> = vec![None; function.exprs.len()];
    let mut order: Vec<CExprId> = Vec::new();
    let mut work = vec![function.body];
    while let Some(id) = work.pop() {
        debug_assert!(
            new_ids[id.0 as usize].is_none(),
            "an expression is reachable once"
        );
        new_ids[id.0 as usize] = Some(CExprId(order.len() as u32));
        order.push(id);
        let mut children = Vec::new();
        function.exprs[id.0 as usize].for_each_child(|child| children.push(child));
        work.extend(children.into_iter().rev());
    }

    let mut present: Vec<usize> = order
        .iter()
        .filter_map(|&id| match &function.exprs[id.0 as usize] {
            CExpr::Join { join, .. } => Some(join.0 as usize),
            _ => None,
        })
        .collect();
    present.sort_unstable();
    let mut join_numbers: Vec<Option<JoinId>> = vec![None; function.joins.len()];
    for (number, &old) in present.iter().enumerate() {
        join_numbers[old] = Some(JoinId(number as u32));
    }

    let mut exprs: Vec<CExpr> = order
        .iter()
        .map(|&id| function.exprs[id.0 as usize].clone())
        .collect();
    let mut joins = vec![CExprId(0); present.len()];
    for (index, expr) in exprs.iter_mut().enumerate() {
        expr.for_each_child_mut(|child| {
            *child =
                new_ids[child.0 as usize].expect("a child of a reachable expression is reachable");
        });
        if let CExpr::Join { join, .. } | CExpr::Jump { join, .. } = expr {
            *join = join_numbers[join.0 as usize].expect("a jump targets a join point in the tree");
        }
        if let CExpr::Join { join, .. } = expr {
            joins[join.0 as usize] = CExprId(index as u32);
        }
    }
    function.body = CExprId(0);
    function.exprs = exprs;
    function.joins = joins;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::FnBuilder;
    use crate::{Atom, Linearity, VarInfo};

    fn unr(name: &str) -> VarInfo {
        VarInfo {
            name: name.to_string(),
            linearity: Linearity::Unr,
            boxed: false,
        }
    }

    #[test]
    fn compact_drops_unreachable_expressions_and_packs_join_points() {
        let mut builder = FnBuilder::new();
        let x = builder.var(unr("x"));
        let y = builder.var(unr("y"));
        let dead_join = builder.new_join();
        let dead_body = builder.push(CExpr::Return(Atom::Int(0)));
        let dead_scope = builder.push(CExpr::Return(Atom::Int(9)));
        let dead = builder.push(CExpr::Join {
            join: dead_join,
            params: vec![x],
            captures: Vec::new(),
            body: dead_body,
            scope: dead_scope,
        });
        builder.define_join(dead_join, dead);
        let live_join = builder.new_join();
        let live_body = builder.push(CExpr::Return(Atom::Var(y)));
        let scope = builder.push(CExpr::Jump {
            join: live_join,
            args: vec![Atom::Int(1)],
        });
        let live = builder.push(CExpr::Join {
            join: live_join,
            params: vec![y],
            captures: Vec::new(),
            body: live_body,
            scope,
        });
        builder.define_join(live_join, live);
        let mut function = builder.finish("f".to_string(), Vec::new(), live);

        compact(&mut function);

        assert_eq!(function.exprs.len(), 3);
        assert_eq!(function.body, CExprId(0));
        assert_eq!(function.joins.len(), 1);
        let (params, body) = function.join(JoinId(0));
        assert_eq!(params, &[y][..]);
        assert_eq!(function.expr(body), &CExpr::Return(Atom::Var(y)));
        let CExpr::Join { scope, .. } = function.expr(function.body) else {
            panic!("the root is the join point");
        };
        assert!(matches!(
            function.expr(*scope),
            CExpr::Jump {
                join: JoinId(0),
                ..
            }
        ));
    }
}

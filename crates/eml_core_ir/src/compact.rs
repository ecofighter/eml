//! パスの間でアリーナを組み直す (docs/spec/core-ir.md のパスの表)。根から前順 (式、子の順) に式を並べ直し、
//! たどれない式を捨てる。パスは木から外れた式をアリーナに残してよいので、次のパスと verifier が、アリーナの全体が
//! 1本の木だと見なせるようにここで揃える。

use crate::{CExpr, CExprId, CoreFn, JoinId};

/// 式を前順に並べ直し、残った join point に元の番号の順で 0 から番号を振り直して索引を作り直す。
/// 長い連鎖で再帰しないように、作業の列でたどる。
///
/// パスの出力を最初に見るのはこの関数なので、2回たどれる式、木の中に定義のない join point への `jump`、
/// 2回定義された join point を、ここでパスの誤りとして返す。組み直した後のアリーナは木になるので、verifier の木の
/// 検査ではもう見つからない。共有された式が入れ子になると、たどる時間が指数的に増えるので、リリースビルドでも
/// 検査する。誤りを返すときは `function` を書き換えない。
pub(crate) fn compact(function: &mut CoreFn) -> Result<(), String> {
    let mut new_ids: Vec<Option<CExprId>> = vec![None; function.exprs.len()];
    let mut order: Vec<CExprId> = Vec::new();
    let mut work = vec![function.body];
    while let Some(id) = work.pop() {
        if new_ids[id.0 as usize].is_some() {
            return Err(format!("expression e{} is reachable twice", id.0));
        }
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
    if let Some(pair) = present.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(format!("join point j{} is defined twice", pair[0]));
    }
    let table_len = present.last().map_or(0, |&last| last + 1);
    let mut join_numbers: Vec<Option<JoinId>> = vec![None; table_len.max(function.joins.len())];
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
        if let CExpr::Jump { join, .. } = expr {
            let old = *join;
            *join = join_numbers
                .get(old.0 as usize)
                .copied()
                .flatten()
                .ok_or_else(|| format!("jump to j{} has no join point in the tree", old.0))?;
        }
        if let CExpr::Join { join, .. } = expr {
            *join = join_numbers[join.0 as usize].expect("a join point in the tree has a number");
            joins[join.0 as usize] = CExprId(index as u32);
        }
    }
    function.body = CExprId(0);
    function.exprs = exprs;
    function.joins = joins;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::FnBuilder;
    use crate::{Atom, VarInfo};

    fn unboxed(name: &str) -> VarInfo {
        VarInfo {
            name: name.to_string(),
            boxed: false,
        }
    }

    #[test]
    fn compact_drops_unreachable_expressions_and_packs_join_points() {
        let mut builder = FnBuilder::new();
        let x = builder.var(unboxed("x"));
        let y = builder.var(unboxed("y"));
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

        compact(&mut function).unwrap();

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

    #[test]
    fn compact_rejects_an_expression_reachable_twice() {
        let mut builder = FnBuilder::new();
        let x = builder.var(unboxed("x"));
        let shared = builder.push(CExpr::Return(Atom::Int(1)));
        let first = builder.push(CExpr::Dup {
            var: x,
            body: shared,
        });
        let second = builder.push(CExpr::Dup {
            var: x,
            body: shared,
        });
        let join = builder.new_join();
        let root = builder.push(CExpr::Join {
            join,
            params: Vec::new(),
            captures: Vec::new(),
            body: first,
            scope: second,
        });
        builder.define_join(join, root);
        let mut function = builder.finish("f".to_string(), vec![x], root);

        let error = compact(&mut function).unwrap_err();
        assert_eq!(error, "expression e0 is reachable twice");
    }

    #[test]
    fn compact_rejects_a_jump_whose_join_point_is_not_in_the_tree() {
        let mut builder = FnBuilder::new();
        let join = builder.new_join();
        let dead_body = builder.push(CExpr::Return(Atom::Int(0)));
        let dead_scope = builder.push(CExpr::Return(Atom::Int(9)));
        let dead = builder.push(CExpr::Join {
            join,
            params: Vec::new(),
            captures: Vec::new(),
            body: dead_body,
            scope: dead_scope,
        });
        builder.define_join(join, dead);
        let body = builder.push(CExpr::Jump {
            join,
            args: Vec::new(),
        });
        let mut function = builder.finish("f".to_string(), Vec::new(), body);

        let error = compact(&mut function).unwrap_err();
        assert_eq!(error, "jump to j0 has no join point in the tree");
    }

    #[test]
    fn compact_rejects_a_join_point_defined_twice() {
        let mut builder = FnBuilder::new();
        let join = builder.new_join();
        let inner_body = builder.push(CExpr::Return(Atom::Int(0)));
        let inner_scope = builder.push(CExpr::Return(Atom::Int(1)));
        let inner = builder.push(CExpr::Join {
            join,
            params: Vec::new(),
            captures: Vec::new(),
            body: inner_body,
            scope: inner_scope,
        });
        let outer_body = builder.push(CExpr::Return(Atom::Int(2)));
        let outer = builder.push(CExpr::Join {
            join,
            params: Vec::new(),
            captures: Vec::new(),
            body: outer_body,
            scope: inner,
        });
        builder.define_join(join, outer);
        let mut function = builder.finish("f".to_string(), Vec::new(), outer);

        let error = compact(&mut function).unwrap_err();
        assert_eq!(error, "join point j0 is defined twice");
    }
}

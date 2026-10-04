//! RC の対象の変数の生存 (docs/spec/core-ir.md)。Perceus の挿入と verifier が使う。

use std::collections::{BTreeSet, HashMap};

use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Linearity, VarId};

pub(crate) type Vars = BTreeSet<VarId>;

/// RC の対象 (`Unr` でボックス化した変数) かどうか。
pub(crate) fn tracked(function: &CoreFn) -> Vec<bool> {
    function
        .vars
        .iter()
        .map(|var| var.boxed && var.linearity == Linearity::Unr)
        .collect()
}

pub(crate) struct Liveness {
    /// 式ごとの、その式から先で使う RC の対象の変数。`Jump` の先の join point の本体で使う変数も含む。
    pub exprs: Vec<Vars>,
    /// join point ごとの、本体で使う RC の対象の変数 (引数を除く)。`Jump` の時点で、ちょうど1つずつ所有している。
    pub joins: HashMap<JoinId, Vars>,
}

enum Task {
    Visit(CExprId),
    Finish(CExprId),
    Needs {
        join: JoinId,
        param: VarId,
        body: CExprId,
    },
}

/// `Let` の連鎖と、join point の本体の連なりは長くなりうるので、再帰せずに作業の列で後順にたどる。join point の
/// 本体は範囲より先に求める。範囲の中の `Jump` が、本体で使う変数を要るためである。
pub(crate) fn liveness(function: &CoreFn, tracked: &[bool]) -> Liveness {
    let tracked_var = |atom: &Atom| match atom {
        Atom::Var(var) if tracked[var.0 as usize] => Some(*var),
        _ => None,
    };
    let mut exprs = vec![Vars::new(); function.exprs.len()];
    let mut joins: HashMap<JoinId, Vars> = HashMap::new();
    let mut work = vec![Task::Visit(function.body)];
    while let Some(task) = work.pop() {
        match task {
            Task::Visit(id) => {
                work.push(Task::Finish(id));
                match function.expr(id) {
                    CExpr::Let { body, .. }
                    | CExpr::Dup { body, .. }
                    | CExpr::Decref { body, .. } => {
                        work.push(Task::Visit(*body));
                    }
                    CExpr::Join {
                        join,
                        param,
                        body,
                        scope,
                    } => {
                        work.push(Task::Visit(*scope));
                        work.push(Task::Needs {
                            join: *join,
                            param: *param,
                            body: *body,
                        });
                        work.push(Task::Visit(*body));
                    }
                    CExpr::Switch { arms, .. } => {
                        work.extend(arms.iter().map(|&(_, arm)| Task::Visit(arm)));
                    }
                    CExpr::Return(_) | CExpr::Jump { .. } => {}
                }
            }
            Task::Needs { join, param, body } => {
                let mut needs = exprs[body.0 as usize].clone();
                needs.remove(&param);
                joins.insert(join, needs);
            }
            Task::Finish(id) => {
                let vars = match function.expr(id) {
                    CExpr::Let { var, rhs, body } => {
                        let mut vars = exprs[body.0 as usize].clone();
                        vars.remove(var);
                        vars.extend(rhs.atoms().iter().filter_map(tracked_var));
                        vars
                    }
                    CExpr::Dup { var, body } | CExpr::Decref { var, body } => {
                        let mut vars = exprs[body.0 as usize].clone();
                        if tracked[var.0 as usize] {
                            vars.insert(*var);
                        }
                        vars
                    }
                    CExpr::Join { join, scope, .. } => {
                        let mut vars = joins.get(join).cloned().unwrap_or_default();
                        vars.extend(exprs[scope.0 as usize].iter().copied());
                        vars
                    }
                    CExpr::Switch { scrutinee, arms } => {
                        let mut vars: Vars = tracked_var(scrutinee).into_iter().collect();
                        for &(_, arm) in arms {
                            vars.extend(exprs[arm.0 as usize].iter().copied());
                        }
                        vars
                    }
                    CExpr::Return(atom) => tracked_var(atom).into_iter().collect(),
                    // 壊れた Core IR (範囲の外の `Jump`) は verifier が報告するので、ここでは空として扱う
                    CExpr::Jump { join, arg } => {
                        let mut vars = joins.get(join).cloned().unwrap_or_default();
                        vars.extend(tracked_var(arg));
                        vars
                    }
                };
                exprs[id.0 as usize] = vars;
            }
        }
    }
    Liveness { exprs, joins }
}

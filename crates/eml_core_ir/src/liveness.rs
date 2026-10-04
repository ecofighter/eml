//! 変数の生存 (docs/spec/core-ir.md)。`analyze` は、関数ごとに1回、すべての変数について生存を求め、join point の
//! `captures` を埋め直す。持つのはブロックの入口 (`Switch` の枝と join point の範囲) で生きている変数だけで、`Let`
//! ごとの集合は持たない。RC の対象だけの集合が要る側は、結果を RC の対象で絞る。生存は変数ごとに独立して決まるので、
//! 絞った結果は RC の対象だけで求めた結果と一致する。

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

pub(crate) struct BlockLiveness {
    /// `Switch` の枝と join point の範囲の入口で生きている変数。
    entries: HashMap<CExprId, Vars>,
    /// join point ごとの `captures`。
    captures: Vec<Vars>,
}

impl BlockLiveness {
    pub(crate) fn entry(&self, id: CExprId) -> &Vars {
        &self.entries[&id]
    }

    pub(crate) fn captures(&self, join: JoinId) -> &Vars {
        &self.captures[join.0 as usize]
    }

    /// 連鎖を終える式の直前で生きている変数。`Join` では、範囲の入口と同じである。
    pub(crate) fn at_end(&self, expr: &CExpr) -> Vars {
        match expr {
            CExpr::Return(atom) => var_of(atom).into_iter().collect(),
            CExpr::TailCall(call) => call.atoms().iter().filter_map(var_of).collect(),
            // 範囲の外への `Jump` は verifier が報告するので、ここでは求めていない `captures` を空として扱う
            CExpr::Jump { join, arg } => {
                let mut vars = self
                    .captures
                    .get(join.0 as usize)
                    .cloned()
                    .unwrap_or_default();
                vars.extend(var_of(arg));
                vars
            }
            CExpr::Switch { scrutinee, arms } => {
                let mut vars: Vars = var_of(scrutinee).into_iter().collect();
                for &(_, arm) in arms {
                    vars.extend(self.entry(arm).iter().copied());
                }
                vars
            }
            CExpr::Join { scope, .. } => self.entry(*scope).clone(),
            CExpr::Let { .. } | CExpr::Dup { .. } | CExpr::Decref { .. } => {
                unreachable!("a chain ends at a control expression")
            }
        }
    }
}

fn var_of(atom: &Atom) -> Option<VarId> {
    match atom {
        Atom::Var(var) => Some(*var),
        _ => None,
    }
}

/// 連鎖の始まりの種類。join point の本体の入口は表に持たず、`captures` に書く。
#[derive(Clone, Copy)]
enum Start {
    Function,
    Block,
    JoinBody { join: JoinId, param: VarId },
}

enum Step {
    Visit(CExprId, Start),
    Finish {
        start: CExprId,
        kind: Start,
        bindings: Vec<CExprId>,
        end: CExprId,
    },
}

/// `Let` の連鎖と join point の本体の連なりは長くなりうるので、再帰せずに作業の列で後順にたどる。join point の
/// 本体は範囲より先に求める。範囲の中の `Jump` と、範囲の中の join point の本体からの `Jump` が、行き先の
/// `captures` を要るためである。
pub(crate) fn analyze(function: &mut CoreFn) -> BlockLiveness {
    let mut live = BlockLiveness {
        entries: HashMap::new(),
        captures: vec![Vars::new(); function.joins.len()],
    };
    let mut work = vec![Step::Visit(function.body, Start::Function)];
    while let Some(step) = work.pop() {
        match step {
            Step::Visit(start, kind) => {
                let mut bindings = Vec::new();
                let mut id = start;
                while let CExpr::Let { body, .. }
                | CExpr::Dup { body, .. }
                | CExpr::Decref { body, .. } = function.expr(id)
                {
                    bindings.push(id);
                    id = *body;
                }
                work.push(Step::Finish {
                    start,
                    kind,
                    bindings,
                    end: id,
                });
                match function.expr(id) {
                    CExpr::Join {
                        join,
                        param,
                        body,
                        scope,
                        ..
                    } => {
                        work.push(Step::Visit(*scope, Start::Block));
                        work.push(Step::Visit(
                            *body,
                            Start::JoinBody {
                                join: *join,
                                param: *param,
                            },
                        ));
                    }
                    CExpr::Switch { arms, .. } => {
                        work.extend(arms.iter().map(|&(_, arm)| Step::Visit(arm, Start::Block)));
                    }
                    _ => {}
                }
            }
            Step::Finish {
                start,
                kind,
                bindings,
                end,
            } => {
                let mut vars = live.at_end(function.expr(end));
                for &id in bindings.iter().rev() {
                    match function.expr(id) {
                        CExpr::Let { var, rhs, .. } => {
                            vars.remove(var);
                            vars.extend(rhs.atoms().iter().filter_map(var_of));
                        }
                        CExpr::Dup { var, .. } | CExpr::Decref { var, .. } => {
                            vars.insert(*var);
                        }
                        _ => unreachable!("only bindings are collected"),
                    }
                }
                match kind {
                    Start::Function => {}
                    Start::Block => {
                        live.entries.insert(start, vars);
                    }
                    Start::JoinBody { join, param } => {
                        vars.remove(&param);
                        live.captures[join.0 as usize] = vars;
                    }
                }
            }
        }
    }
    for (join, &node) in function.joins.iter().enumerate() {
        if let CExpr::Join { captures, .. } = &mut function.exprs[node.0 as usize] {
            *captures = live.captures[join].iter().copied().collect();
        }
    }
    live
}

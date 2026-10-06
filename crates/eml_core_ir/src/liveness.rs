//! 変数の生存 (docs/spec/core-ir.md)。`analyze` は、1回の呼び出しで関数のすべての変数について生存を求め、join point
//! の `captures` を埋め直す。持つのはブロックの入口 (`Switch` の枝と join point の範囲) で生きている変数だけで、`Let`
//! ごとの集合は持たない。RC の対象だけの集合が要る側は、結果を RC の対象で絞る。生存は変数ごとに独立して決まるので、
//! 絞った結果は RC の対象だけで求めた結果と一致する。

use std::collections::{BTreeSet, HashMap};

use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, VarId};

pub(crate) type Vars = BTreeSet<VarId>;

/// RC の対象 (ボックス化した変数) かどうか。
pub(crate) fn tracked(function: &CoreFn) -> Vec<bool> {
    function.vars.iter().map(|var| var.boxed).collect()
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
            CExpr::Jump { join, args } => {
                let mut vars = self
                    .captures
                    .get(join.0 as usize)
                    .cloned()
                    .unwrap_or_default();
                vars.extend(args.iter().filter_map(var_of));
                vars
            }
            CExpr::Switch { scrutinee, arms } => {
                let mut vars: Vars = var_of(scrutinee).into_iter().collect();
                for arm in arms {
                    // フィールドは枝の入口で束縛するので、`Switch` の前では生きていない
                    vars.extend(
                        self.entry(arm.body)
                            .iter()
                            .copied()
                            .filter(|var| !arm.fields.contains(var)),
                    );
                }
                vars
            }
            // verifier は join point の `captures` が定義の位置で範囲にあることを求める。`jump` が届かない join point でも、
            // `Join` まで生かしておかないと、間の呼び出しをまたいだ変数が退避されずに範囲から外れる
            CExpr::Join {
                join,
                params: _,
                captures: _,
                body: _,
                scope,
            } => {
                let mut vars = self.entry(*scope).clone();
                if let Some(captures) = self.captures.get(join.0 as usize) {
                    vars.extend(captures.iter().copied());
                }
                vars
            }
            CExpr::Let {
                var: _,
                rhs: _,
                body: _,
            }
            | CExpr::Dup { var: _, body: _ }
            | CExpr::Decref { var: _, body: _ } => {
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

/// 連鎖の始まりの種類。join point の本体の入口は表に持たず、`captures` に書く。`JoinBody` の `node` は `Join` の式で、
/// 本体の入口から除く引数をそこから読む。
#[derive(Clone, Copy)]
enum Start {
    Function,
    Block,
    JoinBody { join: JoinId, node: CExprId },
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
                while let CExpr::Let {
                    var: _,
                    rhs: _,
                    body,
                }
                | CExpr::Dup { var: _, body }
                | CExpr::Decref { var: _, body } = function.expr(id)
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
                        params: _,
                        captures: _,
                        body,
                        scope,
                    } => {
                        work.push(Step::Visit(*scope, Start::Block));
                        work.push(Step::Visit(
                            *body,
                            Start::JoinBody {
                                join: *join,
                                node: id,
                            },
                        ));
                    }
                    CExpr::Switch { scrutinee: _, arms } => {
                        work.extend(arms.iter().map(|arm| Step::Visit(arm.body, Start::Block)));
                    }
                    CExpr::Jump { join: _, args: _ } | CExpr::Return(_) | CExpr::TailCall(_) => {}
                    CExpr::Let {
                        var: _,
                        rhs: _,
                        body: _,
                    }
                    | CExpr::Dup { var: _, body: _ }
                    | CExpr::Decref { var: _, body: _ } => {
                        unreachable!("the chain above stops at a control expression")
                    }
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
                        CExpr::Let { var, rhs, body: _ } => {
                            vars.remove(var);
                            vars.extend(rhs.atoms().iter().filter_map(var_of));
                        }
                        CExpr::Dup { var, body: _ } | CExpr::Decref { var, body: _ } => {
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
                    Start::JoinBody { join, node } => {
                        let CExpr::Join {
                            join: _,
                            params,
                            captures: _,
                            body: _,
                            scope: _,
                        } = function.expr(node)
                        else {
                            unreachable!("a join body starts at a join point")
                        };
                        for param in params {
                            vars.remove(param);
                        }
                        live.captures[join.0 as usize] = vars;
                    }
                }
            }
        }
    }
    for join in 0..function.joins.len() {
        let node = function.joins[join];
        if let CExpr::Join {
            join: _,
            params: _,
            captures,
            body: _,
            scope: _,
        } = function.expr_mut(node)
        {
            *captures = live.captures[join].iter().copied().collect();
        }
    }
    live
}

//! join point を書き換える最適化 (docs/spec/core-ir.md)。変換の後、Perceus の前に置く。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。`captures` は古くなりうるが、このパスの後にパイプラインが埋め直す。
//!
//! B2 (分かっているタグ)、B5 (小さな本体)、B3 (jump が1つ)、B4 (使われない) の順に1巡だけ回す。B5 を B4 より先に
//! 回すのは、B5 で jump がなくなった join point を、同じ巡の B4 で消すためである。
//!
//! 書き換えは式のアリーナの上でその場で行う。木から外れた式はアリーナに残り、Perceus がアリーナを作り直すときに
//! 捨てる。そのため、jump の位置と親は、根からたどれる式だけで求める。

use std::collections::HashSet;

use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Linearity, Program, Rhs, VarId, VarInfo};

pub(crate) fn simplify(program: &mut Program) {
    for function in &mut program.functions {
        let mut pass = Simplify {
            function,
            unit_params: HashSet::new(),
        };
        pass.split_known_tags();
        pass.forward_small_bodies();
        pass.inline_single_jumps();
        pass.remove_unused();
        pass.renumber();
    }
}

struct Simplify<'a> {
    function: &'a mut CoreFn,
    /// B2 で作った join point の引数。`()` を受けるだけで本体では使わないので、戻すときに束縛を作らない。
    unit_params: HashSet<VarId>,
}

impl Simplify<'_> {
    fn expr(&self, id: CExprId) -> &CExpr {
        self.function.expr(id)
    }

    fn set(&mut self, id: CExprId, expr: CExpr) {
        self.function.exprs[id.0 as usize] = expr;
    }

    /// 根からたどれる式。長い連鎖で再帰しないように、作業の列でたどる。
    fn reachable(&self) -> Vec<CExprId> {
        let mut order = Vec::new();
        let mut work = vec![self.function.body];
        while let Some(id) = work.pop() {
            order.push(id);
            work.extend(children(self.expr(id)));
        }
        order
    }

    /// join point ごとの、木の中の jump の位置。
    fn jumps(&self) -> Vec<Vec<CExprId>> {
        let mut jumps = vec![Vec::new(); self.function.joins.len()];
        for id in self.reachable() {
            if let CExpr::Jump { join, .. } = self.expr(id) {
                jumps[join.0 as usize].push(id);
            }
        }
        jumps
    }

    /// 木の中の各式の親。根と、木から外れた式は `None` である。
    fn parents(&self) -> Vec<Option<CExprId>> {
        let mut parents = vec![None; self.function.exprs.len()];
        for id in self.reachable() {
            for child in children(self.expr(id)) {
                parents[child.0 as usize] = Some(id);
            }
        }
        parents
    }

    /// join point ごとの、木の中に定義が残っているか。
    fn present(&self) -> Vec<bool> {
        let mut present = vec![false; self.function.joins.len()];
        for id in self.reachable() {
            if let CExpr::Join { join, .. } = self.expr(id) {
                present[join.0 as usize] = true;
            }
        }
        present
    }

    /// 木の中の `old` を `new` で置き換える。`old` の親が `new` を指すようにし、親の表も直す。
    fn replace(&mut self, parents: &mut [Option<CExprId>], old: CExprId, new: CExprId) {
        match parents[old.0 as usize] {
            None => self.function.body = new,
            Some(parent) => replace_child(&mut self.function.exprs[parent.0 as usize], old, new),
        }
        parents[new.0 as usize] = parents[old.0 as usize];
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.function.exprs.push(expr);
        CExprId(self.function.exprs.len() as u32 - 1)
    }

    fn new_unit_param(&mut self) -> VarId {
        self.function.vars.push(VarInfo {
            name: "u".to_string(),
            linearity: Linearity::Unr,
            boxed: false,
        });
        let var = VarId(self.function.vars.len() as u32 - 1);
        self.unit_params.insert(var);
        var
    }

    /// `root` の部分木で、`var` の使用を `atom` に置き換える。
    fn substitute(&mut self, root: CExprId, var: VarId, atom: Atom) {
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            let expr = &mut self.function.exprs[id.0 as usize];
            for slot in expr.atoms_mut() {
                if *slot == Atom::Var(var) {
                    *slot = atom;
                }
            }
            work.extend(children(expr));
        }
    }

    fn known_tag(&self, site: CExprId) -> Option<u32> {
        match self.expr(site) {
            CExpr::Jump {
                arg: Atom::Tag(tag),
                ..
            } => Some(*tag),
            _ => None,
        }
    }

    /// B2: 本体が引数で分岐する join point に定数のタグを jump で渡していれば、各枝を join point に切り出し、定数の
    /// jump を枝へ直接向ける。`&&` と `||` を条件にした `if` がこの形になる (docs/spec/core-ir.md)。
    fn split_known_tags(&mut self) {
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let node = self.function.joins[index];
            let CExpr::Join {
                join,
                param,
                body,
                scope,
                ..
            } = self.expr(node).clone()
            else {
                unreachable!("the join index points at join points")
            };
            let CExpr::Switch {
                scrutinee: Atom::Var(scrutinee),
                arms,
            } = self.expr(body).clone()
            else {
                continue;
            };
            let known: Vec<u32> = sites
                .iter()
                .filter_map(|&site| self.known_tag(site))
                .collect();
            let has_arm = |tag: &u32| arms.iter().any(|&(arm_tag, _)| arm_tag == *tag);
            if scrutinee != param || known.is_empty() || !known.iter().all(has_arm) {
                continue;
            }
            let mut arm_joins = Vec::new();
            for &(tag, arm) in &arms {
                self.substitute(arm, param, Atom::Tag(tag));
                let unit = self.new_unit_param();
                let arm_join = JoinId(self.function.joins.len() as u32);
                // 索引は、下で組み立てた `Join` の位置に直す
                self.function.joins.push(arm);
                arm_joins.push((tag, arm_join, unit, arm));
            }
            let dispatch = arm_joins
                .iter()
                .map(|&(tag, arm_join, _, _)| {
                    let jump = self.push(CExpr::Jump {
                        join: arm_join,
                        arg: Atom::Unit,
                    });
                    (tag, jump)
                })
                .collect();
            self.set(
                body,
                CExpr::Switch {
                    scrutinee: Atom::Var(param),
                    arms: dispatch,
                },
            );
            for &site in sites {
                if let Some(tag) = self.known_tag(site) {
                    let &(_, arm_join, _, _) = arm_joins
                        .iter()
                        .find(|&&(arm_tag, ..)| arm_tag == tag)
                        .expect("checked above");
                    self.set(
                        site,
                        CExpr::Jump {
                            join: arm_join,
                            arg: Atom::Unit,
                        },
                    );
                }
            }
            // 枝の join point を外側に並べ、元の join point をいちばん内側に置く。枝は元の join point の定義全体を
            // 範囲にするので、元の本体からも、範囲の中の定数の jump からも届く。元の位置には最初の枝の join point が入る
            let mut inner = self.push(CExpr::Join {
                join,
                param,
                captures: Vec::new(),
                body,
                scope,
            });
            self.function.joins[index] = inner;
            for (position, &(_, arm_join, unit, arm)) in arm_joins.iter().enumerate().rev() {
                let expr = CExpr::Join {
                    join: arm_join,
                    param: unit,
                    captures: Vec::new(),
                    body: arm,
                    scope: inner,
                };
                inner = if position == 0 {
                    self.set(node, expr);
                    node
                } else {
                    self.push(expr)
                };
                self.function.joins[arm_join.0 as usize] = inner;
            }
        }
    }

    /// B3: jump が1つだけの join point を、その jump の位置に戻す。jump の位置では、本体が使う外側の変数がすべて
    /// 範囲にある。
    fn inline_single_jumps(&mut self) {
        let jumps = self.jumps();
        let mut parents = self.parents();
        for (index, sites) in jumps.iter().enumerate() {
            let &[site] = sites.as_slice() else {
                continue;
            };
            let node = self.function.joins[index];
            let CExpr::Join { param, body, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let (param, body) = (*param, *body);
            let CExpr::Jump { arg, .. } = self.expr(site) else {
                unreachable!("a jump site holds a jump")
            };
            let arg = *arg;
            if self.unit_params.contains(&param) {
                self.replace(&mut parents, site, body);
            } else {
                self.set(
                    site,
                    CExpr::Let {
                        var: param,
                        rhs: Rhs::Atom(arg),
                        body,
                    },
                );
                parents[body.0 as usize] = Some(site);
            }
            // jump が範囲そのものだった場合に備え、範囲は置き換えの後に読み直す
            let CExpr::Join { scope, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let scope = *scope;
            self.replace(&mut parents, node, scope);
        }
    }

    /// B5: 本体が1命令だけの join point への jump を、その命令に置き換える。対象は、値を返す本体と、値を別の join point
    /// へ渡す本体である。別の join point は元の join point を範囲に含むので、jump の位置からも届く。
    fn forward_small_bodies(&mut self) {
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let CExpr::Join { param, body, .. } = self.expr(self.function.joins[index]) else {
                unreachable!("the join index points at join points")
            };
            let (param, body) = (*param, *body);
            let small = match self.expr(body) {
                CExpr::Return(value) if movable(*value, param) => self.expr(body).clone(),
                CExpr::Jump { arg, .. } if movable(*arg, param) => self.expr(body).clone(),
                _ => continue,
            };
            for &site in sites {
                let CExpr::Jump { arg, .. } = self.expr(site) else {
                    unreachable!("a jump site holds a jump")
                };
                let passed = *arg;
                let with = |atom: Atom| {
                    if atom == Atom::Var(param) {
                        passed
                    } else {
                        atom
                    }
                };
                let copy = match &small {
                    CExpr::Return(value) => CExpr::Return(with(*value)),
                    CExpr::Jump { join, arg } => CExpr::Jump {
                        join: *join,
                        arg: with(*arg),
                    },
                    _ => unreachable!("only returns and jumps are small"),
                };
                self.set(site, copy);
            }
        }
    }

    /// B4: どの join point にも、たどれる `jump` が届かなければ、join point を消して範囲だけを残す。まだ入らない
    /// 本体の中の `jump` は数えない。その本体ごと消える join point の `jump` で、別の join point が残らないようにするため。
    fn remove_unused(&mut self) {
        let joins = self.function.joins.len();
        let mut used = vec![false; joins];
        let mut deferred: Vec<Option<CExprId>> = vec![None; joins];
        let mut seen = Vec::new();
        let mut work = vec![self.function.body];
        while let Some(id) = work.pop() {
            match self.expr(id) {
                CExpr::Join {
                    join, body, scope, ..
                } => {
                    let index = join.0 as usize;
                    seen.push(index);
                    work.push(*scope);
                    if used[index] {
                        work.push(*body);
                    } else {
                        deferred[index] = Some(*body);
                    }
                }
                CExpr::Jump { join, .. } => {
                    let index = join.0 as usize;
                    if !used[index] {
                        used[index] = true;
                        work.extend(deferred[index]);
                    }
                }
                other => work.extend(children(other)),
            }
        }
        let mut parents = self.parents();
        for index in seen {
            if used[index] {
                continue;
            }
            let node = self.function.joins[index];
            let CExpr::Join { scope, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let scope = *scope;
            self.replace(&mut parents, node, scope);
        }
    }

    /// 木に残った join point に、元の番号の順を保って 0 から番号を振り直し、索引を作り直す。消した join point が
    /// なければ番号は変わらない。
    fn renumber(&mut self) {
        let present = self.present();
        let mut numbers = vec![None; present.len()];
        let mut joins = Vec::new();
        for (index, &here) in present.iter().enumerate() {
            if here {
                numbers[index] = Some(JoinId(joins.len() as u32));
                joins.push(self.function.joins[index]);
            }
        }
        for id in self.reachable() {
            if let CExpr::Join { join, .. } | CExpr::Jump { join, .. } =
                &mut self.function.exprs[id.0 as usize]
            {
                *join = numbers[join.0 as usize].expect("a jump targets a join point in the tree");
            }
        }
        self.function.joins = joins;
    }
}

fn children(expr: &CExpr) -> Vec<CExprId> {
    match expr {
        CExpr::Let { body, .. } | CExpr::Dup { body, .. } | CExpr::Decref { body, .. } => {
            vec![*body]
        }
        CExpr::Join { body, scope, .. } => vec![*body, *scope],
        CExpr::Switch { arms, .. } => arms.iter().map(|&(_, arm)| arm).collect(),
        CExpr::Return(_) | CExpr::Jump { .. } | CExpr::TailCall(_) => Vec::new(),
    }
}

fn replace_child(expr: &mut CExpr, old: CExprId, new: CExprId) {
    let slots: Vec<&mut CExprId> = match expr {
        CExpr::Let { body, .. } | CExpr::Dup { body, .. } | CExpr::Decref { body, .. } => {
            vec![body]
        }
        CExpr::Join { body, scope, .. } => vec![body, scope],
        CExpr::Switch { arms, .. } => arms.iter_mut().map(|(_, arm)| arm).collect(),
        CExpr::Return(_) | CExpr::Jump { .. } | CExpr::TailCall(_) => Vec::new(),
    };
    let slot = slots
        .into_iter()
        .find(|slot| **slot == old)
        .expect("the parent points at the child");
    *slot = new;
}

/// 本体を jump の位置に写してよい値。引数は渡す値に置き換わり、定数はどこでも同じである。ほかの変数は、写すと
/// その変数の使用が増えるので写さない。
fn movable(atom: Atom, param: VarId) -> bool {
    match atom {
        Atom::Var(var) => var == param,
        _ => true,
    }
}

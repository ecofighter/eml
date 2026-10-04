//! join point を書き換える最適化 (docs/spec/core-ir.md)。変換の後、Perceus の前に置く。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。`captures` は古くなりうるが、Perceus の最初の解析が埋め直す。
//!
//! 書き換えは式のアリーナの上でその場で行う。木から外れた式はアリーナに残り、Perceus がアリーナを作り直すときに
//! 捨てる。そのため、jump の位置と親は、根からたどれる式だけで求める。

use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Program, VarId};

pub(crate) fn simplify(program: &mut Program) {
    for function in &mut program.functions {
        let mut pass = Simplify { function };
        pass.forward_small_bodies();
        pass.remove_unused();
        pass.renumber();
    }
}

struct Simplify<'a> {
    function: &'a mut CoreFn,
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

    /// B4: jump がなくなった join point を消し、範囲だけを残す。
    fn remove_unused(&mut self) {
        let jumps = self.jumps();
        let present = self.present();
        let mut parents = self.parents();
        for (index, sites) in jumps.iter().enumerate() {
            if !sites.is_empty() || !present[index] {
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

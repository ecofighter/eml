//! Perceus の `dup` / `decref` の挿入 (docs/spec/core-ir.md)。変数を使うことを所有権の移動として扱い、後でも使う
//! 変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は `Unr` でボックス化した変数だけである。

use std::collections::{BTreeMap, HashMap};

use crate::liveness::{Liveness, Vars, liveness, tracked};
use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Program, Rhs, VarId};

/// 変換の後に、プログラム全体にかける。変換の途中の関数ごとではなく、独立したパスにする (docs/spec/core-ir.md)。
pub(crate) fn insert(program: &mut Program) {
    for function in &mut program.functions {
        insert_rc(function);
    }
}

fn insert_rc(function: &mut CoreFn) {
    let tracked = tracked(function);
    let Liveness { exprs, joins } = liveness(function, &tracked);
    let mut pass = Pass {
        old: &function.exprs,
        tracked: &tracked,
        free: exprs,
        needs: joins,
        new: Vec::new(),
        joins: vec![None; function.joins.len()],
    };
    let owned: Vars = function
        .params
        .iter()
        .copied()
        .filter(|var| tracked[var.0 as usize])
        .collect();
    let body = pass.transform(function.body, &owned);
    let Pass { new, joins, .. } = pass;
    function.body = body;
    function.exprs = new;
    function.joins = joins
        .into_iter()
        .map(|join| join.expect("every join point is rebuilt"))
        .collect();
}

struct Pass<'a> {
    old: &'a [CExpr],
    tracked: &'a [bool],
    /// 式ごとの、その式から先で使う変数。
    free: Vec<Vars>,
    /// join point ごとの、本体で使う変数。
    needs: HashMap<JoinId, Vars>,
    new: Vec<CExpr>,
    joins: Vec<Option<CExprId>>,
}

impl Pass<'_> {
    fn atom_var(&self, atom: &Atom) -> Option<VarId> {
        match atom {
            Atom::Var(var) if self.tracked[var.0 as usize] => Some(*var),
            _ => None,
        }
    }

    /// 値が使う変数を、使う回数の分だけ並べる。
    fn uses(&self, atoms: &[Atom]) -> Vec<VarId> {
        atoms
            .iter()
            .filter_map(|atom| self.atom_var(atom))
            .collect()
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.new.push(expr);
        CExprId(self.new.len() as u32 - 1)
    }

    /// `owned` は入口で所有している変数。どの経路も `Return` か `Jump` で終わり、その時点で渡すもの以外は所有して
    /// いない。
    ///
    /// `Let` の連鎖と join point の本体の連なりは長くなりうるので、その向きはループで歩いて各段を記録し、最後に
    /// 逆順で組み立てる。再帰するのは `Switch` の枝と join point の範囲だけで、深さは E0013 の入れ子の制限で抑えられる。
    fn transform(&mut self, id: CExprId, owned: &Vars) -> CExprId {
        let old = self.old;
        let mut steps: Vec<Step> = Vec::new();
        let mut id = id;
        let mut owned = owned.clone();
        let mut code = loop {
            match &old[id.0 as usize] {
                CExpr::Return(atom) => break self.transform_return(*atom, &owned),
                CExpr::Jump { join, arg } => break self.transform_jump(*join, *arg, &owned),
                CExpr::Switch { scrutinee, arms } => {
                    let arms = arms
                        .iter()
                        .map(|&(tag, arm)| (tag, self.transform(arm, &owned)))
                        .collect();
                    break self.push(CExpr::Switch {
                        scrutinee: *scrutinee,
                        arms,
                    });
                }
                CExpr::Join {
                    join,
                    param,
                    body,
                    scope,
                } => {
                    // 範囲は今の所有から始まる。本体は、引数と、本体で使う変数を1つずつ所有して始まる
                    let scope = self.transform(*scope, &owned);
                    steps.push(Step::Join {
                        join: *join,
                        param: *param,
                        scope,
                    });
                    owned = self.needs.get(join).cloned().unwrap_or_default();
                    if self.tracked[param.0 as usize] {
                        owned.insert(*param);
                    }
                    id = *body;
                }
                CExpr::Let { var, rhs, body } => {
                    let uses = self.uses(&rhs.atoms());
                    let mut after = self.free[body.0 as usize].clone();
                    after.remove(var);
                    let next: Vars = owned.intersection(&after).copied().collect();
                    steps.push(Step::Let {
                        var: *var,
                        rhs: rhs.clone(),
                        uses,
                        after,
                        owned: std::mem::replace(&mut owned, next),
                    });
                    if self.tracked[var.0 as usize] {
                        owned.insert(*var);
                    }
                    id = *body;
                }
                CExpr::Dup { .. } | CExpr::Decref { .. } => {
                    unreachable!("the pass runs once on code without RC instructions")
                }
            }
        };
        for step in steps.into_iter().rev() {
            match step {
                Step::Join { join, param, scope } => {
                    code = self.push(CExpr::Join {
                        join,
                        param,
                        body: code,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(code);
                }
                Step::Let {
                    var,
                    rhs,
                    uses,
                    after,
                    owned,
                } => {
                    code = self.push(CExpr::Let {
                        var,
                        rhs,
                        body: code,
                    });
                    code = self.release_and_duplicate(code, &owned, &uses, &after);
                }
            }
        }
        code
    }

    /// `code` の前で、後で使わない変数を捨てる。`uses` は使うたびに所有権を1つ受け取るので、2回目以降の使用と、
    /// 後でも使う変数の分を複製する。
    fn release_and_duplicate(
        &mut self,
        mut code: CExprId,
        owned: &Vars,
        uses: &[VarId],
        after: &Vars,
    ) -> CExprId {
        for &dead in owned.iter().rev() {
            if !uses.contains(&dead) && !after.contains(&dead) {
                code = self.push(CExpr::Decref {
                    var: dead,
                    body: code,
                });
            }
        }
        let mut counts: BTreeMap<VarId, usize> = BTreeMap::new();
        for &used in uses {
            *counts.entry(used).or_default() += 1;
        }
        for (&used, &count) in counts.iter().rev() {
            let dups = count - usize::from(!after.contains(&used));
            for _ in 0..dups {
                code = self.push(CExpr::Dup {
                    var: used,
                    body: code,
                });
            }
        }
        code
    }

    fn transform_return(&mut self, atom: Atom, owned: &Vars) -> CExprId {
        let returned = self.atom_var(&atom);
        let mut code = self.push(CExpr::Return(atom));
        for &var in owned.iter().rev() {
            if Some(var) != returned {
                code = self.push(CExpr::Decref { var, body: code });
            }
        }
        code
    }

    /// join point の本体は、本体で使う変数をちょうど1つずつ所有して始まる。それ以外を捨て、渡す値を本体でも使う
    /// なら複製する。
    fn transform_jump(&mut self, join: JoinId, arg: Atom, owned: &Vars) -> CExprId {
        let needs = self.needs.get(&join).cloned().unwrap_or_default();
        let passed = self.atom_var(&arg);
        let mut code = self.push(CExpr::Jump { join, arg });
        for &var in owned.iter().rev() {
            if !needs.contains(&var) && Some(var) != passed {
                code = self.push(CExpr::Decref { var, body: code });
            }
        }
        if let Some(var) = passed.filter(|var| needs.contains(var)) {
            code = self.push(CExpr::Dup { var, body: code });
        }
        code
    }
}

/// `transform` が連鎖を歩いた間に記録する1段分。
enum Step {
    Join {
        join: JoinId,
        param: VarId,
        scope: CExprId,
    },
    Let {
        var: VarId,
        rhs: Rhs,
        uses: Vec<VarId>,
        after: Vars,
        /// この束縛の入口で所有している変数
        owned: Vars,
    },
}

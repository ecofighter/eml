//! Perceus の `dup` / `decref` の挿入 (docs/spec/core-ir.md)。変数を使うことを所有権の移動として扱い、後でも使う
//! 変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は `Unr` でボックス化した変数だけである。

use std::collections::{BTreeMap, BTreeSet};

use crate::{Atom, CExpr, CExprId, CoreFn, Linearity, Program, Rhs, VarId};

type Vars = BTreeSet<VarId>;

/// 変換の後に、プログラム全体にかける。変換の途中の関数ごとではなく、独立したパスにする (docs/spec/core-ir.md)。
pub(crate) fn insert(program: &mut Program) {
    for function in &mut program.functions {
        insert_rc(function);
    }
}

fn insert_rc(function: &mut CoreFn) {
    let tracked: Vec<bool> = function
        .vars
        .iter()
        .map(|var| var.boxed && var.linearity == Linearity::Unr)
        .collect();
    let (body, exprs) = {
        let mut pass = Pass {
            old: &function.exprs,
            tracked: &tracked,
            new: Vec::new(),
            free: Vec::new(),
        };
        let owned: Vars = function
            .params
            .iter()
            .copied()
            .filter(|var| tracked[var.0 as usize])
            .collect();
        pass.compute_free();
        let body = pass.transform(function.body, &owned, &Vars::new());
        (body, pass.new)
    };
    function.body = body;
    function.exprs = exprs;
}

struct Pass<'a> {
    old: &'a [CExpr],
    tracked: &'a [bool],
    new: Vec<CExpr>,
    free: Vec<Vars>,
}

impl Pass<'_> {
    fn atom_var(&self, atom: &Atom) -> Option<VarId> {
        match atom {
            Atom::Var(var) if self.tracked[var.0 as usize] => Some(*var),
            _ => None,
        }
    }

    /// 右辺が使う変数を、使う回数の分だけ並べる。
    fn uses(&self, rhs: &Rhs) -> Vec<VarId> {
        rhs.atoms()
            .iter()
            .filter_map(|atom| self.atom_var(atom))
            .collect()
    }

    /// 全式について、式の中で使う対象の変数を求める。lowering は子を親より先に積む (後順) ので、
    /// 前から1回走査すれば足りる。`Let` の連鎖が長くても再帰しない (docs/spec/grammar.md の深さ制限は逐次の文を数えない)。
    fn compute_free(&mut self) {
        let old = self.old;
        for expr in old {
            let vars = match expr {
                CExpr::Let { var, rhs, body } => {
                    let mut vars = self.free[body.0 as usize].clone();
                    vars.remove(var);
                    match rhs {
                        Rhs::Nested(inner) => vars.extend(self.free[inner.0 as usize].iter()),
                        other => vars.extend(self.uses(other)),
                    }
                    vars
                }
                CExpr::Switch { scrutinee, arms } => {
                    let mut vars: Vars = self.atom_var(scrutinee).into_iter().collect();
                    for &(_, arm) in arms {
                        vars.extend(self.free[arm.0 as usize].iter());
                    }
                    vars
                }
                CExpr::Return(atom) => self.atom_var(atom).into_iter().collect(),
                CExpr::Dup { .. } | CExpr::Decref { .. } => {
                    unreachable!("the pass runs once on code without RC instructions")
                }
            };
            self.free.push(vars);
        }
    }

    fn free(&self, id: CExprId) -> Vars {
        self.free[id.0 as usize].clone()
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.new.push(expr);
        CExprId(self.new.len() as u32 - 1)
    }

    /// `owned` は入口で所有している変数。`keep` は、この式が値を返した後でも使うので、所有したまま残す変数。
    ///
    /// `Let` の連鎖は長くなりうるので、本体へ進む向きはループで歩いて各段を記録し、最後に逆順で組み立てる。
    /// 再帰するのは入れ子の式と `Switch` の枝だけで、深さは E0013 の入れ子の制限で抑えられる。
    fn transform(&mut self, id: CExprId, owned: &Vars, keep: &Vars) -> CExprId {
        let old = self.old;
        let mut steps: Vec<Step> = Vec::new();
        let mut id = id;
        let mut owned = owned.clone();
        let mut code = loop {
            match &old[id.0 as usize] {
                CExpr::Return(atom) => break self.transform_return(*atom, &owned, keep),
                CExpr::Switch { scrutinee, arms } => {
                    let arms = arms
                        .iter()
                        .map(|&(tag, arm)| (tag, self.transform(arm, &owned, keep)))
                        .collect();
                    break self.push(CExpr::Switch {
                        scrutinee: *scrutinee,
                        arms,
                    });
                }
                CExpr::Let {
                    var,
                    rhs: Rhs::Nested(inner),
                    body,
                } => {
                    let mut after = self.free(*body);
                    after.remove(var);
                    after.extend(keep.iter().copied());
                    let keep_inner: Vars = after.intersection(&owned).copied().collect();
                    // 入れ子の式の各枝は、使わない変数を枝の先頭で捨てる
                    let inner = self.transform(*inner, &owned, &keep_inner);
                    steps.push(Step::Nested { var: *var, inner });
                    owned = keep_inner;
                    if self.tracked[var.0 as usize] {
                        owned.insert(*var);
                    }
                    id = *body;
                }
                CExpr::Let { var, rhs, body } => {
                    let uses = self.uses(rhs);
                    let mut after = self.free(*body);
                    after.remove(var);
                    after.extend(keep.iter().copied());
                    let next: Vars = owned.intersection(&after).copied().collect();
                    steps.push(Step::Plain {
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
                Step::Nested { var, inner } => {
                    code = self.push(CExpr::Let {
                        var,
                        rhs: Rhs::Nested(inner),
                        body: code,
                    });
                }
                Step::Plain {
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
                    // 後で使わない変数は、この束縛の前で捨てる
                    for &dead in owned.iter().rev() {
                        if !uses.contains(&dead) && !after.contains(&dead) {
                            code = self.push(CExpr::Decref {
                                var: dead,
                                body: code,
                            });
                        }
                    }
                    // 右辺は使うたびに所有権を1つ受け取るので、2回目以降の使用と、後でも使う変数の分を複製する
                    let mut counts: BTreeMap<VarId, usize> = BTreeMap::new();
                    for &used in &uses {
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
                }
            }
        }
        code
    }

    fn transform_return(&mut self, atom: Atom, owned: &Vars, keep: &Vars) -> CExprId {
        let returned = self.atom_var(&atom);
        let mut code = self.push(CExpr::Return(atom));
        for &var in owned.iter().rev() {
            if !keep.contains(&var) && Some(var) != returned {
                code = self.push(CExpr::Decref { var, body: code });
            }
        }
        // 返す値の所有権は呼び出し側に移るので、後でも使うなら複製する
        if let Some(var) = returned.filter(|var| keep.contains(var)) {
            code = self.push(CExpr::Dup { var, body: code });
        }
        code
    }
}

/// `transform` が連鎖を歩いた間に記録する1段分。
enum Step {
    Nested {
        var: VarId,
        inner: CExprId,
    },
    Plain {
        var: VarId,
        rhs: Rhs,
        uses: Vec<VarId>,
        after: Vars,
        /// この束縛の入口で所有している変数
        owned: Vars,
    },
}

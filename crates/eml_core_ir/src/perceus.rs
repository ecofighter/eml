//! Perceus の `dup` / `decref` の挿入 (docs/spec/core-ir.md)。変数を使うことを所有権の移動として扱い、後でも使う
//! 変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は `Unr` でボックス化した変数だけである。呼び出しの
//! フレームに退避する変数 (`saved`) も、同じ生存の集合からここで決める。

use std::collections::BTreeMap;

use crate::builder::FnBuilder;
use crate::liveness::{BlockLiveness, Vars, analyze, tracked};
use crate::{Arm, Atom, CExpr, CExprId, Call, CoreFn, JoinId, Program, Rhs, VarId};

/// 変換の後に、プログラム全体にかける。変換の途中の関数ごとではなく、独立したパスにする (docs/spec/core-ir.md)。
pub(crate) fn insert(program: &mut Program) {
    for function in &mut program.functions {
        insert_rc(function);
    }
}

fn insert_rc(function: &mut CoreFn) {
    let live = analyze(function);
    let tracked = tracked(function);
    let mut pass = Rebuild {
        old: &function.exprs,
        tracked: &tracked,
        live: &live,
        new: FnBuilder::rebuilding(function.joins.len()),
    };
    let owned: Vars = function
        .params
        .iter()
        .copied()
        .filter(|var| tracked[var.0 as usize])
        .collect();
    let body = pass.transform(function.body, &owned);
    let Rebuild { new, .. } = pass;
    let (exprs, joins) = new.into_arenas();
    function.body = body;
    function.exprs = exprs;
    function.joins = joins;
}

struct Rebuild<'a> {
    old: &'a [CExpr],
    tracked: &'a [bool],
    live: &'a BlockLiveness,
    new: FnBuilder,
}

/// 連鎖の今の段で所有している変数の求め方。
enum Segment {
    /// 連鎖の始まり (関数、`Switch` の枝、join point の範囲と本体の入口)。所有は入口の所有そのものである。
    Start(Vars),
    /// `Let` の後。後でも使う RC の対象はすべて所有し、使わなくなった変数は使った時点で手放しているので、所有は
    /// 生きている RC の対象と、直前に束縛した変数 (使わなくても次の段の前までは所有する) になる。
    After(VarId),
}

impl Rebuild<'_> {
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

    fn tracked_only(&self, vars: &Vars) -> Vars {
        vars.iter()
            .copied()
            .filter(|var| self.tracked[var.0 as usize])
            .collect()
    }

    fn owned(&self, segment: &Segment, live: &Vars) -> Vars {
        match segment {
            Segment::Start(owned) => owned.clone(),
            Segment::After(last) => {
                let mut owned = self.tracked_only(live);
                if self.tracked[last.0 as usize] {
                    owned.insert(*last);
                }
                owned
            }
        }
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.new.push(expr)
    }

    /// `owned` は入口で所有している変数。どの経路も `Return`、`TailCall`、`Jump` で終わり、その時点で渡すもの以外は所有して
    /// いない。
    ///
    /// `Let` の連鎖と join point の本体の連なりは長くなりうるので、その向きはループで歩いて各段を記録し、最後に
    /// 逆順で組み立てる。逆順に組み立てるときに生きている変数の集合を1つだけ更新するので、`Let` ごとの集合を持たない。
    /// 再帰するのは `Switch` の枝と join point の範囲だけで、深さは E0013 の入れ子の制限で抑えられる。
    fn transform(&mut self, id: CExprId, owned: &Vars) -> CExprId {
        let old = self.old;
        let mut steps: Vec<Step> = Vec::new();
        let mut id = id;
        let mut segment = Segment::Start(owned.clone());
        let (mut code, mut live) = loop {
            let expr = &old[id.0 as usize];
            match expr {
                CExpr::Let { var, rhs, body } => {
                    // この束縛の前で捨てうるのは、連鎖の始まりなら入口の所有、そうでなければ直前に束縛した変数だけである
                    let released = match std::mem::replace(&mut segment, Segment::After(*var)) {
                        Segment::Start(owned) => owned,
                        Segment::After(last) => self.tracked[last.0 as usize]
                            .then_some(last)
                            .into_iter()
                            .collect(),
                    };
                    steps.push(Step::Let {
                        var: *var,
                        rhs: rhs.clone(),
                        released,
                    });
                    id = *body;
                }
                CExpr::Join {
                    join,
                    params,
                    captures,
                    body,
                    scope,
                } => {
                    // 範囲は今の所有から始まる。本体は、引数と、`captures` のうち RC の対象を1つずつ所有して始まる
                    let before = self.live.at_end(expr);
                    let owned = self.owned(&segment, &before);
                    let scope = self.transform(*scope, &owned);
                    steps.push(Step::Join {
                        join: *join,
                        params: params.clone(),
                        captures: captures.clone(),
                        scope,
                        before,
                    });
                    let mut owned = self.tracked_only(self.live.captures(*join));
                    owned.extend(
                        params
                            .iter()
                            .copied()
                            .filter(|param| self.tracked[param.0 as usize]),
                    );
                    segment = Segment::Start(owned);
                    id = *body;
                }
                CExpr::Return(atom) => {
                    let live = self.live.at_end(expr);
                    let owned = self.owned(&segment, &live);
                    break (self.transform_return(*atom, &owned), live);
                }
                CExpr::TailCall(call) => {
                    let live = self.live.at_end(expr);
                    let owned = self.owned(&segment, &live);
                    break (self.transform_tail_call(call, &owned), live);
                }
                CExpr::Jump { join, args } => {
                    let live = self.live.at_end(expr);
                    let owned = self.owned(&segment, &live);
                    break (self.transform_jump(*join, args, &owned), live);
                }
                CExpr::Switch { scrutinee, arms } => {
                    let live = self.live.at_end(expr);
                    let mut owned = self.owned(&segment, &live);
                    // `Switch` は scrutinee を1回使う (move)。枝の中でも使うなら、`Switch` の前で複製し、枝はその分を
                    // 所有して始まる。どの枝も使わなければ、枝は scrutinee を所有しない (docs/spec/core-ir.md)
                    let consumed = self.atom_var(scrutinee);
                    let kept = consumed.filter(|var| {
                        arms.iter()
                            .any(|arm| self.live.entry(arm.body).contains(var))
                    });
                    if let (Some(var), None) = (consumed, kept) {
                        owned.remove(&var);
                    }
                    let arms = arms
                        .iter()
                        .map(|arm| {
                            // 枝はフィールドの参照を1つずつ所有して始まる。使わないフィールドは、連鎖の始まりの
                            // 所有として、最初の段で捨てる
                            let mut owned = owned.clone();
                            owned.extend(
                                arm.fields
                                    .iter()
                                    .copied()
                                    .filter(|var| self.tracked[var.0 as usize]),
                            );
                            Arm {
                                tag: arm.tag,
                                fields: arm.fields.clone(),
                                body: self.transform(arm.body, &owned),
                            }
                        })
                        .collect();
                    let mut code = self.push(CExpr::Switch {
                        scrutinee: *scrutinee,
                        arms,
                    });
                    if let Some(var) = kept {
                        code = self.push(CExpr::Dup { var, body: code });
                    }
                    break (code, live);
                }
                CExpr::Dup { .. } | CExpr::Decref { .. } => {
                    unreachable!("the pass runs once on code without RC instructions")
                }
            }
        };
        for step in steps.into_iter().rev() {
            match step {
                Step::Join {
                    join,
                    params,
                    captures,
                    scope,
                    before,
                } => {
                    code = self.push(CExpr::Join {
                        join,
                        params,
                        captures,
                        body: code,
                        scope,
                    });
                    self.new.define_join(join, code);
                    live = before;
                }
                Step::Let { var, rhs, released } => {
                    // `live` は、この束縛の後で生きている変数である。呼び出しは、そのうち結果の変数以外を退避する
                    let rhs = match rhs {
                        Rhs::Call { call, saved: _ } => Rhs::Call {
                            call,
                            saved: live.iter().copied().filter(|&v| v != var).collect(),
                        },
                        rhs => rhs,
                    };
                    let atoms = rhs.atoms();
                    let uses = self.uses(&atoms);
                    code = self.push(CExpr::Let {
                        var,
                        rhs,
                        body: code,
                    });
                    let later = |used: VarId| used != var && live.contains(&used);
                    code = self.release_and_duplicate(code, &released, &uses, later);
                    live.remove(&var);
                    live.extend(atoms.iter().filter_map(|atom| match atom {
                        Atom::Var(v) => Some(*v),
                        _ => None,
                    }));
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
        later: impl Fn(VarId) -> bool,
    ) -> CExprId {
        for &dead in owned.iter().rev() {
            if !uses.contains(&dead) && !later(dead) {
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
            let dups = count - usize::from(!later(used));
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

    /// 末尾呼び出しの後で使う変数はないので、呼び出しが使わない変数をすべて捨てる。
    fn transform_tail_call(&mut self, call: &Call, owned: &Vars) -> CExprId {
        let uses = self.uses(&call.atoms());
        let code = self.push(CExpr::TailCall(call.clone()));
        self.release_and_duplicate(code, owned, &uses, |_| false)
    }

    /// join point の本体は、`captures` のうち RC の対象をちょうど1つずつ所有して始まる。それ以外を捨て、渡す値を
    /// 本体でも使うなら複製する。同じ変数を2つの引数に渡すときは、2つ目の分も複製する。
    fn transform_jump(&mut self, join: JoinId, args: &[Atom], owned: &Vars) -> CExprId {
        let needs = self.tracked_only(self.live.captures(join));
        let uses = self.uses(args);
        let code = self.push(CExpr::Jump {
            join,
            args: args.to_vec(),
        });
        self.release_and_duplicate(code, owned, &uses, |var| needs.contains(&var))
    }
}

/// `transform` が連鎖を歩いた間に記録する1段分。
enum Step {
    Join {
        join: JoinId,
        params: Vec<VarId>,
        captures: Vec<VarId>,
        scope: CExprId,
        /// join point の定義の直前 (範囲の入口) で生きている変数。
        before: Vars,
    },
    Let {
        var: VarId,
        rhs: Rhs,
        /// この束縛の前で捨てうる変数。
        released: Vars,
    },
}

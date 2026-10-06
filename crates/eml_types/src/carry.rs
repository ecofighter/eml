//! 持ち越し規則 (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。呼び出しをまたいで持っている値の Kind と、
//! 呼び出しの row の多重度を組にした持ち越しの制約を出す。使用回数のパス (`usage`) は評価の順を区別しないので、
//! 評価の順に沿った生存の計算を別に持つ。呼び出しの評価の順は `eml_hir::call_steps` に従う。Core IR の変換も同じ手順を
//! 読む (docs/spec/expressions.md の「関数適用」)。

use std::collections::BTreeSet;

use eml_hir::{
    Body, EvalStep, ExprId, ExprKind, LocalId, Module, OpMultiplicity, OperationId, PatId, Res,
    Stmt,
};

use crate::check::{BodyTyping, CallRows};
use crate::kind::{Across, Bound, CallKind, CarriedValue, KindOrigin, KindReason, Provenance};
use crate::table::{Table, Ty};
use crate::ty::Multiplicity;

/// 呼び出しをまたいで持ちうる値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Held {
    Local(LocalId),
    /// 評価済みで消費前の部分式の値。
    Temporary(ExprId),
    /// 呼び出し `ExprId` の矢印 `usize` を適用した結果。値でない後の引数を評価する間に持つ。
    Applied(ExprId, usize),
}

type Live = BTreeSet<Held>;

pub(crate) fn constrain(
    module: &Module,
    body: &Body,
    typing: &BodyTyping,
    table: &mut Table<'_>,
    reliable: bool,
) {
    let mut carrying = Carrying {
        module,
        body,
        typing,
        table,
        reliable,
    };
    carrying.function(body.root);
}

struct Carrying<'a, 'c> {
    module: &'a Module,
    body: &'a Body,
    typing: &'a BodyTyping,
    table: &'a mut Table<'c>,
    reliable: bool,
}

impl Carrying<'_, '_> {
    /// 持ち上げる関数の本体 (関数、ラムダ、handle の本体、節)。本体の後に持つ値はないので、空の集合から始める。
    fn function(&mut self, root: ExprId) {
        self.expr(root, &Live::new());
    }

    /// `after` は、`id` を評価している間と後に持っている値である。`id` の評価の前に持っている値を返す。
    fn expr(&mut self, id: ExprId, after: &Live) -> Live {
        // 参照を写しておき、表を読みながら `&mut self` のメソッドを呼べるようにする
        let body = self.body;
        let typing = self.typing;
        match &body.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) => after.clone(),
            ExprKind::Path(Res::Local(local)) => {
                let mut live = after.clone();
                live.insert(Held::Local(*local));
                live
            }
            ExprKind::Path(_) => after.clone(),
            ExprKind::Call { callee, args, .. } => {
                let rows = match typing.calls.get(id) {
                    Some(CallRows::Call {
                        arrows, performs, ..
                    }) => Some((arrows, *performs)),
                    _ => None,
                };
                let mut live = after.clone();
                // 評価の順は `eml_hir::call_steps` だけが持つ。後で使う値は後ろの手順から決まるので、その順を逆にたどる
                for step in eml_hir::call_steps(self.module, body, id).into_iter().rev() {
                    match step {
                        EvalStep::Arrow(index) => {
                            // 矢印の呼び出しの間は、その結果を除いて、後で使う値と評価済みでまだ渡していない引数を持つ
                            live.remove(&Held::Applied(id, index));
                            if let Some((arrows, performs)) = rows
                                && let Some(row) = arrows.get(index)
                            {
                                let across = match performs {
                                    Some((at, op)) if at == index => Across::Operation(op),
                                    _ => Across::Row(row.clone()),
                                };
                                self.carry(id, &live, &across, &CallKind::Call);
                            }
                            let function = match index {
                                0 => Held::Temporary(*callee),
                                _ => Held::Applied(id, index - 1),
                            };
                            live.insert(function);
                            live.insert(Held::Temporary(args[index]));
                        }
                        EvalStep::Eval(expr) => {
                            live.remove(&Held::Temporary(expr));
                            live = self.expr(expr, &live);
                        }
                    }
                }
                live
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut branches = self.expr(*then_branch, after);
                if let Some(else_branch) = else_branch {
                    branches.extend(self.expr(*else_branch, after));
                }
                self.expr(*condition, &branches)
            }
            ExprKind::Block { stmts, tail, .. } => {
                let mut live = match tail {
                    Some(tail) => self.expr(*tail, after),
                    None => after.clone(),
                };
                for stmt in stmts.iter().rev() {
                    live = match stmt {
                        Stmt::Let { pat, init, .. } => {
                            self.unbind(*pat, &mut live);
                            self.expr(*init, &live)
                        }
                        Stmt::Expr(expr) => self.expr(*expr, &live),
                    };
                }
                live
            }
            ExprKind::Annot { expr, .. } => self.expr(*expr, after),
            ExprKind::Lambda {
                body: lambda_body, ..
            } => {
                self.function(*lambda_body);
                let mut live = after.clone();
                live.extend(body.lambda_captures(id).into_iter().map(Held::Local));
                live
            }
            // handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。捕まえた変数は
            // handle の位置でクロージャに移る
            ExprKind::Handle {
                body: handled,
                clauses,
                ret,
                ..
            } => {
                // handle の後で使う値は、本体の実行のうち外へ抜けるエフェクト (外側の row) をまたぐ。`return` の節の
                // クロージャは handler フレームにあり、扱うエフェクトの `multi` の区間にも入るので、本体の row 全体を
                // またぐ (docs/spec/effects.md の「handler の意味」)
                if let Some(CallRows::Handle {
                    body: body_row,
                    outer,
                }) = typing.calls.get(id)
                {
                    self.carry(id, after, &Across::Row(outer.clone()), &CallKind::Handle);
                    if let Some(ret) = ret {
                        let across = Across::Row(body_row.clone());
                        for local in body.captures(ret.body, &[ret.param]) {
                            let Some(&ty) = typing.locals.get(local) else {
                                continue;
                            };
                            let data = &body.locals[local];
                            let value = CarriedValue::ReturnCapture {
                                name: data.name.clone(),
                                binding: data.range,
                                clause: ret.range,
                            };
                            self.carry_value(id, ty, value, &across, &CallKind::Handle);
                        }
                    }
                }
                self.function(*handled);
                let mut live = after.clone();
                live.extend(body.captures(*handled, &[]).into_iter().map(Held::Local));
                for clause in clauses {
                    self.function(clause.body);
                    let bound: Vec<PatId> = clause.patterns().collect();
                    live.extend(
                        body.captures(clause.body, &bound)
                            .into_iter()
                            .map(Held::Local),
                    );
                }
                if let Some(ret) = ret {
                    self.function(ret.body);
                    live.extend(
                        body.captures(ret.body, &[ret.param])
                            .into_iter()
                            .map(Held::Local),
                    );
                }
                live
            }
            // 再開した継続が外側の `multi` の操作を起こすと、節の手元の値も写される。同じ handler のエフェクトは区間の
            // 中で処理されるので、外側の row だけを見ればよい (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
            ExprKind::Resume { k, arg } => {
                if let Some(CallRows::Resume(row)) = typing.calls.get(id) {
                    let name = match &body.exprs[*k].kind {
                        ExprKind::Path(Res::Local(local)) => Some(body.locals[*local].name.clone()),
                        _ => None,
                    };
                    self.carry(
                        id,
                        after,
                        &Across::Row(row.clone()),
                        &CallKind::Resume { k: name },
                    );
                }
                self.parts(&[*k, *arg], after)
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                let mut branches = after.clone();
                for arm in arms {
                    let mut live = self.expr(arm.body, after);
                    self.unbind(arm.pat, &mut live);
                    branches.extend(live);
                }
                self.expr(*scrutinee, &branches)
            }
            ExprKind::Tuple(elements) => self.parts(elements, after),
            ExprKind::Drop(value) => self.expr(*value, after),
        }
    }

    /// 左から順に評価する部分。ある部分を評価している間は、それより前に評価した部分の値を持っている。
    fn parts(&mut self, parts: &[ExprId], after: &Live) -> Live {
        let mut live = after.clone();
        live.extend(parts.iter().map(|&part| Held::Temporary(part)));
        for &part in parts.iter().rev() {
            live.remove(&Held::Temporary(part));
            live = self.expr(part, &live);
        }
        live
    }

    fn unbind(&self, pat: PatId, live: &mut Live) {
        for local in self.body.pat_bindings(pat) {
            live.remove(&Held::Local(local));
        }
    }

    /// 式 `at` をまたいで `held` を持つ。値ごとに、型の Kind と `across` の多重度の組で持ち越しの制約を出す。
    fn carry(&mut self, at: ExprId, held: &Live, across: &Across, call: &CallKind) {
        for &value in held {
            if let Some((ty, carried)) = self.held(value) {
                self.carry_value(at, ty, carried, across, call);
            }
        }
    }

    fn carry_value(
        &mut self,
        at: ExprId,
        ty: Ty,
        value: CarriedValue,
        across: &Across,
        call: &CallKind,
    ) {
        let mults = match across {
            Across::Row(row) => self.table.row_multiplicities(row),
            Across::Operation(op) => vec![Bound::Const(self.table.operation_multiplicity(*op))],
        };
        if mults
            .iter()
            .all(|mult| matches!(mult, Bound::Const(Multiplicity::Never | Multiplicity::Once)))
        {
            return;
        }
        let origin = if self.reliable {
            Provenance::At(KindOrigin {
                range: self.body.exprs[at].range,
                reason: KindReason::CarriedAcross {
                    value,
                    multi: self.multi_operation(across),
                    call: call.clone(),
                },
            })
        } else {
            Provenance::Suppressed
        };
        let previous = self.table.set_kind_origin(origin);
        self.table.carry(ty, &mults);
        self.table.set_kind_origin(previous);
    }

    /// 報告が指す `multi` の操作。row を解き、`multi` の操作を持つ最初のラベルのエフェクトから、宣言の順で最初の `multi`
    /// の操作を選ぶ。row を束縛するのはこの本体の検査だけで、このパスはその後に動くので、報告のときに解いても同じ結果に
    /// なる。
    fn multi_operation(&self, across: &Across) -> Option<OperationId> {
        let module = self.module;
        match across {
            Across::Operation(op) => Some(*op),
            Across::Row(row) => self.table.resolve_row(row).labels.iter().find_map(|label| {
                module.effects[label.effect]
                    .operations
                    .iter()
                    .copied()
                    .find(|&op| matches!(module.operations[op].multiplicity, OpMultiplicity::Multi))
            }),
        }
    }

    fn held(&self, value: Held) -> Option<(Ty, CarriedValue)> {
        match value {
            Held::Local(local) => {
                let ty = *self.typing.locals.get(local)?;
                let data = &self.body.locals[local];
                Some((
                    ty,
                    CarriedValue::Local {
                        name: data.name.clone(),
                        binding: data.range,
                    },
                ))
            }
            Held::Temporary(expr) => {
                let ty = *self.typing.exprs.get(expr)?;
                Some((ty, CarriedValue::Temporary(self.body.exprs[expr].range)))
            }
            Held::Applied(call, index) => {
                let Some(CallRows::Call { results, .. }) = self.typing.calls.get(call) else {
                    return None;
                };
                let ty = *results.get(index)?;
                let ExprKind::Call { callee, args, .. } = &self.body.exprs[call].kind else {
                    return None;
                };
                let range = self.body.exprs[*callee]
                    .range
                    .cover(self.body.exprs[args[index]].range);
                Some((ty, CarriedValue::Temporary(range)))
            }
        }
    }
}

//! 持ち越し規則 (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。呼び出しをまたいで持っている値の Kind と、
//! 呼び出しの row の多重度を組にした持ち越しの制約を出す。使用回数のパス (`usage`) は評価の順を区別しないので、
//! 評価の順に沿った生存の計算を別に持つ。呼び出しの評価の順は `eml_hir::call_steps` に従う。Core IR の変換も同じ手順を
//! 読む (docs/spec/expressions.md の「関数適用」)。

use std::collections::BTreeSet;

use eml_diagnostics::FileId;
use eml_hir::{
    Body, EvalStep, ExprId, ExprKind, LocalId, OpMultiplicity, OperationId, PatId, Program, Res,
    Stmt,
};

use crate::check::{BodyTyping, CallRows};
use crate::kind::{
    Across, Bound, CallKind, CarriedValue, KindOrigin, KindReason, Provenance, Span,
};
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
    program: &Program,
    file: FileId,
    body: &Body,
    typing: &BodyTyping,
    table: &mut Table<'_>,
    reliable: bool,
) {
    let mut carrying = Carrying {
        program,
        file,
        body,
        typing,
        table,
        reliable,
    };
    carrying.function(body.root);
}

struct Carrying<'a, 'c> {
    program: &'a Program,
    file: FileId,
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
                for step in eml_hir::call_steps(self.program, body, id)
                    .into_iter()
                    .rev()
                {
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
            ExprKind::Lambda(closure) => {
                self.function(closure.body);
                let mut live = after.clone();
                live.extend(body.closure_captures(closure).into_iter().map(Held::Local));
                live
            }
            // handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。捕まえた変数は
            // handle の位置でクロージャに移る
            ExprKind::Handle {
                body: handled,
                init,
                clauses,
                ret,
                effect: _,
            } => {
                // handle の後で使う値は、本体の実行のうち外へ抜けるエフェクト (外側の row) をまたぐ。`return` の節の
                // クロージャは handler フレームにあり、扱うエフェクトの `multi` の区間にも入るので、本体の row 全体を
                // またぐ (docs/spec/effects.md の「handler の意味」)
                if let Some(CallRows::Handle {
                    body: body_row,
                    outer,
                }) = typing.calls.get(id)
                {
                    let outer = Across::Row(outer.clone());
                    self.carry(id, after, &outer, &CallKind::Handle);
                    // 状態は本体の実行中、handler フレームにある。扱うエフェクトの操作が起きると状態は節に渡り、捕まえた
                    // 区間に入らないので、外側の row だけをまたぐ (docs/spec/effects.md の「パラメータ付き handler」)
                    if let Some(init) = init
                        && let Some(&ty) = typing.exprs.get(*init)
                    {
                        let value = CarriedValue::HandlerState {
                            init: body.exprs[*init].range,
                        };
                        self.carry_value(id, ty, value, &outer, &CallKind::Handle);
                    }
                    let across = Across::Row(body_row.clone());
                    for local in body.closure_captures(&ret.closure) {
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
                self.function(handled.body);
                let mut live = after.clone();
                live.extend(body.closure_captures(handled).into_iter().map(Held::Local));
                for clause in clauses {
                    self.function(clause.closure.body);
                    live.extend(
                        body.closure_captures(&clause.closure)
                            .into_iter()
                            .map(Held::Local),
                    );
                }
                self.function(ret.closure.body);
                live.extend(
                    body.closure_captures(&ret.closure)
                        .into_iter()
                        .map(Held::Local),
                );
                // 初期値は handle (クロージャの生成と呼び出し) より先に評価する。初期値の中の呼び出しは、handle の後で
                // 使う値と、handle で捕まえる値をまたぐ (docs/spec/expressions.md の「パラメータ付き handler」)
                match init {
                    Some(init) => self.expr(*init, &live),
                    None => live,
                }
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
                span: Span {
                    file: self.file,
                    range: self.body.exprs[at].range,
                },
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
        let program = self.program;
        match across {
            Across::Operation(op) => Some(*op),
            Across::Row(row) => self.table.resolve_row(row).labels.iter().find_map(|label| {
                program[label.effect]
                    .operations
                    .iter()
                    .copied()
                    .find(|&op| matches!(program[op].multiplicity, OpMultiplicity::Multi))
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

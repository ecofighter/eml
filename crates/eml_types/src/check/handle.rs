//! handle、`resume` の検査 (docs/spec/effects.md の「handler の意味」)。

use eml_hir::{EffectId, ExprId, OpClause, OpMultiplicity, ReturnClause};

use crate::scheme::{Rigids, lower_operation};
use crate::table::{ArrowLin, Label, Row, Tail, Ty, TyShape};
use crate::ty::Linearity;

use super::body::{BodyCheck, CallRows};
use super::report::Origin;

impl BodyCheck<'_> {
    /// handle 式の型は、`return` の節があればその本体の型、なければ本体の型である。本体は今の row `ρ` の前に扱う
    /// エフェクトを足した row で、節と `return` の節は `ρ` で検査する。deep handler の節は handler の外側で動くため。
    pub(super) fn handle(
        &mut self,
        id: ExprId,
        effect: Option<EffectId>,
        handled: ExprId,
        clauses: &[OpClause],
        ret: Option<&ReturnClause>,
    ) -> Ty {
        let Some(effect) = effect else {
            return self.broken_handle(handled, clauses, ret);
        };
        let outer = self.ambient.clone();
        // handle ごとにエフェクトの型引数を新しい変数にする。本体の操作の呼び出しと節が、この変数を通じて型引数を共有する
        let module = self.module;
        let args: Vec<Ty> = module.effects[effect]
            .generics
            .type_vars
            .iter()
            .map(|_| self.table.fresh_var())
            .collect();
        // scoped labels なので、同じエフェクトの handler を入れ子にすると内側が処理する
        let inner = Row {
            labels: std::iter::once(Label {
                effect,
                args: args.clone(),
            })
            .chain(outer.labels.iter().cloned())
            .collect(),
            tail: outer.tail,
        };
        self.typing.calls.insert(
            id,
            CallRows::Handle {
                body: inner.clone(),
                outer: outer.clone(),
            },
        );
        let source = self.ambient_source.clone();
        let handled_ty = self.with_ambient(inner, source, |this| this.infer_expr(handled));
        let result = match ret {
            Some(ret) => {
                self.bind_pat(ret.param, handled_ty);
                self.infer_expr(ret.body)
            }
            None => handled_ty,
        };
        for clause in clauses {
            self.op_clause(clause, result, &outer, &args);
        }
        result
    }

    /// 節の引数は操作の引数の型で、`k` は「操作の結果を受け、handle 式の値を返し、外側の row のエフェクトを起こす」
    /// 継続である。`once` の操作の `k` は `Lin`、`multi` の操作の `k` は `Unr` である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    fn op_clause(&mut self, clause: &OpClause, result: Ty, outer: &Row, effect_args: &[Ty]) {
        let operation = &self.module.operations[clause.op];
        // エフェクトの型引数は handle の型引数である。操作自身の型変数は節の中では rigid である。handler は、操作が
        // どの型で呼ばれても動かなければならないため
        let rigids =
            Rigids::with_effect_args(self.table, &operation.signature.generics, effect_args);
        let mut ty = lower_operation(self.table, operation, &rigids);
        for &param in &clause.params {
            match self.table.shape(ty).clone() {
                TyShape::Fn {
                    param: expected,
                    ret,
                    ..
                } => {
                    self.bind_pat(param, expected);
                    ty = ret;
                }
                _ => {
                    let error = self.table.error;
                    self.bind_pat(param, error);
                    ty = error;
                }
            }
        }
        if let Some(k) = clause.k {
            // `multi` の操作の `k` は何度でも再開でき、捨ててもよい (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
            let lin = match operation.multiplicity {
                OpMultiplicity::Multi => Linearity::Unr,
                OpMultiplicity::Once | OpMultiplicity::Never => Linearity::Lin,
            };
            let continuation = self.table.alloc(TyShape::Cont {
                arg: ty,
                lin: ArrowLin::Known(lin),
                row: outer.clone(),
                ret: result,
            });
            self.bind_pat(k, continuation);
        }
        self.check_expr(clause.body, result, Origin::HandlerClause);
    }

    /// 扱うエフェクトが決まらない handler は HIR が報告済みである。本体のエフェクトをすべて受け入れ、型を `Error` に
    /// して、診断を連鎖させない。
    fn broken_handle(
        &mut self,
        handled: ExprId,
        clauses: &[OpClause],
        ret: Option<&ReturnClause>,
    ) -> Ty {
        let source = self.ambient_source.clone();
        self.with_ambient(Row::error(), source, |this| this.infer_expr(handled));
        let error = self.table.error;
        for clause in clauses {
            for pat in clause.patterns() {
                self.bind_pat(pat, error);
            }
            self.check_expr(clause.body, error, Origin::HandlerClause);
        }
        if let Some(ret) = ret {
            self.bind_pat(ret.param, error);
            self.check_expr(ret.body, error, Origin::HandlerClause);
        }
        error
    }

    /// `resume k v` は、`k` の継続の型を関数型 `a -<ρ'>-> b` のように呼ぶ。`k` の型がまだ決まらない場合 (ラムダの
    /// 引数など) にも検査できるよう、推論用の変数でできた継続の型と単一化する。
    pub(super) fn resume(&mut self, id: ExprId, k: ExprId, arg: ExprId) -> Ty {
        let value = self.table.fresh_var();
        let result = self.table.fresh_var();
        let row = Row {
            labels: Vec::new(),
            tail: Tail::Var(self.table.fresh_row_var()),
        };
        let lin = self.table.fresh_arrow_lin();
        let expected = self.table.alloc(TyShape::Cont {
            arg: value,
            lin,
            row: row.clone(),
            ret: result,
        });
        self.check_expr(k, expected, Origin::Continuation);
        self.check_expr(arg, value, Origin::ResumeValue);
        self.typing.calls.insert(id, CallRows::Resume(row.clone()));
        let range = self.body.exprs[id].range;
        self.include_call_row(row, range, "`resume`", true);
        result
    }
}

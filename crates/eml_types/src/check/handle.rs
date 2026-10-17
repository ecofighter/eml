//! handle の検査 (docs/spec/effects.md の「handler の意味」)。

use eml_diagnostics::TextRange;
use eml_hir::{Closure, EffectId, ExprId, OpClause, OpMultiplicity, PatId, PatKind, ReturnClause};

use crate::kind::{KindReason, OperationSide};
use crate::table::{ArrowLin, Label, Row, Ty, TyShape};
use crate::ty::Linearity;

use super::body::{BodyCheck, CallRows};
use super::report::Origin;

impl BodyCheck<'_, '_> {
    /// handle 式の型は、`return` の節があればその本体の型、なければ本体の型である。本体は今の row `ρ` の前に扱う
    /// エフェクトを足した row で、節と `return` の節は `ρ` で検査する。deep handler の節は handler の外側で動くため。
    pub(super) fn handle(
        &mut self,
        id: ExprId,
        effect: Option<EffectId>,
        init: Option<ExprId>,
        handled: &Closure,
        clauses: &[OpClause],
        ret: &ReturnClause,
    ) -> Ty {
        let Some(effect) = effect else {
            return self.broken_handle(init, handled, clauses, ret);
        };
        // 初期値は本体より先に、handle の外の row で評価する。その型が状態の型 σ である
        // (docs/spec/effects.md の「パラメータ付き handler」)
        let state = init.map(|init| self.infer_expr(init));
        let outer = self.ambient.clone();
        // handle ごとにエフェクトの型引数を新しい変数にする。本体の操作の呼び出しと節が、この変数を通じて型引数を共有する
        let program = self.program;
        let args: Vec<Ty> = program[effect]
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
        let handled_ty = self.with_ambient(inner, source, |this| this.infer_expr(handled.body));
        self.bind_pat(ret.value(), handled_ty);
        self.bind_state(ret.state(), state);
        let result = self.infer_expr(ret.closure.body);
        for clause in clauses {
            self.op_clause(clause, result, &outer, &args, state);
        }
        result
    }

    /// 節の状態の引数を σ で束縛する。HIR は状態のある handler の節にだけ状態の引数を作るので、状態のない handler に
    /// 状態の引数があることはない。あっても型を `Error` にして、診断を連鎖させない。
    fn bind_state(&mut self, pat: Option<PatId>, state: Option<Ty>) {
        if let Some(pat) = pat {
            let ty = state.unwrap_or(self.table.error);
            self.bind_pat(pat, ty);
        }
    }

    /// 節の引数は操作の引数の型で、`k` は「操作の結果を受け、handle 式の値を返し、外側の row のエフェクトを起こす」
    /// 関数である。状態のある handler の `k` は、操作の結果の次に状態を受ける。`once` の操作の `k` の矢印は `Lin`、
    /// `multi` の操作の `k` の矢印は `Unr` である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    fn op_clause(
        &mut self,
        clause: &OpClause,
        result: Ty,
        outer: &Row,
        effect_args: &[Ty],
        state: Option<Ty>,
    ) {
        let operation = &self.program[clause.op];
        // 節の型は、操作の閉じた形にエフェクトの型引数を入れて作る。操作の型の作り方を1か所にするため
        let (mut ty, kinds) = self.signatures.operations[clause.op].instantiate_with_effect_args(
            self.table,
            effect_args,
            operation.arity,
        );
        // 制約は節の中の値ではなく操作の型から出るので、節の頭を指す。値の位置は分からない
        let head = match clause.closure.params.last() {
            Some(&last) => TextRange::new(clause.range.start(), self.body.pats[last].range.end()),
            None => clause.range,
        };
        let reason = |side| KindReason::OperationType {
            name: operation.name.clone(),
            side,
            lambda: None,
        };
        self.with_kind_origin(head, reason(OperationSide::Argument), |this| {
            this.table.fix_linearity(&kinds.params, &[]);
        });
        self.with_kind_origin(head, reason(OperationSide::Result), |this| {
            this.table.fix_linearity(&kinds.results, &kinds.linear);
            this.table.multiplicity_at_least_multi(&kinds.rows);
        });
        for &param in clause.args() {
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
        self.bind_state(clause.state(), state);
        self.expect_fn_clause_body(clause, ty, state);
        if let Some(k) = clause.k() {
            // `multi` の操作の `k` は何度でも再開でき、捨ててもよい (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
            let lin = match operation.multiplicity {
                OpMultiplicity::Multi => Linearity::Unr,
                OpMultiplicity::Once | OpMultiplicity::Never => Linearity::Lin,
            };
            let continuation = match state {
                None => self
                    .table
                    .function_with(ty, ArrowLin::Known(lin), outer.clone(), result),
                Some(sigma) => {
                    // `once` の `k v` は `once` の継続を捕まえるので、2つ目の矢印も `Lin` になる
                    let inner_lin = match lin {
                        Linearity::Lin => ArrowLin::Known(Linearity::Lin),
                        Linearity::Unr => self.table.fresh_arrow_lin(),
                    };
                    let inner = self
                        .table
                        .function_with(sigma, inner_lin, outer.clone(), result);
                    // `k v` は部分適用で何も起こさないので、最初の矢印の row は空の閉じた row にする
                    let continuation =
                        self.table
                            .function_with(ty, ArrowLin::Known(lin), Row::pure(), inner);
                    // `k v` の部分適用は `v` を捕まえるので、2つ目の矢印は `a` の Kind 以上になる。コンストラクタや
                    // ラムダの部分適用と同じ規則である (docs/spec/types.md の「関数型」)
                    self.table.closure_kinds(continuation, 2, &[]);
                    continuation
                }
            };
            if let PatKind::Bind(local) = self.body.pats[k].kind {
                self.continuations.insert(local);
            }
            self.bind_pat(k, continuation);
        }
        self.check_expr(clause.closure.body, result, Origin::HandlerClause);
    }

    /// `fn` の節なら、書いた本体に期待する型を記録する。状態のない handler では操作の結果の型、パラメータ付き handler
    /// では結果と次の状態の組である (docs/spec/expressions.md の「handler」と「パラメータ付き handler」)。
    fn expect_fn_clause_body(&mut self, clause: &OpClause, result: Ty, state: Option<Ty>) {
        let Some(k) = clause.k() else { return };
        let PatKind::Bind(k) = self.body.pats[k].kind else {
            return;
        };
        let Some(fn_clause) = self.body.fn_clauses.iter().find(|c| c.k == k) else {
            return;
        };
        let expected = match state {
            None => result,
            Some(sigma) => self.table.tuple(vec![result, sigma]),
        };
        let origin = Origin::FnClauseBody {
            op: self.program[clause.op].name.clone(),
            stateful: state.is_some(),
        };
        self.fn_clause_bodies
            .insert(fn_clause.body, (expected, origin));
    }

    /// 扱うエフェクトが決まらない handler は HIR が報告済みである。本体のエフェクトをすべて受け入れ、型を `Error` に
    /// して、診断を連鎖させない。
    fn broken_handle(
        &mut self,
        init: Option<ExprId>,
        handled: &Closure,
        clauses: &[OpClause],
        ret: &ReturnClause,
    ) -> Ty {
        if let Some(init) = init {
            self.infer_expr(init);
        }
        let source = self.ambient_source.clone();
        self.with_ambient(Row::error(), source, |this| this.infer_expr(handled.body));
        let error = self.table.error;
        for clause in clauses {
            for &pat in &clause.closure.params {
                self.bind_pat(pat, error);
            }
            self.check_expr(clause.closure.body, error, Origin::HandlerClause);
        }
        for &pat in &ret.closure.params {
            self.bind_pat(pat, error);
        }
        self.check_expr(ret.closure.body, error, Origin::HandlerClause);
        error
    }
}

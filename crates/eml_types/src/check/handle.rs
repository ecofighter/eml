//! handle の検査 (docs/spec/effects.md の「handler の意味」)。

use eml_hir::{Closure, EffectId, ExprId, OpClause, OpMultiplicity, PatId, ReturnClause};

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
        let mut ty = self.signatures.operations[clause.op]
            .instantiate_with_effect_args(self.table, effect_args);
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
            self.bind_pat(k, continuation);
        }
        self.check_expr(clause.closure.body, result, Origin::HandlerClause);
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

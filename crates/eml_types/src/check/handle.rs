//! handle、`resume` の検査 (docs/spec/effects.md の「handler の意味」)。

use eml_diagnostics::{Diagnostic, Label as DiagnosticLabel, TextEdit, TextRange, TextSize};
use eml_hir::{
    Closure, EffectId, ExprId, ExprKind, LocalId, OpClause, OpMultiplicity, PatId, PatKind, Res,
    ReturnClause,
};

use crate::codes;
use crate::table::{ArrowLin, Label, Row, Slot, Tail, Ty, TyShape};
use crate::ty::Linearity;

use super::body::{BodyCheck, CallRows, ClauseFrame};
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
        let state = match init {
            Some(init) => Slot::State(self.infer_expr(init)),
            None => Slot::Stateless,
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
    fn bind_state(&mut self, pat: Option<PatId>, state: Slot) {
        if let Some(pat) = pat {
            let ty = match state {
                Slot::State(sigma) => sigma,
                Slot::Stateless | Slot::Var(_) => self.table.error,
            };
            self.bind_pat(pat, ty);
        }
    }

    /// 節の引数は操作の引数の型で、`k` は「操作の結果を受け、handle 式の値を返し、外側の row のエフェクトを起こす」
    /// 継続である。`once` の操作の `k` は `Lin`、`multi` の操作の `k` は `Unr` である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    fn op_clause(
        &mut self,
        clause: &OpClause,
        result: Ty,
        outer: &Row,
        effect_args: &[Ty],
        state: Slot,
    ) {
        let operation = &self.module.operations[clause.op];
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
            let continuation = self.table.alloc(TyShape::Cont {
                arg: ty,
                lin: ArrowLin::Known(lin),
                row: outer.clone(),
                ret: result,
                state,
            });
            self.bind_pat(k, continuation);
        }
        let frame = ClauseFrame {
            k: clause.k().and_then(|pat| self.bound_variable(pat)),
            state: clause.state().and_then(|pat| self.bound_variable(pat)),
        };
        self.clause_frames.push(frame);
        self.check_expr(clause.closure.body, result, Origin::HandlerClause);
        self.clause_frames.pop();
    }

    /// 変数の束縛 (型の明示を含む) のパターンが束縛する変数。タプルなどの分解は変数1つで表せないので `None` にする。
    fn bound_variable(&self, pat: PatId) -> Option<LocalId> {
        match &self.body.pats[pat].kind {
            PatKind::Bind(local) => Some(*local),
            PatKind::Annot { pat, .. } => self.bound_variable(*pat),
            _ => None,
        }
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

    /// `resume k v` は、`k` の継続の型を関数型 `a -<ρ'>-> b` のように呼ぶ。`k` の型がまだ決まらない場合 (ラムダの
    /// 引数など) にも検査できるよう、推論用の変数でできた継続の型と単一化する。状態の欄も推論用の変数にして、
    /// 引数の数に合う欄と単一化する。
    pub(super) fn resume(
        &mut self,
        id: ExprId,
        k: ExprId,
        arg: ExprId,
        arg_end: TextSize,
        state: Option<ExprId>,
    ) -> Ty {
        let value = self.table.fresh_var();
        let result = self.table.fresh_var();
        let row = Row {
            labels: Vec::new(),
            tail: Tail::Var(self.table.fresh_row_var()),
        };
        let lin = self.table.fresh_arrow_lin();
        let slot = self.table.fresh_slot();
        let expected = self.table.alloc(TyShape::Cont {
            arg: value,
            lin,
            row: row.clone(),
            ret: result,
            state: slot,
        });
        self.check_expr(k, expected, Origin::Continuation);
        // 引数の数と `k` の欄を単一化する。どちらの数が合うかは単一化で決まるので、別の検査は要らない
        // (docs/spec/effects.md の「パラメータ付き handler」)
        let (wanted, next) = match state {
            Some(_) => {
                let sigma = self.table.fresh_var();
                (Slot::State(sigma), Some(sigma))
            }
            None => (Slot::Stateless, None),
        };
        if self.table.unify_slot(slot, wanted).is_err() {
            self.resume_state_mismatch(id, k, arg_end, state);
        }
        // 欄が食い違っても、値と状態の中の誤りは報告する。食い違ったときの σ は新しい変数のままなので、状態の式の
        // 型では誤りにならない
        self.check_expr(arg, value, Origin::ResumeValue);
        if let (Some(state), Some(sigma)) = (state, next) {
            self.check_expr(state, sigma, Origin::ResumeState);
        }
        self.typing.calls.insert(id, CallRows::Resume(row.clone()));
        let range = self.body.exprs[id].range;
        self.include_call_row(row, range, "`resume`", true);
        result
    }

    /// `resume` の引数の数が `k` の状態の欄と合わない (docs/spec/diagnostics.md の E2007)。
    fn resume_state_mismatch(
        &mut self,
        id: ExprId,
        k: ExprId,
        arg_end: TextSize,
        state: Option<ExprId>,
    ) {
        let range = self.body.exprs[id].range;
        let diagnostic = match state {
            None => {
                let diagnostic = Diagnostic::error(
                    codes::RESUME_STATE_MISMATCH,
                    "this continuation comes from a handler with a state, so `resume` needs the next state",
                    DiagnosticLabel::new(self.file(), range, "the next state is missing"),
                )
                .with_help("pass the next state as the third argument: `resume k v st`");
                match self.current_state_for(k, id) {
                    Some(name) => {
                        let title = format!("pass the current state `{name}`");
                        diagnostic.with_fix(
                            title,
                            vec![TextEdit {
                                file: self.file(),
                                range: TextRange::empty(arg_end),
                                replacement: format!(" {name}"),
                            }],
                        )
                    }
                    None => diagnostic,
                }
            }
            Some(_) => {
                // 状態の引数を括弧で囲んでも `resume` の式は `)` で終わるので、式の終わりまで消す
                let to = range.end();
                Diagnostic::error(
                    codes::RESUME_STATE_MISMATCH,
                    "this continuation comes from a handler without a state, so `resume` takes no state",
                    DiagnosticLabel::new(self.file(), range, "the state argument is not expected"),
                )
                .with_help("remove the third argument")
                .with_fix(
                    "remove the state argument",
                    vec![TextEdit {
                        file: self.file(),
                        range: TextRange::new(arg_end, to),
                        replacement: String::new(),
                    }],
                )
            }
        };
        self.diagnostics.push(diagnostic);
    }

    /// `resume` が再開する `k` が、囲む節の `k` そのもので、その節の状態の変数がここで見えるときだけ、状態の変数の
    /// 名前を返す。型検査器には変数のスコープの表がないので、同じ名前の別の変数が状態の引数より後で、`resume` より前に
    /// 束縛されていれば、隠されているかもしれないとして返さない。付けるべき fix を付けないことはあるが、誤った fix は
    /// 付けない。
    fn current_state_for(&self, k: ExprId, resume: ExprId) -> Option<String> {
        let ExprKind::Path(Res::Local(local)) = self.body.exprs[k].kind else {
            return None;
        };
        let frame = self
            .clause_frames
            .iter()
            .rev()
            .find(|frame| frame.k == Some(local))?;
        let state = frame.state?;
        let name = &self.body.locals[state].name;
        let state_start: TextSize = self.body.locals[state].range.start();
        let resume_start = self.body.exprs[resume].range.start();
        let shadowed = self.body.locals.iter().any(|(other, data)| {
            other != state
                && data.name == *name
                && data.range.start() > state_start
                && data.range.start() < resume_start
        });
        (!shadowed).then(|| name.clone())
    }
}

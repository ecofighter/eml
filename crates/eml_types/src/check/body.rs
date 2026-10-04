use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{
    Body, ExprId, ExprKind, Function, FunctionId, Literal, LocalId, Module, PatId, PatKind, Res,
    Stmt,
};
use la_arena::ArenaMap;

use crate::builtins::builtin_type;
use crate::codes;
use crate::scheme::{Rigids, Scheme, lower_type};
use crate::table::{Row, Table, Tail, Ty, TyShape, UnifyError};

use super::report::{AmbientSource, Origin, callee_subject};

/// 1つの本体の推論結果。使用回数のパスも読む。
#[derive(Default)]
pub(crate) struct BodyTyping {
    pub exprs: ArenaMap<ExprId, Ty>,
    pub locals: ArenaMap<LocalId, Ty>,
    /// パターンが受けた値の型。`_` で受けた値の Kind に制約を出すのに使う。
    pub pats: ArenaMap<PatId, Ty>,
}

pub(super) struct BodyCheck<'a> {
    pub(super) module: &'a Module,
    pub(super) function: &'a Function,
    pub(super) body: &'a Body,
    pub(super) rigids: &'a Rigids,
    pub(super) schemes: &'a ArenaMap<FunctionId, Scheme>,
    pub(super) table: &'a mut Table,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    /// 本体が起こしてよいエフェクト。シグネチャで最後にたどった矢印の row か、本体を囲むラムダで最後にたどった
    /// 矢印の row である。
    pub(super) ambient: Row,
    pub(super) ambient_source: AmbientSource,
    pub(super) typing: BodyTyping,
}

/// 式に期待する型。check と infer で `if` とブロックの処理を共有するため。
pub(super) enum Expectation {
    Has(Ty, Origin),
    None,
}

/// 関数型として見たときの最初の矢印。
pub(super) enum Arrow {
    Fn { param: Ty, row: Row, ret: Ty },
    Error,
    NotFunction,
}

impl BodyCheck<'_> {
    pub(super) fn file(&self) -> FileId {
        self.module.file
    }

    pub(super) fn signature_range(&self) -> TextRange {
        self.function
            .signature
            .as_ref()
            .map_or(self.function.name_range, |signature| signature.range)
    }

    /// 等式 `f x y = e` は `fn x -> fn y -> e` と同じなので、引数を1つ消費するごとにシグネチャの矢印を1つたどる。
    /// 本体の row は、最後にたどった矢印の row になる。
    pub(super) fn check_function(&mut self, signature: Ty) {
        let body = self.body;
        let mut expected = signature;
        for (index, &pat) in body.params.iter().enumerate() {
            match self.next_arrow(expected) {
                Arrow::Fn { param, row, ret } => {
                    self.bind_pat(pat, param);
                    self.ambient = row;
                    expected = ret;
                }
                Arrow::Error => {
                    let error = self.table.error;
                    self.bind_pat(pat, error);
                }
                Arrow::NotFunction => {
                    let diagnostic = self.signature_arity_error(pat, index);
                    self.diagnostics.push(diagnostic);
                    let error = self.table.error;
                    for &rest in &body.params[index..] {
                        self.bind_pat(rest, error);
                    }
                    expected = error;
                    break;
                }
            }
        }
        self.check_expr(body.root, expected, Origin::Return);
    }

    fn check_expr(&mut self, id: ExprId, expected: Ty, origin: Origin) {
        let body = self.body;
        let expr = &body.exprs[id];
        match &expr.kind {
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let expectation = Expectation::Has(expected, origin);
                self.if_expr(
                    expr.range,
                    *condition,
                    *then_branch,
                    *else_branch,
                    expectation,
                );
                self.typing.exprs.insert(id, expected);
            }
            ExprKind::Block { stmts, tail } => {
                self.block(expr.range, stmts, *tail, Expectation::Has(expected, origin));
                self.typing.exprs.insert(id, expected);
            }
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                if matches!(
                    self.table.shape(expected),
                    TyShape::Fn { .. } | TyShape::Error
                ) {
                    self.check_lambda(id, params, *lambda_body, expected, origin);
                } else {
                    let found = self.infer_expr(id);
                    self.expect(expr.range, expected, found, &origin);
                }
            }
            _ => {
                let found = self.infer_expr(id);
                self.expect(expr.range, expected, found, &origin);
            }
        }
    }

    fn infer_expr(&mut self, id: ExprId) -> Ty {
        let body = self.body;
        let expr = &body.exprs[id];
        let ty = match &expr.kind {
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let ty = self.table.fresh_var();
                self.check_lambda(id, params, *lambda_body, ty, Origin::Inferred);
                ty
            }
            ExprKind::Missing => self.table.error,
            ExprKind::Literal(Literal::Int(_)) => self.table.int,
            ExprKind::Literal(Literal::String(_)) => self.table.string,
            ExprKind::Literal(Literal::Unit) => self.table.unit,
            ExprKind::Path(res) => self.value(*res),
            ExprKind::Call { callee, args } => self.call(id, *callee, args),
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.if_expr(
                expr.range,
                *condition,
                *then_branch,
                *else_branch,
                Expectation::None,
            ),
            ExprKind::Block { stmts, tail } => {
                self.block(expr.range, stmts, *tail, Expectation::None)
            }
            ExprKind::Annot { expr: inner, ty } => {
                let annotated = lower_type(self.table, self.function, self.rigids, *ty);
                let range = self.function.types[*ty].range;
                self.check_expr(*inner, annotated, Origin::Annotation(range));
                annotated
            }
        };
        self.typing.exprs.insert(id, ty);
        ty
    }

    /// 期待する型があれば、両方の枝をその型で検査する。なければ then 節の型を推論し、else 節をそれに合わせる。
    /// `else` がなければ then 節は `Unit` で、式の型も `Unit` になる (docs/spec/expressions.md の「if」)。
    fn if_expr(
        &mut self,
        range: TextRange,
        condition: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
        expectation: Expectation,
    ) -> Ty {
        let bool = self.table.bool;
        self.check_expr(condition, bool, Origin::IfCondition);
        match (else_branch, expectation) {
            (Some(else_branch), Expectation::Has(expected, origin)) => {
                self.check_expr(then_branch, expected, origin.clone());
                self.check_expr(else_branch, expected, origin);
                expected
            }
            (Some(else_branch), Expectation::None) => {
                let ty = self.infer_expr(then_branch);
                let then_range = self.body.exprs[then_branch].range;
                self.check_expr(else_branch, ty, Origin::IfBranches(then_range));
                ty
            }
            (None, expectation) => {
                let unit = self.table.unit;
                self.check_expr(then_branch, unit, Origin::IfWithoutElse);
                match expectation {
                    Expectation::Has(expected, origin) => {
                        self.expect(range, expected, unit, &origin);
                        expected
                    }
                    Expectation::None => unit,
                }
            }
        }
    }

    /// 最後の文が `let` なら、ブロックの値は `()` である (docs/spec/expressions.md)。
    fn block(
        &mut self,
        range: TextRange,
        stmts: &[Stmt],
        tail: Option<ExprId>,
        expectation: Expectation,
    ) -> Ty {
        self.stmts(stmts);
        match (tail, expectation) {
            (Some(tail), Expectation::Has(expected, origin)) => {
                self.check_expr(tail, expected, origin);
                expected
            }
            (Some(tail), Expectation::None) => self.infer_expr(tail),
            (None, Expectation::Has(expected, origin)) => {
                let unit = self.table.unit;
                self.expect(range, expected, unit, &origin);
                expected
            }
            (None, Expectation::None) => self.table.unit,
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, ty, init } => {
                    let ty = match ty {
                        Some(ty) => {
                            let annotated = lower_type(self.table, self.function, self.rigids, *ty);
                            let range = self.function.types[*ty].range;
                            self.check_expr(*init, annotated, Origin::Annotation(range));
                            annotated
                        }
                        None => self.infer_expr(*init),
                    };
                    self.bind_pat(*pat, ty);
                }
                Stmt::Expr(expr) => {
                    let unit = self.table.unit;
                    self.check_expr(*expr, unit, Origin::Statement);
                }
            }
        }
    }

    /// トップレベルの関数を参照するたびに、スキームを具体化する (docs/spec/types.md の「推論」)。
    fn reference(&mut self, function: FunctionId) -> Ty {
        let schemes = self.schemes;
        match schemes.get(function) {
            Some(scheme) => scheme.instantiate(self.table),
            None => self.table.error,
        }
    }

    /// 関数と組み込みの参照は、具体化した後に戻り値の側の閉じた row を開く。純粋な関数を、エフェクトを持つ関数型の
    /// 引数に渡せるようにするため (docs/spec/types.md の「推論」)。局所変数の型は開かない。
    fn value(&mut self, res: Res) -> Ty {
        match res {
            Res::Local(local) => self
                .typing
                .locals
                .get(local)
                .copied()
                .unwrap_or(self.table.error),
            Res::Function(function) => {
                let ty = self.reference(function);
                self.table.open_spine(ty)
            }
            Res::Builtin(builtin) => {
                let ty = builtin_type(self.table, builtin);
                self.table.open_spine(ty)
            }
        }
    }

    /// 呼ばれる値や期待する型がまだ推論用の変数のとき、それを関数型に決める。
    /// 推論用の変数なら、新しい関数型と単一化してから矢印を返す。呼び出しとラムダで、型の決まっていない値を関数として
    /// 使えるようにするため。シグネチャの型は推論用の変数を含まないので、等式の検査でも同じ関数を使える。
    fn next_arrow(&mut self, ty: Ty) -> Arrow {
        if let TyShape::Var(_) = self.table.shape(ty) {
            let function = self.fresh_function();
            // 新しい変数だけでできた関数型なので、単一化は失敗しない
            let unified = self.table.unify(ty, function);
            debug_assert!(
                unified.is_ok(),
                "a fresh function type always unifies with a variable"
            );
        }
        match self.table.shape(ty).clone() {
            TyShape::Fn {
                param, row, ret, ..
            } => Arrow::Fn { param, row, ret },
            TyShape::Error => Arrow::Error,
            _ => Arrow::NotFunction,
        }
    }

    fn fresh_function(&mut self) -> Ty {
        let param = self.table.fresh_var();
        let ret = self.table.fresh_var();
        let lin = self.table.fresh_arrow_lin();
        let row = Row {
            labels: Vec::new(),
            tail: Tail::Var(self.table.fresh_row_var()),
        };
        self.table.function_with(param, lin, row, ret)
    }

    /// 等式と同じく、引数を1つ受けるごとに型の矢印を1つたどる。たどった矢印の row はすべて今の row に含まれなければ
    /// ならない。矢印が余れば部分適用で、残りの関数型が値の型になる (docs/spec/expressions.md の「ラムダ」)。
    fn call(&mut self, id: ExprId, callee: ExprId, args: &[ExprId]) -> Ty {
        let body = self.body;
        let callee_expr = &body.exprs[callee];
        let name = callee_subject(self.module, body, callee);
        let mut ty = self.infer_expr(callee);
        // 1回の呼び出しの E2002 は、どの引数の矢印で起きても1つだけ報告する
        let mut reported = false;
        for (index, &arg) in args.iter().enumerate() {
            match self.next_arrow(ty) {
                Arrow::Fn { param, row, ret } => {
                    let origin = Origin::Argument {
                        callee: callee_expr.range,
                        name: name.clone(),
                        index,
                    };
                    self.check_expr(arg, param, origin);
                    let ok = self.include_call_row(row, body.exprs[id].range, &name, !reported);
                    reported |= !ok;
                    ty = ret;
                }
                Arrow::Error => {
                    self.infer_expr(arg);
                }
                Arrow::NotFunction => {
                    let diagnostic = self.call_arity_error(&name, index, args.len(), arg);
                    self.diagnostics.push(diagnostic);
                    for &rest in &args[index..] {
                        self.infer_expr(rest);
                    }
                    return self.table.error;
                }
            }
        }
        ty
    }

    /// `check` の間だけ今の row とその由来を替え、終わったら戻す。ラムダの本体は、外側の関数ではなく、ラムダで最後に
    /// たどった矢印の row で検査する (docs/spec/types.md の「推論」)。
    fn with_ambient<T>(
        &mut self,
        row: Row,
        source: AmbientSource,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved_row = std::mem::replace(&mut self.ambient, row);
        let saved_source = std::mem::replace(&mut self.ambient_source, source);
        let result = check(self);
        self.ambient = saved_row;
        self.ambient_source = saved_source;
        result
    }

    /// ラムダを、期待する関数型の矢印を引数ごとにたどって検査する。本体のエフェクトは、外側の関数ではなく、最後に
    /// たどった矢印の row に入る (docs/spec/types.md の「推論」)。
    fn check_lambda(
        &mut self,
        id: ExprId,
        params: &[PatId],
        lambda_body: ExprId,
        expected: Ty,
        origin: Origin,
    ) {
        let mut current = expected;
        let mut row = None;
        for (index, &pat) in params.iter().enumerate() {
            match self.next_arrow(current) {
                Arrow::Fn {
                    param,
                    row: arrow,
                    ret,
                } => {
                    self.bind_param(pat, param);
                    row = Some(arrow);
                    current = ret;
                }
                Arrow::Error => {
                    let error = self.table.error;
                    self.bind_pat(pat, error);
                    row = None;
                }
                Arrow::NotFunction => {
                    let diagnostic = self.lambda_arity_error(expected, params.len(), pat, index);
                    self.diagnostics.push(diagnostic);
                    let error = self.table.error;
                    for &rest in &params[index..] {
                        self.bind_pat(rest, error);
                    }
                    current = error;
                    // 矢印が足りないときは最後の矢印の row を使わず、本体のエフェクトを受け入れて診断を連鎖させない
                    row = None;
                    break;
                }
            }
        }
        // 期待する型が壊れていれば、どのエフェクトも受け入れて診断を連鎖させない
        let row = row.unwrap_or_else(Row::error);
        self.with_ambient(row, AmbientSource::Lambda(origin), |this| {
            this.check_expr(lambda_body, current, Origin::LambdaBody)
        });
        self.typing.exprs.insert(id, expected);
    }

    /// ラムダの引数を束縛する。型を明示した引数は、明示した型が、ラムダが期待される引数の型と一致しなければならない。
    fn bind_param(&mut self, pat: PatId, ty: Ty) {
        let body = self.body;
        if let PatKind::Annot {
            pat: inner,
            ty: annotation,
        } = &body.pats[pat].kind
        {
            let annotated = lower_type(self.table, self.function, self.rigids, *annotation);
            self.expect(
                body.pats[pat].range,
                ty,
                annotated,
                &Origin::LambdaParameter,
            );
            self.bind_pat(*inner, annotated);
            return;
        }
        self.bind_pat(pat, ty);
    }

    fn bind_pat(&mut self, pat: PatId, ty: Ty) {
        self.typing.pats.insert(pat, ty);
        let body = self.body;
        match &body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.typing.locals.insert(*local, ty);
            }
            PatKind::Annot { pat, .. } => self.bind_pat(*pat, ty),
            PatKind::Wildcard | PatKind::Missing => {}
            PatKind::Unit => {
                let unit = self.table.unit;
                self.expect(body.pats[pat].range, ty, unit, &Origin::UnitPattern);
            }
        }
    }

    fn expect(&mut self, range: TextRange, expected: Ty, found: Ty, origin: &Origin) {
        match self.table.unify(found, expected) {
            Ok(()) => {}
            Err(UnifyError::Occurs) => self.diagnostics.push(Diagnostic::error(
                codes::INFINITE_TYPE,
                "this expression would have an infinite type",
                Label::new(self.file(), range, "infinite type"),
            )),
            Err(_) => self.mismatch(range, expected, found, origin),
        }
    }
}

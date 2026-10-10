use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{
    Body, Closure, ConstructorId, EffectId, ExprId, ExprKind, Function, Literal, LocalId, MatchArm,
    OperationId, PatId, PatKind, Program, Res, RowRef, Stmt, TypeRefId, TypeRefKind, ValueItem,
};
use la_arena::ArenaMap;

use crate::codes;
use crate::kind::problem::Instance;
use crate::kind::{KindOrigin, KindReason, Provenance, Span};
use crate::shape::{Instantiated, Rigids, lower_type};
use crate::store::TypeStore;
use crate::table::{Row, Table, Tail, Ty, TyShape, UnifyError};

use super::Signatures;
use super::report::{AmbientSource, Origin, callee_subject};

/// 呼び出しの row。持ち越しのパスが読む (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
#[derive(Debug, Clone)]
pub(crate) enum CallRows {
    /// 矢印ごとの row。トップレベルの値を呼ぶときは、開く前の宣言の row である。`performs` は、操作を起こす矢印の
    /// 番号とその操作である。
    Call {
        arrows: Vec<Row>,
        /// 矢印ごとの、適用した結果の型。持ち越しのパスが、後の引数を評価する間に持つ値の Kind を求める。
        results: Vec<Ty>,
        performs: Option<(usize, OperationId)>,
    },
    /// handle。本体の row と、外側の row。
    Handle { body: Row, outer: Row },
}

/// 1つの本体の推論結果。使用回数のパス (`usage`) と持ち越しのパス (`carry`) が読む。
#[derive(Default)]
pub(crate) struct BodyTyping {
    pub exprs: ArenaMap<ExprId, Ty>,
    pub locals: ArenaMap<LocalId, Ty>,
    /// パターンが受けた値の型。`_` で受けた値の Kind に制約を出すのに使う。
    pub pats: ArenaMap<PatId, Ty>,
    /// 参照ごとの具体化。型引数は表の変数のままで、`check_body` が carry の後で書き出す。
    pub instantiations: ArenaMap<ExprId, (ValueItem, Vec<Ty>)>,
    /// 呼び出しの row。持ち越しのパスが読む。
    pub calls: ArenaMap<ExprId, CallRows>,
    /// 呼び出しの矢印ごとの `mask`。キーは呼び出しの式と矢印の番号である。空の `mask` は入れない
    /// (docs/spec/types.md の「推論」)。
    pub masks: HashMap<(ExprId, usize), Vec<EffectId>>,
}

pub(super) struct BodyCheck<'a, 'c> {
    pub(super) program: &'a Program,
    /// 検査している本体のモジュールのファイル。診断が指す。
    pub(super) file: FileId,
    pub(super) function: &'a Function,
    pub(super) body: &'a Body,
    pub(super) rigids: &'a Rigids,
    /// 全宣言の閉じた型の形。本体の検査が呼び出し先について見るのは、これだけである (docs/spec/types.md の「推論」)。
    pub(super) signatures: &'a Signatures,
    pub(super) table: &'a mut Table<'c>,
    /// 書き出した型を登録する表。本体の検査が書き出すのは診断の文言に使う型だけである。
    pub(super) types: &'a mut TypeStore,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    /// 本体が起こしてよいエフェクト。シグネチャで最後にたどった矢印の row か、本体を囲むラムダで最後にたどった
    /// 矢印の row である。
    pub(super) ambient: Row,
    pub(super) ambient_source: AmbientSource,
    pub(super) typing: BodyTyping,
    /// Kind の具体化の記録 (`Instance`)。段2が展開する。
    pub(super) instances: Vec<Instance>,
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

impl BodyCheck<'_, '_> {
    pub(super) fn file(&self) -> FileId {
        self.file
    }

    pub(super) fn signature_range(&self) -> TextRange {
        self.function
            .signature
            .as_ref()
            .map_or(self.function.name_range, |signature| signature.range)
    }

    /// 本体の row が入る矢印の部分の型の範囲。シグネチャの型を、引数の数より1つ少ない回数だけ戻り値の側にたどる。
    /// たどれないときと引数が1つ以下のときは、シグネチャの型全体を指す (docs/implementation/diagnostics.md の E2002)。
    pub(super) fn body_arrow_range(&self) -> TextRange {
        let Some(signature) = &self.function.signature else {
            return self.function.name_range;
        };
        match self.body_arrow() {
            Some(id) if self.body.params.len() > 1 => signature.types[id].range,
            _ => signature.range,
        }
    }

    /// シグネチャに書いた本体の row が、row 変数の手前に `effect` を並べているか。handle の本体の今の row には
    /// handle が足したラベルも入るので、今の row ではなくシグネチャを読む (docs/implementation/diagnostics.md の E2008)。
    pub(super) fn signature_row_lists(&self, effect: EffectId) -> bool {
        let (Some(signature), Some(id)) = (&self.function.signature, self.body_arrow()) else {
            return false;
        };
        match &signature.types[id].kind {
            TypeRefKind::Fn {
                row: RowRef::Open { effects, .. },
                ..
            } => effects.iter().any(|listed| listed.effect == effect),
            _ => false,
        }
    }

    /// 本体の row を持つシグネチャの矢印。引数がないときと、引数の数だけ矢印をたどれないときは `None` である。
    fn body_arrow(&self) -> Option<TypeRefId> {
        let signature = self.function.signature.as_ref()?;
        let mut id = signature.ty;
        for _ in 1..self.body.params.len() {
            match &signature.types[id].kind {
                TypeRefKind::Fn { ret, .. } => id = *ret,
                _ => return None,
            }
        }
        let is_arrow = matches!(signature.types[id].kind, TypeRefKind::Fn { .. });
        (is_arrow && !self.body.params.is_empty()).then_some(id)
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
                    // 期待する型が壊れていれば、どのエフェクトも受け入れて診断を連鎖させない (ラムダの検査と同じ)
                    self.ambient = Row::error();
                }
                Arrow::NotFunction => {
                    let diagnostic = self.signature_arity_error(pat, index);
                    self.diagnostics.push(diagnostic);
                    let error = self.table.error;
                    for &rest in &body.params[index..] {
                        self.bind_pat(rest, error);
                    }
                    expected = error;
                    // 期待する型が壊れていれば、どのエフェクトも受け入れて診断を連鎖させない (ラムダの検査と同じ)
                    self.ambient = Row::error();
                    break;
                }
            }
        }
        self.check_expr(body.root, expected, Origin::Return);
    }

    pub(super) fn check_expr(&mut self, id: ExprId, expected: Ty, origin: Origin) {
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
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.match_expr(*scrutinee, arms, Expectation::Has(expected, origin));
                self.typing.exprs.insert(id, expected);
            }
            ExprKind::Block { stmts, tail, .. } => {
                self.block(expr.range, stmts, *tail, Expectation::Has(expected, origin));
                self.typing.exprs.insert(id, expected);
            }
            ExprKind::List(elements) => match self.list_element(expected) {
                Some(element_ty) => {
                    for &element in elements {
                        self.check_expr(element, element_ty, origin.clone());
                    }
                    self.typing.exprs.insert(id, expected);
                }
                None => {
                    let found = self.infer_expr(id);
                    self.expect(expr.range, expected, found, &origin);
                }
            },
            ExprKind::Lambda(Closure {
                params,
                body: lambda_body,
            }) => {
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

    /// 型が `List t` の形に決まっていれば、要素の型 `t` を返す。
    fn list_element(&self, ty: Ty) -> Option<Ty> {
        match self.table.shape(ty) {
            TyShape::Con(list, args) if *list == self.program.lang.list && args.len() == 1 => {
                Some(args[0])
            }
            _ => None,
        }
    }

    /// 期待する型のないリスト (docs/spec/expressions.md の「リスト」)。最初の要素の型を推論し、残りをそれに合わせる。
    /// `List t` は、コンストラクタのパターンと同じく `Nil` のスキームを `KindReason::Unified` で具体化して作る。
    /// Kind の誤りの由来を、コンストラクタで組む値とそろえるためである。
    fn list(&mut self, range: TextRange, elements: &[ExprId]) -> Ty {
        let nil = ValueItem::Constructor(self.program.lang.nil);
        let instantiated =
            self.with_kind_origin(range, KindReason::Unified, |this| this.instantiate(nil));
        let Some((list, _)) = instantiated else {
            return self.table.error;
        };
        let TyShape::Con(_, args) = self.table.shape(list).clone() else {
            unreachable!("`Nil` makes a `List`")
        };
        let element = args[0];
        if let Some((&first, rest)) = elements.split_first() {
            let found = self.infer_expr(first);
            let first_range = self.body.exprs[first].range;
            self.expect(first_range, element, found, &Origin::Inferred);
            for &later in rest {
                self.check_expr(later, element, Origin::ListElements(first_range));
            }
        }
        list
    }

    pub(super) fn infer_expr(&mut self, id: ExprId) -> Ty {
        let body = self.body;
        let expr = &body.exprs[id];
        let ty = match &expr.kind {
            ExprKind::Lambda(Closure {
                params,
                body: lambda_body,
            }) => {
                let ty = self.table.fresh_var();
                self.check_lambda(id, params, *lambda_body, ty, Origin::Inferred);
                ty
            }
            ExprKind::Missing => self.table.error,
            ExprKind::Literal(Literal::Int(_)) => self.table.int,
            ExprKind::Literal(Literal::String(_)) => self.table.string,
            ExprKind::Literal(Literal::Unit) => self.table.unit,
            ExprKind::Path(res) => self.path(id, *res, expr.range, true),
            ExprKind::Call { callee, args, .. } => self.call(id, *callee, args),
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
            ExprKind::Block { stmts, tail, .. } => {
                self.block(expr.range, stmts, *tail, Expectation::None)
            }
            ExprKind::Annot { expr: inner, ty } => {
                let annotated = lower_type(self.table, &self.body.types, self.rigids, *ty);
                let range = self.body.types[*ty].range;
                self.check_expr(*inner, annotated, Origin::Annotation(range));
                annotated
            }
            ExprKind::Handle {
                body: handled,
                init,
                effect,
                clauses,
                ret,
            } => self.handle(id, *effect, *init, handled, clauses, ret),
            ExprKind::Match {
                scrutinee, arms, ..
            } => self.match_expr(*scrutinee, arms, Expectation::None),
            ExprKind::Tuple(elements) => {
                let fields = elements
                    .iter()
                    .map(|&element| self.infer_expr(element))
                    .collect();
                self.table.tuple(fields)
            }
            ExprKind::List(elements) => self.list(expr.range, elements),
            // `drop` はどんな値も受け取る。値を捨てることは使用の1回に数える (docs/spec/linearity.md)
            ExprKind::Drop(value) => {
                self.infer_expr(*value);
                self.table.unit
            }
        };
        self.typing.exprs.insert(id, ty);
        ty
    }

    /// 期待する型があれば、両方の枝をその型で検査する。なければ then 節の型を推論し、else 節をそれに合わせる。
    /// `else` がなければ then 節は `Unit` で、式の型も `Unit` になる (docs/spec/expressions.md の「`if`」)。
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

    /// 各枝のパターンを scrutinee の型で検査する。期待する型があれば枝の本体をその型で検査し、なければ最初の枝の型を
    /// 推論して後の枝をそれに合わせる (`if` と同じ)。
    fn match_expr(&mut self, scrutinee: ExprId, arms: &[MatchArm], expectation: Expectation) -> Ty {
        let scrutinee_ty = self.infer_expr(scrutinee);
        for arm in arms {
            self.bind_pat(arm.pat, scrutinee_ty);
        }
        match expectation {
            Expectation::Has(expected, origin) => {
                for arm in arms {
                    self.check_expr(arm.body, expected, origin.clone());
                }
                expected
            }
            Expectation::None => {
                let Some((first, rest)) = arms.split_first() else {
                    return self.table.fresh_var();
                };
                let ty = self.infer_expr(first.body);
                let first_range = self.body.exprs[first.body].range;
                for arm in rest {
                    self.check_expr(arm.body, ty, Origin::MatchArms(first_range));
                }
                ty
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
                            let annotated =
                                lower_type(self.table, &self.body.types, self.rigids, *ty);
                            let range = self.body.types[*ty].range;
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

    /// トップレベルの値を参照するたびに、宣言の型の形を具体化する。呼び出し先の Kind の制約は複写せず、Kind の具体化の
    /// 記録を残して段2で展開する (docs/spec/types.md の「推論」)。シグネチャがなければ `None` である。
    fn instantiate(&mut self, decl: ValueItem) -> Option<(Ty, Vec<Ty>)> {
        let shape = self.signatures.get(decl)?;
        let Instantiated {
            ty,
            args,
            lin,
            mult,
        } = shape.instantiate(self.table);
        self.instances.push(Instance {
            decl,
            lin,
            mult,
            origin: self.table.kind_origin(),
        });
        Some((ty, args))
    }

    /// `check` の間だけ、作る Kind の制約の由来を設定する。
    pub(super) fn with_kind_origin<T>(
        &mut self,
        range: TextRange,
        reason: KindReason,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let span = Span {
            file: self.file(),
            range,
        };
        let previous = self
            .table
            .set_kind_origin(Provenance::At(KindOrigin { span, reason }));
        let result = check(self);
        self.table.set_kind_origin(previous);
        result
    }

    /// 名前の参照の型。`open` が偽なら、トップレベルの値の戻り値の側の row を開かない。呼び出しが矢印の row を宣言のまま
    /// 記録し、部分適用の残りだけを開くため (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    ///
    /// 関数と組み込みの参照は、`open` なら、具体化した後に戻り値の側の閉じた row を開く。純粋な関数を、エフェクトを持つ関数型の
    /// 引数に渡せるようにするため (docs/spec/types.md の「推論」)。局所変数の型は開かない。スキームから複写する Kind
    /// の制約は、参照した場所を由来にする。
    ///
    /// トップレベルの値の参照は、参照ごとの具体化の表に型引数を記録する。単相化の instance の表と型クラスの制約が
    /// 型ごとの解決に使う (docs/implementation/architecture.md の「`eml_types` の内部」)。
    fn path(&mut self, id: ExprId, res: Res, range: TextRange, open: bool) -> Ty {
        let item = match res {
            Res::Local(local) => {
                return self
                    .typing
                    .locals
                    .get(local)
                    .copied()
                    .unwrap_or(self.table.error);
            }
            Res::Item(item) => item,
        };
        let name = self.program.value_name(item).to_string();
        let instantiated = self.with_kind_origin(range, KindReason::Passed(name), |this| {
            this.instantiate(item)
        });
        let Some((ty, args)) = instantiated else {
            return self.table.error;
        };
        self.typing.instantiations.insert(id, (item, args));
        if open { self.table.open_spine(ty) } else { ty }
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
    /// ならない。矢印が余れば部分適用で、残りの関数型が値の型になる (docs/spec/expressions.md の「関数適用」)。
    fn call(&mut self, id: ExprId, callee: ExprId, args: &[ExprId]) -> Ty {
        let body = self.body;
        let callee_expr = &body.exprs[callee];
        let name = callee_subject(self.program, body, callee);
        let (mut ty, opened_later) = match &callee_expr.kind {
            // 呼ばれる位置のトップレベルの値は開かずに具体化し、矢印の row を宣言のまま記録する。持ち越し規則は宣言の
            // row で判定する (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
            ExprKind::Path(res @ Res::Item(_)) => {
                let ty = self.path(callee, *res, callee_expr.range, false);
                self.typing.exprs.insert(callee, ty);
                (ty, true)
            }
            _ => (self.infer_expr(callee), false),
        };
        let performs = match &callee_expr.kind {
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {
                Some((self.program[*op].arity.saturating_sub(1), *op))
            }
            _ => None,
        };
        let mut arrows = Vec::new();
        let mut results = Vec::new();
        // 1回の呼び出しの E2002 は、どの引数の矢印で起きても1つだけ報告する
        let mut reported = false;
        for (index, &arg) in args.iter().enumerate() {
            match self.next_arrow(ty) {
                Arrow::Fn { param, row, ret } => {
                    arrows.push(row.clone());
                    results.push(ret);
                    let origin = Origin::Argument {
                        callee: callee_expr.range,
                        name: name.clone(),
                        index,
                    };
                    self.check_expr(arg, param, origin);
                    let ok = self.include_call_row(
                        row,
                        (id, index),
                        body.exprs[id].range,
                        &name,
                        !reported,
                    );
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
                    self.typing.calls.insert(
                        id,
                        CallRows::Call {
                            arrows,
                            results,
                            performs,
                        },
                    );
                    return self.table.error;
                }
            }
        }
        self.typing.calls.insert(
            id,
            CallRows::Call {
                arrows,
                results,
                performs,
            },
        );
        // 部分適用の残りは、ほかの参照と同じく戻り値の側の row を開く
        if opened_later {
            ty = self.table.open_spine(ty);
        }
        ty
    }

    /// `check` の間だけ今の row とその由来を替え、終わったら戻す。ラムダの本体は、外側の関数ではなく、ラムダで最後に
    /// たどった矢印の row で検査する (docs/spec/types.md の「推論」)。
    pub(super) fn with_ambient<T>(
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
            let annotated = lower_type(self.table, &body.types, self.rigids, *annotation);
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

    pub(super) fn bind_pat(&mut self, pat: PatId, ty: Ty) {
        self.typing.pats.insert(pat, ty);
        let body = self.body;
        match &body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.typing.locals.insert(*local, ty);
            }
            PatKind::Annot {
                pat: inner,
                ty: annotation,
            } => {
                let annotated = lower_type(self.table, &body.types, self.rigids, *annotation);
                self.expect(
                    body.pats[pat].range,
                    ty,
                    annotated,
                    &Origin::AnnotatedPattern,
                );
                self.bind_pat(*inner, annotated);
            }
            PatKind::Con { ctor, args } => self.constructor_pattern(pat, *ctor, args, ty),
            PatKind::Tuple(elements) => self.tuple_pattern(pat, elements, ty),
            PatKind::Literal(literal) => self.literal_pattern(pat, literal, ty),
            PatKind::Wildcard | PatKind::Missing => {}
            PatKind::Unit => {
                let unit = self.table.unit;
                self.expect(body.pats[pat].range, ty, unit, &Origin::UnitPattern);
            }
        }
    }

    /// コンストラクタのパターン。スキームを具体化し、結果の型を期待する型と単一化してから、引数のパターンを
    /// フィールドの型で検査する。HIR が引数の個数を確かめている (E1016) ので、引数とフィールドは同じ数である。
    fn constructor_pattern(
        &mut self,
        pat: PatId,
        ctor: ConstructorId,
        args: &[PatId],
        expected: Ty,
    ) {
        let range = self.body.pats[pat].range;
        let instantiated = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.instantiate(ValueItem::Constructor(ctor))
        });
        let mut ty = instantiated.map_or(self.table.error, |(ty, _)| ty);
        let mut fields = Vec::new();
        for _ in args {
            match self.table.shape(ty).clone() {
                TyShape::Fn { param, ret, .. } => {
                    fields.push(param);
                    ty = ret;
                }
                _ => fields.push(self.table.error),
            }
        }
        let unified = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.unify(ty, expected)
        });
        if unified.is_err() {
            let names = &self.program.names;
            let origin = Origin::ConstructorPattern {
                constructor: names.constructor(ctor).to_string(),
                ty: names.ty(self.program[ctor].ty).to_string(),
            };
            self.mismatch(range, expected, ty, &origin);
            // 型の合わないパターンの中は検査しない。網羅性の検査は、`Error` の型のパターンを含む `match` を飛ばす
            // (docs/spec/exhaustiveness.md)
            let error = self.table.error;
            self.typing.pats.insert(pat, error);
            fields = vec![error; args.len()];
        }
        for (&arg, field) in args.iter().zip(fields) {
            self.bind_pat(arg, field);
        }
    }

    /// タプルのパターン。期待する型を、要素の数だけの新しい変数でできたタプルと単一化してから、要素のパターンを検査
    /// する。要素の数が違えば、型の合わないコンストラクタのパターンと同じく、中を検査しない。
    fn tuple_pattern(&mut self, pat: PatId, elements: &[PatId], expected: Ty) {
        let range = self.body.pats[pat].range;
        let fields: Vec<Ty> = elements.iter().map(|_| self.table.fresh_var()).collect();
        let tuple = self.table.tuple(fields.clone());
        let unified = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.unify(tuple, expected)
        });
        let fields = if unified.is_ok() {
            fields
        } else {
            self.mismatch(
                range,
                expected,
                tuple,
                &Origin::TuplePattern(elements.len()),
            );
            // 網羅性の検査は、`Error` の型のパターンを含む `match` を飛ばす (docs/spec/exhaustiveness.md)
            let error = self.table.error;
            self.typing.pats.insert(pat, error);
            vec![error; elements.len()]
        };
        for (&element, field) in elements.iter().zip(fields) {
            self.bind_pat(element, field);
        }
    }

    /// `Int` と `String` のリテラルのパターン。リテラルの型を期待する型と単一化する。
    fn literal_pattern(&mut self, pat: PatId, literal: &Literal, expected: Ty) {
        let range = self.body.pats[pat].range;
        let found = match literal {
            Literal::Int(_) => self.table.int,
            Literal::String(_) => self.table.string,
            Literal::Unit => self.table.unit,
        };
        let unified = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.unify(found, expected)
        });
        if unified.is_err() {
            self.mismatch(range, expected, found, &Origin::LiteralPattern);
            // 網羅性の検査は、`Error` の型のパターンを含む `match` を飛ばす (docs/spec/exhaustiveness.md)
            let error = self.table.error;
            self.typing.pats.insert(pat, error);
        }
    }

    fn expect(&mut self, range: TextRange, expected: Ty, found: Ty, origin: &Origin) {
        let unified = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.unify(found, expected)
        });
        match unified {
            Ok(()) => {}
            Err(UnifyError::Occurs) => self.diagnostics.push(Diagnostic::error(
                codes::INFINITE_TYPE,
                "this expression would have an infinite type",
                Label::new(self.file(), range, "infinite type"),
            )),
            Err(error) => {
                let arrow_linearity = error == UnifyError::ArrowLinearity;
                self.report_mismatch(range, expected, found, origin, arrow_linearity);
            }
        }
    }
}

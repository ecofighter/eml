use std::collections::{HashMap, HashSet};

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{
    Body, Closure, ConstructorId, EffectId, ExprId, ExprKind, Function, Literal, LocalFunction,
    LocalId, MatchArm, OperationId, PatId, PatKind, Program, Res, Segment, Stmt, TypeRefId,
    TypeRefKind, ValueItem,
};
use la_arena::ArenaMap;

use crate::MaskItem;
use crate::kind::problem::Instance;
use crate::kind::{KindOrigin, KindReason, Provenance, Span};
use crate::shape::{Instantiated, Rigids, lower_type};
use crate::store::TypeStore;
use crate::table::{Row, RowVar, Table, Tail, Ty, TyShape, UnifyError};
use crate::{FieldTarget, Update, codes};

use super::Signatures;
use super::report::{AmbientSource, Origin, callee_subject};

/// 呼び出しの row。持ち越しのパスが読む (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
#[derive(Debug, Clone)]
pub(crate) enum CallRows {
    /// 矢印ごとの row。トップレベルの値を呼ぶときは、開く前の宣言の row である。`performs` は、操作を起こす矢印の
    /// 番号とその操作である。
    Call {
        arrows: Vec<Row>,
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
    /// 参照ごとの具体化。型引数と row 変数は表の変数のままで、`check_body` が carry の後で書き出し、分類する。
    pub instantiations: ArenaMap<ExprId, (ValueItem, Vec<Ty>, Vec<RowVar>)>,
    /// 本体が動く row。引数をたどり終えた後の今の row である。`check_body` が `RowYield` に分類する。
    pub ambient: Option<Row>,
    /// ラムダの本体が動く row。キーはラムダの式である。
    pub lambdas: ArenaMap<ExprId, Row>,
    /// 呼び出しの矢印の row。キーは `masks` と同じく、呼び出しの式と矢印の番号である。
    pub arrows: HashMap<(ExprId, usize), Row>,
    /// 呼び出しの row。持ち越しのパスが読む。
    pub calls: ArenaMap<ExprId, CallRows>,
    /// 呼び出しの矢印ごとの `mask`。キーは呼び出しの式と矢印の番号である。空の `mask` は入れない
    /// (docs/spec/types.md の「推論」)。
    pub masks: HashMap<(ExprId, usize), Vec<MaskItem>>,
    /// 射影が指すフィールド。
    pub fields: ArenaMap<ExprId, FieldTarget>,
    /// 更新が作り直すコンストラクタとフィールド。
    pub updates: ArenaMap<ExprId, Update<Ty>>,
    /// 射影と更新が捨てるフィールドの番号と型。使用回数のパスが `_` で受けた値と同じく `Unr` の制約を出す。
    pub discarded: ArenaMap<ExprId, Vec<(u32, Ty)>>,
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
    /// 本体を検査中のローカル関数。内側の関数ほど後ろにある。
    pub(super) local_frames: Vec<LocalFrame>,
    /// 最後の矢印の row を閉じたローカル関数。トップレベルの関数と同じく、値として参照すると row を開く
    /// (docs/spec/types.md の「ローカル関数の推論」)。
    pub(super) closed_locals: HashSet<LocalId>,
    /// `fn` の節の本体に期待する型と由来。キーは書いた本体の式である。HIR が本体を `let` の右辺に下ろすので、その
    /// `let` で本体を期待する型に照らす。隠れた `k` の引数の誤りとして報告しないため (docs/spec/expressions.md の
    /// 「handler」)。
    pub(super) fn_clause_bodies: HashMap<ExprId, (Ty, Origin)>,
}

/// 本体を検査中のローカル関数。本体の中の自分への呼び出しの row は、まだ決まっていないので、包含を確かめずにここへ
/// 積んでおき、row を閉じた後で確かめる (docs/spec/types.md の「ローカル関数の推論」)。
pub(super) struct LocalFrame {
    local: LocalId,
    deferred: Vec<DeferredRow>,
}

/// 後に回した自分への呼び出しの矢印の row。包含の先は、呼び出しの位置の今の row とその由来である。
struct DeferredRow {
    row: Row,
    key: (ExprId, usize),
    name: String,
    ambient: Row,
    source: AmbientSource,
}

/// 式に期待する型。check と infer で `if` とブロックの処理を共有するため。
pub(super) enum Expectation {
    Has(Ty, Origin),
    None,
}

/// 呼び出しで後に回したラムダの引数 (引数、矢印の引数の型、由来) と、後に回した矢印の row (引数の番号、row)。
#[derive(Default)]
struct Postponed {
    lambdas: Vec<(ExprId, Ty, Origin)>,
    rows: Vec<(usize, Row)>,
}

impl Postponed {
    fn is_empty(&self) -> bool {
        self.lambdas.is_empty() && self.rows.is_empty()
    }
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
        self.typing.ambient = Some(self.ambient.clone());
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
            ExprKind::Call { callee, args, .. } => {
                let found = self.call(id, *callee, args, Expectation::Has(expected, origin));
                self.typing.exprs.insert(id, found);
            }
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
            TyShape::Con(list, args, _) if *list == self.program.lang.list && args.len() == 1 => {
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
        let Some((list, _, _)) = instantiated else {
            return self.table.error;
        };
        let TyShape::Con(_, args, _) = self.table.shape(list).clone() else {
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

    /// 作る式。コンストラクタを参照と同じく具体化し、矢印を剥がしてフィールドの型を宣言の順に得る。フィールドの式は
    /// 書いた順に、そのフィールドの型に対して検査する。結果の型は位置のコンストラクタの呼び出しと同じである
    /// (docs/spec/types.md の「フィールドの解決」)。
    fn record(
        &mut self,
        id: ExprId,
        ctor: ConstructorId,
        fields: &[(u32, ExprId)],
        range: TextRange,
    ) -> Ty {
        let mut ty = self.path(id, Res::Item(ValueItem::Constructor(ctor)), range, false);
        let mut declared = Vec::new();
        for _ in &self.program[ctor].fields {
            match self.table.shape(ty).clone() {
                TyShape::Fn { param, ret, .. } => {
                    declared.push(param);
                    ty = ret;
                }
                _ => declared.push(self.table.error),
            }
        }
        for &(field, value) in fields {
            self.check_expr(
                value,
                declared[field as usize],
                Origin::Field { ctor, field },
            );
        }
        ty
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
            ExprKind::Call { callee, args, .. } => self.call(id, *callee, args, Expectation::None),
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
            ExprKind::Record { ctor, fields } => self.record(id, *ctor, fields, expr.range),
            ExprKind::Update { base, fields } => self.update(id, *base, fields),
            ExprKind::Field { base, field } => self.projection(id, *base, field),
            // 各穴は `display e` の呼び出しで、`String` を返す。穴の式の型には、`display` の参照が `Show` の制約を付ける
            // (docs/spec/expressions.md の「補間」)
            ExprKind::Interpolation(segments) => {
                for hole in segments.iter().filter_map(Segment::hole) {
                    let string = self.table.string;
                    self.check_expr(hole, string, Origin::Inferred);
                }
                self.table.string
            }
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
                        None => match self.fn_clause_bodies.remove(init) {
                            Some((expected, origin)) => {
                                self.check_expr(*init, expected, origin);
                                expected
                            }
                            None => self.infer_expr(*init),
                        },
                    };
                    self.bind_pat(*pat, ty);
                }
                Stmt::Functions(functions) => self.local_functions(functions),
                Stmt::Expr(expr) => {
                    let unit = self.table.unit;
                    self.check_expr(*expr, unit, Origin::Statement);
                }
            }
        }
    }

    /// ローカル関数の組 (docs/spec/types.md の「ローカル関数の推論」)。各関数の型を先に置いて名前を束縛し、本体を
    /// ラムダとして検査する。組の中の呼び出しで後に回した row は、固定点で今の row に足りないラベルを足してから、
    /// 各関数の最後の矢印の row の自由な末尾を閉じ、閉じた後で包含を確かめる。
    fn local_functions(&mut self, functions: &[LocalFunction]) {
        let body = self.body;
        let base = self.local_frames.len();
        let mut types = Vec::new();
        let mut members = Vec::new();
        for function in functions {
            let ExprKind::Lambda(closure) = &body.exprs[function.lambda].kind else {
                unreachable!("a local function is a lambda")
            };
            let (ty, last_row, parts) = self.local_function_type(closure.params.len());
            self.bind_pat(function.pat, ty);
            types.push(ty);
            // 名前を束縛できなかった関数 (構文の誤りからの回復) は、ラムダと同じく扱う
            if let Some(&local) = body.defining.get(function.lambda) {
                self.local_frames.push(LocalFrame {
                    local,
                    deferred: Vec::new(),
                });
                members.push((local, closure, last_row, parts));
            }
        }
        for (function, ty) in functions.iter().zip(types) {
            self.check_expr(function.lambda, ty, Origin::Inferred);
        }
        let deferred: Vec<(LocalId, DeferredRow)> = self
            .local_frames
            .split_off(base)
            .into_iter()
            .flat_map(|frame| {
                let callee = frame.local;
                frame.deferred.into_iter().map(move |row| (callee, row))
            })
            .collect();
        self.add_missing_labels(&deferred);
        let group: Vec<LocalId> = members.iter().map(|&(local, ..)| local).collect();
        let closing: Vec<(LocalId, Row, Vec<Ty>)> = members
            .into_iter()
            .filter_map(|(local, closure, last_row, parts)| {
                // 引数と結果の型に現れる末尾も閉じない。`let app f = f ()` のような高階のローカル関数のエフェクトは、
                // 引数から来るためである。組の関数への参照は捕まえた値ではない
                let mut keep = parts;
                keep.extend(
                    body.closure_captures(closure)
                        .into_iter()
                        .filter(|captured| !group.contains(captured))
                        .map(|captured| {
                            self.typing
                                .locals
                                .get(captured)
                                .copied()
                                .unwrap_or(self.table.error)
                        }),
                );
                Some((local, last_row?, keep))
            })
            .collect();
        self.close_group(&closing, &deferred);
        // 1回の呼び出しの E2002 は、どの矢印で起きても1つだけ報告する (`call` と同じ)
        let mut failed = HashSet::new();
        for (_, deferred) in deferred {
            let DeferredRow {
                row,
                key,
                name,
                ambient,
                source,
            } = deferred;
            let range = body.exprs[key.0].range;
            let report = !failed.contains(&key.0);
            let ok = self.with_ambient(ambient, source, |this| {
                this.include_call_row(row, key, range, &name, report)
            });
            if !ok {
                failed.insert(key.0);
            }
        }
    }

    /// 組の固定点 (docs/spec/types.md の「ローカル関数の推論」)。後に回した呼び出しごとに、呼び出し先の row の今の
    /// ラベルのうち足りないものを、呼び出しの位置の今の row の自由な末尾に足す。何も足さなくなるまで繰り返す。
    /// 足せない呼び出しは、後の包含の確かめが E2002 を報告する。
    fn add_missing_labels(&mut self, deferred: &[(LocalId, DeferredRow)]) {
        // 足すラベルは、どれも始めたときにいずれかの呼び出し先か今の row にあったラベルの写しである。有限の最小の解では、
        // ある row のエフェクトごとのラベルは、始めたときの row から写ってきたものを重ねても、異なる row のラベルの数の
        // 和を超えない。呼び出しの輪をたどるたびに数が増える場合 (組の関数を値として渡したときや、高階の関数の row で
        // 呼び出し先の row の手前にラベルが付く場合) は有限の解がなく、いずれこの和を超える。超えた呼び出し先からは
        // 足さないので、固定点は止まり、後の包含が E2002 を報告する。呼び出し先と今の row が末尾を共有するときも
        // 足さない。足すたびに呼び出し先も伸びるためで、呼び出し先の自由な末尾を使わないことと同じである
        let mut rows: Vec<Row> = Vec::new();
        for (_, call) in deferred {
            for row in [&call.row, &call.ambient] {
                let row = self.table.resolve_row(row);
                if !rows.contains(&row) {
                    rows.push(row);
                }
            }
        }
        let mut limits: HashMap<EffectId, usize> = HashMap::new();
        for row in &rows {
            for (effect, count) in self.label_counts(row) {
                *limits.entry(effect).or_default() += count;
            }
        }
        loop {
            let mut added = false;
            for (_, call) in deferred {
                let missing = self.table.missing_labels(&call.row, &call.ambient);
                if missing.is_empty() {
                    continue;
                }
                let shares_tail = self.table.resolve_row(&call.row).tail
                    == self.table.resolve_row(&call.ambient).tail;
                let grown = self
                    .label_counts(&call.row)
                    .into_iter()
                    .any(|(effect, count)| count > limits.get(&effect).copied().unwrap_or(0));
                if shares_tail || grown {
                    continue;
                }
                added |= self.table.add_labels(&call.ambient, missing);
            }
            if !added {
                break;
            }
        }
    }

    fn label_counts(&self, row: &Row) -> HashMap<EffectId, usize> {
        let mut counts = HashMap::new();
        for label in self.table.resolve_row(row).labels {
            *counts.entry(label.effect).or_default() += 1;
        }
        counts
    }

    /// 組の関数の最後の矢印の row の自由な末尾を閉じる。閉じない例外 (docs/spec/types.md の「ローカル関数の推論」) は
    /// 関数ごとに当てはめる。`members` は関数と、その row と、末尾が現れたら閉じない型である。外側のローカル関数への
    /// 呼び出しで後に回したものの今の row に現れる末尾も閉じない。また、閉じない row の関数を組の中で呼ぶ位置の今の
    /// row も閉じない。その包含は閉じない row が決まる前に確かめるので、先に閉じると、閉じない row に後から入る
    /// エフェクトが入らない。
    fn close_group(
        &mut self,
        members: &[(LocalId, Row, Vec<Ty>)],
        deferred: &[(LocalId, DeferredRow)],
    ) {
        let mut keep_rows: Vec<Row> = self
            .local_frames
            .iter()
            .flat_map(|frame| frame.deferred.iter().map(|call| call.ambient.clone()))
            .collect();
        let mut open = vec![false; members.len()];
        loop {
            let mut grew = false;
            for (index, (local, row, keep)) in members.iter().enumerate() {
                if open[index]
                    || self.table.closable_tail(row, keep, &keep_rows).is_some()
                    || !matches!(self.table.resolve_row(row).tail, Tail::Var(_))
                {
                    continue;
                }
                open[index] = true;
                grew = true;
                keep_rows.extend(
                    deferred
                        .iter()
                        .filter(|(callee, _)| callee == local)
                        .map(|(_, call)| call.ambient.clone()),
                );
            }
            if !grew {
                break;
            }
        }
        for ((local, row, keep), open) in members.iter().zip(open) {
            if open {
                continue;
            }
            if let Some(tail) = self.table.closable_tail(row, keep, &keep_rows) {
                let closed = self.table.close_row(tail);
                debug_assert!(closed.is_ok(), "a free row variable closes");
                self.closed_locals.insert(*local);
            }
        }
    }

    /// 引数が `arity` 個のローカル関数の型と、最後の矢印の row と、引数と結果の型。最後でない矢印は引数を受け取るだけ
    /// なので、トップレベルの関数と同じく row を `<>` に閉じる (docs/spec/types.md の「ローカル関数の推論」)。
    fn local_function_type(&mut self, arity: usize) -> (Ty, Option<Row>, Vec<Ty>) {
        let mut ty = self.table.fresh_var();
        let mut parts = vec![ty];
        let mut last = None;
        for index in (0..arity).rev() {
            let row = if index + 1 == arity {
                let row = Row {
                    labels: Vec::new(),
                    tail: Tail::Var(self.table.fresh_row_var()),
                };
                last = Some(row.clone());
                row
            } else {
                Row::pure()
            };
            let param = self.table.fresh_var();
            parts.push(param);
            let lin = self.table.fresh_arrow_lin();
            ty = self.table.function_with(param, lin, row, ty);
        }
        (ty, last, parts)
    }

    /// 呼び出し `call` が、本体を検査中のローカル関数を呼ぶなら、その枠の番号を返す。
    fn self_call_frame(&self, call: ExprId) -> Option<usize> {
        let ExprKind::Call { callee, .. } = &self.body.exprs[call].kind else {
            return None;
        };
        let ExprKind::Path(Res::Local(local)) = self.body.exprs[*callee].kind else {
            return None;
        };
        self.local_frames
            .iter()
            .position(|frame| frame.local == local)
    }

    /// トップレベルの値を参照するたびに、宣言の型の形を具体化する。呼び出し先の Kind の制約は複写せず、Kind の具体化の
    /// 記録を残して段2で展開する (docs/spec/types.md の「推論」)。シグネチャがなければ `None` である。
    pub(super) fn instantiate(&mut self, decl: ValueItem) -> Option<(Ty, Vec<Ty>, Vec<RowVar>)> {
        let shape = self.signatures.get(decl)?;
        let Instantiated {
            ty,
            args,
            rows,
            lin,
            mult,
        } = shape.instantiate(self.table);
        self.instances.push(Instance {
            decl,
            lin,
            mult,
            origin: self.table.kind_origin(),
        });
        Some((ty, args, rows))
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
                let ty = self
                    .typing
                    .locals
                    .get(local)
                    .copied()
                    .unwrap_or(self.table.error);
                // row を閉じたローカル関数は、トップレベルの関数と同じく値として参照すると row を開く。自分の本体の中では
                // まだ閉じていないので、開かない単相の型のままである (docs/spec/types.md の「ローカル関数の推論」)
                return if open && self.closed_locals.contains(&local) {
                    self.table.open_spine(ty)
                } else {
                    ty
                };
            }
            Res::Item(item) => item,
        };
        let name = self.program.value_name(item).to_string();
        let instantiated = self.with_kind_origin(range, KindReason::Passed(name), |this| {
            this.instantiate(item)
        });
        let Some((ty, args, rows)) = instantiated else {
            return self.table.error;
        };
        // 操作以外の宣言は何も捕まえないので、一番外側の矢印を参照ごとの変数にして、`once` の位置にも置けるようにする。
        // 呼び出しの位置でも同じにする。制約がなければ `Unr` に解けるので、呼び出しの意味は変わらない。この変数は宣言の
        // `Shape` の番号を持たないので、`Instance.lin` には入れない (docs/spec/types.md の「トップレベルの関数の参照」)
        let ty = match item {
            ValueItem::Operation(_) => ty,
            _ => self.table.fresh_outer_lin(ty),
        };
        self.typing.instantiations.insert(id, (item, args, rows));
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
    ///
    /// 引数のラムダは、ほかの引数の後に検査する。`map (.name) people` で、ラムダの引数の型を `people` から決めるため
    /// である。期待する型があれば、結果の型をその型と合わせる (docs/spec/types.md
    /// の「引数のラムダを後で検査する」)。
    fn call(
        &mut self,
        id: ExprId,
        callee: ExprId,
        args: &[ExprId],
        expectation: Expectation,
    ) -> Ty {
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
            // ローカル関数も、呼ばれる位置では開かずに矢印の row を記録する。部分適用の残りを開くのは、row を閉じた
            // 関数だけである (docs/spec/types.md の「ローカル関数の推論」)
            ExprKind::Path(res @ Res::Local(local))
                if body.local_functions.get(*local).is_some() =>
            {
                let ty = self.path(callee, *res, callee_expr.range, false);
                self.typing.exprs.insert(callee, ty);
                (ty, self.closed_locals.contains(local))
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
        let mut postponed = Postponed::default();
        // 1回の呼び出しの E2002 は、どの引数の矢印で起きても1つだけ報告する
        let mut reported = false;
        let argument = |index| Origin::Argument {
            callee: callee_expr.range,
            name: name.clone(),
            index,
        };
        for (index, &arg) in args.iter().enumerate() {
            let origin = argument(index);
            // 呼ばれる側の型が変数なら、`next_arrow` がそれを新しい関数型に決める前に、後に回したラムダを検査する。
            // その変数を決めるのはラムダかもしれないためである (`apply (fn x -> x) 1 2`)
            if !postponed.is_empty() && matches!(self.table.shape(ty), TyShape::Var(_)) {
                self.check_postponed(&mut postponed, id, &name, &mut reported);
            }
            match self.next_arrow(ty) {
                Arrow::Fn { param, row, ret } => {
                    arrows.push(row.clone());
                    self.typing.arrows.insert((id, index), row.clone());
                    if self.postponable(arg) {
                        postponed.lambdas.push((arg, param, origin));
                        postponed.rows.push((index, row));
                    } else {
                        self.check_expr(arg, param, origin);
                        // ラムダを後に回した後は、row を引数の順に入れるため、この row も後に回す
                        if postponed.is_empty() {
                            self.include_arg_row(row, id, index, &name, &mut reported);
                        } else {
                            postponed.rows.push((index, row));
                        }
                    }
                    ty = ret;
                }
                Arrow::Error => {
                    self.check_postponed(&mut postponed, id, &name, &mut reported);
                    self.check_against_error(arg, origin);
                }
                Arrow::NotFunction => {
                    let diagnostic = self.call_arity_error(&name, index, args.len(), arg);
                    self.diagnostics.push(diagnostic);
                    self.check_postponed(&mut postponed, id, &name, &mut reported);
                    for (rest_index, &rest) in args.iter().enumerate().skip(index) {
                        self.check_against_error(rest, argument(rest_index));
                    }
                    // 期待する型を `Error` と合わせ、外側の呼び出しに誤りを連鎖させないため、下の共通の後始末を通る
                    ty = self.table.error;
                    break;
                }
            }
        }
        self.typing
            .calls
            .insert(id, CallRows::Call { arrows, performs });
        // 部分適用の残りは、ほかの参照と同じく戻り値の側の row を開く
        if opened_later {
            ty = self.table.open_spine(ty);
        }
        let range = body.exprs[id].range;
        let expectation = if postponed.is_empty() {
            expectation
        } else {
            // `people |> map (.name)` では、期待する型が `map` の型変数を決めてから、ラムダを検査する
            if let Expectation::Has(expected, origin) = expectation {
                self.expect(range, expected, ty, &origin);
            }
            self.check_postponed(&mut postponed, id, &name, &mut reported);
            Expectation::None
        };
        if let Expectation::Has(expected, origin) = expectation {
            self.expect(range, expected, ty, &origin);
        }
        ty
    }

    /// 後に回す引数か。`use` が組んだラムダは後に回さない。`use` のブロックの型の誤りを、シグネチャを指す今の報告の
    /// まま保つためである (docs/spec/types.md の「引数のラムダを後で検査する」)。
    fn postponable(&self, arg: ExprId) -> bool {
        matches!(self.body.exprs[arg].kind, ExprKind::Lambda(_))
            && !self.body.use_lambdas.contains(&arg)
    }

    /// 後に回したラムダを左から検査し、後に回した row を引数の順に今の row に入れる。ラムダの本体が row 変数を決めて
    /// から row を入れるため、この順にする (`try (fn () -> deploy ())`)。ラムダは、後に回したときの矢印の引数の型に
    /// 対して検査する。後の引数で呼び出しが誤りになっても同じである。ラムダの型は誤りより前の矢印で決まっているので、
    /// ラムダの中の誤りを、ラムダを後に回さないときと同じく報告するためである。
    fn check_postponed(
        &mut self,
        postponed: &mut Postponed,
        call: ExprId,
        name: &str,
        reported: &mut bool,
    ) {
        for (arg, param, origin) in std::mem::take(&mut postponed.lambdas) {
            self.check_expr(arg, param, origin);
        }
        for (index, row) in std::mem::take(&mut postponed.rows) {
            self.include_arg_row(row, call, index, name, reported);
        }
    }

    fn include_arg_row(
        &mut self,
        row: Row,
        call: ExprId,
        index: usize,
        name: &str,
        reported: &mut bool,
    ) {
        if let Some(frame) = self.self_call_frame(call) {
            let deferred = DeferredRow {
                row,
                key: (call, index),
                name: name.to_string(),
                ambient: self.ambient.clone(),
                source: self.ambient_source.clone(),
            };
            self.local_frames[frame].deferred.push(deferred);
            return;
        }
        let range = self.body.exprs[call].range;
        let ok = self.include_call_row(row, (call, index), range, name, !*reported);
        *reported |= !ok;
    }

    /// 矢印に当たらない、型の壊れた呼び出しの引数。ラムダは `Error` の型に対して検査し、引数を `Error` で束縛する。
    /// 推論すると引数に新しい変数が付き、射影の E2013 が連鎖するためである。
    fn check_against_error(&mut self, arg: ExprId, origin: Origin) {
        if matches!(self.body.exprs[arg].kind, ExprKind::Lambda(_)) {
            let error = self.table.error;
            self.check_expr(arg, error, origin);
        } else {
            self.infer_expr(arg);
        }
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
        self.typing.lambdas.insert(id, row.clone());
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
        let mut ty = instantiated.map_or(self.table.error, |(ty, _, _)| ty);
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
            Err(_) => self.mismatch(range, expected, found, origin),
        }
    }
}

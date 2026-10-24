//! 持ち越し規則 (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。呼び出しをまたいで持っている値の Kind と、
//! 呼び出しの row の多重度を組にした持ち越しの制約を出す。使用回数のパス (`usage`) は評価の順を区別しないので、
//! 評価の順に沿った生存の計算を別に持つ。呼び出しの評価の順は `eml_hir::call_steps` に従う。Core IR の変換も同じ手順を
//! 読む (docs/spec/expressions.md の「関数適用」)。

use std::collections::{BTreeSet, HashMap};

use eml_diagnostics::FileId;
use eml_hir::{
    Body, EvalStep, ExprId, ExprKind, IndexBase, LocalId, OpMultiplicity, OperationId, PatId,
    Program, Res, Segment, Stmt,
};

use crate::VarClass;
use crate::check::{BodyTyping, CallRows};
use crate::kind::{
    Across, Bound, CallKind, CarriedValue, Crossing, KindOrigin, KindReason, Provenance, Span,
};
use crate::table::{Table, Ty};
use crate::ty::Multiplicity;

/// 呼び出しをまたいで持ちうる値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Held {
    Local(LocalId),
    /// 評価済みで消費前の部分式の値。
    Temporary(ExprId),
}

type Live = BTreeSet<Held>;

pub(crate) fn constrain(
    program: &Program,
    file: FileId,
    body: &Body,
    typing: &BodyTyping,
    table: &mut Table<'_>,
    reliable: bool,
    implicit_drops: &HashMap<ExprId, Vec<LocalId>>,
) {
    let mut carrying = Carrying {
        program,
        file,
        body,
        typing,
        table,
        reliable,
        implicit_drops,
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
    /// 戻らない呼び出しの前で暗黙に手放す変数 (`BodyTypes::implicit_drops`)。
    implicit_drops: &'a HashMap<ExprId, Vec<LocalId>>,
}

impl Carrying<'_, '_> {
    /// 持ち上げる関数の本体 (関数、ラムダ、handle の本体、節)。本体の後に持つ値はないので、空の集合から始める。
    fn function(&mut self, root: ExprId) {
        self.expr(root, &Live::new());
    }

    /// 評価済みの部分式の値。局所変数そのものなら、その局所変数として持つ。E3006 が無名の途中の値でなく変数を名指しし、
    /// 束縛の位置を示すためである。
    fn evaluated(&self, expr: ExprId) -> Held {
        match self.body.exprs[expr].kind {
            ExprKind::Path(Res::Local(local)) => Held::Local(local),
            _ => Held::Temporary(expr),
        }
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
                // translate は、暗黙に手放す変数の `drop` を引数を評価した後、呼び出しの前に置く。その変数は束縛から
                // `drop` まで生きているので、呼び出しそのものはまたがないが、呼ばれる式と引数の評価はまたぐ
                // (docs/spec/core-ir.md の「戻らない呼び出しの前の `drop`」)
                let mut dropped = self.implicit_drops.get(&id);
                // 評価の順は `eml_hir::call_steps` だけが持つ。後で使う値は後ろの手順から決まるので、その順を逆にたどる。
                // 引数はすべて評価してから矢印を順に適用するので、矢印 i の呼び出しの間は、後で使う値と、評価済みで
                // 後の矢印に渡す引数を持つ (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
                for step in eml_hir::call_steps(body, id).into_iter().rev() {
                    match step {
                        EvalStep::Arrow(index) => {
                            if let Some((arrows, performs)) = rows
                                && let Some(row) = arrows.get(index)
                            {
                                let across = match performs {
                                    Some((at, op)) if at == index => Across::Operation(op),
                                    _ => Across::Row(row),
                                };
                                let crossing = self.crossing(across);
                                self.carry(id, &live, &crossing, &CallKind::Call);
                            }
                            if index == 0 {
                                live.insert(self.evaluated(*callee));
                            }
                            live.insert(self.evaluated(args[index]));
                        }
                        EvalStep::Eval(expr) => {
                            if let Some(locals) = dropped.take() {
                                live.extend(locals.iter().map(|&local| Held::Local(local)));
                            }
                            live.remove(&self.evaluated(expr));
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
                        Stmt::Functions(functions) => {
                            for function in functions.iter().rev() {
                                self.unbind(function.pat, &mut live);
                                live = self.expr(function.lambda, &live);
                            }
                            live
                        }
                        Stmt::Expr(expr) => self.expr(*expr, &live),
                    };
                }
                live
            }
            ExprKind::Annot { expr, .. } => self.expr(*expr, after),
            // ローカル関数の組の中の参照は、定義の前から持つ値ではない (docs/spec/linearity.md の「基本の規則」)
            ExprKind::Lambda(closure) => {
                self.function(closure.body);
                let group = body
                    .defining
                    .get(id)
                    .and_then(|&own| body.group_of(own))
                    .map_or(&[][..], |group| &group.members[..]);
                let mut live = after.clone();
                live.extend(
                    body.closure_captures(closure)
                        .into_iter()
                        .filter(|local| !group.contains(local))
                        .map(Held::Local),
                );
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
                // クロージャは handle の実行 (`prompt$` のフレーム) が持ち、扱うエフェクトの `multi` の区間にも入るので、
                // 本体の row 全体をまたぐ (docs/spec/effects.md の「handler の意味」)
                if let Some(CallRows::Handle {
                    body: body_row,
                    outer,
                }) = typing.calls.get(id)
                {
                    let outer = self.crossing(Across::Row(outer));
                    self.carry(id, after, &outer, &CallKind::Handle);
                    // 状態は本体の実行中、evidence にある。扱うエフェクトの操作が起きると状態は節に渡り、捕まえた
                    // 区間に入らないので、外側の row だけをまたぐ (docs/spec/effects.md の「パラメータ付き handler」)
                    if let Some(init) = init
                        && let Some(&ty) = typing.exprs.get(*init)
                    {
                        let value = CarriedValue::HandlerState {
                            init: body.exprs[*init].range,
                        };
                        self.carry_value(id, ty, value, &outer, &CallKind::Handle);
                    }
                    let across = self.crossing(Across::Row(body_row));
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
            ExprKind::Record { fields, .. } => {
                let values: Vec<ExprId> = fields.iter().map(|&(_, value)| value).collect();
                self.parts(&values, after)
            }
            ExprKind::Update { base, fields } => {
                let parts: Vec<ExprId> = std::iter::once(*base)
                    .chain(fields.iter().map(|&(_, value)| value))
                    .collect();
                self.parts(&parts, after)
            }
            ExprKind::Field { base, .. } => self.expr(*base, after),
            ExprKind::List(elements) => self.list(elements, after),
            ExprKind::Interpolation(segments) => self.interpolation(segments, after),
            ExprKind::Drop(value) => self.expr(*value, after),
            // 変数の範囲は handle の本体になる。範囲の後で使う値は外側の row をまたぐ。配列の変数は、`Lin` の状態を
            // 持つ handle と同じく、状態の配列も外側の row をまたぐ (docs/spec/linearity.md の「配列の変数」)。
            // 範囲の中の値は、handle の本体と同じく範囲の中だけで数える
            ExprKind::Var {
                var,
                init,
                body: scope,
                ..
            } => {
                if let Some(CallRows::Handle { outer, .. }) = typing.calls.get(id) {
                    let outer = self.crossing(Across::Row(outer));
                    self.carry(id, after, &outer, &CallKind::Var);
                    if typing.var_classes.get(*var) == Some(&VarClass::Array)
                        && let Some(state) = typing.vars.get(*var)
                    {
                        let data = &body.vars[*var];
                        let value = CarriedValue::VarState {
                            name: data.name.clone(),
                            decl: data.range,
                        };
                        self.carry_value(id, state.ty, value, &outer, &CallKind::Var);
                    }
                }
                let mut live = after.clone();
                live.extend(self.expr(*scope, &Live::new()));
                self.expr(*init, &live)
            }
            ExprKind::VarGet(_) => after.clone(),
            ExprKind::Assign { value, .. } => self.expr(*value, after),
            ExprKind::IndexAssign { index, value, .. } => self.parts(&[*index, *value], after),
            ExprKind::Index { base, index } => match base {
                IndexBase::Var(_) => self.expr(*index, after),
                IndexBase::Expr(base) => self.parts(&[*base, *index], after),
            },
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

    /// リストの要素も左から評価するが、評価済みの要素は最初の要素1つで代表させる。要素の型はどれも同じなので、
    /// 持ち越しの制約も同じになる。`parts` のように1つずつ持つと、生きている値の集合を要素ごとに写すので、要素の数の
    /// 2乗の時間がかかる (docs/implementation/architecture.md の「`eml_types` の内部」)。
    fn list(&mut self, elements: &[ExprId], after: &Live) -> Live {
        let Some((&first, rest)) = elements.split_first() else {
            return after.clone();
        };
        let mut live = after.clone();
        if !rest.is_empty() {
            live.insert(Held::Temporary(first));
        }
        for &element in rest.iter().rev() {
            live = self.expr(element, &live);
        }
        live.remove(&Held::Temporary(first));
        self.expr(first, &live)
    }

    /// 補間の穴も左から評価するが、組み立て中の文字列は持っている値に足さない。`String` は `Unr` なので、持っていても
    /// 制約が増えないため (docs/implementation/architecture.md の「`eml_types` の内部」)。
    fn interpolation(&mut self, segments: &[Segment], after: &Live) -> Live {
        let mut live = after.clone();
        for hole in segments.iter().rev().filter_map(Segment::hole) {
            live = self.expr(hole, &live);
        }
        live
    }

    fn unbind(&self, pat: PatId, live: &mut Live) {
        for local in self.body.pat_bindings(pat) {
            live.remove(&Held::Local(local));
        }
    }

    /// `across` の多重度の成分と、報告が指す `multi` の操作。持っている値によらないので、値ごとでなく、またぐ式
    /// ごとに1回だけ求める。どの成分も `Never` か `Once` に決まっていれば、制約は破れないので成分を空にする。
    fn crossing(&self, across: Across<'_>) -> Crossing {
        let mults = match across {
            Across::Row(row) => self.table.row_multiplicities(row),
            Across::Operation(op) => vec![Bound::Const(self.table.operation_multiplicity(op))],
        };
        if mults
            .iter()
            .all(|mult| matches!(mult, Bound::Const(Multiplicity::Never | Multiplicity::Once)))
        {
            return Crossing {
                mults: Vec::new(),
                multi: None,
            };
        }
        let multi = if self.reliable {
            self.multi_operation(across)
        } else {
            None
        };
        Crossing { mults, multi }
    }

    /// 式 `at` をまたいで `held` を持つ。値ごとに、型の Kind と `crossing` の多重度の組で持ち越しの制約を出す。
    fn carry(&mut self, at: ExprId, held: &Live, crossing: &Crossing, call: &CallKind) {
        if crossing.mults.is_empty() {
            return;
        }
        for &value in held {
            if let Some((ty, carried)) = self.held(value) {
                self.carry_value(at, ty, carried, crossing, call);
            }
        }
    }

    fn carry_value(
        &mut self,
        at: ExprId,
        ty: Ty,
        value: CarriedValue,
        crossing: &Crossing,
        call: &CallKind,
    ) {
        if crossing.mults.is_empty() {
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
                    multi: crossing.multi,
                    call: call.clone(),
                },
            })
        } else {
            Provenance::Suppressed
        };
        let previous = self.table.set_kind_origin(origin);
        self.table.carry(ty, &crossing.mults);
        self.table.set_kind_origin(previous);
    }

    /// 報告が指す `multi` の操作。row を解き、`multi` の操作を持つ最初のラベルのエフェクトから、宣言の順で最初の `multi`
    /// の操作を選ぶ。row を束縛するのはこの本体の検査だけで、このパスはその後に動くので、報告のときに解いても同じ結果に
    /// なる。
    fn multi_operation(&self, across: Across<'_>) -> Option<OperationId> {
        let program = self.program;
        match across {
            Across::Operation(op) => Some(op),
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
                // 隠れた `$k` は書き手が名前を知らないので、節を指す (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
                if let Some(clause) = self.body.fn_clauses.iter().find(|c| c.k == local) {
                    let value = CarriedValue::FnClauseContinuation {
                        clause: clause.range,
                    };
                    return Some((ty, value));
                }
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
        }
    }
}

//! `var` の検査 (docs/spec/types.md の「`var` の変数」)。`var` の範囲は地点ごとの合成のエフェクト `L` を扱う handle と
//! 同じく検査し、変数を使う所は `L` の `perform` と同じく row に `L` を求める。推論の後で変数を型で分け、範囲の外へ出る
//! 閉包を E2019 にする。

use eml_diagnostics::{Diagnostic, FileId, Label as DiagnosticLabel, TextRange};
use eml_extern::ExternType;
use eml_hir::{Body, EffectId, EffectKind, ExprId, ExprKind, IndexBase, Program, VarId};

use crate::kind::{Bound, KindOrigin, KindReason, Provenance, Span};
use crate::shape::lower_type;
use crate::store::TypeStore;
use crate::table::{Exporter, Label, Row, Table, Ty, TyShape};
use crate::ty::Linearity;
use crate::{IndexTarget, VarClass, codes};

use super::body::{BodyCheck, BodyTyping, CallRows, Expectation};
use super::report::Origin;

/// 型検査の間の `var` の変数。`node` は `ExprKind::Var` の式、`ty` は変数の型である。
#[derive(Debug, Clone, Copy)]
pub(crate) struct VarScope {
    pub node: ExprId,
    pub ty: Ty,
}

impl BodyCheck<'_, '_> {
    /// `var x = e` と、その範囲。初期値は範囲の外の row で検査し、範囲は今の row の前に `L` を足した row で検査する。
    /// 値は範囲の値である。
    pub(super) fn var_scope(
        &mut self,
        id: ExprId,
        var: VarId,
        init: ExprId,
        scope: ExprId,
        expectation: Expectation,
    ) -> Ty {
        let data = &self.body.vars[var];
        let ty = match data.ty {
            Some(annotation) => {
                let annotated = lower_type(self.table, &self.body.types, self.rigids, annotation);
                let range = self.body.types[annotation].range;
                self.check_expr(init, annotated, Origin::Annotation(range));
                annotated
            }
            None => self.infer_expr(init),
        };
        self.typing.vars.insert(var, VarScope { node: id, ty });
        let outer = self.ambient.clone();
        let inner = Row {
            labels: std::iter::once(Label {
                effect: data.effect,
                args: Vec::new(),
            })
            .chain(outer.labels.iter().cloned())
            .collect(),
            tail: outer.tail,
        };
        // 持ち越しのパスは範囲を handle と同じく読む
        self.typing.calls.insert(
            id,
            CallRows::Handle {
                body: inner.clone(),
                outer,
            },
        );
        let source = self.ambient_source.clone();
        self.with_ambient(inner, source, |this| match expectation {
            Expectation::Has(expected, origin) => {
                this.check_expr(scope, expected, origin);
                expected
            }
            Expectation::None => this.infer_expr(scope),
        })
    }

    /// 変数の名前の参照。
    pub(super) fn var_get(&mut self, id: ExprId, var: VarId) -> Ty {
        self.use_var(id, var, false);
        self.var_type(var)
    }

    /// `x := e`。
    pub(super) fn assign(&mut self, id: ExprId, var: VarId, value: ExprId) -> Ty {
        let ty = self.var_type(var);
        let origin = self.var_origin(var);
        self.check_expr(value, ty, origin);
        self.use_var(id, var, false);
        self.table.unit
    }

    /// `x[i] := e`。変数の型を `MutArray E` と単一化する。
    pub(super) fn index_assign(
        &mut self,
        id: ExprId,
        var: VarId,
        index: ExprId,
        value: ExprId,
    ) -> Ty {
        let element = self.array_var_element(id, var, true);
        self.check_index(index);
        self.check_expr(value, element, Origin::Inferred);
        self.element_unrestricted(id, element);
        self.use_var(id, var, true);
        self.table.unit
    }

    /// `e[i]`。変数の型がその時点で `Array E` なら変数を読んでから `Array` を読み、そうでなければ変数の型を
    /// `MutArray E` と単一化して配列の変数の要素を読む。変数でない式は `Array E` である。
    pub(super) fn index(&mut self, id: ExprId, base: IndexBase, index: ExprId) -> Ty {
        let (element, target) = match base {
            IndexBase::Var(var) => match self.array_element(self.var_type(var)) {
                Some(element) => (element, IndexTarget::Array),
                None => (
                    self.array_var_element(id, var, false),
                    IndexTarget::ArrayVar,
                ),
            },
            IndexBase::Expr(base) => {
                let found = self.infer_expr(base);
                let element = self.table.fresh_var();
                let array = self.array_type(ExternType::Array, element);
                let range = self.body.exprs[base].range;
                self.expect(range, array, found, &Origin::IndexBase);
                (element, IndexTarget::Array)
            }
        };
        self.check_index(index);
        self.element_unrestricted(id, element);
        match base {
            IndexBase::Var(var) => self.use_var(id, var, true),
            IndexBase::Expr(_) => {
                let row = Row::closed(vec![Label {
                    effect: self.program.panic(),
                    args: Vec::new(),
                }]);
                self.include_use(id, row, "this index");
            }
        }
        self.typing.indexes.insert(id, target);
        element
    }

    fn var_type(&self, var: VarId) -> Ty {
        self.typing
            .vars
            .get(var)
            .map_or(self.table.error, |scope| scope.ty)
    }

    fn var_origin(&self, var: VarId) -> Origin {
        let data = &self.body.vars[var];
        Origin::VarType {
            name: data.name.clone(),
            decl: data.range,
        }
    }

    fn array_type(&mut self, ty: ExternType, element: Ty) -> Ty {
        let def = self.table.extern_type(ty);
        self.table
            .alloc(TyShape::Con(def, vec![element], Vec::new()))
    }

    /// 型が `Array E` の形に決まっていれば、`E` を返す。
    fn array_element(&self, ty: Ty) -> Option<Ty> {
        let array = self.table.extern_type(ExternType::Array);
        match self.table.shape(ty) {
            TyShape::Con(def, args, _) if *def == array => Some(args[0]),
            _ => None,
        }
    }

    /// 変数の型を `MutArray E` と単一化し、`E` を返す。
    fn array_var_element(&mut self, id: ExprId, var: VarId, write: bool) -> Ty {
        let element = self.table.fresh_var();
        let array = self.array_type(ExternType::MutArray, element);
        let ty = self.var_type(var);
        let data = &self.body.vars[var];
        let origin = Origin::IndexedVar {
            name: data.name.clone(),
            decl: data.range,
            write,
        };
        let range = self.body.exprs[id].range;
        self.expect(range, array, ty, &origin);
        element
    }

    fn check_index(&mut self, index: ExprId) {
        let int = self.table.int;
        self.check_expr(index, int, Origin::Index);
    }

    /// 添字で読み書きする要素は `Unr` である。extern の行が要素の型変数を `Unr` に固定するのと同じである。
    /// `MutArray.empty` の要素の型変数は結果にだけ現れて固定されないので、この制約が `Lin` の要素を断る
    /// (docs/spec/types.md の「`var` の変数」)。
    fn element_unrestricted(&mut self, id: ExprId, element: Ty) {
        let range = self.body.exprs[id].range;
        self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table
                .kind_at_most(element, Bound::Const(Linearity::Unr))
        });
    }

    /// 変数を使う所は `L` の `perform` と同じく row に `L` を求める。添字は `Panic` も求める。
    fn use_var(&mut self, id: ExprId, var: VarId, index: bool) {
        let data = &self.body.vars[var];
        let mut labels = vec![Label {
            effect: data.effect,
            args: Vec::new(),
        }];
        if index {
            labels.push(Label {
                effect: self.program.panic(),
                args: Vec::new(),
            });
        }
        let name = format!("`{}`", data.name);
        self.include_use(id, Row::closed(labels), &name);
    }

    fn include_use(&mut self, id: ExprId, row: Row, name: &str) {
        let range = self.body.exprs[id].range;
        let report = !self.failed_calls.contains(&id);
        if !self.include_call_row(row, (id, 0), range, name, report) {
            self.failed_calls.insert(id);
        }
    }

    /// `effect` が `var` の地点のエフェクトで、`range` の位置がその範囲の外か。範囲の外でそのラベルを求める呼び出しは、
    /// 変数を使う関数が範囲の外へ出たときにだけ起こる。
    pub(super) fn escaped_var(&self, effect: EffectId, range: TextRange) -> bool {
        let EffectKind::Var { var, .. } = self.program[effect].kind else {
            return false;
        };
        let Some(scope) = self.typing.vars.get(var) else {
            return false;
        };
        let ExprKind::Var { body: inner, .. } = self.body.exprs[scope.node].kind else {
            unreachable!("a var scope is a var node")
        };
        !self.body.exprs[inner].range.contains_range(range)
    }
}

/// 本体の推論と包含とクラスの制約を解いた後、Kind の問題を解く前に、各変数を型で分ける。`Unr` の変数には `T ≤ Unr`
/// を足し、続けて範囲の外へ出る閉包を E2019 にする (docs/spec/types.md の「`var` の変数」と「範囲の外へ出る閉包」)。
pub(super) fn classify(
    program: &Program,
    file: FileId,
    body: &Body,
    table: &mut Table<'_>,
    types: &mut TypeStore,
    typing: &mut BodyTyping,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut_array = table.extern_type(ExternType::MutArray);
    for (var, data) in body.vars.iter() {
        let Some(scope) = typing.vars.get(var).copied() else {
            continue;
        };
        let class = match table.shape(scope.ty) {
            TyShape::Con(def, _, _) if *def == mut_array => VarClass::Array,
            _ => VarClass::Unr,
        };
        typing.var_classes.insert(var, class);
        if class == VarClass::Unr {
            let reason = KindReason::VarValue {
                var,
                name: data.name.clone(),
                ty: Exporter::diagnostic(table, types).export(scope.ty),
            };
            let span = Span {
                file,
                range: data.range,
            };
            let previous = table.set_kind_origin(Provenance::At(KindOrigin { span, reason }));
            table.kind_at_most(scope.ty, Bound::Const(Linearity::Unr));
            table.set_kind_origin(previous);
        }
    }
    for (var, _) in body.vars.iter() {
        let Some(scope) = typing.vars.get(var).copied() else {
            continue;
        };
        if let Some(diagnostic) = escape(program, file, body, table, types, typing, var, scope) {
            diagnostics.push(diagnostic);
        }
    }
}

/// 範囲の外へ出る閉包。`L` が、ノードの値の型、ノードの場所の row、範囲の外で束縛したものの型のどれかに現れれば、
/// 変数を使う関数が範囲の外へ出うる。範囲から値が出ていく道はこの3つだけである。地点ごとに1件だけ報告する。
/// `outer_locals` は隠された局所変数も含み、型を共有する束縛は同じ1つの出口なので、何件も報告すると読みにくい
/// (docs/spec/types.md の「範囲の外へ出る閉包」)。
#[allow(clippy::too_many_arguments)]
fn escape(
    program: &Program,
    file: FileId,
    body: &Body,
    table: &Table<'_>,
    types: &mut TypeStore,
    typing: &BodyTyping,
    var: VarId,
    scope: VarScope,
) -> Option<Diagnostic> {
    let data = &body.vars[var];
    let effect = data.effect;
    let name = &data.name;
    let error = |label: DiagnosticLabel| {
        Diagnostic::error(
            codes::VAR_ESCAPES,
            format!("a function that uses the variable `{name}` can escape its scope"),
            label,
        )
        .with_note(
            "each run of a `var` statement makes a new variable, so a function that uses one must not outlive its scope",
        )
    };
    let declared = DiagnosticLabel::new(
        file,
        data.range,
        format!("the scope of `{name}` starts here"),
    );
    let ExprKind::Var { last, .. } = body.exprs[scope.node].kind else {
        unreachable!("a var scope is a var node")
    };
    if let Some(&ty) = typing.exprs.get(scope.node)
        && table.effect_occurs(effect, ty)
    {
        let range = last.map_or(body.exprs[scope.node].range, |last| body.exprs[last].range);
        let label = DiagnosticLabel::new(
            file,
            range,
            format!("the value of the scope can hold a function that uses `{name}`"),
        );
        return Some(error(label).with_secondary(declared));
    }
    if let Some(CallRows::Handle { outer, .. }) = typing.calls.get(scope.node)
        && table.effect_occurs_in_row(effect, outer)
    {
        let carrier = table
            .resolve_row(outer)
            .labels
            .into_iter()
            .find(|label| {
                label.effect == effect
                    || label
                        .args
                        .iter()
                        .any(|&arg| table.effect_occurs(effect, arg))
            })
            .expect("the effect occurs in a label of the row");
        let shown = Exporter::diagnostic(table, types).label(&carrier);
        let shown = shown.display(types, &program.names).to_string();
        let label = DiagnosticLabel::new(
            file,
            data.range,
            format!(
                "the effect `{shown}` outside the scope can carry a function that uses `{name}`"
            ),
        );
        return Some(error(label));
    }
    let outer_var = data.outer_vars.iter().rev().find_map(|&outer| {
        let ty = typing.vars.get(outer)?.ty;
        let outer = &body.vars[outer];
        table
            .effect_occurs(effect, ty)
            .then(|| (outer.name.clone(), outer.range))
    });
    let outer_local = || {
        data.outer_locals.iter().rev().find_map(|&local| {
            let &ty = typing.locals.get(local)?;
            let local = &body.locals[local];
            table
                .effect_occurs(effect, ty)
                .then(|| (local.name.clone(), local.range))
        })
    };
    let (binding, range) = outer_var.or_else(outer_local)?;
    let label = DiagnosticLabel::new(
        file,
        range,
        format!(
            "`{binding}` is bound outside the scope, and its type can hold a function that uses `{name}`"
        ),
    );
    Some(error(label).with_secondary(declared))
}

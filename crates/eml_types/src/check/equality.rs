//! `==` と `!=` の比べ方を、引数の型から決める (docs/spec/declarations.md の標準の演算子の表)。型クラスがないので、
//! 比べられる型を `Int`、`String`、`Bool` に限る。

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::{ExprId, FunctionId};

use crate::table::{Ty, TyShape};
use crate::{Equality, Type, codes};

use super::body::BodyCheck;

/// `==` か `!=` の参照。引数の型は、後の文の単一化で決まることがある (`let` で束縛したラムダの引数など)。そのため、
/// 参照の位置では記録だけをし、本体の検査が終わってから比べ方を決める。
pub(super) struct Comparison {
    /// 演算子を指す呼ばれる側の式。
    pub callee: ExprId,
    pub operator: FunctionId,
    /// 参照を具体化した型。最初の矢印の引数が比べる値の型である。
    pub ty: Ty,
}

impl BodyCheck<'_, '_> {
    /// 本体の検査が終わってから呼ぶ。`self.diagnostics` はこの本体だけの診断なので、そこに誤りがあれば本体に誤りがある。
    pub(super) fn resolve_equalities(&mut self) {
        let body_has_error = self.diagnostics.iter().any(Diagnostic::is_error);
        let lang = self.program.lang;
        for comparison in std::mem::take(&mut self.comparisons) {
            // Prelude のシグネチャがなければ参照の型は `Error` で、矢印を持たない
            let TyShape::Fn { param, .. } = self.table.shape(comparison.ty).clone() else {
                continue;
            };
            let equality = match self.table.shape(param) {
                TyShape::Con(id, _) if *id == lang.int => Some(Equality::Int),
                TyShape::Con(id, _) if *id == lang.string => Some(Equality::String),
                TyShape::Con(id, _) if *id == lang.bool => Some(Equality::Bool),
                _ => None,
            };
            if let Some(equality) = equality {
                self.typing.equalities.insert(comparison.callee, equality);
                continue;
            }
            // 同じ本体に別の誤りがあるとき、決まらない型はその誤りの連鎖である。誤りを直せば型が決まるので、
            // E2006 を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if body_has_error && matches!(self.table.shape(param), TyShape::Var(_)) {
                continue;
            }
            let operand = self.table.export(param);
            // 報告済みの誤りの跡には診断を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if operand.contains_error() {
                continue;
            }
            let diagnostic = self.not_comparable(&comparison, &operand);
            self.diagnostics.push(diagnostic);
        }
    }

    /// 比べられない型の値を比べた (E2006)。演算子を指す。
    fn not_comparable(&self, comparison: &Comparison, operand: &Type) -> Diagnostic {
        let op = &self.program[comparison.operator].name;
        let names = &self.program.names;
        let lang = self.program.lang;
        let operand = operand.display(names);
        Diagnostic::error(
            codes::NOT_COMPARABLE,
            format!("values of type `{operand}` cannot be compared with `{op}`"),
            Label::new(
                self.file(),
                self.body.exprs[comparison.callee].range,
                format!("`{op}` cannot compare `{operand}`"),
            ),
        )
        .with_note(format!(
            "`==` and `!=` compare only values of type `{}`, `{}` and `{}`",
            names.ty(lang.int),
            names.ty(lang.string),
            names.ty(lang.bool)
        ))
    }
}

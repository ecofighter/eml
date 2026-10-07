//! `==` と `!=` で比べられない型の値を比べた参照を報告する (docs/spec/declarations.md の標準の演算子の表)。型クラスが
//! ないので、比べられる型を `Int`、`String`、`Bool` に限る。

use eml_diagnostics::{Diagnostic, Label};
use eml_extern::{Extern, ExternType};
use eml_hir::{ExprId, FunctionId, FunctionKind, ValueItem};

use crate::table::TyShape;
use crate::{Type, codes, equality};

use super::body::BodyCheck;

impl BodyCheck<'_, '_> {
    /// 本体の検査が終わってから、`usage::reliable` より前に呼ぶ。比べる値の型は後の文の単一化で決まることがある
    /// (`let` で束縛したラムダの引数など) ためと、E2006 を本体の誤りに数え、線形性の診断を連鎖させないためである
    /// (docs/spec/diagnostics.md の「連鎖する診断の抑止」)。`self.diagnostics` はこの本体だけの診断なので、そこに誤りが
    /// あれば本体に誤りがある。1つの比べ方の E2006 が別の比べ方の E2006 を抑えないよう、比べ方を見る前に1回だけ数える。
    pub(super) fn check_comparisons(&mut self) {
        let body_has_error = self.diagnostics.iter().any(Diagnostic::is_error);
        let mut found = Vec::new();
        for (callee, (decl, args)) in self.typing.instantiations.iter() {
            let ValueItem::Function(operator) = *decl else {
                continue;
            };
            if !matches!(
                self.program[operator].kind,
                FunctionKind::Extern(Some(Extern::Eq | Extern::Ne))
            ) {
                continue;
            }
            // `==` と `!=` は `a -> a -> Bool` なので、最初の型引数が比べる値の型である
            let operand = args[0];
            let exported = self.table.export(operand);
            if equality(self.program, &exported).is_some() {
                continue;
            }
            // 同じ本体に別の誤りがあるとき、決まらない型はその誤りの連鎖である。誤りを直せば型が決まるので、
            // E2006 を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if body_has_error && matches!(self.table.shape(operand), TyShape::Var(_)) {
                continue;
            }
            // 報告済みの誤りの跡には診断を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if exported.contains_error() {
                continue;
            }
            found.push(self.not_comparable(callee, operator, &exported));
        }
        self.diagnostics.extend(found);
    }

    /// 比べられない型の値を比べた (E2006)。演算子を指す。
    fn not_comparable(&self, callee: ExprId, operator: FunctionId, operand: &Type) -> Diagnostic {
        let op = &self.program[operator].name;
        let names = &self.program.names;
        let operand = operand.display(names);
        Diagnostic::error(
            codes::NOT_COMPARABLE,
            format!("values of type `{operand}` cannot be compared with `{op}`"),
            Label::new(
                self.file(),
                self.body.exprs[callee].range,
                format!("`{op}` cannot compare `{operand}`"),
            ),
        )
        .with_note(format!(
            "`==` and `!=` compare only values of type `{}`, `{}` and `{}`",
            names.ty(self.program.extern_type(ExternType::Int)),
            names.ty(self.program.extern_type(ExternType::String)),
            names.ty(self.program.lang.bool)
        ))
    }
}

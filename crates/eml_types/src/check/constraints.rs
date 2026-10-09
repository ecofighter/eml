//! 制約の解決 (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「制約を解く」)。参照ごとに具体化した
//! 制約を、本体の単一化が終わってから解く。証拠は記録しない。translate と一様な位置の計算が、参照ごとの型引数から
//! instance を引き直す。

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::{ClassId, ExprId, FunctionKind, TypeVarId, ValueItem};

use crate::codes;
use crate::store::{TypeId, TypeKind};
use crate::table::{Exporter, RigidVar, Ty, TyShape};

use super::body::BodyCheck;

/// 解けなかった制約。`root` は参照が求めた制約で、`leaf` は解いた先で解けなかった制約である。
enum Failure {
    NoInstance {
        root: (ClassId, Ty),
        leaf: (ClassId, Ty),
    },
    /// `root` は、型が `Error` を含むかを確かめるためだけに持つ。
    Ambiguous { root: (ClassId, Ty), class: ClassId },
}

impl BodyCheck<'_, '_> {
    /// 本体の検査が終わってから、`usage::reliable` より前に呼ぶ。制約の型は後の文の単一化で決まることがあるためと、
    /// E2006 と E2009 を本体の誤りに数え、線形性の診断を連鎖させないためである (今の E2006 の扱いと同じ)。
    pub(super) fn solve_constraints(&mut self) {
        let body_has_error = self.diagnostics.iter().any(Diagnostic::is_error);
        let givens = self.givens();
        let mut found = Vec::new();
        for (expr, (decl, args)) in self.typing.instantiations.iter() {
            for (class, ty) in self.wanted(*decl, args) {
                if let Some(failure) = self.solve(class, ty, &givens, body_has_error) {
                    found.push((expr, *decl, failure));
                    // 1つの参照から出た誤りは1つにまとめる
                    break;
                }
            }
        }
        let diagnostics: Vec<Diagnostic> = found
            .into_iter()
            .filter_map(|(expr, decl, failure)| self.constraint_error(expr, decl, failure))
            .collect();
        self.diagnostics.extend(diagnostics);
    }

    /// 本体に与えられた制約。シグネチャの制約を、上位クラスでたどって閉じたもの。instance のメソッドと既定のメソッドの
    /// シグネチャは、instance の文脈とクラスの制約を HIR が先頭に足してあるので、ここでは区別しない。
    fn givens(&self) -> Vec<(ClassId, RigidVar)> {
        let Some(signature) = &self.function.signature else {
            return Vec::new();
        };
        let mut givens = Vec::new();
        for constraint in &signature.constraints {
            let var = self.rigids.vars()[index(constraint.var)];
            givens.push((constraint.class, var));
            for superclass in self.program.superclasses(constraint.class) {
                givens.push((superclass, var));
            }
        }
        givens
    }

    /// 参照が求める制約。関数はシグネチャの制約、メソッドはクラスの制約 `C T` (T は最初の型引数) とメソッド自身の制約
    /// である。具体化の型引数は `Generics` の順に並ぶので、制約の型変数の番号で引ける。
    fn wanted(&self, decl: ValueItem, args: &[Ty]) -> Vec<(ClassId, Ty)> {
        let (own, signature) = match decl {
            ValueItem::Function(id) => (None, self.program[id].signature.as_ref()),
            ValueItem::Method(id) => {
                let method = &self.program[id];
                (Some((method.class, args[0])), Some(&method.signature))
            }
            ValueItem::Operation(_) | ValueItem::Constructor(_) => return Vec::new(),
        };
        let constraints = signature
            .into_iter()
            .flat_map(|signature| &signature.constraints)
            .map(|constraint| (constraint.class, args[index(constraint.var)]));
        own.into_iter().chain(constraints).collect()
    }

    /// 作業の列で解く。instance の文脈は頭の型引数へ写すので、列に足す型は元の型の部分になり、列はいつか尽きる。
    /// 解けなければ最初の失敗を返す。
    fn solve(
        &self,
        class: ClassId,
        ty: Ty,
        givens: &[(ClassId, RigidVar)],
        body_has_error: bool,
    ) -> Option<Failure> {
        let root = (class, ty);
        let mut work = vec![root];
        let mut next = 0;
        while let Some(&(class, ty)) = work.get(next) {
            next += 1;
            match self.table.shape(ty) {
                TyShape::Con(id, args) => {
                    let Some(instance) = self.program.instance(class, *id) else {
                        return Some(Failure::NoInstance {
                            root,
                            leaf: (class, ty),
                        });
                    };
                    for constraint in &self.program[instance].context {
                        work.push((constraint.class, args[index(constraint.var)]));
                    }
                }
                TyShape::Rigid(var) => {
                    // 操作ごとの型変数は与えられた制約に入らないので、ここで E2006 になる
                    if !givens.contains(&(class, *var)) {
                        return Some(Failure::NoInstance {
                            root,
                            leaf: (class, ty),
                        });
                    }
                }
                // 同じ本体に別の誤りがあるとき、決まらない型はその誤りの連鎖である。誤りを直せば型が決まるので、
                // E2009 を重ねない
                TyShape::Var(_) if body_has_error => {}
                TyShape::Var(_) => return Some(Failure::Ambiguous { root, class }),
                // タプルの instance は Task 9 で足す
                TyShape::Record(_) | TyShape::Fn { .. } => {
                    return Some(Failure::NoInstance {
                        root,
                        leaf: (class, ty),
                    });
                }
                // 報告済みの誤りの跡には診断を重ねない
                TyShape::Error => {}
            }
        }
        None
    }

    /// 解けなかった制約の診断。型が `Error` を含めば、報告済みの誤りの連鎖なので `None` を返す。
    fn constraint_error(
        &mut self,
        expr: ExprId,
        decl: ValueItem,
        failure: Failure,
    ) -> Option<Diagnostic> {
        let program = self.program;
        let name = program.value_name(decl);
        let range = self.body.exprs[expr].range;
        match failure {
            Failure::NoInstance { root, leaf } => {
                let mut exporter = Exporter::new(self.table, self.types);
                let root_ty = exporter.export(root.1);
                let leaf_ty = exporter.export(leaf.1);
                if self.types.contains_error(root_ty) || self.types.contains_error(leaf_ty) {
                    return None;
                }
                let root_shown = self.show_constraint(root.0, root_ty);
                let leaf_shown = self.show_constraint(leaf.0, leaf_ty);
                let mut diagnostic = Diagnostic::error(
                    codes::NO_INSTANCE,
                    format!(
                        "no instance of `{}` for `{}`",
                        self.program.names.class(leaf.0),
                        self.types.display(leaf_ty, &self.program.names)
                    ),
                    Label::new(
                        self.file(),
                        range,
                        format!("`{name}` requires `{root_shown}`"),
                    ),
                );
                if root_shown != leaf_shown {
                    diagnostic = diagnostic.with_note(format!("needed for `{root_shown}`"));
                }
                // 操作ごとの型変数 (`OpVar`) はシグネチャに書けないので、help を出さない
                if matches!(self.types.kind(leaf_ty), TypeKind::Rigid(_))
                    && matches!(self.function.kind, FunctionKind::Defined)
                {
                    diagnostic = diagnostic.with_help(format!(
                        "add `{leaf_shown} =>` to the signature of `{}`",
                        self.function.name
                    ));
                }
                Some(diagnostic)
            }
            Failure::Ambiguous { root, class } => {
                let root_ty = Exporter::new(self.table, self.types).export(root.1);
                if self.types.contains_error(root_ty) {
                    return None;
                }
                Some(
                    Diagnostic::error(
                        codes::AMBIGUOUS_CONSTRAINT,
                        format!(
                            "cannot decide which instance of `{}` `{name}` uses",
                            self.program.names.class(class)
                        ),
                        Label::new(self.file(), range, "the type here is never decided"),
                    )
                    .with_help("add a type annotation"),
                )
            }
        }
    }

    /// 制約 `C T` の表示。`T` が引数のある型構成子か関数型なら、括弧で囲む (`Same (Box (Int -> Int))`)。タプルは
    /// 自分の括弧を持つ。文字列の空白で決めないのは、`(Int, Int) -> Int` のように括弧で始まる関数型があるためである。
    fn show_constraint(&self, class: ClassId, ty: TypeId) -> String {
        let names = &self.program.names;
        let shown = self.types.display(ty, names);
        let class = names.class(class);
        match self.types.kind(ty) {
            TypeKind::Fn { .. } => format!("{class} ({shown})"),
            TypeKind::Con { args, .. } if !args.is_empty() => format!("{class} ({shown})"),
            _ => format!("{class} {shown}"),
        }
    }
}

fn index(var: TypeVarId) -> usize {
    u32::from(var.into_raw()) as usize
}

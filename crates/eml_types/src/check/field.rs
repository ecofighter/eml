//! 射影と更新の検査。フィールドは、射影や更新の時点で分かっている値の型から引く
//! (docs/spec/types.md の「フィールドの解決」)。

use eml_diagnostics::{Diagnostic, Label, TextRange};
use eml_hir::{
    ConstructorId, ExprId, ExprKind, FieldKey, FieldName, FieldUse, LocalId, PatKind, Res,
    TypeDefKind, ValueItem,
};

use crate::codes;
use crate::kind::KindReason;
use crate::table::{Ty, TyShape};
use crate::{FieldTarget, Update};

use super::body::BodyCheck;
use super::report::Origin;

/// 型から引いたフィールド。
struct ResolvedField {
    target: FieldTarget,
    /// フィールドの型。型引数で具体化してある。
    ty: Ty,
    /// ほかのフィールドの番号と、具体化した型。
    rest: Vec<(u32, Ty)>,
}

/// フィールドを引く式の種類。診断の言い方を変える。
#[derive(Clone, Copy)]
enum Site {
    Projection,
    Update,
}

impl BodyCheck<'_, '_> {
    /// 射影 `e.f`。結果はフィールドの型で、取り出さない残りのフィールドは `discarded` に残す。
    pub(super) fn projection(&mut self, id: ExprId, base: ExprId, field: &FieldUse) -> Ty {
        let base_ty = self.infer_expr(base);
        if let TyShape::Var(_) = self.table.shape(base_ty) {
            self.unknown_record(base, Site::Projection, &key_text(&field.field));
            return self.table.error;
        }
        let Some(resolved) = self.resolve_field(id, base_ty, field, Site::Projection) else {
            return self.table.error;
        };
        self.typing.fields.insert(id, resolved.target);
        self.typing.discarded.insert(id, resolved.rest);
        resolved.ty
    }

    /// 更新 `{ e | f = v }`。各 `v` をフィールドの型に対して書いた順に検査する。結果の型は `e` の型のままで、上書き
    /// される古いフィールドは `discarded` に残す。
    pub(super) fn update(
        &mut self,
        id: ExprId,
        base: ExprId,
        fields: &[(FieldName, ExprId)],
    ) -> Ty {
        let base_ty = self.infer_expr(base);
        let target = self.update_target(id, base, base_ty, &fields[0].0);
        let mut written = Vec::new();
        let mut discarded = Vec::new();
        let mut resolved_all = target.is_some();
        for (name, value) in fields {
            let resolved = match &target {
                Some((ctor, types)) => {
                    let key = FieldKey::Name(name.name.clone());
                    let index = self.program[*ctor].field_index(&name.name);
                    if index.is_none() {
                        self.unknown_constructor_field(base_ty, *ctor, &key, name.range);
                    }
                    index.map(|index| (*ctor, index, types[index as usize]))
                }
                None => None,
            };
            match resolved {
                Some((ctor, field, ty)) => {
                    self.check_expr(*value, ty, Origin::Field { ctor, field });
                    written.push(field);
                    discarded.push((field, ty));
                }
                None => {
                    resolved_all = false;
                    self.infer_expr(*value);
                }
            }
        }
        if let (true, Some((ctor, types))) = (resolved_all, target) {
            let update = Update {
                ctor,
                written,
                fields: types,
            };
            self.typing.updates.insert(id, update);
            self.typing.discarded.insert(id, discarded);
        }
        base_ty
    }

    /// 更新が作り直すコンストラクタと、型引数で具体化したフィールドの型。元の値の型がコンストラクタが1つの `data` で
    /// なければ、最初のフィールドについて1件だけ報告する。誤りは型のもので、フィールドごとに変わらないためである
    /// (docs/implementation/diagnostics.md の E2014)。
    fn update_target(
        &mut self,
        id: ExprId,
        base: ExprId,
        base_ty: Ty,
        first: &FieldName,
    ) -> Option<(ConstructorId, Vec<Ty>)> {
        match self.table.shape(base_ty).clone() {
            TyShape::Var(_) => {
                self.unknown_record(base, Site::Update, &first.name);
                return None;
            }
            TyShape::Con(def, _) => {
                if let TypeDefKind::Data { constructors } = &self.program[def].kind
                    && let [ctor] = constructors[..]
                {
                    let types = self.constructor_fields(id, base_ty, ctor)?;
                    return Some((ctor, types));
                }
            }
            _ => {}
        }
        // フィールドを名前で持たない型の E2014 は、射影と同じく `resolve_field` が型ごとの help を付けて出す
        let field = FieldUse {
            field: FieldKey::Name(first.name.clone()),
            range: first.range,
        };
        let resolved = self.resolve_field(id, base_ty, &field, Site::Update);
        debug_assert!(
            resolved.is_none(),
            "only a single-constructor data type has named fields"
        );
        None
    }

    /// 型の決まった `base_ty` から、フィールドを引く。タプルは番号で、コンストラクタが1つの `data` は名前で引く。
    /// 更新は名前だけを使うので、タプルの更新はここで E2014 になる。型変数は呼び出し側が先に E2013 にする。
    fn resolve_field(
        &mut self,
        id: ExprId,
        base_ty: Ty,
        field: &FieldUse,
        site: Site,
    ) -> Option<ResolvedField> {
        let key = &field.field;
        match self.table.shape(base_ty).clone() {
            TyShape::Error => None,
            TyShape::Tuple(elements) => match key {
                FieldKey::Index(index) if (*index as usize) < elements.len() => {
                    let rest = (0..elements.len() as u32)
                        .filter(|other| other != index)
                        .map(|other| (other, elements[other as usize]))
                        .collect();
                    Some(ResolvedField {
                        target: FieldTarget::Tuple {
                            arity: elements.len() as u32,
                            index: *index,
                        },
                        ty: elements[*index as usize],
                        rest,
                    })
                }
                _ => {
                    let help = match site {
                        // `Unit` は要素のないタプルなので、示せる番号の範囲がなく、作り直すタプルもない
                        _ if elements.is_empty() => {
                            format!("`{}` has no fields", self.show(base_ty))
                        }
                        Site::Projection => format!(
                            "the elements of this tuple are numbered from `0` to `{}`",
                            elements.len() - 1
                        ),
                        Site::Update => {
                            "a tuple cannot be updated; build a new tuple instead".to_string()
                        }
                    };
                    self.no_such_field(base_ty, key, field, |diagnostic| {
                        diagnostic.with_help(help)
                    });
                    None
                }
            },
            TyShape::Con(def, _) => {
                let TypeDefKind::Data { constructors } = &self.program[def].kind else {
                    self.no_such_field(base_ty, key, field, |diagnostic| diagnostic);
                    return None;
                };
                match constructors[..] {
                    [ctor] => return self.constructor_field(id, base_ty, ctor, key, field),
                    // `=` のない `data` は E1025 を報告済みなので、重ねない
                    [] => return None,
                    _ => {}
                }
                let note = format!(
                    "`{}` has more than one constructor",
                    self.program.names.ty(def)
                );
                self.no_such_field(base_ty, key, field, |diagnostic| {
                    diagnostic
                        .with_note(note)
                        .with_help("take the value apart with `match`")
                });
                None
            }
            TyShape::Var(_) | TyShape::Rigid(_) | TyShape::Fn { .. } => {
                self.no_such_field(base_ty, key, field, |diagnostic| diagnostic);
                None
            }
        }
    }

    /// コンストラクタが1つの `data` の、名前で指したフィールド。
    fn constructor_field(
        &mut self,
        id: ExprId,
        base_ty: Ty,
        ctor: ConstructorId,
        key: &FieldKey,
        field: &FieldUse,
    ) -> Option<ResolvedField> {
        let index = match key {
            FieldKey::Name(name) => self.program[ctor].field_index(name),
            FieldKey::Index(_) => None,
        };
        let Some(index) = index else {
            self.unknown_constructor_field(base_ty, ctor, key, field.range);
            return None;
        };
        let fields = self.constructor_fields(id, base_ty, ctor)?;
        let rest = (0..fields.len() as u32)
            .filter(|&other| other != index)
            .map(|other| (other, fields[other as usize]))
            .collect();
        Some(ResolvedField {
            target: FieldTarget::Constructor { ctor, field: index },
            ty: fields[index as usize],
            rest,
        })
    }

    /// E2014。コンストラクタが1つの `data` に、そのフィールドがない。help で宣言したフィールドを並べる。
    fn unknown_constructor_field(
        &mut self,
        base_ty: Ty,
        ctor: ConstructorId,
        key: &FieldKey,
        range: TextRange,
    ) {
        let declared: Vec<&str> = self.program[ctor]
            .field_names
            .iter()
            .flatten()
            .map(|field| field.name.as_str())
            .collect();
        let help = (!declared.is_empty()).then(|| {
            let noun = if declared.len() == 1 {
                "field is"
            } else {
                "fields are"
            };
            format!("the {noun} {}", listing(&declared))
        });
        let field = FieldUse {
            field: key.clone(),
            range,
        };
        self.no_such_field(base_ty, key, &field, |diagnostic| match help {
            Some(help) => diagnostic.with_help(help),
            None => diagnostic,
        });
    }

    /// コンストラクタが1つの `data` の、すべてのフィールドの型。コンストラクタをパターンと同じく具体化し、結果の型を
    /// `base_ty` と単一化して、フィールドの型を型引数で具体化する。
    fn constructor_fields(
        &mut self,
        id: ExprId,
        base_ty: Ty,
        ctor: ConstructorId,
    ) -> Option<Vec<Ty>> {
        let range = self.body.exprs[id].range;
        self.with_kind_origin(range, KindReason::Unified, |this| {
            let instantiated = this.instantiate_constructor(ctor)?;
            let mut ty = instantiated;
            let mut fields = Vec::new();
            for _ in &this.program[ctor].fields {
                let TyShape::Fn { param, ret, .. } = this.table.shape(ty).clone() else {
                    return None;
                };
                fields.push(param);
                ty = ret;
            }
            // `base_ty` は同じ `data` の型なので、新しい変数の型引数との単一化は失敗しない
            this.table.unify(ty, base_ty).ok()?;
            Some(fields)
        })
    }

    /// E2013。help は、値がラムダの引数なら引数の型の明示を、そのほかは値の型の明示を示す。
    fn unknown_record(&mut self, base: ExprId, site: Site, field: &str) {
        let range = self.body.exprs[base].range;
        let message = match site {
            Site::Projection => {
                format!("the type of this value must be known to access its field `{field}`")
            }
            Site::Update => "the type of this value must be known to update its fields".to_string(),
        };
        let help = match &self.body.exprs[base].kind {
            ExprKind::Path(Res::Local(local)) if self.body.locals[*local].is_hidden() => format!(
                "write the section as a lambda with an annotated parameter, as in `fn (p : T) -> p.{field}`"
            ),
            ExprKind::Path(Res::Local(local)) if self.is_lambda_parameter(*local) => {
                let name = &self.body.locals[*local].name;
                format!("annotate the type of the parameter, as in `fn ({name} : T) -> …`")
            }
            ExprKind::Path(Res::Local(local)) => {
                let name = &self.body.locals[*local].name;
                format!("annotate the type of the value, as in `({name} : T)`")
            }
            _ => "annotate the type of the value, as in `(… : T)`".to_string(),
        };
        self.diagnostics.push(
            Diagnostic::error(
                codes::FIELD_NEEDS_KNOWN_TYPE,
                message,
                Label::new(
                    self.file(),
                    range,
                    "the type of this value is not known here",
                ),
            )
            .with_help(help),
        );
    }

    /// `local` が、ラムダの引数のパターンがそのまま束縛した変数か。
    fn is_lambda_parameter(&self, local: LocalId) -> bool {
        self.body.exprs.iter().any(|(_, expr)| match &expr.kind {
            ExprKind::Lambda(closure) => closure
                .params
                .iter()
                .any(|&pat| self.body.pats[pat].kind == PatKind::Bind(local)),
            _ => false,
        })
    }

    /// E2014。`extra` が型ごとの note と help を足す。
    fn no_such_field(
        &mut self,
        base_ty: Ty,
        key: &FieldKey,
        field: &FieldUse,
        extra: impl FnOnce(Diagnostic) -> Diagnostic,
    ) {
        let shown = self.show(base_ty);
        let diagnostic = Diagnostic::error(
            codes::NO_SUCH_FIELD,
            format!("no field `{}` on type `{shown}`", key_text(key)),
            Label::new(self.file(), field.range, "unknown field"),
        );
        self.diagnostics.push(extra(diagnostic));
    }

    /// コンストラクタの型をパターンと同じく具体化する。シグネチャがなければ `None` である。
    fn instantiate_constructor(&mut self, ctor: ConstructorId) -> Option<Ty> {
        self.instantiate(ValueItem::Constructor(ctor))
            .map(|(ty, _)| ty)
    }
}

fn key_text(key: &FieldKey) -> String {
    match key {
        FieldKey::Name(name) => name.clone(),
        FieldKey::Index(index) => index.to_string(),
    }
}

/// `` `a` ``、`` `a` and `b` ``、`` `a`, `b` and `c` `` の形の並び。
fn listing(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

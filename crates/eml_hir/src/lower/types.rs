use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};
use la_arena::Arena;

use super::{PathName, path_name};
use crate::codes;
use crate::def_map::{Resolver, TypeItem};
use crate::hir::{
    EffectRef, Generics, RowRef, RowVarDecl, RowVarId, TypeRef, TypeRefId, TypeRefKind, TypeVarDecl,
};

/// 型変数と row 変数の名前の引き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Vars {
    /// シグネチャ。新しい変数の名前を表に入れる。
    Define,
    /// 本体の注釈。シグネチャの表にある名前だけを使える (docs/spec/types.md の「推論」)。
    Signature,
    /// `data` の宣言のフィールド。宣言の型引数だけを使える。宣言は row 変数を持たない
    /// (docs/spec/declarations.md の「`data` と `type`」)。
    Data,
}

pub(super) struct TypeLowering<'a> {
    pub file: FileId,
    pub types: &'a mut Arena<TypeRef>,
    pub generics: &'a mut Generics,
    pub items: Resolver<'a>,
    pub vars: Vars,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}

impl TypeLowering<'_> {
    pub fn lower(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        let Some(ty) = ty else {
            return self.alloc(TypeRefKind::Error, fallback);
        };
        let range = ty.range();
        let kind = match ty {
            ast::Type::PathType(path) => match path_name(path.path()) {
                PathName::Plain(name) => self.applied(&name, Vec::new(), range),
                PathName::Qualified => {
                    self.unsupported(range, "qualified names are not supported yet")
                }
                PathName::Missing => TypeRefKind::Error,
            },
            ast::Type::ParenType(paren) => return self.lower(paren.ty(), range),
            ast::Type::FnType(function) => {
                let param = self.lower(function.param(), range);
                let row = match function.row() {
                    Some(row) => self.row(&row),
                    None => RowRef::Omitted,
                };
                let ret = self.lower(function.ret(), range);
                TypeRefKind::Fn { param, row, ret }
            }
            ast::Type::VarType(var) => match var.name() {
                Some(name) => self.type_var(&name, range),
                None => TypeRefKind::Error,
            },
            ast::Type::AppType(app) => {
                // 型引数を先に変換し、型の名前が誤っていても型引数の中の誤りを報告する
                let args = app
                    .args()
                    .map(|arg| {
                        let range = arg.range();
                        self.lower(Some(arg), range)
                    })
                    .collect();
                match path_name(app.path()) {
                    PathName::Plain(name) => self.applied(&name, args, range),
                    PathName::Qualified => {
                        self.unsupported(range, "qualified names are not supported yet")
                    }
                    PathName::Missing => TypeRefKind::Error,
                }
            }
            ast::Type::TupleType(tuple) => TypeRefKind::Tuple(
                tuple
                    .elements()
                    .map(|element| {
                        let element_range = element.range();
                        self.lower(Some(element), element_range)
                    })
                    .collect(),
            ),
        };
        self.alloc(kind, range)
    }

    fn applied(
        &mut self,
        name: &SyntaxToken,
        args: Vec<TypeRefId>,
        range: TextRange,
    ) -> TypeRefKind {
        match self.items.type_item(name.text()) {
            Some(TypeItem::Type(id)) => {
                let expected = self.items.type_params(id);
                if args.len() != expected {
                    self.arity_error(name.text(), expected, args.len(), range);
                    return TypeRefKind::Error;
                }
                TypeRefKind::Con(id, args)
            }
            Some(TypeItem::Effect(_)) | None => {
                self.diagnostics.push(Diagnostic::error(
                    codes::UNDEFINED_TYPE,
                    format!("cannot find type `{}`", name.text()),
                    Label::new(self.file, range, "not found in this scope"),
                ));
                TypeRefKind::Error
            }
        }
    }

    /// 型とエフェクトの型引数の個数の誤り (E1015)。
    fn arity_error(&mut self, name: &str, expected: usize, given: usize, range: TextRange) {
        let given = match given {
            1 => "1 was given".to_string(),
            n => format!("{n} were given"),
        };
        self.diagnostics.push(Diagnostic::error(
            codes::TYPE_ARGUMENT_COUNT,
            format!("`{name}` takes {}, but {given}", type_arguments(expected)),
            Label::new(
                self.file,
                range,
                format!("expected {}", type_arguments(expected)),
            ),
        ));
    }

    fn row(&mut self, row: &ast::EffectRow) -> RowRef {
        let mut valid = true;
        let tail = match row.tail() {
            Some(tail) => {
                let id = self.row_var(&tail);
                valid &= id.is_some();
                id
            }
            None => None,
        };
        let mut effects = Vec::new();
        for effect in row.effects() {
            let name = match path_name(effect.path()) {
                PathName::Plain(name) => name,
                PathName::Qualified => {
                    self.diagnostics.push(Diagnostic::not_yet_supported(
                        self.file,
                        effect.range(),
                        "qualified names are not supported yet",
                    ));
                    valid = false;
                    continue;
                }
                PathName::Missing => continue,
            };
            match self.items.type_item(name.text()) {
                Some(TypeItem::Effect(id)) => {
                    let args: Vec<TypeRefId> = effect
                        .args()
                        .map(|arg| {
                            let range = arg.range();
                            self.lower(Some(arg), range)
                        })
                        .collect();
                    let expected = self.items.effect_params(id);
                    if args.len() != expected {
                        self.arity_error(name.text(), expected, args.len(), effect.range());
                        valid = false;
                        continue;
                    }
                    effects.push(EffectRef { effect: id, args });
                }
                Some(TypeItem::Type(_)) | None => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::UNDEFINED_TYPE,
                        format!("cannot find effect `{}`", name.text()),
                        Label::new(self.file, effect.range(), "not found in this scope"),
                    ));
                    valid = false;
                }
            }
        }
        let range = row.range();
        match (valid, tail) {
            (false, _) => RowRef::Error,
            (true, Some(tail)) => RowRef::Open {
                effects,
                tail,
                range,
            },
            (true, None) => RowRef::Closed { effects, range },
        }
    }

    fn type_var(&mut self, name: &SyntaxToken, range: TextRange) -> TypeRefKind {
        let text = name.text();
        if let Some((id, _)) = self
            .generics
            .type_vars
            .iter()
            .find(|(_, var)| var.name == text)
        {
            return TypeRefKind::Var(id);
        }
        if self.vars == Vars::Define {
            let id = self.generics.type_vars.alloc(TypeVarDecl {
                name: text.to_string(),
                range,
            });
            return TypeRefKind::Var(id);
        }
        self.diagnostics.push(Diagnostic::error(
            codes::UNDEFINED_TYPE,
            format!("cannot find type variable `{text}`"),
            Label::new(self.file, range, self.undeclared()),
        ));
        TypeRefKind::Error
    }

    fn row_var(&mut self, name: &SyntaxToken) -> Option<RowVarId> {
        let text = name.text();
        let range = name.text_range();
        if let Some((id, _)) = self
            .generics
            .row_vars
            .iter()
            .find(|(_, var)| var.name == text)
        {
            return Some(id);
        }
        if self.vars == Vars::Define {
            return Some(self.generics.row_vars.alloc(RowVarDecl {
                name: text.to_string(),
                range,
            }));
        }
        self.diagnostics.push(Diagnostic::error(
            codes::UNDEFINED_TYPE,
            format!("cannot find row variable `{text}`"),
            Label::new(self.file, range, self.undeclared()),
        ));
        None
    }

    /// 表にない変数の名前を書いたときのラベル。
    fn undeclared(&self) -> &'static str {
        match self.vars {
            Vars::Data => "not a parameter of this `data` declaration",
            Vars::Define | Vars::Signature => "not found in the signature",
        }
    }

    fn unsupported(&mut self, range: TextRange, message: &str) -> TypeRefKind {
        self.diagnostics
            .push(Diagnostic::not_yet_supported(self.file, range, message));
        TypeRefKind::Error
    }

    fn alloc(&mut self, kind: TypeRefKind, range: TextRange) -> TypeRefId {
        self.types.alloc(TypeRef { kind, range })
    }
}

fn type_arguments(n: usize) -> String {
    if n == 1 {
        "1 type argument".to_string()
    } else {
        format!("{n} type arguments")
    }
}

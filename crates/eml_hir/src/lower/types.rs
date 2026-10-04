use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};
use la_arena::Arena;

use super::scope::{ItemScope, TypeItem};
use crate::codes;
use crate::hir::{
    Generics, RowRef, RowVarDecl, RowVarId, TypeRef, TypeRefId, TypeRefKind, TypeVarDecl,
};

pub(super) struct TypeLowering<'a> {
    pub file: FileId,
    pub types: &'a mut Arena<TypeRef>,
    pub generics: &'a mut Generics,
    pub items: &'a ItemScope,
    /// シグネチャなら真で、新しい変数の名前を表に入れる。本体の注釈では表にある名前だけを使える。
    pub define: bool,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}

impl TypeLowering<'_> {
    pub fn lower(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        let Some(ty) = ty else {
            return self.alloc(TypeRefKind::Error, fallback);
        };
        let range = ty.range();
        let kind = match ty {
            ast::Type::PathType(path) => self.path(&path, range),
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
            ast::Type::AppType(_) => {
                self.unsupported(range, "type applications are not supported yet")
            }
            ast::Type::TupleType(_) => self.unsupported(range, "tuple types are not supported yet"),
        };
        self.alloc(kind, range)
    }

    fn path(&mut self, path: &ast::PathType, range: TextRange) -> TypeRefKind {
        let segments: Vec<SyntaxToken> = path.segments().collect();
        let [name] = segments.as_slice() else {
            return self.unsupported(range, "qualified names are not supported yet");
        };
        match self.items.type_item(name.text()) {
            Some(TypeItem::Builtin(builtin)) => TypeRefKind::Builtin(builtin),
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
            let Some(name) = effect.name() else {
                continue;
            };
            match self.items.type_item(name.text()) {
                Some(TypeItem::Effect(effect)) => effects.push(effect),
                Some(TypeItem::Builtin(_)) | None => {
                    // ユーザー定義のエフェクトは段階3で入れる。宣言も E0004 になるので、ここでは未定義として扱う
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
        if self.define {
            let id = self.generics.type_vars.alloc(TypeVarDecl {
                name: text.to_string(),
                range,
            });
            return TypeRefKind::Var(id);
        }
        self.diagnostics.push(Diagnostic::error(
            codes::UNDEFINED_TYPE,
            format!("cannot find type variable `{text}`"),
            Label::new(self.file, range, "not found in the signature"),
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
        if self.define {
            return Some(self.generics.row_vars.alloc(RowVarDecl {
                name: text.to_string(),
                range,
            }));
        }
        self.diagnostics.push(Diagnostic::error(
            codes::UNDEFINED_TYPE,
            format!("cannot find row variable `{text}`"),
            Label::new(self.file, range, "not found in the signature"),
        ));
        None
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

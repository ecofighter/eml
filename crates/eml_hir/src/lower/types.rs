use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};
use la_arena::Arena;
use rowan::ast::AstNode;

use crate::builtin::BuiltinType;
use crate::hir::{EffectRef, RowRef, TypeRef, TypeRefId, TypeRefKind};
use crate::{codes, not_yet_supported};

pub(super) struct TypeLowering<'a> {
    pub file: FileId,
    pub types: &'a mut Arena<TypeRef>,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}

impl TypeLowering<'_> {
    pub fn lower(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        let Some(ty) = ty else {
            return self.alloc(TypeRefKind::Error, fallback);
        };
        let range = ty.syntax().text_range();
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
            ast::Type::VarType(_) => {
                self.unsupported(range, "type variables are not supported yet")
            }
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
        match BuiltinType::from_name(name.text()) {
            Some(builtin) => TypeRefKind::Builtin(builtin),
            None => {
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
        if let Some(tail) = row.tail() {
            self.diagnostics.push(not_yet_supported(
                self.file,
                tail.text_range(),
                "row variables are not supported yet",
            ));
            valid = false;
        }
        let mut effects = Vec::new();
        for effect in row.effects() {
            let Some(name) = effect.name() else {
                continue;
            };
            if name.text() == "IO" {
                effects.push(EffectRef::Io);
            } else {
                // ユーザー定義のエフェクトは段階3で入れる。宣言も E0004 になるので、ここでは未定義として扱う
                self.diagnostics.push(Diagnostic::error(
                    codes::UNDEFINED_TYPE,
                    format!("cannot find effect `{}`", name.text()),
                    Label::new(
                        self.file,
                        effect.syntax().text_range(),
                        "not found in this scope",
                    ),
                ));
                valid = false;
            }
        }
        if valid {
            RowRef::Closed {
                effects,
                range: row.syntax().text_range(),
            }
        } else {
            RowRef::Error
        }
    }

    fn unsupported(&mut self, range: TextRange, message: &str) -> TypeRefKind {
        self.diagnostics
            .push(not_yet_supported(self.file, range, message));
        TypeRefKind::Error
    }

    fn alloc(&mut self, kind: TypeRefKind, range: TextRange) -> TypeRefId {
        self.types.alloc(TypeRef { kind, range })
    }
}

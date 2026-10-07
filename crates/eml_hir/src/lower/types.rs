use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};
use la_arena::Arena;

use super::{NameKind, NameUse, not_found, path_name, unresolved};
use crate::codes;
use crate::def_map::{Resolved, Resolver};
use crate::hir::{
    EffectRef, Generics, RowRef, RowVarDecl, RowVarId, TypeRef, TypeRefId, TypeRefKind, TypeVarDecl,
};
use crate::program::TypeItem;

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
    /// `pub` の item の型を変換するときの、その item の名前。型に同じモジュールの `pub` でない型かエフェクトが現れたら
    /// E1032 にする (docs/spec/modules.md の「公開の範囲」)。`pub` でない item と本体の注釈では `None` である。
    pub public_item: Option<&'a str>,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}

impl TypeLowering<'_> {
    pub fn lower(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        let Some(ty) = ty else {
            return self.alloc(TypeRefKind::Error, fallback);
        };
        let range = ty.range();
        // E1032 は型の全体ではなく、書いた名前 (修飾子を含む) を指す
        let path_range = match &ty {
            ast::Type::PathType(path) => path.path(),
            ast::Type::AppType(app) => app.path(),
            _ => None,
        }
        .map(|path| path.range());
        let kind = match ty {
            ast::Type::PathType(path) => match path_name(path.path()).at(range) {
                Some(at) => self.applied(&at, Vec::new(), range),
                None => TypeRefKind::Error,
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
                match path_name(app.path()).at(range) {
                    Some(at) => self.applied(&at, args, range),
                    None => TypeRefKind::Error,
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
        if let (TypeRefKind::Con(id, _), Some(path_range)) = (&kind, path_range) {
            self.check_exposed(TypeItem::Type(*id), path_range);
        }
        self.alloc(kind, range)
    }

    fn applied(&mut self, at: &NameUse<'_>, args: Vec<TypeRefId>, range: TextRange) -> TypeRefKind {
        match self.items.type_item(at.name) {
            Resolved::Found(TypeItem::Type(id)) => {
                let expected = self.items.type_params(id);
                if args.len() != expected {
                    self.arity_error(&at.written(), expected, args.len(), range);
                    return TypeRefKind::Error;
                }
                TypeRefKind::Con(id, args)
            }
            // エフェクトの名前は型の位置に書けない
            Resolved::Found(TypeItem::Effect(_)) => {
                self.diagnostics
                    .push(not_found(&self.items, self.file, NameKind::Type, at));
                TypeRefKind::Error
            }
            other => {
                if let Some(diagnostic) =
                    unresolved(&self.items, self.file, NameKind::Type, at, other)
                {
                    self.diagnostics.push(diagnostic);
                }
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

    /// 公開の範囲の誤り (E1032)。非公開の型を返す公開の関数を許すと、`pub data` の形で入れる予定の抽象型より先に、
    /// 裏口の抽象型ができてしまう。非公開のエフェクトは、import する側が名前を書けず handle できない
    /// (docs/spec/modules.md の「公開の範囲」)。
    fn check_exposed(&mut self, item: TypeItem, range: TextRange) {
        let Some(owner) = self.public_item else {
            return;
        };
        let Some((name, defined)) = self.items.private_type_item(item) else {
            return;
        };
        let kind = match item {
            TypeItem::Type(_) => "type",
            TypeItem::Effect(_) => "effect",
        };
        self.diagnostics.push(
            Diagnostic::error(
                codes::PRIVATE_IN_PUBLIC,
                format!("the public `{owner}` uses the private {kind} `{name}`"),
                Label::new(self.file, range, format!("`{name}` is not `pub`")),
            )
            .with_secondary(Label::new(
                self.file,
                defined,
                format!("`{name}` is defined here"),
            ))
            .with_help(format!("add `pub` to the declaration of `{name}`")),
        );
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
            let path = path_name(effect.path());
            let Some(at) = path.at(effect.range()) else {
                continue;
            };
            match self.items.type_item(at.name) {
                Resolved::Found(TypeItem::Effect(id)) => {
                    let args: Vec<TypeRefId> = effect
                        .args()
                        .map(|arg| {
                            let range = arg.range();
                            self.lower(Some(arg), range)
                        })
                        .collect();
                    let expected = self.items.effect_params(id);
                    if args.len() != expected {
                        self.arity_error(&at.written(), expected, args.len(), effect.range());
                        valid = false;
                        continue;
                    }
                    let path_range = effect.path().map_or(effect.range(), |path| path.range());
                    self.check_exposed(TypeItem::Effect(id), path_range);
                    effects.push(EffectRef { effect: id, args });
                }
                // 型の名前は row に書けない
                Resolved::Found(TypeItem::Type(_)) => {
                    self.diagnostics
                        .push(not_found(&self.items, self.file, NameKind::Effect, &at));
                    valid = false;
                }
                other => {
                    if let Some(diagnostic) =
                        unresolved(&self.items, self.file, NameKind::Effect, &at, other)
                    {
                        self.diagnostics.push(diagnostic);
                    }
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

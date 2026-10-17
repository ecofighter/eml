use eml_diagnostics::{Diagnostic, FileId, Label, TextEdit, TextRange};
use eml_syntax::{SyntaxToken, ast};
use la_arena::Arena;

use super::{NameKind, NameUse, not_found, path_name, unresolved};
use crate::codes;
use crate::def_map::{Resolved, Resolver};
use crate::hir::{
    EffectRef, Generics, ParamSort, RowRef, RowVarDecl, RowVarId, TypeRef, TypeRefId, TypeRefKind,
    TypeVarDecl, TypeVarId,
};
use crate::program::{TypeDefId, TypeItem};

/// 型変数と row 変数の名前の引き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Vars {
    /// シグネチャ。新しい変数の名前を表に入れる。
    Define,
    /// 本体の注釈。シグネチャの表にある名前だけを使える (docs/spec/types.md の「推論」)。
    Signature,
    /// `data` の宣言のフィールド。宣言の型引数だけを使える。row の末尾には宣言の row の型引数だけを書ける
    /// (docs/spec/types.md の「row の型引数」)。
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
                Some(at) => self.applied(&at, &[], range),
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
                TypeRefKind::Fn {
                    param,
                    once: function.is_once(),
                    row,
                    ret,
                }
            }
            ast::Type::VarType(var) => match var.name() {
                Some(name) => self.type_var(&name, range),
                None => TypeRefKind::Error,
            },
            ast::Type::AppType(app) => {
                let args: Vec<ast::TypeArg> = app.args().collect();
                match path_name(app.path()).at(range) {
                    Some(at) => self.applied(&at, &args, range),
                    None => {
                        self.lower_written_args(&args);
                        TypeRefKind::Error
                    }
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
        if let (TypeRefKind::Con(id, _, _), Some(path_range)) = (&kind, path_range) {
            self.check_exposed(TypeItem::Type(*id), path_range);
        }
        self.alloc(kind, range)
    }

    /// 型の名前に引数を適用する。型の名前が誤っていても引数の中の誤りを報告するよう、名前の誤りより先に引数を変換する。
    /// 引数の種類は宣言の種類と位置ごとに照合する (docs/spec/types.md の「row の型引数」)。
    fn applied(
        &mut self,
        at: &NameUse<'_>,
        args: &[ast::TypeArg],
        range: TextRange,
    ) -> TypeRefKind {
        let id = match self.items.type_item(at.name) {
            Resolved::Found(TypeItem::Type(id)) => id,
            // エフェクトの名前は型の位置に書けない
            Resolved::Found(TypeItem::Effect(_)) => {
                self.lower_written_args(args);
                self.diagnostics
                    .push(not_found(&self.items, self.file, NameKind::Type, at));
                return TypeRefKind::Error;
            }
            Resolved::Found(TypeItem::Class(_)) => {
                self.lower_written_args(args);
                self.diagnostics
                    .push(class_as_type(self.file, &at.written(), range));
                return TypeRefKind::Error;
            }
            other => {
                self.lower_written_args(args);
                self.diagnostics.extend(unresolved(
                    &self.items,
                    self.file,
                    NameKind::Type,
                    at,
                    other,
                ));
                return TypeRefKind::Error;
            }
        };
        let sorts = self.items.type_params(id);
        if args.len() != sorts.len() {
            self.lower_written_args(args);
            self.arity_error(&at.written(), sorts.len(), args.len(), range);
            return TypeRefKind::Error;
        }
        let mut types = Vec::new();
        let mut rows = Vec::new();
        let mut valid = true;
        for (index, (sort, arg)) in sorts.iter().zip(args).enumerate() {
            match (sort, arg) {
                (ParamSort::Type, ast::TypeArg::Type(ty)) => {
                    types.push(self.lower(Some(ty.clone()), ty.range()));
                }
                (ParamSort::Row, ast::TypeArg::Row(row)) => rows.push(self.row_arg(row)),
                (ParamSort::Row, ast::TypeArg::Type(ty)) => {
                    let mut diagnostic = row_expected(self.file, &at.written(), ty);
                    self.declared_here(&mut diagnostic, id, index);
                    self.diagnostics.push(diagnostic);
                    valid = false;
                }
                (ParamSort::Type, ast::TypeArg::Row(row)) => {
                    let mut diagnostic = Diagnostic::error(
                        codes::ARGUMENT_SORT,
                        format!("`{}` takes a type here", at.written()),
                        Label::new(self.file, row.range(), "this is a row"),
                    );
                    self.declared_here(&mut diagnostic, id, index);
                    self.diagnostics.push(diagnostic);
                    valid = false;
                }
            }
        }
        if !valid {
            return TypeRefKind::Error;
        }
        TypeRefKind::Con(id, types, rows)
    }

    /// E1048 に、宣言の型引数の位置を secondary で足す。宣言は別のファイルにあることがある
    /// (docs/spec/types.md の「row の型引数」)。
    fn declared_here(&self, diagnostic: &mut Diagnostic, id: TypeDefId, index: usize) {
        let Some((file, range)) = self.items.type_param_position(id, index) else {
            return;
        };
        let sort = match self.items.type_params(id)[index] {
            ParamSort::Type => "type",
            ParamSort::Row => "row",
        };
        diagnostic.secondary.push(Label::new(
            file,
            range,
            format!("declared as a {sort} parameter here"),
        ));
    }

    /// 種類を照合できない引数を、書いた形のまま変換する。中の誤りを報告するためで、結果は使わない。
    fn lower_written_args(&mut self, args: &[ast::TypeArg]) {
        for arg in args {
            match arg {
                ast::TypeArg::Type(ty) => {
                    self.lower(Some(ty.clone()), ty.range());
                }
                ast::TypeArg::Row(row) => {
                    self.row_arg(row);
                }
            }
        }
    }

    /// 型の適用の引数に書いた row。`<…>` の中身がなければパーサが報告済みである。
    fn row_arg(&mut self, arg: &ast::RowArg) -> RowRef {
        match arg.row() {
            Some(row) => self.row(&row),
            None => RowRef::Error,
        }
    }

    fn arity_error(&mut self, name: &str, expected: usize, given: usize, range: TextRange) {
        self.diagnostics
            .push(arity_error(self.file, name, expected, given, range));
    }

    fn check_exposed(&mut self, item: TypeItem, range: TextRange) {
        if let Some(owner) = self.public_item {
            self.diagnostics.extend(private_in_public(
                &self.items,
                self.file,
                owner,
                item,
                range,
            ));
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
                // 型とクラスの名前は row に書けない
                Resolved::Found(TypeItem::Type(_) | TypeItem::Class(_)) => {
                    self.diagnostics
                        .push(not_found(&self.items, self.file, NameKind::Effect, &at));
                    valid = false;
                }
                other => {
                    self.diagnostics.extend(unresolved(
                        &self.items,
                        self.file,
                        NameKind::Effect,
                        &at,
                        other,
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

/// 型に現れる型変数を、現れるたびに `visit` に渡す。関数型の row に書いたエフェクトの型引数も、型に現れるものに
/// 数える (docs/spec/declarations.md の「宣言の検査」)。木の深さは E0013 で抑えられているので再帰でたどる。
pub(super) fn visit_type_vars(
    types: &Arena<TypeRef>,
    id: TypeRefId,
    visit: &mut impl FnMut(TypeVarId),
) {
    match &types[id].kind {
        TypeRefKind::Error => {}
        TypeRefKind::Var(var) => visit(*var),
        TypeRefKind::Con(_, args, rows) => {
            for &arg in args {
                visit_type_vars(types, arg, visit);
            }
            for row in rows {
                visit_row_type_vars(types, row, visit);
            }
        }
        TypeRefKind::Tuple(args) => {
            for &arg in args {
                visit_type_vars(types, arg, visit);
            }
        }
        TypeRefKind::Fn {
            param, row, ret, ..
        } => {
            visit_type_vars(types, *param, visit);
            visit_row_type_vars(types, row, visit);
            visit_type_vars(types, *ret, visit);
        }
    }
}

fn visit_row_type_vars(types: &Arena<TypeRef>, row: &RowRef, visit: &mut impl FnMut(TypeVarId)) {
    if let RowRef::Closed { effects, .. } | RowRef::Open { effects, .. } = row {
        for effect in effects {
            for &arg in &effect.args {
                visit_type_vars(types, arg, visit);
            }
        }
    }
}

/// 変換が誤りを報告した型か。誤りの跡 (`Error` の型と row) からは型変数が落ちているので、型変数が現れないことを
/// 根拠にした診断を重ねない。
pub(super) fn has_error(types: &Arena<TypeRef>) -> bool {
    types.iter().any(|(_, ty)| match &ty.kind {
        TypeRefKind::Error
        | TypeRefKind::Fn {
            row: RowRef::Error, ..
        } => true,
        TypeRefKind::Con(_, _, rows) => rows.contains(&RowRef::Error),
        TypeRefKind::Var(_) | TypeRefKind::Fn { .. } | TypeRefKind::Tuple(_) => false,
    })
}

pub(super) fn mentions(types: &Arena<TypeRef>, id: TypeRefId, var: TypeVarId) -> bool {
    let mut found = false;
    visit_type_vars(types, id, &mut |other| found |= other == var);
    found
}

/// 公開の範囲の誤り (E1032)。非公開の型を返す公開の関数を許すと、`pub data` の形で入れる予定の抽象型より先に、
/// 裏口の抽象型ができてしまう。非公開のエフェクトは、import する側が名前を書けず handle できない
/// (docs/spec/modules.md の「公開の範囲」)。非公開のクラスの制約を持つ公開の関数は、import する側が instance を
/// 書けない (docs/spec/modules.md の「公開の範囲」)。
pub(super) fn private_in_public(
    items: &Resolver<'_>,
    file: FileId,
    owner: &str,
    item: TypeItem,
    range: TextRange,
) -> Option<Diagnostic> {
    let (name, defined) = items.private_type_item(item)?;
    let kind = match item {
        TypeItem::Type(_) => "type",
        TypeItem::Effect(_) => "effect",
        TypeItem::Class(_) => "class",
    };
    Some(
        Diagnostic::error(
            codes::PRIVATE_IN_PUBLIC,
            format!("the public `{owner}` uses the private {kind} `{name}`"),
            Label::new(file, range, format!("`{name}` is not `pub`")),
        )
        .with_secondary(Label::new(
            file,
            defined,
            format!("`{name}` is defined here"),
        ))
        .with_help(format!("add `pub` to the declaration of `{name}`")),
    )
}

/// 型とエフェクトの型引数の個数の誤り (E1015)。instance の頭も同じ文言を使う。
pub(super) fn arity_error(
    file: FileId,
    name: &str,
    expected: usize,
    given: usize,
    range: TextRange,
) -> Diagnostic {
    let given = match given {
        1 => "1 was given".to_string(),
        n => format!("{n} were given"),
    };
    Diagnostic::error(
        codes::TYPE_ARGUMENT_COUNT,
        format!("`{name}` takes {}, but {given}", type_arguments(expected)),
        Label::new(
            file,
            range,
            format!("expected {}", type_arguments(expected)),
        ),
    )
}

/// row の位置に型を書いた (E1048)。小文字の名前だけを書いたなら、`<e>` に直す fix を付ける
/// (docs/spec/types.md の「row の型引数」)。
fn row_expected(file: FileId, name: &str, ty: &ast::Type) -> Diagnostic {
    let diagnostic = Diagnostic::error(
        codes::ARGUMENT_SORT,
        format!("`{name}` takes a row here"),
        Label::new(file, ty.range(), "this is a type"),
    );
    let ast::Type::VarType(var) = ty else {
        return diagnostic;
    };
    let Some(var) = var.name() else {
        return diagnostic;
    };
    let row = format!("<{}>", var.text());
    diagnostic
        .with_help(format!("write it as a row: `{row}`"))
        .with_fix(
            format!("write `{row}`"),
            vec![TextEdit {
                file,
                range: var.text_range(),
                replacement: row,
            }],
        )
}

/// E1043。クラスは型の名前空間にあるが、型ではない
/// (docs/spec/modules.md の「名前空間」)。
pub(super) fn class_as_type(file: FileId, name: &str, range: TextRange) -> Diagnostic {
    Diagnostic::error(
        codes::CLASS_AS_TYPE,
        format!("`{name}` is a class, not a type"),
        Label::new(file, range, "a class cannot be used as a type"),
    )
}

fn type_arguments(n: usize) -> String {
    if n == 1 {
        "1 type argument".to_string()
    } else {
        format!("{n} type arguments")
    }
}

/// 宣言の型の一番外側の矢印に書いた `once` を E1050 にして外す。宣言した関数そのものは何度でも呼べるので `once` に
/// 意味がなく、外すのは誤りを連鎖させないためである (docs/spec/types.md の「1回だけ呼べる関数の型」)。
pub(super) fn reject_once_declaration(
    file: FileId,
    types: &mut Arena<TypeRef>,
    id: TypeRefId,
    name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let range = types[id].range;
    if let TypeRefKind::Fn { once, .. } = &mut types[id].kind
        && *once
    {
        *once = false;
        diagnostics.push(
            Diagnostic::error(
                codes::ONCE_DECLARATION,
                format!("the outermost arrow of `{name}` cannot be `once`"),
                Label::new(
                    file,
                    range,
                    format!("`{name}` can be called any number of times"),
                ),
            )
            .with_help("remove `once`"),
        );
    }
}

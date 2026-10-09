//! クラス、メソッド、文脈の変換 (docs/spec/declarations.md の「クラスと instance」)。
//! クラスは型の名前空間に、メソッドは値の名前空間に置く。

use std::collections::HashSet;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};
use la_arena::Arena;

use super::effect::mentions;
use super::types::{TypeLowering, Vars, private_in_public};
use super::{ItemLowering, NameKind, PendingBody, path_name, unresolved};
use crate::codes;
use crate::def_map::{DefMap, Resolved};
use crate::hir::{
    ClassDef, ClassId, Constraint, Function, FunctionKind, Generics, ItemId, Method, Signature,
    TypeRefKind, TypeVarDecl, TypeVarId,
};
use crate::item_tree::ClassItem;
use crate::program::{Items, Module, TypeItem};

/// 文脈を書いた位置。位置ごとに、制約に書ける型変数が違う
/// (docs/spec/declarations.md の「宣言の検査」)。
pub(super) enum ContextScope<'a> {
    /// トップレベルの関数のシグネチャ。型に現れる型変数だけに書ける。
    Function,
    /// メソッドのシグネチャ。メソッド自身の型変数だけに書ける。
    Method {
        class_var: TypeVarId,
        class: &'a str,
        var: &'a str,
    },
    /// クラスの頭。クラスの型変数だけに書ける。
    Superclass { var: &'a str },
    /// instance の頭。頭の型変数だけに書ける。
    Instance,
    /// extern と操作のシグネチャ。制約を書けない。
    Forbidden(&'static str),
}

impl ItemLowering<'_> {
    /// 文脈を制約の並びにする。誤った制約は報告して捨てる。型変数は `generics` から名前で引くので、シグネチャでは
    /// 型を変換した後に呼ぶ。`public_item` は E1032 を当てる `pub` の item の名前である。
    pub(super) fn lower_context(
        &mut self,
        context: Option<ast::Context>,
        generics: &Generics,
        scope: ContextScope<'_>,
        public_item: Option<&str>,
    ) -> Vec<Constraint> {
        let Some(context) = context else {
            return Vec::new();
        };
        if let ContextScope::Forbidden(what) = scope {
            self.diagnostics.push(invalid_constraint(
                self.file,
                format!("{what} cannot have constraints"),
                context.range(),
                "remove this context",
            ));
            return Vec::new();
        }
        let mut constraints = Vec::new();
        for constraint in context.constraints() {
            // 型がなければパーサが報告済み
            let Some(ty) = constraint.ty() else {
                continue;
            };
            let range = ty.range();
            let Some((path, var)) = constraint_parts(&ty) else {
                self.diagnostics.push(invalid_constraint(
                    self.file,
                    "a constraint must be a class applied to one type variable",
                    range,
                    "not of the form `C a`",
                ));
                continue;
            };
            let path_range = path.range();
            let Some(class) = self.resolve_class(Some(path)) else {
                continue;
            };
            if let Some(owner) = public_item {
                self.diagnostics.extend(private_in_public(
                    &self.resolver,
                    self.file,
                    owner,
                    TypeItem::Class(class),
                    path_range,
                ));
            }
            let (name, var_range) = (var.text(), var.text_range());
            let found = generics
                .type_vars
                .iter()
                .find(|(_, decl)| decl.name == name)
                .map(|(id, _)| id);
            let var = match (&scope, found) {
                (ContextScope::Function | ContextScope::Method { .. }, None) => {
                    self.diagnostics.push(invalid_constraint(
                        self.file,
                        format!("the constraint on `{name}` is ambiguous"),
                        var_range,
                        format!("`{name}` does not appear in the type"),
                    ));
                    continue;
                }
                (
                    ContextScope::Method {
                        class_var,
                        class,
                        var,
                    },
                    Some(id),
                ) if id == *class_var => {
                    self.diagnostics.push(
                        invalid_constraint(
                            self.file,
                            format!("a method cannot constrain the class variable `{var}`"),
                            range,
                            "remove this constraint",
                        )
                        .with_help(format!("the class already requires `{class} {var}`")),
                    );
                    continue;
                }
                (ContextScope::Superclass { var }, _) if name != *var => {
                    self.diagnostics.push(invalid_constraint(
                        self.file,
                        format!("a superclass constraint must be on the class variable `{var}`"),
                        var_range,
                        "not the class variable",
                    ));
                    continue;
                }
                (ContextScope::Instance, None) => {
                    self.diagnostics.push(invalid_constraint(
                        self.file,
                        "the context of an instance can only constrain the variables of its head",
                        var_range,
                        "not a variable of the head",
                    ));
                    continue;
                }
                (_, Some(id)) => id,
                (ContextScope::Superclass { .. } | ContextScope::Forbidden(_), None) => {
                    unreachable!("the class variable is in the generics of a class")
                }
            };
            constraints.push(Constraint { class, var, range });
        }
        constraints
    }

    /// クラスを書く位置の名前を引く。型かエフェクトに当たったら E1041 にする
    /// (docs/spec/modules.md の「名前空間」)。
    pub(super) fn resolve_class(&mut self, path: Option<ast::Path>) -> Option<ClassId> {
        let range = path.as_ref()?.range();
        let name = path_name(path);
        let at = name.at(range)?;
        let resolved = self.resolver.class(at.name);
        if let Resolved::Found(id) = resolved {
            return Some(id);
        }
        let not_a_class = |label| {
            Diagnostic::error(
                codes::NOT_A_CLASS,
                format!("`{}` is not a class", at.written()),
                Label::new(self.file, range, label),
            )
        };
        let diagnostic = match self.resolver.type_item(at.name) {
            Resolved::Found(TypeItem::Type(_)) => Some(not_a_class("a type, not a class")),
            Resolved::Found(TypeItem::Effect(_)) => Some(not_a_class("an effect, not a class")),
            _ => unresolved(&self.resolver, self.file, NameKind::Class, &at, resolved),
        };
        self.diagnostics.extend(diagnostic);
        None
    }

    /// クラス、メソッド、既定のメソッドの関数を置く。既定のメソッドの関数は名前の表に入らないので、`ItemTree` の
    /// 関数の後ろに置き、本体は `pending` に足して後で変換する。
    pub(super) fn lower_classes(
        &mut self,
        classes: &[ClassItem],
        items: &mut Items,
        pending: &mut Vec<PendingBody>,
    ) {
        for (k, item) in classes.iter().enumerate() {
            let id = self.def_map.class_id(self.module, k);
            let node = item.ptr.to_node(self.root);
            let (var, _) = &item.var;
            let mut class_generics = Generics::default();
            let class_var = class_generics
                .type_vars
                .alloc(TypeVarDecl { name: var.clone() });
            let mut superclasses = Vec::new();
            for constraint in self.lower_context(
                node.context(),
                &class_generics,
                ContextScope::Superclass { var },
                item.public.then_some(item.name.as_str()),
            ) {
                if !superclasses.contains(&constraint.class) {
                    superclasses.push(constraint.class);
                }
            }
            let local = items.classes.alloc(ClassDef {
                name: item.name.clone(),
                name_range: item.name_range,
                var: var.clone(),
                superclasses,
                methods: Vec::new(),
            });
            debug_assert_eq!(ItemId::new(self.module, local), id);
            for (j, method) in item.methods.iter().enumerate() {
                let signature_item = method
                    .signature
                    .as_ref()
                    .expect("`item_tree` keeps only methods with a signature");
                let decl = signature_item.ptr.to_node(self.root);
                let range = decl.ty().map_or(decl.range(), |ty| ty.range());
                let public_item = item.public.then_some(method.name.as_str());
                // クラスの型変数を先頭に置く。シグネチャで同じ名前の型変数はそれを指す
                let mut types = Arena::new();
                let mut generics = class_generics.clone();
                let ty = TypeLowering {
                    file: self.file,
                    types: &mut types,
                    generics: &mut generics,
                    items: self.resolver,
                    vars: Vars::Define,
                    public_item,
                    diagnostics: &mut *self.diagnostics,
                }
                .lower(decl.ty(), range);
                // 型に誤りがあれば変換が報告済みなので、クラスの型変数がないことを重ねて報告しない
                let has_error = types
                    .iter()
                    .any(|(_, ty)| matches!(ty.kind, TypeRefKind::Error));
                if !has_error && !mentions(&types, ty, class_var) {
                    self.diagnostics.push(invalid_constraint(
                        self.file,
                        format!(
                            "the method `{}` does not mention the class variable `{var}`",
                            method.name
                        ),
                        range,
                        format!("`{var}` does not appear in this type"),
                    ));
                }
                let constraints = self.lower_context(
                    decl.context(),
                    &generics,
                    ContextScope::Method {
                        class_var,
                        class: &item.name,
                        var,
                    },
                    public_item,
                );
                let signature = Signature {
                    ty,
                    range,
                    types,
                    generics,
                    constraints,
                };
                let default = (!method.equations.is_empty()).then(|| {
                    // 既定のメソッドは、クラスの制約 `C a` を与えられた制約として持つ
                    let mut signature = signature.clone();
                    signature.constraints.insert(
                        0,
                        Constraint {
                            class: id,
                            var: class_var,
                            range: item.name_range,
                        },
                    );
                    (signature, method.equations.clone())
                });
                let method_id = ItemId::new(
                    self.module,
                    items.methods.alloc(Method {
                        name: method.name.clone(),
                        name_range: signature_item.name_range,
                        class: id,
                        signature,
                        default: None,
                    }),
                );
                debug_assert_eq!(method_id, self.def_map.method_id(self.module, k, j));
                if let Some((signature, equations)) = default {
                    let function = ItemId::new(
                        self.module,
                        items.functions.alloc(Function {
                            name: method.name.clone(),
                            name_range: equations[0].1,
                            signature_name_range: Some(signature_item.name_range),
                            equation_ranges: equations.iter().map(|(_, range)| *range).collect(),
                            signature: Some(signature),
                            kind: FunctionKind::DefaultMethod(method_id),
                        }),
                    );
                    items.methods[method_id.local].default = Some(function);
                    pending.push(PendingBody {
                        function,
                        equations,
                        annotation_vars: None,
                    });
                }
                items.classes[local].methods.push(method_id);
            }
        }
    }
}

/// 制約 `C a` の、クラスの名前と型変数。
fn constraint_parts(ty: &ast::Type) -> Option<(ast::Path, SyntaxToken)> {
    let ast::Type::AppType(app) = ty else {
        return None;
    };
    let args: Vec<ast::Type> = app.args().collect();
    let [ast::Type::VarType(var)] = args.as_slice() else {
        return None;
    };
    Some((app.path()?, var.name()?))
}

fn invalid_constraint(
    file: FileId,
    message: impl Into<String>,
    range: TextRange,
    label: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error(
        codes::INVALID_CONSTRAINT,
        message,
        Label::new(file, range, label),
    )
}

/// 上位クラスの循環 (E1042)。すべてのモジュールのクラスを置いた後に、モジュールの順、クラスの順に、上位クラスを
/// 宣言の順に深さ優先でたどる。たどっている途中のクラスに戻る辺を、循環を閉じる辺として報告し、その辺を持つクラスの
/// 上位クラスを空にする。上位クラスの閉包 (`Program::superclasses`) が止まるようにするため。上位クラスの鎖が
/// いくら長くてもスタックを使い切らないよう、自前のスタックでたどる。
pub(super) fn check_superclass_cycles(
    def_map: &DefMap,
    modules: &mut Arena<Module>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let classes: Vec<ClassId> = modules
        .iter()
        .flat_map(|(module, data)| {
            data.items
                .classes
                .iter()
                .map(move |(local, _)| ItemId::new(module, local))
        })
        .collect();
    let names = def_map.display_names();
    let mut visited: HashSet<ClassId> = HashSet::new();
    let mut open: HashSet<ClassId> = HashSet::new();
    let mut cut = Vec::new();
    for &start in &classes {
        if !visited.insert(start) {
            continue;
        }
        open.insert(start);
        // たどっている途中のクラスと、次に見る上位クラスの番号。クラスの並びが循環の経路になる
        let mut path: Vec<(ClassId, usize)> = vec![(start, 0)];
        while let Some(top) = path.last_mut() {
            let (current, next) = *top;
            top.1 += 1;
            let Some(&superclass) = class(modules, current).superclasses.get(next) else {
                path.pop();
                open.remove(&current);
                continue;
            };
            if open.contains(&superclass) {
                let first = path
                    .iter()
                    .position(|&(on_path, _)| on_path == superclass)
                    .expect("an open class is on the path");
                let cycle: Vec<String> = path[first..]
                    .iter()
                    .map(|&(id, _)| id)
                    .chain([superclass])
                    .map(|id| format!("`{}`", names.class(id)))
                    .collect();
                let def = class(modules, current);
                diagnostics.push(
                    Diagnostic::error(
                        codes::SUPERCLASS_CYCLE,
                        format!(
                            "the superclasses of `{}` make a cycle",
                            names.class(current)
                        ),
                        Label::new(
                            modules[current.module].file,
                            def.name_range,
                            "this class closes the cycle",
                        ),
                    )
                    .with_note(format!("the cycle is {}", cycle.join(" -> "))),
                );
                cut.push(current);
                // 上位クラスを空にするので、残りの辺はたどらない
                path.pop();
                open.remove(&current);
                continue;
            }
            if visited.insert(superclass) {
                open.insert(superclass);
                path.push((superclass, 0));
            }
        }
    }
    for id in cut {
        modules[id.module].items.classes[id.local]
            .superclasses
            .clear();
    }
}

fn class(modules: &Arena<Module>, id: ClassId) -> &ClassDef {
    &modules[id.module].items.classes[id.local]
}

//! instance の変換 (docs/spec/declarations.md の「`instance`」と「宣言の検査」)。
//! instance は名前を持たず、(クラス, 頭の型) の索引で引く。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, Label, TextRange};
use eml_extern::{Extern, ExternType};
use eml_syntax::ast;
use la_arena::{Arena, Idx, RawIdx};

use super::class::ContextScope;
use super::types::{arity_error, class_as_type, visit_type_vars};
use super::{BodyOwner, ItemLowering, NameKind, PendingBody, not_found, path_name, unresolved};
use crate::codes;
use crate::def_map::Resolved;
use crate::hir::{
    ClassId, Constraint, EffectRef, Function, FunctionKind, Generics, InstanceDef, InstanceId,
    InstanceOrigin, ItemId, MethodId, MethodImpl, RowRef, Signature, TypeDefId, TypeDefKind,
    TypeRef, TypeRefId, TypeRefKind, TypeVarDecl, TypeVarId,
};
use crate::item_tree::{InstanceItem, ItemTree, MemberItem};
use crate::program::{Module, TypeItem};

/// 検査した instance の頭。
struct Head {
    ty: TypeDefId,
    /// 書いた型の名前。std の extern の行の名前に使う。
    written: String,
    /// 頭の型変数。`data` の宣言の型引数の順である。
    vars: Vec<String>,
}

/// instance がメソッドに与える定義のうち、置く前のもの。
enum Planned<'m> {
    Function {
        method: MethodId,
        name: String,
        signature: Signature,
        member: &'m MemberItem,
    },
    Extern(MethodId, Extern),
}

impl ItemLowering<'_> {
    /// このモジュールの instance を置く。クラスは別のモジュールにありうるので、すべてのモジュールの item を置いた後に
    /// 呼ぶ。instance のメソッドの関数は名前の表に入らないので、関数のアリーナの後ろに置き、本体は `pending` に
    /// 足して後で変換する。
    pub(super) fn lower_instances(
        &mut self,
        tree: &ItemTree,
        modules: &mut Arena<Module>,
        index: &mut HashMap<(ClassId, TypeDefId), InstanceId>,
        pending: &mut Vec<PendingBody>,
    ) {
        for item in &tree.instances {
            if !self.lower_instance(item, modules, index, pending) {
                // 置かなかった instance のメンバーも名前を解決する。最初の誤りを直すまで本体の誤りが見えないことを
                // 避けるためである。頭が誤っていることもあるので、注釈には頭に書いた型変数をすべて見せる
                let vars = written_vars(item.ptr.to_node(self.root).head());
                pending.extend(
                    item.members
                        .iter()
                        .map(|member| discarded(member, vars.clone())),
                );
            }
        }
    }

    /// instance を1つ置き、置けたかを返す。置けなければ、誤りを報告済みである。
    fn lower_instance(
        &mut self,
        item: &InstanceItem,
        modules: &mut Arena<Module>,
        index: &mut HashMap<(ClassId, TypeDefId), InstanceId>,
        pending: &mut Vec<PendingBody>,
    ) -> bool {
        let node = item.ptr.to_node(self.root);
        let class_path = node.class();
        let class_written = class_path.as_ref().map(|path| {
            path.segments()
                .map(|segment| segment.text())
                .collect::<Vec<_>>()
                .join(".")
        });
        let (Some(class), Some(class_written)) = (self.resolve_class(class_path), class_written)
        else {
            return false;
        };
        // 頭の型がなければパーサが報告済み
        let Some(head_ty) = node.head() else {
            return false;
        };
        let head_range = head_ty.range();
        let Some(head) = self.instance_head(head_ty) else {
            return false;
        };
        let mut generics = Generics::default();
        for var in &head.vars {
            generics.type_vars.alloc(TypeVarDecl { name: var.clone() });
        }
        let context = self.lower_context(node.context(), &generics, ContextScope::Instance, None);
        let names = self.def_map.display_names();
        let (class_name, ty_name) = (names.class(class), names.ty(head.ty));
        if class.module != self.module && head.ty.module != self.module {
            self.diagnostics.push(Diagnostic::error(
                codes::ORPHAN_INSTANCE,
                format!(
                    "an instance of `{class_name}` for `{ty_name}` must be in the module of `{class_name}` or of `{ty_name}`"
                ),
                Label::new(self.file, head_range, "neither is defined in this module"),
            ));
            return false;
        }
        if self.duplicate(modules, index, class, head.ty, head_range) {
            return false;
        }
        let class_def = &modules[class.module].items.classes[class.local];
        let mut planned = Vec::new();
        let mut defined = Vec::new();
        for member in &item.members {
            let found = class_def.methods.iter().copied().find(|method| {
                modules[method.module].items.methods[method.local].name == member.name
            });
            let Some(method) = found else {
                self.diagnostics.push(Diagnostic::error(
                    codes::UNKNOWN_METHOD,
                    format!("`{}` is not a method of `{class_name}`", member.name),
                    Label::new(self.file, member.name_range, "not declared in the class"),
                ));
                pending.push(discarded(member, head.vars.clone()));
                continue;
            };
            // ユーザーのモジュールの `extern` は E1033 にして行を持たないが、定義したものに数えて E1036 を重ねない
            defined.push(method);
            if let Some(keyword) = member.extern_range {
                let name = format!("{class_written} {}.{}", head.written, member.name);
                if let Some(row) = self.extern_row(keyword, &name, Extern::from_name) {
                    planned.push(Planned::Extern(method, row));
                }
                continue;
            }
            let signature = &modules[method.module].items.methods[method.local].signature;
            planned.push(Planned::Function {
                method,
                name: format!("{class_name} {ty_name}.{}", member.name),
                signature: instance_signature(
                    signature,
                    head.ty,
                    &head.vars,
                    &context,
                    member.name_range,
                ),
                member,
            });
        }
        for &method in &class_def.methods {
            let method_def = &modules[method.module].items.methods[method.local];
            if method_def.default.is_none() && !defined.contains(&method) {
                let name = &method_def.name;
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::MISSING_METHOD,
                        format!(
                            "the instance of `{class_name}` for `{ty_name}` does not define `{name}`"
                        ),
                        Label::new(self.file, head_range, format!("`{name}` has no default")),
                    )
                    .with_help(format!("add an equation for `{name}`")),
                );
            }
        }
        let items = &mut modules[self.module].items;
        let instance = ItemId::new(
            self.module,
            items.instances.alloc(InstanceDef {
                class,
                head: head.ty,
                head_range,
                generics,
                context: Vec::new(),
                methods: Vec::new(),
                origin: InstanceOrigin::Written,
            }),
        );
        index.insert((class, head.ty), instance);
        let mut methods = Vec::new();
        for plan in planned {
            match plan {
                Planned::Function {
                    method,
                    name,
                    signature,
                    member,
                } => {
                    let function = ItemId::new(
                        self.module,
                        items.functions.alloc(Function {
                            name,
                            name_range: member
                                .equations
                                .first()
                                .map_or(member.name_range, |(_, range)| *range),
                            signature_name_range: Some(member.name_range),
                            equation_ranges: member
                                .equations
                                .iter()
                                .map(|(_, range)| *range)
                                .collect(),
                            signature: Some(signature),
                            kind: FunctionKind::InstanceMethod(instance, method),
                        }),
                    );
                    pending.push(PendingBody {
                        owner: BodyOwner::Function {
                            id: function,
                            annotation_vars: Some(head.vars.len()),
                        },
                        equations: member.equations.clone(),
                    });
                    methods.push((method, MethodImpl::Function(function)));
                }
                Planned::Extern(method, row) => methods.push((method, MethodImpl::Extern(row))),
            }
        }
        let def = &mut items.instances[instance.local];
        def.context = context;
        def.methods = methods;
        true
    }

    /// このモジュールの `data` の `deriving` ごとに、導出した instance を置く
    /// (docs/spec/declarations.md の「`deriving`」)。重複 (E1035) を手で書いた
    /// instance と同じ索引で判定するので、このモジュールの `lower_instances` の後に呼ぶ。導出できるのは Prelude の
    /// `Eq`、`Ord`、`Show` だけで、名前でなく解決したクラスで判定する (E1038)。
    pub(super) fn derive_instances(
        &mut self,
        tree: &ItemTree,
        modules: &mut Arena<Module>,
        index: &mut HashMap<(ClassId, TypeDefId), InstanceId>,
    ) {
        let lang = self.def_map.lang();
        for (k, item) in tree.data.iter().enumerate() {
            let ty = self.def_map.type_id(self.module, k);
            for (ptr, range) in &item.deriving {
                let Some(class) = self.resolve_class(Some(ptr.to_node(self.root))) else {
                    continue;
                };
                if ![lang.eq, lang.ord, lang.show].contains(&class) {
                    self.diagnostics.push(Diagnostic::error(
                        codes::NOT_DERIVABLE,
                        format!(
                            "`{}` cannot be derived",
                            self.def_map.display_names().class(class)
                        ),
                        Label::new(
                            self.file,
                            *range,
                            "only `Eq`, `Ord` and `Show` can be derived",
                        ),
                    ));
                    continue;
                }
                if self.duplicate(modules, index, class, ty, *range) {
                    continue;
                }
                let items = &modules[self.module].items;
                let def = &items.types[ty.local];
                let mut occurs = vec![false; def.generics.type_vars.len()];
                if let TypeDefKind::Data { constructors } = &def.kind {
                    for constructor in constructors {
                        for &field in &items.constructors[constructor.local].fields {
                            visit_type_vars(&def.types, field, &mut |var| {
                                occurs[u32::from(var.into_raw()) as usize] = true;
                            });
                        }
                    }
                }
                // Haskell と違い、フィールドの制約を簡約した最小の文脈は求めず、現れる型引数すべてに制約を置く
                let context = def
                    .generics
                    .type_vars
                    .iter()
                    .zip(&occurs)
                    .filter(|(_, occurs)| **occurs)
                    .map(|((var, _), _)| Constraint {
                        class,
                        var,
                        range: *range,
                    })
                    .collect();
                let generics = def.generics.clone();
                let instance = ItemId::new(
                    self.module,
                    modules[self.module].items.instances.alloc(InstanceDef {
                        class,
                        head: ty,
                        head_range: *range,
                        generics,
                        context,
                        methods: Vec::new(),
                        origin: InstanceOrigin::Derived(*range),
                    }),
                );
                index.insert((class, ty), instance);
            }
        }
    }

    /// (クラス, 型) の instance がすでにあれば E1035 を出して真を返す。プログラムのモジュールの順に置くので、後に
    /// 置くものが重複になる。
    fn duplicate(
        &mut self,
        modules: &Arena<Module>,
        index: &HashMap<(ClassId, TypeDefId), InstanceId>,
        class: ClassId,
        ty: TypeDefId,
        range: TextRange,
    ) -> bool {
        let Some(first) = index.get(&(class, ty)) else {
            return false;
        };
        let names = self.def_map.display_names();
        let first_module = &modules[first.module];
        let first_range = first_module.items.instances[first.local].head_range;
        self.diagnostics.push(
            Diagnostic::error(
                codes::DUPLICATE_INSTANCE,
                format!(
                    "`{}` already has an instance of `{}`",
                    names.ty(ty),
                    names.class(class)
                ),
                Label::new(self.file, range, "defined again here"),
            )
            .with_secondary(Label::new(
                first_module.file,
                first_range,
                "first defined here",
            )),
        );
        true
    }

    /// instance の頭を検査する。頭は、`data` の型か extern の型のコンストラクタに、互いに異なる型変数を宣言の数だけ
    /// 適用した形でなければならない (E1039、E1015)。誤りがあれば報告して `None` を返す。
    fn instance_head(&mut self, ty: ast::Type) -> Option<Head> {
        let mut ty = ty;
        while let ast::Type::ParenType(paren) = &ty {
            ty = paren.ty()?;
        }
        let range = ty.range();
        let (path, args): (Option<ast::Path>, Vec<ast::Type>) = match &ty {
            ast::Type::PathType(path) => (path.path(), Vec::new()),
            ast::Type::AppType(app) => (app.path(), app.args().collect()),
            ast::Type::VarType(_) => {
                self.invalid_head(range, "a type variable");
                return None;
            }
            ast::Type::TupleType(_) => {
                self.invalid_head(range, "tuples have only built-in instances");
                return None;
            }
            ast::Type::FnType(_) => {
                self.invalid_head(range, "a function type");
                return None;
            }
            ast::Type::ParenType(_) => unreachable!("the parentheses are unwrapped above"),
        };
        let path_range = path.as_ref()?.range();
        let name = path_name(path);
        let at = name.at(path_range)?;
        let written = at.written();
        let mut valid = true;
        let id = match self.resolver.type_item(at.name) {
            Resolved::Found(TypeItem::Type(id)) => Some(id),
            Resolved::Found(TypeItem::Class(_)) => {
                self.diagnostics
                    .push(class_as_type(self.file, &written, path_range));
                None
            }
            Resolved::Found(TypeItem::Effect(_)) => {
                self.diagnostics
                    .push(not_found(&self.resolver, self.file, NameKind::Type, &at));
                None
            }
            other => {
                self.diagnostics.extend(unresolved(
                    &self.resolver,
                    self.file,
                    NameKind::Type,
                    &at,
                    other,
                ));
                None
            }
        };
        // `Unit` は型の表で要素のないタプルなので、処理系の構造的な instance だけを持つ
        if id == Some(self.def_map.externs().ty(ExternType::Unit)) {
            self.invalid_head(range, "`Unit` has only built-in instances");
            valid = false;
        }
        if let Some(id) = id {
            let expected = self.resolver.type_params(id);
            if args.len() != expected {
                self.diagnostics.push(arity_error(
                    self.file,
                    &written,
                    expected,
                    args.len(),
                    range,
                ));
                valid = false;
            }
        }
        let mut vars: Vec<String> = Vec::new();
        for arg in &args {
            let name = match arg {
                ast::Type::VarType(var) => var.name(),
                _ => None,
            };
            let Some(name) = name else {
                self.invalid_head(arg.range(), "not a type variable");
                valid = false;
                continue;
            };
            let name = name.text().to_string();
            if vars.contains(&name) {
                self.invalid_head(arg.range(), &format!("`{name}` appears more than once"));
                valid = false;
                continue;
            }
            vars.push(name);
        }
        Some(Head {
            ty: id?,
            written,
            vars,
        })
        .filter(|_| valid)
    }

    fn invalid_head(&mut self, range: TextRange, label: &str) {
        self.diagnostics.push(Diagnostic::error(
            codes::INVALID_INSTANCE_HEAD,
            "an instance head must be a type constructor applied to distinct type variables",
            Label::new(self.file, range, label),
        ));
    }
}

/// 置かない instance のメンバーの本体。
fn discarded(member: &MemberItem, vars: Vec<String>) -> PendingBody {
    PendingBody {
        owner: BodyOwner::Discarded { vars },
        equations: member.equations.clone(),
    }
}

/// 頭に書いた型変数の名前。重なりは1つにまとめる。
fn written_vars(head: Option<ast::Type>) -> Vec<String> {
    let mut vars = Vec::new();
    let mut work: Vec<ast::Type> = head.into_iter().collect();
    while let Some(ty) = work.pop() {
        match ty {
            ast::Type::VarType(var) => {
                if let Some(name) = var.name().map(|name| name.text().to_string())
                    && !vars.contains(&name)
                {
                    vars.push(name);
                }
            }
            ast::Type::AppType(app) => work.extend(app.args()),
            ast::Type::ParenType(paren) => work.extend(paren.ty()),
            ast::Type::TupleType(tuple) => work.extend(tuple.elements()),
            ast::Type::FnType(function) => {
                work.extend(function.param().into_iter().chain(function.ret()))
            }
            ast::Type::PathType(_) => {}
        }
    }
    vars
}

/// メソッドのシグネチャのクラスの型変数を、instance の頭の型に置き換えたシグネチャ。型変数の並びは、頭の型変数、
/// メソッド自身の型変数の順である。メソッド自身の型変数の名前が頭の型変数と重なれば、後ろに番号を付けて重ならない
/// 名前にする。型の表と単相化の代入が型変数を名前で引くためである。型の注釈の位置はクラスのファイルの中にあるので、
/// どの位置も instance のメンバーの名前の位置に置き換える。
fn instance_signature(
    method: &Signature,
    head: TypeDefId,
    head_vars: &[String],
    context: &[Constraint],
    at: TextRange,
) -> Signature {
    let mut generics = Generics::default();
    for var in head_vars {
        generics.type_vars.alloc(TypeVarDecl { name: var.clone() });
    }
    let own: Vec<&str> = method
        .generics
        .type_vars
        .iter()
        .skip(1)
        .map(|(_, var)| var.name.as_str())
        .collect();
    for (i, &name) in own.iter().enumerate() {
        let taken = |candidate: &str| {
            generics
                .type_vars
                .iter()
                .any(|(_, var)| var.name == candidate)
                || own
                    .iter()
                    .enumerate()
                    .any(|(j, &other)| j != i && other == candidate)
        };
        let mut fresh = name.to_string();
        let mut suffix = 1;
        while taken(&fresh) {
            fresh = format!("{name}{suffix}");
            suffix += 1;
        }
        generics.type_vars.alloc(TypeVarDecl { name: fresh });
    }
    generics.row_vars = method.generics.row_vars.clone();
    let mut copy = SignatureCopy {
        method,
        head,
        head_vars: head_vars.len(),
        at,
        types: Arena::new(),
    };
    let ty = copy.ty(method.ty);
    let mut constraints = context.to_vec();
    constraints.extend(method.constraints.iter().map(|constraint| Constraint {
        class: constraint.class,
        var: copy.var(constraint.var),
        range: at,
    }));
    Signature {
        ty,
        range: at,
        types: copy.types,
        generics,
        constraints,
    }
}

/// メソッドのシグネチャの型の注釈を、instance のメソッドの関数のアリーナへ写す。木の深さは E0013 で抑えられているので
/// 再帰でたどる。
struct SignatureCopy<'a> {
    method: &'a Signature,
    head: TypeDefId,
    head_vars: usize,
    at: TextRange,
    types: Arena<TypeRef>,
}

impl SignatureCopy<'_> {
    /// メソッド自身の型変数 (番号 1 以降) の、新しい番号。
    fn var(&self, var: TypeVarId) -> TypeVarId {
        let index = u32::from(var.into_raw()) as usize;
        debug_assert!(index > 0, "the class variable is replaced by the head");
        Idx::from_raw(RawIdx::from((self.head_vars + index - 1) as u32))
    }

    fn ty(&mut self, id: TypeRefId) -> TypeRefId {
        let kind = match &self.method.types[id].kind {
            TypeRefKind::Error => TypeRefKind::Error,
            TypeRefKind::Var(var) if u32::from(var.into_raw()) == 0 => {
                let args = (0..self.head_vars)
                    .map(|i| self.alloc(TypeRefKind::Var(Idx::from_raw(RawIdx::from(i as u32)))))
                    .collect();
                TypeRefKind::Con(self.head, args)
            }
            TypeRefKind::Var(var) => TypeRefKind::Var(self.var(*var)),
            TypeRefKind::Con(id, args) => {
                let args = args.iter().map(|&arg| self.ty(arg)).collect();
                TypeRefKind::Con(*id, args)
            }
            TypeRefKind::Tuple(elements) => {
                TypeRefKind::Tuple(elements.iter().map(|&element| self.ty(element)).collect())
            }
            TypeRefKind::Fn { param, row, ret } => {
                let param = self.ty(*param);
                let row = self.row(row);
                let ret = self.ty(*ret);
                TypeRefKind::Fn { param, row, ret }
            }
        };
        self.alloc(kind)
    }

    fn row(&mut self, row: &RowRef) -> RowRef {
        match row {
            RowRef::Omitted => RowRef::Omitted,
            RowRef::Error => RowRef::Error,
            RowRef::Closed { effects, .. } => RowRef::Closed {
                effects: self.effects(effects),
                range: self.at,
            },
            RowRef::Open { effects, tail, .. } => RowRef::Open {
                effects: self.effects(effects),
                tail: *tail,
                range: self.at,
            },
        }
    }

    fn effects(&mut self, effects: &[EffectRef]) -> Vec<EffectRef> {
        effects
            .iter()
            .map(|effect| EffectRef {
                effect: effect.effect,
                args: effect.args.iter().map(|&arg| self.ty(arg)).collect(),
            })
            .collect()
    }

    fn alloc(&mut self, kind: TypeRefKind) -> TypeRefId {
        self.types.alloc(TypeRef {
            kind,
            range: self.at,
        })
    }
}

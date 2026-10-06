//! `data` の宣言の変換 (docs/spec/declarations.md の「`data` と `type`」)。名前の表と重複の判定は `DefMap` が持つ。

use eml_diagnostics::{Diagnostic, FileId, Label};
use la_arena::Arena;

use super::duplicate;
use super::types::{TypeLowering, Vars};
use crate::codes;
use crate::def_map::{DefMap, Resolver};
use crate::hir::{Constructor, Generics, ItemId, ModuleId, TypeDef, TypeDefKind, TypeVarDecl};
use crate::item_tree::DataItem;

/// 型の名前と型引数を置く。フィールドの型は `lower_constructors` が、すべての型を置いた後に変換する。
pub(super) fn declare_data(
    file: FileId,
    module: ModuleId,
    items: &[DataItem],
    def_map: &DefMap,
    types: &mut Arena<TypeDef>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (k, item) in items.iter().enumerate() {
        let mut generics = Generics::default();
        for param in item.syntax.params().map(|name| name.token()) {
            let text = param.text();
            let range = param.text_range();
            if let Some((_, first)) = generics.type_vars.iter().find(|(_, var)| var.name == text) {
                diagnostics.push(duplicate(file, text, first.range, range));
                continue;
            }
            generics.type_vars.alloc(TypeVarDecl {
                name: text.to_string(),
                range,
            });
        }
        // `=` のない `data` は、Prelude では処理系が表し方を決める intrinsic の型である。ユーザーのモジュールでは
        // 値を作れないので E1025 にする (docs/spec/declarations.md の「`data` と `type`」)
        let data = TypeDefKind::Data {
            constructors: Vec::new(),
        };
        let kind = match (item.syntax.has_constructors(), module == def_map.prelude()) {
            (true, _) => data,
            (false, true) => TypeDefKind::Builtin,
            (false, false) => {
                diagnostics.push(Diagnostic::error(
                    codes::MISSING_CONSTRUCTORS,
                    format!("`{}` has no constructors", item.name),
                    Label::new(
                        file,
                        item.name_range,
                        "add constructors after `=`, as in `= | A | B`",
                    ),
                ));
                data
            }
        };
        let id = ItemId::new(
            module,
            types.alloc(TypeDef {
                name: item.name.clone(),
                generics,
                types: Arena::new(),
                kind,
            }),
        );
        debug_assert_eq!(id, def_map.type_id(module, k));
    }
}

/// フィールドの型を変換してコンストラクタを置く。タグは宣言の中の順の番号である。重複した `data` のコンストラクタも
/// 置く。使えない印は `DefMap` が持つので、使った位置が診断なしで `Missing` になる
/// (docs/spec/diagnostics.md の「連鎖する診断の抑止」)。
pub(super) fn lower_constructors(
    file: FileId,
    module: ModuleId,
    items: &[DataItem],
    def_map: &DefMap,
    types: &mut Arena<TypeDef>,
    constructors: &mut Arena<Constructor>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let resolver: Resolver<'_> = def_map.resolver(module);
    for (k, item) in items.iter().enumerate() {
        let ty = def_map.type_id(module, k);
        for (j, constructor) in item.constructors.iter().enumerate() {
            let def = &mut types[ty.local];
            let mut lowering = TypeLowering {
                file,
                types: &mut def.types,
                generics: &mut def.generics,
                items: resolver,
                vars: Vars::Data,
                diagnostics: &mut *diagnostics,
            };
            let fields = constructor
                .syntax
                .fields()
                .map(|field| {
                    let range = field.range();
                    lowering.lower(Some(field), range)
                })
                .collect();
            let TypeDefKind::Data {
                constructors: declared,
            } = &mut def.kind
            else {
                unreachable!("`declare_data` makes data types")
            };
            let id = ItemId::new(
                module,
                constructors.alloc(Constructor {
                    name: constructor.name.clone(),
                    range: constructor.name_range,
                    ty,
                    tag: declared.len() as u32,
                    fields,
                }),
            );
            debug_assert_eq!(id, def_map.constructor_id(module, k, j));
            declared.push(id);
        }
    }
}

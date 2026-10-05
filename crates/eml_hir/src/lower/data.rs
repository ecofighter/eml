//! `data` の宣言の変換 (docs/spec/declarations.md の「`data` と `type`」)。型は型の名前空間に、コンストラクタは
//! 値の名前空間に置く (docs/spec/modules.md の「名前空間」)。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, TextRange};
use eml_syntax::ast;
use la_arena::Arena;

use super::duplicate;
use super::scope::ItemScope;
use super::types::{TypeLowering, Vars};
use crate::hir::{Constructor, Generics, TypeDef, TypeDefId, TypeDefKind, TypeVarDecl};

/// 型の名前と型引数だけを先に登録する。フィールドの型と操作のシグネチャが、後ろで宣言した型も引けるようにするため。
/// `declared` は型の名前空間のユーザーの名前で、エフェクトの宣言と共有して重複 (E1003) を見つける。
pub(super) fn declare_data(
    file: FileId,
    items: &[ast::DataItem],
    declared: &mut HashMap<String, TextRange>,
    scope: &mut ItemScope,
    types: &mut Arena<TypeDef>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(TypeDefId, ast::DataItem)> {
    let mut lowered = Vec::new();
    for item in items {
        // 名前がなければパーサが報告済み
        let Some(name) = item.name() else {
            continue;
        };
        let mut generics = Generics::default();
        for param in item.params() {
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
        let params = generics.type_vars.len();
        let range = name.text_range();
        let id = types.alloc(TypeDef {
            name: name.text().to_string(),
            generics,
            types: Arena::new(),
            kind: TypeDefKind::Data {
                constructors: Vec::new(),
            },
        });
        match declared.get(name.text()) {
            Some(&first) => diagnostics.push(duplicate(file, name.text(), first, range)),
            None => {
                declared.insert(name.text().to_string(), range);
                scope.define_type(name.text(), id, params);
            }
        }
        lowered.push((id, item.clone()));
    }
    lowered
}

/// フィールドの型を変換し、コンストラクタを値の名前空間に置く。タグは宣言の中の順の番号である。
pub(super) fn lower_constructors(
    file: FileId,
    data: &[(TypeDefId, ast::DataItem)],
    scope: &mut ItemScope,
    types: &mut Arena<TypeDef>,
    constructors: &mut Arena<Constructor>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut values: HashMap<String, TextRange> = HashMap::new();
    for (ty, item) in data {
        for alt in item.alts() {
            // 名前も演算子もなければパーサが報告済み
            let Some(name) = alt.name().or_else(|| alt.operator()) else {
                continue;
            };
            let def = &mut types[*ty];
            let mut lowering = TypeLowering {
                file,
                types: &mut def.types,
                generics: &mut def.generics,
                items: scope,
                vars: Vars::Data,
                diagnostics: &mut *diagnostics,
            };
            let fields = alt
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
            let range = name.text_range();
            let id = constructors.alloc(Constructor {
                name: name.text().to_string(),
                range,
                ty: *ty,
                tag: declared.len() as u32,
                fields,
            });
            declared.push(id);
            match values.get(name.text()) {
                Some(&first) => diagnostics.push(duplicate(file, name.text(), first, range)),
                None => {
                    values.insert(name.text().to_string(), range);
                    scope.define_constructor(name.text(), id);
                }
            }
        }
    }
}

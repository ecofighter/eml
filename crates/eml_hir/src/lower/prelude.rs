//! 組み込みの Prelude (`prelude.em`) の `data` とシグネチャを変換する。等式のないシグネチャは intrinsic の関数になる。
//! R7b-2 で Prelude を別のモジュールにする。

use std::collections::HashMap;

use eml_diagnostics::FileId;
use eml_syntax::ast;
use la_arena::Arena;

use super::data::{declare_data, lower_constructors};
use super::scope::ItemScope;
use super::types::{TypeLowering, Vars};
use crate::hir::{Constructor, Function, FunctionId, Generics, Signature, TypeDef};

/// Prelude の等式のないシグネチャを intrinsic の関数にする。`pub` の関数だけを名前の表に入れ、すべての関数を名前で
/// 返す。lang item は `pub` でない関数 (`negate`) も引くため。
pub(super) fn lower_prelude(
    scope: &mut ItemScope,
    types: &mut Arena<TypeDef>,
    constructors: &mut Arena<Constructor>,
    functions: &mut Arena<Function>,
) -> HashMap<String, FunctionId> {
    let (parse, syntax_errors) = eml_syntax::parse(FileId::PRELUDE, crate::PRELUDE_SOURCE);
    debug_assert!(syntax_errors.is_empty(), "{syntax_errors:?}");
    let tree = parse.tree();
    let mut diagnostics = Vec::new();
    // シグネチャが `Bool` を引けるように、`data` を先に変換する。重複を見る名前の表は Prelude だけのもので、ユーザーの
    // 定義との重複は E1003 にしない。ユーザーの定義は後で同じ名前を上書きし、Prelude の名前を隠す
    let data: Vec<ast::DataItem> = tree
        .items()
        .filter_map(|item| match item {
            ast::Item::DataItem(data) => Some(data),
            _ => None,
        })
        .collect();
    let declared = declare_data(
        FileId::PRELUDE,
        &data,
        &mut HashMap::new(),
        scope,
        types,
        &mut diagnostics,
    );
    lower_constructors(
        FileId::PRELUDE,
        &declared,
        scope,
        types,
        constructors,
        &mut diagnostics,
    );
    for item in tree.items() {
        let ast::Item::FixityItem(item) = item else {
            continue;
        };
        let fixity = super::fixity_of(&item).expect("every Prelude fixity is well formed");
        for op in item.operators().map(|name| name.token()) {
            scope.declare_prelude_fixity(op.text(), fixity);
        }
    }
    let mut prelude = HashMap::new();
    for item in tree.items() {
        let public = item.pub_keyword().is_some();
        let ast::Item::Signature(signature) = item else {
            continue;
        };
        let name = signature
            .name()
            .expect("every Prelude signature has a name")
            .token();
        let range = signature.ty().map_or(signature.range(), |ty| ty.range());
        let mut signature_types = Arena::new();
        let mut generics = Generics::default();
        let ty = TypeLowering {
            file: FileId::PRELUDE,
            types: &mut signature_types,
            generics: &mut generics,
            items: scope,
            vars: Vars::Define,
            diagnostics: &mut diagnostics,
        }
        .lower(signature.ty(), range);
        let id = functions.alloc(Function {
            name: name.text().to_string(),
            name_range: name.text_range(),
            signature_name_range: Some(name.text_range()),
            equation_ranges: Vec::new(),
            signature: Some(Signature {
                ty,
                range,
                types: signature_types,
                generics,
            }),
            body: None,
            intrinsic: true,
        });
        if public {
            scope.define_prelude_function(name.text(), id);
        }
        prelude.insert(name.text().to_string(), id);
    }
    debug_assert!(diagnostics.is_empty(), "{diagnostics:?}");
    prelude
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::scope::{Assoc, Fixity, ItemScope};

    #[test]
    fn prelude_fixities_follow_the_standard_table() {
        // docs/spec/declarations.md の標準の演算子の表
        let table: &[(&[&str], u8, Assoc)] = &[
            (&["<|"], 0, Assoc::Right),
            (&["|>"], 1, Assoc::Left),
            (&["||"], 2, Assoc::Right),
            (&["&&"], 3, Assoc::Right),
            (&["==", "!=", "<", "<=", ">", ">="], 4, Assoc::None),
            (&["++", "::"], 5, Assoc::Right),
            (&["+", "-"], 6, Assoc::Left),
            (&["*", "/", "%"], 7, Assoc::Left),
            (&[">>", "<<"], 9, Assoc::Right),
        ];
        let mut scope = ItemScope::new();
        let mut types = Arena::new();
        let mut effects = Arena::new();
        crate::lower::scope::builtin_items(&mut types, &mut effects, &mut scope);
        lower_prelude(&mut scope, &mut types, &mut Arena::new(), &mut Arena::new());
        for (ops, precedence, assoc) in table {
            for op in *ops {
                assert_eq!(
                    scope.fixity(op),
                    Fixity {
                        precedence: *precedence,
                        assoc: *assoc
                    },
                    "{op}"
                );
            }
        }
    }
}

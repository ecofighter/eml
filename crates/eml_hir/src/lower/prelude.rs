//! 組み込みの Prelude (`prelude.em`) の `data` とシグネチャを変換する。等式のないシグネチャは intrinsic の関数になる。
//! R7b-2 で Prelude を別のモジュールにする。

use std::collections::HashMap;

use eml_diagnostics::FileId;
use eml_syntax::ast;
use la_arena::Arena;

use super::data::{declare_data, lower_constructors};
use super::scope::ItemScope;
use super::types::{TypeLowering, Vars};
use crate::hir::{Function, Generics, Signature};
use crate::program::{FunctionId, ItemId, Items, ModuleId};

/// Prelude の等式のないシグネチャを intrinsic の関数にする。`pub` の関数だけを名前の表に入れ、すべての関数を名前で
/// 返す。lang item は `pub` でない関数 (`negate`) も引くため。
pub(super) fn lower_prelude(
    module: ModuleId,
    file: FileId,
    tree: &ast::SourceFile,
    items: &mut Items,
    scope: &mut ItemScope,
) -> HashMap<String, FunctionId> {
    let mut diagnostics = Vec::new();
    // シグネチャが `Bool` を引けるように、`data` を先に変換する。重複を見る名前の表は Prelude だけのもので、ユーザーの
    // 定義との重複は E1003 にしない。ユーザーの定義は後で同じ名前を上書きし、Prelude の名前を隠す
    let (items_of_tree, _) = crate::item_tree::item_tree(file, tree);
    let declared = declare_data(
        file,
        module,
        &items_of_tree.data,
        &mut HashMap::new(),
        scope,
        &mut items.types,
        &mut diagnostics,
    );
    lower_constructors(
        file,
        module,
        &declared,
        scope,
        &mut items.types,
        &mut items.constructors,
        &mut diagnostics,
    );
    for item in tree.items() {
        let ast::Item::FixityItem(item) = item else {
            continue;
        };
        let fixity =
            crate::item_tree::fixity_of(&item).expect("every Prelude fixity is well formed");
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
            file,
            types: &mut signature_types,
            generics: &mut generics,
            items: scope,
            vars: Vars::Define,
            diagnostics: &mut diagnostics,
        }
        .lower(signature.ty(), range);
        let id = ItemId::new(
            module,
            items.functions.alloc(Function {
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
                intrinsic: true,
            }),
        );
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
            (&["++"], 5, Assoc::Right),
            (&["+", "-"], 6, Assoc::Left),
            (&["*", "/", "%"], 7, Assoc::Left),
            (&[">>", "<<"], 9, Assoc::Right),
        ];
        let mut files = eml_diagnostics::SourceFiles::new();
        let file = files.add(crate::PRELUDE_PATH, crate::PRELUDE_SOURCE);
        let tree = crate::parse_prelude(file);
        let mut modules: Arena<crate::Module> = Arena::new();
        let module = modules.alloc(crate::Module::new(file, "Prelude"));
        let mut scope = ItemScope::new();
        crate::lower::scope::builtin_items(module, &mut modules[module].items, &mut scope);
        lower_prelude(module, file, &tree, &mut modules[module].items, &mut scope);
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

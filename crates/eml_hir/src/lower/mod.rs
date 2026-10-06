mod data;
mod effect;
mod expr;
mod handler;
mod ops;
mod section;
mod types;

use eml_diagnostics::{Diagnostic, Label};
use eml_syntax::{SyntaxToken, ast};
use la_arena::{Arena, ArenaMap, Idx};

use crate::codes;
use crate::def_map::{DefMap, module_id};
use crate::hir::*;
use crate::item_tree::{FunctionItem, ItemTree};
use crate::program::{ItemId, Items, Module, ModuleId, Program};
use expr::BodyLowering;
use types::{TypeLowering, Vars};

/// 全モジュールの item と本体を変換する。名前は `def_map` で引き、item はその局所の番号の順にアリーナへ置く
/// (docs/implementation/architecture.md の「`eml_hir` の内部」)。
pub fn lower(def_map: &DefMap, trees: &[ItemTree]) -> (Program, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut modules = Arena::new();
    for (index, tree) in trees.iter().enumerate() {
        let module = module_id(index);
        let id = modules.alloc(Module::new(tree.file, def_map.module_name(module)));
        debug_assert_eq!(id, module);
    }
    for (index, tree) in trees.iter().enumerate() {
        let module = module_id(index);
        lower_items(
            def_map,
            module,
            tree,
            &mut modules[module].items,
            &mut diagnostics,
        );
    }
    let lang = def_map.lang();
    // Core IR は `Bool` を、タグ 0 の `False` と 1 の `True` で表す (docs/spec/core-ir.md)
    let tag = |id: ConstructorId| modules[id.module].items.constructors[id.local].tag;
    assert_eq!((tag(lang.false_ctor), tag(lang.true_ctor)), (0, 1));
    for (index, tree) in trees.iter().enumerate() {
        let module = module_id(index);
        let bodies = lower_bodies(def_map, module, tree, &mut modules, &mut diagnostics);
        modules[module].bodies = bodies;
    }
    (
        Program {
            modules,
            prelude: def_map.prelude(),
            entry: def_map.entry(),
            lang,
        },
        diagnostics,
    )
}

/// item を `DefMap` と同じ局所の番号の順に置く。`ItemTree` の順である。
fn lower_items(
    def_map: &DefMap,
    module: ModuleId,
    tree: &ItemTree,
    items: &mut Items,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let file = tree.file;
    data::declare_data(
        file,
        module,
        &tree.data,
        def_map,
        &mut items.types,
        diagnostics,
    );
    effect::declare_effects(
        file,
        module,
        &tree.effects,
        def_map,
        &mut items.effects,
        diagnostics,
    );
    effect::lower_operations(file, module, &tree.effects, def_map, items, diagnostics);
    data::lower_constructors(
        file,
        module,
        &tree.data,
        def_map,
        &mut items.types,
        &mut items.constructors,
        diagnostics,
    );
    let in_prelude = module == def_map.prelude();
    let resolver = def_map.resolver(module);
    for (k, function) in tree.functions.iter().enumerate() {
        let FunctionItem {
            name,
            first_range,
            signature,
            equations,
            ..
        } = function;
        // Prelude の等式のないシグネチャは intrinsic の関数で、E1005 にしない
        // (docs/implementation/architecture.md の「`eml_hir` の内部」)
        let intrinsic = in_prelude && equations.is_empty();
        if let (Some((_, range)), None, false) = (signature, equations.first(), intrinsic) {
            diagnostics.push(Diagnostic::error(
                codes::MISSING_EQUATION,
                format!("`{name}` has a signature but no equation"),
                Label::new(
                    file,
                    *range,
                    format!("add an equation for `{name}` after this signature"),
                ),
            ));
        }
        let signature_name_range = signature.as_ref().map(|(_, range)| *range);
        let signature = signature.as_ref().map(|(node, _)| {
            let range = node.ty().map_or(node.range(), |ty| ty.range());
            let mut types = Arena::new();
            let mut generics = Generics::default();
            let ty = TypeLowering {
                file,
                types: &mut types,
                generics: &mut generics,
                items: resolver,
                vars: Vars::Define,
                diagnostics: &mut *diagnostics,
            }
            .lower(node.ty(), range);
            Signature {
                ty,
                range,
                types,
                generics,
            }
        });
        let id = ItemId::new(
            module,
            items.functions.alloc(Function {
                name: name.clone(),
                name_range: equations.first().map_or(*first_range, |(_, range)| *range),
                signature_name_range,
                equation_ranges: equations.iter().map(|(_, range)| *range).collect(),
                signature,
                intrinsic,
            }),
        );
        debug_assert_eq!(id, def_map.function_id(module, k));
    }
}

/// 本体は、すべてのモジュールの item を置いてから変換する。後ろで定義した関数も、ほかのモジュールの item も引けるように
/// するため。
fn lower_bodies(
    def_map: &DefMap,
    module: ModuleId,
    tree: &ItemTree,
    modules: &mut Arena<Module>,
    diagnostics: &mut Vec<Diagnostic>,
) -> ArenaMap<Idx<Function>, Body> {
    let mut bodies = ArenaMap::default();
    for (k, function) in tree.functions.iter().enumerate() {
        if function.equations.is_empty() {
            continue;
        }
        let id = def_map.function_id(module, k);
        // シグネチャの型変数の表は関数のアリーナの中にあり、本体の変換はほかの item を同じアリーナから読む。
        // そのため、変換の間だけ表を取り出す。シグネチャがなければ、本体の注釈は型変数を引けない
        // (docs/spec/types.md の「推論」)
        let mut generics = modules[module].items.functions[id.local]
            .signature
            .as_mut()
            .map(|signature| std::mem::take(&mut signature.generics))
            .unwrap_or_default();
        let body = BodyLowering::new(
            tree.file,
            def_map.resolver(module),
            modules,
            def_map.lang(),
            &mut generics,
            diagnostics,
        )
        .lower_equations(&function.equations);
        if let Some(signature) = &mut modules[module].items.functions[id.local].signature {
            signature.generics = generics;
        }
        bodies.insert(id.local, body);
    }
    bodies
}

/// 名前の経路の読み方。修飾名は S2 で実装する (docs/spec/modules.md)。
pub(super) enum PathName {
    Plain(SyntaxToken),
    Qualified,
    /// パーサが報告済み。
    Missing,
}

pub(super) fn path_name(path: Option<ast::Path>) -> PathName {
    match path {
        Some(path) if path.is_qualified() => PathName::Qualified,
        Some(path) => path
            .name()
            .map_or(PathName::Missing, |name| PathName::Plain(name.token())),
        None => PathName::Missing,
    }
}

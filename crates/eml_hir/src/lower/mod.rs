mod data;
mod effect;
mod expr;
mod handler;
mod ops;
mod prelude;
mod scope;
mod section;
mod types;

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};
use la_arena::{Arena, ArenaMap};

use crate::codes;
use crate::hir::*;
use crate::item_tree::{FixityItem, FunctionItem, item_tree};
use crate::program::{ItemId, Module, Program};
use expr::BodyLowering;
use scope::{ItemScope, ValueItem};
use types::{TypeLowering, Vars};

/// Prelude と入口のファイルを、別々のモジュールに変換する。名前解決は R7b-3 で `ItemTree` と `DefMap` に分けるまで、
/// 1つの名前の表 (`ItemScope`) で行う。
pub fn lower(
    prelude: (FileId, &ast::SourceFile),
    main: (FileId, &ast::SourceFile),
) -> (Program, Vec<Diagnostic>) {
    let (file, source) = main;
    let mut diagnostics = Vec::new();
    let mut modules = Arena::new();
    let prelude_id = modules.alloc(Module::new(prelude.0, "Prelude"));
    let main_id = modules.alloc(Module::new(file, "Main"));
    let mut scope = ItemScope::new();
    let builtin = scope::builtin_items(prelude_id, &mut modules[prelude_id].items, &mut scope);
    let prelude_functions = prelude::lower_prelude(
        prelude_id,
        prelude.0,
        prelude.1,
        &mut modules[prelude_id].items,
        &mut scope,
    );
    // ユーザーの定義が `Bool`、`True`、`False` を隠す前に引く
    let lang = scope::lang_items(builtin, &scope, &modules, &prelude_functions);
    let (tree, found) = item_tree(file, source);
    diagnostics.extend(found);
    let items = &mut modules[main_id].items;
    // 型の名前空間のユーザーの名前。`data` とエフェクトの間の重複も見つける
    let mut type_names = HashMap::new();
    let data = data::declare_data(
        file,
        main_id,
        &tree.data,
        &mut type_names,
        &mut scope,
        &mut items.types,
        &mut diagnostics,
    );
    // 関数のシグネチャの row がユーザーのエフェクトを引けるように、エフェクトを先に変換する
    effect::lower_effects(
        file,
        main_id,
        &tree.effects,
        &mut type_names,
        &mut scope,
        items,
        &mut diagnostics,
    );
    // フィールドの関数型の row がエフェクトを引けるように、コンストラクタはエフェクトの後に変換する
    data::lower_constructors(
        file,
        main_id,
        &data,
        &mut scope,
        &mut items.types,
        &mut items.constructors,
        &mut diagnostics,
    );
    let mut pending = Vec::new();
    for function in &tree.functions {
        let FunctionItem {
            name,
            first_range,
            signature,
            equations,
            ..
        } = function;
        let (name, first_range) = (name.clone(), *first_range);
        if let (Some((_, range)), None) = (signature, equations.first()) {
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
                items: &scope,
                vars: Vars::Define,
                diagnostics: &mut diagnostics,
            }
            .lower(node.ty(), range);
            Signature {
                ty,
                range,
                types,
                generics,
            }
        });
        let name_range = equations.first().map_or(first_range, |(_, range)| *range);
        let id = ItemId::new(
            main_id,
            items.functions.alloc(Function {
                name: name.clone(),
                name_range,
                signature_name_range,
                equation_ranges: equations.iter().map(|(_, range)| *range).collect(),
                signature,
                intrinsic: false,
            }),
        );
        if let Some(ValueItem::Operation(operation)) = scope.define_function(&name, id) {
            diagnostics.push(duplicate(
                file,
                &name,
                items.operations[operation.local].name_range,
                first_range,
            ));
        }
        if !equations.is_empty() {
            pending.push((id, equations.clone()));
        }
    }
    // fixity の宣言は位置によらずモジュール全体の組み直しに効くので、本体の変換の前に、すべての値を定義してから読む
    declare_fixities(file, &tree.fixities, &mut scope, &mut diagnostics);
    // 本体は、すべての関数の名前がそろってから変換する。後ろで定義した関数も呼べるようにするため
    let mut bodies = ArenaMap::default();
    for (id, equations) in pending {
        // シグネチャの型変数の表は関数のアリーナの中にあり、本体の変換はほかの item を同じアリーナから読む。
        // そのため、変換の間だけ表を取り出す。シグネチャがなければ、本体の注釈は型変数を引けない
        // (docs/spec/types.md の「推論」)
        let mut generics = modules[main_id].items.functions[id.local]
            .signature
            .as_mut()
            .map(|signature| std::mem::take(&mut signature.generics))
            .unwrap_or_default();
        let body = BodyLowering::new(
            file,
            &scope,
            &modules,
            lang,
            &mut generics,
            &mut diagnostics,
        )
        .lower_equations(&equations);
        if let Some(signature) = &mut modules[main_id].items.functions[id.local].signature {
            signature.generics = generics;
        }
        bodies.insert(id.local, body);
    }
    modules[main_id].bodies = bodies;
    (
        Program {
            modules,
            prelude: prelude_id,
            entry: main_id,
            lang,
        },
        diagnostics,
    )
}

/// ユーザーの fixity の宣言を表に入れる。同じ演算子への2回目の宣言は E1021、このモジュールで定義していない演算子
/// への宣言は E1022 にし、どちらも表に入れない (docs/spec/declarations.md の「fixity」)。
fn declare_fixities(
    file: FileId,
    items: &[FixityItem],
    scope: &mut ItemScope,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in items {
        let Some(fixity) = item.fixity else {
            continue;
        };
        for (name, range) in &item.operators {
            let (name, range) = (name.as_str(), *range);
            if !scope.defines_value(name) {
                diagnostics.push(Diagnostic::error(
                    codes::FIXITY_WITHOUT_DEFINITION,
                    format!("`{name}` is not defined in this module"),
                    Label::new(
                        file,
                        range,
                        "a fixity declaration needs a definition of its operator in the same module",
                    ),
                ));
                continue;
            }
            if let Err(first) = scope.declare_fixity(name, fixity, range) {
                diagnostics.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FIXITY,
                        format!("`{name}` has more than one fixity declaration"),
                        Label::new(file, range, "declared again here"),
                    )
                    .with_secondary(Label::new(
                        file,
                        first,
                        "first declared here",
                    )),
                );
            }
        }
    }
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

/// 同じ名前空間のトップレベルの定義の重複 (docs/spec/modules.md の「名前空間」)。ソースで後に書いた方を primary にする。
pub(super) fn duplicate(file: FileId, name: &str, a: TextRange, b: TextRange) -> Diagnostic {
    let (first, again) = if a.start() <= b.start() {
        (a, b)
    } else {
        (b, a)
    };
    Diagnostic::error(
        codes::DUPLICATE_DEFINITION,
        format!("`{name}` is defined more than once"),
        Label::new(file, again, "defined again here"),
    )
    .with_secondary(Label::new(file, first, "first defined here"))
}

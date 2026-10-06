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
use eml_syntax::{SyntaxKind, SyntaxToken, ast};
use la_arena::{Arena, ArenaMap};

use crate::codes;
use crate::hir::*;
use crate::program::{ItemId, Module, Program};
use expr::BodyLowering;
use scope::{Assoc, Fixity, ItemScope, ValueItem};
use types::{TypeLowering, Vars};

/// 同じ名前のシグネチャと等式。名前で対応づけてから、並び方を検査する (docs/spec/declarations.md)。
struct Definition {
    name: String,
    first_range: TextRange,
    /// (item の番号, シグネチャ, 名前の位置)
    signature: Option<(usize, ast::Signature, TextRange)>,
    /// (item の番号, 等式, 名前の位置)
    equations: Vec<(usize, ast::Equation, TextRange)>,
}

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
    let (definitions, data_items, effect_items, fixity_items) =
        collect(file, source, &mut diagnostics);
    let items = &mut modules[main_id].items;
    // 型の名前空間のユーザーの名前。`data` とエフェクトの間の重複も見つける
    let mut type_names = HashMap::new();
    let data = data::declare_data(
        file,
        main_id,
        &data_items,
        &mut type_names,
        &mut scope,
        &mut items.types,
        &mut diagnostics,
    );
    // 関数のシグネチャの row がユーザーのエフェクトを引けるように、エフェクトを先に変換する
    effect::lower_effects(
        file,
        main_id,
        &effect_items,
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
    for definition in definitions {
        let Definition {
            name,
            first_range,
            signature,
            equations,
        } = definition;
        // 等式は連続していなければならない (docs/spec/declarations.md)。離れていても、網羅性の誤りを連鎖させないよう
        // ソースの順に1つの関数として扱う
        for pair in equations.windows(2) {
            let (previous_index, _, previous_range) = &pair[0];
            let (index, _, range) = &pair[1];
            if *index != previous_index + 1 {
                diagnostics.push(
                    Diagnostic::error(
                        codes::NON_CONSECUTIVE_EQUATIONS,
                        format!("the equations of `{name}` are not consecutive"),
                        Label::new(
                            file,
                            *range,
                            "this equation is separated from the ones above",
                        ),
                    )
                    .with_secondary(Label::new(file, *previous_range, "the previous equation"))
                    .with_help(format!(
                        "put every equation of `{name}` together, right after its signature"
                    )),
                );
            }
        }
        match (&signature, equations.first()) {
            (Some((_, _, range)), None) => diagnostics.push(Diagnostic::error(
                codes::MISSING_EQUATION,
                format!("`{name}` has a signature but no equation"),
                Label::new(
                    file,
                    *range,
                    format!("add an equation for `{name}` after this signature"),
                ),
            )),
            (None, Some((_, _, range))) => diagnostics.push(
                Diagnostic::error(
                    codes::MISSING_SIGNATURE,
                    format!("`{name}` has no type signature"),
                    Label::new(file, *range, "every top-level definition needs a signature"),
                )
                .with_help(format!(
                    "add a signature `{name} : ...` on the line before this equation"
                )),
            ),
            (Some((signature_index, _, signature_range)), Some((equation_index, _, range)))
                if *equation_index != signature_index + 1 =>
            {
                diagnostics.push(
                    Diagnostic::error(
                        codes::SIGNATURE_NOT_ADJACENT,
                        format!("the signature of `{name}` is not followed by its equations"),
                        Label::new(
                            file,
                            *range,
                            format!("this equation is not right after the signature of `{name}`"),
                        ),
                    )
                    .with_secondary(Label::new(file, *signature_range, "the signature is here"))
                    .with_help(format!(
                        "move the equations of `{name}` right after its signature"
                    )),
                );
            }
            _ => {}
        }
        let signature_name_range = signature.as_ref().map(|(_, _, range)| *range);
        let signature = signature.map(|(_, node, _)| {
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
        let name_range = equations
            .first()
            .map_or(first_range, |(_, _, range)| *range);
        let id = ItemId::new(
            main_id,
            items.functions.alloc(Function {
                name: name.clone(),
                name_range,
                signature_name_range,
                equation_ranges: equations.iter().map(|(_, _, range)| *range).collect(),
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
            pending.push((
                id,
                equations
                    .into_iter()
                    .map(|(_, equation, range)| (equation, range))
                    .collect::<Vec<_>>(),
            ));
        }
    }
    // fixity の宣言は位置によらずモジュール全体の組み直しに効くので、本体の変換の前に、すべての値を定義してから読む
    declare_fixities(file, &fixity_items, &mut scope, &mut diagnostics);
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

fn collect(
    file: FileId,
    source: &ast::SourceFile,
    diagnostics: &mut Vec<Diagnostic>,
) -> (
    Vec<Definition>,
    Vec<ast::DataItem>,
    Vec<ast::EffectItem>,
    Vec<ast::FixityItem>,
) {
    let mut definitions: Vec<Definition> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut data = Vec::new();
    let mut effects = Vec::new();
    let mut fixities = Vec::new();
    for (index, item) in source.items().enumerate() {
        // `pub` は R7b で実装する (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 1.5)
        if let Some(keyword) = item.pub_keyword() {
            diagnostics.push(Diagnostic::not_yet_supported(
                file,
                keyword.text_range(),
                "`pub` is not supported yet",
            ));
        }
        match item {
            ast::Item::Signature(signature) => {
                let Some(name) = value_name(signature.name()) else {
                    continue;
                };
                let range = name.text_range();
                let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                match &definitions[slot].signature {
                    Some((_, _, first)) => diagnostics.push(
                        Diagnostic::error(
                            codes::DUPLICATE_DEFINITION,
                            format!("`{}` is defined more than once", name.text()),
                            Label::new(file, range, "defined again here"),
                        )
                        .with_secondary(Label::new(
                            file,
                            *first,
                            "first defined here",
                        )),
                    ),
                    None => definitions[slot].signature = Some((index, signature, range)),
                }
            }
            ast::Item::Equation(equation) => {
                let Some(name) = value_name(equation.name()) else {
                    continue;
                };
                let range = name.text_range();
                let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                definitions[slot].equations.push((index, equation, range));
            }
            ast::Item::DataItem(item) => data.push(item),
            ast::Item::EffectItem(item) => effects.push(item),
            ast::Item::FixityItem(item) => fixities.push(item),
            ast::Item::TypeItem(item) => {
                let range = item
                    .type_keyword()
                    .map_or(item.range(), |keyword| keyword.text_range());
                diagnostics.push(Diagnostic::not_yet_supported(
                    file,
                    range,
                    "`type` declarations are not supported yet",
                ));
            }
            ast::Item::ImportItem(item) => {
                let range = item
                    .import_keyword()
                    .map_or(item.range(), |keyword| keyword.text_range());
                diagnostics.push(Diagnostic::not_yet_supported(
                    file,
                    range,
                    "`import` is not supported yet",
                ));
            }
        }
    }
    (definitions, data, effects, fixities)
}

/// fixity の宣言の結合と優先順位。優先順位の範囲の誤りはパーサが報告済みなので、読めなければ `None` にする。
fn fixity_of(item: &ast::FixityItem) -> Option<Fixity> {
    let assoc = match item.assoc()?.kind() {
        SyntaxKind::INFIXL_KW => Assoc::Left,
        SyntaxKind::INFIXR_KW => Assoc::Right,
        _ => Assoc::None,
    };
    let precedence = item
        .precedence()?
        .text()
        .parse::<u8>()
        .ok()
        .filter(|precedence| *precedence <= 9)?;
    Some(Fixity { precedence, assoc })
}

/// ユーザーの fixity の宣言を表に入れる。同じ演算子への2回目の宣言は E1021、このモジュールで定義していない演算子
/// への宣言は E1022 にし、どちらも表に入れない (docs/spec/declarations.md の「fixity」)。
fn declare_fixities(
    file: FileId,
    items: &[ast::FixityItem],
    scope: &mut ItemScope,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in items {
        let Some(fixity) = fixity_of(item) else {
            continue;
        };
        for op in item.operators().map(|name| name.token()) {
            let (name, range) = (op.text(), op.text_range());
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

fn slot(
    definitions: &mut Vec<Definition>,
    by_name: &mut HashMap<String, usize>,
    name: &str,
    range: TextRange,
) -> usize {
    *by_name.entry(name.to_string()).or_insert_with(|| {
        definitions.push(Definition {
            name: name.to_string(),
            first_range: range,
            signature: None,
            equations: Vec::new(),
        });
        definitions.len() - 1
    })
}

/// シグネチャと等式の名前。演算子の定義 (`(</>) : …` と `a </> b = …`) は、演算子の文字列を名前にした関数である
/// (docs/spec/declarations.md の「fixity」)。名前がなければパーサが報告済み。
fn value_name(name: Option<ast::Name>) -> Option<SyntaxToken> {
    name.map(|name| name.token()).filter(|token| {
        matches!(
            token.kind(),
            SyntaxKind::LIDENT | SyntaxKind::OP | SyntaxKind::MINUS
        )
    })
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

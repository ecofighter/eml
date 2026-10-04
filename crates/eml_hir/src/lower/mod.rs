mod expr;
mod ops;
mod types;

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxToken, ast};
use la_arena::Arena;

use crate::codes;
use crate::hir::*;
use expr::BodyLowering;
use types::{TypeLowering, TypeScope};

/// 同じ名前のシグネチャと等式。名前で対応づけてから、並び方を検査する (docs/spec/declarations.md)。
struct Definition {
    name: String,
    first_range: TextRange,
    /// (item の番号, シグネチャ, 名前の位置)
    signature: Option<(usize, ast::Signature, TextRange)>,
    /// (item の番号, 等式, 名前の位置)
    equations: Vec<(usize, ast::Equation, TextRange)>,
}

pub fn lower(file: FileId, source: &ast::SourceFile) -> (Module, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let definitions = collect(file, source, &mut diagnostics);
    let mut functions = Arena::new();
    let mut names = HashMap::new();
    let mut pending = Vec::new();
    for definition in definitions {
        let Definition {
            name,
            first_range,
            signature,
            equations,
        } = definition;
        let mut equations = equations.into_iter();
        let first_equation = equations.next();
        for (_, _, range) in equations {
            // 複数の等式は段階6で `match` に脱糖する
            diagnostics.push(Diagnostic::not_yet_supported(
                file,
                range,
                "defining a function with several equations is not supported yet",
            ));
        }
        match (&signature, &first_equation) {
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
            (Some((signature_index, _, _)), Some((equation_index, _, range)))
                if *equation_index != signature_index + 1 =>
            {
                // 離れたシグネチャと等式は段階6で E1xxx の検査にする
                diagnostics.push(Diagnostic::not_yet_supported(
                    file,
                    *range,
                    "an equation that does not directly follow its signature is not supported yet",
                ));
            }
            _ => {}
        }
        let mut types = Arena::new();
        let mut type_vars = Arena::new();
        let mut row_vars = Arena::new();
        let signature = signature.map(|(_, node, _)| {
            let range = node.ty().map_or(node.range(), |ty| ty.range());
            let ty = TypeLowering {
                file,
                types: &mut types,
                type_vars: &mut type_vars,
                row_vars: &mut row_vars,
                define: true,
                diagnostics: &mut diagnostics,
            }
            .lower(node.ty(), range);
            Signature { ty, range }
        });
        let name_range = first_equation
            .as_ref()
            .map_or(first_range, |(_, _, range)| *range);
        let id = functions.alloc(Function {
            name: name.clone(),
            name_range,
            signature,
            body: None,
            types,
            type_vars,
            row_vars,
        });
        names.insert(name, id);
        if let Some((_, equation, _)) = first_equation {
            pending.push((id, equation));
        }
    }
    // 本体は、すべての関数の名前がそろってから変換する。後ろで定義した関数も呼べるようにするため
    for (id, equation) in pending {
        let function = &mut functions[id];
        let scope = TypeScope {
            types: &mut function.types,
            type_vars: &mut function.type_vars,
            row_vars: &mut function.row_vars,
        };
        let body =
            BodyLowering::new(file, &names, scope, &mut diagnostics).lower_equation(&equation);
        functions[id].body = Some(body);
    }
    diagnostics.sort_by_key(|d| d.primary.range.start());
    (Module { file, functions }, diagnostics)
}

fn collect(
    file: FileId,
    source: &ast::SourceFile,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Definition> {
    let mut definitions: Vec<Definition> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for (index, item) in source.items().enumerate() {
        match item {
            ast::Item::Signature(signature) => {
                let Some(name) = value_name(file, signature.name(), diagnostics) else {
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
                let Some(name) = value_name(file, equation.name(), diagnostics) else {
                    continue;
                };
                let range = name.text_range();
                let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                definitions[slot].equations.push((index, equation, range));
            }
            ast::Item::DataItem(item) => diagnostics.push(Diagnostic::not_yet_supported(
                file,
                item.keyword_range(),
                "`data` declarations are not supported yet",
            )),
            ast::Item::EffectItem(item) => diagnostics.push(Diagnostic::not_yet_supported(
                file,
                item.keyword_range(),
                "`effect` declarations are not supported yet",
            )),
            ast::Item::FixityItem(item) => diagnostics.push(Diagnostic::not_yet_supported(
                file,
                item.keyword_range(),
                "fixity declarations are not supported yet",
            )),
            // `type` は構文の段階 S2 の構文で、パーサが E0004 を報告済み
            ast::Item::TypeItem(_) => {}
        }
    }
    definitions
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

/// 演算子の定義は、fixity の宣言と一緒に段階6で扱う。名前がなければパーサが報告済み。
fn value_name(
    file: FileId,
    token: Option<SyntaxToken>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<SyntaxToken> {
    let token = token?;
    if token.kind() == SyntaxKind::LIDENT {
        return Some(token);
    }
    diagnostics.push(Diagnostic::not_yet_supported(
        file,
        token.text_range(),
        "defining operators is not supported yet",
    ));
    None
}

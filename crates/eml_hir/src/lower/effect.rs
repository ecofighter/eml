//! `effect` の宣言の変換 (docs/spec/declarations.md の「`effect`」)。エフェクトは型の名前空間に、操作は値の名前空間に
//! 置く (docs/spec/modules.md の「名前空間」)。

use eml_diagnostics::{Diagnostic, FileId, Label};
use eml_syntax::{SyntaxKind, ast};
use la_arena::Arena;

use super::duplicate;
use super::types::{TypeLowering, Vars};
use crate::codes;
use crate::def_map::{DefMap, Resolver};
use crate::hir::{
    EffectDef, EffectId, Generics, ItemId, ModuleId, OpMultiplicity, Operation, RowRef, Signature,
    TypeRef, TypeRefId, TypeRefKind, TypeVarDecl, TypeVarId,
};
use crate::item_tree::EffectItem;
use crate::program::Items;

/// エフェクトの名前と型引数を置く。操作のシグネチャは `lower_operations` が、すべての型とエフェクトを置いた後に
/// 変換する。
pub(super) fn declare_effects(
    file: FileId,
    module: ModuleId,
    items: &[EffectItem],
    def_map: &DefMap,
    effects: &mut Arena<EffectDef>,
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
        let id = ItemId::new(
            module,
            effects.alloc(EffectDef {
                name: item.name.clone(),
                generics,
                operations: Vec::new(),
            }),
        );
        debug_assert_eq!(id, def_map.effect_id(module, k));
    }
}

/// 操作のシグネチャを変換して置く。重複した `effect` の操作も置く。使えない印は `DefMap` が持つ。
pub(super) fn lower_operations(
    file: FileId,
    module: ModuleId,
    items: &[EffectItem],
    def_map: &DefMap,
    module_items: &mut Items,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let resolver = def_map.resolver(module);
    let Items {
        effects,
        operations,
        ..
    } = module_items;
    for (k, item) in items.iter().enumerate() {
        let effect = def_map.effect_id(module, k);
        for (j, decl) in item.operations.iter().enumerate() {
            let operation = lower_operation(
                file,
                &decl.syntax,
                effect,
                &effects[effect.local].generics,
                resolver,
                diagnostics,
            )
            .expect("an operation of the item tree has a name");
            let name = operation.name.clone();
            // 同じエフェクトに同じ名前の操作を重ねても、並びには最初の1つだけを入れる。節の名前は最初の操作に解決
            // されるので、2つ目を入れると、重複 (E1003) に加えて節のない操作 (E1013) まで報告してしまう
            let repeated = effects[effect.local]
                .operations
                .iter()
                .any(|&op| operations[op.local].name == name);
            let id = ItemId::new(module, operations.alloc(operation));
            debug_assert_eq!(id, def_map.operation_id(module, k, j));
            if !repeated {
                effects[effect.local].operations.push(id);
            }
        }
    }
}

fn lower_operation(
    file: FileId,
    decl: &ast::OpDecl,
    effect: EffectId,
    effect_generics: &Generics,
    scope: Resolver<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Operation> {
    // 名前がなければパーサが報告済み
    let name = decl.name()?.token();
    let multiplicity = match decl.multiplicity() {
        Some(token) if token.kind() == SyntaxKind::NEVER_KW => OpMultiplicity::Never,
        Some(token) if token.kind() == SyntaxKind::MULTI_KW => OpMultiplicity::Multi,
        _ => OpMultiplicity::Once,
    };
    let range = decl.ty().map_or(decl.range(), |ty| ty.range());
    let mut types = Arena::new();
    // エフェクトの型引数を先頭に写す。シグネチャで同じ名前の型変数は、それを指す (docs/spec/declarations.md の「`effect`」)
    let mut generics = Generics::default();
    for (_, param) in effect_generics.type_vars.iter() {
        generics.type_vars.alloc(param.clone());
    }
    let effect_params = generics.type_vars.len();
    let ty = TypeLowering {
        file,
        types: &mut types,
        generics: &mut generics,
        items: scope,
        vars: Vars::Define,
        diagnostics: &mut *diagnostics,
    }
    .lower(decl.ty(), range);
    let signature = Signature {
        ty,
        range,
        types,
        generics,
    };
    let arity = check_signature(
        file,
        name.text(),
        &signature,
        multiplicity,
        effect_params,
        diagnostics,
    );
    Some(Operation {
        name: name.text().to_string(),
        name_range: name.text_range(),
        effect,
        multiplicity,
        signature,
        effect_params,
        arity,
    })
}

/// 一番外側の `->` をたどり、引数の個数を返す。外側の矢印の row と、関数型でないシグネチャを E1007 に、`never` の
/// 操作の結果の型の誤りを E1008 にする (docs/spec/declarations.md の「`effect`」)。操作の型の row は、型検査が最後の
/// 外側の矢印に付けるので、外側の矢印に row を書く場所はない。引数のない操作には、row を付ける矢印もない。
fn check_signature(
    file: FileId,
    name: &str,
    signature: &Signature,
    multiplicity: OpMultiplicity,
    effect_params: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let types = &signature.types;
    let mut params = Vec::new();
    let mut id = signature.ty;
    while let TypeRefKind::Fn { param, row, ret } = &types[id].kind {
        if let RowRef::Closed { range, .. } | RowRef::Open { range, .. } = row {
            diagnostics.push(
                Diagnostic::error(
                    codes::INVALID_OPERATION_SIGNATURE,
                    "an operation cannot have a row on its outermost arrows",
                    Label::new(file, *range, "remove this row"),
                )
                .with_note("an operation performs only the effect it belongs to"),
            );
        }
        params.push(*param);
        id = *ret;
    }
    if params.is_empty() {
        // 型が壊れていれば、変換が報告済み
        if !matches!(types[id].kind, TypeRefKind::Error) {
            diagnostics.push(
                Diagnostic::error(
                    codes::INVALID_OPERATION_SIGNATURE,
                    "the signature of an operation must be a function type",
                    Label::new(file, signature.range, "this type is not a function type"),
                )
                .with_help(format!(
                    "an operation without arguments takes `Unit`, as in `{name} : Unit -> ...`"
                )),
            );
        }
        return 0;
    }
    if multiplicity == OpMultiplicity::Never {
        let effect_param = match types[id].kind {
            TypeRefKind::Var(var) => (u32::from(var.into_raw()) as usize) < effect_params,
            _ => false,
        };
        let free = match types[id].kind {
            // エフェクトの型引数は handle ごとに決まるので、呼び出した側が自由な型として使えない
            TypeRefKind::Var(_) if effect_param => false,
            TypeRefKind::Var(var) => !params.iter().any(|&param| mentions(types, param, var)),
            TypeRefKind::Error => true,
            TypeRefKind::Con(..) | TypeRefKind::Fn { .. } | TypeRefKind::Tuple(_) => false,
        };
        if !free {
            let label = if effect_param {
                "this is a type parameter of the effect"
            } else {
                "this result type"
            };
            diagnostics.push(
                Diagnostic::error(
                    codes::NEVER_RESULT_NOT_FREE,
                    "the result type of a `never` operation must be a type variable that does not appear in its parameters",
                    Label::new(file, types[id].range, label),
                )
                .with_note(
                    "a `never` operation does not return, so its caller may use the result as any type",
                ),
            );
        }
    }
    params.len()
}

fn mentions(types: &Arena<TypeRef>, id: TypeRefId, var: TypeVarId) -> bool {
    match &types[id].kind {
        TypeRefKind::Var(other) => *other == var,
        TypeRefKind::Fn { param, ret, .. } => {
            mentions(types, *param, var) || mentions(types, *ret, var)
        }
        TypeRefKind::Con(_, args) => args.iter().any(|&arg| mentions(types, arg, var)),
        TypeRefKind::Tuple(elements) => elements
            .iter()
            .any(|&element| mentions(types, element, var)),
        TypeRefKind::Error => false,
    }
}

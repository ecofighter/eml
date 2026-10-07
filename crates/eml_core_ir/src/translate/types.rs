//! 型から決まる変数の性質 (boxed かどうか) と、`==` と `!=` を比べ方ごとのどの extern にするか。

use eml_extern::Extern;
use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{Equality, Type};

use crate::VarInfo;

/// ヒープに置く値の型。ボックス化した変数が RC の対象になる。関数値と型変数の値は、ヒープのクロージャや
/// 文字列かもしれない。インタプリタの `dup` / `decref` はヒープにない値を無視するので、多めに対象にしても正しく動く
/// (docs/spec/core-ir.md)。extern の型がヒープのオブジェクトか (`String`、`File`) は、表の行が決める。
fn boxed(ty: &Type, hir: &HirProgram) -> bool {
    match ty {
        Type::Con { id, .. } => match &hir[*id].kind {
            TypeDefKind::Extern(row) => row.is_some_and(|ty| ty.row().heap),
            TypeDefKind::Data { constructors: _ } => has_fields(hir, *id),
        },
        Type::Fn { .. } | Type::Rigid(_) | Type::Flexible => true,
        // 空のレコードは `Unit` で、値は `()` である。要素のあるレコード (タプル) はヒープのオブジェクトにする
        Type::Record(fields) => !fields.is_empty(),
        Type::Error => false,
    }
}

/// 引数を持つコンストラクタが1つでもある `data` の値は、ヒープの箱かもしれない。同じ型の引数のないコンストラクタの
/// 値は即値のタグで同じ変数に入るが、`dup` と `decref` はそれを無視する (docs/spec/core-ir.md の boxed の判定)。
fn has_fields(hir: &HirProgram, id: TypeDefId) -> bool {
    match &hir[id].kind {
        TypeDefKind::Data { constructors } => constructors
            .iter()
            .any(|&ctor| !hir[ctor].fields.is_empty()),
        TypeDefKind::Extern(_) => false,
    }
}

pub(super) fn var_info(name: &str, ty: &Type, hir: &HirProgram) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        boxed: boxed(ty, hir),
    }
}

/// 関数型の先頭の `count` 個の引数の型と、残りの型。
pub(super) fn split_arrows(ty: &Type, count: usize) -> (Vec<Type>, Type) {
    let mut params = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let Type::Fn { param, ret, .. } = ty else {
            unreachable!("the type checker matched parameters with arrows");
        };
        params.push((**param).clone());
        ty = ret;
    }
    (params, ty.clone())
}

/// `==` と `!=` の比べ方と否定の有無から、比べ方ごとの extern の行を選ぶ。比べ方は、型検査が参照ごとに記録した
/// 型引数から `eml_types::equality` が決める (docs/spec/declarations.md の標準の演算子の表)。
pub(super) fn equality_extern(equality: Equality, negated: bool) -> Extern {
    match (equality, negated) {
        (Equality::Int, false) => Extern::IntEq,
        (Equality::Int, true) => Extern::IntNe,
        (Equality::String, false) => Extern::StrEq,
        (Equality::String, true) => Extern::StrNe,
        (Equality::Bool, false) => Extern::BoolEq,
        (Equality::Bool, true) => Extern::BoolNe,
    }
}

#[cfg(test)]
mod tests {
    use eml_diagnostics::SourceFiles;
    use eml_syntax::ast;

    use crate::IoOp;

    /// Prelude の `effect IO` の操作の名前。
    fn prelude_io_operations() -> Vec<String> {
        let mut files = SourceFiles::new();
        let file = files.add(eml_hir::PRELUDE_PATH, eml_hir::PRELUDE_SOURCE);
        let (parse, errors) = eml_syntax::parse(file, eml_hir::PRELUDE_SOURCE);
        assert!(errors.is_empty(), "{errors:?}");
        parse
            .tree()
            .items()
            .filter_map(|item| match item {
                ast::Item::EffectItem(effect) if effect.name()?.text() == "IO" => Some(effect),
                _ => None,
            })
            .flat_map(|effect| effect.operations())
            .filter_map(|op| Some(op.name()?.text()))
            .collect()
    }

    #[test]
    fn every_io_operation_has_an_io_op_and_back() {
        let names = prelude_io_operations();
        // 壊れた Prelude で、確かめる名前が気づかないうちに減らないようにする
        assert_eq!(names.len(), IoOp::VARIANTS.len(), "{names:?}");
        for name in &names {
            assert!(IoOp::from_name(name).is_some(), "{name}");
        }
        for op in IoOp::VARIANTS {
            assert!(names.iter().any(|name| name == op.name()), "{}", op.name());
        }
    }
}

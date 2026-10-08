//! 型から決まる値の表現 (Repr) と、`==` と `!=` を比べ方ごとのどの extern にするか。

use eml_extern::{Extern, ExternType};
use eml_hir::{Program as HirProgram, TypeDefKind};
use eml_types::{Equality, Type};

use crate::{Repr, VarInfo};

/// 型の Repr (docs/spec/core-ir.md)。総称的な位置の束縛も、S3b-2a では具体化した型から決める。
/// 関数の値と型変数の値は、即値 (捕まえた変数のない関数、引数のないコンストラクタ) にもヒープの物体にもなるので
/// `tobj` にする。
pub(super) fn repr(ty: &Type, hir: &HirProgram) -> Repr {
    match ty {
        Type::Con { id, args: _ } => match &hir[*id].kind {
            TypeDefKind::Extern(row) => match row {
                Some(ExternType::Int) => Repr::Int,
                Some(ExternType::String | ExternType::File) => Repr::Obj,
                Some(ExternType::Unit) | None => Repr::Unit,
            },
            TypeDefKind::Data { constructors } => {
                let with_fields = constructors
                    .iter()
                    .filter(|&&ctor| !hir[ctor].fields.is_empty())
                    .count();
                if with_fields == 0 {
                    Repr::Enum
                } else if with_fields == constructors.len() {
                    Repr::Obj
                } else {
                    Repr::TObj
                }
            }
        },
        Type::Fn { .. } | Type::Rigid(_) | Type::Flexible => Repr::TObj,
        // 空のレコードは `Unit` で、値は `()` である。要素のあるレコード (タプル) はヒープの物体にする
        Type::Record(fields) if fields.is_empty() => Repr::Unit,
        Type::Record(_) => Repr::Obj,
        Type::Error => Repr::Unit,
    }
}

pub(super) fn var_info(name: &str, ty: &Type, hir: &HirProgram) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        repr: repr(ty, hir),
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

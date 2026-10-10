//! 型から決まる値の表現 (Repr)。

use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{TypeId, TypeKind, TypeStore};

use crate::{Repr, VarInfo, data_repr};

/// 型の Repr (docs/spec/core-ir.md の「値の表現」)。総称的な位置の束縛も具体化した型から決める。値を受ける位置の
/// Repr と違えば、box の挿入が変換を入れる (docs/spec/core-ir.md の「位置の規則」)。関数の値と型変数の値は、即値
/// (捕まえた変数のない関数、引数のないコンストラクタ) にもヒープの物体にもなるので `tobj` にする。
pub fn repr(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Repr {
    match types.kind(ty) {
        TypeKind::Con { id, args: _ } => type_def_repr(*id, hir),
        TypeKind::Fn { .. } | TypeKind::Rigid(_) | TypeKind::OpVar(_) | TypeKind::Flexible => {
            Repr::TObj
        }
        // 要素のないタプルは `Unit` で、値は `()` である。要素のあるタプルはヒープの物体にする
        TypeKind::Tuple(elements) if elements.is_empty() => Repr::Unit,
        TypeKind::Tuple(_) => Repr::Obj,
        TypeKind::Error => Repr::Unit,
    }
}

/// 型構成子 `id` の値の Repr。型引数によらない。
pub(super) fn type_def_repr(id: TypeDefId, hir: &HirProgram) -> Repr {
    match &hir[id].kind {
        TypeDefKind::Extern(row) => {
            row.expect(
                "an extern type outside the standard library is E1033, and Core IR receives only programs without errors",
            )
            .row()
            .repr
        }
        TypeDefKind::Data { constructors } => {
            data_repr(constructors.iter().map(|&ctor| hir[ctor].fields.len()))
        }
    }
}

pub(super) fn var_info(name: &str, types: &TypeStore, ty: TypeId, hir: &HirProgram) -> VarInfo {
    named(name, repr(types, ty, hir))
}

pub(super) fn named(name: &str, repr: Repr) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        repr,
    }
}

/// 関数型の先頭の `count` 個の引数の型と、残りの型。
pub(super) fn split_arrows(types: &TypeStore, ty: TypeId, count: usize) -> (Vec<TypeId>, TypeId) {
    let mut params = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
            unreachable!("the type checker matched parameters with arrows");
        };
        params.push(*param);
        ty = *ret;
    }
    (params, ty)
}

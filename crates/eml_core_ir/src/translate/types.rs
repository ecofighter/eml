//! 型から決まる値の表現 (Repr) と Kind の印。

use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{Lin, TypeId, TypeKind, TypeStore};

use eml_lir::{Mark, Repr, VarInfo, data_repr};

/// 型の Repr (docs/spec/core-ir.md の「型の Repr」)。総称的な位置の束縛も具体化した型から決める。値を受ける位置の
/// Repr と違えば、box の挿入が変換を入れる (docs/spec/lir.md の「位置の規則」)。関数の値と型変数の値は、即値
/// (捕まえた変数のない関数、引数のないコンストラクタ) にもヒープの物体にもなるので `tobj` にする。
pub fn repr(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Repr {
    match types.kind(ty) {
        TypeKind::Con { id, args: _ } => type_def_repr(*id, hir),
        TypeKind::Fn { .. }
        | TypeKind::Rigid(_)
        | TypeKind::OpVar(_, _)
        | TypeKind::Flexible(_) => Repr::TObj,
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

/// instance の代入をかけた型の印 (docs/spec/core-ir.md の
/// 「translate が決める印」)。宣言に対して相対的な型は現れないので、`TypeStore::linearity` で読む。
pub(super) fn mark(types: &TypeStore, ty: TypeId) -> Mark {
    mark_of(types.linearity(ty))
}

/// 宣言の型を代入せずに読む所 (`op$`、`con$`、操作の表) の印。`Rigid` と `Var` に依存する型は `poly` になる。
/// 保守的なので、`(a, File)` も `lin` でなく `poly` になる。
pub(super) fn declared_mark(types: &TypeStore, ty: TypeId) -> Mark {
    mark_of(types.linearity_or_poly(ty))
}

/// 型の線形性が決める印。決まらない型 (`Poly`) と Kind 変数は、呼び出し側が決めるので `poly` にする。
fn mark_of(lin: Lin) -> Mark {
    match lin {
        Lin::Unr => Mark::Unr,
        Lin::Lin => Mark::Lin,
        Lin::Poly | Lin::Var(_) => Mark::Poly,
    }
}

pub(super) fn var_info(name: &str, types: &TypeStore, ty: TypeId, hir: &HirProgram) -> VarInfo {
    named(name, repr(types, ty, hir), mark(types, ty))
}

/// 宣言の型で決める `var_info`。
pub(super) fn declared_var_info(
    name: &str,
    types: &TypeStore,
    ty: TypeId,
    hir: &HirProgram,
) -> VarInfo {
    named(name, repr(types, ty, hir), declared_mark(types, ty))
}

pub(super) fn named(name: &str, repr: Repr, mark: Mark) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        repr,
        mark,
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

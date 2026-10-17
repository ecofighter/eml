//! 型から決まる値の表現 (Repr) と Kind の印。

use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{Lin, TypeId, TypeKind, TypeStore};

use eml_lir::{Mark, Repr, VarInfo, data_repr};

/// 型の Repr (docs/spec/core-ir.md の「型の Repr」)。総称的な位置の束縛も具体化した型から決める。値を受ける位置の
/// Repr と違えば、box の挿入が変換を入れる (docs/spec/lir.md の「位置の規則」)。関数の値と型変数の値は、即値
/// (捕まえた変数のない関数、引数のないコンストラクタ) にもヒープの物体にもなるので `tobj` にする。
pub fn repr(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Repr {
    match types.kind(ty) {
        // row は値を持たないので、表し方は型構成子だけで決まる
        TypeKind::Con {
            id,
            args: _,
            rows: _,
        } => type_def_repr(*id, hir),
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

/// instance の代入をかけた型の印 (docs/spec/core-ir.md の「translate が決める印」)。宣言に対して相対的な型は
/// 現れないので、`TypeStore::linearity` で読む。
pub(super) fn mark(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Mark {
    scalar_or(types, ty, hir, || types.linearity(ty))
}

/// 宣言の型を代入せずに読む所 (`op$`、`con$`、操作の表) の印。`Rigid` と `Var` に依存する型は `poly` になる。
/// 保守的なので、`(a, File)` も `lin` でなく `poly` になる。
pub(super) fn declared_mark(types: &TypeStore, ty: TypeId, hir: &HirProgram) -> Mark {
    scalar_or(types, ty, hir, || types.linearity_or_poly(ty))
}

/// Repr がスカラー (RC の対象でない) の値は、型の Kind によらず `unr` である。印は参照の RC を守るためのもので、
/// スカラーとその箱は資源を持たないためである。`Lin` と宣言したフィールドのない型 (`enum`) の値が1回だけ使われる
/// ことは、型検査が守る (docs/spec/core-ir.md の「translate が決める印」)。
fn scalar_or(types: &TypeStore, ty: TypeId, hir: &HirProgram, lin: impl FnOnce() -> Lin) -> Mark {
    if repr(types, ty, hir).is_rc() {
        mark_of(lin())
    } else {
        Mark::Unr
    }
}

/// 型の線形性が決める印。決まらない型 (`Poly`) は、呼び出し側が決めるので `poly` にする。`TypeStore` は `Var` に
/// 依存する型の値を `Poly` として持つので、2つの読み方のどちらも `Var` を返さない (docs/spec/types.md の
/// 「型の線形性」)。
fn mark_of(lin: Lin) -> Mark {
    match lin {
        Lin::Unr => Mark::Unr,
        Lin::Lin => Mark::Lin,
        Lin::Poly => Mark::Poly,
        Lin::Var(_) => unreachable!("the store gives a Kind variable's type the value `Poly`"),
    }
}

pub(super) fn var_info(name: &str, types: &TypeStore, ty: TypeId, hir: &HirProgram) -> VarInfo {
    named(name, repr(types, ty, hir), mark(types, ty, hir))
}

/// 宣言の型で決める `var_info`。
pub(super) fn declared_var_info(
    name: &str,
    types: &TypeStore,
    ty: TypeId,
    hir: &HirProgram,
) -> VarInfo {
    named(name, repr(types, ty, hir), declared_mark(types, ty, hir))
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

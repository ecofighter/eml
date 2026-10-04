//! 型から決まる変数の性質 (boxed かどうか) と、組み込みを Core IR のどの命令にするか。

use eml_hir::LangItems;
use eml_hir::builtin::Builtin;
use eml_types::{Linearity, Type};

use crate::{FALSE, IoOp, PrimOp, TRUE, VarInfo};

/// ヒープに置く値の型。`Unr` でボックス化した変数が RC の対象になる。関数値と型変数の値は、ヒープのクロージャや
/// 文字列かもしれない。インタプリタの `dup` / `decref` はヒープにない値を無視するので、多めに対象にしても正しく動く
/// (docs/spec/core-ir.md)。
fn boxed(ty: &Type, lang: &LangItems) -> bool {
    match ty {
        Type::Con { id, .. } => *id == lang.string,
        Type::Fn { .. } | Type::Cont { .. } | Type::Rigid(_) | Type::Flexible => true,
        Type::Record(_) | Type::Error => false,
    }
}

pub(super) fn var_info(name: &str, ty: &Type, lang: &LangItems) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed: boxed(ty, lang),
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

/// 組み込みを Core IR のどの命令にするか。引数の数は `Builtin::arity` (eml_hir の表) から、引数と結果の型は Prelude の
/// スキームから引くので、ここには変換の種類だけを置く。
pub(super) enum Lowering {
    Prim(PrimOp),
    Io(IoOp),
    /// `>>` は `g (f x)`、`<<` は `f (g x)` である (docs/spec/declarations.md の演算子の表)。
    Compose {
        forward: bool,
    },
    Constructor(u32),
}

pub(super) fn lowering(builtin: Builtin) -> Lowering {
    match builtin {
        Builtin::Println => Lowering::Io(IoOp::Println),
        Builtin::ShowInt => Lowering::Prim(PrimOp::ShowInt),
        Builtin::Not => Lowering::Prim(PrimOp::Not),
        Builtin::IntNeg => Lowering::Prim(PrimOp::IntNeg),
        Builtin::IntAdd => Lowering::Prim(PrimOp::IntAdd),
        Builtin::IntSub => Lowering::Prim(PrimOp::IntSub),
        Builtin::IntMul => Lowering::Prim(PrimOp::IntMul),
        Builtin::IntDiv => Lowering::Prim(PrimOp::IntDiv),
        Builtin::IntMod => Lowering::Prim(PrimOp::IntMod),
        Builtin::IntEq => Lowering::Prim(PrimOp::IntEq),
        Builtin::IntNe => Lowering::Prim(PrimOp::IntNe),
        Builtin::IntLt => Lowering::Prim(PrimOp::IntLt),
        Builtin::IntLe => Lowering::Prim(PrimOp::IntLe),
        Builtin::IntGt => Lowering::Prim(PrimOp::IntGt),
        Builtin::IntGe => Lowering::Prim(PrimOp::IntGe),
        Builtin::StrConcat => Lowering::Prim(PrimOp::StrConcat),
        Builtin::ComposeFwd => Lowering::Compose { forward: true },
        Builtin::ComposeBwd => Lowering::Compose { forward: false },
        Builtin::True => Lowering::Constructor(TRUE),
        Builtin::False => Lowering::Constructor(FALSE),
    }
}

//! 型から決まる変数の性質 (boxed かどうか) と、組み込みを Core IR のどの命令にするか。

use eml_hir::builtin::Builtin;
use eml_hir::{Module, TypeDefId, TypeDefKind};
use eml_types::{Equality, Type};

use crate::{IoOp, PrimOp, VarInfo};

/// ヒープに置く値の型。ボックス化した変数が RC の対象になる。関数値と型変数の値は、ヒープのクロージャや
/// 文字列かもしれない。インタプリタの `dup` / `decref` はヒープにない値を無視するので、多めに対象にしても正しく動く
/// (docs/spec/core-ir.md)。`File` はヒープのオブジェクトである。
fn boxed(ty: &Type, module: &Module) -> bool {
    match ty {
        Type::Con { id, .. } => {
            *id == module.lang.string || *id == module.lang.file || has_fields(module, *id)
        }
        Type::Fn { .. } | Type::Cont { .. } | Type::Rigid(_) | Type::Flexible => true,
        // 空のレコードは `Unit` で、値は `()` である。要素のあるレコード (タプル) はヒープのオブジェクトにする
        Type::Record(fields) => !fields.is_empty(),
        Type::Error => false,
    }
}

/// 引数を持つコンストラクタが1つでもある `data` の値は、ヒープの箱かもしれない。同じ型の引数のないコンストラクタの
/// 値は即値のタグで同じ変数に入るが、`dup` と `decref` はそれを無視する (docs/spec/core-ir.md の boxed の判定)。
fn has_fields(module: &Module, id: TypeDefId) -> bool {
    match &module.types[id].kind {
        TypeDefKind::Data { constructors } => constructors
            .iter()
            .any(|&ctor| !module.constructors[ctor].fields.is_empty()),
        TypeDefKind::Builtin => false,
    }
}

pub(super) fn var_info(name: &str, ty: &Type, module: &Module) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        boxed: boxed(ty, module),
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
    /// `==` と `!=`。比べ方は型検査が引数の型から決め、`BodyTypes::equalities` に入れてある
    /// (docs/spec/declarations.md の標準の演算子の表)。
    Equality {
        negated: bool,
    },
}

pub(super) fn lowering(builtin: Builtin) -> Lowering {
    match builtin {
        Builtin::Println => Lowering::Io(IoOp::Println),
        Builtin::Open => Lowering::Io(IoOp::Open),
        Builtin::ReadAll => Lowering::Io(IoOp::ReadAll),
        Builtin::Close => Lowering::Io(IoOp::Close),
        Builtin::ShowInt => Lowering::Prim(PrimOp::ShowInt),
        Builtin::Not => Lowering::Prim(PrimOp::Not),
        Builtin::IntNeg => Lowering::Prim(PrimOp::IntNeg),
        Builtin::IntAdd => Lowering::Prim(PrimOp::IntAdd),
        Builtin::IntSub => Lowering::Prim(PrimOp::IntSub),
        Builtin::IntMul => Lowering::Prim(PrimOp::IntMul),
        Builtin::IntDiv => Lowering::Prim(PrimOp::IntDiv),
        Builtin::IntMod => Lowering::Prim(PrimOp::IntMod),
        Builtin::IntEq => Lowering::Equality { negated: false },
        Builtin::IntNe => Lowering::Equality { negated: true },
        Builtin::IntLt => Lowering::Prim(PrimOp::IntLt),
        Builtin::IntLe => Lowering::Prim(PrimOp::IntLe),
        Builtin::IntGt => Lowering::Prim(PrimOp::IntGt),
        Builtin::IntGe => Lowering::Prim(PrimOp::IntGe),
        Builtin::StrConcat => Lowering::Prim(PrimOp::StrConcat),
        Builtin::ComposeFwd => Lowering::Compose { forward: true },
        Builtin::ComposeBwd => Lowering::Compose { forward: false },
    }
}

/// 型検査が決めた比べ方の命令。
pub(super) fn equality_op(equality: Equality, negated: bool) -> PrimOp {
    match (equality, negated) {
        (Equality::Int, false) => PrimOp::IntEq,
        (Equality::Int, true) => PrimOp::IntNe,
        (Equality::String, false) => PrimOp::StrEq,
        (Equality::String, true) => PrimOp::StrNe,
        (Equality::Bool, false) => PrimOp::BoolEq,
        (Equality::Bool, true) => PrimOp::BoolNe,
    }
}

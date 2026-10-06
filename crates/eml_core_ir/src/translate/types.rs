//! 型から決まる変数の性質 (boxed かどうか) と、intrinsic を Core IR のどの命令にするか。

use eml_hir::{Program as HirProgram, TypeDefId, TypeDefKind};
use eml_types::{Equality, Type};

use crate::{IoOp, PrimOp, VarInfo};

/// ヒープに置く値の型。ボックス化した変数が RC の対象になる。関数値と型変数の値は、ヒープのクロージャや
/// 文字列かもしれない。インタプリタの `dup` / `decref` はヒープにない値を無視するので、多めに対象にしても正しく動く
/// (docs/spec/core-ir.md)。`File` はヒープのオブジェクトである。
fn boxed(ty: &Type, hir: &HirProgram) -> bool {
    match ty {
        Type::Con { id, .. } => {
            *id == hir.lang.string || *id == hir.lang.file || has_fields(hir, *id)
        }
        Type::Fn { .. } | Type::Cont { .. } | Type::Rigid(_) | Type::Flexible => true,
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
        TypeDefKind::Builtin => false,
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

/// intrinsic を Core IR のどの命令にするか。引数の数は intrinsic のシグネチャ (`hir::Program::arity`) から、引数と結果の
/// 型は Prelude のスキームから引くので、ここには変換の種類だけを置く。
#[derive(Debug, Clone, Copy)]
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

/// Prelude の intrinsic の名前と、Core IR の命令。名前と実装の対応はここだけに置き、網羅のテストが行ごとに Prelude と
/// 照らし合わせる (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.2)。HIR が脱糖する `&&`、`||`、`|>`、
/// `<|` は持たない。
const INTRINSICS: &[(&str, Lowering)] = &[
    ("println", Lowering::Io(IoOp::Println)),
    ("open", Lowering::Io(IoOp::Open)),
    ("read_all", Lowering::Io(IoOp::ReadAll)),
    ("close", Lowering::Io(IoOp::Close)),
    ("show_int", Lowering::Prim(PrimOp::ShowInt)),
    ("not", Lowering::Prim(PrimOp::Not)),
    ("negate", Lowering::Prim(PrimOp::IntNeg)),
    ("+", Lowering::Prim(PrimOp::IntAdd)),
    ("-", Lowering::Prim(PrimOp::IntSub)),
    ("*", Lowering::Prim(PrimOp::IntMul)),
    ("/", Lowering::Prim(PrimOp::IntDiv)),
    ("%", Lowering::Prim(PrimOp::IntMod)),
    ("==", Lowering::Equality { negated: false }),
    ("!=", Lowering::Equality { negated: true }),
    ("<", Lowering::Prim(PrimOp::IntLt)),
    ("<=", Lowering::Prim(PrimOp::IntLe)),
    (">", Lowering::Prim(PrimOp::IntGt)),
    (">=", Lowering::Prim(PrimOp::IntGe)),
    ("++", Lowering::Prim(PrimOp::StrConcat)),
    (">>", Lowering::Compose { forward: true }),
    ("<<", Lowering::Compose { forward: false }),
];

/// Prelude の intrinsic の名前から、Core IR の命令を引く。
pub(super) fn intrinsic(name: &str) -> Option<Lowering> {
    INTRINSICS
        .iter()
        .find(|(intrinsic, _)| *intrinsic == name)
        .map(|&(_, lowering)| lowering)
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

#[cfg(test)]
mod tests {
    use eml_diagnostics::SourceFiles;
    use eml_syntax::ast;

    use super::{INTRINSICS, intrinsic};

    /// HIR が脱糖するので、Core IR に届かない intrinsic。
    const DESUGARED: &[&str] = &["&&", "||", "|>", "<|"];

    /// Prelude の等式のないシグネチャ (intrinsic) の名前。
    fn prelude_intrinsics() -> Vec<String> {
        let mut files = SourceFiles::new();
        let file = files.add(eml_hir::PRELUDE_PATH, eml_hir::PRELUDE_SOURCE);
        let (parse, errors) = eml_syntax::parse(file, eml_hir::PRELUDE_SOURCE);
        // 壊れた Prelude で、確かめる名前が気づかないうちに減らないようにする
        assert!(errors.is_empty(), "{errors:?}");
        let items: Vec<ast::Item> = parse.tree().items().collect();
        let defined: Vec<String> = items
            .iter()
            .filter_map(|item| match item {
                ast::Item::Equation(equation) => Some(equation.name()?.text()),
                _ => None,
            })
            .collect();
        items
            .iter()
            .filter_map(|item| match item {
                ast::Item::Signature(signature) => Some(signature.name()?.text()),
                _ => None,
            })
            .filter(|name| !defined.contains(name))
            .collect()
    }

    #[test]
    fn every_prelude_intrinsic_has_an_implementation() {
        for name in prelude_intrinsics() {
            assert_eq!(
                intrinsic(&name).is_some(),
                !DESUGARED.contains(&name.as_str()),
                "{name}"
            );
        }
    }

    #[test]
    fn every_implementation_names_a_prelude_intrinsic() {
        let names = prelude_intrinsics();
        for (name, _) in INTRINSICS {
            assert!(names.iter().any(|n| n == name), "{name}");
            assert!(intrinsic(name).is_some(), "{name}");
        }
    }
}

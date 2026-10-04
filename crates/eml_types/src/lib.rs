//! 型、row、Kind の検査 (docs/spec/types.md)。

mod check;
mod kind;
mod scc;
mod scheme;
mod table;
mod ty;
mod usage;

use std::fmt::Write;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{ExprId, FunctionId, LocalId, Module};
use la_arena::ArenaMap;

pub use ty::{Effect, KindConstraint, KindTerm, Linearity, Multiplicity, RowTail, Type};

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const TYPE_MISMATCH: ErrorCode = ErrorCode(2001);
    pub const EFFECT_NOT_IN_ROW: ErrorCode = ErrorCode(2002);
    pub const MISSING_MAIN: ErrorCode = ErrorCode(2003);
    pub const INVALID_MAIN_TYPE: ErrorCode = ErrorCode(2004);
    pub const INFINITE_TYPE: ErrorCode = ErrorCode(2005);
}

/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
#[derive(Debug, Default)]
pub struct TypedModule {
    /// シグネチャのある関数だけを含む。
    pub signatures: ArenaMap<FunctionId, Type>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ArenaMap<FunctionId, BodyTypes>,
    pub main: Option<FunctionId>,
    /// スキームに残った Kind の制約のうち、定数を片側に持つもの。テストの表示で使う。
    pub kinds: ArenaMap<FunctionId, Vec<KindConstraint>>,
}

#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, Type>,
    pub locals: ArenaMap<LocalId, Type>,
}

pub fn check(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    check::check_module(module)
}

/// `main` がないこと。`eml check` では検査せず、`eml run` だけが報告する (docs/spec/types.md の「推論」)。
pub fn missing_main(file: FileId) -> Diagnostic {
    Diagnostic::error(
        codes::MISSING_MAIN,
        "`main` is not defined",
        Label::new(
            file,
            TextRange::empty(0.into()),
            "`eml run` starts the program from `main`",
        ),
    )
    .with_help("add `main : Unit -> <IO> Unit` and an equation `main () = ...`")
}

/// テストで推論結果を確かめるための表示。
pub fn dump(module: &Module, typed: &TypedModule) -> String {
    let mut out = String::new();
    for (id, function) in module.functions.iter() {
        if let Some(signature) = typed.signatures.get(id) {
            writeln!(out, "{} : {signature}", function.name).unwrap();
            if let Some(kinds) = typed.kinds.get(id).filter(|kinds| !kinds.is_empty()) {
                let kinds: Vec<String> = kinds.iter().map(ToString::to_string).collect();
                writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
            }
        }
        let (Some(body), Some(types)) = (&function.body, typed.bodies.get(id)) else {
            continue;
        };
        for (local, data) in body.locals.iter() {
            if let Some(ty) = types.locals.get(local) {
                writeln!(
                    out,
                    "  {}#{} : {ty}",
                    data.name,
                    u32::from(local.into_raw())
                )
                .unwrap();
            }
        }
    }
    out
}

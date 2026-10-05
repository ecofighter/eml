//! 型、row、Kind の検査 (docs/spec/types.md)。

mod check;
mod data;
mod exhaustive;
mod kind;
mod scc;
mod scheme;
mod table;
mod ty;
mod usage;

use std::collections::HashMap;
use std::fmt::Write;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::builtin::Builtin;
use eml_hir::{ConstructorId, ExprId, FunctionId, LocalId, Module, OperationId, PatId};
use la_arena::ArenaMap;

pub use ty::{EffectLabel, KindConstraint, KindTerm, Linearity, Multiplicity, RowTail, Type};

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const LINEAR_VALUE_MISUSED: ErrorCode = ErrorCode(3001);
    pub const TYPE_MISMATCH: ErrorCode = ErrorCode(2001);
    pub const EFFECT_NOT_IN_ROW: ErrorCode = ErrorCode(2002);
    pub const MISSING_MAIN: ErrorCode = ErrorCode(2003);
    pub const INVALID_MAIN_TYPE: ErrorCode = ErrorCode(2004);
    pub const INFINITE_TYPE: ErrorCode = ErrorCode(2005);
    pub const NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(4001);
    pub const NON_EXHAUSTIVE_EQUATION: ErrorCode = ErrorCode(4002);
    pub const REFUTABLE_PATTERN: ErrorCode = ErrorCode(4003);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「網羅性の診断」)。
    pub const UNREACHABLE_ARM: ErrorCode = ErrorCode(4004);
}

/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
#[derive(Debug, Default)]
pub struct TypedModule {
    /// シグネチャのある関数だけを含む。
    pub signatures: ArenaMap<FunctionId, Scheme>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ArenaMap<FunctionId, BodyTypes>,
    pub main: Option<FunctionId>,
    /// Prelude のシグネチャから作った組み込みのスキーム。Core IR が、組み込みを包む関数の変数を boxed にするかを
    /// 決めるのに使う。
    pub builtins: HashMap<Builtin, Scheme>,
    /// エフェクトの操作のスキーム。Core IR が、操作を包む関数の変数を boxed にするかを決めるのに使う。
    pub operations: ArenaMap<OperationId, Scheme>,
    /// コンストラクタのスキーム。`Some : a -> Option a` の形である。Core IR が、コンストラクタを包む関数の変数を
    /// boxed にするかを決めるのに使う。
    pub constructors: ArenaMap<ConstructorId, Scheme>,
}

/// 関数の型と、多相化したときに残った Kind の制約のうち、定数を片側に持つもの。変数どうしの制約は部分適用のたびに
/// 増えて読みにくくなるので出さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub ty: Type,
    pub constraints: Vec<KindConstraint>,
}

#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, Type>,
    pub locals: ArenaMap<LocalId, Type>,
    /// パターンが受けた値の型。Core IR が、handler の節の引数の変数を作るのに使う。
    pub pats: ArenaMap<PatId, Type>,
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
    for (id, operation) in module.operations.iter() {
        if let Some(scheme) = typed.operations.get(id) {
            writeln!(out, "{} : {}", operation.name, scheme.ty).unwrap();
            write_kinds(&mut out, scheme);
        }
    }
    for (id, function) in module.functions.iter() {
        if let Some(scheme) = typed.signatures.get(id) {
            writeln!(out, "{} : {}", function.name, scheme.ty).unwrap();
            write_kinds(&mut out, scheme);
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

fn write_kinds(out: &mut String, scheme: &Scheme) {
    if !scheme.constraints.is_empty() {
        let kinds: Vec<String> = scheme.constraints.iter().map(ToString::to_string).collect();
        writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
    }
}

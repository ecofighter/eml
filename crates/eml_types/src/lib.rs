//! 型、row、Kind の検査 (docs/spec/types.md)。

mod carry;
mod check;
mod context;
mod data;
mod dump;
mod exhaustive;
mod kind;
mod scc;
mod shape;
mod table;
mod ty;
mod usage;

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{
    ConstructorId, ExprId, Function, FunctionId, ItemMap, LocalId, OperationId, PatId, Program,
};
use la_arena::ArenaMap;

use crate::kind::problem::KindScheme;
use crate::shape::Shape;

pub use dump::dump;
pub use ty::{ContState, EffectLabel, Linearity, Multiplicity, RowTail, Type};

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const LINEAR_VALUE_MISUSED: ErrorCode = ErrorCode(3001);
    pub const LINEAR_VALUE_USED_TWICE: ErrorCode = ErrorCode(3002);
    pub const LINEAR_VALUE_NOT_CONSUMED: ErrorCode = ErrorCode(3003);
    pub const LINEAR_VALUE_DISCARDED: ErrorCode = ErrorCode(3004);
    pub const CONTINUATION_NOT_HANDLED: ErrorCode = ErrorCode(3005);
    pub const LINEAR_VALUE_KEPT_ACROSS_MULTI: ErrorCode = ErrorCode(3006);
    pub const TYPE_MISMATCH: ErrorCode = ErrorCode(2001);
    pub const EFFECT_NOT_IN_ROW: ErrorCode = ErrorCode(2002);
    pub const MISSING_MAIN: ErrorCode = ErrorCode(2003);
    pub const INVALID_MAIN_TYPE: ErrorCode = ErrorCode(2004);
    pub const INFINITE_TYPE: ErrorCode = ErrorCode(2005);
    pub const NOT_COMPARABLE: ErrorCode = ErrorCode(2006);
    pub const RESUME_STATE_MISMATCH: ErrorCode = ErrorCode(2007);
    pub const NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(4001);
    pub const NON_EXHAUSTIVE_EQUATION: ErrorCode = ErrorCode(4002);
    pub const REFUTABLE_PATTERN: ErrorCode = ErrorCode(4003);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「網羅性の診断」)。
    pub const UNREACHABLE_ARM: ErrorCode = ErrorCode(4004);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「網羅性の診断」)。
    pub const UNREACHABLE_EQUATION: ErrorCode = ErrorCode(4005);
}

/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
/// 宣言の結果を宣言ごとに持つのは、クエリ化と REPL で、宣言ごとに結果を使い回せるようにするため
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug, Default)]
pub struct TypedProgram {
    /// シグネチャのある関数、操作、コンストラクタの型。
    pub decls: HashMap<Decl, DeclType>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ItemMap<Function, BodyTypes>,
}

/// スキームを持つ宣言。具体化の記録が、どの宣言のスキームを使うかも指す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Decl {
    Function(FunctionId),
    Operation(OperationId),
    Constructor(ConstructorId),
}

/// 1つの宣言の型検査の結果。
#[derive(Debug)]
pub struct DeclType {
    /// 後の段階が読む、矢印の線形性のない型。検査の最後に1回だけ書き出しておく。
    pub ty: Type,
    /// 後の段階は Kind を読まない (docs/implementation/architecture.md の「`eml_types` の内部」) ので、外からは読めなくする。
    pub(crate) shape: Shape,
    pub(crate) kinds: KindScheme,
}

/// `==` と `!=` の比べ方。型クラスがないので、型検査が引数の型から決め、比べられる型を限る
/// (docs/spec/declarations.md の標準の演算子の表)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Equality {
    Int,
    String,
    Bool,
}

#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, Type>,
    pub locals: ArenaMap<LocalId, Type>,
    /// パターンが受けた値の型。Core IR が、handler の節の引数の変数を作るのに使う。
    pub pats: ArenaMap<PatId, Type>,
    /// `==` と `!=` の比べ方。キーは演算子を指す呼ばれる側の式である。Core IR が、どの比べる命令にするかを決めるのに
    /// 使う。
    pub equalities: ArenaMap<ExprId, Equality>,
}

pub fn check(program: &Program) -> (TypedProgram, Vec<Diagnostic>) {
    check::check_module(program)
}

/// 入口のモジュールに `main` がないこと。`eml check` では検査せず、`eml run` だけが報告する (docs/spec/types.md の
/// 「推論」)。import した `main` は実行を始める関数にならないので、メッセージは入口のモジュールの定義を求める。
pub fn missing_main(file: FileId) -> Diagnostic {
    Diagnostic::error(
        codes::MISSING_MAIN,
        "the entry module does not define `main`",
        Label::new(
            file,
            TextRange::empty(0.into()),
            "`eml run` starts the program from `main`",
        ),
    )
    .with_help("add `main : Unit -> <IO> Unit` and an equation `main () = ...`")
}

/// 単体テストのための `Program`。Prelude と、`text` を入口にしたモジュールを変換する。
#[cfg(test)]
pub(crate) fn test_program(text: &str) -> eml_hir::Program {
    let (loaded, _) = eml_hir::load("test.em", text, &NoModules);
    let (def_map, _) = eml_hir::def_map(&loaded.modules);
    eml_hir::lower(&def_map, &loaded.modules).0
}

/// 単体テストの入口は import を書かないので、依存先を読む手段は要らない。
#[cfg(test)]
struct NoModules;

#[cfg(test)]
impl eml_hir::ModuleSource for NoModules {
    fn read(&self, _: &eml_hir::ModulePath) -> Result<String, eml_hir::ReadError> {
        Err(eml_hir::ReadError::NotFound)
    }
}

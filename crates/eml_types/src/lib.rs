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
mod store;
mod table;
mod ty;
mod usage;

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles, TextRange};
use eml_extern::ExternType;
use eml_hir::{EffectId, ExprId, Function, ItemMap, LocalId, PatId, Program, ValueItem};
use la_arena::ArenaMap;

use crate::kind::problem::KindScheme;
use crate::shape::Shape;

pub use dump::dump;
pub use store::{EffectLabel, RowTail, TypeId, TypeKind, TypeStore};
pub use ty::{Linearity, Multiplicity};

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
    pub const MASK_CONFLICT: ErrorCode = ErrorCode(2008);
    pub const NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(4001);
    pub const NON_EXHAUSTIVE_EQUATION: ErrorCode = ErrorCode(4002);
    pub const REFUTABLE_PATTERN: ErrorCode = ErrorCode(4003);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「割り当て済みの番号」)。
    pub const UNREACHABLE_ARM: ErrorCode = ErrorCode(4004);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「割り当て済みの番号」)。
    pub const UNREACHABLE_EQUATION: ErrorCode = ErrorCode(4005);
}

/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
/// 宣言の結果を宣言ごとに持つのは、クエリ化と REPL で、宣言ごとに結果を使い回せるようにするため
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug)]
pub struct TypedProgram {
    /// 宣言と本体の型がすべて指す、プログラムに1つの型の表。
    pub types: TypeStore,
    /// シグネチャのある関数、操作、コンストラクタの型。
    pub decls: HashMap<ValueItem, DeclType>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ItemMap<Function, BodyTypes>,
}

/// 1つの宣言の型検査の結果。
#[derive(Debug)]
pub struct DeclType {
    /// 後の段階が読む、矢印の線形性のない型。検査の最後に1回だけ書き出しておく。
    pub ty: TypeId,
    /// 後の段階は Kind を読まない (docs/implementation/architecture.md の「`eml_types` の内部」) ので、外からは読めなくする。
    pub(crate) shape: Shape,
    pub(crate) kinds: KindScheme,
}

/// `==` と `!=` の比べ方。型クラスがないので、比べられる型を限る (docs/spec/declarations.md の標準の演算子の表)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Equality {
    Int,
    String,
    Bool,
}

/// `==` と `!=` の比べ方。比べられない型なら `None`。型検査の E2006 と Core IR の命令の選択が、同じ判定を使うため
/// にここに置く。`Int` と `String` は extern の型なので索引から、`Bool` は lang item から引く。
pub fn equality(program: &Program, types: &TypeStore, ty: TypeId) -> Option<Equality> {
    let TypeKind::Con { id, .. } = types.kind(ty) else {
        return None;
    };
    if *id == program.extern_type(ExternType::Int) {
        Some(Equality::Int)
    } else if *id == program.extern_type(ExternType::String) {
        Some(Equality::String)
    } else if *id == program.lang.bool {
        Some(Equality::Bool)
    } else {
        None
    }
}

#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, TypeId>,
    pub locals: ArenaMap<LocalId, TypeId>,
    /// パターンが受けた値の型。`bind_pat` が束縛するパターンを記録するので、コンストラクタとタプルの引数のパターンは
    /// どれも入る。ラムダの引数の外側にある注釈のパターンは入らない (check/body.rs の `bind_param`)。Core IR が、
    /// handler の節の引数の変数と、`unpack` と `switch` で作るフィールドの変数の Repr を決めるのに使う。
    pub pats: ArenaMap<PatId, TypeId>,
    /// 式の中のトップレベルの item への参照ごとの具体化。キーは参照を表す `ExprKind::Path` の式である。局所変数の参照、
    /// パターンのコンストラクタ、handler の節の操作、シグネチャのない参照は記録しない
    /// (docs/implementation/architecture.md の「`eml_types` の内部」)。
    pub instantiations: ArenaMap<ExprId, Instantiation>,
    /// 呼び出しの矢印ごとの `mask` (docs/spec/types.md の「推論」)。キーは呼び出しの式と矢印の番号である。
    /// Core IR が、その呼び出しで飛ばすエフェクトとして使う。
    pub masks: HashMap<(ExprId, usize), Vec<EffectId>>,
}

/// 1つの参照の具体化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instantiation {
    pub decl: ValueItem,
    /// `Shape::rigids` の順 (関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の
    /// 頭の型引数の順) に並べた型引数。row 変数と Kind 変数は持たない。
    pub args: Vec<TypeId>,
}

/// `files` は、E3003 の fix の字下げをソースから求めるのに使う (docs/implementation/diagnostics.md の「線形性の診断」)。
pub fn check(program: &Program, files: &SourceFiles) -> (TypedProgram, Vec<Diagnostic>) {
    check::check_module(program, files)
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
    test_program_with_files(text).0
}

/// `test_program` と、そのソース。診断を作るテストが使う。
#[cfg(test)]
pub(crate) fn test_program_with_files(text: &str) -> (eml_hir::Program, SourceFiles) {
    let (loaded, _) = eml_hir::load("test.em", text, &NoModules);
    let (def_map, _) = eml_hir::def_map(&loaded.modules);
    let program = eml_hir::lower(&def_map, &loaded.modules).0;
    (program, loaded.files)
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

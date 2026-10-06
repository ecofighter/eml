//! CST から HIR への変換と名前解決 (docs/implementation/architecture.md の「`eml_hir` で行う脱糖と検査」)。

mod def_map;
mod eval;
mod hir;
mod item_tree;
mod lower;
mod pretty;
mod program;

pub use def_map::*;
pub use eval::{EvalStep, call_steps, is_value, known_arity};
pub use hir::*;
pub use item_tree::*;
pub use lower::lower;
pub use pretty::pretty;
pub use program::*;

/// Prelude のソース。HIR の変換が読み、Core IR の intrinsic の表のテストも読む。
pub const PRELUDE_SOURCE: &str = include_str!("prelude.em");

/// 診断の表示に使う Prelude のパス。
pub const PRELUDE_PATH: &str = "Prelude.em";

/// Prelude を構文解析する。Prelude は処理系と一緒に配るソースなので、構文の誤りはない。呼ぶ側 (session、テスト) が
/// 同じ処理を重ねないよう、ここにまとめる。`file` は `PRELUDE_SOURCE` を登録した `SourceFiles` の番号である。
pub fn parse_prelude(file: eml_diagnostics::FileId) -> eml_syntax::ast::SourceFile {
    let (parse, errors) = eml_syntax::parse(file, PRELUDE_SOURCE);
    debug_assert!(errors.is_empty(), "{errors:?}");
    parse.tree()
}

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const UNDEFINED_NAME: ErrorCode = ErrorCode(1001);
    pub const UNDEFINED_TYPE: ErrorCode = ErrorCode(1002);
    pub const DUPLICATE_DEFINITION: ErrorCode = ErrorCode(1003);
    pub const MISSING_SIGNATURE: ErrorCode = ErrorCode(1004);
    pub const MISSING_EQUATION: ErrorCode = ErrorCode(1005);
    pub const NON_ASSOCIATIVE_OPERATORS: ErrorCode = ErrorCode(1006);
    pub const INVALID_OPERATION_SIGNATURE: ErrorCode = ErrorCode(1007);
    pub const NEVER_RESULT_NOT_FREE: ErrorCode = ErrorCode(1008);
    pub const UNHANDLEABLE_EFFECT: ErrorCode = ErrorCode(1009);
    pub const CLAUSE_ARITY: ErrorCode = ErrorCode(1010);
    pub const KEYWORD_ARITY: ErrorCode = ErrorCode(1011);
    pub const MIXED_EFFECTS_IN_HANDLER: ErrorCode = ErrorCode(1012);
    pub const MISSING_CLAUSE: ErrorCode = ErrorCode(1013);
    pub const DUPLICATE_CLAUSE: ErrorCode = ErrorCode(1014);
    pub const TYPE_ARGUMENT_COUNT: ErrorCode = ErrorCode(1015);
    pub const CONSTRUCTOR_ARITY: ErrorCode = ErrorCode(1016);
    pub const DUPLICATE_BINDING: ErrorCode = ErrorCode(1017);
    pub const NON_CONSECUTIVE_EQUATIONS: ErrorCode = ErrorCode(1018);
    pub const SIGNATURE_NOT_ADJACENT: ErrorCode = ErrorCode(1019);
    pub const EQUATION_ARITY_MISMATCH: ErrorCode = ErrorCode(1020);
    pub const DUPLICATE_FIXITY: ErrorCode = ErrorCode(1021);
    pub const FIXITY_WITHOUT_DEFINITION: ErrorCode = ErrorCode(1022);
    pub const INVALID_SECTION: ErrorCode = ErrorCode(1023);
    pub const USE_AT_END_OF_BLOCK: ErrorCode = ErrorCode(1024);
    pub const MISSING_CONSTRUCTORS: ErrorCode = ErrorCode(1025);
}

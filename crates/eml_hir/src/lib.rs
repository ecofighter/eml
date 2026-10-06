//! CST から HIR への変換と名前解決 (docs/implementation/architecture.md の「`eml_hir` で行う脱糖と検査」)。

mod eval;
mod hir;
mod lower;
mod pretty;

pub use eval::{EvalStep, call_steps, is_value, known_arity};
pub use hir::*;
pub use lower::lower;
pub use pretty::pretty;

/// Prelude のソース。HIR の変換が読み、Core IR の intrinsic の表のテストも読む。
pub const PRELUDE_SOURCE: &str = include_str!("prelude.em");

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
}

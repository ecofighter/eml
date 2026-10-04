//! CST から HIR への変換と名前解決 (docs/implementation/architecture.md の「`eml_hir` で行う脱糖と検査」)。

pub mod builtin;
mod hir;
mod lower;
mod pretty;

pub use hir::*;
pub use lower::lower;
pub use pretty::pretty;

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
}

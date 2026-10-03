//! CST から HIR への変換と名前解決 (docs/implementation/architecture.md の「`eml_hir` で行う脱糖と検査」)。

pub mod builtin;
mod hir;
mod lower;
mod pretty;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};

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
}

/// まだ扱えない構文。E0004 はどの段階でも「後で実装する」という同じ意味で使う (docs/spec/diagnostics.md)。
pub fn not_yet_supported(file: FileId, range: TextRange, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        eml_syntax::codes::NOT_YET_SUPPORTED,
        message,
        Label::new(file, range, eml_syntax::NOT_YET_SUPPORTED_LABEL),
    )
}

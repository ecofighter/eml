//! 字句解析、イベント方式のパーサ、rowan の木、型付き AST ラッパ。

mod lexer;
mod syntax_kind;

pub use lexer::{Token, lex};
pub use syntax_kind::{
    EmlLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxNodePtr, SyntaxToken,
};

/// 字句・構文の診断の番号 (E0xxx)。
pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const UNEXPECTED_CHARACTER: ErrorCode = ErrorCode(1);
    pub const UNTERMINATED_STRING: ErrorCode = ErrorCode(2);
    pub const EXPECTED_ITEM: ErrorCode = ErrorCode(3);
    pub const NOT_YET_SUPPORTED: ErrorCode = ErrorCode(4);
}

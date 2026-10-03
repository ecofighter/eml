//! 字句解析、イベント方式のパーサ、rowan の木、型付き AST ラッパ。

pub mod ast;
mod debug_dump;
mod grammar;
mod lexer;
mod parser;
mod sink;
mod syntax_kind;
mod token_set;

use eml_diagnostics::{Diagnostic, FileId, TextSize};
use rowan::GreenNode;
use rowan::ast::AstNode;

pub use debug_dump::debug_tree;
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
    pub const UNTERMINATED_BLOCK_COMMENT: ErrorCode = ErrorCode(5);
    pub const INVALID_NUMBER: ErrorCode = ErrorCode(7);
    pub const INVALID_ESCAPE: ErrorCode = ErrorCode(8);
}

/// E0004 (まだ対応していない構文) のラベル。
pub(crate) const NOT_YET_SUPPORTED_LABEL: &str = "this is implemented in a later stage";

/// パースの結果の木。壊れた入力でも必ず木を作る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
}

impl Parse {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn tree(&self) -> ast::SourceFile {
        ast::SourceFile::cast(self.syntax()).expect("the root is always SOURCE_FILE")
    }
}

pub fn parse(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
    let (tokens, mut diagnostics) = lex(file, text);
    let mut parser = parser::Parser::new(file, &tokens, TextSize::of(text));
    grammar::source_file(&mut parser);
    let (events, parse_diagnostics) = parser.finish();
    diagnostics.extend(parse_diagnostics);
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.range.start());
    let green = sink::build_tree(text, &tokens, events);
    (Parse { green }, diagnostics)
}

//! 型付き AST ラッパ。rowan の木の上の薄い型付きの見方を提供する。

use rowan::ast::AstNode;

use crate::{EmlLanguage, SyntaxKind, SyntaxNode};

/// ファイル全体。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceFile {
    syntax: SyntaxNode,
}

impl AstNode for SourceFile {
    type Language = EmlLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SOURCE_FILE
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(SourceFile { syntax })
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }
}

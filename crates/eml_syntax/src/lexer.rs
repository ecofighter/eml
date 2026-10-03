use eml_diagnostics::{Diagnostic, FileId, Label, TextRange, TextSize};
use logos::Logos;

use crate::SyntaxKind;
use crate::codes;

/// 字句解析の結果の1トークン。trivia (空白とコメント) も含む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub range: TextRange,
}

/// テキストをトークン列に分ける。トークン列をつなげると元のテキストに戻る (lossless)。
/// 認識できない文字の並びは1つの `ERROR_TOKEN` にまとめ、診断を1件出す。
pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut tokens: Vec<Token> = Vec::new();
    let mut diagnostics = Vec::new();
    let mut lexer = SyntaxKind::lexer(text);
    while let Some(result) = lexer.next() {
        let span = lexer.span();
        let range = TextRange::new(
            TextSize::new(span.start as u32),
            TextSize::new(span.end as u32),
        );
        let kind = match result {
            Ok(SyntaxKind::UNTERMINATED_STRING) => {
                diagnostics.push(Diagnostic::error(
                    codes::UNTERMINATED_STRING,
                    "unterminated string literal",
                    Label::new(file, range, "missing closing `\"`"),
                ));
                SyntaxKind::STRING
            }
            Ok(kind) => kind,
            Err(()) => SyntaxKind::ERROR_TOKEN,
        };
        match tokens.last_mut() {
            Some(last)
                if kind == SyntaxKind::ERROR_TOKEN && last.kind == SyntaxKind::ERROR_TOKEN =>
            {
                last.range = last.range.cover(range);
            }
            _ => tokens.push(Token { kind, range }),
        }
    }
    for token in tokens
        .iter()
        .filter(|token| token.kind == SyntaxKind::ERROR_TOKEN)
    {
        let snippet = &text[token.range];
        diagnostics.push(Diagnostic::error(
            codes::UNEXPECTED_CHARACTER,
            format!("unexpected character `{snippet}`"),
            Label::new(file, token.range, "not valid in eml source"),
        ));
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.range.start());
    (tokens, diagnostics)
}

pub mod ast;
mod debug_dump;
mod grammar;
mod layout;
mod lexer;
mod literal;
mod parser;
mod sink;
mod syntax_kind;
mod token_set;

use eml_diagnostics::{Diagnostic, FileId, SourceFiles};
use rowan::GreenNode;
use rowan::ast::AstNode;

pub use debug_dump::debug_tree;
pub use lexer::{Token, lex};
pub use rowan::ast::AstPtr;
pub use syntax_kind::{EmlLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const UNEXPECTED_CHARACTER: ErrorCode = ErrorCode(1);
    pub const UNTERMINATED_STRING: ErrorCode = ErrorCode(2);
    pub const UNTERMINATED_QUOTE: ErrorCode = ErrorCode(17);
    pub const EXPECTED_ITEM: ErrorCode = ErrorCode(3);
    // E0004 (未対応) はどの段階が出しても同じ意味なので、eml_diagnostics::NOT_YET_SUPPORTED に置く (docs/spec/diagnostics.md の「データ構造」)
    pub const UNTERMINATED_BLOCK_COMMENT: ErrorCode = ErrorCode(5);
    pub const TAB_INDENTATION: ErrorCode = ErrorCode(6);
    pub const INVALID_NUMBER: ErrorCode = ErrorCode(7);
    pub const INVALID_ESCAPE: ErrorCode = ErrorCode(8);
    pub const EXPECTED_INDENTED_BLOCK: ErrorCode = ErrorCode(9);
    pub const SPACE_AROUND_DOT: ErrorCode = ErrorCode(10);
    pub const SYNTAX_ERROR: ErrorCode = ErrorCode(11);
    pub const NEEDS_PARENS: ErrorCode = ErrorCode(12);
    pub const NESTING_TOO_DEEP: ErrorCode = ErrorCode(13);
    pub const INVALID_MULTILINE_STRING: ErrorCode = ErrorCode(14);
    pub const BLOCK_CLOSED_BY_COMMA: ErrorCode = ErrorCode(15);
    pub const INVALID_CHAR_LITERAL: ErrorCode = ErrorCode(16);
}

/// 壊れた入力でも必ず木を作る。エラーがあっても後の段階を止めないため。
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

    /// `parse_input` と `parse_query` の木の根。
    pub fn repl_input(&self) -> ast::ReplInput {
        ast::ReplInput::cast(self.syntax())
            .expect("the root of `parse_input` and `parse_query` is a REPL input")
    }
}

pub fn parse(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
    parse_with(file, text, grammar::source_file)
}

/// REPL の入力を読む。根は `REPL_ITEMS`、`REPL_LET`、`REPL_EXPR` のどれかである
/// (docs/spec/grammar.md の「REPL の入力」)。
pub fn parse_input(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
    parse_with(file, text, grammar::repl_input)
}

/// `:type` の問い合わせを読む。根は `REPL_QUERY` で、式をちょうど1つ持つ (docs/spec/grammar.md の「REPL の入力」)。
pub fn parse_query(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
    parse_with(file, text, grammar::repl_query)
}

fn parse_with(file: FileId, text: &str, rule: fn(&mut parser::Parser)) -> (Parse, Vec<Diagnostic>) {
    let (tokens, mut diagnostics) = lex(file, text);
    let (input, layout_diagnostics) = layout::layout(file, text, &tokens);
    diagnostics.extend(layout_diagnostics);
    let mut parser = parser::Parser::new(file, text, input);
    rule(&mut parser);
    let (events, parse_diagnostics) = parser.finish();
    diagnostics.extend(parse_diagnostics);
    let green = sink::build_tree(text, &tokens, events);
    (Parse { green }, diagnostics)
}

/// REPL の入力が終わったか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputStatus {
    Complete,
    Incomplete,
}

/// 端末が行を読むたびに、それまでの行を改行でつないだテキストを渡す。字句とレイアウト段の状態と、`parse_input` と
/// 同じ入力の種類の判定だけを見る (docs/spec/layout.md の「REPL の入力の終わり」)。
pub fn input_status(text: &str) -> InputStatus {
    // 診断は捨てるので、ファイルの番号は字句解析と構文解析に渡す印にすぎない
    let file = SourceFiles::new().add("<input>", "");
    let (tokens, _, open_literal) = lexer::lex_to_end(file, text);
    let (_, _, end) = layout::layout_to_end(file, text, &tokens);
    // 規則 1
    if open_literal || end.open_bracket {
        return InputStatus::Incomplete;
    }
    let Some(last) = tokens.iter().rev().find(|token| !token.kind.is_trivia()) else {
        return InputStatus::Complete;
    };
    // 規則 2
    if layout::BLOCK_STARTERS.contains(&last.kind) {
        return InputStatus::Incomplete;
    }
    // 規則 3
    if ends_with_blank_line(&text[last.range.end().into()..]) {
        return InputStatus::Complete;
    }
    if end.in_block || ends_with_definition(file, text) {
        InputStatus::Incomplete
    } else {
        InputStatus::Complete
    }
}

/// 最後のトークンの後ろ (trivia だけ) が、空白だけの行で終わっているか。
fn ends_with_blank_line(rest: &str) -> bool {
    rest.rsplit_once('\n')
        .is_some_and(|(_, last_line)| last_line.trim().is_empty())
}

/// item の並びで、最後の項目がシグネチャか等式か。等式を何本も続けられるようにするため。
fn ends_with_definition(file: FileId, text: &str) -> bool {
    let (parse, _) = parse_input(file, text);
    let root = parse.syntax();
    root.kind() == SyntaxKind::REPL_ITEMS
        && root
            .children()
            .last()
            .is_some_and(|item| matches!(item.kind(), SyntaxKind::SIGNATURE | SyntaxKind::EQUATION))
}

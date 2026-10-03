//! rust-analyzer と同じイベント方式のパーサ。文法の規則は `grammar` に置き、ここは仕組みだけを持つ。

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind;
use crate::lexer::Token;
use crate::token_set::TokenSet;

/// パーサが出すイベント。`sink::build_tree` がこれを rowan の木に組み立てる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// ノードの開始。`forward_parent` は、`precede` で後から作った親ノードの `Start` までの距離。
    Start {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },
    /// trivia でないトークンを1つ進める。
    Token {
        kind: SyntaxKind,
    },
    Finish,
    /// まだ種類の決まっていない `Start`、または取り消したノード。
    Tombstone,
}

pub(crate) struct Parser {
    file: FileId,
    /// trivia を除いたトークン列。
    tokens: Vec<Token>,
    pos: usize,
    eof: TextSize,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    pub(crate) fn new(file: FileId, tokens: &[Token], eof: TextSize) -> Parser {
        Parser {
            file,
            tokens: tokens
                .iter()
                .copied()
                .filter(|token| !token.kind.is_trivia())
                .collect(),
            pos: 0,
            eof,
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn finish(self) -> (Vec<Event>, Vec<Diagnostic>) {
        (self.events, self.diagnostics)
    }

    /// `n` 個先の trivia でないトークンの種類。入力の終わりを越えたら `EOF`。
    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        self.tokens
            .get(self.pos + n)
            .map_or(SyntaxKind::EOF, |token| token.kind)
    }

    pub(crate) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(crate) fn at_ts(&self, set: TokenSet) -> bool {
        set.contains(self.current())
    }

    pub(crate) fn at_eof(&self) -> bool {
        self.at(SyntaxKind::EOF)
    }

    /// 今のトークンの位置。入力の終わりでは、テキストの末尾の空の範囲。
    pub(crate) fn current_range(&self) -> TextRange {
        self.tokens
            .get(self.pos)
            .map_or(TextRange::empty(self.eof), |token| token.range)
    }

    pub(crate) fn bump_any(&mut self) {
        let kind = self.current();
        assert_ne!(kind, SyntaxKind::EOF, "cannot bump past the end of input");
        self.events.push(Event::Token { kind });
        self.pos += 1;
    }

    // 後の段階の文法で使う。
    #[allow(dead_code)]
    pub(crate) fn bump(&mut self, kind: SyntaxKind) {
        assert!(
            self.at(kind),
            "expected {kind:?}, found {:?}",
            self.current()
        );
        self.bump_any();
    }

    // 後の段階の文法で使う。
    #[allow(dead_code)]
    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump_any();
            true
        } else {
            false
        }
    }

    pub(crate) fn start(&mut self) -> Marker {
        let pos = self.events.len() as u32;
        self.events.push(Event::Tombstone);
        Marker { pos }
    }

    /// 今のトークンの位置に診断を出す。トークンは進めない。`label` はその位置に付ける説明。
    pub(crate) fn error(
        &mut self,
        code: ErrorCode,
        message: impl Into<String>,
        label: impl Into<String>,
    ) {
        let range = self.current_range();
        self.diagnostics.push(Diagnostic::error(
            code,
            message,
            Label::new(self.file, range, label),
        ));
    }
}

/// 開始したノード。`complete` か `abandon` で必ず閉じる。
#[must_use]
pub(crate) struct Marker {
    pos: u32,
}

impl Marker {
    pub(crate) fn complete(self, p: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        match &mut p.events[self.pos as usize] {
            Event::Start { kind: slot, .. } => *slot = kind,
            slot @ Event::Tombstone => {
                *slot = Event::Start {
                    kind,
                    forward_parent: None,
                }
            }
            _ => unreachable!("marker must point at a Start or Tombstone event"),
        }
        p.events.push(Event::Finish);
        CompletedMarker { pos: self.pos }
    }

    // 後の段階の文法で使う。
    #[allow(dead_code)]
    pub(crate) fn abandon(self, p: &mut Parser) {
        if self.pos as usize == p.events.len() - 1 {
            p.events.pop();
        }
    }
}

pub(crate) struct CompletedMarker {
    pos: u32,
}

impl CompletedMarker {
    /// 完了したノードの外側に、新しい親ノードを開始する (二項演算子の左辺などに使う)。
    // 後の段階の文法で使う。
    #[allow(dead_code)]
    pub(crate) fn precede(self, p: &mut Parser) -> Marker {
        let parent = p.start();
        match &mut p.events[self.pos as usize] {
            Event::Start { forward_parent, .. } => *forward_parent = Some(parent.pos - self.pos),
            _ => unreachable!("completed marker must point at a Start event"),
        }
        parent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_tree;
    use crate::lexer::lex;
    use crate::sink::build_tree;
    use crate::{SyntaxKind::*, SyntaxNode};
    use eml_diagnostics::SourceFiles;

    /// テキストを字句解析し、`grammar` でパースして木の表示を返す。仕組みだけを試すための小さな文法を渡す。
    fn run(text: &str, grammar: impl FnOnce(&mut Parser)) -> (String, Vec<Diagnostic>) {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (tokens, _) = lex(file, text);
        let mut p = Parser::new(file, &tokens, TextSize::of(text));
        grammar(&mut p);
        let (events, diagnostics) = p.finish();
        let tree = SyntaxNode::new_root(build_tree(text, &tokens, events));
        assert_eq!(tree.text().to_string(), text, "tree must be lossless");
        (debug_tree(&tree), diagnostics)
    }

    #[test]
    fn trivia_inside_root_goes_to_the_enclosing_node() {
        let (tree, _) = run(" a // c\n b ", |p| {
            let root = p.start();
            let inner = p.start();
            p.bump_any();
            inner.complete(p, ERROR);
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..11
          WHITESPACE@0..1 " "
          ERROR@1..2
            LIDENT@1..2 "a"
          WHITESPACE@2..3 " "
          COMMENT@3..7 "// c"
          WHITESPACE@7..9 "\n "
          LIDENT@9..10 "b"
          WHITESPACE@10..11 " "
        "#);
    }

    #[test]
    fn precede_wraps_a_completed_node() {
        let (tree, _) = run("a b", |p| {
            let root = p.start();
            let first = p.start();
            p.bump_any();
            let first = first.complete(p, ERROR);
            let outer = first.precede(p);
            p.bump_any();
            outer.complete(p, ERROR);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..3
          ERROR@0..3
            ERROR@0..1
              LIDENT@0..1 "a"
            WHITESPACE@1..2 " "
            LIDENT@2..3 "b"
        "#);
    }

    #[test]
    fn abandoned_marker_leaves_no_node() {
        let (tree, _) = run("a", |p| {
            let root = p.start();
            let unused = p.start();
            unused.abandon(p);
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..1
          LIDENT@0..1 "a"
        "#);
    }

    #[test]
    fn lookahead_skips_trivia_and_reports_eof() {
        run("a  // c\n b", |p| {
            let root = p.start();
            assert_eq!(p.current(), LIDENT);
            assert_eq!(p.nth(1), LIDENT);
            assert_eq!(p.nth(2), EOF);
            assert!(!p.eat(UIDENT));
            assert!(p.eat(LIDENT));
            p.bump(LIDENT);
            assert!(p.at_eof());
            root.complete(p, SOURCE_FILE);
        });
    }

    #[test]
    fn error_at_eof_points_at_end_of_text() {
        let (_, diagnostics) = run("a ", |p| {
            let root = p.start();
            p.bump_any();
            p.error(ErrorCode(9999), "expected more", "here");
            root.complete(p, SOURCE_FILE);
        });
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].primary.range,
            TextRange::empty(TextSize::new(2))
        );
    }
}

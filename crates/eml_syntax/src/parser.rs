//! rust-analyzer と同じイベント方式のパーサ。文法の規則は `grammar` に置き、ここは仕組みだけを持つ。
//! 入力はレイアウト段の出力で、仮想トークン (`LAYOUT_*`) を含み、trivia を含まない。

use std::cell::Cell;

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind;
use crate::lexer::{Token, operator_kind};
use crate::token_set::TokenSet;

/// 前進せずに先読みできる回数の上限。文法の誤りによる無限ループを検出する。
const STEP_LIMIT: u32 = 1_000_000;

/// パーサが出すイベント。`sink::build_tree` がこれを rowan の木に組み立てる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// ノードの開始。`forward_parent` は、`precede` で後から作った親ノードの `Start` までの距離。
    Start {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },
    /// trivia でないトークン (の残り全部) を1つ進める。
    Token {
        kind: SyntaxKind,
    },
    /// trivia でないトークンの先頭の `len` バイトだけを、`kind` のトークンとして進める。
    TokenPrefix {
        kind: SyntaxKind,
        len: TextSize,
    },
    Finish,
    /// まだ種類の決まっていない `Start`、または取り消したノード。
    Tombstone,
}

pub(crate) struct Parser<'t> {
    file: FileId,
    text: &'t str,
    /// レイアウト段の出力。
    tokens: Vec<Token>,
    pos: usize,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
    steps: Cell<u32>,
}

impl<'t> Parser<'t> {
    pub(crate) fn new(file: FileId, text: &'t str, tokens: Vec<Token>) -> Parser<'t> {
        Parser {
            file,
            text,
            tokens,
            pos: 0,
            events: Vec::new(),
            diagnostics: Vec::new(),
            steps: Cell::new(0),
        }
    }

    pub(crate) fn finish(self) -> (Vec<Event>, Vec<Diagnostic>) {
        (self.events, self.diagnostics)
    }

    /// `n` 個先のトークンの種類。入力の終わりを越えたら `EOF`。
    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        let steps = self.steps.get();
        assert!(
            steps < STEP_LIMIT,
            "the parser seems stuck at token {}",
            self.pos
        );
        self.steps.set(steps + 1);
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

    /// 項目の区切り。`;` はレイアウトの `SEP` と同じに扱う (spec §4 規則 5)。
    pub(crate) fn at_sep(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::LAYOUT_SEP | SyntaxKind::SEMICOLON
        )
    }

    /// 今のトークンのテキスト。仮想トークンと入力の終わりでは空。
    pub(crate) fn current_text(&self) -> &'t str {
        let text = self.text;
        self.tokens
            .get(self.pos)
            .map_or("", |token| &text[token.range])
    }

    /// 今のトークンの位置。入力の終わりでは、テキストの末尾の空の範囲。
    pub(crate) fn current_range(&self) -> TextRange {
        self.tokens
            .get(self.pos)
            .map_or(TextRange::empty(TextSize::of(self.text)), |token| {
                token.range
            })
    }

    /// 直前のトークンと今のトークンの間に、空白やコメントがないか。
    pub(crate) fn touches_prev(&self) -> bool {
        match (self.pos.checked_sub(1), self.tokens.get(self.pos)) {
            (Some(prev), Some(current)) => self.tokens[prev].range.end() == current.range.start(),
            _ => false,
        }
    }

    /// 今のトークンと次のトークンの間に、空白やコメントがないか。
    pub(crate) fn touches_next(&self) -> bool {
        match (self.tokens.get(self.pos), self.tokens.get(self.pos + 1)) {
            (Some(current), Some(next)) => current.range.end() == next.range.start(),
            _ => false,
        }
    }

    pub(crate) fn bump_any(&mut self) {
        let kind = self.current();
        assert_ne!(kind, SyntaxKind::EOF, "cannot bump past the end of input");
        if !kind.is_virtual() {
            self.events.push(Event::Token { kind });
        }
        self.advance();
    }

    pub(crate) fn bump(&mut self, kind: SyntaxKind) {
        assert!(
            self.at(kind),
            "expected {kind:?}, found {:?}",
            self.current()
        );
        self.bump_any();
    }

    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump_any();
            true
        } else {
            false
        }
    }

    /// 今のトークンを `kind` として木に入れる (型の中の `<` を `L_ANGLE` にする、など)。
    pub(crate) fn bump_remap(&mut self, kind: SyntaxKind) {
        assert!(!self.at_eof() && !self.current().is_virtual());
        self.events.push(Event::Token { kind });
        self.advance();
    }

    /// 今のトークン (2文字以上の演算子) の先頭の1文字を `kind` として読み、残りを今のトークンにする。
    /// 型の中で `<>` や `>->` を分けて読むのに使う (spec §5)。
    pub(crate) fn split_first_char(&mut self, kind: SyntaxKind) {
        let token = self.tokens[self.pos];
        let one = TextSize::new(1);
        assert!(
            token.range.len() > one,
            "cannot split a one-character token"
        );
        self.events.push(Event::TokenPrefix { kind, len: one });
        let rest = TextRange::new(token.range.start() + one, token.range.end());
        self.tokens[self.pos] = Token {
            kind: operator_kind(&self.text[rest]),
            range: rest,
        };
        self.steps.set(0);
    }

    fn advance(&mut self) {
        self.pos += 1;
        self.steps.set(0);
    }

    pub(crate) fn start(&mut self) -> Marker {
        let pos = self.events.len() as u32;
        self.events.push(Event::Tombstone);
        Marker::new(pos)
    }

    /// 今のトークンの位置に診断を出す。トークンは進めない。`label` はその位置に付ける説明。
    /// `ERROR_TOKEN` は字句解析で報告済みなので、その位置には出さない。
    /// 直前の診断と同じ位置にも出さない (1つの誤りから連鎖する診断を抑える)。
    pub(crate) fn error(
        &mut self,
        code: ErrorCode,
        message: impl Into<String>,
        label: impl Into<String>,
    ) {
        if self.at(SyntaxKind::ERROR_TOKEN) {
            return;
        }
        let range = self.current_range();
        if self
            .diagnostics
            .last()
            .is_some_and(|last| last.primary.range.start() == range.start())
        {
            return;
        }
        self.diagnostics.push(Diagnostic::error(
            code,
            message,
            Label::new(self.file, range, label),
        ));
    }
}

/// 開始したノード。`complete` か `abandon` で必ず閉じる。閉じずに捨てるとパニックする。
#[must_use]
pub(crate) struct Marker {
    pos: u32,
    done: bool,
}

impl Marker {
    fn new(pos: u32) -> Marker {
        Marker { pos, done: false }
    }

    pub(crate) fn complete(mut self, p: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        self.done = true;
        match &mut p.events[self.pos as usize] {
            slot @ Event::Tombstone => {
                *slot = Event::Start {
                    kind,
                    forward_parent: None,
                }
            }
            _ => unreachable!("marker must point at a Tombstone event"),
        }
        p.events.push(Event::Finish);
        CompletedMarker { pos: self.pos }
    }

    /// ノードを作らない。イベントは `Tombstone` のまま残す (取り除くと、`precede` で指された位置に
    /// 後のノードが入り、親子関係が壊れるため)。
    pub(crate) fn abandon(mut self, _p: &mut Parser) {
        self.done = true;
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        if !self.done && !std::thread::panicking() {
            panic!("marker must be completed or abandoned");
        }
    }
}

pub(crate) struct CompletedMarker {
    pos: u32,
}

impl CompletedMarker {
    /// 完了したノードの外側に、新しい親ノードを開始する (演算子の列やフィールドアクセスの左辺などに使う)。
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
    use crate::layout::layout;
    use crate::lexer::lex;
    use crate::sink::build_tree;
    use crate::{SyntaxKind::*, SyntaxNode};
    use eml_diagnostics::SourceFiles;

    /// テキストを字句解析とレイアウト段に通し、`grammar` でパースして木の表示を返す。
    /// 仕組みだけを試すための小さな文法を渡す。
    fn run(text: &str, grammar: impl FnOnce(&mut Parser)) -> (String, Vec<Diagnostic>) {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (tokens, _) = lex(file, text);
        let (input, _) = layout(file, text, &tokens);
        let mut p = Parser::new(file, text, input);
        grammar(&mut p);
        let (events, diagnostics) = p.finish();
        let tree = SyntaxNode::new_root(build_tree(text, &tokens, events));
        assert_eq!(tree.text().to_string(), text, "tree must be lossless");
        (debug_tree(&tree), diagnostics)
    }

    #[test]
    fn trivia_inside_root_goes_to_the_enclosing_node() {
        let (tree, _) = run(" a -- c\n b ", |p| {
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
          COMMENT@3..7 "-- c"
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
        run("a  -- c\n b", |p| {
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

    #[test]
    fn virtual_tokens_make_no_tree_tokens() {
        let (tree, _) = run("f =\n  a", |p| {
            let root = p.start();
            let mut virtuals = 0;
            while !p.at_eof() {
                if p.current().is_virtual() {
                    virtuals += 1;
                }
                p.bump_any();
            }
            assert_eq!(virtuals, 2);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..7
          LIDENT@0..1 "f"
          WHITESPACE@1..2 " "
          EQ@2..3 "="
          WHITESPACE@3..6 "\n  "
          LIDENT@6..7 "a"
        "#);
    }

    #[test]
    fn semicolon_is_a_separator() {
        run("a; b", |p| {
            let root = p.start();
            p.bump(LIDENT);
            assert!(p.at_sep());
            p.bump_any();
            p.bump(LIDENT);
            root.complete(p, SOURCE_FILE);
        });
    }

    #[test]
    fn split_first_char_divides_an_operator_token() {
        let (tree, _) = run("<>->", |p| {
            let root = p.start();
            p.split_first_char(L_ANGLE);
            assert_eq!(p.current(), OP);
            assert_eq!(p.current_text(), ">->");
            p.split_first_char(R_ANGLE);
            assert_eq!(p.current(), THIN_ARROW);
            p.bump(THIN_ARROW);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..4
          L_ANGLE@0..1 "<"
          R_ANGLE@1..2 ">"
          THIN_ARROW@2..4 "->"
        "#);
    }

    #[test]
    fn bump_remap_changes_the_tree_kind() {
        let (tree, _) = run("<", |p| {
            let root = p.start();
            p.bump_remap(L_ANGLE);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..1
          L_ANGLE@0..1 "<"
        "#);
    }

    #[test]
    fn touching_tokens() {
        run("a.b c", |p| {
            let root = p.start();
            assert!(!p.touches_prev());
            assert!(p.touches_next());
            p.bump(LIDENT);
            assert!(p.touches_prev());
            assert!(p.touches_next());
            p.bump(DOT);
            assert!(!p.touches_next());
            p.bump(LIDENT);
            p.bump(LIDENT);
            root.complete(p, SOURCE_FILE);
        });
    }

    #[test]
    fn precede_can_be_chained() {
        let (tree, _) = run("a b c", |p| {
            let root = p.start();
            let first = p.start();
            p.bump_any();
            let first = first.complete(p, ERROR);
            let second = first.precede(p);
            p.bump_any();
            let second = second.complete(p, ERROR);
            let third = second.precede(p);
            p.bump_any();
            third.complete(p, ERROR);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..5
          ERROR@0..5
            ERROR@0..3
              ERROR@0..1
                LIDENT@0..1 "a"
              WHITESPACE@1..2 " "
              LIDENT@2..3 "b"
            WHITESPACE@3..4 " "
            LIDENT@4..5 "c"
        "#);
    }

    #[test]
    fn abandoning_a_preceded_marker_keeps_the_child() {
        let (tree, _) = run("a b", |p| {
            let root = p.start();
            let first = p.start();
            p.bump_any();
            let first = first.complete(p, ERROR);
            let outer = first.precede(p);
            outer.abandon(p);
            let next = p.start();
            p.bump_any();
            next.complete(p, ERROR);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..3
          ERROR@0..1
            LIDENT@0..1 "a"
          WHITESPACE@1..2 " "
          ERROR@2..3
            LIDENT@2..3 "b"
        "#);
    }

    #[test]
    fn abandoning_a_marker_that_is_not_the_last_event() {
        let (tree, _) = run("a b", |p| {
            let root = p.start();
            let outer = p.start();
            let inner = p.start();
            p.bump_any();
            inner.complete(p, ERROR);
            outer.abandon(p);
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..3
          ERROR@0..1
            LIDENT@0..1 "a"
          WHITESPACE@1..2 " "
          LIDENT@2..3 "b"
        "#);
    }

    #[test]
    #[should_panic(expected = "marker must be completed or abandoned")]
    fn forgotten_marker_panics() {
        run("a", |p| {
            let _forgotten = p.start();
        });
    }

    #[test]
    #[should_panic(expected = "the parser seems stuck")]
    fn parser_without_progress_panics() {
        run("a", |p| {
            loop {
                p.current();
            }
        });
    }

    #[test]
    fn errors_at_the_same_position_are_reported_once() {
        let (_, diagnostics) = run("a", |p| {
            let root = p.start();
            p.error(ErrorCode(9999), "first", "here");
            p.error(ErrorCode(9998), "second", "here");
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "first");
    }

    #[test]
    fn no_error_is_reported_at_an_error_token() {
        let (_, diagnostics) = run("€", |p| {
            let root = p.start();
            p.error(ErrorCode(9999), "expected something", "here");
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        assert!(diagnostics.is_empty());
    }
}

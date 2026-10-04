//! 構文を差し替えやすくするため、文法の規則は `grammar` に置き、ここには仕組みだけを持つ。
//! 入力はレイアウト段の出力なので、仮想トークンを含み、trivia を含まない。

use std::cell::Cell;

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind;
use crate::lexer::{Token, operator_kind};
use crate::token_set::TokenSet;

/// 前進せずに先読みできる回数の上限。文法の誤りによる無限ループを、ハングではなく panic で見つけるため。
const STEP_LIMIT: u32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// `forward_parent` は、`precede` で後から作った親ノードの `Start` までの距離。
    Start {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },
    Token {
        kind: SyntaxKind,
    },
    /// 型の中で `<>` などを分けて読むとき、トークンの先頭だけを木に入れる。
    TokenPrefix {
        kind: SyntaxKind,
        len: TextSize,
    },
    Finish,
    /// まだ種類の決まっていない `Start`、または取り消したノード。
    Tombstone,
}

/// 式・パターン・型の入れ子の深さの上限 (docs/spec/grammar.md)。後の段階の再帰がスタックを溢れさせないよう、
/// parser で止める。
pub(crate) const NESTING_LIMIT: u32 = 256;

pub(crate) struct Parser<'t> {
    file: FileId,
    text: &'t str,
    tokens: Vec<Token>,
    pos: usize,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
    steps: Cell<u32>,
    depth: u32,
    too_deep: bool,
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
            depth: 0,
            too_deep: false,
        }
    }

    /// 入れ子を1段深くする。上限に達していたら何もせずに偽を返す。
    pub(crate) fn enter(&mut self) -> bool {
        if self.depth == NESTING_LIMIT {
            return false;
        }
        self.depth += 1;
        true
    }

    pub(crate) fn leave(&mut self) {
        self.depth -= 1;
    }

    /// 入れ子が深すぎて読み飛ばした項目の中では、診断を出さない。読み飛ばしたことで連鎖する診断を抑えるため。
    pub(crate) fn set_too_deep(&mut self, too_deep: bool) {
        self.too_deep = too_deep;
    }

    pub(crate) fn is_too_deep(&self) -> bool {
        self.too_deep
    }

    pub(crate) fn finish(self) -> (Vec<Event>, Vec<Diagnostic>) {
        (self.events, self.diagnostics)
    }

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

    /// `nth` と違い、ステップ上限に数えない。`EOF` で必ず終わる有限の先読み走査が、上限に引っかからないようにするため。
    pub(crate) fn peek(&self, n: usize) -> SyntaxKind {
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

    /// `;` はレイアウトの `SEP` と同じに扱う (docs/spec/layout.md の規則 5)。
    pub(crate) fn at_sep(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::LAYOUT_SEP | SyntaxKind::SEMICOLON
        )
    }

    pub(crate) fn current_text(&self) -> &'t str {
        let text = self.text;
        self.tokens
            .get(self.pos)
            .map_or("", |token| &text[token.range])
    }

    pub(crate) fn touches_prev(&self) -> bool {
        match (self.pos.checked_sub(1), self.tokens.get(self.pos)) {
            (Some(prev), Some(current)) => self.tokens[prev].range.end() == current.range.start(),
            _ => false,
        }
    }

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

    /// 型の中の `<` を `L_ANGLE` にするなど、lexer と違う種類で木に入れるときに使う。
    pub(crate) fn bump_remap(&mut self, kind: SyntaxKind) {
        assert!(!self.at_eof() && !self.current().is_virtual());
        self.events.push(Event::Token { kind });
        self.advance();
    }

    /// 型の中で `<>` や `>->` を分けて読むのに使う (docs/spec/grammar.md の「文法上の補足」)。
    pub(crate) fn split_first_char(&mut self, kind: SyntaxKind) {
        self.split_prefix(kind, 1);
    }

    /// トークンの先頭の `len` バイトを `kind` として木に入れ、残りを今のトークンにする。型の中の `-><` を
    /// `->` と `<` に分けるのにも使う (docs/spec/grammar.md の「文法上の補足」)。
    pub(crate) fn split_prefix(&mut self, kind: SyntaxKind, len: u32) {
        let token = self.tokens[self.pos];
        let len = TextSize::new(len);
        assert!(
            token.range.len() > len,
            "the prefix must be shorter than the token"
        );
        self.events.push(Event::TokenPrefix { kind, len });
        let rest = TextRange::new(token.range.start() + len, token.range.end());
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

    /// `ERROR_TOKEN` の位置には出さない。字句解析で報告済みのため。直前の診断と同じ位置にも出さない。
    /// 1つの誤りから連鎖する診断を抑えるため。
    pub(crate) fn error(
        &mut self,
        code: ErrorCode,
        message: impl Into<String>,
        label: impl Into<String>,
    ) {
        if self.too_deep {
            return;
        }
        if self.at(SyntaxKind::ERROR_TOKEN) {
            return;
        }
        let range = self.error_range();
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

    /// 仮想トークンと EOF は次の行の先頭やテキストの終わりにあり、そこを指すと誤りのない行を指してしまう。
    /// そのため、直前の実トークンの直後の空の範囲を指す (docs/spec/layout.md の「エラー回復」)。
    fn error_range(&self) -> TextRange {
        match self.tokens.get(self.pos) {
            Some(token) if !token.kind.is_virtual() => token.range,
            _ => {
                let end = self.tokens[..self.pos.min(self.tokens.len())]
                    .iter()
                    .rev()
                    .find(|token| !token.kind.is_virtual())
                    .map_or(TextSize::new(0), |token| token.range.end());
                TextRange::empty(end)
            }
        }
    }
}

/// 閉じ忘れは文法の実装の誤りなので、`complete` も `abandon` もせずに捨てるとパニックする。
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

    /// イベントは取り除かずに `Tombstone` のまま残す。取り除くと、`precede` で指された位置に後のノードが入り、
    /// 親子関係がずれるため。
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
    /// 左辺を読んだ後で、それを子に持つノード (演算子の列、フィールドアクセスなど) を作るのに使う。
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
mod tests;

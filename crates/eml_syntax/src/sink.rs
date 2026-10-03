use std::mem;

use rowan::{GreenNode, GreenNodeBuilder, Language};

use eml_diagnostics::{TextRange, TextSize};

use crate::lexer::Token;
use crate::parser::Event;
use crate::{EmlLanguage, SyntaxKind};

/// trivia は、直後に始まるノードではなく、それを囲むノードのうち一番内側のものに付ける。
pub(crate) fn build_tree(text: &str, tokens: &[Token], mut events: Vec<Event>) -> GreenNode {
    let mut builder = Builder {
        text,
        tokens,
        next: 0,
        offset: TextSize::new(0),
        inner: GreenNodeBuilder::new(),
    };
    let mut depth = 0usize;
    let mut kinds = Vec::new();
    for i in 0..events.len() {
        match mem::replace(&mut events[i], Event::Tombstone) {
            Event::Start {
                kind,
                forward_parent,
            } => {
                // `precede` で作った親は後ろのイベントにあるので、連鎖をたどって集め、外側から開く。
                kinds.push(kind);
                let mut index = i;
                let mut next = forward_parent;
                while let Some(distance) = next {
                    index += distance as usize;
                    next = match mem::replace(&mut events[index], Event::Tombstone) {
                        Event::Start {
                            kind,
                            forward_parent,
                        } => {
                            kinds.push(kind);
                            forward_parent
                        }
                        // 放棄した親ノードで、連鎖はここで終わる。
                        Event::Tombstone => None,
                        _ => {
                            unreachable!("forward_parent must point at a Start or Tombstone event")
                        }
                    };
                }
                for kind in kinds.drain(..).rev() {
                    if depth > 0 {
                        builder.eat_trivia();
                    }
                    builder.inner.start_node(EmlLanguage::kind_to_raw(kind));
                    depth += 1;
                }
            }
            Event::Token { kind } => {
                builder.eat_trivia();
                builder.token(kind);
            }
            Event::TokenPrefix { kind, len } => {
                builder.eat_trivia();
                builder.token_prefix(kind, len);
            }
            Event::Finish => {
                depth -= 1;
                if depth == 0 {
                    builder.eat_trivia();
                }
                builder.inner.finish_node();
            }
            Event::Tombstone => {}
        }
    }
    builder.inner.finish()
}

struct Builder<'a> {
    text: &'a str,
    tokens: &'a [Token],
    next: usize,
    /// `<>` のように1つのトークンを分けて木に入れるとき、すでに入れたバイト数。
    offset: TextSize,
    inner: GreenNodeBuilder<'static>,
}

impl Builder<'_> {
    fn eat_trivia(&mut self) {
        while let Some(token) = self.tokens.get(self.next) {
            if !token.kind.is_trivia() {
                break;
            }
            self.token(token.kind);
        }
    }

    fn token(&mut self, kind: SyntaxKind) {
        let range = self.tokens[self.next].range;
        let start = range.start() + self.offset;
        self.inner.token(
            EmlLanguage::kind_to_raw(kind),
            &self.text[TextRange::new(start, range.end())],
        );
        self.next += 1;
        self.offset = TextSize::new(0);
    }

    fn token_prefix(&mut self, kind: SyntaxKind, len: TextSize) {
        let range = self.tokens[self.next].range;
        let start = range.start() + self.offset;
        self.inner.token(
            EmlLanguage::kind_to_raw(kind),
            &self.text[TextRange::at(start, len)],
        );
        self.offset += len;
    }
}

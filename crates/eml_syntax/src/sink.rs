use std::mem;

use rowan::{GreenNode, GreenNodeBuilder, Language};

use crate::lexer::Token;
use crate::parser::Event;
use crate::{EmlLanguage, SyntaxKind};

/// イベント列と、trivia を含むトークン列から、rowan の木を組み立てる。
/// trivia は、それを囲むノードのうち一番内側のもの (直後に始まるノードの親) に付ける。
pub(crate) fn build_tree(text: &str, tokens: &[Token], mut events: Vec<Event>) -> GreenNode {
    let mut builder = Builder {
        text,
        tokens,
        next: 0,
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
                // `precede` で作った親ノードを外側から順に開く。
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
                        _ => unreachable!("forward_parent must point at a Start event"),
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
        let token = self.tokens[self.next];
        self.inner
            .token(EmlLanguage::kind_to_raw(kind), &self.text[token.range]);
        self.next += 1;
    }
}

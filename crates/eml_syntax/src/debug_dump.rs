use std::fmt::Write;

use crate::{SyntaxElement, SyntaxNode};

pub fn debug_tree(node: &SyntaxNode) -> String {
    let mut out = String::new();
    write_element(&mut out, SyntaxElement::Node(node.clone()), 0);
    out
}

fn write_element(out: &mut String, element: SyntaxElement, depth: usize) {
    let indent = "  ".repeat(depth);
    match element {
        SyntaxElement::Node(node) => {
            writeln!(out, "{indent}{:?}@{:?}", node.kind(), node.text_range()).unwrap();
            for child in node.children_with_tokens() {
                write_element(out, child, depth + 1);
            }
        }
        SyntaxElement::Token(token) => {
            writeln!(
                out,
                "{indent}{:?}@{:?} {:?}",
                token.kind(),
                token.text_range(),
                token.text()
            )
            .unwrap();
        }
    }
}

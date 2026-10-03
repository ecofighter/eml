use std::fmt::Write;

use crate::{SyntaxElement, SyntaxNode};

/// 木を1行1要素で表示する。CST のスナップショットテストに使う。
/// ノードは `KIND@start..end`、トークンは `KIND@start..end "text"` の形で、子は2文字ずつ字下げする。
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

//! 文法のテストの補助。範囲と trivia を省いた木の形と、診断の一覧を表示する。
#![allow(dead_code)]

use eml_diagnostics::SourceFiles;
use eml_syntax::{SyntaxElement, SyntaxNode, parse};

/// 木の形 (ノードと、trivia でないトークン) を表示し、`---` の後に診断を並べる。
/// 木が元のテキストに戻ることも確認する。
pub fn shape(text: &str) -> String {
    let (root, diagnostics) = parse_text(text);
    let mut out = String::new();
    write_node(&mut out, &root, 0);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for line in diagnostics {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// 診断を `E0011 1:5 message` の形で並べる。行と列は 1 始まりで、列は文字数で数える。
pub fn diagnostics(text: &str) -> Vec<String> {
    parse_text(text).1
}

/// テキストの行を改行でつなぐ。
pub fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}

fn parse_text(text: &str) -> (SyntaxNode, Vec<String>) {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    let root = parse.syntax();
    assert_eq!(root.text().to_string(), text, "tree must be lossless");
    let shown = diagnostics
        .iter()
        .map(|d| {
            let offset = u32::from(d.primary.range.start()) as usize;
            let before = &text[..offset];
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            format!("{} {line}:{column} {}", d.code, d.message)
        })
        .collect();
    (root, shown)
}

fn write_node(out: &mut String, node: &SyntaxNode, depth: usize) {
    out.push_str(&format!("{}{:?}\n", "  ".repeat(depth), node.kind()));
    for child in node.children_with_tokens() {
        match child {
            SyntaxElement::Node(child) => write_node(out, &child, depth + 1),
            SyntaxElement::Token(token) if !token.kind().is_trivia() => {
                out.push_str(&format!(
                    "{}{:?} {:?}\n",
                    "  ".repeat(depth + 1),
                    token.kind(),
                    token.text()
                ));
            }
            SyntaxElement::Token(_) => {}
        }
    }
}

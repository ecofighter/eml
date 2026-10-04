#![allow(dead_code)]

use eml_syntax::{SyntaxElement, SyntaxNode};
use eml_test_support::{parse, short};

/// 木の形 (trivia を除く) と、構文の診断。lossless の確認は `eml_test_support::parse` が行う。
pub fn shape(text: &str) -> String {
    let parsed = parse(text);
    let mut out = String::new();
    write_node(&mut out, &parsed.parse.syntax(), 0);
    let diagnostics = short(&parsed.files, &parsed.diagnostics);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for line in diagnostics {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// 期待値を読みやすくするため、位置はバイトではなく 1 始まりの行と列 (文字数) で表示する。
pub fn diagnostics(text: &str) -> Vec<String> {
    let parsed = parse(text);
    short(&parsed.files, &parsed.diagnostics)
}

/// help の文言を確かめるテストのため、すべての診断の help を順に返す。
pub fn helps(text: &str) -> Vec<String> {
    parse(text)
        .diagnostics
        .into_iter()
        .flat_map(|diagnostic| diagnostic.help)
        .collect()
}

/// トップレベルの子のノードの種類。回復が次の項目から再開することを確かめるため。
pub fn item_kinds(text: &str) -> Vec<String> {
    parse(text)
        .parse
        .syntax()
        .children()
        .map(|node| format!("{:?}", node.kind()))
        .collect()
}

pub fn lines(lines: &[&str]) -> String {
    lines.join("\n")
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

//! コメントと文書が `docs/` の節を `…の「見出し」` の形で引いたとき、その見出しが行き先のファイルにあることと、
//! 文書のリンクの行き先があることを確かめる。節を移したときに、引用だけが古いまま残るのを防ぐため
//! (docs/implementation/testing.md の「文書の引用の検査」)。

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// 引用を探すファイル。`docs/superpowers` と `docs/plans` は作業中の設計と計画で、まだない節を引くので除く。
fn sources(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("CLAUDE.md"), root.join("README.md")];
    let mut dirs = vec![
        root.join("crates"),
        root.join("docs"),
        root.join("std"),
        root.join("tests"),
    ];
    while let Some(dir) = dirs.pop() {
        if dir.ends_with("docs/superpowers")
            || dir.ends_with("docs/plans")
            || dir.ends_with("target")
        {
            continue;
        }
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("rs" | "em" | "md")
            ) {
                files.push(path);
            }
        }
    }
    files
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
}

/// 行の折り返しと字下げをまたいで引用を読めるよう、行頭のコメントの記号を除き、空白をすべて除いてつなぐ。
/// 日本語の文は折り返しで空白が入ったり消えたりするので、見出しとの照合も空白を除いて行う。
fn flatten(path: &Path, text: &str) -> String {
    text.lines()
        .map(|line| {
            let line = line.trim_start();
            if is_markdown(path) {
                line
            } else {
                ["///", "//!", "//", "--"]
                    .iter()
                    .find_map(|marker| line.strip_prefix(marker))
                    .unwrap_or(line)
            }
        })
        .flat_map(str::chars)
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn without_whitespace(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn headings(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter(|line| line.starts_with('#'))
                .map(|line| without_whitespace(line.trim_start_matches('#')))
                .collect()
        })
        .unwrap_or_default()
}

struct Citation {
    target: PathBuf,
    heading: String,
}

/// `docs/…/x.md の「見出し」` (根からのパス) と `[名前](相対パス) の「見出し」` を拾う。
/// パスのない `上の「…」` は同じファイルの中の参照なので、ここでは見ない。
fn citations(file: &Path, root: &Path, flat: &str) -> Vec<Citation> {
    const OPEN: &str = "の「";
    let mut found = Vec::new();
    for (at, _) in flat.match_indices(OPEN) {
        let before = &flat[..at];
        let after = &flat[at + OPEN.len()..];
        let Some(end) = after.find('」') else {
            continue;
        };
        let target = if let Some(link) = before.strip_suffix(')') {
            let Some(open) = link.rfind("](") else {
                continue;
            };
            let path = &link[open + 2..];
            if !path.ends_with(".md") {
                continue;
            }
            file.parent().unwrap().join(path)
        } else if before.ends_with(".md") {
            let start = before
                .char_indices()
                .rev()
                .find(|&(_, c)| !(c.is_ascii_alphanumeric() || "_./-".contains(c)))
                .map_or(0, |(i, c)| i + c.len_utf8());
            let path = &before[start..];
            if !path.starts_with("docs/") {
                continue;
            }
            root.join(path)
        } else {
            continue;
        };
        found.push(Citation {
            target: target.canonicalize().unwrap_or(target),
            heading: after[..end].to_string(),
        });
    }
    found
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[test]
fn every_cited_heading_exists() {
    let root = repo_root();
    let mut broken = Vec::new();
    for file in sources(&root) {
        let text = fs::read_to_string(&file).unwrap();
        for citation in citations(&file, &root, &flatten(&file, &text)) {
            if !headings(&citation.target).contains(&citation.heading) {
                broken.push(format!(
                    "{}: {} の「{}」",
                    relative(&root, &file),
                    relative(&root, &citation.target),
                    citation.heading
                ));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "見出しのない引用:\n{}",
        broken.join("\n")
    );
}

/// コード片とコードブロックを除いた本文。Core IR のテキスト (`prim ev_clause[State.get](e.2)`) のように、コードの中の
/// `](` はリンクでないためである。コード片は同じ長さのバッククォートの並びで閉じる。
///
/// 閉じないコードブロックは残りをすべてコードとして読み、閉じないコード片は行の残りの読み方が決まらない。どちらも
/// リンクを知らないうちに見落とすので、誤りにする。Markdown ではコード片が行をまたげるが、文書ではコード片を行の中で
/// 閉じることにし、行をまたぐものも閉じないコード片として扱う。
fn without_code(text: &str) -> Result<String, String> {
    let mut prose = String::new();
    let mut fenced = None;
    for (number, line) in (1..).zip(text.lines()) {
        if line.trim_start().starts_with("```") {
            fenced = match fenced {
                Some(_) => None,
                None => Some(number),
            };
        } else if fenced.is_none() {
            let mut rest = line;
            while let Some(start) = rest.find('`') {
                prose.push_str(&rest[..start]);
                let run = rest[start..].chars().take_while(|&c| c == '`').count();
                let (fence, after) = rest[start..].split_at(run);
                let Some(end) = after.find(fence) else {
                    return Err(format!(
                        "line {number}: a code span is not closed on its line"
                    ));
                };
                rest = &after[end + run..];
            }
            prose.push_str(rest);
        }
        prose.push('\n');
    }
    match fenced {
        Some(number) => Err(format!("line {number}: a code block is not closed")),
        None => Ok(prose),
    }
}

/// 文書の中のリンク `](相対パス)` の行き先があることを確かめる。節を別のファイルへ移したとき、移した文の相対リンクの
/// 深さが変わるため。
#[test]
fn every_linked_document_exists() {
    let root = repo_root();
    let mut broken = Vec::new();
    for file in sources(&root).into_iter().filter(|file| is_markdown(file)) {
        let text = match without_code(&fs::read_to_string(&file).unwrap()) {
            Ok(text) => text,
            Err(error) => {
                broken.push(format!("{}: {error}", relative(&root, &file)));
                continue;
            }
        };
        for (at, _) in text.match_indices("](") {
            let rest = &text[at + 2..];
            let Some(end) = rest.find(')') else {
                continue;
            };
            let link = rest[..end].split('#').next().unwrap();
            if link.is_empty() || link.contains("://") {
                continue;
            }
            if !file.parent().unwrap().join(link).exists() {
                broken.push(format!("{}: {}", relative(&root, &file), link));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "行き先のないリンクと、閉じないコード:\n{}",
        broken.join("\n")
    );
}

#[test]
fn unclosed_code_is_an_error() {
    assert_eq!(
        without_code("a\n```\nb](c)\n"),
        Err("line 2: a code block is not closed".to_string())
    );
    assert_eq!(
        without_code("a `b\nc` [d](e)\n"),
        Err("line 1: a code span is not closed on its line".to_string())
    );
    assert_eq!(
        without_code("a ``b ` c`` [d](e)\n```\nf](g)\n```\n"),
        Ok("a  [d](e)\n\n\n\n".to_string())
    );
}

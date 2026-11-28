//! 置き場所、層、信頼のテストのソースの走査。わざと`crates/eml_native/tests/placement.rs`の走査の2つ目の写しにしてあ
//! る。共有の補助を置くと、このcrateのMiriのビルドにコンパイラを引き込むからである。どの規則もコードだけを読む。まず
//! コメントと文字列のリテラルを空白にし、位置はすべて保つ。
use std::path::{Path, PathBuf};

/// スケジューラのファイル。docs/spec/runtime.mdの規則がこれを縛る。
pub const NEW: &[&str] = &[
    "counters.rs",
    "rank.rs",
    "job.rs",
    "policy.rs",
    "run.rs",
    "core.rs",
    "idle.rs",
    "hosting.rs",
    "pool.rs",
    "exec.rs",
    "drive.rs",
    "fork.rs",
    "monitor.rs",
    "script.rs",
    "sequence.rs",
    "sequence/main_worker.rs",
];

pub struct Source {
    /// crateの`src/`からの道。区切りは`/`である。
    pub path: String,
    /// 書いたままのファイル。
    pub text: String,
    /// コメントと文字列のリテラルを空白にし、位置をすべて保った文。
    pub blanked: String,
    /// 識別子の文字でない文字の隣の空白を除いた`blanked`。
    pub code: String,
}

pub fn crate_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join(name)
}

pub fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// crateの`src/`の下のすべての`.rs`のファイル。道の順に並べる。
pub fn sources(name: &str) -> Vec<Source> {
    let root = crate_dir(name).join("src");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            let blanked = blank(&text);
            Source {
                path: path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                code: squeeze(&blanked),
                text,
                blanked,
            }
        })
        .collect()
}

/// スケジューラのファイルのうち、今あるもの。
pub fn new_files() -> Vec<Source> {
    sources("eml_sched")
        .into_iter()
        .filter(|source| NEW.contains(&source.path.as_str()))
        .collect()
}

pub fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// コメント、文字列のリテラル、文字のリテラルを空白にし、改行は残す。生の文字列は扱わない。このcrateのソースのファイ
/// ルに生の文字列はない (`trust::no_source_file_has_a_raw_string`)。
pub fn blank(text: &str) -> String {
    let s: Vec<char> = text.chars().collect();
    let mut out = s.clone();
    let mut i = 0;
    let clear = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in &mut out[from..to.min(s.len())] {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };
    while i < s.len() {
        if s[i] == '/' && s.get(i + 1) == Some(&'/') {
            let end = (i..s.len()).find(|&j| s[j] == '\n').unwrap_or(s.len());
            clear(&mut out, i, end);
            i = end;
        } else if s[i] == '/' && s.get(i + 1) == Some(&'*') {
            let end = (i + 2..s.len() - 1)
                .find(|&j| s[j] == '*' && s[j + 1] == '/')
                .map_or(s.len(), |j| j + 2);
            clear(&mut out, i, end);
            i = end;
        } else if s[i] == '"' {
            let mut j = i + 1;
            while j < s.len() && s[j] != '"' {
                j += if s[j] == '\\' { 2 } else { 1 };
            }
            clear(&mut out, i, j + 1);
            i = j + 1;
        } else if s[i] == '\'' && s.get(i + 2) == Some(&'\'') {
            clear(&mut out, i, i + 3);
            i += 3;
        } else if s[i] == '\'' && s.get(i + 1) == Some(&'\\') {
            let end = (i + 2..s.len())
                .find(|&j| s[j] == '\'')
                .unwrap_or(s.len() - 1);
            clear(&mut out, i, end + 1);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}

/// 識別子の文字でない文字の隣の空白を除く。だからパターンは、rustfmtが行を折った所でも合う。
pub fn squeeze(code: &str) -> String {
    let s: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        if s[i].is_whitespace() {
            let end = (i..s.len())
                .find(|&j| !s[j].is_whitespace())
                .unwrap_or(s.len());
            let between_idents = out.chars().last().is_some_and(is_ident)
                && s.get(end).is_some_and(|&c| is_ident(c));
            if between_idents {
                out.push(' ');
            }
            i = end;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// `pattern`が`code`に現れる回数。`word`なら、前後が識別子の文字でない所だけを数える (`grep -w`)。
pub fn count(code: &str, pattern: &str, word: bool) -> usize {
    code.match_indices(pattern)
        .filter(|&(at, _)| {
            !word
                || (!code[..at].chars().next_back().is_some_and(is_ident)
                    && !code[at + pattern.len()..]
                        .chars()
                        .next()
                        .is_some_and(is_ident))
        })
        .count()
}

/// コードが`pattern`を持つファイル。
pub fn files_with(sources: &[Source], pattern: &str, word: bool) -> Vec<String> {
    sources
        .iter()
        .filter(|source| count(&source.code, pattern, word) > 0)
        .map(|source| source.path.clone())
        .collect()
}

/// `#[cfg(test)]`の下の項目を除いたコード。属性から、すぐ後の`;`か、対応する`}`までを除く。
pub fn outside_tests(code: &str) -> String {
    let mut out = String::new();
    let mut at = 0;
    while let Some(found) = code[at..].find("#[cfg(test)]").map(|n| at + n) {
        out.push_str(&code[at..found]);
        let rest = &code[found..];
        let end = match rest.find([';', '{']) {
            Some(n) if rest.as_bytes()[n] == b';' => n + 1,
            Some(n) => n + closing(&rest[n..]),
            None => rest.len(),
        };
        at = found + end;
    }
    out.push_str(&code[at..]);
    out
}

/// `text`の頭にある括弧の組の長さ。閉じる括弧を含む。
pub fn closing(text: &str) -> usize {
    let (open, close) = match text.chars().next() {
        Some('{') => ('{', '}'),
        Some('(') => ('(', ')'),
        Some('[') => ('[', ']'),
        other => panic!("not an opening bracket: {other:?}"),
    };
    let mut depth = 0;
    text.char_indices()
        .find_map(|(k, c)| {
            depth += i32::from(c == open) - i32::from(c == close);
            (depth == 0).then_some(k + c.len_utf8())
        })
        .expect("the brackets close")
}

/// 空白を詰めたコードで、名前が`name`の最初の関数の本体。波括弧は含まない。
pub fn body_of<'a>(code: &'a str, name: &str) -> &'a str {
    let at = code
        .match_indices(&format!("fn {name}"))
        .find(|&(at, pattern)| {
            !code[at + pattern.len()..]
                .chars()
                .next()
                .is_some_and(is_ident)
        })
        .map(|(at, _)| at)
        .unwrap_or_else(|| panic!("no function {name}"));
    let open = at + code[at..].find('{').unwrap();
    let len = closing(&code[open..]);
    &code[open + 1..open + len - 1]
}

/// ブロックの本体の文を、その深さの`;`で分けたもの。最後の式は最後に来る。
pub fn statements(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (k, c) in body.char_indices() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ';' if depth == 0 => {
                out.push(&body[start..k]);
                start = k + 1;
            }
            _ => {}
        }
    }
    if !body[start..].trim().is_empty() {
        out.push(&body[start..]);
    }
    out
}

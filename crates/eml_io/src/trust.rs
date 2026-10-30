//! `unsafe` の一覧のテスト (docs/implementation/testing.md の「`unsafe` の一覧」)。`unsafe` を書いてよいファイルを
//! ここで固定する。新しい `unsafe` が一覧の外に入ると、レビューを通らずに OS を呼ぶ道が増えるためである。

/// 調べるファイル。`src/` のこの module 以外をすべて載せる (`every_source_file_is_listed` が確かめる)。
/// `include_str!` は cfg に依らないので、別の OS 向けのファイルも読む。
const FILES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("lib.rs")),
    ("flag.rs", include_str!("flag.rs")),
    ("mask.rs", include_str!("mask.rs")),
    ("real.rs", include_str!("real.rs")),
    ("resolve.rs", include_str!("resolve.rs")),
    ("test.rs", include_str!("test.rs")),
    ("test/net.rs", include_str!("test/net.rs")),
];

/// 調べないファイル。この module だけである。
const SKIPPED: &[&str] = &["trust.rs"];

/// `unsafe` を持ち、先頭に `#![allow(unsafe_code)]` を置くファイル。
const UNSAFE_FILES: &[&str] = &["mask.rs", "test.rs", "real.rs"];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// コメント、文字列、文字のリテラルを空白に置き換える。位置を保つので、後の探し方がずれない。生の文字列
/// (`r"`、`r#"`) は扱わない。どのファイルにもないことは `the_scanner_skips_comments_and_strings` が確かめる。
fn blank_comments_and_strings(text: &str) -> Vec<char> {
    let s: Vec<char> = text.chars().collect();
    let mut out = s.clone();
    let mut i = 0;
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in &mut out[from..to] {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };
    while i < s.len() {
        if s[i] == '/' && s.get(i + 1) == Some(&'/') {
            let end = (i..s.len()).find(|&j| s[j] == '\n').unwrap_or(s.len());
            blank(&mut out, i, end);
            i = end;
        } else if s[i] == '/' && s.get(i + 1) == Some(&'*') {
            let end = (i + 2..s.len() - 1)
                .find(|&j| s[j] == '*' && s[j + 1] == '/')
                .map_or(s.len(), |j| j + 2);
            blank(&mut out, i, end);
            i = end;
        } else if s[i] == '"' {
            let mut j = i + 1;
            while j < s.len() && s[j] != '"' {
                j += if s[j] == '\\' { 2 } else { 1 };
            }
            blank(&mut out, i, (j + 1).min(s.len()));
            i = j + 1;
        } else if s[i] == '\'' && s.get(i + 2) == Some(&'\'') {
            blank(&mut out, i, i + 3);
            i += 3;
        } else if s[i] == '\'' && s.get(i + 1) == Some(&'\\') {
            let end = (i + 2..s.len())
                .find(|&j| s[j] == '\'')
                .unwrap_or(s.len() - 1);
            blank(&mut out, i, end + 1);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn prepare(text: &str) -> String {
    blank_comments_and_strings(text).into_iter().collect()
}

/// 識別子 `unsafe` が現れるか。`unsafe_code` のような長い識別子は数えない。
fn has_unsafe(text: &str) -> bool {
    let s: Vec<char> = prepare(text).chars().collect();
    let word: Vec<char> = "unsafe".chars().collect();
    s.windows(word.len()).enumerate().any(|(i, w)| {
        w == word.as_slice()
            && (i == 0 || !is_ident(s[i - 1]))
            && !s.get(i + word.len()).is_some_and(|&c| is_ident(c))
    })
}

/// 先頭の項目が `#![allow(unsafe_code)]` か。先頭のコメントは読み飛ばす。
fn starts_with_allow(text: &str) -> bool {
    prepare(text)
        .trim_start()
        .starts_with("#![allow(unsafe_code)]")
}

/// `allow(unsafe_code)` が現れる回数。空白を詰めて数える。
fn allow_count(text: &str) -> usize {
    let squeezed: String = prepare(text)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    squeezed.matches("allow(unsafe_code)").count()
}

#[test]
fn unsafe_stays_in_the_listed_files() {
    let with_unsafe: Vec<&str> = FILES
        .iter()
        .filter(|(_, text)| has_unsafe(text))
        .map(|(file, _)| *file)
        .collect();
    let with_allow: Vec<&str> = FILES
        .iter()
        .filter(|(_, text)| starts_with_allow(text))
        .map(|(file, _)| *file)
        .collect();
    let mut listed = UNSAFE_FILES.to_vec();
    listed.sort();
    let mut with_unsafe = with_unsafe;
    with_unsafe.sort();
    let mut with_allow = with_allow;
    with_allow.sort();
    assert_eq!(with_unsafe, listed, "unsafe appears outside the list");
    assert_eq!(
        with_allow, listed,
        "allow(unsafe_code) differs from the list"
    );
    for (file, text) in FILES {
        let expected = usize::from(UNSAFE_FILES.contains(file));
        assert_eq!(allow_count(text), expected, "{file}: allow(unsafe_code)");
    }
    let lib = FILES.iter().find(|(f, _)| *f == "lib.rs").unwrap().1;
    assert!(
        prepare(lib).contains("#![deny(unsafe_code)]"),
        "lib.rs lacks deny(unsafe_code)"
    );
}

#[test]
fn every_source_file_is_listed() {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files.sort();
    let mut listed: Vec<String> = FILES
        .iter()
        .map(|(f, _)| f.to_string())
        .chain(SKIPPED.iter().map(|f| f.to_string()))
        .collect();
    listed.sort();
    assert_eq!(files, listed);
}

#[test]
fn the_scanner_skips_comments_and_strings() {
    for text in [
        "// unsafe\n",
        "/* unsafe */\n",
        "const S: &str = \"unsafe\";\n",
        "const C: char = '\"'; // unsafe\n",
        "fn f<'a>(x: &'a str) {}\n",
        "#![deny(unsafe_code)]\n",
        "const S: &str = \"a \\\" unsafe\";\n",
    ] {
        assert!(!has_unsafe(text), "{text:?}");
    }
    assert!(has_unsafe("fn f() { unsafe { g() } }\n"));
    assert!(has_unsafe("const C: char = '\"'; unsafe fn f() {}\n"));
    assert!(has_unsafe("fn f<'a>() { unsafe { g() } }\n"));
    assert!(!starts_with_allow("// c\n#![deny(unsafe_code)]\n"));
    assert!(starts_with_allow("//! doc\n#![allow(unsafe_code)]\n"));
    assert_eq!(
        allow_count("// allow(unsafe_code)\n\"allow(unsafe_code)\""),
        0
    );
    // 生の文字列は扱わないので、どのファイルにもあってはならない
    for (file, text) in FILES {
        assert!(
            !text.contains("r\"") && !text.contains("r#\""),
            "{file} has a raw string"
        );
    }
}

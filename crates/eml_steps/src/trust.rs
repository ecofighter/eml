//! `eml_steps`の信頼の検査 (docs/implementation/testing.mdの「`eml_steps`の検証」)。crateは信頼の項目も`unsafe`も持たず、
//! コードはすべて`verus!`の中にある。そうでないと、証明が空でも検証が通ってしまう。

/// 調べるファイル。`src/`のこのファイル以外のすべてを載せる (`every_source_file_is_listed`が確かめる)。
const FILES: &[(&str, &str)] = &[
    ("idle.rs", include_str!("idle.rs")),
    ("lib.rs", include_str!("lib.rs")),
    ("limit.rs", include_str!("limit.rs")),
    ("open.rs", include_str!("open.rs")),
    ("plan.rs", include_str!("plan.rs")),
    ("sleepers.rs", include_str!("sleepers.rs")),
    ("verdict.rs", include_str!("verdict.rs")),
];

/// Verusに確かめさせない項目を表す語と、`unsafe`。
const PATTERNS: &[&str] = &[
    "unsafe",
    "external_body",
    "external_fn_specification",
    "external_type_specification",
    "external_trait_specification",
    "verifier::external",
    "verifier(external",
    "verus_keep_ghost",
    "verus_only",
    "assume_specification",
    "exec_allows_no_decreases_clause",
    "uninterp",
    "axiom",
    "global",
    "assume (",
    "admit (",
    "Tracked::assume_new",
    "Ghost::assume_new",
];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `s[open]`の括弧に対応する閉じ括弧の位置。
fn matching(s: &[char], open: usize) -> usize {
    let (o, c) = match s[open] {
        '{' => ('{', '}'),
        '(' => ('(', ')'),
        '[' => ('[', ']'),
        other => panic!("not a bracket: {other}"),
    };
    let mut depth = 0usize;
    for (i, &ch) in s.iter().enumerate().skip(open) {
        if ch == o {
            depth += 1;
        } else if ch == c {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    panic!("unbalanced brackets");
}

fn blank(s: &mut [char]) {
    for c in s {
        if *c != '\n' {
            *c = ' ';
        }
    }
}

/// コメント、文字列、文字のリテラルを空白に置き換える。位置を保つので、後の探し方がずれない。
fn blank_comments_and_strings(text: &str) -> Vec<char> {
    let s: Vec<char> = text.chars().collect();
    let mut out = s.clone();
    let mut i = 0;
    while i < s.len() {
        if s[i] == '/' && s.get(i + 1) == Some(&'/') {
            let end = (i..s.len()).find(|&j| s[j] == '\n').unwrap_or(s.len());
            blank(&mut out[i..end]);
            i = end;
        } else if s[i] == '/' && s.get(i + 1) == Some(&'*') {
            let end = (i + 2..s.len() - 1)
                .find(|&j| s[j] == '*' && s[j + 1] == '/')
                .map_or(s.len(), |j| j + 2);
            blank(&mut out[i..end]);
            i = end;
        } else if s[i] == '"' {
            let mut j = i + 1;
            while j < s.len() && s[j] != '"' {
                j += if s[j] == '\\' { 2 } else { 1 };
            }
            blank(&mut out[i..=j]);
            i = j + 1;
        } else if s[i] == '\'' && s.get(i + 2) == Some(&'\'') {
            blank(&mut out[i..i + 3]);
            i += 3;
        } else if s[i] == '\'' && s.get(i + 1) == Some(&'\\') {
            // `'\''`の2つ目の`'`は閉じでないので、閉じは3文字目より後から探す
            let end = (i + 3..s.len())
                .find(|&j| s[j] == '\'')
                .unwrap_or(s.len() - 1);
            blank(&mut out[i..=end]);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn starts_with_at(s: &[char], at: usize, word: &str) -> bool {
    let mut rest = s.get(at..).unwrap_or_default().iter();
    word.chars().all(|c| rest.next() == Some(&c))
}

/// `word`の語が`at`から始まり、前が識別子の続きでないか。
fn word_at(s: &[char], at: usize, word: &str) -> bool {
    starts_with_at(s, at, word) && (at == 0 || !is_ident(s[at - 1]))
}

fn skip_space(s: &[char], mut at: usize) -> usize {
    while at < s.len() && s[at].is_whitespace() {
        at += 1;
    }
    at
}

/// `#[cfg(test)]`の後のmoduleを、本体を持つものも`;`で終わるものも空白に置き換える。
fn blank_test_modules(mut s: Vec<char>) -> Vec<char> {
    let marker = "#[cfg(test)]";
    let mut i = 0;
    while i < s.len() {
        if !starts_with_at(&s, i, marker) {
            i += 1;
            continue;
        }
        let at = skip_space(&s, i + marker.len());
        if !word_at(&s, at, "mod") {
            i += marker.len();
            continue;
        }
        let stop = (at..s.len())
            .find(|&j| s[j] == '{' || s[j] == ';')
            .expect("a module body");
        let end = if s[stop] == '{' {
            matching(&s, stop) + 1
        } else {
            stop + 1
        };
        blank(&mut s[i..end]);
        i = end;
    }
    s
}

/// テストでないコードの文字。コメント、文字列、`#[cfg(test)]`のmoduleを空白にしてある。
fn prepare(text: &str) -> Vec<char> {
    blank_test_modules(blank_comments_and_strings(text))
}

/// テストでないコードに現れる`PATTERNS`の語。`(`で終わる語は、`(`の前に空白があっても見つける。
fn trust_words(text: &str) -> Vec<&'static str> {
    let s = prepare(text);
    PATTERNS
        .iter()
        .copied()
        .filter(|pattern| {
            let (word, call) = match pattern.strip_suffix(" (") {
                Some(word) => (word, true),
                None => (*pattern, false),
            };
            (0..s.len()).any(|at| {
                word_at(&s, at, word)
                    && (!call || s.get(skip_space(&s, at + word.len())) == Some(&'('))
            })
        })
        .collect()
}

/// `verus!`の外のコード。`verus!`のブロックと属性はすべて空白にしてある。
fn outside_verus(text: &str) -> String {
    let mut s = prepare(text);
    let mut i = 0;
    while i < s.len() {
        let open = if word_at(&s, i, "verus!") {
            Some(skip_space(&s, i + "verus!".len()))
        } else if starts_with_at(&s, i, "#[") {
            Some(i + 1)
        } else if starts_with_at(&s, i, "#![") {
            Some(i + 2)
        } else {
            None
        };
        if let Some(open) = open {
            let end = matching(&s, open);
            blank(&mut s[i..=end]);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    s.into_iter().collect()
}

#[test]
fn every_source_file_is_listed() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .inspect(|path| assert!(!path.is_dir(), "{} is a directory", path.display()))
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .filter(|file| file != "trust.rs")
        .collect();
    files.sort();
    let mut listed: Vec<String> = FILES.iter().map(|(f, _)| f.to_string()).collect();
    listed.sort();
    assert_eq!(files, listed);
}

#[test]
fn the_crate_is_inside_verus() {
    for (file, text) in FILES {
        for line in outside_verus(text).lines().map(str::trim) {
            let allowed = line.is_empty()
                || (line.starts_with("use ") && line.ends_with(';'))
                || ((line.starts_with("mod ") || line.starts_with("pub mod "))
                    && line.ends_with(';'));
            assert!(allowed, "{file} has code outside verus!: {line}");
        }
    }
}

#[test]
fn no_trust_item_and_no_unsafe() {
    for (file, text) in FILES {
        assert_eq!(trust_words(text), Vec::<&str>::new(), "{file}");
    }
}

#[test]
fn the_scanner_finds_what_it_must_and_skips_comments_strings_and_tests() {
    let text = "// external_body\nconst S: &str = \"assume(x)\";\nconst Q: char = '\\\"';\nverus! {\nfn f() { assume (false); unsafe { g() } }\n#[cfg(verus_only)]\nfn h() {}\n}\nfn k() {}\n#[cfg(test)]\nmod tests {\n    #[verifier::external_body]\n    fn t() {}\n}\n";
    assert_eq!(trust_words(text), ["unsafe", "verus_only", "assume ("]);
    let outside: Vec<String> = outside_verus(text)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(
        outside,
        ["const S: &str = ;", "const Q: char = ;", "fn k() {}"]
    );
}

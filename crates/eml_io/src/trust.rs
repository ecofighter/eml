//! `unsafe` の一覧のテスト (docs/implementation/testing.md の「`unsafe` の一覧」)。`unsafe` を書いてよいファイルを
//! ここで固定する。新しい `unsafe` が一覧の外に入ると、レビューを通らずに OS を呼ぶ道が増えるためである。

/// 調べるファイル。`src/` のこの module 以外をすべて載せる (`every_source_file_is_listed` が確かめる)。
/// `include_str!` は cfg に依らないので、別の OS 向けのファイルも読む。
const FILES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("lib.rs")),
    ("flag.rs", include_str!("flag.rs")),
    ("mask.rs", include_str!("mask.rs")),
    ("real.rs", include_str!("real.rs")),
    ("driver/mod.rs", include_str!("driver/mod.rs")),
    ("driver/backend.rs", include_str!("driver/backend.rs")),
    ("driver/inbox.rs", include_str!("driver/inbox.rs")),
    ("driver/pool.rs", include_str!("driver/pool.rs")),
    ("driver/resolve.rs", include_str!("driver/resolve.rs")),
    ("driver/tests/mod.rs", include_str!("driver/tests/mod.rs")),
    ("driver/tests/fake.rs", include_str!("driver/tests/fake.rs")),
    (
        "driver/tests/abort.rs",
        include_str!("driver/tests/abort.rs"),
    ),
    ("resolve.rs", include_str!("resolve.rs")),
    ("sys/mod.rs", include_str!("sys/mod.rs")),
    ("sys/unix.rs", include_str!("sys/unix.rs")),
    ("sys/epoll.rs", include_str!("sys/epoll.rs")),
    ("sys/kqueue.rs", include_str!("sys/kqueue.rs")),
    ("test.rs", include_str!("test.rs")),
    ("test/net.rs", include_str!("test/net.rs")),
];

/// 調べないファイル。この module だけである。
const SKIPPED: &[&str] = &["trust.rs"];

/// `unsafe` を持ち、先頭に `#![allow(unsafe_code)]` を置くファイル。
const UNSAFE_FILES: &[&str] = &[
    "mask.rs",
    "test.rs",
    "real.rs",
    "sys/unix.rs",
    "sys/epoll.rs",
    "sys/kqueue.rs",
    "driver/tests/fake.rs",
];

/// `the_real_driver_makes_no_slices_and_leaks_nothing` と `the_core_names_no_os` が見ないファイル。compio の上の
/// 本物の driver で、compio の buffer の trait のために slice を作り、壊れたときに鍵を漏らす。
const LEGACY: &[&str] = &["real.rs"];

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
    assert!(has_raw_string("let s = r\"x\";"));
    assert!(has_raw_string("f(br#\"x\"#)"));
    assert!(!has_raw_string("let s = \"driver\";"));
    assert!(!has_raw_string("let r#type = 1;"));
    // 生の文字列は扱わないので、どのファイルにもあってはならない
    for (file, text) in FILES {
        assert!(!has_raw_string(text), "{file} has a raw string");
    }
}

/// `r"`、`r#"` (と `b` の付いた形) を探す。`r#type` のような生の識別子は数えない。`"driver"` のように識別子の文字の後の `r"` は数えない。
fn has_raw_string(text: &str) -> bool {
    let s = text.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    (0..s.len()).any(|i| {
        let hashes = s[i + 1..].iter().take_while(|&&c| c == b'#').count();
        let opens = s[i] == b'r' && s.get(i + 1 + hashes) == Some(&b'"');
        let start = if i > 0 && s[i - 1] == b'b' { i - 1 } else { i };
        opens && (start == 0 || !ident(s[start - 1]))
    })
}

fn is_legacy(file: &str) -> bool {
    LEGACY.contains(&file)
}

/// 核のファイル。OS を名指さず、偽の backend の上で Miri で流せる部分である。
fn is_core(file: &str) -> bool {
    file.starts_with("driver/") && !file.starts_with("driver/tests/")
}

#[test]
fn the_real_driver_makes_no_slices_and_leaks_nothing() {
    for (file, text) in FILES.iter().filter(|(f, _)| !is_legacy(f)) {
        let text = prepare(text);
        for word in ["mem::forget", "Box::leak"] {
            assert!(!text.contains(word), "{file} has {word}");
        }
        // テストの driver は貸しの規則を守って slice を作る (docs/spec/vm.md の「時計と driver」)
        if *file != "test.rs" {
            assert!(!text.contains("from_raw_parts"), "{file} makes a slice");
        }
    }
}

#[test]
fn the_core_names_no_os() {
    let core: Vec<&(&str, &str)> = FILES.iter().filter(|(f, _)| is_core(f)).collect();
    assert!(core.len() >= 5, "the core files are missing");
    for (file, text) in core {
        let text = prepare(text);
        for word in [
            "libc::",
            "windows_sys::",
            "Instant",
            "thread::spawn",
            "Builder::spawn",
            "raw_os_error",
            "ECANCELED",
            "ECONNABORTED",
            "EAGAIN",
        ] {
            assert!(!text.contains(word), "{file} names {word}");
        }
        // スレッドを作る道は `Backend::spawn` だけである
        let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        for (at, _) in squeezed.match_indices(".spawn(") {
            assert!(
                squeezed[..at].ends_with("backend"),
                "{file} spawns a thread outside the backend"
            );
        }
    }
}

/// readiness の backend は `Arrival::Retry`、`Hold::None`、`Cancel::Done` だけを使う。カーネルは同期の呼び出しの間しか
/// buffer に触れないので、暫定の完了型の値を返す理由がない。
#[test]
fn readiness_backends_report_only_retries() {
    for file in ["sys/unix.rs", "sys/epoll.rs", "sys/kqueue.rs"] {
        let (_, text) = FILES
            .iter()
            .find(|(f, _)| *f == file)
            .unwrap_or_else(|| panic!("{file} is not listed"));
        let text = prepare(text);
        for word in [
            "Arrival::Done",
            "Arrival::Cancelled",
            "Hold::Buffer",
            "Cancel::Pending",
        ] {
            assert!(!text.contains(word), "{file} uses {word}");
        }
    }
}

#[test]
fn the_core_walks_its_table_only_to_shut_down() {
    let text = FILES.iter().find(|(f, _)| *f == "driver/mod.rs").unwrap().1;
    let s = blank_comments_and_strings(text);
    // rustfmt は `self.slots.values()` を行に分けるので、空白を詰めた文で探し、元の位置で囲む関数を引く
    let (squeezed, origin): (Vec<char>, Vec<usize>) = s
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.is_whitespace())
        .map(|(i, c)| (*c, i))
        .unzip();
    let mut found = 0;
    for word in [
        "mem::take",
        "slots.iter",
        "slots.values",
        "slots.keys",
        "slots.drain",
    ] {
        let word: Vec<char> = word.chars().collect();
        for (at, _) in squeezed
            .windows(word.len())
            .enumerate()
            .filter(|(_, w)| *w == word.as_slice())
        {
            let name = enclosing_fn(&s, origin[at]);
            let word: String = word.iter().collect();
            assert!(
                name == "quiesce" || name == "drop",
                "driver/mod.rs: {word} in {name}"
            );
            found += 1;
        }
    }
    // `quiesce` と `drop` の表の歩きをすべて見つけていなければ、探し方が規則を確かめていない
    assert!(found >= 4, "the scan found {found} walks of the table");
}

/// `pos` を囲むいちばん内側の `fn` の名前。
fn enclosing_fn(s: &[char], pos: usize) -> String {
    let mut best: Option<String> = None;
    for i in 0..pos {
        if !(word_at(s, i, "fn") && s.get(i + 2).is_some_and(|c| c.is_whitespace())) {
            continue;
        }
        let Some(open) = (i..s.len()).find(|&j| s[j] == '{' || s[j] == ';') else {
            continue;
        };
        if s[open] == ';' || open > pos || matching(s, open) < pos {
            continue;
        }
        let at = skip_space(s, i + 2);
        let end = (at..s.len()).find(|&j| !is_ident(s[j])).unwrap_or(s.len());
        best = Some(s[at..end].iter().collect());
    }
    best.expect("a use outside any fn")
}

fn matching(s: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, &c) in s.iter().enumerate().skip(open) {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    panic!("unbalanced braces");
}

fn word_at(s: &[char], at: usize, word: &str) -> bool {
    let mut rest = s.get(at..).unwrap_or_default().iter();
    word.chars().all(|c| rest.next() == Some(&c)) && (at == 0 || !is_ident(s[at - 1]))
}

fn skip_space(s: &[char], mut at: usize) -> usize {
    while at < s.len() && s[at].is_whitespace() {
        at += 1;
    }
    at
}

//! 信頼の一覧のテスト (docs/implementation/testing.md の「Verus」)。Verus が確かめない項目 (`external_body` や
//! `assume_specification` など) を、テストでないコードから探し、下の一覧と同じであることを確かめる。属性や仮定が
//! 一覧の外に増えると、証明が空になっても検証は通ってしまうためである。

/// 調べるファイル。`src/` のテストでないファイルをすべて載せる (`every_source_file_is_listed` が確かめる)。
const FILES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("lib.rs")),
    ("alloc.rs", include_str!("alloc.rs")),
    ("cell.rs", include_str!("cell.rs")),
    ("control.rs", include_str!("control.rs")),
    ("error.rs", include_str!("error.rs")),
    ("externs.rs", include_str!("externs.rs")),
    ("foreign.rs", include_str!("foreign.rs")),
    ("frozen.rs", include_str!("frozen.rs")),
    ("host.rs", include_str!("host.rs")),
    ("meta.rs", include_str!("meta.rs")),
    ("mode.rs", include_str!("mode.rs")),
    ("rc.rs", include_str!("rc.rs")),
    ("text.rs", include_str!("text.rs")),
    ("word.rs", include_str!("word.rs")),
    ("heap/mod.rs", include_str!("heap/mod.rs")),
    ("heap/chain.rs", include_str!("heap/chain.rs")),
    ("heap/check.rs", include_str!("heap/check.rs")),
    ("heap/core.rs", include_str!("heap/core.rs")),
    ("heap/ctx.rs", include_str!("heap/ctx.rs")),
    ("heap/ctx/arrays.rs", include_str!("heap/ctx/arrays.rs")),
    ("heap/ctx/bytes.rs", include_str!("heap/ctx/bytes.rs")),
    ("heap/ctx/io.rs", include_str!("heap/ctx/io.rs")),
    (
        "heap/ctx/positions.rs",
        include_str!("heap/ctx/positions.rs"),
    ),
    ("heap/ctx/proc.rs", include_str!("heap/ctx/proc.rs")),
    ("heap/ctx/proof.rs", include_str!("heap/ctx/proof.rs")),
    ("heap/ctx/recover.rs", include_str!("heap/ctx/recover.rs")),
    ("heap/ctx/roots.rs", include_str!("heap/ctx/roots.rs")),
    ("heap/ctx/script.rs", include_str!("heap/ctx/script.rs")),
    ("heap/ctx/strings.rs", include_str!("heap/ctx/strings.rs")),
    ("heap/field.rs", include_str!("heap/field.rs")),
    ("heap/graph.rs", include_str!("heap/graph.rs")),
    ("heap/handover.rs", include_str!("heap/handover.rs")),
    ("heap/model.rs", include_str!("heap/model.rs")),
    ("heap/inv.rs", include_str!("heap/inv.rs")),
    ("heap/proof.rs", include_str!("heap/proof.rs")),
    ("heap/recover.rs", include_str!("heap/recover.rs")),
    ("heap/states.rs", include_str!("heap/states.rs")),
    ("heap/string.rs", include_str!("heap/string.rs")),
    ("heap/release.rs", include_str!("heap/release.rs")),
    ("heap/reuse.rs", include_str!("heap/reuse.rs")),
    ("heap/safe.rs", include_str!("heap/safe.rs")),
    ("heap/share.rs", include_str!("heap/share.rs")),
    ("heap/transfer.rs", include_str!("heap/transfer.rs")),
    ("heap/walk.rs", include_str!("heap/walk.rs")),
];

/// 調べないファイル。この module と、`#[cfg(test)]` の module のファイルである。
const SKIPPED: &[&str] = &["trust.rs", "heap/tests/"];

/// Verus に確かめさせない項目を表す語。
const PATTERNS: &[&str] = &[
    "external_body",
    "external_fn_specification",
    "external_type_specification",
    "external_trait_specification",
    "verifier::external",
    "assume_specification",
    "exec_allows_no_decreases_clause",
    "uninterp",
    "axiom",
    "global",
    "assume(",
    "admit(",
    "Tracked::assume_new",
    "Ghost::assume_new",
];

/// 信頼の一覧 (docs/spec/object-model.md の「`unsafe` の核」)。項目と、その表の番号の組である。
const TRUSTED: &[(&str, &str)] = &[
    (
        "word.rs: assume_specification<T>[core::ptr::without_provenance_mut::<T>]",
        "T5",
    ),
    ("word.rs: external_body not_an_immediate", "T5"),
    ("word.rs: global size_of usize == 8", "T6"),
    ("meta.rs: global layout Word is size == 8, align == 8", "T6"),
    (
        "meta.rs: global layout Header is size == 8, align == 4",
        "T6",
    ),
    ("meta.rs: global layout u32 is size == 4, align == 4", "T6"),
    ("meta.rs: global layout i32 is size == 4, align == 4", "T6"),
    (
        "alloc.rs: global layout PageHeader is size == 32, align == 8",
        "T6",
    ),
    ("alloc.rs: external_body alloc_page", "T1"),
    ("alloc.rs: Tracked::assume_new alloc_page", "T1"),
    ("alloc.rs: external_body dealloc_page", "T1"),
    ("alloc.rs: external_body charge_pages", "T1"),
    ("alloc.rs: external_body refund_pages", "T1"),
    ("alloc.rs: external_body over_budget", "T1"),
    ("alloc.rs: external_body too_large", "T5"),
    ("alloc.rs: external_body too_many_words", "T5"),
    ("alloc.rs: external_body bad_page_layout", "T5"),
    ("heap/check.rs: external_body too_many_acquires", "T5"),
    ("heap/mod.rs: external_body too_many_workers", "T5"),
    ("heap/mod.rs: external_body bad_retire", "T5"),
    ("frozen.rs: external_body same_chunk", "T5"),
    ("frozen.rs: external_body not_a_descendant", "T5"),
    ("heap/ctx/strings.rs: external_body bytes_cmp", "T5"),
    ("heap/ctx/bytes.rs: external_body utf8_valid", "T5"),
    ("foreign.rs: verifier::external VTable", "T3"),
    ("foreign.rs: verifier::external Table", "T3"),
    ("foreign.rs: verifier::external drop_value", "T3"),
    ("foreign.rs: external_body value_words", "T3"),
    ("foreign.rs: external_body write", "T3"),
    ("foreign.rs: external_body value", "T3"),
    ("foreign.rs: external_body Finalizer", "T3"),
    ("foreign.rs: external_body finalizer_of", "T3"),
    ("foreign.rs: external_body run_finalizer", "T3"),
    ("cell.rs: external_body rc_load", "T2"),
    ("cell.rs: external_body rc_store", "T2"),
    ("cell.rs: external_body rc_dup_shared", "T2"),
    ("cell.rs: external_body rc_decref_shared", "T2"),
    ("cell.rs: external_body rc_is_last", "T2"),
    ("cell.rs: external_body u32_load", "T2"),
    ("cell.rs: external_body u32_store", "T2"),
    ("text.rs: external_body write_bytes", "T10"),
    ("text.rs: external_body write_byte", "T10"),
    ("text.rs: external_body bytes", "T10"),
    ("text.rs: external_body bytes_ptr", "T10"),
    ("host.rs: external_body HostRef", "T11"),
    ("host.rs: external_body clone", "T11"),
    ("host.rs: external_body OpenFlags", "T11"),
    ("host.rs: external_body host_write_line", "T11"),
    ("host.rs: external_body host_write", "T11"),
    ("host.rs: external_body host_flush", "T11"),
    ("host.rs: external_body FileOut", "T11"),
    ("host.rs: external_body host_open_write", "T11"),
    ("host.rs: external_body file_out_write", "T11"),
    ("host.rs: external_body file_out_flush", "T11"),
    ("host.rs: external_body FileIn", "T11"),
    ("host.rs: external_body host_open", "T11"),
    ("host.rs: external_body Reader", "T11"),
    ("host.rs: external_body StdinSlot", "T11"),
    ("host.rs: external_body fill", "T11"),
    ("host.rs: external_body consume", "T11"),
    ("host.rs: external_body read_text", "T11"),
    ("host.rs: external_trait_specification ExSource", "T11"),
    ("host.rs: external_body StdinHandle", "T11"),
    ("host.rs: external_body open_stdin", "T11"),
    ("host.rs: external_body open_stream", "T11"),
    ("host.rs: external_body host_arg_count", "T11"),
    ("host.rs: external_body host_arg", "T11"),
    ("host.rs: external_body host_env_has", "T11"),
    ("host.rs: external_body host_env_var", "T11"),
    ("host.rs: external_body Child", "T11"),
    ("host.rs: external_body host_spawn", "T11"),
    ("host.rs: external_body child_write", "T11"),
    ("host.rs: external_body child_close_stdin", "T11"),
    ("host.rs: external_body child_wait", "T11"),
    ("externs.rs: external_body float_add", "T12"),
    ("externs.rs: external_body float_sub", "T12"),
    ("externs.rs: external_body float_mul", "T12"),
    ("externs.rs: external_body float_div", "T12"),
    ("externs.rs: external_body float_rem", "T12"),
    ("externs.rs: external_body float_negate", "T12"),
    ("externs.rs: external_body float_compare", "T12"),
    ("externs.rs: external_body canonical_nan", "T12"),
    ("externs.rs: external_body float_of_int", "T12"),
    ("externs.rs: external_body float_truncate", "T12"),
    ("externs.rs: external_body float_floor", "T12"),
    ("externs.rs: external_body float_ceil", "T12"),
    ("externs.rs: external_body float_round", "T12"),
    ("externs.rs: external_body float_to_int", "T12"),
    ("externs.rs: external_body float_sqrt", "T12"),
    ("externs.rs: external_body float_abs", "T12"),
    ("externs.rs: external_body float_sin", "T12"),
    ("externs.rs: external_body float_cos", "T12"),
    ("externs.rs: external_body float_exp", "T12"),
    ("externs.rs: external_body float_log", "T12"),
    ("externs.rs: external_body float_pow", "T12"),
    ("externs.rs: external_body float_infinity", "T12"),
    ("externs.rs: external_body float_nan", "T12"),
    ("externs.rs: external_body float_shows_minus", "T12"),
    ("externs.rs: external_body float_repr", "T12"),
    ("externs.rs: external_body float_fixed", "T12"),
    ("externs.rs: external_body float_parse", "T12"),
    ("heap/ctx/positions.rs: external_body find_byte", "T13"),
    ("alloc.rs: Drop for Pages", "T4"),
    ("heap/walk.rs: Drop for Heap", "T4"),
    ("frozen.rs: Drop for Frozen", "T4"),
    ("host.rs: Drop for HostRef", "T4"),
    ("host.rs: Drop for StdinHandle", "T4"),
    ("host.rs: Drop for StdoutHandle", "T4"),
    ("host.rs: Drop for StderrHandle", "T4"),
    ("host.rs: Drop for Child", "T4"),
];

/// `verus!` の外でも書いてよい `unsafe` の始まり。`Send` と `Sync` の宣言と、検証した関数を呼ぶだけの `Drop` である。
const UNSAFE_OUTSIDE_ALLOWED: &[&str] = &[
    "unsafe impl Send for",
    "unsafe impl Sync for",
    "unsafe impl<M: Mode> Send for",
    "unsafe impl<M: Mode> Sync for",
];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `s[open]` の括弧に対応する閉じ括弧の位置。
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

/// コメント、文字列、文字のリテラルを空白に置き換える。位置を保つので、後の探し方がずれない。
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
            blank(&mut out, i, j + 1);
            i = j + 1;
        } else if s[i] == '\'' && s.get(i + 2) == Some(&'\'') {
            blank(&mut out, i, i + 3);
            i += 3;
        } else if s[i] == '\'' && s.get(i + 1) == Some(&'\\') {
            let end = (i + 2..s.len()).find(|&j| s[j] == '\'').unwrap_or(s.len());
            blank(&mut out, i, end + 1);
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

/// `word` の語が `at` から始まり、前が識別子の続きでないか。
fn word_at(s: &[char], at: usize, word: &str) -> bool {
    starts_with_at(s, at, word) && (at == 0 || !is_ident(s[at - 1]))
}

fn skip_space(s: &[char], mut at: usize) -> usize {
    while at < s.len() && s[at].is_whitespace() {
        at += 1;
    }
    at
}

/// `#[cfg(test)]` の後の `mod 名前 { … }` か `mod 名前;` を空白に置き換える。
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
        for c in &mut s[i..end] {
            if *c != '\n' {
                *c = ' ';
            }
        }
        i = end;
    }
    s
}

fn prepare(text: &str) -> Vec<char> {
    blank_test_modules(blank_comments_and_strings(text))
}

fn squeeze(s: &[char]) -> String {
    let joined: String = s.iter().collect();
    joined.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `from` の後に最初に来る `fn`、`struct`、`enum`、`trait`、`const` の名前。
fn next_item_name(s: &[char], from: usize) -> String {
    const KEYWORDS: &[&str] = &["fn", "struct", "enum", "trait", "const"];
    for i in from..s.len() {
        for keyword in KEYWORDS {
            if word_at(s, i, keyword) && s.get(i + keyword.len()).is_some_and(|c| c.is_whitespace())
            {
                let at = skip_space(s, i + keyword.len());
                // `const fn` は `fn` の側で名前を取る
                if *keyword == "const" && word_at(s, at, "fn") {
                    continue;
                }
                let end = (at..s.len()).find(|&j| !is_ident(s[j])).unwrap_or(s.len());
                return s[at..end].iter().collect();
            }
        }
    }
    panic!("no item after the attribute");
}

/// `pos` を本体に含む一番内側の `fn` の名前。
fn enclosing_fn(s: &[char], pos: usize) -> String {
    let mut best: Option<(usize, String)> = None;
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
        best = Some((i, s[at..end].iter().collect()));
    }
    best.map(|(_, name)| name).expect("a use outside any fn")
}

/// ファイルの中の信頼の項目。`ファイル: 語 名前` の形の文字列にする。
fn trusted_items(file: &str, text: &str) -> Vec<String> {
    let s = prepare(text);
    let mut found = Vec::new();
    for i in 0..s.len() {
        for pattern in PATTERNS {
            if !pattern.starts_with(s[i]) {
                continue;
            }
            let ident_start = pattern.chars().next().is_some_and(is_ident);
            if !starts_with_at(&s, i, pattern) || (ident_start && i > 0 && is_ident(s[i - 1])) {
                continue;
            }
            let after = i + pattern.chars().count();
            if pattern.chars().last().is_some_and(is_ident)
                && s.get(after).is_some_and(|&c| is_ident(c))
            {
                continue;
            }
            let item = match *pattern {
                "assume_specification" => {
                    let close = (after..s.len())
                        .find(|&j| s[j] == ']')
                        .expect("a spec target");
                    format!("{file}: {}", squeeze(&s[i..=close]).replace(' ', ""))
                }
                "global" => {
                    let end = (after..s.len()).find(|&j| s[j] == ';').expect("a global");
                    format!("{file}: {}", squeeze(&s[i..end]))
                }
                "assume(" | "admit(" | "Tracked::assume_new" | "Ghost::assume_new" => {
                    format!("{file}: {pattern} {}", enclosing_fn(&s, i))
                }
                _ => format!("{file}: {pattern} {}", next_item_name(&s, after)),
            };
            found.push(item);
        }
    }
    // `verus!` の外の `Drop` も、Verus が確かめない項目である (T4)
    for (open, _) in drop_impl_ranges(&s) {
        let header = squeeze(&s[..open]);
        let after = &header[header.rfind("Drop for ").expect("a Drop impl") + "Drop for ".len()..];
        let name: String = after.chars().take_while(|&c| is_ident(c)).collect();
        found.push(format!("{file}: Drop for {name}"));
    }
    found
}

/// `verus! { … }` の中の範囲。
fn verus_ranges(s: &[char]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if word_at(s, i, "verus!") {
            let open = (i..s.len()).find(|&j| s[j] == '{').expect("a verus! body");
            let close = matching(s, open);
            ranges.push((open, close));
            i = close;
        }
        i += 1;
    }
    ranges
}

/// `impl … Drop for …` の本体の範囲。
fn drop_impl_ranges(s: &[char]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for i in 0..s.len() {
        if word_at(s, i, "impl") {
            let open = (i..s.len())
                .find(|&j| s[j] == '{' || s[j] == ';')
                .unwrap_or(s.len());
            if open < s.len() && s[open] == '{' && squeeze(&s[i..open]).contains("Drop for") {
                ranges.push((open, matching(s, open)));
            }
        }
    }
    ranges
}

/// `verus!` の外にある、許していない `unsafe` の数。
fn unsafe_outside_verus(text: &str) -> usize {
    let s = prepare(text);
    let inside = verus_ranges(&s);
    let drops = drop_impl_ranges(&s);
    (0..s.len())
        .filter(|&i| word_at(&s, i, "unsafe") && !s.get(i + 6).is_some_and(|&c| is_ident(c)))
        .filter(|&i| !inside.iter().chain(&drops).any(|&(a, b)| a < i && i < b))
        .filter(|&i| {
            !UNSAFE_OUTSIDE_ALLOWED.iter().any(|allowed| {
                squeeze(&s[i..(i + allowed.len() + 8).min(s.len())]).starts_with(allowed)
            })
        })
        .count()
}

#[test]
fn the_trust_list_is_exactly_the_trusted_items() {
    let mut found: Vec<String> = FILES
        .iter()
        .flat_map(|(file, text)| trusted_items(file, text))
        .collect();
    // 1つの関数に同じ項目が2回現れても、一覧には1回だけ載せる
    found.sort();
    found.dedup();
    let mut listed: Vec<String> = TRUSTED.iter().map(|(item, _)| item.to_string()).collect();
    listed.sort();
    assert_eq!(
        found, listed,
        "the trusted items differ from the trust list"
    );
    for (item, number) in TRUSTED {
        assert!(
            [
                "T0", "T1", "T2", "T3", "T4", "T5", "T6", "T7", "T8", "T9", "T10", "T11", "T12",
                "T13"
            ]
            .contains(number),
            "{item} has no trust row"
        );
    }
}

/// `pos` の直前の空白でない文字の位置。
fn prev_non_space(s: &[char], pos: usize) -> Option<usize> {
    (0..pos).rev().find(|&j| !s[j].is_whitespace())
}

/// `end` (`.` の位置) で終わる式の頭。識別子、`.`、`@`、括弧の組を前へたどる。
fn expression_start(s: &[char], end: usize) -> usize {
    let mut i = end;
    while i > 0 {
        let c = s[i - 1];
        if is_ident(c) || c == '.' || c == '@' {
            i -= 1;
        } else if c == ')' || c == ']' {
            let close = if c == ')' { '(' } else { '[' };
            let mut depth = 0usize;
            loop {
                i -= 1;
                if s[i] == c {
                    depth += 1;
                } else if s[i] == close {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        } else {
            break;
        }
    }
    i
}

/// ghost のバイト列を変えうる箇所の数。`.text` への代入と `&mut` の借用、`BlockPerm` の構造体の式を数える。返り値の型
/// (`-> BlockPerm {`) と、`impl`、`struct` の宣言は式でないので数えない。
fn ghost_text_writes(text: &str) -> usize {
    let s = prepare(text);
    let mut n = 0;
    for i in 0..s.len() {
        if s[i] == '.'
            && starts_with_at(&s, i + 1, "text")
            && !s.get(i + 5).is_some_and(|&c| is_ident(c))
        {
            let next = skip_space(&s, i + 5);
            // `text()` は読むだけのメソッドである
            if s.get(next) == Some(&'(') {
                continue;
            }
            let assigns = s.get(next) == Some(&'=') && s.get(next + 1) != Some(&'=');
            let borrows = prev_non_space(&s, expression_start(&s, i))
                .is_some_and(|j| j >= 3 && squeeze(&s[..=j]).ends_with("&mut"));
            if assigns || borrows {
                n += 1;
            }
        }
        if word_at(&s, i, "BlockPerm")
            && !s.get(i + 9).is_some_and(|&c| is_ident(c))
            && s.get(skip_space(&s, i + 9)) == Some(&'{')
        {
            let declared = prev_non_space(&s, i).is_some_and(|j| {
                let word_start = (0..=j)
                    .rev()
                    .find(|&k| !is_ident(s[k]))
                    .map_or(0, |k| k + 1);
                (j >= 1 && s[j - 1] == '-' && s[j] == '>')
                    || ["impl", "struct", "for"].contains(&squeeze(&s[word_start..=j]).as_str())
            });
            if !declared {
                n += 1;
            }
        }
    }
    n
}

/// `BlockPerm` の ghost のバイト列を作るか書き換えるのは meta.rs だけである (docs/spec/object-model.md の
/// 「`unsafe` の核」の T10)。`Text` は meta.rs の外では作れないが、ほかの権限の列を写して代入するか、`&mut` で借りて
/// 書き換えるか、その列を入れた `BlockPerm` を組み立てることはできてしまうためである。
#[test]
fn only_meta_writes_the_ghost_text() {
    for (file, text) in FILES {
        if *file == "meta.rs" {
            continue;
        }
        assert_eq!(ghost_text_writes(text), 0, "{file} writes BlockPerm::text");
    }
}

#[test]
fn ghost_text_writes_are_found() {
    let text = "p.text = q.text;\nf(&mut p.text);\nlet r = &mut self.perm(h).text;\nlet p = BlockPerm { rc, meta, slots, prefix, text };\n";
    assert_eq!(ghost_text_writes(text), 4);
    let text = "fn perm(&self) -> BlockPerm {\n    p.text == q.text && p.text() == t\n}\nimpl BlockPerm {}\n// p.text = q.text\nf(&mut p.text())\n";
    assert_eq!(ghost_text_writes(text), 0);
}

#[test]
fn no_unsafe_outside_verus() {
    for (file, text) in FILES {
        assert_eq!(
            unsafe_outside_verus(text),
            0,
            "{file} has unsafe outside verus!"
        );
    }
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
    let mut checked: Vec<String> = files
        .into_iter()
        .filter(|f| !SKIPPED.iter().any(|skip| f.starts_with(skip)))
        .collect();
    checked.sort();
    let mut listed: Vec<String> = FILES.iter().map(|(f, _)| f.to_string()).collect();
    listed.sort();
    assert_eq!(checked, listed);
}

#[test]
fn comments_strings_and_test_modules_are_skipped() {
    let text = "// external_body\nconst S: &str = \"assume(x)\";\n#[cfg(test)]\nmod tests {\n    #[verifier::external_body]\n    fn t() {}\n}\n";
    assert_eq!(trusted_items("f.rs", text), Vec::<String>::new());
    let text = "verus! {\n#[verifier::external_body]\nfn f() { unsafe { g() } }\nfn h() { proof { assume(false); } }\n}\nfn k() { unsafe { g() } }\n";
    assert_eq!(
        trusted_items("f.rs", text),
        ["f.rs: external_body f", "f.rs: assume( h"]
    );
    assert_eq!(unsafe_outside_verus(text), 1);
    let text = "impl<M: Mode> Drop for Heap<M> {\n    fn drop(&mut self) {}\n}\n#[cfg(test)]\nmod tests {\n    impl Drop for Probe { fn drop(&mut self) {} }\n}\n";
    assert_eq!(trusted_items("f.rs", text), ["f.rs: Drop for Heap"]);
    let text =
        "unsafe impl Send for W {}\nimpl Drop for Heap { fn drop(&mut self) { unsafe { f() } } }\n";
    assert_eq!(unsafe_outside_verus(text), 0);
}

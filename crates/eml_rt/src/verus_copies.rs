//! Verus の写しが本体と同じコードであることを確かめる (docs/implementation/testing.md の「Verus」)。両方からコメントと属性と空白を除き、写しから
//! はさらに Verus だけの書き方を除いて比べる。

/// 比べる項目。名前は、ファイルの一番外の `fn`、`struct`、`enum`、`trait`、`const` か、`型::名前` で `impl 型` の中の
/// `fn` か `const` を指す。表に載らない型は、写しの側で証明に要る部分だけを持てばよい。
const COPIES: &[(&str, &str, &[&str])] = &[
    (
        include_str!("meta.rs"),
        include_str!("../verus/meta.rs"),
        &[
            "Kind",
            "Kind::from_bits",
            "Meta",
            "Meta::LIN",
            "Meta::KIND_SHIFT",
            "Meta::KIND_MASK",
            "Meta::new",
            "Meta::tag",
            "Meta::scan",
            "Meta::kind",
            "Meta::lin",
            "Meta::with_lin",
            "Meta::same_shape",
        ],
    ),
    (
        include_str!("word.rs"),
        include_str!("../verus/word.rs"),
        &[
            "Word::MIN_INT",
            "Word::MAX_INT",
            "fits_imm",
            "imm_bits",
            "is_imm_bits",
            "is_ptr_bits",
            "imm_value_bits",
        ],
    ),
    (
        include_str!("rc.rs"),
        include_str!("../verus/rc.rs"),
        &[
            "LOCAL_MAX",
            "DUP_LIMIT",
            "DROP_LIMIT",
            "DupStep",
            "DecrefStep",
            "dup_step",
            "decref_step",
            "mark_step",
        ],
    ),
    (
        include_str!("heap/share.rs"),
        include_str!("../verus/share.rs"),
        &["Graph", "ShareError", "share_walk"],
    ),
    (
        include_str!("heap/check.rs"),
        include_str!("../verus/check.rs"),
        &["layout_check", "reuse_in_place_check"],
    ),
];

/// 仕様の節の始まりの語。節は、その後の深さ 0 の `{` か `;` の前で終わる。そのため写しの節には `{` を書かない。
const CLAUSES: &[&str] = &[
    "requires",
    "ensures",
    "recommends",
    "invariant",
    "invariant_except_break",
    "decreases",
    "returns",
    "opens_invariants",
];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `at` から始まる語が `word` で、前後が識別子の続きでないか。
fn word_at(s: &[char], at: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    s.len() >= at + w.len()
        && s[at..at + w.len()] == w[..]
        && (at == 0 || !is_ident(s[at - 1]))
        && s.get(at + w.len()).is_none_or(|&c| !is_ident(c))
}

fn skip_space(s: &[char], mut at: usize) -> usize {
    while at < s.len() && s[at].is_whitespace() {
        at += 1;
    }
    at
}

/// `s[open]` の括弧に対応する閉じ括弧の位置。3種類の括弧をまとめて数える。
fn matching(s: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, &c) in s.iter().enumerate().skip(open) {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced brackets");
}

/// `from` から、深さ 0 の `stops` のどれかの位置 (なければ末尾)。
fn find_at_depth_0(s: &[char], from: usize, stops: &[char]) -> usize {
    let mut i = from;
    while i < s.len() {
        match s[i] {
            c if stops.contains(&c) => return i,
            '(' | '[' => i = matching(s, i) + 1,
            _ => i += 1,
        }
    }
    s.len()
}

/// 行のコメントと属性を除く。比べるコードは、文字列のリテラルの中に `//` も `#[` も書かない。
fn strip_comments_and_attributes(text: &str) -> Vec<char> {
    let mut s: Vec<char> = Vec::new();
    for line in text.lines() {
        let code = line.find("//").map_or(line, |i| &line[..i]);
        s.extend(code.chars());
        s.push('\n');
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == '#' {
            let mut j = skip_space(&s, i + 1);
            if s.get(j) == Some(&'!') {
                j = skip_space(&s, j + 1);
            }
            if s.get(j) == Some(&'[') {
                i = matching(&s, j) + 1;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// 項目の終わり: 深さ 0 の `;` か、最初の深さ 0 の `{` に対応する `}`。
fn item_end(s: &[char], from: usize) -> usize {
    let stop = find_at_depth_0(s, from, &['{', ';']);
    if s.get(stop) == Some(&'{') {
        matching(s, stop) + 1
    } else {
        stop + 1
    }
}

/// `s` の中で、`fn` などに続く名前 `name` の項目を、その行の始まりから取り出す。
fn find_item(s: &[char], name: &str) -> Option<(usize, usize)> {
    const KEYWORDS: &[&str] = &["fn", "struct", "enum", "trait", "const"];
    let mut depth = 0usize;
    for i in 0..s.len() {
        match s[i] {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth != 0 {
            continue;
        }
        for keyword in KEYWORDS {
            if word_at(s, i, keyword) {
                let at = skip_space(s, i + keyword.len());
                if word_at(s, at, name) {
                    let start = s[..i].iter().rposition(|&c| c == '\n').map_or(0, |p| p + 1);
                    return Some((start, item_end(s, i)));
                }
            }
        }
    }
    None
}

/// 写しの項目は `verus! { }` の中にあるので、その中身を一番外として扱う。
fn unwrap_verus(s: Vec<char>) -> Vec<char> {
    let Some(at) = (0..s.len())
        .find(|&i| word_at(&s, i, "verus") && s.get(skip_space(&s, i + 5)) == Some(&'!'))
    else {
        return s;
    };
    let open = find_at_depth_0(&s, at, &['{']);
    s[open + 1..matching(&s, open)].to_vec()
}

fn extract(text: &str, path: &str) -> Vec<char> {
    let s = unwrap_verus(strip_comments_and_attributes(text));
    let (scope, name) = match path.split_once("::") {
        Some((ty, name)) => (Some(ty), name),
        None => (None, path),
    };
    let body: &[char] = match scope {
        None => &s,
        Some(ty) => {
            let open = (0..s.len())
                .find(|&i| {
                    word_at(&s, i, "impl") && {
                        let at = skip_space(&s, i + 4);
                        word_at(&s, at, ty) && s.get(skip_space(&s, at + ty.len())) == Some(&'{')
                    }
                })
                .unwrap_or_else(|| panic!("no `impl {ty}`"));
            let open = find_at_depth_0(&s, open, &['{']);
            &s[open + 1..matching(&s, open)]
        }
    };
    let (start, end) = find_item(body, name).unwrap_or_else(|| panic!("no item `{path}`"));
    body[start..end].to_vec()
}

/// 写しから、仕様の節、`proof` の塊、`let ghost` の文、`spec fn` の宣言、戻り値の名前を除く。
fn strip_verus(s: &[char]) -> Vec<char> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if word_at(s, i, "proof") && s.get(skip_space(s, i + 5)) == Some(&'{') {
            i = matching(s, skip_space(s, i + 5)) + 1;
            continue;
        }
        if word_at(s, i, "let") && word_at(s, skip_space(s, i + 3), "ghost") {
            i = find_at_depth_0(s, i, &[';']) + 1;
            continue;
        }
        if word_at(s, i, "spec") && word_at(s, skip_space(s, i + 4), "fn") {
            // `pub open spec fn` の修飾も、行の始まりから除く
            while out.last().is_some_and(|&c: &char| c != '\n') {
                out.pop();
            }
            i = item_end(s, i);
            continue;
        }
        if let Some(clause) = CLAUSES.iter().find(|clause| word_at(s, i, clause)) {
            i = find_at_depth_0(s, i + clause.len(), &['{', ';']);
            continue;
        }
        if s[i] == '-' && s.get(i + 1) == Some(&'>') {
            let open = skip_space(s, i + 2);
            if s.get(open) == Some(&'(') {
                let mut j = skip_space(s, open + 1);
                let ident = j;
                while j < s.len() && is_ident(s[j]) {
                    j += 1;
                }
                let colon = skip_space(s, j);
                if j > ident && s.get(colon) == Some(&':') && s.get(colon + 1) != Some(&':') {
                    let close = matching(s, open);
                    out.extend(['-', '>', ' ']);
                    out.extend(&s[colon + 1..close]);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn squeeze(s: &[char]) -> String {
    s.iter().filter(|c| !c.is_whitespace()).collect()
}

/// 写しの比べる形。証明だけを置いた `else` の枝は、証明を除くと空になるので、それも除く。
fn normalize_copy(copy: &str, item: &str) -> String {
    squeeze(&strip_verus(&extract(copy, item))).replace("else{}", "")
}

#[test]
fn the_verus_copies_match_the_code() {
    for (code, copy, items) in COPIES {
        for item in *items {
            let expected = squeeze(&extract(code, item));
            let actual = normalize_copy(copy, item);
            assert_eq!(
                actual, expected,
                "the Verus copy of `{item}` differs from the code"
            );
        }
    }
}

#[test]
fn verus_only_syntax_is_removed() {
    let copy = "fn f(x: u8) -> (r: Result<(), E>)\n    requires\n        x > 0,\n    ensures\n        r is Ok,\n{\n    let ghost g = x;\n    proof {\n        assert(g == x);\n    }\n    loop\n        invariant\n            g == x,\n        decreases 0nat,\n    {\n        if x > 1 {\n            break;\n        } else {\n            proof {\n                assert(x == 1);\n            }\n        }\n    }\n    Ok(())\n}\n";
    let code = "fn f(x: u8) -> Result<(), E> {\n    loop {\n        if x > 1 {\n            break;\n        }\n    }\n    Ok(())\n}\n";
    assert_eq!(normalize_copy(copy, "f"), squeeze(&extract(code, "f")));
}

#[test]
fn trait_specs_and_spec_fns_are_removed() {
    let copy = "pub trait T: Sized {\n    type N: Copy;\n\n    spec fn view(&self) -> Seq<u8>;\n\n    fn get(&self, i: usize) -> (r: u8)\n        requires\n            i < self.view().len(),\n    ;\n}\n";
    let code = "/// 説明\npub trait T: Sized {\n    type N: Copy;\n    fn get(&self, i: usize) -> u8;\n}\n";
    assert_eq!(normalize_copy(copy, "T"), squeeze(&extract(code, "T")));
}

#[test]
fn items_in_an_impl_are_found_by_type_and_name() {
    let code = "#[derive(Clone)]\npub struct M(u32);\n\nimpl M {\n    const L: u32 = 1 << 31; // 説明\n    pub const fn new(x: u32) -> M {\n        M(x)\n    }\n}\n\nfn new() {}\n";
    assert_eq!(squeeze(&extract(code, "M::L")), "constL:u32=1<<31;");
    assert_eq!(
        squeeze(&extract(code, "M::new")),
        "pubconstfnnew(x:u32)->M{M(x)}"
    );
    assert_eq!(squeeze(&extract(code, "M")), "pubstructM(u32);");
    assert_eq!(squeeze(&extract(code, "new")), "fnnew(){}");
}

#[test]
#[should_panic(expected = "differs")]
fn a_changed_body_is_reported() {
    let copy = "fn f() -> (r: u8)\n    ensures\n        r == 1,\n{\n    1\n}\n";
    let code = "fn f() -> u8 {\n    2\n}\n";
    let expected = squeeze(&extract(code, "f"));
    let actual = normalize_copy(copy, "f");
    assert_eq!(
        actual, expected,
        "the Verus copy of `f` differs from the code"
    );
}

/// `word` を名前とする呼び出し (`word` の後に、空白を挟んでもよい `(` が続く) があるか。
fn calls(s: &[char], word: &str) -> bool {
    starts(s, word).any(|i| s.get(skip_space(s, i + word.len())) == Some(&'('))
}

/// 語 `word` が始まる位置。`word_at` は呼ぶたびに確保するので、先頭の文字で先に絞る。Miri で遅くなりすぎないためである。
fn starts<'a>(s: &'a [char], word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let first = word.chars().next();
    (0..s.len()).filter(move |&i| Some(s[i]) == first && word_at(s, i, word))
}

/// `assume_specification` の宣言のそれぞれについて、`(` の前までを空白を除いて返す。
fn assume_specifications(source: &str) -> Vec<String> {
    let s: Vec<char> = source.chars().collect();
    starts(&s, "assume_specification")
        .map(|i| squeeze(&s[i..find_at_depth_0(&s, i, &['('])]))
        .collect()
}

#[test]
fn calls_are_found_with_or_without_a_space() {
    let s: Vec<char> = "assume (x); admit(); assume_specification<T>[ f ](x)"
        .chars()
        .collect();
    assert!(calls(&s, "assume"));
    assert!(calls(&s, "admit"));
    let s: Vec<char> = "assume_specification<T>[ f ](x); let assumed = (1);"
        .chars()
        .collect();
    assert!(!calls(&s, "assume"));
}

/// 属性は比べる前に除かれるので、証明を空にする属性や `assume` が混ざっても、上のテストは通ってしまう。
/// 信頼してよいのは `<[T]>::contains` の約束の1つだけである。
#[test]
fn the_copies_trust_nothing_but_the_contains_spec() {
    const FORBIDDEN: &[&str] = &[
        "external_body",
        "verifier::external",
        "exec_allows_no_decreases_clause",
    ];
    const FORBIDDEN_CALLS: &[&str] = &["assume", "admit"];
    let sources = COPIES
        .iter()
        .map(|(_, copy, _)| *copy)
        .chain([include_str!("../verus/lib.rs")]);
    let mut trusted = Vec::new();
    for source in sources {
        for word in FORBIDDEN {
            assert!(!source.contains(word), "a Verus copy contains `{word}`");
        }
        let chars: Vec<char> = source.chars().collect();
        for word in FORBIDDEN_CALLS {
            assert!(!calls(&chars, word), "a Verus copy calls `{word}`");
        }
        trusted.extend(assume_specifications(source));
    }
    assert_eq!(
        trusted,
        ["assume_specification<T:PartialEq>[<[T]>::contains]"],
        "only `<[T]>::contains` may be trusted"
    );
}

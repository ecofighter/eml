//! 期待値の表の形。`eml_rt` のテストと `eml_interp` のテスト (`#[path]` で読む) が同じ表を使う。`std` だけを使う。
//! 表は `eml_interp` の側の出力から作る (docs/implementation/testing.md)。

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Int(i64),
    Bool(bool),
    /// 定数の文字列
    Const(String),
    /// 一意な文字列
    Unique(String),
    /// `dup` して渡す共有した文字列。残った参照は結果を読んだ後で手放す
    Shared(String),
    /// `file_root` からの相対パス
    Rel(String),
    /// 同じ名前を `file_root` につないだ絶対パス
    Abs(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Int(i64),
    Bool(bool),
    Order(&'static str),
    Str(String),
    /// `println` が書いた出力
    Out(String),
    /// 位置を除いた誤りの文言
    Err(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub name: String,
    pub args: Vec<Arg>,
    pub outcome: Outcome,
}

pub const TABLE: &str = include_str!("expected.txt");

/// 整数の端の値。0、±1、±2、`MIN`、`MIN+1`、`MAX`、`MAX-1`、±2³¹、±2⁶²
pub const EDGES: [i64; 13] = [
    0,
    1,
    -1,
    2,
    -2,
    i64::MIN,
    i64::MIN + 1,
    i64::MAX,
    i64::MAX - 1,
    1 << 31,
    -(1 << 31),
    1 << 62,
    -(1 << 62),
];

/// 文字列の端の値。複数バイトの文字、制御文字、エスケープする文字、C1 の制御文字、互いに前置きになる組を含む
pub const STRINGS: [&str; 17] = [
    "",
    "a",
    "ab",
    "b",
    "é",
    "日本",
    "😀",
    "a\u{0}",
    "\u{7f}",
    "\u{80}",
    "\u{85}",
    "\u{9f}",
    "\u{2028}",
    "\"q\"",
    "back\\slash",
    "tab\tnl\n",
    "\u{1}\u{1f}",
];

/// 整数を2つ取る行
pub const INT_BINARY: [&str; 12] = [
    "Prelude.+",
    "Prelude.-",
    "Prelude.*",
    "Prelude./",
    "Prelude.%",
    "Prelude.Eq Int.==",
    "Prelude.Eq Int.!=",
    "Prelude.Ord Int.compare",
    "Prelude.Ord Int.<",
    "Prelude.Ord Int.<=",
    "Prelude.Ord Int.>",
    "Prelude.Ord Int.>=",
];

/// 文字列を2つ取る行
pub const STRING_BINARY: [&str; 4] = [
    "Prelude.Eq String.==",
    "Prelude.Eq String.!=",
    "Prelude.Ord String.compare",
    "Prelude.++",
];

const SEED: u64 = 0x9E37_79B9_7F4A_7C15;
const RANDOM_INTS: usize = 50;
const RANDOM_STRINGS: usize = 30;

/// 種を固定した xorshift64。表を作り直しても同じ入力が並ぶ
struct Rng(u64);

impl Rng {
    fn new() -> Rng {
        Rng(SEED)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// 一様な 64 ビットではほとんどの和と積が桁あふれするので、ずらす幅も乱数にして小さい値も混ぜる
    fn int(&mut self) -> i64 {
        let x = self.next() as i64;
        x >> (self.next() % 64)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn random_ints(n: usize) -> Vec<i64> {
    let mut rng = Rng::new();
    (0..n).map(|_| rng.int()).collect()
}

fn random_strings() -> Vec<String> {
    let mut rng = Rng::new();
    (0..RANDOM_STRINGS)
        .map(|_| {
            let parts = 1 + rng.below(8);
            (0..parts)
                .map(|_| STRINGS[rng.below(STRINGS.len())])
                .collect()
        })
        .collect()
}

/// 表を読む。形の誤りは、どの行かを添えて panic する
pub fn cases() -> Vec<Case> {
    TABLE
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
        .map(|(i, l)| parse_line(l).unwrap_or_else(|e| panic!("expected.txt:{}: {e}", i + 1)))
        .collect()
}

fn parse_line(text: &str) -> Result<Case, String> {
    let fields: Vec<&str> = text.split('\t').collect();
    let (outcome, rest) = fields.split_last().ok_or("an empty line")?;
    let (name, args) = rest.split_first().ok_or("no extern name")?;
    let outcome = outcome
        .strip_prefix("=> ")
        .ok_or("the last field does not start with `=> `")?;
    Ok(Case {
        name: name.to_string(),
        args: args
            .iter()
            .map(|a| parse_arg(a))
            .collect::<Result<_, _>>()?,
        outcome: parse_outcome(outcome)?,
    })
}

fn parse_arg(text: &str) -> Result<Arg, String> {
    let (kind, value) = text
        .split_once(':')
        .ok_or_else(|| format!("an argument without a kind: {text}"))?;
    Ok(match kind {
        "i" => Arg::Int(parse_int(value)?),
        "b" => Arg::Bool(parse_bool(value)?),
        "s" => Arg::Const(quoted(value)?),
        "u" => Arg::Unique(quoted(value)?),
        "d" => Arg::Shared(quoted(value)?),
        "f" => Arg::Rel(quoted(value)?),
        "a" => Arg::Abs(quoted(value)?),
        _ => return Err(format!("an unknown argument kind: {text}")),
    })
}

fn parse_outcome(text: &str) -> Result<Outcome, String> {
    let (kind, value) = text
        .split_once(':')
        .ok_or_else(|| format!("a result without a kind: {text}"))?;
    Ok(match kind {
        "i" => Outcome::Int(parse_int(value)?),
        "b" => Outcome::Bool(parse_bool(value)?),
        "o" => Outcome::Order(match value {
            "LT" => "LT",
            "EQ" => "EQ",
            "GT" => "GT",
            _ => return Err(format!("an unknown ordering: {value}")),
        }),
        "s" => Outcome::Str(quoted(value)?),
        "out" => Outcome::Out(quoted(value)?),
        "err" => Outcome::Err(quoted(value)?),
        _ => return Err(format!("an unknown result kind: {text}")),
    })
}

fn parse_int(text: &str) -> Result<i64, String> {
    text.parse().map_err(|_| format!("not an integer: {text}"))
}

fn parse_bool(text: &str) -> Result<bool, String> {
    match text {
        "True" => Ok(true),
        "False" => Ok(false),
        _ => Err(format!("not a Bool: {text}")),
    }
}

fn quoted(text: &str) -> Result<String, String> {
    if text.len() < 2 || !text.starts_with('"') || !text.ends_with('"') {
        return Err(format!("not a quoted string: {text}"));
    }
    Ok(unquote(text))
}

/// 1つの場合を表の1行に書く (表を作るときに使う)
pub fn line(case: &Case) -> String {
    let mut fields = vec![case.name.clone()];
    fields.extend(case.args.iter().map(|a| match a {
        Arg::Int(v) => format!("i:{v}"),
        Arg::Bool(b) => format!("b:{}", bool_name(*b)),
        Arg::Const(s) => format!("s:{s:?}"),
        Arg::Unique(s) => format!("u:{s:?}"),
        Arg::Shared(s) => format!("d:{s:?}"),
        Arg::Rel(s) => format!("f:{s:?}"),
        Arg::Abs(s) => format!("a:{s:?}"),
    }));
    let outcome = match &case.outcome {
        Outcome::Int(v) => format!("i:{v}"),
        Outcome::Bool(b) => format!("b:{}", bool_name(*b)),
        Outcome::Order(o) => format!("o:{o}"),
        Outcome::Str(s) => format!("s:{s:?}"),
        Outcome::Out(s) => format!("out:{s:?}"),
        Outcome::Err(s) => format!("err:{s:?}"),
    };
    fields.push(format!("=> {outcome}"));
    fields.join("\t")
}

fn bool_name(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

/// Rust の `{:?}` で書いた文字列を戻す。`\"`、`\\`、`\'`、`\n`、`\t`、`\r`、`\0`、`\u{…}` を読む
pub fn unquote(text: &str) -> String {
    let inner = &text[1..text.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('u') => {
                assert_eq!(chars.next(), Some('{'), "`\\u` needs `{{` in {text}");
                let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let code = u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or_else(|| panic!("a bad `\\u{{{hex}}}` in {text}"));
                out.push(code);
            }
            other => panic!("an unknown escape `\\{other:?}` in {text}"),
        }
    }
    out
}

/// 表を作るための入力。結果の欄は空で、`eml_interp` の側が埋める
pub fn inputs() -> Vec<(String, Vec<Arg>)> {
    let mut inputs = Vec::new();
    let mut push = |name: &str, args: Vec<Arg>| inputs.push((name.to_string(), args));
    let ints = random_ints(2 * RANDOM_INTS);
    for name in INT_BINARY {
        for a in EDGES {
            for b in EDGES {
                push(name, vec![Arg::Int(a), Arg::Int(b)]);
            }
        }
        for pair in ints.chunks(2) {
            push(name, vec![Arg::Int(pair[0]), Arg::Int(pair[1])]);
        }
    }
    for name in ["Prelude.negate", "Prelude.Show Int.show"] {
        for a in EDGES.into_iter().chain(random_ints(RANDOM_INTS)) {
            push(name, vec![Arg::Int(a)]);
        }
    }
    for name in ["Prelude.Eq Bool.==", "Prelude.Eq Bool.!="] {
        for a in [false, true] {
            for b in [false, true] {
                push(name, vec![Arg::Bool(a), Arg::Bool(b)]);
            }
        }
    }
    let c = |s: &str| Arg::Const(s.to_string());
    for name in STRING_BINARY {
        for a in STRINGS {
            for b in STRINGS {
                push(name, vec![c(a), c(b)]);
            }
        }
    }
    // `++` の左が一意か、共有か、不死か
    push("Prelude.++", vec![Arg::Unique("ab".into()), c("cd")]);
    push("Prelude.++", vec![Arg::Shared("ab".into()), c("cd")]);
    push("Prelude.++", vec![c("ab"), Arg::Unique("cd".into())]);
    let strings: Vec<String> = STRINGS
        .iter()
        .map(|s| s.to_string())
        .chain(random_strings())
        .collect();
    for name in ["Prelude.Show String.show", "Prelude.println"] {
        for s in &strings {
            push(name, vec![c(s)]);
        }
    }
    let rel = |s: &str| vec![Arg::Rel(s.to_string())];
    for path in [
        "plain.txt",
        "empty.txt",
        "bad.bin",
        "big.txt",
        "dir",
        "locked.txt",
        "missing.txt",
        "a\u{0}b",
    ] {
        push("Std.Fs.read_all", rel(path));
    }
    push("Std.Fs.read_all", vec![Arg::Abs("plain.txt".into())]);
    push("Std.Fs.read_all2", rel("plain.txt"));
    inputs
}

/// ファイルを用意する。`locked.txt` を作れなかった (root で動いている) ら false を返す
pub fn fixtures(root: &Path) -> bool {
    std::fs::write(root.join("plain.txt"), "line 1\nline 2\n").unwrap();
    std::fs::write(root.join("empty.txt"), "").unwrap();
    std::fs::write(root.join("bad.bin"), [b'a', 0xff, 0xfe]).unwrap();
    // 1 MiB にして、64 KiB ずつの読みの区切りを何度もまたがせる
    std::fs::write(root.join("big.txt"), "abcdefg\n".repeat(131_072)).unwrap();
    std::fs::create_dir_all(root.join("dir")).unwrap();
    locked(&root.join("locked.txt"))
}

#[cfg(unix)]
fn locked(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, "secret").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    // root は mode 000 のファイルも読めるので、読めたら読めないファイルの場合にならない
    std::fs::read(path).is_err()
}

#[cfg(not(unix))]
fn locked(_: &Path) -> bool {
    false
}

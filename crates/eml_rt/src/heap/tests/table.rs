//! 期待値の表の形と入力。`std` だけを使う。表は `eml_rt` の検査モードの結果から作る
//! (docs/implementation/testing.md の「期待値の表」)。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Int(i64),
    /// 浮動小数のビット列。表には Rust の `{:?}` で書き、読み戻すと同じビット列になる値だけを使う
    Float(u64),
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
    /// 整数を要素に持つ配列。`lin` は `MutArray` の `lin` ビットで、`shared` なら `dup` して渡し、残った参照は結果を
    /// 読んだ後で手放す
    Array {
        elements: Vec<i64>,
        lin: bool,
        shared: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `()` を返す extern の結果
    Unit,
    Int(i64),
    /// 浮動小数の結果 (`float_outcome`)。Rust の `{:?}` で書く。正の quiet NaN (`f64::NAN`) でない NaN は、`{:?}` が
    /// `NaN` と書いて区別しないので、ビット列を添えて書く。表の NaN はどれも `NaN` なので、そろえ忘れると表と合わない
    Float(String),
    Bool(bool),
    Order(&'static str),
    Str(String),
    /// `println` が書いた出力
    Out(String),
    /// 位置を除いた誤りの文言
    Err(String),
    /// 整数を要素に持つ配列と、その `lin` ビット
    Array {
        elements: Vec<i64>,
        lin: bool,
    },
    /// `HASHED_OVER` バイトより長い文字列の結果 (`Str` か `Out`) のバイト数と FNV-1a の値。1 MiB の中身をそのまま
    /// 書くと、表が読めない大きさになる
    Hashed {
        len: usize,
        hash: u64,
    },
}

/// これより長い文字列の結果は、長さと FNV-1a の値で比べる
pub const HASHED_OVER: usize = 4096;

/// 長い文字列の結果を `Hashed` にする。表と比べる前に、両方の側が結果をこれに通す
pub fn digest(outcome: Outcome) -> Outcome {
    match outcome {
        Outcome::Str(s) | Outcome::Out(s) if s.len() > HASHED_OVER => Outcome::Hashed {
            len: s.len(),
            hash: fnv1a(s.as_bytes()),
        },
        other => other,
    }
}

/// 64 ビットの FNV-1a
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
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
    "Prelude.Num Int.+",
    "Prelude.Num Int.-",
    "Prelude.Num Int.*",
    "Prelude.Div Int./",
    "Prelude.Div Int.%",
    "Prelude.Eq Int.==",
    "Prelude.Eq Int.!=",
    "Prelude.Ord Int.compare",
    "Prelude.Ord Int.<",
    "Prelude.Ord Int.<=",
    "Prelude.Ord Int.>",
    "Prelude.Ord Int.>=",
];

/// 文字の端の値。Unicode のスカラー値の両端、サロゲートの前後、ASCII と BMP と補助面の境
pub const CHAR_EDGES: [i64; 12] = [
    0x0, 0x41, 0x61, 0x7f, 0x80, 0xe9, 0xd7ff, 0xe000, 0xffff, 0x1_0000, 0x1_f600, 0x10_ffff,
];

/// `Show Char` の引数。エスケープする文字、その隣、UTF-8 の長さの境目
pub const CHAR_SHOW: [i64; 22] = [
    0x0, 0x1, 0x9, 0xa, 0xd, 0x1f, 0x20, 0x22, 0x27, 0x5c, 0x7b, 0x7e, 0x7f, 0x80, 0xe9, 0x7ff,
    0x800, 0xd7ff, 0xe000, 0xffff, 0x1_0000, 0x10_ffff,
];

/// `Char.of_int` の引数。スカラー値の両端、サロゲートの前後、範囲の外
pub const CHAR_OF_INT: [i64; 10] = [
    -1,
    0,
    0xd7ff,
    0xd800,
    0xdfff,
    0xe000,
    0x10_ffff,
    0x11_0000,
    i64::MIN,
    i64::MAX,
];

/// 文字を2つ取る行
pub const CHAR_BINARY: [&str; 7] = [
    "Prelude.Eq Char.==",
    "Prelude.Eq Char.!=",
    "Prelude.Ord Char.compare",
    "Prelude.Ord Char.<",
    "Prelude.Ord Char.<=",
    "Prelude.Ord Char.>",
    "Prelude.Ord Char.>=",
];

/// 浮動小数の端の値。±0、±1、小数、`MAX`、`MIN`、最小の正規化数と非正規化数、±無限大、NaN
pub const FLOAT_EDGES: [f64; 14] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.1,
    1.5,
    -7.5,
    f64::MAX,
    f64::MIN,
    f64::MIN_POSITIVE,
    5e-324,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
];

/// `Int` への変換の端の値。ちょうど中間の値と、`Int` の範囲の両端の内と外
pub const FLOAT_TO_INT: [f64; 12] = [
    2.5,
    3.5,
    -0.5,
    0.5,
    -2.7,
    2.1,
    // 2^63 より小さい最大の値、2^63、-2^63、-2^63 より大きい側の外の値
    9223372036854774784.0,
    9223372036854775808.0,
    -9223372036854775808.0,
    -9223372036854777856.0,
    1e19,
    -1e19,
];

/// `sin`、`cos`、`exp`、`log` の入力。C の規格 (Annex F) が結果を決めている値に限る。ほかの値の結果は超越数で、
/// libm によって最後の桁が変わりうるためである。`log` は `1.0` (結果は `0.0`) も使う
pub const MATH_EDGES: [f64; 5] = [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN];

/// `pow` の底と指数。どの組も結果がちょうど表せる (`pow 2.0 10.0` は `1024.0`) か、Annex F が決めている
pub const POW_BASES: [f64; 8] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    2.0,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
];

pub const POW_EXPONENTS: [f64; 9] = [
    0.0,
    -0.0,
    1.0,
    2.0,
    10.0,
    -1.0,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
];

/// 正しく丸めた最短の桁が往復できない 2 の冪の指数。Python の `repr` は、ここで `{:.*e}` と違う桁を選ぶ
pub const POW2_EXPONENTS: [i32; 46] = [
    -1017, -1007, -957, -921, -808, -791, -788, -778, -705, -695, -662, -652, -549, -509, -496,
    -489, -383, -366, -296, -140, -97, -77, -44, -24, 89, 122, 132, 172, 182, 275, 305, 345, 378,
    398, 405, 481, 534, 544, 554, 574, 594, 710, 803, 863, 896, 976,
];

/// `Float.parse` の入力。空白、符号、`inf` と `nan` の綴り、`_`、読めない形を含む
pub const FLOAT_TEXTS: [&str; 23] = [
    "1.5",
    " 1.5 ",
    "-inf",
    "+Infinity",
    "nan",
    "NaN",
    "-nan",
    "1e5",
    "1E-5",
    ".5",
    "5.",
    "1_0",
    "abc",
    "",
    " ",
    "0x10",
    "1e400",
    "-0.0",
    "\u{2003}2\u{a0}",
    "1.7976931348623157e+308",
    "5e-324",
    "1e",
    "--1",
];

/// 浮動小数を2つ取る行
pub const FLOAT_BINARY: [&str; 12] = [
    "Prelude.Num Float.+",
    "Prelude.Num Float.-",
    "Prelude.Num Float.*",
    "Prelude.Div Float./",
    "Prelude.Div Float.%",
    "Prelude.Eq Float.==",
    "Prelude.Eq Float.!=",
    "Prelude.Ord Float.compare",
    "Prelude.Ord Float.<",
    "Prelude.Ord Float.<=",
    "Prelude.Ord Float.>",
    "Prelude.Ord Float.>=",
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

/// ビット列が一様な浮動小数。NaN は表に書くと中身が変わるので除く
fn random_floats(n: usize) -> Vec<f64> {
    let mut rng = Rng::new();
    let mut floats = Vec::new();
    while floats.len() < n {
        let x = f64::from_bits(rng.next());
        if !x.is_nan() {
            floats.push(x);
        }
    }
    floats
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
        "x" => Arg::Float(parse_float(value)?.to_bits()),
        "b" => Arg::Bool(parse_bool(value)?),
        "s" => Arg::Const(quoted(value)?),
        "u" => Arg::Unique(quoted(value)?),
        "d" => Arg::Shared(quoted(value)?),
        "f" => Arg::Rel(quoted(value)?),
        "a" => Arg::Abs(quoted(value)?),
        "A" | "D" | "M" => Arg::Array {
            elements: parse_ints(value)?,
            lin: kind == "M",
            shared: kind == "D",
        },
        _ => return Err(format!("an unknown argument kind: {text}")),
    })
}

fn parse_outcome(text: &str) -> Result<Outcome, String> {
    let (kind, value) = text
        .split_once(':')
        .ok_or_else(|| format!("a result without a kind: {text}"))?;
    Ok(match kind {
        "unit" if value == "()" => Outcome::Unit,
        "i" => Outcome::Int(parse_int(value)?),
        "x" => Outcome::Float(float_outcome(parse_float(value)?)),
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
        "A" | "M" => Outcome::Array {
            elements: parse_ints(value)?,
            lin: kind == "M",
        },
        "h" => {
            let (len, hash) = value
                .split_once(':')
                .ok_or_else(|| format!("a hashed result without a hash: {text}"))?;
            Outcome::Hashed {
                len: len.parse().map_err(|_| format!("not a length: {len}"))?,
                hash: u64::from_str_radix(hash, 16).map_err(|_| format!("not a hash: {hash}"))?,
            }
        }
        _ => return Err(format!("an unknown result kind: {text}")),
    })
}

fn parse_int(text: &str) -> Result<i64, String> {
    text.parse().map_err(|_| format!("not an integer: {text}"))
}

/// `[1,2,3]` の形の整数の並び
fn parse_ints(text: &str) -> Result<Vec<i64>, String> {
    let inner = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .ok_or_else(|| format!("not a bracketed list: {text}"))?;
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner.split(',').map(parse_int).collect()
}

fn ints_text(elements: &[i64]) -> String {
    let parts: Vec<String> = elements.iter().map(|e| e.to_string()).collect();
    format!("[{}]", parts.join(","))
}

/// 浮動小数の結果の書き方。`f64::NAN` でない NaN は `NaN(0x…)` と書き、表の `NaN` と合わないようにする
pub fn float_outcome(x: f64) -> String {
    if x.is_nan() && x.to_bits() != f64::NAN.to_bits() {
        format!("NaN({:#018x})", x.to_bits())
    } else {
        format!("{x:?}")
    }
}

/// Rust の `{:?}` で書いた浮動小数。書き戻して同じ文字列になるものだけを受ける
fn parse_float(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|x| format!("{x:?}") == text)
        .ok_or_else(|| format!("not a float written by `{{:?}}`: {text}"))
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
        Arg::Float(bits) => format!("x:{:?}", f64::from_bits(*bits)),
        Arg::Bool(b) => format!("b:{}", bool_name(*b)),
        Arg::Const(s) => format!("s:{s:?}"),
        Arg::Unique(s) => format!("u:{s:?}"),
        Arg::Shared(s) => format!("d:{s:?}"),
        Arg::Rel(s) => format!("f:{s:?}"),
        Arg::Abs(s) => format!("a:{s:?}"),
        Arg::Array {
            elements,
            lin,
            shared,
        } => {
            let kind = match (lin, shared) {
                (true, _) => "M",
                (false, true) => "D",
                (false, false) => "A",
            };
            format!("{kind}:{}", ints_text(elements))
        }
    }));
    let outcome = match &digest(case.outcome.clone()) {
        Outcome::Unit => "unit:()".to_string(),
        Outcome::Int(v) => format!("i:{v}"),
        Outcome::Float(x) => format!("x:{x}"),
        Outcome::Bool(b) => format!("b:{}", bool_name(*b)),
        Outcome::Order(o) => format!("o:{o}"),
        Outcome::Str(s) => format!("s:{s:?}"),
        Outcome::Out(s) => format!("out:{s:?}"),
        Outcome::Err(s) => format!("err:{s:?}"),
        Outcome::Hashed { len, hash } => format!("h:{len}:{hash:016x}"),
        Outcome::Array { elements, lin } => {
            format!("{}:{}", if *lin { "M" } else { "A" }, ints_text(elements))
        }
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

/// 表を作るための入力。結果の欄は `externs.rs` の書き手が埋める
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
    for a in CHAR_OF_INT {
        push("Std.Char.of_int", vec![Arg::Int(a)]);
    }
    for a in CHAR_EDGES {
        push("Std.Char.to_int", vec![Arg::Int(a)]);
    }
    for name in ["Prelude.Show Char.show", "Prelude.Show Char.display"] {
        for a in CHAR_SHOW {
            push(name, vec![Arg::Int(a)]);
        }
    }
    for name in CHAR_BINARY {
        for a in CHAR_EDGES {
            for b in CHAR_EDGES {
                push(name, vec![Arg::Int(a), Arg::Int(b)]);
            }
        }
    }
    for name in ["Prelude.Num Int.negate", "Prelude.Show Int.show"] {
        for a in EDGES.into_iter().chain(random_ints(RANDOM_INTS)) {
            push(name, vec![Arg::Int(a)]);
        }
    }
    let floats = random_floats(2 * RANDOM_INTS);
    let float = |x: f64| Arg::Float(x.to_bits());
    for name in FLOAT_BINARY {
        for a in FLOAT_EDGES {
            for b in FLOAT_EDGES {
                push(name, vec![float(a), float(b)]);
            }
        }
        for pair in floats.chunks(2) {
            push(name, vec![float(pair[0]), float(pair[1])]);
        }
    }
    for a in FLOAT_EDGES.into_iter().chain(random_floats(RANDOM_INTS)) {
        push("Prelude.Num Float.negate", vec![float(a)]);
    }
    for name in ["Prelude.Show Float.show", "Prelude.shows_minus"] {
        for a in FLOAT_EDGES.into_iter().chain(random_floats(RANDOM_INTS)) {
            push(name, vec![float(a)]);
        }
    }
    for k in POW2_EXPONENTS {
        push("Prelude.Show Float.show", vec![float(2f64.powi(k))]);
    }
    for a in EDGES.into_iter().chain(random_ints(RANDOM_INTS)) {
        push("Std.Float.of_int", vec![Arg::Int(a)]);
    }
    for name in [
        "Std.Float.truncate",
        "Std.Float.floor",
        "Std.Float.ceil",
        "Std.Float.round",
    ] {
        for a in FLOAT_EDGES
            .into_iter()
            .chain(FLOAT_TO_INT)
            .chain(random_floats(RANDOM_INTS))
        {
            push(name, vec![float(a)]);
        }
    }
    for name in ["Std.Float.sqrt", "Std.Float.abs"] {
        for a in FLOAT_EDGES.into_iter().chain(random_floats(RANDOM_INTS)) {
            push(name, vec![float(a)]);
        }
    }
    for name in [
        "Std.Float.sin",
        "Std.Float.cos",
        "Std.Float.exp",
        "Std.Float.log",
    ] {
        for a in MATH_EDGES {
            push(name, vec![float(a)]);
        }
    }
    push("Std.Float.log", vec![float(1.0)]);
    for a in POW_BASES {
        for b in POW_EXPONENTS {
            push("Std.Float.pow", vec![float(a), float(b)]);
        }
    }
    push("Std.Float.infinity_value", vec![]);
    push("Std.Float.nan_value", vec![]);
    for n in [-1, 0, 2, 17, 100, 101] {
        for a in FLOAT_EDGES {
            push("Std.Float.to_fixed", vec![Arg::Int(n), float(a)]);
        }
    }
    for (n, a) in [
        (0, 2.5),
        (0, 3.5),
        (0, -0.5),
        (2, 1.005),
        (9, 1.0 / 3.0),
        (20, 0.1),
    ] {
        push("Std.Float.to_fixed", vec![Arg::Int(n), float(a)]);
    }
    for name in ["Std.Float.can_parse", "Std.Float.parse_raw"] {
        for text in FLOAT_TEXTS {
            push(name, vec![Arg::Const(text.to_string())]);
        }
    }
    push("Std.Float.parse_raw", vec![Arg::Unique("2.5".into())]);
    push("Std.Float.parse_raw", vec![Arg::Shared("2.5".into())]);
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
    // 文字列の核 (`byte_length`、`byte_at`、`slice`、`find_from`)。位置は端と、その外側の 1 つずつである
    for s in &strings {
        push("Std.String.byte_length", vec![c(s)]);
    }
    for s in STRINGS {
        let len = s.len() as i64;
        for i in [-1, 0, 1, len - 1, len, i64::MAX, i64::MIN] {
            push("Std.String.byte_at", vec![Arg::Int(i), c(s)]);
        }
        for (start, end) in [
            (0, 0),
            (0, len),
            (0, 1),
            (1, 2),
            (1, len),
            (len, len),
            (-1, 1),
            (2, 1),
            (0, len + 1),
            (i64::MIN, i64::MAX),
        ] {
            push(
                "Std.String.slice",
                vec![Arg::Int(start), Arg::Int(end), c(s)],
            );
        }
        for needle in ["", "a", "b", "ab", "日", "😀", "\u{0}", "\\", "日本語"] {
            for from in [-1, 0, 1, len, len + 1] {
                push(
                    "Std.String.find_from",
                    vec![c(needle), Arg::Int(from), c(s)],
                );
            }
        }
    }
    // 一意な文字列と共有した文字列も、ほかの引数と同じく手放す
    push(
        "Std.String.slice",
        vec![Arg::Int(0), Arg::Int(1), Arg::Unique("ab".into())],
    );
    push(
        "Std.String.slice",
        vec![Arg::Int(0), Arg::Int(1), Arg::Shared("ab".into())],
    );
    push(
        "Std.String.find_from",
        vec![
            Arg::Unique("b".into()),
            Arg::Int(0),
            Arg::Shared("ab".into()),
        ],
    );
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
    for path in [
        "plain.txt",
        "crlf.txt",
        "empty.txt",
        "bad.bin",
        "big.txt",
        "dir",
        "missing.txt",
    ] {
        push("Std.Fs.read_line_raw", rel(path));
    }
    for path in ["plain.txt", "crlf.txt", "empty.txt"] {
        push("Std.Fs.read_line_raw2", rel(path));
    }
    // 配列。長さは負、0、1、3 で、添字は端と、その外側の 1 つずつと、`Int` の両端である。共有された配列の `set` と
    // `thaw` は写す
    let array = |elements: &[i64], lin: bool, shared: bool| Arg::Array {
        elements: elements.to_vec(),
        lin,
        shared,
    };
    for name in ["Std.Array.make", "Std.MutArray.make"] {
        for n in [-1, 0, 1, 3] {
            push(name, vec![Arg::Int(n), Arg::Int(7)]);
        }
    }
    push("Std.MutArray.empty", vec![]);
    let xs = [10, 20, 30];
    for elements in [&[][..], &[5], &xs] {
        push("Std.Array.length", vec![array(elements, false, false)]);
        push("Std.MutArray.length", vec![array(elements, true, false)]);
    }
    for i in [-1, 0, 2, 3, i64::MIN, i64::MAX] {
        push("Std.Array.get", vec![array(&xs, false, false), Arg::Int(i)]);
        push(
            "Std.MutArray.read",
            vec![array(&xs, true, false), Arg::Int(i)],
        );
    }
    push("Std.Array.get", vec![array(&xs, false, true), Arg::Int(1)]);
    for i in [-1, 0, 2, 3] {
        for (lin, shared) in [(false, false), (false, true)] {
            push(
                "Std.Array.set",
                vec![array(&xs, lin, shared), Arg::Int(i), Arg::Int(9)],
            );
        }
        push(
            "Std.MutArray.write",
            vec![array(&xs, true, false), Arg::Int(i), Arg::Int(9)],
        );
    }
    for elements in [&[][..], &xs] {
        push("Std.Array.thaw", vec![array(elements, false, false)]);
        push("Std.Array.thaw", vec![array(elements, false, true)]);
        push("Std.MutArray.freeze", vec![array(elements, true, false)]);
    }
    // `var` の配列の変数の添字の検査。長さは 0、1、3 と `Int` の最大で、添字は端と、その外側の 1 つずつと、`Int` の
    // 両端である
    for len in [0, 1, 3, i64::MAX] {
        let mut indexes = vec![i64::MIN, -1, 0, len - 1, len, i64::MAX];
        indexes.sort_unstable();
        indexes.dedup();
        for i in indexes {
            push("Prelude.check_index", vec![Arg::Int(i), Arg::Int(len)]);
        }
    }
    inputs
}

/// 表のファイルを置く一時ディレクトリ。落とすときに消すので、場合が失敗して panic しても残らない
pub struct Scratch(PathBuf);

impl Scratch {
    /// 一時ディレクトリの下に、`name` とプロセスの番号で名付けた空のディレクトリを作る
    pub fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// ファイルを用意する。`locked.txt` を作れなかった (root で動いている) ら false を返す
pub fn fixtures(root: &Path) -> bool {
    std::fs::write(root.join("plain.txt"), "line 1\nline 2\n").unwrap();
    std::fs::write(root.join("crlf.txt"), "a\r\nb").unwrap();
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

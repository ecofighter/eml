//! リテラルの値の解釈。lexer の検査 (不正なエスケープの E0008) と、値の取り出し (`ast::Literal::value` と
//! `ast::StringLit::parts`) が同じ規則を使うように、エスケープの表をここに置く (docs/spec/lexical.md)。

use crate::lexer::line_len;

/// `\n` などの1文字のエスケープが表す文字。`\u{...}` と補間の `\{` は別に扱う。
pub(crate) fn simple_escape(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '\\' => '\\',
        '"' => '"',
        '0' => '\0',
        _ => return None,
    })
}

/// `\u` に続く `{...}` を読む。`rest` は `\u` の直後から始まる。`}` を探すのは、文字列の外 (行末と最初の `"`) に
/// 出ず、次の `\` の手前までの範囲だけである。`"\u{4\{x}}"` の穴を不正なエスケープに飲み込まないため
/// (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「トークン」)。`}` があれば、`rest` の先頭から `}` の次までのバイト数と、正しいエスケープ (1〜6桁の
/// 16進数で、Unicode のスカラー値) ならその文字を返す。
pub(crate) fn unicode_escape(rest: &str) -> Option<(usize, Option<char>)> {
    let line = &rest[..line_len(rest)];
    let line = &line[..line.find(['"', '\\']).unwrap_or(line.len())];
    let close = line.strip_prefix('{')?.find('}')?;
    let hex = &line[1..close + 1];
    let valid = (1..=6).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    let c = valid
        .then(|| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32))
        .flatten();
    Some((close + 2, c))
}

/// `INT` トークンの値。`Int` の範囲を超えるものは `None` で、字句解析が E0007 を報告している
/// (docs/spec/lexical.md)。
pub(crate) fn int_value(text: &str) -> Option<i64> {
    let digits = text.replace('_', "");
    let (body, radix) = match digits.get(..2) {
        Some("0x") => (&digits[2..], 16),
        Some("0o") => (&digits[2..], 8),
        Some("0b") => (&digits[2..], 2),
        _ => (&digits[..], 10),
    };
    i64::from_str_radix(body, radix).ok()
}

/// `ESCAPE` のトークンの本文 (`\n`、`\u{41}` など) が表す文字。不正なものと、行末の `\` は `None` で、どれも
/// lexer が報告済みか、文字列が閉じていない。
pub(crate) fn escape_value(text: &str) -> Option<char> {
    let rest = text.strip_prefix('\\')?;
    if let Some(hex) = rest.strip_prefix('u') {
        let (len, c) = unicode_escape(hex)?;
        return (len == hex.len()).then_some(c).flatten();
    }
    let mut chars = rest.chars();
    let c = chars.next()?;
    chars.next().is_none().then(|| simple_escape(c)).flatten()
}

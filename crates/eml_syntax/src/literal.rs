//! リテラルの値の解釈。lexer の検査 (不正なエスケープの E0008) と、値の取り出し (`ast::Literal::value`) が同じ規則を
//! 使うように、エスケープの表をここに置く (docs/spec/lexical.md)。

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
/// 出ない範囲だけである。`}` があれば、`rest` の先頭から `}` の次までのバイト数と、正しいエスケープ (1〜6桁の
/// 16進数で、Unicode のスカラー値) ならその文字を返す。
pub(crate) fn unicode_escape(rest: &str) -> Option<(usize, Option<char>)> {
    let line = &rest[..line_len(rest)];
    let line = &line[..line.find('"').unwrap_or(line.len())];
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

/// 通常の文字列リテラル `"..."` の値。補間を含むもの (M3)、不正なエスケープを含むもの、閉じていないものは `None`
/// で、どれも字句解析が報告している。
pub(crate) fn decode_string(text: &str) -> Option<String> {
    if text.len() < 2 {
        return None;
    }
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'u' => {
                let rest = chars.as_str();
                let (len, c) = unicode_escape(rest)?;
                out.push(c?);
                chars = rest[len..].chars();
            }
            c => out.push(simple_escape(c)?),
        }
    }
    Some(out)
}

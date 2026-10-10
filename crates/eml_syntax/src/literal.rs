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

/// `\u` に続く `{...}` を読む。`rest` は `\u` の直後から始まる。`}` を探すのは、リテラルの外 (行末、最初の `"`
/// とバッククォート) に出ず、次の `\` の手前までの範囲だけである。`"\u{4\{x}}"` の穴を不正なエスケープに飲み込まない
/// ため (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「トークン」)。`"` もバッククォートも、正しい
/// エスケープの中には現れない。そのため、どちらのリテラルでも両方で止めれば、lexer と値の取り出しが閉じの文字を
/// 知らずに同じ結果になる。`}` があれば、`rest` の先頭から `}` の次までのバイト数と、正しいエスケープ (1〜6桁の
/// 16進数で、Unicode のスカラー値) ならその文字を返す。
pub(crate) fn unicode_escape(rest: &str) -> Option<(usize, Option<char>)> {
    let line = &rest[..line_len(rest)];
    let line = &line[..line.find(['"', '`', '\\']).unwrap_or(line.len())];
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

/// 複数行の文字列の本文 (開きの `"""` の後ろから閉じの `"""` の手前まで) の形。`content` は値にする範囲 (開きの
/// 行の改行の後ろから、閉じの行の改行の手前まで) で、`indent` は閉じの `"""` の列である
/// (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「複数行の文字列」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MultilineLayout {
    pub(crate) content: std::ops::Range<usize>,
    pub(crate) indent: usize,
}

/// 形の誤り。範囲は本文の先頭からのバイト数である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LayoutError {
    SameLine,
    At {
        range: std::ops::Range<usize>,
        message: &'static str,
        label: &'static str,
    },
}

/// lexer の E0014 と `ast::StringLit::parts` が、この1つの判定を使う。
pub(crate) fn multiline_layout(body: &str) -> Result<MultilineLayout, Vec<LayoutError>> {
    let Some(first_break) = body.find('\n') else {
        return Err(vec![LayoutError::SameLine]);
    };
    let mut errors: Vec<LayoutError> = opening_line_error(body).into_iter().collect();
    let last_break = body.rfind('\n').expect("the body has a line break");
    let closing = &body[last_break + 1..];
    // 閉じの前に文字があると N が決まらないので、字下げは検査しない
    if let Some(offset) = closing.find(|c: char| c != ' ') {
        errors.push(LayoutError::At {
            range: last_break + 1 + offset..body.len(),
            message: "unexpected text before the closing `\"\"\"`",
            label: "only spaces may precede the closing `\"\"\"`",
        });
        return Err(errors);
    }
    let indent = closing.len();
    let content_start = first_break + 1;
    let content_end = if body[..last_break].ends_with('\r') {
        last_break - 1
    } else {
        last_break
    };
    // `"""` の次の行が閉じの行なら、開きの改行と閉じの改行は同じもので、中身は空になる
    let content = content_start..content_end.max(content_start);
    let prefix = " ".repeat(indent);
    let mut line_start = content.start;
    while line_start < content.end {
        let line_end = body[line_start..content.end]
            .find('\n')
            .map_or(content.end, |i| line_start + i);
        let line = body[line_start..line_end].trim_end_matches('\r');
        let blank = line.chars().all(|c| c == ' ');
        if !blank && !line.starts_with(&prefix) {
            let (offset, c) = line
                .char_indices()
                .find(|&(_, c)| c != ' ')
                .expect("the line is not blank");
            errors.push(LayoutError::At {
                range: line_start..line_start + offset + c.len_utf8(),
                message: "line is indented less than the closing `\"\"\"`",
                label: "indent this line at least as deep as the closing `\"\"\"`",
            });
        }
        line_start = line_end + 1;
    }
    if errors.is_empty() {
        Ok(MultilineLayout { content, indent })
    } else {
        Err(errors)
    }
}

/// 開きの `"""` の後ろの、その行の空白でない部分。閉じていない文字列でも検査するので、別に置く。
pub(crate) fn opening_line_error(body: &str) -> Option<LayoutError> {
    let line = &body[..line_len(body)];
    let offset = line.find(|c: char| c != ' ')?;
    Some(LayoutError::At {
        range: offset..line.len(),
        message: "unexpected text after the opening `\"\"\"`",
        label: "the content of a multi-line string starts on the next line",
    })
}

/// `RAW_STRING` の値。閉じていなければ `None` で、lexer が報告済みである。改行は `\n` にそろえる
/// (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「改行の正規化」)。
pub(crate) fn raw_value(text: &str) -> Option<String> {
    let after_r = text.strip_prefix('r')?;
    let hashes = after_r.bytes().take_while(|&b| b == b'#').count();
    let close = format!("\"{}", "#".repeat(hashes));
    let inner = after_r[hashes..]
        .strip_prefix('"')?
        .strip_suffix(close.as_str())?;
    Some(inner.replace("\r\n", "\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_lines_may_be_shorter_or_longer_than_the_indent() {
        assert_eq!(
            multiline_layout("\n  \n  "),
            Ok(MultilineLayout {
                content: 1..3,
                indent: 2
            })
        );
        assert_eq!(
            multiline_layout("\n\n    \n  "),
            Ok(MultilineLayout {
                content: 1..6,
                indent: 2
            })
        );
    }

    #[test]
    fn a_short_line_is_one_error() {
        assert_eq!(
            multiline_layout("\n  x\n    "),
            Err(vec![LayoutError::At {
                range: 1..4,
                message: "line is indented less than the closing `\"\"\"`",
                label: "indent this line at least as deep as the closing `\"\"\"`",
            }])
        );
    }

    #[test]
    fn a_body_without_a_line_break_is_on_one_line() {
        assert_eq!(multiline_layout("x"), Err(vec![LayoutError::SameLine]));
    }
}

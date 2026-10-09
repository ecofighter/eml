//! 文字列、複数行の文字列、raw 文字列、コマンドリテラルの字句。補間、複数行の文字列、raw 文字列は S6、コマンド
//! リテラルはコマンドリテラルの段で実装する。今は閉じまでを1つのトークンにして、診断を1件だけ出す。

use super::Lexer;
use crate::SyntaxKind::*;
use crate::codes;
use crate::literal;
use eml_diagnostics::{NOT_YET_SUPPORTED, NOT_YET_SUPPORTED_LABEL};

impl Lexer<'_> {
    /// 閉じていなければ行末までを `STRING` にする。後ろの行まで文字列として読み込まないため。
    pub(super) fn string(&mut self) {
        let text = self.text;
        let start = self.pos;
        let mut i = start + 1;
        let mut terminated = false;
        while let Some(c) = text[i..].chars().next() {
            match c {
                '"' => {
                    i += 1;
                    terminated = true;
                    break;
                }
                '\n' => break,
                '\r' if text[i + 1..].starts_with('\n') => break,
                '\\' => i = self.escape(i),
                _ => i += c.len_utf8(),
            }
        }
        if !terminated {
            self.error(
                codes::UNTERMINATED_STRING,
                "unterminated string literal",
                start,
                i,
                "missing closing `\"`",
            );
        }
        self.push(STRING, i);
    }

    pub(super) fn escape(&mut self, i: usize) -> usize {
        let text = self.text;
        let Some(c) = text[i + 1..].chars().next() else {
            return i + 1;
        };
        match c {
            // 行末の `\` は文字列の外に出ない。文字列は閉じていない扱いになり、E0002 だけを出す。
            '\n' | '\r' => i + 1,
            'u' => self.unicode_escape(i),
            '{' => self.interpolation(i),
            c if literal::simple_escape(c).is_some() => i + 2,
            _ => {
                let end = i + 1 + c.len_utf8();
                self.error(
                    codes::INVALID_ESCAPE,
                    format!("unknown escape sequence `\\{c}`"),
                    i,
                    end,
                    "not a valid escape",
                );
                end
            }
        }
    }

    pub(super) fn unicode_escape(&mut self, i: usize) -> usize {
        let text = self.text;
        let (end, valid) = match literal::unicode_escape(&text[i + 2..]) {
            Some((len, c)) => (i + 2 + len, c.is_some()),
            None => (i + 2, false),
        };
        if !valid {
            self.error(
                codes::INVALID_ESCAPE,
                format!("invalid unicode escape `{}`", &text[i..end]),
                i,
                end,
                "expected `\\u{` followed by 1 to 6 hex digits and `}`",
            );
        }
        end
    }

    /// 補間は S6 で実装する。今は対応する `}` まで読み飛ばして E0004 を出す。穴の中の文字列も読み飛ばすのは、
    /// `"\{f "x"}"` の内側の `"` で外側の文字列を終わらせないため。
    pub(super) fn interpolation(&mut self, i: usize) -> usize {
        let text = self.text;
        let mut j = i + 2;
        let mut depth = 1;
        while let Some(c) = text[j..].chars().next() {
            match c {
                '\n' => break,
                '{' => {
                    depth += 1;
                    j += 1;
                }
                '}' => {
                    depth -= 1;
                    j += 1;
                    if depth == 0 {
                        break;
                    }
                }
                '"' => j = skip_simple_string(text, j),
                _ => j += c.len_utf8(),
            }
        }
        self.error(
            NOT_YET_SUPPORTED,
            "string interpolation is not supported yet",
            i,
            j,
            NOT_YET_SUPPORTED_LABEL,
        );
        j
    }

    /// S6 で実装する。今は閉じの `"""` までを1つのトークンにして、HIR が E0004 を1件だけ出せるようにする
    /// (docs/implementation/status.md の「未対応の構文と E0004」)。
    pub(super) fn multiline_string(&mut self) {
        let text = self.text;
        let start = self.pos;
        let end = match text[start + 3..].find("\"\"\"") {
            Some(offset) => start + 3 + offset + 3,
            None => {
                self.error(
                    codes::UNTERMINATED_STRING,
                    "unterminated multi-line string",
                    start,
                    start + 3,
                    "missing closing `\"\"\"`",
                );
                text.len()
            }
        };
        self.push(MULTILINE_STRING, end);
    }

    /// S6 で実装する。今は閉じまでを1つのトークンにして、HIR が E0004 を1件だけ出せるようにする
    /// (docs/implementation/status.md の「未対応の構文と E0004」)。
    pub(super) fn raw_string(&mut self, hashes: usize) {
        let text = self.text;
        let start = self.pos;
        let open_len = 1 + hashes + 1;
        let close = format!("\"{}", "#".repeat(hashes));
        let end = match text[start + open_len..].find(&close) {
            Some(offset) => start + open_len + offset + close.len(),
            None => {
                self.error(
                    codes::UNTERMINATED_STRING,
                    "unterminated raw string",
                    start,
                    start + open_len,
                    format!("missing closing `{close}`"),
                );
                text.len()
            }
        };
        self.push(RAW_STRING, end);
    }

    /// コマンドリテラルの段で実装する。今は閉じのバッククォートまでを1つのトークンにして、
    /// parser が E0004 を1件だけ出せるようにする。
    pub(super) fn command(&mut self) {
        let text = self.text;
        let start = self.pos;
        let mut i = start + 1;
        let mut terminated = false;
        while let Some(c) = text[i..].chars().next() {
            match c {
                '`' => {
                    i += 1;
                    terminated = true;
                    break;
                }
                '\n' => break,
                '\r' if text[i + 1..].starts_with('\n') => break,
                '\\' => i = skip_escaped(text, i),
                _ => i += c.len_utf8(),
            }
        }
        if !terminated {
            self.error(
                codes::UNTERMINATED_STRING,
                "unterminated command literal",
                start,
                i,
                "missing closing backtick",
            );
        }
        self.push(COMMAND, i);
    }
}

/// `j` は開きの `"` の位置。閉じの `"` の次 (なければ行末) を返す。
pub(super) fn skip_simple_string(text: &str, j: usize) -> usize {
    let mut k = j + 1;
    while let Some(c) = text[k..].chars().next() {
        match c {
            '"' => return k + 1,
            '\n' => return k,
            '\\' => k = skip_escaped(text, k),
            _ => k += c.len_utf8(),
        }
    }
    k
}

pub(super) fn raw_string_hashes(rest: &str) -> Option<usize> {
    let after_r = rest.strip_prefix('r')?;
    let hashes = after_r.bytes().take_while(|&b| b == b'#').count();
    after_r[hashes..].starts_with('"').then_some(hashes)
}

/// `\` の位置 `i` から、次の1文字までを読み飛ばした位置。改行は読み飛ばさない。文字列やコマンドリテラルを
/// 行の外まで広げないため。
fn skip_escaped(text: &str, i: usize) -> usize {
    let i = i + 1;
    text[i..]
        .chars()
        .next()
        .filter(|&c| c != '\n')
        .map_or(i, |c| i + c.len_utf8())
}

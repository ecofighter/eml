//! 文字列、複数行の文字列、raw 文字列、コマンドリテラルの字句。単一行の文字列とコマンドリテラルは、モードの
//! スタックで細かいトークンに分ける (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「字句」)。複数行の
//! 文字列と raw 文字列は S6b の Task 3 で分ける。今は閉じまでを1つのトークンにして、診断を1件だけ出す。

use super::{Lexer, Mode};
use crate::SyntaxKind::{self, *};
use crate::codes;
use crate::literal;

impl Lexer<'_> {
    /// 単一行の文字列の本文を1トークン読む。複数行の文字列は Task 3 で足す。
    pub(super) fn string_body(&mut self, multiline: bool) {
        debug_assert!(!multiline, "multi-line strings come in Task 3");
        self.body(STRING_TEXT, '"', STRING_END);
    }

    /// コマンドリテラルの中の `"` は本文である
    /// (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「トークン」)。
    pub(super) fn command_body(&mut self) {
        self.body(CMD_TEXT, '`', CMD_END);
    }

    /// 本文は、閉じの文字、`\`、改行の手前で止める。`\r` は、後ろが `\n` のときだけ改行である。改行に着いたら、
    /// 閉じていない層を閉じる。
    fn body(&mut self, text_kind: SyntaxKind, close: char, end_kind: SyntaxKind) {
        let text = self.text;
        let rest = &text[self.pos..];
        let end = rest
            .char_indices()
            .find(|&(i, c)| {
                c == close
                    || matches!(c, '\\' | '\n')
                    || (c == '\r' && rest[i + 1..].starts_with('\n'))
            })
            .map_or(rest.len(), |(i, _)| i);
        if end > 0 {
            self.push(text_kind, self.pos + end);
            return;
        }
        match rest.chars().next() {
            Some(c) if c == close => {
                self.push(end_kind, self.pos + 1);
                self.modes.pop();
            }
            Some('\\') => self.escape_or_hole(),
            _ => self.close_layers(false, false),
        }
    }

    /// `\` から始まるエスケープか補間の開き。行末とファイルの終わりの `\` は長さ 1 の `ESCAPE` にし、E0008 は
    /// 出さない。文字列は閉じていない扱いになり、E0002 だけが出る。
    fn escape_or_hole(&mut self) {
        let text = self.text;
        let i = self.pos;
        match text[i + 1..].chars().next() {
            Some('{') => {
                self.push(INTERP_START, i + 2);
                self.modes.push(Mode::Code {
                    braces: 0,
                    hole: Some(i),
                });
            }
            None | Some('\n') => self.push(ESCAPE, i + 1),
            Some('\r') if text[i + 2..].starts_with('\n') => self.push(ESCAPE, i + 1),
            Some('u') => {
                let end = self.unicode_escape(i);
                self.push(ESCAPE, end);
            }
            Some('`') if matches!(self.modes.last(), Some(Mode::Command { .. })) => {
                self.push(ESCAPE, i + 2);
            }
            Some(c) if literal::simple_escape(c).is_some() => self.push(ESCAPE, i + 2),
            Some(c) => {
                let end = i + 1 + c.len_utf8();
                self.error(
                    codes::INVALID_ESCAPE,
                    format!("unknown escape sequence `\\{c}`"),
                    i,
                    end,
                    "not a valid escape",
                );
                self.push(ESCAPE, end);
            }
        }
    }

    fn unicode_escape(&mut self, i: usize) -> usize {
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

    /// S6b で実装する。今は閉じの `"""` までを1つのトークンにして、HIR が E0004 を1件だけ出せるようにする
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

    /// S6b で実装する。今は閉じまでを1つのトークンにして、HIR が E0004 を1件だけ出せるようにする
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
}

pub(super) fn raw_string_hashes(rest: &str) -> Option<usize> {
    let after_r = rest.strip_prefix('r')?;
    let hashes = after_r.bytes().take_while(|&b| b == b'#').count();
    after_r[hashes..].starts_with('"').then_some(hashes)
}

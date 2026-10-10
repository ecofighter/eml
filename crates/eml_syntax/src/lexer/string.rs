//! 文字列、複数行の文字列、raw 文字列、コマンドリテラルの字句。raw 文字列のほかは、モードのスタックで細かい
//! トークンに分ける (docs/spec/lexical.md の「文字列」)。raw 文字列は中に構造が
//! ないので、閉じまでを1つのトークンにする。

use super::{Lexer, Mode, line_len};
use crate::SyntaxKind::{self, *};
use crate::codes;
use crate::literal::{self, LayoutError};

impl Lexer<'_> {
    pub(super) fn string_body(&mut self, multiline: bool) {
        if multiline {
            self.multiline_body();
        } else {
            self.body(STRING_TEXT, '"', STRING_END);
        }
    }

    /// 穴の中には改行を書けないので、穴の中の `"""` は E0014 にして単一行の文字列として読む。報告済みにしておき、
    /// 開きの行の終わりで閉じるときに E0002 を重ねない
    /// (docs/spec/lexical.md の「改行での回復」)。
    pub(super) fn multiline_start(&mut self, in_hole: bool) {
        let start = self.pos;
        if in_hole {
            self.error(
                codes::INVALID_MULTILINE_STRING,
                "multi-line strings are not allowed inside an interpolation",
                start,
                start + 3,
                "use a single-line string here",
            );
        }
        self.push(STRING_START, start + 3);
        self.modes.push(Mode::String {
            start,
            multiline: !in_hole,
            reported: in_hole,
        });
    }

    /// 本文は改行では止めず、`"""`、`\`、ファイルの終わりの手前で止める。`"` と `""` は本文である
    /// (docs/spec/lexical.md の「複数行の文字列」)。
    fn multiline_body(&mut self) {
        let text = self.text;
        let rest = &text[self.pos..];
        let end = rest
            .char_indices()
            .find(|&(i, c)| c == '\\' || rest[i..].starts_with("\"\"\""))
            .map_or(rest.len(), |(i, _)| i);
        if end > 0 {
            self.push(STRING_TEXT, self.pos + end);
        } else if rest.starts_with('\\') {
            self.escape_or_hole();
        } else {
            self.multiline_end();
        }
    }

    /// 形の検査は、閉じの `"""` で本文が決まってからまとめて行う。
    fn multiline_end(&mut self) {
        let Some(Mode::String { start, .. }) = self.modes.pop() else {
            unreachable!("a multi-line string body is read in its own mode");
        };
        let text = self.text;
        let close = self.pos;
        self.push(STRING_END, close + 3);
        let errors = literal::multiline_layout(&text[start + 3..close]).err();
        self.report_layout(start, close + 3, errors.into_iter().flatten());
    }

    /// `start` は開きの `"""` の位置で、`end` は文字列の終わりである。範囲を文字列の中に収め、文字列の外のコードを
    /// 含めない (docs/spec/lexical.md の「複数行の文字列」)。
    pub(super) fn report_layout(
        &mut self,
        start: usize,
        end: usize,
        errors: impl IntoIterator<Item = LayoutError>,
    ) {
        let body = start + 3;
        for error in errors {
            match error {
                LayoutError::SameLine => self.error(
                    codes::INVALID_MULTILINE_STRING,
                    "a multi-line string must start on a new line after `\"\"\"`",
                    start,
                    end,
                    "this multi-line string is on one line",
                ),
                LayoutError::At {
                    range,
                    message,
                    label,
                } => self.error(
                    codes::INVALID_MULTILINE_STRING,
                    message,
                    body + range.start,
                    body + range.end,
                    label,
                ),
            }
        }
    }

    /// コマンドリテラルの中の `"` は本文である
    /// (docs/spec/lexical.md の「コマンドリテラル」)。
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

    /// `\` から始まるエスケープか補間の開き。行末とファイルの終わりの `\` は長さ 1 の `ESCAPE` にする。単一行の
    /// 文字列とコマンドリテラルでは E0008 を出さない。文字列は閉じていない扱いになり、E0002 だけが出る。複数行の
    /// 文字列は行末で閉じないので、行をつなぐ書き方と取り違えないように E0008 を出す
    /// (docs/spec/lexical.md の「文字列のトークン」)。
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
            None => self.push(ESCAPE, i + 1),
            Some('\n') => self.line_end_escape(),
            Some('\r') if text[i + 2..].starts_with('\n') => self.line_end_escape(),
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

    fn line_end_escape(&mut self) {
        let i = self.pos;
        if matches!(
            self.modes.last(),
            Some(Mode::String {
                multiline: true,
                ..
            })
        ) {
            self.error(
                codes::INVALID_ESCAPE,
                "invalid escape `\\` at the end of a line",
                i,
                i + 1,
                "not a valid escape",
            );
        }
        self.push(ESCAPE, i + 1);
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

    /// 改行を含められるが、穴の中では改行の手前で止める。閉じていなければ、E0002 を一番内側の層の報告にして、外側の
    /// 層を黙って閉じる (docs/spec/lexical.md の「改行での回復」)。
    pub(super) fn raw_string(&mut self, hashes: usize, in_hole: bool) {
        let text = self.text;
        let start = self.pos;
        let open_len = 1 + hashes + 1;
        let close = format!("\"{}", "#".repeat(hashes));
        let rest = &text[start + open_len..];
        let limit = if in_hole { line_len(rest) } else { rest.len() };
        let end = match rest[..limit].find(&close) {
            Some(offset) => start + open_len + offset + close.len(),
            None => {
                self.error(
                    codes::UNTERMINATED_STRING,
                    "unterminated raw string",
                    start,
                    start + open_len,
                    format!("missing closing `{close}`"),
                );
                let end = start + open_len + limit;
                self.push(RAW_STRING, end);
                if in_hole {
                    self.close_layers(true, end == text.len());
                }
                return;
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

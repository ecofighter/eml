//! 規則は docs/spec/lexical.md。CST を lossless にするため、trivia も含めてトークン列をつなげると元のテキストに戻る。

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};
use logos::Logos;

use crate::SyntaxKind::{self, *};
use crate::{NOT_YET_SUPPORTED_LABEL, codes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub range: TextRange,
}

pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lexer = Lexer {
        file,
        text,
        pos: 0,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
    };
    lexer.run();
    let Lexer {
        tokens,
        mut diagnostics,
        ..
    } = lexer;
    for token in tokens.iter().filter(|token| token.kind == ERROR_TOKEN) {
        diagnostics.push(Diagnostic::error(
            codes::UNEXPECTED_CHARACTER,
            format!("unexpected character `{}`", printable(&text[token.range])),
            Label::new(file, token.range, "not valid in eml source"),
        ));
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.range.start());
    (tokens, diagnostics)
}

pub(crate) fn operator_kind(op: &str) -> SyntaxKind {
    match op {
        "=" => EQ,
        "|" => PIPE,
        ":" => COLON,
        "." => DOT,
        "->" => THIN_ARROW,
        "<-" => LEFT_ARROW,
        ".." => DOT2,
        "-" => MINUS,
        _ if op.starts_with(':') => CONOP,
        _ => OP,
    }
}

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum Raw {
    #[regex(r"[ \t\r\n]+")]
    Whitespace,
    #[regex(r"[a-z_][A-Za-z0-9_']*")]
    Lower,
    #[regex(r"[A-Z][A-Za-z0-9_']*")]
    Upper,
    /// 形の不正な数値 (`0xZZ`) も1つのトークンにして E0007 を1件だけ出すため、広くマッチさせて `number_kind` で判定する。
    #[regex(r"[0-9][0-9A-Za-z_]*")]
    Number,
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?")]
    #[regex(r"[0-9][0-9_]*[eE][+-][0-9][0-9_]*")]
    Float,
    #[regex(r"'([^'\\\n]|\\[^\n]|\\u\{[0-9A-Fa-f]*\})'")]
    Char,
    #[regex(r"[!$%&*+\-./<=>?@^|~:]+")]
    Op,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBrack,
    #[token("]")]
    RBrack,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
}

struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    /// 常に文字の境界に置く。`&text[pos..]` で panic しないため。
    pos: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl Lexer<'_> {
    fn run(&mut self) {
        // shebang はファイルの先頭にだけ書ける。先頭の BOM は読み込み時に除いてある (docs/spec/lexical.md)。
        let text = self.text;
        while self.pos < text.len() {
            let rest = &text[self.pos..];
            if self.pos == 0 && rest.starts_with("#!") {
                self.push(SHEBANG, self.pos + line_len(rest));
            } else if rest.starts_with("{-") {
                self.block_comment();
            } else if rest.starts_with("\"\"\"") {
                self.multiline_string();
            } else if rest.starts_with('"') {
                self.string();
            } else if let Some(hashes) = raw_string_hashes(rest) {
                self.raw_string(hashes);
            } else if rest.starts_with('`') {
                self.command();
            } else {
                self.simple();
            }
        }
    }

    /// 連続する `ERROR_TOKEN` は1つにまとめる。認識できない文字の並びに、診断を1件だけ出すため。
    fn push(&mut self, kind: SyntaxKind, end: usize) {
        let range = self.range(self.pos, end);
        match self.tokens.last_mut() {
            Some(last) if kind == ERROR_TOKEN && last.kind == ERROR_TOKEN => {
                last.range = last.range.cover(range);
            }
            _ => self.tokens.push(Token { kind, range }),
        }
        self.pos = end;
    }

    fn range(&self, start: usize, end: usize) -> TextRange {
        TextRange::new(TextSize::new(start as u32), TextSize::new(end as u32))
    }

    fn error(
        &mut self,
        code: ErrorCode,
        message: impl Into<String>,
        start: usize,
        end: usize,
        label: impl Into<String>,
    ) {
        let range = self.range(start, end);
        self.diagnostics.push(Diagnostic::error(
            code,
            message,
            Label::new(self.file, range, label),
        ));
    }

    fn simple(&mut self) {
        let text = self.text;
        let rest = &text[self.pos..];
        let mut raw = Raw::lexer(rest);
        let result = raw.next().expect("the rest of the text is not empty");
        // 認識できない文字は1文字ずつ進める (文字の境界を保つため)。
        let len = match result {
            Ok(_) => raw.span().end,
            Err(()) => rest.chars().next().map_or(1, char::len_utf8),
        };
        let slice = &rest[..len];
        let kind = match result {
            Ok(Raw::Whitespace) => WHITESPACE,
            Ok(Raw::Lower) if slice == "_" => UNDERSCORE,
            Ok(Raw::Lower) => keyword(slice).unwrap_or(LIDENT),
            Ok(Raw::Upper) => UIDENT,
            Ok(Raw::Number) => match number_kind(slice) {
                Some(INT) if int_value(slice).is_none() => {
                    self.error(
                        codes::INVALID_NUMBER,
                        format!("integer literal `{slice}` is too large"),
                        self.pos,
                        self.pos + len,
                        "the largest `Int` is 9223372036854775807",
                    );
                    INT
                }
                Some(kind) => kind,
                None => {
                    self.error(
                        codes::INVALID_NUMBER,
                        format!("invalid number literal `{slice}`"),
                        self.pos,
                        self.pos + len,
                        "not a valid number",
                    );
                    INT
                }
            },
            Ok(Raw::Float) => {
                if self.split_field_index(len) {
                    return;
                }
                FLOAT
            }
            Ok(Raw::Char) => CHAR,
            Ok(Raw::Op) if slice.len() >= 2 && slice.bytes().all(|b| b == b'-') => {
                // `-->` などを演算子として使えるように、`-` だけの並びに限って行コメントにする (Haskell と同じ)。
                self.push(COMMENT, self.pos + line_len(rest));
                return;
            }
            Ok(Raw::Op) => operator_kind(slice),
            Ok(Raw::LParen) => L_PAREN,
            Ok(Raw::RParen) => R_PAREN,
            Ok(Raw::LBrack) => L_BRACK,
            Ok(Raw::RBrack) => R_BRACK,
            Ok(Raw::LBrace) => L_BRACE,
            Ok(Raw::RBrace) => R_BRACE,
            Ok(Raw::Comma) => COMMA,
            Ok(Raw::Semicolon) => SEMICOLON,
            Err(()) => ERROR_TOKEN,
        };
        self.push(kind, self.pos + len);
    }

    /// `t.0.1` の `0.1` はフィールドアクセスの並びなので、空白なしの `.` の直後の `数字.数字` を `INT` `DOT` `INT` に分ける。
    fn split_field_index(&mut self, len: usize) -> bool {
        let text = self.text;
        let start = self.pos;
        let float = &text[start..start + len];
        let after_dot = matches!(
            self.tokens.last(),
            Some(last) if last.kind == DOT && last.range.end() == TextSize::new(start as u32)
        );
        let Some((left, right)) = float.split_once('.') else {
            return false;
        };
        if !after_dot || !right.bytes().all(|b| b.is_ascii_digit() || b == b'_') {
            return false;
        }
        self.push(INT, start + left.len());
        self.push(DOT, start + left.len() + 1);
        self.push(INT, start + len);
        true
    }

    /// 閉じていなければ行末までを `STRING` にする。後ろの行まで文字列として読み込まないため。
    fn string(&mut self) {
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

    fn escape(&mut self, i: usize) -> usize {
        let text = self.text;
        let Some(c) = text[i + 1..].chars().next() else {
            return i + 1;
        };
        match c {
            // 行末の `\` は文字列の外に出ない。文字列は閉じていない扱いになり、E0002 だけを出す。
            '\n' | '\r' => i + 1,
            'n' | 't' | 'r' | '\\' | '"' | '0' => i + 2,
            'u' => self.unicode_escape(i),
            '{' => self.interpolation(i),
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

    fn unicode_escape(&mut self, i: usize) -> usize {
        let text = self.text;
        let rest = &text[i + 2..];
        // `}` の探索は文字列の外 (最初の `"` や行末) に出ない。
        let line = &rest[..line_len(rest)];
        let line = &line[..line.find('"').unwrap_or(line.len())];
        let (end, valid) = match line.strip_prefix('{').and_then(|inner| inner.find('}')) {
            Some(close) => {
                let hex = &line[1..close + 1];
                let valid = (1..=6).contains(&hex.len())
                    && hex.bytes().all(|b| b.is_ascii_hexdigit())
                    && u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .is_some();
                (i + 2 + close + 2, valid)
            }
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

    /// 補間は S2 で実装する。今は対応する `}` まで読み飛ばして E0004 を出す。穴の中の文字列も読み飛ばすのは、
    /// `"\{f "x"}"` の内側の `"` で外側の文字列を終わらせないため。
    fn interpolation(&mut self, i: usize) -> usize {
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
            codes::NOT_YET_SUPPORTED,
            "string interpolation is not supported yet",
            i,
            j,
            NOT_YET_SUPPORTED_LABEL,
        );
        j
    }

    /// S2 で実装する。今は閉じの `"""` までを1つのトークンにして、parser が E0004 を1件だけ出せるようにする。
    fn multiline_string(&mut self) {
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

    /// S2 で実装する。今は閉じまでを1つのトークンにして、parser が E0004 を1件だけ出せるようにする。
    fn raw_string(&mut self, hashes: usize) {
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

    /// S3 で実装する。今は閉じのバッククォートまでを1つのトークンにして、parser が E0004 を1件だけ出せるようにする。
    fn command(&mut self) {
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
                '\\' => {
                    i += 1;
                    if let Some(next) = text[i..].chars().next().filter(|&next| next != '\n') {
                        i += next.len_utf8();
                    }
                }
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

    fn block_comment(&mut self) {
        let text = self.text;
        let start = self.pos;
        let mut i = start + 2;
        let mut depth = 1;
        while i < text.len() {
            let rest = &text[i..];
            if rest.starts_with("{-") {
                depth += 1;
                i += 2;
            } else if rest.starts_with("-}") {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    break;
                }
            } else {
                i += rest.chars().next().map_or(1, char::len_utf8);
            }
        }
        if depth > 0 {
            self.error(
                codes::UNTERMINATED_BLOCK_COMMENT,
                "unterminated block comment",
                start,
                start + 2,
                "missing closing `-}`",
            );
        }
        self.push(BLOCK_COMMENT, i);
    }
}

fn line_len(rest: &str) -> usize {
    let end = rest.find('\n').unwrap_or(rest.len());
    if rest[..end].ends_with('\r') {
        end - 1
    } else {
        end
    }
}

/// `j` は開きの `"` の位置。閉じの `"` の次 (なければ行末) を返す。
fn skip_simple_string(text: &str, j: usize) -> usize {
    let mut k = j + 1;
    while let Some(c) = text[k..].chars().next() {
        match c {
            '"' => return k + 1,
            '\n' => return k,
            '\\' => {
                k += 1;
                if let Some(next) = text[k..].chars().next().filter(|&next| next != '\n') {
                    k += next.len_utf8();
                }
            }
            _ => k += c.len_utf8(),
        }
    }
    k
}

fn raw_string_hashes(rest: &str) -> Option<usize> {
    let after_r = rest.strip_prefix('r')?;
    let hashes = after_r.bytes().take_while(|&b| b == b'#').count();
    after_r[hashes..].starts_with('"').then_some(hashes)
}

/// `INT` トークンの値。`Int` の範囲を超えるものは `None` で、字句解析が E0007 を報告している
/// (docs/spec/lexical.md)。HIR も同じ関数で値を読む。
pub fn int_value(text: &str) -> Option<i64> {
    let digits = text.replace('_', "");
    let (body, radix) = match digits.get(..2) {
        Some("0x") => (&digits[2..], 16),
        Some("0o") => (&digits[2..], 8),
        Some("0b") => (&digits[2..], 2),
        _ => (&digits[..], 10),
    };
    i64::from_str_radix(body, radix).ok()
}

/// 通常の文字列リテラル `"..."` の値。補間を含むもの (S2)、不正なエスケープを含むもの、閉じていないものは `None`
/// で、どれも字句解析が報告している。
pub fn decode_string(text: &str) -> Option<String> {
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
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '0' => out.push('\0'),
            'u' => {
                let rest = chars.as_str();
                let close = rest.strip_prefix('{')?.find('}')?;
                let code = u32::from_str_radix(&rest[1..close + 1], 16).ok()?;
                out.push(char::from_u32(code)?);
                chars = rest[close + 2..].chars();
            }
            _ => return None,
        }
    }
    Some(out)
}

fn number_kind(number: &str) -> Option<SyntaxKind> {
    let digits = |body: &str, radix: u32| {
        body.chars().any(|c| c != '_') && body.chars().all(|c| c == '_' || c.is_digit(radix))
    };
    let radix_body = [("0x", 16), ("0o", 8), ("0b", 2)]
        .into_iter()
        .find_map(|(prefix, radix)| number.strip_prefix(prefix).map(|body| (body, radix)));
    if let Some((body, radix)) = radix_body {
        return digits(body, radix).then_some(INT);
    }
    if digits(number, 10) {
        return Some(INT);
    }
    let (mantissa, exponent) = number.split_once(['e', 'E'])?;
    (digits(mantissa, 10) && digits(exponent, 10)).then_some(FLOAT)
}

fn keyword(ident: &str) -> Option<SyntaxKind> {
    Some(match ident {
        "data" => DATA_KW,
        "type" => TYPE_KW,
        "effect" => EFFECT_KW,
        "where" => WHERE_KW,
        "pub" => PUB_KW,
        "import" => IMPORT_KW,
        "as" => AS_KW,
        "infixl" => INFIXL_KW,
        "infixr" => INFIXR_KW,
        "infix" => INFIX_KW,
        "let" => LET_KW,
        "in" => IN_KW,
        "if" => IF_KW,
        "then" => THEN_KW,
        "else" => ELSE_KW,
        "match" => MATCH_KW,
        "with" => WITH_KW,
        "handle" => HANDLE_KW,
        "from" => FROM_KW,
        "resume" => RESUME_KW,
        "drop" => DROP_KW,
        "return" => RETURN_KW,
        "never" => NEVER_KW,
        "once" => ONCE_KW,
        "multi" => MULTI_KW,
        "use" => USE_KW,
        "fn" => FN_KW,
        "forall" => FORALL_KW,
        "class" => CLASS_KW,
        "instance" => INSTANCE_KW,
        _ => return None,
    })
}

/// 診断のメッセージに埋め込むため、制御文字をエスケープし、長い並びを切り詰める。
fn printable(snippet: &str) -> String {
    const MAX_CHARS: usize = 16;
    let mut out = String::new();
    for c in snippet.chars().take(MAX_CHARS) {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    if snippet.chars().count() > MAX_CHARS {
        out.push('…');
    }
    out
}

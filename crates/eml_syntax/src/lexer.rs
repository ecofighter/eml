//! 字句解析 (構文設計 spec §3)。logos で単純なトークンを切り出し、文字列・コメント・演算子の分類などを
//! 手書きの層で扱う。トークン列をつなげると元のテキストに戻る (lossless)。

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};
use logos::Logos;

use crate::SyntaxKind::{self, *};
use crate::{NOT_YET_SUPPORTED_LABEL, codes};

/// 字句解析の結果の1トークン。trivia (空白とコメント) も含む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub range: TextRange,
}

/// テキストをトークン列に分ける。認識できない文字の並びは1つの `ERROR_TOKEN` にまとめ、診断を1件出す。
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

/// 演算子の文字の並びを分類する。予約記号 (spec §3) は演算子にならない。
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

/// logos で切り出す単純なトークン。キーワードや演算子の種類は、切り出した後で決める。
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum Raw {
    #[regex(r"[ \t\r\n\u{FEFF}]+")]
    Whitespace,
    #[regex(r"[a-z_][A-Za-z0-9_']*")]
    Lower,
    #[regex(r"[A-Z][A-Za-z0-9_']*")]
    Upper,
    /// 整数、指数だけの浮動小数 (`1e9`)、形の不正な数値 (`0xZZ`)。種類は `number_kind` で決める。
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
    /// 次に読むバイト位置。常に文字の境界にある。
    pos: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl Lexer<'_> {
    fn run(&mut self) {
        // shebang はファイルの先頭 (BOM の直後を含む) にだけ書ける。
        let shebang_at = if self.text.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        let text = self.text;
        while self.pos < text.len() {
            let rest = &text[self.pos..];
            if self.pos == shebang_at && rest.starts_with("#!") {
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

    /// `self.pos` から `end` までを1つのトークンにする。連続する `ERROR_TOKEN` は1つにまとめる。
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

    /// logos で1トークンを切り出す。
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
            Ok(Raw::Number) => number_kind(slice).unwrap_or_else(|| {
                self.error(
                    codes::INVALID_NUMBER,
                    format!("invalid number literal `{slice}`"),
                    self.pos,
                    self.pos + len,
                    "not a valid number",
                );
                INT
            }),
            Ok(Raw::Float) => {
                if self.split_field_index(len) {
                    return;
                }
                FLOAT
            }
            Ok(Raw::Char) => CHAR,
            Ok(Raw::Op) if slice.len() >= 2 && slice.bytes().all(|b| b == b'-') => {
                // `-` が2つ以上並び、その後ろに演算子の文字が続かなければ、行コメント (Haskell と同じ)。
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

    /// `t.0.1` の `0.1` のように、空白なしの `.` の直後にある `数字.数字` を、`INT` `DOT` `INT` に分ける。
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

    /// 通常の文字列 `"..."`。行をまたげない。閉じていなければ行末までを `STRING` にする。
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

    /// `i` にある `\` から始まるエスケープを読み、その次の位置を返す。不正なら E0008 を出す。
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

    /// `\u{XXXX}`。16進で1〜6桁の、Unicode のスカラー値であること。
    fn unicode_escape(&mut self, i: usize) -> usize {
        let text = self.text;
        let rest = &text[i + 2..];
        let line = &rest[..line_len(rest)];
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

    /// 補間 `\{...}` は S2 で実装する。S1 では対応する `}` まで読み飛ばし、E0004 を出す。
    /// 穴の中の文字列も読み飛ばすので、`"\{f "x"}"` の内側の `"` で外側の文字列が終わらない。
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

    /// 複数行の文字列 `"""..."""` は S2 で実装する。S1 では閉じの `"""` までを1つのトークンにする。
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

    /// raw 文字列 `r"..."` / `r#"..."#` は S2 で実装する。S1 では閉じまでを1つのトークンにする。行をまたげる。
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

    /// コマンドリテラルは S3 で実装する。S1 では閉じのバッククォートまでを1つのトークンにする。行はまたげない。
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

    /// 入れ子にできるブロックコメント `{- ... -}`。
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

/// 行末 (`\n` または `\r\n`) の手前までの長さ。
fn line_len(rest: &str) -> usize {
    let end = rest.find('\n').unwrap_or(rest.len());
    if rest[..end].ends_with('\r') {
        end - 1
    } else {
        end
    }
}

/// 補間の穴の中の文字列を読み飛ばす。`j` は開きの `"` の位置。閉じの `"` の次 (なければ行末) を返す。
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

/// `r"` または `r#..#"` で始まっていれば、`#` の数を返す。
fn raw_string_hashes(rest: &str) -> Option<usize> {
    let after_r = rest.strip_prefix('r')?;
    let hashes = after_r.bytes().take_while(|&b| b == b'#').count();
    after_r[hashes..].starts_with('"').then_some(hashes)
}

/// `Raw::Number` の種類。整数 (10進、`0x`、`0o`、`0b`) なら `INT`、`1e9` の形なら `FLOAT`、不正なら `None`。
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

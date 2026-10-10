//! 規則は docs/spec/lexical.md。CST を lossless にするため、trivia も含めてトークン列をつなげると元のテキストに戻る。

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};
use logos::Logos;

use crate::SyntaxKind::{self, *};
use crate::codes;
use crate::literal::int_value;
use string::raw_string_hashes;

mod string;

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
        modes: vec![Mode::Code {
            braces: 0,
            hole: None,
        }],
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
        "=>" => FAT_ARROW,
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

/// lexer のモード。底は `Code { hole: None }` で、穴の中は `Code { hole: Some(..) }` である
/// (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「モードのスタック」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `braces` は開いている `{` の数で、穴の閉じを見分けるために穴の中でだけ使う。`hole` は穴の `\{` の位置で、
    /// 閉じていない穴の診断に使う。
    Code {
        braces: u32,
        hole: Option<usize>,
    },
    /// `reported` は、閉じていないことを報告済みか。穴の中の `"""` は開いた時点で E0014 を出すので、行の終わりで
    /// 閉じるときに E0002 を重ねない (Task 3)。
    String {
        start: usize,
        multiline: bool,
        reported: bool,
    },
    Command {
        start: usize,
    },
}

struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    /// 常に文字の境界に置く。`&text[pos..]` で panic しないため。
    pos: usize,
    /// 空にならない。底の `Code` は取り除かない。
    modes: Vec<Mode>,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl Lexer<'_> {
    fn run(&mut self) {
        let text = self.text;
        while self.pos < text.len() {
            match *self.modes.last().expect("the base mode is never popped") {
                Mode::Code { hole, .. } => self.code(hole.is_some()),
                Mode::String { multiline, .. } => self.string_body(multiline),
                Mode::Command { .. } => self.command_body(),
            }
        }
        self.close_layers(false, true);
    }

    /// 穴の中では、トークンを改行の手前で切る。穴の中に改行は書けないので、改行に着いたら穴を閉じる
    /// (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「穴の中の改行」)。
    fn code(&mut self, in_hole: bool) {
        let text = self.text;
        let rest = &text[self.pos..];
        if in_hole && (rest.starts_with('\n') || rest.starts_with("\r\n")) {
            self.close_layers(false, false);
        } else if self.pos == 0 && rest.starts_with("#!") {
            // shebang はファイルの先頭にだけ書ける。先頭の BOM は読み込み時に除いてある (docs/spec/lexical.md)。
            self.push(SHEBANG, self.pos + line_len(rest));
        } else if rest.starts_with("{-") {
            self.block_comment(in_hole);
        } else if rest.starts_with("\"\"\"") {
            self.multiline_string();
        } else if rest.starts_with('"') {
            let start = self.pos;
            self.push(STRING_START, start + 1);
            self.modes.push(Mode::String {
                start,
                multiline: false,
                reported: false,
            });
        } else if let Some(hashes) = raw_string_hashes(rest) {
            self.raw_string(hashes);
        } else if rest.starts_with('`') {
            let start = self.pos;
            self.push(CMD_START, start + 1);
            self.modes.push(Mode::Command { start });
        } else if in_hole && rest.starts_with([' ', '\t', '\r']) {
            // `Raw::Whitespace` は改行も飲み込むので、穴の中では改行の手前までを空白にする。`\r` は、後ろが `\n` の
            // ときだけ改行の一部である
            let len = rest
                .char_indices()
                .find(|&(i, c)| {
                    !matches!(c, ' ' | '\t' | '\r')
                        || (c == '\r' && rest[i + 1..].starts_with('\n'))
                })
                .map_or(rest.len(), |(i, _)| i);
            self.push(WHITESPACE, self.pos + len);
        } else {
            self.simple();
        }
    }

    /// 改行かファイルの終わりで、閉じていない層を上から閉じる。止まるのは穴の外の複数行の文字列か底で、`at_eof` なら
    /// 複数行の文字列も閉じる。報告は一番内側の層の1件だけにする。1つの書き忘れに、層の数だけ誤りを並べないため
    /// (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「改行での回復」)。
    fn close_layers(&mut self, mut reported: bool, at_eof: bool) {
        loop {
            match *self.modes.last().expect("the base mode is never popped") {
                Mode::Code { hole: None, .. } => return,
                Mode::Code {
                    hole: Some(start), ..
                } => {
                    if !reported {
                        self.error(
                            codes::UNTERMINATED_STRING,
                            "unterminated string interpolation",
                            start,
                            start + 2,
                            "missing closing `}`",
                        );
                        reported = true;
                    }
                    // パーサとレイアウト段が穴の終わりを知るための、幅 0 の閉じ。`push` は `ERROR_TOKEN` をまとめるので
                    // 通さない
                    let at = self.range(self.pos, self.pos);
                    self.tokens.push(Token {
                        kind: INTERP_END,
                        range: at,
                    });
                }
                Mode::String {
                    multiline: true, ..
                } if !at_eof => return,
                Mode::String {
                    start,
                    multiline,
                    reported: already,
                } => {
                    if !reported && !already {
                        let (end, label, message) = if multiline {
                            (
                                start + 3,
                                "missing closing `\"\"\"`",
                                "unterminated multi-line string",
                            )
                        } else {
                            (
                                self.pos,
                                "missing closing `\"`",
                                "unterminated string literal",
                            )
                        };
                        self.error(codes::UNTERMINATED_STRING, message, start, end, label);
                    }
                    reported = true;
                }
                Mode::Command { start } => {
                    if !reported {
                        self.error(
                            codes::UNTERMINATED_STRING,
                            "unterminated command literal",
                            start,
                            self.pos,
                            "missing closing backtick",
                        );
                        reported = true;
                    }
                }
            }
            self.modes.pop();
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
            Ok(Raw::LBrace) => {
                if let Some(Mode::Code { braces, .. }) = self.modes.last_mut() {
                    *braces += 1;
                }
                L_BRACE
            }
            Ok(Raw::RBrace) => match self.modes.last_mut() {
                Some(Mode::Code {
                    braces: 0,
                    hole: Some(_),
                }) => {
                    self.push(INTERP_END, self.pos + len);
                    self.modes.pop();
                    return;
                }
                Some(Mode::Code { braces, .. }) => {
                    *braces = braces.saturating_sub(1);
                    R_BRACE
                }
                _ => R_BRACE,
            },
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

    /// 穴の中では改行の手前で止める。閉じていなければ、E0005 を一番内側の層の報告にして、外側の層を黙って閉じる
    /// (docs/superpowers/specs/2026-10-10-s6b-strings-design.md の「改行での回復」)。
    fn block_comment(&mut self, in_hole: bool) {
        let text = self.text;
        let start = self.pos;
        let mut i = start + 2;
        let mut depth = 1;
        while i < text.len() {
            let rest = &text[i..];
            if in_hole && (rest.starts_with('\n') || rest.starts_with("\r\n")) {
                break;
            } else if rest.starts_with("{-") {
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
        if in_hole && depth > 0 {
            self.close_layers(true, i == text.len());
        }
    }
}

pub(crate) fn line_len(rest: &str) -> usize {
    let end = rest.find('\n').unwrap_or(rest.len());
    if rest[..end].ends_with('\r') {
        end - 1
    } else {
        end
    }
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
        "extern" => EXTERN_KW,
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
        "deriving" => DERIVING_KW,
        _ => return None,
    })
}

/// 診断のメッセージに埋め込むため、見えない文字をエスケープし、長い並びを切り詰める。
fn printable(snippet: &str) -> String {
    const MAX_CHARS: usize = 16;
    let mut out = String::new();
    for c in snippet.chars().take(MAX_CHARS) {
        if is_invisible(c) {
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

/// 制御文字と、幅を持たない書式の文字 (BOM、ゼロ幅の空白、書字方向の制御など)。そのまま埋め込むと、メッセージの中で
/// 何の文字か分からない。
fn is_invisible(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{feff}'
        )
}

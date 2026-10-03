use logos::Logos;

/// トークンと構文ノードの種類。トークン (`EOF` まで) を先に並べ、`TokenSet` が 128 ビットに収まるようにする。
/// 字句の規則は spec §7 の「字句」に従う。
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // trivia
    #[regex(r"[ \t\r\n\u{FEFF}]+")]
    WHITESPACE = 0,
    #[regex(r"//[^\r\n]*", allow_greedy = true)]
    COMMENT,

    // リテラルと識別子
    #[regex(r"[0-9]+")]
    INT,
    #[regex(r#""([^"\\\n]|\\[^\n])*""#)]
    STRING,
    /// 閉じていない文字列。字句解析の段階で `STRING` に変換し、診断を出す。木には現れない。
    #[regex(r#""([^"\\\n]|\\[^\n])*"#)]
    UNTERMINATED_STRING,
    #[regex(r"[a-z][A-Za-z0-9_]*|_[A-Za-z0-9_]+")]
    LIDENT,
    #[regex(r"[A-Z][A-Za-z0-9_]*")]
    UIDENT,
    #[token("_")]
    UNDERSCORE,

    // キーワード
    #[token("fn")]
    FN_KW,
    #[token("let")]
    LET_KW,
    #[token("if")]
    IF_KW,
    #[token("then")]
    THEN_KW,
    #[token("else")]
    ELSE_KW,
    #[token("match")]
    MATCH_KW,
    #[token("type")]
    TYPE_KW,
    #[token("effect")]
    EFFECT_KW,
    #[token("handle")]
    HANDLE_KW,
    #[token("resume")]
    RESUME_KW,
    #[token("drop")]
    DROP_KW,
    #[token("return")]
    RETURN_KW,
    #[token("never")]
    NEVER_KW,
    #[token("once")]
    ONCE_KW,
    #[token("multi")]
    MULTI_KW,
    #[token("true")]
    TRUE_KW,
    #[token("false")]
    FALSE_KW,

    // 記号
    #[token("(")]
    L_PAREN,
    #[token(")")]
    R_PAREN,
    #[token("{")]
    L_BRACE,
    #[token("}")]
    R_BRACE,
    #[token(",")]
    COMMA,
    #[token(";")]
    SEMICOLON,
    #[token(":")]
    COLON,
    #[token("=")]
    EQ,
    #[token("->")]
    THIN_ARROW,
    #[token("=>")]
    FAT_ARROW,
    #[token("|")]
    PIPE,
    #[token("<")]
    LT,
    #[token(">")]
    GT,
    #[token("+")]
    PLUS,
    #[token("-")]
    MINUS,
    #[token("*")]
    STAR,
    #[token("/")]
    SLASH,
    #[token("%")]
    PERCENT,
    #[token("++")]
    PLUS2,
    #[token("==")]
    EQ2,
    #[token("!=")]
    NEQ,
    #[token("<=")]
    LTEQ,
    #[token(">=")]
    GTEQ,
    #[token("&&")]
    AMP2,
    #[token("||")]
    PIPE2,
    #[token("!")]
    BANG,

    /// 字句として認識できない文字の並び。
    ERROR_TOKEN,
    /// 入力の終わり。パーサの中でだけ使い、木には現れない。
    EOF,

    // ノード
    SOURCE_FILE,
    ERROR,

    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(self, SyntaxKind::WHITESPACE | SyntaxKind::COMMENT)
    }

    fn from_raw(raw: u16) -> SyntaxKind {
        assert!(raw < SyntaxKind::__LAST as u16, "invalid SyntaxKind {raw}");
        // SAFETY: `SyntaxKind` は `repr(u16)` で、0 から `__LAST` まで値が連続している。
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EmlLanguage {}

impl rowan::Language for EmlLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from_raw(raw.0)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<EmlLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<EmlLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<EmlLanguage>;
pub type SyntaxNodePtr = rowan::ast::SyntaxNodePtr<EmlLanguage>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_round_trip() {
        for raw in 0..SyntaxKind::__LAST as u16 {
            assert_eq!(SyntaxKind::from_raw(raw) as u16, raw);
        }
    }

    #[test]
    fn tokens_fit_in_token_set() {
        assert!((SyntaxKind::EOF as u16) < 128);
    }
}

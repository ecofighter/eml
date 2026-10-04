/// トークン (`EOF` まで) を先に並べるのは、`TokenSet` の 128 ビットに収めるため。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    WHITESPACE = 0,
    /// ドキュメントコメント `-- |` もこれにする。解釈するのはドキュメント生成と LSP で、字句では区別しない。
    COMMENT,
    BLOCK_COMMENT,
    SHEBANG,

    INT,
    // FLOAT、CHAR、MULTILINE_STRING、RAW_STRING、COMMAND は後の段階で実装する構文。
    // 使われた位置で E0004 を出せるように、字句だけ先に用意している。
    FLOAT,
    CHAR,
    /// 補間を含んでいても1つのトークンにする。部品に分けるのは補間を実装する S2 から。
    STRING,
    MULTILINE_STRING,
    RAW_STRING,
    COMMAND,
    LIDENT,
    UIDENT,
    UNDERSCORE,

    DATA_KW,
    TYPE_KW,
    EFFECT_KW,
    WHERE_KW,
    PUB_KW,
    IMPORT_KW,
    AS_KW,
    INFIXL_KW,
    INFIXR_KW,
    INFIX_KW,
    LET_KW,
    IN_KW,
    IF_KW,
    THEN_KW,
    ELSE_KW,
    MATCH_KW,
    WITH_KW,
    HANDLE_KW,
    FROM_KW,
    RESUME_KW,
    DROP_KW,
    RETURN_KW,
    NEVER_KW,
    ONCE_KW,
    MULTI_KW,
    USE_KW,
    FN_KW,
    /// 将来のために予約する。
    FORALL_KW,
    CLASS_KW,
    INSTANCE_KW,

    L_PAREN,
    R_PAREN,
    L_BRACK,
    R_BRACK,
    L_BRACE,
    R_BRACE,
    COMMA,
    SEMICOLON,

    EQ,
    PIPE,
    COLON,
    DOT,
    THIN_ARROW,
    LEFT_ARROW,
    DOT2,

    OP,
    CONOP,
    /// `OP` と分けるのは、前置の負号としても使うため。
    MINUS,

    /// row の括弧かどうかは型の中でしか分からないので、lexer は `OP` にし、parser が付け替える (`<>` などは分割する)。
    L_ANGLE,
    R_ANGLE,

    /// レイアウト段の仮想トークン。CST を lossless に保つため、木には入れない。
    LAYOUT_OPEN,
    LAYOUT_SEP,
    LAYOUT_CLOSE,

    ERROR_TOKEN,
    /// パーサの中でだけ使い、木には現れない。
    EOF,

    SOURCE_FILE,
    ERROR,
    SIGNATURE,
    EQUATION,
    DATA_ITEM,
    ALT,
    TYPE_ITEM,
    EFFECT_ITEM,
    OP_DECL,
    FIXITY_ITEM,

    BLOCK,
    LET_STMT,
    USE_STMT,
    EXPR_STMT,

    IF_EXPR,
    MATCH_EXPR,
    MATCH_ARM,
    HANDLE_EXPR,
    OP_CLAUSE,
    RETURN_CLAUSE,
    LAMBDA_EXPR,
    LET_EXPR,
    /// fixity はユーザーが宣言し、名前解決の後でないと分からない。そのため演算子の列は平たく並べ、HIR で木に組み直す
    /// (docs/spec/expressions.md)。前置の `-` もトークンのまま入る。
    OP_SEQ,
    APP_EXPR,
    RESUME_EXPR,
    DROP_EXPR,
    FIELD_EXPR,
    PATH_EXPR,
    LITERAL,
    UNIT_EXPR,
    PAREN_EXPR,
    TUPLE_EXPR,
    ANNOT_EXPR,
    OP_REF,
    LEFT_SECTION,
    RIGHT_SECTION,
    FIELD_SECTION,

    WILDCARD_PAT,
    BIND_PAT,
    CON_PAT,
    LITERAL_PAT,
    UNIT_PAT,
    PAREN_PAT,
    TUPLE_PAT,
    INFIX_CON_PAT,
    ANNOT_PAT,

    PATH_TYPE,
    VAR_TYPE,
    APP_TYPE,
    FN_TYPE,
    PAREN_TYPE,
    TUPLE_TYPE,
    EFFECT_ROW,
    EFFECT,

    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    /// 括弧の種類の判定はここだけに置く。S2 で補間の `\{` と `}` を足すときも、ここに足す (docs/spec/layout.md の規則 4)。
    pub fn is_opening_bracket(self) -> bool {
        matches!(
            self,
            SyntaxKind::L_PAREN | SyntaxKind::L_BRACK | SyntaxKind::L_BRACE
        )
    }

    pub fn is_closing_bracket(self) -> bool {
        matches!(
            self,
            SyntaxKind::R_PAREN | SyntaxKind::R_BRACK | SyntaxKind::R_BRACE
        )
    }

    pub fn is_virtual(self) -> bool {
        matches!(
            self,
            SyntaxKind::LAYOUT_OPEN | SyntaxKind::LAYOUT_SEP | SyntaxKind::LAYOUT_CLOSE
        )
    }

    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE
                | SyntaxKind::COMMENT
                | SyntaxKind::BLOCK_COMMENT
                | SyntaxKind::SHEBANG
        )
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

    #[test]
    fn brackets() {
        for kind in [
            SyntaxKind::L_PAREN,
            SyntaxKind::L_BRACK,
            SyntaxKind::L_BRACE,
        ] {
            assert!(kind.is_opening_bracket() && !kind.is_closing_bracket());
        }
        for kind in [
            SyntaxKind::R_PAREN,
            SyntaxKind::R_BRACK,
            SyntaxKind::R_BRACE,
        ] {
            assert!(kind.is_closing_bracket() && !kind.is_opening_bracket());
        }
        assert!(!SyntaxKind::LAYOUT_OPEN.is_opening_bracket());
    }
}

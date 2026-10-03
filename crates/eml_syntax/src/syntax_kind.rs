/// トークンと構文ノードの種類。トークン (`EOF` まで) を先に並べ、`TokenSet` が 128 ビットに収まるようにする。
/// 字句の規則は構文設計 spec §3 に従う。字句解析は `lexer` が行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // trivia
    WHITESPACE = 0,
    /// `--` から行末まで。`-- |` のドキュメントコメントも字句としてはこれ。
    COMMENT,
    /// 入れ子にできる `{- -}`。
    BLOCK_COMMENT,
    /// ファイルの先頭の `#!` の行。
    SHEBANG,

    // リテラルと識別子
    INT,
    /// 浮動小数。S1 では使うと E0004。
    FLOAT,
    /// 文字。S1 では使うと E0004。
    CHAR,
    /// 通常の文字列。S1 では補間を含めて1つのトークン。
    STRING,
    /// `"""` の複数行の文字列。S1 では使うと E0004。
    MULTILINE_STRING,
    /// `r"..."` / `r#"..."#`。S1 では使うと E0004。
    RAW_STRING,
    /// バッククォートのコマンドリテラル。S1 では使うと E0004。
    COMMAND,
    LIDENT,
    UIDENT,
    UNDERSCORE,

    // キーワード
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

    // 区切り記号
    L_PAREN,
    R_PAREN,
    L_BRACK,
    R_BRACK,
    L_BRACE,
    R_BRACE,
    COMMA,
    SEMICOLON,

    // 予約記号 (演算子にならない)
    EQ,
    PIPE,
    COLON,
    DOT,
    THIN_ARROW,
    LEFT_ARROW,
    DOT2,

    // 演算子
    /// ユーザーが定義できる演算子。
    OP,
    /// `:` で始まる、コンストラクタの演算子。
    CONOP,
    /// `-`。中置の引き算と、前置の負号の両方に使う。
    MINUS,

    /// row の `<` と `>`。lexer は `OP` にし、parser が型の中で付け替える (分割することもある)。
    L_ANGLE,
    R_ANGLE,

    /// レイアウト段の仮想トークン。parser の入力にだけ現れ、木には入らない。
    LAYOUT_OPEN,
    LAYOUT_SEP,
    LAYOUT_CLOSE,

    /// 字句として認識できない文字の並び。
    ERROR_TOKEN,
    /// 入力の終わり。パーサの中でだけ使い、木には現れない。
    EOF,

    // ノード
    SOURCE_FILE,
    ERROR,
    // 項目
    SIGNATURE,
    EQUATION,
    DATA_ITEM,
    /// `data` の1つの選択肢 (`| Some a`、`| a :: List a`)。
    ALT,
    TYPE_ITEM,
    EFFECT_ITEM,
    /// エフェクトの1つの操作の宣言。
    OP_DECL,
    FIXITY_ITEM,

    // 文
    /// 字下げしたブロック。仮想トークンは木に入らないので、子は文だけ。
    BLOCK,
    LET_STMT,
    USE_STMT,
    EXPR_STMT,

    // 式
    IF_EXPR,
    MATCH_EXPR,
    MATCH_ARM,
    HANDLE_EXPR,
    /// handler の操作の節 (`| op x k -> e`)。
    OP_CLAUSE,
    /// handler の `return` の節。
    RETURN_CLAUSE,
    LAMBDA_EXPR,
    /// `let p = e in e2`。
    LET_EXPR,
    /// 演算子の列。被演算子と演算子のトークンを平たく並べる。前置の `-` もトークンとして入る (spec §7)。
    OP_SEQ,
    /// 関数適用。最初の子が関数、残りが引数。
    APP_EXPR,
    RESUME_EXPR,
    DROP_EXPR,
    /// `e.name`、`e.0`。
    FIELD_EXPR,
    /// 変数、コンストラクタ、修飾された名前。
    PATH_EXPR,
    LITERAL,
    UNIT_EXPR,
    PAREN_EXPR,
    TUPLE_EXPR,
    /// `(e : T)`。
    ANNOT_EXPR,
    /// `(+)`。
    OP_REF,
    /// `(1 +)`。
    LEFT_SECTION,
    /// `(+ 1)`。
    RIGHT_SECTION,
    /// `(.name)`。
    FIELD_SECTION,

    // パターン
    WILDCARD_PAT,
    BIND_PAT,
    CON_PAT,
    LITERAL_PAT,
    UNIT_PAT,
    PAREN_PAT,
    TUPLE_PAT,
    /// `x :: rest`。
    INFIX_CON_PAT,
    /// ラムダの引数の `(x : Int)`。
    ANNOT_PAT,

    // 型
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
    /// レイアウト段の仮想トークンか。parser は読んでもイベントを出さない。
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

//! 型付き AST ラッパ。rowan の木の上の薄い型付きの見方を提供する。
//! ノードごとの構造体と、項目・文・式・パターン・型の enum を持つ。
//! アクセサは、HIR への変換で必要になったものから足していく。

use rowan::NodeOrToken;
use rowan::ast::{AstChildren, AstNode, support};

use crate::{EmlLanguage, SyntaxKind, SyntaxNode, SyntaxToken};

/// ノードの種類1つに対応する構造体を定義する。
macro_rules! ast_node {
    ($($(#[$meta:meta])* $name:ident => $kind:ident,)*) => {$(
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name {
            syntax: SyntaxNode,
        }

        impl AstNode for $name {
            type Language = EmlLanguage;

            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                Self::can_cast(syntax.kind()).then_some($name { syntax })
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.syntax
            }
        }
    )*};
}

/// いくつかのノードの構造体をまとめる enum を定義する。
macro_rules! ast_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant($variant),)*
        }

        impl AstNode for $name {
            type Language = EmlLanguage;

            fn can_cast(kind: SyntaxKind) -> bool {
                $($variant::can_cast(kind))||*
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                $(if $variant::can_cast(syntax.kind()) {
                    return $variant::cast(syntax).map($name::$variant);
                })*
                None
            }

            fn syntax(&self) -> &SyntaxNode {
                match self {
                    $($name::$variant(node) => node.syntax(),)*
                }
            }
        }
    };
}

ast_node! {
    /// ファイル全体。
    SourceFile => SOURCE_FILE,
    Signature => SIGNATURE,
    Equation => EQUATION,
    DataItem => DATA_ITEM,
    Alt => ALT,
    TypeItem => TYPE_ITEM,
    EffectItem => EFFECT_ITEM,
    OpDecl => OP_DECL,
    FixityItem => FIXITY_ITEM,
    Block => BLOCK,
    LetStmt => LET_STMT,
    UseStmt => USE_STMT,
    ExprStmt => EXPR_STMT,
    IfExpr => IF_EXPR,
    MatchExpr => MATCH_EXPR,
    MatchArm => MATCH_ARM,
    HandleExpr => HANDLE_EXPR,
    OpClause => OP_CLAUSE,
    ReturnClause => RETURN_CLAUSE,
    LambdaExpr => LAMBDA_EXPR,
    LetExpr => LET_EXPR,
    OpSeq => OP_SEQ,
    AppExpr => APP_EXPR,
    ResumeExpr => RESUME_EXPR,
    DropExpr => DROP_EXPR,
    FieldExpr => FIELD_EXPR,
    PathExpr => PATH_EXPR,
    Literal => LITERAL,
    UnitExpr => UNIT_EXPR,
    ParenExpr => PAREN_EXPR,
    TupleExpr => TUPLE_EXPR,
    AnnotExpr => ANNOT_EXPR,
    OpRef => OP_REF,
    LeftSection => LEFT_SECTION,
    RightSection => RIGHT_SECTION,
    FieldSection => FIELD_SECTION,
    WildcardPat => WILDCARD_PAT,
    BindPat => BIND_PAT,
    ConPat => CON_PAT,
    LiteralPat => LITERAL_PAT,
    UnitPat => UNIT_PAT,
    ParenPat => PAREN_PAT,
    TuplePat => TUPLE_PAT,
    InfixConPat => INFIX_CON_PAT,
    AnnotPat => ANNOT_PAT,
    PathType => PATH_TYPE,
    VarType => VAR_TYPE,
    AppType => APP_TYPE,
    FnType => FN_TYPE,
    ParenType => PAREN_TYPE,
    TupleType => TUPLE_TYPE,
    EffectRow => EFFECT_ROW,
    Effect => EFFECT,
}

ast_enum! {
    /// トップレベルの項目。
    Item { Signature, Equation, DataItem, TypeItem, EffectItem, FixityItem }
}

ast_enum! {
    /// ブロックの中の文。
    Stmt { LetStmt, UseStmt, ExprStmt }
}

ast_enum! {
    /// 式。本体の位置の字下げしたブロック (`Block`) も式として扱う。
    Expr {
        Block, IfExpr, MatchExpr, HandleExpr, LambdaExpr, LetExpr, OpSeq, AppExpr, ResumeExpr,
        DropExpr, FieldExpr, PathExpr, Literal, UnitExpr, ParenExpr, TupleExpr, AnnotExpr, OpRef,
        LeftSection, RightSection, FieldSection,
    }
}

ast_enum! {
    /// パターン。
    Pat {
        WildcardPat, BindPat, ConPat, LiteralPat, UnitPat, ParenPat, TuplePat, InfixConPat,
        AnnotPat,
    }
}

ast_enum! {
    /// 型。
    Type { PathType, VarType, AppType, FnType, ParenType, TupleType }
}

impl SourceFile {
    pub fn items(&self) -> AstChildren<Item> {
        support::children(&self.syntax)
    }
}

impl Signature {
    /// 名前のトークン (`LIDENT`、または `(OP)` の演算子)。
    pub fn name(&self) -> Option<SyntaxToken> {
        name_token(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl Equation {
    /// 定義する名前のトークン。関数なら `LIDENT`、演算子の定義なら演算子。
    pub fn name(&self) -> Option<SyntaxToken> {
        name_token(&self.syntax)
    }

    /// 引数のパターン。演算子の定義では左辺と右辺。
    pub fn params(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl Block {
    pub fn stmts(&self) -> AstChildren<Stmt> {
        support::children(&self.syntax)
    }
}

/// 演算子の列の要素。前置の `-` は、列の先頭か別の演算子の直後にある `Operator` として現れる (spec §7)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpSeqElement {
    Operand(Expr),
    Operator(SyntaxToken),
}

impl OpSeq {
    pub fn elements(&self) -> impl Iterator<Item = OpSeqElement> {
        self.syntax
            .children_with_tokens()
            .filter_map(|element| match element {
                NodeOrToken::Node(node) => Expr::cast(node).map(OpSeqElement::Operand),
                NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::OP | SyntaxKind::CONOP | SyntaxKind::MINUS
                    ) =>
                {
                    Some(OpSeqElement::Operator(token))
                }
                NodeOrToken::Token(_) => None,
            })
    }
}

impl Literal {
    /// リテラルのトークン (`INT`、`STRING` など)。
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax.first_token()
    }
}

impl PathExpr {
    /// 名前の部分 (`Foo.bar` なら `Foo` と `bar`)。
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| matches!(token.kind(), SyntaxKind::UIDENT | SyntaxKind::LIDENT))
    }
}

/// 直接の子のトークンのうち、最初の名前 (`LIDENT`) か演算子。
fn name_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| {
            matches!(
                token.kind(),
                SyntaxKind::LIDENT | SyntaxKind::OP | SyntaxKind::MINUS
            )
        })
}

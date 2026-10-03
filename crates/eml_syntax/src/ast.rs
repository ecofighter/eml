//! アクセサは、HIR への変換で必要になったものから足していく。

use rowan::NodeOrToken;
use rowan::ast::{AstChildren, AstNode, support};

use crate::{EmlLanguage, SyntaxKind, SyntaxNode, SyntaxToken};

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
    Item { Signature, Equation, DataItem, TypeItem, EffectItem, FixityItem }
}

ast_enum! {
    Stmt { LetStmt, UseStmt, ExprStmt }
}

ast_enum! {
    /// `body ::= block(stmt) | expr` を1つの型で受けられるように、字下げしたブロック (`Block`) も式に含める。
    Expr {
        Block, IfExpr, MatchExpr, HandleExpr, LambdaExpr, LetExpr, OpSeq, AppExpr, ResumeExpr,
        DropExpr, FieldExpr, PathExpr, Literal, UnitExpr, ParenExpr, TupleExpr, AnnotExpr, OpRef,
        LeftSection, RightSection, FieldSection,
    }
}

ast_enum! {
    Pat {
        WildcardPat, BindPat, ConPat, LiteralPat, UnitPat, ParenPat, TuplePat, InfixConPat,
        AnnotPat,
    }
}

ast_enum! {
    Type { PathType, VarType, AppType, FnType, ParenType, TupleType }
}

impl SourceFile {
    pub fn items(&self) -> AstChildren<Item> {
        support::children(&self.syntax)
    }
}

impl Signature {
    pub fn name(&self) -> Option<SyntaxToken> {
        name_token(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl Equation {
    pub fn name(&self) -> Option<SyntaxToken> {
        name_token(&self.syntax)
    }

    /// 演算子の定義では、左辺と右辺のパターンが入る。
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

/// 前置の `-` は、列の先頭か別の演算子の直後にある `Operator` として現れる。単項マイナスも fixity と一緒に HIR で
/// 組み直すため (docs/spec/expressions.md)。
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
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax.first_token()
    }
}

impl PathExpr {
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        path_segments(&self.syntax)
    }
}

impl LetStmt {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl ExprStmt {
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl IfExpr {
    pub fn condition(&self) -> Option<Expr> {
        child_between(
            &self.syntax,
            Some(SyntaxKind::IF_KW),
            Some(SyntaxKind::THEN_KW),
        )
    }

    pub fn then_branch(&self) -> Option<Expr> {
        child_between(
            &self.syntax,
            Some(SyntaxKind::THEN_KW),
            Some(SyntaxKind::ELSE_KW),
        )
    }

    pub fn else_branch(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::ELSE_KW), None)
    }
}

impl AppExpr {
    pub fn callee(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn args(&self) -> impl Iterator<Item = Expr> {
        support::children::<Expr>(&self.syntax).skip(1)
    }
}

impl ParenExpr {
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl AnnotExpr {
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl BindPat {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::LIDENT)
    }
}

impl ParenPat {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }
}

impl PathType {
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        path_segments(&self.syntax)
    }
}

impl ParenType {
    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl FnType {
    pub fn param(&self) -> Option<Type> {
        child_between(&self.syntax, None, Some(SyntaxKind::THIN_ARROW))
    }

    pub fn row(&self) -> Option<EffectRow> {
        support::child(&self.syntax)
    }

    pub fn ret(&self) -> Option<Type> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl EffectRow {
    pub fn effects(&self) -> AstChildren<Effect> {
        support::children(&self.syntax)
    }

    /// `<IO | e>` と `<e>` の row 変数。
    pub fn tail(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::LIDENT)
    }
}

impl Effect {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::UIDENT)
    }
}

fn path_segments(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| matches!(token.kind(), SyntaxKind::UIDENT | SyntaxKind::LIDENT))
}

/// 欠けた部分があっても前後の部分を取り違えないように、区切りのトークンの間で子を探す。
fn child_between<N: AstNode<Language = EmlLanguage>>(
    node: &SyntaxNode,
    after: Option<SyntaxKind>,
    before: Option<SyntaxKind>,
) -> Option<N> {
    let mut started = after.is_none();
    for element in node.children_with_tokens() {
        let kind = element.kind();
        if !started {
            started = Some(kind) == after;
            continue;
        }
        if Some(kind) == before {
            return None;
        }
        if let Some(found) = element.into_node().and_then(N::cast) {
            return Some(found);
        }
    }
    None
}

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

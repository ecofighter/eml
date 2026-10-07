//! アクセサは、HIR への変換で必要になったものから足していく。

use eml_diagnostics::TextRange;
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

        impl $name {
            pub fn range(&self) -> TextRange {
                self.syntax.text_range()
            }

            /// 最初のトークンの範囲。キーワードで始まる構文の診断は、本体全体ではなくキーワードだけを指すと読みやすい。
            pub fn keyword_range(&self) -> TextRange {
                keyword_range(&self.syntax)
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

        impl $name {
            pub fn range(&self) -> TextRange {
                self.syntax().text_range()
            }

            pub fn keyword_range(&self) -> TextRange {
                keyword_range(self.syntax())
            }
        }
    };
}

ast_node! {
    SourceFile => SOURCE_FILE,
    Name => NAME,
    NameRef => NAME_REF,
    Path => PATH,
    Signature => SIGNATURE,
    Equation => EQUATION,
    DataItem => DATA_ITEM,
    Alt => ALT,
    TypeItem => TYPE_ITEM,
    EffectItem => EFFECT_ITEM,
    OpDecl => OP_DECL,
    FixityItem => FIXITY_ITEM,
    ImportItem => IMPORT_ITEM,
    ImportList => IMPORT_LIST,
    ImportName => IMPORT_NAME,
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
    Item { Signature, Equation, DataItem, TypeItem, EffectItem, FixityItem, ImportItem }
}

ast_enum! {
    Stmt { LetStmt, UseStmt, ExprStmt }
}

ast_enum! {
    /// `body ::= block(stmt) | expr` を1つの型で受けられるように、字下げしたブロック (`Block`) も式に含める。
    Expr {
        Block, IfExpr, MatchExpr, HandleExpr, LambdaExpr, LetExpr, OpSeq, AppExpr,
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

ast_enum! {
    /// handler の節。
    Clause { OpClause, ReturnClause }
}

impl Name {
    /// 名前のトークン。`(+)` の形では括弧を除いた演算子のトークンである。
    pub fn token(&self) -> SyntaxToken {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::LIDENT
                        | SyntaxKind::UIDENT
                        | SyntaxKind::OP
                        | SyntaxKind::MINUS
                        | SyntaxKind::CONOP
                )
            })
            .expect("the parser puts one name token in every NAME")
    }

    pub fn text(&self) -> String {
        self.token().text().to_string()
    }
}

impl NameRef {
    pub fn token(&self) -> SyntaxToken {
        self.syntax
            .first_token()
            .expect("the parser puts one name token in every NAME_REF")
    }

    pub fn text(&self) -> String {
        self.token().text().to_string()
    }
}

impl Path {
    pub fn segments(&self) -> AstChildren<NameRef> {
        support::children(&self.syntax)
    }

    /// 最後のセグメント。修飾のない名前ならその名前である。
    pub fn name(&self) -> Option<NameRef> {
        self.segments().last()
    }
}

impl LambdaExpr {
    pub fn params(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl AnnotPat {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl VarType {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::LIDENT)
    }
}

impl SourceFile {
    pub fn items(&self) -> AstChildren<Item> {
        support::children(&self.syntax)
    }
}

impl Signature {
    pub fn is_extern(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::EXTERN_KW).is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl Equation {
    /// 関数の名前。演算子の定義では演算子である。
    pub fn name(&self) -> Option<Name> {
        support::child(&self.syntax)
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

impl Stmt {
    /// 文が行の最初のトークンで始まるとき、その行の字下げ (空白の数)。fix が文の前に行を入れるのに使う。trivia は
    /// 囲むノードに付くので、文の最初のトークンの直前のトークンが、前の行の終わりからの空白である。タブは字句の段階で
    /// 誤りなので、空白だけを数えればよい (docs/spec/lexical.md)。
    pub fn line_indent(&self) -> Option<u32> {
        let first = self.syntax().first_token()?;
        let previous = first.prev_token()?;
        if previous.kind() != SyntaxKind::WHITESPACE {
            return None;
        }
        let (_, indent) = previous.text().rsplit_once('\n')?;
        Some(indent.len() as u32)
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

/// リテラルの値。HIR はトークンの種類を見ずに、これで値を受け取る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralValue {
    Int(i64),
    String(String),
}

impl Literal {
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax.first_token()
    }

    /// 浮動小数、文字、複数行の文字列などの未対応のリテラルと、値が壊れているもの (範囲外の整数、不正なエスケープ、
    /// 閉じていない文字列) は `None` を返す。未対応のリテラルは HIR が E0004 を出し、値の壊れたものは字句解析が
    /// 報告済みである。
    pub fn value(&self) -> Option<LiteralValue> {
        let token = self.token()?;
        match token.kind() {
            SyntaxKind::INT => crate::literal::int_value(token.text()).map(LiteralValue::Int),
            SyntaxKind::STRING => {
                crate::literal::decode_string(token.text()).map(LiteralValue::String)
            }
            _ => None,
        }
    }
}

impl PathExpr {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
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

impl UseStmt {
    /// `use p <- e` の `p`。`<-` がなければ `None`。
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl LetExpr {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }

    /// `in` の前の式。
    pub fn init(&self) -> Option<Expr> {
        self.split_at_in().0
    }

    /// `in` の後の式。
    pub fn body(&self) -> Option<Expr> {
        self.split_at_in().1
    }

    /// 片方の式が欠けても取り違えないよう、`in` の位置で分ける。
    fn split_at_in(&self) -> (Option<Expr>, Option<Expr>) {
        let in_start = self
            .syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| token.kind() == SyntaxKind::IN_KW)
            .map(|token| token.text_range().start());
        let mut before = None;
        let mut after = None;
        for expr in support::children::<Expr>(&self.syntax) {
            match in_start {
                Some(start) if expr.range().start() >= start => {
                    after.get_or_insert(expr);
                }
                _ => {
                    before.get_or_insert(expr);
                }
            }
        }
        (before, after)
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
    /// 最初の子が式でなければ (構文エラーの `ERROR` ノードなど) `None` を返す。2つ目以降の子を呼ばれるものと
    /// 取り違えないため。
    pub fn callee(&self) -> Option<Expr> {
        self.syntax.first_child().and_then(Expr::cast)
    }

    pub fn args(&self) -> impl Iterator<Item = Expr> {
        self.syntax.children().skip(1).filter_map(Expr::cast)
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
    pub fn name(&self) -> Option<Name> {
        support::child(&self.syntax)
    }
}

impl ParenPat {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }
}

impl PathType {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
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
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    /// row に書いたエフェクトの型引数。
    pub fn args(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}

impl EffectItem {
    pub fn is_extern(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::EXTERN_KW).is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax)
            .find(|name| name.token().kind() == SyntaxKind::UIDENT)
    }

    /// 型引数。名前が欠けた宣言でも取り違えないよう、小文字の名前だけを返す。
    pub fn params(&self) -> impl Iterator<Item = Name> {
        lowercase_names(&self.syntax)
    }

    pub fn operations(&self) -> AstChildren<OpDecl> {
        support::children(&self.syntax)
    }
}

impl OpDecl {
    /// `never` / `once` / `multi`。省略したら `None` で、`once` として扱う (docs/spec/declarations.md)。
    pub fn multiplicity(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::NEVER_KW | SyntaxKind::ONCE_KW | SyntaxKind::MULTI_KW
                )
            })
    }

    pub fn name(&self) -> Option<Name> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl HandleExpr {
    /// handle する式。`from` があれば初期値より前、なければ `with` より前にある。
    pub fn body(&self) -> Option<Expr> {
        let before = if self.from_keyword().is_some() {
            SyntaxKind::FROM_KW
        } else {
            SyntaxKind::WITH_KW
        };
        child_between(&self.syntax, Some(SyntaxKind::HANDLE_KW), Some(before))
    }

    pub fn from_keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::FROM_KW)
    }

    /// `from` の後、`with` の前の式。`from` がなければ `None`。
    pub fn init(&self) -> Option<Expr> {
        self.from_keyword()?;
        child_between(
            &self.syntax,
            Some(SyntaxKind::FROM_KW),
            Some(SyntaxKind::WITH_KW),
        )
    }

    pub fn clauses(&self) -> AstChildren<Clause> {
        support::children(&self.syntax)
    }
}

impl OpClause {
    /// 節の先頭の操作の名前。修飾できる (docs/spec/grammar.md の `clause`)。
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    /// 操作の引数、`k`、(パラメータ付き handler なら) 状態のパターン。
    pub fn params(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl ReturnClause {
    pub fn params(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl DropExpr {
    pub fn args(&self) -> AstChildren<Expr> {
        support::children(&self.syntax)
    }
}

impl DataItem {
    pub fn is_extern(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::EXTERN_KW).is_some()
    }

    /// `=` とコンストラクタの並びを書いたか。
    /// `=` か選択肢があるか。`=` を書き忘れた選択肢はパーサが報告済みなので、コンストラクタのない `data` には数えない。
    pub fn has_constructors(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::EQ).is_some() || self.alts().next().is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax)
            .find(|name| name.token().kind() == SyntaxKind::UIDENT)
    }

    /// 型引数。名前が欠けた宣言でも取り違えないよう、小文字の名前だけを返す。
    pub fn params(&self) -> impl Iterator<Item = Name> {
        lowercase_names(&self.syntax)
    }

    pub fn alts(&self) -> AstChildren<Alt> {
        support::children(&self.syntax)
    }
}

impl Alt {
    /// 前置のコンストラクタの名前。
    pub fn name(&self) -> Option<Name> {
        self.name_of(SyntaxKind::UIDENT)
    }

    /// 中置のコンストラクタの演算子。
    pub fn operator(&self) -> Option<Name> {
        self.name_of(SyntaxKind::CONOP)
    }

    fn name_of(&self, kind: SyntaxKind) -> Option<Name> {
        support::children::<Name>(&self.syntax).find(|name| name.token().kind() == kind)
    }

    /// フィールドの型。中置のコンストラクタでは左右の2つである。
    pub fn fields(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}

impl MatchExpr {
    pub fn scrutinee(&self) -> Option<Expr> {
        child_between(
            &self.syntax,
            Some(SyntaxKind::MATCH_KW),
            Some(SyntaxKind::WITH_KW),
        )
    }

    pub fn arms(&self) -> AstChildren<MatchArm> {
        support::children(&self.syntax)
    }
}

impl MatchArm {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl ConPat {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn args(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }
}

impl InfixConPat {
    pub fn lhs(&self) -> Option<Pat> {
        child_between(&self.syntax, None, Some(SyntaxKind::CONOP))
    }

    pub fn operator(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::CONOP)
    }

    pub fn rhs(&self) -> Option<Pat> {
        child_between(&self.syntax, Some(SyntaxKind::CONOP), None)
    }
}

impl AppType {
    /// 適用する型の名前。
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn args(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}

impl TupleExpr {
    /// 要素の式。文法が2つ以上にする (docs/spec/grammar.md の `atom`)。
    pub fn elements(&self) -> AstChildren<Expr> {
        support::children(&self.syntax)
    }
}

impl TuplePat {
    pub fn elements(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }
}

impl TupleType {
    pub fn elements(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}

impl LiteralPat {
    /// `INT`、`STRING`、`CHAR` のトークン。`-1` の `-` は含まない。
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::INT | SyntaxKind::STRING | SyntaxKind::CHAR
                )
            })
    }

    /// `-1` の `-` は字句の一部ではなくパターンの一部なので (docs/spec/grammar.md の `apat`)、ここで符号を付ける。
    /// 値の壊れたリテラルと未対応のリテラル (文字) は `None` を返す。未対応のリテラルは HIR が E0004 を出し、値の
    /// 壊れたものは字句解析が報告済みである。
    pub fn value(&self) -> Option<LiteralValue> {
        let negative = support::token(&self.syntax, SyntaxKind::MINUS).is_some();
        let token = self.token()?;
        match token.kind() {
            SyntaxKind::INT => {
                let n = crate::literal::int_value(token.text())?;
                Some(LiteralValue::Int(if negative { -n } else { n }))
            }
            SyntaxKind::STRING => {
                crate::literal::decode_string(token.text()).map(LiteralValue::String)
            }
            _ => None,
        }
    }
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

fn lowercase_names(node: &SyntaxNode) -> impl Iterator<Item = Name> {
    support::children::<Name>(node).filter(|name| name.token().kind() == SyntaxKind::LIDENT)
}

impl FixityItem {
    /// `infixl`、`infixr`、`infix` のキーワード。`pub` の後にもあるので、最初のトークンとは限らない。
    pub fn assoc(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::INFIXL_KW | SyntaxKind::INFIXR_KW | SyntaxKind::INFIX_KW
                )
            })
    }

    pub fn precedence(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| token.kind() == SyntaxKind::INT)
    }

    /// 宣言した演算子。`-` と `:` で始まる演算子も含む。
    pub fn operators(&self) -> AstChildren<Name> {
        support::children(&self.syntax)
    }
}

fn is_operator(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::OP | SyntaxKind::MINUS | SyntaxKind::CONOP)
}

/// ノードの直下にある演算子のトークン。セクションの被演算子の列の演算子は `OP_SEQ` の子なので、ここには入らない。
fn operator_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| is_operator(token.kind()))
}

impl Item {
    pub fn pub_keyword(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SyntaxKind::PUB_KW)
    }
}

impl TypeItem {
    pub fn type_keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::TYPE_KW)
    }
}

impl ImportItem {
    pub fn import_keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::IMPORT_KW)
    }

    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    /// `as X` の `X`。
    pub fn alias(&self) -> Option<Name> {
        support::child(&self.syntax)
    }

    pub fn list(&self) -> Option<ImportList> {
        support::child(&self.syntax)
    }

    /// パスの後ろ (別名と並びを含む) に構文の誤りがあるか。読めたところまでで import を解釈すると、書いたつもりと
    /// 違う取り込み方を黙って選ぶことになる
    /// (docs/implementation/architecture.md の「名前解決の回復」)。並びの `(:+)` は名前だけを
    /// 落とす誤りなので、ここには数えない。
    pub fn is_malformed(&self) -> bool {
        let has_error = self
            .syntax
            .descendants()
            .any(|node| node.kind() == SyntaxKind::ERROR);
        let missing_alias =
            support::token(&self.syntax, SyntaxKind::AS_KW).is_some() && self.alias().is_none();
        let unclosed_list = self
            .list()
            .is_some_and(|list| support::token(list.syntax(), SyntaxKind::R_PAREN).is_none());
        has_error || missing_alias || unclosed_list
    }
}

impl ImportList {
    pub fn names(&self) -> AstChildren<ImportName> {
        support::children(&self.syntax)
    }
}

impl ImportName {
    /// `parse`、`Style`。`(+)` の形では `None`。
    pub fn name(&self) -> Option<NameRef> {
        support::child(&self.syntax)
    }

    /// `(+)` の演算子。
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }

    /// `Style(..)` の形か。
    pub fn all_constructors(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::DOT2).is_some()
    }
}

impl OpRef {
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}

impl LeftSection {
    /// `(e op)` の `e`。
    pub fn operand(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}

impl RightSection {
    /// `(op e)` の `e`。
    pub fn operand(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}

fn keyword_range(node: &SyntaxNode) -> TextRange {
    node.first_token()
        .map_or(node.text_range(), |token| token.text_range())
}

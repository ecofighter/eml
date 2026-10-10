//! アクセサは、HIR への変換で必要になったものから足していく。

use eml_diagnostics::{TextRange, TextSize};
use rowan::NodeOrToken;
use rowan::ast::{AstChildren, AstNode, support};

use crate::literal::MultilineLayout;
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
    RecordFields => RECORD_FIELDS,
    FieldDecl => FIELD_DECL,
    TypeItem => TYPE_ITEM,
    EffectItem => EFFECT_ITEM,
    OpDecl => OP_DECL,
    FixityItem => FIXITY_ITEM,
    ImportItem => IMPORT_ITEM,
    ImportList => IMPORT_LIST,
    ImportName => IMPORT_NAME,
    ClassItem => CLASS_ITEM,
    InstanceItem => INSTANCE_ITEM,
    Context => CONTEXT,
    Constraint => CONSTRAINT,
    Deriving => DERIVING,
    ExternMethod => EXTERN_METHOD,
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
    StringLit => STRING_LIT,
    Interp => INTERP,
    CommandLit => COMMAND_LIT,
    UnitExpr => UNIT_EXPR,
    ParenExpr => PAREN_EXPR,
    TupleExpr => TUPLE_EXPR,
    ListExpr => LIST_EXPR,
    AnnotExpr => ANNOT_EXPR,
    OpRef => OP_REF,
    LeftSection => LEFT_SECTION,
    RightSection => RIGHT_SECTION,
    FieldSection => FIELD_SECTION,
    RecordExpr => RECORD_EXPR,
    Field => FIELD,
    UpdateExpr => UPDATE_EXPR,
    WildcardPat => WILDCARD_PAT,
    BindPat => BIND_PAT,
    ConPat => CON_PAT,
    LiteralPat => LITERAL_PAT,
    UnitPat => UNIT_PAT,
    ParenPat => PAREN_PAT,
    TuplePat => TUPLE_PAT,
    ListPat => LIST_PAT,
    InfixConPat => INFIX_CON_PAT,
    AnnotPat => ANNOT_PAT,
    RecordPat => RECORD_PAT,
    FieldPat => FIELD_PAT,
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
    Item { Signature, Equation, DataItem, TypeItem, EffectItem, FixityItem, ImportItem, ClassItem, InstanceItem }
}

ast_enum! {
    ClassMember { Signature, Equation }
}

ast_enum! {
    InstanceMember { Equation, ExternMethod, Signature }
}

ast_enum! {
    Stmt { LetStmt, UseStmt, ExprStmt }
}

ast_enum! {
    /// `body ::= block(stmt) | expr` を1つの型で受けられるように、字下げしたブロック (`Block`) も式に含める。
    Expr {
        Block, IfExpr, MatchExpr, HandleExpr, LambdaExpr, LetExpr, OpSeq, AppExpr,
        DropExpr, FieldExpr, PathExpr, Literal, StringLit, CommandLit, UnitExpr, ParenExpr, TupleExpr, ListExpr, AnnotExpr, OpRef,
        LeftSection, RightSection, FieldSection, RecordExpr, UpdateExpr,
    }
}

ast_enum! {
    Pat {
        WildcardPat, BindPat, ConPat, LiteralPat, UnitPat, ParenPat, TuplePat, ListPat, InfixConPat,
        AnnotPat, RecordPat,
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
    pub fn extern_keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::EXTERN_KW)
    }

    pub fn name(&self) -> Option<Name> {
        support::child(&self.syntax)
    }

    pub fn context(&self) -> Option<Context> {
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

    /// 入れ子の上限 (E0013) で一部を読み飛ばしたときに真を返す。
    pub fn is_too_deep(&self) -> bool {
        self.syntax
            .descendants()
            .any(|node| node.kind() == SyntaxKind::TOO_DEEP)
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

    /// 浮動小数と文字の未対応のリテラル、範囲外の整数、閉じていない raw 文字列は `None` を返す。未対応の
    /// リテラルは HIR が E0004 を出し、ほかは字句解析が報告済みである。raw 文字列のほかの文字列は `StringLit` で、
    /// `Literal` ではない。
    pub fn value(&self) -> Option<LiteralValue> {
        let token = self.token()?;
        match token.kind() {
            SyntaxKind::INT => crate::literal::int_value(token.text()).map(LiteralValue::Int),
            SyntaxKind::RAW_STRING => {
                crate::literal::raw_value(token.text()).map(LiteralValue::String)
            }
            _ => None,
        }
    }
}

/// 文字列の部分。`Text` は、エスケープ、複数行の字下げ、改行の正規化を済ませた後の文字列である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringPart {
    Text(String),
    Hole(Option<Expr>),
}

impl StringLit {
    /// 閉じていない文字列、不正なエスケープを含む文字列、形の誤り (E0014) のある複数行の文字列は `None` を返す。
    /// どれも lexer が報告済みである。隣り合う本文とエスケープは1つの `Text` にまとめ、空の `Text` は返さない。
    pub fn parts(&self) -> Option<Vec<StringPart>> {
        let end = support::token(&self.syntax, SyntaxKind::STRING_END)?;
        let start = support::token(&self.syntax, SyntaxKind::STRING_START)?;
        if start.text() == "\"\"\"" {
            // 字下げは lexer の E0014 と同じ判定で決める。本文はトークンをまたぐので、節点のテキストから切り出す
            let node_start = self.syntax.text_range().start();
            let body_start = start.text_range().end();
            let body = TextRange::new(
                body_start - node_start,
                end.text_range().start() - node_start,
            );
            let text = self.syntax.text().to_string();
            let layout = crate::literal::multiline_layout(&text[body]).ok()?;
            return self.multiline_parts(layout, body_start);
        }
        let mut parts = Vec::new();
        let mut text = String::new();
        for element in self.syntax.children_with_tokens() {
            match element {
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::STRING_TEXT => {
                    text.push_str(token.text());
                }
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::ESCAPE => {
                    text.push(crate::literal::escape_value(token.text())?);
                }
                NodeOrToken::Node(node) => {
                    if let Some(interp) = Interp::cast(node) {
                        if !text.is_empty() {
                            parts.push(StringPart::Text(std::mem::take(&mut text)));
                        }
                        parts.push(StringPart::Hole(interp.expr()));
                    }
                }
                NodeOrToken::Token(_) => {}
            }
        }
        if !text.is_empty() {
            parts.push(StringPart::Text(text));
        }
        Some(parts)
    }

    /// `content` の外の本文 (開きの行と閉じの行) は値に入れない。行頭では `indent` 個までの空白を落とす。エスケープと
    /// 穴は行頭の空白でないので、そこで行頭の状態を終える
    /// (docs/spec/lexical.md の「複数行の文字列」「改行の正規化」)。
    fn multiline_parts(
        &self,
        layout: MultilineLayout,
        body_start: TextSize,
    ) -> Option<Vec<StringPart>> {
        let content = TextRange::new(
            body_start + TextSize::new(layout.content.start as u32),
            body_start + TextSize::new(layout.content.end as u32),
        );
        let mut parts = Vec::new();
        let mut text = String::new();
        let (mut at_line_start, mut stripped) = (true, 0);
        for element in self.syntax.children_with_tokens() {
            // `intersect` は接するだけの範囲にも空の範囲を返すので、空のものを除く
            let Some(range) = element
                .text_range()
                .intersect(content)
                .filter(|range| !range.is_empty())
            else {
                continue;
            };
            match element {
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::STRING_TEXT => {
                    let start = range.start() - token.text_range().start();
                    let slice = &token.text()[TextRange::at(start, range.len())];
                    let mut chars = slice.chars().peekable();
                    while let Some(c) = chars.next() {
                        if c == '\r' && chars.peek() == Some(&'\n') {
                            continue;
                        }
                        if at_line_start && c == ' ' && stripped < layout.indent {
                            stripped += 1;
                            continue;
                        }
                        at_line_start = c == '\n';
                        stripped = 0;
                        text.push(c);
                    }
                }
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::ESCAPE => {
                    at_line_start = false;
                    text.push(crate::literal::escape_value(token.text())?);
                }
                NodeOrToken::Node(node) => {
                    if let Some(interp) = Interp::cast(node) {
                        at_line_start = false;
                        if !text.is_empty() {
                            parts.push(StringPart::Text(std::mem::take(&mut text)));
                        }
                        parts.push(StringPart::Hole(interp.expr()));
                    }
                }
                NodeOrToken::Token(_) => {}
            }
        }
        if !text.is_empty() {
            parts.push(StringPart::Text(text));
        }
        Some(parts)
    }

    pub fn holes(&self) -> AstChildren<Interp> {
        support::children(&self.syntax)
    }
}

impl Interp {
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
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
    pub fn extern_keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::EXTERN_KW)
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
    pub fn context(&self) -> Option<Context> {
        support::child(&self.syntax)
    }

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
    /// `deriving` の句。2つ目からはパーサが E0011 にするが、クラスはすべての句から読む。
    pub fn deriving_clauses(&self) -> AstChildren<Deriving> {
        support::children(&self.syntax)
    }

    pub fn extern_keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::EXTERN_KW)
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

    /// フィールドの型。中置のコンストラクタでは左右の2つで、レコードの選択肢では宣言の順の `FieldDecl` の型である。
    pub fn fields(&self) -> impl Iterator<Item = Type> {
        let record = self
            .record_fields()
            .into_iter()
            .flat_map(|record| record.fields().filter_map(|field| field.ty()));
        support::children::<Type>(&self.syntax).chain(record)
    }

    pub fn record_fields(&self) -> Option<RecordFields> {
        support::child(&self.syntax)
    }
}

impl RecordFields {
    pub fn fields(&self) -> AstChildren<FieldDecl> {
        support::children(&self.syntax)
    }
}

impl FieldDecl {
    pub fn name(&self) -> Option<Name> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl RecordExpr {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn fields(&self) -> AstChildren<Field> {
        support::children(&self.syntax)
    }
}

impl UpdateExpr {
    pub fn base(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn fields(&self) -> AstChildren<Field> {
        support::children(&self.syntax)
    }
}

impl Field {
    pub fn name(&self) -> Option<NameRef> {
        support::child(&self.syntax)
    }

    /// 省略形 `{ name }` では `None`。
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl RecordPat {
    pub fn path(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn fields(&self) -> AstChildren<FieldPat> {
        support::children(&self.syntax)
    }
}

impl FieldPat {
    pub fn name(&self) -> Option<NameRef> {
        support::child(&self.syntax)
    }

    /// 省略形 `{ name }` では `None`。
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
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

impl ListExpr {
    pub fn elements(&self) -> AstChildren<Expr> {
        support::children(&self.syntax)
    }
}

impl ListPat {
    pub fn elements(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn r_brack(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::R_BRACK)
    }
}

impl TupleType {
    pub fn elements(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}

impl LiteralPat {
    /// `INT`、`CHAR`、`RAW_STRING` のトークン。`-1` の `-` は含まない。raw 文字列のほかの文字列はトークンでなく、
    /// `string` で取り出す。
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::INT | SyntaxKind::CHAR | SyntaxKind::RAW_STRING
                )
            })
    }

    pub fn string(&self) -> Option<StringLit> {
        support::child(&self.syntax)
    }

    /// `-1` の `-` は字句の一部ではなくパターンの一部なので (docs/spec/grammar.md の `apat`)、ここで符号を付ける。
    /// 値の壊れたリテラル、未対応のリテラル (文字)、穴のある文字列は `None` を返す。未対応のリテラルは HIR が
    /// E0004 を出し、値の壊れたものは字句解析が、穴はパーサが報告済みである。
    pub fn value(&self) -> Option<LiteralValue> {
        if let Some(string) = self.string() {
            return string
                .parts()?
                .into_iter()
                .map(|part| match part {
                    StringPart::Text(text) => Some(text),
                    StringPart::Hole(_) => None,
                })
                .collect::<Option<String>>()
                .map(LiteralValue::String);
        }
        let negative = support::token(&self.syntax, SyntaxKind::MINUS).is_some();
        let token = self.token()?;
        match token.kind() {
            SyntaxKind::INT => {
                let n = crate::literal::int_value(token.text())?;
                Some(LiteralValue::Int(if negative { -n } else { n }))
            }
            SyntaxKind::RAW_STRING => {
                crate::literal::raw_value(token.text()).map(LiteralValue::String)
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

impl ClassItem {
    pub fn keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::CLASS_KW)
    }

    pub fn context(&self) -> Option<Context> {
        support::child(&self.syntax)
    }

    /// クラスの名前。`NAME` の1つ目である。
    pub fn name(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax).next()
    }

    /// クラスの型変数。`NAME` の2つ目である。
    pub fn var(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax).nth(1)
    }

    pub fn members(&self) -> AstChildren<ClassMember> {
        support::children(&self.syntax)
    }
}

impl InstanceItem {
    pub fn keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::INSTANCE_KW)
    }

    pub fn context(&self) -> Option<Context> {
        support::child(&self.syntax)
    }

    /// クラスの名前。頭の型の中の `PATH` は型の節点の子なので、直接の子の `PATH` だけを見る。
    pub fn class(&self) -> Option<Path> {
        support::child(&self.syntax)
    }

    pub fn head(&self) -> Option<Type> {
        support::child(&self.syntax)
    }

    pub fn members(&self) -> AstChildren<InstanceMember> {
        support::children(&self.syntax)
    }
}

impl Context {
    pub fn constraints(&self) -> AstChildren<Constraint> {
        support::children(&self.syntax)
    }
}

impl Constraint {
    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl Deriving {
    pub fn keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::DERIVING_KW)
    }

    pub fn classes(&self) -> AstChildren<Path> {
        support::children(&self.syntax)
    }
}

impl ExternMethod {
    pub fn name(&self) -> Option<Name> {
        support::child(&self.syntax)
    }
}

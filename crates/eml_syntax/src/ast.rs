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

/// 子をそのまま返すアクセサ。`name: Type` は最初の子の節点、`name: [Type]` は子の節点の並び、`name: (KIND)` は
/// 最初のその種類のトークンを返す。形の決まったアクセサを1行ずつの表に書き、手で書くのは解釈の要るものに限る。
macro_rules! accessors {
    ($($node:ident { $($(#[$meta:meta])* $name:ident: $kind:tt,)* })*) => {$(
        impl $node {
            $(accessors!(@one $(#[$meta])* $name $kind);)*
        }
    )*};
    (@one $(#[$meta:meta])* $name:ident [$ty:ident]) => {
        $(#[$meta])*
        pub fn $name(&self) -> AstChildren<$ty> {
            support::children(&self.syntax)
        }
    };
    (@one $(#[$meta:meta])* $name:ident ($kind:ident)) => {
        $(#[$meta])*
        pub fn $name(&self) -> Option<SyntaxToken> {
            support::token(&self.syntax, SyntaxKind::$kind)
        }
    };
    (@one $(#[$meta:meta])* $name:ident $ty:ident) => {
        $(#[$meta])*
        pub fn $name(&self) -> Option<$ty> {
            support::child(&self.syntax)
        }
    };
}

ast_node! {
    SourceFile => SOURCE_FILE,
    ReplItems => REPL_ITEMS,
    ReplLet => REPL_LET,
    ReplExpr => REPL_EXPR,
    ReplQuery => REPL_QUERY,
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
    VarStmt => VAR_STMT,
    ExprStmt => EXPR_STMT,
    IfExpr => IF_EXPR,
    MatchExpr => MATCH_EXPR,
    MatchArm => MATCH_ARM,
    HandleExpr => HANDLE_EXPR,
    OpClause => OP_CLAUSE,
    ReturnClause => RETURN_CLAUSE,
    LambdaExpr => LAMBDA_EXPR,
    LetExpr => LET_EXPR,
    AndBinding => AND_BINDING,
    ParamList => PARAM_LIST,
    OpSeq => OP_SEQ,
    AppExpr => APP_EXPR,
    DropExpr => DROP_EXPR,
    FieldExpr => FIELD_EXPR,
    AssignExpr => ASSIGN_EXPR,
    IndexExpr => INDEX_EXPR,
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
    RowArg => ROW_ARG,
    RowParam => ROW_PARAM,
    KindAnnot => KIND_ANNOT,
}

ast_enum! {
    /// REPL の入力の根 (`eml_syntax::parse_input` と `eml_syntax::parse_query`)。
    ReplInput { ReplItems, ReplLet, ReplExpr, ReplQuery }
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
    Stmt { LetStmt, UseStmt, VarStmt, ExprStmt }
}

ast_enum! {
    /// `body ::= block(stmt) | expr` を1つの型で受けられるように、字下げしたブロック (`Block`) も式に含める。
    Expr {
        Block, IfExpr, MatchExpr, HandleExpr, LambdaExpr, LetExpr, OpSeq, AppExpr,
        DropExpr, FieldExpr, AssignExpr, IndexExpr, PathExpr, Literal, StringLit, CommandLit, UnitExpr, ParenExpr, TupleExpr, ListExpr, AnnotExpr, OpRef,
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

accessors! {
    Path {
        segments: [NameRef],
    }

    LambdaExpr {
        params: [Pat],
        body: Expr,
    }

    AnnotPat {
        pat: Pat,
        ty: Type,
    }

    VarType {
        name: (LIDENT),
    }

    SourceFile {
        items: [Item],
    }

    ReplItems {
        items: [Item],
    }

    ReplLet {
        stmt: LetStmt,
    }

    ReplExpr {
        expr: Expr,
    }

    ReplQuery {
        expr: Expr,
    }

    Signature {
        extern_keyword: (EXTERN_KW),
        name: Name,
        context: Context,
        ty: Type,
    }

    Equation {
        /// 関数の名前。演算子の定義では演算子である。
        name: Name,
        /// 演算子の定義では、左辺と右辺のパターンが入る。
        params: [Pat],
        body: Expr,
    }

    Block {
        stmts: [Stmt],
    }

    StringLit {
        holes: [Interp],
    }

    Interp {
        expr: Expr,
    }

    CommandLit {
        holes: [Interp],
    }

    PathExpr {
        path: Path,
    }

    ParamList {
        pats: [Pat],
    }

    AndBinding {
        pat: Pat,
        params: ParamList,
        body: Expr,
    }

    LetStmt {
        and_bindings: [AndBinding],
        pat: Pat,
        /// 関数の形 (`let go x = …`) の引数。パターンの束縛なら `None`。
        params: ParamList,
        ty: Type,
        body: Expr,
    }

    UseStmt {
        /// `use p <- e` の `p`。`<-` がなければ `None`。
        pat: Pat,
        expr: Expr,
    }

    VarStmt {
        name: Name,
        ty: Type,
    }

    LetExpr {
        and_bindings: [AndBinding],
        pat: Pat,
        /// 関数の形 (`let go x = …`) の引数。パターンの束縛なら `None`。
        params: ParamList,
        ty: Type,
    }

    ExprStmt {
        expr: Expr,
    }

    ParenExpr {
        expr: Expr,
    }

    AnnotExpr {
        expr: Expr,
        ty: Type,
    }

    BindPat {
        name: Name,
    }

    ParenPat {
        pat: Pat,
    }

    PathType {
        path: Path,
    }

    ParenType {
        ty: Type,
    }

    FnType {
        row: EffectRow,
    }

    EffectRow {
        effects: [Effect],
        /// `<IO | e>` と `<e>` の row 変数。
        tail: (LIDENT),
    }

    Effect {
        path: Path,
        /// row に書いたエフェクトの型引数。
        args: [Type],
    }

    RowArg {
        row: EffectRow,
    }

    RowParam {
        name: Name,
    }

    KindAnnot {
        name_ref: NameRef,
    }

    EffectItem {
        extern_keyword: (EXTERN_KW),
        operations: [OpDecl],
    }

    OpDecl {
        context: Context,
        name: Name,
        ty: Type,
    }

    HandleExpr {
        from_keyword: (FROM_KW),
        clauses: [Clause],
    }

    OpClause {
        /// 節の先頭の操作の名前。修飾できる (docs/spec/grammar.md の `clause`)。
        path: Path,
        /// 操作の引数、`k`、(パラメータ付き handler なら) 状態のパターン。
        params: [Pat],
    }

    ReturnClause {
        params: [Pat],
    }

    DropExpr {
        args: [Expr],
    }

    DataItem {
        /// `deriving` の句。2つ目からはパーサが E0011 にするが、クラスはすべての句から読む。
        deriving_clauses: [Deriving],
        extern_keyword: (EXTERN_KW),
        kind: KindAnnot,
        alts: [Alt],
    }

    Alt {
        record_fields: RecordFields,
    }

    RecordFields {
        fields: [FieldDecl],
    }

    FieldDecl {
        name: Name,
        ty: Type,
    }

    RecordExpr {
        path: Path,
        fields: [Field],
    }

    UpdateExpr {
        base: Expr,
        fields: [Field],
    }

    FieldExpr {
        /// `e.f` の `e`。
        base: Expr,
    }

    Field {
        name: NameRef,
        /// 省略形 `{ name }` では `None`。
        expr: Expr,
    }

    RecordPat {
        path: Path,
        fields: [FieldPat],
    }

    FieldPat {
        name: NameRef,
        /// 省略形 `{ name }` では `None`。
        pat: Pat,
    }

    MatchExpr {
        arms: [MatchArm],
    }

    MatchArm {
        pat: Pat,
    }

    ConPat {
        path: Path,
        args: [Pat],
    }

    InfixConPat {
        operator: (CONOP),
    }

    AppType {
        /// 適用する型の名前。
        path: Path,
    }

    TupleExpr {
        /// 要素の式。文法が2つ以上にする (docs/spec/grammar.md の `atom`)。
        elements: [Expr],
    }

    TuplePat {
        elements: [Pat],
    }

    ListExpr {
        elements: [Expr],
    }

    ListPat {
        elements: [Pat],
        r_brack: (R_BRACK),
    }

    TupleType {
        elements: [Type],
    }

    LiteralPat {
        string: StringLit,
    }

    FixityItem {
        /// 宣言した演算子。`-` と `:` で始まる演算子も含む。
        operators: [Name],
    }

    TypeItem {
        type_keyword: (TYPE_KW),
    }

    ImportItem {
        path: Path,
        /// `as X` の `X`。
        alias: Name,
        list: ImportList,
    }

    ImportList {
        names: [ImportName],
    }

    ImportName {
        /// `parse`、`Style`。`(+)` の形では `None`。
        name: NameRef,
    }

    LeftSection {
        /// `(e op)` の `e`。
        operand: Expr,
    }

    RightSection {
        /// `(op e)` の `e`。
        operand: Expr,
    }

    ClassItem {
        keyword: (CLASS_KW),
        context: Context,
        members: [ClassMember],
    }

    InstanceItem {
        keyword: (INSTANCE_KW),
        context: Context,
        /// クラスの名前。頭の型の中の `PATH` は型の節点の子なので、直接の子の `PATH` だけを見る。
        class: Path,
        head: Type,
        members: [InstanceMember],
    }

    Context {
        constraints: [Constraint],
    }

    Constraint {
        ty: Type,
    }

    Deriving {
        keyword: (DERIVING_KW),
        classes: [Path],
    }

    ExternMethod {
        name: Name,
    }
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
    /// 最後のセグメント。修飾のない名前ならその名前である。
    pub fn name(&self) -> Option<NameRef> {
        self.segments().last()
    }
}

impl Equation {
    /// 入れ子の上限 (E0013) で一部を読み飛ばしたときに真を返す。
    pub fn is_too_deep(&self) -> bool {
        self.syntax
            .descendants()
            .any(|node| node.kind() == SyntaxKind::TOO_DEEP)
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
#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Int(i64),
    Float(f64),
    Byte(u8),
    String(String),
    Char(char),
}

impl Literal {
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax.first_token()
    }

    /// 範囲外の整数、閉じていない raw 文字列、壊れた文字のリテラルは `None` を返す。どれも字句解析が報告済みである。
    /// raw 文字列のほかの文字列は `StringLit` で、`Literal` ではない。
    pub fn value(&self) -> Option<LiteralValue> {
        let token = self.token()?;
        match token.kind() {
            SyntaxKind::INT => crate::literal::int_value(token.text()).map(LiteralValue::Int),
            SyntaxKind::FLOAT => crate::literal::float_value(token.text()).map(LiteralValue::Float),
            SyntaxKind::BYTE => crate::literal::byte_value(token.text()).map(LiteralValue::Byte),
            SyntaxKind::RAW_STRING => {
                crate::literal::raw_value(token.text()).map(LiteralValue::String)
            }
            SyntaxKind::CHAR => char_token_value(token.text()).map(LiteralValue::Char),
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

/// `StringLit::parts` が部分を組み立てる。`text` は次の穴までの本文で、穴と終わりで空でなければ `Text` にする。
#[derive(Default)]
struct PartsBuilder {
    parts: Vec<StringPart>,
    text: String,
}

impl PartsBuilder {
    fn flush(&mut self) {
        if !self.text.is_empty() {
            self.parts
                .push(StringPart::Text(std::mem::take(&mut self.text)));
        }
    }

    fn push_hole(&mut self, interp: Interp) {
        self.flush();
        self.parts.push(StringPart::Hole(interp.expr()));
    }

    fn finish(mut self) -> Vec<StringPart> {
        self.flush();
        self.parts
    }
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
        let mut parts = PartsBuilder::default();
        for element in self.syntax.children_with_tokens() {
            match element {
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::STRING_TEXT => {
                    parts.text.push_str(token.text());
                }
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::ESCAPE => {
                    parts.text.push(crate::literal::escape_value(token.text())?);
                }
                NodeOrToken::Node(node) => {
                    if let Some(interp) = Interp::cast(node) {
                        parts.push_hole(interp);
                    }
                }
                NodeOrToken::Token(_) => {}
            }
        }
        Some(parts.finish())
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
        let mut parts = PartsBuilder::default();
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
                        parts.text.push(c);
                    }
                }
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::ESCAPE => {
                    at_line_start = false;
                    parts.text.push(crate::literal::escape_value(token.text())?);
                }
                NodeOrToken::Node(node) => {
                    if let Some(interp) = Interp::cast(node) {
                        at_line_start = false;
                        parts.push_hole(interp);
                    }
                }
                NodeOrToken::Token(_) => {}
            }
        }
        Some(parts.finish())
    }
}

impl Interp {
    /// コマンドリテラルの `\{..xs}`。文字列の穴では、パーサが `..` を読まない。
    pub fn spread(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::DOT2).is_some()
    }
}

/// コマンドリテラルの引数の部分。`Text` は、エスケープと引用を解いた後の文字列である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandPart {
    Text(String),
    Hole(Interp),
}

/// コマンドリテラルの引数1つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandWord {
    /// 先頭の文字から末尾の文字まで。区切りの空白を含まない。
    pub range: TextRange,
    pub parts: Vec<CommandPart>,
    /// 引数の中に `CMD_QUOTE` があったか。`''` は部分が空で `quoted` が真の引数になる。
    pub quoted: bool,
}

impl CommandLit {
    /// 閉じていないリテラル、不正なエスケープ、閉じていない引用を含むリテラルは `None` を返す。どれも lexer が
    /// 報告済みである。区切りは引用の外の `CMD_TEXT` の空白とタブだけで、隣り合う本文、エスケープ、引用の中の本文は
    /// 1つの `Text` にまとめ、空の `Text` は返さない (docs/spec/lexical.md の「コマンドリテラル」)。
    pub fn words(&self) -> Option<Vec<CommandWord>> {
        support::token(&self.syntax, SyntaxKind::CMD_END)?;
        let mut words = Vec::new();
        let mut builder = WordBuilder::default();
        let mut in_quote = false;
        for element in self.syntax.children_with_tokens() {
            match element {
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::CMD_TEXT => {
                        let start = token.text_range().start();
                        for (offset, c) in token.text().char_indices() {
                            let at = start + TextSize::new(offset as u32);
                            let end = at + TextSize::of(c);
                            if !in_quote && matches!(c, ' ' | '\t') {
                                words.extend(builder.finish());
                            } else {
                                builder.push_char(c, at, end);
                            }
                        }
                    }
                    SyntaxKind::ESCAPE => {
                        let c = match token.text() {
                            "\\`" => '`',
                            escape => crate::literal::escape_value(escape)?,
                        };
                        let range = token.text_range();
                        builder.push_char(c, range.start(), range.end());
                    }
                    SyntaxKind::CMD_QUOTE => {
                        in_quote = !in_quote;
                        builder.mark_quote(token.text_range());
                    }
                    _ => {}
                },
                NodeOrToken::Node(node) => {
                    if let Some(interp) = Interp::cast(node) {
                        builder.push_hole(interp);
                    }
                }
            }
        }
        if in_quote {
            return None;
        }
        words.extend(builder.finish());
        Some(words)
    }
}

/// `CommandLit::words` が、区切りに着くまでの引数を組み立てる。
#[derive(Default)]
struct WordBuilder {
    range: Option<TextRange>,
    parts: Vec<CommandPart>,
    text: String,
    quoted: bool,
}

impl WordBuilder {
    fn extend(&mut self, range: TextRange) {
        self.range = Some(self.range.map_or(range, |seen| seen.cover(range)));
    }

    fn push_char(&mut self, c: char, start: TextSize, end: TextSize) {
        self.extend(TextRange::new(start, end));
        self.text.push(c);
    }

    fn mark_quote(&mut self, range: TextRange) {
        self.extend(range);
        self.quoted = true;
    }

    fn push_hole(&mut self, interp: Interp) {
        self.extend(interp.range());
        if !self.text.is_empty() {
            self.parts
                .push(CommandPart::Text(std::mem::take(&mut self.text)));
        }
        self.parts.push(CommandPart::Hole(interp));
    }

    fn finish(&mut self) -> Option<CommandWord> {
        let range = self.range.take()?;
        if !self.text.is_empty() {
            self.parts
                .push(CommandPart::Text(std::mem::take(&mut self.text)));
        }
        Some(CommandWord {
            range,
            parts: std::mem::take(&mut self.parts),
            quoted: std::mem::take(&mut self.quoted),
        })
    }
}

impl VarStmt {
    /// `=` の後の初期値。
    pub fn init(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::EQ), None)
    }
}

impl LetExpr {
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

impl FnType {
    pub fn is_once(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::ONCE_KW).is_some()
    }

    pub fn param(&self) -> Option<Type> {
        child_between(&self.syntax, None, Some(SyntaxKind::THIN_ARROW))
    }

    pub fn ret(&self) -> Option<Type> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl EffectRow {
    /// row 変数の前に書いた `never`。`<never Panic | e>` のように row 変数の直前にない `never` は、誤りとして読んだもので
    /// どの変数の上限でもないので返さない (docs/spec/grammar.md の「文法上の補足」)。
    pub fn bound(&self) -> Option<SyntaxToken> {
        let never = support::token(&self.syntax, SyntaxKind::NEVER_KW)?;
        let next = std::iter::successors(never.next_sibling_or_token(), |element| {
            element.next_sibling_or_token()
        })
        .find(|element| !element.kind().is_trivia())?;
        (next.kind() == SyntaxKind::LIDENT).then_some(never)
    }
}

/// 型の適用の引数。row は `<…>` で書く。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeArg {
    Type(Type),
    Row(RowArg),
}

/// `data` の型引数。row は `<e>` で書く。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeParam {
    Type(Name),
    Row(RowParam),
}

impl EffectItem {
    pub fn name(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax)
            .find(|name| name.token().kind() == SyntaxKind::UIDENT)
    }

    /// 型引数。名前が欠けた宣言でも取り違えないよう、小文字の名前だけを返す。
    pub fn params(&self) -> impl Iterator<Item = Name> {
        lowercase_names(&self.syntax)
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

    /// `from` の後、`with` の前の式。`from` がなければ `None`。
    pub fn init(&self) -> Option<Expr> {
        self.from_keyword()?;
        child_between(
            &self.syntax,
            Some(SyntaxKind::FROM_KW),
            Some(SyntaxKind::WITH_KW),
        )
    }
}

impl OpClause {
    /// `|` の直後の `fn` か `ctl`。
    pub fn keyword(&self) -> Option<SyntaxToken> {
        clause_keyword(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

/// 節の直下に置かれるトークンのうち `fn` か `ctl` だけを探す。`| fn return x` のように `return` 節に付いても読める。
fn clause_keyword(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| matches!(token.kind(), SyntaxKind::FN_KW | SyntaxKind::CTL_KW))
}

impl ReturnClause {
    /// `|` の直後の `fn` か `ctl`。
    pub fn keyword(&self) -> Option<SyntaxToken> {
        clause_keyword(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl DataItem {
    /// `=` とコンストラクタの並びを書いたか。
    /// `=` か選択肢があるか。`=` を書き忘れた選択肢はパーサが報告済みなので、コンストラクタのない `data` には数えない。
    pub fn has_constructors(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::EQ).is_some() || self.alts().next().is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax)
            .find(|name| name.token().kind() == SyntaxKind::UIDENT)
    }

    /// 型引数。名前が欠けた宣言でも取り違えないよう、型引数の名前は小文字のものだけを返す。
    pub fn type_params(&self) -> impl Iterator<Item = TypeParam> {
        self.syntax.children().filter_map(|node| {
            RowParam::cast(node.clone())
                .map(TypeParam::Row)
                .or_else(|| {
                    Name::cast(node)
                        .filter(|name| name.token().kind() == SyntaxKind::LIDENT)
                        .map(TypeParam::Type)
                })
        })
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
}

impl RecordExpr {
    /// フィールドの並びに、パーサが読み飛ばした部分 (`ERROR`) があるか。
    pub fn has_skipped_fields(&self) -> bool {
        self.syntax
            .children()
            .any(|child| child.kind() == SyntaxKind::ERROR)
    }
}

impl FieldExpr {
    /// `.` の後の名前 (`LIDENT`) か番号 (`INT`)。
    pub fn field(&self) -> Option<SyntaxToken> {
        field_token(&self.syntax)
    }
}

impl AssignExpr {
    /// `<-` の左辺。名前か添字かは HIR が確かめる。
    pub fn target(&self) -> Option<Expr> {
        child_between(&self.syntax, None, Some(SyntaxKind::LEFT_ARROW))
    }

    pub fn value(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::LEFT_ARROW), None)
    }
}

impl IndexExpr {
    /// `e[i]` の `e`。
    pub fn base(&self) -> Option<Expr> {
        child_between(&self.syntax, None, Some(SyntaxKind::L_BRACK))
    }

    pub fn index(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::L_BRACK), None)
    }
}

impl FieldSection {
    /// `(.f)` の名前 (`LIDENT`) か番号 (`INT`)。
    pub fn field(&self) -> Option<SyntaxToken> {
        field_token(&self.syntax)
    }
}

impl Field {
    /// `=` を書いたか。`{ name = }` のように値が欠けても、省略形とは区別する。
    pub fn has_eq(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::EQ).is_some()
    }
}

impl FieldPat {
    /// `=` を書いたか。`{ name = }` のようにパターンが欠けても、省略形とは区別する。
    pub fn has_eq(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::EQ).is_some()
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
}

impl MatchArm {
    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl InfixConPat {
    pub fn lhs(&self) -> Option<Pat> {
        child_between(&self.syntax, None, Some(SyntaxKind::CONOP))
    }

    pub fn rhs(&self) -> Option<Pat> {
        child_between(&self.syntax, Some(SyntaxKind::CONOP), None)
    }
}

impl AppType {
    pub fn args(&self) -> impl Iterator<Item = TypeArg> {
        self.syntax.children().filter_map(|node| {
            RowArg::cast(node.clone())
                .map(TypeArg::Row)
                .or_else(|| Type::cast(node).map(TypeArg::Type))
        })
    }
}

impl LiteralPat {
    /// `INT`、`FLOAT`、`BYTE`、`CHAR`、`RAW_STRING` のトークン。`-1` の `-` は含まない。raw 文字列のほかの文字列はトークンでなく、
    /// `string` で取り出す。
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::INT
                        | SyntaxKind::FLOAT
                        | SyntaxKind::BYTE
                        | SyntaxKind::CHAR
                        | SyntaxKind::RAW_STRING
                )
            })
    }

    /// 数のリテラルの前に `-` があるか。`Byte` の負のリテラル (E1054) を HIR が見つけるのに使う。
    pub fn is_negative(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::MINUS).is_some()
    }

    /// `-1` の `-` は字句の一部ではなくパターンの一部なので (docs/spec/grammar.md の `apat`)、ここで符号を付ける。
    /// 値の壊れたリテラルと穴のある文字列は `None` を返す。値の壊れたものは字句解析が、穴はパーサが報告済みである。
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
            SyntaxKind::FLOAT => {
                let x = crate::literal::float_value(token.text())?;
                Some(LiteralValue::Float(if negative { -x } else { x }))
            }
            // 負号は HIR が E1054 にするので、値は符号を見ない
            SyntaxKind::BYTE => crate::literal::byte_value(token.text()).map(LiteralValue::Byte),
            SyntaxKind::RAW_STRING => {
                crate::literal::raw_value(token.text()).map(LiteralValue::String)
            }
            SyntaxKind::CHAR => char_token_value(token.text()).map(LiteralValue::Char),
            _ => None,
        }
    }
}

/// `CHAR` のトークンは引用符を含む。壊れた値は字句解析が報告済みなので `None` にする。
fn char_token_value(text: &str) -> Option<char> {
    let body = text.strip_prefix('\'')?.strip_suffix('\'')?;
    crate::literal::char_value(body).ok()
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

/// ノードの直下にある、射影のフィールドの名前か番号のトークン。`e.f` の `e` の中のトークンは子のノードの中なので、
/// ここには入らない。
fn field_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| matches!(token.kind(), SyntaxKind::LIDENT | SyntaxKind::INT))
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

    pub fn abstract_keyword(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SyntaxKind::ABSTRACT_KW)
    }
}

impl ImportItem {
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

impl ImportName {
    /// `(+)` の演算子。
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }

    /// `Style(..)` の形か。
    pub fn all_constructors(&self) -> bool {
        support::token(&self.syntax, SyntaxKind::DOT2).is_some()
    }

    /// `Style(..)` の `(..)` の位置。
    pub fn all_constructors_range(&self) -> Option<TextRange> {
        let open = support::token(&self.syntax, SyntaxKind::L_PAREN)?;
        let close = support::token(&self.syntax, SyntaxKind::R_PAREN)?;
        self.all_constructors()
            .then(|| TextRange::new(open.text_range().start(), close.text_range().end()))
    }
}

impl OpRef {
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}

impl LeftSection {
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}

impl RightSection {
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}

fn keyword_range(node: &SyntaxNode) -> TextRange {
    node.first_token()
        .map_or(node.text_range(), |token| token.text_range())
}

impl ClassItem {
    /// クラスの名前。`NAME` の1つ目である。
    pub fn name(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax).next()
    }

    /// クラスの型変数。`NAME` の2つ目である。
    pub fn var(&self) -> Option<Name> {
        support::children::<Name>(&self.syntax).nth(1)
    }
}

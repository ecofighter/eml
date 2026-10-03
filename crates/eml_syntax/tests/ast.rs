use eml_diagnostics::SourceFiles;
use eml_syntax::SyntaxKind::{self, *};
use eml_syntax::ast::{Expr, Item, OpSeqElement, Pat, SourceFile, Stmt, Type};
use eml_syntax::parse;
use rowan::ast::AstNode;

fn source(text: &str) -> SourceFile {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    parse.tree()
}

fn first_equation(file: &SourceFile) -> eml_syntax::ast::Equation {
    file.items()
        .find_map(|item| match item {
            Item::Equation(equation) => Some(equation),
            _ => None,
        })
        .expect("an equation")
}

#[test]
fn items_and_their_names() {
    let file = source("len : List a -> Int\nlen Nil = 0\n(<+>) : A\na <+> b = a\ndata T = | A");
    let items: Vec<String> = file
        .items()
        .map(|item| match item {
            Item::Signature(signature) => format!("signature {}", signature.name().unwrap().text()),
            Item::Equation(equation) => format!("equation {}", equation.name().unwrap().text()),
            Item::DataItem(_) => "data".to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        items,
        [
            "signature len",
            "equation len",
            "signature <+>",
            "equation <+>",
            "data"
        ]
    );
}

#[test]
fn signature_type() {
    let file = source("f : Int -> Int");
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    assert!(matches!(signature.ty(), Some(Type::FnType(_))));
}

#[test]
fn equation_parameters_and_body() {
    let file = source("f x (a, b) = x");
    let equation = first_equation(&file);
    let params: Vec<SyntaxKind> = equation.params().map(|pat| pat.syntax().kind()).collect();
    assert_eq!(params, [BIND_PAT, TUPLE_PAT]);
    assert!(matches!(equation.body(), Some(Expr::PathExpr(_))));
    assert!(matches!(equation.params().next(), Some(Pat::BindPat(_))));
}

#[test]
fn block_statements() {
    let file = source("main () =\n  let x = 1\n  use f\n  g x");
    let Some(Expr::Block(block)) = first_equation(&file).body() else {
        panic!("expected a block body");
    };
    let stmts: Vec<&str> = block
        .stmts()
        .map(|stmt| match stmt {
            Stmt::LetStmt(_) => "let",
            Stmt::UseStmt(_) => "use",
            Stmt::ExprStmt(_) => "expr",
        })
        .collect();
    assert_eq!(stmts, ["let", "use", "expr"]);
}

#[test]
fn operator_sequence_elements_keep_prefix_minus() {
    let file = source("x = -a + b");
    let Some(Expr::OpSeq(seq)) = first_equation(&file).body() else {
        panic!("expected an operator sequence");
    };
    let elements: Vec<String> = seq
        .elements()
        .map(|element| match element {
            OpSeqElement::Operand(expr) => format!("operand {:?}", expr.syntax().kind()),
            OpSeqElement::Operator(token) => format!("operator {}", token.text()),
        })
        .collect();
    assert_eq!(
        elements,
        [
            "operator -",
            "operand PATH_EXPR",
            "operator +",
            "operand PATH_EXPR"
        ]
    );
}

#[test]
fn literals_and_paths() {
    let file = source("x = Foo.bar 1");
    let Some(Expr::AppExpr(app)) = first_equation(&file).body() else {
        panic!("expected an application");
    };
    let parts: Vec<Expr> = app.syntax().children().filter_map(Expr::cast).collect();
    let Expr::PathExpr(path) = &parts[0] else {
        panic!("expected a path");
    };
    let segments: Vec<String> = path
        .segments()
        .map(|token| token.text().to_string())
        .collect();
    assert_eq!(segments, ["Foo", "bar"]);
    let Expr::Literal(literal) = &parts[1] else {
        panic!("expected a literal");
    };
    assert_eq!(literal.token().unwrap().text(), "1");
}

#[test]
fn every_node_in_the_corpus_has_an_ast_type() {
    let file = source(include_str!("corpus/s1.em"));
    // enum に入らない、ほかのノードの部品になるノード。
    let parts = [
        SOURCE_FILE,
        ALT,
        OP_DECL,
        MATCH_ARM,
        OP_CLAUSE,
        RETURN_CLAUSE,
        EFFECT_ROW,
        EFFECT,
    ];
    for node in file.syntax().descendants() {
        let kind = node.kind();
        let covered = Item::can_cast(kind)
            || Stmt::can_cast(kind)
            || Expr::can_cast(kind)
            || Pat::can_cast(kind)
            || Type::can_cast(kind)
            || parts.contains(&kind);
        assert!(covered, "{kind:?} has no AST type");
    }
}

use eml_diagnostics::TextRange;
use eml_syntax::SyntaxKind::{self, *};
use eml_syntax::ast::{
    AppExpr, Clause, Expr, Item, LiteralValue, OpSeqElement, Pat, SourceFile, Stmt, Type,
};
use rowan::ast::AstNode;

fn source(text: &str) -> SourceFile {
    eml_test_support::parse_clean(text).parse.tree()
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
        .path()
        .unwrap()
        .segments()
        .map(|segment| segment.text())
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
        PATH,
        NAME,
        NAME_REF,
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

#[test]
fn let_and_expression_statement_parts() {
    let file = source("main () =\n  let x : Int = 1\n  x");
    let Some(Expr::Block(block)) = first_equation(&file).body() else {
        panic!("expected a block body");
    };
    let stmts: Vec<Stmt> = block.stmts().collect();
    let Stmt::LetStmt(stmt) = &stmts[0] else {
        panic!("expected a let statement");
    };
    assert!(matches!(stmt.pat(), Some(Pat::BindPat(_))));
    assert!(matches!(stmt.ty(), Some(Type::PathType(_))));
    assert!(matches!(stmt.body(), Some(Expr::Literal(_))));
    let Stmt::ExprStmt(stmt) = &stmts[1] else {
        panic!("expected an expression statement");
    };
    assert!(matches!(stmt.expr(), Some(Expr::PathExpr(_))));
}

#[test]
fn if_branches() {
    let text = |expr: Option<Expr>| expr.unwrap().syntax().text().to_string();
    let file = source("f = if a then b else c");
    let Some(Expr::IfExpr(e)) = first_equation(&file).body() else {
        panic!("expected an if expression");
    };
    assert_eq!(text(e.condition()), "a");
    assert_eq!(text(e.then_branch()), "b");
    assert_eq!(text(e.else_branch()), "c");
    let file = source("f = if a then b");
    let Some(Expr::IfExpr(e)) = first_equation(&file).body() else {
        panic!("expected an if expression");
    };
    assert_eq!(text(e.then_branch()), "b");
    assert!(e.else_branch().is_none());
}

#[test]
fn application_parts() {
    let file = source("f = g x (h y)");
    let Some(Expr::AppExpr(app)) = first_equation(&file).body() else {
        panic!("expected an application");
    };
    assert_eq!(app.callee().unwrap().syntax().text().to_string(), "g");
    let args: Vec<String> = app.args().map(|a| a.syntax().text().to_string()).collect();
    assert_eq!(args, ["x", "(h y)"]);
}

#[test]
fn function_type_parts() {
    let file = source("f : Int -> <IO | e> String");
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    let Some(Type::FnType(ty)) = signature.ty() else {
        panic!("expected a function type");
    };
    assert_eq!(ty.param().unwrap().syntax().text().to_string(), "Int");
    assert_eq!(ty.ret().unwrap().syntax().text().to_string(), "String");
    let row = ty.row().unwrap();
    let effects: Vec<String> = row
        .effects()
        .map(|e| e.path().unwrap().name().unwrap().text())
        .collect();
    assert_eq!(effects, ["IO"]);
    assert_eq!(row.tail().unwrap().text(), "e");
}

#[test]
fn parenthesized_and_annotated_parts() {
    let file = source("f (x) = (x : (Int))");
    let equation = first_equation(&file);
    let Some(Pat::ParenPat(paren)) = equation.params().next() else {
        panic!("expected a parenthesized pattern");
    };
    let Some(Pat::BindPat(bind)) = paren.pat() else {
        panic!("expected a variable pattern");
    };
    assert_eq!(bind.name().unwrap().text(), "x");
    let Some(Expr::AnnotExpr(annot)) = equation.body() else {
        panic!("expected an annotation");
    };
    assert!(matches!(annot.expr(), Some(Expr::PathExpr(_))));
    let Some(Type::ParenType(paren)) = annot.ty() else {
        panic!("expected a parenthesized type");
    };
    let Some(Type::PathType(path)) = paren.ty() else {
        panic!("expected a type name");
    };
    let segments: Vec<String> = path.path().unwrap().segments().map(|s| s.text()).collect();
    assert_eq!(segments, ["Int"]);
}

#[test]
fn parenthesized_expression() {
    let file = source("f = (g)");
    let Some(Expr::ParenExpr(paren)) = first_equation(&file).body() else {
        panic!("expected a parenthesized expression");
    };
    assert!(matches!(paren.expr(), Some(Expr::PathExpr(_))));
}

#[test]
fn ranges_of_nodes_and_their_keywords() {
    let file = source("f x = if x == 0 then 1 else x");
    let equation = first_equation(&file);
    assert_eq!(equation.range(), TextRange::new(0.into(), 29.into()));
    let body = equation.body().expect("a body");
    assert_eq!(body.range(), TextRange::new(6.into(), 29.into()));
    assert_eq!(body.keyword_range(), TextRange::new(6.into(), 8.into()));
}

#[test]
fn callee_is_the_first_child_even_when_it_is_an_error() {
    // `€` は ERROR ノードになる。最初の `Expr` の子を探すと、引数の `x` を呼ばれるものと取り違える。
    let parsed = eml_test_support::parse("f = € x");
    let app = parsed
        .parse
        .syntax()
        .descendants()
        .find_map(AppExpr::cast)
        .expect("an application");
    assert!(app.callee().is_none());
    let args: Vec<String> = app
        .args()
        .map(|arg| arg.syntax().text().to_string())
        .collect();
    assert_eq!(args, ["x"]);
}

#[test]
fn effect_declaration_parts() {
    let file = source("effect State s where\n  get : Unit -> s\n  never fail : String -> a");
    let Some(Item::EffectItem(effect)) = file.items().next() else {
        panic!("expected an effect");
    };
    assert_eq!(effect.name().unwrap().text(), "State");
    let params: Vec<String> = effect.params().map(|t| t.text().to_string()).collect();
    assert_eq!(params, ["s"]);
    let operations: Vec<(Option<SyntaxKind>, String)> = effect
        .operations()
        .map(|op| {
            (
                op.multiplicity().map(|t| t.kind()),
                op.name().unwrap().text().to_string(),
            )
        })
        .collect();
    assert_eq!(
        operations,
        [
            (None, "get".to_string()),
            (Some(NEVER_KW), "fail".to_string())
        ]
    );
    assert!(
        effect
            .operations()
            .all(|op| matches!(op.ty(), Some(Type::FnType(_))))
    );
}

#[test]
fn handler_clauses_resume_and_drop() {
    let file =
        source("h = handle f () with\n  | ask key k -> resume k key\n  | return x -> drop x");
    let equation = first_equation(&file);
    let Some(Expr::HandleExpr(handle)) = equation.body() else {
        panic!("expected a handler");
    };
    assert!(matches!(handle.body(), Some(Expr::AppExpr(_))));
    assert!(handle.from_keyword().is_none());
    let clauses: Vec<Clause> = handle.clauses().collect();
    let [Clause::OpClause(op), Clause::ReturnClause(ret)] = clauses.as_slice() else {
        panic!("{clauses:?}");
    };
    assert_eq!(op.path().unwrap().name().unwrap().text(), "ask");
    assert_eq!(op.params().count(), 2);
    let Some(Expr::ResumeExpr(resume)) = op.body() else {
        panic!("expected `resume`");
    };
    assert_eq!(resume.args().count(), 2);
    assert_eq!(ret.params().count(), 1);
    let Some(Expr::DropExpr(drop)) = ret.body() else {
        panic!("expected `drop`");
    };
    assert_eq!(drop.args().count(), 1);
}

#[test]
fn handler_with_an_initial_state() {
    let file = source("h = handle f () from s with | return x st -> x");
    let equation = first_equation(&file);
    let Some(Expr::HandleExpr(handle)) = equation.body() else {
        panic!("expected a handler");
    };
    assert!(matches!(handle.body(), Some(Expr::AppExpr(_))));
    assert_eq!(handle.from_keyword().unwrap().kind(), FROM_KW);
}

#[test]
fn effect_arguments_in_a_row() {
    let file = source("f : Unit -> <State Int> Unit");
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    let Some(Type::FnType(function)) = signature.ty() else {
        panic!("expected a function type");
    };
    let effect = function.row().unwrap().effects().next().unwrap();
    assert_eq!(effect.args().count(), 1);
}

#[test]
fn data_declaration_parts() {
    let file = source("data List a = | Nil | Cons a (List a) | a :+ List a");
    let Some(Item::DataItem(data)) = file.items().next() else {
        panic!("expected a data declaration");
    };
    assert_eq!(data.name().unwrap().text(), "List");
    let params: Vec<String> = data
        .params()
        .map(|token| token.text().to_string())
        .collect();
    assert_eq!(params, ["a"]);
    let alts: Vec<(Option<String>, Option<String>, Vec<SyntaxKind>)> = data
        .alts()
        .map(|alt| {
            (
                alt.name().map(|token| token.text().to_string()),
                alt.operator().map(|token| token.text().to_string()),
                alt.fields().map(|ty| ty.syntax().kind()).collect(),
            )
        })
        .collect();
    assert_eq!(
        alts,
        [
            (Some("Nil".to_string()), None, vec![]),
            (Some("Cons".to_string()), None, vec![VAR_TYPE, PAREN_TYPE]),
            (None, Some(":+".to_string()), vec![VAR_TYPE, APP_TYPE]),
        ]
    );
}

#[test]
fn match_arms_and_constructor_patterns() {
    let file = source("f o = match o with | Some (Pair a b) -> a | x :+ _ -> x");
    let equation = first_equation(&file);
    let Some(Expr::MatchExpr(expr)) = equation.body() else {
        panic!("expected a match");
    };
    assert!(matches!(expr.scrutinee(), Some(Expr::PathExpr(_))));
    let arms: Vec<_> = expr.arms().collect();
    assert_eq!(arms.len(), 2);
    let Some(Pat::ConPat(con)) = arms[0].pat() else {
        panic!("expected a constructor pattern");
    };
    let segments: Vec<String> = con
        .path()
        .unwrap()
        .segments()
        .map(|segment| segment.text())
        .collect();
    assert_eq!(segments, ["Some"]);
    let args: Vec<SyntaxKind> = con.args().map(|pat| pat.syntax().kind()).collect();
    assert_eq!(args, [PAREN_PAT]);
    assert!(matches!(arms[0].body(), Some(Expr::PathExpr(_))));
    let Some(Pat::InfixConPat(infix)) = arms[1].pat() else {
        panic!("expected an infix constructor pattern");
    };
    assert!(matches!(infix.lhs(), Some(Pat::BindPat(_))));
    assert_eq!(infix.operator().unwrap().text(), ":+");
    assert!(matches!(infix.rhs(), Some(Pat::WildcardPat(_))));
}

#[test]
fn type_application_parts() {
    let file = source("f : Option (List Int) -> Int");
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    let Some(Type::FnType(function)) = signature.ty() else {
        panic!("expected a function type");
    };
    let Some(Type::AppType(app)) = function.param() else {
        panic!("expected a type application");
    };
    let segments: Vec<String> = app
        .path()
        .unwrap()
        .segments()
        .map(|segment| segment.text())
        .collect();
    assert_eq!(segments, ["Option"]);
    let args: Vec<SyntaxKind> = app.args().map(|ty| ty.syntax().kind()).collect();
    assert_eq!(args, [PAREN_TYPE]);
}

#[test]
fn tuple_and_literal_pattern_parts() {
    let file = source(
        "f : (Int, String) -> Int\nf (n, -1) = match (n, \"s\") with | (0, \"t\") -> 1 | _ -> 2",
    );
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    let Some(Type::FnType(function)) = signature.ty() else {
        panic!("expected a function type");
    };
    let Some(Type::TupleType(tuple)) = function.param() else {
        panic!("expected a tuple type");
    };
    let elements: Vec<SyntaxKind> = tuple.elements().map(|ty| ty.syntax().kind()).collect();
    assert_eq!(elements, [PATH_TYPE, PATH_TYPE]);
    let literal = |pat: Pat| match pat {
        Pat::LiteralPat(literal) => literal.value(),
        _ => None,
    };
    let equation = first_equation(&file);
    let Some(Pat::TuplePat(params)) = equation.params().next() else {
        panic!("expected a tuple pattern");
    };
    let values: Vec<Option<LiteralValue>> = params.elements().map(literal).collect();
    assert_eq!(values, [None, Some(LiteralValue::Int(-1))]);
    let Some(Expr::MatchExpr(expr)) = equation.body() else {
        panic!("expected a match");
    };
    let Some(Expr::TupleExpr(scrutinee)) = expr.scrutinee() else {
        panic!("expected a tuple");
    };
    let elements: Vec<SyntaxKind> = scrutinee.elements().map(|e| e.syntax().kind()).collect();
    assert_eq!(elements, [PATH_EXPR, LITERAL]);
    let Some(Pat::TuplePat(arm)) = expr.arms().next().and_then(|arm| arm.pat()) else {
        panic!("expected a tuple pattern in the first arm");
    };
    let values: Vec<Option<LiteralValue>> = arm.elements().map(literal).collect();
    assert_eq!(
        values,
        [
            Some(LiteralValue::Int(0)),
            Some(LiteralValue::String("t".to_string()))
        ]
    );
}

#[test]
fn a_public_fixity_still_has_its_associativity() {
    // `pub` が最初のトークンになっても、結合の向きのキーワードを引く
    let file = source("pub infixr 6 +++");
    let Some(Item::FixityItem(fixity)) = file.items().next() else {
        panic!("expected a fixity declaration");
    };
    assert_eq!(fixity.assoc().unwrap().kind(), SyntaxKind::INFIXR_KW);
    assert_eq!(fixity.precedence().unwrap().text(), "6");
}

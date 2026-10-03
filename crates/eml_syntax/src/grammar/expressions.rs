//! 文と式 (spec §5 の body / stmt / expr / op_expr / app / postfix / atom)。

use super::*;
use crate::parser::CompletedMarker;

/// 中置の位置に置ける演算子。`-` は前置の負号にもなる。
const OPERATORS: TokenSet = TokenSet::new(&[OP, CONOP, MINUS]);

/// atom を始められるトークン。`ERROR_TOKEN` も atom として読み、診断は出さない (字句解析で報告済み)。
const ATOM_START: TokenSet = TokenSet::new(&[
    INT,
    FLOAT,
    CHAR,
    STRING,
    MULTILINE_STRING,
    RAW_STRING,
    COMMAND,
    LIDENT,
    UIDENT,
    L_PAREN,
    L_BRACK,
    L_BRACE,
    ERROR_TOKEN,
]);

/// atom ではないので、引数や演算の項の位置では括弧が要る式 (spec §5)。
const NEEDS_PARENS: TokenSet = TokenSet::new(&[IF_KW, MATCH_KW, LET_KW]);

/// body ::= block(stmt) | expr
pub(super) fn body(p: &mut Parser) {
    if p.at(LAYOUT_OPEN) {
        let m = p.start();
        block_of(p, "a statement", stmt);
        m.complete(p, BLOCK);
        return;
    }
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
}

/// stmt ::= 'let' pat (':' type)? '=' body | 'use' (pat '<-')? expr | expr
fn stmt(p: &mut Parser) -> bool {
    match p.current() {
        LET_KW => {
            let_stmt(p);
            true
        }
        USE_KW => {
            use_stmt(p);
            true
        }
        _ => {
            let m = p.start();
            if expr(p) {
                m.complete(p, EXPR_STMT);
                true
            } else {
                m.abandon(p);
                false
            }
        }
    }
}

/// ブロックの `let`。後ろに `in` が続けば、1行の形の `let ... in` 式 (spec §7) として式文にする。
fn let_stmt(p: &mut Parser) {
    let m = p.start();
    let_head_and_body(p);
    if p.eat(IN_KW) {
        if !expr(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected an expression",
                format!("found {}", describe(p)),
            );
        }
        let let_expr = m.complete(p, LET_EXPR);
        let_expr.precede(p).complete(p, EXPR_STMT);
    } else {
        m.complete(p, LET_STMT);
    }
}

/// `let pat (: type)? = body` の部分。
fn let_head_and_body(p: &mut Parser) {
    p.bump(LET_KW);
    if !patterns::pattern(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a pattern",
            format!("found {}", describe(p)),
        );
    }
    if p.eat(COLON) {
        types::type_(p);
    }
    if expect(p, EQ) {
        body(p);
    }
}

/// expr。何も読めなければ、診断を出さずに偽を返す。
pub(super) fn expr(p: &mut Parser) -> bool {
    match p.current() {
        IF_KW => if_expr(p),
        MATCH_KW => match_expr(p),
        LET_KW => let_expr(p),
        _ => return op_expr(p, false) != OpExpr::Nothing,
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpExpr {
    Nothing,
    Expr,
    /// `(a +)` の左セクション。被演算子と演算子は、呼び出し側のノードの直下に平たく残る。
    LeftSection,
}

/// op_expr ::= operand (OP operand)* 。CST では `OP_SEQ` に平たく並べる (spec §7)。
/// 前置の `-` も `OP_SEQ` の中のトークンとして置き、HIR が `negate` として組み直す。
/// 演算子が1つもなければ `OP_SEQ` を作らない。
/// `section` が真なら、括弧の直下の `(a +)` を検出して `LeftSection` を返す。
fn op_expr(p: &mut Parser, section: bool) -> OpExpr {
    let m = p.start();
    let mut operands = 0;
    let mut has_operator = false;
    loop {
        while p.at(MINUS) {
            p.bump(MINUS);
            has_operator = true;
        }
        if !operand(p) {
            if has_operator {
                p.error(
                    codes::SYNTAX_ERROR,
                    "expected an expression",
                    format!("found {}", describe(p)),
                );
            }
            break;
        }
        operands += 1;
        if !p.at_ts(OPERATORS) {
            break;
        }
        if section && operands == 1 && p.nth(1) == R_PAREN {
            p.bump_any();
            m.abandon(p);
            return OpExpr::LeftSection;
        }
        p.bump_any();
        has_operator = true;
    }
    if has_operator {
        m.complete(p, OP_SEQ);
        OpExpr::Expr
    } else {
        m.abandon(p);
        if operands > 0 {
            OpExpr::Expr
        } else {
            OpExpr::Nothing
        }
    }
}

/// operand ::= app | lambda 。括弧の要る式は E0012 を出して読む。
fn operand(p: &mut Parser) -> bool {
    if p.at_ts(ATOM_START) {
        app(p);
    } else if p.at(FN_KW) {
        lambda(p);
    } else if p.at_ts(NEEDS_PARENS) {
        needs_parens(p);
    } else {
        return false;
    }
    true
}

/// app ::= postfix+ lambda? 。引数が1つ以上あれば `APP_EXPR` にする。
fn app(p: &mut Parser) {
    let m = p.start();
    postfix(p);
    let mut args = 0;
    loop {
        if p.at_ts(ATOM_START) {
            postfix(p);
            args += 1;
        } else if p.at(FN_KW) {
            // 最後の引数のラムダは、括弧なしで書ける (spec §7)。
            lambda(p);
            args += 1;
            break;
        } else if p.at_ts(NEEDS_PARENS) {
            needs_parens(p);
            args += 1;
            break;
        } else {
            break;
        }
    }
    if args > 0 {
        m.complete(p, APP_EXPR);
    } else {
        m.abandon(p);
    }
}

/// postfix ::= atom ('.' (LIDENT | INT))*
fn postfix(p: &mut Parser) -> bool {
    let Some(mut lhs) = atom(p) else {
        return false;
    };
    while p.at(DOT) && matches!(p.nth(1), LIDENT | INT) {
        let m = lhs.precede(p);
        dot(p);
        p.bump_any();
        lhs = m.complete(p, FIELD_EXPR);
    }
    true
}

fn atom(p: &mut Parser) -> Option<CompletedMarker> {
    let m = p.start();
    let kind = match p.current() {
        INT | STRING => {
            p.bump_any();
            LITERAL
        }
        kind @ (FLOAT | CHAR | MULTILINE_STRING | RAW_STRING | COMMAND) => {
            not_yet_supported(p, unsupported_literal_message(kind));
            p.bump_any();
            LITERAL
        }
        LIDENT | UIDENT => {
            qname(p);
            PATH_EXPR
        }
        L_PAREN => paren_expr(p),
        L_BRACK => {
            unsupported_group(p, "lists are not supported yet");
            ERROR
        }
        L_BRACE => {
            unsupported_group(p, "records are not supported yet");
            ERROR
        }
        ERROR_TOKEN => {
            p.bump_any();
            ERROR
        }
        _ => {
            m.abandon(p);
            return None;
        }
    };
    Some(m.complete(p, kind))
}

fn unsupported_literal_message(kind: SyntaxKind) -> &'static str {
    match kind {
        FLOAT => "floating-point literals are not supported yet",
        CHAR => "character literals are not supported yet",
        MULTILINE_STRING => "multi-line strings are not supported yet",
        RAW_STRING => "raw strings are not supported yet",
        _ => "command literals are not supported yet",
    }
}

/// qvar | qcon ::= (UIDENT '.')* (LIDENT | UIDENT)
fn qname(p: &mut Parser) {
    while p.at(UIDENT) && p.nth(1) == DOT && matches!(p.nth(2), UIDENT | LIDENT) {
        p.bump(UIDENT);
        dot(p);
    }
    p.bump_any();
}

/// `(` で始まる atom。単位、括弧、タプル、型の明示、演算子の参照、セクション。
fn paren_expr(p: &mut Parser) -> SyntaxKind {
    p.bump(L_PAREN);
    if p.eat(R_PAREN) {
        return UNIT_EXPR;
    }
    if p.at_ts(OPERATORS) && p.nth(1) == R_PAREN {
        p.bump_any();
        p.bump(R_PAREN);
        return OP_REF;
    }
    if p.at(OP) || p.at(CONOP) {
        // 右セクション。`(- 1)` は負の数なので、ここには来ない (spec §7)。
        p.bump_any();
        while p.at(MINUS) {
            p.bump(MINUS);
        }
        if !operand(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected an expression",
                format!("found {}", describe(p)),
            );
        }
        expect(p, R_PAREN);
        return RIGHT_SECTION;
    }
    if p.at(DOT) && p.nth(1) == LIDENT {
        // `(.name)`。`.` と名前の間だけを詰める。
        if !p.touches_next() {
            p.error(
                codes::SPACE_AROUND_DOT,
                "unexpected whitespace around `.`",
                "write `.name` without spaces",
            );
        }
        p.bump(DOT);
        p.bump(LIDENT);
        expect(p, R_PAREN);
        return FIELD_SECTION;
    }
    let inner = if p.at_ts(NEEDS_PARENS) {
        expr(p);
        OpExpr::Expr
    } else {
        op_expr(p, true)
    };
    match inner {
        OpExpr::LeftSection => {
            expect(p, R_PAREN);
            return LEFT_SECTION;
        }
        OpExpr::Nothing => p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        ),
        OpExpr::Expr => {}
    }
    if p.eat(COLON) {
        types::type_(p);
        expect(p, R_PAREN);
        return ANNOT_EXPR;
    }
    if p.at(COMMA) {
        let mut count = 1;
        while p.eat(COMMA) {
            if p.at(R_PAREN) && count >= 2 {
                break;
            }
            if !expr(p) {
                p.error(
                    codes::SYNTAX_ERROR,
                    "expected an expression",
                    format!("found {}", describe(p)),
                );
                break;
            }
            count += 1;
        }
        expect(p, R_PAREN);
        return TUPLE_EXPR;
    }
    expect(p, R_PAREN);
    PAREN_EXPR
}
/// 括弧の要る式を、引数や演算の項の位置で見つけた。E0012 を出し、回復のためにそのまま式として読む。
fn needs_parens(p: &mut Parser) {
    p.error(
        codes::NEEDS_PARENS,
        format!(
            "`{}` expression must be parenthesized here",
            p.current_text()
        ),
        "wrap it in parentheses",
    );
    expr(p);
}

/// 'if' expr 'then' body ('else' body)?
fn if_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(IF_KW);
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    skip_sep_before(p, THEN_KW);
    if expect(p, THEN_KW) {
        body(p);
        skip_sep_before(p, ELSE_KW);
        if p.eat(ELSE_KW) {
            body(p);
        }
    }
    m.complete(p, IF_EXPR);
}

/// `if` と同じ列に書いた `then` / `else` の前の `SEP` を1つ読み飛ばす (spec §4、Haskell の DoAndIfThenElse)。
fn skip_sep_before(p: &mut Parser, kind: SyntaxKind) {
    if p.at_sep() && p.nth(1) == kind {
        p.bump_any();
    }
}

/// 'match' expr 'with' arms
fn match_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(MATCH_KW);
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    if expect(p, WITH_KW) {
        branches(p, "an arm starting with `|`", match_arm);
    }
    m.complete(p, MATCH_EXPR);
}

/// `with` の後ろの枝の並び。字下げしたブロックか、同じ行に並べた枝 (spec §7)。
/// 同じ行の形では、枝の本体は次の `|` の手前で終わる (`|` 単独は演算子ではないため)。
fn branches(p: &mut Parser, expected: &str, branch: fn(&mut Parser) -> bool) {
    if p.at(LAYOUT_OPEN) {
        block_of(p, expected, branch);
        return;
    }
    if !p.at(PIPE) {
        p.error(
            codes::SYNTAX_ERROR,
            format!("expected {expected}"),
            format!("found {}", describe(p)),
        );
        return;
    }
    while p.at(PIPE) {
        branch(p);
    }
}

/// arm ::= '|' pat '->' body 。ブロックの中で `|` を書き忘れた枝も、診断したうえで読む。
fn match_arm(p: &mut Parser) -> bool {
    if !p.at(PIPE) && !patterns::at_apat_start(p) {
        return false;
    }
    let m = p.start();
    if !p.eat(PIPE) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected `|` before the arm",
            "each arm starts with `|`",
        );
    }
    if !patterns::pattern(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a pattern",
            format!("found {}", describe(p)),
        );
    }
    if expect(p, THIN_ARROW) {
        body(p);
    }
    m.complete(p, MATCH_ARM);
    true
}

/// lambda ::= 'fn' param+ '->' body
fn lambda(p: &mut Parser) {
    let m = p.start();
    p.bump(FN_KW);
    let mut params = 0;
    while patterns::at_apat_start(p) {
        patterns::param(p);
        params += 1;
    }
    if params == 0 {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a parameter",
            format!("found {}", describe(p)),
        );
    }
    if expect(p, THIN_ARROW) {
        body(p);
    }
    m.complete(p, LAMBDA_EXPR);
}

/// 式の位置の 'let' pat (':' type)? '=' expr 'in' expr
fn let_expr(p: &mut Parser) {
    let m = p.start();
    let_head_and_body(p);
    if expect(p, IN_KW) && !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    m.complete(p, LET_EXPR);
}

/// 'use' (pat '<-')? expr 。脱糖は HIR で行う (spec §7)。
fn use_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(USE_KW);
    if has_left_arrow(p) {
        if !patterns::pattern(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a pattern",
                format!("found {}", describe(p)),
            );
        }
        expect(p, LEFT_ARROW);
    }
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    m.complete(p, USE_STMT);
}

/// 文の終わりまでに、括弧とブロックの外の `<-` があるか (`use p <- e` の形か)。
fn has_left_arrow(p: &Parser) -> bool {
    let mut depth = 0u32;
    let mut n = 0;
    loop {
        match p.nth(n) {
            LEFT_ARROW if depth == 0 => return true,
            L_PAREN | L_BRACK | L_BRACE | LAYOUT_OPEN => depth += 1,
            R_PAREN | R_BRACK | R_BRACE | LAYOUT_CLOSE => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            LAYOUT_SEP | SEMICOLON if depth == 0 => return false,
            EOF => return false,
            _ => {}
        }
        n += 1;
    }
}

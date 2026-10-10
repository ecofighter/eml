use super::*;
use crate::parser::CompletedMarker;

const OPERATORS: TokenSet = TokenSet::new(&[OP, CONOP, MINUS]);

/// `ERROR_TOKEN` も atom として読む。字句解析で報告済みなので、診断を重ねずに先へ進めるため。
const ATOM_START: TokenSet = TokenSet::new(&[
    INT,
    FLOAT,
    CHAR,
    RAW_STRING,
    STRING_START,
    CMD_START,
    LIDENT,
    UIDENT,
    L_PAREN,
    L_BRACK,
    L_BRACE,
    ERROR_TOKEN,
]);

/// 本体が右へできるだけ伸びる形なので、引数や演算の項の位置では括弧が要る (docs/spec/grammar.md の「文法上の補足」)。
/// これにより、`match e with` の `e` が `with` の手前で終わる。
const EXPR_FORMS: TokenSet = TokenSet::new(&[IF_KW, MATCH_KW, HANDLE_KW, FN_KW, LET_KW]);

pub(super) fn body(p: &mut Parser) {
    if p.at(LAYOUT_OPEN) {
        let m = p.start();
        // HIR は `use` の後の残りの文をラムダで包むので、`use` ごとに、残りの文を1段深く数える
        // (docs/spec/expressions.md の「`use`」)
        let mut entered = 0;
        block_of(p, "a statement", |p| {
            let is_use = p.at(USE_KW);
            stmt(p) && (!is_use || enter_link(p, 0, &mut entered))
        });
        for _ in 0..entered {
            p.leave();
        }
        m.complete(p, BLOCK);
        return;
    }
    if !expr(p) {
        expected(p, "an expression");
    }
}

/// `let` のブロックの入れ子は `expr` を通らずに `stmt` へ戻るので、ここでも深さを数える。
fn stmt(p: &mut Parser) -> bool {
    nested(p, true, stmt_inner)
}

fn stmt_inner(p: &mut Parser) -> bool {
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

/// `let ... in` 式とは `in` まで読まないと区別できないので、`in` が続けば `LET_EXPR` の式文に作り替える。
fn let_stmt(p: &mut Parser) {
    let m = p.start();
    let_head_and_body(p);
    if p.eat(IN_KW) {
        if !expr(p) {
            expected(p, "an expression");
        }
        let let_expr = m.complete(p, LET_EXPR);
        let_expr.precede(p).complete(p, EXPR_STMT);
    } else {
        m.complete(p, LET_STMT);
    }
}

fn let_head_and_body(p: &mut Parser) {
    p.bump(LET_KW);
    if !patterns::pattern(p) {
        expected(p, "a pattern");
    }
    if p.eat(COLON) {
        types::type_(p);
    }
    if expect(p, EQ) {
        body(p);
    }
}

/// 何も読めなければ、診断を出さずに偽を返す。何を期待していたかを知っている呼び出し側が診断するため。
pub(super) fn expr(p: &mut Parser) -> bool {
    nested(p, true, expr_inner)
}

fn expr_inner(p: &mut Parser) -> bool {
    match p.current() {
        IF_KW => if_expr(p),
        MATCH_KW => match_expr(p),
        HANDLE_KW => handle_expr(p),
        FN_KW => lambda(p),
        LET_KW => let_expr(p),
        _ => return op_expr(p, false) != OpExpr::Nothing,
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpExpr {
    Nothing,
    Expr,
    /// `(a +)` の左セクション。被演算子 (列なら `OP_SEQ`) と演算子は、呼び出し側のノードの直下に残る。
    LeftSection,
}

/// 演算子が1つもなければ `OP_SEQ` を作らない。1つの被演算子を余計なノードで包まないため。
/// `section` が真なら、括弧の直下の `(a +)` を検出して `LeftSection` を返す。
fn op_expr(p: &mut Parser, section: bool) -> OpExpr {
    nested(p, OpExpr::Expr, |p| op_expr_inner(p, section))
}

/// 列は再帰せずに読むが、HIR は演算子1つにつき1段深い木に組み直すので、前置の `-` も含めて演算子ごとに
/// 入れ子の深さを数える。組み直した木では、読み終えた被演算子が後の演算子の段の下に来うるので、演算子の段は
/// それまでの被演算子の高さの最大を下に確保して数える。数えないと、長い列の木や、括弧に入れた列を左に重ねた木を
/// 後の段階が再帰してスタックを溢れさせる (docs/spec/grammar.md)。
fn op_expr_inner(p: &mut Parser, section: bool) -> OpExpr {
    let mut entered = 0;
    let result = op_seq(p, section, &mut entered);
    for _ in 0..entered {
        p.leave();
    }
    result
}

/// 再帰せずに読む連鎖 (演算子の列、フィールドの参照) の1段を、下に `reserved` 段の高さを確保して入れ子の1段に
/// 数える。上限に達したら、`too_deep` が今の括弧かブロックの中身を読み飛ばすので、連鎖はそこで終わる。
fn enter_link(p: &mut Parser, reserved: u32, entered: &mut u32) -> bool {
    if !p.enter_reserving(reserved) {
        too_deep(p);
        return false;
    }
    *entered += 1;
    true
}

fn op_seq(p: &mut Parser, section: bool, entered: &mut u32) -> OpExpr {
    let m = p.start();
    let mut operands = 0;
    let mut has_operator = false;
    let mut tallest = 0;
    'seq: loop {
        while p.at(MINUS) {
            if !enter_link(p, tallest, entered) {
                break 'seq;
            }
            p.bump(MINUS);
            has_operator = true;
        }
        let (parsed, height) = p.measure(operand);
        if !parsed {
            if has_operator {
                expected(p, "an expression");
            }
            break;
        }
        operands += 1;
        tallest = tallest.max(height);
        if !p.at_ts(OPERATORS) {
            break;
        }
        if section && p.nth(1) == R_PAREN {
            // `(a * b +)` のように、被演算子が演算子の列でもよい。列は `OP_SEQ` にまとめ、セクションの演算子は
            // その外に置く。
            if has_operator {
                m.complete(p, OP_SEQ);
            } else {
                m.abandon(p);
            }
            p.bump_any();
            return OpExpr::LeftSection;
        }
        if !enter_link(p, tallest, entered) {
            break;
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

fn operand(p: &mut Parser) -> bool {
    if p.at_ts(ATOM_START) || p.at(DROP_KW) {
        app(p);
    } else if p.at_ts(EXPR_FORMS) {
        misplaced(p);
    } else {
        return false;
    }
    true
}

/// 引数がなければ `APP_EXPR` を作らない。1つの atom を余計なノードで包まないため。
fn app(p: &mut Parser) {
    let m = p.start();
    // `drop` は引数が atom なので右へ伸びず、演算の項には書けるが、引数の位置では括弧が要る (docs/spec/grammar.md の「文法上の補足」)
    let is_drop = p.at(DROP_KW);
    // `drop` の最初の引数も、ほかの引数と同じループで層を確かめる。
    if is_drop {
        p.bump_any();
    } else {
        postfix(p);
    }
    let mut args = 0;
    loop {
        if p.at_ts(ATOM_START) {
            postfix(p);
            args += 1;
        } else if p.at_ts(EXPR_FORMS) || p.at(DROP_KW) {
            misplaced(p);
            args += 1;
            break;
        } else {
            break;
        }
    }
    if is_drop && args == 0 {
        expected(p, "an expression");
    }
    if is_drop {
        m.complete(p, DROP_EXPR);
    } else if args > 0 {
        m.complete(p, APP_EXPR);
    } else {
        m.abandon(p);
    }
}

/// 連鎖は再帰せずに深い木を作るので、各段を入れ子の深さに数える。先頭の atom は連鎖の段の数だけ深くなるので、
/// 各段はその高さを下に確保して数える。数えないと、長い連鎖の木の解放がスタックを溢れさせる (docs/spec/grammar.md)。
fn postfix(p: &mut Parser) -> bool {
    let (Some(mut lhs), height) = p.measure(atom) else {
        return false;
    };
    let mut entered = 0;
    while p.at(DOT) && matches!(p.nth(1), LIDENT | INT) {
        if !enter_link(p, height, &mut entered) {
            break;
        }
        let m = lhs.precede(p);
        dot(p);
        p.bump_any();
        lhs = m.complete(p, FIELD_EXPR);
    }
    for _ in 0..entered {
        p.leave();
    }
    true
}

fn atom(p: &mut Parser) -> Option<CompletedMarker> {
    let m = p.start();
    let kind = match p.current() {
        INT | FLOAT | CHAR | RAW_STRING => {
            p.bump_any();
            LITERAL
        }
        STRING_START | CMD_START => string_lit(p, false),
        LIDENT | UIDENT => {
            qname(p);
            PATH_EXPR
        }
        L_PAREN => paren_expr(p),
        L_BRACK => list_expr(p),
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

/// `string ::= STRING_START (STRING_TEXT | ESCAPE | interp)* STRING_END` と `command`。閉じていない文字列は lexer が
/// 報告済みなので、`STRING_END` がなくても黙って終える。パターンの文字列 (`in_pattern`) も同じ形に組み、穴ごとに
/// 誤りを出す (docs/spec/grammar.md の「文法上の補足」)。
pub(super) fn string_lit(p: &mut Parser, in_pattern: bool) -> SyntaxKind {
    let (text, end, node) = if p.at(CMD_START) {
        (CMD_TEXT, CMD_END, COMMAND_LIT)
    } else {
        (STRING_TEXT, STRING_END, STRING_LIT)
    };
    p.bump_any();
    loop {
        match p.current() {
            kind if kind == text || kind == ESCAPE => p.bump_any(),
            INTERP_START => {
                if in_pattern {
                    p.error(
                        codes::SYNTAX_ERROR,
                        "string interpolation is not allowed in a pattern",
                        "a pattern matches a fixed string",
                    );
                }
                interp(p, node == COMMAND_LIT);
            }
            _ => break,
        }
    }
    p.eat(end);
    node
}

/// 穴は括弧と同じく1段に数えるので、`paren_expr` と同じく、`expr` の形で始まらない式は `op_expr` で直接読む
/// (docs/spec/grammar.md の「文法上の補足」)。lexer が `INTERP_END` を必ず出すので、
/// 穴はいつもそこで終わる。幅 0 の `INTERP_END` は閉じていない穴で、そこでの誤りは `Parser::error` が抑える。
fn interp(p: &mut Parser, spread: bool) {
    let m = p.start();
    p.bump(INTERP_START);
    if spread {
        p.eat(DOT2);
    }
    let read = if p.at_ts(EXPR_FORMS) {
        expr(p)
    } else {
        op_expr(p, false) != OpExpr::Nothing
    };
    if !read {
        expected(p, "an expression");
    }
    if !p.at(INTERP_END) {
        if read {
            expected(p, token_name(INTERP_END));
        }
        // 括弧の対応は見ずに、この穴の `INTERP_END` まで読み飛ばす。lexer が穴を1行に区切り、閉じを必ず出すので、
        // 文字列の外には出ない。穴の中の入れ子の文字列の穴は、対で数えて飛ばす
        let skipped = p.start();
        let mut holes = 0u32;
        while !(p.at_eof() || holes == 0 && p.at(INTERP_END)) {
            match p.current() {
                INTERP_START => holes += 1,
                INTERP_END => holes -= 1,
                _ => {}
            }
            p.bump_any();
        }
        skipped.complete(p, ERROR);
    }
    p.eat(INTERP_END);
    m.complete(p, INTERP);
}

/// `qvar ::= (UIDENT '.')* LIDENT` と `qcon`。
fn qname(p: &mut Parser) {
    path(p, TokenSet::new(&[UIDENT, LIDENT]));
}

/// 節の先頭は操作の名前なので `qvar` に限る。`M.N` のような `qcon` は、ここでは読まずに誤りにする。
fn at_qvar(p: &Parser) -> bool {
    let mut n = 0;
    while p.peek(n) == UIDENT && p.peek(n + 1) == DOT {
        n += 2;
    }
    p.peek(n) == LIDENT
}

/// 要素は `expr` として読むので、`if` や `fn` も括弧なしで書ける。`[` は括弧と同じく1段に数える
/// (docs/spec/grammar.md の「文法上の補足」)。そのため、`paren_expr` と同じく、`expr` の形で始まらない要素は
/// `op_expr` で直接読み、`expr` の段を重ねない。
fn list_expr(p: &mut Parser) -> SyntaxKind {
    p.bump(L_BRACK);
    if !p.at(R_BRACK) {
        loop {
            let read = if p.at_ts(EXPR_FORMS) {
                expr(p)
            } else {
                op_expr(p, false) != OpExpr::Nothing
            };
            if !read {
                expected(p, "an expression");
                break;
            }
            if !p.eat(COMMA) || p.at(R_BRACK) {
                break;
            }
        }
    }
    close_bracket(p, R_BRACK);
    LIST_EXPR
}

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
        // `(- 1)` は右セクションではなく負の数なので (docs/spec/expressions.md)、`-` はここに来ない。
        // 被演算子は演算子の列でもよい。優先順位による可否は HIR で検査する。
        p.bump_any();
        if op_expr(p, false) == OpExpr::Nothing {
            expected(p, "an expression");
        }
        close_bracket(p, R_PAREN);
        return RIGHT_SECTION;
    }
    if p.at(DOT) && p.nth(1) == LIDENT {
        // `(.name)` では、`.` と名前の間だけに空白を禁じる。
        if !p.touches_next() {
            p.error(
                codes::SPACE_AROUND_DOT,
                "unexpected whitespace around `.`",
                "write `.name` without spaces",
            );
        }
        p.bump(DOT);
        p.bump(LIDENT);
        close_bracket(p, R_PAREN);
        return FIELD_SECTION;
    }
    let inner = if p.at_ts(EXPR_FORMS) {
        expr(p);
        OpExpr::Expr
    } else {
        op_expr(p, true)
    };
    match inner {
        OpExpr::LeftSection => {
            close_bracket(p, R_PAREN);
            return LEFT_SECTION;
        }
        OpExpr::Nothing => expected(p, "an expression"),
        OpExpr::Expr => {}
    }
    if p.eat(COLON) {
        types::type_(p);
        close_bracket(p, R_PAREN);
        return ANNOT_EXPR;
    }
    if p.at(COMMA) {
        let mut count = 1;
        while p.eat(COMMA) {
            if p.at(R_PAREN) && count >= 2 {
                break;
            }
            if !expr(p) {
                expected(p, "an expression");
                break;
            }
            count += 1;
        }
        close_bracket(p, R_PAREN);
        return TUPLE_EXPR;
    }
    close_bracket(p, R_PAREN);
    PAREN_EXPR
}

/// E0012 を出した後も、回復のためにその形を本来の層で読む。`drop` を `app` で読むのは、
/// `g drop k + 2` を `g (drop k) + 2` と同じ木にして、`+ 2` を取り込まないため。
/// 深さは E0012 より先に数える。上限に達した位置で E0013 を出すためである (同じ位置の診断は1件しか残らない)。
/// `app` は自分では深さを数えないので、`g drop drop …` の再帰もここで数える。`expr` の形は `expr` でも
/// 数えるので1段多くなるが、上限に少し早く届くだけである。
fn misplaced(p: &mut Parser) {
    nested(p, (), |p| {
        p.error(
            codes::NEEDS_PARENS,
            format!(
                "`{}` expression must be parenthesized here",
                p.current_text()
            ),
            "wrap it in parentheses",
        );
        if p.at(DROP_KW) {
            app(p);
        } else {
            expr(p);
        }
    });
}

fn if_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(IF_KW);
    if !expr(p) {
        expected(p, "an expression");
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

/// `if` と同じ列に書いた `then` / `else` の前には、レイアウト段が `SEP` を入れる。`else` で始まる文はないので、
/// その `SEP` を1つ読み飛ばしても曖昧にならない (Haskell の DoAndIfThenElse。docs/spec/layout.md)。
fn skip_sep_before(p: &mut Parser, kind: SyntaxKind) {
    if p.at_sep() && p.nth(1) == kind {
        p.bump_any();
    }
}

fn match_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(MATCH_KW);
    if !expr(p) {
        expected(p, "an expression");
    }
    if expect(p, WITH_KW) {
        branches(p, "an arm starting with `|`", match_arm);
    }
    m.complete(p, MATCH_EXPR);
}

/// 同じ行に並べた枝では、本体は次の `|` の手前で終わる。`|` 単独は演算子ではないため。
fn branches(p: &mut Parser, what: &str, branch: fn(&mut Parser) -> bool) {
    if p.at(LAYOUT_OPEN) {
        // 中身のないブロックは、`with` の次の行が字下げされていないときにレイアウト段が E0009 を出して作ったもの。
        // 続く `|` の行はこの式の枝なので、診断を重ねずに枝として読む
        // (docs/implementation/architecture.md の「構文解析の回復」)。
        let recovered = p.nth(1) == LAYOUT_CLOSE;
        block_of(p, what, branch);
        if recovered {
            while p.at(LAYOUT_SEP) && p.nth(1) == PIPE {
                p.bump(LAYOUT_SEP);
                branch(p);
            }
        }
        return;
    }
    if !p.at(PIPE) {
        expected(p, what);
        return;
    }
    while p.at(PIPE) {
        branch(p);
    }
}

/// ブロックの中で `|` を書き忘れた枝も、診断したうえで枝として読む。
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
        expected(p, "a pattern");
    }
    if expect(p, THIN_ARROW) {
        body(p);
    }
    m.complete(p, MATCH_ARM);
    true
}

fn lambda(p: &mut Parser) {
    let m = p.start();
    p.bump(FN_KW);
    let mut params = 0;
    while patterns::at_apat_start(p) {
        patterns::apat(p);
        params += 1;
    }
    if params == 0 {
        expected(p, "a parameter");
    }
    if expect(p, THIN_ARROW) {
        body(p);
    }
    m.complete(p, LAMBDA_EXPR);
}

fn let_expr(p: &mut Parser) {
    let m = p.start();
    let_head_and_body(p);
    if expect(p, IN_KW) && !expr(p) {
        expected(p, "an expression");
    }
    m.complete(p, LET_EXPR);
}

/// 脱糖は HIR で行うので、ここでは形だけを読む (docs/spec/expressions.md)。
fn use_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(USE_KW);
    if has_left_arrow(p) {
        if !patterns::pattern(p) {
            expected(p, "a pattern");
        }
        expect(p, LEFT_ARROW);
    }
    if !expr(p) {
        expected(p, "an expression");
    }
    m.complete(p, USE_STMT);
}

/// `use p <- e` と `use e` は `<-` まで読まないと区別できないので、括弧とブロックの外の `<-` を文の終わりまで探す。
fn has_left_arrow(p: &Parser) -> bool {
    let mut nesting = Nesting::default();
    let mut n = 0;
    loop {
        let kind = p.peek(n);
        if nesting.ends(kind) {
            return false;
        }
        if nesting.at_top() {
            match kind {
                LEFT_ARROW => return true,
                SEMICOLON => return false,
                _ => {}
            }
        }
        nesting.step(kind);
        n += 1;
    }
}

fn handle_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(HANDLE_KW);
    if !expr(p) {
        expected(p, "an expression");
    }
    if p.eat(FROM_KW) && !expr(p) {
        expected(p, "the initial state");
    }
    if expect(p, WITH_KW) {
        branches(p, "a clause starting with `|`", handler_clause);
    }
    m.complete(p, HANDLE_EXPR);
}

fn handler_clause(p: &mut Parser) -> bool {
    if !p.at(PIPE) {
        return false;
    }
    let m = p.start();
    p.bump(PIPE);
    let kind = if p.eat(RETURN_KW) {
        if !patterns::at_apat_start(p) {
            expected(p, "a pattern");
        }
        RETURN_CLAUSE
    } else {
        if at_qvar(p) {
            qname(p);
        } else {
            expected(p, token_name(LIDENT));
        }
        OP_CLAUSE
    };
    while patterns::at_apat_start(p) {
        patterns::apat(p);
    }
    if expect(p, THIN_ARROW) {
        body(p);
    }
    m.complete(p, kind);
    true
}

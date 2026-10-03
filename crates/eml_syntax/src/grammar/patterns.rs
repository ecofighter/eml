use super::*;

pub(super) fn at_apat_start(p: &Parser) -> bool {
    at_apat_start_at(p, 0)
}

/// `-` は、整数が続くときだけ負の数のリテラルとして apat を始める。
pub(super) fn at_apat_start_at(p: &Parser, n: usize) -> bool {
    match p.nth(n) {
        UNDERSCORE | LIDENT | UIDENT | INT | STRING | CHAR | L_PAREN | L_BRACK | L_BRACE => true,
        MINUS => p.nth(n + 1) == INT,
        _ => false,
    }
}

/// 項目の種類を決める先読みに使うので、括弧は対応する閉じ括弧までを数える。
pub(super) fn apat_len(p: &Parser) -> Option<usize> {
    match p.current() {
        UNDERSCORE | LIDENT | INT | STRING | CHAR => Some(1),
        MINUS if p.nth(1) == INT => Some(2),
        UIDENT => {
            let mut n = 1;
            while p.nth(n) == DOT && p.nth(n + 1) == UIDENT {
                n += 2;
            }
            Some(n)
        }
        L_PAREN | L_BRACK | L_BRACE => {
            let mut depth = 0u32;
            let mut n = 0;
            loop {
                match p.peek(n) {
                    L_PAREN | L_BRACK | L_BRACE => depth += 1,
                    R_PAREN | R_BRACK | R_BRACE => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(n + 1);
                        }
                    }
                    EOF => return None,
                    _ => {}
                }
                n += 1;
            }
        }
        _ => None,
    }
}

pub(super) fn pattern(p: &mut Parser) -> bool {
    let m = p.start();
    if !cpat(p) {
        m.abandon(p);
        return false;
    }
    if p.at(CONOP) {
        p.bump(CONOP);
        if !pattern(p) {
            expected(p, "a pattern");
        }
        m.complete(p, INFIX_CON_PAT);
    } else {
        m.abandon(p);
    }
    true
}

fn cpat(p: &mut Parser) -> bool {
    if !p.at(UIDENT) {
        return apat(p);
    }
    let m = p.start();
    qcon(p);
    while at_apat_start(p) {
        apat(p);
    }
    m.complete(p, CON_PAT);
    true
}

pub(super) fn apat(p: &mut Parser) -> bool {
    apat_with(p, false)
}

/// `apat` と違い、`(pat : type)` も書ける。
pub(super) fn param(p: &mut Parser) -> bool {
    apat_with(p, true)
}

fn apat_with(p: &mut Parser, annotated: bool) -> bool {
    let m = p.start();
    let kind = match p.current() {
        UNDERSCORE => {
            p.bump_any();
            WILDCARD_PAT
        }
        LIDENT => {
            p.bump_any();
            BIND_PAT
        }
        UIDENT => {
            qcon(p);
            CON_PAT
        }
        INT | STRING => {
            p.bump_any();
            LITERAL_PAT
        }
        CHAR => {
            not_yet_supported(p, "character literals are not supported yet");
            p.bump_any();
            LITERAL_PAT
        }
        MINUS if p.nth(1) == INT => {
            p.bump(MINUS);
            p.bump(INT);
            LITERAL_PAT
        }
        L_PAREN => paren_pat(p, annotated),
        L_BRACK => {
            unsupported_group(p, "lists are not supported yet");
            ERROR
        }
        L_BRACE => {
            unsupported_group(p, "records are not supported yet");
            ERROR
        }
        _ => {
            m.abandon(p);
            return false;
        }
    };
    m.complete(p, kind);
    true
}

fn paren_pat(p: &mut Parser, annotated: bool) -> SyntaxKind {
    p.bump(L_PAREN);
    if p.eat(R_PAREN) {
        return UNIT_PAT;
    }
    if !pattern(p) {
        expected(p, "a pattern");
    }
    let kind = if annotated && p.eat(COLON) {
        types::type_(p);
        ANNOT_PAT
    } else if p.at(COMMA) {
        let mut count = 1;
        while p.eat(COMMA) {
            if p.at(R_PAREN) && count >= 2 {
                break;
            }
            if !pattern(p) {
                expected(p, "a pattern");
                break;
            }
            count += 1;
        }
        TUPLE_PAT
    } else {
        PAREN_PAT
    };
    close_bracket(p, R_PAREN);
    kind
}

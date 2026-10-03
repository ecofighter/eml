//! パターン (spec §5 の pat / cpat / apat)。

use super::*;

pub(super) fn at_apat_start(p: &Parser) -> bool {
    at_apat_start_at(p, 0)
}

/// `n` 個先から apat を始められるか。`-` は整数が続くときだけ (負の数のリテラル)。
pub(super) fn at_apat_start_at(p: &Parser, n: usize) -> bool {
    match p.nth(n) {
        UNDERSCORE | LIDENT | UIDENT | INT | STRING | CHAR | L_PAREN | L_BRACK | L_BRACE => true,
        MINUS => p.nth(n + 1) == INT,
        _ => false,
    }
}

/// 今の位置から始まる apat のトークン数。apat でなければ `None`。
/// 括弧は対応する閉じ括弧までを数える (項目の種類を決める先読みに使う)。
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
                match p.nth(n) {
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

/// pat ::= cpat (CONOP pat)?  (右結合)
pub(super) fn pattern(p: &mut Parser) -> bool {
    let m = p.start();
    if !cpat(p) {
        m.abandon(p);
        return false;
    }
    if p.at(CONOP) {
        p.bump(CONOP);
        if !pattern(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a pattern",
                format!("found {}", describe(p)),
            );
        }
        m.complete(p, INFIX_CON_PAT);
    } else {
        m.abandon(p);
    }
    true
}

/// cpat ::= qcon apat+ | apat
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

/// ラムダの引数。`(pat : type)` も書ける (spec §5 の param)。
#[allow(dead_code)] // Task 6 のラムダで使う。
pub(super) fn param(p: &mut Parser) -> bool {
    apat_with(p, true)
}

/// apat ::= '_' | LIDENT | qcon | literal | '-' INT | '(' ')' | '(' pat ')' | '(' pat (',' pat)+ ','? ')'
///        | リスト (S2) | レコード (S2)
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
        p.error(
            codes::SYNTAX_ERROR,
            "expected a pattern",
            format!("found {}", describe(p)),
        );
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
                p.error(
                    codes::SYNTAX_ERROR,
                    "expected a pattern",
                    format!("found {}", describe(p)),
                );
                break;
            }
            count += 1;
        }
        TUPLE_PAT
    } else {
        PAREN_PAT
    };
    expect(p, R_PAREN);
    kind
}

//! 型と row (spec §5 の type / btype / type_atom / row)。

use super::*;

const TYPE_ATOM_START: TokenSet = TokenSet::new(&[UIDENT, LIDENT, L_PAREN, L_BRACE]);

pub(super) fn at_type_atom_start(p: &Parser) -> bool {
    p.at_ts(TYPE_ATOM_START)
}

/// type ::= btype ('->' row? type)?
pub(super) fn type_(p: &mut Parser) -> bool {
    let m = p.start();
    if !btype(p) {
        m.abandon(p);
        p.error(
            codes::SYNTAX_ERROR,
            "expected a type",
            format!("found {}", describe(p)),
        );
        return false;
    }
    if p.at(THIN_ARROW) {
        p.bump(THIN_ARROW);
        arrow_result(p);
        m.complete(p, FN_TYPE);
    } else {
        m.abandon(p);
    }
    true
}

/// `->` の右側。`->` で行が終わると、レイアウト段がブロックを開くので、その中の1つの型として読む。
fn arrow_result(p: &mut Parser) {
    let in_block = p.eat(LAYOUT_OPEN);
    if in_block && p.eat(LAYOUT_CLOSE) {
        // レイアウト段の回復で作った空のブロック。E0009 は報告済み。
        return;
    }
    if at_angle(p, '<') {
        effect_row(p);
    }
    type_(p);
    if in_block {
        close_block(p);
    }
}

/// `type` の宣言の `=` の右側。`=` で行が終われば、ブロックの中の1つの型として読む。
pub(super) fn type_or_block(p: &mut Parser) {
    let in_block = p.eat(LAYOUT_OPEN);
    if in_block && p.eat(LAYOUT_CLOSE) {
        return;
    }
    type_(p);
    if in_block {
        close_block(p);
    }
}

/// btype ::= qcon type_atom* | type_atom
pub(super) fn btype(p: &mut Parser) -> bool {
    if !p.at(UIDENT) {
        return type_atom(p);
    }
    let m = p.start();
    qcon(p);
    let mut args = 0;
    while at_type_atom_start(p) {
        type_atom(p);
        args += 1;
    }
    m.complete(p, if args == 0 { PATH_TYPE } else { APP_TYPE });
    true
}

/// type_atom ::= qcon | LIDENT | '(' type ')' | '(' type (',' type)+ ')' | レコード (S2)
pub(super) fn type_atom(p: &mut Parser) -> bool {
    let m = p.start();
    let kind = match p.current() {
        UIDENT => {
            qcon(p);
            PATH_TYPE
        }
        LIDENT => {
            p.bump(LIDENT);
            VAR_TYPE
        }
        L_PAREN => {
            p.bump(L_PAREN);
            type_(p);
            let mut count = 1;
            while p.eat(COMMA) {
                type_(p);
                count += 1;
            }
            expect(p, R_PAREN);
            if count == 1 { PAREN_TYPE } else { TUPLE_TYPE }
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

/// row ::= '<' '>' | '<' LIDENT '>' | '<' effect (',' effect)* ('|' LIDENT)? '>'
fn effect_row(p: &mut Parser) {
    let m = p.start();
    eat_angle(p, '<', L_ANGLE);
    if !at_angle(p, '>') {
        if p.at(LIDENT) {
            p.bump(LIDENT);
        } else {
            effect(p);
            while p.eat(COMMA) {
                effect(p);
            }
            if p.eat(PIPE) {
                expect(p, LIDENT);
            }
        }
    }
    if !eat_angle(p, '>', R_ANGLE) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected `>`",
            format!("found {}", describe(p)),
        );
    }
    m.complete(p, EFFECT_ROW);
}

/// effect ::= qcon type_atom*
fn effect(p: &mut Parser) {
    if !p.at(UIDENT) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an effect",
            format!("found {}", describe(p)),
        );
        return;
    }
    let m = p.start();
    qcon(p);
    while at_type_atom_start(p) {
        type_atom(p);
    }
    m.complete(p, EFFECT);
}

/// 今のトークンが `c` で始まる演算子か。型の中では `<>` や `>->` を分けて読む (spec §5)。
fn at_angle(p: &Parser, c: char) -> bool {
    p.at(OP) && p.current_text().starts_with(c)
}

fn eat_angle(p: &mut Parser, c: char, kind: SyntaxKind) -> bool {
    if !at_angle(p, c) {
        return false;
    }
    if p.current_text().len() == 1 {
        p.bump_remap(kind);
    } else {
        p.split_first_char(kind);
    }
    true
}

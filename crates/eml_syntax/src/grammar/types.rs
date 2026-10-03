use super::*;

const TYPE_ATOM_START: TokenSet = TokenSet::new(&[UIDENT, LIDENT, L_PAREN, L_BRACE]);

pub(super) fn at_type_atom_start(p: &Parser) -> bool {
    p.at_ts(TYPE_ATOM_START)
}

/// 結果の型を始められるトークン。`OP` は row の `<`。
const RESULT_START: TokenSet = TokenSet::new(&[UIDENT, LIDENT, L_PAREN, L_BRACE, OP]);

pub(super) fn type_(p: &mut Parser) -> bool {
    type_in(p, false)
}

/// `in_type_block` は、型だけを入れたブロック (`->` や `=` で行が終わって開いたもの) の中にいるかどうか。
/// 揃えた複数行のシグネチャの回復で、`SEP` が型の続きかどうかを見分けるのに使う。
fn type_in(p: &mut Parser, in_type_block: bool) -> bool {
    let m = p.start();
    if !btype(p) {
        m.abandon(p);
        expected(p, "a type");
        return false;
    }
    if p.at(THIN_ARROW) {
        p.bump(THIN_ARROW);
        arrow_result(p, in_type_block);
        m.complete(p, FN_TYPE);
    } else {
        m.abandon(p);
    }
    true
}

/// `->` で行が終わるとレイアウト段がブロックを開くので、その中の1つの型として読む。
fn arrow_result(p: &mut Parser, in_type_block: bool) {
    let in_block = p.eat(LAYOUT_OPEN);
    if in_block && p.eat(LAYOUT_CLOSE) {
        // レイアウト段の回復で作った空のブロックで、E0009 は報告済み。型のブロックの中で次の行が同じ列に
        // 揃っていれば、行末の `->` で揃えたシグネチャなので、その行を結果の型として読み、診断を重ねない
        // (docs/spec/declarations.md の「シグネチャと等式」)。
        if in_type_block && p.at(LAYOUT_SEP) && RESULT_START.contains(p.nth(1)) {
            p.bump(LAYOUT_SEP);
            row_and_type(p, true);
        }
        return;
    }
    row_and_type(p, in_block || in_type_block);
    if in_block {
        close_block(p);
    }
}

fn row_and_type(p: &mut Parser, in_type_block: bool) {
    if at_angle(p, '<') {
        effect_row(p);
    }
    type_in(p, in_type_block);
}

/// `=` で行が終わるとレイアウト段がブロックを開くので、その中の1つの型として読む。
pub(super) fn type_or_block(p: &mut Parser) {
    let in_block = p.eat(LAYOUT_OPEN);
    if in_block && p.eat(LAYOUT_CLOSE) {
        return;
    }
    type_in(p, in_block);
    if in_block {
        close_block(p);
    }
}

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
            close_bracket(p, R_PAREN);
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
        expected(p, "`>`");
    }
    m.complete(p, EFFECT_ROW);
}

fn effect(p: &mut Parser) {
    if !p.at(UIDENT) {
        expected(p, "an effect");
        return;
    }
    let m = p.start();
    qcon(p);
    while at_type_atom_start(p) {
        type_atom(p);
    }
    m.complete(p, EFFECT);
}

/// 型の中では `<>` や `>->` を分けて読むので、演算子の先頭の文字だけを見る。
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

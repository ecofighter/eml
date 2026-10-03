//! 項目 (spec §5 の item / decl、§6)。

use super::*;

/// 項目を始めるキーワード。将来の予約語も、ここで受けてエラーにする。
const ITEM_KEYWORDS: TokenSet = TokenSet::new(&[
    DATA_KW,
    TYPE_KW,
    EFFECT_KW,
    INFIXL_KW,
    INFIXR_KW,
    INFIX_KW,
    IMPORT_KW,
    PUB_KW,
    FORALL_KW,
    CLASS_KW,
    INSTANCE_KW,
]);

/// fixity の宣言に書ける演算子。
const DECLARABLE_OPERATORS: TokenSet = TokenSet::new(&[OP, CONOP, MINUS]);

pub(super) fn at_item_start(p: &Parser) -> bool {
    p.at_ts(ITEM_KEYWORDS) || (p.at(LIDENT) && p.nth(1) == COLON) || at_operator_signature(p)
}

/// `(OP) : type` の形のシグネチャか。
fn at_operator_signature(p: &Parser) -> bool {
    p.at(L_PAREN) && matches!(p.nth(1), OP | MINUS) && p.nth(2) == R_PAREN
}

/// item ::= 'pub'? decl | equation | import_item
pub(super) fn item(p: &mut Parser) {
    let m = p.start();
    if p.at(PUB_KW) {
        not_yet_supported(p, "`pub` is not supported yet");
        p.bump(PUB_KW);
    }
    match p.current() {
        DATA_KW => data_item(p, m),
        TYPE_KW => type_item(p, m),
        EFFECT_KW => effect_item(p, m),
        INFIXL_KW | INFIXR_KW | INFIX_KW => fixity_item(p, m),
        IMPORT_KW => import_item(p, m),
        FORALL_KW | CLASS_KW | INSTANCE_KW => reserved_item(p, m),
        LIDENT if p.nth(1) == COLON => signature(p, m),
        _ if at_operator_signature(p) => signature(p, m),
        _ => {
            // `pub` の後ろに項目がない
            p.error(
                codes::EXPECTED_ITEM,
                "expected an item",
                "not the start of an item",
            );
            skip_to_sep(p, false);
            m.complete(p, ERROR);
        }
    }
}

/// signature ::= var ':' type 、var ::= LIDENT | '(' OP ')'
fn signature(p: &mut Parser, m: Marker) {
    if p.at(LIDENT) {
        p.bump(LIDENT);
    } else {
        p.bump(L_PAREN);
        p.bump_any();
        p.bump(R_PAREN);
    }
    if expect(p, COLON) {
        types::type_(p);
    }
    m.complete(p, SIGNATURE);
}

/// data_item ::= 'data' UIDENT LIDENT* '=' alts
fn data_item(p: &mut Parser, m: Marker) {
    p.bump(DATA_KW);
    expect(p, UIDENT);
    while p.at(LIDENT) {
        p.bump(LIDENT);
    }
    if expect(p, EQ) {
        alts(p);
    }
    m.complete(p, DATA_ITEM);
}

/// alts ::= block(alt) | alt+
fn alts(p: &mut Parser) {
    if p.at(LAYOUT_OPEN) {
        block_of(p, "a constructor starting with `|`", alt);
        return;
    }
    let mut first = true;
    // 最初の選択肢だけは、`|` を書き忘れた形 (`= A | B`) も受けて診断する。
    while p.at(PIPE) || (first && p.at(UIDENT)) {
        alt(p);
        first = false;
    }
    if first {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a constructor starting with `|`",
            format!("found {}", describe(p)),
        );
    }
}

/// alt ::= '|' UIDENT type_atom* | '|' btype CONOP btype
fn alt(p: &mut Parser) -> bool {
    if !p.at(PIPE) && !types::at_type_atom_start(p) {
        return false;
    }
    let m = p.start();
    if !p.eat(PIPE) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected `|` before the constructor",
            "each constructor starts with `|`",
        );
    }
    if p.at(UIDENT) && p.nth(1) != CONOP {
        p.bump(UIDENT);
        while types::at_type_atom_start(p) {
            types::type_atom(p);
        }
    } else {
        if !types::btype(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a constructor",
                format!("found {}", describe(p)),
            );
        }
        if p.eat(CONOP) {
            if !types::btype(p) {
                p.error(
                    codes::SYNTAX_ERROR,
                    "expected a type",
                    format!("found {}", describe(p)),
                );
            }
        } else {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a constructor name or an infix constructor",
                format!("found {}", describe(p)),
            );
        }
    }
    m.complete(p, ALT);
    true
}

/// type_item ::= 'type' UIDENT LIDENT* '=' type 。S2 で実装するので、読んだうえで E0004 を出す。
fn type_item(p: &mut Parser, m: Marker) {
    not_yet_supported(p, "`type` declarations are not supported yet");
    p.bump(TYPE_KW);
    expect(p, UIDENT);
    while p.at(LIDENT) {
        p.bump(LIDENT);
    }
    if expect(p, EQ) {
        types::type_or_block(p);
    }
    m.complete(p, TYPE_ITEM);
}

/// effect_item ::= 'effect' UIDENT LIDENT* 'where' block(op_decl)
fn effect_item(p: &mut Parser, m: Marker) {
    p.bump(EFFECT_KW);
    expect(p, UIDENT);
    while p.at(LIDENT) {
        p.bump(LIDENT);
    }
    if expect(p, WHERE_KW) {
        if p.at(LAYOUT_OPEN) {
            block_of(p, "an operation signature", op_decl);
        } else {
            p.error(
                codes::SYNTAX_ERROR,
                "expected the operations on indented lines after `where`",
                format!("found {}", describe(p)),
            );
        }
    }
    m.complete(p, EFFECT_ITEM);
}

/// op_decl ::= ('never' | 'once' | 'multi')? LIDENT ':' type
fn op_decl(p: &mut Parser) -> bool {
    if !matches!(p.current(), NEVER_KW | ONCE_KW | MULTI_KW | LIDENT) {
        return false;
    }
    let m = p.start();
    if !p.at(LIDENT) {
        p.bump_any();
    }
    expect(p, LIDENT);
    if expect(p, COLON) {
        types::type_(p);
    }
    m.complete(p, OP_DECL);
    true
}

/// fixity_item ::= ('infixl' | 'infixr' | 'infix') INT OP (',' OP)*
fn fixity_item(p: &mut Parser, m: Marker) {
    p.bump_any();
    if p.at(INT) {
        let text = p.current_text();
        if !(text.len() == 1 && text.as_bytes()[0].is_ascii_digit()) {
            p.error(
                codes::SYNTAX_ERROR,
                "precedence must be an integer from 0 to 9",
                "out of range",
            );
        }
        p.bump(INT);
    } else {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a precedence from 0 to 9",
            format!("found {}", describe(p)),
        );
    }
    loop {
        if p.at_ts(DECLARABLE_OPERATORS) {
            p.bump_any();
        } else {
            p.error(
                codes::SYNTAX_ERROR,
                "expected an operator",
                format!("found {}", describe(p)),
            );
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    m.complete(p, FIXITY_ITEM);
}

/// import は S2 で実装する。E0004 を出し、次の項目まで読み飛ばす。
fn import_item(p: &mut Parser, m: Marker) {
    not_yet_supported(p, "`import` is not supported yet");
    skip_to_sep(p, false);
    m.complete(p, ERROR);
}

fn reserved_item(p: &mut Parser, m: Marker) {
    p.error(
        codes::SYNTAX_ERROR,
        format!("`{}` is reserved for future use", p.current_text()),
        "not usable yet",
    );
    skip_to_sep(p, false);
    m.complete(p, ERROR);
}

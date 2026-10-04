use super::*;

const DECLARABLE_OPERATORS: TokenSet = TokenSet::new(&[OP, CONOP, MINUS]);

#[derive(Debug, Clone, Copy)]
enum ItemKind {
    Data,
    Type,
    Effect,
    Fixity,
    Import,
    /// 将来の予約語。項目としてエラーにし、次の項目から回復する。
    Reserved,
    Signature,
    Equation,
    OperatorEquation,
}

/// 今の位置から始まる項目の種類。`pub` は項目の前置きなので、ここでは見ない。`at_item_start` と `item` が同じ判定を
/// 使うため、項目の種類を足すときはここだけを直す。
fn item_kind(p: &Parser) -> Option<ItemKind> {
    Some(match p.current() {
        DATA_KW => ItemKind::Data,
        TYPE_KW => ItemKind::Type,
        EFFECT_KW => ItemKind::Effect,
        INFIXL_KW | INFIXR_KW | INFIX_KW => ItemKind::Fixity,
        IMPORT_KW => ItemKind::Import,
        FORALL_KW | CLASS_KW | INSTANCE_KW => ItemKind::Reserved,
        LIDENT if p.nth(1) == COLON => ItemKind::Signature,
        _ if at_operator_signature(p) => ItemKind::Signature,
        _ if at_equation(p) => ItemKind::Equation,
        _ if at_operator_equation(p) => ItemKind::OperatorEquation,
        _ => return None,
    })
}

pub(super) fn at_item_start(p: &Parser) -> bool {
    p.at(PUB_KW) || item_kind(p).is_some()
}

/// `LIDENT` の直後が `-` なら、演算子の定義 (`a - b = ...`) とみなす。関数の定義とは2トークンの先読みで区別する。
fn at_equation(p: &Parser) -> bool {
    p.at(LIDENT) && (p.nth(1) == EQ || (p.nth(1) != MINUS && patterns::at_apat_start_at(p, 1)))
}

/// apat の直後が `=` や `::` でも受けるのは、トップレベルのパターンによる束縛をここで診断するため。
fn at_operator_equation(p: &Parser) -> bool {
    patterns::apat_len(p).is_some_and(|len| matches!(p.nth(len), OP | MINUS | CONOP | EQ))
}

fn at_operator_signature(p: &Parser) -> bool {
    p.at(L_PAREN) && matches!(p.nth(1), OP | MINUS) && p.nth(2) == R_PAREN
}

pub(super) fn item(p: &mut Parser) {
    let m = p.start();
    if p.at(PUB_KW) {
        not_yet_supported(p, "`pub` is not supported yet");
        p.bump(PUB_KW);
    }
    match item_kind(p) {
        Some(ItemKind::Data) => data_item(p, m),
        Some(ItemKind::Type) => type_item(p, m),
        Some(ItemKind::Effect) => effect_item(p, m),
        Some(ItemKind::Fixity) => fixity_item(p, m),
        Some(ItemKind::Import) => import_item(p, m),
        Some(ItemKind::Reserved) => reserved_item(p, m),
        Some(ItemKind::Signature) => signature(p, m),
        Some(ItemKind::Equation) => equation(p, m),
        Some(ItemKind::OperatorEquation) => operator_equation(p, m),
        None => {
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
        expected(p, "a constructor starting with `|`");
    }
}

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
    if p.at(UIDENT) && !has_conop_ahead(p) {
        p.bump(UIDENT);
        while types::at_type_atom_start(p) {
            types::type_atom(p);
        }
    } else {
        if !types::btype(p) {
            expected(p, "a constructor");
        }
        if p.eat(CONOP) {
            if !types::btype(p) {
                expected(p, "a type");
            }
        } else {
            expected(p, "a constructor name or an infix constructor");
        }
    }
    m.complete(p, ALT);
    true
}

/// `| List a :: L a` のように左側が型の適用でも中置のコンストラクタとして読むため、選択肢の終わりまでに
/// 括弧の外の `:` 演算子があるかを先読みする (docs/spec/grammar.md の `alt`)。
fn has_conop_ahead(p: &Parser) -> bool {
    let mut nesting = Nesting::default();
    let mut n = 0;
    loop {
        let kind = p.peek(n);
        if nesting.ends(kind) {
            return false;
        }
        // 括弧の外の `|` とブロックの始まりは、今の選択肢の外である。
        if nesting.at_top() {
            match kind {
                CONOP => return true,
                PIPE | LAYOUT_OPEN | SEMICOLON => return false,
                _ => {}
            }
        }
        nesting.step(kind);
        n += 1;
    }
}

/// S2 で実装する。今は E0004 を出したうえで、宣言として最後まで読む。
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
            expected(p, "the operations on indented lines after `where`");
        }
    }
    m.complete(p, EFFECT_ITEM);
}

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
        expected(p, "a precedence from 0 to 9");
    }
    loop {
        if p.at_ts(DECLARABLE_OPERATORS) {
            p.bump_any();
        } else {
            expected(p, "an operator");
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    m.complete(p, FIXITY_ITEM);
}

/// S2 で実装する。今は E0004 を出して次の項目まで読み飛ばす。
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

fn equation(p: &mut Parser, m: Marker) {
    p.bump(LIDENT);
    while patterns::at_apat_start(p) {
        patterns::apat(p);
    }
    if expect(p, EQ) {
        expressions::body(p);
    }
    m.complete(p, EQUATION);
}

/// トップレベルのパターンによる束縛 (`(a, b) = ...`) もここに来るので、エラーにする (docs/spec/grammar.md)。
fn operator_equation(p: &mut Parser, m: Marker) {
    patterns::apat(p);
    if p.at(OP) || p.at(MINUS) {
        p.bump_any();
        if !patterns::apat(p) {
            expected(p, "a pattern");
        }
        if expect(p, EQ) {
            expressions::body(p);
        }
        m.complete(p, EQUATION);
    } else {
        p.error(
            codes::SYNTAX_ERROR,
            "top-level pattern bindings are not allowed",
            "define a value by name instead, as in `x = ...`",
        );
        skip_to_sep(p, false);
        m.complete(p, ERROR);
    }
}

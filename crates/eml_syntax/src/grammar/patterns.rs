use super::*;

pub(super) fn at_apat_start(p: &Parser) -> bool {
    at_apat_start_at(p, 0)
}

/// `-` は、整数が続くときだけ負の数のリテラルとして apat を始める。
pub(super) fn at_apat_start_at(p: &Parser, n: usize) -> bool {
    match p.nth(n) {
        UNDERSCORE | LIDENT | UIDENT | INT | STRING_START | CHAR => true,
        kind if kind.is_opening_bracket() => true,
        MINUS => p.nth(n + 1) == INT,
        _ => false,
    }
}

/// 項目の種類を決める先読みに使うので、括弧は対応する閉じ括弧までを数える。
pub(super) fn apat_len(p: &Parser) -> Option<usize> {
    match p.current() {
        UNDERSCORE | LIDENT | INT | CHAR => Some(1),
        MINUS if p.nth(1) == INT => Some(2),
        UIDENT => {
            let mut n = 1;
            while p.nth(n) == DOT && p.nth(n + 1) == UIDENT {
                n += 2;
            }
            Some(n)
        }
        STRING_START => {
            // 同じ深さの `STRING_END` までを数える。穴の中の入れ子の文字列は、穴の対で深さを数えて飛ばす。
            // `STRING_END` がなければ (閉じていない文字列)、文字列の最後のトークンまでを長さにする
            let mut n = 1;
            let mut holes = 0u32;
            loop {
                match p.peek(n) {
                    INTERP_START => holes += 1,
                    INTERP_END => holes = holes.saturating_sub(1),
                    STRING_END if holes == 0 => return Some(n + 1),
                    STRING_TEXT | ESCAPE | STRING_START | STRING_END => {}
                    // lexer は穴の閉じを必ず出すので、ここには来ない。来ても先読みを止める
                    EOF => return Some(n),
                    _ if holes > 0 => {}
                    _ => return Some(n),
                }
                n += 1;
            }
        }
        kind if kind.is_opening_bracket() => {
            // 開き括弧の次から、対応する閉じ括弧を探す。範囲が先に終わったら (規則 2 で括弧が暗黙に閉じられた
            // など)、パターンは閉じていない。
            let mut nesting = Nesting::default();
            nesting.step(kind);
            let mut n = 1;
            loop {
                let kind = p.peek(n);
                if nesting.ends(kind) {
                    return None;
                }
                nesting.step(kind);
                if nesting.at_top() {
                    return Some(n + 1);
                }
                n += 1;
            }
        }
        _ => None,
    }
}

pub(super) fn pattern(p: &mut Parser) -> bool {
    nested(p, true, pattern_inner)
}

fn pattern_inner(p: &mut Parser) -> bool {
    infix_con_pat(p, 0)
}

/// 中置のコンストラクタの列は右へ再帰して読むが、HIR は fixity で組み直すので、先に読んだ `cpat` が後の段の下に
/// 来うる。そこで右の段は、それまでの `cpat` の高さの最大 (`tallest`) を下に確保して数える。数えないと、括弧に
/// 入れた列を左に重ねたパターンを後の段階が再帰してスタックを溢れさせる (docs/spec/grammar.md)。
fn infix_con_pat(p: &mut Parser, tallest: u32) -> bool {
    let m = p.start();
    let (parsed, height) = p.measure(cpat);
    if !parsed {
        m.abandon(p);
        return false;
    }
    if p.at(CONOP) {
        p.bump(CONOP);
        let tallest = tallest.max(height);
        if !p.enter_reserving(tallest) {
            too_deep(p);
        } else {
            let parsed = infix_con_pat(p, tallest);
            p.leave();
            if !parsed {
                expected(p, "a pattern");
            }
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

/// `(pat : type)` の型の明示を含む (docs/spec/grammar.md の `apat`)。
pub(super) fn apat(p: &mut Parser) -> bool {
    let m = p.start();
    let kind = match p.current() {
        UNDERSCORE => {
            p.bump_any();
            WILDCARD_PAT
        }
        LIDENT => {
            name(p);
            BIND_PAT
        }
        UIDENT => {
            qcon(p);
            CON_PAT
        }
        INT | CHAR => {
            p.bump_any();
            LITERAL_PAT
        }
        STRING_START => {
            let string = p.start();
            expressions::string_lit(p, true);
            string.complete(p, STRING_LIT);
            LITERAL_PAT
        }
        MINUS if p.nth(1) == INT => {
            p.bump(MINUS);
            p.bump(INT);
            LITERAL_PAT
        }
        L_PAREN => paren_pat(p),
        L_BRACK => list_pat(p),
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

fn paren_pat(p: &mut Parser) -> SyntaxKind {
    p.bump(L_PAREN);
    if p.eat(R_PAREN) {
        return UNIT_PAT;
    }
    if !pattern(p) {
        expected(p, "a pattern");
    }
    let kind = if p.eat(COLON) {
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

/// HIR はパターンのリストを右に入れ子の `::` に組むので、k 番目の要素を k - 1 段深く読む。組んだ木では前の要素が
/// いつも浅い位置に来るので、中置のコンストラクタのパターンのように前の高さを確保しなくてよい
/// (docs/spec/grammar.md の「文法上の補足」)。
fn list_pat(p: &mut Parser) -> SyntaxKind {
    p.bump(L_BRACK);
    let mut entered = 0;
    if !p.at(R_BRACK) {
        loop {
            if !pattern(p) {
                expected(p, "a pattern");
                break;
            }
            if !p.eat(COMMA) || p.at(R_BRACK) {
                break;
            }
            if !p.enter() {
                too_deep(p);
                break;
            }
            entered += 1;
        }
    }
    for _ in 0..entered {
        p.leave();
    }
    close_bracket(p, R_BRACK);
    LIST_PAT
}

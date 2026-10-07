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

/// 今の位置から始まる項目の種類。`pub` と `extern` は項目の前置きなので、ここでは見ない。`at_item_start` と `item` が同じ判定を
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
    p.at(PUB_KW) || p.at(EXTERN_KW) || item_kind(p).is_some()
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

/// 項目を1つ読み、import 以外の項目だったかを返す。`declared` は、ファイルの中でこれより前に import 以外の項目があったか。
pub(super) fn item(p: &mut Parser, declared: bool) -> bool {
    let m = p.start();
    let public = p.eat(PUB_KW);
    let external = p.eat(EXTERN_KW);
    // `extern` は `pub` の後ろにだけ書く。誤りを1つ出したら `pub` を読み捨て、項目は今までどおり読む
    if external && p.at(PUB_KW) {
        p.error(
            codes::SYNTAX_ERROR,
            "`extern` cannot be written before `pub`",
            "write `pub extern`",
        );
        p.bump(PUB_KW);
    }
    let kind = item_kind(p);
    // `pub` は宣言だけに付く (docs/spec/grammar.md の `item`)。等式の関数はシグネチャで公開する
    if public {
        match kind {
            Some(ItemKind::Equation | ItemKind::OperatorEquation) => p.error_at_previous(
                codes::SYNTAX_ERROR,
                "`pub` cannot be written on an equation",
                "write `pub` on the signature instead",
            ),
            Some(ItemKind::Import) => p.error_at_previous(
                codes::SYNTAX_ERROR,
                "`pub` cannot be written on an import",
                "only declarations can be public",
            ),
            _ => {}
        }
    }
    // `extern` は、等式のないシグネチャ、`=` のない `data`、`where` のない `effect` にだけ付く
    // (docs/spec/grammar.md の `item`)
    if external
        && kind.is_some_and(|kind| {
            !matches!(
                kind,
                ItemKind::Signature | ItemKind::Data | ItemKind::Effect
            )
        })
    {
        p.error_at_previous(
            codes::SYNTAX_ERROR,
            "`extern` cannot be written on this item",
            "`extern` goes on a signature, a `data` without `=`, or an `effect` without `where`",
        );
    }
    match kind {
        Some(ItemKind::Data) if external => extern_data(p, m),
        Some(ItemKind::Effect) if external => extern_effect(p, m),
        Some(ItemKind::Data) => data_item(p, m),
        Some(ItemKind::Type) => type_item(p, m),
        Some(ItemKind::Effect) => effect_item(p, m),
        Some(ItemKind::Fixity) => fixity_item(p, m),
        Some(ItemKind::Import) => import_item(p, m, declared),
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
    // 項目のない `pub` は宣言ではないので、後ろの import を E0011 にしない
    kind.is_some_and(|kind| !matches!(kind, ItemKind::Import))
}

/// signature ::= var ':' type 、var ::= LIDENT | '(' OP ')'
fn signature(p: &mut Parser, m: Marker) {
    if p.at(LIDENT) {
        name(p);
    } else {
        let n = p.start();
        p.bump(L_PAREN);
        p.bump_any();
        p.bump(R_PAREN);
        n.complete(p, NAME);
    }
    if expect(p, COLON) {
        types::type_(p);
    }
    m.complete(p, SIGNATURE);
}

/// data_item ::= 'data' UIDENT LIDENT* ('=' alts)?
/// `=` のない `data` は、`extern data` なら extern の型で、そうでなければどのモジュールでも HIR が E1025 にする
/// (docs/spec/declarations.md の「`data` と `type`」)。
fn data_item(p: &mut Parser, m: Marker) {
    p.bump(DATA_KW);
    expect_name(p, UIDENT);
    while p.at(LIDENT) {
        name(p);
    }
    if p.eat(EQ) {
        alts(p);
    } else if p.at(PIPE) {
        // `=` の書き忘れ。選択肢は読み、コンストラクタを使う位置に誤りを連鎖させない
        expected(p, "`=`");
        alts(p);
    }
    m.complete(p, DATA_ITEM);
}

/// extern_decl ::= 'data' UIDENT。型引数、`=`、`where` は E0011 にして項目の終わりまで読み飛ばす
/// (docs/spec/grammar.md の `item`)。
fn extern_data(p: &mut Parser, m: Marker) {
    p.bump(DATA_KW);
    expect_name(p, UIDENT);
    reject_extern_tail(p, "data");
    m.complete(p, DATA_ITEM);
}

fn extern_effect(p: &mut Parser, m: Marker) {
    p.bump(EFFECT_KW);
    expect_name(p, UIDENT);
    reject_extern_tail(p, "effect");
    m.complete(p, EFFECT_ITEM);
}

fn reject_extern_tail(p: &mut Parser, keyword: &str) {
    if p.at(LIDENT) || p.at(EQ) || p.at(PIPE) || p.at(WHERE_KW) {
        p.error(
            codes::SYNTAX_ERROR,
            format!("`extern {keyword}` cannot have parameters, `=` or `where`"),
            "an extern declaration is only the name",
        );
        skip_to_sep(p, false);
    }
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
        name(p);
        while types::at_type_atom_start(p) {
            types::type_atom(p);
        }
    } else {
        if !types::btype(p) {
            expected(p, "a constructor");
        }
        if p.at(CONOP) {
            name(p);
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

/// S4 で実装する。CST まで組み、E0004 は HIR が出す (docs/implementation/status.md の「未対応の構文と E0004」)。
fn type_item(p: &mut Parser, m: Marker) {
    p.bump(TYPE_KW);
    expect_name(p, UIDENT);
    while p.at(LIDENT) {
        name(p);
    }
    if expect(p, EQ) {
        types::type_or_block(p);
    }
    m.complete(p, TYPE_ITEM);
}

fn effect_item(p: &mut Parser, m: Marker) {
    p.bump(EFFECT_KW);
    expect_name(p, UIDENT);
    while p.at(LIDENT) {
        name(p);
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
    expect_name(p, LIDENT);
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
            name(p);
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

/// import_item ::= 'import' modpath ('as' UIDENT)? ('(' list(import_name) ')')?
/// import は宣言より前に書く (docs/spec/grammar.md)。宣言の後の import も、後の段階が回復できるよう同じ形の CST に組む
/// (docs/implementation/architecture.md の「名前解決の回復」)。
fn import_item(p: &mut Parser, m: Marker, declared: bool) {
    if declared {
        p.error(
            codes::SYNTAX_ERROR,
            "imports must come before declarations",
            "move this import above the first declaration",
        );
    }
    p.bump(IMPORT_KW);
    if p.at(UIDENT) {
        qcon(p);
    } else {
        expected(p, "a module name");
    }
    if p.eat(AS_KW) {
        expect_name(p, UIDENT);
    }
    if p.at(L_PAREN) {
        import_list(p);
    }
    // `import Report.csv` の `.csv` のような読み残しを IMPORT_ITEM の中に置く。読めたパスだけでファイルを読むと、
    // 書いたつもりと違うモジュールを黙って取り込むので、`item_tree` がこの import を壊れたものとして扱う
    end_of_item(p);
    m.complete(p, IMPORT_ITEM);
}

fn import_list(p: &mut Parser) {
    let m = p.start();
    p.bump(L_PAREN);
    // 閉じ括弧がないまま行が終わったときは、`close_bracket` に1件だけ報告させる (末尾の `,` は許す)
    while !p.at(R_PAREN) && !p.current().is_virtual() && !p.at_eof() {
        if !import_name(p) {
            expected(p, "a name to import");
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    close_bracket(p, R_PAREN);
    m.complete(p, IMPORT_LIST);
}

/// import_name ::= LIDENT | '(' OP ')' | UIDENT ('(' '..' ')')?
/// `(CONOP)` は文法の外だが、CST は `(OP)` と同じ形に組んで E0011 にする。中置のコンストラクタは型と一緒に `T(..)` で
/// 取り込む (docs/spec/modules.md)。
fn import_name(p: &mut Parser) -> bool {
    let m = p.start();
    match p.current() {
        LIDENT => name_ref(p),
        UIDENT => {
            name_ref(p);
            if p.at(L_PAREN) && p.nth(1) == DOT2 && p.nth(2) == R_PAREN {
                p.bump(L_PAREN);
                p.bump(DOT2);
                p.bump(R_PAREN);
            }
        }
        L_PAREN if matches!(p.nth(1), OP | MINUS | CONOP) && p.nth(2) == R_PAREN => {
            p.bump(L_PAREN);
            if p.at(CONOP) {
                p.error(
                    codes::SYNTAX_ERROR,
                    "an infix constructor cannot be listed in an import",
                    "import it together with its type, as in `T(..)`",
                );
            }
            p.bump_any();
            p.bump(R_PAREN);
        }
        _ => {
            m.abandon(p);
            return false;
        }
    }
    m.complete(p, IMPORT_NAME);
    true
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
    name(p);
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
        name(p);
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

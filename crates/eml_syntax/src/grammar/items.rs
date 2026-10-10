use super::*;

const DECLARABLE_OPERATORS: TokenSet = TokenSet::new(&[OP, CONOP, MINUS]);

#[derive(Debug, Clone, Copy)]
enum ItemKind {
    Data,
    Type,
    Effect,
    Fixity,
    Import,
    Class,
    Instance,
    /// 将来の予約語。項目としてエラーにし、次の項目から回復する。
    Reserved,
    Signature,
    Equation,
    OperatorEquation,
    /// `=>` を演算子として定義しようとした等式やシグネチャ。E0011 にし、次の項目から回復する。
    ReservedOperator,
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
        CLASS_KW => ItemKind::Class,
        INSTANCE_KW => ItemKind::Instance,
        FORALL_KW => ItemKind::Reserved,
        LIDENT if p.nth(1) == COLON => ItemKind::Signature,
        _ if at_operator_signature(p) => ItemKind::Signature,
        _ if at_equation(p) => ItemKind::Equation,
        _ if at_operator_equation(p) => ItemKind::OperatorEquation,
        _ if at_reserved_operator(p) => ItemKind::ReservedOperator,
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

/// `(=>)` や `a => b` は、演算子のシグネチャや等式と同じ形で `=>` を使う。`=>` は予約記号なので
/// (docs/spec/lexical.md の「演算子」)、項目の始まりとして認めたうえで E0011 にする。
fn at_reserved_operator(p: &Parser) -> bool {
    (p.at(L_PAREN) && p.nth(1) == FAT_ARROW && p.nth(2) == R_PAREN)
        || patterns::apat_len(p).is_some_and(|len| p.nth(len) == FAT_ARROW)
}

/// 項目を1つ読み、import 以外の項目だったかを返す。`declared` は、ファイルの中でこれより前に import 以外の項目があったか。
pub(super) fn item(p: &mut Parser, declared: bool) -> bool {
    let m = p.start();
    let public = p.eat(PUB_KW);
    // 誤った `extern` は、`extern` のトークンに E0011 を1つだけ出す。`pub` の誤りも重ねない。`extern` を直した後に
    // 残る誤りは、そのとき報告すれば足りるため
    let misordered = p.at(EXTERN_KW) && p.nth(1) == PUB_KW;
    if misordered {
        p.error(
            codes::SYNTAX_ERROR,
            "`extern` cannot be written before `pub`",
            "write `pub extern`",
        );
    }
    let external = p.eat(EXTERN_KW);
    if misordered {
        p.bump(PUB_KW);
    }
    let kind = item_kind(p);
    // `pub` は宣言だけに付く (docs/spec/grammar.md の `item`)。等式の関数はシグネチャで公開する
    if public && !external {
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
            Some(ItemKind::Instance) => p.error_at_previous(
                codes::SYNTAX_ERROR,
                "`pub` cannot be written on an instance",
                "an instance is visible everywhere",
            ),
            _ => {}
        }
    }
    // `extern` は、等式のないシグネチャ、`=` のない `data`、`where` のない `effect` にだけ付く
    // (docs/spec/grammar.md の `item`)
    if external
        && !misordered
        && kind.is_some_and(|kind| {
            !matches!(
                kind,
                ItemKind::Signature
                    | ItemKind::Data
                    | ItemKind::Effect
                    | ItemKind::ReservedOperator
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
        Some(ItemKind::Class) => class_item(p, m),
        Some(ItemKind::Instance) => instance_item(p, m),
        Some(ItemKind::Reserved) => reserved_item(p, m),
        Some(ItemKind::Signature) => signature(p, m),
        Some(ItemKind::Equation) => equation(p, m),
        Some(ItemKind::OperatorEquation) => operator_equation(p, m),
        Some(ItemKind::ReservedOperator) => reserved_operator(p, m, false),
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
        if has_context_ahead(p) {
            context(p);
        }
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
    let mut clauses = 0;
    if p.eat(EQ) {
        alts(p, &mut clauses);
    } else if p.at(PIPE) {
        // `=` の書き忘れ。選択肢は読み、コンストラクタを使う位置に誤りを連鎖させない
        expected(p, "`=`");
        alts(p, &mut clauses);
    }
    while p.at(DERIVING_KW) {
        data_deriving(p, &mut clauses);
    }
    m.complete(p, DATA_ITEM);
}

/// extern_decl ::= 'data' UIDENT。型引数、`=`、`where` は E0011 にする。型引数は名前として読み、残りは項目の終わり
/// まで読み飛ばす (docs/spec/grammar.md の `item`)。
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

/// extern の `data` / `effect` に型引数、`=`、`where` は書けない (docs/spec/declarations.md の「`extern`」)。
/// 誤って書いた型引数も名前として読む。使う位置の型引数の数 (E1015) が書いたとおりになり、誤りが連鎖しない。
fn reject_extern_tail(p: &mut Parser, keyword: &str) {
    if p.at(LIDENT) || p.at(EQ) || p.at(PIPE) || p.at(WHERE_KW) {
        p.error(
            codes::SYNTAX_ERROR,
            format!("`extern {keyword}` cannot have parameters, `=` or `where`"),
            "an extern declaration is only the name",
        );
        while p.at(LIDENT) {
            name(p);
        }
        skip_to_sep(p, false);
    }
}

/// `clauses` は、この `data` でここまでに読んだ `deriving` の句の数。
fn alts(p: &mut Parser, clauses: &mut usize) {
    if p.at(LAYOUT_OPEN) {
        // `deriving` は、ブロックの最後の項目にも、最後の選択肢の続きの行にも書ける (docs/spec/grammar.md の「文法上の補足」)
        let mut derived = false;
        let mut constructors = false;
        block_of(p, "a constructor starting with `|`", |p| {
            if p.at(DERIVING_KW) {
                // `deriving` は選択肢に数えない。選択肢がないことは、後ろに `deriving` や選択肢が続いても1件だけ
                // 報告し、句の数の誤りを重ねない
                if constructors {
                    data_deriving(p, clauses);
                } else {
                    if *clauses == 0 {
                        expected(p, "a constructor starting with `|`");
                    }
                    *clauses += 1;
                    deriving(p);
                }
                derived = constructors;
                return true;
            }
            if derived && p.at(PIPE) {
                p.error(
                    codes::SYNTAX_ERROR,
                    "a constructor cannot follow `deriving`",
                    "move `deriving` after the last constructor",
                );
            }
            if !alt(p) {
                return false;
            }
            constructors = true;
            if p.at(DERIVING_KW) {
                data_deriving(p, clauses);
                derived = true;
            }
            true
        });
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
        if p.at(L_BRACE) {
            record_fields(p);
        } else {
            while types::at_type_atom_start(p) {
                types::type_atom(p);
            }
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

/// `'{' list(ftype) '}'`。フィールドの後に型の atom は続けない
/// (docs/spec/declarations.md の「`data` と `type`」)。
fn record_fields(p: &mut Parser) {
    let m = p.start();
    p.bump(L_BRACE);
    field_list(p, false, field_decl);
    m.complete(p, RECORD_FIELDS);
}

/// `ftype ::= LIDENT ':' type`
fn field_decl(p: &mut Parser) {
    let m = p.start();
    name(p);
    if expect(p, COLON) {
        types::type_(p);
    }
    m.complete(p, FIELD_DECL);
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

/// 型の別名はまだ実装していない。CST まで組み、E0004 は HIR が出す (docs/implementation/status.md の「未対応の構文と E0004」)。
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
        if has_context_ahead(p) {
            context(p);
        }
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

/// class_item ::= 'class' context? UIDENT LIDENT ('where' block(class_member))?
fn class_item(p: &mut Parser, m: Marker) {
    p.bump(CLASS_KW);
    if has_context_ahead(p) {
        context(p);
    }
    expect_name(p, UIDENT);
    expect_name(p, LIDENT);
    if p.eat(WHERE_KW) {
        if p.at(LAYOUT_OPEN) {
            block_of(p, "a method signature or a default equation", class_member);
        } else {
            expected(p, "the methods on indented lines after `where`");
        }
    }
    m.complete(p, CLASS_ITEM);
}

/// class_member ::= signature | equation。メソッドはクラスと一緒に公開するので、`pub` は書けない。
fn class_member(p: &mut Parser) -> bool {
    let m = p.start();
    if p.at(PUB_KW) {
        p.error(
            codes::SYNTAX_ERROR,
            "`pub` cannot be written on a class member",
            "the methods are public when the class is",
        );
        p.bump(PUB_KW);
    }
    match item_kind(p) {
        Some(ItemKind::Signature) => signature(p, m),
        Some(ItemKind::Equation) => equation(p, m),
        Some(ItemKind::OperatorEquation) => operator_equation(p, m),
        Some(ItemKind::ReservedOperator) => reserved_operator(p, m, true),
        _ => {
            m.abandon(p);
            return false;
        }
    }
    true
}

/// instance_item ::= 'instance' context? qUIDENT atype ('where' block(inst_member))?
/// 頭の形 (型コンストラクタに互いに異なる型変数を適用したもの) は HIR が検査する (E1039)。
fn instance_item(p: &mut Parser, m: Marker) {
    p.bump(INSTANCE_KW);
    if has_context_ahead(p) {
        context(p);
    }
    if p.at(UIDENT) {
        qcon(p);
    } else {
        expected(p, "a class name");
    }
    if !types::type_atom(p) {
        expected(p, "a type");
    }
    if p.eat(WHERE_KW) {
        if p.at(LAYOUT_OPEN) {
            block_of(p, "a method equation", instance_member);
        } else {
            expected(p, "the methods on indented lines after `where`");
        }
    }
    m.complete(p, INSTANCE_ITEM);
}

/// inst_member ::= equation | 'extern' var。シグネチャはクラスが決めるので書けないが、CST には組んで回復する。
fn instance_member(p: &mut Parser) -> bool {
    let m = p.start();
    // `pub` を読み飛ばしてメンバーを読み続ける。メンバーを落とすと、メソッドの等式がないことの E1036 が連鎖するため
    if p.at(PUB_KW) {
        p.error(
            codes::SYNTAX_ERROR,
            "`pub` cannot be written on an instance member",
            "an instance is visible everywhere",
        );
        p.bump(PUB_KW);
    }
    if p.eat(EXTERN_KW) {
        if p.at(LIDENT) {
            name(p);
        } else if at_operator_signature(p) {
            let n = p.start();
            p.bump(L_PAREN);
            p.bump_any();
            p.bump(R_PAREN);
            n.complete(p, NAME);
        } else {
            expected(p, "a method name");
        }
        m.complete(p, EXTERN_METHOD);
        return true;
    }
    match item_kind(p) {
        Some(ItemKind::Signature) => {
            p.error(
                codes::SYNTAX_ERROR,
                "an instance cannot have signatures",
                "the type of a method comes from its class",
            );
            signature(p, m);
        }
        Some(ItemKind::Equation) => equation(p, m),
        Some(ItemKind::OperatorEquation) => operator_equation(p, m),
        Some(ItemKind::ReservedOperator) => reserved_operator(p, m, true),
        _ => {
            m.abandon(p);
            return false;
        }
    }
    true
}

/// context ::= btype '=>'。`(Eq a, Show b)` は括弧の中を制約の並びとして読む。
fn context(p: &mut Parser) {
    let m = p.start();
    if p.at(L_PAREN) {
        p.bump(L_PAREN);
        constraint(p);
        while p.eat(COMMA) {
            constraint(p);
        }
        close_bracket(p, R_PAREN);
    } else {
        constraint(p);
    }
    expect(p, FAT_ARROW);
    m.complete(p, CONTEXT);
}

fn constraint(p: &mut Parser) {
    let m = p.start();
    if !types::btype(p) {
        expected(p, "a constraint");
    }
    m.complete(p, CONSTRAINT);
}

/// 文脈は型と同じ形で始まるので、括弧の外の `=>` が型の終わりより前にあるかを先読みする (中置のコンストラクタの
/// `has_conop_ahead` と同じ形)。括弧の外の `->` と `=` と `where` は、文脈の後ろにしか現れない。
fn has_context_ahead(p: &Parser) -> bool {
    let mut nesting = Nesting::default();
    let mut n = 0;
    loop {
        let kind = p.peek(n);
        if nesting.ends(kind) {
            return false;
        }
        if nesting.at_top() {
            match kind {
                FAT_ARROW => return true,
                THIN_ARROW | EQ | WHERE_KW | LAYOUT_OPEN | SEMICOLON => return false,
                _ => {}
            }
        }
        nesting.step(kind);
        n += 1;
    }
}

/// deriving ::= 'deriving' (qUIDENT | '(' qUIDENT (',' qUIDENT)* ')')
fn deriving(p: &mut Parser) {
    let m = p.start();
    p.bump(DERIVING_KW);
    if p.at(L_PAREN) {
        p.bump(L_PAREN);
        loop {
            if !p.at(UIDENT) {
                expected(p, "a class name");
                break;
            }
            qcon(p);
            if !p.eat(COMMA) {
                break;
            }
        }
        close_bracket(p, R_PAREN);
    } else if p.at(UIDENT) {
        qcon(p);
    } else {
        expected(p, "a class name");
    }
    m.complete(p, DERIVING);
}

/// `data` の `deriving` の句。句は1つだけ書ける (docs/spec/grammar.md の「文法上の補足」)。2つ目からも E0011 を出したうえで
/// クラスを読む。HIR はすべての句のクラスを導出するので、使う位置に E2006 を連鎖させない。
fn data_deriving(p: &mut Parser, clauses: &mut usize) {
    if *clauses > 0 {
        p.error(
            codes::SYNTAX_ERROR,
            "a data declaration has one `deriving` clause",
            "list every class in one `deriving (…)`",
        );
    }
    *clauses += 1;
    deriving(p);
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

fn reserved_operator(p: &mut Parser, m: Marker, in_block: bool) {
    if !p.eat(L_PAREN) {
        patterns::apat(p);
    }
    p.error(
        codes::SYNTAX_ERROR,
        "`=>` is reserved and cannot be defined as an operator",
        "`=>` only ends a context, as in `Eq a => a`",
    );
    skip_to_sep(p, in_block);
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

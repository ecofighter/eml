//! 文法は docs/spec/grammar.md。関数の名前は、おおむね文法の規則の名前に合わせてある。
//! 構文の差し替えをこのモジュールの中で済ませるため、文法の規則はここ以外に置かない。
//!
//! エラーからの回復は、同じ深さの `SEP` (ブロックの中なら `CLOSE` も) まで読み飛ばすのを基本にする。
//! レイアウトの区切りが、行の構造に沿った確かな同期点になるため
//! (docs/implementation/architecture.md の「構文解析の回復」)。

mod expressions;
mod items;
mod patterns;
mod scan;
mod types;

use crate::SyntaxKind::{self, *};
use crate::codes;
use crate::parser::{Marker, NESTING_LIMIT, Parser};
use crate::token_set::TokenSet;
use eml_diagnostics::{NOT_YET_SUPPORTED, NOT_YET_SUPPORTED_LABEL};
use scan::Nesting;

pub(crate) fn source_file(p: &mut Parser) {
    let m = p.start();
    let mut declared = false;
    loop {
        while p.at_sep() {
            p.bump_any();
        }
        if p.at_eof() {
            break;
        }
        if p.at(LAYOUT_CLOSE) {
            // 項目の中で読み残したブロックの終わり。次の項目の解析に持ち越さないため、読み捨てる。
            p.bump_any();
            continue;
        }
        p.set_too_deep(false);
        if items::at_item_start(p) {
            declared |= items::item(p, declared);
        } else {
            stray_tokens(p);
        }
        end_of_item(p);
    }
    m.complete(p, SOURCE_FILE);
}

/// 項目の後ろに読み残したトークンを、1件の診断とともに `ERROR` にする。
fn end_of_item(p: &mut Parser) {
    if !p.at_sep() && !p.at_eof() && !p.at(LAYOUT_CLOSE) {
        p.error(
            codes::SYNTAX_ERROR,
            unexpected(p),
            "expected the end of the item",
        );
        let m = p.start();
        skip_to_sep(p, false);
        m.complete(p, ERROR);
    }
}

/// 入れ子の深さを数えて `parse` を呼ぶ。上限を超えたら `too_deep` で読み飛ばし、`on_too_deep` を返す。
fn nested<T>(p: &mut Parser, on_too_deep: T, parse: impl FnOnce(&mut Parser) -> T) -> T {
    if !p.enter() {
        too_deep(p);
        return on_too_deep;
    }
    let result = parse(p);
    p.leave();
    result
}

/// E0013 は項目ごとに1回だけ出し、今の括弧かブロックの中身を読み飛ばす。その後の連鎖する診断は、項目の終わりまで
/// 出さない (`source_file` で戻す)。読み飛ばしは `skip_to_closing` に任せる。括弧とブロックの深さを別々に数えるので、
/// レイアウト段が入れ子の括弧を暗黙に閉じた場合でも、ファイルの残りを飲み込まない。
fn too_deep(p: &mut Parser) {
    if !p.is_too_deep() {
        p.error(
            codes::NESTING_TOO_DEEP,
            "nesting is too deep",
            format!("the parser stops at {NESTING_LIMIT} levels of nesting"),
        );
        p.set_too_deep(true);
    }
    let m = p.start();
    skip_to_closing(p);
    m.complete(p, TOO_DEEP);
}

/// 診断はトークンごとではなく1件だけ出す。`ERROR_TOKEN` は字句解析で報告済みなので、それ以外のトークンの位置に出す。
fn stray_tokens(p: &mut Parser) {
    let m = p.start();
    let mut reported = false;
    let mut nesting = Nesting::default();
    // ブロックの外の区切りまでを1つの `ERROR` にする。読み残したブロックの終わりは、ここでは読み捨てる。
    while !p.at_eof() && (nesting.in_block() || !p.at_sep()) {
        if !reported && !p.at(ERROR_TOKEN) && !p.current().is_virtual() {
            p.error(
                codes::EXPECTED_ITEM,
                "expected an item",
                "not the start of an item",
            );
            reported = true;
        }
        nesting.step(p.current());
        p.bump_any();
    }
    m.complete(p, ERROR);
}

/// ノードは作らないので、呼び出し側が `ERROR` で包む。`;` は `SEP` と同じに扱う (docs/spec/layout.md の規則 5)。
/// ブロックの中で呼んだときは、そのブロックの終わりでも止まる。
fn skip_to_sep(p: &mut Parser, in_block: bool) {
    let mut nesting = Nesting::default();
    while !p.at_eof() {
        if !nesting.in_block() && (p.at_sep() || (in_block && p.at(LAYOUT_CLOSE))) {
            break;
        }
        nesting.step(p.current());
        p.bump_any();
    }
}

/// `item` は、今の位置から項目を始められなければ何も読まずに偽を返す。
/// 空のブロックは、レイアウト段が E0009 を報告したうえで作ったものなので、黙って受け入れる。
fn block_of(p: &mut Parser, what: &str, mut item: impl FnMut(&mut Parser) -> bool) {
    p.bump(LAYOUT_OPEN);
    if p.eat(LAYOUT_CLOSE) {
        return;
    }
    let mut attempted = false;
    loop {
        while p.at_sep() {
            p.bump_any();
        }
        if p.at(LAYOUT_CLOSE) || p.at_eof() {
            break;
        }
        attempted = true;
        if !item(p) {
            expected(p, what);
        }
        if !p.at_sep() && !p.at(LAYOUT_CLOSE) && !p.at_eof() {
            p.error(
                codes::SYNTAX_ERROR,
                unexpected(p),
                "expected a new line or the end of the block",
            );
            let m = p.start();
            skip_to_sep(p, true);
            m.complete(p, ERROR);
        }
    }
    if !attempted {
        expected(p, what);
    }
    p.eat(LAYOUT_CLOSE);
}

/// 型の途中で改行したときなど、1つの要素だけを持つブロックの終わりで使う。
fn close_block(p: &mut Parser) {
    if !p.at(LAYOUT_CLOSE) && !p.at_eof() {
        p.error(
            codes::SYNTAX_ERROR,
            unexpected(p),
            "expected the end of the indented block",
        );
        let m = p.start();
        let mut nesting = Nesting::default();
        while !p.at_eof() && (nesting.in_block() || !p.at(LAYOUT_CLOSE)) {
            nesting.step(p.current());
            p.bump_any();
        }
        m.complete(p, ERROR);
    }
    p.eat(LAYOUT_CLOSE);
}

/// 「何が必要で、実際に何があったか」を示す構文エラー。回復の経路の多くが同じ形の診断を出すので、ここにまとめる。
fn expected(p: &mut Parser, what: &str) {
    p.error(
        codes::SYNTAX_ERROR,
        format!("expected {what}"),
        format!("found {}", describe(p)),
    );
}

fn expect(p: &mut Parser, kind: SyntaxKind) -> bool {
    if p.eat(kind) {
        return true;
    }
    expected(p, token_name(kind));
    false
}

fn token_name(kind: SyntaxKind) -> &'static str {
    match kind {
        EQ => "`=`",
        COLON => "`:`",
        COMMA => "`,`",
        PIPE => "`|`",
        THIN_ARROW => "`->`",
        LEFT_ARROW => "`<-`",
        R_PAREN => "`)`",
        WHERE_KW => "`where`",
        WITH_KW => "`with`",
        THEN_KW => "`then`",
        IN_KW => "`in`",
        UIDENT => "a capitalized name",
        LIDENT => "a lowercase name",
        INT => "an integer",
        _ => "a token",
    }
}

/// 仮想トークンと EOF にはテキストがないので、名詞句として読める言い方にする。
fn unexpected(p: &Parser) -> String {
    match p.current() {
        EOF => "unexpected end of file".to_string(),
        LAYOUT_SEP => "unexpected line break".to_string(),
        LAYOUT_OPEN => "unexpected indented block".to_string(),
        LAYOUT_CLOSE => "unexpected end of block".to_string(),
        _ => format!("unexpected `{}`", p.current_text()),
    }
}

fn describe(p: &Parser) -> String {
    match p.current() {
        EOF => "the end of the file".to_string(),
        LAYOUT_SEP => "a new line".to_string(),
        LAYOUT_OPEN => "an indented block".to_string(),
        LAYOUT_CLOSE => "the end of the block".to_string(),
        _ => format!("`{}`", p.current_text()),
    }
}

fn not_yet_supported(p: &mut Parser, message: &str) {
    p.error(NOT_YET_SUPPORTED, message, NOT_YET_SUPPORTED_LABEL);
}

/// 中身ごと読み飛ばすのは、S4 の構文の中で診断を連鎖させないため。ノードは作らないので、呼び出し側が `ERROR` で包む。
/// 閉じ括弧の種類を見ないのは、レイアウト段と同じ解釈にするため (docs/spec/layout.md の規則 4)。
fn unsupported_group(p: &mut Parser, message: &str) {
    not_yet_supported(p, message);
    p.bump_any();
    skip_to_closing(p);
    if p.current().is_closing_bracket() {
        p.bump_any();
    }
}

/// 括弧の中身を、対応する閉じ括弧の手前まで読み飛ばす (閉じ括弧は読まない)。範囲の終わりは `Nesting::ends` が決める。
fn skip_to_closing(p: &mut Parser) {
    let mut nesting = Nesting::default();
    while !nesting.ends(p.current()) {
        nesting.step(p.current());
        p.bump_any();
    }
}

/// レイアウト段は閉じ括弧の種類を見ずに一番内側の括弧を閉じるので (docs/spec/layout.md の規則 4)、parser も
/// 種類の違う閉じ括弧をこの括弧の終わりとして読み、解釈を揃える。
fn close_bracket(p: &mut Parser, kind: SyntaxKind) {
    if p.eat(kind) {
        return;
    }
    if p.at(SEMICOLON) {
        p.error(
            codes::SYNTAX_ERROR,
            "unexpected `;` inside brackets",
            "`;` separates statements only in a block",
        );
    } else {
        expected(p, token_name(kind));
    }
    if p.current().is_virtual() || p.at_eof() {
        // 規則 2 でレイアウト段が括弧を閉じたので、閉じ括弧はない。
        return;
    }
    if !p.current().is_closing_bracket() {
        // `;` や余計なトークンは、対応する閉じ括弧まで読み飛ばす。診断は上の1件だけにする。
        let m = p.start();
        skip_to_closing(p);
        m.complete(p, ERROR);
    }
    if p.current().is_closing_bracket() {
        p.bump_any();
    }
}

/// `qcon ::= (UIDENT '.')* UIDENT`
fn qcon(p: &mut Parser) {
    path(p, TokenSet::new(&[UIDENT]));
}

/// 修飾名を `PATH` にする。`last` は最後のセグメントになれるトークンで、`.` の後にそれが続く間だけ修飾として読む。
/// 修飾のセグメントは大文字の名前だけである。
fn path(p: &mut Parser, last: TokenSet) {
    let m = p.start();
    while p.at(UIDENT) && p.nth(1) == DOT && last.contains(p.nth(2)) {
        name_ref(p);
        dot(p);
    }
    name_ref(p);
    m.complete(p, PATH);
}

/// 今のトークンを1つ読んで `NAME` にする。呼び出し側が名前のトークンにいることを確かめる。
fn name(p: &mut Parser) {
    let m = p.start();
    p.bump_any();
    m.complete(p, NAME);
}

/// `kind` の名前があれば `NAME` にし、なければ `expect` と同じ診断を出す。
fn expect_name(p: &mut Parser, kind: SyntaxKind) -> bool {
    if p.at(kind) {
        name(p);
        return true;
    }
    expected(p, token_name(kind));
    false
}

/// 今のトークンを1つ読んで `NAME_REF` にする。呼び出し側が名前のトークンにいることを確かめる。
fn name_ref(p: &mut Parser) {
    let m = p.start();
    p.bump_any();
    m.complete(p, NAME_REF);
}

/// `.` の前後の空白を禁じるのは、修飾・フィールドアクセスと区別できるようにするため (docs/spec/grammar.md)。
fn dot(p: &mut Parser) {
    if !p.touches_prev() || !p.touches_next() {
        p.error(
            codes::SPACE_AROUND_DOT,
            "unexpected whitespace around `.`",
            "write `.` without spaces; compose functions with `>>`",
        );
    }
    p.bump(DOT);
}

//! 文法は docs/spec/grammar.md。関数の名前は、おおむね文法の規則の名前に合わせてある。
//! 構文の差し替えをこのモジュールの中で済ませるため、文法の規則はここ以外に置かない。
//!
//! エラーからの回復は、同じ深さの `SEP` (ブロックの中なら `CLOSE` も) まで読み飛ばすのを基本にする。
//! レイアウトの区切りが、行の構造に沿った確かな同期点になるため (docs/spec/layout.md の「エラー回復」)。

mod expressions;
mod items;
mod patterns;
mod types;

use crate::SyntaxKind::{self, *};
use crate::parser::{Marker, Parser};
use crate::token_set::TokenSet;
use crate::{NOT_YET_SUPPORTED_LABEL, codes};

pub(crate) fn source_file(p: &mut Parser) {
    let m = p.start();
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
        if items::at_item_start(p) {
            items::item(p);
        } else {
            stray_tokens(p);
        }
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
    m.complete(p, SOURCE_FILE);
}

/// 診断はトークンごとではなく1件だけ出す。`ERROR_TOKEN` は字句解析で報告済みなので、それ以外のトークンの位置に出す。
fn stray_tokens(p: &mut Parser) {
    let m = p.start();
    let mut reported = false;
    let mut depth = 0u32;
    while !p.at_eof() && (depth != 0 || !p.at_sep()) {
        if !reported && !p.at(ERROR_TOKEN) && !p.current().is_virtual() {
            p.error(
                codes::EXPECTED_ITEM,
                "expected an item",
                "not the start of an item",
            );
            reported = true;
        }
        match p.current() {
            LAYOUT_OPEN => depth += 1,
            LAYOUT_CLOSE => depth = depth.saturating_sub(1),
            _ => {}
        }
        p.bump_any();
    }
    m.complete(p, ERROR);
}

/// ノードは作らないので、呼び出し側が `ERROR` で包む。
fn skip_to_sep(p: &mut Parser, in_block: bool) {
    let mut depth = 0u32;
    while !p.at_eof() {
        match p.current() {
            LAYOUT_SEP | SEMICOLON if depth == 0 => break,
            LAYOUT_CLOSE if depth == 0 && in_block => break,
            LAYOUT_OPEN => depth += 1,
            LAYOUT_CLOSE => depth = depth.saturating_sub(1),
            _ => {}
        }
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
        let mut depth = 0u32;
        while !p.at_eof() && (depth != 0 || !p.at(LAYOUT_CLOSE)) {
            match p.current() {
                LAYOUT_OPEN => depth += 1,
                LAYOUT_CLOSE => depth -= 1,
                _ => {}
            }
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
    p.error(codes::NOT_YET_SUPPORTED, message, NOT_YET_SUPPORTED_LABEL);
}

/// 中身ごと読み飛ばすのは、S2 の構文の中で診断を連鎖させないため。ノードは作らないので、呼び出し側が `ERROR` で包む。
fn unsupported_group(p: &mut Parser, message: &str) {
    not_yet_supported(p, message);
    let open = p.current();
    let close = if open == L_BRACK { R_BRACK } else { R_BRACE };
    let mut depth = 0u32;
    while !p.at_eof() {
        let kind = p.current();
        p.bump_any();
        if kind == open {
            depth += 1;
        } else if kind == close {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
    }
}

fn qcon(p: &mut Parser) {
    while p.at(UIDENT) && p.nth(1) == DOT && p.nth(2) == UIDENT {
        p.bump(UIDENT);
        dot(p);
    }
    p.bump(UIDENT);
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

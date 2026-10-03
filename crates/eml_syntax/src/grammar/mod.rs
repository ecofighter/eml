//! 暫定構文の文法の規則 (spec §7)。構文の差し替えは原則としてこのモジュールの中で行う。
//!
//! 最初の段階では、ファイル全体の構造とエラー回復だけを実装する。項目 (`fn` / `type` / `effect`) の
//! 文法は後の段階で TDD で実装し、それまでは「まだ対応していない」という診断を出す。

use crate::SyntaxKind::*;
use crate::codes;
use crate::parser::Parser;
use crate::token_set::TokenSet;

/// トップレベルのエラー回復の同期点。
const ITEM_START: TokenSet = TokenSet::new(&[FN_KW, TYPE_KW, EFFECT_KW]);

pub(crate) fn source_file(p: &mut Parser) {
    let m = p.start();
    while !p.at_eof() {
        if p.at_ts(ITEM_START) {
            item(p);
        } else {
            stray_tokens(p);
        }
    }
    m.complete(p, SOURCE_FILE);
}

fn item(p: &mut Parser) {
    let keyword = match p.current() {
        FN_KW => "fn",
        TYPE_KW => "type",
        EFFECT_KW => "effect",
        kind => unreachable!("item called at {kind:?}"),
    };
    p.error(
        codes::NOT_YET_SUPPORTED,
        format!("`{keyword}` items are not supported yet"),
        "item syntax is implemented in a later stage",
    );
    let m = p.start();
    p.bump_any();
    skip_to_next_item(p);
    m.complete(p, ERROR);
}

/// 項目の外にあるトークンの並びを、次の同期点まで1つの `ERROR` ノードにまとめる。
/// 診断は1件だけ出す。`ERROR_TOKEN` は字句解析で報告済みなので、それ以外のトークンの位置に出す。
fn stray_tokens(p: &mut Parser) {
    let m = p.start();
    let mut reported = false;
    while !p.at_eof() && !p.at_ts(ITEM_START) {
        if !reported && !p.at(ERROR_TOKEN) && !p.current().is_virtual() {
            p.error(
                codes::EXPECTED_ITEM,
                "expected an item (`fn`, `type`, or `effect`)",
                "not the start of an item",
            );
            reported = true;
        }
        p.bump_any();
    }
    m.complete(p, ERROR);
}

fn skip_to_next_item(p: &mut Parser) {
    while !p.at_eof() && !p.at_ts(ITEM_START) {
        p.bump_any();
    }
}

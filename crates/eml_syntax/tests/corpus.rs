use crate::common::{diagnostics, item_kinds};

const S1: &str = include_str!("corpus/s1.em");
const LATER_STAGES: &str = include_str!("corpus/later_stages.em");

#[test]
fn s1_corpus_has_no_diagnostics() {
    assert_eq!(diagnostics(S1), Vec::<String>::new());
}

#[test]
fn s1_corpus_items() {
    assert_eq!(
        item_kinds(S1),
        [
            "FIXITY_ITEM",
            "FIXITY_ITEM",
            "DATA_ITEM",
            "DATA_ITEM",
            "EFFECT_ITEM",
            "EFFECT_ITEM",
            "EFFECT_ITEM",
            "SIGNATURE",
            "EQUATION",
            "EQUATION",
            "SIGNATURE",
            "EQUATION",
            "SIGNATURE",
            "EQUATION",
            "SIGNATURE",
            "EQUATION",
            "SIGNATURE",
            "EQUATION",
            "SIGNATURE",
            "EQUATION",
            "SIGNATURE",
            "EQUATION",
            "SIGNATURE",
            "EQUATION",
        ]
    );
}

#[test]
fn later_stage_corpus_has_no_syntax_diagnostics() {
    // パーサが E0004 を出す構文はもうない (docs/implementation/status.md の「未対応の構文と E0004」)。
    assert_eq!(diagnostics(LATER_STAGES), Vec::<String>::new());
}

/// 編集の途中のような、任意の位置で切れたソースでも、パニックせずに lossless な木を返す
/// (lossless の確認は `diagnostics` の中で行う)。
#[test]
fn every_prefix_of_the_corpus_parses() {
    for corpus in [S1, LATER_STAGES] {
        for (end, _) in corpus.char_indices() {
            diagnostics(&corpus[..end]);
        }
        diagnostics(corpus);
    }
}

/// 1行を消したソースでも、パニックせずに lossless な木を返す。
#[test]
fn every_line_deletion_of_the_corpus_parses() {
    for corpus in [S1, LATER_STAGES] {
        let lines: Vec<&str> = corpus.lines().collect();
        for skip in 0..lines.len() {
            let text: Vec<&str> = lines
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != skip)
                .map(|(_, line)| *line)
                .collect();
            diagnostics(&text.join("\n"));
        }
    }
}

const STRINGS: &str = include_str!("corpus/strings.em");

#[test]
fn strings_corpus_has_no_syntax_diagnostics() {
    // コマンドリテラルの E0004 は HIR が出すので、`eml_syntax` の診断は空である
    assert_eq!(diagnostics(STRINGS), Vec::<String>::new());
}

#[test]
fn every_prefix_of_the_strings_corpus_parses() {
    for (end, _) in STRINGS.char_indices() {
        diagnostics(&STRINGS[..end]);
    }
}

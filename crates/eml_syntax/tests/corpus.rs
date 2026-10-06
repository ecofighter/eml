mod common;

use common::{diagnostics, item_kinds};

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
fn later_stage_corpus_reports_only_not_yet_supported() {
    // S2・S3 の構文は E0004 だけを出し、ほかの診断を連鎖させない。パーサが E0004 を出すのは、補間、コマンドリテラル、
    // レコード、リストだけである (docs/spec/grammar.md の「実装の段階」)。
    let found = diagnostics(LATER_STAGES);
    assert!(!found.is_empty());
    for line in &found {
        assert!(line.starts_with("E0004 "), "unexpected diagnostic: {line}");
    }
    let mut messages: Vec<&str> = found
        .iter()
        .map(|line| line.splitn(3, ' ').nth(2).unwrap())
        .collect();
    messages.sort();
    messages.dedup();
    assert_eq!(
        messages,
        [
            "command literals are not supported yet",
            "lists are not supported yet",
            "records are not supported yet",
            "string interpolation is not supported yet",
        ]
    );
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

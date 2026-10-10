//! リストのリテラルとパターン (docs/superpowers/specs/2026-10-10-s6a-lists-design.md)。

use crate::common::{diagnostics, lower_files_text, lower_text};
use eml_hir::PatKind;
use eml_test_support::lower_clean;

#[test]
fn list_patterns_become_cons_and_nil() {
    let text = "f : List Int -> Int\nf xs = match xs with\n  | [] -> 0\n  | [a] -> a\n  | [a, b] -> b\n  | a :: rest -> a";
    insta::assert_snapshot!(lower_text(text), @"
    f : List Int -> Int
    f xs#0 = (match xs#0 with | [] -> 0 | [a#1] -> a#1 | [a#2, b#3] -> b#3 | a#4 :: rest#5 -> a#4)
    ");
}

#[test]
fn list_patterns_bind_left_to_right_and_nest() {
    let text =
        "f : List (List Int) -> Int\nf xs = match xs with\n  | [[a], [], [b, c]] -> c\n  | _ -> 0";
    insta::assert_snapshot!(lower_text(text), @"
    f : List (List Int) -> Int
    f xs#0 = (match xs#0 with | [[a#1], [], [b#2, c#3]] -> c#3 | _ -> 0)
    ");
}

#[test]
fn a_name_is_bound_once_per_list_pattern() {
    assert_eq!(
        diagnostics("f : List Int -> Int\nf [x, x] = x\nf _ = 0"),
        ["E1017 2:7 `x` is bound more than once"]
    );
}

/// 1つ目の `::` は `[` から `]` まで、k 番目は k 番目の要素の始まりから `]` まで、`Nil` は `]` である。
#[test]
fn list_pattern_ranges_cover_the_rest_of_the_list() {
    let text = "f : List Int -> Int\nf [a, b] = a\nf _ = 0";
    let lowered = lower_clean(text);
    let program = &lowered.program;
    let (id, _) = program.functions().find(|(_, f)| f.name == "f").unwrap();
    let body = program.body(id).unwrap();
    let source = lowered.files().text(lowered.file());
    let mut ranges: Vec<&str> = body
        .pats
        .iter()
        .filter(|(_, pat)| matches!(pat.kind, PatKind::Con { .. }))
        .map(|(_, pat)| &source[pat.range])
        .collect();
    ranges.sort();
    assert_eq!(ranges, ["[a, b]", "]", "b]"]);
}

#[test]
fn list_syntax_means_the_prelude_list_even_with_a_user_nil() {
    let text = "data Stack = | Nil | Push Int Stack\n\nf : List Int -> Int\nf xs = match xs with\n  | [] -> 0\n  | _ -> 1";
    insta::assert_snapshot!(lower_text(text), @"
    data Stack
      | Nil
      | Push Int Stack
    f : List Int -> Int
    f xs#0 = (match xs#0 with | [] -> 0 | _ -> 1)
    ");
}

#[test]
fn list_syntax_means_the_prelude_list_even_with_an_imported_nil() {
    let text = lower_files_text(
        "import Stack (Stack(..))\n\nf : List Int -> Int\nf xs = match xs with\n  | [] -> 0\n  | _ -> 1",
        &[("Stack.em", "pub data Stack = | Nil | Push Int Stack")],
    );
    insta::assert_snapshot!(text, @"
    -- Main
    f : List Int -> Int
    f xs#0 = (match xs#0 with | [] -> 0 | _ -> 1)
    -- Stack
    data Stack
      | Nil
      | Push Int Stack
    ");
}

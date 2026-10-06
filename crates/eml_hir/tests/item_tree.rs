//! item の収集 (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 3.1)。

use eml_hir::item_tree;
use eml_test_support::{parse, short};

fn tree(text: &str) -> (eml_hir::ItemTree, Vec<String>) {
    let parsed = parse(text);
    let (tree, mut diagnostics) = item_tree(parsed.file, &parsed.parse.tree());
    eml_diagnostics::sort_diagnostics(&mut diagnostics);
    (tree, short(&parsed.files, &diagnostics))
}

#[test]
fn signatures_and_equations_are_grouped_by_name() {
    let (tree, diagnostics) = tree("f : Int -> Int\nf 0 = 1\nf n = n\npub g : Int\ng = 1");
    // `pub` は Task 4 で実装する。それまでは E0004 が出る
    assert_eq!(diagnostics, ["E0004 4:1 `pub` is not supported yet"]);
    let names: Vec<(&str, bool, usize)> = tree
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f.public, f.equations.len()))
        .collect();
    assert_eq!(names, [("f", false, 2), ("g", true, 1)]);
}

#[test]
fn data_effects_and_fixities_list_their_parts() {
    let (tree, _) =
        tree("pub data T = | A | B Int\neffect E where\n  get : Unit -> Int\ninfixl 6 <+>");
    assert_eq!(tree.data[0].name, "T");
    assert!(tree.data[0].public);
    let constructors: Vec<&str> = tree.data[0]
        .constructors
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(constructors, ["A", "B"]);
    assert_eq!(tree.effects[0].operations[0].name, "get");
    assert_eq!(tree.fixities[0].operators[0].0, "<+>");
}

#[test]
fn ordering_errors_are_reported_without_resolving_names() {
    let (_, diagnostics) = tree(
        "f : Int\ng : Int\ng = 1\nf = 2\nh = 3\nk : Int\nk = 1\nk = 2\nm : Int\nm : Int\nm = 1",
    );
    assert_eq!(
        diagnostics,
        [
            "E1019 4:1 the signature of `f` is not followed by its equations",
            "E1004 5:1 `h` has no type signature",
            "E1003 10:1 `m` is defined more than once",
            "E1019 11:1 the signature of `m` is not followed by its equations",
        ]
    );
}

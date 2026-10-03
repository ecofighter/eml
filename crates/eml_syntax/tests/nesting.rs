mod common;

use common::diagnostics;

fn assert_one_nesting_error(text: &str) {
    let found = diagnostics(text);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("E0013 1:") && found[0].ends_with(" nesting is too deep"),
        "{found:?}"
    );
}

#[test]
fn deeply_nested_parentheses_report_one_error() {
    let depth = 10_000;
    assert_one_nesting_error(&format!("x = {}1{}", "(".repeat(depth), ")".repeat(depth)));
}

#[test]
fn deep_if_chain_reports_one_error() {
    assert_one_nesting_error(&format!("x = {}1", "if a then ".repeat(1_000)));
}

#[test]
fn deeply_nested_patterns_report_one_error() {
    let depth = 10_000;
    assert_one_nesting_error(&format!(
        "f {}x{} = 1",
        "(".repeat(depth),
        ")".repeat(depth)
    ));
}

#[test]
fn deeply_nested_types_report_one_error() {
    let depth = 10_000;
    assert_one_nesting_error(&format!(
        "f : {}Int{}",
        "(".repeat(depth),
        ")".repeat(depth)
    ));
}

#[test]
fn moderate_nesting_is_fine() {
    let depth = 100;
    assert!(diagnostics(&format!("x = {}1{}", "(".repeat(depth), ")".repeat(depth))).is_empty());
}

#[test]
fn errors_in_the_next_item_are_still_reported() {
    let depth = 10_000;
    let text = format!("x = {}1{}\ny = 1 +", "(".repeat(depth), ")".repeat(depth));
    let found = diagnostics(&text);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("E0013 1:"), "{found:?}");
    assert_eq!(found[1], "E0011 2:8 expected an expression");
}

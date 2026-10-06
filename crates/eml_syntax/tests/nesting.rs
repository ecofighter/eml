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
fn deep_chain_of_misplaced_resumes_does_not_overflow_the_stack() {
    let found = diagnostics(&format!("x = g {}k", "resume k ".repeat(10_000)));
    let too_deep = found.iter().filter(|d| d.starts_with("E0013 ")).count();
    assert_eq!(too_deep, 1, "{found:?}");
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

fn nested_let_blocks(levels: usize) -> String {
    let mut text = String::from("x =\n");
    for i in 0..levels {
        text.push_str(&format!("{}let a{i} =\n", " ".repeat(i + 1)));
    }
    text.push_str(&format!("{}1", " ".repeat(levels + 1)));
    text
}

fn assert_one_nesting_error_anywhere(text: &str) {
    let found = diagnostics(text);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("E0013 ") && found[0].ends_with(" nesting is too deep"),
        "{found:?}"
    );
}

#[test]
fn very_deep_nested_let_blocks_do_not_overflow_the_stack() {
    assert_one_nesting_error_anywhere(&nested_let_blocks(20_000));
}

#[test]
fn long_field_access_chain_reports_one_error() {
    assert_one_nesting_error(&format!("x = a{}", ".b".repeat(30_000)));
}

#[test]
fn moderate_field_access_chain_is_fine() {
    assert!(diagnostics(&format!("x = a{}", ".b".repeat(200))).is_empty());
}

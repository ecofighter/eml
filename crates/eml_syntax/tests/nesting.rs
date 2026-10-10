use crate::common::diagnostics;

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
fn deep_chain_of_drops_reports_the_nesting_limit() {
    let found = diagnostics(&format!("x = g {}k", "drop ".repeat(10_000)));
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
    assert_one_nesting_error_anywhere(&nested_let_blocks(1_000));
}

#[test]
fn long_field_access_chain_reports_one_error() {
    assert_one_nesting_error(&format!("x = a{}", ".b".repeat(30_000)));
}

#[test]
fn moderate_field_access_chain_is_fine() {
    assert!(diagnostics(&format!("x = a{}", ".b".repeat(200))).is_empty());
}

#[test]
fn long_operator_chain_reports_one_error() {
    assert_one_nesting_error(&format!("x = a{}", " || a".repeat(30_000)));
}

#[test]
fn long_run_of_prefix_minus_reports_one_error() {
    assert_one_nesting_error(&format!("x = {}1", "- ".repeat(30_000)));
}

#[test]
fn moderate_operator_chain_is_fine() {
    assert!(diagnostics(&format!("x = a{}", " + a".repeat(200))).is_empty());
}

/// 括弧に入れた列を左に重ねると、組み直した木の深さは各段の演算子の数の和になる。
fn stacked_chains(levels: usize, operators: usize) -> String {
    let mut expr = String::from("x");
    for _ in 0..levels {
        expr = format!("({expr}){}", " + x".repeat(operators));
    }
    expr
}

#[test]
fn stacked_parenthesized_chains_report_one_error() {
    assert_one_nesting_error(&format!("y = {}", stacked_chains(8, 245)));
}

#[test]
fn moderate_stacked_chains_are_fine() {
    assert!(diagnostics(&format!("y = {}", stacked_chains(4, 50))).is_empty());
}

#[test]
fn long_run_of_use_statements_reports_one_error() {
    let mut text = String::from("f x =\n");
    for i in 0..1_000 {
        text.push_str(&format!("  use n{i} <- pass\n"));
    }
    text.push_str("  x");
    assert_one_nesting_error_anywhere(&text);
}

#[test]
fn two_tall_operands_are_not_counted_on_top_of_each_other() {
    let sum = vec!["x"; 130].join(" + ");
    assert!(diagnostics(&format!("y = ({sum}) * ({sum})")).is_empty());
}

/// 括弧に入れたコンストラクタの列を左に重ねたパターン。HIR が左結合に組み直すと、深さは各段の数の和になる。
fn stacked_con_pats(levels: usize, constructors: usize) -> String {
    let mut pat = String::from("Leaf");
    for _ in 0..levels {
        pat = format!("({pat}){}", " :+ 1".repeat(constructors));
    }
    pat
}

#[test]
fn stacked_parenthesized_constructor_patterns_report_one_error() {
    assert_one_nesting_error_anywhere(&format!(
        "f t = match t with\n  | {} -> 1",
        stacked_con_pats(20, 200)
    ));
}

#[test]
fn moderate_stacked_constructor_patterns_are_fine() {
    assert!(
        diagnostics(&format!(
            "f t = match t with\n  | {} -> 1",
            stacked_con_pats(4, 50)
        ))
        .is_empty()
    );
}

fn list_pattern(n: usize) -> String {
    let names: Vec<String> = (0..n).map(|i| format!("x{i}")).collect();
    format!("f [{}] = 0", names.join(", "))
}

/// HIR がパターンのリストを右に入れ子の `::` に組むので、k 番目の要素は k - 1 段深く読む
/// (docs/spec/grammar.md の「文法上の補足」)。
#[test]
fn a_list_pattern_counts_each_element_as_a_level() {
    const LIMIT: usize = 256;
    assert_eq!(diagnostics(&list_pattern(LIMIT)), Vec::<String>::new());
    let codes: Vec<String> = diagnostics(&list_pattern(LIMIT + 1))
        .into_iter()
        .map(|d| d[..5].to_string())
        .collect();
    assert_eq!(codes, ["E0013"]);
}

#[test]
fn a_list_expression_does_not_count_its_elements() {
    let items: Vec<String> = (0..2000).map(|i| i.to_string()).collect();
    let text = format!("x = [{}]", items.join(", "));
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

fn nested(open: &str, close: &str, depth: usize) -> String {
    format!("x = {}1{}", open.repeat(depth), close.repeat(depth))
}

/// `[` は括弧と同じく1段に数える (docs/spec/grammar.md の「文法上の補足」)。どちらも、`x = ` の式の1段と
/// 中の `1` の演算子の列の1段を足して、上限の 256 に届く。
#[test]
fn a_nested_list_expression_counts_each_bracket_as_one_level() {
    for (open, close) in [("(", ")"), ("[", "]")] {
        assert_eq!(diagnostics(&nested(open, close, 254)), Vec::<String>::new());
        assert_eq!(
            diagnostics(&nested(open, close, 255)),
            ["E0013 1:260 nesting is too deep"]
        );
    }
}

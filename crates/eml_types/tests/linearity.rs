//! 線形性の診断 (docs/spec/diagnostics.md の「線形性の診断」)。番号、文言、指す場所を確かめる。

use eml_test_support::{check, full};

/// `once` の操作と、純粋な条件。どのテストも7行目から関数を書く。
const HEADER: &str =
    "effect Ask where\n  ask : Unit -> Int\n\nflag : Unit -> Bool\nflag () = True\n\n";

fn diagnostics(rest: &str) -> String {
    let checked = check(&format!("{HEADER}{rest}"));
    full(&checked.files, &checked.diagnostics)
}

#[test]
fn a_value_used_twice_points_at_both_uses() {
    let rest = "twice : Unit -> Int\ntwice () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        resume j 1 + resume j 2";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3002 12:29 `j` must be used exactly once, but it is used more than once
      12:29 used again here
      12:16 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_branch_that_does_not_use_a_value() {
    let rest = "branch : Unit -> Int\nbranch () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        if flag () then resume j 1 else 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but some paths do not use it
      11:13 `j` is bound here
      12:41 this branch does not use `j`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn an_omitted_else_does_not_use_a_value() {
    let rest = "omitted : Unit -> Int\nomitted () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        if flag () then drop j\n        0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but some paths do not use it
      11:13 `j` is bound here
      12:9 the omitted `else` does not use `j`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_match_arm_that_does_not_use_a_value() {
    let rest = "arm : Bool -> Int\narm b =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        match b with\n          | True -> resume j 1\n          | False -> 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but some paths do not use it
      11:13 `j` is bound here
      14:22 this branch does not use `j`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_value_never_used_points_at_the_end_of_its_scope() {
    let rest = "unused : Unit -> Int\nunused () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but it is not used
      11:13 `j` is bound here
      12:10 `j` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_shadowed_value_is_not_consumed() {
    let rest = "shadowed : Unit -> Int\nshadowed () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        let j = 1\n        j";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but it is not used
      11:13 `j` is bound here
      13:10 `j` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_continuation_unused_on_some_paths_points_at_the_clause() {
    let rest = "partial : Unit -> Int\npartial () =\n  handle ask () with\n    | ask () k -> if flag () then resume k 1 else 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3005 10:5 the continuation `k` of a `once` operation must be resumed or dropped
      10:5 this clause
      10:14 `k` is bound here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: call `resume k v` or `drop k` on every path
    ");
}

#[test]
fn a_body_with_a_type_error_reports_no_linearity_errors() {
    let rest = "broken : Unit -> Int\nbroken () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        resume j \"no\" + resume j 2";
    let checked = check(&format!("{HEADER}{rest}"));
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E2001"]);
}

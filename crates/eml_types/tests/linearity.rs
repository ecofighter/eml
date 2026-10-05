//! 線形性の診断 (docs/spec/diagnostics.md の「線形性の診断」)。番号、文言、指す場所を確かめる。

use eml_test_support::{check, fixes, full};

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
      12:13 `j` is shadowed here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop` before it is shadowed
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

fn fix_text(rest: &str) -> String {
    let checked = check(&format!("{HEADER}{rest}"));
    fixes(&checked.files, &checked.diagnostics)
}

#[test]
fn the_fix_inserts_drop_before_the_last_statement_of_the_scope() {
    let rest = "unused : Unit -> Int\nunused () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        0";
    insta::assert_snapshot!(fix_text(rest), @r#"
    E3003 11:13
      12:9..12:9 "drop j\n        "
    "#);
}

#[test]
fn the_fix_inserts_drop_into_a_branch_that_is_a_block() {
    let rest = "arm : Bool -> Int\narm b =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        match b with\n          | True -> resume j 1\n          | False ->\n              let n = 0\n              n";
    insta::assert_snapshot!(fix_text(rest), @r#"
    E3003 11:13
      16:15..16:15 "drop j\n              "
    "#);
}

#[test]
fn no_fix_for_a_branch_on_one_line() {
    let rest = "branch : Unit -> Int\nbranch () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        if flag () then resume j 1 else 0";
    assert_eq!(fix_text(rest), "");
}

#[test]
fn no_fix_when_a_shadowing_binding_comes_first() {
    let rest = "shadowed : Unit -> Int\nshadowed () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        let j = 1\n        j";
    assert_eq!(fix_text(rest), "");
}

#[test]
fn no_fix_when_the_binding_is_the_last_statement() {
    let rest = "last : Unit -> Unit\nlast () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n    | return x -> ()";
    assert_eq!(fix_text(rest), "");
    assert!(diagnostics(rest).starts_with("E3003 11:13"));
}

/// HEADER を付けずに検査する。
fn plain(text: &str) -> String {
    let checked = check(text);
    full(&checked.files, &checked.diagnostics)
}

#[test]
fn a_file_read_and_closed_is_clean() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  let (f, text) = read_all f\n  close f\n  println text";
    assert_eq!(plain(text), "");
}

#[test]
fn a_file_closed_twice() {
    let text =
        "main : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  close f\n  close f";
    insta::assert_snapshot!(plain(text), @r"
    E3002 5:9 `f` must be used exactly once, but it is used more than once
      5:9 used again here
      4:9 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn data_with_a_file_field_is_linear() {
    let text = "data Handle = | Handle File\n\nmain : Unit -> <IO> Unit\nmain () =\n  let h = Handle (open \"a.txt\")\n  drop h\n  drop h";
    insta::assert_snapshot!(plain(text), @r"
    E3002 7:8 `h` must be used exactly once, but it is used more than once
      7:8 used again here
      6:8 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn recursive_and_mutually_recursive_data_with_a_file_are_linear() {
    let text = "data Files =\n  | Nil\n  | More File Files\n\ndata A =\n  | NoA\n  | SomeA B\n\ndata B = | B File\n\nmain : Unit -> <IO> Unit\nmain () =\n  let fs = More (open \"a.txt\") Nil\n  let a = SomeA (B (open \"b.txt\"))\n  ()";
    let codes: Vec<String> = check(text)
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E3003", "E3003"]);
}

#[test]
fn a_file_cannot_be_discarded_from_a_tuple() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let (_, text) = read_all (open \"a.txt\")\n  println text";
    insta::assert_snapshot!(plain(text), @r"
    E3004 3:8 a linear value cannot be discarded with `_`
      3:8 this pattern discards it
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: bind it to a name and pass the name to `drop`
    ");
}

#[test]
fn a_file_cannot_go_where_a_value_is_used_twice() {
    let text = "pair : a -> (a, a)\npair x = (x, x)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let (f, g) = pair (open \"a.txt\")\n  close f\n  close g";
    insta::assert_snapshot!(plain(text), @r"
    E3001 6:16 a linear value is passed to `pair`, which may use it more than once or not at all
      6:16 `pair` is used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn no_fix_for_a_function_body_on_one_line() {
    let text = "consume : File -> Int\nconsume f = 0";
    insta::assert_snapshot!(plain(text), @r"
    E3003 2:9 `f` must be used exactly once, but it is not used
      2:9 `f` is bound here
      2:14 `f` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `f` to `drop`
    ");
    let checked = check(text);
    assert_eq!(fixes(&checked.files, &checked.diagnostics), "");
}

const TWO_OPS: &str = "effect Two where\n  one : Unit -> Unit\n  two : Unit -> Unit\n\n";

#[test]
fn two_operation_clauses_capturing_one_file_report_once() {
    let text = format!(
        "{TWO_OPS}main : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  handle one () with\n    | one () k -> resume k (close f)\n    | two () k -> resume k (close f)"
    );
    let checked = check(&text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(
        codes,
        ["E3001"],
        "{}",
        full(&checked.files, &checked.diagnostics)
    );
}

#[test]
fn a_file_captured_by_the_body_and_the_return_clause() {
    let text = format!(
        "{TWO_OPS}main : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  handle close f with\n    | one () k -> resume k ()\n    | two () k -> resume k ()\n    | return x -> close f"
    );
    insta::assert_snapshot!(plain(&text), @r"
    E3002 11:19 `f` must be used exactly once, but it is used more than once
      11:19 used again here
      8:10 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_type_error_in_one_body_does_not_hide_linearity_errors_in_another() {
    let text = "bad : Unit -> <IO> Unit\nbad () = println 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  close f\n  close f";
    let checked = check(text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(
        codes,
        ["E2001", "E3002"],
        "{}",
        full(&checked.files, &checked.diagnostics)
    );
}

#[test]
fn the_fix_for_a_shadowed_value_goes_before_the_shadowing_let() {
    let rest = "last : Unit -> Unit\nlast () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        let j = 1\n    | return x -> ()";
    insta::assert_snapshot!(fix_text(rest), @r#"
    E3003 11:13
      12:9..12:9 "drop j\n        "
    "#);
}

#[test]
fn a_same_name_in_an_unrelated_lambda_keeps_the_fix() {
    // ラムダの引数の `j` は、`drop j` を入れる位置では見えないので、外側の `j` を隠さない
    let rest = "lambda_name : Unit -> Int\nlambda_name () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        let f = fn j -> j + 1\n        f 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but it is not used
      11:13 `j` is bound here
      13:12 `j` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
    insta::assert_snapshot!(fix_text(rest), @r#"
    E3003 11:13
      13:9..13:9 "drop j\n        "
    "#);
}

#[test]
fn a_value_used_twice_inside_one_branch() {
    let rest = "inbranch : Unit -> Int\ninbranch () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        if flag () then resume j 1 + resume j 2 else resume j 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3002 12:45 `j` must be used exactly once, but it is used more than once
      12:45 used again here
      12:32 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn an_unused_lambda_parameter_points_at_the_end_of_the_lambda_body() {
    let rest = "lambda_param : Unit -> Int\nlambda_param () =\n  handle ask () with\n    | ask () k ->\n        let f = fn j -> 0\n        f k";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:20 `j` must be used exactly once, but it is not used
      11:20 `j` is bound here
      11:26 `j` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn an_unused_arm_pattern_variable_points_at_the_end_of_the_arm() {
    let rest = "arm_var : Unit -> Int\narm_var () =\n  handle ask () with\n    | ask () k ->\n        match (k, 1) with\n          | (j, n) -> n";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 12:14 `j` must be used exactly once, but it is not used
      12:14 `j` is bound here
      12:24 `j` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_continuation_used_twice_on_one_path_is_a_double_use() {
    // 2回使う経路があれば、使わない経路があっても E3005 ではなく E3002 にする
    let rest = "twice_or_none : Unit -> Int\ntwice_or_none () =\n  handle ask () with\n    | ask () k -> if flag () then resume k 1 + resume k 2 else 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3002 10:55 `k` must be used exactly once, but it is used more than once
      10:55 used again here
      10:42 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn no_fix_for_an_omitted_else() {
    let rest = "omitted : Unit -> Int\nomitted () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        if flag () then drop j\n        0";
    assert_eq!(fix_text(rest), "");
}

#[test]
fn no_fix_when_the_last_statement_does_not_start_a_line() {
    let rest = "same_line : Unit -> Int\nsame_line () =\n  handle ask () with\n    | ask () k ->\n        let j = k; 0";
    assert!(diagnostics(rest).starts_with("E3003 11:13"));
    assert_eq!(fix_text(rest), "");
}

/// 持ち越し規則のテストの宣言 (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。どのテストも20行目から関数を書く。
const CARRY: &str = "effect Choice where\n  multi choose : Unit -> Bool\n\neffect Ask where\n  ask : Unit -> Int\n\neffect Fail where\n  never fail : Unit -> a\n\neffect Mixed where\n  single : Unit -> Int\n  multi many : Unit -> Int\n\neffect Use where\n  use_file : File -> Unit\n\nconsume : File -> Bool -> <IO> Unit\nconsume f b = close f\n\n";

fn carried(rest: &str) -> String {
    let checked = check(&format!("{CARRY}{rest}"));
    full(&checked.files, &checked.diagnostics)
}

#[test]
fn a_file_kept_across_a_multi_operation() {
    let rest = "held : Unit -> <Choice, IO> Unit\nheld () =\n  let f = open \"a.txt\"\n  let b = choose ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:11 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:11 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn an_evaluated_argument_is_kept_across_a_later_argument() {
    let rest = "temporary : Unit -> <Choice, IO> Unit\ntemporary () =\n  let f = open \"a.txt\"\n  consume f (choose ())";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:14 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      23:14 this call may perform `choose`, a `multi` operation
      23:11 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn an_evaluated_tuple_element_is_kept_across_a_later_element() {
    let rest = "paired : Unit -> <Choice, IO> Unit\npaired () =\n  let f = open \"a.txt\"\n  let (g, b) = (f, choose ())\n  close g";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:20 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      23:20 this call may perform `choose`, a `multi` operation
      23:17 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn a_call_of_a_function_that_performs_a_multi_operation() {
    let rest = "pick : Unit -> <Choice> Bool\npick () = choose ()\n\nthrough : Unit -> <Choice, IO> Unit\nthrough () =\n  let f = open \"a.txt\"\n  let b = pick ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 26:11 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      26:11 this call may perform `choose`, a `multi` operation
      25:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn a_value_kept_across_two_calls_is_reported_once() {
    let rest = "twice : Unit -> <Choice, IO> Unit\ntwice () =\n  let f = open \"a.txt\"\n  let a = choose ()\n  let b = choose ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:11 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:11 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

/// 精度の限界 (spec の「精度の限界」)。`log` の row は呼び出しで今の row と単一化されるので、`println` しか起こさない
/// のに `choose` を起こしうると判定される。
#[test]
fn a_local_lambda_takes_the_row_of_its_caller() {
    let rest = "logged : Unit -> <Choice, IO> Unit\nlogged () =\n  let f = open \"a.txt\"\n  let log = fn () -> println \"x\"\n  log ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:3 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:3 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn a_multi_operation_of_an_effect_with_once_operations() {
    let rest = "many_held : Unit -> <Mixed, IO> Unit\nmany_held () =\n  let f = open \"a.txt\"\n  let n = many ()\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:11 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:11 this call may perform `many`, a `multi` operation
      22:7 `f` is bound here
      12:9 `many` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn a_once_continuation_kept_across_a_multi_operation_in_a_clause() {
    let rest = "inner : Unit -> <Choice> Int\ninner () =\n  handle ask () with\n    | ask () k ->\n        let b = choose ()\n        resume k (if b then 1 else 2)";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:17 `k` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:17 this call may perform `choose`, a `multi` operation
      23:14 `k` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `k` before this call
    ");
}

#[test]
fn a_piped_value_is_kept_across_the_call() {
    let rest = "consume_after : Bool -> File -> <IO> Unit\nconsume_after b f = close f\n\npiped : Unit -> <Choice, IO> Unit\npiped () =\n  let f = open \"a.txt\"\n  f |> consume_after (choose ())";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 26:23 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      26:23 this call may perform `choose`, a `multi` operation
      26:3 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn an_argument_for_a_later_arrow_is_kept_across_the_call() {
    let rest = "choose_then : Unit -> <Choice> (File -> <IO> Unit)\nchoose_then () =\n  let b = choose ()\n  fn f -> close f\n\napplied : Unit -> <Choice, IO> Unit\napplied () =\n  let f = open \"a.txt\"\n  choose_then () f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 28:3 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      28:3 this call may perform `choose`, a `multi` operation
      28:18 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn a_lambda_body_is_checked_on_its_own() {
    let rest = "in_lambda : Unit -> <Choice, IO> Unit\nin_lambda () =\n  let f = open \"a.txt\"\n  let later = fn () ->\n    let b = choose ()\n    close f\n  later ()";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:13 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:13 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn a_value_kept_on_one_branch() {
    let rest = "branch : Bool -> <Choice, IO> Unit\nbranch c =\n  let f = open \"a.txt\"\n  if c then\n    let b = choose ()\n    close f\n  else close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:13 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:13 this call may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

/// 型の誤りを報告済みの本体では、持ち越しのパスも由来を記録しないので、E3006 を重ねない (docs/spec/diagnostics.md)。
#[test]
fn a_body_with_a_type_error_reports_no_carry_over() {
    let rest = "broken : Unit -> <Choice, IO> Unit\nbroken () =\n  let f = open \"a.txt\"\n  let b = choose ()\n  close f\n  1";
    let text = carried(rest);
    assert!(text.contains("E2001"), "{text}");
    assert!(!text.contains("E3006"), "{text}");
}

#[test]
fn a_file_may_be_kept_across_once_and_never_operations() {
    let rest = "across_once : Unit -> <Ask, IO> Unit\nacross_once () =\n  let f = open \"a.txt\"\n  let n = ask ()\n  close f\n\nacross_never : Bool -> <Fail, IO> Unit\nacross_never b =\n  let f = open \"a.txt\"\n  if b then fail () else ()\n  close f";
    assert_eq!(carried(rest), "");
}

/// トップレベルの値の呼び出しは、開く前の宣言の row で判定する。`println` の row は `<IO>` なので、今の row に
/// `Choice` があっても `multi` を起こさない (spec の「型検査が記録するもの」)。
#[test]
fn a_file_may_be_kept_across_a_call_whose_declared_row_has_no_multi() {
    let rest = "across_io : Unit -> <Choice, IO> Unit\nacross_io () =\n  let f = open \"a.txt\"\n  println \"x\"\n  close f\n  let b = choose ()\n  ()";
    assert_eq!(carried(rest), "");
}

/// 操作を直接呼ぶときは、その操作の多重度だけを見る (docs/spec/effects.md)。
#[test]
fn a_file_may_be_kept_across_a_once_operation_of_an_effect_with_multi_operations() {
    let rest = "across_single : Unit -> <Mixed, IO> Unit\nacross_single () =\n  let f = open \"a.txt\"\n  let n = single ()\n  close f";
    assert_eq!(carried(rest), "");
}

#[test]
fn a_file_passed_to_the_call_is_not_kept_across_it() {
    let rest = "chooser_closes : File -> <Choice, IO> Unit\nchooser_closes f =\n  close f\n  let b = choose ()\n  ()\n\npassed : Unit -> <Choice, IO> Unit\npassed () =\n  let f = open \"a.txt\"\n  chooser_closes f";
    assert_eq!(carried(rest), "");
}

#[test]
fn unrestricted_values_may_be_kept_across_a_multi_operation() {
    let rest = "counted : Unit -> <Choice> Int\ncounted () =\n  let n = 1\n  let b = choose ()\n  n\n\nnested : Unit -> <Choice> Int\nnested () =\n  handle (if choose () then 1 else 2) with\n    | choose () k ->\n        let b = choose ()\n        resume k b";
    assert_eq!(carried(rest), "");
}

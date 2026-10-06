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
    E3003 11:13 insert `drop j`
      12:9..12:9 "drop j\n        "
    "#);
}

#[test]
fn the_fix_inserts_drop_into_a_branch_that_is_a_block() {
    let rest = "arm : Bool -> Int\narm b =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        match b with\n          | True -> resume j 1\n          | False ->\n              let n = 0\n              n";
    insta::assert_snapshot!(fix_text(rest), @r#"
    E3003 11:13 insert `drop j`
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
    E3003 11:13 insert `drop j`
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
    E3003 11:13 insert `drop j`
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
fn an_applied_function_is_kept_across_a_later_argument() {
    let rest = "make : File -> <IO> (Unit -> <IO> Unit)\nmake f = fn () -> close f\n\napplied : Unit -> <Choice, IO> Unit\napplied () =\n  let f = open \"a.txt\"\n  make f (if choose () then () else ())";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 26:14 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      26:14 this call may perform `choose`, a `multi` operation
      26:3 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

#[test]
fn a_local_is_kept_across_the_arrow_applied_before_a_later_argument() {
    let rest = "choose_then : Int -> <Choice> (Unit -> <IO> Unit)\nchoose_then n =\n  let b = choose ()\n  fn u -> ()\n\napplied_first : Unit -> <Choice, IO> Unit\napplied_first () =\n  let f = open \"a.txt\"\n  let h = choose_then\n  h 1 (close f)";
    insta::assert_snapshot!(carried(rest), @"
    E3006 29:3 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      29:3 this call may perform `choose`, a `multi` operation
      27:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

/// パイプは Prelude の `|>` の呼び出しで、`multi` の操作を起こす呼び出しは2番目の引数 `k 1 (yes ())` である。そのため primary は
/// 32:8 のこの呼び出しを指し、持ち越す値は 32:3 の `f` を指す。
#[test]
fn a_piped_value_is_kept_across_the_arrow_applied_before_a_later_argument() {
    let rest = "choose_then : Int -> <Choice> (Bool -> File -> <IO> Unit)\nchoose_then n =\n  let b = choose ()\n  fn c -> fn g -> close g\n\nyes : Unit -> Bool\nyes () = True\n\npiped_first : Unit -> <Choice, IO> Unit\npiped_first () =\n  let f = open \"a.txt\"\n  let k = choose_then\n  f |> k 1 (yes ())";
    insta::assert_snapshot!(carried(rest), @"
    E3006 32:8 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      32:8 this call may perform `choose`, a `multi` operation
      32:3 this value is kept alive across the call
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

#[test]
fn a_clause_argument_kept_across_a_resume() {
    let rest = "use_then_choose : File -> <Use, Choice> Unit\nuse_then_choose f =\n  use_file f\n  let b = choose ()\n  ()\n\nresumed : Unit -> <Choice, IO> Unit\nresumed () =\n  let f = open \"a.txt\"\n  handle use_then_choose f with\n    | use_file g k ->\n        let r = resume k ()\n        close g\n        r";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 31:17 `g` must be used exactly once, but it is kept alive across a call that may resume more than once
      31:17 resuming `k` may perform `choose`, a `multi` operation
      30:16 `g` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `g` before this call
    ");
}

#[test]
fn a_file_kept_across_a_handle_whose_body_performs_a_multi_operation() {
    let rest = "across_handle : Unit -> <Choice, IO> Unit\nacross_handle () =\n  let f = open \"a.txt\"\n  let n =\n    handle (if choose () then ask () else 0) with\n      | ask () k -> resume k 1\n  close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 24:5 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      24:5 this handle may perform `choose`, a `multi` operation
      22:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this handle
    ");
}

/// 外側の `multi` の handler が内側の handler フレームを写すと、`return` の節のクロージャも写される (spec の「健全性の根拠」)。
#[test]
fn a_return_clause_capture_under_an_outer_multi_operation() {
    let rest = "returned : Unit -> <Choice, IO> Unit\nreturned () =\n  let f = open \"a.txt\"\n  handle (if choose () then ask () else 0) with\n    | ask () k -> resume k 1\n    | return n -> close f";
    insta::assert_snapshot!(carried(rest), @r"
    E3006 23:3 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:3 this handle may perform `choose`, a `multi` operation
      25:5 the `return` clause captures `f`
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: do not capture `f` in the `return` clause
    ");
}

#[test]
fn a_file_may_be_kept_across_a_handle_and_a_resume_without_multi() {
    let rest = "fine_return : Unit -> <IO> Unit\nfine_return () =\n  let f = open \"a.txt\"\n  handle ask () with\n    | ask () k -> resume k 1\n    | return n -> close f\n\nfine_resume : Unit -> <IO> Unit\nfine_resume () =\n  let f = open \"a.txt\"\n  handle use_file f with\n    | use_file g k ->\n        let r = resume k ()\n        close g\n        r";
    assert_eq!(carried(rest), "");
}

/// 多相な関数のテストの宣言。`CARRY` の後に置くので、どのテストも30行目から関数を書く。
const POLY: &str = "keep : a -> (Unit -> <e> Unit) -> <e> a\nkeep x action =\n  action ()\n  x\n\nchooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()\n\n";

fn polymorphic(rest: &str) -> String {
    carried(&format!("{POLY}{rest}"))
}

/// 関数のスキームに残った Kind の制約の行。なければ空にする。
fn kinds(rest: &str, function: &str) -> String {
    let checked = check(&format!("{CARRY}{POLY}{rest}"));
    let dump = eml_types::dump(&checked.program, &checked.typed);
    let head = format!("{function} : ");
    let mut lines = dump.lines().skip_while(|line| !line.starts_with(&head));
    lines.next();
    lines
        .next()
        .filter(|line| line.starts_with("  kinds: "))
        .unwrap_or("")
        .to_string()
}

#[test]
fn a_scheme_keeps_the_carry_over_of_a_polymorphic_value() {
    assert_eq!(kinds("", "keep"), "  kinds: a => <e> <= Once");
}

#[test]
fn a_scheme_keeps_the_carry_over_of_a_file() {
    let rest = "with_file : (Unit -> <e> Unit) -> <IO | e> Unit\nwith_file action =\n  let f = open \"a.txt\"\n  action ()\n  close f";
    assert_eq!(kinds(rest, "with_file"), "  kinds: <e> <= Once");
}

#[test]
fn a_scheme_keeps_the_carry_over_across_a_multi_operation() {
    let rest = "keep_choose : a -> <Choice> a\nkeep_choose x =\n  let b = choose ()\n  x";
    assert_eq!(kinds(rest, "keep_choose"), "  kinds: a => Multi <= Once");
}

#[test]
fn a_file_kept_by_a_polymorphic_function_across_a_multi_operation() {
    let rest = "kept : Unit -> <Choice, IO> Unit\nkept () =\n  let f = open \"a.txt\"\n  let g = keep f chooser\n  close g";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 33:11 `keep` keeps a linear value alive across a call that may resume more than once
      33:11 `keep` is used here
      22:3 `x` is kept alive across this call
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_carry_over_passes_through_two_functions() {
    let rest = "keep2 : a -> (Unit -> <e> Unit) -> <e> a\nkeep2 x action = keep x action\n\nkept2 : Unit -> <Choice, IO> Unit\nkept2 () =\n  let f = open \"a.txt\"\n  let g = keep2 f chooser\n  close g";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 36:11 `keep2` keeps a linear value alive across a call that may resume more than once
      36:11 `keep2` is used here
      31:18 through this use of `keep`
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_carry_over_through_three_functions_is_reported_once() {
    let rest = "keep2 : a -> (Unit -> <e> Unit) -> <e> a\nkeep2 x action =\n  action ()\n  keep x action\n\nkeep3 : a -> (Unit -> <e> Unit) -> <e> a\nkeep3 x action =\n  action ()\n  keep2 x action\n\nkept3 : Unit -> <Choice, IO> Unit\nkept3 () =\n  let f = open \"a.txt\"\n  let g = keep3 f chooser\n  close g";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 43:11 `keep3` keeps a linear value alive across a call that may resume more than once
      43:11 `keep3` is used here
      37:3 `x` is kept alive across this call
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_file_given_to_a_function_that_keeps_it_across_a_multi_operation() {
    let rest = "keep_choose : a -> <Choice> a\nkeep_choose x =\n  let b = choose ()\n  x\n\nchosen : Unit -> <Choice, IO> Unit\nchosen () =\n  let f = open \"a.txt\"\n  let g = keep_choose f\n  close g";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 38:11 `keep_choose` keeps a linear value alive across a call that may resume more than once
      38:11 `keep_choose` is used here
      32:11 `x` is kept alive across this call
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_multi_action_given_to_a_function_that_keeps_a_file() {
    let rest = "with_file : (Unit -> <e> Unit) -> <IO | e> Unit\nwith_file action =\n  let f = open \"a.txt\"\n  action ()\n  close f\n\nfiled : Unit -> <Choice, IO> Unit\nfiled () = with_file chooser";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 37:12 `with_file` keeps a linear value alive across a call that may resume more than once
      37:12 `with_file` is used here
      33:3 `f` is kept alive across this call
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}

#[test]
fn a_polymorphic_function_may_keep_unrestricted_values_or_avoid_multi() {
    let rest = "unrestricted : Unit -> <Choice> Int\nunrestricted () = keep 1 chooser\n\npure_action : Unit -> <IO> Unit\npure_action () =\n  let f = open \"a.txt\"\n  let g = keep f (fn () -> ())\n  close g";
    assert_eq!(polymorphic(rest), "");
}

#[test]
fn a_carry_over_whose_row_has_no_known_multi_operation() {
    let rest = "ping : (Unit -> <e> Unit) -> Int -> <IO | e> Unit\nping action n =\n  let f = open \"a.txt\"\n  action ()\n  close f\n  pong n\n\npong : Int -> <IO | e> Unit\npong n =\n  handle ping chooser n with\n    | choose () k -> resume k True\n\nchooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()";
    insta::assert_snapshot!(carried(rest), @"
    E3006 23:3 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      23:3 this call may perform `multi` operations
      22:7 `f` is bound here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

#[test]
fn an_omitted_return_clause_discards_a_linear_state() {
    let rest = "omitted : Unit -> <IO> Int\nomitted () =\n  handle ask () from open \"a.txt\" with\n    | ask () k f -> resume k 1 f";
    insta::assert_snapshot!(diagnostics(rest), @"
    E3004 9:22 the state of this handler is discarded by the omitted `return` clause
      9:22 this state has a linear type `File`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: write a `return` clause that takes the state, such as `| return x st -> ...`, and consume the state there
    ");
}

/// 書いた `return` の節を HIR が拒否すると、本体に誤りの跡が残り、使用回数のパスは由来を記録しない。E3004 を連鎖させない。
#[test]
fn a_rejected_return_clause_does_not_report_the_state_again() {
    let rest = "rejected : Unit -> <IO> Int\nrejected () =\n  handle ask () from open \"a.txt\" with\n    | ask () k f -> resume k 1 f\n    | return x -> x";
    let checked = check(&format!("{HEADER}{rest}"));
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E1010"]);
}

#[test]
fn a_clause_that_drops_k_must_still_consume_a_linear_state() {
    let rest = "dropped : Unit -> <IO> Int\ndropped () =\n  handle ask () from open \"a.txt\" with\n    | ask () k f ->\n        drop k\n        0\n    | return x f ->\n        close f\n        x";
    let checked = check(&format!("{HEADER}{rest}"));
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E3003"]);
}

#[test]
fn a_linear_state_is_kept_across_a_multi_operation_of_the_outer_row() {
    let rest = "kept : Unit -> <Choose, IO> Int\nkept () =\n  handle ask () from open \"a.txt\" with\n    | ask () k f -> resume k 1 f\n    | return x f ->\n        close f\n        x";
    let text = format!("effect Choose where\n  multi choose : Unit -> Bool\n\n{HEADER}{rest}");
    let checked = check(&text);
    insta::assert_snapshot!(full(&checked.files, &checked.diagnostics), @"
    E3006 12:3 the state of this handler must be used exactly once, but it is kept alive across a call that may resume more than once
      12:3 this handle may perform `choose`, a `multi` operation
      12:22 the state of this handler
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the state before this handle, or give the handler a state that is not linear
    ");
}

#[test]
fn values_held_while_the_initial_state_is_evaluated_are_carried() {
    let rest = "early : Unit -> <Choose, IO> Int\nearly () =\n  let f = open \"a.txt\"\n  let n =\n    handle ask () from (if choose () then 1 else 2) with\n      | ask () k st -> resume k st st\n      | return x _ -> x\n  close f\n  n";
    let text = format!("effect Choose where\n  multi choose : Unit -> Bool\n\n{HEADER}{rest}");
    let checked = check(&text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    // `f` は初期値の `choose ()` をまたぐ。報告は最も前の呼び出しの1件だけである (docs/spec/diagnostics.md の E3006)
    assert_eq!(codes, ["E3006"]);
}

/// `f` を使うのは handle の本体だけで、handle の後では使わない。それでも初期値は本体より先に評価するので、`f` は
/// 初期値の `choose ()` をまたぐ (docs/spec/expressions.md の「パラメータ付き handler」)。
#[test]
fn a_value_used_only_by_the_handled_body_is_carried_across_the_initial_state() {
    let rest = "use_file : File -> <Ask, IO> Int\nuse_file f =\n  close f\n  ask ()\n\nearly : Unit -> <Choose, IO> Int\nearly () =\n  let f = open \"a.txt\"\n  handle use_file f from (if choose () then 1 else 2) with\n    | ask () k st -> resume k st st\n    | return x _ -> x";
    let text = format!("effect Choose where\n  multi choose : Unit -> Bool\n\n{HEADER}{rest}");
    let checked = check(&text);
    insta::assert_snapshot!(full(&checked.files, &checked.diagnostics), @"
    E3006 18:30 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      18:30 this call may perform `choose`, a `multi` operation
      17:7 `f` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `f` before this call
    ");
}

/// 状態は handle の外側の row だけをまたぐ。本体の row に、扱う `multi` の操作があっても E3006 にしない
/// (docs/spec/linearity.md)。
#[test]
fn a_linear_state_does_not_cross_the_multi_operation_the_handler_handles() {
    let rest = "main : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle (if choose () then 1 else 2) from open \"a.txt\" with\n      | choose () k f -> resume k True f\n      | return x f ->\n          close f\n          x\n  println (show_int n)";
    let text = format!("effect Choose where\n  multi choose : Unit -> Bool\n\n{HEADER}{rest}");
    let checked = check(&text);
    insta::assert_snapshot!(full(&checked.files, &checked.diagnostics), @"");
}

#[test]
fn a_partial_application_of_an_intrinsic_keeps_a_captured_linear_value() {
    // 部分適用した `>>` は、それまでの引数を捕まえる (docs/spec/types.md の「関数型」)。intrinsic の関数の Kind の
    // スキームが、本体のない関数の空のスキームで上書きされると、この誤りが通ってしまう
    let text = "twice : File -> <IO> Unit\ntwice h =\n  let g = (fn u -> close h) >> (fn u -> u)\n  g ()\n  g ()";
    let checked = eml_test_support::check(text);
    assert_eq!(
        eml_test_support::short(&checked.files, &checked.diagnostics),
        ["E3002 5:3 `g` must be used exactly once, but it is used more than once"]
    );
}

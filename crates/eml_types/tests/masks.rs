//! 呼び出しごとの `mask` の記録 (docs/spec/types.md の「推論」)。rigid な row 変数の手前に余ったラベルを、矢印ごとに
//! 記録する。

use eml_test_support::{Checked, check, short_text};

/// 入口のモジュールの関数 `name` の本体の `mask` を、式の位置の順に `<行:列>#<矢印> [<エフェクト>, …]` の形で出す。
fn masks(text: &str, name: &str) -> String {
    let checked: Checked = check(text);
    assert_eq!(short_text(&checked.files, &checked.diagnostics), "");
    let program = &checked.program;
    let (id, _) = program
        .functions()
        .find(|(id, function)| program.modules[id.module].name == "Main" && function.name == name)
        .unwrap();
    let body = program.body(id).unwrap();
    let file = program.file(id.module);
    let mut rows: Vec<_> = checked.typed.bodies[id]
        .masks
        .iter()
        .map(|(&(expr, arrow), effects)| {
            let start = body.exprs[expr].range.start();
            let names: Vec<&str> = effects
                .iter()
                .map(|&effect| program.names.effect(effect))
                .collect();
            let at = checked.files.line_col(file, start);
            (
                (start, arrow),
                format!("{at}#{arrow} [{}]\n", names.join(", ")),
            )
        })
        .collect();
    rows.sort_by_key(|&(key, _)| key);
    rows.into_iter().map(|(_, row)| row).collect()
}

const STATE: &str = "\
effect State s where
  get : Unit -> s
  put : s -> Unit

";

#[test]
fn a_callback_skips_the_labels_before_its_row_variable() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb =\n  let n = get ()\n  cb ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"8:3#0 [State]");
}

#[test]
fn io_before_the_row_variable_is_not_masked() {
    let text = "run : (Unit -> <e> a) -> <IO | e> a\nrun cb =\n  println \"x\"\n  cb ()\n";
    insta::assert_snapshot!(masks(text, "run"), @"");
}

#[test]
fn a_label_twice_is_masked_twice() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int, State String | e> a\nrun cb = cb ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"6:10#0 [State, State]");
}

#[test]
fn a_lambda_body_masks_like_a_function_body() {
    // `get ()` でラムダの row の先頭に `State Int` が入ってから `cb ()` を呼ぶので、`mask` は本体の `cb ()` に付き、外側の呼び出しには付かない
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb = (fn () -> let n = get () in cb ()) ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"6:38#0 [State]");
}

#[test]
fn an_unconstrained_lambda_is_masked_at_its_call() {
    // ラムダの row は本体の `cb ()` で `<e>` に決まるので、`mask` は外側の呼び出しに付く。ラムダの中で飛ばしても外側で飛ばしても、届く handler は同じである
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb = (fn () -> cb ()) ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"6:10#0 [State]");
}

#[test]
fn an_exact_row_needs_no_mask() {
    let text = "run : (Unit -> <e> a) -> <e> a\nrun cb = cb ()\n";
    insta::assert_snapshot!(masks(text, "run"), @"");
}

#[test]
fn only_the_arrow_that_needs_it_is_masked() {
    let text = format!(
        "{STATE}h : (Int -> <e> Int -> <State Int | e> Int) -> <State Int | e> Int\nh f = f 1 2\n"
    );
    insta::assert_snapshot!(masks(&text, "h"), @"6:7#0 [State]");
}

//! 呼び出しごとの `mask` の記録 (docs/spec/types.md の「推論」)。rigid な row 変数の手前に余ったラベルを、矢印ごとに
//! エフェクトごとの項目 (残す数 n、飛ばす数 m) として記録する。

use eml_test_support::{Checked, check, short_text};

/// 入口のモジュールの関数 `name` の本体の `mask` を、式の位置の順に `<行:列>#<矢印> [<項目>, …]` の形で出す。項目は
/// IR のテキストと同じく、エフェクトの名前の後に、n が 0 でなければ `@n` を、m が 1 でなければ `*m` を続ける。
fn masks(text: &str, name: &str) -> String {
    let checked: Checked = check(text);
    assert_eq!(short_text(checked.files(), &checked.diagnostics), "");
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
        .map(|(&(expr, arrow), items)| {
            let start = body.exprs[expr].range.start();
            let names: Vec<String> = items
                .iter()
                .map(|item| {
                    let mut name = program.names.effect(item.effect).to_string();
                    if item.keep != 0 {
                        name.push_str(&format!("@{}", item.keep));
                    }
                    if item.skip != 1 {
                        name.push_str(&format!("*{}", item.skip));
                    }
                    name
                })
                .collect();
            let at = checked.files().line_col(file, start);
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
fn a_continuation_call_inside_an_inner_handle_skips_its_label() {
    // `k 1` は内側の handle の本体にあり、今の row `<Log | e>` の `Log` が `k` の row `<e>` に余る。`mask` は
    // `k 1` の式の矢印 0 に付く。`log` の節の `k2 ()` は外側の row で動くので、`mask` が付かない
    let text = "\
effect Ask where
  ask : Unit -> Int

effect Log where
  log : String -> Unit

f : (Unit -> <Ask | e> Int) -> <e> Int
f action =
  handle action () with
    | ctl ask () k ->
        handle k 1 with
          | ctl log m k2 ->
              k2 ()
";
    insta::assert_snapshot!(masks(text, "f"), @"11:16#0 [Log]");
}

#[test]
fn a_call_of_a_continuation_with_a_state_is_masked_at_its_last_arrow() {
    // 状態のある handler の `k 1 st` は、最初の矢印の row が空なので何も起こさない。`Log` は最後の矢印 1 の row
    // `<e>` に余るので、`mask` は矢印 1 に付く
    let text = "\
effect Ask where
  ask : Unit -> Int

effect Log where
  log : String -> Unit

f : (Unit -> <Ask | e> Int) -> <e> Int
f action =
  handle action () from 0 with
    | ctl ask () k st ->
        handle k 1 st with
          | ctl log m k2 -> k2 ()
    | return x _ -> x
";
    insta::assert_snapshot!(masks(text, "f"), @"11:16#1 [Log]");
}

#[test]
fn a_label_twice_is_masked_twice() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int, State String | e> a\nrun cb = cb ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"6:10#0 [State*2]");
}

#[test]
fn a_lambda_body_masks_like_a_function_body() {
    // `get ()` でラムダの row の先頭に `State Int` が入ってから `cb ()` を呼ぶので、`mask` は本体の `cb ()` に付き、
    // 外側の呼び出しには付かない
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb = (fn () -> let n = get () in cb ()) ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"6:38#0 [State]");
}

#[test]
fn an_unconstrained_lambda_is_masked_at_its_call() {
    // ラムダの row は本体の `cb ()` で `<e>` に決まるので、`mask` は外側の呼び出しに付く。ラムダの中で飛ばしても
    // 外側で飛ばしても、届く handler は同じである
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

#[test]
fn a_callee_that_performs_the_masked_effect_itself_keeps_its_own_label() {
    // `both` の row `<Log | e>` の `Log` は今の row の1つ目の `Log` と対になり、2つ目の `Log` が rest に余る。
    // `both` 自身の `log` は1つ目の handler に届き、`e` を通る `Log` は2つ目を飛ばす (docs/spec/effects.md の「健全性」)
    let text = "\
effect Log where
  log : String -> Unit

both : (Unit -> <e> a) -> <Log | e> a
both action =
  log \"x\"
  action ()

outer : (Unit -> <e> a) -> <Log, Log | e> a
outer action = both action
";
    insta::assert_snapshot!(masks(text, "outer"), @"10:16#0 [Log@1]");
}

#[test]
fn a_transforming_handler_keeps_the_producers_own_label() {
    // handle の本体の row は `<Yield Int, Yield String | e>` で、`g` の `Yield Int` が1つ目と対になる。`g` の
    // `e` を通る `Yield` は、`render` の handler を残し、`Yield String` の handler を飛ばす
    let text = "\
effect Yield a where
  yield : a -> Unit

render : (Unit -> <Yield Int | e> Unit) -> <Yield String | e> Unit
render g =
  handle g () with
    | ctl yield n k ->
      yield (show n)
      k ()
";
    insta::assert_snapshot!(masks(text, "render"), @"6:10#0 [Yield@1]");
}

#[test]
fn a_label_the_callee_names_is_paired_and_only_the_other_effect_is_masked() {
    let text = format!(
        "{STATE}effect Log where\n  log : String -> Unit\n\nrun : (Unit -> <Log | e> a) -> <Log, State Int | e> a\nrun cb = cb ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"9:10#0 [State]");
}

#[test]
fn two_effects_are_masked_with_their_own_counts() {
    // rest は `[State Int, Log]` なので、項目は rest に現れた順に `State`、`Log@1` になる
    let text = format!(
        "{STATE}effect Log where\n  log : String -> Unit\n\nrun : (Unit -> <Log | e> a) -> <Log, State Int, Log | e> a\nrun cb = cb ()\n"
    );
    insta::assert_snapshot!(masks(&text, "run"), @"9:10#0 [State, Log@1]");
}

#[test]
fn a_stateful_continuation_keeps_its_own_label_at_its_last_arrow() {
    // `tests/ui/run/effects/mask_behind_state.em` の `twice` である。節の中の handle の本体の row は `<Log, Log | e>`
    // で、`k` の最後の矢印の row は handle の外側の `<Log | e>` である。`k () (n + 1)` は最後の矢印 1 に `Log@1` を持ち、
    // row の空な最初の矢印 0 には `mask` がない。`action ()` も handle の本体で `<Log | e>` を呼ぶので `Log@1` を持つ。
    // 節の row `<Log | e>` で `<e>` を呼ぶ `cb ()` は、最初の `Log` を飛ばす
    let text = "\
effect Log where
  log : String -> Unit

twice : (Unit -> <e> Unit) -> (Unit -> <Log | e> a) -> <Log | e> a
twice cb action =
  handle action () from 1 with
    | ctl log s k n ->
      log \"outer #\\{n} sees \\{s}\"
      cb ()
      handle k () (n + 1) with
        | ctl log t k2 ->
          log \"inner sees \\{t}\"
          k2 ()
    | return x _ -> x
";
    insta::assert_snapshot!(masks(text, "twice"), @r"
    6:10#0 [Log@1]
    9:7#0 [Log]
    10:14#1 [Log@1]
    ");
}

#[test]
fn a_continuation_under_a_handler_of_its_own_effect_keeps_its_own_label() {
    // `tests/ui/run/effects/mask_behind_continuation.em` の `twice` である。`k ()` は節の中の `Log` の handler を残し、
    // 外側の `Log` を飛ばす。`cb ()` は節の row の最初の `Log` を飛ばす
    let text = "\
effect Log where
  log : String -> Unit

twice : (Unit -> <e> Unit) -> (Unit -> <Log | e> a) -> <Log | e> a
twice cb action =
  handle action () with
    | ctl log s k ->
      log \"outer sees \\{s}\"
      cb ()
      handle k () with
        | ctl log t k2 ->
          log \"inner sees \\{t}\"
          k2 ()
";
    insta::assert_snapshot!(masks(text, "twice"), @r"
    6:10#0 [Log@1]
    9:7#0 [Log]
    10:14#0 [Log@1]
    ");
}

#[test]
fn a_multi_continuation_resumed_twice_keeps_its_own_label_at_both_calls() {
    // `tests/ui/run/effects/mask_behind_multi.em` の `both` である。`IO` は extern のエフェクトなので項目にならない。
    // `cb ()` は節の row の最初の `Choice` を飛ばす
    let text = "\
effect Choice where
  multi choose : String -> Bool

both : (Unit -> <e> Unit) -> (Unit -> <Choice | e> Unit) -> <Choice, IO | e> Unit
both cb action =
  handle action () with
    | ctl choose q k ->
      let _ = choose \"outer sees \\{q}\"
      cb ()
      handle (let _ = k True in k False) with
        | ctl choose t k2 ->
          println \"inner answers \\{t}\"
          k2 False
";
    insta::assert_snapshot!(masks(text, "both"), @r"
    6:10#0 [Choice@1]
    9:7#0 [Choice]
    10:23#0 [Choice@1]
    10:33#0 [Choice@1]
    ");
}

#[test]
fn a_callee_under_two_handlers_of_its_own_effect_keeps_its_own_label() {
    // `tests/ui/run/effects/mask_behind_handles.em` の `outer` である。rest の `Log` は、シグネチャの row でなく、
    // 外側の handle が足したラベルである
    let text = "\
effect Log where
  log : String -> Unit

both : (Unit -> <e> a) -> <Log | e> a
both action =
  log \"x\"
  action ()

outer : (Unit -> <IO | e> a) -> <IO | e> a
outer action =
  handle (handle both action with
            | ctl log s k ->
              println \"inner: \\{s}\"
              k ()) with
    | ctl log s k ->
      println \"middle: \\{s}\"
      k ()
";
    insta::assert_snapshot!(masks(text, "outer"), @"11:18#0 [Log@1]");
}

#[test]
fn a_self_call_under_its_own_handler_has_no_mask() {
    // 自分への呼び出しの包含は、外からの `go n` の包含が `go` の row を閉じた後に確かめる。閉じた row は rigid な末尾を
    // 持たないので `mask` が要らない
    let text = "effect Ask where\n  ask : Unit -> Int\n\ncount : Int -> <IO> Int\ncount n =\n  let go k =\n    if k == 0 then 0\n    else handle ask () + go (k - 1) with | ctl ask () r -> r 1\n  go n";
    assert_eq!(masks(text, "count"), "");
}

#[test]
fn a_rigid_callee_with_a_label_of_the_masked_effect_keeps_it() {
    // `h` の row が明示した `Log` は、今の row の最初の `Log` と対になって残り、2つ目の `Log` を飛ばす
    let text = "effect Log where\n  log : String -> Unit\n\nf : (Unit -> <Log | e> Int) -> <Log, Log | e> Int\nf h = h ()\n";
    insta::assert_snapshot!(masks(text, "f"), @"5:7#0 [Log@1]");
}

const L: &str = "effect L where\n  op : Unit -> Int\n\n";

#[test]
fn resuming_under_a_handler_of_the_same_effect() {
    // `k` の row と今の row が同じ開いた末尾を共有する形は、今と同じく誤りになる。rigid な末尾なら `mask` で通る
    let in_lambda = format!(
        "{L}in_lambda : Unit -> <L> Int\nin_lambda () =\n  let f = fn () ->\n    op () + (handle op () with\n      | ctl op () k -> handle k 1 with\n          | fn op () -> 2)\n  f ()\n"
    );
    let checked = check(&in_lambda);
    let diagnostics = eml_test_support::short(checked.files(), &checked.diagnostics);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0].starts_with("E2002")
            && diagnostics[0].ends_with("`k` performs `L`, which this lambda does not allow"),
        "{diagnostics:?}"
    );
    let in_signature = format!(
        "{L}in_signature : (Unit -> <e> Int) -> <L | e> Int\nin_signature h =\n  op () + (handle op () with\n    | ctl op () k -> handle k (h ()) with\n        | fn op () -> 2)\n"
    );
    insta::assert_snapshot!(masks(&in_signature, "in_signature"), @"
    7:29#0 [L@1]
    7:32#0 [L*2]
    ");
}

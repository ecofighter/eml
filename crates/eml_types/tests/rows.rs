//! エフェクトの row の検査 (E2002、row 変数、未定義のエフェクト)。

use eml_test_support::{check, short_text};

use crate::common::{check_full, check_short, check_text};

#[test]
fn effects_must_be_in_the_signature() {
    let text = "greet : String -> Unit\ngreet name = println name\n\nshout : String -> <IO> Unit\nshout name = println (name ++ \"!\")";
    insta::assert_snapshot!(check_text(text), @r"
    greet : String -> Unit
      name#0 : String
    shout : String -> <IO> Unit
      name#0 : String
    ---
    E2002 2:14 `println` performs `IO`, which the signature of `greet` does not allow
      2:14 this call performs `IO`
      1:9 the row of this signature does not include it
      help: add `IO` to the row of the signature of `greet`, as in `-> <IO> ...`
    ");
}

#[test]
fn main_with_an_erroneous_row_is_not_reported_again() {
    insta::assert_snapshot!(check_text("main : Unit -> <Console> Unit\nmain () = ()"), @r"
        main : Unit -> <{error}> Unit
        ---
        E1002 1:17 cannot find effect `Console`
          1:17 not found in this scope
        ");
}

#[test]
fn a_call_reports_a_missing_effect_once() {
    let text =
        "f : Int -> <IO> Int -> <IO> Unit\nf a b = println \"x\"\n\ng : Int -> Unit\ng n = f 1 2";
    insta::assert_snapshot!(check_text(text), @"
    f : Int -> <IO> Int -> <IO> Unit
      a#0 : Int
      b#1 : Int
    g : Int -> Unit
      n#0 : Int
    ---
    E2002 5:7 `f` performs `IO`, which the signature of `g` does not allow
      5:7 this call performs `IO`
      4:5 the row of this signature does not include it
      help: add `IO` to the row of the signature of `g`, as in `-> <IO> ...`
    ");
}

#[test]
fn a_value_cannot_perform_effects() {
    let text = "v : Unit\nv = println \"x\"";
    insta::assert_snapshot!(check_text(text), @"
    v : Unit
    ---
    E2002 2:5 `println` performs `IO`, which the signature of `v` does not allow
      2:5 this call performs `IO`
      1:5 the row of this signature does not include it
      help: `v` takes no parameters, so it cannot perform `IO`; make it a function taking `()`, as in `v : Unit -> <IO> ...` with `v () = ...`
    ");
}

#[test]
fn row_variables_pass_effects_through() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nshout : String -> <IO> Unit\nshout s = apply println s\n\nquiet : String -> Unit\nquiet s = apply println s";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (a -> <e> b) -> a -> <e> b
      f#0 : a -> <e> b
      x#1 : a
    shout : String -> <IO> Unit
      s#0 : String
    quiet : String -> Unit
      s#0 : String
    ---
    E2002 8:11 `apply` performs `IO`, which the signature of `quiet` does not allow
      8:11 this call performs `IO`
      7:9 the row of this signature does not include it
      help: add `IO` to the row of the signature of `quiet`, as in `-> <IO> ...`
    ");
}

#[test]
fn a_rigid_row_variable_must_be_in_the_ambient_row() {
    let text = "run : (Unit -> <e> Unit) -> Unit\nrun f = f ()";
    insta::assert_snapshot!(check_text(text), @r"
    run : (Unit -> <e> Unit) -> Unit
      f#0 : Unit -> <e> Unit
    ---
    E2002 2:9 `f` performs `e`, which the signature of `run` does not allow
      2:9 this call performs `e`
      1:7 the row of this signature does not include it
      help: add `e` to the row of the signature of `run`, as in `-> <e> ...`
    ");
}

#[test]
fn a_lambda_cannot_perform_effects_its_expected_type_does_not_allow() {
    let text = "each : (String -> Unit) -> Unit\neach f = f \"a\"\n\nmain : Unit -> <IO> Unit\nmain () = each (fn s -> println s)";
    insta::assert_snapshot!(check_text(text), @r"
    each : (String -> Unit) -> Unit
      f#0 : String -> Unit
    main : Unit -> <IO> Unit
      s#0 : String
    ---
    E2002 5:25 `println` performs `IO`, which this lambda does not allow
      5:25 this call performs `IO`
      5:11 argument 1 of `each` does not allow it
    ");
}

#[test]
fn an_undefined_effect_row_is_fresh_at_each_call() {
    let text = "run : (Unit -> <Console> Unit) -> <Console> Unit\nrun f = f ()\n\nloud : Unit -> <IO> Unit\nloud () = run (fn () -> println \"x\")\n\nquiet : Unit -> Unit\nquiet () = run (fn () -> ())";
    insta::assert_snapshot!(check_text(text), @"
    run : (Unit -> <{error}> Unit) -> <{error}> Unit
      f#0 : Unit -> <{error}> Unit
    loud : Unit -> <IO> Unit
    quiet : Unit -> Unit
    ---
    E1002 1:17 cannot find effect `Console`
      1:17 not found in this scope
    E1002 1:36 cannot find effect `Console`
      1:36 not found in this scope
    ");
}

#[test]
fn an_undefined_effect_row_does_not_pass_the_body_effects_to_callers() {
    // 未定義のエフェクトの row はどのエフェクトも受け入れるが、本体のエフェクトを取り込んで呼び出し側に伝えない
    // (docs/spec/types.md の「エラーの扱い」)
    let text = "g : Unit -> <Console> Unit\ng () = println \"x\"\n\nh : Unit -> Unit\nh () = g ()";
    insta::assert_snapshot!(check_text(text), @"
    g : Unit -> <{error}> Unit
    h : Unit -> Unit
    ---
    E1002 1:14 cannot find effect `Console`
      1:14 not found in this scope
    ");
}

#[test]
fn a_missing_effect_points_at_the_arrow_of_the_body() {
    // 本体のエフェクトは、引数の数だけ矢印をたどった最後の矢印の row に入るので、その矢印を指す
    // (docs/implementation/diagnostics.md の E2002)
    let text = "f : Int -> Int -> Unit\nf a b = println \"x\"";
    insta::assert_snapshot!(check_text(text), @"
    f : Int -> Int -> Unit
      a#0 : Int
      b#1 : Int
    ---
    E2002 2:9 `println` performs `IO`, which the signature of `f` does not allow
      2:9 this call performs `IO`
      1:12 the row of this signature does not include it
      help: add `IO` to the row of the signature of `f`, as in `-> <IO> ...`
    ");
}

#[test]
fn an_undefined_effect_does_not_hide_an_unrelated_missing_effect() {
    // `l` の row は `bad` の引数と単一化して末尾 `Error` になるが、`l` が起こす `IO` は既知のエフェクトなので、純粋な `p`
    // の中で呼べば E2002 になる (docs/spec/types.md の「エラーの扱い」は `Error` が関わる制約だけを黙らせる)
    let text = "bad : (Unit -> <Console> Unit) -> Unit\nbad f = ()\n\np : Unit -> Unit\np () =\n  let l = fn () -> println \"x\"\n  bad l\n  l ()";
    let out = check_text(text);
    assert!(
        out.contains("E1002 1:17 cannot find effect `Console`"),
        "{out}"
    );
    assert!(out.contains("E2002 8:3 `l` performs `IO`"), "{out}");
}

#[test]
fn an_unknown_qualifier_in_a_row_is_reported_once() {
    // 修飾子の誤りの row は、未定義のエフェクトの row と同じくどのエフェクトも受け入れる。本体の `IO` も、呼び出す `main`
    // も誤りにならない (docs/spec/types.md の「エラーの扱い」)
    let text = "f : Unit -> <Log.State Int> Unit\nf () = println \"x\"\n\nmain : Unit -> <IO> Unit\nmain () = f ()";
    insta::assert_snapshot!(check_text(text), @"
    f : Unit -> <{error}> Unit
    main : Unit -> <IO> Unit
    ---
    E1031 1:14 unknown module qualifier `Log`
      1:14 no import gives this qualifier
    ");
}

/// `IO` は、row の中で重なっても1つとして扱う (docs/spec/types.md の「推論」)。`<IO | e>` に
/// `e := <IO>` が入っても、`<IO>` の `main` から呼べる。
#[test]
fn io_from_a_row_variable_does_not_count_twice() {
    let text = "\
effect Fail where
  never fail : String -> a

try : (Unit -> <Fail | e> a) -> <IO | e> Int
try action =
  handle action () with
    | ctl fail msg -> 0
    | return x -> 1

work : Unit -> <Fail, IO> Unit
work () = println \"x\"

main : Unit -> <IO> Unit
main () =
  let n = try (fn () -> work ())
  println (show n)
";
    let checked = check(text);
    assert_eq!(short_text(checked.files(), &checked.diagnostics), "");
}

#[test]
fn a_row_reached_only_from_a_capture_closes() {
    // `g` を呼ばないので、捕まえた `h` の row にはどの引数からもエフェクトが入らない。`h x` と `go 1` の包含が、呼び出し先の
    // `h` と `go` の row を `<>` に閉じる。呼ばれない `g` の row は開いたまま残る
    let text = "f : Unit -> Int\nf () =\n  let g = fn h -> (let go x = h x in go 1)\n  0";
    insta::assert_snapshot!(check_text(text), @"
    f : Unit -> Int
      h#0 : Int -> _
      go#1 : Int -> _
      x#2 : Int
      g#3 : (Int -> _) -> <_> _
    ");
}

#[test]
fn a_deferred_self_call_reports_against_the_row_at_the_call() {
    // 自分への呼び出しの包含は本体の後で確かめるが、診断は呼び出しの位置の今の row (純粋なラムダ) を指す
    let text = "effect Ask where\n  ask : Unit -> Int\n\npure_apply : (Unit -> Int) -> Int\npure_apply f = f ()\n\ng : Int -> <Ask> Int\ng n =\n  let go k = if k == 0 then ask () else pure_apply (fn () -> go (k - 1))\n  go n";
    insta::assert_snapshot!(check_text(text), @"
    ask : Unit -> <Ask> Int
    pure_apply : (Unit -> Int) -> Int
      f#0 : Unit -> Int
    g : Int -> <Ask> Int
      n#0 : Int
      go#1 : Int -> <Ask> Int
      k#2 : Int
    ---
    E2002 9:62 `go` performs `Ask`, which this lambda does not allow
      9:62 this call performs `Ask`
      9:41 argument 1 of `pure_apply` does not allow it
    ");
}

#[test]
fn a_row_reached_only_from_a_parameter_closes() {
    // `app` を呼ばないので、引数 `g` の row にはどの引数からもエフェクトが入らない。`g ()` の包含が `g` の row を `<>` に
    // 閉じる。呼ばれない `app` の row は開いたまま残る
    let text = "f : Unit -> Int\nf () =\n  let app g = g ()\n  0";
    insta::assert_snapshot!(check_text(text), @"
    f : Unit -> Int
      app#0 : (Unit -> _) -> <_> _
      g#1 : Unit -> _
    ");
}

#[test]
fn a_rigid_tail_in_a_local_function_does_not_depend_on_call_order() {
    // 引数 `h` の row の rigid な `e` は、`ask` のラベルを足し終えてから `go` の row の末尾に束縛するので、呼ぶ順序に
    // よらず通る (docs/spec/types.md の「推論」)
    let head =
        "effect Ask where\n  ask : Unit -> Int\n\nf : (Unit -> <e> Int) -> <Ask | e> Int\nf h =\n";
    for go in ["h () + ask () + k", "ask () + h () + k"] {
        let out = check_text(&format!("{head}  let go k = {go}\n  go 1"));
        assert!(!out.contains("E2"), "{out}");
    }
}

#[test]
fn a_let_bound_lambda_is_called_inside_and_outside_a_handler_in_any_order() {
    // 呼び出しは row の包含なので、`g` の row は最初の呼び出しの今の row に決まらない
    let head = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> <IO> Int\nf () =\n  let g = fn () -> 1\n";
    let handled = "(handle g () + ask () with | fn ask () -> 2)";
    for body in [format!("{handled} + g ()"), format!("g () + {handled}")] {
        let out = check_text(&format!("{head}  {body}"));
        assert!(!out.contains("---"), "{out}");
    }
}

#[test]
fn row_arguments_unify_as_rows() {
    let text = "data Iter <e> a =\n  | Done\n  | More a (once Unit -> <e> Iter <e> a)\n\nnext : Iter <e> a -> <e> Option a\nnext it =\n  match it with\n    | Done -> None\n    | More x k ->\n        drop (k ())\n        Some x\n\nuse_io : Iter <IO> Int -> <IO> Option Int\nuse_io it = next it";
    insta::assert_snapshot!(check_text(text), @"
    next : Iter <e> a -> <e> Option a
      kinds: carry(a, <e>)
      it#0 : Iter <e> a
      x#1 : a
      k#2 : once Unit -> <e> Iter <e> a
    use_io : Iter <IO> Int -> <IO> Option Int
      it#0 : Iter <IO> Int
    ");
}

#[test]
fn row_arguments_are_displayed_in_declaration_order() {
    let text =
        "data P a <e> b =\n  | P a b (Unit -> <e> Unit)\n\nf : P Int <IO> Bool -> Int\nf p = p";
    insta::assert_snapshot!(check_text(text), @"
    f : P Int <IO> Bool -> Int
      p#0 : P Int <IO> Bool
    ---
    E2001 5:7 mismatched types
      5:7 expected `Int`, found `P Int <IO> Bool`
      4:5 expected because of the signature of `f`
    ");
}

#[test]
fn mismatched_row_arguments_are_errors() {
    let text = "data Iter <e> a =\n  | Done\n\nf : Iter <IO> Int -> Iter <> Int\nf x = x";
    assert_eq!(
        check_short(text)
            .iter()
            .map(|d| &d[..5])
            .collect::<Vec<_>>(),
        ["E2001"]
    );
}

#[test]
fn a_constraint_on_a_type_with_only_row_arguments_is_parenthesized() {
    let text = "data G <e> =\n  | G Int\n\nf : G <IO> -> String\nf g = show g";
    let report = check_full(text);
    assert!(report.starts_with("E2006"), "{report}");
    assert!(report.contains("`Show (G <IO>)`"), "{report}");
}

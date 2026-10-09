//! エフェクトの row の検査 (E2002、row 変数、未定義のエフェクト)。

use eml_test_support::{check, short_text};

use crate::common::check_text;

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

/// handle できない `IO` は、row の中で重なっても1つとして扱う (docs/spec/types.md の「推論」)。`<IO | e>` に
/// `e := <IO>` が入っても、`<IO>` の `main` から呼べる。
#[test]
fn io_from_a_row_variable_does_not_count_twice() {
    let text = "\
effect Fail where
  never fail : String -> a

try : (Unit -> <Fail | e> a) -> <IO | e> Int
try action =
  handle action () with
    | fail msg -> 0
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

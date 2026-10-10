//! リストの式の型検査と持ち越し (docs/spec/expressions.md の「リスト」、docs/implementation/architecture.md の
//! 「`eml_types` の内部」)。

use eml_test_support::{check, full};

fn diagnostics(text: &str) -> String {
    let checked = check(text);
    full(checked.files(), &checked.diagnostics)
}

#[test]
fn an_element_is_checked_against_the_expected_element_type() {
    insta::assert_snapshot!(diagnostics("xs : List String\nxs = [\"a\", 1]"), @"
    E2001 2:12 mismatched types
      2:12 expected `String`, found `Int`
      1:6 expected because of the signature of `xs`
    ");
}

#[test]
fn later_elements_are_checked_against_the_first() {
    insta::assert_snapshot!(diagnostics("f : Unit -> Int\nf () =\n  let xs = [\"a\", 1]\n  0"), @"
    E2001 3:18 mismatched types
      3:18 expected `String`, found `Int`
      3:13 the first element has this type
    ");
}

/// 評価済みの要素は最初の要素1つで代表させるので、持ち越しの誤りは最初の要素の位置に出る。
#[test]
fn a_carried_list_prefix_is_reported_at_the_first_element() {
    let text = [
        "effect Choice where",
        "  multi choose : Unit -> Bool",
        "",
        "f : Unit -> <Choice, IO> List Fs.File",
        "f () = [Fs.open \"a\", Fs.open \"b\", if choose () then Fs.open \"c\" else Fs.open \"d\"]",
    ]
    .join("\n");
    insta::assert_snapshot!(diagnostics(&text), @"
    E3006 5:38 a linear value must be used exactly once, but it is kept alive across a call that may resume more than once
      5:38 this call may perform `choose`, a `multi` operation
      5:9 this value is kept alive across the call
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using the value before this call
    ");
}

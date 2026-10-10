//! 補間の型検査と持ち越し (docs/spec/expressions.md の「補間」)。

use eml_test_support::{check, full};

fn diagnostics(text: &str) -> String {
    let checked = check(text);
    full(checked.files(), &checked.diagnostics)
}

#[test]
fn a_hole_needs_show() {
    let text = "data T = | T\n\nf : T -> String\nf t = \"\\{t}\"";
    let shown = diagnostics(text);
    assert!(shown.starts_with("E2006 4:8"), "{shown}");
    assert!(shown.contains("`display` requires `Show T`"), "{shown}");
}

#[test]
fn a_polymorphic_hole_takes_the_constraint_from_the_signature() {
    assert_eq!(
        diagnostics("f : Show a => a -> String\nf x = \"<\\{x}>\""),
        ""
    );
    let shown = diagnostics("f : a -> String\nf x = \"<\\{x}>\"");
    assert!(shown.contains("`Show a`"), "{shown}");
}

#[test]
fn a_linear_value_held_across_a_multi_hole_is_rejected() {
    let text = [
        "effect Choice where",
        "  multi choose : Unit -> Bool",
        "",
        "f : Unit -> <Choice, IO> String",
        "f () =",
        "  let file = Fs.open \"a\"",
        "  let s = \"\\{choose ()}\"",
        "  Fs.close file",
        "  s",
    ]
    .join("\n");
    insta::assert_snapshot!(diagnostics(&text), @"
    E3006 7:14 `file` must be used exactly once, but it is kept alive across a call that may resume more than once
      7:14 this call may perform `choose`, a `multi` operation
      6:7 `file` is bound here
      2:9 `choose` is declared `multi` here
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
      help: finish using `file` before this call
    ");
    let once = text.replace("multi choose", "once choose");
    assert_eq!(diagnostics(&once), "");
}

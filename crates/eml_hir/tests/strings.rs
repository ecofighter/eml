//! 文字列の補間 (docs/spec/expressions.md の「補間」)。

use crate::common::lower_text;

#[test]
fn holes_call_the_prelude_display() {
    let text = "f : Int -> String\nf n = \"a\\\"\\{n}b\\{n + 1}\"";
    insta::assert_snapshot!(lower_text(text), @r#"
    f : Int -> String
    f n#0 = "a\"\{(@Prelude.display n#0)}b\{(@Prelude.display (@Prelude.+ n#0 1))}"
    "#);
}

#[test]
fn a_user_display_does_not_change_interpolation() {
    let text =
        "display : Int -> String\ndisplay _ = \"mine\"\n\nf : Int -> String\nf n = \"\\{n}\"";
    let shown = lower_text(text);
    assert!(
        shown.contains(r#"f n#0 = "\{(@Prelude.display n#0)}""#),
        "{shown}"
    );
}

#[test]
fn a_string_without_holes_is_a_literal() {
    insta::assert_snapshot!(lower_text("f : Unit -> String\nf () = \"a\\n\""), @r#"
    f : Unit -> String
    f () = "a\n"
    "#);
}

#[test]
fn a_command_literal_calls_the_prelude_cmd_new() {
    let text = "f : String -> List String -> Cmd\nf x xs = `ls -l\\{x} \\{..xs} ''`";
    insta::assert_snapshot!(lower_text(text), @r#"
    f : String -> List String -> Cmd
    f x#0 xs#1 = (@Prelude.cmd_new "ls" (Prelude.:: "-l\{(x#0 : String)}" (@Prelude.cmd_spread xs#1 (Prelude.:: "" Prelude.Nil))))
    "#);
}

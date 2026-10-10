//! レコードの作る式とパターンの型検査 (docs/superpowers/specs/2026-10-10-s6c-records-design.md の「型検査」)。

use eml_test_support::{check, full};

fn diagnostics(text: &str) -> String {
    let checked = check(text);
    full(checked.files(), &checked.diagnostics)
}

#[test]
fn a_field_mismatch_names_the_field() {
    let text = "data P = | P { name : String, age : Int }\n\nf : Unit -> P\nf () = P { name = 3, age = 1 }";
    insta::assert_snapshot!(diagnostics(text), @"
    E2001 4:19 mismatched types
      4:19 expected `String`, found `Int`
      1:16 field `name` of `P`
    ");
}

#[test]
fn an_omitted_linear_field_is_discarded() {
    let text = "data Job = | Job { name : String, log : Fs.File }\n\nf : Job -> String\nf j = match j with\n  | Job { name } -> name";
    insta::assert_snapshot!(diagnostics(text), @"
    E3004 5:5 the field `log` is discarded, but it holds a linear value
      5:5 this pattern leaves out `log`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: bind `log` in the pattern and pass it to `drop`
    ");
}

#[test]
fn construction_instantiates_type_arguments() {
    let text = "data Box a = | Box { item : a, count : Int }\n\nf : Unit -> Box String\nf () = Box { count = 1, item = \"a\" }\n\ng : Box String -> String\ng b = match b with\n  | Box { item } -> item";
    assert_eq!(diagnostics(text), "");
}

//! レコードの作る式とパターンの型検査 (docs/spec/types.md の「フィールドの解決」)。

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

const PERSON: &str = "data Person = | Person { name : String, age : Int }\n\n";

#[test]
fn projection_and_update_resolve_by_type() {
    let text = format!(
        "{PERSON}f : Person -> (String, Person)\nf p = (p.name, {{ p | age = p.age + 1 }})"
    );
    assert_eq!(diagnostics(&text), "");
}

#[test]
fn projection_needs_a_known_type() {
    let text = format!("{PERSON}f : Unit -> Int\nf () =\n  let g = fn p -> p.age\n  0");
    insta::assert_snapshot!(diagnostics(&text), @"
    E2013 5:19 the type of this value must be known to access its field `age`
      5:19 the type of this value is not known here
      help: annotate the type of the parameter, as in `fn (p : T) -> …`
    ");
}

#[test]
fn no_such_field() {
    let text = "data S = | A { x : Int } | B { x : Int }\n\nf : S -> Int\nf s = s.x\n\ng : (Int, Int) -> Int\ng t = t.2\n\nh : Int -> Int\nh n = n.x";
    insta::assert_snapshot!(diagnostics(text), @"
    E2014 4:9 no field `x` on type `S`
      4:9 unknown field
      note: `S` has more than one constructor
      help: take the value apart with `match`
    E2014 7:9 no field `2` on type `(Int, Int)`
      7:9 unknown field
      help: the elements of this tuple are numbered from `0` to `1`
    E2014 10:9 no field `x` on type `Int`
      10:9 unknown field
    ");
}

#[test]
fn same_field_name_in_two_records() {
    let text = "data A = | A { id : Int }\ndata B = | B { id : String }\n\nf : A -> B -> (Int, String)\nf a b = (a.id, b.id)";
    assert_eq!(diagnostics(text), "");
}

#[test]
fn projection_and_update_discard_linear_fields() {
    let text = "data Job = | Job { name : String, log : Fs.File }\n\nf : Job -> String\nf j = j.name\n\ng : Job -> Fs.File -> Job\ng j f = { j | log = f }";
    insta::assert_snapshot!(diagnostics(text), @"
    E3004 4:7 the field `log` is discarded, but it holds a linear value
      4:7 this projection leaves the field `log` behind
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: take the value apart with a record pattern and pass `log` to `drop`
    E3004 7:9 the old value of the field `log` is discarded, but it holds a linear value
      7:9 this update overwrites `log`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: take the old `log` out with a record pattern and pass it to `drop`
    ");
}

#[test]
fn a_generic_projection_constrains_the_other_fields() {
    let text = "data Box a = | Box { x : a, y : a }\n\nf : Box a -> a\nf b = b.x";
    insta::assert_snapshot!(crate::common::kinds(text, "f"), @"  kinds: a <= Unr");
}

#[test]
fn projections_instantiate_type_arguments_and_index_tuples() {
    let text = "data Box a = | Box { item : a, count : Int }\n\nf : Box String -> String\nf b = b.item\n\ng : (Int, String) -> String\ng t = t.1\n\nh : ((Int, Bool), Int) -> Bool\nh t = t.0.1";
    assert_eq!(diagnostics(text), "");
}

#[test]
fn an_unknown_type_that_is_not_a_lambda_parameter_asks_for_an_annotation() {
    let text = format!(
        "{PERSON}f : Unit -> Int\nf () =\n  let g = fn p ->\n    let q = p\n    q.age\n  0"
    );
    insta::assert_snapshot!(diagnostics(&text), @"
    E2013 7:5 the type of this value must be known to access its field `age`
      7:5 the type of this value is not known here
      help: annotate the type of the value, as in `(q : T)`
    ");
}

#[test]
fn fields_that_the_type_does_not_have() {
    let text = format!(
        "{PERSON}f : (Int, Int) -> Int\nf t = t.name\n\ng : Person -> Int\ng p = p.0\n\nh : a -> Int\nh x = x.y\n\nk : (Int -> Int) -> Int\nk f = f.y"
    );
    insta::assert_snapshot!(diagnostics(&text), @"
    E2014 4:9 no field `name` on type `(Int, Int)`
      4:9 unknown field
      help: the elements of this tuple are numbered from `0` to `1`
    E2014 7:9 no field `0` on type `Person`
      7:9 unknown field
      help: the fields are `name` and `age`
    E2014 10:9 no field `y` on type `a`
      10:9 unknown field
    E2014 13:9 no field `y` on type `Int -> Int`
      13:9 unknown field
    ");
}

#[test]
fn unit_has_no_fields() {
    // `Unit` は要素のないタプルなので、番号の範囲を示す help を作れない
    let text = "f : Unit -> Int\nf u = u.0\n\ng : Unit -> Int\ng u = u.name\n\nh : Unit -> Unit\nh u = { u | x = 1 }";
    insta::assert_snapshot!(diagnostics(text), @"
    E2014 2:9 no field `0` on type `Unit`
      2:9 unknown field
      help: `Unit` has no fields
    E2014 5:9 no field `name` on type `Unit`
      5:9 unknown field
      help: `Unit` has no fields
    E2014 8:13 no field `x` on type `Unit`
      8:13 unknown field
      help: `Unit` has no fields
    ");
}

#[test]
fn update_errors() {
    let text = format!(
        "{PERSON}f : (Int, Int) -> (Int, Int)\nf t = {{ t | x = 1 }}\n\ng : Person -> Person\ng p = {{ p | nick = \"a\" }}\n\nh : Person -> Person\nh p = {{ p | age = \"old\" }}"
    );
    insta::assert_snapshot!(diagnostics(&text), @"
    E2014 4:13 no field `x` on type `(Int, Int)`
      4:13 unknown field
      help: a tuple cannot be updated; build a new tuple instead
    E2014 7:13 no field `nick` on type `Person`
      7:13 unknown field
      help: the fields are `name` and `age`
    E2001 10:19 mismatched types
      10:19 expected `Int`, found `String`
      1:41 field `age` of `Person`
    ");
}

#[test]
fn a_projection_in_a_section_discards_without_help() {
    let text = "data Job = | Job { name : String, log : Fs.File }\n\napply : (Job -> String) -> Job -> String\napply f j = f j\n\nf : Job -> String\nf j = apply (.name) j";
    insta::assert_snapshot!(diagnostics(text), @"
    E3004 7:13 the field `log` is discarded, but it holds a linear value
      7:13 this projection leaves the field `log` behind
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_projection_uses_its_base_once() {
    let text = "data Job = | Job { name : String, log : Fs.File }\n\nf : Job -> (String, String)\nf j = (j.name, j.name)";
    insta::assert_snapshot!(diagnostics(text), @"
    E3004 4:8 the field `log` is discarded, but it holds a linear value
      4:8 this projection leaves the field `log` behind
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: take the value apart with a record pattern and pass `log` to `drop`
    E3002 4:16 `j` must be used exactly once, but it is used more than once
      4:16 used again here
      4:8 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    E3004 4:16 the field `log` is discarded, but it holds a linear value
      4:16 this projection leaves the field `log` behind
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: take the value apart with a record pattern and pass `log` to `drop`
    ");
}

const PEOPLE: &str = "data Person = | Person { name : String, age : Int }\n\nmap : (a -> b) -> List a -> List b\nmap f xs = match xs with\n  | [] -> []\n  | x :: rest -> f x :: map f rest\n\n";

#[test]
fn lambda_arguments_are_checked_after_the_others() {
    for body in [
        "map (.name) people",
        "map (fn p -> p.name) people",
        "people |> map (.name)",
        "map (fn p -> p.name ++ \"!\") people",
    ] {
        let text = format!("{PEOPLE}f : List Person -> List String\nf people = {body}");
        assert_eq!(diagnostics(&text), "", "{body}");
    }
    let text = format!("{PEOPLE}f : List Int -> List Int\nf xs = map (+ 1) xs");
    assert_eq!(diagnostics(&text), "");
}

#[test]
fn diagnostics_without_record_lambdas_do_not_change() {
    let konst = "konst : a -> b -> a\nkonst x _ = x\n\nf : Unit -> Int\nf () = konst 1 2 3";
    assert!(
        diagnostics(konst).contains("takes 2 arguments but 3 were given"),
        "{}",
        diagnostics(konst)
    );
    let apply = "apply : (a -> b) -> a -> b\napply f x = f x\n\nf : Unit -> Int\nf () = apply (fn x -> x) 1 2";
    assert!(
        diagnostics(apply).contains("takes 2 arguments but 3 were given"),
        "{}",
        diagnostics(apply)
    );
    let nested = "konst : a -> b -> a\nkonst x _ = x\n\nsame : a -> a -> a -> Int\nsame _ _ _ = 0\n\nf : Unit -> Int\nf () = same (konst 1 2 3) \"x\" 1";
    insta::assert_snapshot!(diagnostics(nested), @"
    E2001 8:24 `konst` takes 2 arguments but 3 were given
      8:24 unexpected argument
    ");
}

#[test]
fn an_arity_error_keeps_the_errors_inside_a_postponed_lambda() {
    let text =
        "g : (Int -> Int) -> Int\ng h = h 1\n\nf : Unit -> Int\nf () = g (fn x -> x ++ \"\") 2";
    insta::assert_snapshot!(diagnostics(text), @"
    E2001 5:19 mismatched types
      5:19 expected `String`, found `Int`
      5:21 argument 1 of `++`
    E2001 5:19 mismatched types
      5:19 expected `Int`, found `String`
      note: the body of a lambda must have the return type the lambda is expected to have
    E2001 5:28 `g` takes 1 argument but 2 were given
      5:28 unexpected argument
    ");
}

#[test]
fn a_typo_in_the_callee_does_not_cascade_into_the_lambda() {
    let text =
        format!("{PEOPLE}f : List Person -> List String\nf people = mapp (fn p -> p.name) people");
    let shown = diagnostics(&text);
    assert!(shown.starts_with("E1001"), "{shown}");
    assert!(!shown.contains("E2013"), "{shown}");
}

#[test]
fn use_blocks_keep_their_diagnostics() {
    let text = "with_x : (Int -> String) -> String\nwith_x k = k 1\n\nf : Unit -> String\nf () =\n  use x <- with_x\n  x";
    insta::assert_snapshot!(diagnostics(text), @"
    E2001 7:3 mismatched types
      7:3 expected `String`, found `Int`
      note: the body of a lambda must have the return type the lambda is expected to have
    ");
    let generic = "with_x : (Int -> a) -> a\nwith_x k = k 1\n\nf : Unit -> String\nf () =\n  use x <- with_x\n  x";
    insta::assert_snapshot!(diagnostics(generic), @"
    E2001 6:3 mismatched types
      6:3 expected `String`, found `Int`
      4:5 expected because of the signature of `f`
    ");
}

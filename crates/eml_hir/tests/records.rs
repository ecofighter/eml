//! レコードの宣言、作る式、パターン、射影、更新、セクション (docs/spec/expressions.md の「レコード」)。

use eml_hir::{Body, ExprId, ExprKind, FieldKey, PatKind, Program};

use crate::common::{diagnostics, lower_files_text, lower_text};

const PERSON: &str = "data Person =\n  | Person { name : String, age : Int }\n\n";

#[test]
fn construction_keeps_the_source_order() {
    let text = format!("{PERSON}f : Unit -> Person\nf () = Person {{ age = 3, name = \"a\" }}");
    insta::assert_snapshot!(lower_text(&text), @r#"
    data Person
      | Person { name : String, age : Int }
    f : Unit -> Person
    f () = Person { age = 3, name = "a" }
    "#);
}

#[test]
fn field_errors() {
    let text = format!(
        "{PERSON}data Q = | Q Int\n\nf : Unit -> Person\nf () = Person {{ name = \"a\", name = \"b\", nick = \"c\" }}\n\ng : Unit -> Q\ng () = Q {{ x = 1 }}"
    );
    insta::assert_snapshot!(diagnostics(&text).join("\n"), @"
    E1045 7:29 the field `name` appears more than once
    E1046 7:41 `Person` has no field `nick`
    E1046 10:8 `Q` has no named fields
    ");
}

#[test]
fn missing_fields_are_listed_together() {
    let text = "data R = | R { a : Int, b : Int, c : Int }\n\nf : Unit -> R\nf () = R { b = 1 }";
    assert_eq!(
        diagnostics(text),
        ["E1047 4:8 missing fields `a` and `c` in `R`"]
    );
}

#[test]
fn one_missing_field_and_three_missing_fields() {
    let text = "data R = | R { a : Int, b : Int, c : Int, d : Int }\n\nf : Unit -> R\nf () = R { a = 1, b = 2, c = 3 }\n\ng : Unit -> R\ng () = R { c = 1 }";
    assert_eq!(
        diagnostics(text),
        [
            "E1047 4:8 missing field `d` in `R`",
            "E1047 7:8 missing fields `a`, `b` and `d` in `R`",
        ]
    );
}

#[test]
fn patterns_bind_in_source_order_and_omit_fields() {
    let text = format!(
        "{PERSON}f : Person -> Int\nf p = match p with\n  | Person {{ age = a, name }} -> a"
    );
    insta::assert_snapshot!(lower_text(&text), @"
    data Person
      | Person { name : String, age : Int }
    f : Person -> Int
    f p#0 = (match p#0 with | Person { name = name#2, age = a#1 } -> a#1)
    ");
}

#[test]
fn omitted_pattern_fields_are_recorded() {
    let text =
        format!("{PERSON}f : Person -> Int\nf p = match p with\n  | Person {{ age }} -> age");
    let lowered = eml_test_support::lower(&text);
    let body = body(&lowered.program, "f");
    let omitted: Vec<(String, u32)> = body
        .omitted_fields
        .iter()
        .map(|(pat, &(ctor, field))| {
            assert_eq!(body.pats[pat].kind, PatKind::Wildcard);
            (lowered.program[ctor].name.clone(), field)
        })
        .collect();
    assert_eq!(omitted, [("Person".to_string(), 0)]);
}

#[test]
fn a_duplicate_pattern_field_is_reported_once() {
    let text = format!(
        "{PERSON}f : Person -> String\nf p = match p with\n  | Person {{ name, name }} -> name"
    );
    assert_eq!(diagnostics(&text).len(), 1);
    assert!(diagnostics(&text)[0].starts_with("E1045"));
}

#[test]
fn an_empty_record_construction_is_a_constructor_reference() {
    let text = "data A = | A {}\n\nf : Unit -> A\nf () = A {}";
    insta::assert_snapshot!(lower_text(text), @"
    data A
      | A {}
    f : Unit -> A
    f () = A
    ");
}

#[test]
fn a_duplicate_declared_field() {
    let text = "data P = | P { a : Int, a : String }";
    insta::assert_snapshot!(lower_text(text), @"
    data P
      | P { a : Int, a : String }
    ---
    E1045 1:25 the field `a` appears more than once
    ");
}

#[test]
fn braces_on_a_positional_constructor_are_unknown_fields() {
    let text = "data Q = | Q Int | E\n\nf : Q -> Int\nf q = match q with\n  | Q {} -> 1\n  | E {} -> 2\n\ng : Unit -> Q\ng () = E {}";
    insta::assert_snapshot!(diagnostics(text).join("\n"), @"
    E1046 5:5 `Q` has no named fields
    E1046 6:5 `E` has no named fields
    E1046 9:8 `E` has no named fields
    ");
}

#[test]
fn an_unknown_pattern_field_still_binds_its_variables() {
    let text =
        format!("{PERSON}f : Person -> Int\nf p = match p with\n  | Person {{ nick = n }} -> n");
    assert_eq!(diagnostics(&text).len(), 1);
    assert!(diagnostics(&text)[0].starts_with("E1046"));
}

#[test]
fn a_punned_field_refers_to_the_name_in_scope() {
    let text = format!(
        "{PERSON}name : String\nname = \"top\"\n\nf : Int -> Person\nf age = Person {{ name, age }}"
    );
    insta::assert_snapshot!(lower_text(&text), @r#"
    data Person
      | Person { name : String, age : Int }
    name : String
    name = "top"
    f : Int -> Person
    f age#0 = Person { name = @name, age = age#0 }
    "#);
}

#[test]
fn sum_types_and_records_of_another_module() {
    let shapes = "pub data Shape =\n  | Circle { radius : Int }\n  | Dot Int";
    let entry = "import Shapes (Shape(..))\n\nf : Unit -> Shape\nf () = Shapes.Circle { radius = 1 }\n\ng : Shape -> Int\ng s = match s with\n  | Circle { radius } -> radius\n  | Dot n -> n";
    insta::assert_snapshot!(lower_files_text(entry, &[("Shapes.em", shapes)]), @"
    -- Main
    f : Unit -> Shape
    f () = Shapes.Circle { radius = 1 }
    g : Shape -> Int
    g s#0 = (match s#0 with | Shapes.Circle { radius = radius#1 } -> radius#1 | Shapes.Dot n#2 -> n#2)
    -- Shapes
    data Shape
      | Circle { radius : Int }
      | Dot Int
    ");
}

fn body<'m>(program: &'m Program, name: &str) -> &'m Body {
    program
        .functions()
        .find(|(_, function)| function.name == name)
        .and_then(|(id, _)| program.body(id))
        .expect("the body")
}

#[test]
fn a_record_is_not_a_value_for_grouping_arguments() {
    let text = format!("{PERSON}f : Int -> Person\nf n = Person {{ name = \"a\", age = n }}");
    let lowered = eml_test_support::lower(&text);
    let body = body(&lowered.program, "f");
    assert!(matches!(
        body.exprs[body.root].kind,
        ExprKind::Record { .. }
    ));
    assert!(!eml_hir::is_value(&lowered.program, body, body.root));
}

#[test]
fn projections_updates_and_field_sections() {
    let text = format!(
        "{PERSON}f : Person -> Person\nf p =\n  let n = p.name\n  let t = (1, (2, 3)).1.0\n  let age = 3\n  let g = (.name)\n  let h = (.0)\n  {{ p | name = n, age }}"
    );
    insta::assert_snapshot!(lower_text(&text), @"
    data Person
      | Person { name : String, age : Int }
    f : Person -> Person
    f p#0 = {
      let n#1 = p#0.name
      let t#2 = (1, (2, 3)).1.0
      let age#3 = 3
      let g#5 = (fn $p#4 -> $p#4.name)
      let h#7 = (fn $p#6 -> $p#6.0)
      { p#0 | name = n#1, age = age#3 }
    }
    ");
}

#[test]
fn a_duplicate_update_field() {
    let text = format!("{PERSON}f : Person -> Person\nf p = {{ p | age = 1, age = 2 }}");
    insta::assert_snapshot!(diagnostics(&text).join("\n"), @"E1045 5:22 the field `age` appears more than once");
}

#[test]
fn a_field_section_is_a_lambda_over_a_hidden_parameter() {
    let text = format!("{PERSON}f : Person -> String\nf = (.name)");
    let lowered = eml_test_support::lower(&text);
    let body = body(&lowered.program, "f");
    let ExprKind::Lambda(closure) = &body.exprs[body.root].kind else {
        panic!("a section lowers to a lambda");
    };
    assert_eq!(closure.params.len(), 1);
    let ExprKind::Field { base, field } = &body.exprs[closure.body].kind else {
        panic!("the body is a projection");
    };
    assert!(matches!(body.exprs[*base].kind, ExprKind::Path(_)));
    assert_eq!(field.field, FieldKey::Name("name".to_string()));
    // 範囲はどれもセクション全体である
    let whole = body.exprs[body.root].range;
    assert_eq!(body.exprs[closure.body].range, whole);
    assert_eq!(body.exprs[*base].range, whole);
}

#[test]
fn projections_and_updates_are_not_values_for_grouping_arguments() {
    let text = format!(
        "{PERSON}f : Person -> String\nf p = p.name\n\ng : Person -> Person\ng p = {{ p | age = 1 }}"
    );
    let lowered = eml_test_support::lower(&text);
    for name in ["f", "g"] {
        let body = body(&lowered.program, name);
        assert!(!eml_hir::is_value(&lowered.program, body, body.root));
    }
}

#[test]
fn use_records_the_lambdas_it_builds() {
    let text = "with_x : (Int -> Int) -> Int\nwith_x k = k 1\n\nf : Unit -> Int\nf () =\n  use x <- with_x\n  with_x (fn y -> x + y)";
    let lowered = eml_test_support::lower(text);
    let body = body(&lowered.program, "f");
    let parameter = |id: ExprId| {
        let ExprKind::Lambda(closure) = &body.exprs[id].kind else {
            panic!("only lambdas are marked");
        };
        let PatKind::Bind(local) = body.pats[closure.params[0]].kind else {
            panic!("the parameter is a variable");
        };
        body.locals[local].name.clone()
    };
    let lambdas = body
        .exprs
        .iter()
        .filter(|(_, expr)| matches!(expr.kind, ExprKind::Lambda(_)))
        .count();
    assert_eq!(lambdas, 2);
    let marked: Vec<String> = body.use_lambdas.iter().map(|&id| parameter(id)).collect();
    assert_eq!(marked, ["x"]);
}

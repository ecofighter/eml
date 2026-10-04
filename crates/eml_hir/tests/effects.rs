mod common;

use common::lower_text;
use eml_test_support::{lower, short};

/// 構文と HIR の診断を、位置の順に並べたもの。
fn errors(text: &str) -> Vec<String> {
    let mut lowered = lower(text);
    lowered.diagnostics.sort_by_key(|d| d.primary.range.start());
    short(&lowered.files, &lowered.diagnostics)
}

#[test]
fn effects_and_operations_are_items() {
    let text = "effect Ask where\n  ask : String -> String\n  never stop : Unit -> a\n\nf : Unit -> <Ask> String\nf () = ask \"x\"";
    insta::assert_snapshot!(lower_text(text), @r#"
    effect Ask
      ask : String -> String
      never stop : Unit -> a
    f : Unit -> <Ask> String
    f () = (@Ask.ask "x")
    "#);
}

#[test]
fn operations_share_the_value_namespace() {
    let text = "effect A where\n  op : Int -> Int\n\neffect B where\n  op : Int -> Int\n\nhelper : Int -> Int\nhelper x = x\n\neffect C where\n  helper : Int -> Int\n\neffect A where\n  other : Int -> Int";
    insta::assert_snapshot!(lower_text(text), @r"
    effect A
      op : Int -> Int
    effect B
      op : Int -> Int
    effect C
      helper : Int -> Int
    effect A
      other : Int -> Int
    helper : Int -> Int
    helper x#0 = x#0
    ---
    E1003 5:3 `op` is defined more than once
    E1003 11:3 `helper` is defined more than once
    E1003 13:8 `A` is defined more than once
    ");
}

#[test]
fn operation_signatures_are_checked() {
    let text = "effect E where\n  with_row : Int -> <IO> Int\n  constant : Int\n  never bad : a -> a\n  never good : Int -> b\n  multi many : Unit -> Int";
    insta::assert_snapshot!(lower_text(text), @r"
    effect E
      with_row : Int -> <IO> Int
      constant : Int
      never bad : a -> a
      never good : Int -> b
      multi many : Unit -> Int
    ---
    E1007 2:21 an operation cannot have a row on its outermost arrows
    E1007 3:14 the signature of an operation must be a function type
    E1008 4:20 the result type of a `never` operation must be a type variable that does not appear in its parameters
    ");
}

#[test]
fn effects_take_type_parameters_and_rows_take_type_arguments() {
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n  never fail : Unit -> a\n\nf : Unit -> <State Int, State (Int -> Int)> Int\nf () = 1";
    insta::assert_snapshot!(lower_text(text), @r"
    effect State s
      get : Unit -> s
      put : s -> Unit
      never fail : Unit -> a
    f : Unit -> <State Int, State (Int -> Int)> Int
    f () = 1
    ");
}

#[test]
fn operations_see_the_type_parameters_of_their_effect_first() {
    let lowered = lower("effect State s where\n  get : Unit -> s\n  never fail : Unit -> a");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let generics = |name: &str| -> (Vec<String>, usize) {
        let (_, operation) = lowered
            .module
            .operations
            .iter()
            .find(|(_, operation)| operation.name == name)
            .unwrap();
        let names = operation
            .signature
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.clone())
            .collect();
        (names, operation.effect_params)
    };
    assert_eq!(generics("get"), (vec!["s".to_string()], 1));
    assert_eq!(
        generics("fail"),
        (vec!["s".to_string(), "a".to_string()], 1)
    );
}

#[test]
fn type_arguments_and_parameters_of_effects_are_checked() {
    let text = "effect State s where\n  get : Unit -> s\n\neffect Pair a a where\n  first : Unit -> a\n\neffect Fail e where\n  never raise : Unit -> e\n\nf : Unit -> <State> Int\nf () = 1\n\ng : Unit -> <State Int Int> Int\ng () = 1\n\nh : Unit -> <State Int> Int\nh () = 1";
    assert_eq!(
        errors(text),
        [
            "E1003 4:15 `a` is defined more than once",
            "E1008 8:25 the result type of a `never` operation must be a type variable that does not appear in its parameters",
            "E1015 10:14 `State` takes 1 type argument, but 0 were given",
            "E1015 13:14 `State` takes 1 type argument, but 2 were given",
        ]
    );
}

#[test]
fn a_function_after_an_operation_of_the_same_name_is_a_duplicate() {
    let text = "effect E where\n  run : Int -> Int\n\nrun : Int -> Int\nrun x = x";
    assert_eq!(errors(text), ["E1003 4:1 `run` is defined more than once"]);
}

#[test]
fn handlers_resume_and_drop_are_expressions() {
    let text = "effect Ask where\n  ask : String -> String\n  never fail : String -> a\n\nf : Unit -> String\nf () =\n  handle ask \"x\" with\n    | ask key k -> resume k key\n    | fail message -> message\n    | return x -> x\n\ng : String -> Unit\ng s = drop s";
    insta::assert_snapshot!(lower_text(text), @r#"
    effect Ask
      ask : String -> String
      never fail : String -> a
    f : Unit -> String
    f () = {
      (handle (@Ask.ask "x") with | ask key#0 k#1 -> (resume k#1 key#0) | fail message#2 -> message#2 | return x#3 -> x#3)
    }
    g : String -> Unit
    g s#0 = (drop s#0)
    "#);
}

#[test]
fn clause_names_are_resolved_among_operations_only() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  let ask = 1\n  handle ask with\n    | ask () k -> resume k ask";
    insta::assert_snapshot!(lower_text(text), @r"
    effect Ask
      ask : Unit -> Int
    f : Unit -> Int
    f () = {
      let ask#0 = 1
      (handle ask#0 with | ask () k#1 -> (resume k#1 ask#0))
    }
    ");
}

#[test]
fn clause_errors_are_reported_together() {
    let text = "effect Ask where\n  ask : Unit -> Int\n  other : Unit -> Int\n\neffect Log where\n  log : String -> Unit\n\neffect Fail where\n  never fail : String -> a\n\nf : Unit -> Int\nf () =\n  handle 1 with\n    | ask () -> 1\n    | ask () k -> resume k 1\n    | log m k -> resume k ()\n    | nope x k -> 1\n    | println s k -> resume k ()\n    | return x -> x\n    | return y -> y\n\ng : Unit -> Int\ng () = handle 1 with | fail m k -> 1\n\nh : Unit -> Int\nh () = handle 1 with | return x -> x";
    assert_eq!(
        errors(text),
        [
            "E1013 13:3 this handler has no clause for `other` of `Ask`",
            "E1010 14:7 the clause for `ask` takes 2 parameters, but this one has 1",
            "E1014 15:7 `ask` has more than one clause in this handler",
            "E1012 16:7 a handler can handle only one effect",
            "E1001 17:7 cannot find effect operation `nope`",
            "E1009 18:7 `IO` cannot be handled",
            "E1014 20:5 this handler has more than one `return` clause",
            "E1010 23:24 the clause for `fail` takes 1 parameter, but this one has 2",
            "E1013 26:8 this handler has no operation clauses",
        ]
    );
}

#[test]
fn resume_and_drop_take_a_fixed_number_of_arguments() {
    let text = "f : Int -> Unit\nf k =\n  resume k\n  drop k 2\n  resume k k k";
    assert_eq!(
        errors(text),
        [
            "E1011 3:3 `resume` takes a continuation and a value, but 1 argument was given",
            "E1011 4:3 `drop` takes one value, but 2 arguments were given",
            "E0004 5:3 `resume` with a handler state is not supported yet",
        ]
    );
}

#[test]
fn handlers_with_an_initial_state_come_in_stage_6() {
    assert_eq!(
        errors("f : Unit -> Int\nf () = handle g () from 1 with | return x st -> x"),
        ["E0004 2:20 handlers with `from` are not supported yet"]
    );
}

#[test]
fn handler_parts_capture_what_they_use() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Int -> Int -> Int\nf a b =\n  handle ask () + a with\n    | ask () k -> resume k b\n    | return x -> x + a";
    let lowered = lower(text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let function = lowered
        .module
        .functions
        .iter()
        .find(|(_, function)| function.name == "f")
        .map(|(_, function)| function)
        .unwrap();
    let body = function.body.as_ref().unwrap();
    let names = |locals: Vec<eml_hir::LocalId>| -> Vec<String> {
        locals
            .into_iter()
            .map(|local| body.locals[local].name.clone())
            .collect()
    };
    // 等式の本体はブロックで、handle はその最後の式である
    let (handle, kind) = body
        .exprs
        .iter()
        .find(|(_, expr)| matches!(expr.kind, eml_hir::ExprKind::Handle { .. }))
        .map(|(id, expr)| (id, &expr.kind))
        .unwrap();
    let eml_hir::ExprKind::Handle {
        body: handled,
        clauses,
        ret,
        ..
    } = kind
    else {
        unreachable!();
    };
    assert_eq!(names(body.captures(handle, &[])), ["a", "b"]);
    assert_eq!(names(body.captures(*handled, &[])), ["a"]);
    let clause = &clauses[0];
    let bound: Vec<_> = clause.patterns().collect();
    assert_eq!(names(body.captures(clause.body, &bound)), ["b"]);
    let ret = ret.as_ref().unwrap();
    assert_eq!(names(body.captures(ret.body, &[ret.param])), ["a"]);
}

#[test]
fn a_repeated_operation_name_is_not_a_missing_clause() {
    let text = "effect Ask where\n  ask : Unit -> Int\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () = handle 1 with | ask () k -> resume k 1";
    assert_eq!(errors(text), ["E1003 3:3 `ask` is defined more than once"]);
}

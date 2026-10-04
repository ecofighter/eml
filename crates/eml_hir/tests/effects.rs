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
      many : Unit -> Int
    ---
    E1007 2:21 an operation cannot have a row on its outermost arrows
    E1007 3:14 the signature of an operation must be a function type
    E1008 4:20 the result type of a `never` operation must be a type variable that does not appear in its parameters
    E0004 6:3 `multi` operations are not supported yet
    ");
}

#[test]
fn effect_type_parameters_and_arguments_come_in_stage_3b() {
    let text = "effect State s where\n  get : Unit -> s\n\nf : Unit -> <State Int> Int\nf () = 1";
    assert_eq!(
        errors(text),
        [
            "E0004 1:14 effect type parameters are not supported yet",
            "E0004 4:14 effects with type arguments are not supported yet",
        ]
    );
}

#[test]
fn a_function_after_an_operation_of_the_same_name_is_a_duplicate() {
    let text = "effect E where\n  run : Int -> Int\n\nrun : Int -> Int\nrun x = x";
    assert_eq!(errors(text), ["E1003 4:1 `run` is defined more than once"]);
}

mod common;

use common::{diagnostics, lower_text};

#[test]
fn a_signature_and_an_equation_become_a_function() {
    insta::assert_snapshot!(lower_text("f : Int -> <IO> Unit\nf x = println (show_int x)"), @r"
    f : Int -> <IO> Unit
    f x#0 = (println (show_int x#0))
    ");
}

#[test]
fn later_lets_shadow_earlier_names() {
    let text = "f : Int -> Int\nf x =\n  let y = x\n  let x = y\n  x";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let y#1 = x#0
      let x#2 = y#1
      x#2
    }
    ");
}

#[test]
fn functions_resolve_in_any_order_and_shadow_builtins() {
    let text =
        "a : Int -> Int\na n = b n\n\nb : Int -> Int\nb n = not n\n\nnot : Int -> Int\nnot n = n";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int -> Int
    a n#0 = (@b n#0)
    b : Int -> Int
    b n#0 = (@not n#0)
    not : Int -> Int
    not n#0 = n#0
    ");
}

#[test]
fn if_without_else_and_annotations() {
    let text = "f : Bool -> Unit\nf b =\n  if b then println \"yes\"\n  (() : Unit)";
    insta::assert_snapshot!(lower_text(text), @r#"
    f : Bool -> Unit
    f b#0 = {
      (if b#0 (println "yes"))
      (() : Unit)
    }
    "#);
}

#[test]
fn unit_and_wildcard_parameters_and_literals() {
    let text = "g : Unit -> Bool -> String\ng () _ = if True then \"a\\n\" else \"b\"";
    insta::assert_snapshot!(lower_text(text), @r#"
    g : Unit -> Bool -> String
    g () _ = (if True "a\n" "b")
    "#);
}

#[test]
fn undefined_names_are_reported() {
    insta::assert_snapshot!(lower_text("f : Int -> Strin\nf x = g y Foo"), @r"
    f : Int -> <error>
    f x#0 = (<missing> <missing> <missing>)
    ---
    E1002 1:12 cannot find type `Strin`
    E1001 2:7 cannot find value `g`
    E1001 2:9 cannot find value `y`
    E1001 2:11 cannot find constructor `Foo`
    ");
}

#[test]
fn signatures_and_equations_are_paired_by_name() {
    let text = "a : Int\nb = 1\nc : Int\nc = 2\nc : Int\nd : Int\nd = 3\nd = 4";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int
    a = <no equation>
    b : <no signature>
    b = 1
    c : Int
    c = 2
    d : Int
    d = (match () with | () -> 3 | () -> 4)
    ---
    E1005 1:1 `a` has a signature but no equation
    E1004 2:1 `b` has no type signature
    E1003 5:1 `c` is defined more than once
    ");
}

#[test]
fn constructs_of_later_stages_are_not_yet_supported() {
    let text =
        "f : Int -> Int\nf x =\n  let first = fn t -> t.0\n  let plus = (+)\n  let y = x in y";
    insta::assert_snapshot!(lower_text(text), @"
    f : Int -> Int
    f x#0 = {
      let first#2 = (fn t#1 -> <missing>)
      let plus#5 = (fn $a#3 $b#4 -> (+ $a#3 $b#4))
      {
        let y#6 = x#0
        y#6
      }
    }
    ---
    E0004 3:23 field access is not supported yet
    ");
}

#[test]
fn row_variables_type_variables_and_unknown_effects() {
    let text = "f : Int -> <e> Int\nf x = x\ng : a -> <State> Int\ng x = 1";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> <e> Int
    f x#0 = x#0
    g : a -> <error> Int
    g x#0 = 1
    ---
    E1002 3:11 cannot find effect `State`
    ");
}

#[test]
fn signature_variables_scope_over_the_body() {
    let text = "f : a -> <e> a\nf x =\n  let y : a = x\n  let g = fn (h : b -> <e> b) -> h\n  y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : a -> <e> a
    f x#0 = {
      let y#1 : a = x#0
      let g#3 = (fn (h#2 : <error> -> <e> <error>) -> h#2)
      y#1
    }
    ---
    E1002 4:19 cannot find type variable `b`
    E1002 4:28 cannot find type variable `b`
    ");
}

#[test]
fn type_variables_and_row_variables_have_separate_names() {
    let text =
        "f : a -> <IO | a> a\nf x = x\n\ng : Int -> <IO | e> Int\ng x = (x : Int -> <f> Int)";
    insta::assert_snapshot!(lower_text(text), @r"
    f : a -> <IO | a> a
    f x#0 = x#0
    g : Int -> <IO | e> Int
    g x#0 = (x#0 : Int -> <error> Int)
    ---
    E1002 5:20 cannot find row variable `f`
    ");
}

#[test]
fn operator_definitions_and_qualified_names() {
    let text = "(<+>) : Int\na <+> b = a\nf : Int -> Int\nf (x) = List.length x";
    insta::assert_snapshot!(lower_text(text), @r"
    <+> : Int
    <+> a#0 b#1 = a#0
    f : Int -> Int
    f x#0 = (<missing> x#0)
    ---
    E0004 4:9 qualified names are not supported yet
    ");
}

#[test]
fn lambdas_bind_their_parameters_only_in_the_body() {
    let text = "f : Int -> Int\nf x =\n  let g = fn y (z : Int) _ -> x + y\n  y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let g#3 = (fn y#1 (z#2 : Int) _ -> (+ x#0 y#1))
      <missing>
    }
    ---
    E1001 4:3 cannot find value `y`
    ");
}

#[test]
fn a_lambda_can_be_the_last_argument() {
    let text = "call : Int -> (Int -> Int) -> Int\ncall n f = f n\n\ng : Int -> Int\ng n = call n fn x -> x + 1";
    insta::assert_snapshot!(lower_text(text), @r"
    call : Int -> (Int -> Int) -> Int
    call n#0 f#1 = (f#1 n#0)
    g : Int -> Int
    g n#0 = (@call n#0 (fn x#1 -> (+ x#1 1)))
    ");
}

#[test]
fn several_equations_become_a_match_on_the_arguments() {
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x y = x + y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int -> Int
    f $0#0 $1#1 = (match ($0#0, $1#1) with | (0, y#2) -> y#2 | (x#3, y#4) -> (+ x#3 y#4))
    ");
}

#[test]
fn equations_of_one_argument_match_on_it_directly() {
    let text = "g : Int -> Int\ng 0 = 1\ng n = n";
    insta::assert_snapshot!(lower_text(text), @r"
    g : Int -> Int
    g $0#0 = (match $0#0 with | 0 -> 1 | n#1 -> n#1)
    ");
}

#[test]
fn equations_must_be_consecutive_and_follow_their_signature() {
    let text = "f : Int -> Int\n\ng : Int\ng = 1\n\nf 0 = 1\n\nh : Int\nh = 2\n\nf n = n";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1019 6:1 the signature of `f` is not followed by its equations",
            "E1018 11:1 the equations of `f` are not consecutive",
        ]
    );
}

#[test]
fn equations_must_take_the_same_number_of_arguments() {
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x = x\nf x y = undefined_name";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1020 3:1 the equations of `f` take different numbers of arguments",
            "E1001 4:9 cannot find value `undefined_name`",
        ]
    );
}

#[test]
fn use_passes_the_rest_of_the_block_as_the_last_argument() {
    let text = "wrap : Int -> (Unit -> Int) -> Int\nwrap n k = k ()\n\nbind : (Int -> Int) -> Int\nbind k = k 1\n\nf : Unit -> Int\nf () =\n  use wrap 1\n  use x <- bind\n  x + 2";
    insta::assert_snapshot!(lower_text(text), @r"
    wrap : Int -> (Unit -> Int) -> Int
    wrap n#0 k#1 = (k#1 ())
    bind : (Int -> Int) -> Int
    bind k#0 = (k#0 1)
    f : Unit -> Int
    f () = {
      (@wrap 1 (fn () -> {
        (@bind (fn x#0 -> {
          (+ x#0 2)
        }))
      }))
    }
    ");
}

#[test]
fn use_at_the_end_of_a_block_has_nothing_to_wrap() {
    let text = "wrap : (Unit -> Int) -> Int\nwrap k = k ()\n\nf : Unit -> Int\nf () =\n  let a = 1\n  use wrap";
    assert_eq!(
        diagnostics(text),
        vec!["E1024 7:3 a `use` must be followed by the rest of its block"]
    );
}

#[test]
fn let_in_is_a_block_with_one_let() {
    let text = "f : Int -> Int\nf x = let y : Int = x + 1 in y * 2";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let y#1 : Int = (+ x#0 1)
      (* y#1 2)
    }
    ");
}

#[test]
fn lambda_and_clause_parameters_are_one_group() {
    let text = "effect Ask where\n  ask : Int -> Int\n\nf : Unit -> Int\nf () =\n  let g = fn x x -> x\n  handle 1 with\n    | ask n n -> 0\n    | return r -> r";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1017 6:16 `x` is bound more than once",
            "E1017 8:13 `n` is bound more than once",
        ]
    );
}

#[test]
fn qualified_effects_are_not_supported_yet() {
    // 不具合1: `ast::Effect::name()` が最初の `UIDENT` を取り、`M` を探して E1002 にしていた
    let text = "f : Unit -> <M.E> Unit\nf () = ()\ng : Unit -> <M.State Int> Unit\ng () = ()";
    assert_eq!(
        diagnostics(text),
        [
            "E0004 1:14 qualified names are not supported yet",
            "E0004 3:14 qualified names are not supported yet",
        ]
    );
}

#[test]
fn minus_is_defined_by_its_name_token() {
    let text = "(-) : Int -> Int -> Int\na - b = a\nf : Int\nf = 3 - 1";
    insta::assert_snapshot!(lower_text(text), @"
    - : Int -> Int -> Int
    - a#0 b#1 = a#0
    f : Int
    f = (@- 3 1)
    ");
}

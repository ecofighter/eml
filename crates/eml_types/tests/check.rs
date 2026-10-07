use crate::common::check_text;
use eml_hir::ValueItem;

#[test]
fn signatures_and_local_types() {
    let text = "add : Int -> Int -> Int\nadd a b =\n  let sum = a + b\n  sum\n\nmain : Unit -> <IO> Unit\nmain () =\n  let message = \"sum: \" ++ show_int (add 1 2)\n  println message";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
      sum#2 : Int
    main : Unit -> <IO> Unit
      message#0 : String
    ");
}

#[test]
fn argument_mismatch_points_at_the_callee() {
    let text = "f : Int -> Int\nf x = x\n\ng : Unit -> Int\ng () = f \"one\"";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
      x#0 : Int
    g : Unit -> Int
    ---
    E2001 5:10 mismatched types
      5:10 expected `Int`, found `String`
      5:8 argument 1 of `f`
    ");
}

#[test]
fn statements_annotations_conditions_and_signatures() {
    let text = "h : Bool -> Int\nh b =\n  1\n  let n : Int = \"x\"\n  if n then 1 else \"two\"";
    insta::assert_snapshot!(check_text(text), @r#"
    h : Bool -> Int
      b#0 : Bool
      n#1 : Int
    ---
    E2001 3:3 mismatched types
      3:3 expected `Unit`, found `Int`
      note: a statement that is not the last one in a block must have type `Unit`
    E2001 4:17 mismatched types
      4:17 expected `Int`, found `String`
      4:11 expected because of this annotation
    E2001 5:6 mismatched types
      5:6 expected `Bool`, found `Int`
      note: the condition of `if` must have type `Bool`
    E2001 5:20 mismatched types
      5:20 expected `Int`, found `String`
      1:5 expected because of the signature of `h`
    "#);
}

#[test]
fn if_without_else_must_be_unit() {
    insta::assert_snapshot!(check_text("k : Bool -> Int\nk b = if b then 1"), @r"
    k : Bool -> Int
      b#0 : Bool
    ---
    E2001 2:7 mismatched types
      2:7 expected `Int`, found `Unit`
      1:5 expected because of the signature of `k`
    E2001 2:17 mismatched types
      2:17 expected `Unit`, found `Int`
      note: an `if` without `else` must have type `Unit`
    ");
}

#[test]
fn main_type_and_parameter_count() {
    let text = "main : Int -> Int\nmain n = n\n\ntwo : Int -> Int\ntwo a b = a";
    insta::assert_snapshot!(check_text(text), @r"
    main : Int -> Int
      n#0 : Int
    two : Int -> Int
      a#0 : Int
      b#1 : {error}
    ---
    E2004 1:8 `main` must have type `Unit -> <IO> Unit`
      1:8 found `Int -> Int`
    E2001 5:7 `two` has 2 parameters but its signature has 1 arrow
      5:7 this parameter has no arrow in the signature
      4:7 the signature
    ");
}

#[test]
fn function_values_and_partial_application() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\npartial : Unit -> Int\npartial () =\n  let f = add 1\n  let g = add\n  0\n\ncurried : Int -> Int -> Int\ncurried a = a";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
    partial : Unit -> Int
      f#0 : Int -> <_> Int
      g#1 : Int -> <_> Int -> <_> Int
    curried : Int -> Int -> Int
      a#0 : Int
    ---
    E2001 11:13 mismatched types
      11:13 expected `Int -> Int`, found `Int`
      10:11 expected because of the signature of `curried`
    ");
}

#[test]
fn name_errors_do_not_cascade() {
    let text = "f : Int -> Int\nf x = g (x + undefined_thing) 1\nb : Bool\nb = 1 < 2 < 3";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
      x#0 : Int
    b : Bool
    ---
    E1001 2:7 cannot find value `g`
      2:7 not found in this scope
    E1001 2:14 cannot find value `undefined_thing`
      2:14 not found in this scope
    E1006 4:11 `<` and `<` cannot be combined without parentheses
      4:11 use parentheses to group the operators
    ");
}

#[test]
fn too_many_arguments_and_non_functions() {
    let text = "one : Int -> Int\none x = x\n\nbad : Unit -> Int\nbad () = one 1 2 + True 3";
    insta::assert_snapshot!(check_text(text), @r"
    one : Int -> Int
      x#0 : Int
    bad : Unit -> Int
    ---
    E2001 5:16 `one` takes 1 argument but 2 were given
      5:16 unexpected argument
    E2001 5:25 `True` is not a function
      5:25 unexpected argument
    ");
}

#[test]
fn polymorphic_functions_are_instantiated_at_each_use() {
    let text = "id : a -> a\nid x = x\n\nuse_both : Unit -> String\nuse_both () =\n  let n = id 1\n  let s = id \"s\"\n  s";
    insta::assert_snapshot!(check_text(text), @r"
    id : a -> a
      x#0 : a
    use_both : Unit -> String
      n#0 : Int
      s#1 : String
    ");
}

#[test]
fn rigid_type_variables_do_not_unify_with_other_types() {
    let text = "f : a -> b\nf x = x\n\ng : a -> Int\ng x = x";
    insta::assert_snapshot!(check_text(text), @r"
    f : a -> b
      x#0 : a
    g : a -> Int
      x#0 : a
    ---
    E2001 2:7 mismatched types
      2:7 expected `b`, found `a`
      1:5 expected because of the signature of `f`
    E2001 5:7 mismatched types
      5:7 expected `Int`, found `a`
      4:5 expected because of the signature of `g`
    ");
}

#[test]
fn annotations_in_the_body_refer_to_the_signature() {
    let text = "f : a -> a\nf x =\n  let y : a = x\n  y\n\ng : a -> Int\ng x = (x : Int)";
    insta::assert_snapshot!(check_text(text), @r"
    f : a -> a
      x#0 : a
      y#1 : a
    g : a -> Int
      x#0 : a
    ---
    E2001 7:8 mismatched types
      7:8 expected `Int`, found `a`
      7:12 expected because of this annotation
    ");
}

#[test]
fn equations_may_return_functions_and_calls_may_pass_more_arguments() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\nadder : Int -> Int -> Int\nadder x = add x\n\ninc : Int -> Int\ninc = adder 1\n\nthree : Unit -> Int\nthree () = adder 1 2 + inc 1";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
    adder : Int -> Int -> Int
      x#0 : Int
    inc : Int -> Int
    three : Unit -> Int
    ");
}

#[test]
fn lambdas_are_checked_against_the_expected_type_or_inferred() {
    let text = "apply : (Int -> Int) -> Int\napply f = f 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = apply (fn x -> x + 1)\n  let id = fn y -> y\n  let s = id \"s\"\n  let k = fn (z : Int) _ -> z\n  println (show_int (k n s))";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (Int -> Int) -> Int
      f#0 : Int -> Int
    main : Unit -> <IO> Unit
      x#0 : Int
      n#1 : Int
      y#2 : String
      id#3 : String -> <IO> String
      s#4 : String
      z#5 : Int
      k#6 : Int -> <IO> String -> <IO> Int
    ");
}

#[test]
fn annotated_lambda_parameters_must_match_and_arities_must_agree() {
    let text = "apply : (Int -> Int) -> Int\napply f = f 1\n\nbad : Unit -> Int\nbad () = apply (fn (x : String) -> 1) + apply (fn a b -> a)";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (Int -> Int) -> Int
      f#0 : Int -> Int
    bad : Unit -> Int
      x#0 : String
      a#1 : Int
      b#2 : {error}
    ---
    E2001 5:20 mismatched types
      5:20 expected `Int`, found `String`
      note: an annotated lambda parameter must have the parameter type the lambda is expected to have
    E2001 5:53 this lambda has 2 parameters but its expected type `Int -> Int` has 1 arrow
      5:53 this parameter has no arrow in the expected type
    ");
}

#[test]
fn an_arity_mismatch_in_a_lambda_does_not_cascade() {
    let text = "apply : (Int -> Int) -> Int\napply f = f 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = apply (fn a b -> println \"x\")\n  ()";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (Int -> Int) -> Int
      f#0 : Int -> Int
    main : Unit -> <IO> Unit
      a#0 : Int
      b#1 : {error}
      n#2 : Int
    ---
    E2001 6:23 this lambda has 2 parameters but its expected type `Int -> Int` has 1 arrow
      6:23 this parameter has no arrow in the expected type
    ");
}

#[test]
fn kinds_follow_from_how_values_are_used() {
    let text = "twice : (a -> a) -> a -> a\ntwice f x = f (f x)\n\nid : a -> a\nid x = x\n\nboth : a -> (a -> a -> b) -> b\nboth x g = g x x\n\npick : c -> c -> c\npick p q = p\n\ncall : d -> d\ncall y = both y pick";
    insta::assert_snapshot!(check_text(text), @r"
    twice : (a -> a) -> a -> a
      kinds: (a -> a) <= Unr
      f#0 : a -> a
      x#1 : a
    id : a -> a
      x#0 : a
    both : a -> (a -> a -> b) -> b
      kinds: a <= Unr
      x#0 : a
      g#1 : a -> a -> b
    pick : c -> c -> c
      kinds: c <= Unr
      p#0 : c
      q#1 : c
    call : d -> d
      kinds: d <= Unr
      y#0 : d
    ");
}

#[test]
fn kinds_are_shared_within_a_strongly_connected_component() {
    let text = "ping : a -> Int -> a\nping x n = if n == 0 then x else pong (first x x) (n - 1)\n\npong : b -> Int -> b\npong y n = ping y n\n\nfirst : c -> c -> c\nfirst u v = u";
    insta::assert_snapshot!(check_text(text), @r"
    ping : a -> Int -> a
      kinds: a <= Unr
      x#0 : a
      n#1 : Int
    pong : b -> Int -> b
      kinds: b <= Unr
      y#0 : b
      n#1 : Int
    first : c -> c -> c
      kinds: c <= Unr
      u#0 : c
      v#1 : c
    ");
}

#[test]
fn kinds_are_shared_around_a_ring_of_functions() {
    let text = "f0 : a -> Int -> a\nf0 x n = if n == 0 then x else f1 x (n - 1)\n\nf1 : b -> Int -> b\nf1 y n = if n == 0 then y else f2 y (n - 1)\n\nf2 : c -> Int -> c\nf2 z n = if n == 0 then first z z else f0 z (n - 1)\n\nfirst : d -> d -> d\nfirst u v = u";
    insta::assert_snapshot!(check_text(text), @r"
    f0 : a -> Int -> a
      kinds: a <= Unr
      x#0 : a
      n#1 : Int
    f1 : b -> Int -> b
      kinds: b <= Unr
      y#0 : b
      n#1 : Int
    f2 : c -> Int -> c
      kinds: c <= Unr
      z#0 : c
      n#1 : Int
    first : d -> d -> d
      kinds: d <= Unr
      u#0 : d
      v#1 : d
    ");
}

#[test]
fn a_reference_to_a_function_without_equations_is_checked() {
    let text = "f : Int -> Int\n\ng : Int -> Int\ng x = f x";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
    g : Int -> Int
      x#0 : Int
    ---
    E1005 1:1 `f` has a signature but no equation
      1:1 add an equation for `f` after this signature
    ");
}

#[test]
fn captured_values_count_inside_the_lambda_body() {
    let text = "dupper : a -> Unit -> (a -> a -> a) -> a\ndupper x = fn () g -> g x x\n\nkeeper : a -> Unit -> a\nkeeper x = fn () -> x";
    insta::assert_snapshot!(check_text(text), @r"
    dupper : a -> Unit -> (a -> a -> a) -> a
      kinds: a <= Unr
      x#0 : a
      g#1 : a -> a -> a
    keeper : a -> Unit -> a
      x#0 : a
    ");
}

#[test]
fn a_callee_that_is_not_a_name_is_described_without_quotes() {
    let text = "inc : Int -> Int\ninc n = n + 1\n\neach : (String -> Unit) -> Unit\neach f = f \"a\"\n\nbad : Bool -> Int\nbad b = (if b then 1 else 2) 3 + (if b then inc else inc) \"x\" + (if b then inc else inc) 1 2\n\nloud : Bool -> Unit\nloud b =\n  (if b then println else println) \"x\"\n  (if b then each else each) (fn s -> println s)";
    insta::assert_snapshot!(check_text(text), @"
    inc : Int -> Int
      n#0 : Int
    each : (String -> Unit) -> Unit
      f#0 : String -> Unit
    bad : Bool -> Int
      b#0 : Bool
    loud : Bool -> Unit
      b#0 : Bool
      s#1 : String
    ---
    E2001 8:30 this expression is not a function
      8:30 unexpected argument
    E2001 8:59 mismatched types
      8:59 expected `Int`, found `String`
      8:35 argument 1 of this expression
    E2001 8:92 this expression takes 1 argument but 2 were given
      8:92 unexpected argument
    E2002 12:3 this expression performs `IO`, which the signature of `loud` does not allow
      12:3 this call performs `IO`
      10:8 the row of this signature does not include it
      help: add `IO` to the row of the signature of `loud`, as in `-> <IO> ...`
    E2002 13:39 `println` performs `IO`, which this lambda does not allow
      13:39 this call performs `IO`
      13:4 argument 1 of this expression does not allow it
    ");
}

#[test]
fn extern_schemes_are_exported() {
    let checked = eml_test_support::check("main : Unit -> <IO> Unit\nmain () = ()");
    let ty = |name: &str| {
        let (id, _) = checked
            .program
            .functions()
            .find(|(_, function)| function.name == name)
            .unwrap();
        checked.typed.decls[&ValueItem::Function(id)]
            .ty
            .display(&checked.program.names)
            .to_string()
    };
    assert_eq!(ty("println"), "String -> <IO> Unit");
    assert_eq!(ty("+"), "Int -> Int -> Int");
    assert_eq!(ty(">>"), "(a -> <e> b) -> (b -> <e> c) -> a -> <e> c");
    // コンストラクタは extern ではなく、Prelude の `data Bool` のスキームとして書き出す
    assert_eq!(
        checked.typed.decls[&ValueItem::Constructor(checked.program.lang.true_ctor)]
            .ty
            .display(&checked.program.names)
            .to_string(),
        "Bool"
    );
}

#[test]
fn operation_types_are_exported() {
    let checked =
        eml_test_support::check("effect State where\n  get : Unit -> Int\n  put : Int -> Unit");
    let ty = |name: &str| {
        let (id, _) = checked
            .program
            .operations()
            .find(|(_, operation)| operation.name == name)
            .unwrap();
        checked.typed.decls[&ValueItem::Operation(id)]
            .ty
            .display(&checked.program.names)
            .to_string()
    };
    assert_eq!(ty("get"), "Unit -> <State> Int");
    assert_eq!(ty("put"), "Int -> <State> Unit");
}

#[test]
fn later_stage_constructs_add_no_type_errors() {
    // E0004 の跡 (`<missing>` の値) を型検査に通しても、誤りを重ねて出さない。
    let text = "counter : Unit -> Int\ncounter () = 0\n\nf : Int -> Int\nf n =\n  let first = fn t -> t.0\n  let plus = (+)\n  0";
    insta::assert_snapshot!(check_text(text), @"
    counter : Unit -> Int
    f : Int -> Int
      n#0 : Int
      t#1 : _
      first#2 : _ -> <_> {error}
      $a#3 : Int
      $b#4 : Int
      plus#5 : Int -> <_> Int -> <_> Int
    ---
    E0004 6:23 field access is not supported yet
      6:23 this is implemented in a later stage
    ");
}

#[test]
fn an_equality_section_compares_by_the_type_it_is_used_at() {
    let text = "ints : Int -> Bool\nints = (== 1)\n\nstrings : String -> Bool\nstrings = (== \"a\")\n\neq : Bool -> Bool -> Bool\neq = (==)";
    let checked = eml_test_support::check(text);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
}

#[test]
fn an_equality_reference_with_an_unknown_type_is_not_comparable() {
    let text = "f : Int -> Int\nf x =\n  let eq = (==)\n  x";
    let checked = eml_test_support::check(text);
    let lines = eml_test_support::short(&checked.files, &checked.diagnostics);
    assert!(
        lines.iter().any(|line| line.starts_with("E2006 3:13")),
        "{lines:?}"
    );
}

#[test]
fn a_broken_signature_adds_no_effect_errors_to_the_equation() {
    // 壊れたシグネチャや足りない矢印の後の本体は、ラムダと同じく末尾が `Error` の row で検査する
    let text =
        "f : Int -> Undefined\nf a b = println \"x\"\n\ng : Int -> Unit\ng a b = println \"x\"";
    let checked = eml_test_support::check(text);
    let lines = eml_test_support::short(&checked.files, &checked.diagnostics);
    assert!(
        lines.iter().any(|line| line.starts_with("E1002")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.starts_with("E2001")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.starts_with("E2002")),
        "{lines:?}"
    );
}

#[test]
fn an_annotated_pattern_must_have_the_type_of_its_value() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf (Some (x : String)) = 1\nf None = 0";
    let checked = eml_test_support::check(text);
    insta::assert_snapshot!(eml_test_support::full(&checked.files, &checked.diagnostics), @"
    E2001 4:9 mismatched types
      4:9 expected `Int`, found `String`
      note: an annotated pattern must have the type of the value it matches
    ");
}

#[test]
fn a_user_extern_adds_no_type_error() {
    // ユーザーのモジュールの extern は E1033 の後も宣言として使え、型検査は誤りを重ねない
    let text = "extern f : Int -> Int\n\ng : Int -> Int\ng x = f x\n\nextern data T\n\nh : T -> T\nh x = x\n\nextern effect E\n\nk : Unit -> <E> Unit\nk () = ()";
    let checked = eml_test_support::check(text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E1033", "E1033", "E1033"]);
}

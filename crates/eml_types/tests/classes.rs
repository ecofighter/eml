//! 型クラスの制約の解決 (docs/spec/types.md の「制約」)。

use crate::common::check_text;
use eml_test_support::{check, short};

const SAME: &str = "class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n  differ x y = not (same x y)\n\n";
const COLOR: &str = "data Color =\n  | Red\n  | Green\n\n";

fn lines(text: &str) -> Vec<String> {
    let checked = check(text);
    short(checked.files(), &checked.diagnostics)
}

#[test]
fn methods_and_constrained_functions_check() {
    let text = format!(
        "{SAME}{COLOR}instance Same Color where\n  same Red Red = True\n  same Green Green = True\n  same _ _ = False\n\nboth : Same a => a -> a -> a -> Bool\nboth x y z = same x y && same y z\n\nmain : Unit -> <IO> Unit\nmain () = if both Red Red Green then println \"y\" else println \"n\""
    );
    insta::assert_snapshot!(check_text(&text), @"
    same : a -> a -> Bool
      kinds: a <= Unr
    differ : a -> a -> Bool
      kinds: a <= Unr
    both : a -> a -> a -> Bool
      kinds: a <= Unr
      x#0 : a
      y#1 : a
      z#2 : a
    main : Unit -> <IO> Unit
    differ : a -> a -> Bool
      kinds: a <= Unr
      x#0 : a
      y#1 : a
    Same Color.same : Color -> Color -> Bool
      $0#0 : Color
      $1#1 : Color
    ");
}

#[test]
fn a_missing_instance_is_reported_at_the_reference() {
    let text = format!("{SAME}{COLOR}f : Color -> Bool\nf c = same c c");
    assert_eq!(
        lines(&text),
        ["E2006 11:7 no instance of `Same` for `Color`"]
    );
}

#[test]
fn a_type_variable_needs_the_constraint_in_the_signature() {
    let text = format!("{SAME}f : a -> Bool\nf x = same x x");
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 7:7 no instance of `Same` for `a`
      7:7 `same` requires `Same a`
      help: add `Same a =>` to the signature of `f`
    ");
}

#[test]
fn a_superclass_is_given_with_its_subclass() {
    let text = format!(
        "{SAME}class Same a => Order a where\n  less : a -> a -> Bool\n\nboth : Order a => a -> a -> Bool\nboth x y = less x y || same x y"
    );
    assert_eq!(lines(&text), Vec::<String>::new());
}

#[test]
fn an_instance_context_is_needed_where_the_instance_is_used() {
    let text = format!(
        "{SAME}data Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same (Box x) (Box y) = same x y\n\nf : Bool\nf = same (Box (fn x -> x + 1)) (Box (fn x -> x))"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 12:5 no instance of `Same` for `Int -> <_> Int`
      12:5 `same` requires `Same (Box (Int -> <_> Int))`
      note: `Same (Box (Int -> <_> Int))` needs `Same (Int -> <_> Int)`
    ");
}

#[test]
fn an_undecided_type_is_ambiguous_unless_the_body_has_another_error() {
    let text = format!("{SAME}f : Int -> Int\nf x =\n  let s = same\n  x");
    assert_eq!(
        lines(&text),
        ["E2009 8:11 cannot decide which instance of `Same` to use for `same`"]
    );
    let text = format!("{SAME}f : Int -> Int\nf x =\n  let s = same\n  x + \"1\"");
    let found = lines(&text);
    assert!(
        found.iter().all(|line| !line.starts_with("E2009")),
        "{found:?}"
    );
}

#[test]
fn a_missing_instance_wins_over_an_undecided_type_in_one_reference() {
    // `Same (Pair ?y (Int -> Int))` は先に `Same ?y` を解くが、決まった誤りの E2006 を報告する
    let text = format!(
        "{SAME}data Pair a b = | Pair a b\n\ninstance (Same a, Same b) => Same (Pair a b) where\n  same _ _ = True\n\nf : Int -> Int\nf x =\n  let g = fn y -> same (Pair y (fn z -> z + 1)) (Pair y (fn z -> z))\n  x"
    );
    assert_eq!(
        lines(&text),
        ["E2006 13:19 no instance of `Same` for `Int -> <_> Int`"]
    );
}

#[test]
fn a_missing_superclass_instance_is_reported_at_the_instance() {
    let text = format!(
        "{SAME}class Same a => Order a where\n  less : a -> a -> Bool\n\n{COLOR}instance Order Color where\n  less _ _ = False"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 13:16 no instance of `Same` for `Color`
      13:16 `Order` requires `Same`, its superclass
    ");
}

#[test]
fn a_superclass_instance_context_must_follow_from_the_instance_context() {
    // `Same (Box a)` の instance は `Same a` を求めるので、文脈のない `Order (Box a)` は上位クラスの instance を使えない
    let text = format!(
        "{SAME}class Same a => Order a where\n  less : a -> a -> Bool\n\ndata Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same (Box x) (Box y) = same x y\n\ninstance Order (Box a) where\n  less _ _ = False"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 14:16 no instance of `Same` for `a`
      14:16 the instance of `Same` for `Box a` requires `Same a`
      help: add `Same a` to the context of this instance
    ");
}

#[test]
fn a_derived_instance_cannot_take_the_superclass_context() {
    // 導出した instance は文脈を書けないので、文脈に足す help ではなく、手で書く help を出す
    let text =
        "data P a = | P Int deriving Ord\n\ninstance Eq a => Eq (P a) where\n  _ == _ = True";
    let checked = check(text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 1:29 no instance of `Eq` for `a`
      1:29 the instance of `Eq` for `P a` requires `Eq a`
      help: a derived instance cannot have `Eq a` in its context; write the instance by hand
    ");
}

/// 診断ごとの番号と help。
fn codes_and_helps(text: &str) -> Vec<(String, Vec<String>)> {
    check(text)
        .diagnostics
        .into_iter()
        .map(|diagnostic| (diagnostic.code.to_string(), diagnostic.help))
        .collect()
}

#[test]
fn a_method_defined_with_too_few_parameters_is_told_how_many_it_needs() {
    // メソッドのシグネチャはクラスが決めるので、`()` を取る関数に変える help は出せない。クラスの row がエフェクトを
    // 許す矢印まで引数を書けば、本体の row がその矢印の row になる
    let class =
        "class Combine a where\n  combine : a -> a -> <IO> a\n\ninstance Combine Int where\n";
    let help = "define `combine` with 2 parameters, so that it performs `IO` when it is called";
    for body in [
        "  combine =\n    println \"x\"\n    fn x y -> x + y",
        "  combine x =\n    println \"x\"\n    fn y -> x + y",
    ] {
        assert_eq!(
            codes_and_helps(&format!("{class}{body}")),
            [("E2002".to_string(), vec![help.to_string()])],
            "{body}"
        );
    }
}

#[test]
fn a_method_whose_class_row_lacks_the_effect_is_told_to_change_the_class() {
    // どの矢印の row もエフェクトを許さなければ、引数を書いても直らない。ユーザーのクラスならクラスの row を足す
    let text = "class Combine a where\n  combine : a -> a -> a\n\ninstance Combine Int where\n  combine =\n    println \"x\"\n    fn x y -> x + y";
    assert_eq!(
        codes_and_helps(text),
        [(
            "E2002".to_string(),
            vec!["the signature of `combine` comes from the class `Combine`; add `IO` to its row there, as in `-> <IO> ...`".to_string()]
        )]
    );
    // 標準ライブラリのクラスは書き換えられず、矢印のないメソッドには row がないので、help を出さない
    let text =
        "data C = | C\n\ninstance Show C where\n  show =\n    println \"x\"\n    fn _ -> \"o\"";
    assert_eq!(codes_and_helps(text), [("E2002".to_string(), vec![])]);
    let text = "class Zero a where\n  zero : a\n\ninstance Zero Int where\n  zero =\n    println \"x\"\n    1";
    assert_eq!(codes_and_helps(text), [("E2002".to_string(), vec![])]);
}

#[test]
fn a_method_with_parameters_is_told_to_change_the_class() {
    // instance のメソッドと既定のメソッドの型はクラスが決めるので、row を足す先はクラスのシグネチャである
    let helps = |text: &str| -> Vec<String> {
        check(text)
            .diagnostics
            .into_iter()
            .flat_map(|diagnostic| diagnostic.help)
            .collect()
    };
    let text = "class Combine a where\n  combine : a -> a -> a\n  twice : a -> a\n  twice x =\n    println \"x\"\n    combine x x\n\ninstance Combine Int where\n  combine x y =\n    println \"x\"\n    x + y";
    assert_eq!(
        helps(text),
        [
            "the signature of `twice` comes from the class `Combine`; add `IO` to its row there, as in `-> <IO> ...`",
            "the signature of `combine` comes from the class `Combine`; add `IO` to its row there, as in `-> <IO> ...`",
        ]
    );
    // 標準ライブラリのクラスは書き換えられないので、help を出さない
    let text = "data C = | C\n\ninstance Show C where\n  show _ =\n    println \"x\"\n    \"C\"";
    let checked = check(text);
    let found: Vec<(String, usize)> = checked
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.to_string(), diagnostic.help.len()))
        .collect();
    assert_eq!(found, [("E2002".to_string(), 0)]);
}

#[test]
fn instance_and_default_bodies_are_checked_at_their_types() {
    let text = format!(
        "class Size a where\n  size : a -> Int\n  twice : a -> Int\n  twice x = size x ++ \"\"\n\n{COLOR}instance Size Color where\n  size _ = \"one\""
    );
    insta::assert_snapshot!(lines(&text).join("\n"), @"
    E2001 4:13 mismatched types
    E2001 4:13 mismatched types
    E2001 11:12 mismatched types
    ");
}

#[test]
fn method_own_variables_can_carry_constraints() {
    let text = format!(
        "{SAME}class Pick a where\n  pick : Same b => a -> b -> b -> b\n\n{COLOR}instance Pick Color where\n  pick _ x y = if same x y then x else y"
    );
    assert_eq!(lines(&text), Vec::<String>::new());
}

#[test]
fn a_head_variable_and_a_method_variable_with_the_same_name_stay_apart() {
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> (b -> Int -> b) -> b\n\ninstance Fold (Box b) where\n  fold (Box _) acc step = step acc 1";
    assert_eq!(lines(text), Vec::<String>::new());
    // 頭の `b` と、付け替えたメソッドの `b1` を取り違えると、型が合わずに E2001 になる
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> (b -> Int -> b) -> b\n\ninstance Fold (Box b) where\n  fold (Box item) _ _ = item";
    assert!(lines(text)[0].starts_with("E2001"), "{:?}", lines(text));
}

#[test]
fn a_clause_variable_has_no_instance() {
    let text = format!(
        "{SAME}effect Pick where\n  pick : a -> a\n\nrun : Int -> Int\nrun v =\n  handle pick v with\n    | pick x k -> if same x x then k x else k x"
    );
    assert!(lines(&text)[0].starts_with("E2006"), "{:?}", lines(&text));
}

#[test]
fn a_constrained_variable_is_unrestricted() {
    // `C a` は `a ≤ Unr` を意味するので、本体が `x` を2回使っても E3002 にならない
    let text = format!("{SAME}twice : Same a => a -> Bool\ntwice x = same x x");
    assert_eq!(lines(&text), Vec::<String>::new());
    assert_eq!(crate::common::kinds(&text, "twice"), "  kinds: a <= Unr");
}

#[test]
fn resolving_an_instance_requires_unrestricted_arguments() {
    // `G b` は `b` によらず `Unr` だが、instance の本体は頭の型変数を `Unr` として検査するので、解くたびに型引数に
    // `Unr` を求める (docs/spec/types.md の「`Unr` のクラス」)
    let text = "class Size a where\n  size : a -> Int\n\ndata G b = | G (Unit -> <IO> b)\n\ninstance Size (G b) where\n  size _ = 0\n\nf : Unit -> <IO> Int\nf () = size (G (fn () -> Fs.open \"x\"))";
    let found = lines(text);
    assert!(
        found.iter().any(|line| line.starts_with("E30")),
        "{found:?}"
    );
}

#[test]
fn a_recursive_reference_at_a_linear_type_is_reported_at_the_reference() {
    // 再帰した参照が `a` を線形な型で具体化すると、シグネチャの `a ≤ Unr` も破れる。その違反は報告せず、参照の誤りだけを出す
    let text = "class Same a where\n  same : a -> a -> Bool\n\ndata Box a = | Box a\n\ninstance Same (Box a) where\n  same _ _ = True\n\nf : Same a => a -> <IO> Int\nf x = f (Box (Fs.open \"x\"))\n\ng : Same a => a -> <IO> Int\ng x = g (Fs.open \"x\")";
    let found = lines(text);
    assert!(
        found.iter().any(|line| line.starts_with("E3001 10:7")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|line| line.starts_with("E2006 13:7")),
        "{found:?}"
    );
}

#[test]
fn a_linear_head_cannot_have_an_instance() {
    let text = "class Size a where\n  size : a -> Int\n\ndata H = | H Fs.File\n\ninstance Size H where\n  size _ = 0\n\ninstance Size Fs.File where\n  size _ = 0";
    let checked = check(text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2010 6:15 `H` is linear, so it cannot have an instance of `Size`
      6:15 a linear type
      note: the methods of a class may copy or drop their arguments, which a linear value forbids
    E2010 9:15 `File` is linear, so it cannot have an instance of `Size`
      9:15 a linear type
      note: the methods of a class may copy or drop their arguments, which a linear value forbids
    ");
}

#[test]
fn an_instance_method_cannot_require_more_than_the_class() {
    let text = "class Pairable a where\n  pair_with : a -> b -> (b, b)\n\ninstance Pairable Int where\n  pair_with _ y = (y, y)";
    assert_eq!(
        lines(text),
        [
            "E2011 5:3 `pair_with` in the instance for `Int` needs more than the signature of `pair_with` allows"
        ]
    );
}

#[test]
fn a_default_or_a_carried_value_can_need_more_than_the_class() {
    // 既定のメソッドも同じ規則で確かめる。メソッド自身の型変数の値を持ったまま row の呼び出しをまたぐと `carry(b, e)` が要る
    let text = "class Pairable a where\n  pair_with : a -> b -> (b, b)\n  pair_with _ y = (y, y)\n\nclass Keep a where\n  keep : a -> b -> (Unit -> <e> Unit) -> <e> b\n\ninstance Keep Int where\n  keep _ x action =\n    action ()\n    x";
    assert_eq!(
        lines(text),
        [
            "E2011 3:3 the default `pair_with` needs more than the signature of `pair_with` allows",
            "E2011 9:3 `keep` in the instance for `Int` needs more than the signature of `keep` allows",
        ]
    );
}

#[test]
fn a_callback_parameter_of_a_method_is_unrestricted() {
    // メソッドのシグネチャに書いた関数型の矢印は `Unr` に固定するので、instance がコールバックを2回呼んでも E2011 にならない
    let text = "class Each a where\n  each : a -> (Int -> Unit) -> Unit\n\ninstance Each Int where\n  each n f =\n    f n\n    f n";
    assert_eq!(lines(text), Vec::<String>::new());
}

#[test]
fn ordinary_instances_and_defaults_have_no_kind_error() {
    let text = format!(
        "{SAME}data Proxy a = | Proxy\n\ninstance Same a => Same (Proxy a) where\n  same _ _ = True\n\ndata Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same = same_box\n\nsame_box : Same a => Box a -> Box a -> Bool\nsame_box (Box x) (Box y) = same x y"
    );
    assert_eq!(lines(&text), Vec::<String>::new());
}

#[test]
fn constrained_polymorphic_recursion_is_rejected() {
    let text = format!(
        "{SAME}depth : Same a => Int -> a -> Int\ndepth n x = if n == 0 then 0 else depth (n - 1) (x, x)"
    );
    let checked = check(&text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2012 6:1 `depth` would need an instance of `Same` at infinitely many types
      6:1 a constrained type variable grows on each recursive call
      note: instances are chosen at compile time, so a constraint cannot follow polymorphic recursion
    E2006 7:35 no instance of `Same` for `(a, a)`
      7:35 `depth` requires `Same (a, a)`
    ");
    let text = "class C a where\n  m : Show2 b => a -> Int -> b -> Int\n\nclass Show2 a where\n  show2 : a -> Int\n\ninstance C Int where\n  m x n y = if n == 0 then show2 y else m x (n - 1) (y, y)";
    assert!(
        lines(text).iter().any(|line| line.starts_with("E2012")),
        "{:?}",
        lines(text)
    );
}

#[test]
fn a_derived_field_without_an_instance_is_reported_at_the_deriving_class() {
    // 誤りは `deriving` のクラス名を指し、どのフィールドの制約かを note で示す
    let text = "data Box a = | Box a deriving Show\n\ndata Wrap = | Wrap Int (Box (Int -> Int)) deriving Show";
    let checked = check(text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 3:52 no instance of `Show` for `Int -> Int`
      3:52 `deriving Show` needs it for a field of `Wrap`
      note: the field of type `Box (Int -> Int)` needs `Show (Box (Int -> Int))`
    ");
}

#[test]
fn deriving_ord_needs_eq() {
    let text = "data Color =\n  | Red\n  | Green\n  deriving Ord";
    let checked = check(text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2006 4:12 no instance of `Eq` for `Color`
      4:12 `Ord` requires `Eq`, its superclass
    ");
    let text = "data Color =\n  | Red\n  | Green\n  deriving (Eq, Ord)";
    assert_eq!(lines(text), Vec::<String>::new());
}

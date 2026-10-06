mod common;

use common::check_text;

#[test]
fn applied_types_are_displayed_with_their_arguments() {
    let text = "data Option a =\n  | None\n  | Some a\n\ndata List a =\n  | Nil\n  | Cons a (List a)\n\nwrap : Int -> Option Int\nwrap n = Some n\n\nkeep : List (Option a) -> List (Option a)\nkeep xs = xs\n\nfuns : Option (Int -> Int) -> Option (Int -> Int)\nfuns f = f";
    insta::assert_snapshot!(check_text(text), @r"
    wrap : Int -> Option Int
      n#0 : Int
    keep : List (Option a) -> List (Option a)
      xs#0 : List (Option a)
    funs : Option (Int -> Int) -> Option (Int -> Int)
      f#0 : Option (Int -> Int)
    ");
}

#[test]
fn the_kind_of_a_data_type_is_the_kind_of_its_effective_arguments() {
    // `Option a` と再帰する `List a` は `a` の Kind を持つ。関数型のフィールドの矢印は `Unr` なので `Fun a` は `a` に
    // よらず、フィールドのない `Tag a` も `a` によらない
    let text = "data Option a =\n  | None\n  | Some a\n\ndata List a =\n  | Nil\n  | Cons a (List a)\n\ndata Fun a =\n  | Fun (a -> a)\n\ndata Tag a =\n  | Tag\n\nopt : Option a -> (Option a -> Option a -> b) -> b\nopt x g = g x x\n\nlist : List a -> (List a -> List a -> b) -> b\nlist x g = g x x\n\nfun : Fun a -> (Fun a -> Fun a -> b) -> b\nfun x g = g x x\n\ntag : Tag a -> (Tag a -> Tag a -> b) -> b\ntag x g = g x x";
    insta::assert_snapshot!(check_text(text), @r"
    opt : Option a -> (Option a -> Option a -> b) -> b
      kinds: a <= Unr
      x#0 : Option a
      g#1 : Option a -> Option a -> b
    list : List a -> (List a -> List a -> b) -> b
      kinds: a <= Unr
      x#0 : List a
      g#1 : List a -> List a -> b
    fun : Fun a -> (Fun a -> Fun a -> b) -> b
      x#0 : Fun a
      g#1 : Fun a -> Fun a -> b
    tag : Tag a -> (Tag a -> Tag a -> b) -> b
      x#0 : Tag a
      g#1 : Tag a -> Tag a -> b
    ");
}

#[test]
fn a_constructor_pattern_of_another_type_is_a_mismatch() {
    let text = "data Option a =\n  | None\n  | Some a\n\ndata List a =\n  | Nil\n  | Cons a (List a)\n\nfirst : Option Int -> Int\nfirst o = match o with\n  | Cons x _ -> x\n  | _ -> 0";
    insta::assert_snapshot!(check_text(text), @r"
    first : Option Int -> Int
      o#0 : Option Int
      x#1 : {error}
    ---
    E2001 11:5 mismatched types
      11:5 expected `Option Int`, found `List _`
      note: `Cons` is a constructor of `List`
    ");
}

#[test]
fn match_arms_have_one_type() {
    let text = "data Color =\n  | Red\n  | Green\n\nname : Color -> String\nname c = match c with\n  | Red -> \"red\"\n  | Green -> 2\n\nsize : Color -> Int\nsize c =\n  let n = match c with\n    | Red -> 1\n    | Green -> \"two\"\n  n";
    insta::assert_snapshot!(check_text(text), @r#"
    name : Color -> String
      c#0 : Color
    size : Color -> Int
      c#0 : Color
      n#1 : Int
    ---
    E2001 8:14 mismatched types
      8:14 expected `String`, found `Int`
      5:8 expected because of the signature of `name`
    E2001 14:16 mismatched types
      14:16 expected `Int`, found `String`
      13:14 the first arm has this type
    "#);
}

#[test]
fn constructors_are_values_and_can_be_partially_applied() {
    let text = "data Option a =\n  | None\n  | Some a\n\ndata Pair a b =\n  | Pair a b\n\napply : (a -> b) -> a -> b\napply f x = f x\n\nboxed : Int -> Option Int\nboxed n = apply Some n\n\npairs : Unit -> Pair Int String\npairs () =\n  let make = Pair 1\n  make \"one\"";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (a -> b) -> a -> b
      f#0 : a -> b
      x#1 : a
    boxed : Int -> Option Int
      n#0 : Int
    pairs : Unit -> Pair Int String
      make#0 : String -> Pair Int String
    ");
}

#[test]
fn match_arms_are_paths_and_discarded_fields_are_unrestricted() {
    // `_` で捨てたフィールドと使わない変数の型は `Unr` になる。`match` の枝は別の経路なので、片方の枝だけで使う `d`
    // も `Unr` になる。各枝で1回ずつ使う `x` は制約を作らない
    let text = "data Option a =\n  | None\n  | Some a\n\ndata Box a =\n  | Box a\n\nskip : Box a -> Int\nskip b = match b with\n  | Box _ -> 1\n\nunused : Box a -> Int\nunused b = match b with\n  | Box x -> 1\n\npick : Option a -> a -> a\npick o d = match o with\n  | Some x -> x\n  | None -> d\n\nsame : Option a -> Option a\nsame o = match o with\n  | Some x -> Some x\n  | None -> None";
    insta::assert_snapshot!(check_text(text), @r"
    skip : Box a -> Int
      kinds: a <= Unr
      b#0 : Box a
    unused : Box a -> Int
      kinds: a <= Unr
      b#0 : Box a
      x#1 : a
    pick : Option a -> a -> a
      kinds: a <= Unr
      o#0 : Option a
      d#1 : a
      x#2 : a
    same : Option a -> Option a
      o#0 : Option a
      x#1 : a
    ");
}

#[test]
fn user_constructors_shadow_the_prelude() {
    // `True` はユーザーの `Answer` のコンストラクタを指す。`||` の脱糖は Prelude の `True` を使うので、条件は `Bool` のまま。名前で引くと `Answer` になり、lang item で引くと `Bool` になる
    let text = "data Answer =\n  | True\n  | No\n\nreply : Bool -> Answer\nreply b = if b || b then True else No";
    insta::assert_snapshot!(check_text(text), @r"
    reply : Bool -> Answer
      b#0 : Bool
    ");
}

#[test]
fn constructors_of_a_duplicate_data_type_do_not_cascade() {
    let text = "data T = | A\n\ndata T = | B\n\nf : T\nf = B";
    insta::assert_snapshot!(check_text(text), @"
    f : T
    ---
    E1003 3:6 `T` is defined more than once
      3:6 defined again here
      1:6 first defined here
    ");
}

#[test]
fn a_user_bool_hides_the_prelude_bool() {
    // status.md の「同じ名前の別の型を区別して表示しない」の既知の制限。Prelude と入口のモジュールが分かれても同じに
    // 振る舞う
    let text = "data Bool = | False | True\nf : Int -> Int\nf x = if True then x else 0";
    let checked = eml_test_support::check(text);
    assert_eq!(
        eml_test_support::short(&checked.files, &checked.diagnostics),
        ["E2001 3:10 mismatched types"]
    );
}

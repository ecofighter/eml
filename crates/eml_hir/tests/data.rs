//! `data` の宣言、コンストラクタ、型の適用、パターン、`match` の変換。

use crate::common::{diagnostics, lower_text};
use eml_hir::{Body, ExprKind, Program, TypeDefKind};

#[test]
fn data_declarations_become_items() {
    let text = "data Option a =\n  | None\n  | Some a\n\ndata List a = | Nil | a :+ List a\n\nf : Option Int -> List (Option Int)\nf o = Some 1 :+ Nil";
    insta::assert_snapshot!(lower_text(text), @r"
    data Option a
      | None
      | Some a
    data List a
      | Nil
      | a :+ List a
    f : Option Int -> List (Option Int)
    f o#0 = (:+ (Some 1) Nil)
    ");
}

#[test]
fn match_arms_and_constructor_patterns() {
    let text = "data Option a = | None | Some a\n\nf : Option (Option Int) -> Int\nf o = match o with\n  | Some (Some n) -> n\n  | Some None -> 1\n  | None -> 0\n\ng : Option Int -> Int\ng (Some x) =\n  let Some y = Some x\n  (fn (Some z) -> z) (Some y)";
    insta::assert_snapshot!(lower_text(text), @r"
    data Option a
      | None
      | Some a
    f : Option (Option Int) -> Int
    f o#0 = (match o#0 with | Some (Some n#1) -> n#1 | Some None -> 1 | None -> 0)
    g : Option Int -> Int
    g (Some x#0) = {
      let Some y#1 = (Some x#0)
      ((fn (Some z#2) -> z#2) (Some y#1))
    }
    ");
}

#[test]
fn infix_constructors_and_a_declared_cons() {
    // ユーザーの `::` は Prelude の `::` を隠すので、宣言がなければ `infixl 9` になる。右に組むには自分の fixity の宣言がいる (docs/spec/declarations.md の「fixity」)
    let text = "infixr 5 ::\ndata L = | E | Int :: L\n\nf : Int -> L\nf x = x :: x :: E\n\ng : L -> Int\ng l = match l with | h :: _ -> h | E -> 0";
    insta::assert_snapshot!(lower_text(text), @r"
    data L
      | E
      | Int :: L
    f : Int -> L
    f x#0 = (:: x#0 (:: x#0 E))
    g : L -> Int
    g l#0 = (match l#0 with | h#1 :: _ -> h#1 | E -> 0)
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
fn match_arms_bind_their_pattern_variables_only_in_the_arm() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int -> Int\nf o k = (fn u -> match o with | Some x -> x + k | None -> u) 0";
    let module = eml_test_support::lower_clean(text).program;
    let body = body(&module, "f");
    let names = |locals: Vec<eml_hir::LocalId>| -> Vec<String> {
        locals
            .iter()
            .map(|&local| body.locals[local].name.clone())
            .collect()
    };
    let (_, expr) = body
        .exprs
        .iter()
        .find(|(_, expr)| matches!(expr.kind, ExprKind::Lambda(_)))
        .expect("the lambda");
    let ExprKind::Lambda(closure) = &expr.kind else {
        unreachable!();
    };
    // 枝のパターンの `x` は捕まえる変数に入らない
    assert_eq!(names(body.closure_captures(closure)), ["o", "k"]);
    let (id, arms) = body
        .exprs
        .iter()
        .find_map(|(id, expr)| match &expr.kind {
            ExprKind::Match { arms, .. } => Some((id, arms)),
            _ => None,
        })
        .expect("the match");
    assert_eq!(names(body.pat_bindings(arms[0].pat)), ["x"]);
    let mut children = Vec::new();
    body.walk_child_exprs(id, |child| children.push(child));
    assert_eq!(children.len(), 3, "the scrutinee and the two arm bodies");
}

#[test]
fn infix_constructor_patterns_group_like_expressions() {
    // fixity の宣言がない `:+` は `infixl 9` なので、パターンも式も左に組む (docs/spec/declarations.md)
    let text = "data T = | L | T :+ Int\n\nf : T -> T\nf t = match t with | a :+ b :+ c -> a :+ b :+ c | L -> L";
    insta::assert_snapshot!(lower_text(text), @r"
    data T
      | L
      | T :+ Int
    f : T -> T
    f t#0 = (match t#0 with | (a#1 :+ b#2) :+ c#3 -> (:+ (:+ a#1 b#2) c#3) | L -> L)
    ");
}

#[test]
fn type_names_and_constructors_must_be_unique() {
    let text = "data A = | X | X\n\ndata B a a = | Y\n\ndata C = | X\n\ndata A = | Z\n\neffect C where\n  op : Unit -> Unit";
    assert_eq!(
        diagnostics(text),
        [
            "E1003 1:16 `X` is defined more than once",
            "E1003 3:10 `a` is defined more than once",
            "E1003 5:12 `X` is defined more than once",
            "E1003 7:6 `A` is defined more than once",
            "E1003 9:8 `C` is defined more than once",
        ]
    );
}

#[test]
fn field_types_and_type_arguments_are_checked() {
    let text = "data P a = | P a b (Int -> <e> Int)\n\ndata Q = | Q (P Int Int) P (Int Int)\n\nf : P -> Int\nf x = 1";
    assert_eq!(
        diagnostics(text),
        [
            "E1002 1:18 cannot find type variable `b`",
            "E1002 1:29 cannot find row variable `e`",
            "E1015 3:15 `P` takes 1 type argument, but 2 were given",
            "E1015 3:26 `P` takes 1 type argument, but 0 were given",
            "E1015 3:29 `Int` takes 0 type arguments, but 1 was given",
            "E1015 5:5 `P` takes 1 type argument, but 0 were given",
        ]
    );
}

#[test]
fn constructor_patterns_are_resolved_and_counted() {
    // 名前の引けないパターンの中の変数 (`a`) も束縛するので、枝の本体で E1001 を連鎖させない
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o = match o with\n  | Some x y -> x\n  | None z -> 0\n  | Nothing -> 0\n  | Some (Pair a b) -> a";
    assert_eq!(
        diagnostics(text),
        [
            "E1016 5:5 `Some` takes 1 argument, but 2 were given",
            "E1016 6:5 `None` takes 0 arguments, but 1 was given",
            "E1001 7:5 cannot find constructor `Nothing`",
            "E1001 8:11 cannot find constructor `Pair`",
        ]
    );
}

#[test]
fn a_name_is_bound_once_per_pattern_and_per_equation() {
    // 別の `let` の束縛は別の組なので、引数の `p` を隠してよい
    let text = "data Pair a b = | Pair a b\n\nf : Int -> Int -> Int\nf x x = x\n\ng : Pair Int Int -> Int\ng p =\n  let Pair y y = p\n  let p = y\n  p";
    assert_eq!(
        diagnostics(text),
        [
            "E1017 4:5 `x` is bound more than once",
            "E1017 8:14 `y` is bound more than once",
        ]
    );
}

#[test]
fn bool_is_a_data_type_of_the_prelude() {
    let lowered = eml_test_support::lower_clean("f : Bool -> Bool\nf b = b && True");
    let program = &lowered.program;
    let TypeDefKind::Data { constructors } = &program[program.lang.bool].kind else {
        panic!("`Bool` is a data type");
    };
    let tags: Vec<(&str, u32)> = constructors
        .iter()
        .map(|&id| (program[id].name.as_str(), program[id].tag))
        .collect();
    assert_eq!(tags, [("False", 0), ("True", 1)]);
    assert_eq!(program[program.lang.false_ctor].name, "False");
    assert_eq!(program[program.lang.true_ctor].name, "True");
}

#[test]
fn data_without_constructors_is_reported_in_a_user_module() {
    assert_eq!(
        diagnostics("data Empty\nf : Empty -> Int\nf e = 1"),
        ["E1025 1:6 `Empty` has no constructors"]
    );
}

#[test]
fn data_missing_its_equals_sign_is_not_reported_as_having_no_constructors() {
    assert_eq!(
        diagnostics("data T | A | B\nf : T\nf = A"),
        ["E0011 1:8 expected `=`"]
    );
}

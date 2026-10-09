//! 一様な位置のグラフ (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「一様な位置と制約付きの多相再帰」)。

use eml_test_support::{check, short};

fn uniform_functions(text: &str) -> Vec<(String, usize)> {
    let checked = check(text);
    assert!(
        checked.diagnostics.iter().all(|d| !d.is_error()),
        "{:?}",
        checked.diagnostics
    );
    let program = &checked.program;
    let mut found: Vec<(String, usize)> = program
        .functions()
        .flat_map(|(id, function)| {
            let count = function
                .signature
                .as_ref()
                .map_or(0, |s| s.generics.type_vars.len());
            let uniform = &checked.typed.uniform;
            (0..count)
                .filter(move |&i| uniform.function(id, i))
                .map(move |i| (function.name.clone(), i))
        })
        .collect();
    found.sort();
    found
}

#[test]
fn polymorphic_recursion_without_constraints_is_uniform() {
    let text =
        "depth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else 1 + depth (n - 1) (x, x)";
    assert_eq!(uniform_functions(text), [("depth".to_string(), 0)]);
}

#[test]
fn another_method_of_the_same_instance_at_a_larger_type_is_not_a_cycle() {
    // メソッドの参照は解決先の関数にだけ辺を引くので、`!=` から `==` への参照は循環にならない (spec の辺の 2.)
    let text = "class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n\ndata Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same (Box x) (Box y) = same x y\n  differ p q = not (same (Box p) (Box q))";
    assert_eq!(uniform_functions(text), Vec::<(String, usize)>::new());
}

#[test]
fn growth_through_a_method_variable_is_found() {
    // instance が `b` の値を写すか捨てると E2011 になるので、`b` によらず `Unr` な `Proxy b` を大きくする
    let text = "data Proxy b = | Proxy\n\ngrow : Proxy b -> Proxy (b, b)\ngrow _ = Proxy\n\nclass C a where\n  m : a -> Int -> Proxy b -> Int\n\ninstance C Int where\n  m x n p = if n == 0 then 0 else m x (n - 1) (grow p)";
    assert_eq!(uniform_functions(text), [("C Int.m".to_string(), 0)]);
}

#[test]
fn growth_through_a_constrained_call_is_found() {
    // `g` の制約 `Show2 (Box (Box a))` を解くと、instance の節点へ大きくなる辺が引かれ、instance のメソッドから `f` に
    // 戻る。単相化すると `f@[T]`、`g@[Box (Box T)]`、`Show2 Box.show2@[Box T]`、`f@[Box T]` と止まらない
    let text = "class Show2 a where\n  show2 : a -> Int\n\ndata Box a = | Box a\n\ninstance Show2 a => Show2 (Box a) where\n  show2 (Box x) = f x\n\nf : Show2 a => a -> Int\nf x = g (Box (Box x))\n\ng : Show2 a => a -> Int\ng x = show2 x";
    let checked = check(text);
    let lines = short(checked.files(), &checked.diagnostics);
    assert!(
        lines.iter().any(|line| line.starts_with("E2012")),
        "{lines:?}"
    );
}

#[test]
fn a_default_method_reaches_the_instance_it_was_resolved_from() {
    // 既定のメソッドの `differ` は、与えられた `Same a` の証拠として同じ instance の `same` を呼ぶ。単相化すると
    // `Same Box.same@[T]`、`differ@[Box (Box T)]`、`Same Box.same@[Box T]` と止まらない
    let text = "class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n  differ x y = not (same x y)\n\ndata Box a = | Box a\n\ninstance Same a => Same (Box a) where\n  same (Box x) (Box y) = differ (Box (Box x)) (Box (Box y))";
    let checked = check(text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2012 8:25 `Same (Box a)` would need an instance of `Same` at infinitely many types
      8:25 a constrained type variable grows on each recursive call
      note: instances are chosen at compile time, so a constraint cannot follow polymorphic recursion
    ");
}

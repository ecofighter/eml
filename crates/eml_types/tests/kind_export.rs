//! 書き出した型の線形性 (docs/spec/types.md の「書き出す型の形」
//! 「宣言の Kind 変数の番号」「値の規則」「`Flexible` と `OpVar` の Kind」)。`dump_linear` は矢印の `Lin` を `%1`、
//! `Poly` を `%m`、`Var(i)` を `%m{i}` と書き、`Unr` は書かない。`Flexible` と `OpVar` は名前の後に同じ印で Kind を書く。
//! 番号は宣言の `Shape` の番号 (型変数の `μ` と矢印の `m` を外側から数えた順) で、トップレベルの関数のシグネチャの一番外の
//! 矢印は `Unr` に固定されていて番号を持たない。

use eml_test_support::{check, short_text};

/// 誤りのないプログラムの、線形性を表示するダンプ。
fn exported(text: &str) -> String {
    let checked = check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        short_text(checked.files(), &checked.diagnostics)
    );
    eml_types::dump_linear(&checked.program, &checked.typed)
}

#[test]
fn a_lambda_that_captures_nothing_is_unr() {
    // `main` は宣言自身の変数を持たないので、`f` の矢印は SCC の最小解をそのまま使う。下限がないので `Unr` になる
    let text =
        "main : Unit -> <IO> Unit\nmain () =\n  let f = fn x -> x + 1\n  println (show (f 1))";
    insta::assert_snapshot!(exported(text), @"
    main : Unit -> <IO> Unit
      x#0 : Int
      f#1 : Int -> <IO> Int
    ");
}

#[test]
fn a_lambda_that_captures_a_file_is_lin() {
    // `g` の矢印は `File` の `h` を捕まえて下限 `Lin` を持つので、最小解が `Lin` になる
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let h = Fs.open \"a.txt\"\n  let g = fn () -> Fs.close h\n  g ()";
    insta::assert_snapshot!(exported(text), @"
    main : Unit -> <IO> Unit
      h#0 : File
      g#1 : Unit %1 -> <IO> Unit
    ");
}

#[test]
fn a_body_variable_below_own_variables_joins_them() {
    // 番号: 0 = `f` の矢印、1 = `f` を受けた後の矢印、2 = `g` の矢印、3 = `g` を受けた後の矢印。`h` の矢印は `f` と
    // `g` を捕まえるので 0 ≤ m と 2 ≤ m を持ち、自分の変数と同じ成分にはない。下にたどると 0 と 2 に届くので、
    // 「書き出すときの規則」の4つ目で `Var(4)` になり、`joins[0] = {0, 2}` を記録する。`f` と `g` はどれも1回だけ
    // 使うので、定数には畳まれない
    let text = "pair : (Int -> Int) -> (Int -> Int) -> Int -> Int\npair f g x =\n  let h = fn y -> f (g y)\n  h x";
    insta::assert_snapshot!(exported(text), @"
    pair : (Int %m0 -> Int) -> (Int %m2 -> Int) %m1 -> Int %m3 -> Int
      joins: %m4 = %m0 | %m2
      f#0 : Int %m0 -> Int
      g#1 : Int %m2 -> Int
      x#2 : Int
      y#3 : Int
      h#4 : Int %m4 -> Int
    ");
}

#[test]
fn a_body_variable_in_an_own_variable_component_takes_its_var() {
    // `f` の番号: 0 = `eat` の矢印、1 = `μ_a`、2 = `eat` を受けた後の矢印、3 = `x` を受けた後の矢印。再帰の呼び出しは
    // `a` を `Unit -> a` で具体化し、その `μ` は同じ SCC なので `μ_a` と等しくなる。`fn () -> x` の矢印は `x` を
    // 捕まえて `μ_a` 以上で、引数の型の Kind として具体化の `μ` 以下なので、`μ_a` と同じ成分に入る。「書き出すときの
    // 規則」の1つ目で `%m1` になる。最初のラムダの `g` も同じ型の変数を通るので同じ矢印である。
    // `use_g` の番号: 0 = `g` の矢印、1 = `g` を受けた後の矢印。注釈 `(g : Int -> Int)` の矢印は `g` の矢印と単一化して
    // 等しいので、同じ成分の `%m0` になる
    let text = "f : (a -> <IO> Unit) -> a -> Int -> <IO> Unit\nf eat x n = if n == 0 then eat x else f (fn g -> eat (g ())) (fn () -> x) (n - 1)\n\nuse_g : (Int -> Int) -> Int -> Int\nuse_g g x =\n  let h = (g : Int -> Int)\n  h x";
    insta::assert_snapshot!(exported(text), @"
    f : (a %m0 -> <IO> Unit) -> a %m2 -> Int %m3 -> <IO> Unit
      eat#0 : a %m0 -> <IO> Unit
      x#1 : a
      n#2 : Int
      g#3 : Unit %m1 -> <IO> a
    use_g : (Int %m0 -> Int) -> Int %m1 -> Int
      g#0 : Int %m0 -> Int
      x#1 : Int
      h#2 : Int %m0 -> Int
    ");
}

#[test]
fn an_own_variable_fixed_by_constants_is_folded() {
    // 番号: 0 = `f` の矢印、1 = `f` を受けた後の矢印。`f` を2回呼ぶのでスキームに 0 ≤ Unr が残り、宣言の型でも本体の
    // 型でも `Unr` に畳む。1 は定数の境界を持たないので `%m1` のまま残る
    let text = "twice : (Int -> Int) -> Int -> Int\ntwice f x = f (f x)";
    insta::assert_snapshot!(exported(text), @"
    twice : (Int -> Int) -> Int %m1 -> Int
      kinds: (Int -> Int) <= Unr
      f#0 : Int -> Int
      x#1 : Int
    ");
}

#[test]
fn joins_are_numbered_in_export_order() {
    // 番号: 0 = `f`、2 = `g`、4 = `h`、6 = `k` の矢印、奇数はそれぞれを受けた後の矢印で、`lin_vars` は 8 である。
    // 式の ID の順に先に書き出す `p` の集合 {4, 6} が `%m8`、`q` の集合 {0, 2} が `%m9` になる。`r` は `p` を捕まえるので、
    // 下にたどって同じ集合 {4, 6} に届き、同じ番号 `%m8` を共有する
    let text = "four : (Int -> Int) -> (Int -> Int) -> (Int -> Int) -> (Int -> Int) -> Int -> Int\nfour f g h k x =\n  let p = fn y -> h (k y)\n  let q = fn y -> f (g y)\n  let r = fn y -> p y\n  r (q x)";
    insta::assert_snapshot!(exported(text), @"
    four : (Int %m0 -> Int) -> (Int %m2 -> Int) %m1 -> (Int %m4 -> Int) %m3 -> (Int %m6 -> Int) %m5 -> Int %m7 -> Int
      joins: %m8 = %m4 | %m6, %m9 = %m0 | %m2
      f#0 : Int %m0 -> Int
      g#1 : Int %m2 -> Int
      h#2 : Int %m4 -> Int
      k#3 : Int %m6 -> Int
      x#4 : Int
      y#5 : Int
      p#6 : Int %m8 -> Int
      y#7 : Int
      q#8 : Int %m9 -> Int
      y#9 : Int
      r#10 : Int %m8 -> Int
    ");
}

#[test]
fn a_clause_in_main_keeps_its_operation_variable_kinds() {
    // `put` の `a` は操作の引数の型に現れるので `OpVar` の Kind は `Unr` で、`a` と書く。`mk` の `a` は結果の型にだけ
    // 現れるので `Poly` で、`a%m` と書く (「`Flexible` と `OpVar` の Kind」)。`main` は型変数を持たないので、本体の型は
    // instance で写さず、この Kind を `linearity` がそのまま読む。`k` の矢印は、操作が `once` なので推論の前から `Lin`
    // (`%1`) である
    let text = "effect Put where\n  put : a -> Unit\n  mk : Unit -> List a\n\nbody : Unit -> <Put> Int\nbody () =\n  put 1\n  let xs = (mk () : List Int)\n  0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = handle body () with\n            | put x k -> k ()\n            | mk () k -> k []\n  println (show n)";
    insta::assert_snapshot!(exported(text), @"
    put : a -> <Put> Unit
      kinds: a <= Unr
    mk : Unit -> <Put> List a
    body : Unit -> <Put> Int
      xs#0 : List Int
    main : Unit -> <IO> Unit
      x#0 : a
      k#1 : Unit %1 -> <IO> Int
      k#2 : List a%m %1 -> <IO> Int
      $r#3 : Int
      n#4 : Int
    ");
}

#[test]
fn an_unresolved_variable_takes_the_least_value() {
    // tests/ui/run/files/file_in_return_clause_on_abort.em の形。使われない `return x` の `x` の型は推論で決まらず、
    // その `μ` は下限を持たないので、最小値の `Unr` の `Flexible` になり `_` と書く
    let text = "effect Fail where\n  never fail : Unit -> a\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"input.txt\"\n  handle fail () with\n    | fail () -> println \"aborted\"\n    | return x -> Fs.close f";
    insta::assert_snapshot!(exported(text), @"
    fail : Unit -> <Fail> a
    main : Unit -> <IO> Unit
      f#0 : File
      x#1 : _
    ");
}

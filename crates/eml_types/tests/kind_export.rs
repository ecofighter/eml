//! 書き出した型の線形性 (docs/spec/types.md の「書き出す型の形」
//! 「宣言の Kind 変数の番号」「値の規則」「`Flexible` と `OpVar` の Kind」)。`dump_linear` は矢印の `Lin` を `once`、
//! `Poly` を `poly`、`Var(i)` を `poly{i}` と頭に書き、`Unr` は書かない。`Flexible` と `OpVar` は名前の後に `:lin` / `:poly` で Kind を書く。
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
    let text = "main : Unit -> <IO, Panic> Unit\nmain () =\n  let f = fn x -> x + 1\n  println (show (f 1))";
    insta::assert_snapshot!(exported(text), @"
    main : Unit -> <IO, Panic> Unit
      x#0 : Int
      f#1 : Int -> <Panic> Int
    ");
}

#[test]
fn a_lambda_that_captures_a_file_is_lin() {
    // `g` の矢印は `File` の `h` を捕まえて下限 `Lin` を持つので、最小解が `Lin` になる
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let h = Fs.open \"a.txt\"\n  let g = fn () -> Fs.close h\n  g ()";
    insta::assert_snapshot!(exported(text), @"
    main : Unit -> <IO> Unit
      h#0 : File
      g#1 : once Unit -> <IO> Unit
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
    pair : (poly0 Int -> Int) -> poly1 (poly2 Int -> Int) -> poly3 Int -> Int
      joins: poly4 = poly0 | poly2
      f#0 : poly0 Int -> Int
      g#1 : poly2 Int -> Int
      x#2 : Int
      y#3 : Int
      h#4 : poly4 Int -> Int
    ");
}

#[test]
fn a_body_variable_in_an_own_variable_component_takes_its_var() {
    // `f` の番号: 0 = `eat` の矢印、1 = `μ_a`、2 = `eat` を受けた後の矢印、3 = `x` を受けた後の矢印。再帰の呼び出しは
    // `a` を `Unit -> a` で具体化し、その `μ` は同じ SCC なので `μ_a` と等しくなる。`fn () -> x` の矢印は `x` を
    // 捕まえて `μ_a` 以上で、引数の型の Kind として具体化の `μ` 以下なので、`μ_a` と同じ成分に入る。「書き出すときの
    // 規則」の1つ目で `poly1` になる。最初のラムダの `g` も同じ型の変数を通るので同じ矢印である。
    // `use_g` の番号: 0 = `g` の矢印、1 = `g` を受けた後の矢印。注釈 `(g : Int -> Int)` の矢印は `g` の矢印と単一化して
    // 等しいので、同じ成分の `poly0` になる
    let text = "f : (a -> <IO> Unit) -> a -> Int -> <IO, Panic> Unit\nf eat x n = if n == 0 then eat x else f (fn g -> eat (g ())) (fn () -> x) (n - 1)\n\nuse_g : (Int -> Int) -> Int -> Int\nuse_g g x =\n  let h = (g : Int -> Int)\n  h x";
    insta::assert_snapshot!(exported(text), @"
    f : (poly0 a -> <IO> Unit) -> poly2 a -> poly3 Int -> <IO, Panic> Unit
      eat#0 : poly0 a -> <IO> Unit
      x#1 : a
      n#2 : Int
      g#3 : poly1 Unit -> a
    use_g : (poly0 Int -> Int) -> poly1 Int -> Int
      g#0 : poly0 Int -> Int
      x#1 : Int
      h#2 : poly0 Int -> Int
    ");
}

#[test]
fn an_own_variable_fixed_by_constants_is_folded() {
    // 番号: 0 = `f` の矢印、1 = `f` を受けた後の矢印。`f` を2回呼ぶのでスキームに 0 ≤ Unr が残り、宣言の型でも本体の
    // 型でも `Unr` に畳む。1 は定数の境界を持たないので `poly1` のまま残る
    let text = "twice : (Int -> Int) -> Int -> Int\ntwice f x = f (f x)";
    insta::assert_snapshot!(exported(text), @"
    twice : (Int -> Int) -> poly1 Int -> Int
      kinds: (Int -> Int) <= Unr
      f#0 : Int -> Int
      x#1 : Int
    ");
}

#[test]
fn joins_are_numbered_in_export_order() {
    // 番号: 0 = `f`、2 = `g`、4 = `h`、6 = `k` の矢印、奇数はそれぞれを受けた後の矢印で、`lin_vars` は 8 である。
    // 式の ID の順に先に書き出す `p` の集合 {4, 6} が `poly8`、`q` の集合 {0, 2} が `poly9` になる。`r` は `p` を捕まえるので、
    // 下にたどって同じ集合 {4, 6} に届き、同じ番号 `poly8` を共有する
    let text = "four : (Int -> Int) -> (Int -> Int) -> (Int -> Int) -> (Int -> Int) -> Int -> Int\nfour f g h k x =\n  let p = fn y -> h (k y)\n  let q = fn y -> f (g y)\n  let r = fn y -> p y\n  r (q x)";
    insta::assert_snapshot!(exported(text), @"
    four : (poly0 Int -> Int) -> poly1 (poly2 Int -> Int) -> poly3 (poly4 Int -> Int) -> poly5 (poly6 Int -> Int) -> poly7 Int -> Int
      joins: poly8 = poly4 | poly6, poly9 = poly0 | poly2
      f#0 : poly0 Int -> Int
      g#1 : poly2 Int -> Int
      h#2 : poly4 Int -> Int
      k#3 : poly6 Int -> Int
      x#4 : Int
      y#5 : Int
      p#6 : poly8 Int -> Int
      y#7 : Int
      q#8 : poly9 Int -> Int
      y#9 : Int
      r#10 : poly8 Int -> Int
    ");
}

#[test]
fn a_clause_in_main_keeps_its_operation_variable_kinds() {
    // `put` の `a` は操作の引数の型に現れるので `OpVar` の Kind は `Unr` で、`a` と書く。`mk` の `a` は結果の型にだけ
    // 現れるので `Poly` で、`a:poly` と書く (「`Flexible` と `OpVar` の Kind」)。`main` は型変数を持たないので、本体の型は
    // instance で写さず、この Kind を `linearity` がそのまま読む。`k` の矢印は、操作が `once` なので推論の前から `Lin`
    // (`once`) である
    let text = "effect Put where\n  put : a -> Unit\n  mk : Unit -> List a\n\nbody : Unit -> <Put> Int\nbody () =\n  put 1\n  let xs = (mk () : List Int)\n  0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = handle body () with\n            | ctl put x k -> k ()\n            | ctl mk () k -> k []\n  println (show n)";
    insta::assert_snapshot!(exported(text), @"
    put : a -> <Put> Unit
      kinds: a <= Unr
    mk : Unit -> <Put> List a
    body : Unit -> <Put> Int
      xs#0 : List Int
    main : Unit -> <IO> Unit
      x#0 : a
      k#1 : once Unit -> <IO> Int
      k#2 : once List a:poly -> <IO> Int
      $r#3 : Int
      n#4 : Int
    ");
}

#[test]
fn an_unresolved_variable_takes_the_least_value() {
    // tests/ui/run/files/file_in_return_clause_on_abort.em の形。使われない `return x` の `x` の型は推論で決まらず、
    // その `μ` は下限を持たないので、最小値の `Unr` の `Flexible` になり `_` と書く
    let text = "effect Fail where\n  never fail : Unit -> a\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"input.txt\"\n  handle fail () with\n    | ctl fail () -> println \"aborted\"\n    | return x -> Fs.close f";
    insta::assert_snapshot!(exported(text), @"
    fail : Unit -> <Fail> a
    main : Unit -> <IO> Unit
      f#0 : File
      x#1 : _
    ");
}

#[test]
fn std_public_functions_keep_their_schemes() {
    // 補助関数をローカル関数に移しても、公開の関数の型は変わらない (docs/spec/stdlib.md)。標準ライブラリは `dump` に出ないので、
    // 型を直接読む。`display_linear` は矢印の線形性を書くが、`Flexible` と `OpVar` の Kind と宣言の Kind の制約は書かないので、
    // この表で確かめるのは矢印の線形性までである。
    let checked = check(
        "main : Unit -> <IO, Panic> Unit\nmain () = List.each (fn x -> println (String.join \",\" [\"a\"])) (String.lines \"a\")",
    );
    assert!(checked.diagnostics.is_empty());
    let public = [
        ("Std.List", "length"),
        ("Std.List", "map"),
        ("Std.List", "filter"),
        ("Std.List", "filter_map"),
        ("Std.String", "split"),
        ("Std.String", "lines"),
        ("Std.String", "join"),
        ("Std.String", "trim"),
        ("Std.String", "to_int"),
        ("Std.String", "starts_with"),
        ("Std.String", "ends_with"),
    ];
    let program = &checked.program;
    let mut shown = String::new();
    for (id, _) in program.functions() {
        let item = eml_hir::ValueItem::Function(id);
        let module = program.modules[id.module].name.to_string();
        let name = program.value_name(item);
        if !public.contains(&(module.as_str(), name)) {
            continue;
        }
        let ty = checked.typed.decls[&item].ty;
        shown.push_str(&format!(
            "{module}.{name} : {}\n",
            checked.typed.types.display_linear(ty, &program.names)
        ));
    }
    insta::assert_snapshot!(shown, @"
    Std.List.length : List a -> <Panic> Int
    Std.List.map : (a -> <e> b) -> poly3 List a -> <e> List b
    Std.List.filter : (a -> <e> Bool) -> poly2 List a -> <e> List a
    Std.List.filter_map : (a -> <e> Option b) -> poly3 List a -> <e> List b
    Std.String.starts_with : String -> poly0 String -> <Panic> Bool
    Std.String.ends_with : String -> poly0 String -> <Panic> Bool
    Std.String.split : String -> poly0 String -> <Panic> List String
    Std.String.lines : String -> <Panic> List String
    Std.String.join : String -> poly0 List String -> String
    Std.String.trim : String -> <Panic> String
    Std.String.to_int : String -> <Panic> Option Int
    ");
}

#[test]
fn a_returned_top_level_function_keeps_a_declaration_variable() {
    // 参照の一番外側の矢印は参照ごとの変数で、戻り値の矢印 (宣言の変数) と等しくなる。制約がないので `Var(0)` のまま残る
    // (docs/spec/types.md の「値を読む式の矢印の線形性」)
    let text = "worker : Unit -> <IO> Unit\nworker () = println \"work\"\n\nget_worker : Unit -> (Unit -> <IO> Unit)\nget_worker () = worker\n\nmain : Unit -> <IO> Unit\nmain () = get_worker () ()";
    insta::assert_snapshot!(exported(text), @"
    worker : Unit -> <IO> Unit
    get_worker : Unit -> poly0 Unit -> <IO> Unit
    main : Unit -> <IO> Unit
    ");
}

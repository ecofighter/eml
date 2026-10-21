//! 型どおりの関数を値として使うときの一様な版 `<f>$uniform` (docs/spec/lir.md の「関数の ABI」)。薄い形と写しの形、
//! 薄い形のフレームを通る yield と再開、版のフレームを通る巻き戻しを、ソースから作ったプログラムで確かめる。

use eml_test_support::{lir, run_stats};
use eml_vm::RunStats;

const HIDE: &str = "\
data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x
";

/// LIR の表示の中の関数 `name` の本体。
fn function(program: &eml_lir::Program, name: &str) -> String {
    let shown = eml_lir::pretty(program);
    let function = program
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("no `{name}` in\n{shown}"));
    let header = format!("fn {}(", function.name);
    let start = shown
        .lines()
        .position(|line| line.contains(&header))
        .unwrap_or_else(|| panic!("no header of `{name}` in\n{shown}"));
    shown
        .lines()
        .skip(start)
        .take_while(|line| *line != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

fn stats(text: &str, expected: &str) -> RunStats {
    let (out, result) = run_stats(text);
    assert_eq!(out, expected, "{result:?}");
    result.unwrap_or_else(|error| panic!("{error}"))
}

/// 末尾の位置で `apply` も `clause` も呼ばない関数の版は薄い形で、引数を取り出して元の関数を呼び、結果を箱に入れる。
/// `apply_twice` は末尾の位置で `apply` を呼ぶので、版は写しの形で、写した本体が末尾の `apply` を持つ。
#[test]
fn a_variant_is_thin_unless_its_function_ends_in_an_apply() {
    let text = format!(
        "{HIDE}
half : Float -> <Panic> Float
half x = x * 0.5

apply_twice : (Float -> <Panic> Float) -> Float -> <Panic> Float
apply_twice f x = f (f x)

main : Unit -> <IO, Panic> Unit
main () =
  let Box f = hide 0 half
  let Box g = hide 0 apply_twice
  println (show (g f 10.0))
"
    );
    let program = lir(&text);
    let thin = function(&program, "half$uniform");
    assert!(thin.contains("= call half("), "{thin}");
    let copy = function(&program, "apply_twice$uniform");
    assert!(copy.contains("tail apply"), "{copy}");
    assert!(!copy.contains("call apply_twice("), "{copy}");
    stats(&text, "2.5\n");
}

/// 写しの形は、結果が型どおりの関数への末尾の位置の直接の呼び出しを、その関数の版へ向け直す。`inner` の版は
/// `outer` の版を、`outer` の版は `inner` の版を末尾で呼ぶので、2つの版の輪は型どおりのコードへ戻らない。
#[test]
fn a_copy_tail_calls_the_variants_of_typed_functions() {
    let text = format!(
        "{HIDE}
outer : (Int -> <Panic> Int) -> Int -> <Panic> Int
outer f n = if n == 0 then f 0 else inner f n

inner : (Int -> <Panic> Int) -> Int -> <Panic> Int
inner f n = outer f (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let Box g = hide 0 inner
  let Box h = hide 0 (fn x -> x + 7)
  println (show (g h 5))
"
    );
    let program = lir(&text);
    let inner = function(&program, "inner$uniform");
    assert!(inner.contains("tail call outer$uniform("), "{inner}");
    let outer = function(&program, "outer$uniform");
    assert!(outer.contains("tail call inner$uniform("), "{outer}");
    assert!(outer.contains("tail apply"), "{outer}");
    stats(&text, "7\n");
}

/// 薄い形の版が呼ぶ関数が yield する。版の `call` には yield の確かめがなく、yield 中は関数がダミーを返し、版は
/// スカラーのダミーを箱に入れ (箱もダミーになる)、構造体のダミーを箱の配置に入れて返す。再開した値は継続を通って
/// `apply` の場所に一様な値として届く (docs/spec/lir.md の「関数の ABI」)。
#[test]
fn a_yield_passes_through_a_thin_variant_frame() {
    let text = format!(
        "effect Ask where
  ask : Int -> Int

{HIDE}
step : Int -> <Ask, Panic> Int
step n = ask n + 1

pair : Int -> <Ask, Panic> (Int, Float)
pair n = (ask n, 2.5)

main : Unit -> <IO, Panic> Unit
main () =
  let Box s = hide 0 step
  let Box p = hide 0 pair
  let r =
    handle (s 1 + s 2) with
      | ctl ask n k -> k (n * 10) + 100
  let q =
    handle (match p 3 with
              | (a, b) -> a) with
      | ctl ask n k -> k (n * 10) + 1000
  println (show r)
  println (show q)
"
    );
    let program = lir(&text);
    for name in ["step", "pair"] {
        let thin = function(&program, &format!("{name}$uniform"));
        assert!(thin.contains(&format!("= call {name}(")), "{thin}");
    }
    let stats = stats(&text, "232\n1030\n");
    assert!(stats.yields > 0, "{stats:?}");
}

/// `never` の操作が、薄い形の版のフレームの下で中断する。`check` の `fail` は末尾の位置にないので、版は薄い形である。
/// 巻き戻しは `check` と版のフレームを外して handler に着地する。検査モードは、残った物体をリークとして報告する。
#[test]
fn an_abort_unwinds_a_variant_frame() {
    let text = format!(
        "effect Fail where
  never fail : String -> a

{HIDE}
check : Int -> <Fail, Panic> Int
check n = if n > 5 then 1 + fail \"big\" else n * 2

main : Unit -> <IO, Panic> Unit
main () =
  let Box c = hide 0 check
  let r =
    handle c 3 + c 9 with
      | ctl fail m -> 0 - 1
  println (show r)
"
    );
    let program = lir(&text);
    let thin = function(&program, "check$uniform");
    assert!(thin.contains("= call check("), "{thin}");
    let stats = stats(&text, "-1\n");
    assert!(stats.unwound_frames >= 2, "{stats:?}");
}

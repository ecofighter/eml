//! row 変数の多重度の上限 `never` (docs/spec/types.md の「row 変数の多重度の上限」)。

use crate::common::{check_full, check_short, kinds};

/// どのテストも頭に置く宣言。`helper` は上限を書いた関数で、`noisy` は `IO` を起こす関数である。8 行で、9 行目から
/// テストの関数を書く。
const HEAD: &str = "helper : (Unit -> <never e> Int) -> <e> Int\nhelper f = f ()\n\nnoisy : Unit -> <IO> Int\nnoisy () =\n  println \"x\"\n  1\n\n";

fn with_head(rest: &str) -> String {
    format!("{HEAD}{rest}")
}

const NEVER: &str = "  kinds: <e> <= Never";

#[test]
fn a_written_bound_stays_in_the_scheme() {
    assert_eq!(kinds(&with_head(""), "helper"), NEVER);
}

#[test]
fn a_call_infers_the_bound() {
    let text = with_head("wrap : (Unit -> <e> Int) -> <e> Int\nwrap f = helper f");
    assert_eq!(kinds(&text, "wrap"), NEVER);
}

#[test]
fn a_recursive_group_shares_an_inferred_bound() {
    let text = with_head(
        "w1 : (Unit -> <e> Int) -> Bool -> <e> Int\nw1 f b = if b then helper f else w2 f True\n\nw2 : (Unit -> <e> Int) -> Bool -> <e> Int\nw2 f b = w1 f b",
    );
    assert_eq!(kinds(&text, "w1"), NEVER);
    assert_eq!(kinds(&text, "w2"), NEVER);
}

#[test]
fn an_unused_row_variable_has_no_bound() {
    let text = with_head("free : (Unit -> <e> Int) -> <e> Int\nfree f = f ()");
    assert_eq!(kinds(&text, "free"), "");
}

#[test]
fn panic_fits_a_never_bound() {
    let text = with_head(
        "main : Unit -> <IO, Panic> Unit\nmain () = println (show (helper (fn () -> 1 / 0)))",
    );
    assert_eq!(check_short(&text), Vec::<String>::new());
}

#[test]
fn an_effect_of_never_operations_fits_a_never_bound() {
    let text = format!(
        "import Fail (Fail(..))\n\n{}",
        with_head("run : Unit -> <Fail String> Int\nrun () = helper (fn () -> fail \"no\")")
    );
    assert_eq!(check_short(&text), Vec::<String>::new());
}

#[test]
fn labels_beside_a_bounded_variable_are_not_bounded() {
    let text = with_head(
        "big : (Unit -> <never e> Int) -> <IO | never e> Int\nbig f =\n  println \"x\"\n  f ()\n\ntab : Int -> (Int -> <never e> Int) -> <Panic | e> List Int\ntab n f = [f n, f (n + 1)]",
    );
    assert_eq!(check_short(&text), Vec::<String>::new());
}

#[test]
fn a_call_that_exceeds_a_written_bound() {
    let text = with_head("main : Unit -> <IO> Unit\nmain () = println (show (helper noisy))");
    insta::assert_snapshot!(check_full(&text), @"
    E2020 10:26 the row passed to `helper` may perform `IO`, but `helper` allows only `never` operations
      10:26 `helper` is used here
      10:33 `IO` is performed here
      1:20 `helper` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn a_multi_operation_exceeds_a_never_bound() {
    let text = with_head(
        "effect Choose where\n  multi choose : Unit -> Bool\n\npick : Unit -> <Choose> Int\npick () = helper (fn () -> if choose () then 1 else 2)",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 13:11 the row passed to `helper` may perform `Choose`, but `helper` allows only `never` operations
      13:11 `helper` is used here
      13:31 `Choose` is performed here
      1:20 `helper` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

/// 操作の引数の row は節の中で `Multi` 以上なので、来た所はエフェクトでなく多重度で示す。`helper` の結果の row の
/// その row 変数は `main` の row に含まれないので、E2002 も出る (docs/implementation/diagnostics.md の E2020)。
#[test]
fn an_operation_argument_row_exceeds_a_never_bound() {
    let text = with_head(
        "effect Ask where\n  ask : (Unit -> <e> Int) -> Int\n\ngo : Unit -> <Ask> Int\ngo () = ask (fn () -> 1)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle go () with\n    | fn ask g -> helper g\n  println (show r)",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2002 18:19 `helper` performs `e`, which the signature of `main` does not allow
      18:19 this call performs `e`
      15:8 the row of this signature does not include it
      help: add `e` to the row of the signature of `main`, as in `-> <e> ...`
    E2020 18:19 the row passed to `helper` may perform `multi` operations, but `helper` allows only `never` operations
      18:19 `helper` is used here
      18:5 `multi` operations may be performed here
      1:20 `helper` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn unwritten_occurrences_carry_the_bound() {
    let text = with_head(
        "pair : (Unit -> <never e> Int) -> (Unit -> <e> Int) -> <e> Int\npair f g = if f () == 0 then g () else 0\n\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (pair (fn () -> 1) noisy))",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 13:26 the row passed to `pair` may perform `IO`, but `pair` allows only `never` operations
      13:26 `pair` is used here
      13:44 `IO` is performed here
      9:18 `pair` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn a_self_call_exceeds_its_own_bound() {
    let text = with_head("h : (Unit -> <never e> Int) -> <IO | e> Int\nh f = h noisy");
    insta::assert_snapshot!(check_full(&text), @"
    E2020 10:7 the row passed to `h` may perform `IO`, but `h` allows only `never` operations
      10:7 `h` is used here
      10:9 `IO` is performed here
      9:15 `h` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn a_mutual_call_exceeds_the_bound() {
    let text = with_head(
        "m1 : (Unit -> <never e> Int) -> Bool -> <IO | e> Int\nm1 f b = if b then f () else m2 b\n\nm2 : Bool -> <IO> Int\nm2 b = m1 noisy b",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 13:8 the row passed to `m1` may perform `IO`, but `m1` allows only `never` operations
      13:8 `m1` is used here
      13:11 `IO` is performed here
      9:16 `m1` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn a_call_from_a_local_function_exceeds_the_bound() {
    let text = with_head(
        "h : (Unit -> <never e> Int) -> <IO | e> Int\nh f =\n  let go x = h noisy\n  go 1",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 11:14 the row passed to `h` may perform `IO`, but `h` allows only `never` operations
      11:14 `h` is used here
      11:16 `IO` is performed here
      9:15 `h` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn a_method_call_exceeds_the_class_bound() {
    let text = with_head(
        "class Runs a where\n  run : a -> (Unit -> <never e> Int) -> <e> Int\n\ngo : Runs a => a -> <IO> Int\ngo x = run x noisy",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 13:8 the row passed to `run` may perform `IO`, but `run` allows only `never` operations
      13:8 `run` is used here
      13:14 `IO` is performed here
      10:24 `run` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn one_call_reports_once_for_two_bounds() {
    let text = with_head(
        "two : (Unit -> <never e1> Int) -> (Unit -> <never e2> Int) -> <IO | e1> Int\ntwo f g = 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (two noisy noisy))",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 13:26 the row passed to `two` may perform `IO`, but `two` allows only `never` operations
      13:26 `two` is used here
      13:30 `IO` is performed here
      9:17 `two` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn an_inferred_bound_names_the_inner_call() {
    let text = with_head(
        "wrap : (Unit -> <e> Int) -> <e> Int\nwrap f = helper f\n\nmain : Unit -> <IO> Unit\nmain () = println (show (wrap noisy))",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 13:26 the row passed to `wrap` may perform `IO`, but `wrap` allows only `never` operations
      13:26 `wrap` is used here
      13:31 `IO` is performed here
      10:10 the row is passed to `helper` here
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn a_chain_names_only_the_nearest_call() {
    let text = with_head(
        "b : (Unit -> <e> Int) -> <e> Int\nb f = helper f\n\na : (Unit -> <e> Int) -> <e> Int\na f = b f\n\nmain : Unit -> <IO> Unit\nmain () = println (show (a noisy))",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 16:26 the row passed to `a` may perform `IO`, but `a` allows only `never` operations
      16:26 `a` is used here
      16:28 `IO` is performed here
      13:7 the row is passed to `b` here
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn a_recursive_group_names_the_call_in_the_group() {
    let text = with_head(
        "w1 : (Unit -> <e> Int) -> Bool -> <e> Int\nw1 f b = if b then helper f else w2 f True\n\nw2 : (Unit -> <e> Int) -> Bool -> <e> Int\nw2 f b = w1 f b\n\nmain : Unit -> <IO> Unit\nmain () = println (show (w2 noisy True))",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 16:26 the row passed to `w2` may perform `IO`, but `w2` allows only `never` operations
      16:26 `w2` is used here
      16:29 `IO` is performed here
      10:20 the row is passed to `helper` here
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

/// 輪の中で `noisy` を渡す `f2 noisy b` には E2020 が出ず、輪の中の `helper` の呼び出しのそれぞれに出て、`noisy` は
/// 来た所の secondary にだけ現れる (docs/spec/types.md の「推論」)。
#[test]
fn a_ring_reports_its_calls_to_the_bounded_function() {
    let text = with_head(
        "f0 : (Unit -> <e> Int) -> Bool -> <IO | e> Int\nf0 g b = if b then helper g else f1 g b\n\nf1 : (Unit -> <e> Int) -> Bool -> <IO | e> Int\nf1 g b = if b then f2 noisy b else f2 g b\n\nf2 : (Unit -> <e> Int) -> Bool -> <IO | e> Int\nf2 g b = if b then helper g else f0 g b",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 10:20 the row passed to `helper` may perform `IO`, but `helper` allows only `never` operations
      10:20 `helper` is used here
      13:23 `IO` is performed here
      1:20 `helper` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    E2020 16:20 the row passed to `helper` may perform `IO`, but `helper` allows only `never` operations
      16:20 `helper` is used here
      13:23 `IO` is performed here
      1:20 `helper` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn an_inferred_bound_broken_inside_its_group() {
    let text = with_head(
        "w1 : (Unit -> <e> Int) -> Bool -> <IO | e> Int\nw1 f b = if b then w2 b else helper f\n\nw2 : Bool -> <IO> Int\nw2 b = w1 noisy b",
    );
    insta::assert_snapshot!(check_full(&text), @"
    E2020 10:30 the row passed to `helper` may perform `IO`, but `helper` allows only `never` operations
      10:30 `helper` is used here
      13:11 `IO` is performed here
      1:20 `helper` bounds this row variable by `never`
      note: a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them
    ");
}

#[test]
fn a_bound_drops_the_carry_it_entails() {
    let text = format!(
        "import FileIn\n\n{}",
        with_head(
            "effect Choice where\n  multi choose : Unit -> Bool\n\nhold : (Unit -> <never e> Unit) -> <IO | e> Unit\nhold action =\n  let f = FileIn.open \"input.txt\"\n  action ()\n  FileIn.close f\n\npick : Unit -> <Choice> Unit\npick () = if choose () then () else ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle hold pick with\n    | ctl choose () k -> k True",
        )
    );
    assert_eq!(
        check_short(&text),
        [
            "E2020 25:10 the row passed to `hold` may perform `Choice`, but `hold` allows only `never` operations"
        ]
    );
    assert_eq!(kinds(&text, "hold"), NEVER);
}

#[test]
fn a_bound_drops_the_carry_inside_its_group() {
    let text = format!(
        "import FileIn\n\n{}",
        with_head(
            "effect Choice where\n  multi choose : Unit -> Bool\n\nhold : Bool -> (Unit -> <never e> Unit) -> <IO | e> Unit\nhold b action =\n  let f = FileIn.open \"input.txt\"\n  action ()\n  FileIn.close f\n  if b then again () else ()\n\nagain : Unit -> <IO> Unit\nagain () =\n  handle hold False pick with\n    | ctl choose () k -> k True\n\npick : Unit -> <Choice> Unit\npick () = if choose () then () else ()",
        )
    );
    assert_eq!(
        check_short(&text),
        [
            "E2020 23:10 the row passed to `hold` may perform `Choice`, but `hold` allows only `never` operations"
        ]
    );
}

/// extern の宣言に書いた上限も、本体のある関数の上限と同じく呼び出しで確かめる。`exit` は `IO` を起こす。
#[test]
fn an_extern_signature_carries_its_never_bound() {
    let text = "import Std.Par\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let (a, b) = Par.par (fn () -> exit 1) (fn () -> 3)\n  println (show (a + b))";
    assert_eq!(
        check_short(text),
        [
            "E2020 5:16 the row passed to `par` may perform `IO`, but `par` allows only `never` operations"
        ]
    );
}

/// 上限はエフェクトの多重度と比べるので、`once` の操作を持つエフェクトは、子が `never` の操作だけを起こしても上限を
/// 超える (docs/spec/types.md の「row 変数の多重度の上限」)。
#[test]
fn an_effect_with_a_once_operation_exceeds_a_never_bound_even_through_its_never_operation() {
    let text = "import Std.Par\n\neffect Mixed where\n  never stop : Unit -> a\n  get : Unit -> Int\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let (c, d) = handle Par.par (fn () -> stop ()) (fn () -> 3) with\n    | ctl stop () -> (0, 0)\n    | fn get () -> 1\n  println (show (c + d))";
    assert_eq!(
        check_short(text),
        [
            "E2020 9:23 the row passed to `par` may perform `Mixed`, but `par` allows only `never` operations"
        ]
    );
}

/// 値として渡した `par` の型も上限を持つので、受け取った関数を通した呼び出しで上限を超える。
#[test]
fn a_value_use_of_par_keeps_the_bound() {
    let text = format!(
        "import Std.Par\n\n{}",
        with_head(
            "apply2 : ((Unit -> <e> Int) -> (Unit -> <e> Int) -> <e> (Int, Int)) -> (Unit -> <e> Int) -> <e> (Int, Int)\napply2 p f = p f f\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let (c, d) = apply2 Par.par noisy\n  println (show (c + d))",
        )
    );
    assert_eq!(
        check_short(&text),
        [
            "E2020 16:23 the row passed to `par` may perform `IO`, but `par` allows only `never` operations"
        ]
    );
}

/// `var` の読み書きは `var` のエフェクトの操作なので、`var` を使う子も上限を超える
/// (docs/spec/types.md の「`var` の変数」)。
#[test]
fn a_par_child_that_uses_a_var_exceeds_the_bound() {
    let text = "import Std.Par\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  var x = 1\n  let (a, b) = Par.par (fn () -> x) (fn () -> 3)\n  println (show (a + b))";
    assert_eq!(
        check_short(text),
        [
            "E2020 6:16 the row passed to `par` may perform `var x`, but `par` allows only `never` operations"
        ]
    );
}

/// 操作が `once` だけのエフェクトも上限を超える。
#[test]
fn a_par_child_that_performs_a_once_operation_exceeds_the_bound() {
    let text = "import Std.Par\n\neffect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let (a, b) = handle Par.par (fn () -> ask ()) (fn () -> 3) with\n    | fn ask () -> 1\n  println (show (a + b))";
    assert_eq!(
        check_short(text),
        [
            "E2020 8:23 the row passed to `par` may perform `Ask`, but `par` allows only `never` operations"
        ]
    );
}

/// コールバックの最後の結果は extern が受け取る値で、`par` は組に1回だけ入れるか、落とす。そのため、結果に
/// `Lin` の値を返す子も受け付ける (docs/spec/declarations.md の「`extern`」)。
#[test]
fn a_par_child_may_return_a_lin_value() {
    let text = "import Std.Par\nimport Std.MutArray\nimport Std.Array\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let (m, n) = Par.par (fn () -> MutArray.make 3 4) (fn () -> 3)\n  let (x, m) = MutArray.read m 0\n  println (show (x + n + Array.length (MutArray.freeze m)))";
    assert_eq!(check_short(text), Vec::<String>::new());
}

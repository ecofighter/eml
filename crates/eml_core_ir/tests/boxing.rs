//! box の挿入のパス (docs/spec/core-ir.md の「パス」)。ほとんどのテストはソースから `Pass::Boxing` までを通し、縮約の
//! 前の IR を見る。末尾呼び出しを確かめるテストは `Pass::Contract` まで通す。T3 の時間は IR のテキストで確かめる。

use crate::common::{core_text, function};
use eml_core_ir::{Pass, Repr, boxing, parse, pretty, verify_scopes, verify_translated};

/// 入力が変換の段の verifier を、出力が範囲の段の verifier を通ることも確かめる。
fn boxing_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_translated(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    boxing(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_scopes(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

const APPLY_TO: &str = "apply_to : (Int -> Int) -> Int -> Int\napply_to f x = f x\n\n";

#[test]
fn a_lambda_used_only_as_a_value_is_made_uniform_in_place() {
    let text = format!(
        "{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show_int (apply_to (fn n -> n + 1) 2))"
    );
    let shown = core_text(&text, Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "main$lambda0"), @"
    internal fn main$lambda0(n.2: tobj) -> tobj {
      let n.0: int = unbox n.2
      let t.1: int = extern Prelude.+(n.0, 1)
      let t.3: tobj = box t.1
      return t.3
    }
    ");
}

#[test]
fn a_top_level_function_used_only_as_a_value_gets_a_boxed_wrapper() {
    let text = format!(
        "inc : Int -> Int\ninc n = n + 1\n\n{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show_int (apply_to inc 2))"
    );
    let shown = core_text(&text, Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "inc"), @"
    fn inc(n.0: int) -> int {
      let t.1: int = extern Prelude.+(n.0, 1)
      return t.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "inc$boxed"), @"
    fn inc$boxed(n.0: tobj) -> tobj {
      let n.2: int = unbox n.0
      let t.1: int = call inc(n.2)
      let t.3: tobj = box t.1
      return t.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main(p.0: unit) -> unit {
      let t.4: tobj = call apply_to(&inc$boxed, 2)
      let t.1: int = unbox t.4
      let t.2: obj = extern Prelude.show_int(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    ");
}

#[test]
fn a_function_also_called_directly_gets_a_boxed_wrapper() {
    let text = "label : Int -> String\nlabel n = show_int n\n\nshow_with : (Int -> String) -> Int -> String\nshow_with f n = f n\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (label 1)\n  println (show_with label 2)";
    let shown = core_text(text, Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main(p.0: unit) -> unit {
      let t.1: obj = call label(1)
      let t.2: unit = extern Prelude.println(t.1)
      let t.3: obj = call show_with(&label$boxed, 2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
    }
    ");
    insta::assert_snapshot!(function(&shown, "label$boxed"), @"
    fn label$boxed(n.0: tobj) -> obj {
      let n.2: int = unbox n.0
      let t.1: obj = call label(n.2)
      return t.1
    }
    ");
    // `label` の `ret` は一様なので、縮約が `f$boxed` の呼び出しを末尾呼び出しにする
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "label$boxed"), @"
    fn label$boxed(n.0: tobj) -> obj {
      let n.2: int = unbox n.0
      tail call label(n.2)
    }
    ");
}

#[test]
fn a_function_made_uniform_by_t3_gets_no_boxed_wrapper() {
    let text = "call_with : (Unit -> Int) -> Int\ncall_with f = f ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = call_with\n  println (show_int (g (fn () -> 1)))";
    let shown = core_text(text, Pass::Boxing);
    assert!(!shown.contains("call_with$boxed"), "{shown}");
    insta::assert_snapshot!(function(&shown, "call_with"), @"
    fn call_with(f.0: tobj) -> tobj {
      let t.2: tobj = apply f.0(())
      let t.1: int = unbox t.2
      return t.2
    }
    ");
}

#[test]
fn fields_are_rebound_at_the_head_of_the_case_target_and_after_an_unpack() {
    let text = "data Option a =\n  | None\n  | Some a\n\nget : Option Int -> Int\nget o = match o with\n  | Some v -> v + 1\n  | None -> 0\n\nsum : (Int, Int) -> Int\nsum p =\n  let (a, b) = p\n  a + b\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (get (Some 1) + sum (1, 2)))";
    let shown = core_text(text, Pass::Boxing);
    insta::assert_snapshot!(function(&shown, "get"), @"
    fn get(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(v.3: tobj) -> b2 }
    b1:
      return 0
    b2:
      let v.1: int = unbox v.3
      let t.2: int = extern Prelude.+(v.1, 1)
      return t.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "sum"), @"
    fn sum(p.0: obj) -> int {
      unpack p.0 (,) #0(a.4: tobj, b.5: tobj)
      let a.1: int = unbox a.4
      let b.2: int = unbox b.5
      let t.3: int = extern Prelude.+(a.1, b.2)
      return t.3
    }
    ");
}

#[test]
fn int_constants_are_boxed_and_other_constants_pass_as_they_are() {
    let text = "keep : a -> b -> c -> d -> Int\nkeep _ _ _ _ = 0\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (keep 5 () True id))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "main"), @"
    fn main(p.0: unit) -> unit {
      let b.4: tobj = box 5
      let t.1: int = call keep(b.4, (), #1, &id)
      let t.2: obj = extern Prelude.show_int(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    ");
}

#[test]
fn a_value_unboxed_by_the_pass_is_passed_on_without_a_new_box() {
    // 恒等のラムダは、入口で `unbox` した値の代わりに、受けた `tobj` をそのまま返す
    let text = format!(
        "{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show_int (apply_to (fn m -> m) 3))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Boxing), "main$lambda0"), @"
    internal fn main$lambda0(m.1: tobj) -> tobj {
      let m.0: int = unbox m.1
      return m.1
    }
    ");
    // 受け直した `apply` の結果をもう一度 `apply` に渡すときも、受けた `tobj` を渡す
    let text = "twice_plus : (Int -> Int) -> Int -> Int\ntwice_plus f x = f (f x) + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (twice_plus (fn n -> n) 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "twice_plus"), @"
    fn twice_plus(f.0: tobj, x.1: int) -> int {
      let x.5: tobj = box x.1
      let t.6: tobj = apply f.0(x.5)
      let t.2: int = unbox t.6
      let t.7: tobj = apply f.0(t.6)
      let t.3: int = unbox t.7
      let t.4: int = extern Prelude.+(t.3, 1)
      return t.4
    }
    ");
}

#[test]
fn a_value_boxed_by_the_pass_is_passed_on_without_a_new_unbox() {
    // `int` の結果を受け直して `box` した変数を `int` の位置へ渡すときは、`unbox` を作らずに受け直した変数を渡す。
    // translate の出力には現れない形なので、IR のテキストで確かめる
    let text = "\
fn g() -> int {
  return 1
}
fn h(n.0: int) -> int {
  return n.0
}
fn f() -> int {
  let r.0: tobj = call g()
  let s.1: int = call h(r.0)
  return s.1
}
";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f() -> int {
      let r.2: int = call g()
      let r.0: tobj = box r.2
      let s.1: int = call h(r.2)
      return s.1
    }
    ");
}

#[test]
fn a_unit_value_passes_to_tobj_without_an_instruction() {
    let text = "id : a -> a\nid x = x\n\nsame : Unit -> Unit\nsame u = id u\n\nmain : Unit -> <IO> Unit\nmain () =\n  same ()\n  println \"done\"";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "same"), @"
    fn same(u.0: unit) -> unit {
      let t.1: unit = call id(u.0)
      return t.1
    }
    ");
}

#[test]
fn a_monomorphic_int_loop_has_no_box() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (loop 3 0))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "loop"), @"
    fn loop(n.0: int, acc.1: int) -> int {
      let t.2: enum = extern Prelude.int_eq(n.0, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern Prelude.-(n.0, 1)
      let t.4: int = extern Prelude.+(acc.1, 1)
      let t.5: int = call loop(t.3, t.4)
      return t.5
    b2:
      return acc.1
    }
    ");
}

#[test]
fn a_function_that_fails_with_a_never_operation_keeps_its_scalar_ret() {
    // `never` の操作の `perform` は戻らないので、T3 は見ない。結果も受け直さない
    let text = "effect Fail where\n  never fail : String -> a\n\ncheck_positive : Int -> <Fail> Int\ncheck_positive n =\n  if n > 0 then n else fail \"not positive\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle check_positive 1 with\n    | fail message -> 0\n  println (show_int r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "check_positive"), @r#"
    fn check_positive(n.0: int) -> int {
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "not positive"
      let t.3: int = perform never Fail.fail(s.2)
      return t.3
    b2:
      return n.0
    }
    "#);
    // 縮約は、`never` の操作の `perform` をどの `ret` の関数でも末尾呼び出しにする
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "check_positive"), @r#"
    fn check_positive(n.0: int) -> int {
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "not positive"
      tail perform never Fail.fail(s.2)
    b2:
      return n.0
    }
    "#);
}

#[test]
fn a_value_lambda_that_fails_ends_in_a_tail_never_perform() {
    // その場で一様にしたラムダでも、`never` の操作の `perform` の束縛は `tobj` に変換せずに返す。束縛の使いには制御が
    // 届かないからである。縮約は、その `return` を末尾呼び出しにする
    let text = "effect Fail where\n  never fail : String -> a\n\napply_to : (Int -> <e> Int) -> Int -> <e> Int\napply_to f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle apply_to (fn n -> if n > 0 then n else fail \"neg\") 1 with\n    | fail message -> 0\n  println (show_int r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "main$lambda0"), @r#"
    internal fn main$lambda0(n.4: tobj) -> tobj {
      let n.0: int = unbox n.4
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "neg"
      let t.3: int = perform never Fail.fail(s.2)
      return t.3
    b2:
      return n.4
    }
    "#);
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "main$lambda0"), @r#"
    internal fn main$lambda0(n.4: tobj) -> tobj {
      let n.0: int = unbox n.4
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "neg"
      tail perform never Fail.fail(s.2)
    b2:
      return n.4
    }
    "#);
}

#[test]
fn t3_raises_the_ret_along_a_chain_of_tail_calls() {
    // `apply_to` は末尾の位置で `apply` するので `ret` が `tobj` になり、それを末尾の位置で呼ぶ `via` も上がる。
    // `through` は、呼び出しと `return` の間に使われない純粋な `let` があっても上がる
    let text = format!(
        "{APPLY_TO}via : Int -> Int\nvia x = apply_to (fn n -> n) x\n\nthrough : (Int -> Int) -> Int -> Int\nthrough f x =\n  let r = f x\n  let s = \"unused\"\n  r\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (via 1 + through (fn n -> n) 2))"
    );
    let shown = core_text(&text, Pass::Contract);
    insta::assert_snapshot!(function(&shown, "apply_to"), @"
    fn apply_to(f.0: tobj, x.1: int) -> tobj {
      let x.3: tobj = box x.1
      tail apply f.0(x.3)
    }
    ");
    insta::assert_snapshot!(function(&shown, "via"), @"
    fn via(x.0: int) -> tobj {
      tail call apply_to(&via$lambda0, x.0)
    }
    ");
    insta::assert_snapshot!(function(&shown, "through"), @"
    fn through(f.0: tobj, x.1: int) -> tobj {
      let x.4: tobj = box x.1
      tail apply f.0(x.4)
    }
    ");
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main(p.0: unit) -> unit {
      let t.6: tobj = call via(1)
      let t.1: int = unbox t.6
      let t.7: tobj = call through(&main$lambda0, 2)
      let t.2: int = unbox t.7
      let t.3: int = extern Prelude.+(t.1, t.2)
      let t.4: obj = extern Prelude.show_int(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    ");
}

#[test]
fn a_tail_call_off_any_loop_may_stay_an_ordinary_call() {
    // handle の本体は `tobj` を返す一様な関数になり、`Int` を返す `answer` の結果を `box` してから返す。この呼び出しは
    // 末尾呼び出しにならないが、関数の値を通るループの上にないので、積むフレームは有界である
    let text = "effect Ask where\n  ask : Unit -> Int\n\nanswer : Unit -> <Ask> Int\nanswer () = ask () + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle answer () with\n    | ask () k -> k 1\n  println (show_int r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "main$handle0"), @"
    internal fn main$handle0(p.0: unit) -> tobj {
      let t.1: int = call answer(())
      let t.2: tobj = box t.1
      return t.2
    }
    ");
}

#[test]
fn t3_takes_time_linear_in_the_chain() {
    // `f_i` は `f_{i+1}` を末尾の位置で呼び、最後の関数は `apply` を末尾の位置で呼ぶ。呼ばれる側の番号が大きいので、
    // 変わらなくなるまで全体を繰り返す形では、1回に1つしか上がらず2乗の時間になる
    const N: usize = 30_000;
    let mut text = String::new();
    for i in 0..N - 1 {
        text.push_str(&format!(
            "fn f{i}(x.0: int) -> int {{\n  let r.1: int = call f{}(x.0)\n  return r.1\n}}\n",
            i + 1
        ));
    }
    text.push_str(&format!(
        "fn f{}(x.0: int) -> int {{\n  let r.1: int = apply &k(x.0)\n  return r.1\n}}\n",
        N - 1
    ));
    text.push_str("fn k(x.0: tobj) -> tobj {\n  return x.0\n}\n");
    let mut program = parse(&text).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(verify_translated(&program), Ok(()));
    let start = std::time::Instant::now();
    boxing(&mut program);
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "took {elapsed:?}"
    );
    assert!(
        program.functions[..N]
            .iter()
            .all(|function| function.ret == Repr::TObj)
    );
}

#[test]
fn an_apply_in_ir_text_boxes_its_argument_and_rebinds_its_result() {
    // 結果を `tobj` の新しい変数で受け、元の変数を直後の `unbox` で定義する。使う所の変数は書き換えない
    let text = "\
fn f(c.0: tobj, n.1: int) -> int {
  let r.2: int = apply c.0(n.1)
  let s.3: int = extern Prelude.+(r.2, 1)
  return s.3
}
";
    insta::assert_snapshot!(boxing_text(text), @"
    fn f(c.0: tobj, n.1: int) -> int {
      let n.4: tobj = box n.1
      let r.5: tobj = apply c.0(n.4)
      let r.2: int = unbox r.5
      let s.3: int = extern Prelude.+(r.2, 1)
      return s.3
    }
    ");
}

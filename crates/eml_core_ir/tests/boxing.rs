//! box の挿入のパス (docs/spec/core-ir.md の「box の挿入」)。ほとんどのテストはソースから `Pass::Boxing` までを
//! 通し、縮約の前の IR を見る。末尾呼び出しを確かめるテストは `Pass::Contract` まで通す。T3 の時間は IR のテキストで
//! 確かめる。

use crate::common::{core_text, function};
use eml_core_ir::{
    Call, Pass, Program, Rhs, Stmt, Term, boxing, parse, pretty, verify_evidence, verify_scopes,
    verify_translated,
};
use eml_lir::Repr;

/// 入力が変換の段の verifier を、出力が範囲の段の verifier を通ることも確かめる。基本操作や節の呼び出し、`clause` の
/// 印がある入力は EP の出力の形なので、EP の段の verifier で確かめる。
fn boxing_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    let verified = if uses_evidence(&program) {
        verify_evidence(&program)
    } else {
        verify_translated(&program)
    };
    if let Err(error) = verified {
        panic!("the input must verify: {error}\n{text}");
    }
    boxing(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_scopes(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

fn uses_evidence(program: &Program) -> bool {
    let evidence_call = |call: &Call| matches!(call, Call::Clause { .. });
    program.functions.iter().any(|function| {
        function.clause.is_some()
            || function.blocks.iter().any(|block| {
                block.stmts.iter().any(|stmt| match stmt {
                    Stmt::Let {
                        var: _,
                        rhs: Rhs::Prim(_, _),
                    } => true,
                    Stmt::Let {
                        var: _,
                        rhs: Rhs::Call { call, .. },
                    } => evidence_call(call),
                    _ => false,
                }) || matches!(&block.term, Term::TailCall(call) if evidence_call(call))
            })
    })
}

const APPLY_TO: &str = "apply_to : (Int -> Int) -> Int -> Int\napply_to f x = f x\n\n";

#[test]
fn a_lambda_used_only_as_a_value_is_made_uniform_in_place() {
    let text = format!(
        "{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show (apply_to (fn n -> n + 1) 2))"
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
        "inc : Int -> Int\ninc n = n + 1\n\n{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show (apply_to inc 2))"
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
      let t.2: obj = extern \"Prelude.Show Int.show\"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    ");
}

#[test]
fn a_function_also_called_directly_gets_a_boxed_wrapper() {
    let text = "label : Int -> String\nlabel n = show n\n\nshow_with : (Int -> String) -> Int -> String\nshow_with f n = f n\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (label 1)\n  println (show_with label 2)";
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
    let text = "call_with : (Unit -> Int) -> Int\ncall_with f = f ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = call_with\n  println (show (g (fn () -> 1)))";
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
    let text = "data Option a =\n  | None\n  | Some a\n\nget : Option Int -> Int\nget o = match o with\n  | Some v -> v + 1\n  | None -> 0\n\nsum : (Int, Int) -> Int\nsum p =\n  let (a, b) = p\n  a + b\n\nmain : Unit -> <IO> Unit\nmain () = println (show (get (Some 1) + sum (1, 2)))";
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
    // 型変数のフィールドは `tobj` なので、`int` の定数は `box` を通り、`unit`、タグ、関数の値はそのまま入る
    let text = "data Four a b c d =\n  | Four a b c d\n\ncount : Four a b c d -> Int\ncount _ = 0\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () = println (show (count (Four 5 () True id)))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let b.5: tobj = box 5
      let d.1: obj = con Four #0(b.5, (), #1, &"id@[_]")
      let t.2: int = call "count@[Int, Unit, Bool, _ -> <_> _]"(d.1)
      let t.3: obj = extern "Prelude.Show Int.show"(t.2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
    }
    "#);
}

#[test]
fn a_value_unboxed_by_the_pass_is_passed_on_without_a_new_box() {
    // 恒等のラムダは、入口で `unbox` した値の代わりに、受けた `tobj` をそのまま返す
    let text = format!(
        "{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show (apply_to (fn m -> m) 3))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Boxing), "main$lambda0"), @"
    internal fn main$lambda0(m.1: tobj) -> tobj {
      let m.0: int = unbox m.1
      return m.1
    }
    ");
    // 受け直した `apply` の結果をもう一度 `apply` に渡すときも、受けた `tobj` を渡す
    let text = "twice_plus : (Int -> Int) -> Int -> Int\ntwice_plus f x = f (f x) + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (twice_plus (fn n -> n) 1))";
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
    let text = "data Box a =\n  | Box a\n\nwrap : Unit -> Box Unit\nwrap u = Box u\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = wrap ()\n  println \"done\"";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "wrap"), @"
    fn wrap(u.0: unit) -> obj {
      let d.1: obj = con Box #0(u.0)
      return d.1
    }
    ");
}

#[test]
fn a_monomorphic_int_loop_has_no_box() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (loop 3 0))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "loop"), @"
    fn loop(n.0: int, acc.1: int) -> int {
      let t.2: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
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
    // `never` の操作の `clause` は戻らないので、T3 は見ない。結果も受け直さない
    let text = "effect Fail where\n  never fail : String -> a\n\ncheck_positive : Int -> <Fail> Int\ncheck_positive n =\n  if n > 0 then n else fail \"not positive\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle check_positive 1 with\n    | fail message -> 0\n  println (show r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "check_positive"), @r#"
    yields fn check_positive(n.0: int) -> int {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "not positive"
      let ev.4: obj = prim ev_lookup[Fail]()
      let c.5: tobj = prim ev_clause[Fail.fail](ev.4)
      let t.3: int = clause Fail.fail c.5(ev.4, s.2)
      return t.3
    b2:
      return n.0
    }
    "#);
    // 縮約は、`never` の操作の `clause` をどの `ret` の関数でも末尾呼び出しにする
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "check_positive"), @r#"
    yields fn check_positive(n.0: int) -> int {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "not positive"
      let ev.4: obj = prim ev_lookup[Fail]()
      let c.5: tobj = prim ev_clause[Fail.fail](ev.4)
      tail clause Fail.fail c.5(ev.4, s.2)
    b2:
      return n.0
    }
    "#);
}

#[test]
fn a_value_lambda_that_fails_ends_in_a_tail_never_clause() {
    // その場で一様にしたラムダでも、`never` の操作の `clause` の束縛は `tobj` に変換せずに返す。束縛の使いには制御が
    // 届かないからである。縮約は、その `return` を末尾呼び出しにする
    let text = "effect Fail where\n  never fail : String -> a\n\napply_to : (Int -> <e> Int) -> Int -> <e> Int\napply_to f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle apply_to (fn n -> if n > 0 then n else fail \"neg\") 1 with\n    | fail message -> 0\n  println (show r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "main$lambda0"), @r#"
    internal yields fn main$lambda0(n.6: tobj) -> tobj {
      let n.0: int = unbox n.6
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "neg"
      let ev.4: obj = prim ev_lookup[Fail]()
      let c.5: tobj = prim ev_clause[Fail.fail](ev.4)
      let t.3: int = clause Fail.fail c.5(ev.4, s.2)
      return t.3
    b2:
      return n.6
    }
    "#);
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "main$lambda0"), @r#"
    internal yields fn main$lambda0(n.6: tobj) -> tobj {
      let n.0: int = unbox n.6
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "neg"
      let ev.4: obj = prim ev_lookup[Fail]()
      let c.5: tobj = prim ev_clause[Fail.fail](ev.4)
      tail clause Fail.fail c.5(ev.4, s.2)
    b2:
      return n.6
    }
    "#);
}

#[test]
fn t3_raises_the_ret_along_a_chain_of_tail_calls() {
    // `apply_to` は末尾の位置で `apply` するので `ret` が `tobj` になり、それを末尾の位置で呼ぶ `via` も上がる。
    // `through` は、呼び出しと `return` の間に使われない純粋な `let` があっても上がる
    let text = format!(
        "{APPLY_TO}via : Int -> Int\nvia x = apply_to (fn n -> n) x\n\nthrough : (Int -> Int) -> Int -> Int\nthrough f x =\n  let r = f x\n  let s = \"unused\"\n  r\n\nmain : Unit -> <IO> Unit\nmain () = println (show (via 1 + through (fn n -> n) 2))"
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
      let t.4: obj = extern \"Prelude.Show Int.show\"(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    ");
}

#[test]
fn a_tail_call_off_any_loop_may_stay_an_ordinary_call() {
    // handle の本体は `tobj` を返す一様な関数になり、`Int` を返す `answer` の結果を `box` してから返す。この呼び出しは
    // 末尾呼び出しにならないが、関数の値を通るループの上にないので、積むフレームは有界である
    let text = "effect Ask where\n  ask : Unit -> Int\n\nanswer : Unit -> <Ask> Int\nanswer () = ask () + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle answer () with\n    | ask () k -> k 1\n  println (show r)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "main$handle0"), @"
    internal yields fn main$handle0(p.0: unit) -> tobj {
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

#[test]
#[should_panic(expected = "internal error: the boxing pass cannot pass 5 to obj in `f`")]
fn a_constant_that_cannot_go_to_its_position_is_an_internal_error() {
    // 変換の段は直接の呼び出しの引数を比べないので、`obj` の引数に渡す `Int` の定数は box の挿入まで届く。型からは
    // 起きない変換なので、パスはパニックする (docs/spec/core-ir.md の「box の挿入」)
    let text = "\
fn h(x.0: obj) -> int {
  return 1
}
fn f() -> int {
  let r.0: int = call h(5)
  return r.0
}
";
    boxing_text(text);
}

#[test]
fn prim_operands_are_converted_like_call_arguments() {
    // `ev_new` は状態を `tobj` で受けるので、`int` の状態は `box` して渡す
    let text = "effect State { get(unit) -> tobj, put(tobj) -> unit }\nfn f(n.0: int) -> obj {\n  let w.1: tobj = prim evv_get()\n  let e.2: obj = prim ev_new[State](n.0, w.1)\n  return e.2\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f(n.0: int) -> obj {
      let w.1: tobj = prim evv_get()
      let n.3: tobj = box n.0
      let e.2: obj = prim ev_new[State](n.3, w.1)
      return e.2
    }
    ");
}

#[test]
fn a_prim_result_is_rebound_to_its_row_repr() {
    // `yielding` の結果は `int` なので、`tobj` の変数で受けるなら `int` の新しい変数で受けて `box` する
    let text = "fn f() -> tobj {\n  let y.0: tobj = prim yielding()\n  return y.0\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f() -> tobj {
      let y.1: int = prim yielding()
      let y.0: tobj = box y.1
      return y.0
    }
    ");
}

#[test]
fn a_clause_call_uses_the_reprs_of_its_operation() {
    // `put` の `int` の引数へ `tobj` の n.2 を `unbox` して渡し、`get` の `int` の結果を受け直して `box` する
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\nyields fn f(c.0: tobj, e.1: obj, n.2: tobj) -> tobj {\n  let u.3: unit = clause Counter.put c.0(e.1, n.2)\n  let c.4: tobj = prim ev_clause[Counter.get](e.1)\n  let m.5: tobj = clause Counter.get c.4(e.1, ())\n  return m.5\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    yields fn f(c.0: tobj, e.1: obj, n.2: tobj) -> tobj {
      let n.6: int = unbox n.2
      let u.3: unit = clause Counter.put c.0(e.1, n.6)
      let c.4: tobj = prim ev_clause[Counter.get](e.1)
      let m.7: int = clause Counter.get c.4(e.1, ())
      let m.5: tobj = box m.7
      return m.5
    }
    ");
}

#[test]
fn a_clause_function_takes_the_reprs_of_its_operation() {
    // 引数 n は `put` の `int` になり、入口で元の `tobj` の変数を `box` で定義する。`ret` は `unit` になる
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\ninternal yields clause Counter.put fn g(e.0: obj, n.1: tobj) -> tobj {\n  return ()\n}\nfn main() -> unit {\n  let w.0: tobj = prim evv_get()\n  let e.1: obj = prim ev_new[Counter](0, w.0)\n  let u.2: unit = prim ev_set_clause[Counter.put](e.1, &g)\n  return ()\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "g"), @"
    internal yields clause Counter.put fn g(e.0: obj, n.2: int) -> unit {
      let n.1: tobj = box n.2
      return ()
    }
    ");
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main() -> unit {
      let w.0: tobj = prim evv_get()
      let b.3: tobj = box 0
      let e.1: obj = prim ev_new[Counter](b.3, w.0)
      let u.2: unit = prim ev_set_clause[Counter.put](e.1, &g)
      return ()
    }
    ");
}

#[test]
fn a_clause_function_keeps_its_captures_uniform() {
    // `int` の捕獲は `tobj` で受けて入口で `unbox` し、`closure` は捕獲を `box` して渡す。evidence と `get` の引数と
    // `ret` はすでに操作の表と合っている
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\ninternal yields clause Counter.get fn g(k.0: int, e.1: obj, u.2: unit) -> int {\n  return k.0\n}\nfn main(k.0: int) -> unit {\n  let w.1: tobj = prim evv_get()\n  let e.2: obj = prim ev_new[Counter]((), w.1)\n  let g.3: tobj = closure g(k.0)\n  let u.4: unit = prim ev_set_clause[Counter.get](e.2, g.3)\n  return ()\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "g"), @"
    internal yields clause Counter.get fn g(k.3: tobj, e.1: obj, u.2: unit) -> int {
      let k.0: int = unbox k.3
      return k.0
    }
    ");
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main(k.0: int) -> unit {
      let w.1: tobj = prim evv_get()
      let e.2: obj = prim ev_new[Counter]((), w.1)
      let k.5: tobj = box k.0
      let g.3: tobj = closure g(k.5)
      let u.4: unit = prim ev_set_clause[Counter.get](e.2, g.3)
      return ()
    }
    ");
}

#[test]
fn a_never_clause_call_is_not_rebound() {
    // `never` の操作の節の呼び出しは戻らないので、`int` の束縛を受け直さず、`return` にも変換を入れない。T3 も見ない
    let text = "effect Fail { never fail(tobj) -> never }\nyields fn f(c.0: tobj, e.1: obj, s.2: obj) -> int {\n  let r.3: int = clause Fail.fail c.0(e.1, s.2)\n  return r.3\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    yields fn f(c.0: tobj, e.1: obj, s.2: obj) -> int {
      let r.3: int = clause Fail.fail c.0(e.1, s.2)
      return r.3
    }
    ");
}

#[test]
fn the_uses_of_a_never_clause_binder_are_not_converted() {
    // `int` の束縛を `tobj` の位置 (`apply` の引数と `tobj` の `ret` の `return`) へ渡しても `box` しない。使いには制御が
    // 届かないからである
    let text = "effect Fail { never fail(tobj) -> never }\nyields fn f(c.0: tobj, e.1: obj, s.2: obj, k.3: tobj) -> tobj {\n  let r.4: int = clause Fail.fail c.0(e.1, s.2)\n  let t.5: tobj = apply k.3(r.4)\n  return t.5\n}\nyields fn g(c.0: tobj, e.1: obj, s.2: obj) -> tobj {\n  let r.3: int = clause Fail.fail c.0(e.1, s.2)\n  return r.3\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(c.0: tobj, e.1: obj, s.2: obj, k.3: tobj) -> tobj {
      let r.4: int = clause Fail.fail c.0(e.1, s.2)
      let t.5: tobj = apply k.3(r.4)
      return t.5
    }
    ");
    insta::assert_snapshot!(function(&shown, "g"), @"
    yields fn g(c.0: tobj, e.1: obj, s.2: obj) -> tobj {
      let r.3: int = clause Fail.fail c.0(e.1, s.2)
      return r.3
    }
    ");
}

#[test]
fn a_clause_function_unboxes_its_return_to_a_scalar_operation_result() {
    // `Counter.get` の結果は `int` なので、節の関数の `ret` を `int` にし、`tobj` の値を返す `return` の前で `unbox` する
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\ninternal yields clause Counter.get fn g(e.0: obj, u.1: unit) -> tobj {\n  let s.2: tobj = prim ev_take_state(e.0)\n  return s.2\n}\nfn main() -> unit {\n  let w.0: tobj = prim evv_get()\n  let e.1: obj = prim ev_new[Counter]((), w.0)\n  let u.2: unit = prim ev_set_clause[Counter.get](e.1, &g)\n  return ()\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "g"), @"
    internal yields clause Counter.get fn g(e.0: obj, u.1: unit) -> int {
      let s.2: tobj = prim ev_take_state(e.0)
      let s.3: int = unbox s.2
      return s.3
    }
    ");
}

#[test]
fn a_never_clause_function_returns_obj() {
    // `never` の操作の節の入口はダミーの物体を返すので、`ret` は `obj` になる
    let text = "effect Fail { never fail(tobj) -> never }\ninternal yields clause Fail.fail fn g(e.0: obj, s.1: tobj) -> tobj {\n  let d.2: obj = prim dummy()\n  return d.2\n}\nfn main() -> unit {\n  let w.0: tobj = prim evv_get()\n  let e.1: obj = prim ev_new[Fail]((), w.0)\n  let u.2: unit = prim ev_set_clause[Fail.fail](e.1, &g)\n  return ()\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "g"), @"
    internal yields clause Fail.fail fn g(e.0: obj, s.1: tobj) -> obj {
      let d.2: obj = prim dummy()
      return d.2
    }
    ");
}

#[test]
fn t3_takes_the_result_of_a_tail_clause_from_its_operation() {
    // `Counter.get` の `int` の結果は `int` の `ret` と互換なので上げない。`State.get` の `tobj` の結果は互換でないので、
    // `ret` を `tobj` に上げる
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\neffect State { get(unit) -> tobj, put(tobj) -> unit }\nyields fn f(c.0: tobj, e.1: obj) -> int {\n  let m.2: int = clause Counter.get c.0(e.1, ())\n  return m.2\n}\nyields fn g(c.0: tobj, e.1: obj) -> int {\n  let m.2: int = clause State.get c.0(e.1, ())\n  return m.2\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(c.0: tobj, e.1: obj) -> int {
      let m.2: int = clause Counter.get c.0(e.1, ())
      return m.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "g"), @"
    yields fn g(c.0: tobj, e.1: obj) -> tobj {
      let m.3: tobj = clause State.get c.0(e.1, ())
      let m.2: int = unbox m.3
      return m.3
    }
    ");
}

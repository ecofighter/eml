//! box の挿入のパス (docs/spec/lir.md の「box の挿入」)。ソースから `eml_lir::Pass::Boxing` までを通し、縮約の
//! 前の LIR を見る。末尾呼び出しを確かめるテストは `eml_lir::Pass::Contract` まで通す。手で書いた LIR を入力にする
//! テストは `eml_lir` の tests/boxing.rs にある。

use crate::common::{function, lir_text};

const APPLY_TO: &str = "apply_to : (Int -> Int) -> Int -> Int\napply_to f x = f x\n\n";

#[test]
fn a_lambda_used_only_as_a_value_is_made_uniform_in_place() {
    let text = format!(
        "{APPLY_TO}main : Unit -> <IO> Unit\nmain () = println (show (apply_to (fn n -> n + 1) 2))"
    );
    let shown = lir_text(&text, eml_lir::Pass::Boxing);
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
    let shown = lir_text(&text, eml_lir::Pass::Boxing);
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
    let shown = lir_text(text, eml_lir::Pass::Boxing);
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "label$boxed"), @"
    fn label$boxed(n.0: tobj) -> obj {
      let n.2: int = unbox n.0
      tail call label(n.2)
    }
    ");
}

#[test]
fn a_function_made_uniform_by_t3_gets_no_boxed_wrapper() {
    let text = "call_with : (Unit -> Int) -> Int\ncall_with f = f ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = call_with\n  println (show (g (fn () -> 1)))";
    let shown = lir_text(text, eml_lir::Pass::Boxing);
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
    let shown = lir_text(text, eml_lir::Pass::Boxing);
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Boxing), "main"), @r#"
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
    insta::assert_snapshot!(function(&lir_text(&text, eml_lir::Pass::Boxing), "main$lambda0"), @"
    internal fn main$lambda0(m.1: tobj) -> tobj {
      let m.0: int = unbox m.1
      return m.1
    }
    ");
    // 受け直した `apply` の結果をもう一度 `apply` に渡すときも、受けた `tobj` を渡す
    let text = "twice_plus : (Int -> Int) -> Int -> Int\ntwice_plus f x = f (f x) + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (twice_plus (fn n -> n) 1))";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Boxing), "twice_plus"), @"
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
fn a_unit_value_passes_to_tobj_without_an_instruction() {
    let text = "data Box a =\n  | Box a\n\nwrap : Unit -> Box Unit\nwrap u = Box u\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = wrap ()\n  println \"done\"";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Boxing), "wrap"), @"
    fn wrap(u.0: unit) -> obj {
      let d.1: obj = con Box #0(u.0)
      return d.1
    }
    ");
}

#[test]
fn a_monomorphic_int_loop_has_no_box() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (loop 3 0))";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Boxing), "loop"), @"
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Boxing), "check_positive"), @r#"
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "check_positive"), @r#"
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Boxing), "main$lambda0"), @r#"
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "main$lambda0"), @r#"
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
    let shown = lir_text(&text, eml_lir::Pass::Contract);
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "main$handle0"), @"
    internal yields fn main$handle0(p.0: unit) -> tobj {
      let t.1: int = call answer(())
      let t.2: tobj = box t.1
      return t.2
    }
    ");
}

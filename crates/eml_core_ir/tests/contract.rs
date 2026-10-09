//! contract のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消すことと、すべてのブロックに
//! 末尾呼び出しの規則を当てることを、IR のテキストで確かめる。後半は、ソースから縮約までを通した結果を見る。

use crate::common::{core_text, function};
use eml_core_ir::{Pass, contract, parse, pretty, verify_scopes};

/// 入力と出力が、どちらも scope の段の verifier を通ることも確かめる。
fn contract_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_scopes(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    contract(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_scopes(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

#[test]
fn unused_bindings_that_cannot_fail_are_removed() {
    let text = "\
layout Pair { Pair(int, int) }
fn f(x.0: int) -> int {
  let p.1: obj = con Pair #0(x.0, x.0)
  let s.2: obj = const \"unused\"
  let c.3: tobj = closure g(())
  return 1
}
fn g(x.0: unit, y.1: tobj) -> tobj {
  return y.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Pair { Pair(int, int) }
    fn f(x.0: int) -> int {
      return 1
    }
    fn g(x.0: unit, y.1: tobj) -> tobj {
      return y.1
    }
    ");
}

#[test]
fn unused_bindings_that_can_fail_or_act_are_kept() {
    // `/` はゼロ除算で止まりうる。extern のエフェクト、呼び出し、`drop` も、使われなくても消さない
    let text = "\
fn f(x.0: int, s.1: obj, t.2: obj) -> int {
  let q.3: int = extern Prelude./(x.0, 0)
  let u.4: unit = extern Prelude.println(s.1)
  let r.5: int = call g(x.0)
  let d.6: unit = drop t.2
  return 1
}
fn g(x.0: int) -> int {
  return x.0
}
";
    assert_eq!(contract_text(text), text);
}

#[test]
fn unused_pure_extern_bindings_are_removed() {
    // 行が `Pure` の extern は止まらないので、使われなければ消す
    let text = "\
fn f(x.0: int, s.1: obj) -> int {
  let t.2: obj = extern \"Prelude.Show Int.show\"(x.0)
  let b.3: enum = extern \"Prelude.Ord Int.<\"(x.0, 0)
  let u.4: obj = extern Prelude.++(s.1, t.2)
  return 1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: int, s.1: obj) -> int {
      return 1
    }
    ");
}

#[test]
fn unused_boxes_and_unboxes_are_removed() {
    // `box` を消すと確保が1つ減るだけで、`unbox` は読むだけなので、どちらも純粋である
    let text = "\
fn f(n.0: int, b.1: tobj) -> int {
  let c.2: tobj = box n.0
  let m.3: int = unbox b.1
  return n.0
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(n.0: int, b.1: tobj) -> int {
      return n.0
    }
    ");
}

#[test]
fn bindings_used_only_by_removed_bindings_go_in_the_same_pass() {
    // `q.4` を消すと `p.3` が、`p.3` を消すと `s.2` が使われなくなる。後ろから1回たどるだけで、ブロックをまたいで
    // すべて消える
    let text = "\
layout Prelude.Bool { False, True }
layout Pair { Pair(tobj, int) }
layout Either { Left(tobj), Right(tobj) }
fn f(x.0: int, c.1: enum) -> int {
  let s.2: obj = extern \"Prelude.Show Int.show\"(x.0)
  let p.3: obj = con Pair #0(s.2, x.0)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let q.4: obj = con Either #1(p.3)
  return 1
b2:
  return 2
}
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Prelude.Bool { False, True }
    layout Pair { Pair(tobj, int) }
    layout Either { Left(tobj), Right(tobj) }
    fn f(x.0: int, c.1: enum) -> int {
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      return 1
    b2:
      return 2
    }
    ");
}

#[test]
fn a_call_returned_at_the_end_of_an_unchanged_block_becomes_a_tail_call() {
    // `f` は使われない `let` を消してから、`h` は何も消さずに、どちらも末尾呼び出しになる。translate は末尾呼び出しを
    // 作らないので、縮約は文を消したかに関わらずすべてのブロックに規則を当てる
    let text = "\
fn f(x.0: int) -> int {
  let r.1: int = call g(x.0)
  let s.2: obj = const \"unused\"
  return r.1
}
fn g(x.0: int) -> int {
  return x.0
}
fn h(x.0: int) -> int {
  let r.1: int = call g(x.0)
  return r.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: int) -> int {
      tail call g(x.0)
    }
    fn g(x.0: int) -> int {
      return x.0
    }
    fn h(x.0: int) -> int {
      tail call g(x.0)
    }
    ");
}

#[test]
fn every_kind_of_call_returned_at_the_end_becomes_a_tail_call() {
    // 直接の呼び出し、関数値の適用、`perform`、`handle`、`resume` のどれも末尾呼び出しになり、`mask` はそのまま運ぶ
    // (docs/spec/core-ir.md の「Core IR」)。結果を返さない呼び出しは残す
    let text = "\
effect Ask { ask/1 }
fn by_call(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return r.1
}
fn by_apply(f.0: tobj) -> tobj {
  let r.1: tobj = mask [Ask] apply f.0(())
  return r.1
}
fn by_perform(s.0: obj) -> tobj {
  let r.1: tobj = perform Ask.ask(s.0)
  return r.1
}
fn by_handle() -> tobj {
  let r.0: tobj = handle Ask((), &body) { ask: &clause } return &ret
  return r.0
}
fn by_resume(k.0: tobj) -> tobj {
  let r.1: tobj = resume k.0((), ())
  return r.1
}
fn not_returned(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return x.0
}
fn body(u.0: unit) -> tobj {
  return ()
}
fn clause(s.0: obj, k.1: tobj, t.2: unit) -> tobj {
  return ()
}
fn ret(v.0: tobj, t.1: unit) -> tobj {
  return v.0
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Ask { ask/1 }
    fn by_call(x.0: int) -> int {
      tail call by_call(x.0)
    }
    fn by_apply(f.0: tobj) -> tobj {
      tail mask [Ask] apply f.0(())
    }
    fn by_perform(s.0: obj) -> tobj {
      tail perform Ask.ask(s.0)
    }
    fn by_handle() -> tobj {
      tail handle Ask((), &body) { ask: &clause } return &ret
    }
    fn by_resume(k.0: tobj) -> tobj {
      tail resume k.0((), ())
    }
    fn not_returned(x.0: int) -> int {
      let r.1: int = call by_call(x.0)
      return x.0
    }
    fn body(u.0: unit) -> tobj {
      return ()
    }
    fn clause(s.0: obj, k.1: tobj, t.2: unit) -> tobj {
      return ()
    }
    fn ret(v.0: tobj, t.1: unit) -> tobj {
      return v.0
    }
    ");
}

#[test]
fn a_call_whose_result_is_not_compatible_with_the_ret_is_not_a_tail_call() {
    // `g` の `unit` は `r.0` の `tobj` と、`r.0` は `f` の `obj` と互換である。互換は推移的でないので、`unit` と `obj`
    // を直接つなぐ末尾呼び出しにはしない
    let text = "\
fn f() -> obj {
  let r.0: tobj = call g()
  return r.0
}
fn g() -> unit {
  return ()
}
";
    assert_eq!(contract_text(text), text);
}

#[test]
fn a_never_perform_returned_at_the_end_becomes_a_tail_call_in_any_function() {
    // `never` の操作の `perform` は戻らないので、結果はどの `ret` とも互換である
    let text = "\
effect Fail { never fail/1 }
fn f(s.0: obj) -> int {
  let r.1: int = perform never Fail.fail(s.0)
  return r.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Fail { never fail/1 }
    fn f(s.0: obj) -> int {
      tail perform never Fail.fail(s.0)
    }
    ");
}

#[test]
fn a_returned_if_value_becomes_tail_calls_in_each_arm() {
    // `let y = if ..; y` の続きは `return` だけのブロックなので、translate が各枝の `jump` を `return` にしてブロックを
    // 消し、縮約が呼び出しの後の `return` を末尾呼び出しにする
    let text = "f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Bool -> Int -> Int\nh c x =\n  let y = if c then f x else g x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show (h True 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "h"), @"
    fn h(c.0: enum, x.1: int) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      tail call g(x.1)
    b2:
      tail call f(x.1)
    }
    ");
}

#[test]
fn a_returned_match_value_becomes_tail_calls_in_each_arm() {
    let text = "data Option a =\n  | None\n  | Some a\n\nf : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Option Int -> Int\nh o =\n  let y = match o with\n    | Some v -> f v\n    | None -> g 0\n  let z = y\n  z\n\nmain : Unit -> <IO> Unit\nmain () = println (show (h None))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "h"), @"
    fn h(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(v.5: tobj) -> b2 }
    b1:
      tail call g(0)
    b2:
      let v.1: int = unbox v.5
      tail call f(v.1)
    }
    ");
}

#[test]
fn returning_a_field_of_a_call_result_is_not_a_tail_call() {
    // 返すのはタプル全体ではなくフィールドなので、呼び出しの後に `unpack` が残り、末尾呼び出しにならない
    let text = "split : Int -> (Int, Int)\nsplit x = (x, x + 1)\n\nfirst : Int -> Int\nfirst x =\n  let (y, _) = split x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show (first 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "first"), @"
    fn first(x.0: int) -> int {
      let t.1: obj = call split(x.0)
      unpack t.1 (,) #0(y.4: tobj, x.5: tobj)
      let y.2: int = unbox y.4
      return y.2
    }
    ");
}

#[test]
fn calls_in_tail_position_are_tail_calls() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\ncall_twice : (Int -> Int) -> Int -> Int\ncall_twice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (loop 3 0 + call_twice (fn x -> x + 1) 1))";
    let shown = core_text(text, Pass::Contract);
    insta::assert_snapshot!(function(&shown, "loop"), @"
    fn loop(n.0: int, acc.1: int) -> int {
      let t.2: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern Prelude.-(n.0, 1)
      let t.4: int = extern Prelude.+(acc.1, 1)
      tail call loop(t.3, t.4)
    b2:
      return acc.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "call_twice"), @"
    fn call_twice(f.0: tobj, x.1: int) -> tobj {
      let x.4: tobj = box x.1
      let t.5: tobj = apply f.0(x.4)
      tail apply f.0(t.5)
    }
    ");
}

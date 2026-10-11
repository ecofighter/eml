//! contract のパス (docs/spec/lir.md の「縮約」)。ソースから縮約までを通した結果を見る。使われない純粋な `let` を
//! 消すことと、すべてのブロックに末尾呼び出しの規則を当てることを手で書いた LIR で確かめるテストは、`eml_lir` の
//! tests/contract.rs にある。

use crate::common::{function, lir_text};

#[test]
fn a_returned_if_value_becomes_tail_calls_in_each_arm() {
    // `let y = if ..; y` の続きは `return` だけのブロックなので、translate が各枝の `jump` を `return` にしてブロックを
    // 消し、縮約が呼び出しの後の `return` を末尾呼び出しにする
    let text = "f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Bool -> Int -> Int\nh c x =\n  let y = if c then f x else g x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show (h True 1))";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "h"), @"
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "h"), @"
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "first"), @"
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
    let shown = lir_text(text, eml_lir::Pass::Contract);
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

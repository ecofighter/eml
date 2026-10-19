//! contract のパス (docs/spec/lir.md の「縮約」)。ソースから縮約までを通した結果を見る。使われない純粋な `let` を
//! 消すことと、すべてのブロックに末尾呼び出しの規則を当てることを手で書いた LIR で確かめるテストは、`eml_lir` の
//! tests/contract.rs にある。

use crate::common::{function, lir_text};

/// 調べる関数をデータに通して呼び、optimize がそれを呼ぶ側に展開しないようにする。取り出した関数の値への `apply` は
/// 直接の呼び出しでないので、関数は自分のコードのまま残る。`f` と `g` は自分を呼ぶので展開されず、調べる関数の中で
/// 直接の呼び出しのまま残る (docs/spec/core-ir.md の「インライン化」)。
/// `hide` は自分を呼ぶので展開されず、取り出した値は呼ぶ側から見えない。
const KEEP: &str = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\nf : Int -> <Panic> Int\nf x = if x > 9 then f (x - 1) else x + 1\n\ng : Int -> <Panic> Int\ng x = if x < 0 then g (x + 1) else x - 1\n\n";

#[test]
fn a_returned_if_value_becomes_tail_calls_in_each_arm() {
    // `let y = if ..; y` の続きは `return` だけのブロックなので、translate が各枝の `jump` を `return` にしてブロックを
    // 消し、縮約が呼び出しの後の `return` を末尾呼び出しにする
    let text = format!(
        "{KEEP}h : Bool -> Int -> <Panic> Int\nh c x =\n  let y = if c then f x else g x\n  y\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box h = hide 0 h\n  println (show (h True 1))"
    );
    let text = text.as_str();
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
    let text = format!(
        "data Option a =\n  | None\n  | Some a\n\n{KEEP}h : Option Int -> <Panic> Int\nh o =\n  let y = match o with\n    | Some v -> f v\n    | None -> g 0\n  let z = y\n  z\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box h = hide 0 h\n  println (show (h None))"
    );
    let text = text.as_str();
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
    let text = format!(
        "{KEEP}split : Int -> <Panic> (Int, Int)\nsplit x = if x > 9 then split (x - 1) else (x, x + 1)\n\nfirst : Int -> <Panic> Int\nfirst x =\n  let (y, _) = split x\n  y\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box first = hide 0 first\n  println (show (first 1))"
    );
    let text = text.as_str();
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
    // ラムダはデータを通して渡し、optimize が `call_twice` を特殊化しないようにする。`apply` の末尾呼び出しを見るためである
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\nloop : Int -> Int -> <Panic> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\ncall_twice : (Int -> <Panic> Int) -> Int -> <Panic> Int\ncall_twice f x = f (f x)\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 (fn x -> x + 1)\n  let Box call_twice = hide 0 call_twice\n  println (show (loop 3 0 + call_twice f 1))";
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

#[test]
fn a_local_self_call_is_a_tail_call() {
    // 持ち上げたローカル関数の自分への呼び出しも直接の呼び出しなので、トップレベルの関数と同じく末尾呼び出しになる
    let text = "f : Int -> Int -> <Panic> Int\nf base n =\n  let go k acc = if k == 0 then acc else go (k - 1) (acc + base)\n  go n 0\n\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (f 2 3))";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Contract), "f$let$go"), @r#"
    fn f$let$go(base.0: int, k.1: int, acc.2: int) -> int {
      let t.3: enum = extern "Prelude.Eq Int.=="(k.1, 0)
      switch t.3 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.4: int = extern Prelude.-(k.1, 1)
      let t.5: int = extern Prelude.+(acc.2, base.0)
      tail call f$let$go(base.0, t.4, t.5)
    b2:
      return acc.2
    }
    "#);
}

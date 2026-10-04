//! Perceus の `dup` / `decref` の位置と、呼び出しの `saved` (docs/spec/core-ir.md)。

mod common;

use common::core_text;
use eml_core_ir::Pass;

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"a\")";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn twice(s0) {
      dup s0
      let t1 = prim ++(s0, s0)
      return t1
    }
    fn ignore(s0) {
      decref s0
      return 1
    }
    fn main(p0) {
      let s1 = const "a"
      let t2 = call twice(s1)
      let t3 = perform println(t2)
      return t3
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn shadowed_and_discarded_strings() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let s = \"x\"\n  let s = s ++ \"y\"\n  let _ = \"z\"\n  println s";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn main(p0) {
      let s1 = const "x"
      let s2 = const "y"
      let t3 = prim ++(s1, s2)
      let s4 = const "z"
      decref s4
      let t5 = perform println(t3)
      return t5
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn a_non_tail_if_keeps_strings_used_later() {
    let text = "pick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn pick(b0, s1) {
      join j0(t3) [s1] {
        let t4 = prim ++(t3, s1)
        return t4
      }
      switch b0 {
        #0 ->
          let s2 = const "none"
          jump j0(s2)
        #1 ->
          dup s1
          jump j0(s1)
      }
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn calls_save_the_variables_used_after_them() {
    let text = "around : Int -> String -> String\naround n s =\n  let m = n + 1\n  let t = if n > 0 then twice s else s\n  t ++ show_int m\n\ntwice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r"
    fn around(n0, s1) {
      let t2 = prim +(n0, 1)
      join j0(t5) [t2] {
        let t6 = prim show_int(t2)
        let t7 = prim ++(t5, t6)
        return t7
      }
      let t3 = prim >(n0, 0)
      switch t3 {
        #0 ->
          jump j0(s1)
        #1 ->
          let t4 = call twice(s1) [t2]
          jump j0(t4)
      }
    }
    fn twice(s0) {
      dup s0
      let t1 = prim ++(s0, s0)
      return t1
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

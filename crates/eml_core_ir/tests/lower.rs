mod common;

use common::core_text;

#[test]
fn hello_world() {
    insta::assert_snapshot!(core_text("main : Unit -> <IO> Unit\nmain () = println \"hi\""), @r#"
    fn main(p0) {
      let s1 = const "hi"
      let t2 = perform println(s1)
      return t2
    }
    "#);
}

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"a\")";
    insta::assert_snapshot!(core_text(text), @r#"
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
    "#);
}

#[test]
fn shadowed_and_discarded_strings() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let s = \"x\"\n  let s = s ++ \"y\"\n  let _ = \"z\"\n  println s";
    insta::assert_snapshot!(core_text(text), @r#"
    fn main(p0) {
      let s1 = const "x"
      let s2 = const "y"
      let t3 = prim ++(s1, s2)
      let s4 = const "z"
      decref s4
      let t5 = perform println(t3)
      return t5
    }
    "#);
}

#[test]
fn a_non_tail_if_keeps_strings_used_later() {
    let text = "pick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r#"
    fn pick(b0, s1) {
      let t3 = {
        switch b0 {
          #0 ->
            let s2 = const "none"
            return s2
          #1 ->
            dup s1
            return s1
        }
      }
      let t4 = prim ++(t3, s1)
      return t4
    }
    fn main(p0) {
      return ()
    }
    "#);
}

#[test]
fn recursion_and_top_level_values() {
    let text = "answer : Int\nanswer = 42\n\ncount : Int -> Int\ncount n = if n == 0 then answer else count (n - 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (count 3))";
    insta::assert_snapshot!(core_text(text), @r"
    fn answer() {
      return 42
    }
    fn count(n0) {
      let t1 = prim ==(n0, 0)
      let t5 = {
        switch t1 {
          #0 ->
            let t3 = prim -(n0, 1)
            let t4 = call count(t3)
            return t4
          #1 ->
            let answer2 = call answer()
            return answer2
        }
      }
      return t5
    }
    fn main(p0) {
      let t1 = call count(3)
      let t2 = prim show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
    ");
}

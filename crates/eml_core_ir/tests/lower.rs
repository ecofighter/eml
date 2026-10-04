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
      join j0(t3) {
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
    fn main(p0) {
      let t1 = call count(3)
      let t2 = prim show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
    ");
}

#[test]
fn partial_and_extra_arguments_use_closures() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\nadder : Int -> Int -> Int\nadder x = add x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = add 1\n  let n = f 2 + adder 3 4\n  println (show_int n)";
    insta::assert_snapshot!(core_text(text), @r"
    fn add(a0, b1) {
      let t2 = prim +(a0, b1)
      return t2
    }
    fn adder(x0) {
      let c1 = closure add(x0)
      return c1
    }
    fn main(p0) {
      let c1 = closure add(1)
      let t2 = apply c1(2)
      let t3 = call adder(3)
      let t4 = apply t3(4)
      let t5 = prim +(t2, t4)
      let t6 = prim show_int(t5)
      let t7 = perform println(t6)
      return t7
    }
    ");
}

#[test]
fn builtins_used_as_values_are_wrapped() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = not >> not\n  apply println (show_int 1)";
    insta::assert_snapshot!(core_text(text), @r"
    fn apply(f0, x1) {
      let t2 = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let c1 = closure builtin$not()
      let c2 = closure builtin$not()
      let c3 = closure builtin$>>(c1, c2)
      decref c3
      let c4 = closure builtin$println()
      let t5 = prim show_int(1)
      let t6 = call apply(c4, t5)
      return t6
    }
    fn builtin$not(p0) {
      let t1 = prim not(p0)
      return t1
    }
    fn builtin$>>(p0, p1, p2) {
      let t3 = apply p0(p2)
      let t4 = apply p1(t3)
      return t4
    }
    fn builtin$println(p0) {
      let t1 = perform println(p0)
      return t1
    }
    ");
}

#[test]
fn lambdas_are_lifted_with_their_captures_first() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let s = \"!\"\n  let shout = fn t -> t ++ s\n  println (apply shout \"hi\")\n  println s";
    insta::assert_snapshot!(core_text(text), @r#"
    fn apply(f0, x1) {
      let t2 = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let s1 = const "!"
      dup s1
      let c2 = closure main$lambda0(s1)
      let s3 = const "hi"
      let t4 = call apply(c2, s3)
      let t5 = perform println(t4)
      let t6 = perform println(s1)
      return t6
    }
    fn main$lambda0(s0, t1) {
      let t2 = prim ++(t1, s0)
      return t2
    }
    "#);
}

#[test]
fn a_zero_arity_callee_is_evaluated_before_its_arguments() {
    let text = "k : Int -> Int -> Int\nk a b = a\n\nfive : Int -> Int\nfive = k 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (five (1 + 2)))";
    insta::assert_snapshot!(core_text(text), @r#"
    fn k(a0, b1) {
      return a0
    }
    fn five() {
      let c0 = closure k(5)
      return c0
    }
    fn main(p0) {
      let five1 = call five()
      let t2 = prim +(1, 2)
      let t3 = apply five1(t2)
      let t4 = prim show_int(t3)
      let t5 = perform println(t4)
      return t5
    }
    "#);
}

#[test]
fn a_tail_if_returns_from_each_arm() {
    let text = "sign : Int -> String\nsign n = if n < 0 then \"negative\" else \"non-negative\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r#"
    fn sign(n0) {
      let t1 = prim <(n0, 0)
      switch t1 {
        #0 ->
          let s3 = const "non-negative"
          return s3
        #1 ->
          let s2 = const "negative"
          return s2
      }
    }
    fn main(p0) {
      return ()
    }
    "#);
}

#[test]
fn ifs_in_a_condition_nest_join_points() {
    let text = "choose : Bool -> Bool -> Int\nchoose a b =\n  let n = if (if a then b else False) then 1 else 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn choose(a0, b1) {
      join j1(t2) {
        join j0(t3) {
          let t4 = prim +(t3, 1)
          return t4
        }
        switch t2 {
          #0 ->
            jump j0(2)
          #1 ->
            jump j0(1)
        }
      }
      switch a0 {
        #0 ->
          jump j1(#0)
        #1 ->
          jump j1(b1)
      }
    }
    fn main(p0) {
      return ()
    }
    ");
}

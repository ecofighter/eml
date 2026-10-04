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
    fn entry$main() {
      tailcall main(())
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
    fn entry$main() {
      tailcall main(())
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
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn a_non_tail_if_keeps_strings_used_later() {
    let text = "pick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r#"
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
          let t2 = prim -(n0, 1)
          tailcall count(t2)
        #1 ->
          tailcall answer()
      }
    }
    fn main(p0) {
      let t1 = call count(3)
      let t2 = prim show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
    fn entry$main() {
      tailcall main(())
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
      let t3 = call adder(3) [t2]
      let t4 = apply t3(4) [t2]
      let t5 = prim +(t2, t4)
      let t6 = prim show_int(t5)
      let t7 = perform println(t6)
      return t7
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn builtins_used_as_values_are_wrapped() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = not >> not\n  apply println (show_int 1)";
    insta::assert_snapshot!(core_text(text), @r"
    fn apply(f0, x1) {
      tailcall apply f0(x1)
    }
    fn main(p0) {
      let c1 = closure builtin$not()
      let c2 = closure builtin$not()
      let c3 = closure builtin$>>(c1, c2)
      decref c3
      let c4 = closure builtin$println()
      let t5 = prim show_int(1)
      tailcall apply(c4, t5)
    }
    fn builtin$not(p0) {
      let t1 = prim not(p0)
      return t1
    }
    fn builtin$>>(p0, p1, p2) {
      let t3 = apply p0(p2) [p1]
      tailcall apply p1(t3)
    }
    fn builtin$println(p0) {
      let t1 = perform println(p0)
      return t1
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn lambdas_are_lifted_with_their_captures_first() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let s = \"!\"\n  let shout = fn t -> t ++ s\n  println (apply shout \"hi\")\n  println s";
    insta::assert_snapshot!(core_text(text), @r#"
    fn apply(f0, x1) {
      tailcall apply f0(x1)
    }
    fn main(p0) {
      let s1 = const "!"
      dup s1
      let c2 = closure main$lambda0(s1)
      let s3 = const "hi"
      let t4 = call apply(c2, s3) [s1]
      let t5 = perform println(t4)
      let t6 = perform println(s1)
      return t6
    }
    fn main$lambda0(s0, t1) {
      let t2 = prim ++(t1, s0)
      return t2
    }
    fn entry$main() {
      tailcall main(())
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
    fn entry$main() {
      tailcall main(())
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
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn ifs_in_a_condition_nest_join_points() {
    let text = "choose : Bool -> Bool -> Int\nchoose a b =\n  let n = if (if a then b else False) then 1 else 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn choose(a0, b1) {
      join j0(t3) [] {
        let t4 = prim +(t3, 1)
        return t4
      }
      join j1(t2) [] {
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
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn calls_in_tail_position_are_tail_calls() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\ncall_twice : (Int -> Int) -> Int -> Int\ncall_twice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn loop(n0, acc1) {
      let t2 = prim ==(n0, 0)
      switch t2 {
        #0 ->
          let t3 = prim -(n0, 1)
          let t4 = prim +(acc1, 1)
          tailcall loop(t3, t4)
        #1 ->
          return acc1
      }
    }
    fn call_twice(f0, x1) {
      dup f0
      let t2 = apply f0(x1) [f0]
      tailcall apply f0(t2)
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn the_entry_applies_a_point_free_main_to_unit() {
    let text = "main : Unit -> <IO> Unit\nmain = fn () -> println \"point-free\"";
    insta::assert_snapshot!(core_text(text), @r#"
    fn main() {
      let c0 = closure main$lambda0()
      return c0
    }
    fn main$lambda0(p0) {
      let s1 = const "point-free"
      let t2 = perform println(s1)
      return t2
    }
    fn entry$main() {
      let f0 = call main()
      tailcall apply f0(())
    }
    "#);
}

#[test]
fn calls_save_the_variables_used_after_them() {
    let text = "around : Int -> String -> String\naround n s =\n  let m = n + 1\n  let t = if n > 0 then twice s else s\n  t ++ show_int m\n\ntwice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
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

#[test]
fn nested_join_points_capture_what_outer_join_points_need() {
    // 内側の join point の本体は外側の join point へ jump するので、外側の本体が使う `s2` も捕まえる
    let text = "label : Bool -> Bool -> String -> String\nlabel a b s =\n  let t =\n    if a then\n      let u = if b then s ++ \"!\" else \"plain\"\n      u ++ \"?\"\n    else s\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r#"
    fn label(a0, b1, s2) {
      join j0(t9) [s2] {
        let t10 = prim ++(t9, s2)
        return t10
      }
      switch a0 {
        #0 ->
          dup s2
          jump j0(s2)
        #1 ->
          join j1(t6) [s2] {
            let s7 = const "?"
            let t8 = prim ++(t6, s7)
            jump j0(t8)
          }
          switch b1 {
            #0 ->
              let s5 = const "plain"
              jump j1(s5)
            #1 ->
              let s3 = const "!"
              dup s2
              let t4 = prim ++(s2, s3)
              jump j1(t4)
          }
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
fn handlers_are_lifted_to_closures() {
    let text = "effect Ask where\n  ask : String -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let prefix = \"n = \"\n  let n =\n    handle ask \"x\" with\n      | ask key k -> resume k 1\n      | return x -> x + 1\n  println (prefix ++ show_int n)";
    insta::assert_snapshot!(core_text(text), @r#"
    fn main(p0) {
      let s1 = const "n = "
      let c2 = closure main$handle0()
      let c3 = closure main$handle0$ask()
      let c4 = closure main$handle0$return()
      let t5 = handle Ask(c2) {ask: c3} return c4 [s1]
      let t6 = prim show_int(t5)
      let t7 = prim ++(s1, t6)
      let t8 = perform println(t7)
      return t8
    }
    fn main$handle0(p0) {
      let s1 = const "x"
      tailcall perform Ask.ask(s1)
    }
    fn main$handle0$ask(key0, k1) {
      decref key0
      tailcall resume k1(1)
    }
    fn main$handle0$return(x0) {
      let t1 = prim +(x0, 1)
      return t1
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn operations_as_values_and_drop() {
    let text = "effect Log where\n  log : String -> String -> Unit\n\nrun : Unit -> <Log> Unit\nrun () =\n  let info = log \"info\"\n  info \"a\"\n\ndiscard : String -> Unit\ndiscard s = drop s\n\nmain : Unit -> <IO> Unit\nmain () = println \"x\"";
    insta::assert_snapshot!(core_text(text), @r#"
    fn run(p0) {
      let s1 = const "info"
      let c2 = closure op$log(s1)
      let s3 = const "a"
      tailcall apply c2(s3)
    }
    fn discard(s0) {
      let t1 = drop s0
      return t1
    }
    fn main(p0) {
      let s1 = const "x"
      let t2 = perform println(s1)
      return t2
    }
    fn op$log(p0, p1) {
      tailcall perform Log.log(p0, p1)
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

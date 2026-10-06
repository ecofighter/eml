//! 型付き HIR から Core IR への変換 (docs/spec/core-ir.md)。`simplify` と Perceus より前の形を見る。

mod common;

use common::core_text;
use eml_core_ir::Pass;

#[test]
fn hello_world() {
    insta::assert_snapshot!(core_text("main : Unit -> <IO> Unit\nmain () = println \"hi\"", Pass::Translate), @r#"
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
fn recursion_and_top_level_values() {
    let text = "answer : Int\nanswer = 42\n\ncount : Int -> Int\ncount n = if n == 0 then answer else count (n - 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (count 3))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
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
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn partial_and_extra_arguments_use_closures() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\nadder : Int -> Int -> Int\nadder x = add x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = add 1\n  let n = f 2 + adder 3 4\n  println (show_int n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
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
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn builtins_used_as_values_are_wrapped() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = not >> not\n  apply println (show_int 1)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    fn apply(f0, x1) {
      let t2 = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let c1 = closure builtin$not()
      let c2 = closure builtin$not()
      let c3 = closure builtin$>>(c1, c2)
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn apply(f0, x1) {
      let t2 = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let s1 = const "!"
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
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn a_zero_arity_callee_is_evaluated_before_its_arguments() {
    let text = "k : Int -> Int -> Int\nk a b = a\n\nfive : Int -> Int\nfive = k 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (five (1 + 2)))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
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
fn calls_in_tail_position_are_tail_calls() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\ncall_twice : (Int -> Int) -> Int -> Int\ncall_twice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    fn loop(n0, acc1) {
      let t2 = prim ==(n0, 0)
      switch t2 {
        #0 ->
          let t3 = prim -(n0, 1)
          let t4 = prim +(acc1, 1)
          let t5 = call loop(t3, t4)
          return t5
        #1 ->
          return acc1
      }
    }
    fn call_twice(f0, x1) {
      let t2 = apply f0(x1)
      let t3 = apply f0(t2)
      return t3
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
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
fn nested_join_points_capture_what_outer_join_points_need() {
    // 内側の join point の本体は外側の join point へ jump するので、外側の本体が使う `s2` も捕まえる
    let text = "label : Bool -> Bool -> String -> String\nlabel a b s =\n  let t =\n    if a then\n      let u = if b then s ++ \"!\" else \"plain\"\n      u ++ \"?\"\n    else s\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn label(a0, b1, s2) {
      join j0(t9) [s2] {
        let t10 = prim ++(t9, s2)
        return t10
      }
      switch a0 {
        #0 ->
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn main(p0) {
      let s1 = const "n = "
      let c2 = closure main$handle0()
      let c3 = closure main$handle0$ask()
      let c4 = closure main$handle0$return()
      let t5 = handle Ask(c2) {ask: c3} return c4
      let t6 = prim show_int(t5)
      let t7 = prim ++(s1, t6)
      let t8 = perform println(t7)
      return t8
    }
    fn main$handle0(p0) {
      let s1 = const "x"
      let t2 = perform Ask.ask(s1)
      return t2
    }
    fn main$handle0$ask(key0, k1) {
      let t2 = resume k1(1)
      return t2
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn run(p0) {
      let s1 = const "info"
      let c2 = closure op$log(s1)
      let s3 = const "a"
      let t4 = apply c2(s3)
      return t4
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

#[test]
fn constructors_with_fields_build_values() {
    let text = "data Option a =\n  | None\n  | Some a\n\nwrap : Int -> Option Int\nwrap n = Some n\n\nnest : Unit -> Option (Option Int)\nnest () = Some (Some 1)\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn wrap(n0) {
      let d1 = con #1(n0)
      return d1
    }
    fn nest(p0) {
      let d1 = con #1(1)
      let d2 = con #1(d1)
      return d2
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
fn a_constructor_used_as_a_function_value_is_wrapped() {
    let text = "data Pair a b =\n  | Pair a b\n\npairs : Int -> Pair Int Int\npairs n =\n  let make = Pair n\n  make 2\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    fn pairs(n0) {
      let c1 = closure con$Pair(n0)
      let t2 = apply c1(2)
      return t2
    }
    fn main(p0) {
      return ()
    }
    fn con$Pair(p0, p1) {
      let d2 = con #0(p0, p1)
      return d2
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn a_match_compiles_to_a_decision_tree_with_arm_join_points() {
    // 各枝の本体を join point にし、選んだ欄に現れないコンストラクタ (`None`) は残りの行列の join point へ jump する
    let text = "data Option a = | None | Some a\n\nf : Option (Option Int) -> Int\nf o = match o with\n  | Some (Some n) -> n\n  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn f(o0) {
      join j0(n1) [] {
        return n1
      }
      join j1() [] {
        return 0
      }
      join j2() [] {
        jump j1()
      }
      switch o0 {
        #0 ->
          jump j2()
        #1(x2) ->
          join j3() [] {
            jump j1()
          }
          switch x2 {
            #0 ->
              jump j3()
            #1(n3) ->
              jump j0(n3)
          }
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
fn tail_and_non_tail_matches() {
    // 末尾にない `match` は、`if` と同じく値を受ける join point の範囲に入り、枝の本体はその join point へ jump する
    let text = "data Option a = | None | Some a\n\ng : Option Int -> Int\ng o =\n  let n = match o with\n    | Some m -> m\n    | None -> 0\n  n + 1\n\nh : Option Int -> Int\nh o = match o with | Some m -> m | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn g(o0) {
      join j0(t3) [] {
        let t4 = prim +(t3, 1)
        return t4
      }
      join j1(m1) [] {
        jump j0(m1)
      }
      join j2() [] {
        jump j0(0)
      }
      switch o0 {
        #0 ->
          jump j2()
        #1(m2) ->
          jump j1(m2)
      }
    }
    fn h(o0) {
      join j0(m1) [] {
        return m1
      }
      join j1() [] {
        return 0
      }
      switch o0 {
        #0 ->
          jump j1()
        #1(m2) ->
          jump j0(m2)
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
fn constructor_patterns_in_let_lambda_and_equation_parameters() {
    // コンストラクタを含むパターンは、続きを本体にする join point の引数で変数を受け、枝が1つの決定木で分解する
    let text = "data Box a = | Box a\n\nby_equation : Box Int -> Int\nby_equation (Box n) = n\n\nby_let : Box Int -> Int\nby_let b =\n  let Box m = b\n  m + 1\n\nby_lambda : Box Int -> Int\nby_lambda b = (fn (Box k) -> k) b\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    fn by_equation(p0) {
      join j0(n1) [] {
        return n1
      }
      switch p0 {
        #0(n2) ->
          jump j0(n2)
      }
    }
    fn by_let(b0) {
      join j0(m1) [] {
        let t3 = prim +(m1, 1)
        return t3
      }
      switch b0 {
        #0(m2) ->
          jump j0(m2)
      }
    }
    fn by_lambda(b0) {
      let c1 = closure by_lambda$lambda0()
      let t2 = apply c1(b0)
      return t2
    }
    fn main(p0) {
      return ()
    }
    fn by_lambda$lambda0(p0) {
      join j0(k1) [] {
        return k1
      }
      switch p0 {
        #0(k2) ->
          jump j0(k2)
      }
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn a_variable_pattern_after_a_switch_binds_the_scrutinee() {
    // `ys` は `xs` の `Switch` の後で、`xs` そのものを受ける。残りの行列の join point は `xs` を捕まえる
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    fn size(xs0) {
      return 2
    }
    fn describe(xs0) {
      join j0() [] {
        return 1
      }
      join j1(ys1) [] {
        let t2 = call size(ys1)
        return t2
      }
      join j2() [xs0] {
        jump j1(xs0)
      }
      switch xs0 {
        #0 ->
          jump j2()
        #1(x3, x4) ->
          join j3() [xs0] {
            jump j1(xs0)
          }
          switch x4 {
            #0 ->
              jump j0()
            #1(x5, x6) ->
              jump j3()
          }
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
fn constructor_patterns_in_handler_clause_parameters() {
    // 操作の節と `return` の節はラムダと同じく関数に持ち上げるので、引数のコンストラクタのパターンも同じ経路で分解する
    let text = "data Box a = | Box a\n\neffect Give where\n  give : Box Int -> Int\n\nrun : Unit -> Int\nrun () =\n  handle Box (give (Box 1)) with\n    | give (Box n) k -> resume k n\n    | return (Box r) -> r\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    fn run(p0) {
      let c1 = closure run$handle0()
      let c2 = closure run$handle0$give()
      let c3 = closure run$handle0$return()
      let t4 = handle Give(c1) {give: c2} return c3
      return t4
    }
    fn main(p0) {
      return ()
    }
    fn run$handle0(p0) {
      let d1 = con #0(1)
      let t2 = perform Give.give(d1)
      let d3 = con #0(t2)
      return d3
    }
    fn run$handle0$give(p0, k1) {
      join j0(n2) [k1] {
        let t4 = resume k1(n2)
        return t4
      }
      switch p0 {
        #0(n3) ->
          jump j0(n3)
      }
    }
    fn run$handle0$return(p0) {
      join j0(r1) [] {
        return r1
      }
      switch p0 {
        #0(r2) ->
          jump j0(r2)
      }
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn equality_picks_the_comparison_of_the_operand_type() {
    // `Int` の `==` は今までどおり `prim ==` のまま表示し、`String` と `Bool` は型を前に付けた名前で表示する
    let text = "same : String -> Bool\nsame s = \"a\" == s\n\nflip : Bool -> Bool\nflip b = b != True\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn same(s0) {
      let s1 = const "a"
      let t2 = prim string==(s1, s0)
      return t2
    }
    fn flip(b0) {
      let t1 = prim bool!=(b0, #1)
      return t1
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
fn tuples_are_built_and_taken_apart_by_parameters_and_let() {
    // タプルの値はタグ 0 のコンストラクタの値で、タプルのパターンは枝が1つの `Switch` で分解する
    let text = "swap : (Int, String) -> (String, Int)\nswap (n, s) = (s, n)\n\nfirst : (Int, Int) -> Int\nfirst p =\n  let (a, _) = p\n  a\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn swap(p0) {
      join j0(n1, s2) [] {
        let d5 = con #0(s2, n1)
        return d5
      }
      switch p0 {
        #0(n3, s4) ->
          jump j0(n3, s4)
      }
    }
    fn first(p0) {
      join j0(a1) [] {
        return a1
      }
      switch p0 {
        #0(a2, x3) ->
          jump j0(a2)
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
fn a_tuple_column_is_a_single_constructor() {
    // タプルの欄を分解してから、その中の `Option` の欄で `Switch` する
    let text = "data Option a = | None | Some a\n\npick : (Option Int, Int) -> Int\npick p = match p with\n  | (Some n, _) -> n\n  | (None, k) -> k\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn pick(p0) {
      join j0(n1) [] {
        return n1
      }
      join j1(k2) [] {
        return k2
      }
      switch p0 {
        #0(x3, k4) ->
          switch x3 {
            #0 ->
              jump j1(k4)
            #1(n5) ->
              jump j0(n5)
          }
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
fn int_literals_compare_in_order_and_fall_back_to_the_rest() {
    // 異なるリテラルを上の行から順に比べ、最後の等しくない枝は残りの行列 (`_` の行) に進む
    let text = "describe : Int -> String\ndescribe n = match n with\n  | 0 -> \"zero\"\n  | 1 -> \"one\"\n  | -1 -> \"minus one\"\n  | _ -> \"many\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn describe(n0) {
      join j0() [] {
        let s1 = const "zero"
        return s1
      }
      join j1() [] {
        let s2 = const "one"
        return s2
      }
      join j2() [] {
        let s3 = const "minus one"
        return s3
      }
      join j3() [] {
        let s4 = const "many"
        return s4
      }
      let t5 = prim ==(n0, 0)
      switch t5 {
        #0 ->
          let t6 = prim ==(n0, 1)
          switch t6 {
            #0 ->
              let t7 = prim ==(n0, -1)
              switch t7 {
                #0 ->
                  jump j3()
                #1 ->
                  jump j2()
              }
            #1 ->
              jump j1()
          }
        #1 ->
          jump j0()
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
fn string_literals_build_each_literal_for_its_comparison() {
    // `String` のリテラルは比べるたびに作る。変数の枝は、比べた出現そのものを受ける
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn greet(name0) {
      join j0() [] {
        let s1 = const "hello"
        return s1
      }
      join j1() [] {
        let s2 = const "konnichiwa"
        return s2
      }
      join j2(other3) [] {
        return other3
      }
      let s4 = const "en"
      let t5 = prim string==(name0, s4)
      switch t5 {
        #0 ->
          let s6 = const "ja"
          let t7 = prim string==(name0, s6)
          switch t7 {
            #0 ->
              jump j2(name0)
            #1 ->
              jump j1()
          }
        #1 ->
          jump j0()
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
fn a_literal_column_inside_a_tuple() {
    // タプルを分解した後、最初の行でいちばん左の調べる欄 (リテラル) を選ぶ。等しくない枝の `Bool` の欄には `True` の
    // 行がないので、残りの行列の join point を作る
    let text = "classify : (Int, Bool) -> Int\nclassify p = match p with\n  | (0, True) -> 1\n  | (_, False) -> 2\n  | (n, _) -> n\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn classify(p0) {
      join j0() [] {
        return 1
      }
      join j1() [] {
        return 2
      }
      join j2(n1) [] {
        return n1
      }
      switch p0 {
        #0(n2, x3) ->
          let t4 = prim ==(n2, 0)
          switch t4 {
            #0 ->
              join j3() [n2] {
                jump j2(n2)
              }
              switch x3 {
                #0 ->
                  jump j1()
                #1 ->
                  jump j3()
              }
            #1 ->
              switch x3 {
                #0 ->
                  jump j1()
                #1 ->
                  jump j0()
              }
          }
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
fn file_operations_are_performed_on_the_io_handler() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  let (f, s) = read_all f\n  close f\n  println s";
    let ir = core_text(text, Pass::Translate);
    for op in ["perform open(", "perform read_all(", "perform close("] {
        assert!(ir.contains(op), "{ir}");
    }
}

//! 型付き HIR から Core IR への変換 (docs/spec/core-ir.md)。`simplify` と Perceus より前の形を見る。

use crate::common::core_text;
use eml_core_ir::Pass;

#[test]
fn an_io_operation_used_as_a_value_is_wrapped_with_its_io_call() {
    // `IO` の操作は perform せずに実行時がその場で処理する (docs/spec/effects.md の「組み込みの `IO`」)
    let text = "each : (String -> <IO> Unit) -> <IO> Unit\neach f = f \"x\"\n\nmain : Unit -> <IO> Unit\nmain () = each println";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn op$println(p0^) {"), "{shown}");
    assert!(shown.contains("perform println(p0)"), "{shown}");
    assert!(!shown.contains("perform IO.println"), "{shown}");
}

#[test]
fn hello_world() {
    insta::assert_snapshot!(core_text("main : Unit -> <IO> Unit\nmain () = println \"hi\"", Pass::Translate), @r#"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      let s1^ = const "hi"
      let t2 = perform println(s1)
      return t2
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn the_entry_function_is_chosen_by_the_caller() {
    // REPL は、`main` の代わりにその回の式から作った関数を入口にする
    // (docs/implementation/architecture.md の「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」)
    let text = "main : Unit -> <IO> Unit\nmain () = println \"main\"\n\nalt : Unit -> <IO> Unit\nalt () = println \"alt\"";
    let checked = eml_test_support::check(text);
    let alt = checked
        .program
        .functions()
        .find(|(_, function)| function.name == "alt")
        .map(|(id, _)| id)
        .expect("alt");
    let program = eml_core_ir::lower_until(&checked.program, &checked.typed, alt, Pass::Translate);
    insta::assert_snapshot!(eml_core_ir::pretty(&program), @r#"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn alt(p0) {
      let s1^ = const "alt"
      let t2 = perform println(s1)
      return t2
    }
    fn entry$alt() {
      tailcall alt(())
    }
    "#);
}

#[test]
fn only_functions_reachable_from_the_entry_are_lowered() {
    // 使わない Prelude の関数を Core IR に入れないため、入口から届く関数だけを変換する (docs/spec/core-ir.md)
    let text = "used : Int -> Int\nused x = x\n\nunused : Int -> Int\nunused x = x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (used 1))";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn used("), "{shown}");
    assert!(shown.contains("fn main("), "{shown}");
    assert!(!shown.contains("fn unused("), "{shown}");
}

#[test]
fn recursion_and_top_level_values() {
    let text = "answer : Int\nanswer = 42\n\ncount : Int -> Int\ncount n = if n == 0 then answer else count (n - 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (count 3))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
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
      let t2^ = prim show_int(t1)
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
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn add(a0, b1) {
      let t2 = prim +(a0, b1)
      return t2
    }
    fn adder(x0) {
      let c1^ = closure add(x0)
      return c1
    }
    fn main(p0) {
      let c1^ = closure add(1)
      let t2 = apply c1(2)
      let t3^ = call adder(3)
      let t4 = apply t3(4)
      let t5 = prim +(t2, t4)
      let t6^ = prim show_int(t5)
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
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn Prelude.not($00) {
      switch $00 {
        #0 ->
          return #1
        #1 ->
          return #0
      }
    }
    fn Prelude.>>(f0^, g1^) {
      let c2^ = closure Prelude.>>$lambda0(f0, g1)
      return c2
    }
    fn apply(f0^, x1^) {
      let t2^ = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let t1^ = call Prelude.>>(&Prelude.not, &Prelude.not)
      let t2^ = prim show_int(1)
      let t3 = call apply(&op$println, t2)
      return t3
    }
    fn Prelude.>>$lambda0(f0^, g1^, x2^) {
      let t3^ = apply f0(x2)
      let t4^ = apply g1(t3)
      return t4
    }
    fn op$println(p0^) {
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
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn apply(f0^, x1^) {
      let t2^ = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let s1^ = const "!"
      let c2^ = closure main$lambda0(s1)
      let s3^ = const "hi"
      let t4^ = call apply(c2, s3)
      let t5 = perform println(t4)
      let t6 = perform println(s1)
      return t6
    }
    fn main$lambda0(s0^, t1^) {
      let t2^ = prim ++(t1, s0)
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn k(a0, b1) {
      return a0
    }
    fn five() {
      let c0^ = closure k(5)
      return c0
    }
    fn main(p0) {
      let five1^ = call five()
      let t2 = prim +(1, 2)
      let t3 = apply five1(t2)
      let t4^ = prim show_int(t3)
      let t5 = perform println(t4)
      return t5
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn a_tail_if_returns_from_each_arm() {
    let text = "sign : Int -> String\nsign n = if n < 0 then \"negative\" else \"non-negative\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn calls_in_tail_position_return_their_result_before_simplify() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\ncall_twice : (Int -> Int) -> Int -> Int\ncall_twice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
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
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main() {
      return &main$lambda0
    }
    fn main$lambda0(p0) {
      let s1^ = const "point-free"
      let t2 = perform println(s1)
      return t2
    }
    fn entry$main() {
      let f0^ = call main()
      tailcall apply f0(())
    }
    "#);
}

#[test]
fn nested_join_points_capture_what_outer_join_points_need() {
    // 内側の join point の本体は外側の join point へ jump するので、外側の本体が使う `s2` も捕まえる
    let text = "label : Bool -> Bool -> String -> String\nlabel a b s =\n  let t =\n    if a then\n      let u = if b then s ++ \"!\" else \"plain\"\n      u ++ \"?\"\n    else s\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn handlers_are_lifted_to_closures() {
    let text = "effect Ask where\n  ask : String -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let prefix = \"n = \"\n  let n =\n    handle ask \"x\" with\n      | ask key k -> resume k 1\n      | return x -> x + 1\n  println (prefix ++ show_int n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    effect IO { println/1, open/1, read_all/1, close/1 }
    effect Ask { ask/1 }
    fn main(p0) {
      let s1^ = const "n = "
      let t2 = handle Ask(&main$handle0, ()) {ask: &main$handle0$ask} return &main$handle0$return
      let t3^ = prim show_int(t2)
      let t4^ = prim ++(s1, t3)
      let t5 = perform println(t4)
      return t5
    }
    fn main$handle0(p0) {
      let s1^ = const "x"
      let t2 = perform Ask.ask(s1)
      return t2
    }
    fn main$handle0$ask(key0^, k1^, p2) {
      let t3 = resume k1(1, ())
      return t3
    }
    fn main$handle0$return(x0, p1) {
      let t2 = prim +(x0, 1)
      return t2
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
    effect IO { println/1, open/1, read_all/1, close/1 }
    effect Log { log/2 }
    fn main(p0) {
      let s1^ = const "x"
      let t2 = perform println(s1)
      return t2
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn constructors_with_fields_build_values() {
    let text = "data Option a =\n  | None\n  | Some a\n\nwrap : Int -> Option Int\nwrap n = Some n\n\nnest : Unit -> Option (Option Int)\nnest () = Some (Some 1)\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
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
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      return ()
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
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
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      return ()
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
    effect IO { println/1, open/1, read_all/1, close/1 }
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
    effect IO { println/1, open/1, read_all/1, close/1 }
    effect Give { give/1 }
    fn main(p0) {
      return ()
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn tuples_are_built_and_taken_apart_by_parameters_and_let() {
    // タプルの値はタグ 0 のコンストラクタの値で、タプルのパターンは枝が1つの `Switch` で分解する
    let text = "swap : (Int, String) -> (String, Int)\nswap (n, s) = (s, n)\n\nfirst : (Int, Int) -> Int\nfirst p =\n  let (a, _) = p\n  a\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
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
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn string_literals_build_each_literal_for_its_comparison() {
    // `String` のリテラルは比べるたびに作る。変数の枝は、比べた出現そのものを受ける
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn a_literal_column_inside_a_tuple() {
    // タプルを分解した後、最初の行でいちばん左の調べる欄 (リテラル) を選ぶ。等しくない枝の `Bool` の欄には `True` の
    // 行がないので、残りの行列の join point を作る
    let text = "classify : (Int, Bool) -> Int\nclassify p = match p with\n  | (0, True) -> 1\n  | (_, False) -> 2\n  | (n, _) -> n\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
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

#[test]
fn functions_without_captures_are_values() {
    let text = "twice : (Int -> Int) -> Int -> Int\ntwice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = 3\n  println (show_int (twice (fn x -> x + 1) 1 + twice (fn x -> x + n) 1))";
    let ir = core_text(text, Pass::Translate);
    // 捕獲のないラムダは関数の値で、捕獲のあるラムダはクロージャである
    assert!(ir.contains("&main$lambda0"), "{ir}");
    assert!(!ir.contains("closure main$lambda0"), "{ir}");
    assert!(ir.contains("closure main$lambda1(3)"), "{ir}");
}

#[test]
fn a_handler_with_a_state_passes_its_initial_value_and_takes_the_state_from_its_clauses() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle ask () + ask () from 10 with\n      | ask () k st -> resume k st (st + 1)\n      | return x st -> x * st\n  println (show_int n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    effect Ask { ask/1 }
    fn main(p0) {
      let t1 = handle Ask(&main$handle0, 10) {ask: &main$handle0$ask} return &main$handle0$return
      let t2^ = prim show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
    fn main$handle0(p0) {
      let t1 = perform Ask.ask(())
      let t2 = perform Ask.ask(())
      let t3 = prim +(t1, t2)
      return t3
    }
    fn main$handle0$ask(p0, k1^, st2) {
      let t3 = prim +(st2, 1)
      let t4 = resume k1(st2, t3)
      return t4
    }
    fn main$handle0$return(x0, st1) {
      let t2 = prim *(x0, st1)
      return t2
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn effects_are_numbered_with_io_first_then_in_declaration_order() {
    let text = "effect A where\n  a : Unit -> Int\neffect B where\n  b : Unit -> Int\nmain : Unit -> <IO> Unit\nmain () =\n  let x = handle a () with\n    | a () k -> resume k 1\n  let y = handle b () with\n    | b () k -> resume k 2\n  println (show_int (x + y))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    effect A { a/1 }
    effect B { b/1 }
    fn main(p0) {
      let t1 = handle A(&main$handle0, ()) {a: &main$handle0$a} return &main$handle0$return
      let t2 = handle B(&main$handle1, ()) {b: &main$handle1$b} return &main$handle1$return
      let t3 = prim +(t1, t2)
      let t4^ = prim show_int(t3)
      let t5 = perform println(t4)
      return t5
    }
    fn main$handle0(p0) {
      let t1 = perform A.a(())
      return t1
    }
    fn main$handle0$a(p0, k1^, p2) {
      let t3 = resume k1(1, ())
      return t3
    }
    fn main$handle0$return($r0, p1) {
      return $r0
    }
    fn main$handle1(p0) {
      let t1 = perform B.b(())
      return t1
    }
    fn main$handle1$b(p0, k1^, p2) {
      let t3 = resume k1(2, ())
      return t3
    }
    fn main$handle1$return($r0, p1) {
      return $r0
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn a_prelude_function_is_lowered_under_the_prelude_name() {
    // Prelude の関数の Core IR の名前には `Prelude.` を付け、ユーザーの関数と名前が重ならないようにする
    // (docs/spec/core-ir.md)
    let text = "main : Unit -> <IO> Unit\nmain () = if not True && True then println \"a\" else println \"b\"";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn Prelude.not("), "{shown}");
    assert!(shown.contains("call Prelude.not("), "{shown}");
    assert!(!shown.contains("fn Prelude.&&("), "{shown}");
}

#[test]
fn a_user_function_hides_the_prelude_function_of_the_same_name() {
    let text = "not : Bool -> Bool\nnot b = b\n\nmain : Unit -> <IO> Unit\nmain () = if not True then println \"a\" else println \"b\"";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn not("), "{shown}");
    assert!(!shown.contains("Prelude.not"), "{shown}");
}

#[test]
fn the_prelude_bool_tags_match_the_core_ir_constants() {
    // Core IR は `Bool` を定数のタグで表す。タグは Prelude の宣言の順で決まる (docs/spec/core-ir.md)
    let program = eml_test_support::lower_clean("").program;
    assert_eq!(program[program.lang.false_ctor].tag, eml_core_ir::FALSE);
    assert_eq!(program[program.lang.true_ctor].tag, eml_core_ir::TRUE);
}

#[test]
fn an_arm_reached_by_one_leaf_sits_at_the_leaf() {
    // 1つの葉からだけ届く枝の本体は、引数を `let` で束縛してその葉に置き、複数の葉から届く枝だけを join point にする
    // (docs/spec/core-ir.md)。`pick` の `(n, _)` は、`0` の case の
    // `default` と、外側の `default` の2つの葉から届く
    let text = "data Shape = | Dot | Box Int Int\n\narea : Shape -> Int\narea s =\n  match s with\n    | Box w h -> w * h\n    | Dot -> 0\n\npick : Int -> Int -> Int\npick a b =\n  match (a, b) with\n    | (0, 1) -> 0\n    | (n, _) -> n\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (area (Box 2 3) + pick 0 1))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect IO { println/1, open/1, read_all/1, close/1 }
    fn area(s0^) {
      switch s0 {
        #0 ->
          return 0
        #1(w4, h5) ->
          let w1 = w4
          let h2 = h5
          let t3 = prim *(w1, h2)
          return t3
      }
    }
    fn pick(a0, b1) {
      let d2^ = con #0(a0, b1)
      join j0(n3) [] {
        return n3
      }
      switch d2 {
        #0(n4, x5) ->
          switch n4 {
            0 ->
              switch x5 {
                1 ->
                  return 0
                _ ->
                  jump j0(n4)
              }
            _ ->
              jump j0(n4)
          }
      }
    }
    fn main(p0) {
      let d1^ = con #1(2, 3)
      let t2 = call area(d1)
      let t3 = call pick(0, 1)
      let t4 = prim +(t2, t3)
      let t5^ = prim show_int(t4)
      let t6 = perform println(t5)
      return t6
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

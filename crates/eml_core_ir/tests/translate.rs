//! 型付き HIR から Core IR への変換 (docs/spec/core-ir.md)。`simplify` と Perceus より前の形を見る。

use crate::common::{core_text, core_text_files};
use eml_core_ir::Pass;

#[test]
fn an_io_operation_used_as_a_value_is_wrapped_with_its_io_call() {
    // `IO` の操作は perform せずに実行時がその場で処理する (docs/spec/effects.md の「組み込みの `IO`」)
    let text = "each : (String -> <IO> Unit) -> <IO> Unit\neach f = f \"x\"\n\nmain : Unit -> <IO> Unit\nmain () = each println";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn op$Prelude.println(p0^) {"), "{shown}");
    assert!(shown.contains("perform println(p0)"), "{shown}");
    assert!(!shown.contains("perform Prelude.IO."), "{shown}");
}

#[test]
fn hello_world() {
    insta::assert_snapshot!(core_text("main : Unit -> <IO> Unit\nmain () = println \"hi\"", Pass::Translate), @r#"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
fn names_outside_the_entry_are_qualified_with_their_module() {
    // テキストの形は関数とエフェクトを名前で引くので、入口以外のモジュールの名前には `モジュール名.` を付ける
    // (docs/spec/core-ir.md)。入れ子のモジュールのエフェクトの `perform` と `handle` も読み戻せることを確かめる
    let csv = "pub data Row = | Row Int\n\npub effect Parse where\n  next : Unit -> Int\n\npub parse : Unit -> <Parse> Row\nparse () =\n  let get = next\n  let make = Row\n  make (get ())";
    let main = "import Report.Csv\n\napply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle Csv.parse () with\n      | Csv.next () k -> k 1\n      | return r -> r\n  let Csv.Row n = r\n  apply println (show_int n)";
    insta::assert_snapshot!(core_text_files(main, &[("Report/Csv.em", csv)], Pass::Translate), @"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    effect Report.Csv.Parse { next/1 }
    fn apply(f0^, x1^) {
      let t2^ = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let t1^ = handle Report.Csv.Parse(&main$handle0, ()) {next: &main$handle0$next} return &main$handle0$return
      join j0(n2) [] {
        let t4^ = extern Prelude.show_int(n2)
        let t5 = call apply(&op$Prelude.println, t4)
        return t5
      }
      switch t1 {
        #0(n3) ->
          jump j0(n3)
      }
    }
    fn Report.Csv.parse(p0) {
      let t1 = apply &op$Report.Csv.next(())
      let t2^ = apply &con$Report.Csv.Row(t1)
      return t2
    }
    fn main$handle0(p0) {
      let t1^ = call Report.Csv.parse(())
      return t1
    }
    fn main$handle0$next(p0, k1^, p2) {
      let t3^ = resume k1(1, ())
      return t3
    }
    fn main$handle0$return(r0^, p1) {
      return r0
    }
    fn op$Prelude.println(p0^) {
      let t1 = perform println(p0)
      return t1
    }
    fn op$Report.Csv.next(p0) {
      tailcall perform Report.Csv.Parse.next(p0)
    }
    fn con$Report.Csv.Row(p0) {
      let d1^ = con #0(p0)
      return d1
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn an_entry_operation_does_not_collide_with_a_prelude_operation() {
    // 入口の `println` という操作を包む関数と、Prelude の `println` を包む関数は、Prelude の側に `Prelude.` が付くので
    // 名前が重ならない (docs/spec/core-ir.md)
    let text = "effect Log where\n  println : String -> Unit\n\neach : (String -> <e> Unit) -> <e> Unit\neach f = f \"x\"\n\nlogged : Unit -> <Log> Unit\nlogged () = each println\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle logged () with\n    | println s k -> k ()\n  each Prelude.println";
    let shown = core_text(text, Pass::Translate);
    assert!(
        shown.contains("fn op$println(p0^) {\n  tailcall perform Log.println(p0)\n}"),
        "{shown}"
    );
    assert!(
        shown.contains(
            "fn op$Prelude.println(p0^) {\n  let t1 = perform println(p0)\n  return t1\n}"
        ),
        "{shown}"
    );
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn answer() {
      return 42
    }
    fn count(n0) {
      let t1 = extern Prelude.int_eq(n0, 0)
      switch t1 {
        #0 ->
          let t3 = extern Prelude.-(n0, 1)
          let t4 = call count(t3)
          return t4
        #1 ->
          let answer2 = call answer()
          return answer2
      }
    }
    fn main(p0) {
      let t1 = call count(3)
      let t2^ = extern Prelude.show_int(t1)
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn add(a0, b1) {
      let t2 = extern Prelude.+(a0, b1)
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
      let t5 = extern Prelude.+(t2, t4)
      let t6^ = extern Prelude.show_int(t5)
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
      let t2^ = extern Prelude.show_int(1)
      let t3 = call apply(&op$Prelude.println, t2)
      return t3
    }
    fn Prelude.>>$lambda0(f0^, g1^, x2^) {
      let t3^ = apply f0(x2)
      let t4^ = apply g1(t3)
      return t4
    }
    fn op$Prelude.println(p0^) {
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
      let t2^ = extern Prelude.++(t1, s0)
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn k(a0, b1) {
      return a0
    }
    fn five() {
      let c0^ = closure k(5)
      return c0
    }
    fn main(p0) {
      let five1^ = call five()
      let t2 = extern Prelude.+(1, 2)
      let t3 = apply five1(t2)
      let t4^ = extern Prelude.show_int(t3)
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    let text = "effect Ask where\n  ask : String -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let prefix = \"n = \"\n  let n =\n    handle ask \"x\" with\n      | ask key k -> k 1\n      | return x -> x + 1\n  println (prefix ++ show_int n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    effect Ask { ask/1 }
    fn main(p0) {
      let s1^ = const "n = "
      let t2 = handle Ask(&main$handle0, ()) {ask: &main$handle0$ask} return &main$handle0$return
      let t3^ = extern Prelude.show_int(t2)
      let t4^ = extern Prelude.++(s1, t3)
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
      let t2 = extern Prelude.+(x0, 1)
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    let text = "data Box a = | Box a\n\neffect Give where\n  give : Box Int -> Int\n\nrun : Unit -> Int\nrun () =\n  handle Box (give (Box 1)) with\n    | give (Box n) k -> k n\n    | return (Box r) -> r\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    // 比べ方ごとの extern は、型の名前を前に付けた名前 (`Prelude.int_eq`、`Prelude.string_eq`、`Prelude.bool_ne`) で表示する
    let text = "same : String -> Bool\nsame s = \"a\" == s\n\nflip : Bool -> Bool\nflip b = b != True\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
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
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle ask () + ask () from 10 with\n      | ask () k st -> k st (st + 1)\n      | return x st -> x * st\n  println (show_int n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    effect Ask { ask/1 }
    fn main(p0) {
      let t1 = handle Ask(&main$handle0, 10) {ask: &main$handle0$ask} return &main$handle0$return
      let t2^ = extern Prelude.show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
    fn main$handle0(p0) {
      let t1 = perform Ask.ask(())
      let t2 = perform Ask.ask(())
      let t3 = extern Prelude.+(t1, t2)
      return t3
    }
    fn main$handle0$ask(p0, k1^, st2) {
      let t3 = extern Prelude.+(st2, 1)
      let t4 = resume k1(st2, t3)
      return t4
    }
    fn main$handle0$return(x0, st1) {
      let t2 = extern Prelude.*(x0, st1)
      return t2
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn effects_are_numbered_with_io_first_then_in_declaration_order() {
    let text = "effect A where\n  a : Unit -> Int\neffect B where\n  b : Unit -> Int\nmain : Unit -> <IO> Unit\nmain () =\n  let x = handle a () with\n    | a () k -> k 1\n  let y = handle b () with\n    | b () k -> k 2\n  println (show_int (x + y))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    effect A { a/1 }
    effect B { b/1 }
    fn main(p0) {
      let t1 = handle A(&main$handle0, ()) {a: &main$handle0$a} return &main$handle0$return
      let t2 = handle B(&main$handle1, ()) {b: &main$handle1$b} return &main$handle1$return
      let t3 = extern Prelude.+(t1, t2)
      let t4^ = extern Prelude.show_int(t3)
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn area(s0^) {
      switch s0 {
        #0 ->
          return 0
        #1(w4, h5) ->
          let w1 = w4
          let h2 = h5
          let t3 = extern Prelude.*(w1, h2)
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
      let t4 = extern Prelude.+(t2, t3)
      let t5^ = extern Prelude.show_int(t4)
      let t6 = perform println(t5)
      return t6
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn effects_of_the_same_name_in_two_modules_stay_apart() {
    // エフェクトの表は名前で引くので、入口の `Log` と `Audit.Log` は別の名前になる (docs/spec/core-ir.md)
    let audit = "pub effect Log where\n  emit : Int -> Unit\n\npub audited : Unit -> <Log> Unit\naudited () = emit 1";
    let main = "import Audit\n\neffect Log where\n  emit : Int -> Unit\n\nlocal : Unit -> <Log> Unit\nlocal () = emit 2\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle Audit.audited () with\n    | Audit.emit n k -> k (println (show_int n))\n  handle local () with\n    | emit n k -> k (println (show_int n))";
    let shown = core_text_files(main, &[("Audit.em", audit)], Pass::Translate);
    for expected in [
        "effect Log { emit/1 }",
        "effect Audit.Log { emit/1 }",
        "perform Log.emit(",
        "perform Audit.Log.emit(",
        "handle Log(",
        "handle Audit.Log(",
    ] {
        assert!(shown.contains(expected), "{expected}\n{shown}");
    }
}

#[test]
fn each_comparison_picks_the_instruction_of_its_operand_type() {
    // 比べる命令は、型検査が `==` と `!=` の参照に記録した型引数から選ぶ
    let text = "compare : Int -> String -> Bool -> (Bool, Bool, Bool, Bool, Bool, Bool)\ncompare n s b = (n == 1, n != 2, s == \"a\", s != \"b\", b == True, b != False)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = compare 1 \"a\" True\n  ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn compare(n0, s1^, b2) {
      let t3 = extern Prelude.int_eq(n0, 1)
      let t4 = extern Prelude.int_ne(n0, 2)
      let s5^ = const "a"
      let t6 = extern Prelude.string_eq(s1, s5)
      let s7^ = const "b"
      let t8 = extern Prelude.string_ne(s1, s7)
      let t9 = extern Prelude.bool_eq(b2, #1)
      let t10 = extern Prelude.bool_ne(b2, #0)
      let d11^ = con #0(t3, t4, t6, t8, t9, t10)
      return d11
    }
    fn main(p0) {
      let s1^ = const "a"
      let t2^ = call compare(1, s1, #1)
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

const STATE: &str = "\
effect State s where
  get : Unit -> s
  put : s -> Unit

";

/// `body` を `State Int` の handler の中で動かす `main`。変換は入口から届く関数だけを作るので、確かめる関数をここから呼ぶ。
fn state_main(body: &str) -> String {
    format!(
        "main : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle {body} from 0 with\n      | get () k st -> k st st\n      | put s k _ -> k () s\n      | return x _ -> x\n  println (show_int n)\n"
    )
}

/// 表示したプログラムから、関数 `name` の定義だけを取り出す。
fn function(core: &str, name: &str) -> String {
    let start = core
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no function `{name}`\n{core}"));
    let end = core[start..]
        .find("\n}\n")
        .map_or(core.len(), |end| start + end + 2);
    core[start..end].to_string()
}

/// 型検査が記録した `mask` を、その呼び出しに付ける (docs/spec/core-ir.md)。
#[test]
fn a_masked_callback_call() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb =\n  let n = get ()\n  cb ()\n\n{}",
        state_main("run (fn () -> 1)")
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run"), @"
    fn run(cb0^) {
      let t1 = perform State.get(())
      let t2^ = mask[State] apply cb0(())
      return t2
    }
    ");
}

/// 引数がそろう既知の関数の呼び出しは、最後の矢印の `mask` を使う。前の矢印は部分適用で、エフェクトを起こさない。
#[test]
fn a_saturated_known_call_takes_the_mask_of_its_last_arrow() {
    let text = format!(
        "{STATE}twice : Int -> (Unit -> <e> a) -> <e> a\ntwice _ cb =\n  let _ = cb ()\n  cb ()\n\nrun : (Unit -> <e> a) -> <State Int | e> a\nrun cb = twice 1 cb\n\n{}",
        state_main("run (fn () -> 1)")
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run"), @"
    fn run(cb0^) {
      let t1^ = mask[State] call twice(1, cb0)
      return t1
    }
    ");
}

/// 矢印ごとの `mask` が変わる境目で `apply` を分ける。
#[test]
fn arrows_with_different_masks_are_applied_apart() {
    let text = format!(
        "{STATE}h : (Int -> <e> Int -> <State Int | e> Int) -> <State Int | e> Int\nh f = f 1 2\n\n{}",
        state_main("h (fn x y -> x + y)")
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "h"), @"
    fn h(f0^) {
      let t1^ = mask[State] apply f0(1)
      let t2 = apply t1(2)
      return t2
    }
    ");
}

/// 節の中の継続の呼び出しの `mask`。節は handle の外側の row で動くので、ふつうは `mask` が要らない。
#[test]
fn a_continuation_call_in_its_clause_has_no_mask() {
    let text = format!(
        "{STATE}run : (Unit -> <State Int | e> a) -> <e> a\nrun action =\n  handle action () from 0 with\n    | get () k st -> k st st\n    | put n k _ -> k () n\n    | return x _ -> x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (run (fn () -> get ())))\n"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run$handle0$get"), @"
    fn run$handle0$get(p0, k1^, st2) {
      let t3^ = resume k1(st2, st2)
      return t3
    }
    ");
}

/// 節の中の handle の本体にある継続の呼び出しは、その handle が足したラベルを飛ばす。型検査が継続の呼び出しの矢印 0 に
/// 記録した `mask` を使う。
#[test]
fn a_continuation_call_inside_an_inner_handle_is_masked() {
    let text = "\
effect Ask where
  ask : Unit -> Int

effect Log where
  log : String -> Unit

f : (Unit -> <Ask | e> Int) -> <e> Int
f action =
  handle action () with
    | ask () k ->
        handle k 1 with
          | log m k2 ->
              k2 ()

act : Unit -> <Ask, Log> Int
act () =
  let n = ask ()
  log \"after\"
  n + 1

main : Unit -> <IO> Unit
main () =
  let r = handle f act with
            | log m k ->
                println (\"outer \" ++ m)
                k ()
  println (show_int r)
";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f$handle1"), @"
    fn f$handle1(k0^, p1) {
      let t2 = mask[Log] resume k0(1, ())
      return t2
    }
    ");
}

/// 既知の関数に引数の数より多くの引数を渡す呼び出しは、引数の数までを `call` し、残りを `apply` する。`call` は関数を
/// 返すだけでエフェクトを起こさないので `mask` を付けず、残りの引数の矢印の `mask` は `apply` に付ける。
#[test]
fn extra_arguments_of_a_known_call_take_the_mask_of_their_arrow() {
    let text = format!(
        "{STATE}pick : Int -> (Unit -> <e> a) -> <e> a\npick _ = fn cb -> cb ()\n\nrun : (Unit -> <e> a) -> <State Int | e> a\nrun cb = pick 1 cb\n\n{}",
        state_main("run (fn () -> 1)")
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run"), @"
    fn run(cb0^) {
      let t1^ = call pick(1)
      let t2^ = mask[State] apply t1(cb0)
      return t2
    }
    ");
}

const ASK: &str = "effect Ask where\n  ask : Unit -> Int\n\n";

/// `ASK` と、handle の式 `handle` の結果の `Int` を表示する `main` の、translate 直後の Core IR。
fn handled(extra: &str, handle: &str) -> String {
    core_text(
        &format!(
            "{ASK}{extra}main : Unit -> <IO> Unit\nmain () =\n  let n =\n{handle}\n  println (show_int n)"
        ),
        Pass::Translate,
    )
}

/// `k` を直接呼ぶだけなら、生の継続への `resume` にする (docs/spec/core-ir.md)。
#[test]
fn a_continuation_called_directly_is_resumed() {
    let shown = handled("", "    handle ask () with\n      | ask () k -> k 1");
    assert!(shown.contains("resume k1(1, ())"), "{shown}");
    assert!(!shown.contains("cont$"), "{shown}");
}

/// 状態のある handler の `k st (st + 1)` は、引数を評価してから1つの `resume` になる。
#[test]
fn a_continuation_with_an_expression_argument_is_resumed_once() {
    let shown = handled(
        "",
        "    handle ask () from 0 with\n      | ask () k st -> k st (st + 1)\n      | return x _ -> x",
    );
    assert_eq!(shown.matches("resume ").count(), 1, "{shown}");
    assert!(
        shown.find("extern Prelude.+(").unwrap() < shown.find("resume ").unwrap(),
        "{shown}"
    );
    assert!(!shown.contains("cont$"), "{shown}");
}

/// 入れ子の handle の本体が `k` を捕まえて直接呼ぶ形も、生の継続のまま `resume` する。
#[test]
fn a_continuation_called_inside_an_inner_handle_is_resumed() {
    let shown = handled(
        "effect Log where\n  log : Int -> Unit\n\n",
        "    handle ask () with\n      | ask () k ->\n          handle k 1 with\n            | log _ j -> j ()",
    );
    assert!(shown.contains("resume k"), "{shown}");
    assert!(!shown.contains("cont$"), "{shown}");
}

/// 入れ子のラムダが `k` を飽和で呼ぶだけなら、ラムダの関数の中でも生の継続を `resume` し、`cont$` で包まない
/// (docs/spec/core-ir.md)。
#[test]
fn a_continuation_called_saturated_in_a_lambda_is_resumed_there() {
    let shown = handled(
        "",
        "    handle ask () with\n      | ask () k ->\n          let f = fn x -> k x\n          f 1",
    );
    assert!(shown.contains("resume k"), "{shown}");
    assert!(!shown.contains("cont$"), "{shown}");
}

/// 状態のある `k v st` の `mask` は、継続の呼び出しの矢印 1 に記録された row から付く。
#[test]
fn a_stateful_continuation_called_inside_an_inner_handle_carries_its_mask() {
    let text = "\
effect Ask where
  ask : Unit -> Int

effect Log where
  log : String -> Unit

f : (Unit -> <Ask | e> Int) -> <e> Int
f action =
  handle action () from 0 with
    | ask () k st ->
        handle k 1 st with
          | log m k2 ->
              k2 ()
    | return x _ -> x

act : Unit -> <Ask, Log> Int
act () =
  let n = ask ()
  log \"after\"
  n + 1

main : Unit -> <IO> Unit
main () =
  let r = handle f act with
            | log m k ->
                println (\"outer \" ++ m)
                k ()
  println (show_int r)
";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("mask[Log] resume k"), "{shown}");
}

/// `k` を関数に渡すと、節の入口で `cont$` のクロージャに包み、渡した先の呼び出しは `apply` になる。
#[test]
fn a_continuation_passed_to_a_function_is_wrapped() {
    let shown = handled(
        "apply_one : (Int -> <e> Int) -> <e> Int\napply_one f = f 1\n\n",
        "    handle ask () with\n      | ask () k -> apply_one k",
    );
    assert!(shown.contains("closure cont$(k"), "{shown}");
    assert!(shown.contains("fn cont$(k0^, v1^) {"), "{shown}");
    assert!(shown.contains("tailcall resume k0(v1, ())"), "{shown}");
    assert!(!shown.contains("cont$state"), "{shown}");
}

/// 状態のある `k v` の部分適用は、`cont$state` のクロージャに包む。
#[test]
fn a_partial_continuation_is_wrapped_with_the_state_wrapper() {
    let shown = handled(
        "later : (Int -> <e> Int) -> Int -> <e> Int\nlater f s = f s\n\n",
        "    handle ask () from 0 with\n      | ask () k st -> later (k 1) st\n      | return x _ -> x",
    );
    assert!(shown.contains("closure cont$state(k"), "{shown}");
    assert!(shown.contains("fn cont$state(k0^, v1^, s2^) {"), "{shown}");
    assert!(shown.contains("tailcall resume k0(v1, s2)"), "{shown}");
}

/// `drop k` だけなら包まない。
#[test]
fn a_dropped_continuation_is_not_wrapped() {
    let shown = handled(
        "",
        "    handle ask () with\n      | ask () k ->\n          drop k\n          0",
    );
    assert!(!shown.contains("cont$"), "{shown}");
    assert!(!shown.contains("resume "), "{shown}");
}

/// handle の結果が関数のとき、`k v x` は `k v` の `resume` と、その結果への `apply` に分かれる。
#[test]
fn a_continuation_applied_to_extra_arguments_is_resumed_then_applied() {
    let text = format!(
        "{ASK}main : Unit -> <IO> Unit\nmain () =\n  let h = handle ask () with\n    | ask () k ->\n        let r = k 1 2\n        fn y -> r + y\n    | return x -> fn y -> x + y\n  println (show_int (h 5))"
    );
    let shown = core_text(&text, Pass::Translate);
    assert!(shown.contains("let t3^ = resume k1(1, ())"), "{shown}");
    assert!(shown.contains("let t4 = apply t3(2)"), "{shown}");
    assert!(!shown.contains("cont$"), "{shown}");
}

/// 値として包んだ `k` を、型検査が `mask` を記録した位置で呼ぶと、`apply` に `mask` が付く。
#[test]
fn a_wrapped_continuation_called_where_a_mask_is_recorded_carries_the_mask() {
    let text = "\
effect Ask where
  ask : Unit -> Int

effect Log where
  log : String -> Unit

f : (Unit -> <Ask | e> Int) -> <e> Int
f action =
  handle action () with
    | ask () k ->
        let g = k
        handle g 1 with
          | log m k2 ->
              k2 ()

act : Unit -> <Ask, Log> Int
act () =
  let n = ask ()
  log \"after\"
  n + 1

main : Unit -> <IO> Unit
main () =
  let r = handle f act with
            | log m k ->
                println (\"outer \" ++ m)
                k ()
  println (show_int r)
";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("mask[Log] apply"), "{shown}");
}

#[test]
fn externs_are_called_by_their_canonical_name() {
    // `==` と `!=` は、型検査が記録した型引数から比べ方の行を選ぶ。`(==)` は HIR がラムダに脱糖するので、包む関数を作らない
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\neq : Int -> Int -> Bool\neq = (==)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = (1 + 2, \"a\" == \"b\", True != False, eq 1 2)\n  println (apply show_int 3)";
    let shown = core_text(text, Pass::Translate);
    for expected in [
        "extern Prelude.+(1, 2)",
        "extern Prelude.string_eq(",
        "extern Prelude.bool_ne(#1, #0)",
        "extern Prelude.int_eq(",
        "call apply(&extern$Prelude.show_int, 3)",
        "fn extern$Prelude.show_int(p0) {\n  let t1^ = extern Prelude.show_int(p0)\n  return t1\n}",
    ] {
        assert!(shown.contains(expected), "{expected}\n{shown}");
    }
    assert!(!shown.contains("extern$Prelude.=="), "{shown}");
}

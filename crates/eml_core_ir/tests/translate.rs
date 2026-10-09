//! 型付き HIR からブロックの列への変換 (docs/spec/core-ir.md)。translate の直後の形を見る。

use crate::common::{core_text, core_text_files, core_text_with_positions, function};
use eml_core_ir::{Pass, Term, parse, pretty_with_positions};

#[test]
fn hello_world() {
    insta::assert_snapshot!(core_text("main : Unit -> <IO> Unit\nmain () = println \"hi\"", Pass::Translate), @r#"
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "hi"
      let t.2: unit = extern Prelude.println(s.1)
      return t.2
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    "#);
}

#[test]
fn a_value_if_with_one_exit_continues_in_the_same_block() {
    // 条件が定数なので続きへ向かうのは then の枝だけで、続きのブロックを作らずに同じブロックで続ける
    let text =
        "main : Unit -> <IO> Unit\nmain () =\n  let n = if True then 1 else 2\n  println (show n)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "main"), @"
    fn main(p.0: unit) -> unit {
      let t.1: obj = extern \"Prelude.Show Int.show\"(1)
      let t.2: unit = extern Prelude.println(t.1)
      return t.2
    }
    ");
}

#[test]
fn a_continuation_entered_by_two_jumps_is_a_merge_block() {
    let text = "pick : Bool -> Int\npick b =\n  let n = if b then 1 else 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "pick"), @"
    fn pick(b.0: enum) -> int {
      switch b.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(2)
    b2:
      jump b3(1)
    b3(t.1: int):
      let t.2: int = extern Prelude.+(t.1, 1)
      return t.2
    }
    ");
}

#[test]
fn and_branches_without_building_a_bool() {
    // `a && b` は HIR で `if a then b else False` になる。`False` の枝は値を作らずに else へ向かい、else には2本が
    // 入るので、`switch` の行き先は `jump` だけを持つ辺のブロックになる
    let text = "inside : Int -> Int -> String\ninside a b = if a < b && b < 10 then \"in\" else \"out\"\n\nmain : Unit -> <IO> Unit\nmain () = println (inside 1 2)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "inside"), @r#"
    fn inside(a.0: int, b.1: int) -> obj {
      let t.2: enum = extern "Prelude.Ord Int.<"(a.0, b.1)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b5()
    b2:
      let t.3: enum = extern "Prelude.Ord Int.<"(b.1, 10)
      switch t.3 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      jump b5()
    b4:
      let s.4: obj = const "in"
      return s.4
    b5:
      let s.5: obj = const "out"
      return s.5
    }
    "#);
}

#[test]
fn or_branches_without_building_a_bool() {
    let text = "outside : Int -> Int -> String\noutside a b = if a < 0 || b < 0 then \"out\" else \"in\"\n\nmain : Unit -> <IO> Unit\nmain () = println (outside 1 2)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "outside"), @r#"
    fn outside(a.0: int, b.1: int) -> obj {
      let t.2: enum = extern "Prelude.Ord Int.<"(a.0, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: enum = extern "Prelude.Ord Int.<"(b.1, 0)
      switch t.3 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b2:
      jump b5()
    b3:
      let s.5: obj = const "in"
      return s.5
    b4:
      jump b5()
    b5:
      let s.4: obj = const "out"
      return s.4
    }
    "#);
}

#[test]
fn a_nested_if_in_a_condition_meets_at_one_switch() {
    // 内側の2つの枝は値が分からないので、どちらも未知の値のラベルへ向かう。2本が入るのでラベルを引数のある
    // ブロックとして置き、そこで1回だけ `switch` する
    let text = "choose : Bool -> Bool -> Bool -> String\nchoose a b c = if (if a then b else c) then \"yes\" else \"no\"\n\nmain : Unit -> <IO> Unit\nmain () = println (choose True False True)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "choose"), @r#"
    fn choose(a.0: enum, b.1: enum, c.2: enum) -> obj {
      switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(c.2)
    b2:
      jump b3(b.1)
    b3(c.3: enum):
      switch c.3 Prelude.Bool { #0 -> b4, #1 -> b5 }
    b4:
      let s.5: obj = const "no"
      return s.5
    b5:
      let s.4: obj = const "yes"
      return s.4
    }
    "#);
}

#[test]
fn a_known_head_selects_its_arm_and_leaves_the_other_out() {
    // 定数の条件は `switch` を作らない。届かない枝は変換しないので、その文字列の定数も作らない
    let text = "main : Unit -> <IO> Unit\nmain () =\n  println (if False then \"never\" else \"else\")\n  println (if True && False then \"never\" else \"and\")";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "else"
      let t.2: unit = extern Prelude.println(s.1)
      let s.3: obj = const "and"
      let t.4: unit = extern Prelude.println(s.3)
      return t.4
    }
    "#);
}

#[test]
fn an_if_statement_continues_after_a_merge_of_unit() {
    let text = "step : Bool -> <IO> Unit\nstep b =\n  if b then println \"a\"\n  if b then println \"b\"\n  println \"c\"\n\nmain : Unit -> <IO> Unit\nmain () = step True";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "step"), @r#"
    fn step(b.0: enum) -> unit {
      switch b.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(())
    b2:
      let s.1: obj = const "a"
      let t.2: unit = extern Prelude.println(s.1)
      jump b3(t.2)
    b3(t.3: unit):
      switch b.0 Prelude.Bool { #0 -> b4, #1 -> b5 }
    b4:
      jump b6(())
    b5:
      let s.4: obj = const "b"
      let t.5: unit = extern Prelude.println(s.4)
      jump b6(t.5)
    b6(t.6: unit):
      let s.7: obj = const "c"
      let t.8: unit = extern Prelude.println(s.7)
      return t.8
    }
    "#);
}

#[test]
fn a_chain_of_returned_continuations_folds_in_one_pass() {
    // 内側の続き `z` は外側の続き `y` へ jump するだけで、外側の続きは `return y` だけである。番号の大きい外側を先に
    // たたむと、内側も `return` だけのブロックになり、同じループでたたまれる
    let text = "f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Bool -> Bool -> Int -> Int\nh a b x =\n  let y =\n    if a then\n      let z = if b then f x else g x\n      z\n    else f (x + 2)\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show (h True False 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "h"), @"
    fn h(a.0: enum, b.1: enum, x.2: int) -> int {
      switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.6: int = extern Prelude.+(x.2, 2)
      let t.7: int = call f(t.6)
      return t.7
    b2:
      switch b.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      let t.4: int = call g(x.2)
      return t.4
    b4:
      let t.3: int = call f(x.2)
      return t.3
    }
    ");
}

#[test]
fn every_extern_call_carries_the_position_of_its_callee() {
    // 演算子の呼び出しは演算子のトークンの位置を、名前で呼ぶ extern は名前の位置を持つ
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let n = 10 / 2\n  println (show n)";
    insta::assert_snapshot!(function(&core_text_with_positions(text), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.1: int = extern Prelude./(10, 2) @"test.em":3:14
      let t.2: obj = extern "Prelude.Show Int.show"(t.1) @"test.em":4:12
      let t.3: unit = extern Prelude.println(t.2) @"test.em":4:3
      return t.3
    }
    "#);
}

#[test]
fn each_reference_to_an_extern_as_a_value_gets_its_own_wrapper() {
    // 包む関数は参照の場所ごとに作り、その場所の位置を持つ。実行時エラーが参照した場所を指すようにするためである
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  apply println \"a\"\n  let say = println\n  say \"b\"";
    insta::assert_snapshot!(core_text_with_positions(text), @r#"
    fn "apply@[String, Unit]"(f.0: tobj, x.1: obj) -> unit {
      let t.2: unit = apply f.0(x.1)
      return t.2
    }
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "a"
      let t.2: unit = call "apply@[String, Unit]"(&main$extern0, s.1)
      let s.3: obj = const "b"
      let t.4: unit = apply &main$extern1(s.3)
      return t.4
    }
    internal fn main$extern0(p.0: obj) -> unit {
      let t.1: unit = extern Prelude.println(p.0) @"test.em":6:9
      return t.1
    }
    internal fn main$extern1(p.0: obj) -> unit {
      let t.1: unit = extern Prelude.println(p.0) @"test.em":7:13
      return t.1
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    "#);
}

#[test]
fn the_file_table_holds_only_the_paths_that_positions_use() {
    // `Prelude.not` は extern を呼ばないので、Prelude のパスは位置に使われず、表に入らない。位置付きの表示を読み直すと
    // `@"…"` のパスから表を作り直す。このプログラムではパスが1つなので同じ表に戻るが、表の順は一般には変わりうる。
    // 持ち上げた関数は外側の関数を変換する間にパスを入れ、表示では後ろに並ぶためである
    let text =
        "main : Unit -> <IO> Unit\nmain () = if not True then println \"a\" else println \"b\"";
    let program = eml_test_support::core_until(text, Pass::Translate);
    assert_eq!(program.files, ["test.em"]);
    let read = parse(&pretty_with_positions(&program)).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(read.files, program.files);
}

#[test]
fn lambdas_and_handlers_are_numbered_in_expression_order() {
    // HIR は子の式を親より先に作るので、内側のラムダと内側の handle の番号が小さい。変換は外側から持ち上げるが、
    // 番号は変換の順に依らない
    let text = "effect A where\n  ask : Unit -> Int\n\neffect B where\n  tell : Int -> Unit\n\nmain : Unit -> <IO> Unit\nmain () =\n  let add = fn x -> fn y -> x + y\n  let n =\n    handle (handle ask () + add 1 2 with | ask () k -> k 1) with\n      | tell m k -> k ()\n  println (show n)";
    let shown = core_text(text, Pass::Translate);
    let names: Vec<&str> = shown
        .lines()
        .filter_map(|line| {
            line.strip_prefix("internal ")
                .unwrap_or(line)
                .strip_prefix("fn ")
        })
        .map(|line| &line[..line.find('(').expect("a parameter list")])
        .collect();
    insta::assert_snapshot!(names.join("\n"), @"
    main
    main$lambda1
    main$lambda0
    main$handle1
    main$handle0
    main$handle0$ask
    main$handle0$return
    main$handle1$tell
    main$handle1$return
    entry$main
    ");
    insta::assert_snapshot!(function(&shown, "main$lambda1"), @"
    internal fn main$lambda1(x.0: int) -> tobj {
      let c.1: tobj = closure main$lambda0(x.0)
      return c.1
    }
    ");
}

#[test]
fn reprs_follow_the_types() {
    // Int は int、Bool は enum、String とタプルは obj、引数のあるコンストラクタとないコンストラクタが混ざる data と関数は
    // tobj になる (docs/spec/core-ir.md)
    let text = "data Shape =\n  | Dot\n  | Circle Int\n\nmake : Int -> Bool -> String -> Shape\nmake n b s =\n  let pair = (n, s)\n  let f = fn x -> x + n\n  let shape = Circle (f 1)\n  drop pair\n  shape\n\nmain : Unit -> <IO> Unit\nmain () =\n  let shape = make 1 True \"s\"\n  drop shape\n  println \"done\"";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "make"), @"
    fn make(n.0: int, b.1: enum, s.2: obj) -> tobj {
      let d.3: obj = con (,) #0(n.0, s.2)
      let c.4: tobj = closure make$lambda0(n.0)
      let t.5: int = apply c.4(1)
      let d.6: tobj = con Shape #1(t.5)
      let t.7: unit = drop d.3
      return d.6
    }
    ");
}

#[test]
fn the_entry_applies_a_point_free_main_to_unit() {
    let text = "main : Unit -> <IO> Unit\nmain = fn () -> println \"point-free\"";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn main() -> tobj {
      return &main$lambda0
    }
    internal fn main$lambda0(p.0: unit) -> unit {
      let s.1: obj = const "point-free"
      let t.2: unit = extern Prelude.println(s.1)
      return t.2
    }
    fn entry$main() -> unit {
      let f.0: tobj = call main()
      let t.1: unit = apply f.0(())
      return t.1
    }
    "#);
}

#[test]
fn handlers_are_lifted_with_their_return_reprs() {
    let text = "effect Ask where\n  ask : String -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let prefix = \"n = \"\n  let n =\n    handle ask \"x\" with\n      | ask key k -> k 1\n      | return x -> x + 1\n  println (prefix ++ show n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    effect Ask { ask/1 }
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "n = "
      let t.2: int = handle Ask((), &main$handle0) { ask: &main$handle0$ask } return &main$handle0$return
      let t.3: obj = extern "Prelude.Show Int.show"(t.2)
      let t.4: obj = extern Prelude.++(s.1, t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    internal fn main$handle0(p.0: unit) -> int {
      let s.1: obj = const "x"
      let t.2: int = perform Ask.ask(s.1)
      return t.2
    }
    internal fn main$handle0$ask(key.0: obj, k.1: tobj, p.2: unit) -> int {
      let t.3: int = resume k.1(1, ())
      return t.3
    }
    internal fn main$handle0$return(x.0: int, p.1: unit) -> int {
      let t.2: int = extern Prelude.+(x.0, 1)
      return t.2
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    "#);
}

#[test]
fn a_continuation_passed_to_a_function_is_wrapped() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : (Int -> <e> Int) -> <e> Int\ntwice f = f 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle ask () with\n      | ask () k -> twice k\n  println (show n)";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "main$handle0$ask"), @"
    internal fn main$handle0$ask(p.0: unit, k.1: tobj, p.2: unit) -> int {
      let c.3: tobj = closure cont$(k.1)
      let t.4: int = call twice(c.3)
      return t.4
    }
    ");
    insta::assert_snapshot!(function(&shown, "cont$"), @"
    internal fn cont$(k.0: tobj, v.1: tobj) -> tobj {
      let t.2: tobj = resume k.0(v.1, ())
      return t.2
    }
    ");
}

#[test]
fn lower_until_stops_after_the_named_pass() {
    // contract は使われない純粋な `let` を消し、Perceus は RC の命令と `save` を足す
    let text = "f : Int -> Int\nf x = x + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let unused = \"unused\"\n  let s = \"used\"\n  let n = f 1\n  println s\n  println (s ++ show n)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "unused"
      let s.2: obj = const "used"
      let t.3: int = call f(1)
      let t.4: unit = extern Prelude.println(s.2)
      let t.5: obj = extern "Prelude.Show Int.show"(t.3)
      let t.6: obj = extern Prelude.++(s.2, t.5)
      let t.7: unit = extern Prelude.println(t.6)
      return t.7
    }
    "#);
    insta::assert_snapshot!(function(&core_text(text, Pass::Contract), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let s.2: obj = const "used"
      let t.3: int = call f(1)
      let t.4: unit = extern Prelude.println(s.2)
      let t.5: obj = extern "Prelude.Show Int.show"(t.3)
      let t.6: obj = extern Prelude.++(s.2, t.5)
      let t.7: unit = extern Prelude.println(t.6)
      return t.7
    }
    "#);
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let s.2: obj = const "used"
      let t.3: int = call f(1) save [s.2]
      dup s.2
      let t.4: unit = extern Prelude.println(s.2)
      let t.5: obj = extern "Prelude.Show Int.show"(t.3)
      let t.6: obj = extern Prelude.++(s.2, t.5)
      let t.7: unit = extern Prelude.println(t.6)
      return t.7
    }
    "#);
}

#[test]
fn a_long_run_of_if_statements_is_translated_without_deep_recursion() {
    // 文の `if` は続きのブロックで次の文を変換するので、文の数だけ Rust のスタックを使わない。どの `if` も条件が
    // 引数なので、`switch` が文の数だけ出る (docs/spec/core-ir.md)
    const COUNT: usize = 10_000;
    let mut text = String::from("step : Bool -> <IO> Unit\nstep b =\n");
    for _ in 0..COUNT {
        text.push_str("  if b then println \"x\"\n");
    }
    text.push_str("  ()\n\nmain : Unit -> <IO> Unit\nmain () = step True");
    let program = eml_test_support::core_until(&text, Pass::Perceus);
    let step = program
        .functions
        .iter()
        .find(|function| function.name == "step")
        .expect("step");
    let switches = step
        .blocks
        .iter()
        .filter(|block| matches!(block.term, Term::Switch { .. }))
        .count();
    assert_eq!(switches, COUNT);
}

const OPTION: &str = "data Option a =\n  | None\n  | Some a\n\n";

#[test]
fn a_destructuring_let_of_an_if_builds_no_tuple() {
    // 両方の枝の値がタプルのリテラルなので、どちらも要素を続きのブロックの引数にして jump し、タプルを作らない
    let text = "order : Bool -> Int -> Int -> Int\norder c a b =\n  let (lo, hi) = if c then (a, b) else (b, a)\n  hi - lo\n\nmain : Unit -> <IO> Unit\nmain () = println (show (order True 1 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "order"), @"
    fn order(c.0: enum, a.1: int, b.2: int) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(b.2, a.1)
    b2:
      jump b3(a.1, b.2)
    b3(lo.3: int, hi.4: int):
      let t.5: int = extern Prelude.-(hi.4, lo.3)
      return t.5
    }
    ");
}

#[test]
fn a_value_matched_right_after_its_let_goes_straight_to_its_arm() {
    // `let found = S; match found` は `S` を `match` の文脈で変換する。どちらの枝も値が分かるので、`Some` も作らない
    let text = format!(
        "{OPTION}describe : Int -> String\ndescribe n =\n  let found = if n > 0 then Some n else None\n  match found with\n    | Some v -> show v\n    | None -> \"none\"\n\nmain : Unit -> <IO> Unit\nmain () = println (describe 1)"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "describe"), @r#"
    fn describe(n.0: int) -> obj {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.3: obj = const "none"
      return s.3
    b2:
      let t.2: obj = extern "Prelude.Show Int.show"(n.0)
      return t.2
    }
    "#);
}

#[test]
fn an_arm_that_uses_the_matched_variable_receives_it() {
    // 枝が `found` を使うので、枝のラベルは値全体も引数に取る。分かっている値は、その葉でだけ `con` を作って渡す
    let text = format!(
        "{OPTION}size : Option Int -> Int\nsize o = 1\n\nlookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int -> Int\npick c k =\n  let found = if c then Some 1 else lookup k\n  match found with\n    | None -> 0\n    | Some _ -> size found\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True 2))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "pick"), @"
    fn pick(c.0: enum, k.1: int) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: tobj = call lookup(k.1)
      switch t.3 Option { #0 -> b3, #1(x.4: int) -> b4 }
    b2:
      let d.2: tobj = con Option #1(1)
      jump b5(d.2)
    b3:
      return 0
    b4:
      jump b5(t.3)
    b5(found.5: tobj):
      let t.6: int = call size(found.5)
      return t.6
    }
    ");
}

#[test]
fn a_match_on_an_if_takes_each_known_arm() {
    let text = "data Result e a =\n  | Err e\n  | Ok a\n\ncheck : Int -> String\ncheck n = match (if n < 0 then Err \"negative\" else Ok n) with\n  | Err message -> message\n  | Ok v -> show v\n\nmain : Unit -> <IO> Unit\nmain () = println (check 1)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "check"), @r#"
    fn check(n.0: int) -> obj {
      let t.1: enum = extern "Prelude.Ord Int.<"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: obj = extern "Prelude.Show Int.show"(n.0)
      return t.3
    b2:
      let s.2: obj = const "negative"
      return s.2
    }
    "#);
}

#[test]
fn two_unknown_values_meet_at_one_switch() {
    // どちらの枝も呼び出しの結果を渡すので、値は分からない。2本が入る未知の値のラベルを置き、そこで1回だけ
    // `switch` する。ラベルの引数の名前は `let` の変数の名前である
    let text = format!(
        "{OPTION}lookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int\npick c =\n  let found = if c then lookup 1 else lookup 2\n  match found with\n    | Some v -> v\n    | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "pick"), @"
    fn pick(c.0: enum) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.2: tobj = call lookup(2)
      jump b3(t.2)
    b2:
      let t.1: tobj = call lookup(1)
      jump b3(t.1)
    b3(found.3: tobj):
      switch found.3 Option { #0 -> b4, #1(v.4: int) -> b5 }
    b4:
      return 0
    b5:
      return v.4
    }
    ");
}

#[test]
fn an_arm_that_binds_the_whole_value_builds_it_only_at_its_leaf() {
    // 分かっている `Some 1` は `x` の枝へ直接向かい、その葉でだけ `con` を作る。分からない値は `switch` の `default`
    // からその値のまま同じ枝へ向かうので、枝は2本が入るブロックになる
    let text = format!(
        "{OPTION}size : Option Int -> Int\nsize o = 1\n\nlookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int -> Int\npick c k = match (if c then Some 1 else lookup k) with\n  | None -> 0\n  | x -> size x\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True 2))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "pick"), @"
    fn pick(c.0: enum, k.1: int) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: tobj = call lookup(k.1)
      switch t.3 Option { #0 -> b3, _ -> b4 }
    b2:
      let d.2: tobj = con Option #1(1)
      jump b5(d.2)
    b3:
      return 0
    b4:
      jump b5(t.3)
    b5(x.4: tobj):
      let t.5: int = call size(x.4)
      return t.5
    }
    ");
}

#[test]
fn nested_case_of_case_goes_straight_to_the_outer_arm() {
    // 内側の `match` の枝は外側の文脈に値を渡すので、`Bool` の値も `Color` の値も作らずに外側の枝へ向かう
    let text = "data Color =\n  | Red\n  | Green\n  | Blue\n\ncode : Bool -> Bool -> Int\ncode a b = match (match (if a then Red else if b then Green else Blue) with | Red -> True | _ -> False) with\n  | True -> 1\n  | False -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (code True False))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "code"), @"
    fn code(a.0: enum, b.1: enum) -> int {
      switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      switch b.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b2:
      return 1
    b3:
      jump b5()
    b4:
      jump b5()
    b5:
      return 0
    }
    ");
}

#[test]
fn a_match_in_a_condition_branches_without_a_bool() {
    let text = format!(
        "{OPTION}has : Option Int -> String\nhas o = if (match o with | Some _ -> True | None -> False) then \"yes\" else \"no\"\n\nmain : Unit -> <IO> Unit\nmain () = println (has None)"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "has"), @r#"
    fn has(o.0: tobj) -> obj {
      switch o.0 Option { #0 -> b1, #1(x.1: int) -> b2 }
    b1:
      let s.3: obj = const "no"
      return s.3
    b2:
      let s.2: obj = const "yes"
      return s.2
    }
    "#);
}

#[test]
fn a_constructor_bound_by_let_is_known_later() {
    // `o` は同じ関数で作った `Some x` なので、`match` は `switch` を出さずに枝を選び、フィールドを直接使う。使われなく
    // なった `con` は contract が消す
    let text = format!(
        "{OPTION}unwrap : Int -> <IO> Int\nunwrap x =\n  let o = Some x\n  println \"built\"\n  match o with\n    | Some y -> y + 1\n    | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (unwrap 1))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "unwrap"), @r#"
    fn unwrap(x.0: int) -> int {
      let d.1: tobj = con Option #1(x.0)
      let s.2: obj = const "built"
      let t.3: unit = extern Prelude.println(s.2)
      let t.4: int = extern Prelude.+(x.0, 1)
      return t.4
    }
    "#);
    insta::assert_snapshot!(function(&core_text(&text, Pass::Contract), "unwrap"), @r#"
    fn unwrap(x.0: int) -> int {
      let s.2: obj = const "built"
      let t.3: unit = extern Prelude.println(s.2)
      let t.4: int = extern Prelude.+(x.0, 1)
      return t.4
    }
    "#);
}

#[test]
fn a_tuple_literal_is_taken_apart_without_building_it() {
    let text = "shift : Int -> Int -> Int\nshift x y =\n  let (a, b) = (y, x + 1)\n  a - b\n\nmain : Unit -> <IO> Unit\nmain () = println (show (shift 1 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "shift"), @"
    fn shift(x.0: int, y.1: int) -> int {
      let t.2: int = extern Prelude.+(x.0, 1)
      let t.3: int = extern Prelude.-(y.1, t.2)
      return t.3
    }
    ");
}

#[test]
fn equations_take_their_parameters_as_columns() {
    // 複数の等式は引数のタプルへの `match` になる。タプルのリテラルの要素を決定木の列にするので、タプルを作らない。
    // 2つ目の等式には2つの葉が向かうので、引数のないブロックに置く
    let text = format!(
        "{OPTION}both : Option Int -> Option Int -> Int\nboth (Some a) (Some b) = a + b\nboth _ _ = 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (both None None))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "both"), @"
    fn both($0.0: tobj, $1.1: tobj) -> int {
      switch $0.0 Option { #1(a.2: int) -> b1, _ -> b2 }
    b1:
      switch $1.1 Option { #1(b.3: int) -> b3, _ -> b4 }
    b2:
      jump b5()
    b3:
      let t.4: int = extern Prelude.+(a.2, b.3)
      return t.4
    b4:
      jump b5()
    b5:
      return 0
    }
    ");
}

#[test]
fn a_row_that_binds_the_whole_tuple_builds_it_at_its_leaf() {
    let text = "first : (Int, Int) -> Int\nfirst p =\n  let (a, _) = p\n  a\n\nclassify : Int -> Int -> Int\nclassify a b = match (a, b) with\n  | (0, _) -> 0\n  | pair -> first pair\n\nmain : Unit -> <IO> Unit\nmain () = println (show (classify 1 2))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "classify"), @"
    fn classify(a.0: int, b.1: int) -> int {
      switch a.0 { 0 -> b1, _ -> b2 }
    b1:
      return 0
    b2:
      let d.2: obj = con (,) #0(a.0, b.1)
      let t.3: int = call first(d.2)
      return t.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "first"), @"
    fn first(p.0: obj) -> int {
      unpack p.0 (,) #0(a.1: int, x.2: int)
      return a.1
    }
    ");
}

#[test]
fn a_leaf_builds_a_known_value_once_however_often_it_passes_it() {
    // `q` は値全体を束縛し、`let p` と `match p` の融合で `p` も同じ値を枝へ渡す。葉は同じ `con` を1回だけ作る
    let text = "pair : Int -> (Int, Int) -> (Int, Int) -> Int\npair a b c = a\n\nf : Int -> Int\nf n =\n  let p = (n, n)\n  match p with\n    | q -> pair n q p\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f"), @"
    fn f(n.0: int) -> int {
      let d.1: obj = con (,) #0(n.0, n.0)
      let t.2: int = call pair(n.0, d.1, d.1)
      return t.2
    }
    ");
}

#[test]
fn a_leaf_builds_one_value_for_the_same_constructor_at_two_types() {
    // `x` と `y` は型引数だけが違う値 `P 1` を受ける。コンストラクタとフィールドのアトムが同じなので作る `con` の
    // 命令も同じになり、葉は `con` を1回だけ作る
    let text = "data P a =\n  | P Int\n\npair : P Int -> P String -> Int\npair x y = 0\n\nf : Int -> Int\nf n = match (P 1, P 1) with\n  | (x, y) -> pair x y\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f"), @"
    fn f(n.0: int) -> int {
      let d.1: obj = con P #0(1)
      let t.2: int = call pair(d.1, d.1)
      return t.2
    }
    ");
}

#[test]
fn literal_equations_switch_once_with_a_default() {
    let text = "name : Int -> String\nname 0 = \"zero\"\nname 1 = \"one\"\nname _ = \"many\"\n\nmain : Unit -> <IO> Unit\nmain () = println (name 1)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "name"), @r#"
    fn name($0.0: int) -> obj {
      switch $0.0 { 0 -> b1, 1 -> b2, _ -> b3 }
    b1:
      let s.1: obj = const "zero"
      return s.1
    b2:
      let s.2: obj = const "one"
      return s.2
    b3:
      let s.3: obj = const "many"
      return s.3
    }
    "#);
}

#[test]
fn an_arm_reached_from_two_leaves_is_placed_after_the_tree() {
    let text = "data Color =\n  | Red\n  | Green\n  | Blue\n\nsame : Color -> Color -> Int\nsame a b = match (a, b) with\n  | (Red, Red) -> 1\n  | (Green, Green) -> 2\n  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (same Red Blue))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "same"), @"
    fn same(a.0: enum, b.1: enum) -> int {
      switch a.0 Color { #0 -> b1, #1 -> b2, _ -> b3 }
    b1:
      switch b.1 Color { #0 -> b4, _ -> b5 }
    b2:
      switch b.1 Color { #1 -> b6, _ -> b7 }
    b3:
      jump b8()
    b4:
      return 1
    b5:
      jump b8()
    b6:
      return 2
    b7:
      jump b8()
    b8:
      return 0
    }
    ");
}

#[test]
fn a_lone_constructor_with_fields_is_unpacked_without_a_switch() {
    let text = "data Box a =\n  | Box a\n\nunbox : Box String -> String\nunbox (Box s) = s\n\nmain : Unit -> <IO> Unit\nmain () = println (unbox (Box \"b\"))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "unbox"), @"
    fn unbox(p.0: obj) -> obj {
      unpack p.0 Box #0(s.1: obj)
      return s.1
    }
    ");
}

#[test]
fn a_lone_constructor_without_fields_is_a_wildcard() {
    // `Token` は値が1つしかないので、引数、`let`、`match` の枝のどこでも、`unpack` も `switch` も出さない
    let text = "data Token =\n  | Token\n\nspend : Token -> Int -> Int\nspend Token n =\n  let Token = Token\n  match Token with\n    | Token -> n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (spend Token 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "spend"), @"
    fn spend(p.0: enum, n.1: int) -> int {
      let t.2: int = extern Prelude.+(n.1, 1)
      return t.2
    }
    ");
}

#[test]
fn a_thousand_shared_arms_are_translated_without_deep_recursion() {
    // `(_, i)` の枝には、`A` の行き先と `default` の2つの葉が向かう。枝は枝の順に1つずつ変換するので、枝の数だけ
    // Rust のスタックを使わない (docs/spec/core-ir.md)
    const COUNT: usize = 1000;
    let mut text = String::from(
        "data AB =\n  | A\n  | B\n\npick : AB -> Int -> Int\npick t n = match (t, n) with\n  | (A, 0) -> 0\n",
    );
    for i in 1..=COUNT {
        text.push_str(&format!("  | (_, {i}) -> {i}\n"));
    }
    text.push_str("  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick A 7))");
    let program = eml_test_support::core_until(&text, Pass::Perceus);
    let pick = program
        .functions
        .iter()
        .find(|function| function.name == "pick")
        .expect("pick");
    let jumps = pick
        .blocks
        .iter()
        .filter(|block| matches!(block.term, Term::Jump { .. }))
        .count();
    // `(_, i)` の枝と最後の枝に、それぞれ2本ずつ
    assert_eq!(jumps, 2 * (COUNT + 1));
}

#[test]
fn an_extern_function_with_an_effect_used_as_a_value_is_wrapped_with_its_extern_call() {
    // `println` は extern の関数なので、ほかの extern の関数と同じく包む関数の中で `Rhs::Extern` にする
    let text = "each : (String -> <IO> Unit) -> <IO> Unit\neach f = f \"x\"\n\nmain : Unit -> <IO> Unit\nmain () = each println";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "main$extern0"), @"
    internal fn main$extern0(p.0: obj) -> unit {
      let t.1: unit = extern Prelude.println(p.0)
      return t.1
    }
    ");
    assert!(!shown.contains("perform"), "{shown}");
}

/// extern のエフェクトはエフェクトの表に入らず、番号も持たない。`IO` は Prelude の最初のエフェクトなので、番号を表と同じく
/// 飛ばさないと、`mask`、`handle`、`perform` がユーザーのエフェクトを1つずれて指す。
#[test]
fn effect_numbers_skip_the_extern_effects() {
    let text = "effect A where\n  ask : Unit -> Int\n\neffect B where\n  tell : Int -> Unit\n\nneeds_a : (Unit -> <e> Int) -> <A | e> Int\nneeds_a f = ask () + f ()\n\ntold : Unit -> <B> Int\ntold () =\n  tell 1\n  2\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle (handle needs_a told with | ask () k -> k 1) with\n      | tell m k -> k ()\n  println (show n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect A { ask/1 }
    effect B { tell/1 }
    fn needs_a(f.0: tobj) -> int {
      let t.1: int = perform A.ask(())
      let t.2: int = mask [A] apply f.0(())
      let t.3: int = extern Prelude.+(t.1, t.2)
      return t.3
    }
    fn told(p.0: unit) -> int {
      let t.1: unit = perform B.tell(1)
      return 2
    }
    fn main(p.0: unit) -> unit {
      let t.1: int = handle B((), &main$handle1) { tell: &main$handle1$tell } return &main$handle1$return
      let t.2: obj = extern \"Prelude.Show Int.show\"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    internal fn main$handle1(p.0: unit) -> int {
      let t.1: int = handle A((), &main$handle0) { ask: &main$handle0$ask } return &main$handle0$return
      return t.1
    }
    internal fn main$handle0(p.0: unit) -> int {
      let t.1: int = call needs_a(&told)
      return t.1
    }
    internal fn main$handle0$ask(p.0: unit, k.1: tobj, p.2: unit) -> int {
      let t.3: int = resume k.1(1, ())
      return t.3
    }
    internal fn main$handle0$return($r.0: int, p.1: unit) -> int {
      return $r.0
    }
    internal fn main$handle1$tell(m.0: int, k.1: tobj, p.2: unit) -> int {
      let t.3: int = resume k.1((), ())
      return t.3
    }
    internal fn main$handle1$return($r.0: int, p.1: unit) -> int {
      return $r.0
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    ");
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
    let program = eml_core_ir::lower_until(
        &checked.program,
        &checked.typed,
        alt,
        checked.files(),
        Pass::Translate,
    );
    insta::assert_snapshot!(eml_core_ir::pretty(&program), @r#"
    fn alt(p.0: unit) -> unit {
      let s.1: obj = const "alt"
      let t.2: unit = extern Prelude.println(s.1)
      return t.2
    }
    fn entry$alt() -> unit {
      let t.0: unit = call alt(())
      return t.0
    }
    "#);
}

#[test]
fn names_outside_the_entry_are_qualified_with_their_module() {
    // テキストの形は関数とエフェクトを名前で引くので、入口以外のモジュールの名前には `モジュール名.` を付ける
    // (docs/spec/core-ir.md)。入れ子のモジュールのエフェクトの `perform` と `handle` も読み戻せることを確かめる
    let csv = "pub data Row = | Row Int\n\npub effect Parse where\n  next : Unit -> Int\n\npub parse : Unit -> <Parse> Row\nparse () =\n  let get = next\n  let make = Row\n  make (get ())";
    let main = "import Report.Csv\n\napply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle Csv.parse () with\n      | Csv.next () k -> k 1\n      | return r -> r\n  let Csv.Row n = r\n  apply println (show n)";
    insta::assert_snapshot!(core_text_files(main, &[("Report/Csv.em", csv)], Pass::Translate), @r#"
    layout Report.Csv.Row { Row(int) }
    effect Report.Csv.Parse { next/1 }
    fn "apply@[String, Unit]"(f.0: tobj, x.1: obj) -> unit {
      let t.2: unit = apply f.0(x.1)
      return t.2
    }
    fn main(p.0: unit) -> unit {
      let t.1: obj = handle Report.Csv.Parse((), &main$handle0) { next: &main$handle0$next } return &main$handle0$return
      unpack t.1 Report.Csv.Row #0(n.2: int)
      let t.3: obj = extern "Prelude.Show Int.show"(n.2)
      let t.4: unit = call "apply@[String, Unit]"(&main$extern0, t.3)
      return t.4
    }
    fn Report.Csv.parse(p.0: unit) -> obj {
      let t.1: int = apply &op$Report.Csv.next(())
      let t.2: obj = apply &con$Report.Csv.Row(t.1)
      return t.2
    }
    internal fn main$handle0(p.0: unit) -> obj {
      let t.1: obj = call Report.Csv.parse(())
      return t.1
    }
    internal fn main$handle0$next(p.0: unit, k.1: tobj, p.2: unit) -> obj {
      let t.3: obj = resume k.1(1, ())
      return t.3
    }
    internal fn main$handle0$return(r.0: obj, p.1: unit) -> obj {
      return r.0
    }
    internal fn main$extern0(p.0: obj) -> unit {
      let t.1: unit = extern Prelude.println(p.0)
      return t.1
    }
    internal fn op$Report.Csv.next(p.0: unit) -> int {
      let t.1: int = perform Report.Csv.Parse.next(p.0)
      return t.1
    }
    internal fn con$Report.Csv.Row(p.0: int) -> obj {
      let d.1: obj = con Report.Csv.Row #0(p.0)
      return d.1
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    "#);
}

#[test]
fn an_entry_operation_does_not_collide_with_a_prelude_operation() {
    // 入口の `println` という操作を包む関数は `op$println` で、Prelude の `println` を包む関数は参照する場所の名前を
    // 持つ `main$extern0` なので、名前が重ならない (docs/spec/core-ir.md)
    let text = "effect Log where\n  println : String -> Unit\n\neach : (String -> <e> Unit) -> <e> Unit\neach f = f \"x\"\n\nlogged : Unit -> <Log> Unit\nlogged () = each println\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle logged () with\n    | println s k -> k ()\n  each Prelude.println";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "op$println"), @"
    internal fn op$println(p.0: obj) -> unit {
      let t.1: unit = perform Log.println(p.0)
      return t.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "main$extern0"), @"
    internal fn main$extern0(p.0: obj) -> unit {
      let t.1: unit = extern Prelude.println(p.0)
      return t.1
    }
    ");
}

#[test]
fn only_functions_reachable_from_the_entry_are_lowered() {
    // 使わない Prelude の関数を Core IR に入れないため、入口から届く関数だけを変換する (docs/spec/core-ir.md)
    let text = "used : Int -> Int\nused x = x\n\nunused : Int -> Int\nunused x = x\n\nmain : Unit -> <IO> Unit\nmain () = println (show (used 1))";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn used("), "{shown}");
    assert!(shown.contains("fn main("), "{shown}");
    assert!(!shown.contains("fn unused("), "{shown}");
}

#[test]
fn recursion_and_top_level_values() {
    let text = "answer : Int\nanswer = 42\n\ncount : Int -> Int\ncount n = if n == 0 then answer else count (n - 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (count 3))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    layout Prelude.Bool { False, True }
    fn answer() -> int {
      return 42
    }
    fn count(n.0: int) -> int {
      let t.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: int = extern Prelude.-(n.0, 1)
      let t.4: int = call count(t.3)
      return t.4
    b2:
      let answer.2: int = call answer()
      return answer.2
    }
    fn main(p.0: unit) -> unit {
      let t.1: int = call count(3)
      let t.2: obj = extern \"Prelude.Show Int.show\"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    ");
}

#[test]
fn partial_and_extra_arguments_use_closures() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\nadder : Int -> Int -> Int\nadder x = add x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = add 1\n  let n = f 2 + adder 3 4\n  println (show n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    fn add(a.0: int, b.1: int) -> int {
      let t.2: int = extern Prelude.+(a.0, b.1)
      return t.2
    }
    fn adder(x.0: int) -> tobj {
      let c.1: tobj = closure add(x.0)
      return c.1
    }
    fn main(p.0: unit) -> unit {
      let c.1: tobj = closure add(1)
      let t.2: int = apply c.1(2)
      let t.3: tobj = call adder(3)
      let t.4: int = apply t.3(4)
      let t.5: int = extern Prelude.+(t.2, t.4)
      let t.6: obj = extern \"Prelude.Show Int.show\"(t.5)
      let t.7: unit = extern Prelude.println(t.6)
      return t.7
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    ");
}

#[test]
fn builtins_used_as_values_are_wrapped() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = not >> not\n  apply println (show 1)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    layout Prelude.Bool { False, True }
    fn Prelude.not($0.0: enum) -> enum {
      switch $0.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      return #1
    b2:
      return #0
    }
    fn "Prelude.>>@[Bool, Bool, Bool]"(f.0: tobj, g.1: tobj) -> tobj {
      let c.2: tobj = closure "Prelude.>>@[Bool, Bool, Bool]$lambda0"(f.0, g.1)
      return c.2
    }
    fn "apply@[String, Unit]"(f.0: tobj, x.1: obj) -> unit {
      let t.2: unit = apply f.0(x.1)
      return t.2
    }
    fn main(p.0: unit) -> unit {
      let t.1: tobj = call "Prelude.>>@[Bool, Bool, Bool]"(&Prelude.not, &Prelude.not)
      let t.2: obj = extern "Prelude.Show Int.show"(1)
      let t.3: unit = call "apply@[String, Unit]"(&main$extern0, t.2)
      return t.3
    }
    internal fn "Prelude.>>@[Bool, Bool, Bool]$lambda0"(f.0: tobj, g.1: tobj, x.2: enum) -> enum {
      let t.3: enum = apply f.0(x.2)
      let t.4: enum = apply g.1(t.3)
      return t.4
    }
    internal fn main$extern0(p.0: obj) -> unit {
      let t.1: unit = extern Prelude.println(p.0)
      return t.1
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    "#);
}

#[test]
fn lambdas_are_lifted_with_their_captures_first() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let s = \"!\"\n  let shout = fn t -> t ++ s\n  println (apply shout \"hi\")\n  println s";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn "apply@[String, String]"(f.0: tobj, x.1: obj) -> obj {
      let t.2: obj = apply f.0(x.1)
      return t.2
    }
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "!"
      let c.2: tobj = closure main$lambda0(s.1)
      let s.3: obj = const "hi"
      let t.4: obj = call "apply@[String, String]"(c.2, s.3)
      let t.5: unit = extern Prelude.println(t.4)
      let t.6: unit = extern Prelude.println(s.1)
      return t.6
    }
    internal fn main$lambda0(s.0: obj, t.1: obj) -> obj {
      let t.2: obj = extern Prelude.++(t.1, s.0)
      return t.2
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    "#);
}

#[test]
fn a_zero_arity_callee_is_evaluated_before_its_arguments() {
    let text = "k : Int -> Int -> Int\nk a b = a\n\nfive : Int -> Int\nfive = k 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show (five (1 + 2)))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    fn k(a.0: int, b.1: int) -> int {
      return a.0
    }
    fn five() -> tobj {
      let c.0: tobj = closure k(5)
      return c.0
    }
    fn main(p.0: unit) -> unit {
      let five.1: tobj = call five()
      let t.2: int = extern Prelude.+(1, 2)
      let t.3: int = apply five.1(t.2)
      let t.4: obj = extern \"Prelude.Show Int.show\"(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    ");
}

#[test]
fn a_tail_if_returns_from_each_arm() {
    let text = "sign : Int -> String\nsign n = if n < 0 then \"negative\" else \"non-negative\"\n\nmain : Unit -> <IO> Unit\nmain () = println (sign 1)";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "sign"), @r#"
    fn sign(n.0: int) -> obj {
      let t.1: enum = extern "Prelude.Ord Int.<"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.3: obj = const "non-negative"
      return s.3
    b2:
      let s.2: obj = const "negative"
      return s.2
    }
    "#);
}

#[test]
fn nested_value_ifs_meet_in_their_own_merge_blocks() {
    // 内側の `if` の続きのブロックは、外側の `if` の続きのブロックへ jump する。どちらの続きも、入口で定義した `s` を
    // 引数で受け取らずにそのまま使う。入口がすべての行き先を支配するためである
    let text = "label : Bool -> Bool -> String -> String\nlabel a b s =\n  let t =\n    if a then\n      let u = if b then s ++ \"!\" else \"plain\"\n      u ++ \"?\"\n    else s\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (label True False \"x\")";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "label"), @r#"
    fn label(a.0: enum, b.1: enum, s.2: obj) -> obj {
      switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b6(s.2)
    b2:
      switch b.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      let s.5: obj = const "plain"
      jump b5(s.5)
    b4:
      let s.3: obj = const "!"
      let t.4: obj = extern Prelude.++(s.2, s.3)
      jump b5(t.4)
    b5(t.6: obj):
      let s.7: obj = const "?"
      let t.8: obj = extern Prelude.++(t.6, s.7)
      jump b6(t.8)
    b6(t.9: obj):
      let t.10: obj = extern Prelude.++(t.9, s.2)
      return t.10
    }
    "#);
}

#[test]
fn handlers_are_lifted_to_closures() {
    let text = "effect Ask where\n  ask : String -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let prefix = \"n = \"\n  let n =\n    handle ask \"x\" with\n      | ask key k -> k 1\n      | return x -> x + 1\n  println (prefix ++ show n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    effect Ask { ask/1 }
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "n = "
      let t.2: int = handle Ask((), &main$handle0) { ask: &main$handle0$ask } return &main$handle0$return
      let t.3: obj = extern "Prelude.Show Int.show"(t.2)
      let t.4: obj = extern Prelude.++(s.1, t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    internal fn main$handle0(p.0: unit) -> int {
      let s.1: obj = const "x"
      let t.2: int = perform Ask.ask(s.1)
      return t.2
    }
    internal fn main$handle0$ask(key.0: obj, k.1: tobj, p.2: unit) -> int {
      let t.3: int = resume k.1(1, ())
      return t.3
    }
    internal fn main$handle0$return(x.0: int, p.1: unit) -> int {
      let t.2: int = extern Prelude.+(x.0, 1)
      return t.2
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    "#);
}

#[test]
fn operations_as_values_and_drop() {
    let text = "effect Log where\n  log : String -> String -> Unit\n\nrun : Unit -> <Log> Unit\nrun () =\n  let info = log \"info\"\n  info \"a\"\n\ndiscard : String -> Unit\ndiscard s = drop s\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle run () with\n    | log _ _ k -> k ()\n  discard \"x\"";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "run"), @r#"
    fn run(p.0: unit) -> unit {
      let s.1: obj = const "info"
      let c.2: tobj = closure op$log(s.1)
      let s.3: obj = const "a"
      let t.4: unit = apply c.2(s.3)
      return t.4
    }
    "#);
    insta::assert_snapshot!(function(&shown, "op$log"), @"
    internal fn op$log(p.0: obj, p.1: obj) -> unit {
      let t.2: unit = perform Log.log(p.0, p.1)
      return t.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "discard"), @"
    fn discard(s.0: obj) -> unit {
      let t.1: unit = drop s.0
      return t.1
    }
    ");
}

#[test]
fn constructors_with_fields_build_values() {
    let text = "data Option a =\n  | None\n  | Some a\n\nwrap : Int -> Option Int\nwrap n = Some n\n\nnest : Unit -> Option (Option Int)\nnest () = Some (Some 1)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = wrap 1\n  let _ = nest ()\n  ()";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "wrap"), @"
    fn wrap(n.0: int) -> tobj {
      let d.1: tobj = con Option #1(n.0)
      return d.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "nest"), @"
    fn nest(p.0: unit) -> tobj {
      let d.1: tobj = con Option #1(1)
      let d.2: tobj = con Option #1(d.1)
      return d.2
    }
    ");
}

#[test]
fn a_constructor_used_as_a_function_value_is_wrapped() {
    let text = "data Pair a b =\n  | Pair a b\n\npairs : Int -> Pair Int Int\npairs n =\n  let make = Pair n\n  make 2\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = pairs 1\n  ()";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "pairs"), @"
    fn pairs(n.0: int) -> obj {
      let c.1: tobj = closure con$Pair(n.0)
      let t.2: obj = apply c.1(2)
      return t.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "con$Pair"), @"
    internal fn con$Pair(p.0: tobj, p.1: tobj) -> obj {
      let d.2: obj = con Pair #0(p.0, p.1)
      return d.2
    }
    ");
}

#[test]
fn a_match_compiles_to_a_decision_tree() {
    // 選んだ欄に現れないコンストラクタ (`None`) は `default` に進む。2つの葉から入る `_` の枝は、木の後のブロックになる
    let text = "data Option a = | None | Some a\n\nf : Option (Option Int) -> Int\nf o = match o with\n  | Some (Some n) -> n\n  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f None))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "f"), @"
    fn f(o.0: tobj) -> int {
      switch o.0 Option { #1(x.1: tobj) -> b1, _ -> b2 }
    b1:
      switch x.1 Option { #1(n.2: int) -> b3, _ -> b4 }
    b2:
      jump b5()
    b3:
      return n.2
    b4:
      jump b5()
    b5:
      return 0
    }
    ");
}

#[test]
fn tail_and_non_tail_matches() {
    // 末尾にない `match` は、`if` と同じく枝から続きのブロックへ値を渡して jump する
    let text = "data Option a = | None | Some a\n\ng : Option Int -> Int\ng o =\n  let n = match o with\n    | Some m -> m\n    | None -> 0\n  n + 1\n\nh : Option Int -> Int\nh o = match o with | Some m -> m | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (g None + h None))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "g"), @"
    fn g(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(m.1: int) -> b2 }
    b1:
      jump b3(0)
    b2:
      jump b3(m.1)
    b3(t.2: int):
      let t.3: int = extern Prelude.+(t.2, 1)
      return t.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "h"), @"
    fn h(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(m.1: int) -> b2 }
    b1:
      return 0
    b2:
      return m.1
    }
    ");
}

#[test]
fn constructor_patterns_in_let_lambda_and_equation_parameters() {
    // コンストラクタが1つの型のパターンは、`switch` を出さずに `unpack` で分解し、続きを同じブロックで変換する
    let text = "data Box a = | Box a\n\nby_equation : Box Int -> Int\nby_equation (Box n) = n\n\nby_let : Box Int -> Int\nby_let b =\n  let Box m = b\n  m + 1\n\nby_lambda : Box Int -> Int\nby_lambda b = (fn (Box k) -> k) b\n\nmain : Unit -> <IO> Unit\nmain () = println (show (by_equation (Box 1) + by_let (Box 2) + by_lambda (Box 3)))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "by_equation"), @"
    fn by_equation(p.0: obj) -> int {
      unpack p.0 Box #0(n.1: int)
      return n.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "by_let"), @"
    fn by_let(b.0: obj) -> int {
      unpack b.0 Box #0(m.1: int)
      let t.2: int = extern Prelude.+(m.1, 1)
      return t.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "by_lambda"), @"
    fn by_lambda(b.0: obj) -> int {
      let t.1: int = apply &by_lambda$lambda0(b.0)
      return t.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "by_lambda$lambda0"), @"
    internal fn by_lambda$lambda0(p.0: obj) -> int {
      unpack p.0 Box #0(k.1: int)
      return k.1
    }
    ");
}

#[test]
fn a_variable_pattern_after_a_switch_binds_the_scrutinee() {
    // `ys` は `xs` の `switch` の後で、`xs` そのものを受ける。`ys` の枝には、2つの葉が `xs` を引数で渡す
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = println (show (describe Nil))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "describe"), @"
    fn describe(xs.0: tobj) -> int {
      switch xs.0 List { #1(x.1: int, x.2: tobj) -> b1, _ -> b2 }
    b1:
      switch x.2 List { #0 -> b3, _ -> b4 }
    b2:
      jump b5(xs.0)
    b3:
      return 1
    b4:
      jump b5(xs.0)
    b5(ys.3: tobj):
      let t.4: int = call size(ys.3)
      return t.4
    }
    ");
}

#[test]
fn constructor_patterns_in_handler_clause_parameters() {
    // 操作の節と `return` の節はラムダと同じく関数に持ち上げるので、引数のコンストラクタのパターンも同じ経路で分解する
    let text = "data Box a = | Box a\n\neffect Give where\n  give : Box Int -> Int\n\nrun : Unit -> Int\nrun () =\n  handle Box (give (Box 1)) with\n    | give (Box n) k -> k n\n    | return (Box r) -> r\n\nmain : Unit -> <IO> Unit\nmain () = println (show (run ()))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "run$handle0$give"), @"
    internal fn run$handle0$give(p.0: obj, k.1: tobj, p.2: unit) -> int {
      unpack p.0 Box #0(n.3: int)
      let t.4: int = resume k.1(n.3, ())
      return t.4
    }
    ");
    insta::assert_snapshot!(function(&shown, "run$handle0$return"), @"
    internal fn run$handle0$return(p.0: obj, p.1: unit) -> int {
      unpack p.0 Box #0(r.2: int)
      return r.2
    }
    ");
}

#[test]
fn tuples_are_built_and_taken_apart_by_parameters_and_let() {
    // タプルの値はタグ 0 のコンストラクタの値で、タプルのパターンは `unpack` で分解する
    let text = "swap : (Int, String) -> (String, Int)\nswap (n, s) = (s, n)\n\nfirst : (Int, Int) -> Int\nfirst p =\n  let (a, _) = p\n  a\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = swap (1, \"a\")\n  println (show (first (2, 3)))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "swap"), @"
    fn swap(p.0: obj) -> obj {
      unpack p.0 (,) #0(n.1: int, s.2: obj)
      let d.3: obj = con (,) #0(s.2, n.1)
      return d.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "first"), @"
    fn first(p.0: obj) -> int {
      unpack p.0 (,) #0(a.1: int, x.2: int)
      return a.1
    }
    ");
}

#[test]
fn a_tuple_column_is_a_single_constructor() {
    // タプルの欄を `unpack` で分解してから、その中の `Option` の欄で `switch` する
    let text = "data Option a = | None | Some a\n\npick : (Option Int, Int) -> Int\npick p = match p with\n  | (Some n, _) -> n\n  | (None, k) -> k\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick (None, 1)))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "pick"), @"
    fn pick(p.0: obj) -> int {
      unpack p.0 (,) #0(x.1: tobj, k.2: int)
      switch x.1 Option { #0 -> b1, #1(n.3: int) -> b2 }
    b1:
      return k.2
    b2:
      return n.3
    }
    ");
}

#[test]
fn int_literals_compare_in_order_and_fall_back_to_the_rest() {
    // 異なるリテラルを上の行から順に比べ、最後の等しくない枝は残りの行列 (`_` の行) に進む
    let text = "describe : Int -> String\ndescribe n = match n with\n  | 0 -> \"zero\"\n  | 1 -> \"one\"\n  | -1 -> \"minus one\"\n  | _ -> \"many\"\n\nmain : Unit -> <IO> Unit\nmain () = println (describe 2)";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "describe"), @r#"
    fn describe(n.0: int) -> obj {
      switch n.0 { 0 -> b1, 1 -> b2, -1 -> b3, _ -> b4 }
    b1:
      let s.1: obj = const "zero"
      return s.1
    b2:
      let s.2: obj = const "one"
      return s.2
    b3:
      let s.3: obj = const "minus one"
      return s.3
    b4:
      let s.4: obj = const "many"
      return s.4
    }
    "#);
}

#[test]
fn string_literals_are_cases_of_one_switch() {
    // `String` のリテラルは、文字列定数の表を指す case として1つの `switch` に並べる。変数の枝は、比べた出現そのものを受ける
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = println (greet \"en\")";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "greet"), @r#"
    fn greet(name.0: obj) -> obj {
      switch name.0 { "en" -> b1, "ja" -> b2, _ -> b3 }
    b1:
      let s.1: obj = const "hello"
      return s.1
    b2:
      let s.2: obj = const "konnichiwa"
      return s.2
    b3:
      return name.0
    }
    "#);
}

#[test]
fn a_literal_column_inside_a_tuple() {
    // タプルを分解した後、最初の行でいちばん左の調べる欄 (リテラル) を選ぶ。`(_, False)` の枝には、リテラルが等しい
    // 枝と等しくない枝の2つの葉から入るので、木の後のブロックになる
    let text = "classify : (Int, Bool) -> Int\nclassify p = match p with\n  | (0, True) -> 1\n  | (_, False) -> 2\n  | (n, _) -> n\n\nmain : Unit -> <IO> Unit\nmain () = println (show (classify (0, True)))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "classify"), @"
    fn classify(p.0: obj) -> int {
      unpack p.0 (,) #0(n.1: int, x.2: enum)
      switch n.1 { 0 -> b1, _ -> b2 }
    b1:
      switch x.2 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b2:
      switch x.2 Prelude.Bool { #0 -> b5, _ -> b6 }
    b3:
      jump b7()
    b4:
      return 1
    b5:
      jump b7()
    b6:
      return n.1
    b7:
      return 2
    }
    ");
}

#[test]
fn file_operations_are_extern_calls() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"a.txt\"\n  let (f, s) = Fs.read_all f\n  Fs.close f\n  println s";
    let ir = core_text(text, Pass::Translate);
    for op in [
        "extern Std.Fs.open(",
        "extern Std.Fs.read_all(",
        "extern Std.Fs.close(",
    ] {
        assert!(ir.contains(op), "{ir}");
    }
}

#[test]
fn functions_without_captures_are_values() {
    let text = "twice : (Int -> Int) -> Int -> Int\ntwice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = 3\n  println (show (twice (fn x -> x + 1) 1 + twice (fn x -> x + n) 1))";
    let ir = core_text(text, Pass::Translate);
    // 捕獲のないラムダは関数の値で、捕獲のあるラムダはクロージャである
    assert!(ir.contains("&main$lambda0"), "{ir}");
    assert!(!ir.contains("closure main$lambda0"), "{ir}");
    assert!(ir.contains("closure main$lambda1(3)"), "{ir}");
}

#[test]
fn a_handler_with_a_state_passes_its_initial_value_and_takes_the_state_from_its_clauses() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle ask () + ask () from 10 with\n      | ask () k st -> k st (st + 1)\n      | return x st -> x * st\n  println (show n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect Ask { ask/1 }
    fn main(p.0: unit) -> unit {
      let t.1: int = handle Ask(10, &main$handle0) { ask: &main$handle0$ask } return &main$handle0$return
      let t.2: obj = extern \"Prelude.Show Int.show\"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    internal fn main$handle0(p.0: unit) -> int {
      let t.1: int = perform Ask.ask(())
      let t.2: int = perform Ask.ask(())
      let t.3: int = extern Prelude.+(t.1, t.2)
      return t.3
    }
    internal fn main$handle0$ask(p.0: unit, k.1: tobj, st.2: int) -> int {
      let t.3: int = extern Prelude.+(st.2, 1)
      let t.4: int = resume k.1(st.2, t.3)
      return t.4
    }
    internal fn main$handle0$return(x.0: int, st.1: int) -> int {
      let t.2: int = extern Prelude.*(x.0, st.1)
      return t.2
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    ");
}

#[test]
fn effects_are_numbered_in_declaration_order() {
    let text = "effect A where\n  a : Unit -> Int\neffect B where\n  b : Unit -> Int\nmain : Unit -> <IO> Unit\nmain () =\n  let x = handle a () with\n    | a () k -> k 1\n  let y = handle b () with\n    | b () k -> k 2\n  println (show (x + y))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    effect A { a/1 }
    effect B { b/1 }
    fn main(p.0: unit) -> unit {
      let t.1: int = handle A((), &main$handle0) { a: &main$handle0$a } return &main$handle0$return
      let t.2: int = handle B((), &main$handle1) { b: &main$handle1$b } return &main$handle1$return
      let t.3: int = extern Prelude.+(t.1, t.2)
      let t.4: obj = extern \"Prelude.Show Int.show\"(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    internal fn main$handle0(p.0: unit) -> int {
      let t.1: int = perform A.a(())
      return t.1
    }
    internal fn main$handle0$a(p.0: unit, k.1: tobj, p.2: unit) -> int {
      let t.3: int = resume k.1(1, ())
      return t.3
    }
    internal fn main$handle0$return($r.0: int, p.1: unit) -> int {
      return $r.0
    }
    internal fn main$handle1(p.0: unit) -> int {
      let t.1: int = perform B.b(())
      return t.1
    }
    internal fn main$handle1$b(p.0: unit, k.1: tobj, p.2: unit) -> int {
      let t.3: int = resume k.1(2, ())
      return t.3
    }
    internal fn main$handle1$return($r.0: int, p.1: unit) -> int {
      return $r.0
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
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
    // 1つの葉からだけ届く枝の本体は、その葉のブロックで変換し、複数の葉から届く枝だけを木の後のブロックにする
    // (docs/spec/core-ir.md)。`pick` の `(n, _)` は、`0` の case の `default` と、外側の `default` の2つの葉から届く
    let text = "data Shape = | Dot | Box Int Int\n\narea : Shape -> Int\narea s =\n  match s with\n    | Box w h -> w * h\n    | Dot -> 0\n\npick : Int -> Int -> Int\npick a b =\n  match (a, b) with\n    | (0, 1) -> 0\n    | (n, _) -> n * 2\n\nmain : Unit -> <IO> Unit\nmain () = println (show (area (Box 2 3) + pick 0 1))";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @"
    layout Shape { Dot, Box(int, int) }
    fn area(s.0: tobj) -> int {
      switch s.0 Shape { #0 -> b1, #1(w.1: int, h.2: int) -> b2 }
    b1:
      return 0
    b2:
      let t.3: int = extern Prelude.*(w.1, h.2)
      return t.3
    }
    fn pick(a.0: int, b.1: int) -> int {
      switch a.0 { 0 -> b1, _ -> b2 }
    b1:
      switch b.1 { 1 -> b3, _ -> b4 }
    b2:
      jump b5(a.0)
    b3:
      return 0
    b4:
      jump b5(a.0)
    b5(n.2: int):
      let t.3: int = extern Prelude.*(n.2, 2)
      return t.3
    }
    fn main(p.0: unit) -> unit {
      let d.1: tobj = con Shape #1(2, 3)
      let t.2: int = call area(d.1)
      let t.3: int = call pick(0, 1)
      let t.4: int = extern Prelude.+(t.2, t.3)
      let t.5: obj = extern \"Prelude.Show Int.show\"(t.4)
      let t.6: unit = extern Prelude.println(t.5)
      return t.6
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
    }
    ");
}

#[test]
fn effects_of_the_same_name_in_two_modules_stay_apart() {
    // エフェクトの表は名前で引くので、入口の `Log` と `Audit.Log` は別の名前になる (docs/spec/core-ir.md)
    let audit = "pub effect Log where\n  emit : Int -> Unit\n\npub audited : Unit -> <Log> Unit\naudited () = emit 1";
    let main = "import Audit\n\neffect Log where\n  emit : Int -> Unit\n\nlocal : Unit -> <Log> Unit\nlocal () = emit 2\n\nmain : Unit -> <IO> Unit\nmain () =\n  handle Audit.audited () with\n    | Audit.emit n k -> k (println (show n))\n  handle local () with\n    | emit n k -> k (println (show n))";
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
    layout (,,,,,) { (,,,,,)(tobj, tobj, tobj, tobj, tobj, tobj) }
    fn compare(n.0: int, s.1: obj, b.2: enum) -> obj {
      let t.3: enum = extern "Prelude.Eq Int.=="(n.0, 1)
      let t.4: enum = extern "Prelude.Eq Int.!="(n.0, 2)
      let s.5: obj = const "a"
      let t.6: enum = extern "Prelude.Eq String.=="(s.1, s.5)
      let s.7: obj = const "b"
      let t.8: enum = extern "Prelude.Eq String.!="(s.1, s.7)
      let t.9: enum = extern "Prelude.Eq Bool.=="(b.2, #1)
      let t.10: enum = extern "Prelude.Eq Bool.!="(b.2, #0)
      let d.11: obj = con (,,,,,) #0(t.3, t.4, t.6, t.8, t.9, t.10)
      return d.11
    }
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "a"
      let t.2: obj = call compare(1, s.1, #1)
      return ()
    }
    fn entry$main() -> unit {
      let t.0: unit = call main(())
      return t.0
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
        "main : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle {body} from 0 with\n      | get () k st -> k st st\n      | put s k _ -> k () s\n      | return x _ -> x\n  println (show n)\n"
    )
}

/// 型検査が記録した `mask` を、その呼び出しに付ける (docs/spec/core-ir.md)。
#[test]
fn a_masked_callback_call() {
    let text = format!(
        "{STATE}run : (Unit -> <e> a) -> <State Int | e> a\nrun cb =\n  let n = get ()\n  cb ()\n\n{}",
        state_main("run (fn () -> 1)")
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run@[Int]"), @r#"
    fn "run@[Int]"(cb.0: tobj) -> int {
      let t.1: int = perform State.get(())
      let t.2: int = mask [State] apply cb.0(())
      return t.2
    }
    "#);
}

/// 引数がそろう既知の関数の呼び出しは、最後の矢印の `mask` を使う。前の矢印は部分適用で、エフェクトを起こさない。
#[test]
fn a_saturated_known_call_takes_the_mask_of_its_last_arrow() {
    let text = format!(
        "{STATE}twice : Int -> (Unit -> <e> a) -> <e> a\ntwice _ cb =\n  let _ = cb ()\n  cb ()\n\nrun : (Unit -> <e> a) -> <State Int | e> a\nrun cb = twice 1 cb\n\n{}",
        state_main("run (fn () -> 1)")
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run@[Int]"), @r#"
    fn "run@[Int]"(cb.0: tobj) -> int {
      let t.1: int = mask [State] call "twice@[Int]"(1, cb.0)
      return t.1
    }
    "#);
}

/// 矢印ごとの `mask` が変わる境目で `apply` を分ける。
#[test]
fn arrows_with_different_masks_are_applied_apart() {
    let text = format!(
        "{STATE}h : (Int -> <e> Int -> <State Int | e> Int) -> <State Int | e> Int\nh f = f 1 2\n\n{}",
        state_main("h (fn x y -> x + y)")
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "h"), @"
    fn h(f.0: tobj) -> int {
      let t.1: tobj = mask [State] apply f.0(1)
      let t.2: int = apply t.1(2)
      return t.2
    }
    ");
}

/// 節の中の継続の呼び出しの `mask`。節は handle の外側の row で動くので、ふつうは `mask` が要らない。
#[test]
fn a_continuation_call_in_its_clause_has_no_mask() {
    let text = format!(
        "{STATE}run : (Unit -> <State Int | e> a) -> <e> a\nrun action =\n  handle action () from 0 with\n    | get () k st -> k st st\n    | put n k _ -> k () n\n    | return x _ -> x\n\nmain : Unit -> <IO> Unit\nmain () = println (show (run (fn () -> get ())))\n"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run@[Int]$handle0$get"), @r#"
    internal fn "run@[Int]$handle0$get"(p.0: unit, k.1: tobj, st.2: int) -> int {
      let t.3: int = resume k.1(st.2, st.2)
      return t.3
    }
    "#);
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
  println (show r)
";
    // handle の番号は HIR の式の ID の順なので、内側の handle が `f$handle0` になる
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "f$handle0"), @"
    internal fn f$handle0(k.0: tobj, p.1: unit) -> int {
      let t.2: int = mask [Log] resume k.0(1, ())
      return t.2
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
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "run@[Int]"), @r#"
    fn "run@[Int]"(cb.0: tobj) -> int {
      let t.1: tobj = call "pick@[Int]"(1)
      let t.2: int = mask [State] apply t.1(cb.0)
      return t.2
    }
    "#);
}

const ASK: &str = "effect Ask where\n  ask : Unit -> Int\n\n";

/// `ASK` と、handle の式 `handle` の結果の `Int` を表示する `main` の、translate 直後の Core IR。
fn handled(extra: &str, handle: &str) -> String {
    core_text(
        &format!(
            "{ASK}{extra}main : Unit -> <IO> Unit\nmain () =\n  let n =\n{handle}\n  println (show n)"
        ),
        Pass::Translate,
    )
}

/// `k` を直接呼ぶだけなら、生の継続への `resume` にする (docs/spec/core-ir.md)。
#[test]
fn a_continuation_called_directly_is_resumed() {
    let shown = handled("", "    handle ask () with\n      | ask () k -> k 1");
    assert!(shown.contains("resume k.1(1, ())"), "{shown}");
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
  println (show r)
";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("mask [Log] resume k"), "{shown}");
}

/// 状態のある `k v` の部分適用は、`cont$state` のクロージャに包む。
#[test]
fn a_partial_continuation_is_wrapped_with_the_state_wrapper() {
    let shown = handled(
        "later : (Int -> <e> Int) -> Int -> <e> Int\nlater f s = f s\n\n",
        "    handle ask () from 0 with\n      | ask () k st -> later (k 1) st\n      | return x _ -> x",
    );
    assert!(shown.contains("closure cont$state(k"), "{shown}");
    insta::assert_snapshot!(function(&shown, "cont$state"), @"
    internal fn cont$state(k.0: tobj, v.1: tobj, s.2: tobj) -> tobj {
      let t.3: tobj = resume k.0(v.1, s.2)
      return t.3
    }
    ");
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
        "{ASK}main : Unit -> <IO> Unit\nmain () =\n  let h = handle ask () with\n    | ask () k ->\n        let r = k 1 2\n        fn y -> r + y\n    | return x -> fn y -> x + y\n  println (show (h 5))"
    );
    let shown = core_text(&text, Pass::Translate);
    assert!(
        shown.contains("let t.3: tobj = resume k.1(1, ())"),
        "{shown}"
    );
    assert!(shown.contains("let t.4: int = apply t.3(2)"), "{shown}");
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
  println (show r)
";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("mask [Log] apply"), "{shown}");
}

#[test]
fn externs_are_called_by_their_canonical_name() {
    // `==` と `!=` は、型検査が記録した型引数から比べ方の行を選ぶ。`(==)` は HIR がラムダに脱糖するので、包む関数を作らない
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\neq : Int -> Int -> Bool\neq = (==)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = (1 + 2, \"a\" == \"b\", True != False, eq 1 2)\n  println (apply show 3)";
    let shown = core_text(text, Pass::Translate);
    for expected in [
        "extern Prelude.+(1, 2)",
        "extern \"Prelude.Eq String.==\"(",
        "extern \"Prelude.Eq Bool.!=\"(#1, #0)",
        "extern \"Prelude.Eq Int.==\"(",
        "call \"apply@[Int, String]\"(&main$extern0, 3)",
        "fn main$extern0(p.0: int) -> obj {\n  let t.1: obj = extern \"Prelude.Show Int.show\"(p.0)\n  return t.1\n}",
    ] {
        assert!(shown.contains(expected), "{expected}\n{shown}");
    }
    assert!(!shown.contains("eq$extern"), "{shown}");
}

#[test]
fn a_condition_true_on_every_path_meets_at_one_block() {
    // `(a || True) && True` はどの道でも真なので、`else` の枝は変換せず、どの葉も `then` の枝の1つのブロックへ向かう
    let text = "noisy : String -> Bool -> <IO> Bool\nnoisy name b =\n  println name\n  b\n\nmain : Unit -> <IO> Unit\nmain () =\n  let other = \"other\"\n  let a = noisy \"a\" True\n  if (a || True) && True then println \"x\" else println other\n  println \"end\"";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "other"
      let s.2: obj = const "a"
      let t.3: enum = call noisy(s.2, #1)
      switch t.3 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3()
    b2:
      jump b3()
    b3:
      let s.4: obj = const "x"
      let t.5: unit = extern Prelude.println(s.4)
      let s.6: obj = const "end"
      let t.7: unit = extern Prelude.println(s.6)
      return t.7
    }
    "#);
}

#[test]
fn a_match_on_a_constructed_value_takes_its_arm() {
    // scrutinee がコンストラクタの適用なので、値を作らずに枝を選び、フィールドを直接使う
    let text = format!(
        "{OPTION}unwrap : Int -> Int\nunwrap x = match Some x with\n  | Some y -> y\n  | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (unwrap 1))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "unwrap"), @"
    fn unwrap(x.0: int) -> int {
      return x.0
    }
    ");
}

#[test]
fn a_wildcard_arm_does_not_block_the_known_tags() {
    // 決定木は行列に現れないコンストラクタを `default` にまとめる。ワイルドカードの枝があっても、分かっているタグは
    // その場で枝を選び、`if` の結果で分岐し直さない
    let text = "data Color = | Red | Green | Blue\n\npick : Bool -> Int\npick b =\n  let n = match (if b then Red else Green) with\n    | Red -> 1\n    | _ -> 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "pick"), @"
    fn pick(b.0: enum) -> int {
      switch b.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(2)
    b2:
      jump b3(1)
    b3(t.1: int):
      let t.2: int = extern Prelude.+(t.1, 1)
      return t.2
    }
    ");
}

#[test]
fn known_values_take_their_arms_through_a_literal_column() {
    // `Yes 0` はフィールドのリテラルまで分かるので、`switch` を出さずに `Yes 0` の枝を選ぶ。`_` の枝には `No` と、
    // 中身が 0 でない `Yes` の2つの葉が向かうが、分かっている値はその場で選ぶ
    let text = "data Opt = | No | Yes Int\n\npick : Bool -> Int\npick b =\n  match (if b then Yes 0 else No) with\n    | Yes 0 -> 1\n    | _ -> 2\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "pick"), @"
    fn pick(b.0: enum) -> int {
      switch b.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      return 2
    b2:
      return 1
    }
    ");
}

#[test]
fn a_known_and_an_unknown_value_share_an_arm() {
    // 分かっている `Some 1` はフィールドを引数に `Some v` の枝へ直接向かう。分からない呼び出しの結果は `switch` の
    // case から同じ枝へ向かうので、枝は2本が入るブロックになる
    let text = format!(
        "{OPTION}lookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int\npick c =\n  let n = match (if c then Some 1 else lookup 2) with\n    | Some v -> v\n    | None -> 0\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "pick"), @"
    fn pick(c.0: enum) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.1: tobj = call lookup(2)
      switch t.1 Option { #0 -> b3, #1(v.2: int) -> b4 }
    b2:
      jump b5(1)
    b3:
      jump b6(0)
    b4:
      jump b5(v.2)
    b5(v.3: int):
      jump b6(v.3)
    b6(t.4: int):
      let t.5: int = extern Prelude.+(t.4, 1)
      return t.5
    }
    ");
}

#[test]
fn a_known_tag_reaching_a_default_that_uses_the_scrutinee_passes_the_value() {
    // `Green` は `default` の枝 `x` に進み、その枝は値全体を使うので、タグの定数 `#1` をそのまま渡す。`Red` は枝の
    // 値を続きへ渡し、`if` の結果で分岐し直さない
    let text = "data Color = | Red | Green | Blue\n\ncode : Color -> Int\ncode c = 7\n\npick : Bool -> Int\npick b =\n  let n = match (if b then Red else Green) with\n    | Red -> 1\n    | x -> code x\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "pick"), @"
    fn pick(b.0: enum) -> int {
      switch b.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.1: int = call code(#1)
      jump b3(t.1)
    b2:
      jump b3(1)
    b3(t.2: int):
      let t.3: int = extern Prelude.+(t.2, 1)
      return t.3
    }
    ");
}

#[test]
fn a_known_tag_without_a_case_takes_the_default() {
    // `c` は `let` で束縛したタグなので、`switch` を出さずに `default` の枝を選ぶ
    let text = "data Color = | Red | Green | Blue\n\nmain : Unit -> <IO> Unit\nmain () =\n  let c = Green\n  let n = match c with\n    | Red -> 1\n    | _ -> 2\n  println (show n)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "main"), @"
    fn main(p.0: unit) -> unit {
      let t.1: obj = extern \"Prelude.Show Int.show\"(2)
      let t.2: unit = extern Prelude.println(t.1)
      return t.2
    }
    ");
}

#[test]
fn known_tags_that_take_the_default_jump_straight_to_it() {
    // `Green` と `Blue` はどちらも `default` に進むので、`if` の結果で分岐し直さずに `_` の枝へ直接向かう
    let text = "data Color = | Red | Green | Blue\n\npick : Bool -> Int\npick b =\n  match (if b then Green else Blue) with\n    | Red -> 1\n    | _ -> 2\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick True))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "pick"), @"
    fn pick(b.0: enum) -> int {
      switch b.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3()
    b2:
      jump b3()
    b3:
      return 2
    }
    ");
}

/// 表示の先頭の配置の行。
fn layouts(shown: &str) -> String {
    shown
        .lines()
        .filter(|line| line.starts_with("layout "))
        .map(|line| format!("{line}\n"))
        .collect()
}

#[test]
fn layouts_hold_the_declared_field_reprs_in_order_of_first_use() {
    // `Pair a Int` のフィールドは宣言の型で決まる。`Option` は2つの具体化で1つの配置を使う。使わない `Unused` は
    // 配置を持たない。タプルは要素の数ごとに1つの配置で、フィールドはどれも `tobj` である
    let text = "data Pair a =\n  | Pair a Int\n\ndata Unused =\n  | Unused Int\n\ndata Option a =\n  | None\n  | Some a\n\nfirst : Pair a -> a\nfirst (Pair x _) = x\n\npick : Option Int -> Option String -> Int\npick a b = match (a, b) with\n  | (Some n, _) -> n\n  | (None, Some _) -> 1\n  | (None, None) -> 0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let t = ((1, 2), 3, 4)\n  drop t\n  println (show (first (Pair (pick (Some 1) None) 2)))";
    insta::assert_snapshot!(layouts(&core_text(text, Pass::Translate)), @"
    layout Pair { Pair(tobj, int) }
    layout Option { None, Some(tobj) }
    layout (,) { (,)(tobj, tobj) }
    layout (,,) { (,,)(tobj, tobj, tobj) }
    ");
}

#[test]
fn a_tuple_taken_apart_without_being_built_adds_no_layout() {
    // `match (a, b)` のタプルは決定木が分解するだけで作らないので、タプルの配置は表に入らない
    let text = "pick : Int -> Int -> Int\npick a b = match (a, b) with\n  | (0, _) -> 1\n  | (_, 0) -> 2\n  | _ -> 3\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick 1 2))";
    insta::assert_snapshot!(layouts(&core_text(text, Pass::Translate)), @"");
}

#[test]
fn an_if_names_the_bool_of_the_prelude() {
    let text = "f : Bool -> Int\nf b = if b then 1 else 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f True))";
    insta::assert_snapshot!(layouts(&core_text(text, Pass::Translate)), @"layout Prelude.Bool { False, True }");
}

#[test]
fn a_type_named_like_a_type_of_another_module_gets_its_own_layout() {
    // 配置は型の定義ごとに1つで、名前はモジュールで修飾するので、短い名前が同じ2つの型は別の配置になる
    // (docs/spec/core-ir.md の「データの配置」)
    let csv = "pub data Row = | Row Int\n\npub row : Int -> Row\nrow n = Row n";
    let main = "import Report.Csv\n\ndata Row =\n  | Row String\n\nrow : String -> Row\nrow s = Row s\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Row s = row \"a\"\n  let Csv.Row n = Csv.row 1\n  println s\n  println (show n)";
    insta::assert_snapshot!(layouts(&core_text_files(main, &[("Report/Csv.em", csv)], Pass::Translate)), @"
    layout Row { Row(obj) }
    layout Report.Csv.Row { Row(int) }
    ");
}

#[test]
fn instances_of_one_function_get_their_own_reprs() {
    // 型引数ごとの instance は、その型の Repr で別々に変換する (docs/spec/core-ir.md の「変換の規則」)
    let text = "id : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = id 1\n  println (id \"s\")";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "id@[Int]"), @r#"
    fn "id@[Int]"(x.0: int) -> int {
      return x.0
    }
    "#);
    insta::assert_snapshot!(function(&shown, "id@[String]"), @r#"
    fn "id@[String]"(x.0: obj) -> obj {
      return x.0
    }
    "#);
}

#[test]
fn lifted_functions_are_named_after_their_instance() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : a -> (a -> a) -> a\ntwice x f =\n  let g = fn y -> f (f y)\n  handle g x with\n    | ask () k -> k 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (twice 1 (fn n -> n + 1)))";
    let shown = core_text(text, Pass::Translate);
    for name in ["twice@[Int]$lambda0", "twice@[Int]$handle0"] {
        assert!(
            shown.contains(&format!("{name:?}")),
            "no `{name}` in\n{shown}"
        );
    }
}

#[test]
fn a_boxed_wrapper_is_named_after_its_instance() {
    let text = "inc : a -> Int -> Int\ninc _ n = n + 1\n\napply_to : (Int -> Int) -> Int -> Int\napply_to f x = f x\n\nmain : Unit -> <IO> Unit\nmain () = println (show (apply_to (inc ()) 1))";
    let shown = core_text(text, Pass::Boxing);
    assert!(
        shown.contains("\"inc@[Unit]$boxed\""),
        "no wrapper in\n{shown}"
    );
}

const SAME: &str = "class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n  differ x y = not (same x y)\n\ndata Color =\n  | Red\n  | Green\n\ninstance Same Color where\n  same Red Red = True\n  same Green Green = True\n  same _ _ = False\n\n";

#[test]
fn a_method_call_resolves_to_the_instance_function() {
    let text = format!(
        "{SAME}main : Unit -> <IO> Unit\nmain () = if same Red Green then println \"y\" else println \"n\""
    );
    let shown = core_text(&text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.1: enum = call "Same Color.same"(#0, #1)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.4: obj = const "n"
      let t.5: unit = extern Prelude.println(s.4)
      return t.5
    b2:
      let s.2: obj = const "y"
      let t.3: unit = extern Prelude.println(s.2)
      return t.3
    }
    "#);
    insta::assert_snapshot!(function(&shown, "Same Color.same"), @r#"
    fn "Same Color.same"($0.0: enum, $1.1: enum) -> enum {
      switch $0.0 Color { #0 -> b1, #1 -> b2 }
    b1:
      switch $1.1 Color { #0 -> b3, _ -> b4 }
    b2:
      switch $1.1 Color { #1 -> b5, _ -> b6 }
    b3:
      return #1
    b4:
      jump b7()
    b5:
      return #1
    b6:
      jump b7()
    b7:
      return #0
    }
    "#);
}

#[test]
fn a_default_method_is_an_instance_at_the_head_type() {
    let text = format!(
        "{SAME}main : Unit -> <IO> Unit\nmain () = if differ Red Green then println \"y\" else println \"n\""
    );
    let shown = core_text(&text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "differ@[Color]"), @r#"
    fn "differ@[Color]"(x.0: enum, y.1: enum) -> enum {
      let t.2: enum = call "Same Color.same"(x.0, y.1)
      let t.3: enum = call Prelude.not(t.2)
      return t.3
    }
    "#);
}

#[test]
fn a_constraint_passes_through_two_generic_functions() {
    let text = format!(
        "{SAME}twice : Same a => a -> Bool\ntwice x = same x x\n\nouter : Same a => a -> Bool\nouter x = twice x\n\nmain : Unit -> <IO> Unit\nmain () = if outer Red then println \"y\" else println \"n\""
    );
    let shown = core_text(&text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "outer@[Color]"), @r#"
    fn "outer@[Color]"(x.0: enum) -> enum {
      let t.1: enum = call "twice@[Color]"(x.0)
      return t.1
    }
    "#);
    insta::assert_snapshot!(function(&shown, "twice@[Color]"), @r#"
    fn "twice@[Color]"(x.0: enum) -> enum {
      let t.1: enum = call "Same Color.same"(x.0, x.0)
      return t.1
    }
    "#);
}

#[test]
fn an_instance_method_with_fewer_parameters_is_called_then_applied() {
    let text = "class Combine a where\n  combine : a -> a -> Int\n\ninstance Combine Int where\n  combine = add\n\nadd : Int -> Int -> Int\nadd a b = a + b\n\nmain : Unit -> <IO> Unit\nmain () = println (show (combine 1 2))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.1: tobj = call "Combine Int.combine"()
      let t.2: int = apply t.1(1, 2)
      let t.3: obj = extern "Prelude.Show Int.show"(t.2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
    }
    "#);
}

#[test]
fn extern_methods_are_called_with_the_instruction_of_their_instance() {
    // extern で結んだメソッドは、instance の行の extern 命令を直接出す
    // (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「メソッドへの参照の解決」)
    let text = "same : String -> Bool -> Bool\nsame s b = \"a\" == s && b != True\n\norder : Int -> Ordering\norder n = compare n 0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = same \"b\" False\n  let _ = order 1\n  ()";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "same"), @r#"
    fn same(s.0: obj, b.1: enum) -> enum {
      let s.2: obj = const "a"
      let t.3: enum = extern "Prelude.Eq String.=="(s.2, s.0)
      switch t.3 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      return #0
    b2:
      let t.4: enum = extern "Prelude.Eq Bool.!="(b.1, #1)
      return t.4
    }
    "#);
    insta::assert_snapshot!(function(&shown, "order"), @r#"
    fn order(n.0: int) -> enum {
      let t.1: enum = extern "Prelude.Ord Int.compare"(n.0, 0)
      return t.1
    }
    "#);
}

#[test]
fn extern_methods_used_as_values_are_wrapped_per_instance() {
    // `$externN` の番号は instance ごとに振る。`shown@[Bool]` の `show` は関数の行き先なので、包む関数を作らない
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nshown : Show a => a -> String\nshown x = apply show x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let first = compare 1\n  println (shown 1 ++ shown \"a\" ++ shown True)\n  println (show (first 2))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "shown@[Int]"), @r#"
    fn "shown@[Int]"(x.0: int) -> obj {
      let t.1: obj = call "apply@[Int, String]"(&"shown@[Int]$extern0", x.0)
      return t.1
    }
    "#);
    insta::assert_snapshot!(function(&shown, "shown@[Int]$extern0"), @r#"
    internal fn "shown@[Int]$extern0"(p.0: int) -> obj {
      let t.1: obj = extern "Prelude.Show Int.show"(p.0)
      return t.1
    }
    "#);
    insta::assert_snapshot!(function(&shown, "shown@[String]$extern0"), @r#"
    internal fn "shown@[String]$extern0"(p.0: obj) -> obj {
      let t.1: obj = extern "Prelude.Show String.show"(p.0)
      return t.1
    }
    "#);
    insta::assert_snapshot!(function(&shown, "shown@[Bool]"), @r#"
    fn "shown@[Bool]"(x.0: enum) -> obj {
      let t.1: obj = call "apply@[Bool, String]"(&"Prelude.Show Bool.show", x.0)
      return t.1
    }
    "#);
    insta::assert_snapshot!(function(&shown, "main$extern0"), @r#"
    internal fn main$extern0(p.0: int, p.1: int) -> enum {
      let t.2: enum = extern "Prelude.Ord Int.compare"(p.0, p.1)
      return t.2
    }
    "#);
    assert!(!shown.contains("shown@[Bool]$extern"), "{shown}");
}

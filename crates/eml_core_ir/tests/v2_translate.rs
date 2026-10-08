//! 型付き HIR からブロックの列への変換 (docs/spec/core-ir.md)。段の API の `v2::lower_until` を直接
//! 呼ぶ。HIR と型検査の結果は `eml_test_support::check` で作り、`Checked` の `program`、`typed`、`files()` を渡す。

use eml_core_ir::v2::{
    self, Pass, Program, parse, pretty, pretty_with_positions, verify, verify_scopes,
};

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にする。
fn lowered(text: &str, last: Pass) -> Program {
    let checked = eml_test_support::check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "unexpected diagnostics:\n{}",
        eml_test_support::short_text(checked.files(), &checked.diagnostics)
    );
    let main = checked
        .program
        .main()
        .expect("the test program defines `main`");
    v2::lower_until(
        &checked.program,
        &checked.typed,
        main,
        checked.files(),
        last,
    )
}

/// 表示した IR を読み直し、同じ表示に戻ることと、読み直した IR が同じ段の verifier を通ることを確かめる。`pretty` と
/// `parse` が同じ誤りをして、違うプログラムで表示だけがそろうことを見つけるためである。
fn read_back(shown: &str, last: Pass, show: fn(&Program) -> String) {
    let parsed = parse(shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(show(&parsed), shown, "the printed Core IR must read back");
    let verified = match last {
        Pass::Translate | Pass::Contract => verify_scopes(&parsed),
        Pass::Perceus => verify(&parsed),
    };
    if let Err(error) = verified {
        panic!("the Core IR read back must verify: {error}\n{shown}");
    }
}

fn core_text(text: &str, last: Pass) -> String {
    let shown = pretty(&lowered(text, last));
    read_back(&shown, last, pretty);
    shown
}

fn core_text_with_positions(text: &str) -> String {
    let shown = pretty_with_positions(&lowered(text, Pass::Translate));
    read_back(&shown, Pass::Translate, pretty_with_positions);
    shown
}

/// 名前で選んだ関数の表示。ほかの関数 (`entry$main` など) を期待値から外すため。
fn function(shown: &str, name: &str) -> String {
    let start = shown
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no `{name}` in\n{shown}"));
    let end = shown[start..]
        .find("\n}\n")
        .expect("a function ends with `}`")
        + start
        + 3;
    shown[start..end].to_string()
}

#[test]
fn hello_world() {
    insta::assert_snapshot!(core_text("main : Unit -> <IO> Unit\nmain () = println \"hi\"", Pass::Translate), @r#"
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "hi"
      let t.2: unit = extern Prelude.println(s.1)
      return t.2
    }
    fn entry$main() -> unit {
      tail call main(())
    }
    "#);
}

#[test]
fn a_value_if_with_one_exit_continues_in_the_same_block() {
    // 条件が定数なので続きへ向かうのは then の枝だけで、続きのブロックを作らずに同じブロックで続ける
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let n = if True then 1 else 2\n  println (show_int n)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "main"), @"
    fn main(p.0: unit) -> unit {
      let t.1: obj = extern Prelude.show_int(1)
      let t.2: unit = extern Prelude.println(t.1)
      return t.2
    }
    ");
}

#[test]
fn a_continuation_entered_by_two_jumps_is_a_merge_block() {
    let text = "pick : Bool -> Int\npick b =\n  let n = if b then 1 else 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "pick"), @"
    fn pick(b.0: enum) -> int {
      switch b.0 { #0 -> b1, #1 -> b2 }
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
      let t.2: enum = extern Prelude.<(a.0, b.1)
      switch t.2 { #0 -> b1, #1 -> b2 }
    b1:
      jump b5()
    b2:
      let t.3: enum = extern Prelude.<(b.1, 10)
      switch t.3 { #0 -> b3, #1 -> b4 }
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
      let t.2: enum = extern Prelude.<(a.0, 0)
      switch t.2 { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: enum = extern Prelude.<(b.1, 0)
      switch t.3 { #0 -> b3, #1 -> b4 }
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
      switch a.0 { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(c.2)
    b2:
      jump b3(b.1)
    b3(c.3: enum):
      switch c.3 { #0 -> b4, #1 -> b5 }
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
      switch b.0 { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(())
    b2:
      let s.1: obj = const "a"
      let t.2: unit = extern Prelude.println(s.1)
      jump b3(t.2)
    b3(t.3: unit):
      switch b.0 { #0 -> b4, #1 -> b5 }
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
fn a_returned_if_value_becomes_tail_calls_in_each_arm() {
    // `let y = if ..; y` の続きは `return` だけのブロックなので、各枝の `jump` を `return` にしてブロックを消し、
    // 呼び出しの後の `return` を末尾呼び出しにする
    let text = "f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Bool -> Int -> Int\nh c x =\n  let y = if c then f x else g x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h True 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "h"), @"
    fn h(c.0: enum, x.1: int) -> int {
      switch c.0 { #0 -> b1, #1 -> b2 }
    b1:
      tail call g(x.1)
    b2:
      tail call f(x.1)
    }
    ");
}

#[test]
fn a_chain_of_returned_continuations_folds_in_one_pass() {
    // 内側の続き `z` は外側の続き `y` へ jump するだけで、外側の続きは `return y` だけである。番号の大きい外側を先に
    // たたむと、内側も `return` だけのブロックになり、同じループでたたまれる
    let text = "f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Bool -> Bool -> Int -> Int\nh a b x =\n  let y =\n    if a then\n      let z = if b then f x else g x\n      z\n    else f (x + 2)\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h True False 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "h"), @"
    fn h(a.0: enum, b.1: enum, x.2: int) -> int {
      switch a.0 { #0 -> b1, #1 -> b2 }
    b1:
      let t.6: int = extern Prelude.+(x.2, 2)
      tail call f(t.6)
    b2:
      switch b.1 { #0 -> b3, #1 -> b4 }
    b3:
      tail call g(x.2)
    b4:
      tail call f(x.2)
    }
    ");
}

#[test]
fn every_extern_call_carries_the_position_of_its_callee() {
    // 演算子の呼び出しは演算子のトークンの位置を、名前で呼ぶ extern は名前の位置を持つ
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let n = 10 / 2\n  println (show_int n)";
    insta::assert_snapshot!(function(&core_text_with_positions(text), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let t.1: int = extern Prelude./(10, 2) @"test.em":3:14
      let t.2: obj = extern Prelude.show_int(t.1) @"test.em":4:12
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
    fn apply(f.0: tobj, x.1: tobj) -> tobj {
      tail apply f.0(x.1)
    }
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "a"
      let t.2: unit = call apply(&main$extern0, s.1)
      let s.3: obj = const "b"
      tail apply &main$extern1(s.3)
    }
    fn main$extern0(p.0: obj) -> unit {
      let t.1: unit = extern Prelude.println(p.0) @"test.em":6:9
      return t.1
    }
    fn main$extern1(p.0: obj) -> unit {
      let t.1: unit = extern Prelude.println(p.0) @"test.em":7:13
      return t.1
    }
    fn entry$main() -> unit {
      tail call main(())
    }
    "#);
}

#[test]
fn lambdas_and_handlers_are_numbered_in_expression_order() {
    // HIR は子の式を親より先に作るので、内側のラムダと内側の handle の番号が小さい。変換は外側から持ち上げるが、
    // 番号は変換の順に依らない
    let text = "effect A where\n  ask : Unit -> Int\n\neffect B where\n  tell : Int -> Unit\n\nmain : Unit -> <IO> Unit\nmain () =\n  let add = fn x -> fn y -> x + y\n  let n =\n    handle (handle ask () + add 1 2 with | ask () k -> k 1) with\n      | tell m k -> k ()\n  println (show_int n)";
    let shown = core_text(text, Pass::Translate);
    let names: Vec<&str> = shown
        .lines()
        .filter_map(|line| line.strip_prefix("fn "))
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
    fn main$lambda1(x.0: int) -> tobj {
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
      let d.3: obj = con #0(n.0, s.2)
      let c.4: tobj = closure make$lambda0(n.0)
      let t.5: int = apply c.4(1)
      let d.6: tobj = con #1(t.5)
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
    fn main$lambda0(p.0: unit) -> unit {
      let s.1: obj = const "point-free"
      let t.2: unit = extern Prelude.println(s.1)
      return t.2
    }
    fn entry$main() -> unit {
      let f.0: tobj = call main()
      tail apply f.0(())
    }
    "#);
}

#[test]
fn handlers_are_lifted_with_their_return_reprs() {
    let text = "effect Ask where\n  ask : String -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let prefix = \"n = \"\n  let n =\n    handle ask \"x\" with\n      | ask key k -> k 1\n      | return x -> x + 1\n  println (prefix ++ show_int n)";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    effect Ask { ask/1 }
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "n = "
      let t.2: int = handle Ask((), &main$handle0) { ask: &main$handle0$ask } return &main$handle0$return
      let t.3: obj = extern Prelude.show_int(t.2)
      let t.4: obj = extern Prelude.++(s.1, t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    fn main$handle0(p.0: unit) -> int {
      let s.1: obj = const "x"
      tail perform Ask.ask(s.1)
    }
    fn main$handle0$ask(key.0: obj, k.1: tobj, p.2: unit) -> int {
      tail resume k.1(1, ())
    }
    fn main$handle0$return(x.0: int, p.1: unit) -> int {
      let t.2: int = extern Prelude.+(x.0, 1)
      return t.2
    }
    fn entry$main() -> unit {
      tail call main(())
    }
    "#);
}

#[test]
fn a_continuation_passed_to_a_function_is_wrapped() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : (Int -> <e> Int) -> <e> Int\ntwice f = f 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle ask () with\n      | ask () k -> twice k\n  println (show_int n)";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "main$handle0$ask"), @"
    fn main$handle0$ask(p.0: unit, k.1: tobj, p.2: unit) -> int {
      let c.3: tobj = closure cont$(k.1)
      tail call twice(c.3)
    }
    ");
    insta::assert_snapshot!(function(&shown, "cont$"), @"
    fn cont$(k.0: tobj, v.1: tobj) -> tobj {
      tail resume k.0(v.1, ())
    }
    ");
}

#[test]
fn lower_until_stops_after_the_named_pass() {
    // contract は使われない純粋な `let` を消し、Perceus は RC の命令と `save` を足す
    let text = "f : Int -> Int\nf x = x + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let unused = \"unused\"\n  let s = \"used\"\n  let n = f 1\n  println s\n  println (s ++ show_int n)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "unused"
      let s.2: obj = const "used"
      let t.3: int = call f(1)
      let t.4: unit = extern Prelude.println(s.2)
      let t.5: obj = extern Prelude.show_int(t.3)
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
      let t.5: obj = extern Prelude.show_int(t.3)
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
      let t.5: obj = extern Prelude.show_int(t.3)
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
    let program = lowered(&text, Pass::Perceus);
    let step = program
        .functions
        .iter()
        .find(|function| function.name == "step")
        .expect("step");
    let switches = step
        .blocks
        .iter()
        .filter(|block| matches!(block.term, v2::Term::Switch { .. }))
        .count();
    assert_eq!(switches, COUNT);
}

const OPTION: &str = "data Option a =\n  | None\n  | Some a\n\n";

#[test]
fn a_destructuring_let_of_an_if_builds_no_tuple() {
    // 両方の枝の値がタプルのリテラルなので、どちらも要素を続きのブロックの引数にして jump し、タプルを作らない
    let text = "order : Bool -> Int -> Int -> Int\norder c a b =\n  let (lo, hi) = if c then (a, b) else (b, a)\n  hi - lo\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (order True 1 2))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "order"), @"
    fn order(c.0: enum, a.1: int, b.2: int) -> int {
      switch c.0 { #0 -> b1, #1 -> b2 }
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
        "{OPTION}describe : Int -> String\ndescribe n =\n  let found = if n > 0 then Some n else None\n  match found with\n    | Some v -> show_int v\n    | None -> \"none\"\n\nmain : Unit -> <IO> Unit\nmain () = println (describe 1)"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "describe"), @r#"
    fn describe(n.0: int) -> obj {
      let t.1: enum = extern Prelude.>(n.0, 0)
      switch t.1 { #0 -> b1, #1 -> b2 }
    b1:
      let s.3: obj = const "none"
      return s.3
    b2:
      let t.2: obj = extern Prelude.show_int(n.0)
      return t.2
    }
    "#);
}

#[test]
fn an_arm_that_uses_the_matched_variable_receives_it() {
    // 枝が `found` を使うので、枝のラベルは値全体も引数に取る。分かっている値は、その葉でだけ `con` を作って渡す
    let text = format!(
        "{OPTION}size : Option Int -> Int\nsize o = 1\n\nlookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int -> Int\npick c k =\n  let found = if c then Some 1 else lookup k\n  match found with\n    | None -> 0\n    | Some _ -> size found\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True 2))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "pick"), @"
    fn pick(c.0: enum, k.1: int) -> int {
      switch c.0 { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: tobj = call lookup(k.1)
      switch t.3 { #0 -> b3, #1(x.4: int) -> b4 }
    b2:
      let d.2: tobj = con #1(1)
      jump b5(d.2)
    b3:
      return 0
    b4:
      jump b5(t.3)
    b5(found.5: tobj):
      tail call size(found.5)
    }
    ");
}

#[test]
fn a_match_on_an_if_takes_each_known_arm() {
    let text = "data Result e a =\n  | Err e\n  | Ok a\n\ncheck : Int -> String\ncheck n = match (if n < 0 then Err \"negative\" else Ok n) with\n  | Err message -> message\n  | Ok v -> show_int v\n\nmain : Unit -> <IO> Unit\nmain () = println (check 1)";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "check"), @r#"
    fn check(n.0: int) -> obj {
      let t.1: enum = extern Prelude.<(n.0, 0)
      switch t.1 { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: obj = extern Prelude.show_int(n.0)
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
        "{OPTION}lookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int\npick c =\n  let found = if c then lookup 1 else lookup 2\n  match found with\n    | Some v -> v\n    | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "pick"), @"
    fn pick(c.0: enum) -> int {
      switch c.0 { #0 -> b1, #1 -> b2 }
    b1:
      let t.2: tobj = call lookup(2)
      jump b3(t.2)
    b2:
      let t.1: tobj = call lookup(1)
      jump b3(t.1)
    b3(found.3: tobj):
      switch found.3 { #0 -> b4, #1(v.4: int) -> b5 }
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
        "{OPTION}size : Option Int -> Int\nsize o = 1\n\nlookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int -> Int\npick c k = match (if c then Some 1 else lookup k) with\n  | None -> 0\n  | x -> size x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True 2))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "pick"), @"
    fn pick(c.0: enum, k.1: int) -> int {
      switch c.0 { #0 -> b1, #1 -> b2 }
    b1:
      let t.3: tobj = call lookup(k.1)
      switch t.3 { #0 -> b3, _ -> b4 }
    b2:
      let d.2: tobj = con #1(1)
      jump b5(d.2)
    b3:
      return 0
    b4:
      jump b5(t.3)
    b5(x.4: tobj):
      tail call size(x.4)
    }
    ");
}

#[test]
fn nested_case_of_case_goes_straight_to_the_outer_arm() {
    // 内側の `match` の枝は外側の文脈に値を渡すので、`Bool` の値も `Color` の値も作らずに外側の枝へ向かう
    let text = "data Color =\n  | Red\n  | Green\n  | Blue\n\ncode : Bool -> Bool -> Int\ncode a b = match (match (if a then Red else if b then Green else Blue) with | Red -> True | _ -> False) with\n  | True -> 1\n  | False -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (code True False))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "code"), @"
    fn code(a.0: enum, b.1: enum) -> int {
      switch a.0 { #0 -> b1, #1 -> b2 }
    b1:
      switch b.1 { #0 -> b3, #1 -> b4 }
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
      switch o.0 { #0 -> b1, #1(x.1: int) -> b2 }
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
        "{OPTION}unwrap : Int -> <IO> Int\nunwrap x =\n  let o = Some x\n  println \"built\"\n  match o with\n    | Some y -> y + 1\n    | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (unwrap 1))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "unwrap"), @r#"
    fn unwrap(x.0: int) -> int {
      let d.1: tobj = con #1(x.0)
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
    let text = "shift : Int -> Int -> Int\nshift x y =\n  let (a, b) = (y, x + 1)\n  a - b\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (shift 1 2))";
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
        "{OPTION}both : Option Int -> Option Int -> Int\nboth (Some a) (Some b) = a + b\nboth _ _ = 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (both None None))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "both"), @"
    fn both($0.0: tobj, $1.1: tobj) -> int {
      switch $0.0 { #1(a.2: int) -> b1, _ -> b2 }
    b1:
      switch $1.1 { #1(b.3: int) -> b3, _ -> b4 }
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
    let text = "first : (Int, Int) -> Int\nfirst p =\n  let (a, _) = p\n  a\n\nclassify : Int -> Int -> Int\nclassify a b = match (a, b) with\n  | (0, _) -> 0\n  | pair -> first pair\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (classify 1 2))";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "classify"), @"
    fn classify(a.0: int, b.1: int) -> int {
      switch a.0 { 0 -> b1, _ -> b2 }
    b1:
      return 0
    b2:
      let d.2: obj = con #0(a.0, b.1)
      tail call first(d.2)
    }
    ");
    insta::assert_snapshot!(function(&shown, "first"), @"
    fn first(p.0: obj) -> int {
      unpack p.0 #0(a.1: int, x.2: int)
      return a.1
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
    let text = "data Color =\n  | Red\n  | Green\n  | Blue\n\nsame : Color -> Color -> Int\nsame a b = match (a, b) with\n  | (Red, Red) -> 1\n  | (Green, Green) -> 2\n  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (same Red Blue))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "same"), @"
    fn same(a.0: enum, b.1: enum) -> int {
      switch a.0 { #0 -> b1, #1 -> b2, _ -> b3 }
    b1:
      switch b.1 { #0 -> b4, _ -> b5 }
    b2:
      switch b.1 { #1 -> b6, _ -> b7 }
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
      unpack p.0 #0(s.1: obj)
      return s.1
    }
    ");
}

#[test]
fn a_lone_constructor_without_fields_is_a_wildcard() {
    // `Token` は値が1つしかないので、引数、`let`、`match` の枝のどこでも、`unpack` も `switch` も出さない
    let text = "data Token =\n  | Token\n\nspend : Token -> Int -> Int\nspend Token n =\n  let Token = Token\n  match Token with\n    | Token -> n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (spend Token 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "spend"), @"
    fn spend(p.0: enum, n.1: int) -> int {
      let t.2: int = extern Prelude.+(n.1, 1)
      return t.2
    }
    ");
}

#[test]
fn a_returned_match_value_becomes_tail_calls_in_each_arm() {
    let text = format!(
        "{OPTION}f : Int -> Int\nf x = x + 1\n\ng : Int -> Int\ng x = x - 1\n\nh : Option Int -> Int\nh o =\n  let y = match o with\n    | Some v -> f v\n    | None -> g 0\n  let z = y\n  z\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h None))"
    );
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "h"), @"
    fn h(o.0: tobj) -> int {
      switch o.0 { #0 -> b1, #1(v.1: int) -> b2 }
    b1:
      tail call g(0)
    b2:
      tail call f(v.1)
    }
    ");
}

#[test]
fn returning_a_field_of_a_call_result_is_not_a_tail_call() {
    // 返すのはタプル全体ではなくフィールドなので、呼び出しの後に `unpack` が残り、末尾呼び出しにならない
    let text = "split : Int -> (Int, Int)\nsplit x = (x, x + 1)\n\nfirst : Int -> Int\nfirst x =\n  let (y, _) = split x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (first 1))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Translate), "first"), @"
    fn first(x.0: int) -> int {
      let t.1: obj = call split(x.0)
      unpack t.1 #0(y.2: int, x.3: int)
      return y.2
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
    text.push_str(
        "  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick A 7))",
    );
    let program = lowered(&text, Pass::Perceus);
    let pick = program
        .functions
        .iter()
        .find(|function| function.name == "pick")
        .expect("pick");
    let jumps = pick
        .blocks
        .iter()
        .filter(|block| matches!(block.term, v2::Term::Jump { .. }))
        .count();
    // `(_, i)` の枝と最後の枝に、それぞれ2本ずつ
    assert_eq!(jumps, 2 * (COUNT + 1));
}

//! 生存解析と Perceus の `dup` / `decref` の位置、呼び出しの `saved` (docs/spec/core-ir.md の「パス」)。IR のテキストを
//! 入力にするテストは、入力が scope の段の verifier を、出力が所有の段の verifier を通ることも確かめる。後半は、
//! ソースからパイプラインを通した結果を見る。

use crate::common::{core_text, function};
use eml_core_ir::liveness::live_in;
use eml_core_ir::{Pass, VarId, parse, perceus, pretty, verify, verify_scopes};

fn perceus_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_scopes(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    perceus(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

#[test]
fn live_in_holds_every_variable_used_later() {
    // RC の対象でない変数も入る。`saved` が使うためである。jump の引数と case のフィールドは行き先の入口で生きている
    let program = parse(
        "\
fn f(x.0: int, o.1: tobj) -> int {
  let c.2: enum = extern Prelude.<(x.0, 10)
  switch c.2 { #0 -> b1, #1 -> b2 }
b1:
  jump b4(x.0)
b2:
  switch o.1 { #0 -> b3, #1(y.3: int) -> b5 }
b3:
  jump b4(1)
b4(t.4: int):
  return t.4
b5:
  return y.3
}
",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let sets = live_in(&program.functions[0]);
    let expected: [&[u32]; 6] = [&[0, 1], &[0], &[1], &[], &[4], &[3]];
    assert_eq!(
        sets,
        expected.map(|set| set.iter().copied().map(VarId).collect::<Vec<_>>())
    );
}

#[test]
fn a_second_use_is_dupped() {
    let text = "\
fn twice(s.0: obj) -> obj {
  let t.1: obj = extern Prelude.++(s.0, s.0)
  return t.1
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn twice(s.0: obj) -> obj {
      dup s.0
      let t.1: obj = extern Prelude.++(s.0, s.0)
      return t.1
    }
    ");
}

#[test]
fn unused_parameters_are_released_at_the_start() {
    // RC の対象でない引数は数えない
    let text = "\
fn first(a.0: obj, b.1: tobj, n.2: int) -> obj {
  return a.0
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn first(a.0: obj, b.1: tobj, n.2: int) -> obj {
      decref b.1
      return a.0
    }
    ");
}

#[test]
fn a_variable_that_dies_on_one_edge_is_released_in_that_block() {
    // `b1` は `s.1` を使わずに `b3` へ行くので、入口で捨てる。`b2` は `s.1` を引数で渡す
    let text = "\
fn pick(c.0: enum, s.1: obj) -> obj {
  switch c.0 { #0 -> b1, #1 -> b2 }
b1:
  let t.2: obj = const \"none\"
  jump b3(t.2)
b2:
  jump b3(s.1)
b3(r.3: obj):
  return r.3
}
";
    insta::assert_snapshot!(perceus_text(text), @r#"
    fn pick(c.0: enum, s.1: obj) -> obj {
      switch c.0 { #0 -> b1, #1 -> b2 }
    b1:
      decref s.1
      let t.2: obj = const "none"
      jump b3(t.2)
    b2:
      jump b3(s.1)
    b3(r.3: obj):
      return r.3
    }
    "#);
}

#[test]
fn an_unused_parameter_of_a_merge_block_is_released_at_its_start() {
    let text = "\
fn ignore(c.0: enum, s.1: obj) -> int {
  switch c.0 { #0 -> b1, #1 -> b2 }
b1:
  jump b3(s.1)
b2:
  let t.2: obj = extern Prelude.++(s.1, s.1)
  jump b3(t.2)
b3(r.3: obj):
  return 0
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn ignore(c.0: enum, s.1: obj) -> int {
      switch c.0 { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(s.1)
    b2:
      dup s.1
      let t.2: obj = extern Prelude.++(s.1, s.1)
      jump b3(t.2)
    b3(r.3: obj):
      decref r.3
      return 0
    }
    ");
}

#[test]
fn an_unused_unpack_field_is_released_after_the_unpack() {
    let text = "\
fn first(p.0: obj) -> obj {
  unpack p.0 #0(a.1: obj, b.2: obj)
  return a.1
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn first(p.0: obj) -> obj {
      unpack p.0 #0(a.1: obj, b.2: obj)
      decref b.2
      return a.1
    }
    ");
}

#[test]
fn an_unpacked_value_used_later_is_dupped_before_the_unpack() {
    // `Unpack` は S3b-2a では値を消費する (docs/spec/core-ir.md)
    let text = "\
fn again(p.0: obj) -> obj {
  unpack p.0 #0(a.1: int, b.2: obj)
  let r.3: obj = con #0(p.0, b.2)
  return r.3
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn again(p.0: obj) -> obj {
      dup p.0
      unpack p.0 #0(a.1: int, b.2: obj)
      let r.3: obj = con #0(p.0, b.2)
      return r.3
    }
    ");
}

#[test]
fn a_scrutinee_used_by_a_target_is_dupped_before_the_switch() {
    // 行き先はどれも複製した分を所有して始まり、使わない行き先は入口で捨てる。使わないフィールドも入口で捨てる
    let text = "\
fn describe(xs.0: tobj) -> tobj {
  switch xs.0 { #0 -> b1, #1(h.1: obj, t.2: tobj) -> b2 }
b1:
  return xs.0
b2:
  return t.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn describe(xs.0: tobj) -> tobj {
      dup xs.0
      switch xs.0 { #0 -> b1, #1(h.1: obj, t.2: tobj) -> b2 }
    b1:
      return xs.0
    b2:
      decref xs.0
      decref h.1
      return t.2
    }
    ");
}

#[test]
fn an_unused_call_result_is_released_and_calls_save_what_is_used_later() {
    // `saved` は呼び出しの後で生きている変数から結果の変数を除いたもので、RC の対象でない `n.1` も入る
    let text = "\
fn run(s.0: obj, n.1: int) -> int {
  let r.2: obj = call echo(s.0)
  let m.3: int = extern Prelude.+(n.1, 1)
  return m.3
}
fn echo(s.0: obj) -> obj {
  return s.0
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn run(s.0: obj, n.1: int) -> int {
      let r.2: obj = call echo(s.0) save [n.1]
      decref r.2
      let m.3: int = extern Prelude.+(n.1, 1)
      return m.3
    }
    fn echo(s.0: obj) -> obj {
      return s.0
    }
    ");
}

#[test]
fn an_argument_used_after_the_call_is_dupped_and_saved() {
    let text = "\
fn around(s.0: obj) -> obj {
  let t.1: obj = call echo(s.0)
  let u.2: obj = extern Prelude.++(t.1, s.0)
  return u.2
}
fn echo(s.0: obj) -> obj {
  return s.0
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn around(s.0: obj) -> obj {
      dup s.0
      let t.1: obj = call echo(s.0) save [s.0]
      let u.2: obj = extern Prelude.++(t.1, s.0)
      return u.2
    }
    fn echo(s.0: obj) -> obj {
      return s.0
    }
    ");
}

#[test]
fn a_field_passed_to_an_arm_and_used_after_it_is_dupped_before_the_jump() {
    // case-of-case で枝のラベルへフィールドを渡し、枝の後でも使う形。`b3` は `v.2` を引数で渡し、`b5` でも使うので
    // jump の前で複製する。`b4` は `v.2` を渡さないので、所有をそのまま `b5` へ持っていく
    let text = "\
fn pair(o.0: tobj, c.1: enum) -> obj {
  switch o.0 { #0 -> b1, #1(v.2: obj) -> b2 }
b1:
  let e.3: obj = const \"none\"
  return e.3
b2:
  switch c.1 { #0 -> b3, #1 -> b4 }
b3:
  jump b5(v.2)
b4:
  let n.4: obj = const \"n\"
  jump b5(n.4)
b5(x.5: obj):
  let r.6: obj = con #0(x.5, v.2)
  return r.6
}
";
    insta::assert_snapshot!(perceus_text(text), @r#"
    fn pair(o.0: tobj, c.1: enum) -> obj {
      switch o.0 { #0 -> b1, #1(v.2: obj) -> b2 }
    b1:
      let e.3: obj = const "none"
      return e.3
    b2:
      switch c.1 { #0 -> b3, #1 -> b4 }
    b3:
      dup v.2
      jump b5(v.2)
    b4:
      let n.4: obj = const "n"
      jump b5(n.4)
    b5(x.5: obj):
      let r.6: obj = con #0(x.5, v.2)
      return r.6
    }
    "#);
}

#[test]
fn a_tail_call_that_passes_a_variable_twice_dups_it_once() {
    let text = "\
fn both(s.0: obj) -> obj {
  tail call join(s.0, s.0)
}
fn join(a.0: obj, b.1: obj) -> obj {
  let c.2: obj = extern Prelude.++(a.0, b.1)
  return c.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn both(s.0: obj) -> obj {
      dup s.0
      tail call join(s.0, s.0)
    }
    fn join(a.0: obj, b.1: obj) -> obj {
      let c.2: obj = extern Prelude.++(a.0, b.1)
      return c.2
    }
    ");
}

/// 生存解析、Perceus、verifier、テキストの表示と読み込みは、プログラムの大きさに比例して Rust のスタックを使わない
/// (docs/spec/core-ir.md の「パス」)。2万の条件の列を debug ビルドで処理し、どの条件の枝でも使わない文字列を1つずつ
/// 捨てることを確かめる。translate から通す長い列は tests/verify.rs が確かめる。
#[test]
fn twenty_thousand_conditions_are_rewritten() {
    const CONDITIONS: u32 = 20_000;
    let mut text = String::from("fn entry$main(b.0: enum, s.1: obj) -> obj {\n");
    for n in 0..CONDITIONS {
        let base = 3 * n;
        if n > 0 {
            text.push_str(&format!("b{base}:\n"));
        }
        text.push_str(&format!(
            "  switch b.0 {{ #0 -> b{}, #1 -> b{} }}\nb{}:\n  jump b{}()\nb{}:\n  let t.{}: obj = const \"x\"\n  jump b{}()\n",
            base + 1,
            base + 2,
            base + 1,
            base + 3,
            base + 2,
            n + 2,
            base + 3
        ));
    }
    text.push_str(&format!("b{}:\n  return s.1\n}}\n", 3 * CONDITIONS));
    let shown = perceus_text(&text);
    assert_eq!(shown.matches("decref t.").count(), CONDITIONS as usize);
    assert!(!shown.contains("dup"));
}

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"a\")";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn twice(s.0: obj) -> obj {
      dup s.0
      let t.1: obj = extern Prelude.++(s.0, s.0)
      return t.1
    }
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "a"
      let t.2: obj = call twice(s.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    fn entry$main() -> unit {
      tail call main(())
    }
    "#);
}

#[test]
fn shadowed_and_discarded_strings() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let s = \"x\"\n  let s = s ++ \"y\"\n  let _ = \"z\"\n  println s";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "x"
      let s.2: obj = const "y"
      let t.3: obj = extern Prelude.++(s.1, s.2)
      let t.5: unit = extern Prelude.println(t.3)
      return t.5
    }
    fn entry$main() -> unit {
      tail call main(())
    }
    "#);
}

#[test]
fn a_non_tail_if_keeps_strings_used_later() {
    let text = "pick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (pick True \"s\")";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "pick"), @r#"
    fn pick(b.0: enum, s.1: obj) -> obj {
      switch b.0 { #0 -> b1, #1 -> b2 }
    b1:
      let s.2: obj = const "none"
      jump b3(s.2)
    b2:
      dup s.1
      jump b3(s.1)
    b3(t.3: obj):
      let t.4: obj = extern Prelude.++(t.3, s.1)
      return t.4
    }
    "#);
}

#[test]
fn calls_save_the_variables_used_after_them() {
    let text = "around : Int -> String -> String\naround n s =\n  let m = n + 1\n  let t = if n > 0 then twice s else s\n  t ++ show_int m\n\ntwice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (around 1 \"s\")";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "around"), @"
    fn around(n.0: int, s.1: obj) -> obj {
      let t.2: int = extern Prelude.+(n.0, 1)
      let t.3: enum = extern Prelude.>(n.0, 0)
      switch t.3 { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(s.1)
    b2:
      let t.4: obj = call twice(s.1) save [t.2]
      jump b3(t.4)
    b3(t.5: obj):
      let t.6: obj = extern Prelude.show_int(t.2)
      let t.7: obj = extern Prelude.++(t.5, t.6)
      return t.7
    }
    ");
}

#[test]
fn constructor_arguments_are_owned_by_the_value() {
    let text = "data Pair a b =\n  | Pair a b\n\ntwice : String -> Pair String String\ntwice s = Pair s s\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = twice \"s\"\n  ()";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "twice"), @"
    fn twice(s.0: obj) -> obj {
      dup s.0
      let d.1: obj = con #0(s.0, s.0)
      return d.1
    }
    ");
}

#[test]
fn a_known_value_that_skips_its_field_releases_it() {
    // `Some s` と `Some "x"` は値を作らずに `Some _` の枝へ向かい、枝はフィールドを受け取らない。`s` はどこでも使わない
    // ので、関数の入口で捨てる
    let text = "data Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () = println (pick False True \"s\")";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "pick"), @r#"
    fn pick(a.0: enum, b.1: enum, s.2: obj) -> obj {
      decref s.2
      switch a.0 { #0 -> b1, #1 -> b2 }
    b1:
      switch b.1 { #0 -> b3, #1 -> b4 }
    b2:
      let s.4: obj = const "none"
      return s.4
    b3:
      jump b5()
    b4:
      jump b5()
    b5:
      let s.5: obj = const "some"
      return s.5
    }
    "#);
}

#[test]
fn an_unused_field_is_decreffed_when_its_arm_starts() {
    // `switch` が `o` を move で受け取り、`Some` の行き先はフィールドを所有して始まる。使わないので入口で捨てる
    let text = "data Option a = | None | Some a\n\nflag : Option String -> Int\nflag o = match o with\n  | Some _ -> 0\n  | None -> 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (flag None))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "flag"), @"
    fn flag(o.0: tobj) -> int {
      switch o.0 { #0 -> b1, #1(x.1: obj) -> b2 }
    b1:
      return 1
    b2:
      decref x.1
      return 0
    }
    ");
}

#[test]
fn a_scrutinee_used_in_an_arm_is_dupped_before_the_switch() {
    // `ys` が受ける `xs` は行き先でも使うので、`switch` の前で複製する。その参照を使わない行き先は入口で捨てる
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (describe Nil))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "describe"), @"
    fn describe(xs.0: tobj) -> int {
      dup xs.0
      switch xs.0 { #1(x.1: int, x.2: tobj) -> b1, _ -> b2 }
    b1:
      switch x.2 { #0 -> b3, _ -> b4 }
    b2:
      jump b5(xs.0)
    b3:
      decref xs.0
      return 1
    b4:
      jump b5(xs.0)
    b5(ys.3: tobj):
      tail call size(ys.3)
    }
    ");
}

#[test]
fn a_string_switch_dups_a_scrutinee_that_an_arm_uses() {
    // 文字列のリテラルは1つの `switch` で比べる。`other` の枝は scrutinee を使うので `switch` の前で複製し、使わない
    // 行き先は入口で捨てる
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = println (greet \"en\")";
    insta::assert_snapshot!(function(&core_text(text, Pass::Perceus), "greet"), @r#"
    fn greet(name.0: obj) -> obj {
      dup name.0
      switch name.0 { "en" -> b1, "ja" -> b2, _ -> b3 }
    b1:
      decref name.0
      let s.1: obj = const "hello"
      return s.1
    b2:
      decref name.0
      let s.2: obj = const "konnichiwa"
      return s.2
    b3:
      return name.0
    }
    "#);
}

#[test]
fn a_discarded_call_result_is_released() {
    // `shadowed_and_discarded_strings` の捨てた文字列は contract が消すので、呼び出しの結果を捨てる場合の解放はここで確かめる
    let text = "f : Unit -> String\nf () = \"z\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = f ()\n  println \"x\"";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn f(p.0: unit) -> obj {
      let s.1: obj = const "z"
      return s.1
    }
    fn main(p.0: unit) -> unit {
      let t.1: obj = call f(())
      decref t.1
      let s.2: obj = const "x"
      let t.3: unit = extern Prelude.println(s.2)
      return t.3
    }
    fn entry$main() -> unit {
      tail call main(())
    }
    "#);
}

#[test]
fn equations_allocate_no_tuple_for_their_arguments() {
    // 等式の脱糖が作る引数のタプルは、translate が決定木の列にするので作らない
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x y = x + y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 1 2))";
    let f = function(&core_text(text, Pass::Perceus), "f");
    assert!(!f.contains("con #"), "{f}");
}

#[test]
fn a_masked_call_keeps_its_mask_through_the_passes() {
    // 末尾呼び出しは `mask` を保つ (docs/spec/core-ir.md)
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\nrun : (Unit -> <e> a) -> <State Int | e> a\nrun cb =\n  let n = get ()\n  cb ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n =\n    handle run (fn () -> 1) from 0 with\n      | get () k st -> k st st\n      | put s k _ -> k () s\n      | return x _ -> x\n  println (show_int n)\n";
    let core = core_text(text, Pass::Perceus);
    assert!(core.contains("tail mask [State] apply cb.0(())"), "{core}");
}

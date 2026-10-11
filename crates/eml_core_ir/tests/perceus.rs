//! Perceus の `dup` / `decref` / `release` の位置と、呼び出しの `saved` (docs/spec/lir.md の「Perceus」)。ソースから
//! パイプラインを通した結果を見る。手で書いた LIR を入力にするテストと生存解析のテストは、`eml_lir` の
//! tests/perceus.rs にある。

use crate::common::{function, lir_text};

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"a\")";
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
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
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
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
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "pick"), @r#"
    fn pick(b.0: enum, s.1: obj) -> obj {
      switch b.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
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
    let text = "around : Int -> String -> String\naround n s =\n  let m = n + 1\n  let t = if n > 0 then twice s else s\n  t ++ show m\n\ntwice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (around 1 \"s\")";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "around"), @"
    fn around(n.0: int, s.1: obj) -> obj {
      let t.2: int = extern Prelude.+(n.0, 1)
      let t.3: enum = extern \"Prelude.Ord Int.>\"(n.0, 0)
      switch t.3 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(s.1)
    b2:
      let t.4: obj = call twice(s.1) save [t.2]
      jump b3(t.4)
    b3(t.5: obj):
      let t.6: obj = extern \"Prelude.Show Int.show\"(t.2)
      let t.7: obj = extern Prelude.++(t.5, t.6)
      return t.7
    }
    ");
}

#[test]
fn constructor_arguments_are_owned_by_the_value() {
    let text = "data Pair a b =\n  | Pair a b\n\ntwice : String -> Pair String String\ntwice s = Pair s s\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = twice \"s\"\n  ()";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "twice"), @"
    fn twice(s.0: obj) -> obj {
      dup s.0
      let d.1: obj = con Pair #0(s.0, s.0)
      return d.1
    }
    ");
}

#[test]
fn a_known_value_that_skips_its_field_releases_it() {
    // `Some s` と `Some "x"` は値を作らずに `Some _` の枝へ向かい、枝はフィールドを受け取らない。`s` はどこでも使わない
    // ので、関数の入口で捨てる
    let text = "data Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () = println (pick False True \"s\")";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "pick"), @r#"
    fn pick(a.0: enum, b.1: enum, s.2: obj) -> obj {
      decref s.2
      switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      switch b.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
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
fn a_dead_scrutinee_whose_fields_are_unused_is_decreffed() {
    // どの行き先も `o` を所有して始まる。フィールドを使わないので、`release` ではなく `decref` で手放す
    let text = "data Option a = | None | Some a\n\nflag : Option String -> Int\nflag o = match o with\n  | Some _ -> 0\n  | None -> 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (flag None))";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "flag"), @"
    fn flag(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
    b1:
      decref o.0
      return 1
    b2:
      decref o.0
      return 0
    }
    ");
}

#[test]
fn a_default_target_owns_the_scrutinee_without_a_dup() {
    // `ys` が受ける `xs` は `switch` の前で複製しない。入れ子のパターンでは、外側の `xs` が生きているので、内側の
    // scrutinee の `x.2` を複製してから読む
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = println (show (describe Nil))";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "describe"), @"
    fn describe(xs.0: tobj) -> int {
      switch xs.0 List { #1(x.5: tobj, x.2: tobj) -> b1, _ -> b2 }
    b1:
      dup x.2
      switch x.2 List { #0 -> b3, _ -> b4 }
    b2:
      jump b5(xs.0)
    b3:
      decref xs.0
      decref x.2
      return 1
    b4:
      decref x.2
      jump b5(xs.0)
    b5(ys.3: tobj):
      tail call size(ys.3)
    }
    ");
}

#[test]
fn a_string_switch_leaves_the_scrutinee_to_its_targets() {
    // 文字列のリテラルは1つの `switch` で比べる。`switch` の前で複製せず、`other` の枝は scrutinee をそのまま返し、
    // ほかの行き先は入口で手放す
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = println (greet \"en\")";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "greet"), @r#"
    fn greet(name.0: obj) -> obj {
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
fn a_nested_pattern_gives_up_the_parent_before_the_release() {
    // 内側の `x.1` を読むとき、外側の `w` はまだ生きているので、`x.1` を複製する。`Cons (Cons a _) _` の行き先では、
    // 先に `w` を手放してから `x.1` を `release` する。`x.1` の参照が1つに戻り、`release` が一意の側を通る
    let text = "data List a =\n  | Nil\n  | Cons a (List a)\n\nlength : List a -> Int\nlength xs = match xs with\n  | Nil -> 0\n  | Cons _ rest -> 1 + length rest\n\ndescribe : List (List String) -> String\ndescribe w = match w with\n  | Cons (Cons a _) _ -> a\n  | Cons Nil _ -> show (length w)\n  | Nil -> \"empty\"\n\nmain : Unit -> <IO> Unit\nmain () = println (describe Nil)";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "describe"), @r#"
    fn describe(w.0: tobj) -> obj {
      switch w.0 List { #0 -> b1, #1(x.1: tobj, x.2: tobj) -> b2 }
    b1:
      decref w.0
      let s.7: obj = const "empty"
      return s.7
    b2:
      dup x.1
      switch x.1 List { #0 -> b3, #1(a.3: obj, x.4: tobj) -> b4 }
    b3:
      decref x.1
      let t.5: int = call "length@[Main.List String]"(w.0)
      let t.6: obj = extern "Prelude.Show Int.show"(t.5)
      return t.6
    b4:
      decref w.0
      release x.1 List #1(a.3, _)
      return a.3
    }
    "#);
}

#[test]
fn a_file_taken_out_of_a_tuple_is_neither_dupped_nor_decreffed() {
    // `File` は `Lin` なので、組を分解した所で `release` が両方のフィールドを残し、その後は1回ずつ使う
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"input.txt\"\n  let (f, first) = Fs.read_all f\n  let (f, rest) = Fs.read_all f\n  Fs.close f\n  println first\n  println (\"[\" ++ rest ++ \"]\")";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "main"), @r#"
    fn main(p.0: unit) -> unit {
      let s.1: obj = const "input.txt"
      let t.2: obj = extern Std.Fs.open(s.1)
      let t.3: obj = extern Std.Fs.read_all(t.2)
      unpack t.3 (,) #0(f.4: obj, first.5: obj)
      release t.3 (,) #0(f.4, first.5)
      let t.6: obj = extern Std.Fs.read_all(f.4)
      unpack t.6 (,) #0(f.7: obj, rest.8: obj)
      release t.6 (,) #0(f.7, rest.8)
      let t.9: unit = extern Std.Fs.close(f.7)
      let t.10: unit = extern Prelude.println(first.5)
      let s.11: obj = const "["
      let s.12: obj = const "]"
      let t.13: obj = extern Prelude.++(rest.8, s.12)
      let t.14: obj = extern Prelude.++(s.11, t.13)
      let t.15: unit = extern Prelude.println(t.14)
      return t.15
    }
    "#);
}

#[test]
fn a_discarded_call_result_is_released() {
    // `shadowed_and_discarded_strings` の捨てた文字列は contract が消すので、呼び出しの結果を捨てる場合の解放はここで確かめる
    let text = "f : Unit -> String\nf () = \"z\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = f ()\n  println \"x\"";
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
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
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x y = x + y\n\nmain : Unit -> <IO> Unit\nmain () = println (show (f 1 2))";
    let f = function(&lir_text(text, eml_lir::Pass::Perceus), "f");
    assert!(!f.contains("con #"), "{f}");
}

/// 補間は穴を評価するたびにつなぐので、穴の呼び出しをまたいで退避するのは組み立て中の文字列だけである
/// (docs/spec/core-ir.md の「変換の規則」)。`n` を最初の穴でだけ使い、ほかの穴の引数を定数にして、組み立て中の
/// 文字列のほかに生きている変数をなくす。
#[test]
fn interpolation_saves_only_the_accumulator_across_holes() {
    let text = "g : Int -> <IO> Int\ng n =\n  println \"g\"\n  n\n\nf : Int -> <IO> String\nf n = \"a\\{g n}b\\{g 2}c\\{g 3}\"\n\nmain : Unit -> <IO> Unit\nmain () = println (f 1)";
    let shown = function(&lir_text(text, eml_lir::Pass::Perceus), "f");
    let lines: Vec<&str> = shown.lines().collect();
    let mut saves = 0;
    for (i, line) in lines.iter().enumerate() {
        let Some((_, saved)) = line.split_once(" save [") else {
            continue;
        };
        saves += 1;
        // 退避した変数が、穴の値をつなぐ次の `++` の左の引数 (組み立て中の文字列) であることを確かめる
        let accumulator = lines[i..]
            .iter()
            .find_map(|line| line.split_once("extern Prelude.++("))
            .and_then(|(_, args)| args.split_once(", "))
            .map(|(left, _)| left);
        assert_eq!(Some(saved.trim_end_matches(']')), accumulator, "{shown}");
    }
    assert_eq!(saves, 6, "two calls per hole: {shown}");
}

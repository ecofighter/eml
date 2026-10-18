//! Perceus の `dup` / `decref` / `release` の位置 (docs/spec/lir.md の「Perceus」)。ソースからパイプラインを通した
//! 結果を見る。手で書いた LIR を入力にするテストと生存解析のテストは、`eml_lir` の
//! tests/perceus.rs にある。

use eml_extern::Extern;
use eml_lir::liveness::{insert_var, live_after_term, live_in, step_back};
use eml_lir::{Atom, Rhs, Stmt, VarId};

use crate::common::{function, lir_text};

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ntwice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box twice = hide 0 twice\n  println (twice \"a\")";
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
    layout Prelude.Bool { False, True }
    layout Box { Box(tobj) }
    effect Prelude.IO { never io_failed(enum, obj) -> never }
    fn twice(s.0: obj) -> obj {
      dup s.0
      let t.1: obj = extern Prelude.++(s.0, s.0)
      return t.1
    }
    fn entry$main() -> unit {
      let t.1: obj = call "hide@[String -> String]{1=twice}"(0)
      unpack t.1 Box #0(twice.2: tobj)
      let s.3: obj = const "a"
      release t.1 Box #0(twice.2)
      let t.4: obj = apply twice.2(s.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    fn "hide@[String -> String]{1=twice}"(n.0: int) -> obj {
      let t.2: enum = extern "Prelude.Ord Int.<="(n.0, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.4: int = extern Prelude.-(n.0, 1)
      tail call "hide@[String -> String]{1=twice}"(t.4)
    b2:
      let d.3: obj = con Box #0(&twice)
      return d.3
    }
    "#);
}

#[test]
fn shadowed_and_discarded_strings() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let s = \"x\"\n  let s = s ++ \"y\"\n  let _ = \"z\"\n  println s";
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
    effect Prelude.IO { never io_failed(enum, obj) -> never }
    fn entry$main() -> unit {
      let s.1: obj = const "x"
      let s.2: obj = const "y"
      let t.3: obj = extern Prelude.++(s.1, s.2)
      let t.4: unit = extern Prelude.println(t.3)
      return t.4
    }
    "#);
}

#[test]
fn a_non_tail_if_keeps_strings_used_later() {
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\npick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box pick = hide 0 pick\n  println (pick True \"s\")";
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
fn constructor_arguments_are_owned_by_the_value() {
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata Pair a b =\n  | Pair a b\n\ntwice : String -> Pair String String\ntwice s = Pair s s\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box twice = hide 0 twice\n  let _ = twice \"s\"\n  ()";
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
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box pick = hide 0 pick\n  println (pick False True \"s\")";
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
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata Option a = | None | Some a\n\nflag : Option String -> Int\nflag o = match o with\n  | Some _ -> 0\n  | None -> 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box flag = hide 0 flag\n  println (show (flag None))";
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
    // `ys` が受ける `xs` は `switch` の前で複製しない。入れ子のパターンの内側の scrutinee `x.2` は、借りたまま読む
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = match xs with\n  | Nil -> 2\n  | Cons _ rest -> size rest\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box describe = hide 0 describe\n  println (show (describe Nil))";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "describe"), @"
    fn describe(xs.0: tobj) -> int {
      switch xs.0 List { #1(x.5: tobj, x.2: tobj) -> b1, _ -> b2 }
    b1:
      switch x.2 List { #0 -> b3, _ -> b4 }
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
fn a_string_switch_leaves_the_scrutinee_to_its_targets() {
    // 文字列のリテラルは1つの `switch` で比べる。`switch` の前で複製せず、`other` の枝は scrutinee をそのまま返し、
    // ほかの行き先は入口で手放す
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ngreet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box greet = hide 0 greet\n  println (greet \"en\")";
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
fn a_nested_pattern_releases_from_the_parent_down() {
    // 内側の `x.1` は借りたまま読むので、複製しない。`Cons (Cons a _) _` の行き先では、`a` を返す所で、先に `w` を
    // `release` して `x.1` を受け取り、次に `x.1` を `release` する。上の物体を先に一意にするので、`x.1` の
    // `release` も一意の側を通る
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata List a =\n  | Nil\n  | Cons a (List a)\n\nlength : List a -> Int\nlength xs = match xs with\n  | Nil -> 0\n  | Cons _ rest -> 1 + length rest\n\ndescribe : List (List String) -> String\ndescribe w = match w with\n  | Cons (Cons a _) _ -> a\n  | Cons Nil _ -> show (length w)\n  | Nil -> \"empty\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box describe = hide 0 describe\n  println (describe Nil)";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "describe"), @r#"
    fn describe(w.0: tobj) -> obj {
      switch w.0 List { #0 -> b1, #1(x.1: tobj, x.2: tobj) -> b2 }
    b1:
      decref w.0
      let s.7: obj = const "empty"
      return s.7
    b2:
      switch x.1 List { #0 -> b3, #1(a.3: obj, x.4: tobj) -> b4 }
    b3:
      let t.5: int = call "length@[Main.List String]"(w.0)
      let t.6: obj = extern "Prelude.Show Int.show"(t.5)
      return t.6
    b4:
      release w.0 List #1(x.1, _)
      release x.1 List #1(a.3, _)
      return a.3
    }
    "#);
}

#[test]
fn a_file_taken_out_of_a_tuple_is_neither_dupped_nor_decreffed() {
    // `File` は `Lin` なので、組を分解した所で `release` が両方のフィールドを残し、その後は1回ずつ使う
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"input.txt\"\n  let (f, first) = Fs.read_all f\n  let (f, rest) = Fs.read_all f\n  Fs.close f\n  println first\n  println (\"[\" ++ rest ++ \"]\")";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "entry$main"), @r#"
    fn entry$main() -> unit {
      let s.1: obj = const "input.txt"
      let t.2: obj lin = extern Std.Fs.open(s.1)
      let t.3: obj lin = extern Std.Fs.read_all(t.2)
      unpack t.3 (,) #0(f.4: obj lin, first.5: obj)
      release t.3 (,) #0(f.4, first.5)
      let t.6: obj lin = extern Std.Fs.read_all(f.4)
      unpack t.6 (,) #0(f.7: obj lin, rest.8: obj)
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
    // `f` は自分を呼ぶので展開されず、捨てる結果を返す呼び出しとして残る
    // (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「インライン化」)
    let text = "f : Int -> String\nf n = if n > 0 then f (n - 1) else \"z\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = f 0\n  println \"x\"";
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
    layout Prelude.Bool { False, True }
    effect Prelude.IO { never io_failed(enum, obj) -> never }
    fn f(n.0: int) -> obj {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.4: obj = const "z"
      return s.4
    b2:
      let t.2: int = extern Prelude.-(n.0, 1)
      tail call f(t.2)
    }
    fn entry$main() -> unit {
      let t.1: obj = call f(0)
      decref t.1
      let s.2: obj = const "x"
      let t.3: unit = extern Prelude.println(s.2)
      return t.3
    }
    "#);
}

#[test]
fn equations_allocate_no_tuple_for_their_arguments() {
    // 等式の脱糖が作る引数のタプルは、translate が決定木の列にするので作らない
    let text = "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\nf : Int -> Int -> Int\nf 0 y = y\nf x y = x + y\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box f = hide 0 f\n  println (show (f 1 2))";
    let f = function(&lir_text(text, eml_lir::Pass::Perceus), "f");
    assert!(!f.contains("con #"), "{f}");
}

/// 補間は穴を評価するたびにつなぐので、穴の呼び出しをまたいで生きているのは組み立て中の文字列だけである
/// (docs/spec/core-ir.md の「変換の規則」)。`n` を最初の穴でだけ使い、ほかの穴の引数を定数にして、組み立て中の
/// 文字列のほかに生きている変数をなくす。生存は、VM のスロットの割り当てと同じく、Perceus の後の LIR で文の単位に
/// 求める。
#[test]
fn interpolation_keeps_only_the_accumulator_live_across_holes() {
    // `g` は自分を呼ぶので展開されず、穴ごとの呼び出しとして残る。`f` はデータを通して呼び、`main` に展開されないように
    // する (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「インライン化」)
    let text = "g : Int -> Int\ng n = if n > 9 then g (n - 1) else n + 1\n\nf : Int -> String\nf n = \"a\\{g n}b\\{g 2}c\\{g 3}\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box f = hide 0 f\n  println (f 1)";
    let text = format!(
        "data Box a = | Box a\n\nhide : Int -> a -> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\n{text}"
    );
    let program = eml_test_support::lir(&text);
    let shown = function(&eml_lir::pretty(&program), "f");
    let f = program
        .functions
        .iter()
        .find(|function| function.name == "f")
        .expect("a function `f`");
    let live_in = live_in(f);
    let mut calls = 0;
    for block in &f.blocks {
        let mut live = live_after_term(f, &block.term, &live_in);
        block.term.for_each_atom(|atom| insert_var(&mut live, atom));
        // 後ろからたどるので、呼び出しに着いたときの `accumulator` は、その後で穴の値をつなぐ `++` の左の引数
        // (組み立て中の文字列) である
        let mut accumulator = None;
        for stmt in block.stmts.iter().rev() {
            match stmt {
                Stmt::Let {
                    var: _,
                    rhs:
                        Rhs::Extern {
                            ext: Extern::StrConcat,
                            args,
                            at: _,
                        },
                } => {
                    accumulator = match args[0] {
                        Atom::Var(left) => Some(left),
                        _ => None,
                    }
                }
                Stmt::Let {
                    var,
                    rhs: Rhs::Call(_),
                } => {
                    calls += 1;
                    let others: Vec<VarId> = live.iter().copied().filter(|v| v != var).collect();
                    assert_eq!(others, Vec::from_iter(accumulator), "{shown}");
                }
                _ => {}
            }
            step_back(stmt, &mut live);
        }
    }
    // 穴の `display` は小さいので展開されて extern の `show` になり、呼び出しは穴ごとに `g` の1つだけ残る
    assert_eq!(calls, 3, "one call per hole: {shown}");
}

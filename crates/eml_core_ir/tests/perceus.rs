//! Perceus の `dup` / `decref` / `release` の位置 (docs/spec/lir.md の「Perceus」)。ソースからパイプラインを通した
//! 結果を見る。手で書いた LIR を入力にするテストと生存解析のテストは、`eml_lir` の
//! tests/perceus.rs にある。

use eml_extern::Extern;
use eml_lir::liveness::{insert_var, live_after_term, live_in, step_back};
use eml_lir::{Atom, Rhs, Stmt, VarId};

use crate::common::{function, lir_text};

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ntwice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box twice = hide 0 twice\n  println (twice \"a\")";
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
    layout Box { Box(tobj) }
    layout Prelude.Bool { False, True }
    effect Prelude.IO { never io_failed(enum, obj) -> never }
    effect Prelude.Panic { never panicked(obj) -> never }
    fn twice(s.0: obj) -> obj {
      dup s.0
      let t.1: obj = extern Prelude.++(s.0, s.0)
      return t.1
    }
    fn entry$main() -> unit {
      let t.0.6: tobj = call "hide@[String -> String]{1=twice}"(0)
      let s.3: obj = const "a"
      let t.4: obj = apply t.0.6(s.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    fn "hide@[String -> String]{1=twice}"(n.0: int) -> tobj {
      let t.2: enum = extern "Prelude.Ord Int.<="(n.0, 0)
      switch t.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.4: int = extern "Prelude.Num Int.-"(n.0, 1)
      tail call "hide@[String -> String]{1=twice}"(t.4)
    b2:
      return &twice
    }
    "#);
}

#[test]
fn shadowed_and_discarded_strings() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let s = \"x\"\n  let s = s ++ \"y\"\n  let _ = \"z\"\n  println s";
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
    effect Prelude.IO { never io_failed(enum, obj) -> never }
    effect Prelude.Panic { never panicked(obj) -> never }
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
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\npick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box pick = hide 0 pick\n  println (pick True \"s\")";
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
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata Pair a b =\n  | Pair a b\n\ntwice : String -> Pair String String\ntwice s = Pair s s\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box twice = hide 0 twice\n  let _ = twice \"s\"\n  ()";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "twice"), @"
    fn twice(s.0: obj) -> (obj, obj) {
      dup s.0
      return (s.0, s.0)
    }
    ");
}

#[test]
fn a_known_value_that_skips_its_field_releases_it() {
    // `Some s`と`Some "x"`は値を作らずに`Some _`の枝へ向かい、枝はフィールドを受け取らない。`s`を使うのは`None`の
    // 枝だけなので、ほかの行き先はエントリで捨てる
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> s\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box pick = hide 0 pick\n  println (pick False True \"s\")";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "pick"), @r#"
    fn pick(a.0: enum, b.1: enum, s.2: obj) -> obj {
      switch a.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      decref s.2
      switch b.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b2:
      return s.2
    b3:
      jump b5()
    b4:
      jump b5()
    b5:
      let s.4: obj = const "some"
      return s.4
    }
    "#);
}

#[test]
fn a_dead_scrutinee_whose_fields_are_unused_is_decreffed() {
    // どの行き先も `o` を所有して始まる。フィールドを使わないので、`release` ではなく `decref` で手放す
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata Option a = | None | Some a\n\nflag : Option String -> Option String\nflag o = match o with\n  | Some _ -> None\n  | None -> o\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box flag = hide 0 flag\n  let _ = flag None\n  ()";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "flag"), @"
    fn flag(o.0: tobj) -> tobj {
      switch o.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
    b1:
      return o.0
    b2:
      decref o.0
      return #0
    }
    ");
}

#[test]
fn a_default_target_owns_the_scrutinee_without_a_dup() {
    // `ys` が受ける `xs` は `switch` の前で複製しない。入れ子のパターンの内側の scrutinee `x.2` は、借りたまま読む
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata List a = | Nil | Cons a (List a)\n\ndescribe : List Int -> List Int\ndescribe xs = match xs with\n  | Cons _ Nil -> Nil\n  | ys -> Cons 0 ys\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box describe = hide 0 describe\n  let _ = describe Nil\n  ()";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "describe"), @"
    fn describe(xs.0: tobj) -> tobj {
      switch xs.0 List { #1(x.5: tobj, x.2: tobj) -> b1, _ -> b2 }
    b1:
      switch x.2 List { #0 -> b3, _ -> b4 }
    b2:
      jump b5(xs.0)
    b3:
      decref xs.0
      return #0
    b4:
      jump b5(xs.0)
    b5(ys.3: tobj):
      let b.6: tobj = box 0
      let d.4: tobj = con List #1(b.6, ys.3)
      return d.4
    }
    ");
}

#[test]
fn a_string_switch_leaves_the_scrutinee_to_its_targets() {
    // 文字列のリテラルは1つの `switch` で比べる。`switch` の前で複製せず、`other` の枝は scrutinee をそのまま返し、
    // ほかの行き先はエントリで手放す
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ngreet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box greet = hide 0 greet\n  println (greet \"en\")";
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
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\ndata List a =\n  | Nil\n  | Cons a (List a)\n\nlength : List a -> <Panic> Int\nlength xs = match xs with\n  | Nil -> 0\n  | Cons _ rest -> 1 + length rest\n\ndescribe : List (List String) -> <Panic> String\ndescribe w = match w with\n  | Cons (Cons a _) _ -> a\n  | Cons Nil _ -> show (length w)\n  | Nil -> \"empty\"\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box describe = hide 0 describe\n  println (describe Nil)";
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
      decref w.0
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
    // 読みは `FileIn` を借りて結果と `FileIn` の組を返し、組は最適化で消える。`FileIn` は `Lin` なので、`dup` も `decref` も
    // 入らずに、借りる読みの後で `close` に1回だけ渡る
    let text = "import FileIn\nmain : Unit -> <IO> Unit\nmain () =\n  let f = FileIn.open \"input.txt\"\n  let (first, f) = FileIn.read_all f\n  let (rest, f) = FileIn.read_all f\n  FileIn.close f\n  println first\n  println (\"[\" ++ rest ++ \"]\")";
    insta::assert_snapshot!(function(&lir_text(text, eml_lir::Pass::Perceus), "entry$main"), @r#"
    fn entry$main() -> unit {
      let s.1: obj = const "input.txt"
      let t.2: obj lin = extern Std.FileIn.open(s.1)
      let t.3: obj = extern Std.FileIn.read_all(t.2)
      let t.4: obj = extern Std.FileIn.read_all(t.2)
      let t.5: unit = extern Std.FileIn.close(t.2)
      let t.6: unit = extern Prelude.println(t.3)
      let s.7: obj = const "["
      let s.8: obj = const "]"
      let t.9: obj = extern Prelude.++(t.4, s.8)
      let t.10: obj = extern Prelude.++(s.7, t.9)
      let t.11: unit = extern Prelude.println(t.10)
      return t.11
    }
    "#);
}

#[test]
fn a_discarded_call_result_is_released() {
    // `shadowed_and_discarded_strings` の捨てた文字列は contract が消すので、呼び出しの結果を捨てる場合の解放はここで確かめる
    // `f` は自分を呼ぶので展開されず、捨てる結果を返す呼び出しとして残る
    // (docs/spec/core-ir.md の「インライン化」)
    let text = "f : Int -> <Panic> String\nf n = if n > 0 then f (n - 1) else \"z\"\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let _ = f 0\n  println \"x\"";
    insta::assert_snapshot!(lir_text(text, eml_lir::Pass::Perceus), @r#"
    layout Prelude.Bool { False, True }
    effect Prelude.IO { never io_failed(enum, obj) -> never }
    effect Prelude.Panic { never panicked(obj) -> never }
    fn f(n.0: int) -> obj {
      let t.1: enum = extern "Prelude.Ord Int.>"(n.0, 0)
      switch t.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let s.4: obj = const "z"
      return s.4
    b2:
      let t.2: int = extern "Prelude.Num Int.-"(n.0, 1)
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
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\nf : Int -> Int -> <Panic> Int\nf 0 y = y\nf x y = x + y\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 f\n  println (show (f 1 2))";
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
    // する (docs/spec/core-ir.md の「インライン化」)
    let text = "g : Int -> <Panic> Int\ng n = if n > 9 then g (n - 1) else n + 1\n\nf : Int -> <Panic> String\nf n = \"a\\{g n}b\\{g 2}c\\{g 3}\"\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 f\n  println (f 1)";
    let text = format!(
        "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\n{text}"
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
                    rhs: Rhs::Extern { ext, args, at: _ },
                } if ext.std() == Some(Extern::StrConcat) => {
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

const HIDE: &str = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\n";

#[test]
fn readers_of_a_string_that_the_caller_keeps_borrow_it_and_hold_no_rc() {
    // bench/words_loop.emの形。呼ばれる側は文字列を読むだけなので、3つとも借りる。借りたパラメータには`dup`も
    // `decref`も要らない。持ち主は`main`で、`grow`の結果を最後に1回だけ手放す
    let text = "import Byte\nimport String\n\ngrow : Int -> String -> <Panic> String\ngrow n acc = if n == 0 then acc else grow (n - 1) (acc ++ \" ab\")\n\nskip_spaces : String -> Int -> <Panic> Int\nskip_spaces s i =\n  if i >= String.byte_length s then i\n  else if String.byte_at i s == ' 'u8 then skip_spaces s (i + 1) else i\n\nskip_word : String -> Int -> <Panic> Int\nskip_word s i =\n  if i >= String.byte_length s then i\n  else if String.byte_at i s == ' 'u8 then i else skip_word s (i + 1)\n\ncount : String -> Int -> Int -> <Panic> Int\ncount s i acc =\n  if i >= String.byte_length s then acc\n  else count s (skip_word s (skip_spaces s i)) (acc + 1)\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let s = grow 30000 \"\"\n  println (show (count s 0 0))";
    let shown = lir_text(text, eml_lir::Pass::Perceus);
    for name in ["skip_spaces", "skip_word", "count"] {
        let reader = function(&shown, name);
        assert!(reader.contains("(borrowed s.0: obj"), "{reader}");
        for rc in ["dup ", "decref ", "release "] {
            assert!(!reader.contains(rc), "{reader}");
        }
    }
    insta::assert_snapshot!(function(&shown, "count"), @r#"
    fn count(borrowed s.0: obj, i.1: int, acc.2: int) -> int {
      let t.3: int = extern Std.String.byte_length(s.0)
      let t.4: enum = extern "Prelude.Ord Int.>="(i.1, t.3)
      switch t.4 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let t.5: int = call skip_spaces(s.0, i.1)
      let t.6: int = call skip_word(s.0, t.5)
      let t.7: int = extern "Prelude.Num Int.+"(acc.2, 1)
      tail call count(s.0, t.6, t.7)
    b2:
      return acc.2
    }
    "#);
    insta::assert_snapshot!(function(&shown, "entry$main"), @r#"
    fn entry$main() -> unit {
      let s.1: obj = const ""
      let t.2: obj = call grow(30000, s.1)
      let t.3: int = call count(t.2, 0, 0)
      decref t.2
      let t.4: obj = extern "Prelude.Show Int.show"(t.3)
      let t.5: unit = extern Prelude.println(t.4)
      return t.5
    }
    "#);
}

#[test]
fn a_field_passed_to_a_reader_needs_no_dup_but_one_passed_to_a_consumer_does() {
    // `p.name`の後で`p`をまた使う。`len`は`s`を読むだけなので、フィールドは`p`から借りたまま渡し、`dup`は
    // 入らない。`keep`は`s`をデータに入れて消費するので、渡す前に1回`dup`する
    let text = format!(
        "import String\n\n{HIDE}data Person = | Person String Int\n\nlen : String -> Int -> <Panic> Int\nlen s i = if i >= String.byte_length s then i else len s (i + 1)\n\ndata Held = | Held String\n\nkeep : String -> Int -> <Panic> Held\nkeep s i = if i > 0 then keep s (i - 1) else Held s\n\nread_then_use : Person -> <Panic> Int\nread_then_use p = match p with\n  | Person name age -> len name 0 + String.byte_length name + age\n\nconsume_then_use : Person -> <Panic> Int\nconsume_then_use p = match p with\n  | Person name age -> let _ = keep name 0 in String.byte_length name + age\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box a = hide 0 read_then_use\n  let Box b = hide 0 consume_then_use\n  println (show (a (Person \"x\" 1) + b (Person \"y\" 2)))"
    );
    let shown = lir_text(&text, eml_lir::Pass::Perceus);
    let reader = function(&shown, "read_then_use");
    assert!(
        reader.starts_with("fn read_then_use(borrowed p.0."),
        "{reader}"
    );
    assert!(!reader.contains("dup "), "{reader}");
    let consumer = function(&shown, "consume_then_use");
    assert_eq!(consumer.matches("dup p.0.").count(), 1, "{consumer}");
    insta::assert_snapshot!(reader, @r#"
    fn read_then_use(borrowed p.0.7: obj, p.1.8: int) -> int {
      let t.3: int = call len(p.0.7, 0)
      let t.4: int = extern Std.String.byte_length(p.0.7)
      let t.5: int = extern "Prelude.Num Int.+"(t.3, t.4)
      let t.6: int = extern "Prelude.Num Int.+"(t.5, p.1.8)
      return t.6
    }
    "#);
    insta::assert_snapshot!(consumer, @r#"
    fn consume_then_use(p.0.6: obj, p.1.7: int) -> int {
      dup p.0.6
      let t.0.8: obj = call keep(p.0.6, 0)
      decref t.0.8
      let t.4: int = extern Std.String.byte_length(p.0.6)
      decref p.0.6
      let t.5: int = extern "Prelude.Num Int.+"(t.4, p.1.7)
      return t.5
    }
    "#);
}

#[test]
fn a_merged_string_that_is_only_read_is_borrowed_through_the_merge() {
    // `u`は`s`か`t`で、読むだけである。`s`も`t`も呼ぶ側が持ち続けるので、どちらも借り、合流の引数`u`も
    // 借りる。借りた2つの値が渡る合流には、`dup`も`decref`も入らない
    let text = format!(
        "import String\n\n{HIDE}pick_len : Bool -> String -> String -> Int\npick_len c s t =\n  let u = if c then s else t\n  String.byte_length u\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 pick_len\n  println (show (f True \"s\" \"t\"))"
    );
    let shown = lir_text(&text, eml_lir::Pass::Perceus);
    let pick_len = function(&shown, "pick_len");
    assert!(
        pick_len.starts_with("fn pick_len(c.0: enum, borrowed s.1: obj, borrowed t.2: obj)"),
        "{pick_len}"
    );
    assert!(pick_len.contains("b3(borrowed "), "{pick_len}");
    for rc in ["dup ", "decref ", "release "] {
        assert!(!pick_len.contains(rc), "{pick_len}");
    }
    insta::assert_snapshot!(pick_len, @"
    fn pick_len(c.0: enum, borrowed s.1: obj, borrowed t.2: obj) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(t.2)
    b2:
      jump b3(s.1)
    b3(borrowed t.3: obj):
      let t.4: int = extern Std.String.byte_length(t.3)
      return t.4
    }
    ");
}

#[test]
fn a_parameter_live_across_a_yield_point_is_owned_and_moved_into_the_continuation() {
    // `s`は`yield_now`の後でも読むので、待ちをまたいで生きる。継続の閉包が捕まえることは消費する位置なので、
    // `s`は借りられず、閉包へ`dup`なしで移る (docs/spec/lir.mdの「借用の推論」)
    let text = format!(
        "import Async (Async)\nimport String\n\n{HIDE}pause_then_read : String -> <IO, Async> Int\npause_then_read s =\n  Async.yield_now ()\n  String.byte_length s\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 pause_then_read\n  println (show (Async.run (fn () -> f \"s\")))"
    );
    let shown = lir_text(&text, eml_lir::Pass::Perceus);
    let pause = function(&shown, "pause_then_read");
    assert!(
        pause.starts_with("yields fn pause_then_read(s.0: obj)"),
        "{pause}"
    );
    let captures: Vec<&str> = pause
        .lines()
        .filter(|line| line.contains("= closure pause_then_read$k"))
        .collect();
    assert!(!captures.is_empty(), "{pause}");
    for capture in captures {
        assert!(capture.contains("(s.0"), "{capture}");
    }
    assert!(!pause.contains("dup s."), "{pause}");
    for header in shown
        .lines()
        .filter(|line| line.contains(" fn pause_then_read$k"))
    {
        assert!(!header.contains("borrowed"), "{header}");
    }
    insta::assert_snapshot!(pause, @r#"
    yields fn pause_then_read(s.0: obj) -> int {
      let ev.6: obj = prim ev_lookup[Std.Async.Async]()
      let c.7: tobj = prim ev_clause[Std.Async.Async.reschedule](ev.6)
      let t.3: enum = clause Std.Async.Async.reschedule c.7(ev.6, ())
      let y.8: int = prim yielding()
      switch y.8 { 0 -> b2, _ -> b1 }
    b1:
      let k.9: tobj poly = prim yield_cont_take()
      let c.10: tobj = closure pause_then_read$k0(s.0)
      let k.11: tobj poly = closure compose$(k.9, c.10)
      let u.12: unit = prim yield_cont_set(k.11)
      return 0
    b2:
      switch t.3 Std.Async.Wake { #0 -> b3, #1 -> b4 }
    b3:
      jump b5(())
    b4:
      let s.4: obj = const "task was cancelled"
      let t.5: unit = extern Prelude.fail_io(#5, s.4)
      jump b5(t.5)
    b5(t.1: unit):
      let t.2: int = extern Std.String.byte_length(s.0)
      decref s.0
      return t.2
    }
    "#);
}

#[test]
fn map_over_an_int_list_keeps_its_reuse() {
    // 借用の推論は、再利用の元になる引数を借りない。`xs`は`Cons`で分解して消費し、新しい`Cons`が同じ札を
    // 持ち回る
    let text = format!(
        "{HIDE}data List a = | Nil | Cons a (List a)\n\nmap : (Int -> Int) -> List Int -> <Panic> List Int\nmap f xs = match xs with\n  | Nil -> Nil\n  | Cons x rest -> Cons (f x) (map f rest)\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box m = hide 0 map\n  let _ = m (fn x -> x) (Cons 1 Nil)\n  ()"
    );
    let shown = lir_text(&text, eml_lir::Pass::Reuse);
    let map = function(&shown, "map");
    assert!(map.contains("reuse "), "{map}");
    assert!(!map.contains("borrowed xs"), "{map}");
    insta::assert_snapshot!(map, @"
    fn map(f.0: tobj, xs.1: tobj) -> tobj {
      switch xs.1 List { #0 -> b1, #1(x.7: tobj, rest.3: tobj) -> b2 }
    b1:
      decref f.0
      decref xs.1
      return #0
    b2:
      let r.9: token = release xs.1 List #1(x.7, rest.3)
      dup f.0
      let t.8: tobj = apply f.0(x.7)
      let t.5: tobj = call map(f.0, rest.3)
      let d.6: tobj = con List #1(t.8, t.5) reuse r.9 in place
      return d.6
    }
    ");
}

#[test]
fn a_function_returning_a_scalar_field_of_a_list_node_borrows_the_list() {
    // リストは先頭の`Int`だけを読む。スカラーのフィールドを消費してもルートは所有にならないので、リストの
    // パラメータは借り、関数にRCの命令は入らない。`main`はリストをその後も使う
    let text = format!(
        "{HIDE}data List a = | Nil | Cons a (List a)\n\nhead_or_zero : List Int -> Int\nhead_or_zero xs = match xs with\n  | Cons x _ -> x\n  | Nil -> 0\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 head_or_zero\n  let xs = Cons 1 (Cons 2 Nil)\n  println (show (f xs))\n  println (show (f xs))"
    );
    let shown = lir_text(&text, eml_lir::Pass::Perceus);
    let head = function(&shown, "head_or_zero");
    assert!(
        head.starts_with("fn head_or_zero(borrowed xs.0: tobj)"),
        "{head}"
    );
    for rc in ["dup ", "decref ", "release "] {
        assert!(!head.contains(rc), "{head}");
    }
    insta::assert_snapshot!(head, @"
    fn head_or_zero(borrowed xs.0: tobj) -> int {
      switch xs.0 List { #0 -> b1, #1(x.3: tobj, x.2: tobj) -> b2 }
    b1:
      return 0
    b2:
      let x.1: int = unbox x.3
      return x.1
    }
    ");
}

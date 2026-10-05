//! Perceus の `dup` / `decref` の位置と、呼び出しの `saved` (docs/spec/core-ir.md)。

mod common;

use common::core_text;
use eml_core_ir::Pass;

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"a\")";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
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
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn main(p0) {
      let s1 = const "x"
      let s2 = const "y"
      let t3 = prim ++(s1, s2)
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
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
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
fn calls_save_the_variables_used_after_them() {
    let text = "around : Int -> String -> String\naround n s =\n  let m = n + 1\n  let t = if n > 0 then twice s else s\n  t ++ show_int m\n\ntwice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r"
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
fn constructor_arguments_are_owned_by_the_value() {
    let text = "data Pair a b =\n  | Pair a b\n\ntwice : String -> Pair String String\ntwice s = Pair s s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r"
    fn twice(s0) {
      dup s0
      let d1 = con #0(s0, s0)
      return d1
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
fn a_split_arm_with_fields_still_drops_its_unused_field() {
    // B2 が切り出した `Some _` の枝は、フィールド `x10` を引数に受けて始まり、使わないので入口で decref する
    let text = "data Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn pick(a0, b1, s2) {
      join j0() [] {
        let s7 = const "none"
        return s7
      }
      join j1(x10) [] {
        decref x10
        let s8 = const "some"
        return s8
      }
      switch a0 {
        #0 ->
          switch b1 {
            #0 ->
              decref s2
              let s4 = const "x"
              jump j1(s4)
            #1 ->
              jump j1(s2)
          }
        #1 ->
          decref s2
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
fn an_unused_field_is_decreffed_when_its_arm_starts() {
    // `Switch` が `o` を move で受け取り、`Some` の枝はフィールド `x1` を所有して始まる。使わないので入口で捨てる
    let text = "data Option a = | None | Some a\n\nflag : Option String -> Int\nflag o = match o with\n  | Some _ -> 0\n  | None -> 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r"
    fn flag(o0) {
      switch o0 {
        #0 ->
          return 1
        #1(x1) ->
          decref x1
          return 0
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
fn a_scrutinee_used_in_an_arm_is_dupped_before_the_switch() {
    // `ys` が受ける `xs` は枝の中でも使うので、`Switch` の前で複製する。その参照を使わない枝は入口側で捨てる
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r"
    fn size(xs0) {
      decref xs0
      return 2
    }
    fn describe(xs0) {
      join j0(ys1) [] {
        tailcall size(ys1)
      }
      dup xs0
      switch xs0 {
        #0 ->
          jump j0(xs0)
        #1(x2, x3) ->
          switch x3 {
            #0 ->
              decref xs0
              return 1
            #1(x4, x5) ->
              decref x5
              jump j0(xs0)
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
fn a_string_compared_twice_is_dupped_before_each_comparison() {
    // 比べる `prim` は出現の所有権を受け取るので、後の比較と枝でも使う `name0` を比べるたびに複製する。使わない枝は
    // 入口で捨てる
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn greet(name0) {
      let s4 = const "en"
      dup name0
      let t5 = prim string==(name0, s4)
      switch t5 {
        #0 ->
          let s6 = const "ja"
          dup name0
          let t7 = prim string==(name0, s6)
          switch t7 {
            #0 ->
              let other3 = name0
              return other3
            #1 ->
              decref name0
              let s2 = const "konnichiwa"
              return s2
          }
        #1 ->
          decref name0
          let s1 = const "hello"
          return s1
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
fn a_discarded_call_result_is_released() {
    // `shadowed_and_discarded_strings` の捨てた文字列は DCE で消えるので、呼び出しの結果を捨てる場合の解放はここで確かめる
    let text = "f : Unit -> String\nf () = \"z\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = f ()\n  println \"x\"";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn f(p0) {
      let s1 = const "z"
      return s1
    }
    fn main(p0) {
      let t1 = call f(())
      decref t1
      let s2 = const "x"
      let t3 = perform println(s2)
      return t3
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

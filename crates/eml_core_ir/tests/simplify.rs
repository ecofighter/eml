//! Core IR の `simplify` の書き換え (docs/spec/core-ir.md)。

mod common;

use common::core_text;
use eml_core_ir::Pass;

#[test]
fn a_join_point_that_returns_its_value_is_forwarded_and_removed() {
    // `let y = if ...` の後に `y` を返すだけなら、join point の本体を各 jump の位置に写し、join point を消す
    let text = "select : Bool -> Int\nselect c =\n  let y = if c then 1 else 2\n  y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn select(c0) {
      switch c0 {
        #0 ->
          return 2
        #1 ->
          return 1
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
fn a_join_point_that_passes_its_value_on_is_forwarded() {
    // 内側の join point の本体は外側への jump だけなので、内側への jump を外側への jump にする
    let text = "nested : Bool -> Bool -> Int\nnested a b =\n  let y =\n    if a then\n      let z = if b then 1 else 2\n      z\n    else 3\n  y + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn nested(a0, b1) {
      join j0(t3) [] {
        let t4 = prim +(t3, 1)
        return t4
      }
      switch a0 {
        #0 ->
          jump j0(3)
        #1 ->
          switch b1 {
            #0 ->
              jump j0(2)
            #1 ->
              jump j0(1)
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
fn known_tags_jump_straight_to_their_arm() {
    // `a && b` の偽は分かっているので、`a` が偽の枝は条件の値で分岐せずに `2` を返す
    let text = "both : Bool -> Bool -> Int\nboth a b = if a && b then 1 else 2\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn both(a0, b1) {
      switch a0 {
        #0 ->
          return 2
        #1 ->
          let t2 = b1
          switch t2 {
            #0 ->
              return 2
            #1 ->
              return 1
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
fn an_arm_reached_twice_stays_a_join_point() {
    // `||` の真の枝は2か所から来るので join point に残り、偽の枝は1か所からなので戻す
    let text = "either : Bool -> Bool -> String -> String\neither a b s = if a || b then s ++ \"!\" else s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r#"
    fn either(a0, b1, s2) {
      join j0(u7) [s2] {
        let s4 = const "!"
        let t5 = prim ++(s2, s4)
        return t5
      }
      switch a0 {
        #0 ->
          let t3 = b1
          switch t3 {
            #0 ->
              return s2
            #1 ->
              jump j0(())
          }
        #1 ->
          jump j0(())
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
fn split_arms_use_the_known_tag() {
    // 切り出した枝の中では、条件の値をその枝のタグに置き換える
    let text = "describe : Bool -> Bool -> Bool\ndescribe a b =\n  let v = a && b\n  if v then not v else v\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn describe(a0, b1) {
      switch a0 {
        #0 ->
          return #0
        #1 ->
          let t2 = b1
          switch t2 {
            #0 ->
              return #0
            #1 ->
              let t3 = prim not(#1)
              return t3
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
fn join_points_left_without_jumps_are_removed() {
    // `(a || True) && True` の join point は、消える join point の本体の中にしか jump がない。残すと captures が呼び出しをまたいで生きない
    let text = "noisy : String -> Bool -> <IO> Bool\nnoisy name b =\n  println name\n  b\n\nmain : Unit -> <IO> Unit\nmain () =\n  let other = \"other\"\n  let a = noisy \"a\" True\n  if (a || True) && True then println \"x\" else println other\n  println \"end\"";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r#"
    fn noisy(name0, b1) {
      let t2 = perform println(name0)
      return b1
    }
    fn main(p0) {
      let s1 = const "other"
      let s2 = const "a"
      let t3 = call noisy(s2, #1)
      join j0(t9) [] {
        let s10 = const "end"
        let t11 = perform println(s10)
        return t11
      }
      join j1(u13) [] {
        let s6 = const "x"
        let t7 = perform println(s6)
        jump j0(t7)
      }
      switch t3 {
        #0 ->
          jump j1(())
        #1 ->
          jump j1(())
      }
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn an_if_in_a_condition_jumps_straight_to_the_outer_join_point() {
    let text = "choose : Bool -> Bool -> Int\nchoose a b =\n  let n = if (if a then b else False) then 1 else 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn choose(a0, b1) {
      join j0(t3) [] {
        let t4 = prim +(t3, 1)
        return t4
      }
      switch a0 {
        #0 ->
          jump j0(2)
        #1 ->
          let t2 = b1
          switch t2 {
            #0 ->
              jump j0(2)
            #1 ->
              jump j0(1)
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

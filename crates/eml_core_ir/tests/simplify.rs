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
      join j0() [s2] {
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
              jump j0()
          }
        #1 ->
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
      join j1() [] {
        let s6 = const "x"
        let t7 = perform println(s6)
        jump j0(t7)
      }
      switch t3 {
        #0 ->
          jump j1()
        #1 ->
          jump j1()
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

#[test]
fn a_bool_match_in_a_condition_jumps_straight_to_the_branch() {
    // `match` の各枝が返す `True` と `False` は分かっているタグなので、`Bool` で分岐し直さずに `if` の枝へ直接進む
    let text = "data Option a = | None | Some a\n\npick : Option Int -> Int\npick o = if (match o with | Some _ -> True | None -> False) then 1 else 2\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn pick(o0) {
      switch o0 {
        #0 ->
          return 2
        #1(x1) ->
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
fn known_tags_of_a_larger_type_jump_straight_to_their_arm() {
    // 3つのタグのどれを渡す jump も、その枝へ直接向かう
    let text = "data Color = | Red | Green | Blue\n\ncode : Int -> Int\ncode n =\n  let c = if n == 0 then Red else if n == 1 then Green else Blue\n  match c with\n    | Red -> 10\n    | Green -> 20\n    | Blue -> 30\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn code(n0) {
      let t1 = prim ==(n0, 0)
      switch t1 {
        #0 ->
          let t2 = prim ==(n0, 1)
          switch t2 {
            #0 ->
              return 30
            #1 ->
              return 20
          }
        #1 ->
          return 10
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
fn a_mixed_switch_splits_only_the_arms_without_fields() {
    // `None` の枝だけを切り出す。`Some` の値を渡す jump は元の join point を通り、`Some _` の枝は `Switch` に残る
    let text = "data Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r#"
    fn pick(a0, b1, s2) {
      join j1() [] {
        let s7 = const "none"
        return s7
      }
      join j0(t6) [] {
        switch t6 {
          #0 ->
            jump j1()
          #1(x9) ->
            let s8 = const "some"
            return s8
        }
      }
      switch a0 {
        #0 ->
          switch b1 {
            #0 ->
              let s4 = const "x"
              let d5 = con #1(s4)
              jump j0(d5)
            #1 ->
              let d3 = con #1(s2)
              jump j0(d3)
          }
        #1 ->
          jump j1()
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
fn arms_with_fields_keep_the_join_point_argument() {
    // フィールドを束縛する枝の中の `o` (join point の引数) は、タグの定数に置き換えない
    let text = "data Option a = | None | Some a\n\nh : Option Int -> Int -> Int\nh o y = y\n\npick : Bool -> Int -> Int\npick c x =\n  let o = if c then None else if x > 0 then Some x else Some 0\n  match o with\n    | None -> 0\n    | Some y -> h o y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn h(o0, y1) {
      return y1
    }
    fn pick(c0, x1) {
      join j0(t5) [] {
        switch t5 {
          #0 ->
            return 0
          #1(y7) ->
            let y6 = y7
            tailcall h(t5, y6)
        }
      }
      switch c0 {
        #0 ->
          let t2 = prim >(x1, 0)
          switch t2 {
            #0 ->
              let d4 = con #1(0)
              jump j0(d4)
            #1 ->
              let d3 = con #1(x1)
              jump j0(d3)
          }
        #1 ->
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
fn jumps_that_pass_constructed_values_are_left_alone() {
    // どの jump も `con` で作った値を渡すので、B2 は join point を変えない。引数を持つコンストラクタの case-of-case は
    // 使われない束縛を消すパスと一緒に入れる (docs/implementation/status.md)
    let text = "data Option a = | None | Some a\n\npick : Bool -> Int\npick c = match (if c then Some 1 else Some 2) with\n  | Some n -> n\n  | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn pick(c0) {
      join j0(t3) [] {
        switch t3 {
          #0 ->
            return 0
          #1(n5) ->
            let n4 = n5
            return n4
        }
      }
      switch c0 {
        #0 ->
          let d2 = con #1(2)
          jump j0(d2)
        #1 ->
          let d1 = con #1(1)
          jump j0(d1)
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
fn split_arms_of_a_data_type_use_the_known_tag() {
    // 切り出した引数のない枝の中では、`c` をその枝のタグに置き換える
    let text = "data Color = | Red | Green | Blue\n\npick : Bool -> Color\npick b =\n  let c = if b then Red else Green\n  match c with\n    | Red -> c\n    | Green -> Blue\n    | Blue -> c\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn pick(b0) {
      switch b0 {
        #0 ->
          return #2
        #1 ->
          return #0
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

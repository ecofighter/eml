//! Perceus の `dup` / `decref` の位置と、呼び出しの `saved` (docs/spec/core-ir.md)。

use crate::common::core_text;
use eml_core_ir::Pass;

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"a\")";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn twice(s0^) {
      dup s0
      let t1^ = prim ++(s0, s0)
      return t1
    }
    fn main(p0) {
      let s1^ = const "a"
      let t2^ = call twice(s1)
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
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      let s1^ = const "x"
      let s2^ = const "y"
      let t3^ = prim ++(s1, s2)
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
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @"
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
fn calls_save_the_variables_used_after_them() {
    let text = "around : Int -> String -> String\naround n s =\n  let m = n + 1\n  let t = if n > 0 then twice s else s\n  t ++ show_int m\n\ntwice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @"
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
fn constructor_arguments_are_owned_by_the_value() {
    let text = "data Pair a b =\n  | Pair a b\n\ntwice : String -> Pair String String\ntwice s = Pair s s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @"
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
fn a_split_arm_with_fields_still_drops_its_unused_field() {
    // B2 が切り出した `Some _` の枝は、フィールド `x10` を引数に受けて始まり、使わないので入口で decref する
    let text = "data Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @"
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
fn an_unused_field_is_decreffed_when_its_arm_starts() {
    // `Switch` が `o` を move で受け取り、`Some` の枝はフィールド `x1` を所有して始まる。使わないので入口で捨てる
    let text = "data Option a = | None | Some a\n\nflag : Option String -> Int\nflag o = match o with\n  | Some _ -> 0\n  | None -> 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @"
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
fn a_scrutinee_used_in_an_arm_is_dupped_before_the_switch() {
    // `ys` が受ける `xs` は枝の中でも使うので、`Switch` の前で複製する。その参照を使わない枝は入口側で捨てる
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @"
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
fn a_string_compared_twice_is_dupped_before_each_comparison() {
    // 比べる `prim` は出現の所有権を受け取るので、後の比較と枝でも使う `name0` を比べるたびに複製する。使わない枝は
    // 入口で捨てる
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @"
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
fn a_discarded_call_result_is_released() {
    // `shadowed_and_discarded_strings` の捨てた文字列は DCE で消えるので、呼び出しの結果を捨てる場合の解放はここで確かめる
    let text = "f : Unit -> String\nf () = \"z\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = f ()\n  println \"x\"";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn f(p0) {
      let s1^ = const "z"
      return s1
    }
    fn main(p0) {
      let t1^ = call f(())
      decref t1
      let s2^ = const "x"
      let t3 = perform println(s2)
      return t3
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn equations_allocate_no_tuple_for_their_arguments() {
    // 等式の脱糖が作る引数のタプルは、simplify の K1 と DCE で消える
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x y = x + y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 1 2))";
    let core = core_text(text, Pass::Perceus);
    let start = core.find("fn f(").expect("the function");
    let f = &core[start
        ..core[start..]
            .find("\n}\n")
            .map_or(core.len(), |end| start + end)];
    assert!(!f.contains("con #"), "{f}");
}

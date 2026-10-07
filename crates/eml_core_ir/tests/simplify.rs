//! Core IR の `simplify` の書き換え (docs/implementation/architecture.md の「`simplify` の書き換え」)。

use crate::common::core_text;
use eml_core_ir::Pass;

#[test]
fn a_join_point_that_returns_its_value_is_forwarded_and_removed() {
    // `let y = if ...` の後に `y` を返すだけなら、join point の本体を各 jump の位置に写し、join point を消す
    let text = "select : Bool -> Int\nselect c =\n  let y = if c then 1 else 2\n  y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn a_join_point_that_passes_its_value_on_is_forwarded() {
    // 内側の join point の本体は外側への jump だけなので、内側への jump を外側への jump にする
    let text = "nested : Bool -> Bool -> Int\nnested a b =\n  let y =\n    if a then\n      let z = if b then 1 else 2\n      z\n    else 3\n  y + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn known_tags_jump_straight_to_their_arm() {
    // `a && b` の偽は分かっているので、`a` が偽の枝は条件の値で分岐せずに `2` を返す
    let text = "both : Bool -> Bool -> Int\nboth a b = if a && b then 1 else 2\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn an_arm_reached_twice_stays_a_join_point() {
    // `||` の真の枝は2か所から来るので join point に残り、偽の枝は1か所からなので戻す
    let text = "either : Bool -> Bool -> String -> String\neither a b s = if a || b then s ++ \"!\" else s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn split_arms_use_the_known_tag() {
    // 切り出した枝の中では、条件の値をその枝のタグに置き換える
    let text = "describe : Bool -> Bool -> Bool\ndescribe a b =\n  let v = a && b\n  if v then not v else v\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn join_points_left_without_jumps_are_removed() {
    // `(a || True) && True` の join point は、消える join point の本体の中にしか jump がない。残すと captures が呼び出しをまたいで生きない
    let text = "noisy : String -> Bool -> <IO> Bool\nnoisy name b =\n  println name\n  b\n\nmain : Unit -> <IO> Unit\nmain () =\n  let other = \"other\"\n  let a = noisy \"a\" True\n  if (a || True) && True then println \"x\" else println other\n  println \"end\"";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r#"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn noisy(name0^, b1) {
      let t2 = perform println(name0)
      return b1
    }
    fn main(p0) {
      let s2^ = const "a"
      let t3 = call noisy(s2, #1)
      join j0(t9) [] {
        let s10^ = const "end"
        let t11 = perform println(s10)
        return t11
      }
      join j1() [] {
        let s6^ = const "x"
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
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn a_bool_match_in_a_condition_jumps_straight_to_the_branch() {
    // `match` の各枝が返す `True` と `False` は分かっているタグなので、`Bool` で分岐し直さずに `if` の枝へ直接進む
    let text = "data Option a = | None | Some a\n\npick : Option Int -> Int\npick o = if (match o with | Some _ -> True | None -> False) then 1 else 2\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn known_tags_of_a_larger_type_jump_straight_to_their_arm() {
    // 3つのタグのどれを渡す jump も、その枝へ直接向かう
    let text = "data Color = | Red | Green | Blue\n\ncode : Int -> Int\ncode n =\n  let c = if n == 0 then Red else if n == 1 then Green else Blue\n  match c with\n    | Red -> 10\n    | Green -> 20\n    | Blue -> 30\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn a_mixed_switch_splits_every_arm_that_a_known_value_reaches() {
    // `None` の枝と、値が届く `Some _` の枝を切り出す。`Some` の値を渡す jump は、フィールドを引数にその枝へ直接向かい、`con` は消える
    let text = "data Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn an_arm_that_uses_the_whole_value_gets_it_as_an_argument() {
    // 枝が値全体 `o` も使うので、切り出した join point はフィールドに加えて値も受ける。`o` はタグの定数に置き換えず、`con` は残る
    let text = "data Option a = | None | Some a\n\nh : Option Int -> Int -> Int\nh o y = y\n\npick : Bool -> Int -> Int\npick c x =\n  let o = if c then None else if x > 0 then Some x else Some 0\n  match o with\n    | None -> 0\n    | Some y -> h o y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn jumps_that_pass_constructed_values_go_to_their_arms() {
    // どの jump も `con` で作った値を渡すので、B2 は `Some n` の枝を join point にして、jump がフィールドを渡す
    let text = "data Option a = | None | Some a\n\npick : Bool -> Int\npick c = match (if c then Some 1 else Some 2) with\n  | Some n -> n\n  | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn split_arms_of_a_data_type_use_the_known_tag() {
    // 切り出した引数のない枝の中では、`c` をその枝のタグに置き換える
    let text = "data Color = | Red | Green | Blue\n\npick : Bool -> Color\npick b =\n  let c = if b then Red else Green\n  match c with\n    | Red -> c\n    | Green -> Blue\n    | Blue -> c\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn a_join_point_with_two_parameters_reached_once_is_inlined() {
    // 引数が2つの枝は jump が1つだけなので、本体を jump の位置に写し、引数を `let` の連鎖で束縛する
    let text = "data Option a = | None | Some a\ndata Pair a b = | Pair a b\n\nadd : Pair (Option Int) (Option Int) -> Int\nadd p = match p with\n  | Pair (Some a) (Some b) -> a + b\n  | Pair x y -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
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
fn a_join_point_with_two_parameters_is_forwarded_by_position() {
    // 2つの leaf から届く枝の本体は2つ目の引数を返すだけなので、各 jump の2つ目の引数を位置で対応させて写す
    let text = "data Option a = | None | Some a\ndata Color = | Red | Green\ndata Pair a b = | Pair a b\n\nsecond : Pair Color (Option Int) -> Option Int\nsecond p = match p with\n  | Pair Red (Some n) -> Some n\n  | Pair t o -> o\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

/// `name` の関数だけの表示。関数は列0の `fn` で始まり、列0の `}` で終わる。
fn function(core: &str, name: &str) -> String {
    let start = core.find(&format!("fn {name}(")).expect("the function");
    let rest = &core[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |end| end + 2);
    rest[..end].to_string()
}

#[test]
fn a_match_on_a_tuple_literal_builds_no_tuple() {
    // `(a, b)` を作ってすぐ分解するので、K1 が `switch` を枝にし、DCE が使われなくなった `con` を消す
    let text = "pair : Int -> Int -> Int\npair a b = match (a, b) with\n  | (x, y) -> x + y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pair 1 2))";
    let pair = function(&core_text(text, Pass::Simplify), "pair");
    assert!(!pair.contains("con #"), "{pair}");
    assert!(!pair.contains("switch"), "{pair}");
}

#[test]
fn a_match_on_a_constructed_value_takes_its_arm() {
    let text = "data Option a = | None | Some a\n\nunwrap : Int -> Int\nunwrap x = match Some x with\n  | Some y -> y\n  | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (unwrap 1))";
    let unwrap = function(&core_text(text, Pass::Simplify), "unwrap");
    assert!(!unwrap.contains("con #"), "{unwrap}");
    assert!(!unwrap.contains("switch"), "{unwrap}");
}

#[test]
fn unused_bindings_that_cannot_fail_are_removed() {
    let text = "f : Int -> Int\nf x =\n  let p = (x, x)\n  let s = \"unused\"\n  1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 1))";
    let f = function(&core_text(text, Pass::Simplify), "f");
    assert!(!f.contains("con #"), "{f}");
    assert!(!f.contains("const"), "{f}");
}

#[test]
fn unused_bindings_that_can_fail_are_kept() {
    // `/` はゼロ除算で実行時エラーになるので、使われなくても消さない
    let text = "f : Int -> Int\nf x =\n  let q = x / 0\n  1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 1))";
    let f = function(&core_text(text, Pass::Simplify), "f");
    assert!(f.contains("prim /("), "{f}");
}

#[test]
fn a_switch_on_a_join_point_argument_is_not_known() {
    // 両方の jump が関数の呼び出しの結果を渡すので、join point の引数の値は分からない。K1 は引数への `switch` を残す
    let text = "data Option a = | None | Some a\n\nlookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int\npick c =\n  let o = if c then lookup 1 else lookup 2\n  let n = match o with\n    | Some v -> v\n    | None -> 0\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    assert_eq!(pick.matches("switch").count(), 2, "{pick}");
}

#[test]
fn a_switch_through_an_alias_takes_its_arm() {
    // 単一の jump の join point を B3 が戻すと `let t = d` の別名ができる。K1 がそれをたどって `con` まで届き、`switch` を枝にして DCE が `con` を消す
    let text = "data Option a = | None | Some a\n\nh : Int -> Int\nh x =\n  let p = match x with\n    | n -> Some n\n  match p with\n    | Some y -> y\n    | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (h 1))";
    let h = function(&core_text(text, Pass::Simplify), "h");
    assert!(!h.contains("switch"), "{h}");
    assert!(!h.contains("con #"), "{h}");
}

#[test]
fn a_jump_that_passes_a_constructed_value_goes_to_its_arm() {
    // どの jump も `Some` の値を渡すので、`Some n` の枝をフィールドを引数に取る join point にし、`con` は消える
    let text = "data Option a = | None | Some a\n\npick : Bool -> Int\npick c =\n  let n = match (if c then Some 1 else Some 2) with\n    | Some n -> n\n    | None -> 0\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    assert!(!pick.contains("con #"), "{pick}");
}

#[test]
fn an_arm_that_uses_the_whole_value_also_receives_it() {
    // 変数の枝 `x` は scrutinee そのものを受けるので、切り出した join point に値も渡し、`con` は残る。
    // 新しい join point の引数は新しい変数にするので、verifier の「2回束縛」にならない
    let text = "data Option a = | None | Some a\n\nsize : Option Int -> Int\nsize o = 1\n\npick : Bool -> Int\npick c =\n  let n = match (if c then Some 1 else None) with\n    | None -> 0\n    | x -> size x\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    assert!(pick.contains("con #1(1)"), "{pick}");
    core_text(text, Pass::Perceus);
}

#[test]
fn a_wildcard_arm_does_not_block_the_known_tags() {
    // 決定木は行列に現れないコンストラクタを `default` にまとめ、残りの枝の join point を作らない。ワイルドカードの枝が
    // あっても B2 が `switch` に届き、`if` の結果で分岐し直さない
    let text = "data Color = | Red | Green | Blue\n\npick : Bool -> Int\npick b =\n  let n = match (if b then Red else Green) with\n    | Red -> 1\n    | _ -> 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    assert_eq!(pick.matches("switch").count(), 1, "{pick}");
}

#[test]
fn f_floats_an_arm_join_point_so_that_b2_reaches_the_switch() {
    // `_` の枝には `No` と、中身が 0 でない `Yes` の2つの葉から届くので、枝は join point になり、`if` の join point の
    // 本体の先頭に置かれる。F がその join point を外へ出すので、B2 が本体の `switch` に届き、`if` の結果で分岐し直さない
    let text = "data Opt = | No | Yes Int\n\npick : Bool -> Int\npick b =\n  match (if b then Yes 0 else No) with\n    | Yes 0 -> 1\n    | _ -> 2\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    insta::assert_snapshot!(pick, @"
    fn pick(b0) {
      join j0(x4) [] {
        switch x4 {
          0 ->
            return 1
          _ ->
            return 2
        }
      }
      switch b0 {
        #0 ->
          return 2
        #1 ->
          jump j0(0)
      }
    }
    ");
}

#[test]
fn a_known_and_an_unknown_jump_share_the_split_arm() {
    // 分かっている jump は切り出した join point へ直接向かい、分からない jump は、残った1つの jump の位置に戻された元の join point の `switch` が、同じ join point へ転送する
    let text = "data Option a = | None | Some a\n\nlookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int\npick c =\n  let n = match (if c then Some 1 else lookup 2) with\n    | Some v -> v\n    | None -> 0\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    insta::assert_snapshot!(pick, @"
    fn pick(c0) {
      join j0(t6) [] {
        let t7 = prim +(t6, 1)
        return t7
      }
      join j1(v8) [] {
        let v4 = v8
        jump j0(v4)
      }
      switch c0 {
        #0 ->
          let t2^ = call lookup(2)
          let t3^ = t2
          switch t3 {
            #0 ->
              jump j0(0)
            #1(v5) ->
              jump j1(v5)
          }
        #1 ->
          jump j1(1)
      }
    }
    ");
}

#[test]
fn a_known_and_an_unknown_jump_share_an_arm_that_uses_the_whole_value() {
    // 枝が値全体も使うので、切り出した join point には値も渡す。分からない側の転送も、そのときの値を同じ join point へ渡す
    let text = "data Option a = | None | Some a\n\nsize : Option Int -> Int\nsize o = 1\n\nlookup : Int -> Option Int\nlookup n = Some n\n\npick : Bool -> Int\npick c =\n  let n = match (if c then Some 1 else lookup 2) with\n    | None -> 0\n    | x -> size x\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    insta::assert_snapshot!(pick, @"
    fn pick(c0) {
      join j0(t6) [] {
        let t7 = prim +(t6, 1)
        return t7
      }
      join j1(t8^) [] {
        let x4^ = t8
        let t5 = call size(x4)
        jump j0(t5)
      }
      switch c0 {
        #0 ->
          let t2^ = call lookup(2)
          let t3^ = t2
          switch t3 {
            #0 ->
              jump j0(0)
            _ ->
              jump j1(t3)
          }
        #1 ->
          let d1^ = con #1(1)
          jump j1(d1)
      }
    }
    ");
}

#[test]
fn a_known_tag_reaching_a_default_that_uses_the_scrutinee_passes_the_value() {
    // `Green` は `default` に進み、`default` の枝は値全体 (`x`) を使うので、B2 は `default` を切り出した join point
    // に値を渡す (`jump j1(#1)`)。`Red` は枝の値をそのまま渡し (`jump j0(1)`)、`if` の結果で分岐し直さない
    let text = "data Color = | Red | Green | Blue\n\ncode : Color -> Int\ncode c = 7\n\npick : Bool -> Int\npick b =\n  let n = match (if b then Red else Green) with\n    | Red -> 1\n    | x -> code x\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    insta::assert_snapshot!(pick, @"
    fn pick(b0) {
      join j0(t4) [] {
        let t5 = prim +(t4, 1)
        return t5
      }
      join j1(t6) [] {
        let x2 = t6
        let t3 = call code(x2)
        jump j0(t3)
      }
      switch b0 {
        #0 ->
          jump j1(#1)
        #1 ->
          jump j0(1)
      }
    }
    ");
}

#[test]
fn a_call_moved_into_a_branch_becomes_a_tail_call() {
    let text = "g : Int -> Int\ng x = x\n\nh : Int -> Int\nh x = x\n\nf : Bool -> Int -> Int\nf c x =\n  let y = if c then g x else h x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f True 1))";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    fn g(x0) {
      return x0
    }
    fn h(x0) {
      return x0
    }
    fn f(c0, x1) {
      switch c0 {
        #0 ->
          tailcall h(x1)
        #1 ->
          tailcall g(x1)
      }
    }
    fn main(p0) {
      let t1 = call f(#1, 1)
      let t2^ = prim show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn every_kind_of_call_in_tail_position_becomes_a_tail_call() {
    // 関数値の適用、`handle`、`resume`、操作の `perform` のどれも、T で末尾呼び出しになる
    // (docs/implementation/architecture.md の「`simplify` の書き換え」)
    let text = "effect Ask where\n  ask : String -> Int\n\ntwice : (Int -> Int) -> Int -> Int\ntwice f x = f (f x)\n\nasked : Unit -> <Ask> Int\nasked () = ask \"x\"\n\nanswer : Unit -> Int\nanswer () =\n  handle asked () with\n    | ask key k -> k 1\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (answer ()))";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r#"
    effect Prelude.IO { println/1, open/1, read_all/1, close/1 }
    effect Ask { ask/1 }
    fn asked(p0) {
      let s1^ = const "x"
      tailcall perform Ask.ask(s1)
    }
    fn answer(p0) {
      tailcall handle Ask(&answer$handle0, ()) {ask: &answer$handle0$ask} return &answer$handle0$return
    }
    fn main(p0) {
      let t1 = call answer(())
      let t2^ = prim show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
    fn answer$handle0(p0) {
      tailcall asked(())
    }
    fn answer$handle0$ask(key0^, k1^, p2) {
      tailcall resume k1(1, ())
    }
    fn answer$handle0$return($r0, p1) {
      return $r0
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn a_known_tag_without_a_case_takes_the_default() {
    // K1: 分かっているタグの case がなければ `default` の本体で置き換える
    // (docs/implementation/architecture.md の「`simplify` の書き換え」)
    let text = "data Color = | Red | Green | Blue\n\nmain : Unit -> <IO> Unit\nmain () =\n  let c = Green\n  let n = match c with\n    | Red -> 1\n    | _ -> 2\n  println (show_int n)";
    let shown = core_text(text, Pass::Simplify);
    let main = shown.split("fn entry$main").next().unwrap();
    assert!(!main.contains("switch"), "{shown}");
}

#[test]
fn known_tags_that_take_the_default_jump_straight_to_it() {
    // B2: 分かっているタグ (`Green` と `Blue`) が `default` に進むので、`default` を join point に切り出して直接 jump
    // する。切り出した本体は小さいので、B5 が両方の jump を `return 2` にし、`if` の結果で分岐し直さない
    let text = "data Color = | Red | Green | Blue\n\npick : Bool -> Int\npick b =\n  match (if b then Green else Blue) with\n    | Red -> 1\n    | _ -> 2\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    insta::assert_snapshot!(pick, @"
    fn pick(b0) {
      switch b0 {
        #0 ->
          return 2
        #1 ->
          return 2
      }
    }
    ");
}

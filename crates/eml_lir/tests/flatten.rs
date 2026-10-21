//! 開くパス (docs/spec/lir.md の「開く」)。構造体はまだソースから作られない
//! ので、手で書いた構造体の LIR を box の挿入から開くパスまで通す。

use crate::common::function;
use eml_lir::{Pass, parse, passes_until, pretty, verify_flattened};

const VEC3: &str = "layout Vec3 { Vec3(float, float, float) }\n";
const PAIR: &str = "layout (,) { (,)(tobj, tobj) }\n";
const OPTION: &str = "layout Option { None, Some(tobj) }\n";

/// 開いた後の表示。表示を読み直すと同じ表示に戻り、読み直したものが開いた後の段の verifier を通ることも確かめる。
fn flatten_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}\n{text}"));
    passes_until(&mut program, Pass::Flatten);
    let shown = pretty(&program);
    let parsed = parse(&shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(pretty(&parsed), shown, "the printed LIR must read back");
    if let Err(error) = verify_flattened(&parsed) {
        panic!("the LIR read back must verify: {error}\n{shown}");
    }
    shown
}

#[test]
fn con_and_unpack_of_a_struct_vanish() {
    // 構造体の `con` は消え、束縛した変数の葉は `con` の引数になる。定数の引数はそのまま定数の葉になる。`unpack` も
    // 消え、フィールドの変数は値の葉に置き換わる
    let text = format!(
        "{VEC3}fn f(x.0: float, y.1: float) -> float {{
  let v.2: Vec3{{float, float, float}} = con Vec3 #0(x.0, y.1, 1.5)
  unpack v.2 Vec3 #0(a.3: float, b.4: float, c.5: float)
  let s.6: float = extern \"Prelude.Num Float.+\"(a.3, c.5)
  return s.6
}}
"
    );
    insta::assert_snapshot!(function(&flatten_text(&text), "f"), @r#"
    fn f(x.0: float, y.1: float) -> float {
      let s.6: float = extern "Prelude.Num Float.+"(x.0, 1.5)
      return s.6
    }
    "#);
}

#[test]
fn box_builds_the_layout_con() {
    // 構造体の箱は配置の `con` で、葉を配置の宣言したフィールドの Repr に合わせる。`Ball` の `tobj` のフィールドに入る
    // `Vec3` は再帰で箱にし、`float` のフィールドには葉をそのまま入れる。`(Int, Float)` の箱はどちらのフィールドも
    // `tobj` なので、葉をそれぞれ `box` する
    // (docs/spec/lir.md の「開く」)
    let text = format!(
        "{VEC3}{PAIR}{OPTION}layout Ball {{ Ball(tobj, float) }}
fn f(b.0: Ball{{Vec3{{float, float, float}}, float}}, p.1: (,){{int, float}}) -> obj {{
  let o.2: tobj = con Option #1(b.0)
  let q.3: tobj = con Option #1(p.1)
  let t.4: obj = con (,) #0(o.2, q.3)
  return t.4
}}
"
    );
    insta::assert_snapshot!(function(&flatten_text(&text), "f"), @"
    fn f(b.0.7: float, b.1.8: float, b.2.9: float, b.3.10: float, p.0.11: int, p.1.12: float) -> obj source(Ball{Vec3{float, float, float}, float}, (,){int, float}) -> obj {
      let b.13: obj = con Vec3 #0(b.0.7, b.1.8, b.2.9)
      let b.5: obj = con Ball #0(b.13, b.3.10)
      let o.2: tobj = con Option #1(b.5)
      let p.14: tobj = box p.0.11
      let p.15: tobj = box p.1.12
      let p.6: obj = con (,) #0(p.14, p.15)
      let q.3: tobj = con Option #1(p.6)
      let t.4: obj = con (,) #0(o.2, q.3)
      return t.4
    }
    ");
}

#[test]
fn a_leaf_from_an_unbox_is_not_reboxed() {
    // 箱の `tobj` のフィールドから取り出した葉を同じ関数の中で箱に戻すときは、`box` せずにそのフィールドの語を渡す。
    // 組の要素を入れ替えて箱に戻すとき、要素ごとに `Float` の箱を作らないためである。取り出した葉はほかに使われない
    // ので、`unbox` も残らない
    let text = format!(
        "{PAIR}internal uniform from 0 fn swap(p.0: (,){{float, float}}) -> (,){{float, float}} {{
  unpack p.0 (,) #0(a.1: float, b.2: float)
  let q.3: (,){{float, float}} = con (,) #0(b.2, a.1)
  return q.3
}}
"
    );
    insta::assert_snapshot!(function(&flatten_text(&text), "swap"), @"
    internal uniform from 0 fn swap(p.4: tobj) -> tobj {
      let (p.0.6: tobj, p.1.8: tobj) = unbox p.4 (,)
      let q.5: obj = con (,) #0(p.1.8, p.0.6)
      return q.5
    }
    ");
}

#[test]
fn unbox_becomes_a_multi_def_with_unused_leaves_skipped() {
    // 構造体の `unbox` は箱のフィールドを読んで葉を定義する1つの文になる。使わない葉は `_` と書き、読まない
    let text = format!(
        "{VEC3}internal uniform from 0 fn f(v.0: Vec3{{float, float, float}}) -> float {{
  unpack v.0 Vec3 #0(x.1: float, y.2: float, z.3: float)
  return y.2
}}
"
    );
    insta::assert_snapshot!(function(&flatten_text(&text), "f"), @"
    internal uniform from 0 fn f(v.4: tobj) -> tobj {
      let (_, v.1.7: float, _) = unbox v.4 Vec3
      let y.5: tobj = box v.1.7
      return y.5
    }
    ");
}

#[test]
fn unit_leaves_make_no_variables() {
    // `unit` の葉は変数を作らず、使う所は `()` になる。`unit` でない葉が1つだけの構造体は、値を1つ受け渡す。`apply` の
    // 引数と結果は一様なので、`int` の箱でなく `P` の箱で受け渡す
    let text = "\
layout P { P(tobj, int) }
fn g(p.0: P{unit, int}) -> P{unit, int} {
  return p.0
}
fn f(n.0: int, c.6: tobj) -> unit {
  let p.1: P{unit, int} = con P #0((), n.0)
  let q.2: P{unit, int} = call g(p.1)
  let w.7: P{unit, int} = apply c.6(q.2)
  unpack w.7 P #0(u.3: unit, m.4: int)
  let r.5: unit = extern Prelude.check_index(m.4, m.4)
  return u.3
}
";
    let shown = flatten_text(text);
    insta::assert_snapshot!(function(&shown, "g"), @"
    fn g(p.1.1: int) -> int source(P{unit, int}) -> P{unit, int} {
      return p.1.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "f"), @"
    fn f(n.0: int, c.6: tobj) -> unit {
      let q.1.10: int = call g(n.0)
      let q.8: obj = con P #0((), q.1.10)
      let w.9: tobj = apply c.6(q.8)
      let (_, w.1.11: int) = unbox w.9 P
      let r.5: unit = extern Prelude.check_index(w.1.11, w.1.11)
      return ()
    }
    ");
}

#[test]
fn calls_applies_and_returns_carry_leaves() {
    // 関数の引数、直接の呼び出しの引数、ブロックの引数と `jump` の実引数を葉の並びに開く。構造体の結果は複数の値の
    // `let` で受け、`return` は値の並びを返す。関数の結果も値の並びになる。`apply` の引数と結果と、一様な版を指す
    // `closure` の引数は一様なので、構造体の箱で渡す
    let vec3 = "Vec3{float, float, float}";
    let text = format!(
        "{VEC3}fn add(a.0: {vec3}, b.1: {vec3}) -> {vec3} {{
  unpack a.0 Vec3 #0(x.2: float, y.3: float, z.4: float)
  unpack b.1 Vec3 #0(u.5: float, v.6: float, w.7: float)
  let p.8: float = extern \"Prelude.Num Float.+\"(x.2, u.5)
  let q.9: float = extern \"Prelude.Num Float.+\"(y.3, v.6)
  let r.10: float = extern \"Prelude.Num Float.+\"(z.4, w.7)
  let s.11: {vec3} = con Vec3 #0(p.8, q.9, r.10)
  return s.11
}}
fn f(a.0: {vec3}, c.1: tobj) -> {vec3} {{
  let s.2: {vec3} = call add(a.0, a.0)
  let t.3: {vec3} = apply c.1(s.2)
  let k.4: tobj = closure add(t.3)
  let r.5: {vec3} = apply k.4(a.0)
  jump b1(r.5)
b1(m.6: {vec3}):
  return m.6
}}
"
    );
    let shown = flatten_text(&text);
    insta::assert_snapshot!(function(&shown, "add"), @r#"
    fn add(a.0.12: float, a.1.13: float, a.2.14: float, b.0.15: float, b.1.16: float, b.2.17: float) -> (float, float, float) source(Vec3{float, float, float}, Vec3{float, float, float}) -> Vec3{float, float, float} {
      let p.8: float = extern "Prelude.Num Float.+"(a.0.12, b.0.15)
      let q.9: float = extern "Prelude.Num Float.+"(a.1.13, b.1.16)
      let r.10: float = extern "Prelude.Num Float.+"(a.2.14, b.2.17)
      return (p.8, q.9, r.10)
    }
    "#);
    insta::assert_snapshot!(function(&shown, "f"), @"
    fn f(a.0.11: float, a.1.12: float, a.2.13: float, c.1: tobj) -> (float, float, float) source(Vec3{float, float, float}, tobj) -> Vec3{float, float, float} {
      let (s.0.17: float, s.1.18: float, s.2.19: float) = call add(a.0.11, a.1.12, a.2.13, a.0.11, a.1.12, a.2.13)
      let s.7: obj = con Vec3 #0(s.0.17, s.1.18, s.2.19)
      let t.8: tobj = apply c.1(s.7)
      let (_, _, _) = unbox t.8 Vec3
      let k.4: tobj = closure add$uniform(t.8)
      let a.9: obj = con Vec3 #0(a.0.11, a.1.12, a.2.13)
      let r.10: tobj = apply k.4(a.9)
      let (r.0.23: float, r.1.24: float, r.2.25: float) = unbox r.10 Vec3
      jump b1(r.0.23, r.1.24, r.2.25)
    b1(m.0.14: float, m.1.15: float, m.2.16: float):
      return (m.0.14, m.1.15, m.2.16)
    }
    ");
}

#[test]
fn a_call_that_never_returns_binds_a_struct_to_a_dummy_obj() {
    // 戻らない呼び出し (`never` の操作の `clause`、`exit`) の構造体の束縛は、`unr` の `obj` の変数で受け、葉をダミーの
    // 値で定義する。box の挿入はこの束縛の使いを変換しないので、1語の位置 (`tobj` のフィールド) の使いはその変数に
    // なる。`t` は使われないが、縮約の前なので残る。制御が届かないので、どれも実行されない
    let text = format!(
        "{VEC3}{PAIR}{OPTION}effect Fail {{ never fail(tobj) -> never }}
yields fn f(c.0: tobj, e.1: obj, s.2: obj) -> Vec3{{float, float, float}} {{
  let v.3: Vec3{{float, float, float}} = clause Fail.fail c.0(e.1, s.2)
  let o.4: tobj = con Option #1(v.3)
  let t.5: obj = con (,) #0(o.4, v.3)
  return v.3
}}
fn g(n.0: int) -> (,){{obj, int}} {{
  let p.1: (,){{obj, int}} = extern Prelude.exit(n.0)
  return p.1
}}
"
    );
    let shown = flatten_text(&text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(c.0: tobj, e.1: obj, s.2: obj) -> (float, float, float) source(tobj, obj, obj) -> Vec3{float, float, float} {
      let v.6: obj = clause Fail.fail c.0(e.1, s.2)
      let o.4: tobj = con Option #1(v.6)
      let t.5: obj = con (,) #0(o.4, v.6)
      return (0.0, 0.0, 0.0)
    }
    ");
    insta::assert_snapshot!(function(&shown, "g"), @"
    fn g(n.0: int) -> (obj, int) source(int) -> (,){obj, int} {
      let p.2: obj = extern Prelude.exit(n.0)
      let p.0.3: obj = prim dummy()
      return (p.0.3, 0)
    }
    ");
}

#[test]
fn a_struct_drop_drops_each_rc_leaf() {
    // 構造体の `drop` は、RC の対象の葉ごとの `drop` になる。結果の `unit` の変数は `()` に置き換わる
    let text = "\
layout T { T(tobj, tobj, tobj) }
fn f(t.0: T{obj lin, int, tobj poly} lin) -> unit {
  let u.1: unit = drop t.0
  return u.1
}
";
    insta::assert_snapshot!(function(&flatten_text(text), "f"), @"
    fn f(t.0.2: obj lin, t.1.3: int, t.2.4: tobj poly) -> unit source(T{obj lin, int, tobj poly}) -> unit {
      let u.5: unit = drop t.0.2
      let u.6: unit = drop t.2.4
      return ()
    }
    ");
}

#[test]
fn a_nested_struct_is_unboxed_from_its_tobj_field() {
    // 箱の `tobj` のフィールドにある入れ子の構造体は、外の `unbox` に続く構造体の `unbox` で開く。入れ子の葉をどれも
    // 使わなければ、その `unbox` は消え、外の `unbox` もそのフィールドを読まない
    let ball = "Ball{Vec3{float, float, float}, float}";
    let text = format!(
        "{VEC3}layout Ball {{ Ball(tobj, float) }}
internal uniform from 0 fn inner(b.0: {ball}) -> float {{
  unpack b.0 Ball #0(v.1: Vec3{{float, float, float}}, r.2: float)
  unpack v.1 Vec3 #0(x.3: float, y.4: float, z.5: float)
  let s.6: float = extern \"Prelude.Num Float.+\"(y.4, r.2)
  return s.6
}}
internal uniform from 0 fn outer(b.0: {ball}) -> float {{
  unpack b.0 Ball #0(v.1: Vec3{{float, float, float}}, r.2: float)
  return r.2
}}
"
    );
    let shown = flatten_text(&text);
    insta::assert_snapshot!(function(&shown, "inner"), @r#"
    internal uniform from 0 fn inner(b.7: tobj) -> tobj {
      let (b.0.9: tobj, b.3.13: float) = unbox b.7 Ball
      let (_, b.1.11: float, _) = unbox b.0.9 Vec3
      let s.6: float = extern "Prelude.Num Float.+"(b.1.11, b.3.13)
      let s.8: tobj = box s.6
      return s.8
    }
    "#);
    insta::assert_snapshot!(function(&shown, "outer"), @"
    internal uniform from 0 fn outer(b.3: tobj) -> tobj {
      let (_, b.3.9: float) = unbox b.3 Ball
      let r.4: tobj = box b.3.9
      return r.4
    }
    ");
}

#[test]
fn a_struct_unbox_leaves_the_lin_box_to_perceus() {
    // 構造体の `unbox` の後に、開くパスは `lin` の箱を手放す `drop` を置かない。箱を消費するかは道ごとに違うので、
    // 道ごとに手放すのは Perceus である (docs/spec/lir.md の「開く」)
    let text = "\
layout Tok { Tok(int, float) }
internal uniform from 0 fn f(t.0: Tok{int, float} lin) -> int {
  unpack t.0 Tok #0(a.1: int, b.2: float)
  return a.1
}
internal uniform from 0 fn kept(t.0: Tok{int, float} lin) -> Tok{int, float} lin {
  unpack t.0 Tok #0(a.1: int, b.2: float)
  let c.3: unit = extern Prelude.check_index(a.1, a.1)
  return t.0
}
";
    let shown = flatten_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    internal uniform from 0 fn f(t.3: tobj lin) -> tobj {
      let (t.0.5: int, _) = unbox t.3 Tok
      let a.4: tobj = box t.0.5
      return a.4
    }
    ");
    insta::assert_snapshot!(function(&shown, "kept"), @"
    internal uniform from 0 fn kept(t.4: tobj lin) -> tobj lin {
      let (t.0.5: int, _) = unbox t.4 Tok
      let c.3: unit = extern Prelude.check_index(t.0.5, t.0.5)
      return t.4
    }
    ");
}

#[test]
fn a_box_taken_from_a_struct_leaf_is_unboxed_from_the_leaf() {
    // 構造体の葉にある箱から構造体を取り出すと、`unbox` は葉の変数から読む
    let text = format!(
        "{VEC3}layout W {{ W(tobj, int) }}
fn g(v.0: Vec3{{float, float, float}} lin) -> float {{
  unpack v.0 Vec3 #0(x.1: float, y.2: float, z.3: float)
  return x.1
}}
fn f(s.0: W{{tobj lin, int}} lin) -> float {{
  unpack s.0 W #0(t.1: tobj lin, n.2: int)
  let r.3: float = call g(t.1)
  let q.4: float = extern \"Prelude.Num Float.+\"(r.3, 1.0)
  return q.4
}}
"
    );
    insta::assert_snapshot!(function(&flatten_text(&text), "f"), @r#"
    fn f(s.0.6: tobj lin, s.1.7: int) -> float source(W{tobj lin, int}) -> float {
      let (t.0.8: float, t.1.9: float, t.2.10: float) = unbox s.0.6 Vec3
      let r.3: float = call g(t.0.8, t.1.9, t.2.10)
      let q.4: float = extern "Prelude.Num Float.+"(r.3, 1.0)
      return q.4
    }
    "#);
}

#[test]
fn a_box_used_in_two_branches_is_unboxed_in_each() {
    // box の挿入は使う所ごとに取り出すので、二つの枝はそれぞれ構造体を取り出す
    let text = format!(
        "{VEC3}layout W {{ W(tobj, int) }}
layout Prelude.Bool {{ False, True }}
fn g(v.0: Vec3{{float, float, float}} lin) -> float {{
  unpack v.0 Vec3 #0(x.1: float, y.2: float, z.3: float)
  return x.1
}}
fn f(s.0: W{{tobj lin, int}} lin, c.1: enum) -> float {{
  unpack s.0 W #0(t.2: tobj lin, n.3: int)
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let r.4: float = call g(t.2)
  return r.4
b2:
  let q.5: float = call g(t.2)
  let u.6: float = extern \"Prelude.Num Float.+\"(q.5, 1.0)
  return u.6
}}
"
    );
    insta::assert_snapshot!(function(&flatten_text(&text), "f"), @r#"
    fn f(s.0.9: tobj lin, s.1.10: int, c.1: enum) -> float source(W{tobj lin, int}, enum) -> float {
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let (t.0.11: float, t.1.12: float, t.2.13: float) = unbox s.0.9 Vec3
      let r.4: float = call g(t.0.11, t.1.12, t.2.13)
      return r.4
    b2:
      let (t.0.14: float, t.1.15: float, t.2.16: float) = unbox s.0.9 Vec3
      let q.5: float = call g(t.0.14, t.1.15, t.2.16)
      let u.6: float = extern "Prelude.Num Float.+"(q.5, 1.0)
      return u.6
    }
    "#);
}

#[test]
fn a_box_from_a_tail_position_apply_is_opened_to_its_leaves() {
    // 構造体を返す関数の末尾の位置の `apply` も、結果は `tobj` の箱である。ほかの束縛と同じく箱を葉に開いて返すので、
    // 縮約は末尾呼び出しにしない (docs/spec/lir.md の「末尾の位置の呼び出し」)
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn f(k.0: obj) -> (,){float, float} {
  let p.1: (,){float, float} = apply k.0(1)
  return p.1
}
";
    insta::assert_snapshot!(function(&flatten_text(text), "f"), @"
    fn f(k.0: obj) -> (float, float) source(obj) -> (,){float, float} {
      let b.2: tobj = box 1
      let p.3: tobj = apply k.0(b.2)
      let (p.0.4: tobj, p.1.6: tobj) = unbox p.3 (,)
      let p.0.5: float = unbox p.0.4
      let p.1.7: float = unbox p.1.6
      return (p.0.5, p.1.7)
    }
    ");
}

#[test]
fn a_switch_on_a_constant_leaf_becomes_a_jump() {
    // 定数を引数にした構造体の `con` を分けて `switch` すると、葉を置き換えた `switch` は定数で分ける。合う case への
    // ジャンプにし、辺の入らなくなったブロックを落として番号を振り直す
    let text = format!(
        "{OPTION}layout Color {{ Red, Green }}
fn f(o.0: tobj) -> int {{
  let p.1: (,){{enum, tobj}} = con (,) #0(#1, o.0)
  unpack p.1 (,) #0(c.2: enum, x.3: tobj)
  switch c.2 Color {{ #0 -> b1, #1 -> b2 }}
b1:
  return 1
b2:
  switch x.3 Option {{ #0 -> b3, _ -> b4 }}
b3:
  return 2
b4:
  return 3
}}
"
    );
    insta::assert_snapshot!(function(&flatten_text(&format!("{PAIR}{text}")), "f"), @"
    fn f(o.0: tobj) -> int {
      jump b1()
    b1:
      switch o.0 Option { #0 -> b2, _ -> b3 }
    b2:
      return 2
    b3:
      return 3
    }
    ");
}

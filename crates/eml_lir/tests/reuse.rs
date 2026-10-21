//! reuse のパス (docs/spec/lir.md の「reuse」)。入力は Perceus の後の LIR で、入力と出力の両方が所有の段の verifier を
//! 通ることも確かめる。

use eml_lir::{Pass, parse, pretty, reuse, verify};

use crate::common::{function, passes_text};

fn reuse_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    reuse(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

/// パスが何も変えないこと。入力も `pretty` を通して比べる。
fn assert_unchanged(text: &str) {
    let input = pretty(&parse(text).unwrap_or_else(|error| panic!("{error}")));
    assert_eq!(reuse_text(text), input);
}

const BOOL: &str = "layout Prelude.Bool { False, True }\n";
const P2: &str = "layout P { P(obj, obj) }\n";

#[test]
fn a_release_pairs_with_a_con_of_the_same_size() {
    let text = format!(
        "{P2}fn swap(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj)
  release x.0 P #0(a.1, b.2)
  let d.3: obj = con P #0(b.2, a.1)
  return d.3
}}
"
    );
    insta::assert_snapshot!(reuse_text(&text), @"
    layout P { P(obj, obj) }
    fn swap(x.0: obj) -> obj {
      unpack x.0 P #0(a.1: obj, b.2: obj)
      let r.4: token = release x.0 P #0(a.1, b.2)
      let d.3: obj = con P #0(b.2, a.1) reuse r.4 in place
      return d.3
    }
    ");
}

#[test]
fn the_same_constructor_is_rebuilt_in_place_and_skips_unchanged_fields() {
    // 走査する語 (s) も生の語 (k) も、元と同じ位置なら書かない。m は新しい値なので書く
    let text = "\
layout Cell { Cell(int, int, obj) }
fn bump(x.0: obj) -> obj {
  unpack x.0 Cell #0(n.1: int, k.2: int, s.3: obj)
  release x.0 Cell #0(_, _, s.3)
  let m.4: int = extern \"Prelude.Num Int.+\"(n.1, 1)
  let d.5: obj = con Cell #0(m.4, k.2, s.3)
  return d.5
}
";
    insta::assert_snapshot!(reuse_text(text), @"
    layout Cell { Cell(int, int, obj) }
    fn bump(x.0: obj) -> obj {
      unpack x.0 Cell #0(n.1: int, k.2: int, s.3: obj)
      let r.6: token = release x.0 Cell #0(_, _, s.3)
      let m.4: int = extern \"Prelude.Num Int.+\"(n.1, 1)
      let d.5: obj = con Cell #0(m.4, =k.2, =s.3) reuse r.6 in place
      return d.5
    }
    ");
}

#[test]
fn another_constructor_of_the_same_size_rewrites_the_header() {
    // cons の箱を組の箱に使う。配置が違うので、ヘッダを書き直す
    let text = "\
layout Prelude.List { Nil, ::(tobj, tobj) }
layout (,) { (,)(tobj, tobj) }
fn flip(xs.0: tobj) -> tobj {
  switch xs.0 Prelude.List { #0 -> b1, #1(h.1: tobj, t.2: tobj) -> b2 }
b1:
  return xs.0
b2:
  release xs.0 Prelude.List #1(h.1, t.2)
  let d.3: obj = con (,) #0(t.2, h.1)
  return d.3
}
";
    insta::assert_snapshot!(reuse_text(text), @"
    layout Prelude.List { Nil, ::(tobj, tobj) }
    layout (,) { (,)(tobj, tobj) }
    fn flip(xs.0: tobj) -> tobj {
      switch xs.0 Prelude.List { #0 -> b1, #1(h.1: tobj, t.2: tobj) -> b2 }
    b1:
      return xs.0
    b2:
      let r.4: token = release xs.0 Prelude.List #1(h.1, t.2)
      let d.3: obj = con (,) #0(t.2, h.1) reuse r.4
      return d.3
    }
    ");
}

#[test]
fn a_lin_source_rebuilt_as_poly_is_not_in_place() {
    // ヘッダの `lin` ビットを書き直すため、その場で作り直さない。フィールドは同じ位置に残るので書かない
    // (docs/spec/object-model.md の「`lin` ビット」)
    let text = "\
layout P { P(tobj, tobj) }
fn rebuild(x.0: obj lin) -> obj poly {
  unpack x.0 P #0(a.1: tobj poly, b.2: tobj poly)
  release x.0 P #0(a.1, b.2)
  let d.3: obj poly = con P #0(a.1, b.2)
  return d.3
}
";
    insta::assert_snapshot!(reuse_text(text), @"
    layout P { P(tobj, tobj) }
    fn rebuild(x.0: obj lin) -> obj poly {
      unpack x.0 P #0(a.1: tobj poly, b.2: tobj poly)
      let r.4: token = release x.0 P #0(a.1, b.2)
      let d.3: obj poly = con P #0(=a.1, =b.2) reuse r.4
      return d.3
    }
    ");
}

#[test]
fn a_decref_of_a_known_value_is_a_source() {
    // レコードの更新の形。フィールドがみなスカラーなので、Perceus は `release` でなく `decref` を出す
    let text = "\
layout Stats { Stats(int, int) }
fn count(st.0: obj) -> obj {
  unpack st.0 Stats #0(n.1: int, k.2: int)
  decref st.0
  let m.3: int = extern \"Prelude.Num Int.+\"(n.1, 1)
  let d.4: obj = con Stats #0(m.3, k.2)
  return d.4
}
";
    insta::assert_snapshot!(reuse_text(text), @"
    layout Stats { Stats(int, int) }
    fn count(st.0: obj) -> obj {
      unpack st.0 Stats #0(n.1: int, k.2: int)
      let r.5: token = release st.0 Stats #0(_, _)
      let m.3: int = extern \"Prelude.Num Int.+\"(n.1, 1)
      let d.4: obj = con Stats #0(m.3, =k.2) reuse r.5 in place
      return d.4
    }
    ");
}

#[test]
fn a_decref_of_an_unknown_value_is_not_a_source() {
    assert_unchanged(
        "\
layout Stats { Stats(int, int) }
fn fresh(st.0: obj) -> obj {
  decref st.0
  let d.1: obj = con Stats #0(1, 2)
  return d.1
}
",
    );
}

#[test]
fn a_token_flows_into_switch_targets_and_is_dropped_where_unused() {
    let text = format!(
        "{BOOL}{P2}fn f(x.0: obj, c.1: enum) -> obj {{
  unpack x.0 P #0(a.2: obj, b.3: obj)
  release x.0 P #0(a.2, b.3)
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let d.4: obj = con P #0(b.3, a.2)
  return d.4
b2:
  let s.5: obj = extern Prelude.++(a.2, b.3)
  return s.5
}}
"
    );
    insta::assert_snapshot!(reuse_text(&text), @"
    layout Prelude.Bool { False, True }
    layout P { P(obj, obj) }
    fn f(x.0: obj, c.1: enum) -> obj {
      unpack x.0 P #0(a.2: obj, b.3: obj)
      let r.6: token = release x.0 P #0(a.2, b.3)
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let d.4: obj = con P #0(b.3, a.2) reuse r.6 in place
      return d.4
    b2:
      drop_token r.6
      let s.5: obj = extern Prelude.++(a.2, b.3)
      return s.5
    }
    ");
}

#[test]
fn a_token_unused_below_a_switch_target_is_dropped_once() {
    // b2 より下のどこでも使わないので、b2 の入口で1回だけ手放す。b3 と b4 は手放さない
    let text = format!(
        "{BOOL}{P2}fn f(x.0: obj, c.1: enum, e.2: enum) -> obj {{
  unpack x.0 P #0(a.3: obj, b.4: obj)
  release x.0 P #0(a.3, b.4)
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let d.5: obj = con P #0(b.4, a.3)
  return d.5
b2:
  switch e.2 Prelude.Bool {{ #0 -> b3, #1 -> b4 }}
b3:
  let s.6: obj = extern Prelude.++(a.3, b.4)
  return s.6
b4:
  let s.7: obj = extern Prelude.++(b.4, a.3)
  return s.7
}}
"
    );
    insta::assert_snapshot!(function(&reuse_text(&text), "f"), @"
    fn f(x.0: obj, c.1: enum, e.2: enum) -> obj {
      unpack x.0 P #0(a.3: obj, b.4: obj)
      let r.8: token = release x.0 P #0(a.3, b.4)
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let d.5: obj = con P #0(b.4, a.3) reuse r.8 in place
      return d.5
    b2:
      drop_token r.8
      switch e.2 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      let s.6: obj = extern Prelude.++(a.3, b.4)
      return s.6
    b4:
      let s.7: obj = extern Prelude.++(b.4, a.3)
      return s.7
    }
    ");
}

#[test]
fn a_token_is_not_passed_through_a_jump() {
    assert_unchanged(&format!(
        "{P2}fn f(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj)
  release x.0 P #0(a.1, b.2)
  jump b1(a.1, b.2)
b1(p.3: obj, q.4: obj):
  let d.5: obj = con P #0(q.4, p.3)
  return d.5
}}
"
    ));
}

#[test]
fn a_source_no_path_uses_stays_a_release() {
    assert_unchanged(&format!(
        "{BOOL}{P2}layout Box {{ Box(obj) }}
fn f(x.0: obj, c.1: enum) -> obj {{
  unpack x.0 P #0(a.2: obj, b.3: obj)
  release x.0 P #0(a.2, b.3)
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  decref b.3
  let d.4: obj = con Box #0(a.2)
  return d.4
b2:
  let s.5: obj = extern Prelude.++(a.2, b.3)
  return s.5
}}
"
    ));
}

#[test]
fn a_token_crosses_a_non_yielding_call() {
    let text = format!(
        "{P2}fn g(s.0: obj) -> obj {{
  return s.0
}}
fn f(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj)
  release x.0 P #0(a.1, b.2)
  let c.3: obj = call g(a.1)
  let d.4: obj = con P #0(c.3, b.2)
  return d.4
}}
"
    );
    insta::assert_snapshot!(function(&reuse_text(&text), "f"), @"
    fn f(x.0: obj) -> obj {
      unpack x.0 P #0(a.1: obj, b.2: obj)
      let r.5: token = release x.0 P #0(a.1, b.2)
      let c.3: obj = call g(a.1)
      let d.4: obj = con P #0(c.3, =b.2) reuse r.5 in place
      return d.4
    }
    ");
}

#[test]
fn a_token_does_not_cross_a_yielding_call() {
    // 継続を2回再開すると、同じ箱に2つの値を作ってしまう (docs/spec/core-ir.md の「yield の地点」)
    assert_unchanged(&format!(
        "{P2}yields fn g(s.0: obj) -> obj {{
  return s.0
}}
yields fn f(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj)
  release x.0 P #0(a.1, b.2)
  let c.3: obj = call g(a.1)
  let d.4: obj = con P #0(c.3, b.2)
  return d.4
}}
"
    ));
}

#[test]
fn three_tokens_go_to_the_cons_with_the_most_skips() {
    // `bench/upsert.em` の `split` を融合した形。最も後に定義した token を先に選ぶと、x.46 の token が d.54 に行き、
    // 書かないフィールドがなくなる
    let text = "\
layout Tree { Leaf, Node(int, tobj, tobj, tobj, tobj) }
fn split(t.13: tobj) -> tobj {
  switch t.13 Tree { #1(level.37: int, a.38: tobj, k.39: obj, v.96: tobj, x.41: tobj) -> b1, _ -> b2 }
b1:
  switch x.41 Tree { #1(right_level.42: int, b.43: tobj, rk.44: obj, rv.97: tobj, x.46: tobj) -> b3, _ -> b4 }
b2:
  return t.13
b3:
  switch x.46 Tree { #1(grand_level.47: int, c.48: tobj, gk.49: obj, gv.98: tobj, d.51: tobj) -> b5, _ -> b6 }
b4:
  return t.13
b5:
  let t.53: int = extern \"Prelude.Num Int.+\"(right_level.42, 1)
  release t.13 Tree #1(_, a.38, k.39, v.96, x.41)
  release x.41 Tree #1(_, b.43, rk.44, rv.97, x.46)
  release x.46 Tree #1(_, c.48, gk.49, gv.98, d.51)
  let d.54: tobj = con Tree #1(level.37, a.38, k.39, v.96, b.43)
  let d.55: tobj = con Tree #1(grand_level.47, c.48, gk.49, gv.98, d.51)
  let d.56: tobj = con Tree #1(t.53, d.54, rk.44, rv.97, d.55)
  return d.56
b6:
  return t.13
}
";
    insta::assert_snapshot!(function(&reuse_text(text), "split"), @"
    fn split(t.13: tobj) -> tobj {
      switch t.13 Tree { #1(level.37: int, a.38: tobj, k.39: obj, v.96: tobj, x.41: tobj) -> b1, _ -> b2 }
    b1:
      switch x.41 Tree { #1(right_level.42: int, b.43: tobj, rk.44: obj, rv.97: tobj, x.46: tobj) -> b3, _ -> b4 }
    b2:
      return t.13
    b3:
      switch x.46 Tree { #1(grand_level.47: int, c.48: tobj, gk.49: obj, gv.98: tobj, d.51: tobj) -> b5, _ -> b6 }
    b4:
      return t.13
    b5:
      let t.53: int = extern \"Prelude.Num Int.+\"(right_level.42, 1)
      let r.99: token = release t.13 Tree #1(_, a.38, k.39, v.96, x.41)
      let r.100: token = release x.41 Tree #1(_, b.43, rk.44, rv.97, x.46)
      let r.101: token = release x.46 Tree #1(_, c.48, gk.49, gv.98, d.51)
      let d.54: tobj = con Tree #1(=level.37, =a.38, =k.39, =v.96, b.43) reuse r.99 in place
      let d.55: tobj = con Tree #1(=grand_level.47, =c.48, =gk.49, =gv.98, =d.51) reuse r.101 in place
      let d.56: tobj = con Tree #1(t.53, d.54, =rk.44, =rv.97, d.55) reuse r.100 in place
      return d.56
    b6:
      return t.13
    }
    ");
}

const LIST: &str = "layout Prelude.List { Nil, ::(tobj, tobj) }\n";

/// std の `List.map` のループの Perceus の後の形。`b2` の本体は `body` で、`ys` を手放す `release` が新しい `con` の
/// 後にある。`g` は `yields` の印で選ぶ自分を呼ぶ関数である。
fn map_loop(yields: &str, body: &str) -> String {
    format!(
        "{LIST}{yields}fn go(acc.1: tobj, ys.2: tobj) -> tobj {{
  switch ys.2 Prelude.List {{ #0 -> b1, #1(x.12: tobj, rest.4: tobj) -> b2 }}
b1:
  decref ys.2
  return acc.1
b2:
{body}}}
{yields}fn g(n.0: int) -> int {{
  return n.0
}}
"
    )
}

#[test]
fn a_release_after_a_con_moves_up_to_it() {
    let text = map_loop(
        "",
        "  let x.3: int = unbox x.12
  let t.10: int = extern \"Prelude.Num Int.*\"(x.3, 3)
  let t.11: int = extern \"Prelude.Num Int.+\"(t.10, 1)
  let t.13: tobj = box t.11
  let d.7: tobj = con Prelude.List #1(t.13, acc.1)
  release ys.2 Prelude.List #1(_, rest.4)
  tail call go(d.7, rest.4)
",
    );
    insta::assert_snapshot!(function(&reuse_text(&text), "go"), @"
    fn go(acc.1: tobj, ys.2: tobj) -> tobj {
      switch ys.2 Prelude.List { #0 -> b1, #1(x.12: tobj, rest.4: tobj) -> b2 }
    b1:
      decref ys.2
      return acc.1
    b2:
      let x.3: int = unbox x.12
      let t.10: int = extern \"Prelude.Num Int.*\"(x.3, 3)
      let t.11: int = extern \"Prelude.Num Int.+\"(t.10, 1)
      let t.13: tobj = box t.11
      let r.14: token = release ys.2 Prelude.List #1(_, rest.4)
      let d.7: tobj = con Prelude.List #1(t.13, acc.1) reuse r.14 in place
      tail call go(d.7, rest.4)
    }
    ");
}

#[test]
fn a_release_does_not_move_above_a_use_of_an_unkept_field() {
    // `unbox x.12` が `con` と `release` の間にある。`release` を上げると、手放した x.12 を読むことになる
    assert_unchanged(&map_loop(
        "",
        "  dup acc.1
  let d.7: tobj = con Prelude.List #1(acc.1, acc.1)
  let x.3: int = unbox x.12
  release ys.2 Prelude.List #1(_, rest.4)
  let t.10: int = extern \"Prelude.Num Int.*\"(x.3, 3)
  tail call go(d.7, rest.4)
",
    ));
}

#[test]
fn a_release_does_not_move_across_a_yielding_call() {
    assert_unchanged(&map_loop(
        "yields ",
        "  let x.3: int = unbox x.12
  let t.13: tobj = box x.3
  let d.7: tobj = con Prelude.List #1(t.13, acc.1)
  let n.5: int = call g(x.3)
  release ys.2 Prelude.List #1(_, rest.4)
  tail call go(d.7, rest.4)
",
    ));
}

#[test]
fn a_moved_release_without_a_partner_stays_put() {
    // 前にある `con` は1語の箱なので、2語の `ys` の箱とは組にならない
    assert_unchanged(&format!(
        "layout Box {{ Box(tobj) }}\n{}",
        map_loop(
            "",
            "  let x.3: int = unbox x.12
  let t.13: tobj = box x.3
  let d.7: obj = con Box #0(t.13)
  decref acc.1
  release ys.2 Prelude.List #1(_, rest.4)
  tail call go(d.7, rest.4)
",
        )
    ));
}

/// 2つの `release` が同じブロックで上がる。`blocker` は2つの `con` の間に置く文である。
fn two_moves(blocker: &str) -> String {
    format!(
        "layout P {{ P(tobj, tobj) }}
layout T {{ T(tobj, tobj, tobj, int) }}
fn f(x.0: obj, y.1: obj, p.2: tobj, q.3: tobj) -> obj {{
  unpack x.0 P #0(a.4: tobj, b.5: tobj)
  unpack y.1 P #0(c.6: tobj, d.7: tobj)
  dup p.2
  let e.8: obj = con P #0(p.2, q.3)
{blocker}  let g.10: obj = con P #0(p.2, e.8)
  release x.0 P #0(a.4, _)
  release y.1 P #0(_, d.7)
  let h.11: obj = con T #0(g.10, a.4, d.7, n.9)
  return h.11
}}
"
    )
}

#[test]
fn two_releases_move_up_in_one_block() {
    // 間に何もなければ、先の `release` が前の `con` へ、後の `release` が後ろの `con` へ上がり、順は変わらない
    let text = two_moves("").replace("  let e.8", "  let n.9: int = unbox b.5\n  let e.8");
    insta::assert_snapshot!(function(&reuse_text(&text), "f"), @"
    fn f(x.0: obj, y.1: obj, p.2: tobj, q.3: tobj) -> obj {
      unpack x.0 P #0(a.4: tobj, b.5: tobj)
      unpack y.1 P #0(c.6: tobj, d.7: tobj)
      dup p.2
      let n.9: int = unbox b.5
      let r.12: token = release x.0 P #0(a.4, _)
      let e.8: obj = con P #0(p.2, q.3) reuse r.12 in place
      let r.13: token = release y.1 P #0(_, d.7)
      let g.10: obj = con P #0(p.2, e.8) reuse r.13 in place
      let h.11: obj = con T #0(g.10, a.4, d.7, n.9)
      return h.11
    }
    ");
    // x の残さないフィールド b.5 を2つの `con` の間で読むので、x の `release` は後ろの `con` までしか上がらない。y の
    // `release` は前の `con` へ上がるので、2つの `release` の順が入れ替わる
    let text = two_moves("  let n.9: int = unbox b.5\n");
    insta::assert_snapshot!(function(&reuse_text(&text), "f"), @"
    fn f(x.0: obj, y.1: obj, p.2: tobj, q.3: tobj) -> obj {
      unpack x.0 P #0(a.4: tobj, b.5: tobj)
      unpack y.1 P #0(c.6: tobj, d.7: tobj)
      dup p.2
      let r.13: token = release y.1 P #0(_, d.7)
      let e.8: obj = con P #0(p.2, q.3) reuse r.13 in place
      let n.9: int = unbox b.5
      let r.12: token = release x.0 P #0(a.4, _)
      let g.10: obj = con P #0(p.2, e.8) reuse r.12 in place
      let h.11: obj = con T #0(g.10, a.4, d.7, n.9)
      return h.11
    }
    ");
}

#[test]
fn a_moved_decref_source() {
    // レコードの更新の形で、`decref` が新しい `con` の後にある
    let text = "\
layout Stats { Stats(int, int) }
fn bump(st.0: obj) -> obj {
  unpack st.0 Stats #0(n.1: int, k.2: int)
  let m.3: int = extern \"Prelude.Num Int.+\"(n.1, 1)
  let d.4: obj = con Stats #0(m.3, 0)
  decref st.0
  return d.4
}
";
    insta::assert_snapshot!(reuse_text(text), @"
    layout Stats { Stats(int, int) }
    fn bump(st.0: obj) -> obj {
      unpack st.0 Stats #0(n.1: int, k.2: int)
      let m.3: int = extern \"Prelude.Num Int.+\"(n.1, 1)
      let r.5: token = release st.0 Stats #0(_, _)
      let d.4: obj = con Stats #0(m.3, 0) reuse r.5 in place
      return d.4
    }
    ");
}

#[test]
fn a_moved_source_keeps_its_skips() {
    // 残すフィールド a.2 は、上げた `release` の後に所有になるので、`con` は元と同じ位置の a.2 を書かない
    let text = format!(
        "{P2}fn f(x.0: obj, q.1: obj) -> obj {{
  unpack x.0 P #0(a.2: obj, b.3: obj)
  dup a.2
  let d.4: obj = con P #0(a.2, q.1)
  release x.0 P #0(a.2, _)
  let e.5: obj = con P #0(d.4, a.2)
  return e.5
}}
"
    );
    insta::assert_snapshot!(reuse_text(&text), @"
    layout P { P(obj, obj) }
    fn f(x.0: obj, q.1: obj) -> obj {
      unpack x.0 P #0(a.2: obj, b.3: obj)
      dup a.2
      let r.6: token = release x.0 P #0(a.2, _)
      let d.4: obj = con P #0(=a.2, q.1) reuse r.6 in place
      let e.5: obj = con P #0(d.4, a.2)
      return e.5
    }
    ");
}

#[test]
fn reuse_of_a_function_does_not_depend_on_the_rest_of_the_program() {
    let f = "\
fn f(x.0: obj) -> obj {
  unpack x.0 P #0(a.1: obj, b.2: obj)
  release x.0 P #0(a.1, b.2)
  let d.3: obj = con P #0(b.2, a.1)
  return d.3
}
";
    let other = "\
fn h(y.0: obj, z.1: obj) -> obj {
  unpack y.0 P #0(a.2: obj, b.3: obj)
  release y.0 P #0(a.2, b.3)
  unpack z.1 P #0(c.4: obj, d.5: obj)
  release z.1 P #0(c.4, d.5)
  let e.6: obj = con P #0(a.2, d.5)
  let g.7: obj = con P #0(c.4, b.3)
  let k.8: obj = con P #0(e.6, g.7)
  return k.8
}
";
    let alone = reuse_text(&format!("{P2}{f}"));
    let together = reuse_text(&format!("{P2}{other}{f}"));
    assert_eq!(function(&alone, "f"), function(&together, "f"));
}

#[test]
fn swapping_tuple_elements_still_reuses_the_box() {
    // 構造体の `unbox` の箱を解く `release unboxed` も reuse の元になる。組の要素を入れ替えて箱に戻す形では、今までの
    // `release` と `con … reuse` の組が構造体の後も残る
    // (docs/spec/lir.md の「reuse の元」)
    let text = "\
layout (,) { (,)(tobj, tobj) }
internal role body fn swap(p.0: (,){float, float}) -> (,){float, float} {
  unpack p.0 (,) #0(a.1: float, b.2: float)
  let q.3: (,){float, float} = con (,) #0(b.2, a.1)
  return q.3
}
";
    insta::assert_snapshot!(function(&passes_text(text, Pass::Reuse), "swap"), @"
    internal role body fn swap(p.4: tobj) -> tobj {
      let (p.0.6: tobj, p.1.8: tobj) = unbox p.4 (,)
      let r.10: token = release unboxed p.4 (,) #0(p.0.6, p.1.8)
      let q.5: obj = con (,) #0(p.1.8, p.0.6) reuse r.10 in place
      return q.5
    }
    ");
}

#[test]
fn a_decref_of_an_unboxed_box_becomes_a_marked_token() {
    // 葉をどれも残さない箱は、Perceus が `decref` で手放す。構築子は構造体の `unbox` で分かっているので reuse の元に
    // なり、token を作る `release` にも印を付ける。箱がダミーなら空の token を作るためである
    let text = "\
layout Tok { Tok(int, float) }
internal role body fn bump(t.0: Tok{int, float}) -> Tok{int, float} {
  unpack t.0 Tok #0(n.1: int, x.2: float)
  let m.3: int = extern \"Prelude.Num Int.+\"(n.1, 1)
  let s.4: Tok{int, float} = con Tok #0(m.3, x.2)
  return s.4
}
";
    insta::assert_snapshot!(function(&passes_text(text, Pass::Reuse), "bump"), @r#"
    internal role body fn bump(t.5: tobj) -> tobj {
      let (t.0.7: int, t.1.8: float) = unbox t.5 Tok
      let r.9: token = release unboxed t.5 Tok #0(_, _)
      let m.3: int = extern "Prelude.Num Int.+"(t.0.7, 1)
      let s.6: obj = con Tok #0(m.3, =t.1.8) reuse r.9 in place
      return s.6
    }
    "#);
}

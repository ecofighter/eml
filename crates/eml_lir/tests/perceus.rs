//! 生存解析と Perceus の `dup` / `decref` / `release` の位置、呼び出しの `saved` (docs/spec/core-ir.md の「パス」)。
//! IR のテキストを入力にするテストは、入力が scope の段の verifier を、出力が所有の段の verifier を通ることも
//! 確かめる。ソースからパイプラインを通すテストは `eml_core_ir` の tests/perceus.rs にある。

use eml_lir::liveness::live_in;
use eml_lir::{VarId, parse, perceus, pretty, verify, verify_scopes};

fn perceus_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_scopes(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    perceus(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

#[test]
fn live_in_holds_every_variable_used_later() {
    // RC の対象でない変数も入る。`saved` が使うためである。jump の引数と case のフィールドは行き先の入口で生きている
    let program = parse(
        "\
layout Prelude.Bool { False, True }
layout Option { None, Some(tobj) }
fn f(x.0: int, o.1: tobj) -> int {
  let c.2: enum = extern \"Prelude.Ord Int.<\"(x.0, 10)
  switch c.2 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b4(x.0)
b2:
  switch o.1 Option { #0 -> b3, #1(y.3: int) -> b5 }
b3:
  jump b4(1)
b4(t.4: int):
  return t.4
b5:
  return y.3
}
",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let sets = live_in(&program.functions[0]);
    let expected: [&[u32]; 6] = [&[0, 1], &[0], &[1], &[], &[4], &[3]];
    assert_eq!(
        sets,
        expected.map(|set| set.iter().copied().map(VarId).collect::<Vec<_>>())
    );
}

#[test]
fn a_second_use_is_dupped() {
    let text = "\
fn twice(s.0: obj) -> obj {
  let t.1: obj = extern Prelude.++(s.0, s.0)
  return t.1
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn twice(s.0: obj) -> obj {
      dup s.0
      let t.1: obj = extern Prelude.++(s.0, s.0)
      return t.1
    }
    ");
}

#[test]
fn unused_parameters_are_released_at_the_start() {
    // RC の対象でない引数は数えない
    let text = "\
fn first(a.0: obj, b.1: tobj, n.2: int) -> obj {
  return a.0
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn first(a.0: obj, b.1: tobj, n.2: int) -> obj {
      decref b.1
      return a.0
    }
    ");
}

#[test]
fn a_variable_that_dies_on_one_edge_is_released_in_that_block() {
    // `b1` は `s.1` を使わずに `b3` へ行くので、入口で捨てる。`b2` は `s.1` を引数で渡す
    let text = "\
layout Prelude.Bool { False, True }
fn pick(c.0: enum, s.1: obj) -> obj {
  switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let t.2: obj = const \"none\"
  jump b3(t.2)
b2:
  jump b3(s.1)
b3(r.3: obj):
  return r.3
}
";
    insta::assert_snapshot!(perceus_text(text), @r#"
    layout Prelude.Bool { False, True }
    fn pick(c.0: enum, s.1: obj) -> obj {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      decref s.1
      let t.2: obj = const "none"
      jump b3(t.2)
    b2:
      jump b3(s.1)
    b3(r.3: obj):
      return r.3
    }
    "#);
}

#[test]
fn an_unused_parameter_of_a_merge_block_is_released_at_its_start() {
    let text = "\
layout Prelude.Bool { False, True }
fn ignore(c.0: enum, s.1: obj) -> int {
  switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(s.1)
b2:
  let t.2: obj = extern Prelude.++(s.1, s.1)
  jump b3(t.2)
b3(r.3: obj):
  return 0
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    layout Prelude.Bool { False, True }
    fn ignore(c.0: enum, s.1: obj) -> int {
      switch c.0 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(s.1)
    b2:
      dup s.1
      let t.2: obj = extern Prelude.++(s.1, s.1)
      jump b3(t.2)
    b3(r.3: obj):
      decref r.3
      return 0
    }
    ");
}

#[test]
fn an_unused_unpack_field_is_released_after_the_unpack() {
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn first(p.0: obj) -> obj {
  unpack p.0 (,) #0(a.1: obj, b.2: obj)
  return a.1
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    layout (,) { (,)(tobj, tobj) }
    fn first(p.0: obj) -> obj {
      unpack p.0 (,) #0(a.1: obj, b.2: obj)
      release p.0 (,) #0(a.1, _)
      return a.1
    }
    ");
}

#[test]
fn a_live_unpacked_value_dups_the_fields_used_later() {
    // `unpack` は値を読むだけなので、後でも使う `p` は複製しない。借りたフィールドのうち使う `b` だけを複製する
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn again(p.0: obj) -> obj {
  unpack p.0 (,) #0(a.1: tobj, b.2: obj)
  let r.3: obj = con (,) #0(p.0, b.2)
  return r.3
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    layout (,) { (,)(tobj, tobj) }
    fn again(p.0: obj) -> obj {
      unpack p.0 (,) #0(a.1: tobj, b.2: obj)
      dup b.2
      let r.3: obj = con (,) #0(p.0, b.2)
      return r.3
    }
    ");
}

#[test]
fn a_target_releases_its_scrutinee_and_keeps_the_fields_it_uses() {
    // どの行き先も scrutinee を所有して始まる。フィールドのない b1 は scrutinee をそのまま返し、b2 は使う `t` だけを
    // 残して scrutinee を手放す
    let text = "\
layout List { Nil, Cons(tobj, tobj) }
fn describe(xs.0: tobj) -> tobj {
  switch xs.0 List { #0 -> b1, #1(h.1: obj, t.2: tobj) -> b2 }
b1:
  return xs.0
b2:
  return t.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    layout List { Nil, Cons(tobj, tobj) }
    fn describe(xs.0: tobj) -> tobj {
      switch xs.0 List { #0 -> b1, #1(h.1: obj, t.2: tobj) -> b2 }
    b1:
      return xs.0
    b2:
      release xs.0 List #1(_, t.2)
      return t.2
    }
    ");
}

#[test]
fn a_field_used_twice_is_dupped_after_the_release() {
    let text = "\
layout Option { None, Some(tobj) }
fn twice(o.0: tobj) -> obj {
  switch o.0 Option { #0 -> b1, #1(s.1: obj) -> b2 }
b1:
  let e.2: obj = const \"e\"
  return e.2
b2:
  let t.3: obj = extern Prelude.++(s.1, s.1)
  return t.3
}
";
    insta::assert_snapshot!(perceus_text(text), @r#"
    layout Option { None, Some(tobj) }
    fn twice(o.0: tobj) -> obj {
      switch o.0 Option { #0 -> b1, #1(s.1: obj) -> b2 }
    b1:
      decref o.0
      let e.2: obj = const "e"
      return e.2
    b2:
      release o.0 Option #1(s.1)
      dup s.1
      let t.3: obj = extern Prelude.++(s.1, s.1)
      return t.3
    }
    "#);
}

#[test]
fn a_field_used_after_a_call_is_owned_before_the_call_saves_it() {
    // 借りたフィールドは `save` に入れられないので、入口の `release` で所有にしてから退避する
    let text = "\
layout Option { None, Some(tobj) }
fn keep(o.0: tobj) -> obj {
  switch o.0 Option { #0 -> b1, #1(s.1: obj) -> b2 }
b1:
  let e.2: obj = const \"e\"
  return e.2
b2:
  let n.3: int = call k(1)
  return s.1
}
fn k(a.0: int) -> int {
  return a.0
}
";
    insta::assert_snapshot!(perceus_text(text), @r#"
    layout Option { None, Some(tobj) }
    fn keep(o.0: tobj) -> obj {
      switch o.0 Option { #0 -> b1, #1(s.1: obj) -> b2 }
    b1:
      decref o.0
      let e.2: obj = const "e"
      return e.2
    b2:
      release o.0 Option #1(s.1)
      let n.3: int = call k(1) save [s.1]
      return s.1
    }
    fn k(a.0: int) -> int {
      return a.0
    }
    "#);
}

#[test]
fn an_unused_call_result_is_released_and_calls_save_what_is_used_later() {
    // `saved` は呼び出しの後で生きている変数から結果の変数を除いたもので、RC の対象でない `n.1` も入る
    let text = "\
fn run(s.0: obj, n.1: int) -> int {
  let r.2: obj = call echo(s.0)
  let m.3: int = extern Prelude.+(n.1, 1)
  return m.3
}
fn echo(s.0: obj) -> obj {
  return s.0
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn run(s.0: obj, n.1: int) -> int {
      let r.2: obj = call echo(s.0) save [n.1]
      decref r.2
      let m.3: int = extern Prelude.+(n.1, 1)
      return m.3
    }
    fn echo(s.0: obj) -> obj {
      return s.0
    }
    ");
}

#[test]
fn an_argument_used_after_the_call_is_dupped_and_saved() {
    let text = "\
fn around(s.0: obj) -> obj {
  let t.1: obj = call echo(s.0)
  let u.2: obj = extern Prelude.++(t.1, s.0)
  return u.2
}
fn echo(s.0: obj) -> obj {
  return s.0
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn around(s.0: obj) -> obj {
      dup s.0
      let t.1: obj = call echo(s.0) save [s.0]
      let u.2: obj = extern Prelude.++(t.1, s.0)
      return u.2
    }
    fn echo(s.0: obj) -> obj {
      return s.0
    }
    ");
}

#[test]
fn a_field_passed_to_an_arm_and_used_after_it_is_dupped_before_the_jump() {
    // case-of-case で枝のラベルへフィールドを渡し、枝の後でも使う形。`b3` は `v.2` を引数で渡し、`b5` でも使うので
    // jump の前で複製する。`b4` は `v.2` を渡さないので、所有をそのまま `b5` へ持っていく
    let text = "\
layout Option { None, Some(tobj) }
layout Prelude.Bool { False, True }
layout (,) { (,)(tobj, tobj) }
fn pair(o.0: tobj, c.1: enum) -> obj {
  switch o.0 Option { #0 -> b1, #1(v.2: obj) -> b2 }
b1:
  let e.3: obj = const \"none\"
  return e.3
b2:
  switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
b3:
  jump b5(v.2)
b4:
  let n.4: obj = const \"n\"
  jump b5(n.4)
b5(x.5: obj):
  let r.6: obj = con (,) #0(x.5, v.2)
  return r.6
}
";
    insta::assert_snapshot!(perceus_text(text), @r#"
    layout Option { None, Some(tobj) }
    layout Prelude.Bool { False, True }
    layout (,) { (,)(tobj, tobj) }
    fn pair(o.0: tobj, c.1: enum) -> obj {
      switch o.0 Option { #0 -> b1, #1(v.2: obj) -> b2 }
    b1:
      decref o.0
      let e.3: obj = const "none"
      return e.3
    b2:
      release o.0 Option #1(v.2)
      switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      dup v.2
      jump b5(v.2)
    b4:
      let n.4: obj = const "n"
      jump b5(n.4)
    b5(x.5: obj):
      let r.6: obj = con (,) #0(x.5, v.2)
      return r.6
    }
    "#);
}

#[test]
fn a_tail_call_that_passes_a_variable_twice_dups_it_once() {
    let text = "\
fn both(s.0: obj) -> obj {
  tail call join(s.0, s.0)
}
fn join(a.0: obj, b.1: obj) -> obj {
  let c.2: obj = extern Prelude.++(a.0, b.1)
  return c.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn both(s.0: obj) -> obj {
      dup s.0
      tail call join(s.0, s.0)
    }
    fn join(a.0: obj, b.1: obj) -> obj {
      let c.2: obj = extern Prelude.++(a.0, b.1)
      return c.2
    }
    ");
}

#[test]
fn an_unboxed_value_that_dies_is_released_after_the_unbox() {
    // `unbox` は値を読むだけなので、最後の使いなら読んだ直後に手放す
    let text = "\
fn f(b.0: tobj) -> int {
  let n.1: int = unbox b.0
  let m.2: int = extern Prelude.+(n.1, 1)
  return m.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    fn f(b.0: tobj) -> int {
      let n.1: int = unbox b.0
      decref b.0
      let m.2: int = extern Prelude.+(n.1, 1)
      return m.2
    }
    ");
}

#[test]
fn an_unboxed_value_used_later_is_neither_dupped_nor_released() {
    let text = "\
fn f(b.0: tobj) -> tobj {
  let n.1: int = unbox b.0
  let s.2: obj = extern \"Prelude.Show Int.show\"(n.1)
  let u.3: unit = extern Prelude.println(s.2)
  return b.0
}
";
    assert_eq!(perceus_text(text), text);
}

#[test]
fn an_unboxed_field_is_owned_at_the_target_and_released_after_the_unbox() {
    // 行き先の入口でフィールドを所有にするので、`unbox` の後の `decref` は所有を手放すだけである
    let text = "\
layout Option { None, Some(tobj) }
fn f(o.0: tobj) -> int {
  switch o.0 Option { #0 -> b1, #1(x.1: tobj) -> b2 }
b1:
  return 0
b2:
  let n.2: int = unbox x.1
  return n.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    layout Option { None, Some(tobj) }
    fn f(o.0: tobj) -> int {
      switch o.0 Option { #0 -> b1, #1(x.1: tobj) -> b2 }
    b1:
      decref o.0
      return 0
    b2:
      release o.0 Option #1(x.1)
      let n.2: int = unbox x.1
      decref x.1
      return n.2
    }
    ");
}

/// 生存解析、Perceus、verifier、テキストの表示と読み込みは、プログラムの大きさに比例して Rust のスタックを使わない
/// (docs/spec/core-ir.md の「パス」)。2万の条件の列を debug ビルドで処理し、どの条件の枝でも使わない文字列を1つずつ
/// 捨てることを確かめる。translate から通す長い列は `eml_core_ir` の tests/verify.rs が確かめる。
#[test]
fn twenty_thousand_conditions_are_rewritten() {
    const CONDITIONS: u32 = 20_000;
    let mut text = String::from(
        "layout Prelude.Bool { False, True }\nfn entry$main(b.0: enum, s.1: obj) -> obj {\n",
    );
    for n in 0..CONDITIONS {
        let base = 3 * n;
        if n > 0 {
            text.push_str(&format!("b{base}:\n"));
        }
        text.push_str(&format!(
            "  switch b.0 Prelude.Bool {{ #0 -> b{}, #1 -> b{} }}\nb{}:\n  jump b{}()\nb{}:\n  let t.{}: obj = const \"x\"\n  jump b{}()\n",
            base + 1,
            base + 2,
            base + 1,
            base + 3,
            base + 2,
            n + 2,
            base + 3
        ));
    }
    text.push_str(&format!("b{}:\n  return s.1\n}}\n", 3 * CONDITIONS));
    let shown = perceus_text(&text);
    assert_eq!(shown.matches("decref t.").count(), CONDITIONS as usize);
    assert!(!shown.contains("dup"));
}

#[test]
fn a_target_dups_the_fields_of_a_live_scrutinee_before_it_decrefs_a_dead_value() {
    // `o` がまだ生きているので、そのフィールドを複製する。順は入口の順で、死んだ `y` の `decref` より先になる
    // (docs/spec/core-ir.md の「Perceus」)
    let text = "\
layout Option { None, Some(tobj) }
layout (,) { (,)(tobj, tobj) }
fn f(o.0: tobj, y.1: obj) -> obj {
  switch o.0 Option { #0 -> b1, #1(s.2: obj) -> b2 }
b1:
  return y.1
b2:
  let p.3: obj = con (,) #0(s.2, o.0)
  return p.3
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    layout Option { None, Some(tobj) }
    layout (,) { (,)(tobj, tobj) }
    fn f(o.0: tobj, y.1: obj) -> obj {
      switch o.0 Option { #0 -> b1, #1(s.2: obj) -> b2 }
    b1:
      decref o.0
      return y.1
    b2:
      dup s.2
      decref y.1
      let p.3: obj = con (,) #0(s.2, o.0)
      return p.3
    }
    ");
}

#[test]
#[should_panic(expected = "a switch with tag cases has a layout")]
fn a_tag_switch_without_a_layout_stops_perceus() {
    // `release` に写す `Ctor` が決まらない。黙って飛ばすと、フィールドを所有にする `release` が抜ける
    let mut program = parse(
        "fn f(d.0: tobj) -> obj {\n  switch d.0 { #0 -> b1, #1(x.1: obj) -> b2 }\nb1:\n  let e.2: obj = const \"e\"\n  return e.2\nb2:\n  return x.1\n}\n",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    perceus(&mut program);
}

#[test]
fn prims_consume_their_arguments() {
    // 基本操作も節の呼び出しも引数を消費するので、`e.0` を2回使うと1回目の前に `dup e.0` が入る
    let text = "\
effect Counter { get(unit) -> int, put(int) -> unit }
yields fn f(e.0: obj) -> int {
  let c.1: tobj = prim ev_clause[Counter.get](e.0)
  let m.2: int = clause Counter.get c.1(e.0, ())
  return m.2
}
";
    insta::assert_snapshot!(perceus_text(text), @"
    effect Counter { get(unit) -> int, put(int) -> unit }
    yields fn f(e.0: obj) -> int {
      dup e.0
      let c.1: tobj = prim ev_clause[Counter.get](e.0)
      let m.2: int = clause Counter.get c.1(e.0, ())
      return m.2
    }
    ");
}

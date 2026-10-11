//! 生存解析と Perceus の `dup` / `decref` / `release` の位置、呼び出しの `saved` (docs/spec/lir.md の「Perceus」)。
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
fn a_dead_scrutinee_is_released_where_its_fields_are_needed() {
    // どの行き先も scrutinee を所有して始まり、フィールドは借りて始まる。フィールドのない b1 は scrutinee をそのまま
    // 返す。b2 は `t` を返す所で、死んだ scrutinee を `release` で解いて `t` だけを受け取る
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
    // 借りたフィールドは `save` に入れられないので、呼び出しの前の `release` で所有にしてから退避する
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
    // jump の前で複製する。`b5` の入口で生きている `v.2` は、どちらの辺でも jump の前で `o` を解いて所有にする
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
      switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      release o.0 Option #1(v.2)
      dup v.2
      jump b5(v.2)
    b4:
      let n.4: obj = const "n"
      release o.0 Option #1(v.2)
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
fn an_unboxed_field_stays_borrowed_and_its_owner_is_decrefed_after_it() {
    // `unbox` は読むだけなので、フィールドは借りたまま読む。最後の属する変数 `x` が `unbox` で死ぬので、その直後に
    // 持ち主の `o` を `decref` する
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
      let n.2: int = unbox x.1
      decref o.0
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
fn a_field_of_a_live_scrutinee_is_dupped_where_it_is_consumed() {
    // 死んだ `y` は行き先の入口で捨てる。`s` は `con` が消費する所で所有を取り、持ち主の `o` もその `con` が使うので
    // 複製する (docs/spec/lir.md の「所有を取る時点」)
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
      decref y.1
      dup s.2
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
fn prims_read_borrowed_arguments_and_consume_the_rest() {
    // `ev_clause` は evidence を借りるだけなので、`e.0` の所有は節の呼び出しまで残り、`dup e.0` は入らない。節の
    // 呼び出しは引数を消費する (docs/spec/lir.md の「基本操作」)
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
      let c.1: tobj = prim ev_clause[Counter.get](e.0)
      let m.2: int = clause Counter.get c.1(e.0, ())
      return m.2
    }
    ");
}

#[test]
fn a_borrowed_prim_argument_is_read() {
    // `ev_take_state` と `ev_set_state` は evidence を借りるので、2回使っても `dup` しない。evidence は最後に使った
    // 文の直後で `decref` する。借りたフィールドもそのまま渡す
    let shown = perceus_text(
        "\
layout W { W(obj) }
fn f(e.0: obj) -> unit {
  let s.1: tobj = prim ev_take_state(e.0)
  let u.2: unit = prim ev_set_state(e.0, s.1)
  return u.2
}
fn g(w.0: obj) -> tobj {
  unpack w.0 W #0(e.1: obj)
  let v.2: tobj = prim ev_evv(e.1)
  return v.2
}
",
    );
    assert!(!shown.contains("dup e.0"), "{shown}");
    assert!(!shown.contains("dup e.1"), "{shown}");
    assert!(!shown.contains("release"), "{shown}");
    insta::assert_snapshot!(shown, @"
    layout W { W(obj) }
    fn f(e.0: obj) -> unit {
      let s.1: tobj = prim ev_take_state(e.0)
      let u.2: unit = prim ev_set_state(e.0, s.1)
      decref e.0
      return u.2
    }
    fn g(w.0: obj) -> tobj {
      unpack w.0 W #0(e.1: obj)
      let v.2: tobj = prim ev_evv(e.1)
      decref w.0
      return v.2
    }
    ");
}

#[test]
fn a_field_is_released_from_a_dead_owner_without_a_dup() {
    // `Lin` の形: 持ち主は分解の後に使われないので、フィールドを消費する所で持ち主を解き、`release` で受け取る
    // (docs/spec/lir.md の「値の表現」)
    let shown = perceus_text(
        "\
layout W { W(obj, int) }
fn f(w.0: obj) -> unit {
  unpack w.0 W #0(h.1: obj, n.2: int)
  let u.3: unit = extern Std.Fs.close(h.1)
  return u.3
}
",
    );
    assert!(!shown.contains("dup h.1"), "{shown}");
    insta::assert_snapshot!(shown, @"
    layout W { W(obj, int) }
    fn f(w.0: obj) -> unit {
      unpack w.0 W #0(h.1: obj, n.2: int)
      release w.0 W #0(h.1, _)
      let u.3: unit = extern Std.Fs.close(h.1)
      return u.3
    }
    ");
}

#[test]
fn an_intermediate_target_leaves_the_fields_borrowed() {
    // 決定木の途中の行き先 b2 では、後の行 (b3) が分解した値を使っても、フィールドに RC の命令を置かない
    let shown = perceus_text(
        "\
layout Prelude.Bool { False, True }
layout Option { None, Some(tobj) }
fn f(o.0: tobj, c.1: enum) -> tobj {
  switch o.0 Option { #0 -> b1, #1(x.2: tobj) -> b2 }
b1:
  return o.0
b2:
  switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
b3:
  return o.0
b4:
  return x.2
}
",
    );
    assert!(!shown.contains("dup x.2"), "{shown}");
    insta::assert_snapshot!(shown, @"
    layout Prelude.Bool { False, True }
    layout Option { None, Some(tobj) }
    fn f(o.0: tobj, c.1: enum) -> tobj {
      switch o.0 Option { #0 -> b1, #1(x.2: tobj) -> b2 }
    b1:
      return o.0
    b2:
      switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      return o.0
    b4:
      release o.0 Option #1(x.2)
      return x.2
    }
    ");
}

#[test]
fn a_field_that_is_only_read_gets_no_rc_instruction() {
    // `unbox` は読む使いなので、借りたフィールドのまま読む。最後の属する変数が死んだ所で持ち主を `decref` する
    let shown = perceus_text(
        "\
layout Box { Box(tobj) }
fn f(b.0: obj) -> int {
  unpack b.0 Box #0(t.1: tobj)
  let n.2: int = unbox t.1
  return n.2
}
",
    );
    assert!(!shown.contains("release"), "{shown}");
    assert!(!shown.contains("dup"), "{shown}");
    insta::assert_snapshot!(shown, @"
    layout Box { Box(tobj) }
    fn f(b.0: obj) -> int {
      unpack b.0 Box #0(t.1: tobj)
      let n.2: int = unbox t.1
      decref b.0
      return n.2
    }
    ");
}

#[test]
fn a_field_live_after_a_call_is_owned_before_it() {
    // 持ち主が死んでいれば解き、生きていれば `dup` する。フレームは借りた値を持てない (R7)
    let shown = perceus_text(
        "\
layout Pair { Pair(obj, obj) }
fn g(s.0: obj) -> obj {
  return s.0
}
fn dead_owner(p.0: obj) -> obj {
  unpack p.0 Pair #0(a.1: obj, b.2: obj)
  let r.3: obj = call g(a.1)
  return b.2
}
fn live_owner(p.0: obj) -> obj {
  unpack p.0 Pair #0(a.1: obj, b.2: obj)
  let r.3: obj = call g(a.1)
  let q.4: obj = con Pair #0(p.0, b.2)
  return q.4
}
",
    );
    insta::assert_snapshot!(shown, @"
    layout Pair { Pair(obj, obj) }
    fn g(s.0: obj) -> obj {
      return s.0
    }
    fn dead_owner(p.0: obj) -> obj {
      unpack p.0 Pair #0(a.1: obj, b.2: obj)
      release p.0 Pair #0(a.1, b.2)
      let r.3: obj = call g(a.1) save [b.2]
      decref r.3
      return b.2
    }
    fn live_owner(p.0: obj) -> obj {
      unpack p.0 Pair #0(a.1: obj, b.2: obj)
      dup a.1
      dup b.2
      let r.3: obj = call g(a.1) save [p.0, b.2]
      decref r.3
      let q.4: obj = con Pair #0(p.0, b.2)
      return q.4
    }
    ");
}

#[test]
fn a_field_live_at_a_join_is_owned_before_the_jump() {
    // 合流するブロックは所有だけで始まるので、行き先で生きている a.2 を、どちらの辺でも `jump` の前で所有にする
    let shown = perceus_text(
        "\
layout Prelude.Bool { False, True }
layout Pair { Pair(obj, obj) }
fn f(p.0: obj, c.1: enum) -> obj {
  unpack p.0 Pair #0(a.2: obj, b.3: obj)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(b.3)
b2:
  let s.4: obj = const \"x\"
  jump b3(s.4)
b3(r.5: obj):
  let q.6: obj = con Pair #0(r.5, a.2)
  return q.6
}
",
    );
    insta::assert_snapshot!(shown, @r#"
    layout Prelude.Bool { False, True }
    layout Pair { Pair(obj, obj) }
    fn f(p.0: obj, c.1: enum) -> obj {
      unpack p.0 Pair #0(a.2: obj, b.3: obj)
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      release p.0 Pair #0(a.2, b.3)
      jump b3(b.3)
    b2:
      let s.4: obj = const "x"
      release p.0 Pair #0(a.2, _)
      jump b3(s.4)
    b3(r.5: obj):
      let q.6: obj = con Pair #0(r.5, a.2)
      return q.6
    }
    "#);
}

#[test]
fn a_field_live_after_its_owner_is_consumed_is_dupped_before() {
    // `| Box s -> (Some b, s)` の形。`con` が b の最後の参照を消費した後も s を使うので、その前に `dup` する
    // (spec の「所有を取る時点」の 4.)
    let shown = perceus_text(
        "\
layout Box { Box(obj) }
layout Option { None, Some(tobj) }
layout (,) { (,)(tobj, tobj) }
fn g(b.0: obj) -> obj {
  unpack b.0 Box #0(s.1: obj)
  let d.2: tobj = con Option #1(b.0)
  let d.3: obj = con (,) #0(d.2, s.1)
  return d.3
}
",
    );
    assert!(
        shown.find("dup s.1").unwrap() < shown.find("con Option").unwrap(),
        "{shown}"
    );
    insta::assert_snapshot!(shown, @"
    layout Box { Box(obj) }
    layout Option { None, Some(tobj) }
    layout (,) { (,)(tobj, tobj) }
    fn g(b.0: obj) -> obj {
      unpack b.0 Box #0(s.1: obj)
      dup s.1
      let d.2: tobj = con Option #1(b.0)
      let d.3: obj = con (,) #0(d.2, s.1)
      return d.3
    }
    ");
}

#[test]
fn nested_values_are_released_from_the_parent_down() {
    let shown = perceus_text(
        "\
layout Outer { Outer(obj) }
layout Inner { Inner(obj, int) }
fn f(o.0: obj) -> obj {
  unpack o.0 Outer #0(c.1: obj)
  unpack c.1 Inner #0(g.2: obj, n.3: int)
  return g.2
}
",
    );
    assert!(!shown.contains("dup"), "{shown}");
    assert!(
        shown.find("release o.0").unwrap() < shown.find("release c.1").unwrap(),
        "{shown}"
    );
    insta::assert_snapshot!(shown, @"
    layout Outer { Outer(obj) }
    layout Inner { Inner(obj, int) }
    fn f(o.0: obj) -> obj {
      unpack o.0 Outer #0(c.1: obj)
      unpack c.1 Inner #0(g.2: obj, n.3: int)
      release o.0 Outer #0(c.1)
      release c.1 Inner #0(g.2, _)
      return g.2
    }
    ");
}

#[test]
fn a_descendant_under_a_needed_field_is_dupped_before_the_release() {
    // c.1 と g.2 を同じ `con` が消費する。間の c.1 も必要なので、g.2 は `release` で受け取れず、o.0 がまだ所有されて
    // いる間に `dup` する (spec の「持ち主を解く」の 3.)
    let shown = perceus_text(
        "\
layout Outer { Outer(obj) }
layout Inner { Inner(obj, int) }
layout Pair { Pair(obj, obj) }
fn f(o.0: obj) -> obj {
  unpack o.0 Outer #0(c.1: obj)
  unpack c.1 Inner #0(g.2: obj, n.3: int)
  let p.4: obj = con Pair #0(c.1, g.2)
  return p.4
}
",
    );
    assert!(
        shown.find("dup g.2").unwrap() < shown.find("release o.0").unwrap(),
        "{shown}"
    );
    insta::assert_snapshot!(shown, @"
    layout Outer { Outer(obj) }
    layout Inner { Inner(obj, int) }
    layout Pair { Pair(obj, obj) }
    fn f(o.0: obj) -> obj {
      unpack o.0 Outer #0(c.1: obj)
      unpack c.1 Inner #0(g.2: obj, n.3: int)
      dup g.2
      release o.0 Outer #0(c.1)
      let p.4: obj = con Pair #0(c.1, g.2)
      return p.4
    }
    ");
}

#[test]
fn two_variables_of_one_slot_keep_one_name_in_the_release() {
    // 同じ値を2回分解すると、2つの変数が同じスロットを束縛する。`release` が名前を書けるのは1つなので、番号の小さい
    // a.1 を代表にし、b.3 は `dup` する (spec の「持ち主を解く」の 2.)
    let shown = perceus_text(
        "\
layout P { P(obj, int) }
fn f(p.0: obj) -> obj {
  unpack p.0 P #0(a.1: obj, n.2: int)
  unpack p.0 P #0(b.3: obj, m.4: int)
  let r.5: obj = extern Prelude.++(a.1, b.3)
  return r.5
}
",
    );
    assert!(shown.contains("dup b.3"), "{shown}");
    assert!(shown.contains("release p.0 P #0(a.1, _)"), "{shown}");
    insta::assert_snapshot!(shown, @"
    layout P { P(obj, int) }
    fn f(p.0: obj) -> obj {
      unpack p.0 P #0(a.1: obj, n.2: int)
      unpack p.0 P #0(b.3: obj, m.4: int)
      dup b.3
      release p.0 P #0(a.1, _)
      let r.5: obj = extern Prelude.++(a.1, b.3)
      return r.5
    }
    ");
}

#[test]
fn the_owner_of_a_borrowed_scrutinee_is_kept_at_the_target() {
    // b2 の scrutinee h.1 は借用なので、その持ち主 x.0 が、b2 で生きている v.2 のために保持される。b1 では何も
    // 生きていないので、入口で x.0 を `decref` する
    let shown = perceus_text(
        "\
layout Option { None, Some(tobj) }
layout W { W(tobj) }
fn f(x.0: obj) -> tobj {
  unpack x.0 W #0(h.1: tobj)
  switch h.1 Option { #0 -> b1, #1(v.2: tobj) -> b2 }
b1:
  return ()
b2:
  return v.2
}
",
    );
    insta::assert_snapshot!(shown, @"
    layout Option { None, Some(tobj) }
    layout W { W(tobj) }
    fn f(x.0: obj) -> tobj {
      unpack x.0 W #0(h.1: tobj)
      switch h.1 Option { #0 -> b1, #1(v.2: tobj) -> b2 }
    b1:
      decref x.0
      return ()
    b2:
      release x.0 W #0(h.1)
      release h.1 Option #1(v.2)
      return v.2
    }
    ");
}

#[test]
fn a_field_of_another_constructor_of_the_parent_is_dupped() {
    // x.0 を b1 でもう一度分解し、b2 では E #0 の a.1 と E #1 の c.6 がどちらも生きている。b2 は到達しない行き先
    // だが、Perceus はそれを知らずに x.0 を解く。1つの `release` に書けるのは1つのコンストラクタだけなので、番号の
    // 小さい a.1 の E #0 で x.0 を解き、c.6 は `dup` する (spec の「持ち主を解く」の 2.)
    let shown = perceus_text(
        "\
layout E { L(tobj), R(tobj, tobj) }
layout P { P(tobj, tobj) }
fn f(x.0: obj) -> obj {
  switch x.0 E { #0(a.1: tobj) -> b1, #1(b.2: tobj, c.3: tobj) -> b3 }
b1:
  switch x.0 E { #0(a.4: tobj) -> b4, #1(b.5: tobj, c.6: tobj) -> b2 }
b2:
  let p.7: obj = con P #0(a.1, c.6)
  return p.7
b3:
  return x.0
b4:
  return x.0
}
",
    );
    assert!(shown.contains("dup c.6"), "{shown}");
    assert!(shown.contains("release x.0 E #0(a.1)"), "{shown}");
    insta::assert_snapshot!(shown, @"
    layout E { L(tobj), R(tobj, tobj) }
    layout P { P(tobj, tobj) }
    fn f(x.0: obj) -> obj {
      switch x.0 E { #0(a.1: tobj) -> b1, #1(b.2: tobj, c.3: tobj) -> b3 }
    b1:
      switch x.0 E { #0(a.4: tobj) -> b4, #1(b.5: tobj, c.6: tobj) -> b2 }
    b2:
      dup c.6
      release x.0 E #0(a.1)
      let p.7: obj = con P #0(a.1, c.6)
      return p.7
    b3:
      return x.0
    b4:
      return x.0
    }
    ");
}

#[test]
fn a_descendant_under_an_owned_field_is_dupped_when_the_owner_is_resolved() {
    // s.5 の `++` が c.2 を消費しても c.2 は後で生き、o.0 も生きているので、c.2 を `dup` で所有にする。その後 o.0 は
    // 2回目の `unpack` で死ぬ。t.7 で o.0 を解くとき、g.3 と o.0 の間の c.2 はもう所有されているので、`release` で
    // c.2 から g.3 を受け取らず、g.3 を `dup` する (spec の「持ち主を解く」の 3.)
    let shown = perceus_text(
        "\
layout Outer { Outer(obj) }
layout Inner { Inner(obj, int) }
layout Pair { Pair(obj, obj) }
fn f(o.0: obj, k.1: obj) -> obj {
  unpack o.0 Outer #0(c.2: obj)
  unpack c.2 Inner #0(g.3: obj, n.4: int)
  let s.5: obj = extern Prelude.++(c.2, k.1)
  unpack o.0 Outer #0(c.6: obj)
  let t.7: obj = extern Prelude.++(s.5, g.3)
  let q.8: obj = con Pair #0(t.7, c.2)
  return q.8
}
",
    );
    assert!(shown.contains("dup g.3"), "{shown}");
    assert!(!shown.contains("release"), "{shown}");
    insta::assert_snapshot!(shown, @"
    layout Outer { Outer(obj) }
    layout Inner { Inner(obj, int) }
    layout Pair { Pair(obj, obj) }
    fn f(o.0: obj, k.1: obj) -> obj {
      unpack o.0 Outer #0(c.2: obj)
      unpack c.2 Inner #0(g.3: obj, n.4: int)
      dup c.2
      dup c.2
      let s.5: obj = extern Prelude.++(c.2, k.1)
      unpack o.0 Outer #0(c.6: obj)
      dup g.3
      decref o.0
      let t.7: obj = extern Prelude.++(s.5, g.3)
      let q.8: obj = con Pair #0(t.7, c.2)
      return q.8
    }
    ");
}

#[test]
fn a_long_chain_of_borrowed_switches_is_rewritten_in_linear_time() {
    // 借りたフィールドへの `switch` が深く連なる形。行き先に写す状態は、所有と持ち主ごとの生きている借りた変数
    // だけなので、時間は深さに比例する (spec の「時間」)
    const DEPTH: usize = 20000;
    let mut text = String::from("layout Option { None, Some(tobj) }\nfn f(v.0: tobj) -> tobj {\n");
    // ブロック b(2k+1) は `None` の行き先で `()` を返し、b(2k+2) は次のフィールドへの `switch` を続ける
    for k in 0..DEPTH {
        if k > 0 {
            text.push_str(&format!("b{}:\n", 2 * k));
        }
        text.push_str(&format!(
            "  switch v.{k} Option {{ #0 -> b{}, #1(v.{}: tobj) -> b{} }}\nb{}:\n  return ()\n",
            2 * k + 1,
            k + 1,
            2 * k + 2,
            2 * k + 1
        ));
    }
    text.push_str(&format!("b{}:\n  return v.{DEPTH}\n}}\n", 2 * DEPTH));
    let start = std::time::Instant::now();
    perceus_text(&text);
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
}

/// 生きている変数が多い形の大きさ。debug ビルドで、線形なら形ごとに0.6〜1.6秒で終わり、1つの命令の手間が
/// 生存集合の大きさに比例すると数十秒かかる。
const MANY: usize = 32000;

/// 1つの命令の手間が生存集合の大きさに比例すると、時間は大きさの2乗で伸びる (docs/spec/lir.md の「Perceus の時間」)。
/// 大きさを2倍にしたときの時間の比で見ると、並列に流れるほかのテストの負荷で比がぶれて誤って落ちる。そこで、鎖の
/// テストと同じく、大きい1つの大きさに十分ゆるい上限を置く。
fn assert_linear_in_live_variables(generate: fn(usize) -> String) {
    let text = generate(MANY);
    let start = std::time::Instant::now();
    perceus_text(&text);
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "size {MANY} took {elapsed:?}"
    );
}

/// n 個の引数が使う所まで生き、各文が死ぬ累積を消費する。フィールドはない。
fn many_live_parameters(n: usize) -> String {
    let params: Vec<String> = (0..n).map(|i| format!("a.{i}: obj")).collect();
    let mut text = format!("fn f({}) -> obj {{\n", params.join(", "));
    let mut acc = "a.0".to_string();
    for i in 1..n {
        let var = format!("s.{}", n + i);
        text.push_str(&format!(
            "  let {var}: obj = extern Prelude.++({acc}, a.{i})\n"
        ));
        acc = var;
    }
    text.push_str(&format!("  return {acc}\n}}\n"));
    text
}

/// 1つの持ち主から同じスロットを n 回借り、1つずつ `unbox` で読む。読むたびに、持ち主に属する変数が残るかを確かめる。
fn many_reads_of_one_owner(n: usize) -> String {
    let mut text = String::from("layout Box { Box(tobj) }\nfn f(b.0: obj) -> int {\n");
    for i in 1..=n {
        text.push_str(&format!("  unpack b.0 Box #0(t.{i}: tobj)\n"));
    }
    for i in 1..=n {
        text.push_str(&format!("  let m.{}: int = unbox t.{i}\n", n + i));
    }
    text.push_str(&format!("  return m.{}\n}}\n", 2 * n));
    text
}

/// n 個の持ち主がフィールドを1つずつ貸し、そのフィールドを1つずつ消費する。消費のたびに持ち主を1つ解く。
fn many_owners_resolved_one_by_one(n: usize) -> String {
    let params: Vec<String> = (0..n).map(|i| format!("p.{i}: obj")).collect();
    let mut text = format!(
        "layout Box {{ Box(obj) }}\nfn f({}) -> obj {{\n",
        params.join(", ")
    );
    for i in 0..n {
        text.push_str(&format!("  unpack p.{i} Box #0(x.{}: obj)\n", n + i));
    }
    let mut acc = format!("x.{n}");
    for i in 1..n {
        let var = format!("s.{}", 2 * n + i);
        text.push_str(&format!(
            "  let {var}: obj = extern Prelude.++({acc}, x.{})\n",
            n + i
        ));
        acc = var;
    }
    text.push_str(&format!("  return {acc}\n}}\n"));
    text
}

#[test]
fn many_live_parameters_are_rewritten_in_linear_time() {
    assert_linear_in_live_variables(many_live_parameters);
}

#[test]
fn many_reads_of_one_owner_are_rewritten_in_linear_time() {
    assert_linear_in_live_variables(many_reads_of_one_owner);
}

#[test]
fn many_owners_resolved_one_by_one_are_rewritten_in_linear_time() {
    assert_linear_in_live_variables(many_owners_resolved_one_by_one);
}

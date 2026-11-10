//! 借用のパス (docs/spec/lir.mdの「借用の推論」)。入力は縮約の後の手書きのLIRである。出力はどれも、
//! verifierの借用の段を通り、読み直すと同じ表示に戻り、Perceusの後に所有の段を通らなければならない。

use eml_lir::{Program, borrow, parse, perceus, pretty, verify, verify_borrowed, verify_flattened};

use crate::common::function;

fn read(text: &str) -> Program {
    let program = parse(text).unwrap_or_else(|error| panic!("{error}\n{text}"));
    if let Err(error) = verify_flattened(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    program
}

/// 借用のパスの後の表示。
fn borrow_text(text: &str) -> String {
    let mut program = read(text);
    borrow(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_borrowed(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    let parsed = parse(&shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(pretty(&parsed), shown, "the printed LIR must read back");
    perceus(&mut program);
    if let Err(error) = verify(&program) {
        panic!(
            "the output must verify after Perceus: {error}\n{}",
            pretty(&program)
        );
    }
    shown
}

const SHOW: &str = "extern \"Prelude.Show Int.show\"";
const LENGTH: &str = "extern Std.String.byte_length";

#[test]
fn only_unr_reference_counted_parameters_of_functions_called_directly_are_borrowed() {
    // `unr`の印の`obj`と`tobj`の引数だけを貸せる。固定の関数 (`$uniform`の変種と、
    // 値として生まれる関数) のエントリの引数は所有のままである
    let text = format!(
        "\
fn f(s.0: obj, n.1: int, h.2: obj lin, t.3: tobj) -> int {{
  let k.4: int = {LENGTH}(s.0)
  let u.5: unit = drop h.2
  switch t.3 {{ _ -> b1 }}
b1:
  return k.4
}}
fn f$uniform(s.0: obj) -> tobj {{
  let k.1: int = {LENGTH}(s.0)
  let b.2: tobj = box k.1
  return b.2
}}
uniform from 0 fn main$lambda1(s.0: obj) -> obj {{
  let k.1: int = {LENGTH}(s.0)
  let t.2: obj = {SHOW}(k.1)
  return t.2
}}
"
    );
    insta::assert_snapshot!(borrow_text(&text), @r#"
    fn f(borrowed s.0: obj, n.1: int, h.2: obj lin, borrowed t.3: tobj) -> int {
      let k.4: int = extern Std.String.byte_length(s.0)
      let u.5: unit = drop h.2
      switch t.3 { _ -> b1 }
    b1:
      return k.4
    }
    fn f$uniform(s.0: obj) -> tobj {
      let k.1: int = extern Std.String.byte_length(s.0)
      let b.2: tobj = box k.1
      return b.2
    }
    uniform from 0 fn main$lambda1(s.0: obj) -> obj {
      let k.1: int = extern Std.String.byte_length(s.0)
      let t.2: obj = extern "Prelude.Show Int.show"(k.1)
      return t.2
    }
    "#);
}

#[test]
fn a_consumed_parameter_or_field_owns_its_root() {
    // 消費する引数、所有の位置に渡す引数、捕獲する引数と、フィールドを消費する引数は所有になる。
    // 呼び出しに貸すだけの引数と、フィールドを読むだけの引数は貸されたままである
    let text = format!(
        "\
layout Box {{ Box(tobj) }}
fn reader(s.0: obj) -> int {{
  let k.1: int = {LENGTH}(s.0)
  return k.1
}}
fn keeper(s.0: obj) -> obj {{
  return s.0
}}
fn f(a.0: obj, b.1: obj, c.2: obj, d.3: obj, e.4: obj) -> tobj {{
  let k.5: int = call reader(a.0)
  let t.6: obj = call keeper(b.1)
  unpack c.2 Box #0(x.7: tobj)
  let u.8: tobj = closure g(d.3)
  unpack e.4 Box #0(y.9: tobj)
  switch y.9 {{ _ -> b1 }}
b1:
  return x.7
}}
uniform from 1 fn g(d.0: obj, v.1: tobj) -> tobj {{
  return v.1
}}
"
    );
    insta::assert_snapshot!(borrow_text(&text), @"
    layout Box { Box(tobj) }
    fn reader(borrowed s.0: obj) -> int {
      let k.1: int = extern Std.String.byte_length(s.0)
      return k.1
    }
    fn keeper(s.0: obj) -> obj {
      return s.0
    }
    fn f(borrowed a.0: obj, b.1: obj, c.2: obj, d.3: obj, borrowed e.4: obj) -> tobj {
      let k.5: int = call reader(a.0)
      let t.6: obj = call keeper(b.1)
      unpack c.2 Box #0(x.7: tobj)
      let u.8: tobj = closure g(d.3)
      unpack e.4 Box #0(y.9: tobj)
      switch y.9 { _ -> b1 }
    b1:
      return x.7
    }
    uniform from 1 fn g(d.0: obj, v.1: tobj) -> tobj {
      return v.1
    }
    ");
}

#[test]
fn a_consumed_scalar_field_leaves_its_root_borrowed() {
    // スカラーのフィールドを消費しても`dup`は要らないので、根は所有にならない
    let text = "\
layout Pair { Pair(int, int) }
fn getx(p.0: obj) -> int {
  unpack p.0 Pair #0(a.1: int, b.2: int)
  return a.1
}
";
    insta::assert_snapshot!(borrow_text(text), @"
    layout Pair { Pair(int, int) }
    fn getx(borrowed p.0: obj) -> int {
      unpack p.0 Pair #0(a.1: int, b.2: int)
      return a.1
    }
    ");
}

#[test]
fn a_merge_parameter_is_borrowed_when_every_edge_passes_a_borrowed_value() {
    // 合流のブロックの引数は、どの辺も呼んだ側から借りた値か定数を渡すときに貸された引数になる。
    // 新しい値が1つでもあれば所有になり、すると別の辺が渡す値は消費されるので、エントリの引数も所有になる
    let merge = |name: &str, other: &str| {
        format!(
            "\
fn {name}(s.0: tobj, c.1: int) -> int {{
  switch c.1 {{ 0 -> b1, _ -> b2 }}
b1:
  jump b3(s.0)
b2:
{other}
b3(u.3: tobj):
  switch u.3 {{ _ -> b4 }}
b4:
  return 0
}}
"
        )
    };
    let text = merge("constant", "  jump b3(())")
        + &merge(
            "fresh",
            &format!("  let t.2: obj = {SHOW}(c.1)\n  jump b3(t.2)"),
        );
    insta::assert_snapshot!(borrow_text(&text), @r#"
    fn constant(borrowed s.0: tobj, c.1: int) -> int {
      switch c.1 { 0 -> b1, _ -> b2 }
    b1:
      jump b3(s.0)
    b2:
      jump b3(())
    b3(borrowed u.3: tobj):
      switch u.3 { _ -> b4 }
    b4:
      return 0
    }
    fn fresh(s.0: tobj, c.1: int) -> int {
      switch c.1 { 0 -> b1, _ -> b2 }
    b1:
      jump b3(s.0)
    b2:
      let t.2: obj = extern "Prelude.Show Int.show"(c.1)
      jump b3(t.2)
    b3(u.3: tobj):
      switch u.3 { _ -> b4 }
    b4:
      return 0
    }
    "#);
}

#[test]
fn a_value_taken_apart_before_a_constructor_of_the_same_size_is_owned() {
    // 同じ物理的な語の数の`con`の前で分解する値は、reuseがその箱を使えるように所有のままである。
    // 大きさの違う`con`と、`unpack`の前にだけある`con`は数えない
    let text = "\
layout List { Nil, Cons(int, tobj) }
layout Pair { Pair(int, int) }
layout Box { Box(int) }
fn relist(xs.0: tobj) -> tobj {
  switch xs.0 List { #0 -> b1, #1(h.1: int, t.2: tobj) -> b2 }
b1:
  return #0
b2:
  let c.3: tobj = con List #1(h.1, #0)
  return c.3
}
fn rebox(xs.0: tobj) -> tobj {
  switch xs.0 List { #0 -> b1, #1(h.1: int, t.2: tobj) -> b2 }
b1:
  return #0
b2:
  let c.3: obj = con Box #0(h.1)
  return c.3
}
fn swap(p.0: obj) -> obj {
  unpack p.0 Pair #0(a.1: int, b.2: int)
  let q.3: obj = con Pair #0(b.2, a.1)
  return q.3
}
fn before(p.0: obj) -> obj {
  let q.1: obj = con Pair #0(1, 2)
  unpack p.0 Pair #0(a.2: int, b.3: int)
  return q.1
}
";
    insta::assert_snapshot!(borrow_text(text), @"
    layout List { Nil, Cons(int, tobj) }
    layout Pair { Pair(int, int) }
    layout Box { Box(int) }
    fn relist(xs.0: tobj) -> tobj {
      switch xs.0 List { #0 -> b1, #1(h.1: int, t.2: tobj) -> b2 }
    b1:
      return #0
    b2:
      let c.3: tobj = con List #1(h.1, #0)
      return c.3
    }
    fn rebox(borrowed xs.0: tobj) -> tobj {
      switch xs.0 List { #0 -> b1, #1(h.1: int, t.2: tobj) -> b2 }
    b1:
      return #0
    b2:
      let c.3: obj = con Box #0(h.1)
      return c.3
    }
    fn swap(p.0: obj) -> obj {
      unpack p.0 Pair #0(a.1: int, b.2: int)
      let q.3: obj = con Pair #0(b.2, a.1)
      return q.3
    }
    fn before(borrowed p.0: obj) -> obj {
      let q.1: obj = con Pair #0(1, 2)
      unpack p.0 Pair #0(a.2: int, b.3: int)
      return q.1
    }
    ");
}

#[test]
fn a_tail_call_in_its_cycle_owns_the_parameter_it_feeds_a_fresh_value() {
    // 自分の輪の上で新しい値を渡す末尾呼び出しは、その引数を所有にする。そのため輪の上で降格は要らない。
    // 貸された引数をそのまま渡すなら、貸されたままである
    let looping = |name: &str, arg: &str, fresh: &str| {
        format!(
            "\
fn {name}(s.0: obj, n.1: int) -> int {{
  switch n.1 {{ 0 -> b1, _ -> b2 }}
b1:
  let k.2: int = {LENGTH}(s.0)
  return k.2
b2:
  let m.3: int = extern \"Prelude.Num Int.-\"(n.1, 1)
{fresh}  tail call {name}({arg}, m.3)
}}
"
        )
    };
    let text = looping("passes", "s.0", "")
        + &looping("fresh", "t.4", &format!("  let t.4: obj = {SHOW}(m.3)\n"));
    insta::assert_snapshot!(borrow_text(&text), @r#"
    fn passes(borrowed s.0: obj, n.1: int) -> int {
      switch n.1 { 0 -> b1, _ -> b2 }
    b1:
      let k.2: int = extern Std.String.byte_length(s.0)
      return k.2
    b2:
      let m.3: int = extern "Prelude.Num Int.-"(n.1, 1)
      tail call passes(s.0, m.3)
    }
    fn fresh(s.0: obj, n.1: int) -> int {
      switch n.1 { 0 -> b1, _ -> b2 }
    b1:
      let k.2: int = extern Std.String.byte_length(s.0)
      return k.2
    b2:
      let m.3: int = extern "Prelude.Num Int.-"(n.1, 1)
      let t.4: obj = extern "Prelude.Show Int.show"(m.3)
      tail call fresh(t.4, m.3)
    }
    "#);
}

#[test]
fn a_value_reference_goes_to_the_owned_copy() {
    // `&f`と`closure f(..)`は`f$owned`へ向く。直接の呼び出しはそのままである
    let text = format!(
        "\
fn reader(s.0: obj, t.1: tobj) -> obj {{
  let k.2: int = {LENGTH}(s.0)
  let u.3: unit = drop t.1
  let v.4: obj = {SHOW}(k.2)
  return v.4
}}
fn user(x.0: obj, y.1: obj) -> tobj {{
  let t.2: obj = call reader(x.0, ())
  let c.3: tobj = closure reader(y.1)
  let p.4: tobj = apply c.3(&reader)
  return t.2
}}
"
    );
    insta::assert_snapshot!(borrow_text(&text), @r#"
    fn reader(borrowed s.0: obj, t.1: tobj) -> obj {
      let k.2: int = extern Std.String.byte_length(s.0)
      let u.3: unit = drop t.1
      let v.4: obj = extern "Prelude.Show Int.show"(k.2)
      return v.4
    }
    fn user(borrowed x.0: obj, y.1: obj) -> tobj {
      let t.2: obj = call reader(x.0, ())
      let c.3: tobj = closure reader$owned(y.1)
      let p.4: tobj = apply c.3(&reader$owned)
      return t.2
    }
    fn reader$owned(s.0: obj, t.1: tobj) -> obj {
      let k.2: int = extern Std.String.byte_length(s.0)
      let u.3: unit = drop t.1
      let v.4: obj = extern "Prelude.Show Int.show"(k.2)
      return v.4
    }
    "#);
}

#[test]
fn a_tail_call_lending_a_fresh_value_is_demoted() {
    // 呼ばれる関数は`open`でないので、新しい値を貸す末尾呼び出しは呼び出しと`return`になる。
    // 貸された値を渡す末尾呼び出しはそのままである
    let text = format!(
        "\
fn reader(s.0: obj) -> int {{
  let k.1: int = {LENGTH}(s.0)
  return k.1
}}
fn caller(n.0: int) -> int {{
  let s.1: obj = {SHOW}(n.0)
  tail call reader(s.1)
}}
fn passes(s.0: obj) -> int {{
  tail call reader(s.0)
}}
"
    );
    insta::assert_snapshot!(borrow_text(&text), @r#"
    fn reader(borrowed s.0: obj) -> int {
      let k.1: int = extern Std.String.byte_length(s.0)
      return k.1
    }
    fn caller(n.0: int) -> int {
      let s.1: obj = extern "Prelude.Show Int.show"(n.0)
      let r.2: int = call reader(s.1)
      return r.2
    }
    fn passes(borrowed s.0: obj) -> int {
      tail call reader(s.0)
    }
    "#);
}

#[test]
fn a_tail_call_into_an_open_callee_goes_to_the_owned_copy() {
    // 呼ばれる関数が`tail apply`で終わるので、末尾呼び出しは代わりにその写しへ向き、スタックを一定に保つ
    let text = format!(
        "\
fn g(s.0: obj, k.1: tobj) -> tobj {{
  let n.2: int = {LENGTH}(s.0)
  let t.3: obj = {SHOW}(n.2)
  tail apply k.1(t.3)
}}
fn h(n.0: int, k.1: tobj) -> tobj {{
  let s.2: obj = {SHOW}(n.0)
  tail call g(s.2, k.1)
}}
"
    );
    insta::assert_snapshot!(borrow_text(&text), @r#"
    fn g(borrowed s.0: obj, k.1: tobj) -> tobj {
      let n.2: int = extern Std.String.byte_length(s.0)
      let t.3: obj = extern "Prelude.Show Int.show"(n.2)
      tail apply k.1(t.3)
    }
    fn h(n.0: int, k.1: tobj) -> tobj {
      let s.2: obj = extern "Prelude.Show Int.show"(n.0)
      tail call g$owned(s.2, k.1)
    }
    fn g$owned(s.0: obj, k.1: tobj) -> tobj {
      let n.2: int = extern Std.String.byte_length(s.0)
      let t.3: obj = extern "Prelude.Show Int.show"(n.2)
      tail apply k.1(t.3)
    }
    "#);
}

/// 文字列とリストの残りを渡して互いを末尾で呼ぶ2つの関数。`a`は値としても使う。`open`が真なら、
/// `b`はリストの終わりで`tail apply`をする。
fn cycle(open: bool) -> String {
    let (k_param, k_arg, nil) = if open {
        (", k.2: tobj", ", k.2", "  tail apply k.2(())")
    } else {
        ("", "", "  return ()")
    };
    format!(
        "\
layout List {{ Nil, Cons(tobj, tobj) }}
fn a(s.0: obj, l.1: tobj{k_param}) -> tobj {{
  switch l.1 List {{ #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }}
b1:
  return ()
b2:
  tail call b(s.0, t.4{k_arg})
}}
fn b(s.0: obj, l.1: tobj{k_param}) -> tobj {{
  switch l.1 List {{ #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }}
b1:
{nil}
b2:
  let n.5: int = {LENGTH}(s.0)
  tail call a(s.0, t.4{k_arg})
}}
fn user(x.0: obj) -> tobj {{
  return &a
}}
"
    )
}

#[test]
fn the_owned_copy_of_a_closed_tail_cycle_demotes_into_the_original() {
    // `a$owned`の引数は所有なので、`b`への呼び出しは降格する。その後は元の輪が回る
    insta::assert_snapshot!(borrow_text(&cycle(false)), @"
    layout List { Nil, Cons(tobj, tobj) }
    fn a(borrowed s.0: obj, borrowed l.1: tobj) -> tobj {
      switch l.1 List { #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }
    b1:
      return ()
    b2:
      tail call b(s.0, t.4)
    }
    fn b(borrowed s.0: obj, borrowed l.1: tobj) -> tobj {
      switch l.1 List { #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }
    b1:
      return ()
    b2:
      let n.5: int = extern Std.String.byte_length(s.0)
      tail call a(s.0, t.4)
    }
    fn user(borrowed x.0: obj) -> tobj {
      return &a$owned
    }
    fn a$owned(s.0: obj, l.1: tobj) -> tobj {
      switch l.1 List { #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }
    b1:
      return ()
    b2:
      let r.5: tobj = call b(s.0, t.4)
      return r.5
    }
    ");
}

#[test]
fn the_owned_copies_of_an_open_tail_cycle_form_their_own_cycle() {
    // `b`は`open`なので、`a$owned`は`b$owned`を末尾で呼び、写しどうしで輪を作る
    insta::assert_snapshot!(borrow_text(&cycle(true)), @"
    layout List { Nil, Cons(tobj, tobj) }
    fn a(borrowed s.0: obj, borrowed l.1: tobj, k.2: tobj) -> tobj {
      switch l.1 List { #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }
    b1:
      return ()
    b2:
      tail call b(s.0, t.4, k.2)
    }
    fn b(borrowed s.0: obj, borrowed l.1: tobj, k.2: tobj) -> tobj {
      switch l.1 List { #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }
    b1:
      tail apply k.2(())
    b2:
      let n.5: int = extern Std.String.byte_length(s.0)
      tail call a(s.0, t.4, k.2)
    }
    fn user(borrowed x.0: obj) -> tobj {
      return &a$owned
    }
    fn a$owned(s.0: obj, l.1: tobj, k.2: tobj) -> tobj {
      switch l.1 List { #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }
    b1:
      return ()
    b2:
      tail call b$owned(s.0, t.4, k.2)
    }
    fn b$owned(s.0: obj, l.1: tobj, k.2: tobj) -> tobj {
      switch l.1 List { #0 -> b1, #1(h.3: tobj, t.4: tobj) -> b2 }
    b1:
      tail apply k.2(())
    b2:
      let n.5: int = extern Std.String.byte_length(s.0)
      tail call a$owned(s.0, t.4, k.2)
    }
    ");
}

#[test]
fn uniform_variants_lend_to_a_borrowing_callee() {
    // 薄い形の`call f`は、変種の所有の引数を貸し、Perceusは呼び出しの後に`decref`を置く。
    // 写しの形の末尾呼び出しは所有の引数を貸すので、降格する
    let text = format!(
        "\
fn f(s.0: obj) -> int {{
  let k.1: int = {LENGTH}(s.0)
  return k.1
}}
fn f$uniform(s.0: obj) -> tobj {{
  let r.1: int = call f(s.0)
  let b.2: tobj = box r.1
  return b.2
}}
fn shown(s.0: obj) -> obj {{
  let k.1: int = {LENGTH}(s.0)
  let t.2: obj = {SHOW}(k.1)
  return t.2
}}
fn g(s.0: obj) -> obj {{
  tail call shown(s.0)
}}
fn g$uniform(s.0: obj) -> tobj {{
  tail call shown(s.0)
}}
"
    );
    insta::assert_snapshot!(borrow_text(&text), @r#"
    fn f(borrowed s.0: obj) -> int {
      let k.1: int = extern Std.String.byte_length(s.0)
      return k.1
    }
    fn f$uniform(s.0: obj) -> tobj {
      let r.1: int = call f(s.0)
      let b.2: tobj = box r.1
      return b.2
    }
    fn shown(borrowed s.0: obj) -> obj {
      let k.1: int = extern Std.String.byte_length(s.0)
      let t.2: obj = extern "Prelude.Show Int.show"(k.1)
      return t.2
    }
    fn g(borrowed s.0: obj) -> obj {
      tail call shown(s.0)
    }
    fn g$uniform(s.0: obj) -> tobj {
      let r.1: obj = call shown(s.0)
      return r.1
    }
    "#);
}

#[test]
fn a_tail_call_returning_several_values_is_demoted_to_a_call_binding_them() {
    // 複数の値を返す呼び出しを降格すると、値をすべて束縛し、順に返す
    let text = format!(
        "\
fn pair(s.0: obj) -> (int, int) {{
  let k.1: int = {LENGTH}(s.0)
  return (k.1, k.1)
}}
fn caller(n.0: int) -> (int, int) {{
  let s.1: obj = {SHOW}(n.0)
  tail call pair(s.1)
}}
"
    );
    insta::assert_snapshot!(borrow_text(&text), @r#"
    fn pair(borrowed s.0: obj) -> (int, int) {
      let k.1: int = extern Std.String.byte_length(s.0)
      return (k.1, k.1)
    }
    fn caller(n.0: int) -> (int, int) {
      let s.1: obj = extern "Prelude.Show Int.show"(n.0)
      let (r.2: int, r.3: int) = call pair(s.1)
      return (r.2, r.3)
    }
    "#);
}

#[test]
fn the_marks_of_a_function_do_not_depend_on_its_value_references() {
    // 2つのプログラムが`f`を共有する。一方は`f`を呼ぶだけで、もう一方は`&f`も参照する。
    // `f`の表示はどちらでも同じで、`h`は`&f$owned`だけが違う
    let program = |value: &str| {
        format!(
            "\
fn f(s.0: obj) -> obj {{
  let k.1: int = {LENGTH}(s.0)
  let t.2: obj = {SHOW}(k.1)
  return t.2
}}
fn g(s.0: obj) -> obj {{
  return s.0
}}
fn h(n.0: int) -> tobj {{
  let s.1: obj = {SHOW}(n.0)
  let t.2: obj = call f(s.1)
  let u.3: unit = drop t.2
  let v.4: unit = drop s.1
  return {value}
}}
"
        )
    };
    let called = borrow_text(&program("&g"));
    let referenced = borrow_text(&program("&f"));
    assert_eq!(function(&called, "f"), function(&referenced, "f"));
    assert_eq!(
        function(&called, "h"),
        function(&referenced, "h").replace("&f$owned", "&g")
    );
    assert!(function(&referenced, "h").contains("return &f$owned"));
    assert!(!called.contains("f$owned"));
}

/// `length`個の関数の鎖。各関数は文字列を次の関数に渡し、自分でも読む。
fn chain(length: usize) -> String {
    let mut text = String::new();
    for index in 0..length {
        if index + 1 == length {
            text.push_str(&format!(
                "fn f{index}(s.0: obj) -> int {{\n  let k.1: int = {LENGTH}(s.0)\n  return k.1\n}}\n"
            ));
        } else {
            text.push_str(&format!(
                "fn f{index}(s.0: obj) -> int {{\n  let a.1: int = call f{}(s.0)\n  let b.2: int = {LENGTH}(s.0)\n  let c.3: int = extern \"Prelude.Num Int.+\"(a.1, b.2)\n  return c.3\n}}\n",
                index + 1
            ));
        }
    }
    text
}

fn borrow_chain(length: usize) -> Program {
    let mut program = parse(&chain(length)).unwrap_or_else(|error| panic!("{error}"));
    borrow(&mut program);
    program
}

fn assert_every_parameter_is_borrowed(program: &Program) {
    if let Err(error) = verify_borrowed(program) {
        panic!("the output must verify: {error}");
    }
    for function in &program.functions {
        assert_eq!(function.lends(), [true], "{}", function.name);
    }
}

#[test]
fn a_long_call_chain_borrows_every_parameter() {
    // 成分を再帰なしで求めて解くので、長い鎖でもスタックがあふれず、どの関数も文字列を借りる
    assert_every_parameter_is_borrowed(&borrow_chain(2000));
}

#[test]
#[ignore = "timing: borrow on a 20000-function chain, release only"]
fn borrow_is_linear_on_a_long_call_chain() {
    // 同じ鎖を20000個の関数で、releaseビルドで流す。上限はゆるく、2乗の実装だけが超える
    let mut program = parse(&chain(20000)).unwrap_or_else(|error| panic!("{error}"));
    let start = std::time::Instant::now();
    borrow(&mut program);
    let elapsed = start.elapsed();
    assert_every_parameter_is_borrowed(&program);
    assert!(
        elapsed < std::time::Duration::from_secs(TIMING_SECS),
        "took {elapsed:?}"
    );
}

const TIMING_SECS: u64 = 5;

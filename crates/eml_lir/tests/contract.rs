//! contract のパス (docs/spec/lir.md の「縮約」)。使われない純粋な `let` を消すことと、すべてのブロックに
//! 末尾呼び出しの規則を当てることを、IR のテキストで確かめる。ソースから縮約までを通すテストは `eml_core_ir` の
//! tests/contract.rs にある。

use crate::common::{function, passes_text};
use eml_lir::{Pass, contract, parse, pretty, verify_flattened, verify_scopes};

/// 入力と出力が、どちらも scope の段の verifier を通ることも確かめる。
fn contract_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_scopes(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    contract(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_scopes(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

#[test]
fn unused_bindings_that_cannot_fail_are_removed() {
    let text = "\
layout Pair { Pair(int, int) }
fn f(x.0: int) -> int {
  let p.1: obj = con Pair #0(x.0, x.0)
  let s.2: obj = const \"unused\"
  let c.3: tobj = closure g(())
  return 1
}
fn g(x.0: unit, y.1: tobj) -> tobj {
  return y.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Pair { Pair(int, int) }
    fn f(x.0: int) -> int {
      return 1
    }
    fn g(x.0: unit, y.1: tobj) -> tobj {
      return y.1
    }
    ");
}

#[test]
fn unused_bindings_that_can_fail_or_act_are_kept() {
    // `/` はゼロ除算で止まりうる。extern のエフェクト、呼び出し、`drop` も、使われなくても消さない
    let text = "\
fn f(x.0: int, s.1: obj, t.2: obj) -> int {
  let q.3: int = extern \"Prelude.Div Int./\"(x.0, 0)
  let u.4: unit = extern Prelude.println(s.1)
  let r.5: int = call g(x.0)
  let d.6: unit = drop t.2
  return 1
}
fn g(x.0: int) -> int {
  return x.0
}
";
    assert_eq!(contract_text(text), text);
}

#[test]
fn unused_pure_extern_bindings_are_removed() {
    // 行が `Pure` の extern は止まらないので、使われなければ消す
    let text = "\
fn f(x.0: int, s.1: obj) -> int {
  let t.2: obj = extern \"Prelude.Show Int.show\"(x.0)
  let b.3: enum = extern \"Prelude.Ord Int.<\"(x.0, 0)
  let u.4: obj = extern Prelude.++(s.1, t.2)
  return 1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: int, s.1: obj) -> int {
      return 1
    }
    ");
}

#[test]
fn unused_boxes_and_unboxes_are_removed() {
    // `box` を消すと確保が1つ減るだけで、`unbox` は読むだけなので、どちらも純粋である
    let text = "\
fn f(n.0: int, b.1: tobj) -> int {
  let c.2: tobj = box n.0
  let m.3: int = unbox b.1
  return n.0
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(n.0: int, b.1: tobj) -> int {
      return n.0
    }
    ");
}

#[test]
fn bindings_used_only_by_removed_bindings_go_in_the_same_pass() {
    // `q.4` を消すと `p.3` が、`p.3` を消すと `s.2` が使われなくなる。後ろから1回たどるだけで、ブロックをまたいで
    // すべて消える
    let text = "\
layout Prelude.Bool { False, True }
layout Pair { Pair(tobj, int) }
layout Either { Left(tobj), Right(tobj) }
fn f(x.0: int, c.1: enum) -> int {
  let s.2: obj = extern \"Prelude.Show Int.show\"(x.0)
  let p.3: obj = con Pair #0(s.2, x.0)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let q.4: obj = con Either #1(p.3)
  return 1
b2:
  return 2
}
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Prelude.Bool { False, True }
    layout Pair { Pair(tobj, int) }
    layout Either { Left(tobj), Right(tobj) }
    fn f(x.0: int, c.1: enum) -> int {
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      return 1
    b2:
      return 2
    }
    ");
}

#[test]
fn a_call_returned_at_the_end_of_an_unchanged_block_becomes_a_tail_call() {
    // `f` は使われない `let` を消してから、`h` は何も消さずに、どちらも末尾呼び出しになる。translate は末尾呼び出しを
    // 作らないので、縮約は文を消したかに関わらずすべてのブロックに規則を当てる
    let text = "\
fn f(x.0: int) -> int {
  let r.1: int = call g(x.0)
  let s.2: obj = const \"unused\"
  return r.1
}
fn g(x.0: int) -> int {
  return x.0
}
fn h(x.0: int) -> int {
  let r.1: int = call g(x.0)
  return r.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: int) -> int {
      tail call g(x.0)
    }
    fn g(x.0: int) -> int {
      return x.0
    }
    fn h(x.0: int) -> int {
      tail call g(x.0)
    }
    ");
}

#[test]
fn every_kind_of_call_returned_at_the_end_becomes_a_tail_call() {
    // 直接の呼び出し、関数値の適用、節の呼び出しのどれも末尾呼び出しになる (docs/spec/lir.md の「縮約」)。
    // 結果を返さない呼び出しは残す
    let text = "\
effect Ask { ask(tobj) -> tobj }
fn by_call(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return r.1
}
fn by_apply(f.0: tobj) -> tobj {
  let r.1: tobj = apply f.0(())
  return r.1
}
yields fn by_clause(c.0: tobj, e.1: obj, s.2: obj) -> tobj {
  let r.3: tobj = clause Ask.ask c.0(e.1, s.2)
  return r.3
}
fn not_returned(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return x.0
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Ask { ask(tobj) -> tobj }
    fn by_call(x.0: int) -> int {
      tail call by_call(x.0)
    }
    fn by_apply(f.0: tobj) -> tobj {
      tail apply f.0(())
    }
    yields fn by_clause(c.0: tobj, e.1: obj, s.2: obj) -> tobj {
      tail clause Ask.ask c.0(e.1, s.2)
    }
    fn not_returned(x.0: int) -> int {
      let r.1: int = call by_call(x.0)
      return x.0
    }
    ");
}

#[test]
fn an_incompatible_tail_position_call_is_not_made_tail() {
    // `g` の `unit` は `f` の `obj` と互換でないので、縮約は末尾呼び出しにしない (docs/spec/lir.md の「縮約」)
    let text = "\
fn f() -> obj {
  let r.0: tobj = call g()
  return r.0
}
fn g() -> unit {
  return ()
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f() -> obj {
      let r.0: tobj = call g()
      return r.0
    }
    fn g() -> unit {
      return ()
    }
    ");
}

#[test]
fn a_clause_call_returned_at_the_end_becomes_a_tail_call() {
    // 節の呼び出しも、操作の結果が `ret` と互換なら末尾呼び出しになる。`g` では `Counter.put` の `unit` の結果が
    // `obj` の `ret` と互換でないので、末尾呼び出しにしない
    let text = "\
effect Counter { get(unit) -> int, put(int) -> unit }
yields fn f(c.0: tobj, e.1: obj) -> int {
  let m.2: int = clause Counter.get c.0(e.1, ())
  return m.2
}
yields fn g(c.0: tobj, e.1: obj, n.2: int) -> obj {
  let u.3: tobj = clause Counter.put c.0(e.1, n.2)
  return u.3
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Counter { get(unit) -> int, put(int) -> unit }
    yields fn f(c.0: tobj, e.1: obj) -> int {
      tail clause Counter.get c.0(e.1, ())
    }
    yields fn g(c.0: tobj, e.1: obj, n.2: int) -> obj {
      let u.3: tobj = clause Counter.put c.0(e.1, n.2)
      return u.3
    }
    ");
}

#[test]
fn a_never_clause_returned_at_the_end_becomes_a_tail_call_in_any_function() {
    // `never` の操作の節の呼び出しは戻らないので、結果はどの `ret` とも互換である
    let text = "\
effect Fail { never fail(tobj) -> never }
yields fn f(c.0: tobj, e.1: obj, s.2: obj) -> int {
  let r.3: int = clause Fail.fail c.0(e.1, s.2)
  return r.3
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Fail { never fail(tobj) -> never }
    yields fn f(c.0: tobj, e.1: obj, s.2: obj) -> int {
      tail clause Fail.fail c.0(e.1, s.2)
    }
    ");
}

#[test]
fn a_call_whose_result_mark_is_not_compatible_with_the_ret_mark_is_not_a_tail_call() {
    // `h` の `lin` は `r` の `poly` と、`r` は `f` の `unr` と互換である。印の互換も推移的でないので、`lin` と `unr` を
    // 直接つなぐ末尾呼び出しにはしない。`apply` の結果はいつも `poly` なので、どの戻り値の印とも互換である
    // (docs/spec/lir.md の「縮約」と「Kind の印」)
    let text = "\
effect L { take(tobj lin) -> tobj lin }
fn f(s.0: obj) -> obj {
  let r.1: obj poly = call h(s.0)
  return r.1
}
yields fn g(c.0: tobj, e.1: obj, x.2: tobj lin) -> tobj {
  let r.3: tobj poly = clause L.take c.0(e.1, x.2)
  return r.3
}
fn h(s.0: obj) -> obj lin {
  let f.1: obj lin = extern Std.FileIn.open(s.0)
  return f.1
}
fn k(c.0: tobj) -> tobj lin {
  let r.1: tobj poly = apply c.0(())
  return r.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect L { take(tobj lin) -> tobj lin }
    fn f(s.0: obj) -> obj {
      let r.1: obj poly = call h(s.0)
      return r.1
    }
    yields fn g(c.0: tobj, e.1: obj, x.2: tobj lin) -> tobj {
      let r.3: tobj poly = clause L.take c.0(e.1, x.2)
      return r.3
    }
    fn h(s.0: obj) -> obj lin {
      let f.1: obj lin = extern Std.FileIn.open(s.0)
      return f.1
    }
    fn k(c.0: tobj) -> tobj lin {
      tail apply c.0(())
    }
    ");
}

/// 根を読むだけの `root_get` は、結果が使われなければ消す。根に置く `root_set` は消さない。
#[test]
fn an_unused_root_get_is_removed_and_a_root_set_is_kept() {
    let text = "\
fn f(x.0: tobj) -> unit {
  let y.1: tobj = prim root_get[0]()
  let u.2: unit = prim root_set[1](x.0)
  return ()
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: tobj) -> unit {
      let u.2: unit = prim root_set[1](x.0)
      return ()
    }
    ");
}

const VEC3: &str = "layout Vec3 { Vec3(float, float, float) }\n";

#[test]
fn a_multi_value_call_returned_in_order_becomes_tail() {
    // `let (x.0, .., x.n) = <呼び出し>` の直後の `return (x.0, .., x.n)` は、同じ並びで同じ順なら末尾呼び出しになる。
    // `apply` の結果は `tobj` なので、構造体を返す `h` の `apply` は葉に開いて返し、末尾呼び出しにならない
    // (docs/spec/lir.md の「縮約」)
    let text = format!(
        "{VEC3}fn g(x.0: float) -> Vec3{{float, float, float}} {{
  let v.1: Vec3{{float, float, float}} = con Vec3 #0(x.0, x.0, 1.0)
  return v.1
}}
fn f(x.0: float) -> Vec3{{float, float, float}} {{
  let v.1: Vec3{{float, float, float}} = call g(x.0)
  return v.1
}}
fn h(k.0: obj, x.1: float) -> Vec3{{float, float, float}} {{
  let v.2: Vec3{{float, float, float}} = apply k.0(x.1)
  return v.2
}}
"
    );
    let shown = passes_text(&text, Pass::Contract);
    insta::assert_snapshot!(function(&shown, "f"), @"
    fn f(x.0: float) -> (float, float, float) {
      tail call g(x.0)
    }
    ");
    insta::assert_snapshot!(function(&shown, "h"), @"
    fn h(k.0: obj, x.1: float) -> (float, float, float) {
      let x.3: tobj = box x.1
      let v.4: tobj = apply k.0(x.3)
      let (v.0.5: float, v.1.6: float, v.2.7: float) = unbox v.4 Vec3
      return (v.0.5, v.1.6, v.2.7)
    }
    ");
}

#[test]
fn a_reordered_return_is_not_tail() {
    // 値の順を入れ替えて返すなら、呼び出しの結果をそのまま返すのではないので、末尾呼び出しにしない
    let text = format!(
        "{VEC3}fn g(x.0: float) -> Vec3{{float, float, float}} {{
  let v.1: Vec3{{float, float, float}} = con Vec3 #0(x.0, x.0, 1.0)
  return v.1
}}
fn f(x.0: float) -> Vec3{{float, float, float}} {{
  let v.1: Vec3{{float, float, float}} = call g(x.0)
  unpack v.1 Vec3 #0(a.2: float, b.3: float, c.4: float)
  let w.5: Vec3{{float, float, float}} = con Vec3 #0(b.3, a.2, c.4)
  return w.5
}}
"
    );
    insta::assert_snapshot!(function(&passes_text(&text, Pass::Contract), "f"), @"
    fn f(x.0: float) -> (float, float, float) {
      let (v.0.6: float, v.1.7: float, v.2.8: float) = call g(x.0)
      return (v.1.7, v.0.6, v.2.8)
    }
    ");
}

#[test]
fn an_unused_struct_unbox_is_removed_and_the_call_becomes_tail() {
    // `g` の入口で引数を受け直す `unbox` は、box の挿入の覗き穴が箱をそのまま渡すので使われない。`h` の `apply` の
    // 結果を受け直す `unbox` は、葉をどれも使わない。構造体の `unbox` は純粋な `let` なので縮約がどちらも消し、
    // その後の呼び出しは末尾呼び出しになる
    let text = "\
layout (,) { (,)(tobj, tobj) }
internal uniform from 1 fn g(k.0: obj, p.1: (,){float, float}) -> tobj {
  let r.2: tobj = apply k.0(p.1)
  return r.2
}
fn h(k.0: obj, x.1: float) -> (,){float, float} {
  let p.2: (,){float, float} = apply k.0(x.1)
  let q.3: (,){float, float} = call h(k.0, 2.0)
  return q.3
}
";
    let shown = passes_text(text, Pass::Contract);
    insta::assert_snapshot!(function(&shown, "g"), @"
    internal uniform from 1 fn g(k.0: obj, p.3: tobj) -> tobj {
      tail apply k.0(p.3)
    }
    ");
    insta::assert_snapshot!(function(&shown, "h"), @"
    fn h(k.0: obj, x.1: float) -> (float, float) {
      let x.4: tobj = box x.1
      let p.5: tobj = apply k.0(x.4)
      tail call h(k.0, 2.0)
    }
    ");
}

#[test]
fn an_unused_unbox_of_a_lin_box_after_a_tail_position_call_is_removed() {
    // 縮約が使われない `m.5` を消すと、構造体の `unbox` の葉はどれも使われなくなる。箱をそのまま返す末尾の位置
    // では、この `unbox` を箱が `lin` でも消す。箱は呼び出しの結果として返るので、Perceus が手放す所はない
    let text = "\
layout Tok { Tok(int, float) }
internal uniform from 1 fn f(k.0: obj, u.1: unit) -> Tok{int, float} lin {
  let t.2: Tok{int, float} lin = apply k.0(1)
  unpack t.2 Tok #0(n.3: int, x.4: float)
  let m.5: enum = extern \"Prelude.Ord Int.<\"(n.3, 1)
  return t.2
}
";
    insta::assert_snapshot!(function(&passes_text(text, Pass::Contract), "f"), @"
    internal uniform from 1 fn f(k.0: obj, u.1: unit) -> tobj lin {
      let b.6: tobj = box 1
      tail apply k.0(b.6)
    }
    ");
}

#[test]
fn an_unused_unbox_of_another_box_after_a_tail_position_call_is_kept() {
    // 末尾の位置の呼び出しの後ろで消すのは、呼び出しの結果の箱の `unbox` だけである。ほかの `lin` の箱の `unbox` は
    // 葉がどれも使われなくても残し、Perceus がその箱を `drop` で手放す。そのため `g` の呼び出しは末尾呼び出しに
    // ならない (docs/spec/lir.md の「縮約」)
    let text = "\
layout Tok { Tok(int, float) }
fn g(k.0: obj) -> int {
  return 1
}
fn f(k.0: obj, w.1: obj lin) -> int {
  let r.2: int = call g(k.0)
  let (_, _) = unbox w.1 Tok
  return r.2
}
";
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_flattened(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    contract(&mut program);
    if let Err(error) = verify_flattened(&program) {
        panic!("the output must verify: {error}");
    }
    insta::assert_snapshot!(function(&pretty(&program), "f"), @"
    fn f(k.0: obj, w.1: obj lin) -> int {
      let r.2: int = call g(k.0)
      let (_, _) = unbox w.1 Tok
      return r.2
    }
    ");
}

#[test]
fn a_multi_value_call_that_would_pass_lin_to_unr_is_not_tail() {
    // 呼ばれる側の葉は `lin` で、呼ぶ側の結果の葉は `unr` である。束縛の `poly` の葉を通せばどちらとも受け渡せるが、
    // `tail` にすると `lin` を `unr` に直接つなぐので、値の数が同じなら印を値ごとに比べて `let` と `return` のまま残す
    let text = "\
layout P { P(obj, obj) }
fn g(a.0: obj lin, b.1: obj) -> P{obj lin, obj} lin {
  let v.2: P{obj lin, obj} lin = con P #0(a.0, b.1)
  return v.2
}
fn f(a.0: obj lin, b.1: obj) -> P{obj, obj} {
  let v.2: P{obj poly, obj} poly = call g(a.0, b.1)
  return v.2
}
";
    insta::assert_snapshot!(function(&passes_text(text, Pass::Contract), "f"), @"
    fn f(a.0: obj lin, b.1: obj) -> (obj, obj) {
      let (v.0.3: obj poly, v.1.4: obj) = call g(a.0, b.1)
      return (v.0.3, v.1.4)
    }
    ");
}

#[test]
fn a_multi_value_call_that_passes_unr_to_lin_is_tail() {
    let text = "\
layout P { P(obj, obj) }
fn g(a.0: obj, b.1: obj) -> P{obj, obj} {
  let v.2: P{obj, obj} = con P #0(a.0, b.1)
  return v.2
}
fn f(a.0: obj, b.1: obj) -> P{obj lin, obj} lin {
  let v.2: P{obj poly, obj} poly = call g(a.0, b.1)
  return v.2
}
";
    insta::assert_snapshot!(function(&passes_text(text, Pass::Contract), "f"), @"
    fn f(a.0: obj, b.1: obj) -> (obj lin, obj) {
      tail call g(a.0, b.1)
    }
    ");
}

#[test]
fn a_par_whose_result_is_returned_stays_a_let() {
    // join に親のフレームが要るので、`par` は末尾呼び出しにしない (docs/spec/lir.md の「末尾の位置の呼び出し」)
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn f(a.0: tobj, b.1: tobj) -> obj {
  let t.2: obj = par (,) #0(a.0, b.1)
  return t.2
}
";
    assert_eq!(contract_text(text), text);
}

//! EP のパスのうち、yield の地点で関数を分ける部分 (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md
//! の「yield の地点」「継続の関数」)。入力は手書きの translate の出力の形で、出力が EP の後の段の verifier を通ることも
//! 確かめる。

use crate::common::function;
use eml_core_ir::{evidence, parse, pretty, verify_evidence, verify_translated};

fn evidence_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_translated(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    evidence(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_evidence(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

#[test]
fn a_function_that_does_not_yield_is_unchanged() {
    let text = "fn g() -> int {\n  return 1\n}\nfn f() -> int {\n  let x.0: int = call g()\n  let y.1: int = extern \"Prelude.+\"(x.0, 1)\n  return y.1\n}\n";
    assert_eq!(evidence_text(text), pretty(&parse(text).unwrap()));
}

#[test]
fn a_yield_point_continues_in_a_continuation_function() {
    let text = "yields fn g() -> int {\n  return 1\n}\nyields fn f(a.0: int) -> int {\n  let x.1: int = call g()\n  let y.2: int = extern \"Prelude.+\"(x.1, a.0)\n  return y.2\n}\n";
    let shown = evidence_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(a.0: int) -> int {
      let x.1: int = call g()
      let y.3: int = prim yielding()
      switch y.3 { 0 -> b2, _ -> b1 }
    b1:
      let k.4: tobj = prim yield_cont_take()
      let c.5: tobj = closure f$k0(a.0)
      let k.6: tobj = closure compose$(k.4, c.5)
      let u.7: unit = prim yield_cont_set(k.6)
      return 0
    b2:
      let y.2: int = extern Prelude.+(x.1, a.0)
      return y.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k0"), @"
    internal yields fn f$k0(a.0: int, x.1: int) -> int {
      let y.2: int = extern Prelude.+(x.1, a.0)
      return y.2
    }
    ");
}

#[test]
fn a_tail_position_call_is_not_a_yield_point() {
    // 後ろに純粋な `let` だけが続く呼び出しも末尾の位置にある。呼ばれた側の yield はそのまま呼び出し元へ返る
    let text = "\
yields fn g() -> int {
  return 1
}
yields fn f() -> int {
  let x.0: int = call g()
  return x.0
}
yields fn h(a.0: tobj) -> int {
  let x.1: int = call g()
  let c.2: tobj = closure k(a.0)
  return x.1
}
fn k(a.0: tobj, b.1: tobj) -> tobj {
  return a.0
}
";
    assert_eq!(evidence_text(text), pretty(&parse(text).unwrap()));
}

#[test]
fn a_continuation_ends_at_the_next_yield_point() {
    // `f$k0` の中の2つ目の地点は `f$k1` で続く。`f$k0` の速い道は `f$k1` を直接呼び、続きを写さない
    let text = "\
yields fn g(n.0: int) -> int {
  return n.0
}
yields fn f(a.0: int) -> int {
  let x.1: int = call g(a.0)
  let y.2: int = call g(x.1)
  let z.3: int = extern \"Prelude.+\"(y.2, a.0)
  return z.3
}
";
    let shown = evidence_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(a.0: int) -> int {
      let x.1: int = call g(a.0)
      let y.4: int = prim yielding()
      switch y.4 { 0 -> b2, _ -> b1 }
    b1:
      let k.5: tobj = prim yield_cont_take()
      let c.6: tobj = closure f$k0(a.0)
      let k.7: tobj = closure compose$(k.5, c.6)
      let u.8: unit = prim yield_cont_set(k.7)
      return 0
    b2:
      let y.2: int = call g(x.1)
      let y.9: int = prim yielding()
      switch y.9 { 0 -> b4, _ -> b3 }
    b3:
      let k.10: tobj = prim yield_cont_take()
      let c.11: tobj = closure f$k1(a.0)
      let k.12: tobj = closure compose$(k.10, c.11)
      let u.13: unit = prim yield_cont_set(k.12)
      return 0
    b4:
      let z.3: int = extern Prelude.+(y.2, a.0)
      return z.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k0"), @"
    internal yields fn f$k0(a.0: int, x.1: int) -> int {
      let y.2: int = call g(x.1)
      let y.4: int = prim yielding()
      switch y.4 { 0 -> b2, _ -> b1 }
    b1:
      let k.5: tobj = prim yield_cont_take()
      let c.6: tobj = closure f$k1(a.0)
      let k.7: tobj = closure compose$(k.5, c.6)
      let u.8: unit = prim yield_cont_set(k.7)
      return 0
    b2:
      let r.9: int = call f$k1(a.0, y.2)
      return r.9
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k1"), @"
    internal yields fn f$k1(a.0: int, y.2: int) -> int {
      let z.3: int = extern Prelude.+(y.2, a.0)
      return z.3
    }
    ");
}

#[test]
fn a_continuation_calls_a_join_block_function() {
    // yield の地点の後の if の2つの枝が、合流のブロック b3 に jump する。`f$k0` の2つの jump は `f$j3` の呼び出しに
    // なり、`f$j3` は b3 の入口で生きている `a.0` と、b3 の引数を受ける。元の `f` の jump はそのまま残る
    let text = "\
yields fn g() -> int {
  return 1
}
yields fn f(a.0: int) -> int {
  let x.1: int = call g()
  switch a.0 { 0 -> b1, _ -> b2 }
b1:
  let p.2: int = extern \"Prelude.+\"(x.1, 1)
  jump b3(p.2)
b2:
  jump b3(x.1)
b3(v.3: int):
  let w.4: int = extern \"Prelude.+\"(v.3, a.0)
  return w.4
}
";
    let shown = evidence_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(a.0: int) -> int {
      let x.1: int = call g()
      let y.5: int = prim yielding()
      switch y.5 { 0 -> b2, _ -> b1 }
    b1:
      let k.6: tobj = prim yield_cont_take()
      let c.7: tobj = closure f$k0(a.0)
      let k.8: tobj = closure compose$(k.6, c.7)
      let u.9: unit = prim yield_cont_set(k.8)
      return 0
    b2:
      switch a.0 { 0 -> b3, _ -> b4 }
    b3:
      let p.2: int = extern Prelude.+(x.1, 1)
      jump b5(p.2)
    b4:
      jump b5(x.1)
    b5(v.3: int):
      let w.4: int = extern Prelude.+(v.3, a.0)
      return w.4
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k0"), @"
    internal yields fn f$k0(a.0: int, x.1: int) -> int {
      switch a.0 { 0 -> b1, _ -> b2 }
    b1:
      let p.2: int = extern Prelude.+(x.1, 1)
      let r.5: int = call f$j3(a.0, p.2)
      return r.5
    b2:
      let r.6: int = call f$j3(a.0, x.1)
      return r.6
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$j3"), @"
    internal yields fn f$j3(a.0: int, v.3: int) -> int {
      let w.4: int = extern Prelude.+(v.3, a.0)
      return w.4
    }
    ");
}

#[test]
fn a_join_block_function_ends_at_its_yield_point() {
    // 合流のブロック b3 の中の地点は `f$k1` で続く。`f$j3` は b3 の写しで、その地点の速い道は `f$k1` を呼ぶ
    let text = "\
yields fn g(n.0: int) -> int {
  return n.0
}
yields fn f(a.0: int) -> int {
  let x.1: int = call g(a.0)
  switch a.0 { 0 -> b1, _ -> b2 }
b1:
  jump b3(x.1)
b2:
  jump b3(a.0)
b3(v.2: int):
  let w.3: int = call g(v.2)
  let z.4: int = extern \"Prelude.+\"(w.3, x.1)
  return z.4
}
";
    let shown = evidence_text(text);
    insta::assert_snapshot!(function(&shown, "f$k0"), @"
    internal yields fn f$k0(a.0: int, x.1: int) -> int {
      switch a.0 { 0 -> b1, _ -> b2 }
    b1:
      let r.5: int = call f$j3(x.1, x.1)
      return r.5
    b2:
      let r.6: int = call f$j3(x.1, a.0)
      return r.6
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$j3"), @"
    internal yields fn f$j3(x.1: int, v.2: int) -> int {
      let w.3: int = call g(v.2)
      let y.5: int = prim yielding()
      switch y.5 { 0 -> b2, _ -> b1 }
    b1:
      let k.6: tobj = prim yield_cont_take()
      let c.7: tobj = closure f$k1(x.1)
      let k.8: tobj = closure compose$(k.6, c.7)
      let u.9: unit = prim yield_cont_set(k.8)
      return 0
    b2:
      let r.10: int = call f$k1(x.1, w.3)
      return r.10
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k1"), @"
    internal yields fn f$k1(x.1: int, w.3: int) -> int {
      let z.4: int = extern Prelude.+(w.3, x.1)
      return z.4
    }
    ");
}

#[test]
fn an_apply_without_the_yields_mark_is_not_a_yield_point() {
    // 印のない `apply` は確認を持たず、`yields apply` は持つ。継続の関数の中の印のない `apply` も同じである
    let text = "\
yields fn f(h.0: tobj, a.1: tobj) -> tobj {
  let x.2: tobj = apply h.0(a.1)
  let y.3: tobj = yields apply h.0(x.2)
  let z.4: tobj = apply h.0(y.3)
  return x.2
}
";
    let shown = evidence_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(h.0: tobj, a.1: tobj) -> tobj {
      let x.2: tobj = apply h.0(a.1)
      let y.3: tobj = yields apply h.0(x.2)
      let y.5: int = prim yielding()
      switch y.5 { 0 -> b2, _ -> b1 }
    b1:
      let k.6: tobj = prim yield_cont_take()
      let c.7: tobj = closure f$k0(h.0, x.2)
      let k.8: tobj = closure compose$(k.6, c.7)
      let u.9: unit = prim yield_cont_set(k.8)
      let d.10: obj = prim dummy()
      return d.10
    b2:
      let z.4: tobj = apply h.0(y.3)
      return x.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k0"), @"
    internal yields fn f$k0(h.0: tobj, x.2: tobj, y.3: tobj) -> tobj {
      let z.4: tobj = apply h.0(y.3)
      return x.2
    }
    ");
}

#[test]
fn the_dummy_follows_the_return_repr() {
    // `obj` と `tobj` の `ret` には `prim dummy()` の物体を、`int`、`enum`、`unit` には定数を返す
    let text = "\
yields fn g() -> unit {
  return ()
}
yields fn o(s.0: obj) -> obj {
  let u.1: unit = call g()
  return s.0
}
yields fn t(s.0: tobj) -> tobj {
  let u.1: unit = call g()
  return s.0
}
yields fn i() -> int {
  let u.0: unit = call g()
  return 1
}
yields fn e() -> enum {
  let u.0: unit = call g()
  return #1
}
yields fn n() -> unit {
  let u.0: unit = call g()
  let v.1: unit = call g()
  return ()
}
";
    let shown = evidence_text(text);
    insta::assert_snapshot!(function(&shown, "o"), @"
    yields fn o(s.0: obj) -> obj {
      let u.1: unit = call g()
      let y.2: int = prim yielding()
      switch y.2 { 0 -> b2, _ -> b1 }
    b1:
      let k.3: tobj = prim yield_cont_take()
      let c.4: tobj = closure o$k0(s.0)
      let k.5: tobj = closure compose$(k.3, c.4)
      let u.6: unit = prim yield_cont_set(k.5)
      let d.7: obj = prim dummy()
      return d.7
    b2:
      return s.0
    }
    ");
    insta::assert_snapshot!(function(&shown, "t"), @"
    yields fn t(s.0: tobj) -> tobj {
      let u.1: unit = call g()
      let y.2: int = prim yielding()
      switch y.2 { 0 -> b2, _ -> b1 }
    b1:
      let k.3: tobj = prim yield_cont_take()
      let c.4: tobj = closure t$k0(s.0)
      let k.5: tobj = closure compose$(k.3, c.4)
      let u.6: unit = prim yield_cont_set(k.5)
      let d.7: obj = prim dummy()
      return d.7
    b2:
      return s.0
    }
    ");
    insta::assert_snapshot!(function(&shown, "i"), @"
    yields fn i() -> int {
      let u.0: unit = call g()
      let y.1: int = prim yielding()
      switch y.1 { 0 -> b2, _ -> b1 }
    b1:
      let k.2: tobj = prim yield_cont_take()
      let k.3: tobj = closure compose$(k.2, &i$k0)
      let u.4: unit = prim yield_cont_set(k.3)
      return 0
    b2:
      return 1
    }
    ");
    insta::assert_snapshot!(function(&shown, "e"), @"
    yields fn e() -> enum {
      let u.0: unit = call g()
      let y.1: int = prim yielding()
      switch y.1 { 0 -> b2, _ -> b1 }
    b1:
      let k.2: tobj = prim yield_cont_take()
      let k.3: tobj = closure compose$(k.2, &e$k0)
      let u.4: unit = prim yield_cont_set(k.3)
      return #0
    b2:
      return #1
    }
    ");
    insta::assert_snapshot!(function(&shown, "n"), @"
    yields fn n() -> unit {
      let u.0: unit = call g()
      let y.2: int = prim yielding()
      switch y.2 { 0 -> b2, _ -> b1 }
    b1:
      let k.3: tobj = prim yield_cont_take()
      let k.4: tobj = closure compose$(k.3, &n$k0)
      let u.5: unit = prim yield_cont_set(k.4)
      return ()
    b2:
      let v.1: unit = call g()
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b4, _ -> b3 }
    b3:
      let k.7: tobj = prim yield_cont_take()
      let k.8: tobj = closure compose$(k.7, &n$k1)
      let u.9: unit = prim yield_cont_set(k.8)
      return ()
    b4:
      return ()
    }
    ");
}

#[test]
fn helpers_are_made_once_in_their_final_form() {
    // 2つの関数が yield の地点を持っても `compose$` は1つで、その中の `apply` は分けられない。`id$` は yield を
    // 始める節の入口だけが使うので、まだ作られない
    let text = "\
yields fn g() -> int {
  return 1
}
yields fn f() -> int {
  let x.0: int = call g()
  let y.1: int = extern \"Prelude.+\"(x.0, 1)
  return y.1
}
yields fn h() -> int {
  let x.0: int = call g()
  let y.1: int = extern \"Prelude.+\"(x.0, 2)
  return y.1
}
";
    let shown = evidence_text(text);
    assert_eq!(shown.matches("fn compose$(").count(), 1, "{shown}");
    assert!(!shown.contains("fn id$("), "{shown}");
    insta::assert_snapshot!(function(&shown, "compose$"), @"
    yields fn compose$(k.0: tobj, c.1: tobj, x.2: tobj) -> tobj {
      let r.3: tobj = yields apply k.0(x.2)
      let y.4: int = prim yielding()
      switch y.4 { 0 -> b2, _ -> b1 }
    b1:
      let k.5: tobj = prim yield_cont_take()
      let k.6: tobj = closure compose$(k.5, c.1)
      let u.7: unit = prim yield_cont_set(k.6)
      let d.8: obj = prim dummy()
      return d.8
    b2:
      let t.9: tobj = yields apply c.1(r.3)
      return t.9
    }
    ");
}

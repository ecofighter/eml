//! EP のパス (docs/spec/core-ir.md の「evidence passing」)。yield の地点で関数を分ける部分は、手書きの translate の
//! 出力の形を入力にし、出力が EP の後の段の verifier を通ることも確かめる。handle、`perform`、`resume`、`mask` の変換は、
//! ソースから EP の直後までを通した Core IR で確かめる。

use crate::common::{core_text, function};
use eml_core_ir::{Pass, evidence, parse, pretty, verify_evidence, verify_translated};

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
      let k.4: tobj poly = prim yield_cont_take()
      let c.5: tobj = closure f$k0(a.0)
      let k.6: tobj poly = closure compose$(k.4, c.5)
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
      let k.5: tobj poly = prim yield_cont_take()
      let c.6: tobj = closure f$k0(a.0)
      let k.7: tobj poly = closure compose$(k.5, c.6)
      let u.8: unit = prim yield_cont_set(k.7)
      return 0
    b2:
      let y.2: int = call g(x.1)
      let y.9: int = prim yielding()
      switch y.9 { 0 -> b4, _ -> b3 }
    b3:
      let k.10: tobj poly = prim yield_cont_take()
      let c.11: tobj = closure f$k1(a.0)
      let k.12: tobj poly = closure compose$(k.10, c.11)
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
      let y.3: int = prim yielding()
      switch y.3 { 0 -> b2, _ -> b1 }
    b1:
      let k.4: tobj poly = prim yield_cont_take()
      let c.5: tobj = closure f$k1(a.0)
      let k.6: tobj poly = closure compose$(k.4, c.5)
      let u.7: unit = prim yield_cont_set(k.6)
      return 0
    b2:
      let r.8: int = call f$k1(a.0, y.2)
      return r.8
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k1"), @"
    internal yields fn f$k1(a.0: int, y.1: int) -> int {
      let z.2: int = extern Prelude.+(y.1, a.0)
      return z.2
    }
    ");
}

#[test]
fn a_continuation_copies_nested_switches_and_their_case_fields() {
    // 地点の後の区間は、`switch` の行き先をたどって写す。b2 の `switch` は入れ子である。`Some` の場合が束ねる `v.3`
    // は `f$k0` の中で定義されるので、`f$k0` の引数にならない。b4 の中の2つ目の地点は `f$k1` で続く
    let text = "\
layout Option { None, Some(int) }
yields fn g(n.0: int) -> int {
  return n.0
}
yields fn f(o.0: tobj, a.1: int) -> int {
  let x.2: int = call g(a.1)
  switch o.0 Option { #0 -> b1, #1(v.3: int) -> b2 }
b1:
  return x.2
b2:
  switch v.3 { 0 -> b3, _ -> b4 }
b3:
  return a.1
b4:
  let y.4: int = call g(v.3)
  let z.5: int = extern \"Prelude.+\"(y.4, x.2)
  return z.5
}
";
    let shown = evidence_text(text);
    insta::assert_snapshot!(function(&shown, "f$k0"), @"
    internal yields fn f$k0(o.0: tobj, a.1: int, x.2: int) -> int {
      switch o.0 Option { #0 -> b1, #1(v.3: int) -> b2 }
    b1:
      return x.2
    b2:
      switch v.3 { 0 -> b3, _ -> b4 }
    b3:
      return a.1
    b4:
      let y.4: int = call g(v.3)
      let y.5: int = prim yielding()
      switch y.5 { 0 -> b6, _ -> b5 }
    b5:
      let k.6: tobj poly = prim yield_cont_take()
      let c.7: tobj = closure f$k1(x.2)
      let k.8: tobj poly = closure compose$(k.6, c.7)
      let u.9: unit = prim yield_cont_set(k.8)
      return 0
    b6:
      let r.10: int = call f$k1(x.2, y.4)
      return r.10
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k1"), @"
    internal yields fn f$k1(x.0: int, y.1: int) -> int {
      let z.2: int = extern Prelude.+(y.1, x.0)
      return z.2
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
      let k.6: tobj poly = prim yield_cont_take()
      let c.7: tobj = closure f$k0(a.0)
      let k.8: tobj poly = closure compose$(k.6, c.7)
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
      let r.3: int = call f$j3(a.0, p.2)
      return r.3
    b2:
      let r.4: int = call f$j3(a.0, x.1)
      return r.4
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$j3"), @"
    internal yields fn f$j3(a.0: int, v.1: int) -> int {
      let w.2: int = extern Prelude.+(v.1, a.0)
      return w.2
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
      let r.2: int = call f$j3(x.1, x.1)
      return r.2
    b2:
      let r.3: int = call f$j3(x.1, a.0)
      return r.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$j3"), @"
    internal yields fn f$j3(x.0: int, v.1: int) -> int {
      let w.2: int = call g(v.1)
      let y.3: int = prim yielding()
      switch y.3 { 0 -> b2, _ -> b1 }
    b1:
      let k.4: tobj poly = prim yield_cont_take()
      let c.5: tobj = closure f$k1(x.0)
      let k.6: tobj poly = closure compose$(k.4, c.5)
      let u.7: unit = prim yield_cont_set(k.6)
      return 0
    b2:
      let r.8: int = call f$k1(x.0, w.2)
      return r.8
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k1"), @"
    internal yields fn f$k1(x.0: int, w.1: int) -> int {
      let z.2: int = extern Prelude.+(w.1, x.0)
      return z.2
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
      let k.6: tobj poly = prim yield_cont_take()
      let c.7: tobj = closure f$k0(h.0, x.2)
      let k.8: tobj poly = closure compose$(k.6, c.7)
      let u.9: unit = prim yield_cont_set(k.8)
      let d.10: obj poly = prim dummy()
      return d.10
    b2:
      let z.4: tobj = apply h.0(y.3)
      return x.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k0"), @"
    internal yields fn f$k0(h.0: tobj, x.1: tobj, y.2: tobj) -> tobj {
      let z.3: tobj = apply h.0(y.2)
      return x.1
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
      let k.3: tobj poly = prim yield_cont_take()
      let c.4: tobj = closure o$k0(s.0)
      let k.5: tobj poly = closure compose$(k.3, c.4)
      let u.6: unit = prim yield_cont_set(k.5)
      let d.7: obj poly = prim dummy()
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
      let k.3: tobj poly = prim yield_cont_take()
      let c.4: tobj = closure t$k0(s.0)
      let k.5: tobj poly = closure compose$(k.3, c.4)
      let u.6: unit = prim yield_cont_set(k.5)
      let d.7: obj poly = prim dummy()
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
      let k.2: tobj poly = prim yield_cont_take()
      let k.3: tobj poly = closure compose$(k.2, &i$k0)
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
      let k.2: tobj poly = prim yield_cont_take()
      let k.3: tobj poly = closure compose$(k.2, &e$k0)
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
      let k.3: tobj poly = prim yield_cont_take()
      let k.4: tobj poly = closure compose$(k.3, &n$k0)
      let u.5: unit = prim yield_cont_set(k.4)
      return ()
    b2:
      let v.1: unit = call g()
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b4, _ -> b3 }
    b3:
      let k.7: tobj poly = prim yield_cont_take()
      let k.8: tobj poly = closure compose$(k.7, &n$k1)
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
    yields fn compose$(k.0: tobj poly, c.1: tobj poly, x.2: tobj poly) -> tobj poly {
      let r.3: tobj poly = yields apply k.0(x.2)
      let y.4: int = prim yielding()
      switch y.4 { 0 -> b2, _ -> b1 }
    b1:
      let u.5: unit = drop r.3
      let k.6: tobj poly = prim yield_cont_take()
      let k.7: tobj poly = closure compose$(k.6, c.1)
      let u.8: unit = prim yield_cont_set(k.7)
      let d.9: obj poly = prim dummy()
      return d.9
    b2:
      let t.10: tobj poly = yields apply c.1(r.3)
      return t.10
    }
    ");
}

const ASK: &str = "effect Ask where\n  ask : Unit -> Int\n\n";

/// 関数の値をデータに入れて取り出す。optimize はデータを通った値を既知の閉包としないので
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「既知の閉包」)、取り出した値への呼び出しは
/// 特殊化されない。handle の本体に使えば handler への特殊化も起きないので、EP が handle、`perform`、`resume` を
/// 下ろす形を確かめられる。
const HIDE: &str = "data Box a = | Box a\n\nhide : a -> Box a\nhide x = Box x\n\n";

/// handle の場所の形 (docs/spec/core-ir.md の「handle と `prompt$`」)。
/// in-place の入口を `ev_set_clause` で入れてから `evv` を替え、本体の結果を `prompt$` に渡す。
#[test]
fn a_handle_installs_evidence_and_calls_prompt() {
    let text = format!(
        "{ASK}{HIDE}main : Unit -> <IO> Unit\nmain () =\n  let Box b = hide (fn () -> ask () + 1)\n  let r =\n    handle b () with\n      | ctl ask () k -> k 41\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main"), @r#"
    fn main(p.0: unit) -> unit {
      let d.7: obj = con Box #0(&main$lambda0)
      unpack d.7 Box #0(b.2: tobj)
      let c.3: tobj = closure main$handle0(b.2)
      let w.8: tobj = prim evv_get()
      let ev.9: obj = prim ev_new[Ask]((), w.8)
      let u.10: unit = prim ev_set_clause[Ask.ask](ev.9, &main$handle0$ask$entry)
      let w.11: tobj = prim evv_push(w.8, ev.9)
      let u.12: unit = prim evv_set(w.11)
      let x.13: tobj poly = yields apply c.3(()) prompt ev.9
      let t.4: int = call prompt$(ev.9, &main$handle0$return, x.13)
      let t.5: obj = extern "Prelude.Show Int.show"(t.4)
      let t.6: unit = extern Prelude.println(t.5)
      return t.6
    }
    "#);
}

#[test]
fn an_immediate_clause_becomes_its_entry() {
    // 末尾で1回だけ再開する節は、`k` と状態の引数を除いた入口になり、状態を evidence の欄から出し入れする。
    // resume は残らない
    let text = format!(
        "{ASK}{HIDE}main : Unit -> <IO> Unit\nmain () =\n  let Box b = hide (fn () -> ask () + 1)\n  let r =\n    handle b () with\n      | ctl ask () k -> k 41\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    assert!(!shown.contains("fn main$handle0$ask("), "{shown}");
    insta::assert_snapshot!(function(&shown, "main$handle0$ask$entry"), @"
    internal clause Ask.ask fn main$handle0$ask$entry(e.4: obj, p.0: unit) -> int {
      let p.2: unit = prim ev_take_state(e.4)
      let u.5: unit = prim ev_set_state(e.4, ())
      return 41
    }
    ");
}

#[test]
fn a_clause_that_does_not_resume_at_once_yields_to_its_handler() {
    // 入口は節のクロージャを作って `yield_to` を呼び、ダミーを返す。節の関数は `prompt$` が `apply c(k, st)` で呼ぶ
    let text = format!(
        "{ASK}{HIDE}main : Unit -> <IO> Unit\nmain () =\n  let Box b = hide (fn () -> ask () + 1)\n  let r =\n    handle b () with\n      | ctl ask () k ->\n        let v = k 10\n        v * 2\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$handle0$ask$entry"), @"
    internal yields clause Ask.ask fn main$handle0$ask$entry(e.0: obj, p.1: unit) -> int {
      let c.2: tobj = closure main$handle0$ask(p.1)
      let u.3: unit = prim yield_to(e.0, c.2, &id$)
      return 0
    }
    ");
}

#[test]
fn a_resume_becomes_an_apply() {
    // `resume k(v, s)` は、含む関数の `yields` の印を写した `apply k(v, s)` になる。handle を含む `main` が `yields` で
    // ないので、節の関数も `apply` も印を持たない
    let text = format!(
        "{ASK}{HIDE}main : Unit -> <IO> Unit\nmain () =\n  let Box b = hide (fn () -> ask () + 1)\n  let r =\n    handle b () with\n      | ctl ask () k ->\n        let v = k 10\n        v * 2\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$handle0$ask"), @"
    internal fn main$handle0$ask(p.0: unit, k.1: tobj lin, p.2: unit) -> int {
      let t.3: int = apply k.1(10, ())
      let t.4: int = extern Prelude.*(t.3, 2)
      return t.4
    }
    ");
}

#[test]
fn a_clause_that_performs_another_effect_is_not_run_in_place() {
    // 節の本体が外の handler の操作を呼ぶ (yield の地点を持つ) と、すぐに再開する節でも `yield_to` の入口になる
    let text = format!(
        "{ASK}{HIDE}effect Log where\n  log : Int -> Unit\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box b = hide (fn () -> ask () + 1)\n  let r =\n    handle (handle b () with\n              | ctl ask () k ->\n                log 1\n                k 41) with\n      | ctl log n k -> k ()\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$handle0$ask$entry"), @"
    internal yields clause Ask.ask fn main$handle0$ask$entry(e.0: obj, p.1: unit) -> int {
      let c.2: tobj = closure main$handle0$ask(p.1)
      let u.3: unit = prim yield_to(e.0, c.2, &id$)
      return 0
    }
    ");
    insta::assert_snapshot!(function(&shown, "main$handle1$log$entry"), @"
    internal clause Log.log fn main$handle1$log$entry(e.4: obj, n.0: int) -> unit {
      let p.2: unit = prim ev_take_state(e.4)
      let u.5: unit = prim ev_set_state(e.4, ())
      return ()
    }
    ");
}

#[test]
fn a_never_clause_aborts() {
    // `never` の節の入口は `yield_abort` を呼ぶ。`ret` は `obj` で、ダミーの物体を返す
    let text = "effect Fail where\n  never fail : String -> a\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle fail \"x\" + 1 with\n      | ctl fail _ -> 0\n  println (show r)";
    let shown = core_text(text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$handle0$fail$entry"), @"
    internal clause Fail.fail fn main$handle0$fail$entry(e.0: obj, p.1: obj) -> obj poly {
      let c.2: tobj = closure main$handle0$fail(p.1)
      let u.3: unit = prim yield_abort(e.0, c.2)
      let d.4: obj poly = prim dummy()
      return d.4
    }
    ");
}

#[test]
fn a_perform_looks_up_evidence_and_calls_its_clause() {
    // `perform` は evidence を探し、その入口の欄を読んで節の呼び出しにする。末尾でないので yield の地点になる
    let text = format!(
        "{ASK}{HIDE}main : Unit -> <IO> Unit\nmain () =\n  let Box b = hide (fn () -> ask () + 1)\n  let r =\n    handle b () with\n      | ctl ask () k -> k 41\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$lambda0"), @"
    internal yields fn main$lambda0(p.0: unit) -> int {
      let ev.3: obj = prim ev_lookup[Ask]()
      let c.4: tobj = prim ev_clause[Ask.ask](ev.3)
      let t.1: int = clause Ask.ask c.4(ev.3, ())
      let y.5: int = prim yielding()
      switch y.5 { 0 -> b2, _ -> b1 }
    b1:
      let k.6: tobj poly = prim yield_cont_take()
      let k.7: tobj poly = closure compose$(k.6, &main$lambda0$k0)
      let u.8: unit = prim yield_cont_set(k.7)
      return 0
    b2:
      let t.2: int = extern Prelude.+(t.1, 1)
      return t.2
    }
    ");
}

/// `name` の instance の関数の名前。row の変数を持つ instance は `"run<e>"` のように引用符付きで表示される。
fn instance_name(shown: &str, name: &str) -> String {
    let quoted = format!("\"{name}");
    shown
        .lines()
        .find_map(|line| {
            line.split("fn ")
                .nth(1)
                .filter(|rest| rest.starts_with(&quoted))
        })
        .map(|rest| {
            rest.split('(')
                .next()
                .unwrap()
                .trim_matches('"')
                .to_string()
        })
        .unwrap_or_else(|| panic!("no `{name}` in\n{shown}"))
}

#[test]
fn a_masked_call_restores_the_evidence_list_and_wraps_the_continuation() {
    // 遅い道は κ を `mask$[State]` で包んでから `run$k1` を `compose$` で足す。速い道の先頭で `evv_set(w0)` が
    // `evv` を戻し、`run$k1` の引数に w0 はない
    let text = format!(
        "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\n{HIDE}run : (Unit -> <e> String) -> <State Int | e> String\nrun cb =\n  let n = get ()\n  let s = cb ()\n  s ++ show n\n\ncb : Unit -> <State String> String\ncb () = get () ++ \"!\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box f = hide cb\n  let r = handle (handle run f with\n                    | ctl get () k -> k 42\n                    | ctl put _ k -> k ()) with\n            | ctl get () k -> k \"str\"\n            | ctl put _ k -> k ()\n  println r"
    );
    let shown = core_text(&text, Pass::Evidence);
    let run = instance_name(&shown, "run");
    insta::assert_snapshot!(function(&shown, &run), @r#"
    yields fn "run<e>"(cb.0: tobj poly) -> obj {
      let ev.5: obj = prim ev_lookup[State]()
      let c.6: tobj = prim ev_clause[State.get](ev.5)
      let t.1: int = clause State.get c.6(ev.5, ())
      let y.11: int = prim yielding()
      switch y.11 { 0 -> b2, _ -> b1 }
    b1:
      let k.12: tobj poly = prim yield_cont_take()
      let c.13: tobj poly = closure "run<e>$k0"(cb.0)
      let k.14: tobj poly = closure compose$(k.12, c.13)
      let u.15: unit = prim yield_cont_set(k.14)
      let d.16: obj poly = prim dummy()
      return d.16
    b2:
      let w.7: tobj = prim evv_get()
      let w.8: tobj = prim evv_mask[State](w.7)
      let u.9: unit = prim evv_set(w.8)
      let t.2: obj = yields apply cb.0(())
      let y.17: int = prim yielding()
      switch y.17 { 0 -> b4, _ -> b3 }
    b3:
      let k.18: tobj poly = prim yield_cont_take()
      let k.19: tobj poly = closure "mask$[State]"(k.18)
      let c.20: tobj = closure "run<e>$k1"(t.1)
      let k.21: tobj poly = closure compose$(k.19, c.20)
      let u.22: unit = prim yield_cont_set(k.21)
      let d.23: obj poly = prim dummy()
      return d.23
    b4:
      let u.10: unit = prim evv_set(w.7)
      let t.3: obj = extern "Prelude.Show Int.show"(t.1)
      let t.4: obj = extern Prelude.++(t.2, t.3)
      return t.4
    }
    "#);
    insta::assert_snapshot!(function(&shown, &format!("{run}$k1")), @r#"
    internal yields fn "run<e>$k1"(t.0: int, t.1: obj) -> obj {
      let t.2: obj = extern "Prelude.Show Int.show"(t.0)
      let t.3: obj = extern Prelude.++(t.1, t.2)
      return t.3
    }
    "#);
}

#[test]
fn a_masked_call_in_a_continuation_restores_the_evidence_list_before_it_continues() {
    // 2つ目の `cb ()` は1つ目の地点の後の区間にあるので、`run$k0` の中の地点になる。`run$k0` の速い道は `run$k1` を
    // 呼んで止まるが、その前に `evv_set(w0)` で `evv` を戻す。`run$k1` の本体には `evv_set` がない
    let text = format!(
        "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\n{HIDE}run : (Unit -> <e> String) -> <State Int | e> String\nrun cb =\n  let a = cb ()\n  let b = cb ()\n  a ++ b\n\ncb : Unit -> <State String> String\ncb () = get ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box f = hide cb\n  let Box run = hide run\n  let r = handle (handle run f with\n                    | ctl get () k -> k 42\n                    | ctl put _ k -> k ()) with\n            | ctl get () k -> k \"str\"\n            | ctl put _ k -> k ()\n  println r"
    );
    let shown = core_text(&text, Pass::Evidence);
    let run = instance_name(&shown, "run");
    insta::assert_snapshot!(function(&shown, &format!("{run}$k0")), @r#"
    internal yields fn "run<e>$k0"(cb.0: tobj, t.1: obj) -> obj {
      let w.3: tobj = prim evv_get()
      let w.4: tobj = prim evv_mask[State](w.3)
      let u.5: unit = prim evv_set(w.4)
      let t.2: obj = yields apply cb.0(())
      let y.7: int = prim yielding()
      switch y.7 { 0 -> b2, _ -> b1 }
    b1:
      let k.8: tobj poly = prim yield_cont_take()
      let k.9: tobj poly = closure "mask$[State]"(k.8)
      let c.10: tobj = closure "run<e>$k1"(t.1)
      let k.11: tobj poly = closure compose$(k.9, c.10)
      let u.12: unit = prim yield_cont_set(k.11)
      let d.13: obj poly = prim dummy()
      return d.13
    b2:
      let u.6: unit = prim evv_set(w.3)
      let r.14: obj = call "run<e>$k1"(t.1, t.2)
      return r.14
    }
    "#);
    insta::assert_snapshot!(function(&shown, &format!("{run}$k1")), @r#"
    internal yields fn "run<e>$k1"(t.0: obj, t.1: obj) -> obj {
      let t.2: obj = extern Prelude.++(t.0, t.1)
      return t.2
    }
    "#);
}

#[test]
fn a_resume_in_a_yielding_function_becomes_a_yielding_apply() {
    // handle を含む `f` は `Log` を外へ通すので `yields` で、節の関数も `yields` になる。`resume k(v, s)` は
    // `yields apply k(v, s)` になり、末尾でなければ yield の地点になる
    let text = format!(
        "{ASK}{HIDE}effect Log where\n  log : Int -> Unit\n\nf : Unit -> <Log> Int\nf () =\n  let Box b = hide (fn () -> ask () + 1)\n  handle b () with\n    | ctl ask () k ->\n      let v = k 10\n      v * 2\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle f () with\n            | ctl log _ k -> k ()\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "f$handle0$ask"), @"
    internal yields fn f$handle0$ask(p.0: unit, k.1: tobj lin, p.2: unit) -> int {
      let t.3: int = yields apply k.1(10, ())
      let y.5: int = prim yielding()
      switch y.5 { 0 -> b2, _ -> b1 }
    b1:
      let k.6: tobj poly = prim yield_cont_take()
      let k.7: tobj poly = closure compose$(k.6, &f$handle0$ask$k0)
      let u.8: unit = prim yield_cont_set(k.7)
      return 0
    b2:
      let t.4: int = extern Prelude.*(t.3, 2)
      return t.4
    }
    ");
}

/// row のビットで純粋と決まった instance は `yields` でないので、EP は yield の確認を足さない
/// (docs/spec/core-ir.md の「row のビットと instance の鍵」)。
#[test]
fn a_pure_instance_of_a_row_polymorphic_function_has_no_yield_check() {
    let text = format!(
        "{ASK}{HIDE}twice : (Unit -> <e> Int) -> <e> Int\ntwice f = f () + f ()\n\npure : Unit -> Int\npure () =\n  let Box f = hide (fn () -> 1)\n  let Box twice = hide twice\n  twice f\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (show (pure ()))\n  let Box g = hide ask\n  let Box twice = hide twice\n  println (show (handle twice g with\n    | ctl ask () k -> k 2))"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "twice"), @"
    fn twice(f.0: tobj) -> int {
      let t.1: int = apply f.0(())
      let t.2: int = apply f.0(())
      let t.3: int = extern Prelude.+(t.1, t.2)
      return t.3
    }
    ");
    insta::assert_snapshot!(function(&shown, "twice<e>"), @r#"
    yields fn "twice<e>"(f.0: tobj) -> int {
      let t.1: int = yields apply f.0(())
      let y.4: int = prim yielding()
      switch y.4 { 0 -> b2, _ -> b1 }
    b1:
      let k.5: tobj poly = prim yield_cont_take()
      let c.6: tobj = closure "twice<e>$k0"(f.0)
      let k.7: tobj poly = closure compose$(k.5, c.6)
      let u.8: unit = prim yield_cont_set(k.7)
      return 0
    b2:
      let t.2: int = yields apply f.0(())
      let y.9: int = prim yielding()
      switch y.9 { 0 -> b4, _ -> b3 }
    b3:
      let k.10: tobj poly = prim yield_cont_take()
      let c.11: tobj = closure "twice<e>$k1"(t.1)
      let k.12: tobj poly = closure compose$(k.10, c.11)
      let u.13: unit = prim yield_cont_set(k.12)
      return 0
    b4:
      let t.3: int = extern Prelude.+(t.1, t.2)
      return t.3
    }
    "#);
}

/// `shown` のうち、`rhs` を右辺に持つ `let` の行。1つだけあることも確かめる。
fn let_line<'a>(shown: &'a str, rhs: &str) -> &'a str {
    let lines: Vec<&str> = shown
        .lines()
        .filter(|line| line.trim_start().starts_with("let ") && line.contains(rhs))
        .collect();
    assert_eq!(lines.len(), 1, "one `let` of `{rhs}` in\n{shown}");
    lines[0]
}

/// 束縛する変数の Repr と印 (`tobj lin` など)。
fn binding(line: &str) -> &str {
    let (_, rest) = line.split_once(": ").expect("a `let` names its Repr");
    rest.split(" = ").next().unwrap()
}

#[test]
fn closures_ep_builds_take_the_join_of_their_captures() {
    // yield をまたいで `File` が生きている `use_it` の続きのクロージャは `lin` で、それを足した κ も `lin` である。
    // `count` の続きは `Int` しか捕まえないので `unr` になる。すぐに再開しない節の入口が作る節のクロージャも、
    // 操作の引数の `File` を捕まえて `lin` になる
    // (docs/spec/core-ir.md の「EP が足す変数の印」)
    let text = format!(
        "effect Ask where\n  ask : Unit -> Int\n\neffect Sink where\n  put : Fs.File -> Unit\n\nuse_it : Fs.File -> <Ask, IO> Unit\nuse_it f =\n  let n = ask ()\n  Fs.close f\n  println (show n)\n\ncount : Int -> <Ask> Int\ncount m =\n  let n = ask ()\n  n + m\n\n{HIDE}main : Unit -> <IO> Unit\nmain () =\n  let Box u = hide use_it\n  let Box c = hide count\n  handle u (Fs.open \"input.txt\") with\n    | ctl ask () k -> k 1\n  let r = handle c 1 with\n    | ctl ask () k -> k 2\n  println (show r)\n  let Box p = hide put\n  handle p (Fs.open \"input.txt\") with\n    | ctl put f k ->\n      Fs.close f\n      let u = k ()\n      println \"resumed\"\n      u"
    );
    let shown = core_text(&text, Pass::Evidence);
    let use_it = function(&shown, "use_it");
    let rest = let_line(&use_it, "closure use_it$k0(");
    assert_eq!(binding(rest), "tobj lin", "{use_it}");
    // IO の extern の後の地点も κ に足すので、`use_it$k0` のクロージャを足す `compose$` を選ぶ
    let (_, closure) = rest.trim_start().split_once(' ').unwrap();
    let (closure, _) = closure.split_once(':').unwrap();
    let composed = let_line(&use_it, &format!(", {closure})"));
    assert!(composed.contains("closure compose$("), "{use_it}");
    assert_eq!(binding(composed), "tobj lin", "{use_it}");
    let count = function(&shown, "count");
    assert_eq!(
        binding(let_line(&count, "closure count$k0(")),
        "tobj",
        "{count}"
    );
    let entry = function(&shown, "main$handle2$put$entry");
    assert_eq!(
        binding(let_line(&entry, "closure main$handle2$put(")),
        "tobj lin",
        "{entry}"
    );
}

#[test]
fn evidence_and_evv_are_unr_and_state_is_poly() {
    // EP が基本操作の結果を束縛する変数は、基本操作の表の結果の印を持つ。evidence、`evv`、入口のクロージャは `unr`、
    // 状態、κ、節の本体、ダミーは `poly` である。ただし in-place の入口が取り出す状態は、translate が節の状態の引数に
    // 付けた印のままである。handle の本体の結果は `File` でありうるので `poly` である
    // (docs/spec/core-ir.md の「EP が足す変数の印」)
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\neffect Ask where\n  ask : Unit -> Int\n\nrun : (Unit -> <e> String) -> <State Int | e> String\nrun cb =\n  let n = get ()\n  let s = cb ()\n  s ++ show n\n\ncb : Unit -> <State String> String\ncb () = get () ++ \"!\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle (handle run cb with\n                    | ctl get () k -> k 42\n                    | ctl put _ k -> k ()) with\n            | ctl get () k -> k \"str\"\n            | ctl put _ k -> k ()\n  println r\n  let Box b = hide (fn () -> ask () + 1)\n  let a = handle b () with\n    | ctl ask () k ->\n      let v = k 10\n      v * 2\n  println (show a)";
    let text = format!("{HIDE}{text}");
    let text = text.as_str();
    let program = eml_test_support::core_until(text, Pass::Evidence);
    let mut seen = std::collections::HashSet::new();
    for function in &program.functions {
        for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
            let eml_core_ir::Stmt::Let { var, rhs } = stmt else {
                continue;
            };
            let info = &function.vars[var.0 as usize];
            match rhs {
                eml_core_ir::Rhs::Prim(prim, _) => {
                    seen.insert(prim.name());
                    if function.clause.is_some() && *prim == eml_lir::Prim::EvTakeState {
                        continue;
                    }
                    assert_eq!(
                        info.mark,
                        prim.row().ret_mark,
                        "`{}` in `{}`",
                        info.name,
                        function.name
                    );
                }
                eml_core_ir::Rhs::Call {
                    call: eml_core_ir::Call::Apply { .. },
                    mask: _,
                } if function.name == "main" => {
                    assert_eq!(info.mark, eml_lir::Mark::Poly, "`{}`", info.name);
                }
                _ => {}
            }
        }
    }
    for prim in [
        "evv_get",
        "ev_new",
        "evv_push",
        "ev_lookup",
        "ev_clause",
        "ev_evv",
        "ev_renew",
        "evv_mask",
        "ev_take_state",
        "yield_cont_take",
        "yield_finish",
        "dummy",
    ] {
        assert!(seen.contains(prim), "no `{prim}`");
    }
}

/// `IO` の extern は、失敗すると継続を作らずに巻き戻すので、`yields` の関数の中でも yield の地点に数えない
/// (docs/spec/core-ir.md の「IO の extern」)。`println` の後に yield の確認はなく、`ask` の後にだけある。
#[test]
fn an_io_extern_is_not_a_yield_point() {
    let text = format!(
        "{ASK}{HIDE}f : Unit -> <Ask, IO> Int\nf () =\n  println \"a\"\n  let n = ask ()\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box g = hide f\n  let r = handle g () with\n            | ctl ask () k -> k 1\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "f"), @r#"
    yields fn f(p.0: unit) -> int {
      let s.1: obj = const "a"
      let t.2: unit = extern Prelude.println(s.1)
      let ev.5: obj = prim ev_lookup[Ask]()
      let c.6: tobj = prim ev_clause[Ask.ask](ev.5)
      let t.3: int = clause Ask.ask c.6(ev.5, ())
      let y.7: int = prim yielding()
      switch y.7 { 0 -> b2, _ -> b1 }
    b1:
      let k.8: tobj poly = prim yield_cont_take()
      let k.9: tobj poly = closure compose$(k.8, &f$k0)
      let u.10: unit = prim yield_cont_set(k.9)
      return 0
    b2:
      let t.4: int = extern Prelude.+(t.3, 1)
      return t.4
    }
    "#);
}

/// `never` の操作の節の呼び出しは戻らないので、末尾でなくても yield の地点に数えない。`fail` の後に yield の確認は
/// なく、`ask` の後にだけある。
#[test]
fn a_never_clause_call_is_not_a_yield_point() {
    let text = format!(
        "{ASK}{HIDE}effect Fail where\n  never fail : String -> a\n\nf : Unit -> <Ask, Fail> Int\nf () =\n  let n = fail \"x\"\n  let m = ask ()\n  n + m\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box g = hide f\n  let r = handle (handle g () with\n                    | ctl ask () k -> k 1) with\n            | ctl fail _ -> 0\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "f"), @r#"
    yields fn f(p.0: unit) -> int {
      let s.1: obj = const "x"
      let ev.5: obj = prim ev_lookup[Fail]()
      let c.6: tobj = prim ev_clause[Fail.fail](ev.5)
      let t.2: int = clause Fail.fail c.6(ev.5, s.1)
      let ev.7: obj = prim ev_lookup[Ask]()
      let c.8: tobj = prim ev_clause[Ask.ask](ev.7)
      let t.3: int = clause Ask.ask c.8(ev.7, ())
      let y.9: int = prim yielding()
      switch y.9 { 0 -> b2, _ -> b1 }
    b1:
      let k.10: tobj poly = prim yield_cont_take()
      let c.11: tobj = closure f$k0(t.2)
      let k.12: tobj poly = closure compose$(k.10, c.11)
      let u.13: unit = prim yield_cont_set(k.12)
      return 0
    b2:
      let t.4: int = extern Prelude.+(t.2, t.3)
      return t.4
    }
    "#);
}

/// `never` の入口は `yield_abort` で中断を始めるだけで、κ を作らずに巻き戻すので `yields` を立てない。
#[test]
fn a_never_entry_does_not_yield() {
    let text = "effect Fail where\n  never fail : String -> a\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle fail \"x\" + 1 with\n      | ctl fail _ -> 0\n  println (show r)";
    let shown = core_text(text, Pass::Evidence);
    let entry = function(&shown, "main$handle0$fail$entry");
    assert!(
        entry.starts_with("internal clause Fail.fail fn "),
        "{entry}"
    );
}

/// `$externN` は extern を呼ぶだけで、`IO` の extern も yield の地点でないので、`yields` を立てない。
#[test]
fn an_extern_wrapper_does_not_yield() {
    // 包む関数は小さいので、特殊化した複製に展開されて消える。データを通して渡し、包む関数を値として残す
    let text = format!(
        "{HIDE}main : Unit -> <IO> Unit\nmain () =\n  let Box close = hide Fs.close\n  let Box open = hide Fs.open\n  match try_io (fn () -> List.each close (List.map open [\"a.txt\"])) with\n    | Ok () -> println \"ok\"\n    | Err e -> println \"\\{{e}}\""
    );
    let shown = core_text(&text, Pass::Evidence);
    let wrappers: Vec<&str> = shown
        .lines()
        .filter(|line| line.contains("fn ") && line.contains("$extern"))
        .collect();
    insta::assert_snapshot!(wrappers.join("\n"), @"
    internal fn main$extern0(p.0: obj lin) -> unit {
    internal fn main$extern1(p.0: obj) -> obj lin {
    ");
}

/// `yields` でない関数の `mask` 付きの呼び出しは、`evv` を替えて戻すだけで、yield の確認も遅い道も持たない。
/// 内側の `Fail` の handler を飛ばす `fail` は、継続を作らずに外側の handler へ巻き戻す。
#[test]
fn a_masked_call_in_a_non_yielding_function_has_no_slow_path() {
    let text = format!(
        "effect Fail where\n  never fail : String -> a\n\n{HIDE}run : (Unit -> <e> Int) -> <Fail | e> Int\nrun cb =\n  let x = cb ()\n  x + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box cb = hide (fn () -> fail \"outer\")\n  let r = handle (handle run cb with\n                    | ctl fail _ -> 1) with\n            | ctl fail _ -> 2\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "run"), @"
    fn run(cb.0: tobj poly) -> int {
      let w.3: tobj = prim evv_get()
      let w.4: tobj = prim evv_mask[Fail](w.3)
      let u.5: unit = prim evv_set(w.4)
      let t.1: int = apply cb.0(())
      let u.6: unit = prim evv_set(w.3)
      let t.2: int = extern Prelude.+(t.1, 1)
      return t.2
    }
    ");
}

#[test]
fn continuations_of_many_yield_points_stay_linear_in_size() {
    // 継続の関数の大きさは元のコードの2倍ほどに収まる (docs/spec/core-ir.md の「継続の関数」)。`ask` 1つにつき、`f` は
    // 呼び出しと確認と遅い道で7個、継続の関数 `f$kN` は引数を足して9個、boxing が足す `f$kN$boxed` は4個の変数を
    // 持つ。N = 2000 で測ると合わせて 40069 個 (20N 個ほど) になる。継続の関数が元の関数の変数の表を丸ごと写すと、
    // N^2 個ほどになる
    const COUNT: usize = 2000;
    let text = format!(
        "{ASK}{HIDE}f : Unit -> <Ask> Int\nf () =\n{}  0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box g = hide f\n  let r = handle g () with\n            | ctl ask () k -> k 1\n  println (show r)",
        "  let _ = ask ()\n".repeat(COUNT)
    );
    let program = eml_test_support::lir(&text);
    let vars: usize = program
        .functions
        .iter()
        .map(|function| function.vars.len())
        .sum();
    assert!(vars <= 24 * COUNT, "{vars} variables for {COUNT} calls");
}

#[test]
fn clause_local_function_resumes_once() {
    // 生の `k` を持ち上げたローカル関数の引数に渡すので、節の `return` は `k` の末尾の `resume` で終わらず、節は
    // その場で動かない (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)。実行した値は UI テストで確かめる
    let text = format!(
        "{ASK}main : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle ask () + 1 with\n      | ctl ask () k -> (let finish v = k (v + 1) in finish 1)\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$handle0$ask$entry"), @"
    internal yields clause Ask.ask fn main$handle0$ask$entry(e.0: obj, p.1: unit) -> int {
      let c.2: tobj = closure main$handle0$ask(p.1)
      let u.3: unit = prim yield_to(e.0, c.2, &id$)
      return 0
    }
    ");
}

/// `fn` の節は `| ctl ask () k -> k n` と同じ形に下ろすので、yield の地点のない本体はその場で動く入口になる
/// (docs/spec/core-ir.md の「evidence passing」)。
#[test]
fn a_fn_clause_runs_in_place() {
    let program = |clause: &str| {
        format!(
            "{ASK}{HIDE}run : Int -> Int\nrun n =\n  let Box b = hide (fn () -> ask () + 1)\n  handle b () with\n    | {clause}\n\nmain : Unit -> <IO> Unit\nmain () = println (show (run 41))"
        )
    };
    let fn_clause = core_text(&program("fn ask () -> n"), Pass::Evidence);
    let ctl_clause = core_text(&program("ctl ask () k -> k n"), Pass::Evidence);
    assert_eq!(
        function(&fn_clause, "run$handle0$ask$entry"),
        function(&ctl_clause, "run$handle0$ask$entry")
    );
    insta::assert_snapshot!(function(&fn_clause, "run$handle0$ask$entry"), @"
    internal clause Ask.ask fn run$handle0$ask$entry(n.0: int, e.5: obj, p.1: unit) -> int {
      let p.3: unit = prim ev_take_state(e.5)
      let u.6: unit = prim ev_set_state(e.5, ())
      return n.0
    }
    ");
}

/// 本体が別のエフェクトの操作を起こす `fn` の節は yield の地点を持つので、その場で動かず `yield_to` の入口になる。
#[test]
fn a_fn_clause_that_performs_yields() {
    let text = format!(
        "{ASK}{HIDE}effect Log where\n  log : Int -> Unit\n\nmain : Unit -> <IO> Unit\nmain () =\n  let Box b = hide (fn () -> ask () + 1)\n  let r =\n    handle (handle b () with\n              | fn ask () ->\n                log 1\n                41) with\n      | ctl log n k -> k ()\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$handle0$ask$entry"), @"
    internal yields clause Ask.ask fn main$handle0$ask$entry(e.0: obj, p.1: unit) -> int {
      let c.2: tobj = closure main$handle0$ask(p.1)
      let u.3: unit = prim yield_to(e.0, c.2, &id$)
      return 0
    }
    ");
}

/// 本体が `println` を呼ぶ `fn` の節は、IO の extern を yield の地点に数えないので、その場で動く。節の本体は handler の
/// 外側の `evv` で動かすので、入口の先頭で `evv` を evidence の外側に替え、`return` の前に戻す
/// (docs/spec/core-ir.md の「`perform` と節の3つの形」)。
#[test]
fn a_fn_clause_that_prints_runs_in_place_under_the_outer_evv() {
    let text = format!(
        "{ASK}{HIDE}run : Unit -> <IO> Int\nrun () =\n  let Box b = hide (fn () -> ask () + 1)\n  handle b () with\n    | fn ask () ->\n      println \"x\"\n      1\n\nmain : Unit -> <IO> Unit\nmain () = println (show (run ()))"
    );
    let shown = core_text(&text, Pass::Evidence);
    let entry = function(&shown, "run$handle0$ask$entry");
    assert!(!entry.contains("yield_to"), "{entry}");
    insta::assert_snapshot!(entry, @r#"
    internal clause Ask.ask fn run$handle0$ask$entry(e.6: obj, p.0: unit) -> int {
      let w.7: tobj = prim evv_get()
      let o.8: tobj = prim ev_evv(e.6)
      let u.11: unit = prim evv_set(o.8)
      let p.2: unit = prim ev_take_state(e.6)
      let s.3: obj = const "x"
      let t.4: unit = extern Prelude.println(s.3)
      let u.9: unit = prim ev_set_state(e.6, ())
      let u.10: unit = prim evv_set(w.7)
      return 1
    }
    "#);
}

/// 何も呼ばない節には、`evv` を替える命令を足さない。
#[test]
fn a_fn_clause_that_calls_nothing_has_no_under() {
    let text = format!(
        "effect Get where\n  get : Unit -> Int\n\n{HIDE}run : Int -> Int\nrun n =\n  let Box b = hide (fn () -> get () + 1)\n  handle b () from n with\n    | fn get () st -> (st, st)\n    | return x _ -> x\n\nmain : Unit -> <IO> Unit\nmain () = println (show (run 41))"
    );
    let shown = core_text(&text, Pass::Evidence);
    let entry = function(&shown, "run$handle0$get$entry");
    assert!(!entry.contains("evv_get"), "{entry}");
    insta::assert_snapshot!(entry, @"
    internal clause Get.get fn run$handle0$get$entry(e.4: obj, p.0: unit) -> int {
      let st.2: int = prim ev_take_state(e.4)
      let u.5: unit = prim ev_set_state(e.4, st.2)
      return st.2
    }
    ");
}

/// `never` の操作は戻らないので、末尾でない `perform` も yield の地点に数えない。本体が外の handler の `fail` を
/// 呼ぶ節もその場で動き、`fail` が外の handler に届くように `evv` を替える。
#[test]
fn a_fn_clause_that_fails_runs_in_place() {
    let text = format!(
        "{ASK}{HIDE}effect Fail where\n  never fail : String -> a\n\nrun : Int -> <Fail> Int\nrun n =\n  let Box b = hide (fn () -> ask () + 1)\n  handle b () with\n    | fn ask () -> if n == 0 then fail \"zero\" else n\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle run 0 with\n      | ctl fail _ -> 0\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    let entry = function(&shown, "run$handle0$ask$entry");
    assert!(!entry.contains("yield_to"), "{entry}");
    insta::assert_snapshot!(entry, @r#"
    internal clause Ask.ask fn run$handle0$ask$entry(n.0: int, e.9: obj, p.1: unit) -> int {
      let w.10: tobj = prim evv_get()
      let o.11: tobj = prim ev_evv(e.9)
      let u.14: unit = prim evv_set(o.11)
      let p.3: unit = prim ev_take_state(e.9)
      let t.4: enum = extern "Prelude.Eq Int.=="(n.0, 0)
      switch t.4 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      jump b3(n.0)
    b2:
      let s.5: obj = const "zero"
      let ev.15: obj = prim ev_lookup[Fail]()
      let c.16: tobj = prim ev_clause[Fail.fail](ev.15)
      let t.6: int = clause Fail.fail c.16(ev.15, s.5)
      jump b3(t.6)
    b3(t.7: int):
      let u.12: unit = prim ev_set_state(e.9, ())
      let u.13: unit = prim evv_set(w.10)
      return t.7
    }
    "#);
}

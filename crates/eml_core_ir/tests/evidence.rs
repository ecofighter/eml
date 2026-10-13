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
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b6, _ -> b5 }
    b5:
      let k.7: tobj = prim yield_cont_take()
      let c.8: tobj = closure f$k1(x.2)
      let k.9: tobj = closure compose$(k.7, c.8)
      let u.10: unit = prim yield_cont_set(k.9)
      return 0
    b6:
      let r.11: int = call f$k1(x.2, y.4)
      return r.11
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k1"), @"
    internal yields fn f$k1(x.2: int, y.4: int) -> int {
      let z.5: int = extern Prelude.+(y.4, x.2)
      return z.5
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

const ASK: &str = "effect Ask where\n  ask : Unit -> Int\n\n";

/// handle の場所の形 (docs/spec/core-ir.md の「handle と `prompt$`」)。
/// in-place の入口を `ev_set_clause` で入れてから `evv` を替え、本体の結果を `prompt$` に渡す。
#[test]
fn a_handle_installs_evidence_and_calls_prompt() {
    let text = format!(
        "{ASK}main : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle ask () + 1 with\n      | ask () k -> k 41\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main"), @r#"
    fn main(p.0: unit) -> unit {
      let w.4: tobj = prim evv_get()
      let ev.5: obj = prim ev_new[Ask]((), w.4)
      let u.6: unit = prim ev_set_clause[Ask.ask](ev.5, &main$handle0$ask$entry)
      let w.7: tobj = prim evv_push(w.4, ev.5)
      let u.8: unit = prim evv_set(w.7)
      let x.9: tobj = yields apply &main$handle0(())
      let t.1: int = call prompt$(ev.5, &main$handle0$return, x.9)
      let t.2: obj = extern "Prelude.Show Int.show"(t.1)
      let t.3: unit = extern Prelude.println(t.2)
      return t.3
    }
    "#);
}

#[test]
fn an_immediate_clause_becomes_its_entry() {
    // 末尾で1回だけ再開する節は、`k` と状態の引数を除いた入口になり、状態を evidence の欄から出し入れする。
    // resume は残らない
    let text = format!(
        "{ASK}main : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle ask () + 1 with\n      | ask () k -> k 41\n  println (show r)"
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
        "{ASK}main : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle ask () + 1 with\n      | ask () k ->\n        let v = k 10\n        v * 2\n  println (show r)"
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
        "{ASK}main : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle ask () + 1 with\n      | ask () k ->\n        let v = k 10\n        v * 2\n  println (show r)"
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
        "{ASK}effect Log where\n  log : Int -> Unit\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle (handle ask () + 1 with\n              | ask () k ->\n                log 1\n                k 41) with\n      | log n k -> k ()\n  println (show r)"
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
    let text = "effect Fail where\n  never fail : String -> a\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle fail \"x\" + 1 with\n      | fail _ -> 0\n  println (show r)";
    let shown = core_text(text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$handle0$fail$entry"), @"
    internal yields clause Fail.fail fn main$handle0$fail$entry(e.0: obj, p.1: obj) -> obj {
      let c.2: tobj = closure main$handle0$fail(p.1)
      let u.3: unit = prim yield_abort(e.0, c.2, &id$)
      let d.4: obj = prim dummy()
      return d.4
    }
    ");
}

#[test]
fn a_perform_looks_up_evidence_and_calls_its_clause() {
    // `perform` は evidence を探し、その入口の欄を読んで節の呼び出しにする。末尾でないので yield の地点になる
    let text = format!(
        "{ASK}main : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle ask () + 1 with\n      | ask () k -> k 41\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main$handle0"), @"
    internal yields fn main$handle0(p.0: unit) -> int {
      let ev.3: obj = prim ev_lookup[Ask]()
      let c.4: tobj = prim ev_clause[Ask.ask](ev.3)
      let t.1: int = clause Ask.ask c.4(ev.3, ())
      let y.5: int = prim yielding()
      switch y.5 { 0 -> b2, _ -> b1 }
    b1:
      let k.6: tobj = prim yield_cont_take()
      let k.7: tobj = closure compose$(k.6, &main$handle0$k0)
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
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\nrun : (Unit -> <e> String) -> <State Int | e> String\nrun cb =\n  let n = get ()\n  let s = cb ()\n  s ++ show n\n\ncb : Unit -> <State String> String\ncb () = get () ++ \"!\"\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle (handle run cb with\n                    | get () k -> k 42\n                    | put _ k -> k ()) with\n            | get () k -> k \"str\"\n            | put _ k -> k ()\n  println r";
    let shown = core_text(text, Pass::Evidence);
    let run = instance_name(&shown, "run");
    insta::assert_snapshot!(function(&shown, &run), @r#"
    yields fn "run<e>"(cb.0: tobj poly) -> obj {
      let ev.5: obj = prim ev_lookup[State]()
      let c.6: tobj = prim ev_clause[State.get](ev.5)
      let t.1: int = clause State.get c.6(ev.5, ())
      let y.11: int = prim yielding()
      switch y.11 { 0 -> b2, _ -> b1 }
    b1:
      let k.12: tobj = prim yield_cont_take()
      let c.13: tobj = closure "run<e>$k0"(cb.0)
      let k.14: tobj = closure compose$(k.12, c.13)
      let u.15: unit = prim yield_cont_set(k.14)
      let d.16: obj = prim dummy()
      return d.16
    b2:
      let w.7: tobj = prim evv_get()
      let w.8: tobj = prim evv_mask[State](w.7)
      let u.9: unit = prim evv_set(w.8)
      let t.2: obj = yields apply cb.0(())
      let y.17: int = prim yielding()
      switch y.17 { 0 -> b4, _ -> b3 }
    b3:
      let k.18: tobj = prim yield_cont_take()
      let k.19: tobj = closure "mask$[State]"(k.18)
      let c.20: tobj = closure "run<e>$k1"(t.1)
      let k.21: tobj = closure compose$(k.19, c.20)
      let u.22: unit = prim yield_cont_set(k.21)
      let d.23: obj = prim dummy()
      return d.23
    b4:
      let u.10: unit = prim evv_set(w.7)
      let t.3: obj = extern "Prelude.Show Int.show"(t.1)
      let t.4: obj = extern Prelude.++(t.2, t.3)
      return t.4
    }
    "#);
    insta::assert_snapshot!(function(&shown, &format!("{run}$k1")), @r#"
    internal yields fn "run<e>$k1"(t.1: int, t.2: obj) -> obj {
      let t.3: obj = extern "Prelude.Show Int.show"(t.1)
      let t.4: obj = extern Prelude.++(t.2, t.3)
      return t.4
    }
    "#);
}

#[test]
fn a_masked_call_in_a_continuation_restores_the_evidence_list_before_it_continues() {
    // 2つ目の `cb ()` は1つ目の地点の後の区間にあるので、`run$k0` の中の地点になる。`run$k0` の速い道は `run$k1` を
    // 呼んで止まるが、その前に `evv_set(w0)` で `evv` を戻す。`run$k1` の本体には `evv_set` がない
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\nrun : (Unit -> <e> String) -> <State Int | e> String\nrun cb =\n  let a = cb ()\n  let b = cb ()\n  a ++ b\n\ncb : Unit -> <State String> String\ncb () = get ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle (handle run cb with\n                    | get () k -> k 42\n                    | put _ k -> k ()) with\n            | get () k -> k \"str\"\n            | put _ k -> k ()\n  println r";
    let shown = core_text(text, Pass::Evidence);
    let run = instance_name(&shown, "run");
    insta::assert_snapshot!(function(&shown, &format!("{run}$k0")), @r#"
    internal yields fn "run<e>$k0"(cb.0: tobj, t.1: obj) -> obj {
      let w.8: tobj = prim evv_get()
      let w.9: tobj = prim evv_mask[State](w.8)
      let u.10: unit = prim evv_set(w.9)
      let t.2: obj = yields apply cb.0(())
      let y.12: int = prim yielding()
      switch y.12 { 0 -> b2, _ -> b1 }
    b1:
      let k.13: tobj = prim yield_cont_take()
      let k.14: tobj = closure "mask$[State]"(k.13)
      let c.15: tobj = closure "run<e>$k1"(t.1)
      let k.16: tobj = closure compose$(k.14, c.15)
      let u.17: unit = prim yield_cont_set(k.16)
      let d.18: obj = prim dummy()
      return d.18
    b2:
      let u.11: unit = prim evv_set(w.8)
      let r.19: obj = call "run<e>$k1"(t.1, t.2)
      return r.19
    }
    "#);
    insta::assert_snapshot!(function(&shown, &format!("{run}$k1")), @r#"
    internal yields fn "run<e>$k1"(t.1: obj, t.2: obj) -> obj {
      let t.3: obj = extern Prelude.++(t.1, t.2)
      return t.3
    }
    "#);
}

#[test]
fn a_resume_in_a_yielding_function_becomes_a_yielding_apply() {
    // handle を含む `f` は `Log` を外へ通すので `yields` で、節の関数も `yields` になる。`resume k(v, s)` は
    // `yields apply k(v, s)` になり、末尾でなければ yield の地点になる
    let text = format!(
        "{ASK}effect Log where\n  log : Int -> Unit\n\nf : Unit -> <Log> Int\nf () =\n  handle ask () + 1 with\n    | ask () k ->\n      let v = k 10\n      v * 2\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r = handle f () with\n            | log _ k -> k ()\n  println (show r)"
    );
    let shown = core_text(&text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "f$handle0$ask"), @"
    internal yields fn f$handle0$ask(p.0: unit, k.1: tobj lin, p.2: unit) -> int {
      let t.3: int = yields apply k.1(10, ())
      let y.5: int = prim yielding()
      switch y.5 { 0 -> b2, _ -> b1 }
    b1:
      let k.6: tobj = prim yield_cont_take()
      let k.7: tobj = closure compose$(k.6, &f$handle0$ask$k0)
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
        "{ASK}twice : (Unit -> <e> Int) -> <e> Int\ntwice f = f () + f ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (show (twice (fn () -> 1)))\n  println (show (handle twice ask with\n    | ask () k -> k 2))"
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
      let k.5: tobj = prim yield_cont_take()
      let c.6: tobj = closure "twice<e>$k0"(f.0)
      let k.7: tobj = closure compose$(k.5, c.6)
      let u.8: unit = prim yield_cont_set(k.7)
      return 0
    b2:
      let t.2: int = yields apply f.0(())
      let y.9: int = prim yielding()
      switch y.9 { 0 -> b4, _ -> b3 }
    b3:
      let k.10: tobj = prim yield_cont_take()
      let c.11: tobj = closure "twice<e>$k1"(t.1)
      let k.12: tobj = closure compose$(k.10, c.11)
      let u.13: unit = prim yield_cont_set(k.12)
      return 0
    b4:
      let t.3: int = extern Prelude.+(t.1, t.2)
      return t.3
    }
    "#);
}

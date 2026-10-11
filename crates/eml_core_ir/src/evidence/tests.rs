//! 結合テストの入力 (translate の出力の形) からは作れないものを確かめる。handle と perform の変換が使う補助の関数と、
//! その変換が付ける `Annotations` と、節の呼び出しの地点である。

use std::collections::{HashMap, HashSet};

use crate::{Program, parse, pretty, verify_evidence};
use eml_lir::{BlockId, FnIdx};

use super::Helpers;
use super::split::{Annotations, split};

fn program(text: &str) -> Program {
    parse(text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

fn shown(program: &Program) -> String {
    let shown = pretty(program);
    if let Err(error) = verify_evidence(program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

fn split_named(program: &mut Program, name: &str, annotations: &Annotations) {
    let index = program
        .functions
        .iter()
        .position(|function| function.name == name)
        .unwrap_or_else(|| panic!("no `{name}`"));
    split(
        program,
        FnIdx(index as u32),
        annotations,
        &mut Helpers::default(),
    );
}

#[test]
fn helpers_have_their_final_form() {
    // `prompt$` は `resume$` と `rewind$` を参照するので、まとめて作られる。同じ多重集合の `mask$` は1つである
    let mut program =
        program("effect State { get(unit) -> tobj }\nfn main() -> unit {\n  return ()\n}\n");
    let mut helpers = Helpers::default();
    let id = helpers.id(&mut program);
    let prompt = helpers.prompt(&mut program);
    let mask = helpers.mask(&mut program, &[0]);
    assert_eq!(helpers.id(&mut program), id);
    assert_eq!(helpers.prompt(&mut program), prompt);
    assert_eq!(helpers.mask(&mut program, &[0]), mask);
    insta::assert_snapshot!(shown(&program), @r#"
    effect State { get(unit) -> tobj }
    fn main() -> unit {
      return ()
    }
    yields fn id$(x.0: tobj) -> tobj {
      return x.0
    }
    yields fn prompt$(ev.0: obj, ret.1: tobj, x.2: tobj) -> tobj {
      let w.3: tobj = prim ev_evv(ev.0)
      let u.4: unit = prim evv_set(w.3)
      let s.5: int = prim yield_status(ev.0)
      switch s.5 { 0 -> b1, 1 -> b2, 2 -> b3, _ -> b4 }
    b1:
      let st.6: tobj = prim ev_take_state(ev.0)
      let r.7: tobj = yields apply ret.1(x.2, st.6)
      return r.7
    b2:
      let k.8: tobj = prim yield_cont_take()
      let c.9: tobj = prim yield_finish()
      let st.10: tobj = prim ev_take_state(ev.0)
      let k.11: tobj = closure resume$(ev.0, ret.1, k.8)
      let r.12: tobj = yields apply c.9(k.11, st.10)
      return r.12
    b3:
      let k.13: tobj = prim yield_cont_take()
      let u.14: unit = drop k.13
      let c.15: tobj = prim yield_finish()
      let st.16: tobj = prim ev_take_state(ev.0)
      let r.17: tobj = yields apply c.15(st.16)
      return r.17
    b4:
      let st.18: tobj = prim ev_take_state(ev.0)
      let k.19: tobj = prim yield_cont_take()
      let k.20: tobj = closure rewind$(ev.0, ret.1, st.18, k.19)
      let u.21: unit = prim yield_cont_set(k.20)
      let d.22: obj = prim dummy()
      return d.22
    }
    yields fn resume$(ev.0: obj, ret.1: tobj, k.2: tobj, v.3: tobj, s.4: tobj) -> tobj {
      let e.5: obj = prim ev_renew(ev.0, s.4)
      let w.6: tobj = prim evv_get()
      let w.7: tobj = prim evv_push(w.6, e.5)
      let u.8: unit = prim evv_set(w.7)
      let x.9: tobj = yields apply k.2(v.3)
      let r.10: tobj = call prompt$(e.5, ret.1, x.9)
      return r.10
    }
    yields fn rewind$(ev.0: obj, ret.1: tobj, s.2: tobj, k.3: tobj, v.4: tobj) -> tobj {
      let r.5: tobj = call resume$(ev.0, ret.1, k.3, v.4, s.2)
      return r.5
    }
    yields fn "mask$[State]"(k.0: tobj, v.1: tobj) -> tobj {
      let w.2: tobj = prim evv_get()
      let m.3: tobj = prim evv_mask[State](w.2)
      let u.4: unit = prim evv_set(m.3)
      let r.5: tobj = yields apply k.0(v.1)
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b2, _ -> b1 }
    b1:
      let k.7: tobj = prim yield_cont_take()
      let k.8: tobj = closure "mask$[State]"(k.7)
      let u.9: unit = prim yield_cont_set(k.8)
      let d.10: obj = prim dummy()
      return d.10
    b2:
      let u.11: unit = prim evv_set(w.2)
      return r.5
    }
    "#);
}

#[test]
fn a_mask_names_its_multiset() {
    let mut program =
        program("effect State { get(unit) -> tobj }\nfn main() -> unit {\n  return ()\n}\n");
    let mut helpers = Helpers::default();
    let once = helpers.mask(&mut program, &[0]);
    let twice = helpers.mask(&mut program, &[0, 0]);
    assert_ne!(once, twice);
    assert_eq!(program.function(twice).name, "mask$[State, State]");
}

#[test]
fn a_masked_point_wraps_the_continuation_and_resumes_after_restoring_the_evv() {
    // `call p()` は `yields` でない相手への呼び出しだが、`mask` の呼び出しなので地点になる。遅い道は κ を
    // `mask$[State]` で包み、続きは `evv_set(w.2)` の後から始まるので `w.2` を捕まえない。handle の本体の
    // `yields apply` は地点にしない
    let mut program = program(
        "\
effect State { get(unit) -> tobj }
fn p() -> int {
  return 2
}
yields fn f(a.0: int, h.1: tobj) -> int {
  let w.2: tobj = prim evv_get()
  let m.3: tobj = prim evv_mask[State](w.2)
  let u.4: unit = prim evv_set(m.3)
  let x.5: int = call p()
  let u.6: unit = prim evv_set(w.2)
  let r.7: tobj = yields apply h.1(())
  let s.8: int = extern Prelude.+(x.5, a.0)
  return s.8
}
",
    );
    let at = (BlockId(0), 3);
    let annotations = Annotations {
        not_yield_points: HashSet::from([(BlockId(0), 5)]),
        masked: HashMap::from([(at, vec![0])]),
        forced: HashSet::from([at]),
    };
    split_named(&mut program, "f", &annotations);
    insta::assert_snapshot!(shown(&program), @r#"
    effect State { get(unit) -> tobj }
    fn p() -> int {
      return 2
    }
    yields fn f(a.0: int, h.1: tobj) -> int {
      let w.2: tobj = prim evv_get()
      let m.3: tobj = prim evv_mask[State](w.2)
      let u.4: unit = prim evv_set(m.3)
      let x.5: int = call p()
      let y.9: int = prim yielding()
      switch y.9 { 0 -> b2, _ -> b1 }
    b1:
      let k.10: tobj = prim yield_cont_take()
      let k.11: tobj = closure "mask$[State]"(k.10)
      let c.12: tobj = closure f$k0(a.0, h.1)
      let k.13: tobj = closure compose$(k.11, c.12)
      let u.14: unit = prim yield_cont_set(k.13)
      return 0
    b2:
      let u.6: unit = prim evv_set(w.2)
      let r.7: tobj = yields apply h.1(())
      let s.8: int = extern Prelude.+(x.5, a.0)
      return s.8
    }
    internal yields fn f$k0(a.0: int, h.1: tobj, x.5: int) -> int {
      let r.7: tobj = yields apply h.1(())
      let s.8: int = extern Prelude.+(x.5, a.0)
      return s.8
    }
    yields fn "mask$[State]"(k.0: tobj, v.1: tobj) -> tobj {
      let w.2: tobj = prim evv_get()
      let m.3: tobj = prim evv_mask[State](w.2)
      let u.4: unit = prim evv_set(m.3)
      let r.5: tobj = yields apply k.0(v.1)
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b2, _ -> b1 }
    b1:
      let k.7: tobj = prim yield_cont_take()
      let k.8: tobj = closure "mask$[State]"(k.7)
      let u.9: unit = prim yield_cont_set(k.8)
      let d.10: obj = prim dummy()
      return d.10
    b2:
      let u.11: unit = prim evv_set(w.2)
      return r.5
    }
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
    "#);
}

#[test]
fn a_clause_call_is_a_yield_point_unless_it_is_in_tail_position() {
    let mut program = program(
        "\
effect State { get(unit) -> tobj }
yields fn f(c.0: tobj, e.1: obj) -> tobj {
  let v.2: tobj = clause State.get c.0(e.1, ())
  let w.3: tobj = clause State.get c.0(e.1, ())
  return w.3
}
",
    );
    split_named(&mut program, "f", &Annotations::default());
    insta::assert_snapshot!(shown(&program), @"
    effect State { get(unit) -> tobj }
    yields fn f(c.0: tobj, e.1: obj) -> tobj {
      let v.2: tobj = clause State.get c.0(e.1, ())
      let y.4: int = prim yielding()
      switch y.4 { 0 -> b2, _ -> b1 }
    b1:
      let k.5: tobj = prim yield_cont_take()
      let c.6: tobj = closure f$k0(c.0, e.1)
      let k.7: tobj = closure compose$(k.5, c.6)
      let u.8: unit = prim yield_cont_set(k.7)
      let d.9: obj = prim dummy()
      return d.9
    b2:
      let w.3: tobj = clause State.get c.0(e.1, ())
      return w.3
    }
    internal yields fn f$k0(c.0: tobj, e.1: obj, v.2: tobj) -> tobj {
      let w.3: tobj = clause State.get c.0(e.1, ())
      return w.3
    }
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

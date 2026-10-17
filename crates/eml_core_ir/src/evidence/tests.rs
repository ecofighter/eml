//! 結合テストの入力 (translate の出力の形) からは作れないものを確かめる。handle と perform の変換が使う補助の関数と、
//! その変換が付ける `Annotations` と、節の呼び出しの地点である。

use std::collections::{HashMap, HashSet};

use crate::{Program, parse, pretty, verify_evidence};
use eml_lir::{BlockId, FnIdx, MaskItem};

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
    // `prompt$` は `resume$` と `rewind$` を参照するので、まとめて作られる。同じ項目の `mask$` は1つである
    let mut program =
        program("effect State { get(unit) -> tobj }\nfn main() -> unit {\n  return ()\n}\n");
    let mut helpers = Helpers::default();
    let id = helpers.id(&mut program);
    let prompt = helpers.prompt(&mut program);
    let mask = helpers.mask(&mut program, &[item(0, 0, 1)]);
    assert_eq!(helpers.id(&mut program), id);
    assert_eq!(helpers.prompt(&mut program), prompt);
    assert_eq!(helpers.mask(&mut program, &[item(0, 0, 1)]), mask);
    insta::assert_snapshot!(shown(&program), @r#"
    effect State { get(unit) -> tobj }
    fn main() -> unit {
      return ()
    }
    yields fn id$(x.0: tobj poly) -> tobj poly {
      return x.0
    }
    yields fn prompt$(ev.0: obj, ret.1: tobj poly, x.2: tobj poly) -> tobj poly {
      let w.3: tobj = prim ev_evv(ev.0)
      let u.4: unit = prim evv_set(w.3)
      let s.5: int = prim yield_status(ev.0)
      switch s.5 { 0 -> b1, 1 -> b2, 2 -> b3, _ -> b4 }
    b1:
      let st.6: tobj poly = prim ev_take_state(ev.0)
      let r.7: tobj poly = yields apply ret.1(x.2, st.6)
      return r.7
    b2:
      let u.8: unit = drop x.2
      let k.9: tobj poly = prim yield_cont_take()
      let c.10: tobj poly = prim yield_finish()
      let st.11: tobj poly = prim ev_take_state(ev.0)
      let k.12: tobj poly = closure resume$(ev.0, ret.1, k.9)
      let r.13: tobj poly = yields apply c.10(k.12, st.11)
      return r.13
    b3:
      let u.14: unit = drop ret.1
      let u.15: unit = drop x.2
      let c.16: tobj poly = prim yield_finish()
      let st.17: tobj poly = prim ev_take_state(ev.0)
      let r.18: tobj poly = yields apply c.16(st.17)
      return r.18
    b4:
      let u.19: unit = drop x.2
      let st.20: tobj poly = prim ev_take_state(ev.0)
      let k.21: tobj poly = prim yield_cont_take()
      let k.22: tobj poly = closure rewind$(ev.0, ret.1, st.20, k.21)
      let u.23: unit = prim yield_cont_set(k.22)
      let d.24: obj poly = prim dummy()
      return d.24
    }
    yields fn resume$(ev.0: obj, ret.1: tobj poly, k.2: tobj poly, v.3: tobj poly, s.4: tobj poly) -> tobj poly {
      let e.5: obj = prim ev_renew(ev.0, s.4)
      let w.6: tobj = prim evv_get()
      let w.7: tobj = prim evv_push(w.6, e.5)
      let u.8: unit = prim evv_set(w.7)
      let x.9: tobj poly = yields apply k.2(v.3) prompt e.5
      let r.10: tobj poly = call prompt$(e.5, ret.1, x.9)
      return r.10
    }
    yields fn rewind$(ev.0: obj, ret.1: tobj poly, s.2: tobj poly, k.3: tobj poly, v.4: tobj poly) -> tobj poly {
      let r.5: tobj poly = call resume$(ev.0, ret.1, k.3, v.4, s.2)
      return r.5
    }
    yields fn "mask$[State]"(k.0: tobj poly, v.1: tobj poly) -> tobj poly {
      let w.2: tobj = prim evv_get()
      let m.3: tobj = prim evv_mask[State](w.2)
      let u.4: unit = prim evv_set(m.3)
      let r.5: tobj poly = yields apply k.0(v.1)
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b2, _ -> b1 }
    b1:
      let u.7: unit = drop r.5
      let k.8: tobj poly = prim yield_cont_take()
      let k.9: tobj poly = closure "mask$[State]"(k.8)
      let u.10: unit = prim yield_cont_set(k.9)
      let d.11: obj poly = prim dummy()
      return d.11
    b2:
      let u.12: unit = prim evv_set(w.2)
      return r.5
    }
    "#);
}

fn item(effect: u32, keep: u32, skip: u32) -> MaskItem {
    MaskItem { effect, keep, skip }
}

#[test]
fn a_mask_with_kept_handlers_is_named_by_its_items() {
    let mut program = program(
        "effect State { get(unit) -> tobj }\neffect Yield { y(unit) -> tobj }\nfn main() -> unit {\n  return ()\n}\n",
    );
    let mut helpers = Helpers::default();
    let mask = helpers.mask(&mut program, &[item(1, 1, 1)]);
    assert_eq!(program.function(mask).name, "mask$[Yield@1]");
    assert!(
        shown(&program).contains("prim evv_mask[Yield@1](w.2)"),
        "{}",
        shown(&program)
    );
}

#[test]
fn a_mask_names_its_items() {
    let mut program =
        program("effect State { get(unit) -> tobj }\nfn main() -> unit {\n  return ()\n}\n");
    let mut helpers = Helpers::default();
    let once = helpers.mask(&mut program, &[item(0, 0, 1)]);
    let twice = helpers.mask(&mut program, &[item(0, 0, 2)]);
    assert_ne!(once, twice);
    assert_eq!(program.function(twice).name, "mask$[State*2]");
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
        masked: HashMap::from([(at, vec![item(0, 0, 1)])]),
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
      let k.10: tobj poly = prim yield_cont_take()
      let k.11: tobj poly = closure "mask$[State]"(k.10)
      let c.12: tobj = closure f$k0(a.0, h.1)
      let k.13: tobj poly = closure compose$(k.11, c.12)
      let u.14: unit = prim yield_cont_set(k.13)
      return 0
    b2:
      let u.6: unit = prim evv_set(w.2)
      let r.7: tobj = yields apply h.1(())
      let s.8: int = extern Prelude.+(x.5, a.0)
      return s.8
    }
    internal yields fn f$k0(a.0: int, h.1: tobj, x.2: int) -> int {
      let r.3: tobj = yields apply h.1(())
      let s.4: int = extern Prelude.+(x.2, a.0)
      return s.4
    }
    yields fn "mask$[State]"(k.0: tobj poly, v.1: tobj poly) -> tobj poly {
      let w.2: tobj = prim evv_get()
      let m.3: tobj = prim evv_mask[State](w.2)
      let u.4: unit = prim evv_set(m.3)
      let r.5: tobj poly = yields apply k.0(v.1)
      let y.6: int = prim yielding()
      switch y.6 { 0 -> b2, _ -> b1 }
    b1:
      let u.7: unit = drop r.5
      let k.8: tobj poly = prim yield_cont_take()
      let k.9: tobj poly = closure "mask$[State]"(k.8)
      let u.10: unit = prim yield_cont_set(k.9)
      let d.11: obj poly = prim dummy()
      return d.11
    b2:
      let u.12: unit = prim evv_set(w.2)
      return r.5
    }
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
      let k.5: tobj poly = prim yield_cont_take()
      let c.6: tobj = closure f$k0(c.0, e.1)
      let k.7: tobj poly = closure compose$(k.5, c.6)
      let u.8: unit = prim yield_cont_set(k.7)
      let d.9: obj poly = prim dummy()
      return d.9
    b2:
      let w.3: tobj = clause State.get c.0(e.1, ())
      return w.3
    }
    internal yields fn f$k0(c.0: tobj, e.1: obj, v.2: tobj) -> tobj {
      let w.3: tobj = clause State.get c.0(e.1, ())
      return w.3
    }
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

/// `shown` の関数 `name` の、ラベル `label` のブロックの文の行。入口のブロックのラベルは `b0` とする。
fn block_lines<'a>(shown: &'a str, name: &str, label: &str) -> Vec<&'a str> {
    let header = [format!("fn {name}("), format!("fn {name:?}(")];
    let mut lines = shown
        .lines()
        .skip_while(|line| !header.iter().any(|header| line.contains(header.as_str())))
        .skip(1)
        .take_while(|line| *line != "}");
    let mut current = "b0";
    let mut found = Vec::new();
    for line in &mut lines {
        if let Some(next) = line.strip_suffix(':') {
            current = next;
        } else if current == label {
            found.push(line.trim());
        }
    }
    assert!(!found.is_empty(), "no block `{label}` in `{name}`\n{shown}");
    found
}

fn drops(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| line.split(" = drop ").nth(1).map(str::to_string))
        .collect()
}

#[test]
fn a_slow_path_drops_a_non_unr_yield_result() {
    // 遅い道の yield の結果はいつもダミーである。`unr` でない RC の結果は、EP が遅い道の先頭で drop する。Perceus に
    // `decref` を置かせないためである (docs/spec/core-ir.md の「EP の明示の `drop`」)。
    // `unr` の結果と RC でない結果には何も置かない
    let mut program = program(
        "\
effect Src { take(unit) -> tobj lin }
effect Ask { ask(unit) -> int }
effect Get { get(unit) -> tobj }
fn keep(v.0: tobj lin, n.1: int, w.2: tobj) -> tobj lin {
  return v.0
}
yields fn f(s.0: tobj, a.1: tobj, g.2: tobj, e.3: obj) -> tobj lin {
  let v.4: tobj lin = clause Src.take s.0(e.3, ())
  let n.5: int = clause Ask.ask a.1(e.3, ())
  let w.6: tobj = clause Get.get g.2(e.3, ())
  let r.7: tobj lin = call keep(v.4, n.5, w.6)
  return r.7
}
",
    );
    split_named(&mut program, "f", &Annotations::default());
    let shown = shown(&program);
    let slow = block_lines(&shown, "f", "b1");
    assert_eq!(drops(&slow), ["v.4"], "{shown}");
    assert!(slow[0].ends_with("= drop v.4"), "{shown}");
    for label in ["b3", "b5"] {
        assert!(
            drops(&block_lines(&shown, "f", label)).is_empty(),
            "{shown}"
        );
    }
}

#[test]
fn prompt_drops_x_on_its_yield_paths_and_ret_on_abort() {
    // 状態 1、2、3 の道の `x` はいつもダミーで、状態 2 (中断) の道では `return` の節のクロージャ `ret` も使われない。
    // どれも `poly` なので、Perceus の `decref` でなく EP の drop で道の先頭で手放す
    // (docs/spec/core-ir.md の「EP の明示の `drop`」)
    let mut program =
        program("effect State { get(unit) -> tobj }\nfn main() -> unit {\n  return ()\n}\n");
    Helpers::default().prompt(&mut program);
    let shown = pretty(&program);
    assert!(
        drops(&block_lines(&shown, "prompt$", "b1")).is_empty(),
        "{shown}"
    );
    for (label, expected) in [
        ("b2", &["x.2"][..]),
        ("b3", &["ret.1", "x.2"][..]),
        ("b4", &["x.2"][..]),
    ] {
        let lines = block_lines(&shown, "prompt$", label);
        assert_eq!(
            drops(&lines[..expected.len()]),
            expected,
            "{label}\n{shown}"
        );
    }
    // 中断は κ を持たないので、中断の道が捨てるのは `ret` と `x` だけである
    // (docs/superpowers/specs/2026-10-17-s13c-unwinding-design.md の「`eml_rt` と EP の変更」)
    assert_eq!(
        drops(&block_lines(&shown, "prompt$", "b3")).len(),
        2,
        "{shown}"
    );
}

#[test]
fn compose_and_mask_drop_r_on_their_slow_paths() {
    // 遅い道の `r` は κ が yield したときのダミーである
    let mut program =
        program("effect State { get(unit) -> tobj }\nfn main() -> unit {\n  return ()\n}\n");
    let mut helpers = Helpers::default();
    helpers.compose(&mut program);
    helpers.mask(&mut program, &[item(0, 0, 1)]);
    let shown = pretty(&program);
    for name in ["compose$", "mask$[State]"] {
        let slow = block_lines(&shown, name, "b1");
        assert_eq!(
            drops(&slow[..1]),
            [if name == "compose$" { "r.3" } else { "r.5" }],
            "{shown}"
        );
        assert_eq!(drops(&slow).len(), 1, "{shown}");
        assert!(
            drops(&block_lines(&shown, name, "b2")).is_empty(),
            "{shown}"
        );
    }
}

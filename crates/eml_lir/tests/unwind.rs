//! 巻き戻しの点 (`eml_lir::unwind::points`)。入力は所有の段の verifier を通る、Perceus の後の LIR のテキストである
//! (docs/superpowers/specs/2026-10-17-s13c-unwinding-design.md の「巻き戻しの表」)。

use eml_lir::unwind::{UnwindPoint, points};
use eml_lir::{BlockId, VarId, parse, verify};

/// `name` の関数の巻き戻しの点。入力が所有の段の verifier を通ることも確かめる。
fn points_of(text: &str, name: &str) -> Vec<UnwindPoint> {
    let program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    let function = program
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("no `{name}`"));
    points(&program, function)
}

fn point(block: u32, stmt: usize, owned: &[(u32, u32)], landing: Option<u32>) -> UnwindPoint {
    UnwindPoint {
        block: BlockId(block),
        stmt,
        owned: owned
            .iter()
            .map(|&(var, count)| (VarId(var), count))
            .collect(),
        landing: landing.map(VarId),
    }
}

#[test]
fn call_lists_the_owned_variables_live_after_it() {
    // 呼び出しの束縛 `r` は、呼び出しが戻らなければまだないので数えない
    let text = "\
fn g(x.0: obj) -> obj {
  return x.0
}
fn f(a.0: obj, b.1: obj) -> obj {
  let r.2: obj = call g(a.0)
  decref r.2
  return b.1
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(1, 1)], None)]);
    assert_eq!(points_of(text, "g"), []);
}

#[test]
fn a_dupped_variable_is_listed_with_its_count() {
    let text = "\
fn g(x.0: obj) -> obj {
  return x.0
}
fn f(a.0: obj, b.1: obj) -> obj {
  dup b.1
  dup b.1
  let r.2: obj = call g(a.0)
  decref r.2
  let s.3: obj = extern Prelude.++(b.1, b.1)
  decref s.3
  return b.1
}
";
    assert_eq!(points_of(text, "f"), [point(0, 2, &[(1, 3)], None)]);
}

#[test]
fn a_borrowed_field_crossing_a_call_is_not_listed() {
    // `s` は `p` から借りているので、巻き戻しで手放すのは持ち主の `p` だけである
    let text = "\
layout P { P(obj) }
fn g(x.0: obj) -> obj {
  return x.0
}
fn f(p.0: obj, x.1: obj) -> obj {
  switch p.0 P { #0(s.2: obj) -> b1 }
b1:
  let r.3: obj = call g(x.1)
  decref r.3
  dup s.2
  decref p.0
  return s.2
}
";
    assert_eq!(points_of(text, "f"), [point(1, 0, &[(0, 1)], None)]);
}

#[test]
fn effectful_extern_and_yield_abort_are_points() {
    let text = "\
fn f(s.0: obj, t.1: obj, e.2: obj, c.3: tobj, v.4: tobj) -> obj {
  let u.5: unit = extern Prelude.println(s.0)
  let w.6: unit = prim yield_abort(e.2, c.3, v.4)
  return t.1
}
fn g(t.0: obj) -> obj {
  let z.1: obj = extern Prelude.exit(1)
  decref t.0
  return z.1
}
";
    assert_eq!(
        points_of(text, "f"),
        [
            point(0, 0, &[(1, 1), (2, 1), (3, 1), (4, 1)], None),
            point(0, 1, &[(1, 1)], None),
        ]
    );
    assert_eq!(points_of(text, "g"), [point(0, 0, &[(0, 1)], None)]);
}

#[test]
fn pure_extern_and_tail_call_are_not() {
    // 止まりうる extern (`MayFail`) も、エフェクトを起こさないので点でない
    let text = "\
fn g(x.0: obj) -> obj {
  return x.0
}
fn f(n.0: int, s.1: obj) -> obj {
  let m.2: obj = extern \"Prelude.Show Int.show\"(n.0)
  decref m.2
  let q.3: int = extern Prelude.+(n.0, 2)
  tail call g(s.1)
}
";
    assert_eq!(points_of(text, "f"), []);
}

#[test]
fn landing_is_the_prompt_variable() {
    let text = "\
yields fn f(b.0: tobj, ev.1: obj) -> obj {
  let x.2: tobj = yields apply b.0(()) prompt ev.1
  decref x.2
  return ev.1
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(1, 1)], Some(1))]);
}

#[test]
fn clause_call_is_a_point() {
    let text = "\
effect Ask { ask(unit) -> tobj }
fn f(c.0: tobj, e.1: obj, k.2: obj) -> obj {
  let x.3: tobj = clause Ask.ask c.0(e.1, ())
  decref x.3
  return k.2
}
";
    assert_eq!(points_of(text, "f"), [point(0, 0, &[(2, 1)], None)]);
}

#[test]
#[should_panic(
    expected = "unwind points need verified LIR: `a.0` is still owned at the end of the function in `f`"
)]
fn unverified_lir_panics() {
    let program = parse("fn f(a.0: obj) -> unit {\n  return ()\n}\n")
        .unwrap_or_else(|error| panic!("{error}"));
    points(&program, &program.functions[0]);
}
